use std::num::NonZeroU64;
use std::sync::{Mutex, OnceLock};
const CACHE_BUDGET: u64 = 64 * 1024 * 1024;
struct Buffers {
    input: wgpu::Buffer,
    source: wgpu::Buffer,
    params: wgpu::Buffer,
    output: wgpu::Buffer,
    readback: wgpu::Buffer,
    capacities: [u64; 3],
}
fn capacities(required: [u64; 3], limits: &wgpu::Limits) -> Option<[u64; 3]> {
    let capacities = [
        required[0].max(4).checked_next_power_of_two()?,
        required[1].max(4).checked_next_power_of_two()?,
        required[2].max(4).checked_next_power_of_two()?,
    ];
    if required.contains(&0)
        || capacities
            .iter()
            .any(|n| *n > limits.max_buffer_size || *n > limits.max_storage_buffer_binding_size)
        || capacities[0] + capacities[1] + 2 * capacities[2] + 48 > CACHE_BUDGET
    {
        return None;
    }
    Some(capacities)
}
struct Gpu {
    job_timing: crate::gpu_timing::JobTiming,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    buffers: Option<Buffers>,
    #[cfg(test)]
    allocations: usize,
}
impl Gpu {
    fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) =
            pollster::block_on(crate::gpu_timing::request_job_device(&adapter)).ok()?;
        let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Retouch"),
            source: wgpu::ShaderSource::Wgsl(include_str!("retouch.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Retouch blend"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(error) = pollster::block_on(validation_scope.pop()) {
            #[cfg(test)]
            eprintln!("Retouch GPU validation: {error}");
            #[cfg(not(test))]
            let _ = error;
            return None;
        }
        Some(Self {
            job_timing: crate::gpu_timing::JobTiming::new("retouch", &device, &queue),
            device,
            queue,
            pipeline,
            buffers: None,
            #[cfg(test)]
            allocations: 0,
        })
    }
    fn render(
        &mut self,
        values: &[[f32; 8]],
        source: &[[f32; 4]],
        params: [f32; 12],
    ) -> Option<Vec<u8>> {
        let size = values.len() as u64 * 4;
        let limits = self.device.limits();
        if source.len() as u64 * 16 > limits.max_storage_buffer_binding_size
            || size == 0
            || values.len() as u64 * 32 > limits.max_storage_buffer_binding_size
            || size > limits.max_buffer_size
            || (values.len() as u64).div_ceil(256)
                > u64::from(limits.max_compute_workgroups_per_dimension)
        {
            return None;
        }
        let required = [values.len() as u64 * 32, source.len() as u64 * 16, size];
        let capacity = capacities(required, &limits)?;
        if self
            .buffers
            .as_ref()
            .is_none_or(|b| b.capacities.iter().zip(required).any(|(c, r)| *c < r))
        {
            let create = |label, size, usage| {
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
            #[cfg(test)]
            {
                self.allocations += 1;
            }
            self.buffers = Some(Buffers {
                input: create(
                    "Retouch inputs",
                    capacity[0],
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                ),
                source: create(
                    "Retouch source",
                    capacity[1],
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                ),
                params: create(
                    "Retouch parameters",
                    48,
                    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                ),
                output: create(
                    "Retouch output",
                    capacity[2],
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                ),
                readback: create(
                    "Retouch readback",
                    capacity[2],
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                ),
                capacities: capacity,
            });
        }
        let buffers = self.buffers.as_ref()?;
        crate::gpu_metrics::write_buffer!(
            self.queue,
            &buffers.input,
            0,
            bytemuck::cast_slice(values)
        );
        crate::gpu_metrics::write_buffer!(
            self.queue,
            &buffers.source,
            0,
            bytemuck::cast_slice(source)
        );
        crate::gpu_metrics::write_buffer!(
            self.queue,
            &buffers.params,
            0,
            bytemuck::cast_slice(&params)
        );
        let binding = |buffer, size| {
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
                    resource: binding(&buffers.input, required[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: binding(&buffers.output, size),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: binding(&buffers.source, required[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: buffers.params.as_entire_binding(),
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
            pass.dispatch_workgroups((values.len() as u32).div_ceil(256), 1, 1);
        }
        crate::gpu_metrics::copy_buffer_to_buffer!(
            encoder,
            &buffers.output,
            0,
            &buffers.readback,
            0,
            size
        );
        if let Some(sample) = &mut job {
            sample.resolve(&mut encoder);
        }
        self.queue.submit([encoder.finish()]);
        if let Some(sample) = job {
            sample.submitted();
        }
        let (send, recv) = std::sync::mpsc::channel();
        buffers
            .readback
            .slice(..size)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = send.send(r);
            });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        self.job_timing.collect(&self.device);
        recv.recv().ok()?.ok()?;
        let bytes = crate::gpu_metrics::mapped_read!(buffers.readback.slice(..size))
            .ok()?
            .to_vec();
        buffers.readback.unmap();
        Some(bytes)
    }
}
pub(super) fn render(
    values: &[[f32; 8]],
    source: &[[f32; 4]],
    params: [f32; 12],
) -> Option<Vec<u8>> {
    if values.len() < 4096 {
        return None;
    }
    static GPU: OnceLock<Mutex<Option<Gpu>>> = OnceLock::new();
    let mut gpu = GPU.get_or_init(|| Mutex::new(Gpu::new())).lock().ok()?;
    let device_limits = gpu.as_ref()?.device.limits();
    capacities(
        [
            values.len() as u64 * 32,
            source.len() as u64 * 16,
            values.len() as u64 * 4,
        ],
        &device_limits,
    )?;
    if (values.len() as u64).div_ceil(256)
        > u64::from(device_limits.max_compute_workgroups_per_dimension)
    {
        return None;
    }
    let result = gpu.as_mut()?.render(values, source, params);
    if result.is_none() {
        *gpu = None;
    }
    result
}

#[cfg(test)]
pub(super) fn force_render(
    values: &[[f32; 8]],
    source: &[[f32; 4]],
    params: [f32; 12],
) -> Option<Vec<u8>> {
    Gpu::new()?.render(values, source, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_capacity_is_bounded_and_rejects_empty_or_oversized_work() {
        let limits = wgpu::Limits::default();
        assert_eq!(capacities([129, 65, 17], &limits), Some([256, 128, 32]));
        assert_eq!(capacities([0, 16, 4], &limits), None);
        assert_eq!(
            capacities([32 * 1024 * 1024, 32 * 1024 * 1024, 4], &limits),
            None
        );
        assert_eq!(capacities([u64::MAX, 16, 4], &limits), None);
    }
    #[test]
    #[ignore = "requires hardware GPU"]
    fn reused_buffers_match_cpu_after_growth_and_smaller_dispatches() {
        let mut gpu = Gpu::new().expect("GPU required");
        for (count, width) in [(64, 8), (17, 4), (512, 16), (1, 1), (64, 8)] {
            let source: Vec<[f32; 4]> = (0..width * width)
                .map(|i| {
                    let a = if i % 3 == 0 { 0.4 } else { 1. };
                    [i as f32 / (width * width) as f32 * a, 0.3 * a, 0.6 * a, a]
                })
                .collect();
            let values: Vec<[f32; 8]> = (0..count)
                .map(|i| {
                    let s = source[i % (width * width)];
                    [
                        s[0],
                        s[1],
                        s[2],
                        s[3],
                        (i % width) as f32,
                        (i / width % width) as f32,
                        0.7,
                        0.,
                    ]
                })
                .collect();
            let params = [
                width as f32,
                width as f32,
                0.,
                1.,
                -1.2,
                0.4,
                0.,
                0.,
                0.7,
                0.2,
                0.1,
                1.,
            ];
            let old = gpu.buffers.as_ref().map(|b| b.capacities);
            let allocations = gpu.allocations;
            let actual = gpu.render(&values, &source, params).expect("GPU dispatch");
            assert_eq!(actual.len(), count * 4);
            let expected: Vec<_> = values
                .iter()
                .flat_map(|v| super::super::kernel(*v, &source, params))
                .collect();
            assert!(actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1));
            if let Some(old) = old {
                if old[0] >= count as u64 * 32
                    && old[1] >= (width * width) as u64 * 16
                    && old[2] >= count as u64 * 4
                {
                    assert_eq!(gpu.allocations, allocations);
                    assert_eq!(gpu.buffers.as_ref().unwrap().capacities, old);
                }
            }
        }
    }
}
