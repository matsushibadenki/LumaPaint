//! Extra pixel gradient kinds. Large pixels stay in Rust, with automatic CPU fallback.
use lumapaint_core::gradient::{Gradient, PixelGradientStyle};
use std::sync::{Mutex, OnceLock};
use wgpu::util::DeviceExt;
struct Gpu {
    job_timing: crate::gpu_timing::JobTiming,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
}
impl Gpu {
    fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) =
            pollster::block_on(crate::gpu_timing::request_job_device(&adapter)).ok()?;
        let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Pixel gradient"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gradient_raster.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Extra pixel gradients"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if pollster::block_on(validation_scope.pop()).is_some() {
            return None;
        }
        Some(Self {
            job_timing: crate::gpu_timing::JobTiming::new("pixel_gradient", &device, &queue),
            device,
            queue,
            pipeline,
        })
    }
    fn render(&self, g: &Gradient, w: u32, h: u32) -> Option<Vec<u8>> {
        let size = u64::from(w) * u64::from(h) * 4;
        let limits = self.device.limits();
        if size == 0
            || size > limits.max_storage_buffer_binding_size
            || size > limits.max_buffer_size
            || (w as u64 * h as u64).div_ceil(256)
                > u64::from(limits.max_compute_workgroups_per_dimension)
        {
            return None;
        }
        let [a, b, c, d, e, f] = g.geometry?;
        let params = [
            w as f32,
            h as f32,
            match g.pixel_style? {
                PixelGradientStyle::Angular => 0.,
                PixelGradientStyle::Reflected => 1.,
                PixelGradientStyle::Diamond => 2.,
            },
            if g.dither { 1. } else { 0. },
            a,
            b,
            c,
            d,
            e,
            f,
            0.,
            0.,
        ];
        let ramp: Vec<u32> = (0..=4096)
            .map(|i| u32::from_le_bytes(g.sample(i as f32 / 4096.)))
            .collect();
        let uniform = crate::gpu_metrics::create_buffer_init!(
            self.device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            }
        );
        let ramp = crate::gpu_metrics::create_buffer_init!(
            self.device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&ramp),
                usage: wgpu::BufferUsages::STORAGE,
            }
        );
        let output = crate::gpu_metrics::create_buffer!(
            self.device,
            &wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }
        );
        let readback = crate::gpu_metrics::create_buffer!(
            self.device,
            &wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }
        );
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ramp.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let mut job = self.job_timing.start(&self.device);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                timestamp_writes: job.as_ref().map(|sample| sample.writes()),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((w * h).div_ceil(256), 1, 1);
        }
        crate::gpu_metrics::copy_buffer_to_buffer!(encoder, &output, 0, &readback, 0, size);
        if let Some(sample) = &mut job {
            sample.resolve(&mut encoder);
        }
        self.queue.submit([encoder.finish()]);
        if let Some(sample) = job {
            sample.submitted();
        }
        let (send, recv) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = send.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        self.job_timing.collect(&self.device);
        recv.recv().ok()?.ok()?;
        let bytes = crate::gpu_metrics::mapped_read!(readback.slice(..))
            .ok()?
            .to_vec();
        readback.unmap();
        Some(bytes)
    }
}
pub fn rasterize(g: &Gradient, w: u32, h: u32) -> Result<Vec<u8>, String> {
    g.validate()?;
    if g.pixel_style.is_none()
        || g.geometry.is_none()
        || w == 0
        || h == 0
        || u64::from(w) * u64::from(h) > 16_777_216
    {
        return Err("Invalid pixel gradient raster request".into());
    }
    static GPU: OnceLock<Mutex<Option<Gpu>>> = OnceLock::new();
    if let Ok(gpu) = GPU.get_or_init(|| Mutex::new(Gpu::new())).lock() {
        if let Some(pixels) = gpu.as_ref().and_then(|gpu| gpu.render(g, w, h)) {
            return Ok(pixels);
        }
    }
    Ok(cpu(g, w, h))
}
fn cpu(g: &Gradient, w: u32, h: u32) -> Vec<u8> {
    let ramp: Vec<_> = (0..=4096).map(|i| g.sample(i as f32 / 4096.)).collect();
    let mut pixels = vec![0; (w * h * 4) as usize];
    for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let mut t = g.position_at((i as u32 % w) as f32 + 0.5, (i as u32 / w) as f32 + 0.5);
        if g.dither {
            t += (((i as u32).wrapping_mul(1664525).wrapping_add(1013904223) >> 24) as f32 / 255.
                - 0.5)
                / 255.;
        }
        let c = ramp[(t.clamp(0., 1.) * 4096.).round() as usize];
        *pixel = [
            (c[0] as u16 * c[3] as u16 / 255) as u8,
            (c[1] as u16 * c[3] as u16 / 255) as u8,
            (c[2] as u16 * c[3] as u16 / 255) as u8,
            c[3],
        ];
    }
    pixels
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a real GPU"]
    fn gpu_matches_cpu_for_all_extra_styles_with_alpha_and_dither() {
        let gpu = Gpu::new().expect("Real GPU required");
        let mut g: Gradient = test_gradient();
        for style in [
            PixelGradientStyle::Angular,
            PixelGradientStyle::Reflected,
            PixelGradientStyle::Diamond,
        ] {
            g.pixel_style = Some(style);
            for dither in [false, true] {
                g.dither = dither;
                let actual = gpu.render(&g, 47, 31).unwrap();
                let expected = cpu(&g, 47, 31);
                assert!(
                    actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                    "GPU gradient differs: {style:?}"
                );
            }
        }
    }
    pub(super) fn test_gradient() -> Gradient {
        use lumapaint_core::gradient::*;
        Gradient {
            geometry: Some([22., 3., -2., 17., 23., 15.]),
            pixel_style: None,
            kind: GradientKind::Linear,
            angle: 0.,
            aspect: 1.,
            dither: false,
            method: GradientMethod::Perceptual,
            stops: vec![
                GradientStop {
                    position: 0.,
                    color: [255, 0, 0, 64],
                    midpoint: 0.3,
                },
                GradientStop {
                    position: 1.,
                    color: [0, 0, 255, 255],
                    midpoint: 0.5,
                },
            ],
        }
    }
    #[test]
    fn cpu_extra_gradients_have_premultiplied_transparency() {
        let mut g = test_gradient();
        g.pixel_style = Some(PixelGradientStyle::Diamond);
        let pixels = cpu(&g, 47, 31);
        assert_eq!(pixels.len(), 47 * 31 * 4);
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3].iter().all(|c| *c <= p[3])));
    }
}

#[cfg(test)]
mod imported_tests {
    #[test]
    fn editing_one_svg_gradient_keeps_other_instance_pixels() {
        use lumapaint_core::{document::Document, svg_backend::SvgGeometryBackend};
        let source = r##"<svg width="160" height="100"><defs><linearGradient id="g"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><rect id="shape" width="25" height="30" fill="url(#g)"/></defs><use id="one" href="#shape" x="10"/><use id="two" href="#shape" x="80"/></svg>"##;
        let mut doc = Document::default();
        let settings = serde_json::json!({"name":"Gradient","width":160,"height":100,"unit":"pixels","resolution":72,"artboards":false,"canvasColor":"transparent","pixelAspectRatio":1});
        // Use the actual core transaction; the renderer sees only the resulting SVG.
        doc.set_document_settings(serde_json::from_value(settings).unwrap())
            .unwrap();
        lumapaint_svg::attach(&mut doc);
        doc.import_svg("SVG".into(), source.into()).unwrap();
        let backend = lumapaint_svg::UsvgGeometryBackend;
        let objects = backend.objects(source, "test", [160., 100.]);
        let mut g = objects[0].fill_gradient.clone().unwrap();
        g.stops[0].color = [0, 255, 0, 128];
        g.stops[1].color = [0, 255, 0, 128];
        let id = doc.direct_objects()[0].1.id.clone();
        doc.select_direct_objects(vec![id.clone()]).unwrap();
        doc.set_selected_vector_gradient(&[id], "fill", g).unwrap();
        let before = crate::vector::rasterize_svg(source, 160, 100).unwrap();
        let after =
            crate::vector::rasterize_svg(&doc.svg_layers().next().unwrap().source, 160, 100)
                .unwrap();
        for y in 0..100 {
            assert_eq!(
                &before.pixels[(y * 160 + 75) * 4..(y * 160 + 110) * 4],
                &after.pixels[(y * 160 + 75) * 4..(y * 160 + 110) * 4]
            );
        }
        let pixel = &after.pixels[(15 * 160 + 20) * 4..(15 * 160 + 20) * 4 + 4];
        assert_eq!(pixel, &[0, 128, 0, 128]);
    }
}

#[cfg(all(test, feature = "skia"))]
mod fill_layer_tests {
    use lumapaint_core::{
        document::{Document, Point},
        selection::SelectionShape,
    };
    use lumapaint_formats::native::NativeDocumentCodec;
    #[test]
    fn gradient_fill_keeps_pixels_editable_selection_and_single_undo() {
        let mut doc = Document::default();
        doc.begin_selection(Point { x: 20., y: 30. }, SelectionShape::Ellipse);
        doc.extend_selection(Point { x: 100., y: 90. }, true);
        doc.finish();
        let before = doc.encode().unwrap();
        let gradient = super::tests::test_gradient();
        let layer = doc
            .add_gradient_fill_layer(gradient, &crate::vector::skia_paths::SkiaPathEngine)
            .unwrap();
        assert!(doc.selection().is_none());
        let svg = doc.svg_layers().find(|l| l.id == layer).unwrap();
        assert!(svg.vector_objects[0].fill_gradient.is_some());
        let rendered = crate::vector::rasterize_svg(&svg.source, 960, 640).unwrap();
        assert_eq!(rendered.pixels[(30 * 960 + 20) * 4 + 3], 0);
        assert!(rendered.pixels[(60 * 960 + 60) * 4 + 3] > 0);
        let saved = doc.encode().unwrap();
        let restored = Document::decode(&saved).unwrap();
        assert!(restored
            .svg_layers()
            .find(|l| l.id == layer)
            .unwrap()
            .vector_objects[0]
            .fill_gradient
            .is_some());
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.redo();
        assert_eq!(doc.encode().unwrap(), saved);
    }
}
