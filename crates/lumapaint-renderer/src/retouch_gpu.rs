use std::sync::{Mutex, OnceLock};
use wgpu::util::DeviceExt;
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
}
impl Gpu {
    fn new() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).ok()?;
        device.push_error_scope(wgpu::ErrorFilter::Validation);
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
        if let Some(error) = pollster::block_on(device.pop_error_scope()) {
            #[cfg(test)]
            eprintln!("Retouch GPU validation: {error}");
            #[cfg(not(test))]
            let _ = error;
            return None;
        }
        Some(Self {
            device,
            queue,
            pipeline,
        })
    }
    fn render(
        &self,
        values: &[[f32; 8]],
        source: &[[f32; 4]],
        params: [f32; 12],
    ) -> Option<Vec<u8>> {
        let size = values.len() as u64 * 4;
        let limits = self.device.limits();
        if source.len() as u64 * 16 > u64::from(limits.max_storage_buffer_binding_size)
            || size == 0
            || values.len() as u64 * 32 > u64::from(limits.max_storage_buffer_binding_size)
            || size > limits.max_buffer_size
            || (values.len() as u64).div_ceil(256)
                > u64::from(limits.max_compute_workgroups_per_dimension)
        {
            return None;
        }
        let input = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Retouch inputs"),
                contents: bytemuck::cast_slice(values),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let source_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(source),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: source_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((values.len() as u32).div_ceil(256), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, size);
        self.queue.submit([encoder.finish()]);
        let (send, recv) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = send.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        recv.recv().ok()?.ok()?;
        let bytes = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
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
    let result = gpu.as_ref()?.render(values, source, params);
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
