//! Retained GPU textures + perspective-correct textured 3D triangles.
use crate::{native_composite, Viewport};
use lumapaint_core::document::SvgLayer;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use wgpu::util::DeviceExt;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 4],
    uv: [f32; 2],
    light: f32,
    opacity: f32,
    color: [f32; 4],
}
struct Image {
    _lease: crate::gpu_metrics::CachedTextureLease,
    _texture: wgpu::Texture,
    binding: wgpu::BindGroup,
}
struct TextureEntry {
    source: String,
    size: [u32; 2],
    image: Arc<Image>,
    bytes: u64,
}
struct Batch {
    buffer: wgpu::Buffer,
    count: u32,
    image: Arc<Image>,
}
struct Layer {
    _target_lease: crate::gpu_metrics::CachedTextureLease,
    target: native_composite::Composite,
    _depth_lease: crate::gpu_metrics::CachedTextureLease,
    _color_lease: crate::gpu_metrics::CachedTextureLease,
    _depth: wgpu::Texture,
    _color: wgpu::Texture,
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
    batches: Vec<Batch>,
}
pub(crate) struct Display {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    composite: native_composite::Pipeline,
    layers: HashMap<String, Layer>,
    textures: Vec<TextureEntry>,
    ready: HashSet<String>,
}
impl Display {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Motion mesh texture"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Motion mesh"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("Motion mesh perspective"),source:wgpu::ShaderSource::Wgsl(r#"
@group(0) @binding(0) var image:texture_2d<f32>;
@group(0) @binding(1) var smp:sampler;
struct Out {@builtin(position) position:vec4<f32>,@location(0) uv:vec2<f32>,@location(1) light:f32,@location(2) opacity:f32,@location(3) color:vec4<f32>}
@vertex fn vs_main(@location(0) position:vec4<f32>,@location(1) uv:vec2<f32>,@location(2) light:f32,@location(3) opacity:f32,@location(4) color:vec4<f32>)->Out {var o:Out;o.position=position;o.uv=uv;o.light=light;o.opacity=opacity;o.color=color;return o;}
@fragment fn fs_main(o:Out)->@location(0) vec4<f32>{let sampled=textureSample(image,smp,o.uv);let color=select(sampled,o.color,o.color.a>=0.);if(color.a*o.opacity<.001){discard;}return vec4(color.rgb*o.light,color.a)*o.opacity;}
"#.into())});
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Motion mesh depth-tested triangles"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x2,2=>Float32,3=>Float32,4=>Float32x4],
                })],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState{count:4,..Default::default()},
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            layout,
            pipeline,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            composite: native_composite::Pipeline::new(device, format),
            layers: HashMap::new(),
            textures: Vec::new(),
            ready: HashSet::new(),
        }
    }
    pub fn begin_frame(&mut self, visible: &HashSet<String>) {
        self.ready.clear();
        self.layers.retain(|id, _| visible.contains(id));
        if visible.is_empty() {
            self.textures.clear();
        }
    }
    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        source: &str,
        size: [u32; 2],
    ) -> Option<Arc<Image>> {
        if let Some(i) = self
            .textures
            .iter()
            .position(|e| e.source == source && e.size == size)
        {
            let entry = self.textures.remove(i);
            let image = entry.image.clone();
            self.textures.push(entry);
            crate::performance::count("motion_mesh_texture_hits", 1);
            return Some(image);
        }
        let bytes = size[0] as u64 * size[1] as u64 * 4;
        if bytes > 16 * 1024 * 1024 || source.len() > 1024 * 1024 {
            return None;
        }
        let pixels = crate::vector::rasterize_svg(source, size[0], size[1])
            .ok()?
            .pixels;
        let texture = crate::gpu_metrics::create_texture!(
            device,
            &wgpu::TextureDescriptor {
                label: Some("Retained motion surface"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[]
            }
        );
        crate::gpu_metrics::write_texture!(
            queue,
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: Some(size[1])
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1
            }
        );
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Motion mesh texture"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let image = Arc::new(Image {
            _lease: crate::gpu_metrics::CachedTextureLease::new(&texture),
            _texture: texture,
            binding,
        });
        while self.textures.len() >= 64
            || self.textures.iter().map(|e| e.bytes).sum::<u64>() + bytes > 64 * 1024 * 1024
        {
            self.textures.remove(0);
        }
        self.textures.push(TextureEntry {
            source: source.into(),
            size,
            image: image.clone(),
            bytes,
        });
        crate::performance::count("motion_mesh_texture_uploads", 1);
        Some(image)
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &SvgLayer,
        v: Viewport,
    ) -> bool {
        let Some(scene) = &layer.motion_scene else {
            return false;
        };
        if scene.surfaces.len() > 64 || v.width as u64 * v.height as u64 * 36 > 128 * 1024 * 1024 {
            return false;
        }
        let existing = self.layers.get(&layer.id).map_or(0, |l| {
            l.target.size[0] as u64 * l.target.size[1] as u64 * 36
        });
        let total = self
            .layers
            .values()
            .map(|l| l.target.size[0] as u64 * l.target.size[1] as u64 * 36)
            .sum::<u64>();
        if total - existing + v.width as u64 * v.height as u64 * 36 > 128 * 1024 * 1024 {
            return false;
        }
        let scale = v.screen_zoom() * v.scale;
        let left = (v.width as f32 - v.document_width * scale) * 0.5 + v.pan_x * v.scale;
        let top = (v.height as f32 - v.document_height * scale) * 0.5 + v.pan_y * v.scale;
        let mut batches = Vec::new();
        for surface in &scene.surfaces {
            let [width, height] = surface.size;
            let size = [
                (width * scale * 2.).ceil().clamp(16., 2048.) as u32,
                (height * scale * 2.).ceil().clamp(16., 2048.) as u32,
            ];
            let Some(image) = self.texture(device, queue, &surface.source, size) else {
                return false;
            };
            let data = surface
                .triangles
                .iter()
                .flat_map(|triangle| triangle.iter())
                .map(|vertex| {
                    let x = (left + vertex.point[0] * scale) / v.width as f32 * 2. - 1.;
                    let y = 1. - (top + vertex.point[1] * scale) / v.height as f32 * 2.;
                    let w = vertex.distance;
                    Vertex {
                        position: [x * w, y * w, vertex.depth * w, w],
                        uv: vertex.uv,
                        light: vertex.light,
                        opacity: surface.opacity,
                        color: vertex.color.unwrap_or([-1.; 4]),
                    }
                })
                .collect::<Vec<_>>();
            if data.is_empty() {
                continue;
            }
            let buffer = crate::gpu_metrics::create_buffer_init!(
                device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("Motion mesh vertices"),
                    contents: bytemuck::cast_slice(&data),
                    usage: wgpu::BufferUsages::VERTEX,
                }
            );
            crate::performance::count(
                "motion_mesh_vertex_upload_bytes",
                (data.len() * std::mem::size_of::<Vertex>()) as u64,
            );
            batches.push(Batch {
                buffer,
                count: data.len() as u32,
                image,
            });
        }
        if !self
            .layers
            .get(&layer.id)
            .is_some_and(|l| l.target.size == [v.width, v.height])
        {
            let target = self.composite.target(device, [v.width, v.height]);
            let depth = crate::gpu_metrics::create_texture!(
                device,
                &wgpu::TextureDescriptor {
                    label: Some("Motion mesh depth"),
                    size: wgpu::Extent3d {
                        width: v.width,
                        height: v.height,
                        depth_or_array_layers: 1
                    },
                    mip_level_count: 1,
                    sample_count: 4,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Depth32Float,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[]
                }
            );
            let view = depth.create_view(&Default::default());
            let color = crate::gpu_metrics::create_texture!(
                device,
                &wgpu::TextureDescriptor {
                    label: Some("Motion mesh MSAA"),
                    size: wgpu::Extent3d {
                        width: v.width,
                        height: v.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 4,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                }
            );
            let color_view = color.create_view(&Default::default());
            self.layers.insert(
                layer.id.clone(),
                Layer {
                    _target_lease: crate::gpu_metrics::CachedTextureLease::new(&target._texture),
                    target,
                    _depth_lease: crate::gpu_metrics::CachedTextureLease::new(&depth),
                    _color_lease: crate::gpu_metrics::CachedTextureLease::new(&color),
                    _depth: depth,
                    _color: color,
                    color: color_view,
                    depth: view,
                    batches: Vec::new(),
                },
            );
        }
        let prepared = self.layers.get_mut(&layer.id).unwrap();
        prepared.batches = batches;
        prepared.target.opacity = layer.effective_opacity();
        crate::gpu_metrics::write_buffer!(
            queue,
            &prepared.target.uniform,
            0,
            bytemuck::cast_slice(&[prepared.target.opacity, 0., 0., 0.]),
        );
        prepared.target.dirty.set(true);
        self.ready.insert(layer.id.clone());
        true
    }
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        for id in &self.ready {
            let layer = &self.layers[id];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Motion mesh GPU projection"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &layer.color,
                    depth_slice: None,
                    resolve_target: Some(&layer.target.view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &layer.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            for batch in &layer.batches {
                pass.set_bind_group(0, &batch.image.binding, &[]);
                pass.set_vertex_buffer(0, batch.buffer.slice(..));
                pass.draw(0..batch.count, 0..1);
            }
            crate::performance::count("motion_mesh_gpu_frames", 1);
        }
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, id: &str) {
        pass.set_pipeline(&self.composite.draw);
        pass.set_bind_group(0, &self.layers[id].target.binding, &[]);
        pass.draw(0..6, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::motion::{self, mesh, Clip};
    fn gpu() -> (wgpu::Device, wgpu::Queue) {
        pollster::block_on(async {
            let adapter = wgpu::Instance::default()
                .request_adapter(&Default::default())
                .await
                .expect("real GPU");
            eprintln!("motion mesh adapter: {:?}", adapter.get_info());
            adapter.request_device(&Default::default()).await.unwrap()
        })
    }
    fn pixels(display: &Display, device: &wgpu::Device, queue: &wgpu::Queue, id: &str) -> Vec<u8> {
        let size = wgpu::Extent3d {
            width: 128,
            height: 128,
            depth_or_array_layers: 1,
        };
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Mesh test output"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 128 * 128 * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        display.encode(&mut encoder);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            display.draw(&mut pass, id);
        }
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(512),
                    rows_per_image: Some(128),
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = read.slice(..).get_mapped_range().unwrap().to_vec();
        read.unmap();
        bytes
    }
    fn viewport() -> Viewport {
        Viewport {
            width: 128,
            height: 128,
            scale: 1.,
            zoom: 1.6,
            pasteboard_color: None,
            dark: false,
            pan_x: 0.,
            pan_y: 0.,
            document_width: 128.,
            document_height: 128.,
            canvas_color: lumapaint_core::document::CanvasColor::Transparent,
        }
    }
    fn layer() -> SvgLayer {
        serde_json::from_value(
            serde_json::json!({"id":"mesh","name":"Mesh","visible":true,"source":"<svg/>"}),
        )
        .unwrap()
    }
    #[test]
    #[ignore = "requires a real GPU"]
    fn gpu_extrusion_preserves_counters_and_shades_visible_sidewalls() {
        let (device, queue) = gpu();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut display = Display::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let mut layer = layer();
        let object:lumapaint_core::vector::VectorObject=serde_json::from_value(serde_json::json!({"id":"ring","name":"Ring","path":{"data":"M24 24H104V104H24Z M44 44V84H84V44Z","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":{"color":[40,120,220,255]},"stroke":null,"strokeWidth":0,"visible":true})).unwrap();
        let mut clip = Clip {
            extruded: true,
            angle: 0.,
            mesh_depth: 20.,
            rotation_x: 0.,
            rotation_y: 0.,
            camera_distance: 300.,
            ..Default::default()
        };
        let source = "<path d='M24 24H104V104H24Z M44 44V84H84V44Z' fill='#2878dc'/>";
        let preset = motion::preset("tx-layered-depth").unwrap();
        let mut render = |clip: &Clip| {
            let surface = mesh::surface_for_object(
                source.into(),
                [24., 24., 104., 104.],
                clip,
                preset,
                0.,
                0,
                1,
                24,
                &object,
            );
            assert!(
                surface
                    .triangles
                    .iter()
                    .flatten()
                    .any(|v| v.color.is_some())
                    || clip.mesh_depth == 0.
            );
            layer.motion_scene = Some(Arc::new(mesh::Scene {
                surfaces: vec![surface],
            }));
            display.begin_frame(&HashSet::from(["mesh".into()]));
            assert!(display.prepare(&device, &queue, &layer, viewport()));
            pixels(&display, &device, &queue, "mesh")
        };
        let front = render(&clip);
        assert_eq!(front[(64 * 128 + 64) * 4 + 3], 0, "hole must remain open");
        clip.rotation_y = 70.;
        let solid = render(&clip);
        clip.mesh_depth = 0.;
        let flat = render(&clip);
        assert_ne!(solid, flat, "edge-on letters must retain visible thickness");
        assert!(
            solid.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count()
                > flat.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count()
        );
        assert!(pollster::block_on(scope.pop()).is_none());
    }
    #[test]
    #[ignore = "requires a real GPU"]
    fn gpu_motion_mesh_reuses_textures_and_depth_tests_transparent_surfaces() {
        let (device, queue) = gpu();
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut display = Display::new(&device, wgpu::TextureFormat::Rgba8Unorm);
        let mut layer = layer();
        let clip = Clip {
            angle: 360.,
            mesh_depth: 40.,
            camera_distance: 300.,
            ..Default::default()
        };
        let preset = motion::preset("3d-cube").unwrap();
        let source = "<rect x='24' y='24' width='80' height='80' fill='#d43b25'/>";
        let mut frames = Vec::new();
        let mut first = None;
        for p in [0., 0.23, 0.51] {
            layer.motion_scene = Some(Arc::new(mesh::Scene {
                surfaces: vec![mesh::surface(
                    source.into(),
                    [24., 24., 104., 104.],
                    &clip,
                    preset,
                    p,
                    0,
                    1,
                    24,
                )],
            }));
            display.begin_frame(&HashSet::from(["mesh".into()]));
            assert!(display.prepare(&device, &queue, &layer, viewport()));
            let image = display.textures[0].image.clone();
            if let Some(first) = &first {
                assert!(Arc::ptr_eq(first, &image));
            } else {
                first = Some(image);
            }
            let raster = pixels(&display, &device, &queue, "mesh");
            assert!(raster.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
            frames.push(raster);
        }
        assert_ne!(frames[0], frames[1]);
        assert_ne!(frames[1], frames[2]);
        assert_eq!(display.textures.len(), 1);
        let make = |color: &str, z: f32, opacity: f32| {
            mesh::Surface{opacity,size:[80.,80.],source:format!("<svg xmlns='http://www.w3.org/2000/svg' width='80' height='80'><rect width='80' height='80' fill='{color}'/></svg>"),triangles:mesh::Mesh::plane(80.,80.).project([0.;3],[0.,0.,z],[64.,64.],300.)}
        };
        for opacity in [1., 0.] {
            layer.motion_scene = Some(Arc::new(mesh::Scene {
                surfaces: vec![make("blue", 60., opacity), make("red", 0., 1.)],
            }));
            display.begin_frame(&HashSet::from(["mesh".into()]));
            assert!(display.prepare(&device, &queue, &layer, viewport()));
            let raster = pixels(&display, &device, &queue, "mesh");
            let pixel = &raster[(64 * 128 + 64) * 4..][..4];
            if opacity == 1. {
                assert!(pixel[2] > pixel[0], "near blue must survive far red");
            } else {
                assert!(
                    pixel[0] > pixel[2],
                    "transparent front must not write depth"
                );
            }
        }
        assert!(pollster::block_on(error_scope.pop()).is_none());
    }
}
