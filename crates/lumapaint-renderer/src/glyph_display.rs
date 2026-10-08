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
        // Initial application pilot only: one physical pixel per document pixel,
        // integer canvas placement. Other densities retain established sampling.
        if scale != 1. || left.fract() != 0. || top.fract() != 0. {
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
        for g in &mut run.glyphs {
            g.clip = [
                g.clip[0].max(canvas[0]),
                g.clip[1].max(canvas[1]),
                g.clip[2].min(canvas[2]),
                g.clip[3].min(canvas[3]),
            ];
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
        // Exercise the same final sRGB layer composition used by the application.
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
            display.draw(&mut pass, &layer.id);
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
        let actual = buffer.slice(..).get_mapped_range().unwrap();
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
        drop(actual);
        buffer.unmap();

        display.begin_frame(&visible);
        assert!(display.prepare(&device, &queue, &layer, v));
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
        display.begin_frame(&visible);
        assert!(!display.prepare(&device, &queue, &layer, v.with_screen_zoom(0.5)));
        assert!(!display.ready(&layer));
        display.begin_frame(&visible);
        layer.source = crate::glyph_draw::source("ABC ABC", "stroke='black'");
        assert!(!display.prepare(&device, &queue, &layer, v));
        assert!(!display.ready(&layer));
        display.begin_frame(&HashSet::new());
        assert!(display.layers.is_empty());
    }
}
