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
    #[cfg(feature = "skia")]
    pub region: std::cell::Cell<Option<[u32; 4]>>,
    #[cfg(feature = "skia")]
    viewport: Option<[u32; 9]>,
    #[cfg(feature = "skia")]
    objects: Vec<(String, [u32; 4])>,
}
pub(super) struct Pipeline {
    pub layout: wgpu::BindGroupLayout,
    pub draw: wgpu::RenderPipeline,
    #[cfg(feature = "skia")]
    pub clear: wgpu::RenderPipeline,
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
        #[cfg(feature = "skia")]
        let clear_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Clear native damaged region"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex fn vs_main(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> {
 let p=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.))[i];return vec4(p,0.,1.);
}
@fragment fn fs_main()->@location(0) vec4<f32>{return vec4(0.);}
"#
                .into(),
            ),
        });
        #[cfg(feature = "skia")]
        let clear = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Replace damaged pixels with transparent"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &clear_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &clear_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        Self {
            layout,
            draw,
            #[cfg(feature = "skia")]
            clear,
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
            #[cfg(feature = "skia")]
            region: std::cell::Cell::new(None),
            #[cfg(feature = "skia")]
            viewport: None,
            #[cfg(feature = "skia")]
            objects: Vec::new(),
        }
    }
}

// Coordinates are device pixels, including the geometry's AA fringe.
#[cfg(feature = "skia")]
pub(super) fn intersects(a: [u32; 4], b: [u32; 4]) -> bool {
    a[2] > 0
        && a[3] > 0
        && b[2] > 0
        && b[3] > 0
        && a[0] < b[0] + b[2]
        && b[0] < a[0] + a[2]
        && a[1] < b[1] + b[3]
        && b[1] < a[1] + a[3]
}
#[cfg(feature = "skia")]
impl Composite {
    pub fn damage<'a>(
        &mut self,
        viewport: [u32; 9],
        objects: impl Iterator<Item = (&'a str, [u32; 4], bool)>,
    ) {
        let mut full = self.dirty.get() || self.viewport != Some(viewport);
        let mut region: Option<[u32; 4]> = None;
        let mut count = 0;
        for (i, (id, rect, changed)) in objects.enumerate() {
            count = i + 1;
            if let Some((old_id, old_rect)) = self.objects.get_mut(i) {
                if old_id != id {
                    full = true;
                    old_id.clear();
                    old_id.push_str(id);
                }
                if changed || *old_rect != rect {
                    for r in [*old_rect, rect] {
                        if r[2] == 0 || r[3] == 0 {
                            continue;
                        }
                        region = Some(match region {
                            None => r,
                            Some(a) => {
                                let x = a[0].min(r[0]);
                                let y = a[1].min(r[1]);
                                [
                                    x,
                                    y,
                                    (a[0] + a[2]).max(r[0] + r[2]) - x,
                                    (a[1] + a[3]).max(r[1] + r[3]) - y,
                                ]
                            }
                        });
                    }
                }
                *old_rect = rect;
            } else {
                full = true;
                self.objects.push((id.into(), rect));
            }
        }
        full |= self.objects.len() != count;
        self.objects.truncate(count);
        self.viewport = Some(viewport);
        if let Some(r) = region {
            full |= u64::from(r[2]) * u64::from(r[3]) * 2
                >= u64::from(self.size[0]) * u64::from(self.size[1]);
        }
        self.region.set(if full { None } else { region });
        self.dirty.set(full || region.is_some());
    }
}
