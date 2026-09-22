//! Exercise the production brush shaders on an offscreen GPU texture.
//! Run explicitly with: cargo test -p lumapaint-renderer gpu_brush -- --ignored --nocapture
use super::*;
use lumapaint_core::document::{Brush, SelectionRegion};

const W: u32 = 1024;
const H: u32 = 768;

struct Gpu {
    device: wgpu::Device,
    uniform_layout: wgpu::BindGroupLayout,
    queue: wgpu::Queue,
    brush: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    uniforms: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    selection_layout: wgpu::BindGroupLayout,
    outline: wgpu::RenderPipeline,
    tile_nearest: bool,
}

impl Gpu {
    fn new() -> Self {
        Self::new_with_viewport(1.0, 960.0 / 976.0)
    }

    fn new_with_viewport(scale: f32, zoom: f32) -> Self {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&Default::default())
                .await
                .expect("GPU adapter required");
            eprintln!("GPU regression adapter: {:?}", adapter.get_info());
            let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();
            let uniform_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: None,
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
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                // The default case has fit = 1 and translation = (32,64).
                contents: bytemuck::bytes_of(&Uniforms {
                    viewport: [W as f32, H as f32, scale, zoom],
                    appearance: [0.0; 4],
                    document: [960.0, 640.0, 0.0, 0.0],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let uniforms = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &uniform_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let selection_layout = create_selection_layout(&device);
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&uniform_layout, &selection_layout],
                push_constant_ranges: &[],
            });
            let brush = create_brush_pipeline(&device, &layout);
            let outline =
                create_selection_pipeline(&device, &layout, wgpu::TextureFormat::Rgba8UnormSrgb);
            let (texture_layout, composite) =
                create_brush_composite(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            Self {
                device,
                uniform_layout,
                queue,
                brush,
                composite,
                uniforms,
                texture_layout,
                selection_layout,
                outline,
                tile_nearest: Viewport {
                    width: W,
                    height: H,
                    scale,
                    zoom,
                    dark: false,
                    pan_x: 0.0,
                    pan_y: 0.0,
                    document_width: 960.0,
                    document_height: 640.0,
                    canvas_color: CanvasColor::White,
                }
                .tile_filter_nearest(),
            }
        })
    }

    fn render(&self, strokes: &[Stroke]) -> Vec<u8> {
        self.render_with_outline(strokes, None)
    }

    fn render_with_outline(&self, strokes: &[Stroke], outline: Option<&Selection>) -> Vec<u8> {
        let all: Vec<_> = strokes
            .iter()
            .flat_map(|stroke| segments(stroke, 1.0))
            .collect();
        let lengths: Vec<_> = strokes
            .iter()
            .map(|s| segments(s, 1.0).len() as u32)
            .collect();
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&all),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let size = wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        };
        let texture = |format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let paint = texture(
            BRUSH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let output = texture(
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let paint_view = paint.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let sampler = self.device.create_sampler(&Default::default());
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&paint_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut start = 0;
        for (index, count) in lengths.iter().enumerate() {
            let selection = selection_bind_group(
                &self.device,
                &self.selection_layout,
                strokes[index].selection.as_ref(),
            );
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &paint_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&self.brush);
                pass.set_bind_group(0, &self.uniforms, &[]);
                pass.set_bind_group(1, &selection, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, start..start + count);
                start += count;
            }
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &output_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: if index == 0 {
                                wgpu::LoadOp::Clear(wgpu::Color::WHITE)
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&self.composite);
                pass.set_bind_group(0, &group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        if let Some(selection) = outline {
            let group = selection_bind_group(&self.device, &self.selection_layout, Some(selection));
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.outline);
            pass.set_bind_group(0, &self.uniforms, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (W * H * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(W * 4),
                    rows_per_image: Some(H),
                },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let result = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        result
    }

    fn render_tiled_strokes(&self, strokes: &[Stroke]) -> Vec<u8> {
        self.render_tiled_strokes_at_scale(strokes, 1)
    }

    fn render_tiled_strokes_at_scale(&self, strokes: &[Stroke], scale: u32) -> Vec<u8> {
        let width = 960 * scale;
        let height = 640 * scale;
        let mut document = TiledRasterDocument::new(width, height).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        for stroke in strokes {
            paint_stroke_into_tiles_at_scale(&mut document, "paint", stroke, scale).unwrap();
            document.discard_history();
        }
        let texture = TileTexture::new(&self.device, &self.queue, width, height).unwrap();
        texture
            .upload(&self.queue, &document.prepare_full_uploads())
            .unwrap();
        let (layout, _) = create_svg_pipeline(
            &self.device,
            &self.uniform_layout,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        let pipeline = create_textured_layer_pipeline(
            &self.device,
            &self.uniform_layout,
            &layout,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            include_str!("tile.wgsl"),
            "Tiled comparison",
        );
        let tile_view = texture.texture().create_view(&Default::default());
        let filter = if self.tile_nearest {
            wgpu::FilterMode::Nearest
        } else {
            wgpu::FilterMode::Linear
        };
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: filter,
            min_filter: filter,
            ..Default::default()
        });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&tile_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let output = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Tiled brush comparison"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &self.uniforms, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..6, 0..1);
        }
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(W * H * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(W * 4),
                    rows_per_image: Some(H),
                },
            },
            wgpu::Extent3d {
                width: W,
                height: H,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let pixels = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        pixels
    }
}

fn figure_eight(brush: Brush, count: usize) -> Stroke {
    let points = (0..=count * 240)
        .map(|i| {
            let t = std::f32::consts::TAU * i as f32 / 240.0 + std::f32::consts::FRAC_PI_2;
            Point {
                x: 480.0 + 240.0 * t.sin(),
                y: 320.0 + 180.0 * (2.0 * t).sin(),
            }
        })
        .collect();
    Stroke {
        brush,
        points,
        pressures: vec![],
        selection: None,
    }
}

fn brush_pixel_difference(v1: &[u8], tiled: &[u8]) -> (f64, u8, u64, u64) {
    assert_eq!(v1.len(), tiled.len());
    let mut max_diff = 0u8;
    let mut total_diff = 0u64;
    let mut changed = 0u64;
    let mut over_16 = 0u64;
    for (original, projected) in v1.as_chunks::<4>().0.iter().zip(tiled.as_chunks::<4>().0) {
        if original[..3] == [255; 3] && projected[..3] == [255; 3] {
            continue;
        }
        changed += 1;
        for (a, b) in original[..3].iter().zip(&projected[..3]) {
            let delta = a.abs_diff(*b);
            max_diff = max_diff.max(delta);
            total_diff += u64::from(delta);
            over_16 += u64::from(delta > 16);
        }
    }
    assert!(changed > 0, "comparison produced no paint");
    (
        total_diff as f64 / (changed * 3) as f64,
        max_diff,
        over_16,
        changed,
    )
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_v1_and_tile_brush_pixel_difference_diagnostic() {
    let gpu = Gpu::new();
    let line = Stroke {
        brush: Brush {
            size: 24.0,
            hardness: 1.0,
            color: [0, 0, 0],
        },
        points: vec![Point { x: 90.0, y: 120.0 }, Point { x: 500.0, y: 120.0 }],
        pressures: vec![],
        selection: None,
    };
    let arc = Stroke {
        brush: Brush {
            size: 48.0,
            hardness: 0.0,
            color: [30, 90, 180],
        },
        points: vec![
            Point { x: 100.0, y: 340.0 },
            Point { x: 240.0, y: 250.0 },
            Point { x: 400.0, y: 340.0 },
        ],
        pressures: vec![],
        selection: None,
    };
    let crossing = Stroke {
        brush: Brush {
            size: 32.0,
            hardness: 0.25,
            color: [0, 0, 0],
        },
        points: vec![Point { x: 140.0, y: 470.0 }, Point { x: 450.0, y: 560.0 }],
        pressures: vec![],
        selection: None,
    };
    let crossing_back = Stroke {
        points: vec![Point { x: 140.0, y: 560.0 }, Point { x: 450.0, y: 470.0 }],
        ..crossing.clone()
    };
    for (name, strokes) in [
        ("hard", vec![line]),
        ("soft", vec![arc]),
        ("crossing", vec![crossing, crossing_back]),
    ] {
        let v1 = gpu.render(&strokes);
        let tiled = gpu.render_tiled_strokes(&strokes);
        let (mean_diff, max_diff, over_16, changed) = brush_pixel_difference(&v1, &tiled);
        eprintln!(
            "tile-v1 {name}: changed_pixels={changed}, mean_channel_diff={:.2}, max_channel_diff={max_diff}, channels_over_16={over_16}",
            mean_diff,
        );
        assert!(
            max_diff <= 16 && mean_diff <= 2.0,
            "{name} tile preview differs too much from the v1 brush"
        );
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_v1_and_tile_brush_zoom_retina_diagnostic() {
    let hard = Stroke {
        brush: Brush {
            size: 24.0,
            hardness: 1.0,
            color: [0, 0, 0],
        },
        points: vec![Point { x: 380.0, y: 320.0 }, Point { x: 580.0, y: 320.0 }],
        pressures: vec![],
        selection: None,
    };
    let soft = Stroke {
        brush: Brush {
            size: 48.0,
            hardness: 0.0,
            color: [30, 90, 180],
        },
        points: vec![
            Point { x: 380.0, y: 360.0 },
            Point { x: 480.0, y: 240.0 },
            Point { x: 580.0, y: 360.0 },
        ],
        pressures: vec![],
        selection: None,
    };
    for (viewport, scale, zoom) in [
        ("half", 1.0, 0.5),
        ("one", 1.0, 960.0 / 976.0),
        ("double", 1.0, 2.0),
        ("retina", 2.0, 1.0),
    ] {
        let gpu = Gpu::new_with_viewport(scale, zoom);
        for (brush, strokes) in [("hard", vec![hard.clone()]), ("soft", vec![soft.clone()])] {
            let v1 = gpu.render(&strokes);
            let tiled = gpu.render_tiled_strokes(&strokes);
            let (mean, max, over_16, changed) = brush_pixel_difference(&v1, &tiled);
            eprintln!("tile-v1 {viewport}/{brush}: changed_pixels={changed}, mean_channel_diff={mean:.2}, max_channel_diff={max}, channels_over_16={over_16}");
            if viewport == "one" {
                assert!(mean <= 2.0 && max <= 16, "{brush} lost 1:1 tile parity");
            }
            if viewport == "double" || viewport == "retina" {
                let high_resolution = gpu.render_tiled_strokes_at_scale(&strokes, 2);
                let (high_mean, max, over_16, changed) =
                    brush_pixel_difference(&v1, &high_resolution);
                eprintln!("tile-v1 {viewport}/{brush}/2x: changed_pixels={changed}, mean_channel_diff={high_mean:.2}, max_channel_diff={max}, channels_over_16={over_16}");
                if viewport == "double" {
                    assert!(
                        high_mean < mean,
                        "2x tiles must improve {brush} at 200% zoom"
                    );
                }
            }
        }
    }
}

fn save_image(name: &str, pixels: Vec<u8>) {
    if let Ok(dir) = std::env::var("LUMAPAINT_GPU_ARTIFACTS") {
        std::fs::create_dir_all(&dir).unwrap();
        resvg::tiny_skia::Pixmap::from_vec(
            pixels,
            resvg::tiny_skia::IntSize::from_wh(W, H).unwrap(),
        )
        .unwrap()
        .save_png(std::path::Path::new(&dir).join(name))
        .unwrap();
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_tile_upload_updates_edge_and_clears_hidden_content() {
    let gpu = Gpu::new();
    let texture = TileTexture::new(&gpu.device, &gpu.queue, 257, 3).unwrap();
    let mut document = TiledRasterDocument::new(257, 3).unwrap();
    document.add_layer("paint".into(), "Paint".into()).unwrap();
    let changed = document
        .write_rect("paint", [255, 2, 2, 1], &[200, 0, 0, 255, 0, 0, 200, 255])
        .unwrap()
        .unwrap();
    texture
        .upload(&gpu.queue, &document.prepare_uploads(&changed).unwrap())
        .unwrap();

    let read = |gpu: &Gpu| {
        let row_pitch = 1280;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tile upload readback"),
            size: u64::from(row_pitch * 3),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.texture().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_pitch),
                    rows_per_image: Some(3),
                },
            },
            wgpu::Extent3d {
                width: 257,
                height: 3,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let data = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        data
    };
    let pixels = read(&gpu);
    assert_eq!(
        &pixels[(2 * 1280 + 255 * 4) as usize..][..4],
        &[200, 0, 0, 255]
    );
    assert_eq!(
        &pixels[(2 * 1280 + 256 * 4) as usize..][..4],
        &[0, 0, 200, 255]
    );

    let hidden = document
        .set_layer_appearance("paint", false, 1.0)
        .unwrap()
        .unwrap();
    texture
        .upload(&gpu.queue, &document.prepare_uploads(&hidden).unwrap())
        .unwrap();
    let cleared = read(&gpu);
    assert_eq!(&cleared[(2 * 1280 + 255 * 4) as usize..][..8], &[0; 8]);
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_tile_preview_uses_document_coordinates_and_premultiplied_blending() {
    let gpu = Gpu::new();
    let texture = TileTexture::new(&gpu.device, &gpu.queue, 257, 3).unwrap();
    let mut document = TiledRasterDocument::new(257, 3).unwrap();
    document.add_layer("paint".into(), "Paint".into()).unwrap();
    let changed = document
        .write_rect("paint", [255, 2, 2, 1], &[200, 0, 0, 255, 0, 0, 200, 255])
        .unwrap()
        .unwrap();
    texture
        .upload(&gpu.queue, &document.prepare_uploads(&changed).unwrap())
        .unwrap();
    let (layout, _) = create_svg_pipeline(
        &gpu.device,
        &gpu.uniform_layout,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let pipeline = create_textured_layer_pipeline(
        &gpu.device,
        &gpu.uniform_layout,
        &layout,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        include_str!("tile.wgsl"),
        "Tiled preview test",
    );
    let view = texture.texture().create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let tile_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let viewport = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&Uniforms {
                viewport: [305.0, 51.0, 1.0, 1.0],
                appearance: [0.0; 4],
                document: [257.0, 3.0, 0.0, 0.0],
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let viewport_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &gpu.uniform_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: viewport.as_entire_binding(),
        }],
    });
    let render = || {
        let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 305,
                height: 51,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target = output.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &viewport_group, &[]);
            pass.set_bind_group(1, &tile_group, &[]);
            pass.draw(0..6, 0..1);
        }
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 1280 * 51,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1280),
                    rows_per_image: Some(51),
                },
            },
            wgpu::Extent3d {
                width: 305,
                height: 51,
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let pixels = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        pixels
    };
    let pixels = render();
    assert_eq!(
        &pixels[(26 * 1280 + 279 * 4) as usize..][..4],
        &[200, 0, 0, 255]
    );
    assert_eq!(
        &pixels[(26 * 1280 + 280 * 4) as usize..][..4],
        &[0, 0, 200, 255]
    );
    assert_eq!(&pixels[(26 * 1280 + 281 * 4) as usize..][..4], &[0; 4]);

    let hidden = document
        .set_layer_appearance("paint", false, 1.0)
        .unwrap()
        .unwrap();
    texture
        .upload(&gpu.queue, &document.prepare_uploads(&hidden).unwrap())
        .unwrap();
    let hidden_pixels = render();
    assert_eq!(
        &hidden_pixels[(26 * 1280 + 279 * 4) as usize..][..8],
        &[0; 8]
    );
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_composite_selection_clips_holes_and_moves_as_one_region() {
    let gpu = Gpu::new();
    let mut selection = Selection::new(SelectionShape::Rectangle, [200.0, 180.0, 220.0, 220.0]);
    selection.regions.extend([
        SelectionRegion {
            shape: SelectionShape::Ellipse,
            bounds: [340.0, 140.0, 240.0, 260.0],
            operation: SelectionOperation::Add,
        },
        SelectionRegion {
            shape: SelectionShape::Rectangle,
            bounds: [280.0, 240.0, 100.0, 60.0],
            operation: SelectionOperation::Subtract,
        },
        SelectionRegion {
            shape: SelectionShape::Rectangle,
            bounds: [400.0, 0.0, 40.0, 640.0],
            operation: SelectionOperation::Subtract,
        },
    ]);
    for selection in [selection.clone(), selection.translated(60.0, 20.0)] {
        for hardness in [0.0, 1.0] {
            let mut stroke = Stroke {
                brush: Brush {
                    size: 512.0,
                    hardness,
                    color: [0, 0, 0],
                },
                points: vec![Point { x: 80.0, y: 300.0 }, Point { x: 880.0, y: 300.0 }],
                pressures: vec![],
                selection: None,
            };
            let baseline = gpu.render(&[stroke.clone()]);
            stroke.selection = Some(selection.clone());
            let pixels = gpu.render(&[stroke.clone()]);
            for y in (150..440).step_by(3) {
                for x in (180..700).step_by(3) {
                    let offset = (((y + 64) * W + x + 32) * 4) as usize;
                    let expected = if selection.contains(Point {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    }) {
                        baseline[offset]
                    } else {
                        255
                    };
                    assert!(
                        pixels[offset].abs_diff(expected) <= 1,
                        "Composite selection mismatch at {x},{y}"
                    );
                }
            }
            save_image(
                "composite-selection.png",
                gpu.render_with_outline(&[stroke], Some(&selection)),
            );
        }
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_selection_outline_has_no_union_seam_or_fully_subtracted_border() {
    let gpu = Gpu::new();
    let white = Stroke {
        brush: Brush {
            color: [255, 255, 255],
            ..Brush::default()
        },
        points: vec![Point { x: 10.0, y: 10.0 }],
        pressures: vec![],
        selection: None,
    };
    let mut selection = Selection::new(SelectionShape::Rectangle, [100.5, 100.5, 100.0, 200.0]);
    selection.regions.push(SelectionRegion {
        shape: SelectionShape::Rectangle,
        bounds: [200.5, 100.5, 100.0, 200.0],
        operation: SelectionOperation::Add,
    });
    let pixels = gpu.render_with_outline(std::slice::from_ref(&white), Some(&selection));
    for y in 180..300 {
        for x in 230..235 {
            assert_eq!(
                pixels[((y * W + x) * 4) as usize],
                255,
                "Internal union seam"
            );
        }
    }
    assert!(
        pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[0] < 180),
        "Visible outline is missing"
    );
    selection.regions.push(SelectionRegion {
        shape: SelectionShape::Rectangle,
        bounds: [0.0, 0.0, 960.0, 640.0],
        operation: SelectionOperation::Subtract,
    });
    let empty = gpu.render_with_outline(&[white], Some(&selection));
    assert!(
        empty.as_chunks::<4>().0.iter().all(|pixel| pixel[0] == 255),
        "An empty selection must have no outline"
    );
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_selection_clips_brush_footprint_and_survives_reload() {
    let gpu = Gpu::new();
    for shape in [SelectionShape::Rectangle, SelectionShape::Ellipse] {
        for inverted in [false, true] {
            for hardness in [0.0, 1.0] {
                let mut doc = Document::default();
                doc.begin_selection(Point { x: 300.0, y: 200.0 }, shape);
                doc.extend_selection(Point { x: 500.0, y: 400.0 }, true);
                if inverted {
                    doc.invert_selection().unwrap();
                }
                let selection = doc.selection().unwrap().clone();
                doc.begin(
                    Point { x: 80.0, y: 300.0 },
                    Brush {
                        size: 512.0,
                        hardness,
                        color: [0, 0, 0],
                    },
                )
                .unwrap();
                doc.extend(Point { x: 880.0, y: 300.0 }).unwrap();
                doc.finish();
                doc.deselect();
                let strokes: Vec<_> = doc.visible_strokes().cloned().collect();
                let pixels = gpu.render(&strokes);
                let mut unrestricted = strokes.clone();
                unrestricted[0].selection = None;
                let baseline = gpu.render(&unrestricted);
                for y in (180..420).step_by(3) {
                    for x in (280..520).step_by(3) {
                        let selected = selection.contains(Point {
                            x: x as f32 + 0.5,
                            y: y as f32 + 0.5,
                        });
                        let offset = (((y + 64) * W + x + 32) * 4) as usize;
                        let value = pixels[offset];
                        if selected {
                            assert!(
                                value.abs_diff(baseline[offset]) <= 1 && value < 240,
                                "Missing paint: {shape:?}, inverted={inverted} at {x},{y}: {value}"
                            );
                        } else {
                            assert!(value >= 254, "Paint leaked outside selection: {shape:?}, inverted={inverted} at {x},{y}: {value}");
                        }
                    }
                }
                let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
                assert_eq!(
                    pixels,
                    gpu.render(&loaded.visible_strokes().cloned().collect::<Vec<_>>())
                );
                save_image(
                    &format!("selection-{shape:?}-{inverted}-{hardness}.png"),
                    pixels,
                );
            }
        }
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_brush_self_crossing_matches_separate_strokes() {
    let gpu = Gpu::new();
    for size in [8.0, 48.0, 128.0] {
        for hardness in [0.0, 0.5, 1.0] {
            let brush = Brush {
                size,
                hardness,
                color: [32, 32, 32],
            };
            let continuous = gpu.render(&[figure_eight(brush, 3)]);
            let separate = gpu.render(&vec![figure_eight(brush, 1); 3]);
            let mut max_diff = 0;
            // Region around all six passes through the crossing; excludes pen lifts.
            for y in 284..484 {
                for x in 412..612 {
                    let offset = (y * W as usize + x) * 4;
                    max_diff = max_diff.max(continuous[offset].abs_diff(separate[offset]));
                }
            }
            eprintln!(
                "size={size} hardness={hardness}: self/separate max RGB difference={max_diff}/255"
            );
            save_image(&format!("continuous-{size}-{hardness}.png"), continuous);
            save_image(&format!("separate-{size}-{hardness}.png"), separate);
            assert!(
                max_diff <= 4,
                "Self crossing must match independently composited strokes: {max_diff}"
            );
        }
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_brush_sampling_density_does_not_change_width() {
    let gpu = Gpu::new();
    for hardness in [0.0, 0.5, 1.0] {
        let brush = Brush {
            size: 32.0,
            hardness,
            color: [0, 0, 0],
        };
        let sparse = Stroke {
            brush,
            points: vec![Point { x: 80.0, y: 320.0 }, Point { x: 880.0, y: 320.0 }],
            pressures: vec![],
            selection: None,
        };
        let dense = Stroke {
            brush,
            points: (0..=800)
                .map(|i| Point {
                    x: 80.0 + i as f32,
                    y: 320.0,
                })
                .collect(),
            pressures: vec![],
            selection: None,
        };
        let a = gpu.render(&[sparse]);
        let b = gpu.render(&[dense]);
        let mut max_diff = 0;
        let mut max_ripple = 0;
        for y in 360..408 {
            let mut low = 255;
            let mut high = 0;
            for x in 200..800 {
                let offset = (y * W as usize + x) * 4;
                max_diff = max_diff.max(a[offset].abs_diff(b[offset]));
                low = low.min(a[offset]);
                high = high.max(a[offset]);
            }
            max_ripple = max_ripple.max(high - low);
        }
        eprintln!("hardness={hardness}: input density difference={max_diff}, straight edge ripple={max_ripple}");
        assert!(max_diff <= 3);
        assert!(max_ripple <= 3);
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_brush_crossings_follow_normal_alpha_including_repeated_passes() {
    let gpu = Gpu::new();
    let brush = Brush {
        size: 64.0,
        hardness: 0.0,
        color: [0, 0, 0],
    };
    let line = |reverse: bool| Stroke {
        brush,
        points: (0..=120)
            .map(|i| {
                let x = 180.0 + i as f32 * 5.0;
                Point {
                    x,
                    y: if reverse {
                        320.0 - (x - 480.0) * 0.6
                    } else {
                        320.0 + (x - 480.0) * 0.6
                    },
                }
            })
            .collect(),
        pressures: vec![],
        selection: None,
    };
    let a = line(false);
    let b = line(true);
    let mut connected = a.clone();
    // The connecting route is well outside the measured crossing region.
    connected
        .points
        .extend([Point { x: 840.0, y: 560.0 }, Point { x: 120.0, y: 560.0 }]);
    connected.points.extend(b.points.clone());
    let first = gpu.render(std::slice::from_ref(&a));
    let second = gpu.render(std::slice::from_ref(&b));
    let split = gpu.render(&[a.clone(), b.clone()]);
    let joined = gpu.render(&[connected]);
    let repeated = gpu.render(&[a.clone(), b.clone(), a.clone(), b.clone(), a, b]);
    let decode = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let encode = |v: f32| {
        let s = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    let mut max_formula_diff = 0;
    let mut max_self_diff = 0;
    for y in 354..414 {
        for x in 482..542 {
            let i = (y * W as usize + x) * 4;
            let transmission = decode(first[i]) * decode(second[i]);
            max_formula_diff = max_formula_diff.max(split[i].abs_diff(encode(transmission)));
            max_formula_diff =
                max_formula_diff.max(repeated[i].abs_diff(encode(transmission.powi(3))));
            max_self_diff = max_self_diff.max(split[i].abs_diff(joined[i]));
        }
    }
    eprintln!("crossing alpha reference difference={max_formula_diff}/255, connected/separate={max_self_diff}/255");
    save_image("crossing-one-stroke.png", joined);
    save_image("crossing-two-strokes.png", split);
    save_image("crossing-six-passes.png", repeated);
    assert!(
        max_formula_diff <= 4,
        "Compositing must follow source-over, not maximum coverage"
    );
    assert!(
        max_self_diff <= 4,
        "Pen lifts must not change the crossing profile"
    );
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_vector_drag_translates_cached_content_and_clips_to_document() {
    let gpu = Gpu::new();
    let (layout, pipeline) = create_svg_pipeline(
        &gpu.device,
        &gpu.uniform_layout,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="20" y="20" width="20" height="20" fill="red"/></svg>"#;
    let raster = vector::rasterize_svg(source, 960, 640).unwrap();
    assert!(raster.fully_contained);
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 960,
                height: 640,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &raster.pixels,
    );
    let view = texture.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let render = |offset: [f32; 2]| {
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&Uniforms {
                    viewport: [W as f32, H as f32, 1.0, 960.0 / 976.0],
                    appearance: [0.0; 4],
                    document: [960.0, 640.0, offset[0], offset[1]],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let uniforms = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &gpu.uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let size = wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        };
        let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = output.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &uniforms, &[]);
            pass.set_bind_group(1, &group, &[]);
            pass.draw(0..6, 0..1);
        }
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (W * H * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(W * 4),
                    rows_per_image: Some(H),
                },
            },
            size,
        );
        gpu.queue.submit(Some(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let data = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        data
    };
    let alpha = |image: &[u8], x: u32, y: u32| image[((y * W + x) * 4 + 3) as usize];
    let original = render([0.0, 0.0]);
    let moved = render([100.0, 50.0]);
    assert_eq!(alpha(&original, 62, 94), 255);
    assert_eq!(alpha(&moved, 62, 94), 0);
    assert_eq!(alpha(&moved, 162, 144), 255);
    let clipped = render([-30.0, 0.0]);
    assert_eq!(
        alpha(&clipped, 27, 94),
        0,
        "Do not paint onto the gray workspace"
    );
    assert_eq!(alpha(&clipped, 37, 94), 255);
}
