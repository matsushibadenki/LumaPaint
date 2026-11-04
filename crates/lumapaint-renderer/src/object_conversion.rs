//! Bake live artwork into ordinary paths and embedded images, without JS frame payloads.
use crate::text_outlines::{clip_geometry, geometry};
use base64::Engine;
use lumapaint_core::{
    gradient::{Gradient, GradientKind, GradientMethod, GradientStop},
    image_frame::{FrameFit, FrameImage, ImageFrame},
    object_conversion::{ConversionKind, ObjectConversion},
    vector::{FillRule, VectorObject, VectorObjectKind, VectorPaint, VectorPath},
};
use resvg::{tiny_skia as ts, usvg};
use skia_safe::{Path, PathFillType, PathOp};
const UNSUPPORTED: &str = "This appearance cannot be expanded; use Rasterize / このアピアランスは拡張できません。ラスタライズを使用してください / 无法扩展此外观，请使用栅格化";
fn clean(original: &VectorObject) -> VectorObject {
    let mut o = original.clone();
    o.text = None;
    o.image_frame = None;
    o.fill = None;
    o.stroke = None;
    o.fill_gradient = None;
    o.stroke_gradient = None;
    o.stroke_width = 0.;
    o.stroke_style = Default::default();
    o.live_corners = None;
    o.rectangle_radii = None;
    o.opacity = 1.;
    o.blend_mode = "normal".into();
    o.kind = VectorObjectKind::Compound;
    o.transform = [1., 0., 0., 1., 0., 0.];
    o.bounds_reset = false;
    o.clipping_group = None;
    o
}
fn set_path(o: &mut VectorObject, p: &Path) {
    o.path = VectorPath {
        data: p.to_svg(),
        fill_rule: if p.fill_type() == PathFillType::EvenOdd {
            FillRule::EvenOdd
        } else {
            FillRule::NonZero
        },
    };
    o.control_points = p.points().iter().map(|p| [p.x, p.y]).collect();
}
fn paint(
    p: &usvg::Paint,
    opacity: f32,
    transform: ts::Transform,
) -> Result<(VectorPaint, Option<Gradient>), String> {
    let rgba = |c: usvg::Color, a: f32| [c.red, c.green, c.blue, (a * 255.).round() as u8];
    if let usvg::Paint::Color(c) = p {
        return Ok((
            VectorPaint {
                registration: false,
                color: rgba(*c, opacity),
            },
            None,
        ));
    }
    let (base, kind, axis): (&usvg::BaseGradient, _, _) = match p {
        usvg::Paint::LinearGradient(g) => {
            let dx = g.x2() - g.x1();
            let dy = g.y2() - g.y1();
            (
                g,
                GradientKind::Linear,
                ts::Transform::from_row(dx, dy, -dy, dx, g.x1(), g.y1()),
            )
        }
        usvg::Paint::RadialGradient(g)
            if g.cx() == g.fx() && g.cy() == g.fy() && g.fr().get() == 0. =>
        {
            (
                g,
                GradientKind::Radial,
                ts::Transform::from_row(g.r().get(), 0., 0., g.r().get(), g.cx(), g.cy()),
            )
        }
        _ => return Err(UNSUPPORTED.into()),
    };
    if base.spread_method() != usvg::SpreadMethod::Pad {
        return Err(UNSUPPORTED.into());
    }
    let m = transform.pre_concat(base.transform()).pre_concat(axis);
    let mut gradient = Gradient {
        geometry: Some([m.sx, m.ky, m.kx, m.sy, m.tx, m.ty]),
        pixel_style: None,
        kind,
        angle: 0.,
        aspect: 1.,
        dither: false,
        method: GradientMethod::Classic,
        stops: base
            .stops()
            .iter()
            .map(|s| GradientStop {
                position: s.offset().get(),
                color: rgba(s.color(), s.opacity().get() * opacity),
                midpoint: 0.5,
            })
            .collect(),
    };
    // Core SVG output bakes interpolation into dense stops. Collapse only
    // collinear samples (within one 8-bit level), retaining color discontinuities.
    while gradient.stops.len() > 64 {
        let candidate = gradient
            .stops
            .windows(3)
            .enumerate()
            .find(|(_, p)| {
                let span = p[2].position - p[0].position;
                if span <= 0. || p[1].position == p[0].position || p[1].position == p[2].position {
                    return false;
                }
                let t = (p[1].position - p[0].position) / span;
                (0..4).all(|c| {
                    ((p[0].color[c] as f32 * (1. - t) + p[2].color[c] as f32 * t)
                        - p[1].color[c] as f32)
                        .abs()
                        <= 1.
                })
            })
            .map(|(i, _)| i + 1);
        let Some(index) = candidate else {
            return Err(UNSUPPORTED.into());
        };
        gradient.stops.remove(index);
    }
    gradient.validate()?;
    Ok((
        VectorPaint {
            registration: false,
            color: [0, 0, 0, 255],
        },
        Some(gradient),
    ))
}
fn raster(
    node: &usvg::Node,
    parent: ts::Transform,
    original: &VectorObject,
    request: &ObjectConversion,
    document_resolution: u32,
) -> Result<VectorObject, String> {
    let b = node.abs_layer_bounding_box().ok_or("Empty artwork")?;
    let scale = request.resolution as f32 / document_resolution.max(1) as f32;
    let width = (b.width() * scale).ceil().max(1.) as u32;
    let height = (b.height() * scale).ceil().max(1.) as u32;
    if width > 16384 || height > 16384 || width as u64 * height as u64 > 32_000_000 {
        return Err("Raster size exceeds 32 megapixels / 画像サイズが3200万画素を超えます / 图像尺寸超过3200万像素".into());
    }
    let mut pixmap = ts::Pixmap::new(width, height).ok_or("Raster allocation failed")?;
    if !request.transparent {
        pixmap.fill(ts::Color::WHITE);
    }
    // render_node translates by its absolute bbox; cancel that translation before applying ancestors.
    let transform = ts::Transform::from_scale(scale, scale)
        .pre_translate(-b.x(), -b.y())
        .pre_concat(parent)
        .pre_translate(b.x(), b.y());
    resvg::render_node(node, transform, &mut pixmap.as_mut()).ok_or("Empty artwork")?;
    if request.grayscale {
        for p in pixmap.data_mut().as_chunks_mut::<4>().0 {
            let v =
                (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32).round() as u8;
            p[0] = v;
            p[1] = v;
            p[2] = v;
        }
    }
    let data_uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD
            .encode(pixmap.encode_png().map_err(|e| e.to_string())?)
    );
    let x = b.x();
    let y = b.y();
    let right = x + width as f32 / scale;
    let bottom = y + height as f32 / scale;
    let mut object = clean(original);
    object.kind = VectorObjectKind::Rectangle;
    object.path = VectorPath {
        data: format!("M{x} {y}H{right}V{bottom}H{x}Z"),
        fill_rule: FillRule::NonZero,
    };
    object.control_points = vec![[x, y], [right, y], [right, bottom], [x, bottom]];
    object.image_frame = Some(ImageFrame {
        fitting: FrameFit::Stretch,
        content_transform: Some([1. / scale, 0., 0., 1. / scale, x, y]),
        image: Some(FrameImage {
            source_path: None,
            fingerprint: "converted".into(),
            name: original.name.clone(),
            width,
            height,
            encoded_width: width,
            encoded_height: height,
            orientation_transform: [1., 0., 0., 1., 0., 0.],
            data_uri,
        }),
    });
    object.validate()?;
    Ok(object)
}
fn push_path(
    mut object: VectorObject,
    path: Path,
    request: &ObjectConversion,
    output: &mut Vec<VectorObject>,
) -> Result<(), String> {
    if path.is_empty() {
        return Ok(());
    }
    if output.len() >= 4096 {
        return Err(
            "Too many converted paths / 変換後のパス数が上限を超えます / 转换后的路径数量超出限制"
                .into(),
        );
    }
    set_path(&mut object, &path);
    if request.kind == ConversionKind::Expand && request.fill {
        if let Some(g) = object.fill_gradient.clone() {
            let m = g.geometry.unwrap();
            let bounds = path.bounds();
            let points = [
                [bounds.left, bounds.top],
                [bounds.right, bounds.top],
                [bounds.right, bounds.bottom],
                [bounds.left, bounds.bottom],
            ];
            let inverse = lumapaint_core::image_frame::inverse(m)?;
            let local: Vec<_> = points
                .iter()
                .map(|p| {
                    [
                        inverse[0] * p[0] + inverse[2] * p[1] + inverse[4],
                        inverse[1] * p[0] + inverse[3] * p[1] + inverse[5],
                    ]
                })
                .collect();
            let lo = local.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) - 1.;
            let hi = local.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max) + 1.;
            let opaque_radial =
                g.kind == GradientKind::Radial && g.stops.iter().all(|stop| stop.color[3] == 255);
            let output_start = output.len();
            for i in 0..request.colors {
                let a = if i == 0 {
                    local.iter().map(|p| p[0]).fold(0., f32::min) - 1.
                } else {
                    i as f32 / request.colors as f32
                };
                let b = if i + 1 == request.colors {
                    local.iter().map(|p| p[0]).fold(1., f32::max) + 1.
                } else {
                    (i + 1) as f32 / request.colors as f32
                };
                let mut band = skia_safe::PathBuilder::new();
                for (n, [u, v]) in [[a, lo], [b, lo], [b, hi], [a, hi]].iter().enumerate() {
                    let p = (m[0] * u + m[2] * v + m[4], m[1] * u + m[3] * v + m[5]);
                    if n == 0 {
                        band.move_to(p);
                    } else {
                        band.line_to(p);
                    }
                }
                band.close();
                let p = if g.kind == GradientKind::Radial {
                    // Work in gradient coordinates so rotation, shear and unequal
                    // radii remain part of the ordinary output geometry.
                    let disk = |radius: f32| {
                        let map = |x: f32, y: f32| {
                            (
                                m[0] * x * radius + m[2] * y * radius + m[4],
                                m[1] * x * radius + m[3] * y * radius + m[5],
                            )
                        };
                        let mut b = skia_safe::PathBuilder::new();
                        let k = 0.552_284_8;
                        b.move_to(map(1., 0.));
                        b.cubic_to(map(1., k), map(k, 1.), map(0., 1.));
                        b.cubic_to(map(-k, 1.), map(-1., k), map(-1., 0.));
                        b.cubic_to(map(-1., -k), map(-k, -1.), map(0., -1.));
                        b.cubic_to(map(k, -1.), map(1., -k), map(1., 0.));
                        b.close();
                        b.detach()
                    };
                    let outer = if i + 1 == request.colors {
                        path.clone() // Pad the final color beyond radius one.
                    } else {
                        skia_safe::op(
                            &path,
                            &disk((i + 1) as f32 / request.colors as f32),
                            PathOp::Intersect,
                        )
                        .ok_or("Radial gradient expansion failed")?
                    };
                    if i == 0 || opaque_radial {
                        outer
                    } else {
                        skia_safe::op(
                            &outer,
                            &disk(i as f32 / request.colors as f32),
                            PathOp::Difference,
                        )
                        .ok_or("Radial gradient expansion failed")?
                    }
                } else {
                    skia_safe::op(&path, &band.detach(), PathOp::Intersect)
                        .ok_or("Gradient expansion failed")?
                };
                if p.is_empty() {
                    continue;
                }
                let mut o = object.clone();
                o.fill_gradient = None;
                o.fill = Some(VectorPaint {
                    registration: false,
                    color: g.sample((i as f32 + 0.5) / request.colors as f32),
                });
                set_path(&mut o, &p);
                o.validate()?;
                if output.len() >= 4096 {
                    return Err("Too many gradient paths".into());
                }
                output.push(o);
            }
            if opaque_radial {
                // Paint outer disks first, covering antialiased shared edges.
                output[output_start..].reverse();
            }
            return Ok(());
        }
    }
    object.validate()?;
    output.push(object);
    Ok(())
}
fn collect(
    group: &usvg::Group,
    parent: ts::Transform,
    clips: &[Path],
    original: &VectorObject,
    request: &ObjectConversion,
    resolution: u32,
    output: &mut Vec<VectorObject>,
) -> Result<(), String> {
    let transform = parent.pre_concat(group.transform());
    if group.blend_mode() != usvg::BlendMode::Normal {
        return Err(UNSUPPORTED.into());
    }
    if group.opacity().get() != 1. || group.mask().is_some() || !group.filters().is_empty() {
        if !clips.is_empty() {
            return Err(UNSUPPORTED.into());
        }
        let mut r = request.clone();
        r.transparent = true;
        r.grayscale = false;
        output.push(raster(
            &usvg::Node::Group(Box::new(group.clone())),
            parent,
            original,
            &r,
            resolution,
        )?);
        return Ok(());
    }
    let mut clips = clips.to_vec();
    if let Some(c) = group.clip_path() {
        if c.clip_path().is_some() {
            return Err(UNSUPPORTED.into());
        }
        clips.push(clip_geometry(
            c.root(),
            transform.pre_concat(c.transform()),
        )?);
    }
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => {
                collect(g, transform, &clips, original, request, resolution, output)?
            }
            usvg::Node::Text(t) => collect(
                t.flattened(),
                transform,
                &clips,
                original,
                request,
                resolution,
                output,
            )?,
            usvg::Node::Image(_) => {
                if !clips.is_empty() {
                    return Err(UNSUPPORTED.into());
                }
                let mut r = request.clone();
                r.transparent = true;
                output.push(raster(node, transform, original, &r, resolution)?);
            }
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let fill_first = p.paint_order() == usvg::PaintOrder::FillAndStroke;
                for stroke in [!fill_first, fill_first] {
                    if stroke && request.kind == ConversionKind::Expand && !request.stroke {
                        continue;
                    }
                    let (path, paint_source, opacity, rule) = if stroke {
                        let Some(s) = p.stroke() else {
                            continue;
                        };
                        let stroke = s.to_tiny_skia();
                        let source = if let Some(dash) = &stroke.dash {
                            p.data().dash(dash, 1.).ok_or(UNSUPPORTED)?
                        } else {
                            p.data().clone()
                        };
                        (
                            source
                                .stroke(
                                    &ts::Stroke {
                                        dash: None,
                                        ..stroke
                                    },
                                    1.,
                                )
                                .ok_or(UNSUPPORTED)?,
                            s.paint(),
                            s.opacity().get(),
                            PathFillType::Winding,
                        )
                    } else {
                        let Some(f) = p.fill() else {
                            continue;
                        };
                        (
                            p.data().clone(),
                            f.paint(),
                            f.opacity().get(),
                            if f.rule() == usvg::FillRule::EvenOdd {
                                PathFillType::EvenOdd
                            } else {
                                PathFillType::Winding
                            },
                        )
                    };
                    let mut path = geometry(&path, transform)?.with_fill_type(rule);
                    for clip in &clips {
                        path = skia_safe::op(&path, clip, PathOp::Intersect)
                            .ok_or("Clipping failed")?;
                    }
                    let mut object = clean(original);
                    let (fill, gradient) = paint(paint_source, opacity, transform)?;
                    object.fill = Some(fill);
                    object.fill_gradient = gradient;
                    push_path(object, path, request, output)?;
                }
            }
        }
    }
    Ok(())
}
pub fn convert(
    source: &str,
    originals: &[VectorObject],
    resolution: u32,
    request: &ObjectConversion,
) -> Result<Vec<VectorObject>, String> {
    request.validate()?;
    let original = originals.first().ok_or("Empty selection")?;
    // Embedded images are already ordinary objects; never resample them on expansion.
    if matches!(
        request.kind,
        ConversionKind::Expand | ConversionKind::Appearance
    ) && original.image_frame.is_some()
    {
        return Ok(originals.to_vec());
    }
    if request.kind == ConversionKind::Expand
        && originals.iter().any(|o| {
            o.text
                .as_ref()
                .and_then(|t| t.appearance.as_ref())
                .is_some_and(|a| a.enabled)
        })
    {
        return Err(
            "Expand Appearance first / 先にアピアランスを分割してください / 请先扩展外观".into(),
        );
    }
    if request.kind == ConversionKind::Expand && !request.object && original.text.is_some() {
        return Ok(originals.to_vec());
    }
    let options = usvg::Options {
        fontdb: crate::vector::system_fonts(),
        font_resolver: crate::vector::font_resolver(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default().resolve_data,
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(source, &options).map_err(|e| e.to_string())?;
    let root = usvg::Node::Group(Box::new(tree.root().clone()));
    if request.kind == ConversionKind::Rasterize || request.kind == ConversionKind::Flatten {
        // Opaque artwork can remain vector. Transparency is composited as one region,
        // so overlaps between selected objects never acquire seams or double opacity.
        let mut output = Vec::new();
        if request.kind == ConversionKind::Flatten
            && request.preserve_vectors
            && collect(
                tree.root(),
                ts::Transform::identity(),
                &[],
                original,
                request,
                resolution,
                &mut output,
            )
            .is_ok()
            && output.iter().all(|o| {
                o.image_frame.is_none()
                    && o.fill.is_none_or(|f| f.color[3] == 255)
                    && o.fill_gradient
                        .as_ref()
                        .is_none_or(|g| g.stops.iter().all(|s| s.color[3] == 255))
            })
        {
            return Ok(output);
        }
        return Ok(vec![raster(
            &root,
            ts::Transform::identity(),
            original,
            request,
            resolution,
        )?]);
    }
    let mut output = Vec::new();
    collect(
        tree.root(),
        ts::Transform::identity(),
        &[],
        original,
        request,
        resolution,
        &mut output,
    )?;
    if request.kind == ConversionKind::Expand
        && !request.stroke
        && (original.stroke.is_some() || original.stroke_gradient.is_some())
    {
        let mut stroke = original.clone();
        stroke.fill = None;
        stroke.fill_gradient = None;
        output.push(stroke);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::{
        document::{vector_svg, Document, TextSettings},
        text_appearance::TextAppearance,
        vector::VectorText,
    };
    use lumapaint_formats::native::NativeDocumentCodec;
    fn request(kind: ConversionKind) -> ObjectConversion {
        ObjectConversion {
            kind,
            object: true,
            fill: true,
            stroke: true,
            colors: 16,
            resolution: 144,
            transparent: true,
            grayscale: false,
            preserve_vectors: true,
        }
    }
    fn text(preset: Option<&str>) -> Document {
        let mut d = Document::default();
        d.set_text_object(TextSettings {
            id: None,
            position: [70., 80.],
            color: [30, 70, 120],
            text: VectorText {
                content: "Title 見出し".into(),
                font_size: 60.,
                appearance: preset.map(|p| TextAppearance {
                    preset: p.into(),
                    enabled: true,
                    amount: 1.2,
                    custom: None,
                }),
                ..Default::default()
            },
        })
        .unwrap();
        d
    }
    fn difference(a: &str, b: &str) -> f64 {
        let a = crate::vector::rasterize_svg(a, 960, 640).unwrap();
        let b = crate::vector::rasterize_svg(b, 960, 640).unwrap();
        let error: u64 = a
            .pixels
            .iter()
            .zip(&b.pixels)
            .map(|(a, b)| a.abs_diff(*b) as u64)
            .sum();
        let ink: u64 = a.pixels.iter().map(|v| *v as u64).sum();
        error as f64 / ink.max(1) as f64
    }
    #[test]
    fn object_conversion_appearance_preserves_presets_affine_and_history() {
        crate::vector::with_compatibility_renderer(|| {
            for preset in lumapaint_core::text_appearance::presets() {
                let mut d = text(Some(&preset.id));
                d.affine_selected_vectors([0.95, 0.12, -0.12, 0.95, 14., 5.])
                    .unwrap();
                let before = d.encode().unwrap();
                let svg = d.svg_layers().next().unwrap().source.clone();
                let r = request(ConversionKind::Appearance);
                d.convert_selected_objects(false, |s, o, dpi| convert(s, o, dpi, &r))
                    .unwrap();
                let layer = d.svg_layers().next().unwrap();
                assert!(layer.vector_objects.iter().all(|o| o.text.is_none()));
                assert!(layer.vector_objects.iter().any(|o| o.image_frame.is_none()));
                let error = difference(&svg, &layer.source);
                assert!(error < 0.10, "{} error {error}", preset.id);
                let after = d.encode().unwrap();
                let restored = Document::decode(&after).unwrap();
                assert!(restored.snapshot().text_objects.is_empty());
                d.undo();
                assert_eq!(d.encode().unwrap(), before);
                d.redo();
                assert_eq!(d.encode().unwrap(), after);
            }
        });
    }
    #[test]
    fn object_conversion_expansion_chain_preserves_embedded_shadow() {
        crate::vector::with_compatibility_renderer(|| {
            let mut d = text(Some("gold"));
            let appearance = request(ConversionKind::Appearance);
            d.convert_selected_objects(false, |s, o, dpi| convert(s, o, dpi, &appearance))
                .unwrap();
            let before = d.svg_layers().next().unwrap().source.clone();
            let images: Vec<_> = d
                .svg_layers()
                .next()
                .unwrap()
                .vector_objects
                .iter()
                .filter_map(|o| o.image_frame.clone())
                .collect();
            let expand = request(ConversionKind::Expand);
            d.convert_selected_objects(false, |s, o, dpi| convert(s, o, dpi, &expand))
                .unwrap();
            let layer = d.svg_layers().next().unwrap();
            assert_eq!(
                images,
                layer
                    .vector_objects
                    .iter()
                    .filter_map(|o| o.image_frame.clone())
                    .collect::<Vec<_>>()
            );
            assert!(difference(&before, &layer.source) < 0.12);
        });
    }
    #[test]
    fn object_conversion_raster_flatten_and_atomic_failure() {
        crate::vector::with_compatibility_renderer(|| {
            for kind in [ConversionKind::Rasterize, ConversionKind::Flatten] {
                let mut d = text(Some("gold"));
                let before = d.encode().unwrap();
                let svg = d.svg_layers().next().unwrap().source.clone();
                let r = request(kind);
                d.convert_selected_objects(true, |s, o, dpi| convert(s, o, dpi, &r))
                    .unwrap();
                let l = d.svg_layers().next().unwrap();
                assert_eq!(l.vector_objects.len(), 1);
                assert!(
                    !l.source.contains("<clipPath"),
                    "Uncropped raster images must export without redundant clipping"
                );
                let image = l.vector_objects[0]
                    .image_frame
                    .as_ref()
                    .unwrap()
                    .image
                    .as_ref()
                    .unwrap();
                assert!(image.source_path.is_none());
                assert!(difference(&svg, &l.source) < 0.08);
                let after = d.encode().unwrap();
                d.undo();
                assert_eq!(d.encode().unwrap(), before);
                d.redo();
                assert_eq!(d.encode().unwrap(), after);
            }
            let mut d = text(Some("gold"));
            let before = d.encode().unwrap();
            let r = request(ConversionKind::Expand);
            assert!(d
                .convert_selected_objects(false, |s, o, dpi| convert(s, o, dpi, &r))
                .is_err());
            assert_eq!(d.encode().unwrap(), before);
            let mut r = request(ConversionKind::Rasterize);
            r.resolution = 1201;
            assert!(d
                .convert_selected_objects(true, |s, o, dpi| convert(s, o, dpi, &r))
                .is_err());
            assert_eq!(d.encode().unwrap(), before);
        });
    }
    #[test]
    fn conversion_inside_clipping_keeps_mask_history_and_composition() {
        crate::vector::with_compatibility_renderer(|| {
            let mut seed = text(None);
            let r = request(ConversionKind::Appearance);
            seed.convert_selected_objects(false, |s, o, dpi| convert(s, o, dpi, &r))
                .unwrap();
            let template = seed.svg_layers().next().unwrap().vector_objects[0].clone();
            for kind in [
                ConversionKind::Expand,
                ConversionKind::Rasterize,
                ConversionKind::Flatten,
            ] {
                let mut d = Document::default();
                let layer = d.add_vector_layer().unwrap();
                for (id, path, color) in [
                    ("art-a", "M0 0H160V160H0Z", [240, 30, 80, 128]),
                    ("art-b", "M50 20H180V180H50Z", [20, 180, 220, 150]),
                    ("mask", "M40 40H120V120H40Z", [0, 0, 0, 255]),
                ] {
                    let mut o = template.clone();
                    o.id = id.into();
                    o.name = id.into();
                    o.group_path.clear();
                    o.path.data = path.into();
                    o.fill_gradient = None;
                    o.stroke = None;
                    o.fill = Some(VectorPaint {
                        registration: false,
                        color,
                    });
                    d.upsert_vector_object(&layer, o).unwrap();
                }
                d.select_vector_objects(vec!["art-a".into(), "art-b".into(), "mask".into()])
                    .unwrap();
                d.clipping_path("create").unwrap();
                d.select_direct_objects(vec!["art-a".into(), "art-b".into()])
                    .unwrap();
                let before = d.encode().unwrap();
                let source = d.svg_layers().next().unwrap().source.clone();
                let mask = d
                    .svg_layers()
                    .next()
                    .unwrap()
                    .vector_objects
                    .iter()
                    .find(|o| o.id == "mask")
                    .unwrap()
                    .clone();
                let r = request(kind);
                d.convert_selected_objects(kind == ConversionKind::Flatten, |s, o, dpi| {
                    convert(s, o, dpi, &r)
                })
                .unwrap();
                let objects = &d.svg_layers().next().unwrap().vector_objects;
                assert_eq!(objects.iter().find(|o| o.id == "mask").unwrap(), &mask);
                assert!(objects
                    .iter()
                    .all(|o| o.group_path.contains(mask.clipping_group.as_ref().unwrap())));
                let after = d.encode().unwrap();
                let actual = &d.svg_layers().next().unwrap().source;
                assert!(difference(&source, actual) < 0.01);
                let pixels = crate::vector::rasterize_svg(actual, 960, 640).unwrap();
                assert_eq!(pixels.pixels[(30 * 960 + 60) * 4 + 3], 0);
                assert!(pixels.pixels[(60 * 960 + 60) * 4 + 3] > 0);
                let reopened = Document::decode(&after).unwrap();
                assert_eq!(
                    reopened.svg_layers().next().unwrap().source,
                    actual.as_str()
                );
                d.undo();
                assert_eq!(d.encode().unwrap(), before);
                d.redo();
                assert_eq!(d.encode().unwrap(), after);
                d.select_direct_objects(vec!["mask".into()]).unwrap();
                let before = d.encode().unwrap();
                assert!(d
                    .convert_selected_objects(false, |s, o, dpi| convert(s, o, dpi, &r))
                    .is_err());
                assert_eq!(d.encode().unwrap(), before);
            }
        });
    }
    #[test]
    fn translucent_gradient_solid_bands_preserve_alpha_and_backdrop() {
        crate::vector::with_compatibility_renderer(|| {
            let mut d = text(None);
            let r = request(ConversionKind::Appearance);
            d.convert_selected_objects(false, |s, o, dpi| convert(s, o, dpi, &r))
                .unwrap();
            let mut original = d.svg_layers().next().unwrap().vector_objects[0].clone();
            original.path.data = "M10 10H190V190H10Z".into();
            original.stroke = None;
            for kind in [GradientKind::Linear, GradientKind::Radial] {
                for alpha in [[128, 128], [0, 255], [255, 0]] {
                    original.fill_gradient = Some(Gradient {
                        geometry: Some([65., 25., -20., 45., 90., 90.]),
                        pixel_style: None,
                        kind,
                        angle: 0.,
                        aspect: 1.,
                        dither: false,
                        method: GradientMethod::Classic,
                        stops: vec![
                            GradientStop {
                                position: 0.,
                                color: [240, 30, 100, alpha[0]],
                                midpoint: 0.5,
                            },
                            GradientStop {
                                position: 1.,
                                color: [20, 190, 240, alpha[1]],
                                midpoint: 0.5,
                            },
                        ],
                    });
                    let source = vector_svg(200, 200, &[original.clone()]);
                    for colors in [2, 32, 256] {
                        let mut r = request(ConversionKind::Expand);
                        r.colors = colors;
                        let expanded = convert(&source, &[original.clone()], 72, &r).unwrap();
                        assert_eq!(expanded.len(), colors as usize);
                        assert!(expanded
                            .iter()
                            .all(|o| o.image_frame.is_none() && o.fill_gradient.is_none()));
                        let g = original.fill_gradient.as_ref().unwrap();
                        let band_at = |x: f32, y: f32| {
                            ((g.position_at(x, y).clamp(0., 1.) * colors as f32).floor() as u32)
                                .min(colors - 1)
                        };
                        for backdrop in [None, Some([30, 80, 20, 255]), Some([180, 20, 200, 100])] {
                            let mut objects = vec![];
                            if let Some(color) = backdrop {
                                let mut bg = original.clone();
                                bg.id = "background".into();
                                bg.fill_gradient = None;
                                bg.fill = Some(VectorPaint {
                                    registration: false,
                                    color,
                                });
                                objects.push(bg);
                            }
                            objects.extend(expanded.clone());
                            let actual = crate::vector::rasterize_svg(
                                &vector_svg(200, 200, &objects),
                                200,
                                200,
                            )
                            .unwrap();
                            let mut checked = 0;
                            for y in 20..180 {
                                for x in 20..180 {
                                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                                    let band = band_at(px, py);
                                    // Compare interiors. Shared-edge antialiasing is a
                                    // viewer-dependent display artifact, not overlap.
                                    if [(-1., 0.), (1., 0.), (0., -1.), (0., 1.)]
                                        .iter()
                                        .any(|(dx, dy)| band_at(px + dx, py + dy) != band)
                                    {
                                        continue;
                                    }
                                    let c = g.sample((band as f32 + 0.5) / colors as f32);
                                    let bg = backdrop.unwrap_or([0; 4]);
                                    let alpha = c[3] as f32 / 255.;
                                    let ba = bg[3] as f32 / 255.;
                                    let mut expected = [0.; 4];
                                    for channel in 0..3 {
                                        expected[channel] = c[channel] as f32 * alpha
                                            + bg[channel] as f32 * ba * (1. - alpha);
                                    }
                                    expected[3] = 255. * (alpha + ba * (1. - alpha));
                                    let pixel =
                                        &actual.pixels[(y * 200 + x) * 4..(y * 200 + x + 1) * 4];
                                    for channel in 0..4 {
                                        assert!((pixel[channel] as f32-expected[channel]).abs()<=3.,"band alpha/composite {kind:?} {colors} ({x},{y}) {pixel:?} != {expected:?}");
                                    }
                                    checked += 1;
                                }
                            }
                            assert!(checked > 100);
                        }
                    }
                }
            }
        });
    }
    #[test]
    fn object_conversion_expands_fill_strokes_and_keeps_disabled_strokes() {
        crate::vector::with_compatibility_renderer(|| {
            let mut d = text(None);
            let r = request(ConversionKind::Appearance);
            d.convert_selected_objects(false, |s, o, dpi| convert(s, o, dpi, &r))
                .unwrap();
            let mut o = d.svg_layers().next().unwrap().vector_objects[0].clone();
            o.stroke = Some(VectorPaint {
                registration: false,
                color: [240, 10, 40, 255],
            });
            o.stroke_width = 4.;
            o.fill_gradient = Some(Gradient {
                geometry: Some([60., 0., 0., 60., 70., 80.]),
                pixel_style: None,
                kind: GradientKind::Linear,
                angle: 0.,
                aspect: 1.,
                dither: false,
                method: GradientMethod::Classic,
                stops: vec![
                    GradientStop {
                        position: 0.,
                        color: [0, 200, 255, 255],
                        midpoint: 0.5,
                    },
                    GradientStop {
                        position: 1.,
                        color: [255, 20, 20, 255],
                        midpoint: 0.5,
                    },
                ],
            });
            for kind in [GradientKind::Linear, GradientKind::Radial] {
                o.fill_gradient.as_mut().unwrap().kind = kind;
                o.fill_gradient.as_mut().unwrap().geometry = Some([60., 12., -15., 35., 70., 80.]);
                if kind == GradientKind::Radial {
                    let mut rectangle = o.clone();
                    rectangle.stroke = None;
                    rectangle.path.data = "M0 0H200V200H0Z".into();
                    let source = vector_svg(200, 200, &[rectangle.clone()]);
                    let expanded =
                        convert(&source, &[rectangle], 72, &request(ConversionKind::Expand))
                            .unwrap();
                    let pixels =
                        crate::vector::rasterize_svg(&vector_svg(200, 200, &expanded), 200, 200)
                            .unwrap();
                    for y in 1..199 {
                        for x in 1..199 {
                            assert_eq!(
                                pixels.pixels[(y * 200 + x) * 4 + 3],
                                255,
                                "opaque radial expansion must not expose the background at {x},{y}"
                            );
                        }
                    }
                }
                let source = vector_svg(960, 640, &[o.clone()]);
                let mut r = request(ConversionKind::Expand);
                let expanded = convert(&source, &[o.clone()], 72, &r).unwrap();
                assert!(expanded
                    .iter()
                    .all(|o| o.stroke.is_none() && o.fill_gradient.is_none()));
                assert!(difference(&source, &vector_svg(960, 640, &expanded)) < 0.13);
                r.stroke = false;
                r.fill = false;
                let expanded = convert(&source, &[o.clone()], 72, &r).unwrap();
                assert!(expanded.iter().any(|o| o.stroke.is_some()));
                assert!(difference(&source, &vector_svg(960, 640, &expanded)) < 0.05);
            }
        });
    }
}
