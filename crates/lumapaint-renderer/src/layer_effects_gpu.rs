//! GPU batch backend. Pixels/configuration remain in Rust; failed dispatches
//! leave the caller's input intact for the portable CPU implementation.
use lumapaint_core::layer_effects::LayerEffects;
use std::{
    num::NonZeroU64,
    sync::{Mutex, OnceLock},
};
pub(super) const CONFIG_LEN: usize = 1132;
const BUDGET: u64 = 64 * 1024 * 1024;
const MIN_PIXELS: usize = 262144;
pub(super) fn config(e: &LayerEffects) -> Option<Vec<f32>> {
    e.validate().ok()?;
    let neutral = LayerEffects::default();
    let color_effects = if e.enabled { e } else { &neutral };
    let plan = color_effects.prepare();
    let mask = e.mask.as_ref();
    let e = color_effects;
    let mut out = vec![0.; CONFIG_LEN + 3];
    out[0] = e.values[9] / 100.;
    out[1] = e.values[8] / 100.;
    out[2] = ((50. - e.grading_blend) / 25.).exp2();
    out[3] = e.grading_balance;
    out[4] = f32::from(e.mixer.iter().flatten().any(|v| *v != 0.));
    out[5] = f32::from(e.grading.iter().any(|z| z[1] != 0. || z[2] != 0.));
    out[6] = f32::from(e.grading_blend == 50.);
    for c in 0..3 {
        out[8 + c * 256..8 + (c + 1) * 256].copy_from_slice(&plan.tone_tables()[c]);
    }
    for c in 0..4 {
        let base = 776 + c * 68;
        out[base] = e.curves[c].len() as f32;
        out[base + 1] = f32::from(e.curve_smooth[c]);
        for (i, p) in e.curves[c].iter().enumerate() {
            let b = base + 4 + i * 4;
            out[b] = p[0];
            out[b + 1] = p[1];
            out[b + 2] = plan.curve_tangents()[c][i];
        }
    }
    for (i, v) in e.mixer.iter().flatten().enumerate() {
        out[1048 + i] = *v;
    }
    for c in 0..3 {
        out[1072 + c * 4..1075 + c * 4].copy_from_slice(&e.grading[c]);
        out[1084 + c * 4..1087 + c * 4].copy_from_slice(&plan.grading_tints()[c]);
    }
    if let Some(t) = &e.screentone {
        out[1096..1115].copy_from_slice(&[
            t.kind as u32 as f32 + 1.,
            t.variant as f32,
            t.frequency,
            t.dpi,
            t.density,
            t.size,
            t.angle,
            t.offset[0],
            t.offset[1],
            t.gradient_end,
            t.extent,
            t.seed as f32,
            t.color[0] as f32,
            t.color[1] as f32,
            t.color[2] as f32,
            0.,
            f32::from(t.paper),
            f32::from(t.luminance),
            f32::from(t.inverted),
        ]);
    }
    if let Some(mask) = mask.filter(|m| m.enabled) {
        use lumapaint_core::{
            layer_mask::MaskContent,
            selection::{Selection, SelectionOperation, SelectionShape},
        };
        out.extend(lumapaint_core::image_frame::inverse(mask.transform).ok()?);
        out[1126] = f32::from(mask.inverted);
        out[1127] = mask.density;
        match mask.content.as_ref()? {
            MaskContent::Pixel {
                width,
                height,
                runs,
                gray,
            } => {
                out[1125] = 1.;
                out[1128] = *width as f32;
                out[1129] = *height as f32;
                out[1130] = runs.len() as f32;
                for run in runs {
                    out.extend(run.map(f32::from_bits));
                }
                out[1131] = gray.len() as f32;
                for run in gray {
                    out.extend(run.map(f32::from_bits));
                }
            }
            MaskContent::Vector { selection, edits } => {
                out[1125] = 2.;
                out[1130] = selection.regions.len() as f32;
                fn append(out: &mut Vec<f32>, selection: &Selection) {
                    for region in &selection.regions {
                        let shape = match region.shape {
                            SelectionShape::Rectangle => 0.,
                            SelectionShape::Ellipse => 1.,
                            SelectionShape::Polygon => 2.,
                            SelectionShape::Stroke => 3.,
                        };
                        let op = match region.operation {
                            SelectionOperation::Replace => 0.,
                            SelectionOperation::Add => 1.,
                            SelectionOperation::Subtract => 2.,
                            SelectionOperation::Intersect => 3.,
                            SelectionOperation::Invert => 4.,
                        };
                        out.extend([shape, op, region.radius, region.points.len() as f32]);
                        out.extend(region.bounds);
                        for p in &region.points {
                            out.extend([p.x, p.y]);
                        }
                    }
                }
                append(&mut out, selection);
                out[1131] = edits.len() as f32;
                for edit in edits {
                    out.push(f32::from(edit.reveal));
                    out.extend(edit.to_geometry);
                    out.extend([
                        edit.selection.regions.len() as f32,
                        edit.clip.as_ref().map_or(0, |c| c.regions.len()) as f32,
                    ]);
                    append(&mut out, &edit.selection);
                    if let Some(clip) = &edit.clip {
                        append(&mut out, clip);
                    }
                }
            }
        }
    }
    Some(out)
}
fn capacity(bytes: u64, limits: &wgpu::Limits) -> Option<u64> {
    if bytes == 0 || !bytes.is_multiple_of(4) {
        return None;
    }
    let capacity = bytes.checked_next_power_of_two()?;
    (capacity <= limits.max_buffer_size
        && capacity <= limits.max_storage_buffer_binding_size
        && capacity <= (BUDGET - CONFIG_LEN as u64 * 4) / 3
        && (bytes / 4).div_ceil(256) <= u64::from(limits.max_compute_workgroups_per_dimension))
    .then_some(capacity)
}
struct Buffers {
    input: wgpu::Buffer,
    output: wgpu::Buffer,
    readback: wgpu::Buffer,
    config: wgpu::Buffer,
    capacity: u64,
}
struct Gpu {
    job_timing: crate::gpu_timing::JobTiming,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    buffers: Option<Buffers>,
    source: Option<(u64, usize)>,
    #[cfg(test)]
    uploaded_bytes: usize,
}
impl Gpu {
    fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) =
            pollster::block_on(crate::gpu_timing::request_job_device(&adapter)).ok()?;
        let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Layer effects"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("layer_effects.wgsl"),
                    "\n",
                    include_str!("screentone.wgsl"),
                    "\n",
                    include_str!("layer_mask.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Layer effects batch"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(error) = pollster::block_on(validation_scope.pop()) {
            #[cfg(test)]
            eprintln!("Effect shader validation: {error}");
            #[cfg(not(test))]
            let _ = error;
            return None;
        }
        Some(Self {
            job_timing: crate::gpu_timing::JobTiming::new("layer_effects", &device, &queue),
            device,
            queue,
            pipeline,
            buffers: None,
            source: None,
            #[cfg(test)]
            uploaded_bytes: 0,
        })
    }
    #[cfg(test)]
    fn render(&mut self, pixels: &[u8], config: &[f32]) -> Option<Vec<u8>> {
        self.render_source(pixels, config, None)
    }
    fn render_source(
        &mut self,
        pixels: &[u8],
        config: &[f32],
        source: Option<u64>,
    ) -> Option<Vec<u8>> {
        let size = pixels.len() as u64;
        let cap = capacity(size, &self.device.limits())?;
        let config_bytes = (config.len() * 4) as u64;
        if config_bytes > self.device.limits().max_storage_buffer_binding_size
            || cap * 3 + config_bytes > BUDGET
        {
            return None;
        }
        let memory_scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let result = (|| {
            if self
                .buffers
                .as_ref()
                .is_none_or(|b| b.capacity < size || b.config.size() < config_bytes)
            {
                let make = |label, size, usage| {
                    crate::gpu_metrics::create_buffer!(
                        self.device,
                        &wgpu::BufferDescriptor {
                            label: Some(label),
                            size,
                            usage,
                            mapped_at_creation: false,
                        }
                    )
                };
                self.buffers = Some(Buffers {
                    input: make(
                        "Effect input",
                        cap,
                        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    ),
                    output: make(
                        "Effect output",
                        cap,
                        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    ),
                    readback: make(
                        "Effect readback",
                        cap,
                        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    ),
                    config: make(
                        "Effect tables",
                        config_bytes,
                        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    ),
                    capacity: cap,
                });
            }
            let b = self.buffers.as_ref()?;
            let identity = source.map(|id| (id, pixels.len()));
            if identity.is_none() || self.source != identity {
                crate::gpu_metrics::write_buffer!(self.queue, &b.input, 0, pixels);
                #[cfg(test)]
                {
                    self.uploaded_bytes += pixels.len();
                }
            }
            self.source = identity;
            crate::gpu_metrics::write_buffer!(
                self.queue,
                &b.config,
                0,
                bytemuck::cast_slice(config)
            );
            let binding = |buffer| {
                wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer,
                    offset: 0,
                    size: NonZeroU64::new(size),
                })
            };
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: binding(&b.input),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: binding(&b.output),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: b.config.as_entire_binding(),
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
                pass.dispatch_workgroups((size as u32 / 4).div_ceil(256), 1, 1);
            }
            crate::gpu_metrics::copy_buffer_to_buffer!(encoder, &b.output, 0, &b.readback, 0, size);
            if let Some(sample) = &mut job {
                sample.resolve(&mut encoder);
            }
            self.queue.submit([encoder.finish()]);
            if let Some(sample) = job {
                sample.submitted();
            }
            let (send, recv) = std::sync::mpsc::channel();
            b.readback
                .slice(..size)
                .map_async(wgpu::MapMode::Read, move |r| {
                    let _ = send.send(r);
                });
            self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
            self.job_timing.collect(&self.device);
            recv.recv().ok()?.ok()?;
            let bytes = crate::gpu_metrics::mapped_read!(b.readback.slice(..size))
                .ok()?
                .to_vec();
            b.readback.unmap();
            Some(bytes)
        })();
        let validation = pollster::block_on(validation_scope.pop());
        let memory = pollster::block_on(memory_scope.pop());
        if validation.is_some() || memory.is_some() {
            self.source = None;
            None
        } else {
            if result.is_none() {
                self.source = None;
            }
            result
        }
    }
}
enum State {
    Idle,
    Starting,
    Ready(Box<Gpu>),
    Unavailable,
}
static GPU: OnceLock<Mutex<State>> = OnceLock::new();
pub(super) fn apply(pixels: &mut [u8], effects: &LayerEffects) -> bool {
    apply_source(pixels, effects, None)
}
// A revision identifies immutable source pixels, never adjusted output.
pub(super) fn apply_source(pixels: &mut [u8], effects: &LayerEffects, source: Option<u64>) -> bool {
    apply_mapped(pixels, effects, source, None)
}
pub(super) fn apply_region(
    pixels: &mut [u8],
    effects: &LayerEffects,
    width: u32,
    mapping: [f32; 4],
) -> bool {
    apply_mapped(pixels, effects, None, Some((width, mapping)))
}
fn apply_mapped(
    pixels: &mut [u8],
    effects: &LayerEffects,
    source: Option<u64>,
    region: Option<(u32, [f32; 4])>,
) -> bool {
    if (effects.screentone.is_some() || effects.mask.is_some()) && region.is_none() {
        return false;
    }
    if !effects.active()
        || pixels.len() / 4 < MIN_PIXELS
        || !pixels.len().is_multiple_of(4)
        || (pixels.len() as u64)
            .checked_next_power_of_two()
            .is_none_or(|c| c > (BUDGET - CONFIG_LEN as u64 * 4) / 3)
    {
        return false;
    }
    let Ok(mut state) = GPU.get_or_init(|| Mutex::new(State::Idle)).try_lock() else {
        return false;
    };
    if matches!(*state, State::Idle) {
        *state = State::Starting;
        // Adapter/shader initialization must not delay the first effect edit.
        std::thread::spawn(|| {
            let ready = Gpu::new();
            if let Ok(mut state) = GPU.get().unwrap().lock() {
                *state = ready.map_or(State::Unavailable, |gpu| State::Ready(Box::new(gpu)));
            }
        });
        return false;
    }
    let State::Ready(gpu) = &mut *state else {
        return false;
    };
    if capacity(pixels.len() as u64, &gpu.device.limits()).is_none() {
        return false;
    }
    let Some(mut config) = config(effects) else {
        return false;
    };
    if let Some((width, mapping)) = region {
        config[1120] = width as f32;
        config[1121..1125].copy_from_slice(&mapping);
    }
    let result = gpu.render_source(pixels, &config, source);
    if let Some(result) = result {
        pixels.copy_from_slice(&result);
        true
    } else {
        *state = State::Unavailable;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_and_capacity_contract_are_valid_without_a_gpu() {
        let module = wgpu::naga::front::wgsl::parse_str(concat!(
            include_str!("layer_effects.wgsl"),
            "\n",
            include_str!("screentone.wgsl"),
            "\n",
            include_str!("layer_mask.wgsl")
        ))
        .unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
        let limits = wgpu::Limits::default();
        assert_eq!(capacity(4, &limits), Some(4));
        assert_eq!(capacity(1028, &limits), Some(2048));
        assert_eq!(capacity(0, &limits), None);
        assert_eq!(capacity(3, &limits), None);
        assert_eq!(capacity(BUDGET, &limits), None);
        let mut e = LayerEffects::default();
        e.curves[0][1][0] = 0.;
        assert!(config(&e).is_none());
        let mut small = vec![17; 16];
        let before = small.clone();
        assert!(!apply(
            &mut small,
            &LayerEffects {
                enabled: true,
                ..Default::default()
            }
        ));
        assert_eq!(small, before);
    }
    fn pixels(n: usize) -> Vec<u8> {
        (0..n)
            .flat_map(|i| {
                let v = (i as u32).wrapping_mul(2654435761);
                let a = [0u8, 1, 64, 128, 255][i % 5];
                let rgb = [v as u8, (v >> 8) as u8, (v >> 16) as u8]
                    .map(|c| (u16::from(c) * u16::from(a) / 255) as u8);
                rgb.into_iter().chain([a])
            })
            .collect()
    }
    fn effects(case: usize) -> LayerEffects {
        let mut e = LayerEffects {
            enabled: true,
            ..Default::default()
        };
        if case > 0 {
            e.values = [1.2, 25., -35., 20., 10., -15., 30., -20., 25., -15.];
        }
        if case > 1 {
            e.curves = std::array::from_fn(|_| {
                (0..16)
                    .map(|i| {
                        let x = i as f32 / 15.;
                        [
                            x,
                            if i == 0 {
                                0.
                            } else if i == 15 {
                                1.
                            } else {
                                (x + 0.04 * (i as f32).sin()).clamp(0., 1.)
                            },
                        ]
                    })
                    .collect()
            });
            e.curve_smooth = [true, false, true, false];
        }
        if case > 2 {
            e.mixer = std::array::from_fn(|i| [i as f32 * 9. - 30., 40., -20.]);
            e.grading = [[15., 60., -15.], [230., 40., 12.], [330., 20., -8.]];
            e.grading_blend = 20.;
            e.grading_balance = -45.;
        }
        if case > 3 {
            e.values = [
                -5., -100., 100., -100., 100., -100., -100., 100., 100., -100.,
            ];
            e.grading_blend = 100.;
            e.grading_balance = 100.;
        }
        e
    }
    #[test]
    #[ignore = "requires hardware GPU"]
    fn gpu_effects_match_cpu_alpha_and_rounding_after_growth_and_smaller_batches() {
        let mut gpu = Gpu::new().expect("GPU required");
        for case in 0..5 {
            let e = effects(case);
            let config = config(&e).unwrap();
            for n in [65536, 17, 262144, 1, 8192] {
                let mut input = pixels(n);
                if n > 0 {
                    input[..4].copy_from_slice(&[17, 33, 66, 0]);
                }
                let mut expected = input.clone();
                crate::apply_layer_effects_cpu(&mut expected, &e);
                let old = gpu.buffers.as_ref().map(|b| b.capacity);
                let actual = gpu.render(&input, &config).expect("GPU dispatch");
                assert_eq!(actual.len(), input.len());
                for (i, (a, b)) in actual.iter().zip(&expected).enumerate() {
                    let tolerance = u8::from(i % 4 != 3);
                    assert!(
                        a.abs_diff(*b) <= tolerance,
                        "case={case}, count={n}, byte={i}: {a} != {b}"
                    );
                }
                assert_eq!(&actual[..4], &[17, 33, 66, 0]);
                if old.is_some_and(|c| c >= input.len() as u64) {
                    assert_eq!(gpu.buffers.as_ref().unwrap().capacity, old.unwrap());
                }
            }
        }
        // Public asynchronous initialization must leave input intact until ready.
        let e = effects(3);
        let mut input = pixels(MIN_PIXELS);
        let original = input.clone();
        let start = std::time::Instant::now();
        loop {
            if apply(&mut input, &e) {
                break;
            }
            assert!(input == original);
            assert!(
                start.elapsed() < std::time::Duration::from_secs(20),
                "GPU did not become available"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut expected = original;
        crate::apply_layer_effects_cpu(&mut expected, &e);
        assert!(input
            .iter()
            .zip(expected)
            .enumerate()
            .all(|(i, (a, b))| a.abs_diff(b) <= u8::from(i % 4 != 3)));
    }
    #[test]
    #[ignore = "requires hardware GPU"]
    fn resident_source_skips_uploads_and_invalidates_after_other_work() {
        let mut gpu = Gpu::new().expect("GPU required");
        let input = pixels(262144);
        for case in [1, 3, 1, 0, 4] {
            let e = effects(case);
            let actual = gpu
                .render_source(&input, &config(&e).unwrap(), Some(42))
                .unwrap();
            let mut expected = input.clone();
            crate::apply_layer_effects_cpu(&mut expected, &e);
            assert!(actual
                .iter()
                .zip(&expected)
                .enumerate()
                .all(|(i, (a, b))| a.abs_diff(*b) <= u8::from(i % 4 != 3)));
            assert_eq!(gpu.uploaded_bytes, input.len());
        }
        let e = effects(3);
        let c = config(&e).unwrap();
        // Unidentified work must evict the identity, even at the same size.
        let mut changed = input.clone();
        changed[..4].copy_from_slice(&[255, 0, 0, 255]);
        gpu.render(&changed, &c).unwrap();
        gpu.render_source(&input, &c, Some(42)).unwrap();
        assert_eq!(gpu.uploaded_bytes, input.len() * 3);
        gpu.render_source(&changed, &c, Some(43)).unwrap();
        assert_eq!(gpu.uploaded_bytes, input.len() * 4);
        let smaller = pixels(17);
        gpu.render_source(&smaller, &c, Some(43)).unwrap();
        assert_eq!(gpu.uploaded_bytes, input.len() * 4 + smaller.len());
        gpu.render_source(&input, &c, Some(42)).unwrap();
        assert_eq!(gpu.uploaded_bytes, input.len() * 5 + smaller.len());
    }

    #[test]
    #[ignore = "requires hardware GPU; Release benchmark includes completed GPU work"]
    fn benchmark_direct_effect_display_against_readback_upload() {
        use std::time::Instant;
        let mut gpu = Gpu::new().expect("GPU required");
        let mut direct = pollster::block_on(crate::effects_display::EffectsDisplay::new(
            &gpu.device,
            &gpu.queue,
        ))
        .unwrap();
        for dimension in [1024, 2048] {
            let input = pixels(dimension as usize * dimension as usize);
            let effects = effects(3);
            let config = config(&effects).unwrap();
            let legacy_texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Benchmark readback-upload texture"),
                size: wgpu::Extent3d {
                    width: dimension,
                    height: dimension,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let mut texture = None;
            let mut old_times = Vec::new();
            let mut new_times = Vec::new();
            for sample in 0..8 {
                let start = Instant::now();
                let output = gpu.render_source(&input, &config, Some(1)).unwrap();
                gpu.queue.write_texture(
                    legacy_texture.as_image_copy(),
                    &output,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(dimension * 4),
                        rows_per_image: Some(dimension),
                    },
                    legacy_texture.size(),
                );
                gpu.queue.submit([]);
                gpu.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                let old = start.elapsed().as_secs_f64() * 1000.;
                let start = Instant::now();
                texture = direct.render(
                    &gpu.device,
                    &gpu.queue,
                    crate::effects_display::Request {
                        mapping: None,
                        pixels: &input,
                        size: (dimension, dimension),
                        revision: 1,
                        effects: &effects,
                        opacity: 1.,
                        previous: texture.as_ref(),
                    },
                );
                assert!(texture.is_some());
                gpu.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                let new = start.elapsed().as_secs_f64() * 1000.;
                if sample >= 2 {
                    old_times.push(old);
                    new_times.push(new);
                }
            }
            old_times.sort_by(f64::total_cmp);
            new_times.sort_by(f64::total_cmp);
            eprintln!("effects display {dimension}x{dimension}: readback+upload median {:.3} ms, direct {:.3} ms; avoided output CPU transfers {} bytes/update (input resident on both paths)", (old_times[2]+old_times[3])*0.5, (new_times[2]+new_times[3])*0.5, input.len()*2);
        }
    }

    #[test]
    #[ignore = "requires hardware GPU; release benchmark recommended"]
    fn benchmark_effects_gpu_and_cpu_with_transfers() {
        use std::time::Instant;
        let mut gpu = Gpu::new().expect("GPU required");
        for (case, count) in [(1, 262144), (3, 262144), (1, 1048576), (3, 1048576)] {
            let e = effects(case);
            let initial_config = config(&e).unwrap();
            let input = pixels(count);
            gpu.render(&input, &initial_config).unwrap();
            let mut cpu = Vec::new();
            let mut times = Vec::new();
            for _ in 0..5 {
                let mut output = input.clone();
                let start = Instant::now();
                crate::apply_layer_effects_cpu(&mut output, &e);
                cpu.push(start.elapsed().as_secs_f64() * 1000.);
                let mut result = input.clone();
                let start = Instant::now();
                let config = config(&e).unwrap();
                let rendered = gpu.render(&input, &config).unwrap();
                result.copy_from_slice(&rendered);
                times.push(start.elapsed().as_secs_f64() * 1000.);
                assert!(result
                    .iter()
                    .zip(&output)
                    .enumerate()
                    .all(|(i, (a, b))| a.abs_diff(*b) <= u8::from(i % 4 != 3)));
            }
            let mut resident = Vec::new();
            gpu.render_source(&input, &initial_config, Some(99))
                .unwrap();
            let uploaded = gpu.uploaded_bytes;
            for _ in 0..5 {
                let mut result = input.clone();
                let start = Instant::now();
                let config = config(&e).unwrap();
                let rendered = gpu.render_source(&input, &config, Some(99)).unwrap();
                result.copy_from_slice(&rendered);
                resident.push(start.elapsed().as_secs_f64() * 1000.);
            }
            assert_eq!(gpu.uploaded_bytes, uploaded);
            resident.sort_by(f64::total_cmp);
            eprintln!("resident case={case}, pixels={count}: median {:.3} ms; input uploads during 5 updates: 0 bytes", resident[2]);
            cpu.sort_by(f64::total_cmp);
            times.sort_by(f64::total_cmp);
            eprintln!("effects case={case}, pixels={count}: CPU median {:.3} ms; GPU median {:.3} ms (upload/dispatch/readback included)",cpu[2],times[2]);
        }
    }
}

#[cfg(test)]
mod screentone_tests {
    use super::*;
    use lumapaint_core::screentone::{Screentone, ToneKind};
    #[test]
    #[ignore = "requires hardware GPU"]
    fn gpu_screentone_all_52_designs_match_document_reference() {
        let mut gpu = Gpu::new().expect("GPU required");
        let kinds = [
            ToneKind::Dot,
            ToneKind::Gradient,
            ToneKind::Line,
            ToneKind::Pattern,
            ToneKind::Cg,
            ToneKind::Sand,
            ToneKind::Crosshatch,
            ToneKind::Effect,
            ToneKind::Background,
            ToneKind::White,
            ToneKind::Transfer,
            ToneKind::Color,
            ToneKind::Copy,
        ];
        let (width, height) = (192u32, 96u32);
        let mut sheet = image::RgbaImage::new(width * 4, height * 13);
        for (row, kind) in kinds.into_iter().enumerate() {
            for variant in 0..4 {
                let tone = Screentone {
                    kind,
                    variant,
                    angle: 17.,
                    density: 47.,
                    extent: 100.,
                    offset: [13., 11.],
                    color: [26, 74, 153],
                    ..Default::default()
                };
                let effects = LayerEffects {
                    enabled: true,
                    screentone: Some(tone),
                    ..Default::default()
                };
                let input: Vec<u8> = (0..width * height)
                    .flat_map(|i| {
                        let a = [0u8, 64, 128, 255][(i / width % 4) as usize];
                        let c = (i % width) as u8 / 3;
                        [c.min(a), c.min(a), c.min(a), a]
                    })
                    .collect();
                let mapping = [-10., -8., 1.2, 0.9];
                let mut params = config(&effects).unwrap();
                params[1120] = width as f32;
                params[1121..1125].copy_from_slice(&mapping);
                let actual = gpu.render_source(&input, &params, None).unwrap();
                let mut expected = input.clone();
                crate::screentone::apply_cpu(&mut expected, &effects, width, mapping);
                let mismatches = actual
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(expected.as_chunks::<4>().0.iter())
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(
                    mismatches < actual.len() / 4 / 200,
                    "{kind:?}/{variant}: {mismatches} differing pixels"
                );
                for (i, p) in actual.as_chunks::<4>().0.iter().enumerate() {
                    let bg = if kind == ToneKind::White { 32 } else { 255 };
                    let color = [0, 1, 2]
                        .map(|c| (p[c] as u32 + bg * (255 - p[3] as u32) / 255).min(255) as u8);
                    sheet.put_pixel(
                        variant * width + i as u32 % width,
                        row as u32 * height + i as u32 / width,
                        image::Rgba([color[0], color[1], color[2], 255]),
                    );
                }
            }
        }
        if let Ok(path) = std::env::var("LUMAPAINT_TONE_SHEET") {
            sheet.save(path).unwrap();
        }
    }
}

#[cfg(test)]
mod mask_tests {
    use super::*;
    use lumapaint_core::{
        document::Point,
        layer_mask::{LayerMask, MaskKind},
        selection::{Selection, SelectionOperation, SelectionShape},
    };
    #[test]
    #[ignore = "requires hardware GPU"]
    fn layer_masks_gpu_match_cpu() {
        let mut gpu = Gpu::new().expect("GPU mask shader must compile");
        let mut selections = vec![Selection::new(SelectionShape::Ellipse, [4., 8., 24., 16.])];
        let mut compound = Selection::new(SelectionShape::Rectangle, [0., 0., 32., 32.]);
        let mut hole = Selection::new(SelectionShape::Ellipse, [8., 8., 16., 16.])
            .regions
            .remove(0);
        hole.operation = SelectionOperation::Subtract;
        compound.regions.push(hole);
        selections.push(compound);
        let mut polygon = Selection::new(SelectionShape::Polygon, [2., 2., 26., 26.]);
        polygon.regions[0].points = vec![
            Point { x: 2., y: 2. },
            Point { x: 28., y: 8. },
            Point { x: 8., y: 28. },
        ];
        selections.push(polygon);
        let mut stroke = Selection::new(SelectionShape::Stroke, [2., 2., 26., 26.]);
        stroke.regions[0].points = vec![Point { x: 4., y: 4. }, Point { x: 24., y: 20. }];
        stroke.regions[0].radius = 3.;
        selections.push(stroke);
        for selection in selections {
            for kind in [MaskKind::Pixel, MaskKind::Vector] {
                for inverted in [false, true] {
                    let mut mask =
                        LayerMask::from_selection(kind, Some(&selection), 32, 32).unwrap();
                    mask.transform_by([1.2, 0.3, -0.2, 0.8, 4., -2.]).unwrap();
                    mask.paint_selection(
                        &Selection::new(SelectionShape::Ellipse, [4., 4., 12., 12.]),
                        Some(&Selection::new(
                            SelectionShape::Rectangle,
                            [6., 0., 20., 32.],
                        )),
                        false,
                    )
                    .unwrap();
                    mask.paint_selection(
                        &Selection::new(SelectionShape::Rectangle, [10., 6., 5., 8.]),
                        None,
                        true,
                    )
                    .unwrap();
                    mask.transform_by([1., 0.1, 0., 1., 1., -1.]).unwrap();
                    if kind == MaskKind::Pixel {
                        mask.paint_brush_dabs(
                            (32, 32),
                            &[lumapaint_core::tiles::RasterDab {
                                x: 16.,
                                y: 16.,
                                radius: 10.,
                                hardness: 0.5,
                                weight: 0.2,
                                texture: 0.,
                                texture_scale: 1.,
                            }],
                            lumapaint_core::document::Brush {
                                color: [80; 3],
                                opacity: 0.6,
                                alpha: 0.7,
                                ..Default::default()
                            },
                            None,
                        )
                        .unwrap();
                    }
                    mask.inverted = inverted;
                    mask.density = 0.65;
                    let effects = LayerEffects {
                        mask: Some(mask),
                        ..Default::default()
                    };
                    let input = [31, 47, 63, 128].repeat(64 * 64);
                    let mapping = [-2., -3., 0.6, 0.7];
                    let mut expected = input.clone();
                    crate::screentone::apply_cpu(&mut expected, &effects, 64, mapping);
                    let mut plan = config(&effects).unwrap();
                    plan[1120] = 64.;
                    plan[1121..1125].copy_from_slice(&mapping);
                    let actual = gpu.render(&input, &plan).unwrap();
                    assert!(
                        actual
                            .iter()
                            .zip(&expected)
                            .all(|(a, b)| a.abs_diff(*b) <= 1),
                        "{kind:?} inverted={inverted}, mismatches={:?}",
                        actual
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .zip(expected.as_chunks::<4>().0.iter())
                            .enumerate()
                            .filter(|(_, (a, b))| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > 1))
                            .take(10)
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}
