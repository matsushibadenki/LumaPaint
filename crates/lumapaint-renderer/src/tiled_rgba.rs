//! Immutable, premultiplied RGBA snapshots for retained display composites.
//! Pixel document tiles use a different editing/history contract; these snapshots
//! share unchanged display tiles and never mutate another consumer's pixels.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct PixelLedger {
    live_bytes: AtomicU64,
    live_allocations: AtomicU64,
    allocated_bytes: AtomicU64,
    released_bytes: AtomicU64,
}

static PIXEL_LEDGER: std::sync::OnceLock<Arc<PixelLedger>> = std::sync::OnceLock::new();

struct PixelStorage {
    pixels: Vec<u8>,
    ledger: Option<Arc<PixelLedger>>,
}

impl PixelStorage {
    fn new(pixels: Vec<u8>) -> Self {
        let ledger = crate::performance::enabled()
            .then(|| Arc::clone(PIXEL_LEDGER.get_or_init(Default::default)));
        Self::with_ledger(pixels, ledger)
    }

    fn with_ledger(pixels: Vec<u8>, ledger: Option<Arc<PixelLedger>>) -> Self {
        if let Some(ledger) = &ledger {
            let bytes = pixels.capacity() as u64;
            ledger.live_bytes.fetch_add(bytes, Ordering::Relaxed);
            ledger.live_allocations.fetch_add(1, Ordering::Relaxed);
            ledger.allocated_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
        Self { pixels, ledger }
    }
}

impl Clone for PixelStorage {
    fn clone(&self) -> Self {
        Self::with_ledger(self.pixels.clone(), self.ledger.clone())
    }
}

impl std::ops::Deref for PixelStorage {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.pixels
    }
}

impl std::ops::DerefMut for PixelStorage {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }
}

impl Drop for PixelStorage {
    fn drop(&mut self) {
        if let Some(ledger) = &self.ledger {
            let bytes = self.pixels.capacity() as u64;
            ledger.live_bytes.fetch_sub(bytes, Ordering::Relaxed);
            ledger.live_allocations.fetch_sub(1, Ordering::Relaxed);
            ledger.released_bytes.fetch_add(bytes, Ordering::Relaxed);
        }
    }
}

/// Process-wide tile Vec capacities; excludes Arc headers and allocator overhead.
/// Fields are sampled independently and may span concurrent worker updates.
pub(crate) fn pixel_capacity() -> (u64, u64, u64, u64) {
    PIXEL_LEDGER.get().map_or((0, 0, 0, 0), |ledger| {
        (
            ledger.live_allocations.load(Ordering::Relaxed),
            ledger.live_bytes.load(Ordering::Relaxed),
            ledger.allocated_bytes.load(Ordering::Relaxed),
            ledger.released_bytes.load(Ordering::Relaxed),
        )
    })
}

pub const TILE_EDGE: u32 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Region {
    fn intersection(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (x < right && y < bottom).then(|| Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
    fn bytes(self) -> Result<usize, String> {
        (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| "Invalid RGBA region size".into())
    }
}

#[derive(Clone)]
struct Tile {
    bounds: Region,
    pixels: Arc<PixelStorage>,
}

#[derive(Clone)]
pub struct TiledRgba {
    width: u32,
    height: u32,
    tiles: Vec<Tile>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PatchStats {
    pub tiles_examined: usize,
    pub tiles_changed: usize,
    pub copied_pixel_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RebuildStats {
    pub shared_tiles: usize,
    pub rendered_tiles: usize,
    pub rendered_pixel_bytes: usize,
}

/// Borrowed bytes contain row gaps; upload only `bounds.width` pixels per row.
pub struct TileRows<'a> {
    pub bounds: Region,
    pub stride: u32,
    pub pixels: &'a [u8],
}

impl TiledRgba {
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Self, String> {
        let canvas = Region {
            x: 0,
            y: 0,
            width,
            height,
        };
        if width == 0 || height == 0 || canvas.bytes()? != rgba.len() {
            return Err("Invalid RGBA snapshot dimensions or payload".into());
        }
        Self::from_regions(width, height, |bounds| {
            let mut pixels = Vec::with_capacity(bounds.bytes()?);
            for dy in 0..bounds.height {
                let start = (((bounds.y + dy) as usize * width as usize) + bounds.x as usize) * 4;
                pixels.extend_from_slice(&rgba[start..start + bounds.width as usize * 4]);
            }
            Ok(pixels)
        })
    }

    /// Build one tile at a time without allocating a contiguous canvas.
    pub(crate) fn from_regions(
        width: u32,
        height: u32,
        mut render: impl FnMut(Region) -> Result<Vec<u8>, String>,
    ) -> Result<Self, String> {
        let canvas = Region {
            x: 0,
            y: 0,
            width,
            height,
        };
        if width == 0 || height == 0 {
            return Err("Invalid RGBA snapshot dimensions".into());
        }
        canvas.bytes()?;
        let mut tiles = Vec::new();
        for row in 0..height.div_ceil(TILE_EDGE) {
            for column in 0..width.div_ceil(TILE_EDGE) {
                let x = column * TILE_EDGE;
                let y = row * TILE_EDGE;
                let bounds = Region {
                    x,
                    y,
                    width: TILE_EDGE.min(width - x),
                    height: TILE_EDGE.min(height - y),
                };
                let pixels = render(bounds)?;
                if pixels.len() != bounds.bytes()? {
                    return Err("Invalid rendered RGBA tile payload".into());
                }
                tiles.push(Tile {
                    bounds,
                    pixels: Arc::new(PixelStorage::new(pixels)),
                });
            }
        }
        Ok(Self {
            width,
            height,
            tiles,
        })
    }

    /// Reuse exact global tile coverage outside the dirty region across crop changes.
    /// Changed tile shapes/alignment are rendered afresh; old snapshots stay immutable.
    pub(crate) fn rebuilt(
        &self,
        width: u32,
        height: u32,
        shift: [i64; 2],
        dirty: Option<Region>,
        mut render: impl FnMut(Region) -> Result<Vec<u8>, String>,
    ) -> Result<(Self, RebuildStats), String> {
        use std::collections::HashMap;
        if width == 0 || height == 0 {
            return Err("Invalid RGBA snapshot dimensions".into());
        }
        Region {
            x: 0,
            y: 0,
            width,
            height,
        }
        .bytes()?;
        if let Some(region) = dirty {
            if region.width == 0
                || region.height == 0
                || region.x.checked_add(region.width).is_none_or(|n| n > width)
                || region
                    .y
                    .checked_add(region.height)
                    .is_none_or(|n| n > height)
            {
                return Err("Invalid rebuilt RGBA dirty region".into());
            }
        }
        let previous: HashMap<_, _> = self
            .tiles
            .iter()
            .filter_map(|tile| {
                let x = i64::from(tile.bounds.x).checked_add(shift[0])?;
                let y = i64::from(tile.bounds.y).checked_add(shift[1])?;
                let x = u32::try_from(x).ok()?;
                let y = u32::try_from(y).ok()?;
                Some(((x, y, tile.bounds.width, tile.bounds.height), tile))
            })
            .collect();
        let mut tiles = Vec::new();
        let mut stats = RebuildStats::default();
        for row in 0..height.div_ceil(TILE_EDGE) {
            for column in 0..width.div_ceil(TILE_EDGE) {
                let x = column * TILE_EDGE;
                let y = row * TILE_EDGE;
                let bounds = Region {
                    x,
                    y,
                    width: TILE_EDGE.min(width - x),
                    height: TILE_EDGE.min(height - y),
                };
                let old = previous.get(&(x, y, bounds.width, bounds.height));
                let pixels = if let Some(tile) =
                    old.filter(|_| dirty.is_none_or(|d| bounds.intersection(d).is_none()))
                {
                    stats.shared_tiles += 1;
                    Arc::clone(&tile.pixels)
                } else {
                    let pixels = render(bounds)?;
                    if pixels.len() != bounds.bytes()? {
                        return Err("Invalid rendered RGBA tile payload".into());
                    }
                    stats.rendered_tiles += 1;
                    stats.rendered_pixel_bytes += pixels.len();
                    if let Some(tile) = old.filter(|tile| tile.pixels.pixels == pixels) {
                        stats.shared_tiles += 1;
                        Arc::clone(&tile.pixels)
                    } else {
                        Arc::new(PixelStorage::new(pixels))
                    }
                };
                tiles.push(Tile { bounds, pixels });
            }
        }
        Ok((
            Self {
                width,
                height,
                tiles,
            },
            stats,
        ))
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn validate(&self, region: Region) -> Result<(), String> {
        if region.width == 0
            || region.height == 0
            || region
                .x
                .checked_add(region.width)
                .is_none_or(|end| end > self.width)
            || region
                .y
                .checked_add(region.height)
                .is_none_or(|end| end > self.height)
        {
            return Err("RGBA patch is outside snapshot".into());
        }
        Ok(())
    }

    /// Replace encoded pixels, including transparent pixels, without blending.
    /// Invalid input returns before cloning or modifying any tile.
    pub fn patched(&self, region: Region, rgba: &[u8]) -> Result<(Self, PatchStats), String> {
        self.validate(region)?;
        if region.bytes()? != rgba.len() {
            return Err("Invalid RGBA patch payload".into());
        }
        let mut result = self.clone();
        let mut stats = PatchStats::default();
        for tile in &mut result.tiles {
            let Some(part) = tile.bounds.intersection(region) else {
                continue;
            };
            stats.tiles_examined += 1;
            let row_offsets = |dy: u32| {
                let source = (((part.y - region.y + dy) as usize * region.width as usize)
                    + (part.x - region.x) as usize)
                    * 4;
                let target = (((part.y - tile.bounds.y + dy) as usize
                    * tile.bounds.width as usize)
                    + (part.x - tile.bounds.x) as usize)
                    * 4;
                (source, target)
            };
            let length = part.width as usize * 4;
            let identical = (0..part.height).all(|dy| {
                let (source, target) = row_offsets(dy);
                rgba[source..source + length] == tile.pixels[target..target + length]
            });
            if identical {
                continue;
            }
            // The result shares each tile with `self`, so only changed tiles clone.
            stats.copied_pixel_bytes += tile.pixels.len();
            stats.tiles_changed += 1;
            let pixels = Arc::make_mut(&mut tile.pixels);
            for dy in 0..part.height {
                let (source, target) = row_offsets(dy);
                pixels[target..target + length].copy_from_slice(&rgba[source..source + length]);
            }
        }
        Ok((result, stats))
    }

    pub fn borrowed_rows(&self, region: Region) -> Result<Vec<TileRows<'_>>, String> {
        self.validate(region)?;
        Ok(self
            .tiles
            .iter()
            .filter_map(|tile| {
                let part = tile.bounds.intersection(region)?;
                let stride = tile.bounds.width * 4;
                let offset = (((part.y - tile.bounds.y) * tile.bounds.width)
                    + (part.x - tile.bounds.x)) as usize
                    * 4;
                let end =
                    offset + (part.height - 1) as usize * stride as usize + part.width as usize * 4;
                Some(TileRows {
                    bounds: part,
                    stride,
                    pixels: &tile.pixels[offset..end],
                })
            })
            .collect())
    }

    /// Upload borrowed tile rows without materializing a contiguous image.
    /// All destination checks complete before the first queue write.
    pub fn upload_region(
        &self,
        queue: &wgpu::Queue,
        target: &wgpu::Texture,
        region: Region,
        destination: [u32; 2],
    ) -> Result<usize, String> {
        self.validate(region)?;
        if !target.usage().contains(wgpu::TextureUsages::COPY_DST)
            || !matches!(
                target.format(),
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
            )
            || target.sample_count() != 1
            || target.dimension() != wgpu::TextureDimension::D2
            || destination[0]
                .checked_add(region.width)
                .is_none_or(|end| end > target.width())
            || destination[1]
                .checked_add(region.height)
                .is_none_or(|end| end > target.height())
        {
            return Err("Invalid tiled RGBA upload target".into());
        }
        let rows = self.borrowed_rows(region)?;
        for row in rows {
            crate::gpu_metrics::write_texture!(
                queue,
                wgpu::TexelCopyTextureInfo {
                    texture: target,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: destination[0] + row.bounds.x - region.x,
                        y: destination[1] + row.bounds.y - region.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                row.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row.stride),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: row.bounds.width,
                    height: row.bounds.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        region.bytes()
    }

    /// Materialize only when an API requires a contiguous image.
    pub fn materialize(&self) -> Vec<u8> {
        let mut pixels = vec![0; self.payload_bytes()];
        for tile in &self.tiles {
            for dy in 0..tile.bounds.height {
                let target = (((tile.bounds.y + dy) as usize * self.width as usize)
                    + tile.bounds.x as usize)
                    * 4;
                let source = dy as usize * tile.bounds.width as usize * 4;
                let length = tile.bounds.width as usize * 4;
                pixels[target..target + length]
                    .copy_from_slice(&tile.pixels[source..source + length]);
            }
        }
        pixels
    }

    /// Logical snapshot payload, including tiles shared with other snapshots.
    pub fn payload_bytes(&self) -> usize {
        self.tiles.iter().map(|tile| tile.pixels.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_capacity_counts_sharing_copy_on_write_and_cross_thread_release() {
        let ledger = Arc::new(PixelLedger::default());
        let mut pixels = Vec::with_capacity(128);
        pixels.resize(64, 0);
        let original = Arc::new(PixelStorage::with_ledger(pixels, Some(ledger.clone())));
        let mut shared = original.clone();
        assert_eq!(ledger.live_bytes.load(Ordering::Relaxed), 128);
        assert_eq!(ledger.live_allocations.load(Ordering::Relaxed), 1);
        Arc::make_mut(&mut shared)[0] = 42;
        let copied_capacity = shared.pixels.capacity() as u64;
        assert_eq!(original[0], 0);
        assert_eq!(
            ledger.live_bytes.load(Ordering::Relaxed),
            128 + copied_capacity
        );
        assert_eq!(ledger.live_allocations.load(Ordering::Relaxed), 2);
        std::thread::spawn(move || drop(original)).join().unwrap();
        assert_eq!(ledger.live_bytes.load(Ordering::Relaxed), copied_capacity);
        drop(shared);
        assert_eq!(ledger.live_bytes.load(Ordering::Relaxed), 0);
        assert_eq!(ledger.live_allocations.load(Ordering::Relaxed), 0);
        assert_eq!(
            ledger.allocated_bytes.load(Ordering::Relaxed),
            ledger.released_bytes.load(Ordering::Relaxed)
        );
    }

    #[test]
    fn untracked_pixel_storage_keeps_copy_on_write_untracked() {
        let original = Arc::new(PixelStorage::with_ledger(vec![1, 2], None));
        let mut shared = original.clone();
        Arc::make_mut(&mut shared)[0] = 3;
        assert!(shared.ledger.is_none());
        assert_eq!(original[0], 1);
    }

    #[test]
    fn resized_snapshots_share_aligned_tiles_and_render_changed_alignment() {
        let old = TiledRgba::from_rgba(512, 256, &[10, 20, 30, 100].repeat(512 * 256)).unwrap();
        let saved = old.materialize();
        let (expanded, stats) = old
            .rebuilt(
                768,
                384,
                [128, 128],
                Some(Region {
                    x: 0,
                    y: 0,
                    width: 128,
                    height: 384,
                }),
                |r| {
                    let mut pixels = Vec::new();
                    for y in r.y..r.y + r.height {
                        for x in r.x..r.x + r.width {
                            pixels.extend_from_slice(if (128..640).contains(&x) && y >= 128 {
                                &[10, 20, 30, 100]
                            } else {
                                &[80, 40, 20, 128]
                            });
                        }
                    }
                    Ok(pixels)
                },
            )
            .unwrap();
        assert_eq!(stats.shared_tiles, 8);
        assert_eq!(stats.rendered_tiles, 10);
        assert!(Arc::ptr_eq(&old.tiles[0].pixels, &expanded.tiles[7].pixels));
        let pixels = expanded.materialize();
        for y in 0..384 {
            for x in 0..768 {
                let start = (y * 768 + x) * 4;
                let expected = if (128..640).contains(&x) && y >= 128 {
                    [10, 20, 30, 100]
                } else {
                    [80, 40, 20, 128]
                };
                assert_eq!(pixels[start..start + 4], expected);
            }
        }
        let (contracted, stats) = old
            .rebuilt(256, 128, [-128, -128], None, |_| {
                panic!("aligned retained tiles must be reused")
            })
            .unwrap();
        assert_eq!(stats.shared_tiles, 2);
        assert_eq!(
            contracted.materialize(),
            [10, 20, 30, 100].repeat(256 * 128)
        );
        let (_, stats) = old
            .rebuilt(256, 128, [1, 1], None, |r| Ok(vec![0; r.bytes()?]))
            .unwrap();
        assert_eq!(stats.shared_tiles, 0);
        assert_eq!(stats.rendered_tiles, 2);
        assert!(old
            .rebuilt(
                1,
                1,
                [0, 0],
                Some(Region {
                    x: u32::MAX,
                    y: 0,
                    width: 2,
                    height: 1
                }),
                |_| panic!("invalid region must not render")
            )
            .is_err());
        assert!(old.rebuilt(1, 1, [0, 0], None, |_| Ok(vec![])).is_err());
        assert_eq!(old.materialize(), saved);
    }

    #[test]
    fn region_builder_matches_full_image_and_rejects_invalid_tiles() {
        let (width, height) = (259, 131);
        let pixel = |x: u32, y: u32| [x as u8, y as u8, (x + y) as u8, 128];
        let expected: Vec<_> = (0..height)
            .flat_map(|y| (0..width).flat_map(move |x| pixel(x, y)))
            .collect();
        let mut largest = 0;
        let image = TiledRgba::from_regions(width, height, |region| {
            let pixels: Vec<_> = (region.y..region.y + region.height)
                .flat_map(|y| (region.x..region.x + region.width).flat_map(move |x| pixel(x, y)))
                .collect();
            largest = largest.max(pixels.len());
            Ok(pixels)
        })
        .unwrap();
        assert_eq!(image.materialize(), expected);
        assert_eq!(largest, (TILE_EDGE * TILE_EDGE * 4) as usize);
        assert!(TiledRgba::from_regions(1, 1, |_| Ok(vec![0; 3])).is_err());
        assert!(TiledRgba::from_regions(1, 1, |_| Err("render failure".into())).is_err());
        assert!(TiledRgba::from_regions(0, 1, |_| panic!("invalid size must not render")).is_err());
    }

    #[test]
    fn boundary_patch_matches_contiguous_image_and_preserves_snapshot() {
        let (width, height) = (513, 259);
        let original: Vec<_> = (0..width * height * 4).map(|n| (n % 251) as u8).collect();
        let image = TiledRgba::from_rgba(width, height, &original).unwrap();
        assert_eq!(image.materialize(), original);
        let region = Region {
            x: 127,
            y: 127,
            width: 3,
            height: 3,
        };
        let patch = vec![0; 36];
        let (edited, stats) = image.patched(region, &patch).unwrap();
        let mut expected = original.clone();
        for y in 127..130 {
            let offset = (y * width as usize + 127) * 4;
            expected[offset..offset + 12].fill(0);
        }
        assert_eq!(edited.materialize(), expected);
        assert_eq!(image.materialize(), original);
        assert_eq!(stats.tiles_changed, 4);
        assert_eq!(stats.copied_pixel_bytes, 4 * 128 * 128 * 4);
        for (old, new) in image.tiles.iter().zip(&edited.tiles) {
            assert_eq!(
                Arc::ptr_eq(&old.pixels, &new.pixels),
                old.bounds.intersection(region).is_none()
            );
        }
        let (unchanged, stats) = edited.patched(region, &patch).unwrap();
        assert_eq!(stats.tiles_changed, 0);
        assert_eq!(stats.copied_pixel_bytes, 0);
        assert!(edited
            .tiles
            .iter()
            .zip(&unchanged.tiles)
            .all(|(a, b)| Arc::ptr_eq(&a.pixels, &b.pixels)));
    }

    #[test]
    fn edge_tiles_and_borrowed_strides_reconstruct_patch_exactly() {
        let (width, height) = (259, 131);
        let pixels: Vec<_> = (0..width * height * 4).map(|n| (n % 253) as u8).collect();
        let image = TiledRgba::from_rgba(width, height, &pixels).unwrap();
        for region in [
            Region {
                x: 126,
                y: 126,
                width: 133,
                height: 5,
            },
            Region {
                x: 258,
                y: 130,
                width: 1,
                height: 1,
            },
        ] {
            let mut output = vec![0; region.bytes().unwrap()];
            for row in image.borrowed_rows(region).unwrap() {
                for dy in 0..row.bounds.height {
                    let target = (((row.bounds.y - region.y + dy) * region.width) + row.bounds.x
                        - region.x) as usize
                        * 4;
                    let source = dy as usize * row.stride as usize;
                    let length = row.bounds.width as usize * 4;
                    output[target..target + length]
                        .copy_from_slice(&row.pixels[source..source + length]);
                }
            }
            for dy in 0..region.height {
                let offset = (((region.y + dy) * width) + region.x) as usize * 4;
                assert_eq!(
                    &output[dy as usize * region.width as usize * 4
                        ..(dy + 1) as usize * region.width as usize * 4],
                    &pixels[offset..offset + region.width as usize * 4]
                );
            }
        }
        assert!(image
            .patched(
                Region {
                    x: 258,
                    y: 130,
                    width: 2,
                    height: 1
                },
                &[0; 8]
            )
            .is_err());
        assert!(image
            .patched(
                Region {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1
                },
                &[0; 3]
            )
            .is_err());
        assert!(image
            .borrowed_rows(Region {
                x: u32::MAX,
                y: 0,
                width: 2,
                height: 1
            })
            .is_err());
        assert_eq!(image.materialize(), pixels);
    }

    #[test]
    fn one_pixel_edit_copies_one_tile_instead_of_whole_large_image() {
        let pixels = vec![128; 2048 * 2048 * 4];
        let image = TiledRgba::from_rgba(2048, 2048, &pixels).unwrap();
        let (_, stats) = image
            .patched(
                Region {
                    x: 1024,
                    y: 1024,
                    width: 1,
                    height: 1,
                },
                &[0; 4],
            )
            .unwrap();
        assert_eq!(stats.tiles_changed, 1);
        assert_eq!(stats.copied_pixel_bytes, 65536);
        assert_eq!(image.payload_bytes(), 16777216);
    }
}
