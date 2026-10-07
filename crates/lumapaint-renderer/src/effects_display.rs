//! Display effects on the compositor's own device. No readback, mapping or CPU wait.
use super::layer_effects_gpu::{config, CONFIG_LEN};
use lumapaint_core::layer_effects::LayerEffects;

const CONFIG_BYTES: u64 = (CONFIG_LEN as u64 + 3) * 4;
const BUDGET: u64 = 64 * 1024 * 1024;

pub(super) fn layout(size: (u32, u32), limits: &wgpu::Limits) -> Option<(u64, u64, u32)> {
    let (width, height) = size;
    if width == 0
        || height == 0
        || width > limits.max_texture_dimension_2d
        || height > limits.max_texture_dimension_2d
    {
        return None;
    }
    let input = u64::from(width) * u64::from(height) * 4;
    let stride = width.checked_mul(4)?.checked_add(255)? / 256 * 256;
    let output = u64::from(stride) * u64::from(height);
    let power = output.checked_next_power_of_two()?;
    // Keep geometric growth normally, but do not reject a 4K viewport solely
    // because rounding both scratch buffers to 32 MiB consumes the config budget.
    let capacity = if power * 2 + CONFIG_BYTES <= BUDGET {
        power
    } else {
        output.checked_add(65_535)? / 65_536 * 65_536
    };
    (input / 4 >= 262_144
        && input.div_ceil(1024) <= u64::from(limits.max_compute_workgroups_per_dimension)
        && capacity <= limits.max_buffer_size
        && capacity <= limits.max_storage_buffer_binding_size
        && capacity * 2 + CONFIG_BYTES <= BUDGET)
        .then_some((input, capacity, stride))
}

struct Buffers {
    input: wgpu::Buffer,
    output: wgpu::Buffer,
    config: wgpu::Buffer,
    capacity: u64,
}

pub(super) struct EffectsDisplay {
    pipeline: wgpu::ComputePipeline,
    buffers: Option<Buffers>,
    source: Option<(u64, usize)>,
    #[cfg(test)]
    uploaded_bytes: usize,
}

impl EffectsDisplay {
    pub async fn new(device: &wgpu::Device) -> Option<Self> {
        let memory_scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Direct display effects"),
            source: wgpu::ShaderSource::Wgsl(include_str!("layer_effects.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Direct display effects"),
            layout: None,
            module: &shader,
            entry_point: Some("display"),
            compilation_options: Default::default(),
            cache: None,
        });
        let invalid = validation_scope.pop().await;
        let memory = memory_scope.pop().await;
        if invalid.is_some() || memory.is_some() {
            return None;
        }
        Some(Self {
            pipeline,
            buffers: None,
            source: None,
            #[cfg(test)]
            uploaded_bytes: 0,
        })
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        request: Request<'_>,
    ) -> Option<wgpu::Texture> {
        let Request {
            pixels,
            size,
            revision,
            effects,
            opacity,
            previous,
        } = request;
        let (bytes, capacity, stride) = layout(size, &device.limits())?;
        if pixels.len() as u64 != bytes || !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return None;
        }
        let plan = config(effects)?;
        if self.buffers.as_ref().is_none_or(|b| b.capacity < capacity) {
            let make = |label, size, usage| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size,
                    usage,
                    mapped_at_creation: false,
                })
            };
            self.buffers = Some(Buffers {
                input: make(
                    "Effect display input",
                    capacity,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                ),
                output: make(
                    "Effect display output",
                    capacity,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                ),
                config: make(
                    "Effect display configuration",
                    CONFIG_BYTES,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                ),
                capacity,
            });
            self.source = None;
        }
        let buffers = self.buffers.as_ref()?;
        let identity = (revision, pixels.len());
        if self.source != Some(identity) {
            queue.write_buffer(&buffers.input, 0, pixels);
            #[cfg(test)]
            {
                self.uploaded_bytes += pixels.len();
            }
            self.source = Some(identity);
        }
        let mut parameters = [0.; CONFIG_LEN + 3];
        parameters[..CONFIG_LEN].copy_from_slice(&plan);
        parameters[CONFIG_LEN] = size.0 as f32;
        parameters[CONFIG_LEN + 1] = (stride / 4) as f32;
        parameters[CONFIG_LEN + 2] = opacity;
        queue.write_buffer(&buffers.config, 0, bytemuck::cast_slice(&parameters));
        let texture = previous
            .filter(|texture| texture.width() == size.0 && texture.height() == size.1)
            .cloned()
            .unwrap_or_else(|| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Direct effects texture"),
                    size: wgpu::Extent3d {
                        width: size.0,
                        height: size.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
            });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Direct effects buffers"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &buffers.input,
                        offset: 0,
                        size: std::num::NonZeroU64::new(bytes),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffers.output.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffers.config.as_entire_binding(),
                },
            ],
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((bytes / 4).div_ceil(256) as u32, 1, 1);
        }
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &buffers.output,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size.1),
                },
            },
            texture.as_image_copy(),
            texture.size(),
        );
        // The compositor submits after this on the same queue, so the copy is visible
        // without a CPU fence. Later writes to reused buffers are ordered by submission.
        queue.submit([encoder.finish()]);
        Some(texture)
    }
}

pub(super) struct Request<'a> {
    pub pixels: &'a [u8],
    pub size: (u32, u32),
    pub revision: u64,
    pub effects: &'a LayerEffects,
    pub opacity: f32,
    pub previous: Option<&'a wgpu::Texture>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_layout_limits_and_alignment() {
        let limits = wgpu::Limits::default();
        assert!(layout((0, 1024), &limits).is_none());
        assert!(layout((64, 64), &limits).is_none());
        let (bytes, capacity, stride) = layout((513, 512), &limits).unwrap();
        assert_eq!(bytes, 513 * 512 * 4);
        assert_eq!(stride % 256, 0);
        assert!(capacity >= u64::from(stride) * 512);
        assert!(capacity * 2 + CONFIG_BYTES <= BUDGET);
        assert!(layout((3840, 2160), &limits).is_some());
        assert!(layout((8192, 8192), &limits).is_none());
    }

    fn read(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
        let stride = (texture.width() * 4).div_ceil(256) * 256;
        let bytes = u64::from(stride) * u64::from(texture.height());
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Test-only display readback"),
            size: bytes,
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
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .expect("GPU buffer mapped after successful map callback");
        mapped
            .chunks(stride as usize)
            .flat_map(|row| row[..texture.width() as usize * 4].iter().copied())
            .collect()
    }

    #[test]
    #[ignore = "requires a real GPU; readback is used only for verification"]
    fn direct_display_matches_cpu_and_retains_source_across_adjustments() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let mut gpu =
            pollster::block_on(EffectsDisplay::new(&device)).expect("effect display pipeline");
        let mut previous = None;
        for (version, size) in [(1, (513, 512)), (2, (1024, 512)), (3, (512, 512))] {
            let pixels: Vec<_> = (0..size.0 * size.1)
                .flat_map(|i| {
                    let a = [0u8, 1, 64, 128, 255][i as usize % 5];
                    let rgb = [i as u8, (i / 5) as u8, (i / 13) as u8]
                        .map(|c| (u16::from(c) * u16::from(a) / 255) as u8);
                    rgb.into_iter().chain([a])
                })
                .collect();
            let uploaded = gpu.uploaded_bytes;
            for (index, opacity) in [1., 0.5, 0., 1.].into_iter().enumerate() {
                let mut effects = LayerEffects {
                    enabled: true,
                    ..Default::default()
                };
                if index > 0 {
                    effects.values = [1.2, 25., -35., 20., 10., -15., 30., -20., 25., -15.];
                    effects.curves[0] = vec![[0., 0.], [0.3, 0.2], [0.7, 0.8], [1., 1.]];
                    effects.mixer[2] = [15., -20., 10.];
                    effects.grading[0] = [30., 20., 5.];
                }
                let texture = gpu
                    .render(
                        &device,
                        &queue,
                        Request {
                            pixels: &pixels,
                            size,
                            revision: version,
                            effects: &effects,
                            opacity,
                            previous: previous.as_ref(),
                        },
                    )
                    .unwrap();
                let actual = read(&device, &queue, &texture);
                let mut expected = pixels.clone();
                crate::apply_layer_effects_cpu(&mut expected, &effects);
                for byte in &mut expected {
                    *byte = (*byte as f32 * opacity).round() as u8;
                }
                let max = actual
                    .iter()
                    .zip(&expected)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                assert!(max <= 1, "GPU/CPU color tolerance: {max}");
                for (a, b) in actual
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(expected.as_chunks::<4>().0.iter())
                {
                    assert_eq!(a[3], b[3]);
                }
                assert_eq!(gpu.uploaded_bytes, uploaded + pixels.len());
                previous = Some(texture);
            }
        }
        // Two independent layers share the scratch buffers. The second submission
        // must not overwrite the first texture's inputs/config before its dispatch.
        let mut textures = Vec::new();
        for (revision, rgba) in [(20, [80, 40, 20, 255]), (21, [10, 20, 30, 64])] {
            let input = rgba.repeat(512 * 512);
            let effects = LayerEffects {
                enabled: true,
                ..Default::default()
            };
            let texture = gpu
                .render(
                    &device,
                    &queue,
                    Request {
                        pixels: &input,
                        size: (512, 512),
                        revision,
                        effects: &effects,
                        opacity: 1.,
                        previous: None,
                    },
                )
                .unwrap();
            let mut expected = input;
            crate::apply_layer_effects_cpu(&mut expected, &effects);
            textures.push((texture, expected));
        }
        for (texture, expected) in textures {
            let actual = read(&device, &queue, &texture);
            assert!(actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1));
        }
    }
}
