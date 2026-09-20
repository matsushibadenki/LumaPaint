//! GPU rendering shared across desktop platforms; native view ownership lives in the host.
use lumapaint_core::document::{Document, Point, HEIGHT, WIDTH};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
pub use wgpu;
use wgpu::util::DeviceExt;

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub zoom: f32,
    pub dark: bool,
}

impl Viewport {
    pub fn document_point(&self, x: f32, y: f32) -> Point {
        let width = self.width as f32 / self.scale;
        let height = self.height as f32 / self.scale;
        let fit = ((width - 48.0) / WIDTH)
            .min((height - 48.0) / HEIGHT)
            .max(0.01);
        Point {
            x: (x - width * 0.5) / (fit * self.zoom) + WIDTH * 0.5,
            y: (y - height * 0.5) / (fit * self.zoom) + HEIGHT * 0.5,
        }
    }
    pub fn new(width: f64, height: f64, scale: f64, zoom: f64, dark: bool) -> Result<Self, String> {
        if ![width, height, scale, zoom].iter().all(|v| v.is_finite())
            || width <= 0.0
            || height <= 0.0
            || !(0.5..=8.0).contains(&scale)
            || !(0.25..=4.0).contains(&zoom)
        {
            return Err("Invalid viewport dimensions, scale, or zoom".into());
        }
        let width = (width * scale).round().max(1.0);
        let height = (height * scale).round().max(1.0);
        if width > 8192.0 || height > 8192.0 {
            return Err("Viewport exceeds the 8192-pixel preview limit".into());
        }
        Ok(Self {
            width: width as u32,
            height: height as u32,
            scale: scale as f32,
            zoom: zoom as f32,
            dark,
        })
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    viewport: [f32; 4],
    appearance: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Segment {
    ends: [f32; 4],
    color: [f32; 4],
    radius: f32,
    padding: [f32; 3],
}

fn segments(document: &Document) -> Vec<Segment> {
    let mut result = Vec::new();
    for stroke in document.visible_strokes() {
        let rgb = stroke.brush.color.map(|channel| {
            let value = f32::from(channel) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        });
        for (index, point) in stroke.points.iter().enumerate() {
            let previous = stroke.points[index.saturating_sub(1)];
            result.push(Segment {
                ends: [previous.x, previous.y, point.x, point.y],
                color: [rgb[0], rgb[1], rgb[2], 1.0],
                radius: stroke.brush.size * 0.5,
                padding: [0.0; 3],
            });
        }
    }
    result
}

fn rasterize_svg(source: &str) -> Result<Vec<u8>, String> {
    let options = resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: resvg::usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(source, &options).map_err(|error| error.to_string())?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(WIDTH as u32, HEIGHT as u32)
        .ok_or("SVG raster allocation failed")?;
    let size = tree.size();
    let scale = (WIDTH / size.width()).min(HEIGHT / size.height());
    let transform = resvg::tiny_skia::Transform::from_row(
        scale,
        0.0,
        0.0,
        scale,
        (WIDTH - size.width() * scale) * 0.5,
        (HEIGHT - size.height() * scale) * 0.5,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(pixmap.take())
}

pub fn validate_svg(source: &str) -> Result<(), String> {
    rasterize_svg(source).map(|_| ())
}

/// The host must keep the native surface target alive until this renderer is dropped.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    brush_pipeline: wgpu::RenderPipeline,
    svg_pipeline: wgpu::RenderPipeline,
    svg_bind_layout: wgpu::BindGroupLayout,
    svg_sampler: wgpu::Sampler,
    svg_cache: HashMap<String, CachedSvg>,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    device_error: Arc<Mutex<Option<String>>>,
    pub adapter_name: String,
    pub backend: String,
}

struct CachedSvg {
    source: String,
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

impl Renderer {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        viewport: Viewport,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("LumaPaint preview device"),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let capabilities = surface.get_capabilities(&adapter);
        let device_error = Arc::new(Mutex::new(None));
        let errors = Arc::clone(&device_error);
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            if let Ok(mut slot) = errors.lock() {
                *slot = Some(error.to_string());
            }
        }));
        let errors = Arc::clone(&device_error);
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut slot) = errors.lock() {
                *slot = Some(format!("GPU device lost ({reason:?}): {message}"));
            }
        });
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .ok_or("No sRGB surface format available")?;
        let mut config = surface
            .get_default_config(&adapter, viewport.width, viewport.height)
            .ok_or("No compatible surface configuration")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::Fifo;
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shared viewport layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Canvas layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU preview pattern"),
            source: wgpu::ShaderSource::Wgsl(include_str!("preview.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("LumaPaint preview pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let brush_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Round brush"),
            source: wgpu::ShaderSource::Wgsl(include_str!("brush.wgsl").into()),
        });
        let brush_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Round brush pipeline"), layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &brush_shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Segment>() as u64, step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32] }],
            },
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &brush_shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview: None, cache: None,
        });
        let svg_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("SVG texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let svg_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SVG layer layout"),
            bind_group_layouts: &[&bind_layout, &svg_bind_layout],
            push_constant_ranges: &[],
        });
        let svg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SVG layer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("svg.wgsl").into()),
        });
        let svg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("SVG layer pipeline"),
            layout: Some(&svg_layout),
            vertex: wgpu::VertexState {
                module: &svg_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &svg_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let svg_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        if let Some(error) = device.pop_error_scope().await {
            return Err(error.to_string());
        }
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Preview viewport"),
            contents: bytemuck::bytes_of(&Self::uniforms(viewport)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Preview viewport"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        surface.configure(&device, &config);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            brush_pipeline,
            svg_pipeline,
            svg_bind_layout,
            svg_sampler,
            svg_cache: HashMap::new(),
            uniforms,
            bind_group,
            device_error,
            adapter_name: info.name,
            backend: format!("{:?}", info.backend),
        })
    }

    fn uniforms(viewport: Viewport) -> Uniforms {
        Uniforms {
            viewport: [
                viewport.width as f32,
                viewport.height as f32,
                viewport.scale,
                viewport.zoom,
            ],
            appearance: [if viewport.dark { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        }
    }

    pub fn render(&mut self, viewport: Viewport, document: &Document) -> Result<(), String> {
        if let Some(error) = self
            .device_error
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
        {
            return Err(error.clone());
        }
        if viewport.width > self.device.limits().max_texture_dimension_2d
            || viewport.height > self.device.limits().max_texture_dimension_2d
        {
            return Err("Viewport exceeds GPU texture limits".into());
        }
        if self.config.width != viewport.width || self.config.height != viewport.height {
            self.config.width = viewport.width;
            self.config.height = viewport.height;
            self.surface.configure(&self.device, &self.config);
        }
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                self.surface
                    .get_current_texture()
                    .map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        };
        self.queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Self::uniforms(viewport)),
        );
        let view = frame.texture.create_view(&Default::default());
        let segments = segments(document);
        self.svg_cache
            .retain(|id, _| document.svg_layers().any(|layer| &layer.id == id));
        for layer in document.visible_svg_layers() {
            if self
                .svg_cache
                .get(&layer.id)
                .is_some_and(|cached| cached.source == layer.source)
            {
                continue;
            }
            let pixels = rasterize_svg(&layer.source)?;
            let texture = self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("SVG layer texture"),
                    size: wgpu::Extent3d {
                        width: WIDTH as u32,
                        height: HEIGHT as u32,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &pixels,
            );
            let view = texture.create_view(&Default::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("SVG layer"),
                layout: &self.svg_bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.svg_sampler),
                    },
                ],
            });
            self.svg_cache.insert(
                layer.id.clone(),
                CachedSvg {
                    source: layer.source.clone(),
                    _texture: texture,
                    bind_group,
                },
            );
        }
        let brush_buffer = (!segments.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Stroke segments"),
                    contents: bytemuck::cast_slice(&segments),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Preview render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
            if let Some(buffer) = &brush_buffer {
                pass.set_pipeline(&self.brush_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..segments.len() as u32);
            }
            for layer in document.visible_svg_layers() {
                let bind_group = &self.svg_cache[&layer.id].bind_group;
                pass.set_pipeline(&self.svg_pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_bind_group(1, bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_center_is_stable_at_retina_and_zoom() {
        let viewport = Viewport::new(800.0, 500.0, 2.0, 2.0, true).unwrap();
        let point = viewport.document_point(400.0, 250.0);
        assert_eq!((point.x, point.y), (480.0, 320.0));
    }

    #[test]
    fn retina_dimensions_preserve_logical_size() {
        let viewport = Viewport::new(641.5, 300.0, 2.0, 1.0, false).unwrap();
        assert_eq!((viewport.width, viewport.height), (1283, 600));
        assert_eq!(viewport.scale, 2.0);
    }

    #[test]
    fn invalid_and_oversized_viewports_are_rejected_before_allocation() {
        for width in [0.0, -1.0, f64::NAN, f64::INFINITY, 8193.0] {
            assert!(Viewport::new(width, 300.0, 1.0, 1.0, false).is_err());
        }
        assert!(Viewport::new(5000.0, 300.0, 2.0, 1.0, false).is_err());
        assert!(Viewport::new(300.0, 300.0, 0.0, 1.0, false).is_err());
        assert!(Viewport::new(300.0, 300.0, 1.0, 0.0, false).is_err());
    }

    #[test]
    fn svg_validation_rasterizes_vector_content() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><circle cx="20" cy="10" r="8" fill="#538fd2"/></svg>"##;
        let pixels = rasterize_svg(svg).unwrap();
        assert_eq!(pixels.len(), WIDTH as usize * HEIGHT as usize * 4);
        assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
        assert!(validate_svg("not svg").is_err());
    }
}
