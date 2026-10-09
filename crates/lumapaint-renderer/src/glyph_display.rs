//! Opt-in document display pilot. Default text rendering remains quality-gated.
use crate::native_composite as composite;
use crate::{
    glyph_atlas::GlyphAtlas,
    glyph_draw::{GlyphBatch, GlyphPipeline},
    glyph_run::GlyphRun,
    Viewport,
};
use lumapaint_core::document::SvgLayer;
use resvg::tiny_skia;
use std::collections::{HashMap, HashSet};
struct Layer {
    source: String,
    key: [u32; 8],
    batch: GlyphBatch,
    target: composite::Composite,
    bytes: u64,
}
pub(crate) struct Display {
    atlas: GlyphAtlas,
    pipeline: GlyphPipeline,
    composite: composite::Pipeline,
    layers: HashMap<String, Layer>,
    ready: HashSet<String>,
}
impl Display {
    pub fn opt_in(device: &wgpu::Device, format: wgpu::TextureFormat) -> Option<Self> {
        if std::env::var("LUMAPAINT_GLYPH_ATLAS").ok().as_deref() != Some("1") {
            return None;
        }
        Self::new(device, format)
    }
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Option<Self> {
        Some(Self {
            atlas: GlyphAtlas::new(device, 2048)?,
            pipeline: GlyphPipeline::new(device, wgpu::TextureFormat::Rgba8Unorm),
            composite: composite::Pipeline::new(device, format),
            layers: HashMap::new(),
            ready: HashSet::new(),
        })
    }
    pub fn begin_frame(&mut self, visible: &HashSet<String>) {
        self.ready.clear();
        self.layers.retain(|id, _| visible.contains(id));
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &SvgLayer,
        v: Viewport,
    ) -> bool {
        let prepared = self.prepare_inner(device, queue, layer, v);
        if prepared {
            self.ready.insert(layer.id.clone());
        } else {
            self.ready.remove(&layer.id);
            crate::performance::count("glyph_display_fallbacks", 1);
        }
        prepared
    }
    fn prepare_inner(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &SvgLayer,
        v: Viewport,
    ) -> bool {
        if layer.source.len() > 512 * 1024 || !layer.source.contains("<text") {
            return false;
        }
        let opacity = layer.effective_opacity();
        let scale = v.screen_zoom() * v.scale;
        let left = (v.width as f32 - v.document_width * scale) * 0.5 + v.pan_x * v.scale;
        let top = (v.height as f32 - v.document_height * scale) * 0.5 + v.pan_y * v.scale;
        let key = [
            v.width,
            v.height,
            scale.to_bits(),
            left.to_bits(),
            top.to_bits(),
            opacity.to_bits(),
            v.document_width.to_bits(),
            v.document_height.to_bits(),
        ];
        // Only the explicitly compared physical densities are qualified.
        // Other densities keep compatibility. Fractional canvas edge masks are qualified
        // only when boundary ink cannot overlap another glyph.
        if !matches!(scale, 0.25 | 0.5 | 0.75 | 1. | 1.25 | 1.5 | 2.) {
            return false;
        }
        if !scale.is_finite()
            || scale <= 0.
            || !opacity.is_finite()
            || !(0. ..=1.).contains(&opacity)
            || v.width == 0
            || v.height == 0
            || v.width > 8192
            || v.height > 8192
        {
            return false;
        }
        if self.layers.get(&layer.id).is_some_and(|l| {
            l.key == key && l.source == layer.source && l.target.size == [v.width, v.height]
        }) {
            crate::performance::count("glyph_display_layer_hits", 1);
            return true;
        }
        let tree = match crate::vector::parse_for_glyph_atlas(&layer.source) {
            Ok(t) => t,
            Err(_) => return false,
        };
        let ts = tiny_skia::Transform::from_row(scale, 0., 0., scale, left, top);
        let Some(mut run) = GlyphRun::from_tree(&tree, [v.width, v.height], ts) else {
            return false;
        };
        let canvas = [
            left,
            top,
            left + v.document_width * scale,
            top + v.document_height * scale,
        ];
        if canvas.iter().any(|v| !v.is_finite()) {
            return false;
        }
        if canvas.iter().any(|v| v.fract() != 0.) {
            if !run.ink_fits_viewport() {
                crate::performance::count("glyph_display_viewport_edge_fallback", 1);
                return false;
            }
            let mut boundary_masks = 0;
            for g in &mut run.glyphs {
                let Some(()) = g.key.set_canvas_clip([
                    canvas[0] - g.origin[0],
                    canvas[1] - g.origin[1],
                    canvas[2] - g.origin[0],
                    canvas[3] - g.origin[1],
                ]) else {
                    return false;
                };
                boundary_masks += u64::from(g.key.has_canvas_clip());
                // Keep partial boundary pixels; coverage is in the atlas mask.
                g.clip = [
                    g.clip[0].max(canvas[0].floor()),
                    g.clip[1].max(canvas[1].floor()),
                    g.clip[2].min(canvas[2].ceil()),
                    g.clip[3].min(canvas[3].ceil()),
                ];
            }
            if boundary_masks > 0 && !run.clipped_ink_is_disjoint() {
                crate::performance::count("glyph_display_fractional_canvas_fallback", 1);
                return false;
            }
            crate::performance::count("glyph_canvas_boundary_masks", boundary_masks);
        } else {
            for g in &mut run.glyphs {
                g.clip = [
                    g.clip[0].max(canvas[0]),
                    g.clip[1].max(canvas[1]),
                    g.clip[2].min(canvas[2]),
                    g.clip[3].min(canvas[3]),
                ];
            }
        }
        let bytes = u64::from(v.width) * u64::from(v.height) * 4
            + run.glyphs.len() as u64 * 64
            + layer.source.len() as u64;
        let other: u64 = self
            .layers
            .iter()
            .filter(|(id, _)| *id != &layer.id)
            .map(|(_, l)| l.bytes)
            .sum();
        if self.layers.len() >= 64 && !self.layers.contains_key(&layer.id)
            || other + bytes > 64 * 1024 * 1024
        {
            return false;
        }
        let batch = match self.pipeline.prepare(device, queue, &mut self.atlas, &run) {
            Ok(b) => b,
            Err(_) => return false,
        };
        let mut target = self.composite.target(device, [v.width, v.height]);
        target.opacity = opacity;
        crate::gpu_metrics::write_buffer!(
            queue,
            &target.uniform,
            0,
            bytemuck::cast_slice(&[opacity, 0., 0., 0.])
        );
        self.layers.insert(
            layer.id.clone(),
            Layer {
                source: layer.source.clone(),
                key,
                batch,
                target,
                bytes,
            },
        );
        crate::performance::count("glyph_display_layer_rebuilds", 1);
        true
    }
    pub fn ready(&self, layer: &SvgLayer) -> bool {
        self.ready.contains(&layer.id)
            && self.layers.get(&layer.id).is_some_and(|l| {
                l.source == layer.source && l.target.opacity == layer.effective_opacity()
            })
    }
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        for id in &self.ready {
            let layer = &self.layers[id];
            if !layer.target.dirty.replace(false) {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Glyph atlas layer composition"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &layer.target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.pipeline.draw(&mut pass, &self.atlas, &layer.batch);
            crate::performance::count("glyph_display_layer_redraws", 1);
        }
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, id: &str) {
        if !self.ready.contains(id) {
            return;
        }
        pass.set_pipeline(&self.composite.draw);
        pass.set_bind_group(0, &self.layers[id].target.binding, &[]);
        pass.draw(0..6, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a real GPU"]
    fn gpu_display_cache_tracks_source_view_and_opacity() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let mut display = Display::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb).unwrap();
        let mut layer = SvgLayer {
            id: "glyphs".into(),
            name: "glyphs".into(),
            visible: true,
            opacity: 1.,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
            source: crate::glyph_draw::source("ABC ABC", ""),
            paint_layer: false,
            vector_layer: true,
            vector_objects: vec![],
        };
        let v = Viewport {
            width: 256,
            height: 128,
            scale: 1.,
            zoom: 1.,
            pasteboard_color: None,
            dark: false,
            pan_x: 0.,
            pan_y: 0.,
            document_width: 256.,
            document_height: 128.,
            canvas_color: lumapaint_core::document::CanvasColor::White,
        }
        .with_screen_zoom(1.);
        let visible = HashSet::from([layer.id.clone()]);
        display.begin_frame(&visible);
        assert!(display.prepare(&device, &queue, &layer, v));
        assert!(display.ready(&layer));
        let mut encoder = device.create_command_encoder(&Default::default());
        display.encode(&mut encoder);
        queue.submit([encoder.finish()]);
        assert!(!display.layers[&layer.id].target.dirty.get());
        let display_id = layer.id.clone();
        let render = |display: &Display| {
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 256,
                    height: 128,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = target.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&Default::default());
            display.encode(&mut encoder);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                display.draw(&mut pass, &display_id);
            }
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 256 * 128 * 4,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(1024),
                        rows_per_image: Some(128),
                    },
                },
                target.size(),
            );
            queue.submit([encoder.finish()]);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let actual = buffer.slice(..).get_mapped_range().unwrap().to_vec();
            buffer.unmap();
            actual
        };
        let actual = render(&display);
        let tree = crate::vector::parse_for_glyph_atlas(&layer.source).unwrap();
        let mut expected = tiny_skia::Pixmap::new(256, 128).unwrap();
        resvg::render(
            &tree,
            tiny_skia::Transform::identity(),
            &mut expected.as_mut(),
        );
        let max = actual
            .iter()
            .zip(expected.data())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        eprintln!("glyph display final sRGB composition rgba_max={max}");
        assert!(max <= 2);

        for density in [0.25, 0.5, 0.75, 1., 1.25, 1.5, 2., 1.] {
            let zoomed = v.with_screen_zoom(density);
            display.begin_frame(&visible);
            assert!(display.prepare(&device, &queue, &layer, zoomed));
            let actual = render(&display);
            let ts = tiny_skia::Transform::from_row(
                density,
                0.,
                0.,
                density,
                (256. - 256. * density) * 0.5,
                (128. - 128. * density) * 0.5,
            );
            let mut expected = tiny_skia::Pixmap::new(256, 128).unwrap();
            resvg::render(&tree, ts, &mut expected.as_mut());
            let max = actual
                .iter()
                .zip(expected.data())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            eprintln!("glyph display density={density} rgba_max={max}");
            assert!(max <= 2);
            display.begin_frame(&visible);
            assert!(display.prepare(&device, &queue, &layer, zoomed));
            assert!(!display.layers[&layer.id].target.dirty.get());
        }
        // Compare fractional root coverage after frame/glyph composition.
        let base = layer.source.clone();
        let mut boundary_comparisons = 0;
        let mut boundary_densities = [0usize; 7];
        for boundary in [0, 1, 2, 3] {
            layer.source = if boundary > 0 {
                crate::glyph_draw::source("A", "")
                    .replace("<g clip-path='url(#c)'>", "<g>")
                    .replace(
                        "x='20' y='50'",
                        if boundary != 2 {
                            "x='-3' y='17'"
                        } else {
                            "x='245' y='132'"
                        },
                    )
            } else {
                base.clone()
            };
            if boundary == 3 {
                layer.source = layer
                    .source
                    .replace("<g>", "<g clip-path='url(#c)'>")
                    .replace(
                        "x='10' y='10' width='230' height='108'",
                        "x='-2.5' y='-2.5' width='15.5' height='25.5'",
                    );
            }
            let tree = crate::vector::parse_for_glyph_atlas(&layer.source).unwrap();
            for (density_index, density) in
                [0.25, 0.5, 0.75, 1., 1.25, 1.5, 2.].into_iter().enumerate()
            {
                for alpha in [1., 0.6] {
                    layer.opacity = alpha;
                    for (dx, dy) in [
                        (12.25, 20.5),
                        (12.75, 20.125),
                        (-12.25, -20.5),
                        (12.25, 20.5),
                    ] {
                        let mut moved = v.with_screen_zoom(density);
                        // Bring the document edge into view even at 2x density.
                        let left = match boundary {
                            0 => (256. - 256. * density) * 0.5 + dx,
                            2 => 256. - 256. * density + dx,
                            _ => dx,
                        };
                        let top = match boundary {
                            0 => (128. - 128. * density) * 0.5 + dy,
                            2 => 128. - 128. * density + dy,
                            _ => dy,
                        };
                        moved.pan_x = left - (256. - 256. * density) * 0.5;
                        moved.pan_y = top - (128. - 128. * density) * 0.5;
                        let transform =
                            tiny_skia::Transform::from_row(density, 0., 0., density, left, top);
                        display.begin_frame(&visible);
                        if GlyphRun::from_tree(&tree, [256, 128], transform)
                            .is_none_or(|run| !run.ink_fits_viewport())
                        {
                            // Offscreen/partially viewport-clipped ink retains compatibility.
                            assert!(!display.prepare(&device, &queue, &layer, moved));
                            assert!(!display.ready(&layer));
                            continue;
                        }
                        assert!(
                            display.prepare(&device, &queue, &layer, moved),
                            "density={density} pan=({dx},{dy}) boundary={boundary}"
                        );
                        boundary_comparisons += usize::from(boundary > 0);
                        boundary_densities[density_index] += usize::from(boundary > 0);
                        let actual = render(&display);
                        let mut expected = tiny_skia::Pixmap::new(256, 128).unwrap();
                        resvg::render(&tree, transform, &mut expected.as_mut());
                        let mut clip = tiny_skia::Pixmap::new(256, 128).unwrap();
                        clip.fill(tiny_skia::Color::BLACK);
                        let clear = tiny_skia::Paint {
                            blend_mode: tiny_skia::BlendMode::Clear,
                            ..Default::default()
                        };
                        let rectangle = tiny_skia::PathBuilder::from_rect(
                            tiny_skia::Rect::from_xywh(left, top, 256. * density, 128. * density)
                                .unwrap(),
                        );
                        clip.fill_path(
                            &rectangle,
                            &clear,
                            tiny_skia::FillRule::Winding,
                            tiny_skia::Transform::identity(),
                            None,
                        );
                        let mut mask =
                            tiny_skia::Mask::from_pixmap(clip.as_ref(), tiny_skia::MaskType::Alpha);
                        mask.invert();
                        expected.apply_mask(&mask);
                        let max = actual
                            .iter()
                            .zip(expected.data())
                            .map(|(a, b)| a.abs_diff((f32::from(*b) * alpha).round() as u8))
                            .max()
                            .unwrap();
                        eprintln!("glyph fractional density={density} pan=({dx},{dy}) opacity={alpha} boundary={boundary} rgba_max={max}");
                        if max > 2 {
                            for (i, (a, b)) in actual
                                .iter()
                                .zip(expected.data())
                                .enumerate()
                                .filter(|(_, (a, b))| {
                                    a.abs_diff((f32::from(**b) * alpha).round() as u8) > 2
                                })
                                .take(8)
                            {
                                eprintln!("canvas mismatch x={} y={} channel={} actual={} expected={} root_alpha={}",
                                    (i/4)%256, (i/4)/256, i%4, a, b, mask.data()[i/4]);
                            }
                        }
                        assert!(max <= 2);
                        display.begin_frame(&visible);
                        assert!(display.prepare(&device, &queue, &layer, moved));
                        assert!(!display.layers[&layer.id].target.dirty.get());
                    }
                }
            }
        }
        assert!(boundary_comparisons >= 12);
        assert!(
            boundary_densities.iter().all(|&count| count >= 4),
            "{boundary_densities:?}"
        );
        // Confirm the diagnosed viewport-bottom discrepancy is never qualified.
        layer.source = crate::glyph_draw::source("A", "")
            .replace("<g clip-path='url(#c)'>", "<g>")
            .replace("x='20' y='50'", "x='245' y='132'");
        let mut viewport_edge = v.with_screen_zoom(0.75);
        viewport_edge.pan_x = 12.25;
        viewport_edge.pan_y = 20.5;
        display.begin_frame(&visible);
        assert!(!display.prepare(&device, &queue, &layer, viewport_edge));
        assert!(!display.ready(&layer));
        eprintln!(
            "qualified canvas comparisons={boundary_comparisons} densities={boundary_densities:?}"
        );
        layer.source = base;
        layer.opacity = 1.;
        display.begin_frame(&visible);
        assert!(display.prepare(&device, &queue, &layer, v));
        render(&display);
        assert!(!display.layers[&layer.id].target.dirty.get());
        layer.opacity = 0.6;
        assert!(!display.ready(&layer));
        assert!(display.prepare(&device, &queue, &layer, v));
        assert_eq!(display.layers[&layer.id].target.opacity, 0.6);
        let mut shifted = v;
        shifted.pan_x = 3.;
        shifted.pan_y = 2.;
        assert!(display.prepare(&device, &queue, &layer, shifted));
        // Return to a previous view and source (Undo) also prepares correct instances.
        layer.source = crate::glyph_draw::source("XYZ XYZ", "");
        assert!(!display.ready(&layer));
        assert!(display.prepare(&device, &queue, &layer, v));
        layer.source = crate::glyph_draw::source("ABC ABC", "");
        assert!(display.prepare(&device, &queue, &layer, v));
        // Rejection must invalidate an already-ready layer in the same frame.
        assert!(display.ready(&layer));
        assert!(!display.prepare(&device, &queue, &layer, v.with_screen_zoom(0.8)));
        assert!(!display.ready(&layer));
        display.begin_frame(&visible);
        assert!(
            display.prepare(&device, &queue, &layer, v.with_screen_zoom(1.25)),
            "two-stage layout transforms must qualify 125%"
        );
        assert!(display.ready(&layer));
        display.begin_frame(&visible);
        layer.source = crate::glyph_draw::source("ABC ABC", "stroke='black'");
        assert!(!display.prepare(&device, &queue, &layer, v));
        assert!(!display.ready(&layer));
        display.begin_frame(&HashSet::new());
        assert!(display.layers.is_empty());
    }
}
