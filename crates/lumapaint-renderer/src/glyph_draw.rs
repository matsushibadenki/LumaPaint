//! Instanced coverage drawing. Retain batches; placement/color are separate from masks.
use crate::{
    glyph_atlas::{AtlasError, GlyphAtlas},
    glyph_run::GlyphRun,
};
use wgpu::util::DeviceExt;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    rect: [f32; 4],
    atlas: [f32; 4],
    color: [f32; 4],
    clip: [f32; 4],
}
pub struct GlyphPipeline {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}
pub struct GlyphBatch {
    binding: wgpu::BindGroup,
    count: u32,
    generation: u64,
}
impl GlyphPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Glyph atlas instances"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = crate::create_layer_pipeline_with_fragment(
            device,
            &[&layout],
            format,
            include_str!("glyph_draw.wgsl"),
            "Retained glyph atlas",
            if format.is_srgb() {
                "fs_main"
            } else {
                "fs_encoded"
            },
        );
        Self { layout, pipeline }
    }
    pub fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &mut GlyphAtlas,
        run: &GlyphRun,
    ) -> Result<GlyphBatch, AtlasError> {
        let mut instances = Vec::with_capacity(run.glyphs.len());
        for g in &run.glyphs {
            let slot = atlas.ensure(queue, &g.key)?;
            let r = slot.rectangle;
            instances.push(Instance {
                rect: [g.origin[0], g.origin[1], r[2] as f32, r[3] as f32],
                atlas: r.map(|v| v as f32),
                color: g.color,
                clip: g.clip,
            });
        }
        if instances.is_empty() {
            return Err(AtlasError::InvalidMask);
        }
        let storage = crate::gpu_metrics::create_buffer_init!(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Glyph instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::STORAGE
            }
        );
        let size = [run.size[0] as f32, run.size[1] as f32, 0., 0.];
        let uniform = crate::gpu_metrics::create_buffer_init!(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Glyph viewport"),
                contents: bytemuck::cast_slice(&size),
                usage: wgpu::BufferUsages::UNIFORM
            }
        );
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Glyph atlas batch"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(atlas.view()),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: storage.as_entire_binding(),
                },
            ],
        });
        crate::performance::count(
            "glyph_instance_uploaded_bytes",
            (instances.len() * std::mem::size_of::<Instance>() + 16) as u64,
        );
        Ok(GlyphBatch {
            binding,
            count: instances.len() as u32,
            generation: atlas.generation(),
        })
    }
    /// Reject stale batches after reset; caller must then rebuild or use fallback.
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        atlas: &GlyphAtlas,
        batch: &GlyphBatch,
    ) -> bool {
        if batch.generation != atlas.generation() {
            return false;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &batch.binding, &[]);
        pass.draw(0..6, 0..batch.count);
        true
    }
}

#[cfg(test)]
pub(crate) use tests::source;

#[cfg(test)]
mod tests {
    use super::*;
    use resvg::{tiny_skia, usvg};
    pub(crate) fn source(text: &str, extra: &str) -> String {
        let fonts = crate::vector::system_fonts();
        // A reproducible locally installed non-variable outline font; no download.
        let family = fonts
            .faces()
            .find_map(|info| {
                fonts
                    .with_face_data(info.id, |data, index| {
                        let face = ttf_parser::Face::parse(data, index).ok()?;
                        (!face.is_variable() && face.glyph_index('A').is_some())
                            .then(|| info.families[0].0.clone())
                    })
                    .flatten()
            })
            .unwrap();
        let family = family.replace('&', "&amp;").replace('"', "&quot;");
        let source=format!("<svg xmlns='http://www.w3.org/2000/svg' width='256' height='128'><defs><clipPath id='c'><rect x='10' y='10' width='230' height='108'/></clipPath></defs><g clip-path='url(#c)'><text x='20' y='50' font-family=\"{family}\" font-size='24' fill='#395d91' {extra}>{text}</text></g></svg>");
        if extra.contains("data-atlas-boundary") {
            source.replace(
                "x='10' y='10' width='230' height='108'",
                "x='22.25' y='30.5' width='11.5' height='18.25'",
            )
        } else {
            source
        }
    }
    fn tree(text: &str, extra: &str) -> usvg::Tree {
        usvg::Tree::from_str(
            &source(text, extra),
            &usvg::Options {
                fontdb: crate::vector::system_fonts(),
                font_resolver: crate::vector::font_resolver(),
                ..Default::default()
            },
        )
        .unwrap()
    }
    #[test]
    fn run_reuses_shaping_and_rejects_effects() {
        let t = tree("ABC ABC", " ");
        let first = GlyphRun::from_tree(&t, [256, 128], tiny_skia::Transform::identity()).unwrap();
        let moved =
            GlyphRun::from_tree(&t, [256, 128], tiny_skia::Transform::from_translate(3., 2.))
                .unwrap();
        assert_eq!(first.glyphs.len(), 6);
        assert!(GlyphRun::from_tree(
            &t,
            [256, 128],
            tiny_skia::Transform::from_translate(0.5, 0.)
        )
        .is_some());
        assert!(
            GlyphRun::from_tree(&t, [256, 128], tiny_skia::Transform::from_scale(0.75, 0.75))
                .is_some()
        );
        let single = source("A", "data-atlas-boundary='1'");
        let start = single.find("<text ").unwrap();
        let end = single.find("</text>").unwrap() + 7;
        let duplicate = format!(
            "{}{}{}",
            &single[..end],
            &single[start..end],
            &single[end..]
        );
        let options = usvg::Options {
            fontdb: crate::vector::system_fonts(),
            font_resolver: crate::vector::font_resolver(),
            ..Default::default()
        };
        let overlap = usvg::Tree::from_str(&duplicate, &options).unwrap();
        assert!(
            GlyphRun::from_tree(&overlap, [256, 128], tiny_skia::Transform::identity()).is_none(),
            "overlapping glyph blending must keep compatibility"
        );
        let nested = single
            .replace(
                "<g clip-path='url(#c)'>",
                "<g clip-path='url(#c)'><g clip-path='url(#c)'>",
            )
            .replace("</g></svg>", "</g></g></svg>");
        assert!(
            GlyphRun::from_tree(
                &usvg::Tree::from_str(&nested, &options).unwrap(),
                [256, 128],
                tiny_skia::Transform::identity()
            )
            .is_none(),
            "nested fractional masks must not be reduced to a rectangle intersection"
        );
        for (a, b) in first.glyphs.iter().zip(&moved.glyphs) {
            assert_eq!(a.key, b.key);
            assert_eq!([a.origin[0] + 3., a.origin[1] + 2.], b.origin);
        }
        assert!(GlyphRun::from_tree(
            &tree("ABC", "stroke='black'"),
            [256, 128],
            tiny_skia::Transform::identity()
        )
        .is_none());
        assert!(GlyphRun::from_tree(
            &tree("ABC", "text-decoration='underline'"),
            [256, 128],
            tiny_skia::Transform::identity()
        )
        .is_none());
    }
    #[test]
    #[ignore = "requires a real GPU"]
    fn gpu_shaped_text_matches_compatibility_and_reuses_masks() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let pipeline = GlyphPipeline::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let mut atlas = GlyphAtlas::new(&device, 2048).unwrap();
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
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut compared = 0;
        let mut boundary_compared = 0;
        for density in [0.25, 0.5, 0.75, 1., 1.25, 1.5, 2.] {
            for (text, style, transform) in [
                ("ABC ABC", "", tiny_skia::Transform::identity()),
                (
                    "ABC ABC",
                    "fill-opacity='0.6'",
                    tiny_skia::Transform::identity(),
                ),
                ("日本語 中文 ABC", "", tiny_skia::Transform::identity()),
                (
                    "ABC",
                    "writing-mode='vertical-rl'",
                    tiny_skia::Transform::identity(),
                ),
                ("ABC ABC", "", tiny_skia::Transform::from_translate(3., 2.)),
                (
                    "A",
                    "data-atlas-boundary='1'",
                    tiny_skia::Transform::identity(),
                ),
                (
                    "A",
                    "data-atlas-boundary='1' fill-opacity='0.6'",
                    tiny_skia::Transform::identity(),
                ),
            ] {
                let transform =
                    transform.pre_concat(tiny_skia::Transform::from_scale(density, density));
                let t = tree(text, style);

                let Some(run) = GlyphRun::from_tree(&t, [256, 128], transform) else {
                    eprintln!("glyph atlas density={density} fixture={text}/{style} conservative overlapping-ink fallback");
                    continue;
                };
                compared += 1;
                if style.contains("data-atlas-boundary") {
                    boundary_compared += 1;
                }
                let batch = pipeline.prepare(&device, &queue, &mut atlas, &run).unwrap();
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
                    assert!(pipeline.draw(&mut pass, &atlas, &batch));
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
                let mut expected = tiny_skia::Pixmap::new(256, 128).unwrap();
                resvg::render(&t, transform, &mut expected.as_mut());
                let max = actual
                    .iter()
                    .zip(expected.data())
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                let mean = actual
                    .iter()
                    .zip(expected.data())
                    .map(|(a, b)| u64::from(a.abs_diff(*b)))
                    .sum::<u64>() as f64
                    / actual.len() as f64;
                eprintln!("glyph atlas density={density} fixture={text}/{style} rgba_max={max} mean={mean}");
                assert!(
                    max <= 2,
                    "glyph atlas quality gate {text}/{style}: max={max}, mean={mean}"
                );
                drop(actual);
                buffer.unmap();
            }
        }
        assert!(
            compared >= 25 && boundary_compared >= 8,
            "quality suite must exercise actual GPU clips, not only fallback"
        );
    }
}
