//! GPU rendering shared across desktop platforms; native view ownership lives in the host.
use lumapaint_core::document::{
    CanvasColor, Document, Point, Selection, SelectionOperation, SelectionShape, Stroke, HEIGHT,
    WIDTH,
};
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
    pub pan_x: f32,
    pub pan_y: f32,
    pub document_width: f32,
    pub document_height: f32,
    pub canvas_color: CanvasColor,
}

impl Viewport {
    pub fn document_point(&self, x: f32, y: f32) -> Point {
        let width = self.width as f32 / self.scale;
        let height = self.height as f32 / self.scale;
        let fit = ((width - 48.0) / self.document_width)
            .min((height - 48.0) / self.document_height)
            .max(0.01);
        Point {
            x: (x - width * 0.5 - self.pan_x) / (fit * self.zoom) + self.document_width * 0.5,
            y: (y - height * 0.5 - self.pan_y) / (fit * self.zoom) + self.document_height * 0.5,
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
            pan_x: 0.0,
            pan_y: 0.0,
            document_width: WIDTH,
            document_height: HEIGHT,
            canvas_color: CanvasColor::White,
        })
    }

    pub fn with_pan(mut self, x: f32, y: f32) -> Result<Self, String> {
        if !x.is_finite() || !y.is_finite() || x.abs() > 8192.0 || y.abs() > 8192.0 {
            return Err("Invalid canvas pan offset".into());
        }
        self.pan_x = x;
        self.pan_y = y;
        Ok(self)
    }

    pub fn with_document(
        mut self,
        width: u32,
        height: u32,
        canvas_color: CanvasColor,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("Invalid document dimensions".into());
        }
        self.document_width = width as f32;
        self.document_height = height as f32;
        self.canvas_color = canvas_color;
        Ok(self)
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    viewport: [f32; 4],
    appearance: [f32; 4],
    document: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Segment {
    ends: [f32; 4],
    color: [f32; 4],
    radius: f32,
    hardness: f32,
    weight: f32,
    padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SelectionGpuRegion {
    bounds: [f32; 4],
    info: [f32; 4],
}

fn create_selection_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Selection regions"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}
fn selection_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    selection: Option<&Selection>,
) -> wgpu::BindGroup {
    let regions: Vec<_> = selection
        .map(|selection| {
            selection
                .regions
                .iter()
                .map(|region| SelectionGpuRegion {
                    bounds: region.bounds,
                    info: [
                        match region.operation {
                            SelectionOperation::Replace => 0.0,
                            SelectionOperation::Add => 1.0,
                            SelectionOperation::Subtract => 2.0,
                            SelectionOperation::Invert => 3.0,
                        },
                        if region.shape == SelectionShape::Ellipse {
                            1.0
                        } else {
                            0.0
                        },
                        0.0,
                        0.0,
                    ],
                })
                .collect()
        })
        .unwrap_or_else(|| {
            vec![SelectionGpuRegion {
                bounds: [0.0; 4],
                info: [4.0, 0.0, 0.0, 0.0],
            }]
        });
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Selection regions"),
        contents: bytemuck::cast_slice(&regions),
        usage: wgpu::BufferUsages::STORAGE,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Selection regions"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

fn linear_color(color: [u8; 3]) -> [f32; 4] {
    let rgb = color.map(|channel| {
        let value = f32::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    [rgb[0], rgb[1], rgb[2], 1.0]
}

// Integrate a radial brush tip along arc length, independent of input event density.
// The carry across polyline vertices is essential: restarting dabs at every
// vertex would reintroduce dark knots. A partial final interval has less weight.
fn segments(stroke: &Stroke, opacity: f32) -> Vec<Segment> {
    let points = smooth_points(&stroke.points, &stroke.pressures);
    dabs_along_path(&points, stroke.brush, opacity)
}

fn dabs_along_path(
    points: &[(Point, f32)],
    brush: lumapaint_core::document::Brush,
    opacity: f32,
) -> Vec<Segment> {
    let base_radius = brush.size * 0.5;
    // Soft tips tolerate wider sampling. Avoid thousands of tiny half-float
    // additions on wide brushes; hard edges keep a one-pixel upper bound.
    let spacing = (base_radius * 0.08).min(1.0 + 3.0 * (1.0 - brush.hardness));
    let color = linear_color(brush.color);
    let mut result = Vec::new();
    let mut emit = |p: Point, pressure: f32, length: f32| {
        let radius = base_radius * pressure.max(0.05);
        result.push(Segment {
            ends: [p.x, p.y, p.x, p.y],
            color,
            radius,
            hardness: brush.hardness,
            // Optical depth per unit arc length; the shader converts it to alpha.
            weight: length / radius * opacity,
            padding: 0.0,
        })
    };
    let mut traversed = 0.0;
    let mut next = spacing * 0.5;
    for pair in points.windows(2) {
        let (a, pressure_a) = pair[0];
        let (b, pressure_b) = pair[1];
        let length = (b.x - a.x).hypot(b.y - a.y);
        if length <= f32::EPSILON {
            continue;
        }
        while next < traversed + length {
            let t = (next - traversed) / length;
            emit(
                Point {
                    x: a.x + (b.x - a.x) * t,
                    y: a.y + (b.y - a.y) * t,
                },
                pressure_a + (pressure_b - pressure_a) * t,
                spacing,
            );
            next += spacing;
        }
        traversed += length;
    }
    if let Some((last, pressure)) = points.last() {
        if traversed < spacing * 0.5 {
            // A click still produces a round mark.
            emit(*last, *pressure, base_radius);
        } else {
            // Correct the last quadrature cell instead of adding a full end dab.
            let covered = next - spacing * 0.5;
            let remainder = traversed - covered;
            if remainder > 0.0 {
                emit(*last, *pressure, remainder);
            } else if let Some(last_dab) = result.last_mut() {
                last_dab.weight += remainder / base_radius;
            }
        }
    }
    result
}

fn smooth_points(points: &[Point], pressures: &[f32]) -> Vec<(Point, f32)> {
    let pressure_at = |index: usize| pressures.get(index).copied().unwrap_or(1.0);
    if points.len() <= 2 {
        return points
            .iter()
            .enumerate()
            .map(|(index, point)| (*point, pressure_at(index)))
            .collect();
    }

    let mut result = vec![(points[0], pressure_at(0))];
    for index in 0..points.len() - 1 {
        let p0 = points[index.saturating_sub(1)];
        let p1 = points[index];
        let p2 = points[index + 1];
        let p3 = points[(index + 2).min(points.len() - 1)];
        let distance = (p2.x - p1.x).hypot(p2.y - p1.y);
        let subdivisions = (distance / 1.5).ceil().clamp(1.0, 64.0) as usize;
        for step in 1..=subdivisions {
            let t = step as f32 / subdivisions as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            result.push((
                Point {
                    x: 0.5
                        * ((2.0 * p1.x)
                            + (-p0.x + p2.x) * t
                            + (2.0 * p0.x - 5.0 * p1.x + 4.0 * p2.x - p3.x) * t2
                            + (-p0.x + 3.0 * p1.x - 3.0 * p2.x + p3.x) * t3),
                    y: 0.5
                        * ((2.0 * p1.y)
                            + (-p0.y + p2.y) * t
                            + (2.0 * p0.y - 5.0 * p1.y + 4.0 * p2.y - p3.y) * t2
                            + (-p0.y + 3.0 * p1.y - 3.0 * p2.y + p3.y) * t3),
                },
                pressure_at(index) + (pressure_at(index + 1) - pressure_at(index)) * t,
            ));
        }
    }
    result
}

const BRUSH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

fn create_brush_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    let brush_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Round brush"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}",
                include_str!("selection_common.wgsl"),
                include_str!("brush.wgsl")
            )
            .into(),
        ),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Round brush pipeline"), layout: Some(layout),
            vertex: wgpu::VertexState {
                module: &brush_shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Segment>() as u64, step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32, 3 => Float32, 4 => Float32] }],
            },
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &brush_shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: BRUSH_FORMAT, blend: Some(wgpu::BlendState {
color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Max },
alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
}), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview: None, cache: None,
        })
}

fn create_selection_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Selection outline"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}",
                include_str!("selection_common.wgsl"),
                include_str!("selection.wgsl")
            )
            .into(),
        ),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Selection outline"),
        layout: Some(layout),
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
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

fn create_brush_composite(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Brush mask texture layout"),
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
    let brush_composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Brush composite layout"),
        bind_group_layouts: &[&bind_layout],
        push_constant_ranges: &[],
    });
    let brush_composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Brush mask composite"),
        source: wgpu::ShaderSource::Wgsl(include_str!("brush_composite.wgsl").into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Brush composite pipeline"),
        layout: Some(&brush_composite_layout),
        vertex: wgpu::VertexState {
            module: &brush_composite_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &brush_composite_shader,
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
    (bind_layout, pipeline)
}

pub mod vector;

fn rasterize_svg(source: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
    vector::rasterize_svg(source, width, height).map(|raster| raster.pixels)
}

pub fn validate_svg(source: &str) -> Result<(), String> {
    rasterize_svg(source, WIDTH as u32, HEIGHT as u32).map(|_| ())
}

/// The host must keep the native surface target alive until this renderer is dropped.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    selection_pipeline: wgpu::RenderPipeline,
    selection_layout: wgpu::BindGroupLayout,
    brush_pipeline: wgpu::RenderPipeline,
    brush_composite_pipeline: wgpu::RenderPipeline,
    brush_bind_layout: wgpu::BindGroupLayout,
    brush_sampler: wgpu::Sampler,
    svg_pipeline: wgpu::RenderPipeline,
    svg_bind_layout: wgpu::BindGroupLayout,
    svg_sampler: wgpu::Sampler,
    svg_cache: HashMap<String, CachedSvg>,
    uniform_layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    device_error: Arc<Mutex<Option<String>>>,
    pub adapter_name: String,
    pub backend: String,
}

fn create_svg_pipeline(
    device: &wgpu::Device,
    bind_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
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
        bind_group_layouts: &[bind_layout, &svg_bind_layout],
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
    (svg_bind_layout, svg_pipeline)
}

struct CachedSvg {
    fully_contained: bool,
    source: String,
    opacity: f32,
    size: (u32, u32),
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
        let selection_layout = create_selection_layout(&device);
        let paint_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Selected paint layout"),
            bind_group_layouts: &[&bind_layout, &selection_layout],
            push_constant_ranges: &[],
        });
        let brush_pipeline = create_brush_pipeline(&device, &paint_layout);
        let selection_pipeline = create_selection_pipeline(&device, &paint_layout, format);
        let (brush_bind_layout, brush_composite_pipeline) = create_brush_composite(&device, format);
        let brush_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let (svg_bind_layout, svg_pipeline) = create_svg_pipeline(&device, &bind_layout, format);
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
            selection_pipeline,
            selection_layout,
            brush_pipeline,
            brush_composite_pipeline,
            brush_bind_layout,
            brush_sampler,
            svg_pipeline,
            svg_bind_layout,
            svg_sampler,
            svg_cache: HashMap::new(),
            uniform_layout: bind_layout,
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
            appearance: [
                if viewport.dark { 1.0 } else { 0.0 },
                viewport.pan_x,
                viewport.pan_y,
                if viewport.canvas_color == CanvasColor::Transparent {
                    1.0
                } else {
                    0.0
                },
            ],
            document: [viewport.document_width, viewport.document_height, 0.0, 0.0],
        }
    }

    pub fn render(&mut self, viewport: Viewport, document: &Document) -> Result<(), String> {
        self.render_vector_drag(viewport, document, [0.0, 0.0])
    }

    /// Transient drag offset in document pixels; no document edit or text shaping per frame
    /// is needed when the selected layer's full content already fits in its cached texture.
    pub fn render_vector_drag(
        &mut self,
        viewport: Viewport,
        document: &Document,
        offset: [f32; 2],
    ) -> Result<(), String> {
        if offset
            .iter()
            .any(|v| !v.is_finite() || v.abs() >= 100_000.0)
        {
            return Err("Invalid vector drag offset".into());
        }
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
        let uniforms = Self::uniforms(viewport);
        let vector_overlays = document.vector_overlay_selections_at_offset(offset[0], offset[1]);
        let outline_selections = document
            .selection()
            .into_iter()
            .chain(vector_overlays.iter())
            .map(|selection| {
                selection_bind_group(&self.device, &self.selection_layout, Some(selection))
            })
            .collect::<Vec<_>>();
        let stroke_selections: Vec<_> = document
            .visible_strokes()
            .map(|stroke| {
                selection_bind_group(
                    &self.device,
                    &self.selection_layout,
                    stroke.selection.as_ref(),
                )
            })
            .collect();
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        let view = frame.texture.create_view(&Default::default());
        let stroke_segments: Vec<_> = document
            .visible_strokes()
            .map(|stroke| segments(stroke, document.paint_layer_opacity()))
            .collect();
        self.svg_cache
            .retain(|id, _| document.svg_layers().any(|layer| &layer.id == id));
        let (width, height) = document.dimensions();
        let mut translated_layers = std::collections::HashSet::new();
        for original in document.visible_svg_layers() {
            let dragging = offset != [0.0, 0.0];
            if dragging
                && document.vector_layer_moves_as_unit(original)
                && self.svg_cache.get(&original.id).is_some_and(|cached| {
                    cached.fully_contained
                        && cached.source == original.source
                        && cached.opacity == original.effective_opacity()
                        && cached.size == (width, height)
                })
            {
                translated_layers.insert(original.id.as_str());
                continue;
            }
            let preview = if dragging {
                document.translated_vector_layer(original, offset[0], offset[1])?
            } else {
                None
            };
            let layer = preview.as_ref().unwrap_or(original);
            if self.svg_cache.get(&layer.id).is_some_and(|cached| {
                cached.source == layer.source
                    && cached.opacity == layer.effective_opacity()
                    && cached.size == (width, height)
            }) {
                continue;
            }
            let raster = vector::rasterize_svg(&layer.source, width, height)?;
            let mut pixels = raster.pixels;
            let effective_opacity = layer.effective_opacity();
            if effective_opacity != 1.0 {
                for value in &mut pixels {
                    *value = (f32::from(*value) * effective_opacity).round() as u8;
                }
            }
            let texture = self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("SVG layer texture"),
                    size: wgpu::Extent3d {
                        width,
                        height,
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
                    fully_contained: raster.fully_contained,
                    source: layer.source.clone(),
                    opacity: effective_opacity,
                    size: (width, height),
                    _texture: texture,
                    bind_group,
                },
            );
        }
        let translated_uniforms = if translated_layers.is_empty() {
            None
        } else {
            let mut moved = uniforms;
            moved.document[2] = offset[0];
            moved.document[3] = offset[1];
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Vector drag offset"),
                    contents: bytemuck::bytes_of(&moved),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Vector drag uniforms"),
                layout: &self.uniform_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            }))
        };
        let brush_buffers: Vec<_> = stroke_segments
            .iter()
            .map(|segments| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Brush dabs"),
                        contents: bytemuck::cast_slice(segments),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            })
            .collect();
        let brush_mask = (!brush_buffers.is_empty()).then(|| {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Brush paint mask"),
                size: wgpu::Extent3d {
                    width: viewport.width,
                    height: viewport.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: BRUSH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let mask_view = texture.create_view(&Default::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Brush paint mask"),
                layout: &self.brush_bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&mask_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.brush_sampler),
                    },
                ],
            });
            (texture, mask_view, bind_group)
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
        }
        if let Some((_mask, mask_view, mask_bind_group)) = &brush_mask {
            for ((buffer, segments), selection_group) in brush_buffers
                .iter()
                .zip(stroke_segments.iter())
                .zip(&stroke_selections)
            {
                let mut mask_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Brush paint pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: mask_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                mask_pass.set_pipeline(&self.brush_pipeline);
                mask_pass.set_bind_group(0, &self.bind_group, &[]);
                mask_pass.set_bind_group(1, selection_group, &[]);
                mask_pass.set_vertex_buffer(0, buffer.slice(..));
                mask_pass.draw(0..6, 0..segments.len() as u32);
                drop(mask_pass);

                let mut composite_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Stroke composite pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                composite_pass.set_pipeline(&self.brush_composite_pipeline);
                composite_pass.set_bind_group(0, mask_bind_group, &[]);
                composite_pass.draw(0..6, 0..1);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SVG layer render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            for layer in document.visible_svg_layers() {
                let bind_group = &self.svg_cache[&layer.id].bind_group;
                pass.set_pipeline(&self.svg_pipeline);
                let uniforms = if translated_layers.contains(layer.id.as_str()) {
                    translated_uniforms.as_ref().unwrap_or(&self.bind_group)
                } else {
                    &self.bind_group
                };
                pass.set_bind_group(0, uniforms, &[]);
                pass.set_bind_group(1, bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
            for selection_group in &outline_selections {
                pass.set_pipeline(&self.selection_pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_bind_group(1, selection_group, &[]);
                pass.draw(0..3, 0..1);
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
    fn brush_curve_interpolation_preserves_endpoints_and_adds_smooth_samples() {
        let points = [
            Point { x: 0.0, y: 10.0 },
            Point { x: 10.0, y: 0.0 },
            Point { x: 20.0, y: 10.0 },
        ];
        let smoothed = smooth_points(&points, &[0.25, 0.5, 1.0]);
        assert!(smoothed.len() > points.len());
        assert_eq!((smoothed[0].0.x, smoothed[0].0.y), (0.0, 10.0));
        assert_eq!(smoothed[0].1, 0.25);
        let last = smoothed.last().unwrap();
        assert!((last.0.x - 20.0).abs() < 0.001);
        assert!((last.0.y - 10.0).abs() < 0.001);
        assert!((last.1 - 1.0).abs() < 0.001);
        assert!(smoothed.iter().any(|point| point.0.y < 1.0));
    }

    #[test]
    fn pressure_changes_brush_radius_along_the_stroke() {
        let stroke = Stroke {
            brush: lumapaint_core::document::Brush {
                size: 40.0,
                ..Default::default()
            },
            points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 110.0, y: 10.0 }],
            pressures: vec![0.25, 1.0],
            selection: None,
        };
        let dabs = segments(&stroke, 1.0);
        assert!(dabs.first().unwrap().radius < 6.0);
        assert!(dabs.last().unwrap().radius > 19.0);
    }

    #[test]
    fn document_center_is_stable_at_retina_and_zoom() {
        let viewport = Viewport::new(800.0, 500.0, 2.0, 2.0, true).unwrap();
        let point = viewport.document_point(400.0, 250.0);
        assert_eq!((point.x, point.y), (480.0, 320.0));
    }

    #[test]
    fn pan_offset_moves_document_and_pointer_mapping_together() {
        let viewport = Viewport::new(800.0, 500.0, 2.0, 2.0, true)
            .unwrap()
            .with_pan(40.0, -20.0)
            .unwrap();
        let point = viewport.document_point(440.0, 230.0);
        assert!((point.x - WIDTH * 0.5).abs() < 0.001);
        assert!((point.y - HEIGHT * 0.5).abs() < 0.001);
    }

    #[test]
    fn custom_document_size_controls_pointer_mapping() {
        let viewport = Viewport::new(800.0, 500.0, 1.0, 1.0, false)
            .unwrap()
            .with_document(434, 418, CanvasColor::White)
            .unwrap();
        let point = viewport.document_point(400.0, 250.0);
        assert_eq!((point.x, point.y), (217.0, 209.0));
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
        let pixels = rasterize_svg(svg, WIDTH as u32, HEIGHT as u32).unwrap();
        assert_eq!(pixels.len(), WIDTH as usize * HEIGHT as usize * 4);
        assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
        assert!(validate_svg("not svg").is_err());
    }
}

#[cfg(test)]
mod gpu_tests;
