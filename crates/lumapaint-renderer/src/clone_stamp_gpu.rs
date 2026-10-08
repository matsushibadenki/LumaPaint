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
            label: Some("Clone stamp"),
            source: wgpu::ShaderSource::Wgsl(include_str!("clone_stamp.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Clone stamp blend"),
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
            job_timing: crate::gpu_timing::JobTiming::new("clone_stamp", &device, &queue),
            device,
            queue,
            pipeline,
        })
    }
    fn render(&self, values: &[[f32; 12]]) -> Option<Vec<u8>> {
        let size = values.len() as u64 * 4;
        let limits = self.device.limits();
        if size == 0
            || values.len() as u64 * 48 > limits.max_storage_buffer_binding_size
            || size > limits.max_buffer_size
            || (values.len() as u64).div_ceil(256)
                > u64::from(limits.max_compute_workgroups_per_dimension)
        {
            return None;
        }
        let input = crate::gpu_metrics::create_buffer_init!(
            self.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Stamp blend inputs"),
                contents: bytemuck::cast_slice(values),
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
                    resource: input.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
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
            pass.dispatch_workgroups((values.len() as u32).div_ceil(256), 1, 1);
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
pub(super) fn render(values: &[[f32; 12]]) -> Option<Vec<u8>> {
    if values.len() < 4096 {
        return None;
    }
    static GPU: OnceLock<Mutex<Option<Gpu>>> = OnceLock::new();
    let mut gpu = GPU.get_or_init(|| Mutex::new(Gpu::new())).lock().ok()?;
    let result = gpu.as_ref()?.render(values);
    if result.is_none() {
        *gpu = None;
    }
    result
}
