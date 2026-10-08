//! Scoped wgpu API accounting. Logical payloads are not driver VRAM/staging usage.
use crate::performance;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

#[derive(Default)]
struct CacheRegistry {
    entries: HashMap<wgpu::Texture, Weak<CacheLease>>,
}

struct CacheLease {
    bytes: Option<u64>,
}

/// Tracks cache ownership, not completion of GPU work or physical VRAM release.
pub(crate) struct CachedTextureLease {
    _lease: Option<Arc<CacheLease>>,
    key: Option<wgpu::Texture>,
}

static CACHE_REGISTRY: OnceLock<Mutex<CacheRegistry>> = OnceLock::new();

impl CachedTextureLease {
    pub(crate) fn new(texture: &wgpu::Texture) -> Self {
        if !performance::enabled() {
            return Self {
                _lease: None,
                key: None,
            };
        }
        let mut registry = CACHE_REGISTRY
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Remove dead keys: retaining a key until the next acquisition would retain
        // a GPU handle. Leases therefore remove their key on final drop below.
        let lease = registry.entries.get(texture).and_then(Weak::upgrade);
        let lease = lease.unwrap_or_else(|| {
            let bytes = texture_bytes(&wgpu::TextureDescriptor {
                label: None,
                size: texture.size(),
                mip_level_count: texture.mip_level_count(),
                sample_count: texture.sample_count(),
                dimension: texture.dimension(),
                format: texture.format(),
                usage: texture.usage(),
                view_formats: &[],
            });
            let lease = Arc::new(CacheLease { bytes });
            registry
                .entries
                .insert(texture.clone(), Arc::downgrade(&lease));
            lease
        });
        Self {
            _lease: Some(lease),
            key: Some(texture.clone()),
        }
    }
}

impl Drop for CachedTextureLease {
    fn drop(&mut self) {
        let Some(lease) = self._lease.take() else {
            return;
        };
        // Serialize last-owner detection against acquisition and other releases.
        let mut registry = CACHE_REGISTRY
            .get()
            .unwrap()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        drop(lease);
        let key = self.key.take().unwrap();
        if registry
            .entries
            .get(&key)
            .is_some_and(|lease| lease.strong_count() == 0)
        {
            registry.entries.remove(&key);
        }
    }
}

pub(crate) fn cached_texture_capacity() -> (u64, u64, u64) {
    let Some(registry) = CACHE_REGISTRY.get() else {
        return (0, 0, 0);
    };
    let registry = registry.lock().unwrap_or_else(|error| error.into_inner());
    let mut bytes = 0u64;
    let mut count = 0;
    let mut unknown = 0;
    for lease in registry.entries.values().filter_map(Weak::upgrade) {
        count += 1;
        match lease.bytes {
            Some(size) => bytes = bytes.saturating_add(size),
            None => unknown += 1,
        }
    }
    (count, bytes, unknown)
}
fn texel_bytes(
    format: wgpu::TextureFormat,
    extent: wgpu::Extent3d,
    aspect: Option<wgpu::TextureAspect>,
) -> Option<u64> {
    let (w, h) = format.block_dimensions();
    u64::from(extent.width.div_ceil(w))
        .checked_mul(u64::from(extent.height.div_ceil(h)))?
        .checked_mul(u64::from(extent.depth_or_array_layers))?
        .checked_mul(u64::from(format.block_copy_size(aspect)?))
}
fn texture_bytes(desc: &wgpu::TextureDescriptor<'_>) -> Option<u64> {
    if desc.mip_level_count == 0
        || desc.mip_level_count > 32
        || desc.sample_count == 0
        || desc.size.width == 0
        || desc.size.height == 0
        || desc.size.depth_or_array_layers == 0
    {
        return None;
    }
    let mut total = 0u64;
    for mip in 0..desc.mip_level_count {
        let extent = wgpu::Extent3d {
            width: desc.size.width.checked_shr(mip).unwrap_or(0).max(1),
            height: desc.size.height.checked_shr(mip).unwrap_or(0).max(1),
            depth_or_array_layers: if desc.dimension == wgpu::TextureDimension::D3 {
                desc.size
                    .depth_or_array_layers
                    .checked_shr(mip)
                    .unwrap_or(0)
                    .max(1)
            } else {
                desc.size.depth_or_array_layers
            },
        };
        total = total.checked_add(
            texel_bytes(desc.format, extent, None)?.checked_mul(u64::from(desc.sample_count))?,
        )?;
    }
    Some(total)
}
pub(crate) fn texture_created(desc: &wgpu::TextureDescriptor<'_>) {
    if !performance::enabled() {
        return;
    }
    performance::count("gpu_texture_creations", 1);
    match texture_bytes(desc) {
        Some(bytes) => performance::count("gpu_texture_created_logical_bytes", bytes),
        None => performance::count("gpu_texture_creation_unmeasured_count", 1),
    }
}
#[cfg(all(feature = "skia", target_os = "macos"))]
pub(crate) fn external_texture_created(format: wgpu::TextureFormat, size: wgpu::Extent3d) {
    if !performance::enabled() {
        return;
    }
    performance::count("gpu_external_texture_creations", 1);
    performance::count("gpu_texture_creations", 1);
    match texel_bytes(format, size, None) {
        Some(bytes) => performance::count("gpu_texture_created_logical_bytes", bytes),
        None => performance::count("gpu_texture_creation_unmeasured_count", 1),
    }
}
pub(crate) fn texture_written(
    destination: &wgpu::TexelCopyTextureInfo<'_>,
    size: wgpu::Extent3d,
    source_len: usize,
) {
    if !performance::enabled() {
        return;
    }
    performance::count("gpu_texture_write_calls", 1);
    performance::count("gpu_texture_write_source_bytes", source_len as u64);
    match texel_bytes(destination.texture.format(), size, Some(destination.aspect)) {
        Some(bytes) => {
            performance::count("gpu_texture_written_texel_bytes", bytes);
            performance::count("gpu_host_to_resource_payload_bytes", bytes);
        }
        None => performance::count("gpu_texture_write_unmeasured_formats", 1),
    }
}

pub(crate) fn texture_copied(
    kind: &'static str,
    source: &wgpu::TexelCopyTextureInfo<'_>,
    size: wgpu::Extent3d,
) {
    if !performance::enabled() {
        return;
    }
    performance::count("gpu_texture_copy_calls", 1);
    match texel_bytes(source.texture.format(), size, Some(source.aspect)) {
        Some(bytes) => {
            performance::count(kind, bytes);
            performance::count("gpu_encoded_copy_payload_bytes", bytes);
        }
        None => performance::count("gpu_texture_copy_unmeasured_formats", 1),
    }
}

macro_rules! copy_texture_to_texture {
    ($encoder:expr, $source:expr, $destination:expr, $size:expr $(,)?) => {{
        match ($source, $destination, $size) {
            (source, destination, size) => {
                $crate::gpu_metrics::texture_copied(
                    "gpu_copy_texture_to_texture_texel_bytes",
                    &source,
                    size,
                );
                let _timer = $crate::performance::time("gpu_copy_encode_host");
                $encoder.copy_texture_to_texture(source, destination, size)
            }
        }
    }};
}
#[cfg(test)]
macro_rules! copy_texture_to_buffer {
    ($encoder:expr, $source:expr, $destination:expr, $size:expr $(,)?) => {{
        match ($source, $destination, $size) {
            (source, destination, size) => {
                $crate::gpu_metrics::texture_copied(
                    "gpu_copy_texture_to_buffer_texel_bytes",
                    &source,
                    size,
                );
                let _timer = $crate::performance::time("gpu_copy_encode_host");
                $encoder.copy_texture_to_buffer(source, destination, size)
            }
        }
    }};
}
macro_rules! copy_buffer_to_texture {
    ($encoder:expr, $source:expr, $destination:expr, $size:expr $(,)?) => {{
        match ($source, $destination, $size) {
            (source, destination, size) => {
                $crate::gpu_metrics::texture_copied(
                    "gpu_copy_buffer_to_texture_texel_bytes",
                    &destination,
                    size,
                );
                let _timer = $crate::performance::time("gpu_copy_encode_host");
                $encoder.copy_buffer_to_texture(source, destination, size)
            }
        }
    }};
}
macro_rules! copy_buffer_to_buffer {
    ($encoder:expr, $source:expr, $source_offset:expr, $destination:expr, $destination_offset:expr, $size:expr $(,)?) => {{
        match (
            $source,
            $source_offset,
            $destination,
            $destination_offset,
            $size,
        ) {
            (source, source_offset, destination, destination_offset, size) => {
                if $crate::performance::enabled() {
                    $crate::performance::count("gpu_buffer_copy_calls", 1);
                    $crate::performance::count("gpu_copy_buffer_to_buffer_bytes", size);
                    $crate::performance::count("gpu_encoded_copy_payload_bytes", size);
                }
                let _timer = $crate::performance::time("gpu_copy_encode_host");
                $encoder.copy_buffer_to_buffer(
                    source,
                    source_offset,
                    destination,
                    destination_offset,
                    size,
                )
            }
        }
    }};
}
macro_rules! mapped_read {
    ($slice:expr $(,)?) => {{
        let result = $slice.get_mapped_range();
        if $crate::performance::enabled() {
            match &result {
                Ok(view) => {
                    $crate::performance::count("gpu_mapped_read_views", 1);
                    $crate::performance::count("gpu_mapped_read_view_bytes", view.len() as u64);
                }
                Err(_) => $crate::performance::count("gpu_mapped_read_view_failures", 1),
            }
        }
        result
    }};
}
#[cfg(test)]
pub(crate) use copy_texture_to_buffer;
pub(crate) use {
    copy_buffer_to_buffer, copy_buffer_to_texture, copy_texture_to_texture, mapped_read,
};
macro_rules! create_texture {
    ($device:expr, $desc:expr $(,)?) => {{
        match $desc {
            descriptor => {
                if !$crate::performance::enabled() {
                    $device.create_texture(descriptor)
                } else {
                    let timer = $crate::performance::time("gpu_texture_create_host");
                    let result = $device.create_texture(descriptor);
                    drop(timer);
                    $crate::gpu_metrics::texture_created(descriptor);
                    result
                }
            }
        }
    }};
}
macro_rules! create_texture_with_data {
    ($device:expr, $queue:expr, $desc:expr, $order:expr, $data:expr $(,)?) => {{
        match ($queue, $desc, $order, $data) {
            (queue, descriptor, order, data) => {
                if !$crate::performance::enabled() {
                    $device.create_texture_with_data(queue, descriptor, order, data)
                } else {
                    let timer = $crate::performance::time("gpu_texture_create_with_data_host");
                    let result = $device.create_texture_with_data(queue, descriptor, order, data);
                    drop(timer);
                    $crate::gpu_metrics::texture_created(descriptor);
                    $crate::performance::count("gpu_texture_initial_data_bytes", data.len() as u64);
                    $crate::performance::count(
                        "gpu_host_to_resource_payload_bytes",
                        data.len() as u64,
                    );
                    result
                }
            }
        }
    }};
}
macro_rules! create_buffer {
    ($device:expr, $desc:expr $(,)?) => {{
        match $desc {
            descriptor => {
                if !$crate::performance::enabled() {
                    $device.create_buffer(descriptor)
                } else {
                    let timer = $crate::performance::time("gpu_buffer_create_host");
                    let result = $device.create_buffer(descriptor);
                    drop(timer);
                    $crate::performance::count("gpu_buffer_creations", 1);
                    $crate::performance::count("gpu_buffer_created_capacity_bytes", result.size());
                    result
                }
            }
        }
    }};
}
macro_rules! create_buffer_init {
    ($device:expr, $desc:expr $(,)?) => {{
        match $desc {
            descriptor => {
                if !$crate::performance::enabled() {
                    $device.create_buffer_init(descriptor)
                } else {
                    let timer = $crate::performance::time("gpu_buffer_create_with_data_host");
                    let result = $device.create_buffer_init(descriptor);
                    drop(timer);
                    $crate::performance::count("gpu_buffer_creations", 1);
                    $crate::performance::count("gpu_buffer_created_capacity_bytes", result.size());
                    $crate::performance::count(
                        "gpu_buffer_initial_data_bytes",
                        descriptor.contents.len() as u64,
                    );
                    $crate::performance::count(
                        "gpu_host_to_resource_payload_bytes",
                        descriptor.contents.len() as u64,
                    );
                    result
                }
            }
        }
    }};
}
macro_rules! write_buffer {
    ($queue:expr, $buffer:expr, $offset:expr, $data:expr $(,)?) => {{
        match ($buffer, $offset, $data) {
            (buffer, offset, data) => {
                if !$crate::performance::enabled() {
                    $queue.write_buffer(buffer, offset, data)
                } else {
                    let timer = $crate::performance::time("gpu_buffer_write_host");
                    $queue.write_buffer(buffer, offset, data);
                    drop(timer);
                    $crate::performance::count("gpu_buffer_write_calls", 1);
                    $crate::performance::count("gpu_buffer_written_bytes", data.len() as u64);
                    $crate::performance::count(
                        "gpu_host_to_resource_payload_bytes",
                        data.len() as u64,
                    );
                }
            }
        }
    }};
}
macro_rules! write_texture {
    ($queue:expr, $destination:expr, $data:expr, $layout:expr, $size:expr $(,)?) => {{
        match ($destination, $data, $layout, $size) {
            (destination, data, layout, size) => {
                if !$crate::performance::enabled() {
                    $queue.write_texture(destination, data, layout, size)
                } else {
                    let timer = $crate::performance::time("gpu_texture_write_host");
                    $queue.write_texture(destination, data, layout, size);
                    drop(timer);
                    $crate::gpu_metrics::texture_written(&destination, size, data.len());
                }
            }
        }
    }};
}
pub(crate) use {
    create_buffer, create_buffer_init, create_texture, create_texture_with_data, write_buffer,
    write_texture,
};

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor(
        format: wgpu::TextureFormat,
        dimension: wgpu::TextureDimension,
        size: [u32; 3],
        mips: u32,
        samples: u32,
    ) -> wgpu::TextureDescriptor<'static> {
        wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: size[2],
            },
            mip_level_count: mips,
            sample_count: samples,
            dimension,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        }
    }
    #[test]
    fn texture_accounting_handles_mips_arrays_volumes_and_compressed_edges() {
        assert_eq!(
            texture_bytes(&descriptor(
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureDimension::D2,
                [8, 4, 1],
                u32::MAX,
                1
            )),
            None
        );
        assert_eq!(
            texture_bytes(&descriptor(
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureDimension::D2,
                [8, 4, 3],
                3,
                1
            )),
            Some((32 + 8 + 2) * 3 * 4)
        );
        assert_eq!(
            texture_bytes(&descriptor(
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureDimension::D3,
                [8, 4, 4],
                3,
                1
            )),
            Some((128 + 16 + 2) * 8)
        );
        assert_eq!(
            texture_bytes(&descriptor(
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureDimension::D2,
                [8, 4, 1],
                1,
                4
            )),
            Some(32 * 4 * 4)
        );
        assert_eq!(
            texture_bytes(&descriptor(
                wgpu::TextureFormat::Bc1RgbaUnorm,
                wgpu::TextureDimension::D2,
                [7, 5, 1],
                1,
                1
            )),
            Some(32)
        );
        assert_eq!(
            texture_bytes(&descriptor(
                wgpu::TextureFormat::Depth24PlusStencil8,
                wgpu::TextureDimension::D2,
                [8, 4, 1],
                1,
                1
            )),
            None
        );
        assert_eq!(
            texture_bytes(&descriptor(
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureDimension::D2,
                [u32::MAX; 3],
                1,
                1
            )),
            None
        );
    }
    #[test]
    fn production_wgpu_allocations_and_writes_use_accounted_entry_points() {
        fn inspect(directory: &std::path::Path) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    inspect(&path);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs")
                    || path
                        .file_name()
                        .is_some_and(|name| name == "gpu_metrics.rs" || name == "gpu_tests.rs")
                {
                    continue;
                }
                let source = std::fs::read_to_string(&path).unwrap();
                let production = source.split("#[cfg(test)]\nmod tests").next().unwrap();
                let normalized: String =
                    production.chars().filter(|c| !c.is_whitespace()).collect();
                for method in [
                    "create_buffer",
                    "create_buffer_init",
                    "create_texture",
                    "create_texture_with_data",
                    "write_buffer",
                    "write_texture",
                    "copy_buffer_to_buffer",
                    "copy_buffer_to_texture",
                    "copy_texture_to_texture",
                    "copy_texture_to_buffer",
                    "get_mapped_range",
                ] {
                    assert!(
                        !normalized.contains(&format!(".{method}(")),
                        "unaccounted {method} in {}",
                        path.display()
                    );
                }
            }
        }
        inspect(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    }
    #[test]
    #[ignore = "requires a GPU and LUMAPAINT_RENDER_METRICS=1"]
    fn gpu_resource_metrics_separate_texels_padding_and_initial_uploads() {
        use wgpu::util::DeviceExt;
        assert!(performance::enabled(), "enable LUMAPAINT_RENDER_METRICS=1");
        let (device, queue) = pollster::block_on(async {
            let adapter = wgpu::Instance::default()
                .request_adapter(&Default::default())
                .await
                .unwrap();
            adapter.request_device(&Default::default()).await.unwrap()
        });
        performance::take();
        let source = create_buffer_init!(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &[1, 2, 3, 4],
                usage: wgpu::BufferUsages::COPY_SRC
            }
        );
        let target = create_buffer!(
            device,
            &wgpu::BufferDescriptor {
                label: None,
                size: 16,
                usage: wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false
            }
        );
        write_buffer!(queue, &target, 0, &[9, 8, 7, 6]);
        let descriptor = |width, height| wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        };
        let initialized = create_texture_with_data!(
            device,
            &queue,
            &descriptor(4, 2),
            wgpu::util::TextureDataOrder::LayerMajor,
            &[0u8; 32]
        );
        let texture = create_texture!(device, &descriptor(257, 7));
        assert_eq!(cached_texture_capacity(), (0, 0, 0));
        let first = CachedTextureLease::new(&texture);
        let shared = CachedTextureLease::new(&texture.clone());
        assert_eq!(cached_texture_capacity(), (1, 7196, 0));
        let replacement = CachedTextureLease::new(&initialized);
        assert_eq!(cached_texture_capacity(), (2, 7228, 0));
        drop(first);
        assert_eq!(cached_texture_capacity(), (2, 7228, 0));
        std::thread::spawn(move || drop(shared)).join().unwrap();
        assert_eq!(cached_texture_capacity(), (1, 32, 0));
        drop(replacement);
        assert_eq!(cached_texture_capacity(), (0, 0, 0));
        assert!(CACHE_REGISTRY
            .get()
            .unwrap()
            .lock()
            .unwrap()
            .entries
            .is_empty());
        let mut pixels = vec![201; 1280 * 7];
        for row in pixels.as_chunks_mut::<1280>().0 {
            row[..257 * 4].fill(37);
        }
        write_texture!(
            queue,
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1280),
                rows_per_image: Some(7)
            },
            texture.size()
        );
        let counts = performance::take().counts;
        assert_eq!(counts["gpu_buffer_creations"], 2);
        assert_eq!(
            counts["gpu_buffer_created_capacity_bytes"],
            source.size() + target.size()
        );
        assert_eq!(counts["gpu_buffer_initial_data_bytes"], 4);
        assert_eq!(counts["gpu_buffer_write_calls"], 1);
        assert_eq!(counts["gpu_buffer_written_bytes"], 4);
        assert_eq!(counts["gpu_texture_creations"], 2);
        assert_eq!(counts["gpu_texture_created_logical_bytes"], 7196 + 32);
        assert_eq!(counts["gpu_texture_initial_data_bytes"], 32);
        assert_eq!(counts["gpu_texture_write_calls"], 1);
        assert_eq!(counts["gpu_texture_write_source_bytes"], 8960);
        assert_eq!(counts["gpu_texture_written_texel_bytes"], 7196);
        assert_eq!(counts["gpu_host_to_resource_payload_bytes"], 7236);
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 8960,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let upload = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &[37; 512],
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        copy_buffer_to_texture!(
            encoder,
            wgpu::TexelCopyBufferInfo {
                buffer: &upload,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(2),
                }
            },
            initialized.as_image_copy(),
            initialized.size()
        );
        copy_texture_to_texture!(
            encoder,
            initialized.as_image_copy(),
            texture.as_image_copy(),
            initialized.size()
        );
        copy_buffer_to_buffer!(encoder, &source, 0, &target, 0, 4);
        copy_texture_to_buffer!(
            encoder,
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(1280),
                    rows_per_image: Some(7),
                },
            },
            texture.size(),
        );
        queue.submit([encoder.finish()]);
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let output = mapped_read!(readback.slice(..)).unwrap();
        for row in output.as_chunks::<1280>().0 {
            assert!(row[..257 * 4].iter().all(|&byte| byte == 37));
        }
        drop(output);
        let copies = performance::take().counts;
        assert_eq!(copies["gpu_copy_buffer_to_texture_texel_bytes"], 32);
        assert_eq!(copies["gpu_copy_texture_to_texture_texel_bytes"], 32);
        assert_eq!(copies["gpu_copy_buffer_to_buffer_bytes"], 4);
        assert_eq!(copies["gpu_copy_texture_to_buffer_texel_bytes"], 7196);
        assert_eq!(copies["gpu_encoded_copy_payload_bytes"], 7264);
        assert_eq!(copies["gpu_mapped_read_views"], 1);
        assert_eq!(copies["gpu_mapped_read_view_bytes"], 8960);
        readback.unmap();
        drop(initialized);
    }
}
