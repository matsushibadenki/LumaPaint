//! Thread-confined Metal cache rasterization. CPU fallback remains in vector.rs.
use metal::foreign_types::ForeignType;
use skia_safe::{gpu, ImageInfo, Surface};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

enum State {
    Uninitialized,
    Ready(gpu::DirectContext),
    Unavailable,
}

thread_local! {
    // Ganesh contexts may not be shared concurrently across rendering workers.
    static CONTEXT: RefCell<State> = const { RefCell::new(State::Uninitialized) };
    static SHARED_CONTEXTS: RefCell<HashMap<(usize, usize), State>> = RefCell::new(HashMap::new());
    static LAST_CACHE_SAMPLE: RefCell<Option<Instant>> = const { RefCell::new(None) };
}

#[derive(Debug, Default)]
struct CacheUsage {
    contexts: usize,
    resources: usize,
    bytes: usize,
    purgeable_bytes: usize,
    budget_bytes: usize,
}

fn cache_usage() -> Option<CacheUsage> {
    let mut usage = CacheUsage::default();
    let mut add = |state: &State| {
        if let State::Ready(context) = state {
            let resources = context.resource_cache_usage();
            usage.contexts += 1;
            usage.resources = usage.resources.saturating_add(resources.resource_count);
            usage.bytes = usage.bytes.saturating_add(resources.resource_bytes);
            usage.purgeable_bytes = usage
                .purgeable_bytes
                .saturating_add(context.resource_cache_purgeable_bytes());
            usage.budget_bytes = usage
                .budget_bytes
                .saturating_add(context.resource_cache_limit());
        }
    };
    CONTEXT.with(|slot| {
        let state = slot.try_borrow().ok()?;
        add(&state);
        Some(())
    })?;
    SHARED_CONTEXTS.with(|slot| {
        for context in slot.try_borrow().ok()?.values() {
            add(context);
        }
        Some(())
    })?;
    Some(usage)
}

fn report_cache() {
    if !crate::performance::enabled() {
        return;
    }
    let now = Instant::now();
    let sample = LAST_CACHE_SAMPLE.with(|slot| {
        let mut last = slot.borrow_mut();
        if last.is_some_and(|last| now.duration_since(last) < Duration::from_secs(1)) {
            return false;
        }
        *last = Some(now);
        true
    });
    if sample {
        if let Some(usage) = cache_usage() {
            eprintln!(
                "lumapaint-skia-cache thread={:?} {:?}",
                std::thread::current().id(),
                usage
            );
        }
    }
}

/// The texture is allocated on wgpu's exact Metal device. Skia finishes writing
/// on wgpu's queue before its sampling commands; no CPU wait or upload is involved.
pub(super) fn rasterize_shared(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    info: &ImageInfo,
    draw: impl FnOnce(&mut Surface),
) -> Option<wgpu::Texture> {
    // SAFETY: the guard is kept while taking an owned reference to the device.
    let raw_device = unsafe {
        let hal = device.as_hal::<wgpu::hal::api::Metal>()?;
        metal::Device::from_ptr(objc2::rc::Retained::into_raw(hal.raw_device().clone()).cast())
    };
    // SAFETY: callers pass the queue belonging to this device. Keep an owned
    // Metal reference after releasing the HAL guard. Both engines commit to
    // the same serial queue, so GPU ordering replaces a CPU completion wait.
    let raw_queue = unsafe {
        let hal = queue.as_hal::<wgpu::hal::api::Metal>()?;
        let retained = objc2::rc::Retained::retain(hal.as_raw() as *const _
            as *mut objc2::runtime::ProtocolObject<dyn objc2_metal::MTLCommandQueue>)?;
        metal::CommandQueue::from_ptr(objc2::rc::Retained::into_raw(retained).cast())
    };
    // Windows can use the same physical device with different serial queues.
    // Reusing a context across queues would lose producer/consumer ordering.
    let key = (raw_device.as_ptr() as usize, raw_queue.as_ptr() as usize);
    let result = SHARED_CONTEXTS.with(|slot| {
        let mut contexts = slot.try_borrow_mut().ok()?;
        // Bound thread-local contexts even when many windows/devices are opened.
        if !contexts.contains_key(&key) && contexts.len() >= 4 {
            contexts.clear();
        }
        let state = contexts.entry(key).or_insert_with(|| {
            // BackendContext and Ganesh retain wgpu's exact device and queue.
            let backend = unsafe {
                gpu::mtl::BackendContext::new(raw_device.as_ptr().cast(), raw_queue.as_ptr().cast())
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
        crate::gpu_metrics::external_texture_created(
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::Extent3d {
                width: info.width() as u32,
                height: info.height() as u32,
                depth_or_array_layers: 1,
            },
        );
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
        if !context.submit(gpu::SyncCpu::No) || context.abandoned() {
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
                objc2::rc::Retained::retain(
                    raw.as_ptr()
                        .cast::<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLTexture>>(),
                )?,
                wgpu::TextureFormat::Rgba8Unorm,
                objc2_metal::MTLTextureType::Type2D,
                1,
                1,
                wgpu::hal::CopyExtent {
                    width: size.width,
                    height: size.height,
                    depth: 1,
                },
                None,
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
                wgpu::TextureUses::COLOR_TARGET,
            )
        })
    });
    report_cache();
    result
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
    let result = CONTEXT.with(|slot| {
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
    });
    report_cache();
    result
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
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
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
                    &queue,
                    &source,
                    (dimension, dimension),
                    1.0,
                )
                .unwrap();
                // Measure completed GPU work for both paths. Production keeps
                // these operations asynchronous; submission latency alone would
                // make the comparison with legacy readback misleading.
                queue.submit([]);
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
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
    fn shared_queue_orders_interleaved_skia_and_wgpu_without_cpu_waits() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shared queue ordering regression"),
            size: 64 * 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let info = ImageInfo::new(
            (16, 16),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        for i in 0..64u8 {
            let texture = rasterize_shared(&device, &queue, &info, |surface| {
                surface
                    .canvas()
                    .clear(skia_safe::Color::from_rgb(i, 255 - i, i * 3));
            })
            .unwrap();
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: u64::from(i) * 256,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            // No poll or CPU wait between Skia writes and wgpu reads. Resources
            // may be released by the caller while their commands remain queued.
        }
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = buffer
            .slice(..)
            .get_mapped_range()
            .expect("GPU buffer mapped after successful map callback");
        for i in 0..64u8 {
            assert_eq!(
                &bytes[usize::from(i) * 256..usize::from(i) * 256 + 4],
                &[i, 255 - i, i * 3, 255]
            );
        }
        drop(bytes);
        buffer.unmap();
        SHARED_CONTEXTS.with(|slot| slot.borrow_mut().clear());
    }

    #[test]
    #[ignore = "requires a real Metal device"]
    fn different_window_queues_keep_separate_contexts() {
        SHARED_CONTEXTS.with(|slot| slot.borrow_mut().clear());
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let first = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let second = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let info = ImageInfo::new(
            (16, 16),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut reads = Vec::new();
        for (device, queue) in [&first, &second, &first, &second] {
            let texture = rasterize_shared(device, queue, &info, |surface| {
                surface.canvas().clear(skia_safe::Color::RED);
            })
            .unwrap();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Multi-window queue readback"),
                size: 256,
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
                        bytes_per_row: Some(256),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            reads.push((device, buffer));
        }
        SHARED_CONTEXTS.with(|slot| assert_eq!(slot.borrow().len(), 2));
        for (device, buffer) in reads {
            let (tx, rx) = std::sync::mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            rx.recv().unwrap().unwrap();
            assert_eq!(
                &buffer
                    .slice(..)
                    .get_mapped_range()
                    .expect("GPU buffer mapped after successful map callback")[..4],
                &[255, 0, 0, 255]
            );
            buffer.unmap();
        }
        SHARED_CONTEXTS.with(|slot| slot.borrow_mut().clear());
    }

    #[test]
    #[ignore = "requires a real Metal device"]
    fn shared_texture_matches_metal_and_survives_context_eviction() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
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
            let (texture, rectangle) = super::super::rasterize_svg_object_shared(
                &device,
                &queue,
                &source,
                (180, 80),
                opacity,
            )
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
            let mapped = buffer
                .slice(..)
                .get_mapped_range()
                .expect("GPU buffer mapped after successful map callback");
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
        assert!(super::super::rasterize_svg_object_shared(
            &device,
            &queue,
            unsupported,
            (16, 16),
            1.0
        )
        .is_none());
        // Force an unavailable shared context: the caller can use the established
        // CPU raster/upload path without losing the document edit.
        let device_key = objc2::rc::Retained::as_ptr(
            unsafe { device.as_hal::<wgpu::hal::api::Metal>() }
                .unwrap()
                .raw_device(),
        ) as usize;
        let queue_key = unsafe { queue.as_hal::<wgpu::hal::api::Metal>() }
            .unwrap()
            .as_raw() as *const _ as usize;
        let key = (device_key, queue_key);
        SHARED_CONTEXTS.with(|slot| {
            slot.borrow_mut().insert(key, State::Unavailable);
        });
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="red"/></svg>"#;
        assert!(
            super::super::rasterize_svg_object_shared(&device, &queue, source, (16, 16), 1.0)
                .is_none()
        );
        assert!(super::super::rasterize_svg_object(source, (16, 16)).is_ok());
        CONTEXT.with(|slot| *slot.borrow_mut() = State::Uninitialized);
        SHARED_CONTEXTS.with(|slot| slot.borrow_mut().clear());
    }

    #[test]
    #[ignore = "requires a real Metal device"]
    fn metal_cache_renders_paths_and_text_with_premultiplied_pixels() {
        CONTEXT.with(|state| *state.borrow_mut() = State::Uninitialized);
        SHARED_CONTEXTS.with(|slot| slot.borrow_mut().clear());
        assert_eq!(cache_usage().unwrap().contexts, 0);
        CONTEXT.with(|slot| {
            let _borrow = slot.borrow_mut();
            assert!(cache_usage().is_none());
        });
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="red" fill-opacity="0.5"/></svg>"#;
        let gpu = super::super::rasterize_svg(source, 64, 64).unwrap();
        assert_eq!(gpu.backend, super::super::SvgBackend::SkiaGpu);
        let usage = cache_usage().unwrap();
        assert_eq!(usage.contexts, 1);
        assert_eq!(usage.budget_bytes, 64 * 1024 * 1024);
        assert!(usage.resources > 0);
        assert!(usage.bytes > 0);
        CONTEXT.with(|state| *state.borrow_mut() = State::Unavailable);
        let released = cache_usage().unwrap();
        assert_eq!(released.contexts, 0);
        assert_eq!(released.bytes, 0);
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
