//! Thread-confined Metal cache rasterization. CPU fallback remains in vector.rs.
use metal::foreign_types::ForeignType;
use skia_safe::{gpu, ImageInfo, Surface};
use std::cell::RefCell;
use std::collections::HashMap;

enum State {
    Uninitialized,
    Ready(gpu::DirectContext),
    Unavailable,
}

thread_local! {
    // Ganesh contexts may not be shared concurrently across rendering workers.
    static CONTEXT: RefCell<State> = const { RefCell::new(State::Uninitialized) };
    static SHARED_CONTEXTS: RefCell<HashMap<usize, State>> = RefCell::new(HashMap::new());
}

/// The texture is allocated on wgpu's exact Metal device. Skia finishes writing
/// before ownership is handed to wgpu; no pixel buffer or upload is involved.
pub(super) fn rasterize_shared(
    device: &wgpu::Device,
    info: &ImageInfo,
    draw: impl FnOnce(&mut Surface),
) -> Option<wgpu::Texture> {
    // SAFETY: the guard is kept while taking an owned reference to the device.
    let raw_device = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?
        .raw_device()
        .lock()
        .clone();
    let key = raw_device.as_ptr() as usize;
    SHARED_CONTEXTS.with(|slot| {
        let mut contexts = slot.try_borrow_mut().ok()?;
        // Bound thread-local contexts even when many windows/devices are opened.
        if !contexts.contains_key(&key) && contexts.len() >= 4 {
            contexts.clear();
        }
        let state = contexts.entry(key).or_insert_with(|| {
            let queue = raw_device.new_command_queue();
            // BackendContext and Ganesh retain the exact device and queue.
            let backend = unsafe {
                gpu::mtl::BackendContext::new(raw_device.as_ptr().cast(), queue.as_ptr().cast())
            };
            gpu::direct_contexts::make_metal(&backend, None).map_or(
                State::Unavailable,
                |mut context| {
                    context.set_resource_cache_limit(64 * 1024 * 1024);
                    State::Ready(context)
                },
            )
        });
        let State::Ready(context) = state else {
            return None;
        };
        if context.abandoned() {
            *state = State::Unavailable;
            return None;
        }
        let descriptor = metal::TextureDescriptor::new();
        descriptor.set_texture_type(metal::MTLTextureType::D2);
        descriptor.set_pixel_format(metal::MTLPixelFormat::RGBA8Unorm);
        descriptor.set_width(info.width() as u64);
        descriptor.set_height(info.height() as u64);
        descriptor.set_storage_mode(metal::MTLStorageMode::Private);
        descriptor.set_usage(
            metal::MTLTextureUsage::RenderTarget
                | metal::MTLTextureUsage::ShaderRead
                | metal::MTLTextureUsage::PixelFormatView,
        );
        let raw = raw_device.new_texture(&descriptor);
        // TextureInfo retains raw; all wrappers live until rendering completes.
        let texture_info = unsafe { gpu::mtl::TextureInfo::new(raw.as_ptr().cast()) };
        let backend = unsafe {
            gpu::backend_textures::make_mtl(
                (info.width(), info.height()),
                gpu::Mipmapped::No,
                &texture_info,
                "LP shared vector",
            )
        };
        let mut surface = gpu::surfaces::wrap_backend_texture(
            context,
            &backend,
            gpu::SurfaceOrigin::TopLeft,
            Some(0),
            skia_safe::ColorType::RGBA8888,
            None,
            None,
        )?;
        draw(&mut surface);
        context.flush(None);
        if !context.submit(gpu::SyncCpu::Yes) || context.abandoned() {
            *state = State::Unavailable;
            return None;
        }
        drop(surface);
        let size = wgpu::Extent3d {
            width: info.width() as u32,
            height: info.height() as u32,
            depth_or_array_layers: 1,
        };
        // SAFETY: raw came from this device, is initialized by Skia (including
        // transparent pixels), and matches the descriptor. wgpu owns its retained
        // Metal texture after import. sRGB is a sampling view, preserving the
        // same encoded premultiplied bytes as the old RGBA upload path.
        let hal = unsafe {
            wgpu::hal::metal::Device::texture_from_raw(
                raw,
                wgpu::TextureFormat::Rgba8Unorm,
                metal::MTLTextureType::D2,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width: size.width,
                    height: size.height,
                    depth: 1,
                },
            )
        };
        Some(unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Metal>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some("Shared Skia vector texture"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
                },
            )
        })
    })
}

fn context() -> Option<gpu::DirectContext> {
    let device = metal::Device::all()
        .into_iter()
        .find(|device| !device.is_low_power())
        .or_else(metal::Device::system_default)?;
    let queue = device.new_command_queue();
    // BackendContext retains both handles; Ganesh takes its own references.
    let backend =
        unsafe { gpu::mtl::BackendContext::new(device.as_ptr().cast(), queue.as_ptr().cast()) };
    let mut context = gpu::direct_contexts::make_metal(&backend, None)?;
    context.set_resource_cache_limit(64 * 1024 * 1024);
    Some(context)
}

pub(super) fn rasterize(info: &ImageInfo, draw: impl FnOnce(&mut Surface)) -> Option<Vec<u8>> {
    CONTEXT.with(|slot| {
        let mut state = slot.try_borrow_mut().ok()?;
        if matches!(*state, State::Uninitialized) {
            *state = context().map_or(State::Unavailable, State::Ready);
        }
        let State::Ready(context) = &mut *state else {
            return None;
        };
        if context.abandoned() {
            *state = State::Unavailable;
            return None;
        }
        let mut surface = gpu::surfaces::render_target(
            context,
            gpu::Budgeted::Yes,
            info,
            Some(0),
            gpu::SurfaceOrigin::TopLeft,
            None,
            false,
            false,
        )?;
        draw(&mut surface);
        context.flush_and_submit();
        // This stays inside Rust. The existing wgpu upload contract consumes RGBA.
        // A failed readback/allocation must never discard the CPU-renderable edit.
        let mut pixels = vec![0; info.width() as usize * info.height() as usize * 4];
        if surface.read_pixels(info, &mut pixels, info.width() as usize * 4, (0, 0)) {
            Some(pixels)
        } else {
            *state = State::Unavailable;
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::util::DeviceExt;
    #[test]
    fn unavailable_gpu_returns_control_to_cpu() {
        CONTEXT.with(|state| *state.borrow_mut() = State::Unavailable);
        let info = ImageInfo::new_n32_premul((16, 16), None);
        assert!(rasterize(&info, |_| panic!("unavailable GPU must not draw")).is_none());
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="red"/></svg>"#;
        let result = super::super::rasterize_svg(source, 16, 16).unwrap();
        assert_eq!(result.backend, super::super::SvgBackend::Skia);
        assert_eq!(
            &result.pixels[(8 * 16 + 8) * 4..(8 * 16 + 8) * 4 + 4],
            &[255, 0, 0, 255]
        );
        CONTEXT.with(|state| *state.borrow_mut() = State::Uninitialized);
    }

    #[test]
    #[ignore = "requires Metal; local cache-generation benchmark"]
    fn benchmark_shared_texture_transfer() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        for dimension in [1024u32, 2048] {
            let source = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{dimension}" height="{dimension}"><rect width="{dimension}" height="{dimension}" fill="red" fill-opacity="0.5"/><circle cx="400" cy="400" r="200" fill="blue"/></svg>"#
            );
            let mut legacy = Vec::new();
            let mut shared = Vec::new();
            let mut transferred = 0;
            for sample in 0..9 {
                let start = std::time::Instant::now();
                let (pixels, bounds) =
                    super::super::rasterize_svg_object(&source, (dimension, dimension)).unwrap();
                transferred = pixels.len();
                let texture = device.create_texture_with_data(
                    &queue,
                    &wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d {
                            width: bounds[2] as u32,
                            height: bounds[3] as u32,
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
                queue.submit([]);
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                drop(texture);
                let start = std::time::Instant::now();
                let (texture, _) = super::super::rasterize_svg_object_shared(
                    &device,
                    &source,
                    (dimension, dimension),
                    1.0,
                )
                .unwrap();
                let direct_elapsed = start.elapsed().as_secs_f64() * 1000.0;
                drop(texture);
                if sample > 0 {
                    legacy.push(elapsed);
                    shared.push(direct_elapsed);
                }
            }
            legacy.sort_by(f64::total_cmp);
            shared.sort_by(f64::total_cmp);
            eprintln!("shared cache benchmark {dimension}: legacy median={:.3} ms shared median={:.3} ms legacy readback+upload bytes={} shared=0", (legacy[3]+legacy[4])*0.5, (shared[3]+shared[4])*0.5, transferred*2);
        }
    }

    #[test]
    #[ignore = "requires a real Metal device"]
    fn shared_texture_matches_metal_and_survives_context_eviction() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        for (body, opacity) in [
            (
                r#"<defs><linearGradient id="g" x1="0" x2="1"><stop stop-color="red"/><stop offset="1" stop-color="blue" stop-opacity=".4"/></linearGradient></defs><rect x="8" y="7" width="125" height="61" fill="url(#g)"/>"#,
                1.0,
            ),
            (
                r#"<defs><radialGradient id="g"><stop stop-color="white"/><stop offset="1" stop-color="black"/></radialGradient></defs><rect x="8" y="7" width="125" height="61" fill="url(#g)"/>"#,
                1.0,
            ),
            (
                r##"<rect x="8" y="7" width="25" height="31" fill="#a83d72" fill-opacity="0.5"/><path d="M4 5 C10 40 35 3 50 40" fill="none" stroke="blue" stroke-width="3"/>"##,
                1.0,
            ),
            (
                r#"<text x="5" y="40" font-size="24" font-family="sans-serif">GPU 日本語</text>"#,
                0.6,
            ),
        ] {
            let source = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="180" height="80">{body}</svg>"#
            );
            let (texture, rectangle) =
                super::super::rasterize_svg_object_shared(&device, &source, (180, 80), opacity)
                    .unwrap();
            // The imported texture owns its resources, independent of Ganesh's
            // thread-local context and backend wrappers.
            SHARED_CONTEXTS.with(|slot| slot.borrow_mut().clear());
            let _view = texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
                ..Default::default()
            });
            let width = rectangle[2] as u32;
            let height = rectangle[3] as u32;
            let stride = (width * 4).div_ceil(256) * 256;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: stride as u64 * height as u64,
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
                        rows_per_image: Some(height),
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
            CONTEXT.with(|slot| *slot.borrow_mut() = State::Uninitialized);
            let (mut expected, reference_rectangle) =
                super::super::rasterize_svg_object(&source, (180, 80)).unwrap();
            assert_eq!(rectangle, reference_rectangle);
            for value in &mut expected {
                *value = (*value as f32 * opacity).round() as u8;
            }
            let mapped = buffer.slice(..).get_mapped_range();
            let actual: Vec<_> = mapped
                .chunks(stride as usize)
                .flat_map(|row| row[..width as usize * 4].iter().copied())
                .collect();
            assert_eq!(actual.len(), expected.len());
            let difference = actual
                .iter()
                .zip(&expected)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                difference <= 1,
                "shared texture pixel difference: {difference}; max {:?}",
                actual
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .max_by_key(|(_, (a, b))| a.abs_diff(**b))
            );
            drop(mapped);
            buffer.unmap();
        }
        let unsupported = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><defs><pattern id="g" width="4" height="4" patternUnits="userSpaceOnUse"><rect width="4" height="4" fill="red"/></pattern></defs><rect width="16" height="16" fill="url(#g)"/></svg>"#;
        assert!(
            super::super::rasterize_svg_object_shared(&device, unsupported, (16, 16), 1.0)
                .is_none()
        );
        // Force an unavailable shared context: the caller can use the established
        // CPU raster/upload path without losing the document edit.
        let key = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }
            .unwrap()
            .raw_device()
            .lock()
            .as_ptr() as usize;
        SHARED_CONTEXTS.with(|slot| {
            slot.borrow_mut().insert(key, State::Unavailable);
        });
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="red"/></svg>"#;
        assert!(
            super::super::rasterize_svg_object_shared(&device, source, (16, 16), 1.0).is_none()
        );
        assert!(super::super::rasterize_svg_object(source, (16, 16)).is_ok());
        CONTEXT.with(|slot| *slot.borrow_mut() = State::Uninitialized);
        SHARED_CONTEXTS.with(|slot| slot.borrow_mut().clear());
    }

    #[test]
    #[ignore = "requires a real Metal device"]
    fn metal_cache_renders_paths_and_text_with_premultiplied_pixels() {
        CONTEXT.with(|state| *state.borrow_mut() = State::Uninitialized);
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="red" fill-opacity="0.5"/></svg>"#;
        let gpu = super::super::rasterize_svg(source, 64, 64).unwrap();
        assert_eq!(gpu.backend, super::super::SvgBackend::SkiaGpu);
        CONTEXT.with(|state| *state.borrow_mut() = State::Unavailable);
        let cpu = super::super::rasterize_svg(source, 64, 64).unwrap();
        for (actual, expected) in gpu.pixels.iter().zip(&cpu.pixels) {
            assert!(actual.abs_diff(*expected) <= 1);
        }
        CONTEXT.with(|state| *state.borrow_mut() = State::Uninitialized);
        let text = r#"<svg xmlns="http://www.w3.org/2000/svg" width="180" height="80"><text x="5" y="40" font-size="24" font-family="sans-serif">GPU 日本語</text></svg>"#;
        let result = super::super::rasterize_svg(text, 180, 80).unwrap();
        assert_eq!(result.backend, super::super::SvgBackend::SkiaGpu);
        assert!(result
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0));
    }
}
