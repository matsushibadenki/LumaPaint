//! Exercise the production brush shaders on an offscreen GPU texture.
//! Run explicitly with: cargo test -p lumapaint-renderer gpu_brush -- --ignored --nocapture
use super::*;
use lumapaint_core::document::{Brush, SelectionRegion};
#[cfg(test)]
use lumapaint_formats::native::NativeDocumentCodec;

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
        self.render_raster_strokes(strokes, scale, false)
    }

    fn render_pixel_strokes(&self, strokes: &[Stroke]) -> Vec<u8> {
        self.render_raster_strokes(strokes, 1, true)
    }

    fn render_raster_strokes(&self, strokes: &[Stroke], scale: u32, pixel_layer: bool) -> Vec<u8> {
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
        // Additional pixel layers use a premultiplied RGBA image and the SVG
        // compositor, instead of the background layer's tile shader/sampler.
        let image = pixel_layer.then(|| {
            let mut pixels = vec![0; (width * height * 4) as usize];
            for upload in document.prepare_full_uploads() {
                for y in 0..upload.extent[1] as usize {
                    let from = y * upload.bytes_per_row as usize;
                    let to = ((upload.origin[1] as usize + y) * width as usize
                        + upload.origin[0] as usize)
                        * 4;
                    let count = upload.extent[0] as usize * 4;
                    pixels[to..to + count].copy_from_slice(&upload.pixels[from..from + count]);
                }
            }
            self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("Pixel layer brush comparison"),
                    size: wgpu::Extent3d {
                        width,
                        height,
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
                &pixels,
            )
        });
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
            if pixel_layer {
                include_str!("svg.wgsl")
            } else {
                include_str!("tile.wgsl")
            },
            "Tiled comparison",
        );
        let tile_view = image
            .as_ref()
            .unwrap_or(texture.texture())
            .create_view(&Default::default());
        let filter = if self.tile_nearest && !pixel_layer {
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
        eraser: false,
        clear: false,
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
        eraser: false,
        clear: false,
        brush: Brush {
            simulation: Default::default(),
            envelope: Default::default(),
            size: 24.0,
            hardness: 1.0,
            color: [0, 0, 0],
        },
        points: vec![Point { x: 90.0, y: 120.0 }, Point { x: 500.0, y: 120.0 }],
        pressures: vec![],
        selection: None,
    };
    let arc = Stroke {
        eraser: false,
        clear: false,
        brush: Brush {
            simulation: Default::default(),
            envelope: Default::default(),
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
        eraser: false,
        clear: false,
        brush: Brush {
            simulation: Default::default(),
            envelope: Default::default(),
            size: 32.0,
            hardness: 0.25,
            color: [0, 0, 0],
        },
        points: vec![Point { x: 140.0, y: 470.0 }, Point { x: 450.0, y: 560.0 }],
        pressures: vec![],
        selection: None,
    };
    let crossing_back = Stroke {
        eraser: false,
        clear: false,
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
        eraser: false,
        clear: false,
        brush: Brush {
            simulation: Default::default(),
            envelope: Default::default(),
            size: 24.0,
            hardness: 1.0,
            color: [0, 0, 0],
        },
        points: vec![Point { x: 380.0, y: 320.0 }, Point { x: 580.0, y: 320.0 }],
        pressures: vec![],
        selection: None,
    };
    let soft = Stroke {
        eraser: false,
        clear: false,
        brush: Brush {
            simulation: Default::default(),
            envelope: Default::default(),
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
                        high_mean < mean || (mean < 1.0 && high_mean < 1.0),
                        "2x tiles must improve {brush} at 200% zoom (or both stay below one 8-bit level)"
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
    let batch =
        ValidatedTileUploads::new((257, 3), document.prepare_uploads(&changed).unwrap()).unwrap();
    assert_eq!(batch.write_count(), 1);
    texture.upload_validated(&gpu.queue, &batch).unwrap();

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
                eraser: false,
                clear: false,
                brush: Brush {
                    simulation: Default::default(),
                    envelope: Default::default(),
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
        eraser: false,
        clear: false,
        brush: Brush {
            simulation: Default::default(),
            envelope: Default::default(),
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
                        simulation: Default::default(),
                        envelope: Default::default(),
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
                simulation: Default::default(),
                envelope: Default::default(),
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
            simulation: Default::default(),
            envelope: Default::default(),
            size: 32.0,
            hardness,
            color: [0, 0, 0],
        };
        let sparse = Stroke {
            eraser: false,
            clear: false,
            brush,
            points: vec![Point { x: 80.0, y: 320.0 }, Point { x: 880.0, y: 320.0 }],
            pressures: vec![],
            selection: None,
        };
        let dense = Stroke {
            eraser: false,
            clear: false,
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
        simulation: Default::default(),
        envelope: Default::default(),
        size: 64.0,
        hardness: 0.0,
        color: [0, 0, 0],
    };
    let line = |reverse: bool| Stroke {
        eraser: false,
        clear: false,
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

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_text_frame_overlay_pipeline_is_valid() {
    let gpu = Gpu::new();
    gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&gpu.uniform_layout],
            push_constant_ranges: &[],
        });
    let pipeline =
        frame_overlay::pipeline(&gpu.device, &layout, wgpu::TextureFormat::Rgba8UnormSrgb);
    let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
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
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let viewport = Viewport {
        width: W,
        height: H,
        scale: 1.0,
        zoom: 1.0,
        dark: false,
        pan_x: 0.0,
        pan_y: 0.0,
        document_width: 960.0,
        document_height: 640.0,
        canvas_color: CanvasColor::White,
    };
    let (buffer, count) = frame_overlay::buffer(
        &gpu.device,
        FrameOverlay {
            corners: [[10., 10.], [400., 10.], [400., 200.], [10., 200.]],
            handles: true,
            baseline: None,
        },
        viewport,
    );
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
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &gpu.uniforms, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..count, 0..1);
    }
    gpu.queue.submit(Some(encoder.finish()));
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(pollster::block_on(gpu.device.pop_error_scope()).is_none());
}

#[test]
#[ignore = "requires a GPU adapter"]
fn gpu_channel_components_and_alpha() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (layout, _) = create_svg_pipeline(&gpu.device, &gpu.uniform_layout, format);
    let pipeline = create_textured_layer_pipeline(
        &gpu.device,
        &gpu.uniform_layout,
        &layout,
        format,
        include_str!("channel.wgsl"),
        "Channel test",
    );
    let size = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = |usage| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
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
    let source = texture(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    let output = texture(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    gpu.queue.write_texture(
        source.as_image_copy(),
        &[255, 0, 0, 128],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        size,
    );
    let source_view = source.create_view(&Default::default());
    let output_view = output.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    for (mode, expected) in [
        (1, 255),
        (2, 0),
        (3, 0),
        (4, 128),
        (5, 255),
        (6, 0),
        (7, 0),
        (8, 255),
    ] {
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&Uniforms {
                    viewport: [1., 1., 1., 1.],
                    appearance: [(mode * 2) as f32, 0., 0., 0.],
                    document: [1., 1., 0., 0.],
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
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
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
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            size,
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range();
        for value in &bytes[..3] {
            assert!(
                (i32::from(*value) - expected).abs() <= 1,
                "mode {mode}: {bytes:?}"
            );
        }
        assert_eq!(bytes[3], 255);
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_svg_rect_upload_matches_full_image_after_move_clear_and_undo() {
    let gpu = Gpu::new();
    let (width, height) = (257u32, 7u32);
    let stride = width as usize * 4;
    let empty = vec![0; stride * height as usize];
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("SVG differential upload regression"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &empty,
    );
    let mut first = empty.clone();
    first[stride + 7 * 4..stride + 7 * 4 + 4].copy_from_slice(&[64, 32, 16, 128]);
    let mut moved = empty.clone();
    moved[6 * stride + 256 * 4..6 * stride + 257 * 4].copy_from_slice(&[1, 0, 0, 1]);
    let mut previous = empty.clone();
    for next in [&first, &moved, &empty, &first, &first] {
        let rect = changed_svg_rect(&previous, next, stride);
        upload_svg_rect(&gpu.queue, &texture, next, stride, rect);
        let pitch = 1280;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SVG readback"),
            size: pitch * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
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
        let data = buffer.slice(..).get_mapped_range();
        for y in 0..height as usize {
            assert_eq!(
                &data[y * pitch as usize..y * pitch as usize + stride],
                &next[y * stride..(y + 1) * stride]
            );
        }
        drop(data);
        buffer.unmap();
        previous.clone_from(next);
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_pixel_tile_upload_preserves_untouched_pixels_opacity_and_edge_clears() {
    let gpu = Gpu::new();
    let (width, height) = (257u32, 7u32);
    let stride = width as usize * 4;
    let empty = vec![0; stride * height as usize];
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("SVG differential upload regression"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &empty,
    );
    let mut first = empty.clone();
    first[stride + 7 * 4..stride + 7 * 4 + 4].copy_from_slice(&[64, 32, 16, 128]);
    let mut moved = empty.clone();
    moved[6 * stride + 256 * 4..6 * stride + 257 * 4].copy_from_slice(&[1, 0, 0, 1]);
    let mut previous = empty.clone();
    for next in [&first, &moved, &empty, &first, &first] {
        let mut uploads = Vec::new();
        for x in 0..2 {
            let tile_width = (width - x * TILE_SIZE).min(TILE_SIZE);
            let mut pixels = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
            let mut changed = false;
            for y in 0..height as usize {
                let start = y * stride + (x * TILE_SIZE * 4) as usize;
                let end = start + tile_width as usize * 4;
                changed |= next[start..end] != previous[start..end];
                let dest = y * TILE_SIZE as usize * 4;
                pixels[dest..dest + tile_width as usize * 4].copy_from_slice(&next[start..end]);
            }
            if changed {
                uploads.push(TileUpload {
                    coord: TileCoord { x, y: 0 },
                    origin: [x * TILE_SIZE, 0],
                    extent: [tile_width, height],
                    bytes_per_row: TILE_SIZE * 4,
                    pixels,
                });
            }
        }
        upload_pixel_tiles(
            &gpu.queue,
            &texture,
            ValidatedTileUploads::new((width, height), uploads).unwrap(),
            0.5,
        );
        let pitch = 1280;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("SVG readback"),
            size: pitch * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
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
        let data = buffer.slice(..).get_mapped_range();
        for y in 0..height as usize {
            assert_eq!(
                &data[y * pitch as usize..y * pitch as usize + stride],
                &next[y * stride..(y + 1) * stride]
                    .iter()
                    .map(|value| (f32::from(*value) * 0.5).round() as u8)
                    .collect::<Vec<_>>()
            );
        }
        drop(data);
        buffer.unmap();
        previous.clone_from(next);
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_background_and_pixel_layer_brush_width_match_across_zoom() {
    for (scale, zoom) in [
        (1., 0.2),
        (1., 0.5),
        (1., 960. / 976.),
        (1., 2.),
        (2., 0.5),
        (2., 2.),
    ] {
        let gpu = Gpu::new_with_viewport(scale, zoom);
        for size in [8., 24., 64.] {
            for hardness in [0., 0.5, 1.] {
                for pressure in [0.4, 1.] {
                    let stroke = Stroke {
                        eraser: false,
                        clear: false,
                        brush: Brush {
                            size,
                            hardness,
                            color: [0; 3],
                            ..Default::default()
                        },
                        points: vec![Point { x: 200., y: 320. }, Point { x: 760., y: 320. }],
                        pressures: vec![pressure; 2],
                        selection: None,
                    };
                    let direct = gpu.render(std::slice::from_ref(&stroke));
                    let pixel = gpu.render_pixel_strokes(&[stroke]);
                    // Integrate coverage through the middle of a horizontal line.
                    // Measure in physical screen pixels, including soft brush edges.
                    let width = |image: &[u8]| -> f32 {
                        let mut total = 0.;
                        for x in W / 2 - 4..W / 2 + 4 {
                            for y in 0..H {
                                let s = image[((y * W + x) * 4) as usize] as f32 / 255.;
                                let linear = if s <= 0.04045 {
                                    s / 12.92
                                } else {
                                    ((s + 0.055) / 1.055).powf(2.4)
                                };
                                total += 1. - linear;
                            }
                        }
                        total / 8.
                    };
                    let a = width(&direct);
                    let b = width(&pixel);
                    eprintln!("scale={scale} zoom={zoom} size={size} hardness={hardness} pressure={pressure}: background={a:.2}px pixel={b:.2}px");
                    // One document pixel in the raster layer can cover multiple
                    // display pixels at high zoom. Allow that sampling precision,
                    // never a zoom-dependent expansion of the brush itself.
                    let physical_scale = ((W as f32 / scale - 48.) / 960.)
                        .min((H as f32 / scale - 48.) / 640.)
                        * zoom
                        * scale;
                    assert!((a - b).abs() <= physical_scale.max(1.0),
                        "layer-dependent width beyond raster sampling precision: background={a}, pixel={b}");
                }
            }
        }
    }
}

#[test]
#[ignore = "Requires an available GPU; run explicitly on the desktop host"]
fn gpu_artboard_exterior_pipeline_is_valid() {
    let gpu = Gpu::new();
    gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (layout, _) = create_svg_pipeline(&gpu.device, &gpu.uniform_layout, format);
    let _pipeline = create_textured_layer_pipeline(
        &gpu.device,
        &gpu.uniform_layout,
        &layout,
        format,
        include_str!("workspace.wgsl"),
        "Artboard exterior test",
    );
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(pollster::block_on(gpu.device.pop_error_scope()).is_none());
}

#[test]
#[ignore = "requires a GPU adapter"]
fn gpu_artboard_exterior_masks_page_pixels() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (layout, _) = create_svg_pipeline(&gpu.device, &gpu.uniform_layout, format);
    let pipeline = create_textured_layer_pipeline(
        &gpu.device,
        &gpu.uniform_layout,
        &layout,
        format,
        include_str!("workspace.wgsl"),
        "Exterior mask test",
    );
    let size = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = |usage| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
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
    let source = texture(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    let output = texture(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    gpu.queue.write_texture(
        source.as_image_copy(),
        &[255, 0, 0, 128],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        size,
    );
    let source_view = source.create_view(&Default::default());
    let output_view = output.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    for (pan, expected_alpha) in [(0.0, 0), (100.0, 128)] {
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&Uniforms {
                    viewport: [100., 100., 1., 1.],
                    appearance: [0., pan, 0., 0.],
                    document: [4., 4., 0., 0.],
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
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
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
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            size,
        );
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range();
        assert_eq!(bytes[3], expected_alpha, "pan {pan}: {bytes:?}");
    }
}

#[test]
#[ignore = "requires a GPU adapter"]
fn gpu_object_texture_moves_across_artboard_without_reupload() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let (texture_layout, _) = create_svg_pipeline(&gpu.device, &gpu.uniform_layout, format);
    let geometry_layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
    let pipeline = create_layer_pipeline(
        &gpu.device,
        &[&gpu.uniform_layout, &texture_layout, &geometry_layout],
        format,
        include_str!("object.wgsl"),
        "Object translation regression",
    );
    let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="-20" y="10" width="60" height="30" fill="red" opacity="0.5"/></svg>"#;
    let (pixels, rectangle) = vector::rasterize_svg_object(source, (960, 640)).unwrap();
    // This upload and the object rectangle are reused for every drag frame.
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: rectangle[2] as u32,
                height: rectangle[3] as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &pixels,
    );
    let texture_view = texture.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let image = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &texture_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let bounds_buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&rectangle),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let geometry = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &geometry_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: bounds_buffer.as_entire_binding(),
        }],
    });
    let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = output.create_view(&Default::default());
    for offset in [-10.0, 40.0, 960.0] {
        let uniforms_buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&Uniforms {
                    viewport: [W as f32, H as f32, 1., 960. / 976.],
                    appearance: [0.; 4],
                    document: [960., 640., offset, 0.],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let uniforms = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &gpu.uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms_buffer.as_entire_binding(),
            }],
        });
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (W * H * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
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
            pass.set_bind_group(1, &image, &[]);
            pass.set_bind_group(2, &geometry, &[]);
            pass.draw(0..6, 0..1);
        }
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
        gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let data = readback.slice(..).get_mapped_range();
        let x = (42. + offset) as usize;
        let pixel = (89 * W as usize + x) * 4;
        assert_eq!(data[pixel + 3], 128, "offset {offset}");
        assert!(data[pixel] > 0 && data[pixel + 1] == 0);
        assert_eq!(data[(89 * W as usize + 500) * 4 + 3], 0);
    }
}
