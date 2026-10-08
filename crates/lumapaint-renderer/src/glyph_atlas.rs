//! Bounded, renderer-owned coverage atlas. Shaping and text layout remain upstream.
//! Used by the opt-in glyph display pilot; default text display remains quality-gated.
use resvg::tiny_skia;
use std::collections::HashMap;

/// Exact outline/raster identity: no font-name or hash-only equality assumptions.
/// Build once per shaped glyph, then retain across placement/color changes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    outline: Vec<u32>,
    transform: [u32; 6],
    size: [u32; 2],
    even_odd: bool,
}
impl GlyphKey {
    pub fn new(
        path: &tiny_skia::Path,
        transform: tiny_skia::Transform,
        size: [u32; 2],
        even_odd: bool,
    ) -> Option<Self> {
        let matrix = [
            transform.sx,
            transform.ky,
            transform.kx,
            transform.sy,
            transform.tx,
            transform.ty,
        ];
        if size.contains(&0)
            || size.iter().any(|&s| s > 512)
            || !matrix.iter().all(|v| v.is_finite())
        {
            return None;
        }
        let mut outline = Vec::new();
        for segment in path.segments() {
            use tiny_skia::PathSegment::*;
            let (tag, points): (u32, Vec<tiny_skia::Point>) = match segment {
                MoveTo(p) => (0, vec![p]),
                LineTo(p) => (1, vec![p]),
                QuadTo(a, b) => (2, vec![a, b]),
                CubicTo(a, b, c) => (3, vec![a, b, c]),
                Close => (4, vec![]),
            };
            outline.push(tag);
            for p in points {
                outline.extend([p.x.to_bits(), p.y.to_bits()]);
            }
            // Bound CPU identity storage as well as texture capacity.
            if outline.len() > 4096 {
                return None;
            }
        }
        Some(Self {
            outline,
            transform: matrix.map(f32::to_bits),
            size,
            even_odd,
        })
    }

    /// Rasterize the already shaped outline with the compatibility coverage engine.
    /// Color and opacity are deliberately absent; they belong to glyph instances.
    pub fn rasterize(&self) -> Option<Vec<u8>> {
        let mut builder = tiny_skia::PathBuilder::new();
        let mut i = 0;
        while i < self.outline.len() {
            let tag = self.outline[i];
            i += 1;
            let mut point = || {
                let p = (
                    f32::from_bits(self.outline[i]),
                    f32::from_bits(self.outline[i + 1]),
                );
                i += 2;
                p
            };
            match tag {
                0 => {
                    let (x, y) = point();
                    builder.move_to(x, y);
                }
                1 => {
                    let (x, y) = point();
                    builder.line_to(x, y);
                }
                2 => {
                    let (x, y) = point();
                    let (a, b) = point();
                    builder.quad_to(x, y, a, b);
                }
                3 => {
                    let (x, y) = point();
                    let (a, b) = point();
                    let (c, d) = point();
                    builder.cubic_to(x, y, a, b, c, d);
                }
                4 => builder.close(),
                _ => return None,
            }
        }
        let path = builder.finish()?;
        let m = self.transform.map(f32::from_bits);
        let mut pixmap = tiny_skia::Pixmap::new(self.size[0], self.size[1])?;
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(255, 255, 255, 255);
        pixmap.fill_path(
            &path,
            &paint,
            if self.even_odd {
                tiny_skia::FillRule::EvenOdd
            } else {
                tiny_skia::FillRule::Winding
            },
            tiny_skia::Transform::from_row(m[0], m[1], m[2], m[3], m[4], m[5]),
            None,
        );
        Some(pixmap.pixels().iter().map(|p| p.alpha()).collect())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphSlot {
    /// Changes on explicit reset. Never use instances from a previous generation.
    pub generation: u64,
    /// Content rectangle; a transparent texel gutter surrounds every slot.
    pub rectangle: [u32; 4],
}
#[derive(Debug, PartialEq, Eq)]
pub enum AtlasError {
    Capacity,
    InvalidMask,
    GenerationExhausted,
}

/// A single R8 page, up to 4 MiB. Capacity failures leave resident slots intact.
/// Reset must occur only when the caller rebuilds all referencing instance buffers.
pub struct GlyphAtlas {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    _lease: crate::gpu_metrics::CachedTextureLease,
    side: u32,
    generation: u64,
    entries: HashMap<GlyphKey, GlyphSlot>,
    key_bytes: usize,
    cursor: [u32; 2],
    row_height: u32,
}
impl GlyphAtlas {
    pub fn new(device: &wgpu::Device, side: u32) -> Option<Self> {
        if !(4..=2048).contains(&side) || side > device.limits().max_texture_dimension_2d {
            return None;
        }
        let texture = crate::gpu_metrics::create_texture!(
            device,
            &wgpu::TextureDescriptor {
                label: Some("retained glyph coverage atlas"),
                size: wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }
        );
        let view = texture.create_view(&Default::default());
        let lease = crate::gpu_metrics::CachedTextureLease::new(&texture);
        Some(Self {
            texture,
            view,
            _lease: lease,
            side,
            generation: 1,
            entries: HashMap::new(),
            key_bytes: 0,
            cursor: [0, 0],
            row_height: 0,
        })
    }
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }
    pub fn logical_texture_bytes(&self) -> u64 {
        u64::from(self.side).pow(2)
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn reset(&mut self) -> Result<(), AtlasError> {
        let next = self
            .generation
            .checked_add(1)
            .ok_or(AtlasError::GenerationExhausted)?;
        self.entries.clear();
        self.key_bytes = 0;
        self.cursor = [0, 0];
        self.row_height = 0;
        self.generation = next;
        crate::performance::count("glyph_atlas_resets", 1);
        Ok(())
    }
    /// Return a retained glyph, rasterizing/uploading its mask only on a miss.
    pub fn ensure(&mut self, queue: &wgpu::Queue, key: &GlyphKey) -> Result<GlyphSlot, AtlasError> {
        self.get_or_insert(queue, key, || key.rasterize())
    }

    // Failure does not alter packing/cache state. Injectable rasterization is
    // private so callers cannot associate different coverage with an exact key.
    fn get_or_insert(
        &mut self,
        queue: &wgpu::Queue,
        key: &GlyphKey,
        raster: impl FnOnce() -> Option<Vec<u8>>,
    ) -> Result<GlyphSlot, AtlasError> {
        if let Some(&slot) = self.entries.get(key) {
            crate::performance::count("glyph_atlas_hits", 1);
            return Ok(slot);
        }
        crate::performance::count("glyph_atlas_misses", 1);
        let charge = key.outline.len() * 4 + std::mem::size_of::<GlyphKey>();
        if self.entries.len() >= 16384 || self.key_bytes + charge > 4 * 1024 * 1024 {
            return Err(AtlasError::Capacity);
        }
        let size = [key.size[0] + 2, key.size[1] + 2];
        let (origin, row_height) =
            pack(self.side, self.cursor, self.row_height, size).ok_or(AtlasError::Capacity)?;
        let _timer = crate::performance::time("glyph_atlas_miss_raster_upload");
        let alpha = raster().ok_or(AtlasError::InvalidMask)?;
        if alpha.len() != (key.size[0] * key.size[1]) as usize {
            return Err(AtlasError::InvalidMask);
        }
        let mut padded = vec![0u8; (size[0] * size[1]) as usize];
        for (y, row) in alpha.chunks_exact(key.size[0] as usize).enumerate() {
            let start = (y + 1) * size[0] as usize + 1;
            padded[start..start + row.len()].copy_from_slice(row);
        }
        crate::gpu_metrics::write_texture!(
            queue,
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0
                },
                aspect: wgpu::TextureAspect::All
            },
            &padded,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0]),
                rows_per_image: Some(size[1])
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1
            }
        );
        crate::performance::count("glyph_atlas_uploaded_bytes", padded.len() as u64);
        let slot = GlyphSlot {
            generation: self.generation,
            rectangle: [origin[0] + 1, origin[1] + 1, key.size[0], key.size[1]],
        };
        self.cursor = [origin[0] + size[0], origin[1]];
        self.row_height = row_height;
        self.key_bytes += charge;
        self.entries.insert(key.clone(), slot);
        Ok(slot)
    }
}
fn pack(side: u32, cursor: [u32; 2], row_height: u32, size: [u32; 2]) -> Option<([u32; 2], u32)> {
    if size[0] > side || size[1] > side {
        return None;
    }
    let mut origin = cursor;
    let mut height = row_height;
    if origin[0] + size[0] > side {
        origin = [0, origin[1] + height];
        height = 0;
    }
    if origin[1] + size[1] > side {
        return None;
    }
    Some((origin, height.max(size[1])))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(phase: f32) -> GlyphKey {
        let path =
            tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(1., 1., 4., 5.).unwrap());
        GlyphKey::new(
            &path,
            tiny_skia::Transform::from_translate(phase, 0.),
            [8, 8],
            false,
        )
        .unwrap()
    }
    #[test]
    fn raster_identity_and_coverage() {
        assert_eq!(key(0.), key(0.));
        assert_ne!(key(0.), key(0.25));
        let a = key(0.).rasterize().unwrap();
        assert_eq!(a.iter().filter(|&&v| v == 255).count(), 20);
        let b = key(0.25).rasterize().unwrap();
        assert_ne!(a, b);
        assert!(b.iter().any(|&v| v > 0 && v < 255));
    }
    #[test]
    fn packing_checks_bounds_and_wraps_without_overlap() {
        assert_eq!(pack(16, [10, 0], 10, [10, 6]), Some(([0, 10], 6)));
        assert_eq!(pack(16, [10, 10], 6, [10, 6]), None);
        assert_eq!(pack(16, [0, 0], 0, [17, 1]), None);
    }
    #[test]
    #[ignore = "requires a real GPU"]
    fn gpu_retains_masks_and_invalidates_slots_on_reset() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let mut atlas = GlyphAtlas::new(&device, 20).unwrap();
        let k = key(0.);
        let slot = atlas.get_or_insert(&queue, &k, || k.rasterize()).unwrap();
        assert_eq!(
            atlas
                .get_or_insert(&queue, &k, || panic!("warm glyph must not rasterize"))
                .unwrap(),
            slot
        );
        // Invalid data and capacity failures must preserve the first resident entry.
        let other = key(0.25);
        assert_eq!(
            atlas.get_or_insert(&queue, &other, || Some(vec![])),
            Err(AtlasError::InvalidMask)
        );
        assert_eq!(atlas.cursor, [10, 0]);
        assert_eq!(atlas.entries.len(), 1);
        assert_eq!(
            atlas.get_or_insert(
                &queue,
                &GlyphKey {
                    size: [20, 20],
                    ..other.clone()
                },
                || panic!("capacity must be checked before rasterization")
            ),
            Err(AtlasError::Capacity)
        );
        assert_eq!(atlas.get_or_insert(&queue, &k, || panic!()).unwrap(), slot);
        atlas.reset().unwrap();
        let next = atlas.get_or_insert(&queue, &k, || k.rasterize()).unwrap();
        assert_ne!(slot.generation, next.generation);
        assert_eq!(slot.rectangle, next.rectangle);
        assert_eq!(atlas.logical_texture_bytes(), 400);
        // Test-only readback verifies coverage and zero gutters after reuse/reset.
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 20,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            atlas.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(20),
                },
            },
            wgpu::Extent3d {
                width: 20,
                height: 20,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, |r| r.unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let pixels = buffer.slice(..).get_mapped_range().unwrap();
        let expected = k.rasterize().unwrap();
        for y in 0..10usize {
            for x in 0..10usize {
                let reference = if x == 0 || y == 0 || x == 9 || y == 9 {
                    0
                } else {
                    expected[(y - 1) * 8 + x - 1]
                };
                assert_eq!(pixels[y * 256 + x], reference);
            }
        }
        drop(pixels);
        buffer.unmap();
    }
}
