//! Isolate a vector layer in encoded sRGB before applying its opacity once.
use crate::create_layer_pipeline_with_fragment;
pub(super) struct Composite {
    pub _texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub binding: wgpu::BindGroup,
    pub uniform: wgpu::Buffer,
    pub opacity: f32,
    pub size: [u32; 2],
    pub dirty: std::cell::Cell<bool>,
}
pub(super) struct Pipeline {
    pub layout: wgpu::BindGroupLayout,
    pub draw: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}
impl Pipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Native isolated layer"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let source = r#"
@group(0) @binding(0) var<uniform> opacity:vec4<f32>;
@group(0) @binding(1) var image:texture_2d<f32>;
@group(0) @binding(2) var smp:sampler;
struct Out{@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>}
@vertex fn vs_main(@builtin(vertex_index) i:u32)->Out {
 let p=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.))[i];
 var o:Out;o.position=vec4(p.x*2.-1.,1.-p.y*2.,0.,1.);o.uv=p;return o;
}
fn linear(c:vec3<f32>)->vec3<f32>{return select(c/12.92,pow((c+.055)/1.055,vec3(2.4)),c>vec3(.04045));}
@fragment fn fs_main(o:Out)->@location(0) vec4<f32>{let p=textureSample(image,smp,o.uv)*opacity.x;return vec4(linear(p.rgb),p.a);}
"#;
        let draw = create_layer_pipeline_with_fragment(
            device,
            &[&layout],
            format,
            source,
            "Native encoded layer composition",
            "fs_main",
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        Self {
            layout,
            draw,
            sampler,
        }
    }
    pub fn target(&self, device: &wgpu::Device, size: [u32; 2]) -> Composite {
        let texture = crate::gpu_metrics::create_texture!(
            device,
            &wgpu::TextureDescriptor {
                label: Some("Native encoded layer target"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[]
            }
        );
        let view = texture.create_view(&Default::default());
        let uniform = crate::gpu_metrics::create_buffer!(
            device,
            &wgpu::BufferDescriptor {
                label: Some("Native layer opacity"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false
            }
        );
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Native encoded composite binding"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        Composite {
            _texture: texture,
            view,
            binding,
            uniform,
            opacity: f32::NAN,
            size,
            dirty: std::cell::Cell::new(true),
        }
    }
}
