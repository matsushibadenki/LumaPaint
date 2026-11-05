//! Viewport SVG rendering on the compositor's device and serial Metal queue.
//! Never reads pixels back in production. Unsupported SVGs retain resvg rendering.
use super::{gpu_cache, parsed_text, usvg};
use std::{cell::RefCell, collections::VecDeque};

const SCENE_BUDGET: usize = 64 * 1024 * 1024;
const OBJECT_SCENE_BUDGET: usize = 16 * 1024 * 1024;

// Native fills remain the first choice. Mixed fill/stroke editing guides and
// transformed paths can use the SVG GPU compositor without rasterizing the
// entire document first. Text/images keep their dedicated display contracts.
pub(crate) fn editable_layer(layer: &lumapaint_core::document::SvgLayer) -> bool {
    layer.vector_layer
        && !layer.paint_layer
        && !layer.vector_objects.is_empty()
        && layer.source.len() <= 4 * 1024 * 1024
        && layer
            .vector_objects
            .iter()
            .all(|o| o.text.is_none() && o.image_frame.is_none() && o.blend_mode == "normal")
}

struct Scene {
    source: String,
    size: [f32; 2],
    picture: Option<skia_safe::Picture>,
}

impl Scene {
    fn charge(&self) -> usize {
        self.source.len()
            + self
                .picture
                .as_ref()
                .map_or(0, |p| p.approximate_bytes_used())
    }
}

thread_local! {
    // Thread-confined, bounded LRU: multiple SVG layers must not recompile
    // each other's drawing commands on every pan. Skia reports picture bytes;
    // source payloads count toward the same budget.
    static SCENES: RefCell<VecDeque<Scene>> = const { RefCell::new(VecDeque::new()) };
}

struct ObjectScene {
    object: lumapaint_core::vector::VectorObject,
    document: [u32; 2],
    scene: Scene,
}
impl ObjectScene {
    fn charge(&self) -> usize {
        // Include both the sanitized source and the retained model snapshot.
        self.scene.charge()
            + self.scene.source.len() * 2
            + self.object.control_points.len() * std::mem::size_of::<[f32; 2]>()
    }
}
thread_local! {
    static OBJECT_SCENES: RefCell<VecDeque<ObjectScene>> = const { RefCell::new(VecDeque::new()) };
}

// Ungrouped canonical paths are independent drawing commands, but their layer
// opacity must still be applied once after all overlaps. Clips/groups and
// noncanonical sources use the whole-SVG renderer below instead of splitting.
pub(crate) fn render_editable(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layer: &lumapaint_core::document::SvgLayer,
    document: [u32; 2],
    size: [u32; 2],
    rect: [f32; 4],
    opacity: f32,
) -> Option<wgpu::Texture> {
    if force_cpu()
        || !editable_layer(layer)
        || !valid_target(device, document, size, rect, opacity)
        || layer.vector_objects.len() > 4096
        || layer
            .vector_objects
            .iter()
            .any(|o| !o.group_path.is_empty() || o.clipping_group.is_some())
        || layer.source
            != lumapaint_core::document::vector_svg(document[0], document[1], &layer.vector_objects)
    {
        return None;
    }
    let _timer = crate::performance::time("workspace_editable_gpu_host");
    let pictures = OBJECT_SCENES.with(|slot| {
        let mut scenes = slot.try_borrow_mut().ok()?;
        let mut pictures = Vec::with_capacity(layer.vector_objects.len());
        let mut frame_charge = 0;
        for object in layer.vector_objects.iter().filter(|o| o.visible) {
            if let Some(index) = scenes
                .iter()
                .position(|old| old.document == document && old.object == *object)
            {
                let scene = scenes.remove(index)?;
                scenes.push_back(scene);
                crate::performance::count("workspace_editable_object_hits", 1);
            } else {
                let source = lumapaint_core::document::vector_svg(
                    document[0],
                    document[1],
                    std::slice::from_ref(object),
                );
                let tree = parsed_text::parse_workspace(&source).ok()?;
                if !supported(tree.root(), 0) {
                    return None;
                }
                let normalized = normalized_for_gpu(&tree)?;
                let dom =
                    skia_safe::svg::Dom::from_str(&normalized, skia_safe::FontMgr::empty()).ok()?;
                let bounds = tree.root().abs_layer_bounding_box();
                let mut recorder = skia_safe::PictureRecorder::new();
                let canvas = recorder.begin_recording(
                    skia_safe::Rect::new(
                        bounds.left() - 2.,
                        bounds.top() - 2.,
                        bounds.right() + 2.,
                        bounds.bottom() + 2.,
                    ),
                    true,
                );
                dom.render(canvas);
                let scene = ObjectScene {
                    object: object.clone(),
                    document,
                    scene: Scene {
                        source,
                        size: document.map(|n| n as f32),
                        picture: Some(recorder.finish_recording_as_picture(None)?),
                    },
                };
                if scene.charge() > OBJECT_SCENE_BUDGET {
                    return None;
                }
                while scenes.len() >= 4096
                    || scenes.iter().map(ObjectScene::charge).sum::<usize>() + scene.charge()
                        > OBJECT_SCENE_BUDGET
                {
                    scenes.pop_front()?;
                }
                scenes.push_back(scene);
                crate::performance::count("workspace_editable_object_builds", 1);
            }
            frame_charge += scenes.back()?.charge();
            if frame_charge > OBJECT_SCENE_BUDGET {
                return None;
            }
            pictures.push(scenes.back()?.scene.picture.as_ref()?.clone());
        }
        Some(pictures)
    })?;
    let density = size[0] as f32 / rect[2];
    let info = skia_safe::ImageInfo::new(
        (size[0] as i32, size[1] as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let texture = gpu_cache::rasterize_shared(device, queue, &info, |surface| {
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::TRANSPARENT);
        canvas
            .translate((-rect[0] * density, -rect[1] * density))
            .scale((density, density));
        if opacity != 1. {
            canvas.save_layer_alpha_f(None, opacity);
        }
        for picture in &pictures {
            canvas.draw_picture(picture, None, None);
        }
        if opacity != 1. {
            canvas.restore();
        }
    })?;
    crate::performance::count("workspace_editable_retained_frames", 1);
    Some(texture)
}

fn force_cpu() -> bool {
    crate::performance::enabled()
        && std::env::var("LUMAPAINT_WORKSPACE_FORCE_CPU").as_deref() == Ok("1")
}
fn valid_target(
    device: &wgpu::Device,
    document: [u32; 2],
    size: [u32; 2],
    rect: [f32; 4],
    opacity: f32,
) -> bool {
    !size.contains(&0)
        && !document.contains(&0)
        && size
            .iter()
            .all(|v| *v <= 8192 && *v <= device.limits().max_texture_dimension_2d)
        && u64::from(size[0]) * u64::from(size[1]) * 4 <= 32 * 1024 * 1024
        && rect.iter().all(|v| v.is_finite())
        && rect[2] > 0.
        && rect[3] > 0.
        && opacity.is_finite()
        && (0. ..=1.).contains(&opacity)
}

fn supported(group: &usvg::Group, depth: usize) -> bool {
    if depth > 64
        || group.filters().len() > 1
        || group
            .filters()
            .iter()
            .any(|filter| !supported_filter(filter))
        || group.blend_mode() != usvg::BlendMode::Normal
        || group.isolate()
    {
        return false;
    }
    if let Some(mask) = group.mask() {
        if mask.mask().is_some() || !supported(mask.root(), depth + 1) {
            return false;
        }
    }
    if let Some(clip) = group.clip_path() {
        // Chained clip paths and clip groups with effects use the reference path.
        if clip.clip_path().is_some() || !supported(clip.root(), depth + 1) {
            return false;
        }
    }
    group.children().iter().all(|node| match node {
        usvg::Node::Group(child) => supported(child, depth + 1),
        usvg::Node::Path(path) => {
            path.paint_order() == usvg::PaintOrder::FillAndStroke
                && path.rendering_mode() == usvg::ShapeRendering::GeometricPrecision
                && path
                    .fill()
                    .is_none_or(|fill| !matches!(fill.paint(), usvg::Paint::Pattern(_)))
                && path
                    .stroke()
                    .is_none_or(|stroke| !matches!(stroke.paint(), usvg::Paint::Pattern(_)))
        }
        // Text/images have separate display paths and compatibility contracts.
        usvg::Node::Text(_) | usvg::Node::Image(_) => false,
    })
}

fn supported_filter(filter: &usvg::filter::Filter) -> bool {
    use usvg::filter::Kind;
    filter.primitives().len() <= 32
        && filter.primitives().iter().all(|p| {
            matches!(
                p.kind(),
                Kind::GaussianBlur(_)
                    | Kind::Offset(_)
                    | Kind::Flood(_)
                    | Kind::Composite(_)
                    | Kind::ColorMatrix(_)
                    | Kind::Merge(_)
                    | Kind::ComponentTransfer(_)
            )
        })
}

// Skia's SVG implementation treats every mask as luminance. Convert the
// already-sanitized alpha-mask content to white while retaining its alpha.
// This is display-only: the original document/source/export remains untouched.
fn normalized_for_gpu(tree: &usvg::Tree) -> Option<String> {
    let mut source = tree.to_string(&usvg::WriteOptions::default());
    if source.len() > 8 * 1024 * 1024 {
        return None;
    }
    let xml = usvg::roxmltree::Document::parse(&source).ok()?;
    let mut edits = Vec::new();
    for node in xml
        .descendants()
        .filter(|n| n.has_tag_name("mask") && n.attribute("mask-type") == Some("alpha"))
    {
        let id = node.attribute("id")?;
        let mask = tree.masks().iter().find(|m| m.id() == id)?;
        let rect = mask.rect();
        let mut name = format!("lp_alpha_{}", node.id().get());
        while xml
            .descendants()
            .any(|n| n.attribute("id") == Some(name.as_str()))
        {
            name.push('_');
        }
        let range = node.range();
        let content_start = node.children().next()?.range().start;
        let content_end = range.start + source[range.clone()].rfind("</mask>")?;
        let wrapper = format!(
            r#"<defs><filter id="{name}" filterUnits="userSpaceOnUse" primitiveUnits="userSpaceOnUse" x="{}" y="{}" width="{}" height="{}" color-interpolation-filters="sRGB"><feColorMatrix type="matrix" values="0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 1 0"/></filter></defs><g filter="url(#{name})">"#,
            rect.x(),
            rect.y(),
            rect.width(),
            rect.height()
        );
        edits.push((content_start, wrapper));
        edits.push((content_end, "</g>".into()));
    }
    edits.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
    for (index, text) in edits {
        source.insert_str(index, &text);
    }
    (source.len() <= 8 * 1024 * 1024).then_some(source)
}

pub(crate) fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    document: [u32; 2],
    size: [u32; 2],
    rect: [f32; 4],
    opacity: f32,
) -> Option<wgpu::Texture> {
    if force_cpu()
        || source.len() > 4 * 1024 * 1024
        || !valid_target(device, document, size, rect, opacity)
    {
        return None;
    }
    let _timer = crate::performance::time("workspace_svg_gpu_host");
    SCENES.with(|slot| {
        let mut scenes = slot.try_borrow_mut().ok()?;
        if let Some(index) = scenes.iter().position(|old| old.source == source) {
            let scene = scenes.remove(index)?;
            scenes.push_back(scene);
            crate::performance::count("workspace_svg_gpu_scene_hits", 1);
        } else {
            let tree = parsed_text::parse_workspace(source).ok()?;
            let dom = if supported(tree.root(), 0) {
                let normalized = normalized_for_gpu(&tree)?;
                // Only parser-sanitized, self-contained geometry crosses into Skia.
                (normalized.len() <= 8 * 1024 * 1024)
                    .then(|| {
                        skia_safe::svg::Dom::from_str(&normalized, skia_safe::FontMgr::empty()).ok()
                    })
                    .flatten()
            } else {
                None
            };
            let scene = Scene {
                source: source.into(),
                size: [tree.size().width(), tree.size().height()],
                picture: dom.and_then(|dom| {
                    let bounds = tree.root().abs_layer_bounding_box();
                    let mut recorder = skia_safe::PictureRecorder::new();
                    // Retain path/shader preparation and a bounds hierarchy. At
                    // high zoom Skia visits only operations intersecting the view.
                    let canvas = recorder.begin_recording(
                        skia_safe::Rect::new(
                            bounds.left() - 2.,
                            bounds.top() - 2.,
                            bounds.right() + 2.,
                            bounds.bottom() + 2.,
                        ),
                        true,
                    );
                    dom.render(canvas);
                    recorder
                        .finish_recording_as_picture(None)
                        .filter(|picture| {
                            picture.approximate_bytes_used() <= SCENE_BUDGET - source.len()
                        })
                }),
            };
            while scenes.len() >= 8
                || scenes.iter().map(Scene::charge).sum::<usize>() + scene.charge() > SCENE_BUDGET
            {
                scenes.pop_front()?;
            }
            scenes.push_back(scene);
            crate::performance::count("workspace_svg_gpu_scene_builds", 1);
        }
        let scene = scenes.back()?;
        let picture = scene.picture.as_ref()?;
        let scale = (document[0] as f32 / scene.size[0]).min(document[1] as f32 / scene.size[1]);
        let density = size[0] as f32 / rect[2];
        let x = ((document[0] as f32 - scene.size[0] * scale) * 0.5 - rect[0]) * density;
        let y = ((document[1] as f32 - scene.size[1] * scale) * 0.5 - rect[1]) * density;
        let info = skia_safe::ImageInfo::new(
            (size[0] as i32, size[1] as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let texture = gpu_cache::rasterize_shared(device, queue, &info, |surface| {
            let canvas = surface.canvas();
            canvas.clear(skia_safe::Color::TRANSPARENT);
            canvas
                .translate((x, y))
                .scale((scale * density, scale * density));
            if opacity != 1. {
                canvas.save_layer_alpha_f(None, opacity);
            }
            canvas.draw_picture(picture, None, None);
            if opacity != 1. {
                canvas.restore();
            }
        })?;
        crate::performance::count("workspace_svg_gpu_frames", 1);
        Some(texture)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::util::DeviceExt;

    fn gpu() -> (wgpu::Device, wgpu::Queue) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        pollster::block_on(adapter.request_device(&Default::default())).unwrap()
    }

    fn read(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
        let stride = (texture.width() * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(stride) * u64::from(texture.height()),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(texture.height()),
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        mapped
            .chunks(stride as usize)
            .flat_map(|row| row[..texture.width() as usize * 4].iter().copied())
            .collect()
    }

    fn difference(expected: &[u8], actual: &[u8]) -> (f64, f64, u8) {
        assert_eq!(expected.len(), actual.len());
        let deltas: Vec<_> = expected
            .iter()
            .zip(actual)
            .map(|(a, b)| a.abs_diff(*b))
            .collect();
        (
            deltas.iter().map(|v| u64::from(*v)).sum::<u64>() as f64 / deltas.len() as f64,
            deltas
                .chunks(4)
                .filter(|p| p.iter().any(|v| *v > 32))
                .count() as f64
                / (deltas.len() / 4) as f64,
            *deltas.iter().max().unwrap(),
        )
    }

    #[test]
    #[ignore = "requires Metal and LUMAPAINT_DRAG_FIXTURES from the native preview test"]
    fn editable_drag_gpu_matches_cpu_and_measures_redraw() {
        use crate::{frame_cache::FrameRasterCache, workspace::WorkspaceCache, Viewport};
        use lumapaint_formats::native::NativeDocumentCodec;
        let dir = std::path::PathBuf::from(std::env::var("LUMAPAINT_DRAG_FIXTURES").unwrap());
        let (device, queue) = gpu();
        let limits = device.limits();
        for (name, zoom) in [("75", 0.75), ("100", 1.), ("200", 2.), ("400", 4.)] {
            let viewport = Viewport::new(800., 554., 1., 1., false)
                .unwrap()
                .with_screen_zoom(zoom);
            for kind in ["point", "scale", "rotate"] {
                let mut gpu_cache = WorkspaceCache::default();
                let mut cpu_cache = WorkspaceCache::default();
                let mut frame_cache = FrameRasterCache::default();
                let mut times = [Vec::new(), Vec::new()];
                for step in 0..9 {
                    let doc = lumapaint_core::document::Document::decode(
                        &std::fs::read(dir.join(format!("{kind}-{name}-{step}.lumapaint")))
                            .unwrap(),
                    )
                    .unwrap();
                    let layer = doc.svg_layers().next().unwrap();
                    assert!(editable_layer(layer));
                    let mut images = None;
                    // Alternate order so neither path consistently gets the first parse.
                    for gpu_first in if step % 2 == 0 {
                        [false, true]
                    } else {
                        [true, false]
                    } {
                        let start = std::time::Instant::now();
                        if gpu_first {
                            let result = gpu_cache
                                .prepare_with_gpu(
                                    &doc,
                                    viewport,
                                    [0., 0.],
                                    &Default::default(),
                                    Some(&limits),
                                    Some((&device, &queue, None)),
                                )
                                .unwrap()
                                .unwrap();
                            assert!(result[0].image.pixels.is_empty());
                            assert!(
                                result[0].texture.is_some(),
                                "editable path fell back to CPU"
                            );
                            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                            images = Some(result);
                        } else {
                            // Previous production path prepared the full document image,
                            // plus a viewport image at high zoom. Excludes upload costs.
                            let _ = frame_cache.prepare_display_layer(layer, 960, 640).unwrap();
                            if zoom > 1.5 {
                                let _ = cpu_cache.prepare(&doc, viewport, [0., 0.]).unwrap();
                            }
                        }
                        if step != 0 {
                            times[usize::from(gpu_first)]
                                .push(start.elapsed().as_secs_f64() * 1000.);
                        }
                    }
                    let images = images.unwrap();
                    let actual = read(&device, &queue, images[0].texture.as_ref().unwrap());
                    let a = viewport.document_point(0., 0.);
                    let b = viewport.document_point(800., 554.);
                    let whole = render(
                        &device,
                        &queue,
                        &layer.source,
                        [960, 640],
                        [800, 554],
                        [a.x, a.y, b.x - a.x, b.y - a.y],
                        layer.effective_opacity(),
                    )
                    .unwrap();
                    assert_eq!(
                        actual,
                        read(&device, &queue, &whole),
                        "retained objects differ from complete GPU scene"
                    );
                    let expected = WorkspaceCache::default()
                        .prepare(&doc, viewport, [0., 0.])
                        .unwrap()
                        .unwrap()
                        .remove(0)
                        .pixels;
                    let (mean, fraction, max) = difference(&expected, &actual);
                    // Compare stable interiors separately from rasterizer AA edges.
                    let mut interior_max = 0;
                    for y in 1..553usize {
                        for x in 1..799usize {
                            let mut lo = [255u8; 4];
                            let mut hi = [0u8; 4];
                            for yy in y - 1..=y + 1 {
                                for xx in x - 1..=x + 1 {
                                    for c in 0..4 {
                                        let v = expected[(yy * 800 + xx) * 4 + c];
                                        lo[c] = lo[c].min(v);
                                        hi[c] = hi[c].max(v);
                                    }
                                }
                            }
                            if (0..4).all(|c| hi[c] == lo[c]) {
                                for c in 0..4 {
                                    interior_max = interior_max.max(
                                        expected[(y * 800 + x) * 4 + c]
                                            .abs_diff(actual[(y * 800 + x) * 4 + c]),
                                    );
                                }
                            }
                        }
                    }
                    eprintln!("drag pixels kind={kind} zoom={name} step={step} mean={mean:.6} fraction_gt32={fraction:.6} max={max} interior_max={interior_max}");
                    assert!(mean < 1. && fraction < 0.01 && interior_max <= 3);
                    if std::env::var("LUMAPAINT_DRAG_IMAGES").as_deref() == Ok("1") && step == 8 {
                        for (label, pixels) in [("cpu", expected.as_ref()), ("gpu", &actual)] {
                            image::save_buffer(
                                dir.join(format!("{kind}-{name}-{label}.png")),
                                pixels,
                                800,
                                554,
                                image::ColorType::Rgba8,
                            )
                            .unwrap();
                        }
                    }
                    assert!(gpu_cache
                        .prepare_with_gpu(
                            &doc,
                            viewport,
                            [0., 0.],
                            &Default::default(),
                            Some(&limits),
                            Some((&device, &queue, None))
                        )
                        .unwrap()
                        .is_none());
                }
                for values in &mut times {
                    values.sort_by(f64::total_cmp);
                }
                eprintln!("drag timing kind={kind} zoom={name} cpu_median_ms={:.4} gpu_median_ms={:.4} samples=8",
                    (times[0][3]+times[0][4])*0.5, (times[1][3]+times[1][4])*0.5);
            }
        }
    }

    #[test]
    #[ignore = "requires Metal and native drag fixtures; retained style/order/fallback coverage"]
    fn editable_objects_preserve_style_order_and_fallback() {
        use lumapaint_formats::native::NativeDocumentCodec;
        let doc = lumapaint_core::document::Document::decode(
            &std::fs::read(
                std::path::Path::new(&std::env::var("LUMAPAINT_DRAG_FIXTURES").unwrap())
                    .join("editable-base.lumapaint"),
            )
            .unwrap(),
        )
        .unwrap();
        let mut layer = doc.svg_layers().next().unwrap().clone();
        let (device, queue) = gpu();
        let rect = [260., 120., 320., 240.];
        for variant in 0..6 {
            match variant {
                1 => layer.vector_objects.reverse(),
                2 => {
                    layer.vector_objects[0].opacity = 0.2;
                    layer.vector_objects[0].fill.as_mut().unwrap().color = [30, 240, 70, 128];
                }
                3 => layer.vector_objects[0].visible = false,
                4 => {
                    let o = &mut layer.vector_objects[67];
                    o.stroke = Some(lumapaint_core::vector::VectorPaint {
                        registration: false,
                        color: [0, 0, 0, 128],
                    });
                    o.stroke_width = 1.5;
                    o.transform[1] = 0.3;
                    o.fill_gradient = Some(
                        serde_json::from_value(serde_json::json!({
                            "kind":"radial", "angle":30, "aspect":0.6, "method":"classic",
                            "stops":[{"position":0,"midpoint":0.5,"color":[255,0,0,80]},
                                {"position":1,"midpoint":0.5,"color":[0,100,255,200]}]
                        }))
                        .unwrap(),
                    );
                }
                5 => layer.opacity = 0.3,
                _ => {}
            }
            layer.source = lumapaint_core::document::vector_svg(960, 640, &layer.vector_objects);
            let retained = render_editable(
                &device,
                &queue,
                &layer,
                [960, 640],
                [640, 480],
                rect,
                layer.opacity,
            )
            .unwrap();
            let full = render(
                &device,
                &queue,
                &layer.source,
                [960, 640],
                [640, 480],
                rect,
                layer.opacity,
            )
            .unwrap();
            assert_eq!(
                read(&device, &queue, &retained),
                read(&device, &queue, &full),
                "variant {variant}"
            );
        }
        layer.vector_objects[0].group_path.push("group".into());
        layer.source = lumapaint_core::document::vector_svg(960, 640, &layer.vector_objects);
        assert!(render_editable(
            &device,
            &queue,
            &layer,
            [960, 640],
            [640, 480],
            rect,
            layer.opacity
        )
        .is_none());
        assert!(render(
            &device,
            &queue,
            &layer.source,
            [960, 640],
            [640, 480],
            rect,
            layer.opacity
        )
        .is_some());
        layer.vector_objects[0].group_path.clear();
        layer.source.push(' '); // Canonical mismatch must not silently drop source content.
        assert!(render_editable(
            &device,
            &queue,
            &layer,
            [960, 640],
            [640, 480],
            rect,
            layer.opacity
        )
        .is_none());
        OBJECT_SCENES.with(|cache| {
            assert!(
                cache
                    .borrow()
                    .iter()
                    .map(ObjectScene::charge)
                    .sum::<usize>()
                    <= OBJECT_SCENE_BUDGET
            )
        });
    }

    #[test]
    fn eligibility_rejects_unverified_filters_and_patterns() {
        for body in [
            r#"<defs><filter id="f"><feTurbulence baseFrequency=".1"/></filter></defs><rect width="64" height="64" filter="url(#f)"/>"#,
            r#"<defs><pattern id="p" width="8" height="8" patternUnits="userSpaceOnUse"><circle cx="4" cy="4" r="3"/></pattern></defs><rect width="64" height="64" fill="url(#p)"/>"#,
            r#"<g style="mix-blend-mode:multiply"><rect width="64" height="64"/></g>"#,
        ] {
            let source = format!(r#"<svg width="64" height="64">{body}</svg>"#);
            assert!(
                !supported(parsed_text::parse(&source).unwrap().root(), 0),
                "{body}"
            );
        }
    }

    #[test]
    #[ignore = "requires Metal; checks viewport clip, alpha, scene invalidation and fallback"]
    fn gpu_workspace_clipping_alpha_and_invalidation() {
        let (device, queue) = gpu();
        let source = r#"<svg width="64" height="64"><defs><clipPath id="c"><rect x="16" y="8" width="32" height="48"/></clipPath><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue" stop-opacity=".5"/></linearGradient></defs><g clip-path="url(#c)"><rect width="64" height="64" fill="url(#g)"/><g opacity=".5"><rect x="24" y="16" width="16" height="32" fill="lime"/><rect x="28" y="20" width="16" height="32" fill="yellow"/></g></g></svg>"#;
        for rect in [
            [0., 0., 64., 64.],
            [-8., 4., 32., 32.],
            [28.25, 16.5, 16., 16.],
        ] {
            for opacity in [1., 0.5] {
                let texture =
                    render(&device, &queue, source, [64, 64], [64, 64], rect, opacity).unwrap();
                let actual = read(&device, &queue, &texture);
                let mut expected =
                    super::super::rasterize_svg_workspace(source, [64, 64], [64, 64], rect)
                        .unwrap();
                for value in &mut expected {
                    *value = (*value as f32 * opacity).round() as u8;
                }
                let (mean, fraction, max) = difference(&expected, &actual);
                assert!(
                    mean < 1.0 && fraction < 0.01,
                    "mean={mean}, fraction={fraction}, max={max}"
                );
            }
        }
        let changed = source.replace("stop-color=\"red\"", "stop-color=\"white\"");
        let a = render(
            &device,
            &queue,
            source,
            [64, 64],
            [64, 64],
            [0., 0., 64., 64.],
            1.,
        )
        .unwrap();
        let b = render(
            &device,
            &queue,
            &changed,
            [64, 64],
            [64, 64],
            [0., 0., 64., 64.],
            1.,
        )
        .unwrap();
        assert_ne!(read(&device, &queue, &a), read(&device, &queue, &b));
        assert!(render(
            &device,
            &queue,
            source,
            [64, 64],
            [0, 64],
            [0., 0., 64., 64.],
            1.
        )
        .is_none());
        assert!(render(
            &device,
            &queue,
            source,
            [64, 64],
            [64, 64],
            [0., 0., f32::NAN, 64.],
            1.
        )
        .is_none());
    }

    #[test]
    #[ignore = "requires Metal; GPU/CPU cache transitions and exterior content"]
    fn gpu_workspace_cache_transitions() {
        use crate::{workspace::WorkspaceCache, Viewport};
        use lumapaint_core::{document::Document, layer_effects::LayerEffects};
        let (device, queue) = gpu();
        let mut document = Document::default();
        document.import_svg("exterior".into(), r##"<svg width="960" height="640"><rect x="-40" y="20" width="100" height="80" fill="#404040"/></svg>"##.into()).unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        let viewport = Viewport::new(800., 554., 1., 1., false)
            .unwrap()
            .with_screen_zoom(1.)
            .with_pan(150., 80.)
            .unwrap();
        let limits = device.limits();
        let mut cache = WorkspaceCache::default();
        let prepare = |cache: &mut WorkspaceCache, doc: &Document| {
            cache.prepare_with_gpu(
                doc,
                viewport,
                [0., 0.],
                &Default::default(),
                Some(&limits),
                Some((&device, &queue, None)),
            )
        };
        let first = prepare(&mut cache, &document).unwrap().unwrap().remove(0);
        let actual = read(&device, &queue, first.texture.as_ref().unwrap());
        assert!(first.image.pixels.is_empty());
        assert!(prepare(&mut cache, &document).unwrap().is_none());
        let expected = WorkspaceCache::default()
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap()
            .remove(0);
        let (mean, fraction, max) = difference(expected.pixels.as_ref(), &actual);
        assert!(
            max <= 1,
            "exterior bounds: mean={mean}, fraction={fraction}, max={max}"
        );
        // Switching to a CPU-only caller must regenerate, not expose an empty buffer.
        let cpu = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap()
            .remove(0);
        assert_eq!(cpu.pixels, expected.pixels);
        assert!(prepare(&mut cache, &document).unwrap().unwrap()[0]
            .texture
            .is_some());
        let mut effects = LayerEffects {
            enabled: true,
            ..Default::default()
        };
        effects.values[0] = 1.;
        document.set_layer_effects(&id, effects).unwrap();
        let effect = prepare(&mut cache, &document).unwrap().unwrap().remove(0);
        assert!(effect.texture.is_none());
        assert!(!effect.image.pixels.is_empty());
        assert!(effect.effect.is_some());
        // Skia's shared texture is sample/copy-source only. Effects must
        // allocate a writable target rather than copy into the imported texture.
        let mut display =
            pollster::block_on(crate::effects_display::EffectsDisplay::new(&device, &queue))
                .unwrap();
        let (effects, revision, mapping) = effect.effect.unwrap();
        let adjusted = display
            .render(
                &device,
                &queue,
                crate::effects_display::Request {
                    pixels: &effect.image.pixels,
                    size: effect.image.size,
                    revision,
                    effects: &effects,
                    opacity: 1.,
                    previous: first.texture.as_ref(),
                    mapping: Some(mapping),
                },
            )
            .unwrap();
        assert!(adjusted.usage().contains(wgpu::TextureUsages::COPY_DST));
        assert!(read(&device, &queue, &adjusted) != actual);
        document.undo();
        assert!(prepare(&mut cache, &document).unwrap().unwrap()[0]
            .texture
            .is_some());
        document.redo();
        assert!(prepare(&mut cache, &document).unwrap().unwrap()[0]
            .texture
            .is_none());
        document.set_layer_effects(&id, Default::default()).unwrap();
        assert!(prepare(&mut cache, &document).unwrap().unwrap()[0]
            .texture
            .is_some());
        // A change in a second layer should reuse the first GPU texture.
        document
            .import_svg(
                "second".into(),
                r#"<svg width="960" height="640"><rect width="20" height="20" fill="blue"/></svg>"#
                    .into(),
            )
            .unwrap();
        let images = prepare(&mut cache, &document).unwrap().unwrap();
        let reused = images.iter().find(|v| v.image.id == id).unwrap();
        assert!(reused.texture.is_none() && reused.image.pixels.is_empty());
    }

    #[test]
    #[ignore = "requires Metal; direct texture effects, history and renderer availability"]
    fn gpu_workspace_effects_cache_and_history() {
        use crate::{effects_display::EffectsDisplay, workspace::WorkspaceCache, Viewport};
        use lumapaint_core::{document::Document, layer_effects::LayerEffects};
        let (device, queue) = gpu();
        let mut display = pollster::block_on(EffectsDisplay::new(&device, &queue)).unwrap();
        let mut doc = Document::default();
        doc.import_svg("effects".into(),r#"<svg width="960" height="640"><rect width="960" height="640" fill="red" opacity=".5"/><circle cx="480" cy="320" r="200" fill="blue" opacity=".4"/></svg>"#.into()).unwrap();
        let id = doc.svg_layers().next().unwrap().id.clone();
        let viewport = Viewport::new(800., 554., 1., 1., false).unwrap();
        let limits = device.limits();
        let mut cache = WorkspaceCache::default();
        let mut render_doc = |cache: &mut WorkspaceCache, doc: &Document| {
            cache
                .prepare_with_gpu(
                    doc,
                    viewport,
                    [0., 0.],
                    &Default::default(),
                    Some(&limits),
                    Some((&device, &queue, Some(&mut display))),
                )
                .unwrap()
        };
        let before = render_doc(&mut cache, &doc).unwrap().remove(0);
        let before = read(&device, &queue, before.texture.as_ref().unwrap());
        let mut effects = LayerEffects {
            enabled: true,
            ..Default::default()
        };
        effects.values[0] = 0.7;
        doc.set_layer_effects(&id, effects).unwrap();
        let changed = render_doc(&mut cache, &doc).unwrap().remove(0);
        assert!(changed.image.pixels.is_empty() && changed.effect.is_none());
        let changed = read(&device, &queue, changed.texture.as_ref().unwrap());
        assert_ne!(changed, before);
        assert!(render_doc(&mut cache, &doc).is_none());
        doc.undo();
        let undone = render_doc(&mut cache, &doc).unwrap().remove(0);
        assert_eq!(
            read(&device, &queue, undone.texture.as_ref().unwrap()),
            before
        );
        doc.redo();
        let redone = render_doc(&mut cache, &doc).unwrap().remove(0);
        assert_eq!(
            read(&device, &queue, redone.texture.as_ref().unwrap()),
            changed
        );
        let fallback = cache
            .prepare_with_gpu(
                &doc,
                viewport,
                [0., 0.],
                &Default::default(),
                Some(&limits),
                Some((&device, &queue, None)),
            )
            .unwrap()
            .unwrap()
            .remove(0);
        assert!(
            fallback.texture.is_none()
                && !fallback.image.pixels.is_empty()
                && fallback.effect.is_some()
        );
        assert!(render_doc(&mut cache, &doc).unwrap()[0].texture.is_some());
    }

    fn effect_fixture(body: &str, size: [u32; 2], case: &str) -> String {
        let [w, h] = size;
        let defs = format!(
            r##"<defs>
          <linearGradient id="qaMask"><stop stop-color="#f03010" stop-opacity=".25"/><stop offset="1" stop-color="#20e0b0" stop-opacity=".85"/></linearGradient>
          <mask id="qaLuma" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="url(#qaMask)"/></mask>
          <mask id="qaAlpha" mask-type="alpha" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="url(#qaMask)"/></mask>
          <filter id="qaBlur" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="1.5"/></filter>
          <filter id="qaShadow" x="-20%" y="-20%" width="140%" height="140%" color-interpolation-filters="sRGB"><feGaussianBlur in="SourceAlpha" stdDeviation="2" result="blur"/><feOffset in="blur" dx="5" dy="4" result="offset"/><feFlood flood-color="#182860" flood-opacity=".6" result="color"/><feComposite in="color" in2="offset" operator="in" result="shadow"/><feMerge><feMergeNode in="shadow"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
          <filter id="qaColor"><feColorMatrix type="saturate" values=".4"/></filter>
          <filter id="qaTransfer"><feComponentTransfer><feFuncR type="linear" slope=".7" intercept=".1"/><feFuncG type="gamma" amplitude=".8" exponent="1.2" offset=".1"/><feFuncB type="table" tableValues="0 .2 .8 1"/><feFuncA type="linear" slope=".7"/></feComponentTransfer></filter>
          <filter id="qaArithmetic"><feFlood flood-color="lime" flood-opacity=".4" result="c"/><feComposite in="SourceGraphic" in2="c" operator="arithmetic" k1=".2" k2=".5" k3=".3" k4=".1"/></filter>
        </defs>"##
        );
        let content = match case {
            "luminance" => format!(r#"<g mask="url(#qaLuma)">{body}</g>"#),
            "alpha" => format!(r#"<g mask="url(#qaAlpha)">{body}</g>"#),
            "color" => format!(r#"<g filter="url(#qaColor)">{body}</g>"#),
            "transfer" => format!(r#"<g filter="url(#qaTransfer)">{body}</g>"#),
            "arithmetic" => format!(r#"<g filter="url(#qaArithmetic)">{body}</g>"#),
            "blur" => format!(r#"<g filter="url(#qaBlur)">{body}</g>"#),
            "shadow" => format!(r#"<g filter="url(#qaShadow)">{body}</g>"#),
            "combined" => format!(
                r#"<g mask="url(#qaAlpha)"><g filter="url(#qaShadow)" opacity=".72">{body}</g></g>"#
            ),
            _ => panic!("unknown fixture"),
        };
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}">{defs}{content}</svg>"#
        )
    }

    #[test]
    #[ignore = "requires Metal; SVG luminance/alpha masks, blur and shadow composition"]
    fn gpu_svg_effects_preserve_alpha_and_overlap() {
        let (device, queue) = gpu();
        let body = r#"<rect x="8" y="8" width="32" height="32" fill="red" opacity=".65"/><circle cx="37" cy="32" r="19" fill="blue" opacity=".55"/>"#;
        for case in [
            "luminance",
            "alpha",
            "blur",
            "shadow",
            "combined",
            "color",
            "transfer",
            "arithmetic",
        ] {
            let source = effect_fixture(body, [64, 64], case);
            for rect in [
                [0., 0., 64., 64.],
                [-8., -7., 80., 80.],
                [12.25, 16.5, 24., 24.],
            ] {
                let texture =
                    render(&device, &queue, &source, [64, 64], [96, 96], rect, 1.).unwrap();
                let actual = read(&device, &queue, &texture);
                let expected =
                    super::super::rasterize_svg_workspace(&source, [64, 64], [96, 96], rect)
                        .unwrap();
                let (mean, fraction, max) = difference(&expected, &actual);
                let alpha = expected
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(actual.as_chunks::<4>().0.iter())
                    .map(|(a, b)| a[3].abs_diff(b[3]))
                    .max()
                    .unwrap();
                eprintln!("SVG effects {case} rect={rect:?} mean={mean:.4} fraction={fraction:.6} max={max} alpha_max={alpha}");
                assert!(
                    mean < 1.5 && fraction < 0.01,
                    "{case}: material color/alpha change"
                );
                assert!(
                    actual
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[..3].iter().all(|c| *c <= p[3])),
                    "premultiplied alpha"
                );
            }
        }
        // An opaque BLACK alpha mask must reveal, not erase, its content.
        let black = r#"<svg width="64" height="64"><defs><mask id="m" mask-type="alpha"><rect width="64" height="64" fill="black" opacity=".5"/></mask></defs><rect width="64" height="64" fill="red" mask="url(#m)"/></svg>"#;
        let texture = render(
            &device,
            &queue,
            black,
            [64, 64],
            [64, 64],
            [0., 0., 64., 64.],
            1.,
        )
        .unwrap();
        let data = read(&device, &queue, &texture);
        assert_eq!(
            &data[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4],
            &[128, 0, 0, 128]
        );
    }

    #[test]
    #[ignore = "requires Metal; GPU texture effects preserve bytes, masks and opacity"]
    fn gpu_texture_effects_match_existing_buffer_pipeline() {
        use crate::effects_display::{EffectsDisplay, Request};
        use lumapaint_core::{
            layer_effects::LayerEffects,
            layer_mask::{LayerMask, MaskKind},
            selection::{Selection, SelectionShape},
        };
        let (device, queue) = gpu();
        let mut display = pollster::block_on(EffectsDisplay::new(&device, &queue)).unwrap();
        let source = r#"<svg width="64" height="64"><defs><linearGradient id="g"><stop stop-color="red" stop-opacity="0"/><stop offset="1" stop-color="blue" stop-opacity=".8"/></linearGradient></defs><rect width="64" height="64" fill="url(#g)"/><circle cx="32" cy="32" r="20" fill="lime" opacity=".4"/></svg>"#;
        let raw = render(
            &device,
            &queue,
            source,
            [64, 64],
            [512, 512],
            [0., 0., 64., 64.],
            1.,
        )
        .unwrap();
        let pixels = read(&device, &queue, &raw);
        for index in 0..3 {
            let mut effects = LayerEffects {
                enabled: true,
                ..Default::default()
            };
            effects.values = [0.4, 15., -10., 5., 0., 0., 12., -8., 10., -12.];
            effects.curves[0] = vec![[0., 0.], [0.4, 0.3], [1., 1.]];
            if index > 0 {
                let mut mask = LayerMask::from_selection(
                    MaskKind::Vector,
                    Some(&Selection::new(SelectionShape::Ellipse, [4., 8., 48., 40.])),
                    64,
                    64,
                )
                .unwrap();
                mask.density = 0.7;
                mask.inverted = index == 2;
                effects.mask = Some(mask);
            }
            let mapping = [0., 0., 0.125, 0.125];
            for opacity in [1., 0.43, 0.] {
                let a = display
                    .render_texture(&device, &queue, &raw, &effects, mapping, opacity)
                    .unwrap();
                let b = display
                    .render(
                        &device,
                        &queue,
                        Request {
                            pixels: &pixels,
                            size: (512, 512),
                            revision: 1,
                            effects: &effects,
                            opacity,
                            mapping: Some(mapping),
                            previous: None,
                        },
                    )
                    .unwrap();
                assert!(
                    read(&device, &queue, &a) == read(&device, &queue, &b),
                    "texture input must retain exactly the same RGBA/effects result"
                );
            }
        }
    }

    #[test]
    #[ignore = "requires Metal and LUMAPAINT_COMPLEX_SVG; real artwork GPU comparison"]
    fn complex_artwork_gpu_comparison() {
        let source =
            std::fs::read_to_string(std::env::var("LUMAPAINT_COMPLEX_SVG").unwrap()).unwrap();
        let tree = parsed_text::parse(&source).unwrap();
        let document = [
            tree.size().width().ceil() as u32,
            tree.size().height().ceil() as u32,
        ];
        let (device, queue) = gpu();
        for (name, zoom) in [
            (
                "fit",
                (752. / document[0] as f32).min(506. / document[1] as f32),
            ),
            ("200", 2.),
            ("400", 4.),
        ] {
            let mut cpu_times = Vec::new();
            let mut gpu_times = Vec::new();
            for sample in 0..7 {
                let rect = [
                    (document[0] as f32 - 800. / zoom) * 0.5 - sample as f32 * 13.25 / zoom,
                    (document[1] as f32 - 554. / zoom) * 0.5 + sample as f32 * 7.5 / zoom,
                    800. / zoom,
                    554. / zoom,
                ];
                let mut cpu = Vec::new();
                let mut gpu_pixels = Vec::new();
                for fast in if sample % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let start = std::time::Instant::now();
                    let texture = if fast {
                        render(&device, &queue, &source, document, [800, 554], rect, 1.)
                            .expect("artwork must use GPU")
                    } else {
                        cpu = super::super::rasterize_svg_workspace(
                            &source,
                            document,
                            [800, 554],
                            rect,
                        )
                        .unwrap();
                        device.create_texture_with_data(
                            &queue,
                            &wgpu::TextureDescriptor {
                                label: None,
                                size: wgpu::Extent3d {
                                    width: 800,
                                    height: 554,
                                    depth_or_array_layers: 1,
                                },
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: wgpu::TextureFormat::Rgba8Unorm,
                                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                                view_formats: &[],
                            },
                            wgpu::util::TextureDataOrder::LayerMajor,
                            &cpu,
                        )
                    };
                    // Both timings include GPU completion, not only command submission.
                    queue.submit([]);
                    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    let ms = start.elapsed().as_secs_f64() * 1000.;
                    if sample > 0 {
                        if fast { &mut gpu_times } else { &mut cpu_times }.push(ms);
                    }
                    if fast {
                        gpu_pixels = read(&device, &queue, &texture);
                    }
                }
                let (mean, fraction, max) = difference(&cpu, &gpu_pixels);
                eprintln!("complex GPU pixels zoom={name} sample={sample} mean={mean:.5} pixels_gt32={fraction:.6} max={max}");
                if let Ok(dir) = std::env::var("LUMAPAINT_COMPLEX_OUTPUT") {
                    if sample == 0 {
                        for (label, pixels) in [("cpu", &cpu), ("gpu", &gpu_pixels)] {
                            super::super::tiny_skia::Pixmap::from_vec(
                                pixels.clone(),
                                super::super::tiny_skia::IntSize::from_wh(800, 554).unwrap(),
                            )
                            .unwrap()
                            .save_png(
                                std::path::Path::new(&dir).join(format!("{name}-{label}.png")),
                            )
                            .unwrap();
                        }
                    }
                }
                assert!(
                    mean < 1.0 && fraction < 0.01,
                    "material rendering difference at {name}/{sample}"
                );
            }
            cpu_times.sort_by(f64::total_cmp);
            gpu_times.sort_by(f64::total_cmp);
            eprintln!(
                "complex GPU timing zoom={name} cpu_median_ms={:.3} gpu_median_ms={:.3}",
                (cpu_times[2] + cpu_times[3]) * 0.5,
                (gpu_times[2] + gpu_times[3]) * 0.5
            );
        }
    }
    #[test]
    #[ignore = "requires Metal and licensed LUMAPAINT_COMPLEX_SVG fixture"]
    fn complex_artwork_effects_gpu_comparison() {
        use crate::effects_display::{EffectsDisplay, Request};
        use lumapaint_core::layer_effects::LayerEffects;
        use lumapaint_formats::native::NativeDocumentCodec;
        let original =
            std::fs::read_to_string(std::env::var("LUMAPAINT_COMPLEX_SVG").unwrap()).unwrap();
        let body = &original[original.find("<svg").unwrap()..];
        let document = [1040, 715];
        let (device, queue) = gpu();
        let mut display = pollster::block_on(EffectsDisplay::new(&device, &queue)).unwrap();
        for case in [
            "luminance",
            "alpha",
            "blur",
            "shadow",
            "combined",
            "layer_effects",
        ] {
            let source = effect_fixture(
                body,
                document,
                if case == "layer_effects" {
                    "combined"
                } else {
                    case
                },
            );
            let mut effects = LayerEffects::default();
            if case == "layer_effects" {
                effects.enabled = true;
                effects.values[0] = 0.3;
                effects.values[1] = 12.;
                effects.curves[0] = vec![[0., 0.], [0.4, 0.3], [1., 1.]];
            }
            let opacity = if effects.active() { 0.65 } else { 1. };
            if let Ok(dir) = std::env::var("LUMAPAINT_COMPLEX_OUTPUT") {
                let mut doc = lumapaint_formats::svg::import(case.into(), source.clone()).unwrap();
                let id = doc.svg_layers().next().unwrap().id.clone();
                doc.set_layer_effects(&id, effects.clone()).unwrap();
                doc.set_layer_settings(lumapaint_core::document::LayerSettings {
                    id,
                    name: case.into(),
                    opacity,
                    locked: false,
                    alpha_locked: false,
                    mask_enabled: true,
                    mask_inverted: false,
                    mask_density: 1.,
                })
                .unwrap();
                std::fs::write(
                    std::path::Path::new(&dir).join(format!("{case}.lumapaint")),
                    doc.encode().unwrap(),
                )
                .unwrap();
            }
            for (name, zoom) in [("fit", 506_f32 / 715.), ("200", 2.), ("400", 4.)] {
                let mut cpu_times = Vec::new();
                let mut gpu_times = Vec::new();
                let full_reference = if matches!(case, "combined" | "layer_effects") {
                    let left = ((1040. * zoom - 800.) * 0.5).ceil().max(0.) as usize + 128;
                    let top = ((715. * zoom - 554.) * 0.5).ceil().max(0.) as usize + 128;
                    let w = 800 + 2 * left;
                    let h = 554 + 2 * top;
                    let pixels = super::super::rasterize_svg_workspace(
                        &source,
                        document,
                        [w as u32, h as u32],
                        [
                            (1040. - 800. / zoom) * 0.5 - left as f32 / zoom,
                            (715. - 554. / zoom) * 0.5 - top as f32 / zoom,
                            w as f32 / zoom,
                            h as f32 / zoom,
                        ],
                    )
                    .unwrap();
                    Some((pixels, w, left, top))
                } else {
                    None
                };
                for sample in 0..5 {
                    let rect = [
                        (1040. - 800. / zoom) * 0.5 - sample as f32 * 13. / zoom,
                        (715. - 554. / zoom) * 0.5 + sample as f32 * 7. / zoom,
                        800. / zoom,
                        554. / zoom,
                    ];
                    let mapping = [rect[0], rect[1], rect[2] / 800., rect[3] / 554.];
                    let mut cpu = Vec::new();
                    let mut actual = Vec::new();
                    for fast in if sample % 2 == 0 {
                        [false, true]
                    } else {
                        [true, false]
                    } {
                        let start = std::time::Instant::now();
                        let texture = if fast {
                            let raw =
                                render(&device, &queue, &source, document, [800, 554], rect, 1.)
                                    .expect("GPU eligibility");
                            if effects.active() {
                                display
                                    .render_texture(
                                        &device, &queue, &raw, &effects, mapping, opacity,
                                    )
                                    .unwrap()
                            } else {
                                raw
                            }
                        } else {
                            cpu = super::super::rasterize_svg_workspace(
                                &source,
                                document,
                                [800, 554],
                                rect,
                            )
                            .unwrap();
                            if effects.active() {
                                display
                                    .render(
                                        &device,
                                        &queue,
                                        Request {
                                            pixels: &cpu,
                                            size: (800, 554),
                                            revision: sample + 1,
                                            effects: &effects,
                                            opacity,
                                            previous: None,
                                            mapping: Some(mapping),
                                        },
                                    )
                                    .unwrap()
                            } else {
                                device.create_texture_with_data(
                                    &queue,
                                    &wgpu::TextureDescriptor {
                                        label: None,
                                        size: wgpu::Extent3d {
                                            width: 800,
                                            height: 554,
                                            depth_or_array_layers: 1,
                                        },
                                        mip_level_count: 1,
                                        sample_count: 1,
                                        dimension: wgpu::TextureDimension::D2,
                                        format: wgpu::TextureFormat::Rgba8Unorm,
                                        usage: wgpu::TextureUsages::TEXTURE_BINDING
                                            | wgpu::TextureUsages::COPY_SRC,
                                        view_formats: &[],
                                    },
                                    wgpu::util::TextureDataOrder::LayerMajor,
                                    &cpu,
                                )
                            }
                        };
                        queue.submit([]);
                        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                        let ms = start.elapsed().as_secs_f64() * 1000.;
                        if sample > 0 {
                            if fast { &mut gpu_times } else { &mut cpu_times }.push(ms);
                        }
                        if fast {
                            actual = read(&device, &queue, &texture);
                        } else if effects.active() {
                            cpu = read(&device, &queue, &texture);
                        }
                    }

                    if sample == 0 && case == "blur" {
                        // Distinguish actual filter differences from reference
                        // rasterization clipping blur inputs at the viewport edge.
                        let pad = 32.;
                        let extended = super::super::rasterize_svg_workspace(
                            &source,
                            document,
                            [864, 618],
                            [
                                rect[0] - pad / zoom,
                                rect[1] - pad / zoom,
                                864. / zoom,
                                618. / zoom,
                            ],
                        )
                        .unwrap();
                        let mut reference = Vec::with_capacity(800 * 554 * 4);
                        for y in 32..586usize {
                            reference.extend_from_slice(
                                &extended[(y * 864 + 32) * 4..(y * 864 + 832) * 4],
                            );
                        }
                        let (mean, fraction, max) = difference(&reference, &actual);
                        eprintln!("effects padded reference case={case} zoom={name} mean={mean:.5} pixels_gt32={fraction:.6} max={max}");
                        assert!(
                            mean < 1.5 && fraction < 0.01,
                            "padded blur reference mismatch"
                        );
                    }
                    if let Some((pixels, w, left, top)) = &full_reference {
                        let (mean, fraction, max) = difference(&cpu, &actual);
                        eprintln!("effects old viewport reference case={case} zoom={name} sample={sample} mean={mean:.5} pixels_gt32={fraction:.6} max={max}");
                        if sample == 0 {
                            if let Ok(dir) = std::env::var("LUMAPAINT_COMPLEX_OUTPUT") {
                                super::super::tiny_skia::Pixmap::from_vec(
                                    cpu.clone(),
                                    super::super::tiny_skia::IntSize::from_wh(800, 554).unwrap(),
                                )
                                .unwrap()
                                .save_png(
                                    std::path::Path::new(&dir)
                                        .join(format!("{case}-{name}-old-viewport.png")),
                                )
                                .unwrap();
                            }
                        }
                        cpu.clear();
                        let x = left - sample as usize * 13;
                        let y = top + sample as usize * 7;
                        for yy in y..y + 554 {
                            cpu.extend_from_slice(
                                &pixels[(yy * w + x) * 4..(yy * w + x + 800) * 4],
                            );
                        }
                        if effects.active() {
                            let texture = display
                                .render(
                                    &device,
                                    &queue,
                                    Request {
                                        pixels: &cpu,
                                        size: (800, 554),
                                        revision: 100 + sample,
                                        effects: &effects,
                                        opacity,
                                        previous: None,
                                        mapping: Some(mapping),
                                    },
                                )
                                .unwrap();
                            cpu = read(&device, &queue, &texture);
                        }
                    }
                    let (mean, fraction, max) = difference(&cpu, &actual);
                    let alpha_mean = cpu
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(actual.as_chunks::<4>().0.iter())
                        .map(|(a, b)| a[3].abs_diff(b[3]) as f64)
                        .sum::<f64>()
                        / (800. * 554.);
                    eprintln!("effects pixels case={case} zoom={name} sample={sample} mean={mean:.5} alpha_mean={alpha_mean:.5} pixels_gt32={fraction:.6} max={max}");
                    if sample == 0 {
                        if let Ok(dir) = std::env::var("LUMAPAINT_COMPLEX_OUTPUT") {
                            for (label, pixels) in [("cpu", &cpu), ("gpu", &actual)] {
                                super::super::tiny_skia::Pixmap::from_vec(
                                    pixels.clone(),
                                    super::super::tiny_skia::IntSize::from_wh(800, 554).unwrap(),
                                )
                                .unwrap()
                                .save_png(
                                    std::path::Path::new(&dir)
                                        .join(format!("{case}-{name}-{label}.png")),
                                )
                                .unwrap();
                            }
                        }
                    }
                    // AA differences can exceed 32 at dense path edges. Require
                    // the large differences to remain near reference edges or
                    // the viewport border (resvg clips filter input there).
                    let mut interior_outliers = 0;
                    for y in 24..530usize {
                        for x in 24..776usize {
                            let i = (y * 800 + x) * 4;
                            if (0..4).all(|c| cpu[i + c].abs_diff(actual[i + c]) <= 32) {
                                continue;
                            }
                            let edge = (0..4).any(|c| {
                                let mut lo = 255;
                                let mut hi = 0;
                                for yy in y - 1..=y + 1 {
                                    for xx in x - 1..=x + 1 {
                                        let v = cpu[(yy * 800 + xx) * 4 + c];
                                        lo = lo.min(v);
                                        hi = hi.max(v);
                                    }
                                }
                                hi - lo > 16
                            });
                            if !edge {
                                interior_outliers += 1;
                            }
                        }
                    }
                    eprintln!("effects edge audit case={case} zoom={name} sample={sample} interior_outliers={interior_outliers}");
                    assert!(
                        mean < 1.5 && fraction < 0.02 && interior_outliers < 45,
                        "material difference {case}/{name}: interior={interior_outliers}"
                    );
                }
                cpu_times.sort_by(f64::total_cmp);
                gpu_times.sort_by(f64::total_cmp);
                eprintln!(
                    "effects timing case={case} zoom={name} cpu_ms={:.3} gpu_ms={:.3}",
                    (cpu_times[1] + cpu_times[2]) * 0.5,
                    (gpu_times[1] + gpu_times[2]) * 0.5
                );
            }
        }
    }
}
