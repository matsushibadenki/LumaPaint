//! Sparse RGBA8 raster tiles. A fully zero tile has no allocation.
use crate::document::{mask_factor, LayerSettings, Point};
use crate::selection::Selection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const TILE_SIZE: u32 = 256;
const TILE_BYTES: usize = TILE_SIZE as usize * TILE_SIZE as usize * 4;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TileCoord {
    pub x: u32,
    pub y: u32,
}

/// Validated, history-free state shared by the live tiled document, recovery,
/// and the future binary project container. Pixel payloads remain raw here;
/// compression and checksums belong to the container boundary.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TiledRasterState {
    pub width: u32,
    pub height: u32,
    /// Physical pixel density, independent from canvas pixels and renderer scale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<RasterResolution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icc_profile: Option<Vec<u8>>,
    pub layers: Vec<RasterLayerState>,
}

/// Conservative bounds and header validation for retained RGB ICC profiles.
pub fn validate_rgb_icc(profile: &[u8]) -> Result<(), String> {
    if profile.len() < 132
        || profile.len() > 128 * 1024
        || &profile[36..40] != b"acsp"
        || &profile[16..20] != b"RGB "
    {
        return Err("Invalid or oversized RGB ICC profile".into());
    }
    let length = u32::from_be_bytes(profile[..4].try_into().expect("ICC header")) as usize;
    if length != profile.len() {
        return Err("Invalid ICC profile length".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RasterResolution {
    pub x_ppi: f64,
    pub y_ppi: f64,
    pub x_display: ResolutionDisplayUnit,
    pub y_display: ResolutionDisplayUnit,
    pub width_display: PrintDimensionUnit,
    pub height_display: PrintDimensionUnit,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionDisplayUnit {
    PixelsPerInch,
    PixelsPerCentimeter,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PrintDimensionUnit {
    Inches,
    Centimeters,
    Points,
    Picas,
    Columns,
}
impl RasterResolution {
    pub fn validate(&self) -> Result<(), String> {
        if [self.x_ppi, self.y_ppi]
            .iter()
            .any(|ppi| !ppi.is_finite() || !(1. / 65536. ..=32767.99998474121).contains(ppi))
        {
            return Err("Invalid raster resolution".into());
        }
        Ok(())
    }
    pub fn size_inches(&self, width: u32, height: u32) -> [f64; 2] {
        [
            f64::from(width) / self.x_ppi,
            f64::from(height) / self.y_ppi,
        ]
    }
}

/// Separable RGB blend modes; independent from any renderer or file format.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RasterBlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    HardLight,
    Darken,
    Lighten,
    Difference,
    Exclusion,
}
impl RasterBlendMode {
    fn blend(self, back: f64, source: f64) -> f64 {
        match self {
            Self::Normal => source,
            Self::Multiply => back * source,
            Self::Screen => back + source - back * source,
            Self::Overlay => {
                if back <= 0.5 {
                    2. * back * source
                } else {
                    1. - 2. * (1. - back) * (1. - source)
                }
            }
            Self::HardLight => {
                if source <= 0.5 {
                    2. * back * source
                } else {
                    1. - 2. * (1. - back) * (1. - source)
                }
            }
            Self::Darken => back.min(source),
            Self::Lighten => back.max(source),
            Self::Difference => (back - source).abs(),
            Self::Exclusion => back + source - 2. * back * source,
        }
    }
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RasterLayerState {
    #[serde(default)]
    pub effects: crate::layer_effects::LayerEffects,
    #[serde(default)]
    pub blend_mode: RasterBlendMode,
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub locked: bool,
    pub alpha_locked: bool,
    #[serde(default)]
    pub mask_present: bool,
    #[serde(default = "crate::layer_mask::default_linked")]
    pub mask_linked: bool,
    pub mask_enabled: bool,
    pub mask_inverted: bool,
    pub mask_density: f32,
    pub tiles: Vec<RasterTileState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off_canvas_tiles: Vec<OffCanvasTileState>,
    pub mask_tiles: Vec<MaskTileState>,
}

/// Composite validated snapshot layers with the same CPU kernel as the live document.
/// This borrows pixel payloads; import verification does not clone a full document.
pub fn composite_raster_layers_tile(
    layers: &[RasterLayerState],
    coord: TileCoord,
) -> Option<Vec<u8>> {
    composite_sources(
        coord,
        layers.iter().filter_map(|layer| {
            if !layer.visible || layer.opacity <= 0.0 {
                return None;
            }
            Some(CompositeSource {
                effects: &layer.effects,
                blend_mode: layer.blend_mode,
                pixels: &layer.tiles.iter().find(|tile| tile.coord == coord)?.pixels,
                opacity: layer.opacity,
                mask: layer
                    .mask_tiles
                    .iter()
                    .find(|tile| tile.coord == coord)
                    .map(|tile| tile.values.as_slice()),
                mask_enabled: layer.mask_enabled,
                mask_inverted: layer.mask_inverted,
                mask_density: layer.mask_density,
            })
        }),
    )
}
struct CompositeSource<'a> {
    effects: &'a crate::layer_effects::LayerEffects,
    blend_mode: RasterBlendMode,
    pixels: &'a [u8],
    opacity: f32,
    mask: Option<&'a [u8]>,
    mask_enabled: bool,
    mask_inverted: bool,
    mask_density: f32,
}
fn composite_sources<'a>(
    coord: TileCoord,
    sources: impl Iterator<Item = CompositeSource<'a>>,
) -> Option<Vec<u8>> {
    let sources: Vec<_> = sources.collect();
    if sources.is_empty() {
        return None;
    }
    // Overlay depends on the unassociated backdrop color. Quantizing each
    // intermediate premultiplied layer can shift that color by multiple steps
    // when its alpha is low, so retain precision until the final tile output.
    // Keep the existing integer path for documents without overlay.
    if sources
        .iter()
        .any(|s| s.blend_mode == RasterBlendMode::Overlay)
    {
        let prepared: Vec<_> = sources
            .iter()
            .map(|s| s.effects.active().then(|| s.effects.prepare()))
            .collect();
        let mut result = vec![0u8; TILE_BYTES];
        for (index, target) in result.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let mut dst = [0f64; 3];
            let mut da = 0f64;
            for (source, effects) in sources.iter().zip(&prepared) {
                let px: [u8; 4] = source.pixels[index * 4..index * 4 + 4].try_into().unwrap();
                let px = effects.as_ref().map_or(px, |e| {
                    e.apply_at(
                        px,
                        [
                            (coord.x * TILE_SIZE + index as u32 % TILE_SIZE) as f32 + 0.5,
                            (coord.y * TILE_SIZE + index as u32 / TILE_SIZE) as f32 + 0.5,
                        ],
                    )
                });
                let mask_value = source.mask.map_or(255, |tile| tile[index]);
                let coverage = source.mask_density * f32::from(mask_value) / 255.;
                let mask = if !source.mask_enabled {
                    1.
                } else if source.mask_inverted {
                    1. - coverage
                } else {
                    coverage
                };
                let a = f64::from((f32::from(px[3]) * (source.opacity * mask)).round()) / 255.;
                if a == 0. {
                    continue;
                }
                if a == 1. && da == 1. {
                    for c in 0..3 {
                        dst[c] = source.blend_mode.blend(dst[c], f64::from(px[c]) / 255.);
                    }
                    continue;
                }
                if da == 0. {
                    for c in 0..3 {
                        dst[c] = if a == 1. {
                            f64::from(px[c]) / 255.
                        } else {
                            a * (f64::from(px[c]) / 255.)
                        };
                    }
                    da = a;
                    continue;
                }
                let inv_da = 1. / da;
                let remaining = 1. - a;
                for c in 0..3 {
                    let cs = f64::from(px[c]) / 255.;
                    let cb = dst[c] * inv_da;
                    dst[c] = remaining * dst[c]
                        + a * ((1. - da) * cs + da * source.blend_mode.blend(cb, cs));
                }
                da = a + da * remaining;
            }
            for c in 0..3 {
                target[c] = (dst[c] * 255.).round().clamp(0., 255.) as u8;
            }
            target[3] = (da * 255.).round().clamp(0., 255.) as u8;
        }
        return result.iter().any(|b| *b != 0).then_some(result);
    }
    let mut result = vec![0u8; TILE_BYTES];
    for source in sources {
        let effects = source.effects.active().then(|| source.effects.prepare());
        for (index, (target, pixel)) in result
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(source.pixels.as_chunks::<4>().0.iter())
            .enumerate()
        {
            let adjusted = effects.as_ref().map_or(*pixel, |e| {
                e.apply_at(
                    *pixel,
                    [
                        (coord.x * TILE_SIZE + index as u32 % TILE_SIZE) as f32 + 0.5,
                        (coord.y * TILE_SIZE + index as u32 / TILE_SIZE) as f32 + 0.5,
                    ],
                )
            });
            let pixel = &adjusted;
            let mask_value = source.mask.map_or(255, |tile| tile[index]);
            let coverage = source.mask_density * f32::from(mask_value) / 255.0;
            let mask = if !source.mask_enabled {
                1.0
            } else if source.mask_inverted {
                1.0 - coverage
            } else {
                coverage
            };
            let opacity = source.opacity * mask;
            let alpha = (f32::from(pixel[3]) * opacity).round() as u32;
            if alpha == 0 {
                continue;
            }
            let remaining = 255 - alpha;
            let out_alpha = (alpha + div_255(u32::from(target[3]) * remaining)).min(255);
            for channel in 0..3 {
                let foreground = div_255(u32::from(pixel[channel]) * alpha);
                let background = div_255(u32::from(target[channel]) * remaining);
                target[channel] = if source.blend_mode == RasterBlendMode::Normal || target[3] == 0
                {
                    (foreground + background).min(out_alpha) as u8
                } else {
                    // W3C source-over: uncovered source + blended overlap + uncovered backdrop.
                    let ab = f64::from(target[3]) / 255.;
                    let asrc = f64::from(alpha) / 255.;
                    let cb_premult = f64::from(target[channel]) / 255.;
                    let cb = if ab > 0. { cb_premult / ab } else { 0. };
                    let cs = f64::from(pixel[channel]) / 255.;
                    (((1. - asrc) * cb_premult
                        + asrc * ((1. - ab) * cs + ab * source.blend_mode.blend(cb, cs)))
                        * 255.)
                        .round()
                        .clamp(0., f64::from(out_alpha)) as u8
                };
            }
            target[3] = out_alpha as u8;
        }
    }
    result.iter().any(|byte| *byte != 0).then_some(result)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RasterTileState {
    pub coord: TileCoord,
    pub pixels: Vec<u8>,
}

/// Raw straight RGBA for a signed tile outside the canvas. Bytes within the
/// canvas must be zero; edge tiles can contain data on both sides of an edge.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OffCanvasTileState {
    pub x: i32,
    pub y: i32,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaskTileState {
    pub coord: TileCoord,
    pub values: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TileChange {
    pub coord: TileCoord,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// A mask tile is absent when every pixel is fully visible (255).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SparseMask {
    width: u32,
    height: u32,
    #[serde(with = "tile_pairs")]
    tiles: BTreeMap<TileCoord, Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct MaskChange {
    coord: TileCoord,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

impl SparseMask {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            tiles: BTreeMap::new(),
        }
    }

    pub fn allocated_tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let coord = TileCoord {
            x: x / TILE_SIZE,
            y: y / TILE_SIZE,
        };
        let index = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize;
        Some(self.tiles.get(&coord).map_or(255, |tile| tile[index]))
    }

    /// Translate allocated mask pixels; uncovered areas remain fully visible.
    pub fn translated(&self, dx: i32, dy: i32) -> Self {
        let mut result = Self::new(self.width, self.height);
        for (coord, tile) in &self.tiles {
            for (index, &value) in tile.iter().enumerate() {
                if value == 255 {
                    continue;
                }
                let x = i64::from(coord.x * TILE_SIZE) + (index as i64 % i64::from(TILE_SIZE));
                let y = i64::from(coord.y * TILE_SIZE) + (index as i64 / i64::from(TILE_SIZE));
                if x >= i64::from(self.width) || y >= i64::from(self.height) {
                    continue;
                }
                let (x, y) = (x + i64::from(dx), y + i64::from(dy));
                if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
                    continue;
                }
                let (x, y) = (x as u32, y as u32);
                let dest = TileCoord {
                    x: x / TILE_SIZE,
                    y: y / TILE_SIZE,
                };
                let offset = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize;
                result
                    .tiles
                    .entry(dest)
                    .or_insert_with(|| vec![255; (TILE_SIZE * TILE_SIZE) as usize])[offset] = value;
            }
        }
        result
    }

    /// Transform deviations from the sparse mask's fully visible default.
    pub fn transformed(&self, matrix: [f32; 6]) -> Result<Self, String> {
        let mut pixels = SparseTiles::new(self.width, self.height)?;
        for (coord, values) in &self.tiles {
            let mut data = vec![0; TILE_BYTES];
            for (idx, value) in values.iter().enumerate() {
                data[idx * 4 + 3] = 255 - value;
            }
            pixels.tiles.insert(*coord, data);
        }
        let moved = pixels.transformed(matrix)?;
        let mut mask = Self::new(self.width, self.height);
        for (coord, pixels) in moved.tiles {
            let mut values = vec![255; (TILE_SIZE * TILE_SIZE) as usize];
            for (idx, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                values[idx] = 255 - pixel[3];
            }
            if values.iter().any(|v| *v != 255) {
                mask.tiles.insert(coord, values);
            }
        }
        Ok(mask)
    }

    fn write_rect(&mut self, rect: [u32; 4], values: &[u8]) -> Result<Vec<MaskChange>, String> {
        let [x, y, width, height] = rect;
        let end_x = x.checked_add(width).ok_or("Mask rectangle overflow")?;
        let end_y = y.checked_add(height).ok_or("Mask rectangle overflow")?;
        let expected = (width as usize)
            .checked_mul(height as usize)
            .ok_or("Mask rectangle overflow")?;
        if width == 0 || height == 0 || end_x > self.width || end_y > self.height {
            return Err("Mask rectangle outside canvas".into());
        }
        if values.len() != expected {
            return Err("Incorrect mask rectangle length".into());
        }
        let mut changes = Vec::new();
        for tile_y in y / TILE_SIZE..=(end_y - 1) / TILE_SIZE {
            for tile_x in x / TILE_SIZE..=(end_x - 1) / TILE_SIZE {
                let coord = TileCoord {
                    x: tile_x,
                    y: tile_y,
                };
                let before = self.tiles.get(&coord).cloned();
                let mut after = before
                    .clone()
                    .unwrap_or_else(|| vec![255; (TILE_SIZE * TILE_SIZE) as usize]);
                let left = x.max(tile_x * TILE_SIZE);
                let top = y.max(tile_y * TILE_SIZE);
                let right = end_x.min((tile_x + 1) * TILE_SIZE);
                let bottom = end_y.min((tile_y + 1) * TILE_SIZE);
                for row in top..bottom {
                    let source = ((row - y) * width + left - x) as usize;
                    let target = ((row % TILE_SIZE) * TILE_SIZE + left % TILE_SIZE) as usize;
                    let len = (right - left) as usize;
                    after[target..target + len].copy_from_slice(&values[source..source + len]);
                }
                let after = after.iter().any(|value| *value != 255).then_some(after);
                if before == after {
                    continue;
                }
                match &after {
                    Some(bytes) => {
                        self.tiles.insert(coord, bytes.clone());
                    }
                    None => {
                        self.tiles.remove(&coord);
                    }
                }
                changes.push(MaskChange {
                    coord,
                    before,
                    after,
                });
            }
        }
        Ok(changes)
    }

    fn paint_dabs(
        &mut self,
        dabs: &[RasterDab],
        brush: crate::document::Brush,
        selection: Option<&Selection>,
    ) -> Result<Vec<MaskChange>, String> {
        let density = dab_density(self.width, self.height, dabs, selection)?;
        let mut changes = Vec::new();
        for (coord, coverage) in density {
            let before = self.tiles.get(&coord).cloned();
            let mut after = before
                .clone()
                .unwrap_or_else(|| vec![255; (TILE_SIZE * TILE_SIZE) as usize]);
            for (index, (pixel, depth)) in after.iter_mut().zip(coverage).enumerate() {
                let amount = (1.0 - (-depth).exp()) * brush.opacity * brush.alpha;
                *pixel = crate::brush_blend::composite_gray(
                    *pixel,
                    brush.color[0],
                    amount,
                    brush.blend_mode,
                    [
                        coord.x * TILE_SIZE + index as u32 % TILE_SIZE,
                        coord.y * TILE_SIZE + index as u32 / TILE_SIZE,
                    ],
                );
            }
            let after = after.iter().any(|value| *value != 255).then_some(after);
            if before == after {
                continue;
            }
            match &after {
                Some(bytes) => {
                    self.tiles.insert(coord, bytes.clone());
                }
                None => {
                    self.tiles.remove(&coord);
                }
            }
            changes.push(MaskChange {
                coord,
                before,
                after,
            });
        }
        Ok(changes)
    }

    fn apply_changes(&mut self, changes: &[MaskChange], undo: bool) -> Result<(), String> {
        let mut previous = None;
        for change in changes {
            let current = if undo { &change.after } else { &change.before };
            if change.coord.x >= self.width.div_ceil(TILE_SIZE)
                || change.coord.y >= self.height.div_ceil(TILE_SIZE)
                || previous.is_some_and(|coord| coord >= (change.coord.y, change.coord.x))
                || [change.before.as_ref(), change.after.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|bytes| {
                        bytes.len() != (TILE_SIZE * TILE_SIZE) as usize
                            || bytes.iter().all(|value| *value == 255)
                            || bytes.iter().enumerate().any(|(index, value)| {
                                let x = change.coord.x * TILE_SIZE + index as u32 % TILE_SIZE;
                                let y = change.coord.y * TILE_SIZE + index as u32 / TILE_SIZE;
                                (x >= self.width || y >= self.height) && *value != 255
                            })
                    })
                || self.tiles.get(&change.coord) != current.as_ref()
            {
                return Err("Invalid mask change".into());
            }
            previous = Some((change.coord.y, change.coord.x));
        }
        for change in changes {
            let value = if undo { &change.before } else { &change.after };
            match value {
                Some(bytes) => {
                    self.tiles.insert(change.coord, bytes.clone());
                }
                None => {
                    self.tiles.remove(&change.coord);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SparseTiles {
    width: u32,
    height: u32,
    #[serde(with = "tile_pairs")]
    tiles: BTreeMap<TileCoord, Vec<u8>>,
}

impl SparseTiles {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("Invalid tile canvas dimensions".into());
        }
        Ok(Self {
            width,
            height,
            tiles: BTreeMap::new(),
        })
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn allocated_tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn tile(&self, coord: TileCoord) -> Option<&[u8]> {
        self.tiles.get(&coord).map(Vec::as_slice)
    }

    pub fn allocated_coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
        self.tiles.keys().copied()
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let coord = TileCoord {
            x: x / TILE_SIZE,
            y: y / TILE_SIZE,
        };
        let Some(tile) = self.tiles.get(&coord) else {
            return Some([0; 4]);
        };
        let index = (((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4) as usize;
        Some(tile[index..index + 4].try_into().unwrap())
    }

    /// Translate allocated pixels, clipping to the canvas and preserving sparsity.
    pub fn translated(&self, dx: i32, dy: i32) -> Self {
        let mut result = Self {
            width: self.width,
            height: self.height,
            tiles: BTreeMap::new(),
        };
        for (coord, tile) in &self.tiles {
            for (index, pixel) in tile.as_chunks::<4>().0.iter().enumerate() {
                if *pixel == [0, 0, 0, 0] {
                    continue;
                }
                let x = i64::from(coord.x * TILE_SIZE) + (index as i64 % i64::from(TILE_SIZE));
                let y = i64::from(coord.y * TILE_SIZE) + (index as i64 / i64::from(TILE_SIZE));
                if x >= i64::from(self.width) || y >= i64::from(self.height) {
                    continue;
                }
                let (x, y) = (x + i64::from(dx), y + i64::from(dy));
                if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
                    continue;
                }
                let (x, y) = (x as u32, y as u32);
                let dest = TileCoord {
                    x: x / TILE_SIZE,
                    y: y / TILE_SIZE,
                };
                let offset = (((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4) as usize;
                result
                    .tiles
                    .entry(dest)
                    .or_insert_with(|| vec![0; TILE_BYTES])[offset..offset + 4]
                    .copy_from_slice(pixel);
            }
        }
        result
    }

    /// Translate canvas and signed outside tiles together without resampling.
    pub fn translated_with_off_canvas(
        &self,
        outside: &[OffCanvasTileState],
        dx: i32,
        dy: i32,
    ) -> Result<(Self, Vec<OffCanvasTileState>), String> {
        let mut result = Self::new(self.width, self.height)?;
        let mut overflow = BTreeMap::<(i32, i32), Vec<u8>>::new();
        let edge = i64::from(TILE_SIZE);
        let mut move_tile = |tx: i64, ty: i64, bytes: &[u8]| -> Result<(), String> {
            if bytes.len() != TILE_BYTES {
                return Err("Invalid raster tile".into());
            }
            for (index, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
                if *pixel == [0; 4] {
                    continue;
                }
                let x = tx * edge + (index as i64 % edge) + i64::from(dx);
                let y = ty * edge + (index as i64 / edge) + i64::from(dy);
                if x < i64::from(i32::MIN)
                    || y < i64::from(i32::MIN)
                    || x >= i64::from(i32::MAX)
                    || y >= i64::from(i32::MAX)
                {
                    return Err("Transformed raster coordinates exceed supported range".into());
                }
                let offset = (y.rem_euclid(edge) * edge + x.rem_euclid(edge)) as usize * 4;
                if x >= 0 && y >= 0 && x < i64::from(self.width) && y < i64::from(self.height) {
                    let coord = TileCoord {
                        x: x as u32 / TILE_SIZE,
                        y: y as u32 / TILE_SIZE,
                    };
                    result
                        .tiles
                        .entry(coord)
                        .or_insert_with(|| vec![0; TILE_BYTES])[offset..offset + 4]
                        .copy_from_slice(pixel);
                } else {
                    let key = (x.div_euclid(edge) as i32, y.div_euclid(edge) as i32);
                    if !overflow.contains_key(&key) && overflow.len() >= 2048 {
                        return Err("Too many off-canvas tiles".into());
                    }
                    overflow.entry(key).or_insert_with(|| vec![0; TILE_BYTES])[offset..offset + 4]
                        .copy_from_slice(pixel);
                }
            }
            Ok(())
        };
        for (coord, bytes) in &self.tiles {
            move_tile(i64::from(coord.x), i64::from(coord.y), bytes)?;
        }
        for tile in outside {
            move_tile(i64::from(tile.x), i64::from(tile.y), &tile.pixels)?;
        }
        Ok((
            result,
            overflow
                .into_iter()
                .map(|((x, y), pixels)| OffCanvasTileState { x, y, pixels })
                .collect(),
        ))
    }

    /// Transform signed raster tiles with premultiplied bilinear sampling.
    /// The work limit covers both canvas and off-canvas destination tiles.
    pub fn transformed_with_off_canvas(
        &self,
        outside: &[OffCanvasTileState],
        matrix: [f32; 6],
    ) -> Result<(Self, Vec<OffCanvasTileState>), String> {
        crate::layer_mask::validate_transform(matrix)?;
        if matrix[..4] == [1., 0., 0., 1.]
            && matrix[4].fract() == 0.
            && matrix[5].fract() == 0.
            && matrix[4] >= i32::MIN as f32
            && matrix[4] < 2147483648.
            && matrix[5] >= i32::MIN as f32
            && matrix[5] < 2147483648.
        {
            return self.translated_with_off_canvas(outside, matrix[4] as i32, matrix[5] as i32);
        }
        validate_off_canvas_tiles(outside, self.width, self.height)?;
        if outside
            .iter()
            .map(|t| &t.pixels)
            .chain(self.tiles.values())
            .any(|pixels| {
                pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|p| p[3] == 0 && *p != [0; 4])
            })
        {
            return Err("Cannot resample hidden transparent RGB / 透明画素のRGBを保持した補間には未対応です / 暂不支持保留透明像素RGB的重采样".into());
        }
        let inverse = crate::image_frame::inverse(matrix)?;
        let edge = i64::from(TILE_SIZE);
        let mut candidates = BTreeSet::<(i32, i32)>::new();
        let sources = self
            .tiles
            .keys()
            .map(|c| (i64::from(c.x), i64::from(c.y)))
            .chain(outside.iter().map(|t| (i64::from(t.x), i64::from(t.y))));
        for (tx, ty) in sources {
            let x0 = (tx * edge) as f64 - 1.;
            let y0 = (ty * edge) as f64 - 1.;
            let x1 = ((tx + 1) * edge) as f64 + 1.;
            let y1 = ((ty + 1) * edge) as f64 + 1.;
            let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].map(|(x, y)| {
                (
                    f64::from(matrix[0]) * x + f64::from(matrix[2]) * y + f64::from(matrix[4]),
                    f64::from(matrix[1]) * x + f64::from(matrix[3]) * y + f64::from(matrix[5]),
                )
            });
            let min_x = corners
                .iter()
                .map(|p| p.0)
                .fold(f64::INFINITY, f64::min)
                .floor();
            let max_x = corners
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil();
            let min_y = corners
                .iter()
                .map(|p| p.1)
                .fold(f64::INFINITY, f64::min)
                .floor();
            let max_y = corners
                .iter()
                .map(|p| p.1)
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil();
            if min_x < i32::MIN as f64
                || min_y < i32::MIN as f64
                || max_x >= i32::MAX as f64
                || max_y >= i32::MAX as f64
            {
                return Err("Transformed raster coordinates exceed supported range".into());
            }
            let (x0, x1, y0, y1) = (
                (min_x as i64).div_euclid(edge),
                (max_x as i64).div_euclid(edge),
                (min_y as i64).div_euclid(edge),
                (max_y as i64).div_euclid(edge),
            );
            if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > 2048 {
                return Err("Transformed raster exceeds tile limit".into());
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    candidates.insert((x as i32, y as i32));
                    if candidates.len() > 2048 {
                        return Err("Transformed raster exceeds tile limit".into());
                    }
                }
            }
        }
        let outside_map: BTreeMap<_, _> = outside
            .iter()
            .map(|t| ((t.x, t.y), t.pixels.as_slice()))
            .collect();
        let sample = |x: i64, y: i64| -> [u8; 4] {
            if x < i64::from(i32::MIN)
                || y < i64::from(i32::MIN)
                || x >= i64::from(i32::MAX)
                || y >= i64::from(i32::MAX)
            {
                return [0; 4];
            }
            let offset = (y.rem_euclid(edge) * edge + x.rem_euclid(edge)) as usize * 4;
            let bytes =
                if x >= 0 && y >= 0 && x < i64::from(self.width) && y < i64::from(self.height) {
                    self.tiles
                        .get(&TileCoord {
                            x: x as u32 / TILE_SIZE,
                            y: y as u32 / TILE_SIZE,
                        })
                        .map(Vec::as_slice)
                } else {
                    outside_map
                        .get(&(x.div_euclid(edge) as i32, y.div_euclid(edge) as i32))
                        .copied()
                };
            bytes.map_or([0; 4], |b| b[offset..offset + 4].try_into().expect("RGBA"))
        };
        let mut result = Self::new(self.width, self.height)?;
        let mut overflow = Vec::new();
        for (tx, ty) in candidates {
            let mut inside = vec![0; TILE_BYTES];
            let mut out = vec![0; TILE_BYTES];
            let (mut has_inside, mut has_out) = (false, false);
            for iy in 0..edge {
                for ix in 0..edge {
                    let x = i64::from(tx) * edge + ix;
                    let y = i64::from(ty) * edge + iy;
                    let px = x as f64 + 0.5;
                    let py = y as f64 + 0.5;
                    let sx = f64::from(inverse[0]) * px
                        + f64::from(inverse[2]) * py
                        + f64::from(inverse[4])
                        - 0.5;
                    let sy = f64::from(inverse[1]) * px
                        + f64::from(inverse[3]) * py
                        + f64::from(inverse[5])
                        - 0.5;
                    let bx = sx.floor() as i64;
                    let by = sy.floor() as i64;
                    let fx = sx - sx.floor();
                    let fy = sy - sy.floor();
                    let mut alpha = 0.;
                    let mut rgb = [0.; 3];
                    for (dx, dy, w) in [
                        (0, 0, (1. - fx) * (1. - fy)),
                        (1, 0, fx * (1. - fy)),
                        (0, 1, (1. - fx) * fy),
                        (1, 1, fx * fy),
                    ] {
                        let p = sample(bx.saturating_add(dx), by.saturating_add(dy));
                        let a = f64::from(p[3]) * w;
                        alpha += a;
                        for c in 0..3 {
                            rgb[c] += f64::from(p[c]) * a;
                        }
                    }
                    let a = alpha.round().clamp(0., 255.) as u8;
                    if a == 0 {
                        continue;
                    }
                    let offset = (iy * edge + ix) as usize * 4;
                    let in_canvas =
                        x >= 0 && y >= 0 && x < i64::from(self.width) && y < i64::from(self.height);
                    let target = if in_canvas {
                        has_inside = true;
                        &mut inside
                    } else {
                        has_out = true;
                        &mut out
                    };
                    for c in 0..3 {
                        target[offset + c] = (rgb[c] / alpha).round().clamp(0., 255.) as u8;
                    }
                    target[offset + 3] = a;
                }
            }
            if has_inside {
                result.tiles.insert(
                    TileCoord {
                        x: tx as u32,
                        y: ty as u32,
                    },
                    inside,
                );
            }
            if has_out {
                overflow.push(OffCanvasTileState {
                    x: tx,
                    y: ty,
                    pixels: out,
                });
            }
        }
        Ok((result, overflow))
    }

    /// Affine resampling in straight-alpha storage, interpolating premultiplied colors.
    pub fn transformed(&self, matrix: [f32; 6]) -> Result<Self, String> {
        crate::layer_mask::validate_transform(matrix)?;
        if matrix == [1., 0., 0., 1., 0., 0.] {
            return Ok(self.clone());
        }
        if matrix[..4] == [1., 0., 0., 1.]
            && matrix[4].fract() == 0.
            && matrix[5].fract() == 0.
            && matrix[4] >= i32::MIN as f32
            && matrix[4] < 2147483648.
            && matrix[5] >= i32::MIN as f32
            && matrix[5] < 2147483648.
        {
            return Ok(self.translated(matrix[4] as i32, matrix[5] as i32));
        }
        if self.tiles.values().any(|pixels| {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] == 0 && *p != [0; 4])
        }) {
            return Err("Cannot resample hidden transparent RGB / 透明画素のRGBを保持した補間には未対応です / 暂不支持保留透明像素RGB的重采样".into());
        }
        let inv = crate::image_frame::inverse(matrix)?;
        let mut candidates = BTreeSet::new();
        for coord in self.allocated_coords() {
            let x0 = coord.x * TILE_SIZE;
            let y0 = coord.y * TILE_SIZE;
            let x1 = (x0 + TILE_SIZE).min(self.width);
            let y1 = (y0 + TILE_SIZE).min(self.height);
            let points = [
                (x0 as f64 - 1., y0 as f64 - 1.),
                (x1 as f64 + 1., y0 as f64 - 1.),
                (x0 as f64 - 1., y1 as f64 + 1.),
                (x1 as f64 + 1., y1 as f64 + 1.),
            ];
            let mapped = points.map(|(x, y)| {
                (
                    f64::from(matrix[0]) * x + f64::from(matrix[2]) * y + f64::from(matrix[4]),
                    f64::from(matrix[1]) * x + f64::from(matrix[3]) * y + f64::from(matrix[5]),
                )
            });
            let min_x = mapped
                .iter()
                .map(|p| p.0)
                .fold(f64::INFINITY, f64::min)
                .floor()
                .clamp(0., self.width as f64) as u32;
            let max_x = mapped
                .iter()
                .map(|p| p.0)
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .clamp(0., self.width as f64) as u32;
            let min_y = mapped
                .iter()
                .map(|p| p.1)
                .fold(f64::INFINITY, f64::min)
                .floor()
                .clamp(0., self.height as f64) as u32;
            let max_y = mapped
                .iter()
                .map(|p| p.1)
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .clamp(0., self.height as f64) as u32;
            if min_x >= max_x || min_y >= max_y {
                continue;
            }
            for y in min_y / TILE_SIZE..=(max_y - 1) / TILE_SIZE {
                for x in min_x / TILE_SIZE..=(max_x - 1) / TILE_SIZE {
                    candidates.insert(TileCoord { x, y });
                }
            }
        }
        let mut result = Self::new(self.width, self.height)?;
        for coord in candidates {
            let mut tile = vec![0; TILE_BYTES];
            for y in coord.y * TILE_SIZE..((coord.y + 1) * TILE_SIZE).min(self.height) {
                for x in coord.x * TILE_SIZE..((coord.x + 1) * TILE_SIZE).min(self.width) {
                    let px = x as f64 + 0.5;
                    let py = y as f64 + 0.5;
                    let sx =
                        f64::from(inv[0]) * px + f64::from(inv[2]) * py + f64::from(inv[4]) - 0.5;
                    let sy =
                        f64::from(inv[1]) * px + f64::from(inv[3]) * py + f64::from(inv[5]) - 0.5;
                    let ix = sx.floor() as i64;
                    let iy = sy.floor() as i64;
                    let fx = sx - sx.floor();
                    let fy = sy - sy.floor();
                    let mut alpha = 0.;
                    let mut rgb = [0.; 3];
                    for (ox, oy, w) in [
                        (0, 0, (1. - fx) * (1. - fy)),
                        (1, 0, fx * (1. - fy)),
                        (0, 1, (1. - fx) * fy),
                        (1, 1, fx * fy),
                    ] {
                        let (xx, yy) = (ix + ox, iy + oy);
                        if xx < 0
                            || yy < 0
                            || xx >= i64::from(self.width)
                            || yy >= i64::from(self.height)
                        {
                            continue;
                        }
                        let sample = self.pixel(xx as u32, yy as u32).unwrap();
                        let a = f64::from(sample[3]) * w;
                        alpha += a;
                        for c in 0..3 {
                            rgb[c] += f64::from(sample[c]) * a;
                        }
                    }
                    let offset = (((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) * 4) as usize;
                    let out_alpha = alpha.round().clamp(0., 255.) as u8;
                    if out_alpha != 0 {
                        for c in 0..3 {
                            tile[offset + c] = (rgb[c] / alpha).round().clamp(0., 255.) as u8;
                        }
                        tile[offset + 3] = out_alpha;
                    }
                }
            }
            if tile.iter().any(|v| *v != 0) {
                result.tiles.insert(coord, tile);
            }
        }
        Ok(result)
    }

    /// Replace a rectangle of straight-alpha RGBA8 pixels; return only changed tiles.
    /// The result can be applied backward for Undo and forward for Redo.
    pub fn write_rect(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<TileChange>, String> {
        let end_x = x.checked_add(width).ok_or("Tile rectangle overflow")?;
        let end_y = y.checked_add(height).ok_or("Tile rectangle overflow")?;
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|count| count.checked_mul(4))
            .ok_or("Tile rectangle overflow")?;
        if width == 0 || height == 0 || end_x > self.width || end_y > self.height {
            return Err("Tile rectangle outside canvas".into());
        }
        if pixels.len() != expected {
            return Err("Incorrect RGBA rectangle length".into());
        }
        let mut changes = Vec::new();
        for tile_y in y / TILE_SIZE..=(end_y - 1) / TILE_SIZE {
            for tile_x in x / TILE_SIZE..=(end_x - 1) / TILE_SIZE {
                let coord = TileCoord {
                    x: tile_x,
                    y: tile_y,
                };
                let before = self.tiles.get(&coord).cloned();
                let mut after = before.clone().unwrap_or_else(|| vec![0; TILE_BYTES]);
                let left = x.max(tile_x * TILE_SIZE);
                let top = y.max(tile_y * TILE_SIZE);
                let right = end_x.min((tile_x + 1) * TILE_SIZE);
                let bottom = end_y.min((tile_y + 1) * TILE_SIZE);
                for row in top..bottom {
                    let source = (((row - y) * width + left - x) * 4) as usize;
                    let target = (((row % TILE_SIZE) * TILE_SIZE + left % TILE_SIZE) * 4) as usize;
                    let length = ((right - left) * 4) as usize;
                    after[target..target + length]
                        .copy_from_slice(&pixels[source..source + length]);
                }
                let after = after.iter().any(|byte| *byte != 0).then_some(after);
                if before == after {
                    continue;
                }
                match &after {
                    Some(bytes) => {
                        self.tiles.insert(coord, bytes.clone());
                    }
                    None => {
                        self.tiles.remove(&coord);
                    }
                }
                changes.push(TileChange {
                    coord,
                    before,
                    after,
                });
            }
        }
        Ok(changes)
    }

    /// Accumulate optical density across a single stroke, then composite once.
    /// This keeps self-crossings from becoming separate opacity layers.
    pub fn paint_dabs(
        &mut self,
        dabs: &[RasterDab],
        color: [u8; 3],
    ) -> Result<Vec<TileChange>, String> {
        self.paint_dabs_clipped(dabs, color, None)
    }

    pub fn paint_dabs_clipped(
        &mut self,
        dabs: &[RasterDab],
        color: [u8; 3],
        selection: Option<&Selection>,
    ) -> Result<Vec<TileChange>, String> {
        self.paint_dabs_with_alpha_lock(
            dabs,
            crate::document::Brush {
                color,
                ..Default::default()
            },
            selection,
            false,
            None,
        )
    }

    fn paint_dabs_with_alpha_lock(
        &mut self,
        dabs: &[RasterDab],
        brush: crate::document::Brush,
        selection: Option<&Selection>,
        alpha_locked: bool,
        targets: Option<&BTreeSet<TileCoord>>,
    ) -> Result<Vec<TileChange>, String> {
        let density =
            dab_density_on_tiles(self.width, self.height, dabs, selection, None, targets)?;
        let mut changes = Vec::new();
        for (coord, coverage) in density {
            let before = self.tiles.get(&coord).cloned();
            let mut after = before.clone().unwrap_or_else(|| vec![0; TILE_BYTES]);
            for (index, (pixel, depth)) in after
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(coverage)
                .enumerate()
            {
                let amount = (1.0 - (-depth).exp()) * brush.opacity * brush.alpha;
                *pixel = crate::brush_blend::composite(
                    *pixel,
                    brush.color,
                    amount,
                    brush.blend_mode,
                    alpha_locked,
                    [
                        coord.x * TILE_SIZE + index as u32 % TILE_SIZE,
                        coord.y * TILE_SIZE + index as u32 / TILE_SIZE,
                    ],
                );
            }
            let after = after.iter().any(|byte| *byte != 0).then_some(after);
            if before == after {
                continue;
            }
            match &after {
                Some(bytes) => {
                    self.tiles.insert(coord, bytes.clone());
                }
                None => {
                    self.tiles.remove(&coord);
                }
            }
            changes.push(TileChange {
                coord,
                before,
                after,
            });
        }
        changes.sort_by_key(|change| (change.coord.y, change.coord.x));
        Ok(changes)
    }

    pub fn apply_changes(&mut self, changes: &[TileChange], undo: bool) -> Result<(), String> {
        // Validate the entire batch before changing any tile.
        let mut previous = None;
        for change in changes {
            let current = if undo { &change.after } else { &change.before };
            if change.coord.x >= self.width.div_ceil(TILE_SIZE)
                || change.coord.y >= self.height.div_ceil(TILE_SIZE)
                || previous.is_some_and(|coord| coord >= (change.coord.y, change.coord.x))
                || [change.before.as_ref(), change.after.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|bytes| !valid_tile_payload(change.coord, bytes, self.width, self.height))
                || self.tiles.get(&change.coord) != current.as_ref()
            {
                return Err("Invalid tile change".into());
            }
            previous = Some((change.coord.y, change.coord.x));
        }
        for change in changes {
            let value = if undo { &change.before } else { &change.after };
            match value {
                Some(bytes) => {
                    self.tiles.insert(change.coord, bytes.clone());
                }
                None => {
                    self.tiles.remove(&change.coord);
                }
            }
        }
        Ok(())
    }
}

fn validate_off_canvas_tiles(
    tiles: &[OffCanvasTileState],
    width: u32,
    height: u32,
) -> Result<(), String> {
    if tiles.len() > 2048 {
        return Err("Too many off-canvas tiles".into());
    }
    let mut coords = BTreeSet::new();
    for tile in tiles {
        if tile.pixels.len() != TILE_BYTES || !coords.insert((tile.x, tile.y)) {
            return Err("Invalid off-canvas tile state".into());
        }
        let x0 = i64::from(tile.x) * i64::from(TILE_SIZE);
        let y0 = i64::from(tile.y) * i64::from(TILE_SIZE);
        if x0 < i64::from(i32::MIN)
            || y0 < i64::from(i32::MIN)
            || x0 >= i64::from(i32::MAX)
            || y0 >= i64::from(i32::MAX)
        {
            return Err("Invalid off-canvas tile coordinates".into());
        }
        let mut nonzero = false;
        for (i, pixel) in tile.pixels.as_chunks::<4>().0.iter().enumerate() {
            let x = x0 + i as i64 % i64::from(TILE_SIZE);
            let y = y0 + i as i64 / i64::from(TILE_SIZE);
            if x >= 0 && x < i64::from(width) && y >= 0 && y < i64::from(height) {
                if *pixel != [0; 4] {
                    return Err("Off-canvas tile overlaps canvas".into());
                }
            } else if *pixel != [0; 4] {
                nonzero = true;
            }
        }
        if !nonzero {
            return Err("Empty off-canvas tile".into());
        }
    }
    Ok(())
}

fn valid_tile_payload(coord: TileCoord, bytes: &[u8], width: u32, height: u32) -> bool {
    if bytes.len() != TILE_BYTES {
        return false;
    }
    let mut painted = false;
    for (index, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let x = coord.x * TILE_SIZE + (index as u32 % TILE_SIZE);
        let y = coord.y * TILE_SIZE + (index as u32 / TILE_SIZE);
        if x >= width || y >= height {
            if *pixel != [0, 0, 0, 0] {
                return false;
            }
        } else if *pixel != [0, 0, 0, 0] {
            painted = true;
        }
    }
    painted
}

fn valid_mask_payload(coord: TileCoord, values: &[u8], width: u32, height: u32) -> bool {
    if values.len() != TILE_BYTES / 4
        || coord.x >= width.div_ceil(TILE_SIZE)
        || coord.y >= height.div_ceil(TILE_SIZE)
    {
        return false;
    }
    let mut edited = false;
    for (index, value) in values.iter().enumerate() {
        let x = coord.x * TILE_SIZE + index as u32 % TILE_SIZE;
        let y = coord.y * TILE_SIZE + index as u32 / TILE_SIZE;
        if x >= width || y >= height {
            if *value != 255 {
                return false;
            }
        } else if *value != 255 {
            edited = true;
        }
    }
    edited
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RasterLayer {
    pub effects: crate::layer_effects::LayerEffects,
    pub blend_mode: RasterBlendMode,
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub locked: bool,
    pub alpha_locked: bool,
    #[serde(default)]
    pub mask_present: bool,
    #[serde(default = "crate::layer_mask::default_linked")]
    pub mask_linked: bool,
    pub mask_enabled: bool,
    pub mask_inverted: bool,
    pub mask_density: f32,
    pub mask: SparseMask,
    pub tiles: SparseTiles,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub off_canvas_tiles: Vec<OffCanvasTileState>,
}

impl RasterLayer {
    fn settings(&self) -> LayerSettings {
        LayerSettings {
            id: self.id.clone(),
            name: self.name.clone(),
            opacity: self.opacity,
            locked: self.locked,
            alpha_locked: self.alpha_locked,
            mask_enabled: self.mask_enabled,
            mask_inverted: self.mask_inverted,
            mask_density: self.mask_density,
        }
    }

    fn apply_settings(&mut self, settings: &LayerSettings) {
        self.name.clone_from(&settings.name);
        self.opacity = settings.opacity;
        self.locked = settings.locked;
        self.alpha_locked = settings.alpha_locked;
        self.mask_enabled = settings.mask_enabled;
        self.mask_inverted = settings.mask_inverted;
        self.mask_density = settings.mask_density;
    }

    pub fn effective_opacity(&self) -> f32 {
        self.opacity * mask_factor(self.mask_enabled, self.mask_inverted, self.mask_density)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum RasterEdit {
    #[serde(skip)]
    Stored(crate::history_storage::StoredHistory),
    ImportedLayer {
        layer: Box<RasterLayer>,
        index: usize,
    },
    Effects {
        id: String,
        before: Box<crate::layer_effects::LayerEffects>,
        after: Box<crate::layer_effects::LayerEffects>,
    },
    StackOrder {
        before: Vec<String>,
        after: Vec<String>,
        coords: Vec<TileCoord>,
    },
    Blend {
        layer_id: String,
        before: RasterBlendMode,
        after: RasterBlendMode,
    },
    Settings {
        before: LayerSettings,
        after: LayerSettings,
        before_mask_present: bool,
        after_mask_present: bool,
    },
    Pixels {
        layer_id: String,
        changes: Vec<TileChange>,
    },
    LinkedMaskMove {
        layer_id: String,
        changes: Vec<TileChange>,
        #[serde(default)]
        masks: Vec<MaskChange>,
        #[serde(default)]
        outside: Option<(Vec<OffCanvasTileState>, Vec<OffCanvasTileState>)>,
        before: Box<crate::layer_effects::LayerEffects>,
        after: Box<crate::layer_effects::LayerEffects>,
    },
    Appearance {
        layer_id: String,
        before: (bool, f32),
        after: (bool, f32),
    },
    Locks {
        layer_id: String,
        before: (bool, bool),
        after: (bool, bool),
    },
    Mask {
        layer_id: String,
        before: (bool, bool, f32, bool),
        after: (bool, bool, f32, bool),
    },
    SparseMaskLink {
        layer_id: String,
        before: bool,
        after: bool,
    },
    SparseMaskTransform {
        layer_id: String,
        pixels: Vec<TileChange>,
        masks: Vec<MaskChange>,
        #[serde(default)]
        outside: Option<(Vec<OffCanvasTileState>, Vec<OffCanvasTileState>)>,
    },
    MaskPixels {
        layer_id: String,
        changes: Vec<MaskChange>,
        before_mask_present: bool,
    },
    Reorder {
        layer_id: String,
        from: usize,
        to: usize,
        coords: Vec<TileCoord>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileInvalidation {
    pub layer_id: String,
    pub coords: Vec<TileCoord>,
}

/// One premultiplied RGBA8 upload. The buffer always has a 256-pixel row pitch;
/// the extent clips the right and bottom edge of the canvas.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileUpload {
    pub coord: TileCoord,
    pub origin: [u32; 2],
    pub extent: [u32; 2],
    pub bytes_per_row: u32,
    pub pixels: Vec<u8>,
}

/// One sampled brush dab in document pixels. Radius already includes pressure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RasterDab {
    pub material: crate::natural_media::MaterialSettings,
    pub texture: f32,
    pub texture_scale: f32,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub hardness: f32,
    pub weight: f32,
}

impl RasterDab {
    fn validate(self) -> Result<(), String> {
        self.material.validate()?;
        if !self.texture.is_finite()
            || !(0.0..=2.0).contains(&self.texture)
            || !self.texture_scale.is_finite()
            || !(0.001..=2.0).contains(&self.texture_scale)
            || !self.x.is_finite()
            || !self.y.is_finite()
            || self.x.abs() >= 100_000.0
            || self.y.abs() >= 100_000.0
            || !self.radius.is_finite()
            || !(0.025..=512.0).contains(&self.radius)
            || !self.hardness.is_finite()
            || !(0.0..=1.0).contains(&self.hardness)
            || !self.weight.is_finite()
            || !(0.0..=64.0).contains(&self.weight)
        {
            return Err("Invalid raster brush dab".into());
        }
        Ok(())
    }
}

/// Integer document-space grain shared with brush.wgsl; independent of event frequency.
pub fn brush_texture(x: f32, y: f32, texture: f32) -> f32 {
    if texture == 0.0 {
        return 1.0;
    }
    let grain = |x: f32, y: f32| {
        let mut hash =
            (x.floor() as u32).wrapping_mul(1973) ^ (y.floor() as u32).wrapping_mul(9277) ^ 89173;
        hash = (hash ^ (hash >> 13)).wrapping_mul(1274126177);
        (hash & 255) as f32 / 255.0
    };
    let g = grain(x, y);
    let pencil = 0.03 + 0.4 * g * g;
    if texture <= 1.0 {
        return 1.0 + (pencil - 1.0) * texture;
    }
    let dry = if grain(x / 1.5, y / 5.0) < 0.48 {
        0.0
    } else {
        0.65
    };
    pencil + (dry - pencil) * (texture - 1.0)
}

pub(crate) fn dab_density(
    width: u32,
    height: u32,
    dabs: &[RasterDab],
    selection: Option<&Selection>,
) -> Result<BTreeMap<TileCoord, Vec<f32>>, String> {
    dab_density_on_tiles(width, height, dabs, selection, None, None)
}

fn dab_density_on_tiles(
    width: u32,
    height: u32,
    dabs: &[RasterDab],
    selection: Option<&Selection>,
    occupied: Option<&BTreeMap<TileCoord, Vec<u8>>>,
    targets: Option<&BTreeSet<TileCoord>>,
) -> Result<BTreeMap<TileCoord, Vec<f32>>, String> {
    if dabs.len() > 65_536 {
        return Err("Too many raster brush dabs".into());
    }
    for dab in dabs {
        dab.validate()?;
    }
    if let Some(selection) = selection {
        selection.validate()?;
    }
    let mut density: BTreeMap<TileCoord, Vec<f32>> = BTreeMap::new();
    // The grain at a document pixel is shared by overlapping dabs. Watercolor
    // depends on each dab's radial distance, and animated brush envelopes can
    // change the texture, so neither may reuse this value.
    let cache_grain = dabs.first().is_some_and(|first| {
        (first.texture > 0. || first.material.enabled)
            && !(first.material.enabled
                && first.material.medium == crate::natural_media::Medium::Watercolor
                && first.material.edge != 0.)
            && dabs.iter().all(|dab| {
                dab.material == first.material
                    && dab.texture == first.texture
                    && dab.texture_scale == first.texture_scale
            })
    });
    let mut grain_cache: BTreeMap<TileCoord, Vec<f32>> = BTreeMap::new();
    for dab in dabs {
        if dab.weight == 0.0 {
            continue;
        }
        // Texture coordinates remain in document space, but a downsampled tip
        // still needs an output-pixel filter footprint. Otherwise a dab between
        // pixel centers can disappear entirely from thumbnails/onion references.
        let filter_scale = dab.texture_scale.max(1.0);
        let fringe = 2.0 * filter_scale;
        let left = ((dab.x - dab.radius - fringe).floor() as i32).max(0) as u32;
        let top = ((dab.y - dab.radius - fringe).floor() as i32).max(0) as u32;
        let right = ((dab.x + dab.radius + fringe).ceil() as i32)
            .min(width as i32)
            .max(0) as u32;
        let bottom = ((dab.y + dab.radius + fringe).ceil() as i32)
            .min(height as i32)
            .max(0) as u32;
        if left >= right || top >= bottom {
            continue;
        }
        let inner = dab.radius * dab.hardness;
        // These values depend on the dab, not the covered pixel. Computing
        // them once matters most for the many overlapping dabs in a stroke.
        let (m, a, p) = dab.material.packed();
        let pixel_size = 1. / dab.texture_scale;
        let mut footprint: f32 = 1.;
        if dab.texture > 0. || dab.material.enabled {
            footprint =
                crate::natural_media::texture_footprint(dab.material.grain_transform, pixel_size);
        }
        if dab.material.enabled && dab.material.paper.enabled {
            footprint = footprint.max(crate::natural_media::texture_footprint(
                dab.material.paper.texture_transform,
                pixel_size,
            ));
        }
        let samples = crate::natural_media::texture_samples_for_footprint(footprint);
        let jitter = footprint > 8.;
        let mut offsets = [(0., 0.); 144];
        for j in 0..samples {
            for i in 0..samples {
                offsets[(j * samples + i) as usize] =
                    crate::natural_media::texture_sample_offset(i, j, samples, jitter);
            }
        }
        for tile_y in top / TILE_SIZE..bottom.div_ceil(TILE_SIZE) {
            for tile_x in left / TILE_SIZE..right.div_ceil(TILE_SIZE) {
                let coord = TileCoord {
                    x: tile_x,
                    y: tile_y,
                };
                if occupied.is_some_and(|tiles| !tiles.contains_key(&coord))
                    || targets.is_some_and(|tiles| !tiles.contains(&coord))
                {
                    continue;
                }
                let coverage = density
                    .entry(coord)
                    .or_insert_with(|| vec![0.0; TILE_BYTES / 4]);
                // Limit additional scratch storage for large sparse documents.
                if cache_grain && samples > 1 && grain_cache.len() < 16 {
                    grain_cache
                        .entry(coord)
                        .or_insert_with(|| vec![-1.; TILE_BYTES / 4]);
                }
                let mut cached = grain_cache.get_mut(&coord);
                for y in top.max(tile_y * TILE_SIZE)..bottom.min((tile_y + 1) * TILE_SIZE) {
                    for x in left.max(tile_x * TILE_SIZE)..right.min((tile_x + 1) * TILE_SIZE) {
                        let selection_alpha = selection.map_or(1., |s| {
                            s.coverage(Point {
                                x: x as f32 + 0.5,
                                y: y as f32 + 0.5,
                            })
                        });
                        if selection_alpha <= 0. {
                            continue;
                        }
                        let dx = x as f32 + 0.5 - dab.x;
                        let dy = y as f32 + 0.5 - dab.y;
                        let distance = dx.hypot(dy);
                        // Preserve the existing 1x/2x radial profile while keeping
                        // antialiasing at least one output pixel wide when reduced.
                        let aa =
                            ((dx.abs() + dy.abs()) / distance.max(0.001)).max(0.01) * filter_scale;
                        let edge = (inner + aa).max(dab.radius + aa);
                        let t = ((distance - inner) / (edge - inner)).clamp(0.0, 1.0);
                        let profile = 1.0 - t * t * (3.0 - 2.0 * t);
                        if profile <= 0.0 {
                            continue;
                        }
                        let index = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize;
                        let prior = cached.as_ref().map_or(-1., |values| values[index]);
                        let mut pigment = prior.max(0.);
                        if prior < 0. {
                            for &(ox, oy) in &offsets[..(samples * samples) as usize] {
                                let px = (x as f32 + 0.5 + ox) * pixel_size;
                                let py = (y as f32 + 0.5 + oy) * pixel_size;
                                let paper_point = dab.material.paper.texture_point(px, py);
                                let grain_point = dab.material.grain_point(px, py);
                                pigment +=
                                    crate::natural_media::deposition_mapped(
                                        grain_point.0,
                                        grain_point.1,
                                        paper_point.0,
                                        paper_point.1,
                                        distance / dab.radius,
                                        m,
                                        a,
                                        p,
                                    ) * brush_texture(grain_point.0, grain_point.1, dab.texture);
                            }
                            if let Some(values) = cached.as_mut() {
                                values[index] = pigment;
                            }
                        }
                        coverage[index] += pigment / (samples * samples) as f32
                            * (4.0 + 12.0 * dab.hardness)
                            * dab.weight
                            * profile;
                    }
                }
            }
        }
    }
    // Selection coverage clips the completed stroke, not each dab's optical
    // depth. Otherwise overlapping dabs would turn a feathered edge opaque.
    if let Some(selection) = selection.filter(|s| s.has_soft_mask()) {
        for (coord, tile) in &mut density {
            for (index, depth) in tile.iter_mut().enumerate() {
                if *depth <= 0. {
                    continue;
                }
                let point = Point {
                    x: (coord.x * TILE_SIZE + index as u32 % TILE_SIZE) as f32 + 0.5,
                    y: (coord.y * TILE_SIZE + index as u32 / TILE_SIZE) as f32 + 0.5,
                };
                let alpha = selection.coverage(point);
                if alpha < 1. {
                    let coverage = (1. - (-*depth).exp()) * alpha;
                    *depth = -(1. - coverage).max(1e-7).ln();
                }
            }
        }
    }
    Ok(density)
}

/// Tile-backed document model for the next raster pipeline. It has an independent
/// project format and preview path; legacy stroke/SVG documents remain separate.
#[derive(Clone, Debug, PartialEq)]
pub struct TiledRasterDocument {
    layer_edit_selection: Option<(String, crate::layer_mask::LayerEditTarget)>,
    resolution: Option<RasterResolution>,
    icc_profile: Option<Vec<u8>>,
    width: u32,
    height: u32,
    layers: Vec<RasterLayer>,
    undo: Vec<RasterEdit>,
    redo: Vec<RasterEdit>,
    revision: u64,
}

impl TiledRasterDocument {
    pub fn layer_edit_target(&self, id: &str) -> crate::layer_mask::LayerEditTarget {
        use crate::layer_mask::LayerEditTarget;
        let target = self
            .layer_edit_selection
            .as_ref()
            .filter(|(layer, _)| layer == id)
            .map_or(LayerEditTarget::Content, |(_, t)| *t);
        if target == LayerEditTarget::Mask
            && !self
                .layers
                .iter()
                .any(|l| l.id == id && l.effects.mask.is_some())
        {
            LayerEditTarget::None
        } else {
            target
        }
    }
    pub fn select_layer_target(
        &mut self,
        id: String,
        target: crate::layer_mask::LayerEditTarget,
    ) -> Result<(), String> {
        let layer = self
            .layers
            .iter()
            .find(|l| l.id == id)
            .ok_or("Layer not found")?;
        if target == crate::layer_mask::LayerEditTarget::Mask && layer.effects.mask.is_none() {
            return Err("Layer mask not found".into());
        }
        self.layer_edit_selection = Some((id, target));
        Ok(())
    }

    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        SparseTiles::new(width, height)?;
        Ok(Self {
            layer_edit_selection: None,
            resolution: None,
            icc_profile: None,
            width,
            height,
            layers: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        })
    }

    pub fn state(&self) -> TiledRasterState {
        TiledRasterState {
            resolution: self.resolution,
            icc_profile: self.icc_profile.clone(),
            width: self.width,
            height: self.height,
            layers: self
                .layers
                .iter()
                .map(|layer| RasterLayerState {
                    effects: layer.effects.clone(),
                    blend_mode: layer.blend_mode,
                    id: layer.id.clone(),
                    name: layer.name.clone(),
                    visible: layer.visible,
                    opacity: layer.opacity,
                    locked: layer.locked,
                    alpha_locked: layer.alpha_locked,
                    mask_present: layer.mask_present || layer.mask.allocated_tile_count() > 0,
                    mask_linked: layer.mask_linked,
                    mask_enabled: layer.mask_enabled,
                    mask_inverted: layer.mask_inverted,
                    mask_density: layer.mask_density,
                    tiles: layer
                        .tiles
                        .tiles
                        .iter()
                        .map(|(coord, pixels)| RasterTileState {
                            coord: *coord,
                            pixels: pixels.clone(),
                        })
                        .collect(),
                    off_canvas_tiles: layer.off_canvas_tiles.clone(),
                    mask_tiles: layer
                        .mask
                        .tiles
                        .iter()
                        .map(|(coord, values)| MaskTileState {
                            coord: *coord,
                            values: values.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Restore validated authoritative pixels without importing edit history.
    /// The caller must validate container sizes before decoding untrusted bytes;
    /// this method validates all semantic fields and tile payloads atomically.
    pub fn from_state(state: TiledRasterState) -> Result<Self, String> {
        let mut document = Self::new(state.width, state.height)?;
        if let Some(resolution) = state.resolution {
            resolution.validate()?;
        }
        document.resolution = state.resolution;
        if let Some(profile) = &state.icc_profile {
            validate_rgb_icc(profile)?;
        }
        document.icc_profile = state.icc_profile;
        if state.layers.len() > 16 {
            return Err("Too many raster layers".into());
        }
        for saved in state.layers {
            if !saved.opacity.is_finite()
                || !(0.0..=1.0).contains(&saved.opacity)
                || !saved.mask_density.is_finite()
                || !(0.0..=1.0).contains(&saved.mask_density)
            {
                return Err("Invalid raster layer state".into());
            }
            document.add_layer(saved.id.clone(), saved.name)?;
            let layer = document.layers.last_mut().expect("layer was just added");
            layer.blend_mode = saved.blend_mode;
            saved.effects.validate()?;
            layer.effects = saved.effects;
            layer.visible = saved.visible;
            layer.opacity = saved.opacity;
            layer.locked = saved.locked;
            layer.alpha_locked = saved.alpha_locked;
            layer.mask_present =
                saved.mask_present || saved.mask_enabled || !saved.mask_tiles.is_empty();
            layer.mask_linked = saved.mask_linked;
            layer.mask_enabled = saved.mask_enabled;
            layer.mask_inverted = saved.mask_inverted;
            layer.mask_density = saved.mask_density;
            validate_off_canvas_tiles(&saved.off_canvas_tiles, document.width, document.height)?;
            layer.off_canvas_tiles = saved.off_canvas_tiles;
            for tile in saved.tiles {
                if layer.tiles.tiles.contains_key(&tile.coord)
                    || !valid_tile_payload(
                        tile.coord,
                        &tile.pixels,
                        document.width,
                        document.height,
                    )
                {
                    return Err("Invalid raster tile state".into());
                }
                layer.tiles.tiles.insert(tile.coord, tile.pixels);
            }
            for tile in saved.mask_tiles {
                if layer.mask.tiles.contains_key(&tile.coord)
                    || !valid_mask_payload(
                        tile.coord,
                        &tile.values,
                        document.width,
                        document.height,
                    )
                {
                    return Err("Invalid raster mask tile state".into());
                }
                layer.mask.tiles.insert(tile.coord, tile.values);
            }
        }
        document.revision = 0;
        document.discard_history();
        Ok(document)
    }

    pub fn layers(&self) -> &[RasterLayer] {
        &self.layers
    }

    pub fn resolution(&self) -> Option<RasterResolution> {
        self.resolution
    }

    pub fn icc_profile(&self) -> Option<&[u8]> {
        self.icc_profile.as_deref()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Drop import-time edits after replaying a legacy document. This leaves
    /// the current pixels and revision intact while releasing tile snapshots.
    pub fn discard_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Prepare only affected tiles after an edit. A transparent payload is
    /// returned when a previously visible tile must be cleared on the GPU.
    pub fn prepare_uploads(
        &self,
        invalidation: &TileInvalidation,
    ) -> Result<Vec<TileUpload>, String> {
        if !self
            .layers
            .iter()
            .any(|layer| layer.id == invalidation.layer_id)
        {
            return Err("Unknown raster layer invalidation".into());
        }
        self.prepare_coords(&invalidation.coords)
    }

    /// Populate a new GPU texture from the current document state.
    pub fn prepare_full_uploads(&self) -> Vec<TileUpload> {
        let coords = self
            .layers
            .iter()
            .flat_map(|layer| layer.tiles.allocated_coords())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        self.prepare_coords(&coords)
            .expect("allocated tile coordinates are within the document")
    }

    /// Compare two composites and upload only changed tiles, including a
    /// transparent payload for tiles removed by Undo or visibility changes.
    pub fn prepare_changed_uploads(&self, previous: &Self) -> Result<Vec<TileUpload>, String> {
        if self.dimensions() != previous.dimensions() {
            return Err("Raster documents have different dimensions".into());
        }
        let coords = self
            .layers
            .iter()
            .chain(&previous.layers)
            .flat_map(|layer| layer.tiles.allocated_coords())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        self.prepare_changed_uploads_in_coords(previous, &coords)
    }

    /// Compare only coordinates known to contain a change. Callers must pass
    /// a conservative superset of every possibly affected tile.
    pub fn prepare_changed_uploads_in_coords(
        &self,
        previous: &Self,
        coords: &[TileCoord],
    ) -> Result<Vec<TileUpload>, String> {
        if self.dimensions() != previous.dimensions() {
            return Err("Raster documents have different dimensions".into());
        }
        let before = previous.prepare_coords(coords)?;
        let after = self.prepare_coords(coords)?;
        Ok(after
            .into_iter()
            .zip(before)
            .filter_map(|(next, old)| (next.pixels != old.pixels).then_some(next))
            .collect())
    }

    fn prepare_coords(&self, coords: &[TileCoord]) -> Result<Vec<TileUpload>, String> {
        let ordered = coords.iter().copied().collect::<BTreeSet<_>>();
        if ordered.iter().any(|coord| {
            coord.x >= self.width.div_ceil(TILE_SIZE) || coord.y >= self.height.div_ceil(TILE_SIZE)
        }) {
            return Err("Raster invalidation outside canvas".into());
        }
        Ok(ordered
            .into_iter()
            .map(|coord| {
                let x = coord.x * TILE_SIZE;
                let y = coord.y * TILE_SIZE;
                TileUpload {
                    coord,
                    origin: [x, y],
                    extent: [
                        (self.width - x).min(TILE_SIZE),
                        (self.height - y).min(TILE_SIZE),
                    ],
                    bytes_per_row: TILE_SIZE * 4,
                    pixels: self
                        .composite_tile(coord)
                        .unwrap_or_else(|| vec![0; TILE_BYTES]),
                }
            })
            .collect())
    }

    /// Composite one tile in layer order. Input tiles use straight alpha;
    /// the returned RGBA8 pixels are premultiplied for the GPU compositor.
    pub fn composite_tile(&self, coord: TileCoord) -> Option<Vec<u8>> {
        if coord.x >= self.width.div_ceil(TILE_SIZE) || coord.y >= self.height.div_ceil(TILE_SIZE) {
            return None;
        }
        composite_sources(
            coord,
            self.layers.iter().filter_map(|layer| {
                if !layer.visible || layer.opacity <= 0.0 {
                    return None;
                }
                Some(CompositeSource {
                    effects: &layer.effects,
                    blend_mode: layer.blend_mode,
                    pixels: layer.tiles.tile(coord)?,
                    opacity: layer.opacity,
                    mask: layer.mask.tiles.get(&coord).map(Vec::as_slice),
                    mask_enabled: layer.mask_enabled,
                    mask_inverted: layer.mask_inverted,
                    mask_density: layer.mask_density,
                })
            }),
        )
    }

    pub fn add_layer(&mut self, id: String, name: String) -> Result<(), String> {
        if id.is_empty()
            || id.len() > 64
            || name.trim().is_empty()
            || name.chars().count() > 255
            || self.layers.len() >= 16
            || self.layers.iter().any(|layer| layer.id == id)
        {
            return Err("Invalid or duplicate raster layer".into());
        }
        self.layers.push(RasterLayer {
            effects: Default::default(),
            blend_mode: RasterBlendMode::Normal,
            id,
            name,
            visible: true,
            opacity: 1.0,
            locked: false,
            alpha_locked: false,
            mask_present: false,
            mask_linked: true,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
            mask: SparseMask::new(self.width, self.height),
            tiles: SparseTiles::new(self.width, self.height)?,
            off_canvas_tiles: Vec::new(),
        });
        self.revision += 1;
        Ok(())
    }

    /// Attach worker-prepared pixels as one atomic, undoable layer import.
    pub fn import_layer(
        &mut self,
        id: String,
        name: String,
        tiles: SparseTiles,
    ) -> Result<(), String> {
        if tiles.dimensions() != self.dimensions() {
            return Err("Imported layer dimensions differ from document".into());
        }
        // Reuse add_layer validation; no operation after this can fail.
        self.add_layer(id, name)?;
        let index = self.layers.len() - 1;
        self.layers[index].tiles = tiles;
        self.revision -= 1;
        self.record_edit(RasterEdit::ImportedLayer {
            layer: Box::new(self.layers[index].clone()),
            index,
        });
        Ok(())
    }

    /// Apply a panel submission as one validated, undoable edit without copying pixels.
    pub fn set_layer_settings(
        &mut self,
        settings: LayerSettings,
    ) -> Result<Option<TileInvalidation>, String> {
        if settings.name.trim().is_empty()
            || settings.name.chars().count() > 255
            || settings.name.chars().any(char::is_control)
            || !settings.opacity.is_finite()
            || !(0.0..=1.0).contains(&settings.opacity)
            || !settings.mask_density.is_finite()
            || !(0.0..=1.0).contains(&settings.mask_density)
        {
            return Err("Invalid raster layer settings".into());
        }
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == settings.id)
            .ok_or("Unknown raster layer")?;
        let before = layer.settings();
        if before == settings {
            return Ok(None);
        }
        let affects_pixels = before.opacity != settings.opacity
            || before.mask_enabled != settings.mask_enabled
            || before.mask_inverted != settings.mask_inverted
            || before.mask_density != settings.mask_density;
        let coords = if affects_pixels {
            layer.tiles.allocated_coords().collect()
        } else {
            Vec::new()
        };
        let before_mask_present = layer.mask_present;
        let after_mask_present = before_mask_present || settings.mask_enabled;
        layer.apply_settings(&settings);
        layer.mask_present = after_mask_present;
        let layer_id = settings.id.clone();
        self.record_edit(RasterEdit::Settings {
            before,
            after: settings,
            before_mask_present,
            after_mask_present,
        });
        Ok(Some(TileInvalidation { layer_id, coords }))
    }

    pub fn set_sparse_mask_linked(&mut self, id: &str, linked: bool) -> Result<bool, String> {
        let layer = self
            .layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or("Unknown raster layer")?;
        if layer.locked {
            return Err("Raster layer is locked".into());
        }
        if !layer.mask_present {
            return Err("Layer mask not found".into());
        }
        let before = layer.mask_linked;
        if before == linked {
            return Ok(false);
        }
        layer.mask_linked = linked;
        self.record_edit(RasterEdit::SparseMaskLink {
            layer_id: id.into(),
            before,
            after: linked,
        });
        Ok(true)
    }

    pub fn set_layer_effects(
        &mut self,
        id: &str,
        mut effects: crate::layer_effects::LayerEffects,
    ) -> Result<Option<TileInvalidation>, String> {
        let previous = &self
            .layers
            .iter()
            .find(|l| l.id == id)
            .ok_or("Unknown raster layer")?
            .effects;
        effects.retain_mask_content(previous)?;
        effects.validate()?;
        let layer = self
            .layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or("Unknown raster layer")?;
        if layer.effects == effects {
            return Ok(None);
        }
        if layer.locked {
            return Err("Raster layer is locked".into());
        }
        let before = std::mem::replace(&mut layer.effects, effects.clone());
        let coords = layer.tiles.allocated_coords().collect();
        self.record_edit(RasterEdit::Effects {
            id: id.into(),
            before: Box::new(before),
            after: Box::new(effects),
        });
        Ok(Some(TileInvalidation {
            layer_id: id.into(),
            coords,
        }))
    }
    pub fn set_layer_blend_mode(
        &mut self,
        layer_id: &str,
        mode: RasterBlendMode,
    ) -> Result<Option<TileInvalidation>, String> {
        let layer = self
            .layers
            .iter_mut()
            .find(|l| l.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if layer.blend_mode == mode {
            return Ok(None);
        }
        if layer.locked {
            return Err("Raster layer is locked".into());
        }
        let before = layer.blend_mode;
        let coords = layer.tiles.allocated_coords().collect();
        layer.blend_mode = mode;
        self.record_edit(RasterEdit::Blend {
            layer_id: layer_id.into(),
            before,
            after: mode,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    pub fn set_layer_appearance(
        &mut self,
        layer_id: &str,
        visible: bool,
        opacity: f32,
    ) -> Result<Option<TileInvalidation>, String> {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err("Invalid raster layer opacity".into());
        }
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        let before = (layer.visible, layer.opacity);
        let after = (visible, opacity);
        if before == after {
            return Ok(None);
        }
        let coords = layer.tiles.allocated_coords().collect();
        layer.visible = visible;
        layer.opacity = opacity;
        self.record_edit(RasterEdit::Appearance {
            layer_id: layer_id.into(),
            before,
            after,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    pub fn set_layer_locks(
        &mut self,
        layer_id: &str,
        locked: bool,
        alpha_locked: bool,
    ) -> Result<bool, String> {
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        let before = (layer.locked, layer.alpha_locked);
        let after = (locked, alpha_locked);
        if before == after {
            return Ok(false);
        }
        (layer.locked, layer.alpha_locked) = after;
        self.record_edit(RasterEdit::Locks {
            layer_id: layer_id.into(),
            before,
            after,
        });
        Ok(true)
    }

    pub fn set_layer_mask(
        &mut self,
        layer_id: &str,
        enabled: bool,
        inverted: bool,
        density: f32,
    ) -> Result<Option<TileInvalidation>, String> {
        if !density.is_finite() || !(0.0..=1.0).contains(&density) {
            return Err("Invalid raster mask density".into());
        }
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        let before = (
            layer.mask_enabled,
            layer.mask_inverted,
            layer.mask_density,
            layer.mask_present,
        );
        let after = (enabled, inverted, density, layer.mask_present || enabled);
        if before == after {
            return Ok(None);
        }
        let coords = layer.tiles.allocated_coords().collect();
        (
            layer.mask_enabled,
            layer.mask_inverted,
            layer.mask_density,
            layer.mask_present,
        ) = after;
        self.record_edit(RasterEdit::Mask {
            layer_id: layer_id.into(),
            before,
            after,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    pub fn write_mask_rect(
        &mut self,
        layer_id: &str,
        rect: [u32; 4],
        values: &[u8],
    ) -> Result<Option<TileInvalidation>, String> {
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if layer.locked {
            return Err("Raster layer is locked".into());
        }
        let changes = layer.mask.write_rect(rect, values)?;
        if changes.is_empty() {
            return Ok(None);
        }
        let coords = changes.iter().map(|change| change.coord).collect();
        let before_mask_present = layer.mask_present;
        layer.mask_present = true;
        self.record_edit(RasterEdit::MaskPixels {
            layer_id: layer_id.into(),
            changes,
            before_mask_present,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    pub fn paint_mask_dabs(
        &mut self,
        layer_id: &str,
        dabs: &[RasterDab],
        value: u8,
        selection: Option<&Selection>,
    ) -> Result<Option<TileInvalidation>, String> {
        self.paint_mask_brush_dabs(
            layer_id,
            dabs,
            crate::document::Brush {
                color: [value; 3],
                ..Default::default()
            },
            selection,
        )
    }

    pub fn paint_mask_brush_dabs(
        &mut self,
        layer_id: &str,
        dabs: &[RasterDab],
        brush: crate::document::Brush,
        selection: Option<&Selection>,
    ) -> Result<Option<TileInvalidation>, String> {
        brush.validate()?;
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if layer.locked || !layer.visible {
            return Err("Raster layer is hidden or locked".into());
        }
        let changes = layer.mask.paint_dabs(dabs, brush, selection)?;
        if changes.is_empty() {
            return Ok(None);
        }
        let coords = changes.iter().map(|change| change.coord).collect();
        let before_mask_present = layer.mask_present;
        layer.mask_present = true;
        self.record_edit(RasterEdit::MaskPixels {
            layer_id: layer_id.into(),
            changes,
            before_mask_present,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    /// Atomic panel ordering, top to bottom. Histories store identities, never pixel copies.
    pub fn reorder_layers(&mut self, ids: &[String]) -> Result<Option<TileInvalidation>, String> {
        let before = self.layers.iter().map(|l| l.id.clone()).collect::<Vec<_>>();
        let after = ids.iter().rev().cloned().collect::<Vec<_>>();
        let unique = after.iter().collect::<BTreeSet<_>>();
        if after.len() != before.len()
            || unique.len() != before.len()
            || before.iter().any(|id| !unique.contains(id))
        {
            return Err("Invalid raster layer order".into());
        }
        if before == after {
            return Ok(None);
        }
        let first = before.iter().zip(&after).position(|(a, b)| a != b).unwrap();
        let last = before
            .iter()
            .zip(&after)
            .rposition(|(a, b)| a != b)
            .unwrap();
        let coords = self.layers[first..=last]
            .iter()
            .flat_map(|l| l.tiles.allocated_coords())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        self.layers
            .sort_by_key(|l| after.iter().position(|id| id == &l.id).unwrap());
        let layer_id = after[first].clone();
        self.record_edit(RasterEdit::StackOrder {
            before,
            after,
            coords: coords.clone(),
        });
        Ok(Some(TileInvalidation { layer_id, coords }))
    }

    /// Move a layer to a zero-based back-to-front index.
    pub fn move_layer(
        &mut self,
        layer_id: &str,
        to: usize,
    ) -> Result<Option<TileInvalidation>, String> {
        let from = self
            .layers
            .iter()
            .position(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if to >= self.layers.len() {
            return Err("Raster layer index outside stack".into());
        }
        if from == to {
            return Ok(None);
        }
        let coords = self.layers[from.min(to)..=from.max(to)]
            .iter()
            .flat_map(|layer| layer.tiles.allocated_coords())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        self.record_edit(RasterEdit::Reorder {
            layer_id: layer_id.into(),
            from,
            to,
            coords: coords.clone(),
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    /// Move linked raster content and its effect mask as a single history edit.
    pub fn transform_linked_effect_mask(
        &mut self,
        id: &str,
        matrix: [f32; 6],
        moved: SparseTiles,
        moved_mask: Option<SparseMask>,
    ) -> Result<Option<TileInvalidation>, String> {
        self.transform_linked_effect_mask_with_off_canvas(id, matrix, moved, moved_mask, None)
    }

    pub fn transform_linked_effect_mask_with_off_canvas(
        &mut self,
        id: &str,
        matrix: [f32; 6],
        moved: SparseTiles,
        moved_mask: Option<SparseMask>,
        moved_outside: Option<Vec<OffCanvasTileState>>,
    ) -> Result<Option<TileInvalidation>, String> {
        if let Some(outside) = &moved_outside {
            validate_off_canvas_tiles(outside, self.width, self.height)?;
        }
        let layer = self
            .layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or("Unknown raster layer")?;
        if !layer.off_canvas_tiles.is_empty() && moved_outside.is_none() {
            return Err("Cannot transform a layer containing off-canvas pixels".into());
        }
        if layer.locked || layer.alpha_locked {
            return Err("Raster layer is locked".into());
        }
        if moved_mask.is_some() != (layer.mask_present && layer.mask_linked) {
            return Err("Mask link state changed".into());
        }
        if moved_mask
            .as_ref()
            .is_some_and(|m| m.width != layer.mask.width || m.height != layer.mask.height)
        {
            return Err("Transformed mask dimensions differ".into());
        }
        let before = layer.effects.clone();
        let mut after = before.clone();
        let mask = after.mask.as_mut().ok_or("Layer mask not found")?;
        if !mask.linked {
            return Err("Layer mask is not linked".into());
        }
        if moved.dimensions() != layer.tiles.dimensions() {
            return Err("Transformed raster dimensions differ".into());
        }
        if matrix == [1., 0., 0., 1., 0., 0.] {
            return Ok(None);
        }
        mask.transform_by(matrix)?;
        let coords = layer
            .tiles
            .allocated_coords()
            .chain(moved.allocated_coords())
            .collect::<BTreeSet<_>>();
        let changes = coords
            .into_iter()
            .filter_map(|coord| {
                let before = layer.tiles.tiles.get(&coord).cloned();
                let after = moved.tiles.get(&coord).cloned();
                (before != after).then_some(TileChange {
                    coord,
                    before,
                    after,
                })
            })
            .collect::<Vec<_>>();
        let invalidated = layer
            .tiles
            .allocated_coords()
            .chain(moved.allocated_coords())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let masks = if let Some(mask) = moved_mask {
            let coords = layer
                .mask
                .tiles
                .keys()
                .chain(mask.tiles.keys())
                .copied()
                .collect::<BTreeSet<_>>();
            let changes = coords
                .into_iter()
                .filter_map(|coord| {
                    let before = layer.mask.tiles.get(&coord).cloned();
                    let after = mask.tiles.get(&coord).cloned();
                    (before != after).then_some(MaskChange {
                        coord,
                        before,
                        after,
                    })
                })
                .collect();
            layer.mask = mask;
            changes
        } else {
            Vec::new()
        };
        layer.tiles = moved;
        let outside = moved_outside.map(|after| (layer.off_canvas_tiles.clone(), after));
        if let Some((_, after)) = &outside {
            layer.off_canvas_tiles = after.clone();
        }
        layer.effects = after.clone();
        self.record_edit(RasterEdit::LinkedMaskMove {
            layer_id: id.into(),
            changes,
            masks,
            outside,
            before: Box::new(before),
            after: Box::new(after),
        });
        Ok(Some(TileInvalidation {
            layer_id: id.into(),
            coords: invalidated,
        }))
    }

    /// Commit precomputed sparse mask and optional linked content as one edit.
    pub fn transform_sparse_mask(
        &mut self,
        id: &str,
        pixels: Option<SparseTiles>,
        mask: SparseMask,
    ) -> Result<Option<TileInvalidation>, String> {
        self.transform_sparse_mask_with_off_canvas(id, pixels, mask, None)
    }

    /// Commit worker-prepared canvas and outside pixels in the same undo entry.
    pub fn transform_sparse_mask_with_off_canvas(
        &mut self,
        id: &str,
        pixels: Option<SparseTiles>,
        mask: SparseMask,
        moved_outside: Option<Vec<OffCanvasTileState>>,
    ) -> Result<Option<TileInvalidation>, String> {
        if let Some(outside) = &moved_outside {
            validate_off_canvas_tiles(outside, self.width, self.height)?;
        }
        let layer = self
            .layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or("Unknown raster layer")?;
        if !layer.off_canvas_tiles.is_empty() && pixels.is_some() && moved_outside.is_none() {
            return Err("Cannot transform a layer containing off-canvas pixels".into());
        }
        if moved_outside.is_some() && pixels.is_none() {
            return Err("Off-canvas pixels require linked raster content".into());
        }
        if layer.locked || (pixels.is_some() && layer.alpha_locked) {
            return Err("Raster layer is locked".into());
        }
        if !layer.mask_present || pixels.is_some() != layer.mask_linked {
            return Err("Mask link state changed".into());
        }
        if mask.width != layer.mask.width
            || mask.height != layer.mask.height
            || pixels
                .as_ref()
                .is_some_and(|p| p.dimensions() != layer.tiles.dimensions())
        {
            return Err("Transformed raster dimensions differ".into());
        }
        let pixel_coords = layer
            .tiles
            .allocated_coords()
            .chain(pixels.iter().flat_map(|p| p.allocated_coords()))
            .collect::<BTreeSet<_>>();
        let changes = if let Some(moved) = &pixels {
            pixel_coords
                .iter()
                .filter_map(|coord| {
                    let before = layer.tiles.tiles.get(coord).cloned();
                    let after = moved.tiles.get(coord).cloned();
                    (before != after).then_some(TileChange {
                        coord: *coord,
                        before,
                        after,
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mask_coords = layer
            .mask
            .tiles
            .keys()
            .chain(mask.tiles.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        let masks = mask_coords
            .iter()
            .filter_map(|coord| {
                let before = layer.mask.tiles.get(coord).cloned();
                let after = mask.tiles.get(coord).cloned();
                (before != after).then_some(MaskChange {
                    coord: *coord,
                    before,
                    after,
                })
            })
            .collect::<Vec<_>>();
        let outside = moved_outside.map(|after| (layer.off_canvas_tiles.clone(), after));
        if changes.is_empty()
            && masks.is_empty()
            && outside
                .as_ref()
                .is_none_or(|(before, after)| before == after)
        {
            return Ok(None);
        }
        let coords = pixel_coords
            .into_iter()
            .chain(mask_coords)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if let Some(moved) = pixels {
            layer.tiles = moved;
        }
        layer.mask = mask;
        if let Some((_, after)) = &outside {
            layer.off_canvas_tiles = after.clone();
        }
        self.record_edit(RasterEdit::SparseMaskTransform {
            layer_id: id.into(),
            pixels: changes,
            masks,
            outside,
        });
        Ok(Some(TileInvalidation {
            layer_id: id.into(),
            coords,
        }))
    }

    pub fn write_rect(
        &mut self,
        layer_id: &str,
        rect: [u32; 4],
        pixels: &[u8],
    ) -> Result<Option<TileInvalidation>, String> {
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if layer.locked || layer.alpha_locked {
            return Err("Raster layer pixel replacement is locked".into());
        }
        let changes = layer
            .tiles
            .write_rect(rect[0], rect[1], rect[2], rect[3], pixels)?;
        if changes.is_empty() {
            return Ok(None);
        }
        let changed_coords = changes.iter().map(|change| change.coord).collect();
        self.record_edit(RasterEdit::Pixels {
            layer_id: layer_id.into(),
            changes,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords: changed_coords,
        }))
    }

    pub fn paint_dabs(
        &mut self,
        layer_id: &str,
        dabs: &[RasterDab],
        color: [u8; 3],
    ) -> Result<Option<TileInvalidation>, String> {
        self.paint_dabs_clipped(layer_id, dabs, color, None)
    }

    pub fn clear_selection(
        &mut self,
        layer_id: &str,
        selection: &Selection,
    ) -> Result<Option<TileInvalidation>, String> {
        selection.validate()?;
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if layer.locked || layer.alpha_locked || !layer.visible {
            return Err("Raster layer is hidden or locked".into());
        }
        let mut changes = vec![];
        for coord in layer.tiles.allocated_coords().collect::<Vec<_>>() {
            let before = layer.tiles.tiles[&coord].clone();
            let mut after = before.clone();
            for (index, pixel) in after.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = coord.x * TILE_SIZE + index as u32 % TILE_SIZE;
                let y = coord.y * TILE_SIZE + index as u32 / TILE_SIZE;
                let alpha = selection.coverage(crate::document::Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                });
                for c in pixel {
                    *c = (f32::from(*c) * (1. - alpha)).round() as u8;
                }
            }
            if before == after {
                continue;
            }
            let after = after.iter().any(|b| *b != 0).then_some(after);
            if let Some(data) = &after {
                layer.tiles.tiles.insert(coord, data.clone());
            } else {
                layer.tiles.tiles.remove(&coord);
            }
            changes.push(TileChange {
                coord,
                before: Some(before),
                after,
            });
        }
        if changes.is_empty() {
            return Ok(None);
        }
        let coords = changes.iter().map(|change| change.coord).collect();
        self.record_edit(RasterEdit::Pixels {
            layer_id: layer_id.into(),
            changes,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    pub fn erase_dabs_clipped(
        &mut self,
        layer_id: &str,
        dabs: &[RasterDab],
        selection: Option<&Selection>,
    ) -> Result<Option<TileInvalidation>, String> {
        self.erase_dabs_clipped_in_tiles(layer_id, dabs, selection, None)
    }

    /// Recompute a complete stroke only on selected tiles. None processes all tiles.
    /// Dab order and optical-density accumulation are identical to a full stroke.
    pub fn erase_dabs_clipped_in_tiles(
        &mut self,
        layer_id: &str,
        dabs: &[RasterDab],
        selection: Option<&Selection>,
        targets: Option<&BTreeSet<TileCoord>>,
    ) -> Result<Option<TileInvalidation>, String> {
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if layer.locked || layer.alpha_locked || !layer.visible {
            return Err("Raster layer is hidden or locked".into());
        }
        let density = dab_density_on_tiles(
            self.width,
            self.height,
            dabs,
            selection,
            Some(&layer.tiles.tiles),
            targets,
        )?;
        let mut changes = Vec::new();
        for (coord, coverage) in density {
            let Some(before) = layer.tiles.tiles.get(&coord).cloned() else {
                continue;
            };
            let mut after = before.clone();
            for (pixel, depth) in after.as_chunks_mut::<4>().0.iter_mut().zip(coverage) {
                if depth > 0. && pixel[3] > 0 {
                    pixel[3] = (f32::from(pixel[3]) * (-depth).exp()).round() as u8;
                    if pixel[3] == 0 {
                        pixel.fill(0);
                    }
                }
            }
            if after == before {
                continue;
            }
            let after = after.iter().any(|byte| *byte != 0).then_some(after);
            if let Some(bytes) = &after {
                layer.tiles.tiles.insert(coord, bytes.clone());
            } else {
                layer.tiles.tiles.remove(&coord);
            }
            changes.push(TileChange {
                coord,
                before: Some(before),
                after,
            });
        }
        if changes.is_empty() {
            return Ok(None);
        }
        let coords = changes.iter().map(|change| change.coord).collect();
        self.record_edit(RasterEdit::Pixels {
            layer_id: layer_id.into(),
            changes,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    pub fn paint_dabs_clipped(
        &mut self,
        layer_id: &str,
        dabs: &[RasterDab],
        color: [u8; 3],
        selection: Option<&Selection>,
    ) -> Result<Option<TileInvalidation>, String> {
        self.paint_dabs_clipped_in_tiles(layer_id, dabs, color, selection, None)
    }

    /// Same compositing and history as paint_dabs_clipped, restricted to target tiles.
    pub fn paint_dabs_clipped_in_tiles(
        &mut self,
        layer_id: &str,
        dabs: &[RasterDab],
        color: [u8; 3],
        selection: Option<&Selection>,
        targets: Option<&BTreeSet<TileCoord>>,
    ) -> Result<Option<TileInvalidation>, String> {
        self.paint_brush_dabs_clipped_in_tiles(
            layer_id,
            dabs,
            crate::document::Brush {
                color,
                ..Default::default()
            },
            selection,
            targets,
        )
    }

    pub fn paint_brush_dabs_clipped_in_tiles(
        &mut self,
        layer_id: &str,
        dabs: &[RasterDab],
        brush: crate::document::Brush,
        selection: Option<&Selection>,
        targets: Option<&BTreeSet<TileCoord>>,
    ) -> Result<Option<TileInvalidation>, String> {
        brush.validate()?;
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if !layer.visible || layer.locked {
            return Err("Raster layer is hidden or locked".into());
        }
        let changes = layer.tiles.paint_dabs_with_alpha_lock(
            dabs,
            brush,
            selection,
            layer.alpha_locked,
            targets,
        )?;
        if changes.is_empty() {
            return Ok(None);
        }
        let coords = changes.iter().map(|change| change.coord).collect();
        self.record_edit(RasterEdit::Pixels {
            layer_id: layer_id.into(),
            changes,
        });
        Ok(Some(TileInvalidation {
            layer_id: layer_id.into(),
            coords,
        }))
    }

    pub fn undo(&mut self) -> Result<Option<TileInvalidation>, String> {
        self.apply_history(true)
    }

    pub fn redo(&mut self) -> Result<Option<TileInvalidation>, String> {
        self.apply_history(false)
    }

    pub fn manage_history(
        &mut self,
        store: &dyn crate::history_storage::HistoryStore,
        budget: usize,
        states: usize,
    ) -> Result<usize, String> {
        let excess = (self.undo.len() + self.redo.len()).saturating_sub(states);
        let old_undo = excess.min(self.undo.len());
        self.undo.drain(..old_undo);
        self.redo.drain(..excess - old_undo);
        let size = |entry: &RasterEdit| match entry {
            RasterEdit::Stored(_) => 0,
            RasterEdit::LinkedMaskMove {
                changes,
                masks,
                outside,
                ..
            } => {
                outside.as_ref().map_or(0, |(before, after)| {
                    (before.len() + after.len()) * TILE_BYTES
                }) + changes
                    .iter()
                    .map(|c| {
                        c.before.as_ref().map_or(0, Vec::len) + c.after.as_ref().map_or(0, Vec::len)
                    })
                    .sum::<usize>()
                    + masks
                        .iter()
                        .map(|c| {
                            c.before.as_ref().map_or(0, Vec::len)
                                + c.after.as_ref().map_or(0, Vec::len)
                        })
                        .sum::<usize>()
            }
            RasterEdit::Pixels { changes, .. } => changes
                .iter()
                .map(|change| {
                    change.before.as_ref().map_or(0, Vec::len)
                        + change.after.as_ref().map_or(0, Vec::len)
                })
                .sum(),
            RasterEdit::SparseMaskTransform {
                pixels,
                masks,
                outside,
                ..
            } => {
                outside.as_ref().map_or(0, |(before, after)| {
                    (before.len() + after.len()) * TILE_BYTES
                }) + pixels
                    .iter()
                    .map(|c| {
                        c.before.as_ref().map_or(0, Vec::len) + c.after.as_ref().map_or(0, Vec::len)
                    })
                    .sum::<usize>()
                    + masks
                        .iter()
                        .map(|c| {
                            c.before.as_ref().map_or(0, Vec::len)
                                + c.after.as_ref().map_or(0, Vec::len)
                        })
                        .sum::<usize>()
            }
            RasterEdit::MaskPixels { changes, .. } => changes
                .iter()
                .map(|change| {
                    change.before.as_ref().map_or(0, Vec::len)
                        + change.after.as_ref().map_or(0, Vec::len)
                })
                .sum(),
            RasterEdit::ImportedLayer { layer, .. } => {
                layer.tiles.allocated_tile_count() * TILE_BYTES
            }
            _ => 4096,
        };
        let mut retained = self.undo.iter().chain(&self.redo).map(size).sum::<usize>();
        for history in [&mut self.undo, &mut self.redo] {
            let cold = history.len().saturating_sub(2);
            for entry in history.iter_mut().take(cold) {
                if retained <= budget {
                    break;
                }
                let bytes_count = size(entry);
                if bytes_count == 0 {
                    continue;
                }
                let bytes = serde_json::to_vec(entry).map_err(|error| error.to_string())?;
                *entry = RasterEdit::Stored(store.write(&bytes)?);
                retained = retained.saturating_sub(bytes_count);
            }
        }
        Ok(retained)
    }

    fn record_edit(&mut self, edit: RasterEdit) {
        self.undo.push(edit);
        self.redo.clear();
        self.revision += 1;
    }

    fn apply_history(&mut self, undo: bool) -> Result<Option<TileInvalidation>, String> {
        let source = if undo { &mut self.undo } else { &mut self.redo };
        if let Some(RasterEdit::Stored(blob)) = source.last() {
            let restored =
                serde_json::from_slice(&blob.0.read()?).map_err(|error| error.to_string())?;
            *source.last_mut().unwrap() = restored;
        }
        let source = if undo { &self.undo } else { &self.redo };
        let Some(edit) = source.last() else {
            return Ok(None);
        };
        let invalidation = match edit {
            RasterEdit::Stored(_) => unreachable!("history was restored before applying"),
            RasterEdit::ImportedLayer { layer, index } => {
                if undo {
                    if self.layers.get(*index) != Some(&**layer) {
                        return Err("Imported raster layer changed outside history".into());
                    }
                    self.layers.remove(*index);
                    if self
                        .layer_edit_selection
                        .as_ref()
                        .is_some_and(|(id, _)| id == &layer.id)
                    {
                        self.layer_edit_selection = None;
                    }
                } else {
                    if *index > self.layers.len()
                        || self.layers.iter().any(|item| item.id == layer.id)
                    {
                        return Err("Imported raster layer order changed outside history".into());
                    }
                    self.layers.insert(*index, (**layer).clone());
                }
                TileInvalidation {
                    layer_id: layer.id.clone(),
                    coords: layer.tiles.allocated_coords().collect(),
                }
            }
            RasterEdit::Effects { id, before, after } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *id)
                    .ok_or("Missing effect layer")?;
                if layer.effects != **if undo { after } else { before } {
                    return Err("Layer effects changed outside history".into());
                }
                layer.effects = (**if undo { before } else { after }).clone();
                TileInvalidation {
                    layer_id: id.clone(),
                    coords: layer.tiles.allocated_coords().collect(),
                }
            }
            RasterEdit::StackOrder {
                before,
                after,
                coords,
            } => {
                let expected = if undo { after } else { before };
                let next = if undo { before } else { after };
                if self.layers.iter().map(|l| &l.id).ne(expected.iter()) {
                    return Err("Raster layer order changed outside history".into());
                }
                self.layers
                    .sort_by_key(|l| next.iter().position(|id| id == &l.id).unwrap());
                TileInvalidation {
                    layer_id: next[0].clone(),
                    coords: coords.clone(),
                }
            }
            RasterEdit::Blend {
                layer_id,
                before,
                after,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *layer_id)
                    .ok_or("Raster history layer is missing")?;
                if layer.blend_mode != if undo { *after } else { *before } {
                    return Err("Raster blend mode changed outside history".into());
                }
                let coords = layer.tiles.allocated_coords().collect();
                layer.blend_mode = if undo { *before } else { *after };
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords,
                }
            }
            RasterEdit::Settings {
                before,
                after,
                before_mask_present,
                after_mask_present,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == before.id)
                    .ok_or("Raster history layer is missing")?;
                let (expected, next) = if undo {
                    (after, before)
                } else {
                    (before, after)
                };
                if layer.settings() != *expected
                    || layer.mask_present
                        != if undo {
                            *after_mask_present
                        } else {
                            *before_mask_present
                        }
                {
                    return Err("Raster settings changed outside history".into());
                }
                let affects_pixels = before.opacity != after.opacity
                    || before.mask_enabled != after.mask_enabled
                    || before.mask_inverted != after.mask_inverted
                    || before.mask_density != after.mask_density;
                let coords = if affects_pixels {
                    layer.tiles.allocated_coords().collect()
                } else {
                    Vec::new()
                };
                layer.apply_settings(next);
                layer.mask_present = if undo {
                    *before_mask_present
                } else {
                    *after_mask_present
                };
                TileInvalidation {
                    layer_id: before.id.clone(),
                    coords,
                }
            }
            RasterEdit::Pixels { layer_id, changes } => {
                self.layers
                    .iter_mut()
                    .find(|layer| layer.id == *layer_id)
                    .ok_or("Raster history layer is missing")?
                    .tiles
                    .apply_changes(changes, undo)?;
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords: changes.iter().map(|change| change.coord).collect(),
                }
            }
            RasterEdit::LinkedMaskMove {
                layer_id,
                changes,
                masks,
                outside,
                before,
                after,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *layer_id)
                    .ok_or("Raster history layer is missing")?;
                if let Some((prior, next)) = outside {
                    if layer.off_canvas_tiles != *if undo { next } else { prior } {
                        return Err("Off-canvas pixels changed outside history".into());
                    }
                }
                if layer.effects != **if undo { after } else { before } {
                    return Err("Layer effects changed outside history".into());
                }
                if masks.is_empty() {
                    layer.tiles.apply_changes(changes, undo)?;
                } else {
                    let mut pixels = layer.tiles.clone();
                    let mut mask = layer.mask.clone();
                    pixels.apply_changes(changes, undo)?;
                    mask.apply_changes(masks, undo)?;
                    layer.tiles = pixels;
                    layer.mask = mask;
                }
                if let Some((prior, next)) = outside {
                    layer.off_canvas_tiles = if undo { prior } else { next }.clone();
                }
                layer.effects = (**if undo { before } else { after }).clone();
                let coords = changes
                    .iter()
                    .map(|c| c.coord)
                    .chain(layer.tiles.allocated_coords())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords,
                }
            }
            RasterEdit::Appearance {
                layer_id,
                before,
                after,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == *layer_id)
                    .ok_or("Raster history layer is missing")?;
                if (layer.visible, layer.opacity) != if undo { *after } else { *before } {
                    return Err("Raster appearance changed outside history".into());
                }
                let coords = layer.tiles.allocated_coords().collect();
                (layer.visible, layer.opacity) = if undo { *before } else { *after };
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords,
                }
            }
            RasterEdit::Locks {
                layer_id,
                before,
                after,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == *layer_id)
                    .ok_or("Raster history layer is missing")?;
                if (layer.locked, layer.alpha_locked) != if undo { *after } else { *before } {
                    return Err("Raster locks changed outside history".into());
                }
                (layer.locked, layer.alpha_locked) = if undo { *before } else { *after };
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords: Vec::new(),
                }
            }
            RasterEdit::Mask {
                layer_id,
                before,
                after,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == *layer_id)
                    .ok_or("Raster history layer is missing")?;
                if (
                    layer.mask_enabled,
                    layer.mask_inverted,
                    layer.mask_density,
                    layer.mask_present,
                ) != if undo { *after } else { *before }
                {
                    return Err("Raster mask changed outside history".into());
                }
                let coords = layer.tiles.allocated_coords().collect();
                (
                    layer.mask_enabled,
                    layer.mask_inverted,
                    layer.mask_density,
                    layer.mask_present,
                ) = if undo { *before } else { *after };
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords,
                }
            }
            RasterEdit::SparseMaskTransform {
                layer_id,
                pixels,
                masks,
                outside,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *layer_id)
                    .ok_or("Raster history layer is missing")?;
                if let Some((before, after)) = outside {
                    if layer.off_canvas_tiles != *if undo { after } else { before } {
                        return Err("Off-canvas pixels changed outside history".into());
                    }
                }
                // Validate both deltas before applying either, preserving atomic history.
                let mut next_pixels = layer.tiles.clone();
                let mut next_mask = layer.mask.clone();
                next_pixels.apply_changes(pixels, undo)?;
                next_mask.apply_changes(masks, undo)?;
                let coords = pixels
                    .iter()
                    .map(|c| c.coord)
                    .chain(masks.iter().map(|c| c.coord))
                    .chain(layer.tiles.allocated_coords())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                layer.tiles = next_pixels;
                layer.mask = next_mask;
                if let Some((before, after)) = outside {
                    layer.off_canvas_tiles = if undo { before } else { after }.clone();
                }
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords,
                }
            }
            RasterEdit::SparseMaskLink {
                layer_id,
                before,
                after,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *layer_id)
                    .ok_or("Raster history layer is missing")?;
                if layer.mask_linked != if undo { *after } else { *before } {
                    return Err("Mask link changed outside history".into());
                }
                layer.mask_linked = if undo { *before } else { *after };
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords: Vec::new(),
                }
            }
            RasterEdit::MaskPixels {
                layer_id,
                changes,
                before_mask_present,
            } => {
                let layer = self
                    .layers
                    .iter_mut()
                    .find(|layer| layer.id == *layer_id)
                    .ok_or("Raster history layer is missing")?;
                if layer.mask_present != if undo { true } else { *before_mask_present } {
                    return Err("Raster mask presence changed outside history".into());
                }
                layer.mask.apply_changes(changes, undo)?;
                layer.mask_present = if undo { *before_mask_present } else { true };
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords: changes.iter().map(|change| change.coord).collect(),
                }
            }
            RasterEdit::Reorder {
                layer_id,
                from,
                to,
                coords,
            } => {
                let current = if undo { *to } else { *from };
                let destination = if undo { *from } else { *to };
                if self.layers.get(current).map(|layer| layer.id.as_str())
                    != Some(layer_id.as_str())
                {
                    return Err("Raster layer order changed outside history".into());
                }
                let layer = self.layers.remove(current);
                self.layers.insert(destination, layer);
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords: coords.clone(),
                }
            }
        };
        let edit = if undo {
            self.undo.pop().unwrap()
        } else {
            self.redo.pop().unwrap()
        };
        if undo {
            self.redo.push(edit);
        } else {
            self.undo.push(edit);
        }
        self.revision += 1;
        Ok(Some(invalidation))
    }
}

fn div_255(value: u32) -> u32 {
    (value + 127) / 255
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_filtered_grain_matches_uncached_for_varying_dab_geometry() {
        use crate::natural_media::{MaterialSettings, Medium, PaperKind, PaperSettings};
        for (enabled, medium, edge) in [
            (false, Medium::Oil, 0.3),
            (true, Medium::Oil, 0.3),
            (true, Medium::Watercolor, 0.3),
            (true, Medium::Watercolor, 0.0),
        ] {
            let material = MaterialSettings {
                enabled,
                medium,
                edge,
                grain_transform: [64., 0., 0., 64., 0., 0.],
                paper: PaperSettings {
                    enabled: true,
                    kind: PaperKind::Washi,
                    texture_transform: [64., 0., 0., 64., 0., 0.],
                    ..Default::default()
                },
                ..Default::default()
            };
            let dabs: Vec<_> = (0..20)
                .map(|i| RasterDab {
                    material,
                    texture: if enabled { 0. } else { 1. },
                    texture_scale: 1.,
                    x: 8. + i as f32 * 0.3,
                    y: 10.,
                    radius: 1. + i as f32 * 0.1,
                    hardness: 0.6,
                    weight: 0.03,
                })
                .collect();
            let mut uncached = dabs.clone();
            // Paper color affects neither relief nor deposition, but makes the
            // settings nonuniform and exercises the uncached path.
            uncached[0].material.paper.color = [1, 2, 3];
            assert_eq!(
                dab_density(32, 32, &dabs, None).unwrap(),
                dab_density(32, 32, &uncached, None).unwrap()
            );
        }
    }

    #[test]
    #[ignore = "Manual timing comparison"]
    fn filtered_grain_cache_timing_with_variable_pressure() {
        use crate::natural_media::{MaterialSettings, Medium, PaperKind, PaperSettings};
        use std::time::Instant;
        for medium in [
            Medium::Graphite,
            Medium::Charcoal,
            Medium::Pastel,
            Medium::Gouache,
            Medium::Oil,
        ] {
            let material = MaterialSettings {
                enabled: true,
                medium,
                grain_transform: [64., 0., 0., 64., 0., 0.],
                paper: PaperSettings {
                    enabled: true,
                    kind: PaperKind::Washi,
                    texture_transform: [64., 0., 0., 64., 0., 0.],
                    ..Default::default()
                },
                ..Default::default()
            };
            let dabs: Vec<_> = (0..300)
                .map(|i| RasterDab {
                    material,
                    texture: 0.,
                    texture_scale: 1.,
                    x: 4. + i as f32 * 0.08,
                    y: 15.,
                    radius: 0.7 + (i % 50) as f32 * 0.06,
                    hardness: 0.6,
                    weight: 0.03,
                })
                .collect();
            let mut uncached = dabs.clone();
            uncached[0].material.paper.color = [1, 2, 3];
            let start = Instant::now();
            let before = dab_density(40, 32, &uncached, None).unwrap();
            let t0 = start.elapsed();
            let start = Instant::now();
            let after = dab_density(40, 32, &dabs, None).unwrap();
            let t1 = start.elapsed();
            assert_eq!(before, after);
            eprintln!("{medium:?}: uncached {t0:?}, cached {t1:?}");
        }
    }

    #[test]
    fn reduced_dry_brush_averages_grain_over_pixel_footprint() {
        let mut dab = RasterDab {
            material: Default::default(),
            texture: 2.,
            texture_scale: 1.,
            x: 10.5,
            y: 10.5,
            radius: 10.,
            hardness: 1.,
            weight: 1.,
        };
        dab.material.grain_transform = [2., 0., 0., 2., 0., 0.];
        let density = dab_density(32, 32, &[dab], None).unwrap();
        let actual = density[&TileCoord { x: 0, y: 0 }][(10 * TILE_SIZE + 10) as usize];
        let expected = [-0.25, 0.25]
            .iter()
            .flat_map(|dy| {
                [-0.25, 0.25]
                    .iter()
                    .map(move |dx| brush_texture((10.5 + dx) * 2., (10.5 + dy) * 2., 2.))
            })
            .sum::<f32>()
            / 4.
            * 16.;
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }

    #[test]
    fn targeted_dabs_match_full_stroke_preserve_other_tiles_and_undo() {
        let mut base = TiledRasterDocument::new(768, 256).unwrap();
        base.add_layer("paint".into(), "Paint".into()).unwrap();
        let dabs = [RasterDab {
            material: Default::default(),
            texture: 1.,
            texture_scale: 1.,
            x: 256.,
            y: 120.,
            radius: 60.,
            hardness: 0.3,
            weight: 0.4,
        }];
        base.paint_dabs_clipped("paint", &dabs, [50, 80, 100], None)
            .unwrap();
        base.discard_history();
        let targets = BTreeSet::from([TileCoord { x: 1, y: 0 }]);
        let selection = Selection::new(
            crate::selection::SelectionShape::Rectangle,
            [240., 80., 60., 60.],
        );
        for erase in [false, true] {
            let mut full = base.clone();
            let mut partial = base.clone();
            let edit = |doc: &mut TiledRasterDocument, targets| {
                if erase {
                    doc.erase_dabs_clipped_in_tiles("paint", &dabs, Some(&selection), targets)
                } else {
                    doc.paint_dabs_clipped_in_tiles(
                        "paint",
                        &dabs,
                        [200, 30, 70],
                        Some(&selection),
                        targets,
                    )
                }
            };
            edit(&mut full, None).unwrap();
            let changes = edit(&mut partial, Some(&targets)).unwrap().unwrap();
            assert_eq!(changes.coords, vec![TileCoord { x: 1, y: 0 }]);
            assert_eq!(
                partial.layers()[0].tiles.tile(TileCoord { x: 1, y: 0 }),
                full.layers()[0].tiles.tile(TileCoord { x: 1, y: 0 })
            );
            assert_eq!(
                partial.layers()[0].tiles.tile(TileCoord { x: 0, y: 0 }),
                base.layers()[0].tiles.tile(TileCoord { x: 0, y: 0 })
            );
            let edited = partial.state();
            partial.undo().unwrap();
            assert_eq!(partial.state(), base.state());
            partial.redo().unwrap();
            assert_eq!(partial.state(), edited);
            let mut invalid = dabs;
            invalid[0].x = f32::NAN;
            assert!(partial
                .paint_dabs_clipped_in_tiles("paint", &invalid, [0; 3], None, Some(&targets))
                .is_err());
            assert_eq!(partial.state(), edited);
        }
    }

    #[test]
    fn layer_effects_preserve_pixels_validate_atomically_and_undo() {
        let mut d = TiledRasterDocument::new(1, 1).unwrap();
        d.add_layer("image".into(), "Image".into()).unwrap();
        d.write_rect("image", [0, 0, 1, 1], &[64, 64, 64, 128])
            .unwrap();
        d.discard_history();
        let original = d.state();
        let e = crate::layer_effects::LayerEffects {
            enabled: true,
            grading_blend: 75.,
            grading_balance: -35.,
            values: [1., 0., 0., 0., 0., 0., 0., 0., 0., 0.],
            ..Default::default()
        };
        let update = d.set_layer_effects("image", e.clone()).unwrap().unwrap();
        assert_eq!(update.coords, vec![TileCoord { x: 0, y: 0 }]);
        assert_eq!(d.state().layers[0].tiles, original.layers[0].tiles);
        assert_eq!(
            &d.prepare_uploads(&update).unwrap()[0].pixels[..4],
            &[45, 45, 45, 128]
        );
        assert!(d.set_layer_effects("image", e.clone()).unwrap().is_none());
        let changed = d.state();
        let mut invalid = e;
        invalid.grading_balance = 101.;
        assert!(d.set_layer_effects("image", invalid).is_err());
        assert_eq!(d.state(), changed);
        d.undo().unwrap();
        assert_eq!(d.state(), original);
        assert!(!d.can_undo());
        d.redo().unwrap();
        assert_eq!(d.state(), changed);
        let mut legacy = serde_json::to_value(original).unwrap();
        legacy["layers"][0]
            .as_object_mut()
            .unwrap()
            .remove("effects");
        let state: TiledRasterState = serde_json::from_value(legacy).unwrap();
        assert!(
            !TiledRasterDocument::from_state(state).unwrap().layers()[0]
                .effects
                .enabled
        );
    }

    #[test]
    fn full_layer_order_is_atomic_single_undo_and_invalidates_crossed_tiles() {
        let mut doc = TiledRasterDocument::new(1024, 1).unwrap();
        for (id, x, color) in [
            ("fixed", 768, [0, 255, 0, 255]),
            ("back", 0, [255, 0, 0, 255]),
            ("front", 0, [0, 0, 255, 255]),
        ] {
            doc.add_layer(id.into(), id.into()).unwrap();
            doc.write_rect(id, [x, 0, 1, 1], &color).unwrap();
        }
        doc.write_mask_rect("back", [0, 0, 1, 1], &[128]).unwrap();
        doc.discard_history();
        let original = doc.state();
        let revision = doc.revision();
        let coord = TileCoord { x: 0, y: 0 };
        assert_eq!(&doc.composite_tile(coord).unwrap()[..4], &[0, 0, 255, 255]);
        let order = vec!["back".into(), "front".into(), "fixed".into()];
        let update = doc.reorder_layers(&order).unwrap().unwrap();
        assert_eq!(update.coords, vec![coord]);
        let uploads = doc.prepare_uploads(&update).unwrap();
        assert_eq!(uploads.len(), 1);
        assert_eq!(&uploads[0].pixels[..4], &[255, 0, 0, 255]);
        assert_eq!(doc.revision(), revision + 1);
        assert_eq!(&doc.composite_tile(coord).unwrap()[..4], &[255, 0, 0, 255]);
        let reordered = doc.state();
        for layer in &reordered.layers {
            assert_eq!(
                layer,
                original.layers.iter().find(|l| l.id == layer.id).unwrap()
            );
        }
        assert!(doc.reorder_layers(&order).unwrap().is_none());
        assert_eq!(doc.revision(), revision + 1);
        for invalid in [
            vec!["back".into(), "back".into(), "fixed".into()],
            vec!["missing".into(), "front".into(), "fixed".into()],
            vec!["fixed".into()],
        ] {
            assert!(doc.reorder_layers(&invalid).is_err());
            assert_eq!(doc.state(), reordered);
        }
        doc.undo().unwrap().unwrap();
        assert_eq!(doc.state(), original);
        assert!(!doc.can_undo());
        assert!(doc.reorder_layers(&["missing".into()]).is_err());
        assert!(doc.can_redo());
        doc.redo().unwrap().unwrap();
        assert_eq!(doc.state(), reordered);
    }

    #[test]
    fn blend_edit_invalidates_owned_tiles_and_is_atomic_undoable() {
        let mut doc = TiledRasterDocument::new(257, 1).unwrap();
        doc.add_layer("top".into(), "Top".into()).unwrap();
        doc.write_rect(
            "top",
            [255, 0, 2, 1],
            &[64, 128, 192, 255, 128, 64, 32, 128],
        )
        .unwrap();
        doc.discard_history();
        let original = doc.state();
        let revision = doc.revision();
        let update = doc
            .set_layer_blend_mode("top", RasterBlendMode::Multiply)
            .unwrap()
            .unwrap();
        assert_eq!(
            update.coords,
            vec![TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 0 }]
        );
        assert_eq!(doc.revision(), revision + 1);
        assert_eq!(doc.state().layers[0].tiles, original.layers[0].tiles);
        assert!(doc
            .set_layer_blend_mode("top", RasterBlendMode::Multiply)
            .unwrap()
            .is_none());
        assert_eq!(doc.revision(), revision + 1);
        assert!(doc
            .set_layer_blend_mode("missing", RasterBlendMode::Screen)
            .is_err());
        assert_eq!(doc.undo().unwrap().unwrap().coords, update.coords);
        assert_eq!(doc.state(), original);
        assert!(doc.can_redo());
        doc.redo().unwrap();
        assert_eq!(doc.layers()[0].blend_mode, RasterBlendMode::Multiply);
        doc.undo().unwrap();
        doc.set_layer_blend_mode("top", RasterBlendMode::Screen)
            .unwrap();
        assert!(!doc.can_redo());
        doc.set_layer_locks("top", true, false).unwrap();
        let locked = doc.state();
        let revision = doc.revision();
        assert!(doc
            .set_layer_blend_mode("top", RasterBlendMode::Difference)
            .is_err());
        assert_eq!(doc.state(), locked);
        assert_eq!(doc.revision(), revision);
    }
    #[test]
    fn blend_modes_have_expected_opaque_pixels_and_partial_alpha() {
        let coord = TileCoord { x: 0, y: 0 };
        let mut document = TiledRasterDocument::new(1, 1).unwrap();
        document.add_layer("base".into(), "Base".into()).unwrap();
        document.add_layer("top".into(), "Top".into()).unwrap();
        document
            .write_rect("base", [0, 0, 1, 1], &[64, 128, 192, 255])
            .unwrap();
        document
            .write_rect("top", [0, 0, 1, 1], &[128, 64, 128, 255])
            .unwrap();
        for (mode, expected) in [
            (RasterBlendMode::Multiply, [32, 32, 96, 255]),
            (RasterBlendMode::Screen, [160, 160, 224, 255]),
            (RasterBlendMode::Overlay, [64, 65, 192, 255]),
            (RasterBlendMode::HardLight, [65, 64, 192, 255]),
            (RasterBlendMode::Darken, [64, 64, 128, 255]),
            (RasterBlendMode::Lighten, [128, 128, 192, 255]),
            (RasterBlendMode::Difference, [64, 64, 64, 255]),
            (RasterBlendMode::Exclusion, [128, 128, 127, 255]),
        ] {
            document.layers[1].blend_mode = mode;
            assert_eq!(&document.composite_tile(coord).unwrap()[..4], &expected);
            assert_eq!(
                composite_raster_layers_tile(&document.state().layers, coord),
                document.composite_tile(coord)
            );
        }
        document
            .write_rect("base", [0, 0, 1, 1], &[0, 0, 0, 128])
            .unwrap();
        document
            .write_rect("top", [0, 0, 1, 1], &[255, 255, 255, 128])
            .unwrap();
        document.layers[1].blend_mode = RasterBlendMode::Multiply;
        assert_eq!(
            &document.composite_tile(coord).unwrap()[..4],
            &[64, 64, 64, 192]
        );
        document.layers[0].visible = false;
        assert_eq!(
            &document.composite_tile(coord).unwrap()[..4],
            &[128, 128, 128, 128]
        );
    }
    #[test]
    fn paper_grain_is_fixed_and_has_dry_gaps() {
        let grains: Vec<_> = (0..100)
            .map(|x| brush_texture(x as f32 + 0.5, 8.5, 2.0))
            .collect();
        assert!(grains.contains(&0.0));
        assert!(grains.iter().any(|value| *value > 0.0));
        for x in 0..100 {
            assert_eq!(
                brush_texture(x as f32 + 0.25, 8.25, 1.0),
                brush_texture(x as f32 + 0.75, 8.75, 1.0)
            );
            assert_eq!(brush_texture(x as f32, 8.0, 0.0), 1.0);
        }
    }

    #[test]
    fn integer_translation_preserves_transparent_rgb_across_canvas_edge() {
        let mut doc = TiledRasterDocument::new(2, 1).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        doc.write_rect("paint", [0, 0, 1, 1], &[12, 34, 56, 0])
            .unwrap();
        let layer = &doc.layers()[0];
        let (pixels, outside) = layer
            .tiles
            .translated_with_off_canvas(&layer.off_canvas_tiles, -1, 0)
            .unwrap();
        assert_eq!(pixels.pixel(0, 0), Some([0, 0, 0, 0]));
        assert_eq!(&outside[0].pixels[1020..1024], &[12, 34, 56, 0]);
        let (returned, outside) = pixels.translated_with_off_canvas(&outside, 1, 0).unwrap();
        assert!(outside.is_empty());
        assert_eq!(returned.pixel(0, 0), Some([12, 34, 56, 0]));
        assert!(returned
            .transformed_with_off_canvas(&outside, [1., 0., 0., 1., 0.5, 0.])
            .is_err());
    }

    #[test]
    fn affine_off_canvas_sampling_and_linked_mask_history() {
        let mut source = TiledRasterDocument::new(2, 2).unwrap();
        source.add_layer("a".into(), "Layer".into()).unwrap();
        source
            .write_rect("a", [1, 0, 1, 1], &[0, 0, 220, 255])
            .unwrap();
        let mut state = source.state();
        state.layers[0].mask_present = true;
        let mut tile = vec![0; TILE_BYTES];
        let at = (TILE_SIZE as usize - 1) * 4;
        tile[at..at + 4].copy_from_slice(&[240, 0, 0, 255]);
        state.layers[0].off_canvas_tiles.push(OffCanvasTileState {
            x: -1,
            y: 0,
            pixels: tile,
        });
        let mut doc = TiledRasterDocument::from_state(state.clone()).unwrap();
        let layer = &doc.layers()[0];
        let (rotated, rotated_outside) = layer
            .tiles
            .transformed_with_off_canvas(&layer.off_canvas_tiles, [0., 1., -1., 0., 1., 1.])
            .unwrap();
        assert_eq!(rotated.pixel(0, 0), Some([240, 0, 0, 255]));
        let tile = rotated_outside
            .iter()
            .find(|t| t.x == 0 && t.y == 0)
            .unwrap();
        let offset = 2 * TILE_SIZE as usize * 4;
        assert_eq!(&tile.pixels[offset..offset + 4], &[0, 0, 220, 255]);
        let mut hidden = layer.off_canvas_tiles.clone();
        let hidden_offset = (TILE_SIZE as usize - 1) * 4 + 3;
        hidden[0].pixels[hidden_offset] = 0;
        assert!(layer
            .tiles
            .transformed_with_off_canvas(&hidden, [1., 0., 0., 1., 0.5, 0.])
            .is_err());
        assert!(layer
            .tiles
            .transformed_with_off_canvas(
                &layer.off_canvas_tiles,
                [100_000., 0., 0., 100_000., 0., 0.]
            )
            .is_err());
        let matrix = [1., 0., 0., 1., 0.5, 0.];
        let (pixels, outside) = layer
            .tiles
            .transformed_with_off_canvas(&layer.off_canvas_tiles, matrix)
            .unwrap();
        assert_eq!(pixels.pixel(0, 0), Some([240, 0, 0, 128]));
        assert_eq!(pixels.pixel(1, 0), Some([0, 0, 220, 128]));
        assert!(outside
            .iter()
            .any(|t| t.pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 128)));
        let mask = layer.mask.transformed(matrix).unwrap();
        doc.transform_sparse_mask_with_off_canvas("a", Some(pixels), mask, Some(outside))
            .unwrap();
        let after = doc.state();
        doc.undo().unwrap();
        assert_eq!(doc.state(), state);
        doc.redo().unwrap();
        assert_eq!(doc.state(), after);
        let layer = &doc.layers()[0];
        let (rotated, overflow) = layer
            .tiles
            .transformed_with_off_canvas(&layer.off_canvas_tiles, [0., 1., -1., 0., 1., 0.])
            .unwrap();
        assert!(!rotated.tiles.is_empty() || !overflow.is_empty());
        let (scaled, overflow) = layer
            .tiles
            .transformed_with_off_canvas(&layer.off_canvas_tiles, [2., 0., 0., 2., 0., 0.])
            .unwrap();
        assert!(!scaled.tiles.is_empty() && !overflow.is_empty());
    }

    #[test]
    fn linked_integer_translation_preserves_off_canvas_pixels_with_one_undo() {
        let mut source = TiledRasterDocument::new(2, 1).unwrap();
        source.add_layer("a".into(), "Layer".into()).unwrap();
        source
            .write_rect("a", [1, 0, 1, 1], &[0, 0, 220, 255])
            .unwrap();
        let mut state = source.state();
        state.layers[0].mask_present = true;
        let mut tile = vec![0; TILE_BYTES];
        let at = (TILE_SIZE as usize - 1) * 4;
        tile[at..at + 4].copy_from_slice(&[240, 0, 0, 255]);
        state.layers[0].off_canvas_tiles.push(OffCanvasTileState {
            x: -1,
            y: 0,
            pixels: tile,
        });
        let mut doc = TiledRasterDocument::from_state(state.clone()).unwrap();
        let layer = &doc.layers()[0];
        let (moved, outside) = layer
            .tiles
            .translated_with_off_canvas(&layer.off_canvas_tiles, 1, 0)
            .unwrap();
        let mask = layer.mask.translated(1, 0);
        doc.transform_sparse_mask_with_off_canvas("a", Some(moved), mask, Some(outside))
            .unwrap();
        assert_eq!(doc.layers()[0].tiles.pixel(0, 0), Some([240, 0, 0, 255]));
        let after = doc.state();
        let tile = &after.layers[0].off_canvas_tiles[0];
        assert_eq!(&tile.pixels[8..12], &[0, 0, 220, 255]);
        doc.undo().unwrap();
        assert_eq!(doc.state(), state);
        doc.redo().unwrap();
        assert_eq!(doc.state(), after);
    }

    #[test]
    fn sparse_eraser_density_matches_full_density_without_allocating_empty_tiles() {
        let coord = TileCoord { x: 0, y: 0 };
        let occupied = BTreeMap::from([(coord, vec![255; TILE_BYTES])]);
        let dabs = [RasterDab {
            material: Default::default(),
            texture: 0.0,
            texture_scale: 1.0,
            x: TILE_SIZE as f32,
            y: TILE_SIZE as f32,
            radius: 80.0,
            hardness: 0.35,
            weight: 1.0,
        }];
        let full = dab_density(TILE_SIZE * 2, TILE_SIZE * 2, &dabs, None).unwrap();
        let sparse = dab_density_on_tiles(
            TILE_SIZE * 2,
            TILE_SIZE * 2,
            &dabs,
            None,
            Some(&occupied),
            None,
        )
        .unwrap();
        assert_eq!(sparse.len(), 1);
        assert_eq!(sparse[&coord], full[&coord]);
        let empty = BTreeMap::new();
        assert!(dab_density_on_tiles(
            TILE_SIZE * 2,
            TILE_SIZE * 2,
            &dabs,
            None,
            Some(&empty),
            None
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn effect_mask_and_off_canvas_transform_share_history() {
        let mut doc = TiledRasterDocument::new(3, 2).unwrap();
        doc.add_layer("a".into(), "A".into()).unwrap();
        doc.write_rect("a", [2, 0, 1, 1], &[20, 30, 40, 255])
            .unwrap();
        doc.write_mask_rect("a", [2, 0, 1, 1], &[64]).unwrap();
        let effects = crate::layer_effects::LayerEffects {
            mask: Some(
                crate::layer_mask::LayerMask::from_selection(
                    crate::layer_mask::MaskKind::Pixel,
                    None,
                    3,
                    2,
                )
                .unwrap(),
            ),
            ..Default::default()
        };
        doc.set_layer_effects("a", effects).unwrap();
        let mut state = doc.state();
        let mut tile = vec![0; TILE_BYTES];
        let at = (TILE_SIZE as usize - 1) * 4;
        tile[at..at + 4].copy_from_slice(&[240, 0, 0, 255]);
        state.layers[0].off_canvas_tiles.push(OffCanvasTileState {
            x: -1,
            y: 0,
            pixels: tile,
        });
        let mut doc = TiledRasterDocument::from_state(state.clone()).unwrap();
        let layer = &doc.layers()[0];
        let matrix = [1., 0., 0., 1., 1., 0.];
        let (pixels, outside) = layer
            .tiles
            .transformed_with_off_canvas(&layer.off_canvas_tiles, matrix)
            .unwrap();
        let mask = layer.mask.transformed(matrix).unwrap();
        doc.transform_linked_effect_mask_with_off_canvas(
            "a",
            matrix,
            pixels,
            Some(mask),
            Some(outside),
        )
        .unwrap();
        assert_eq!(doc.layers()[0].tiles.pixel(0, 0), Some([240, 0, 0, 255]));
        assert_eq!(
            doc.layers()[0].effects.mask.as_ref().unwrap().transform,
            matrix
        );
        let after = doc.state();
        doc.undo().unwrap();
        assert_eq!(doc.state(), state);
        doc.redo().unwrap();
        assert_eq!(doc.state(), after);
    }

    #[test]
    fn coexisting_linked_masks_transform_with_one_undo() {
        for sparse_linked in [true, false] {
            let mut doc = TiledRasterDocument::new(300, 3).unwrap();
            doc.add_layer("a".into(), "A".into()).unwrap();
            doc.write_rect("a", [255, 0, 1, 1], &[20, 30, 40, 255])
                .unwrap();
            doc.write_mask_rect("a", [255, 0, 1, 1], &[64]).unwrap();
            doc.set_sparse_mask_linked("a", sparse_linked).unwrap();
            let effects = crate::layer_effects::LayerEffects {
                mask: Some(
                    crate::layer_mask::LayerMask::from_selection(
                        crate::layer_mask::MaskKind::Pixel,
                        None,
                        300,
                        3,
                    )
                    .unwrap(),
                ),
                ..Default::default()
            };
            doc.set_layer_effects("a", effects).unwrap();
            doc.discard_history();
            let before = doc.state();
            let matrix = [1., 0., 0., 1., 1., 1.];
            let pixels = doc.layers()[0].tiles.transformed(matrix).unwrap();
            let mask = sparse_linked.then(|| doc.layers()[0].mask.transformed(matrix).unwrap());
            doc.transform_linked_effect_mask("a", matrix, pixels, mask)
                .unwrap();
            assert_eq!(doc.layers()[0].tiles.pixel(256, 1), Some([20, 30, 40, 255]));
            assert_eq!(
                doc.layers()[0].effects.mask.as_ref().unwrap().transform,
                matrix
            );
            assert_eq!(
                doc.layers()[0].mask.pixel(
                    if sparse_linked { 256 } else { 255 },
                    if sparse_linked { 1 } else { 0 }
                ),
                Some(64)
            );
            let after = doc.state();
            doc.undo().unwrap();
            assert_eq!(doc.state(), before);
            assert!(!doc.can_undo());
            doc.redo().unwrap();
            assert_eq!(doc.state(), after);
        }
    }

    #[test]
    fn sparse_mask_link_and_transform_are_undoable_and_atomic() {
        let mut doc = TiledRasterDocument::new(300, 3).unwrap();
        doc.add_layer("a".into(), "A".into()).unwrap();
        doc.write_rect("a", [255, 0, 1, 1], &[20, 30, 40, 255])
            .unwrap();
        doc.write_mask_rect("a", [255, 0, 1, 1], &[64]).unwrap();
        doc.discard_history();
        let before = doc.state();
        let matrix = [1., 0., 0., 1., 1., 1.];
        let pixels = doc.layers()[0].tiles.transformed(matrix).unwrap();
        let mask = doc.layers()[0].mask.transformed(matrix).unwrap();
        doc.transform_sparse_mask("a", Some(pixels), mask).unwrap();
        assert_eq!(doc.layers()[0].tiles.pixel(256, 1), Some([20, 30, 40, 255]));
        assert_eq!(doc.layers()[0].mask.pixel(256, 1), Some(64));
        let after = doc.state();
        doc.undo().unwrap();
        assert_eq!(doc.state(), before);
        assert!(!doc.can_undo());
        doc.redo().unwrap();
        assert_eq!(doc.state(), after);
        doc.set_sparse_mask_linked("a", false).unwrap();
        let pixels_before = doc.layers()[0].tiles.clone();
        let moved_mask = doc.layers()[0]
            .mask
            .transformed([1., 0., 0., 1., 0.5, 0.])
            .unwrap();
        doc.transform_sparse_mask("a", None, moved_mask).unwrap();
        assert_eq!(doc.layers()[0].tiles, pixels_before);
        assert_eq!(doc.layers()[0].mask.pixel(256, 1), Some(159));
        doc.undo().unwrap();
        doc.undo().unwrap();
        assert!(doc.layers()[0].mask_linked);
        doc.redo().unwrap();
        assert!(!doc.layers()[0].mask_linked);
    }

    #[test]
    fn affine_linked_mask_resamples_and_undoes_as_one_edit() {
        let mut doc = TiledRasterDocument::new(8, 8).unwrap();
        doc.add_layer("a".into(), "A".into()).unwrap();
        doc.write_rect("a", [1, 1, 1, 1], &[200, 10, 20, 255])
            .unwrap();
        let effects = crate::layer_effects::LayerEffects {
            mask: Some(
                crate::layer_mask::LayerMask::from_selection(
                    crate::layer_mask::MaskKind::Pixel,
                    None,
                    8,
                    8,
                )
                .unwrap(),
            ),
            ..Default::default()
        };
        doc.set_layer_effects("a", effects).unwrap();
        doc.discard_history();
        let before = doc.state();
        let matrix = [1., 0., 0., 1., 0.5, 0.];
        let moved = doc.layers()[0].tiles.transformed(matrix).unwrap();
        assert_eq!(moved.pixel(1, 1), Some([200, 10, 20, 128]));
        assert_eq!(moved.pixel(2, 1), Some([200, 10, 20, 128]));
        doc.transform_linked_effect_mask("a", matrix, moved, None)
            .unwrap();
        assert_eq!(
            doc.layers()[0].effects.mask.as_ref().unwrap().transform,
            matrix
        );
        let after = doc.state();
        doc.undo().unwrap();
        assert_eq!(doc.state(), before);
        doc.redo().unwrap();
        assert_eq!(doc.state(), after);
        let rotation = [0., 1., -1., 0., 4., 0.];
        let rotated = TiledRasterDocument::from_state(before).unwrap().layers()[0]
            .tiles
            .transformed(rotation)
            .unwrap();
        assert_eq!(rotated.pixel(2, 1), Some([200, 10, 20, 255]));
        let scaled = TiledRasterDocument::from_state(after).unwrap().layers()[0]
            .tiles
            .transformed([2., 0., 0., 2., 0., 0.])
            .unwrap();
        assert!(scaled.pixel(3, 3).unwrap()[3] > 0);
        assert!(scaled.transformed([0., 0., 0., 0., 0., 0.]).is_err());
    }

    #[test]
    fn linked_effect_mask_move_is_one_undo_and_respects_locks() {
        let mut doc = TiledRasterDocument::new(300, 2).unwrap();
        doc.add_layer("a".into(), "A".into()).unwrap();
        doc.write_rect("a", [255, 0, 1, 1], &[1, 2, 3, 255])
            .unwrap();
        let effects = crate::layer_effects::LayerEffects {
            mask: Some(
                crate::layer_mask::LayerMask::from_selection(
                    crate::layer_mask::MaskKind::Pixel,
                    None,
                    300,
                    2,
                )
                .unwrap(),
            ),
            ..Default::default()
        };
        doc.set_layer_effects("a", effects).unwrap();
        doc.discard_history();
        let before = doc.state();
        let moved = doc.layers()[0].tiles.translated(1, 1);
        doc.transform_linked_effect_mask("a", [1., 0., 0., 1., 1., 1.], moved, None)
            .unwrap();
        assert_eq!(doc.layers()[0].tiles.pixel(256, 1), Some([1, 2, 3, 255]));
        assert_eq!(
            doc.layers()[0].effects.mask.as_ref().unwrap().transform,
            [1., 0., 0., 1., 1., 1.]
        );
        let after = doc.state();
        doc.undo().unwrap();
        assert_eq!(doc.state(), before);
        assert!(!doc.can_undo());
        doc.redo().unwrap();
        assert_eq!(doc.state(), after);
        let mut effects = doc.layers()[0].effects.clone();
        effects.mask.as_mut().unwrap().linked = false;
        doc.set_layer_effects("a", effects).unwrap();
        let moved = doc.layers()[0].tiles.translated(1, 0);
        assert!(doc
            .transform_linked_effect_mask("a", [1., 0., 0., 1., 1., 0.], moved, None)
            .is_err());
    }

    #[test]
    fn sparse_translation_moves_pixels_and_mask_across_tiles_and_clips() {
        let mut pixels = SparseTiles::new(300, 2).unwrap();
        pixels
            .write_rect(255, 0, 2, 1, &[10, 20, 30, 255, 1, 2, 3, 128])
            .unwrap();
        let mut mask = SparseMask::new(300, 2);
        mask.write_rect([255, 0, 2, 1], &[0, 127]).unwrap();
        let moved = pixels.translated(1, 1);
        let moved_mask = mask.translated(1, 1);
        assert_eq!(moved.pixel(256, 1), Some([10, 20, 30, 255]));
        assert_eq!(moved.pixel(257, 1), Some([1, 2, 3, 128]));
        assert_eq!(moved_mask.pixel(256, 1), Some(0));
        assert_eq!(moved_mask.pixel(257, 1), Some(127));
        assert_eq!(moved_mask.pixel(255, 0), Some(255));
        assert_eq!(pixels.translated(i32::MAX, 0).allocated_tile_count(), 0);
        assert_eq!(mask.translated(i32::MIN, 0).allocated_tile_count(), 0);
        assert_eq!(pixels.translated(0, 0), pixels);
        assert_eq!(mask.translated(0, 0), mask);
    }

    #[test]
    fn eraser_removes_alpha_and_undo_restores_pixels() {
        let mut doc = TiledRasterDocument::new(32, 32).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        doc.write_rect("paint", [10, 10, 1, 1], &[200, 50, 20, 255])
            .unwrap();
        doc.write_rect("paint", [1, 1, 1, 1], &[17, 18, 19, 0])
            .unwrap();
        let dab = RasterDab {
            material: Default::default(),
            texture: 0.0,
            texture_scale: 1.0,
            x: 10.5,
            y: 10.5,
            radius: 8.0,
            hardness: 1.0,
            weight: 1.0,
        };
        doc.erase_dabs_clipped("paint", &[dab], None)
            .unwrap()
            .unwrap();
        assert_eq!(doc.layers()[0].tiles.pixel(10, 10), Some([0, 0, 0, 0]));
        assert_eq!(doc.layers()[0].tiles.pixel(1, 1), Some([17, 18, 19, 0]));
        doc.undo().unwrap();
        assert_eq!(
            doc.layers()[0].tiles.pixel(10, 10),
            Some([200, 50, 20, 255])
        );
        doc.redo().unwrap();
        assert_eq!(doc.layers()[0].tiles.pixel(1, 1), Some([17, 18, 19, 0]));
    }

    #[test]
    fn panel_settings_are_atomic_and_restore_together_without_touching_pixels() {
        let mut doc = TiledRasterDocument::new(32, 32).unwrap();
        doc.add_layer("paint".into(), "Original".into()).unwrap();
        doc.write_rect("paint", [0, 0, 1, 1], &[255, 0, 0, 255])
            .unwrap();
        doc.discard_history();
        let original = doc.state();
        let mut settings = doc.layers()[0].settings();
        settings.name = "Renamed".into();
        settings.opacity = 0.5;
        settings.locked = true;
        settings.mask_enabled = true;
        settings.mask_density = 0.75;
        let mut invalid = settings.clone();
        invalid.mask_density = f32::NAN;
        assert!(doc.set_layer_settings(invalid).is_err());
        assert_eq!(doc.state(), original);
        assert!(!doc.can_undo());
        assert_eq!(
            doc.set_layer_settings(settings.clone())
                .unwrap()
                .unwrap()
                .coords
                .len(),
            1
        );
        assert_eq!(doc.layers()[0].settings(), settings);
        assert_eq!(doc.state().layers[0].tiles, original.layers[0].tiles);
        doc.undo().unwrap();
        assert_eq!(doc.state(), original);
        assert!(!doc.can_undo());
        doc.redo().unwrap();
        assert_eq!(doc.layers()[0].settings(), settings);
        assert!(doc.set_layer_settings(settings.clone()).unwrap().is_none());
        settings.name = "Metadata only".into();
        assert!(doc
            .set_layer_settings(settings)
            .unwrap()
            .unwrap()
            .coords
            .is_empty());
        assert!(doc.undo().unwrap().unwrap().coords.is_empty());
    }

    #[test]
    fn writes_only_touched_tiles_and_restores_each_side_of_boundary() {
        let mut tiles = SparseTiles::new(512, 300).unwrap();
        let changes = tiles
            .write_rect(255, 255, 2, 2, &[10, 20, 30, 255].repeat(4))
            .unwrap();
        assert_eq!(changes.len(), 4);
        assert_eq!(tiles.allocated_tile_count(), 4);
        assert_eq!(tiles.pixel(256, 256), Some([10, 20, 30, 255]));
        assert_eq!(tiles.pixel(0, 0), Some([0; 4]));
        tiles.apply_changes(&changes, true).unwrap();
        assert_eq!(tiles.allocated_tile_count(), 0);
        tiles.apply_changes(&changes, false).unwrap();
        assert_eq!(tiles.pixel(255, 255), Some([10, 20, 30, 255]));
    }

    #[test]
    fn transparent_rgb_write_is_preserved_and_undoable() {
        let mut tiles = SparseTiles::new(256, 256).unwrap();
        assert!(tiles
            .write_rect(0, 0, 1, 1, &[0, 0, 0, 0])
            .unwrap()
            .is_empty());
        tiles.write_rect(0, 0, 1, 1, &[255, 0, 0, 255]).unwrap();
        let changes = tiles.write_rect(0, 0, 1, 1, &[200, 1, 2, 0]).unwrap();
        assert_eq!(tiles.pixel(0, 0), Some([200, 1, 2, 0]));
        tiles.apply_changes(&changes, true).unwrap();
        assert_eq!(tiles.pixel(0, 0), Some([255, 0, 0, 255]));
        tiles.apply_changes(&changes, false).unwrap();
        assert_eq!(tiles.pixel(0, 0), Some([200, 1, 2, 0]));
        assert!(tiles.transformed([1., 0., 0., 1., 0.5, 0.]).is_err());
    }

    #[test]
    fn invalid_rect_and_delta_leave_all_tiles_unchanged() {
        let mut tiles = SparseTiles::new(256, 256).unwrap();
        let baseline = tiles.clone();
        assert!(tiles.write_rect(255, 0, 2, 1, &[0; 8]).is_err());
        assert!(tiles.write_rect(0, 0, 1, 1, &[0; 3]).is_err());
        assert_eq!(tiles, baseline);
        let invalid = [TileChange {
            coord: TileCoord { x: 0, y: 0 },
            before: None,
            after: Some(vec![0; 3]),
        }];
        assert!(tiles.apply_changes(&invalid, false).is_err());
        assert_eq!(tiles, baseline);
    }

    #[test]
    fn stale_delta_cannot_replace_a_newer_edit() {
        let mut tiles = SparseTiles::new(256, 256).unwrap();
        let old = tiles.write_rect(0, 0, 1, 1, &[1, 2, 3, 255]).unwrap();
        tiles.write_rect(0, 0, 1, 1, &[4, 5, 6, 255]).unwrap();
        let baseline = tiles.clone();
        assert!(tiles.apply_changes(&old, true).is_err());
        assert_eq!(tiles, baseline);
    }

    #[test]
    fn raster_document_undoes_only_the_last_edited_layer() {
        let mut document = TiledRasterDocument::new(512, 256).unwrap();
        document.add_layer("lower".into(), "Lower".into()).unwrap();
        document.add_layer("upper".into(), "Upper".into()).unwrap();
        let changed = document
            .write_rect("lower", [255, 0, 2, 1], &[255, 0, 0, 255].repeat(2))
            .unwrap();
        assert_eq!(
            changed.unwrap().coords,
            [TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 0 }]
        );
        document
            .write_rect("upper", [255, 0, 1, 1], &[0, 0, 255, 255])
            .unwrap();
        let revision = document.revision();
        assert_eq!(document.undo().unwrap().unwrap().layer_id, "upper");
        assert_eq!(document.revision(), revision + 1);
        assert_eq!(
            document.layers()[0].tiles.pixel(256, 0),
            Some([255, 0, 0, 255])
        );
        assert_eq!(document.layers()[1].tiles.pixel(255, 0), Some([0; 4]));
        assert_eq!(
            document.redo().unwrap().unwrap().coords,
            [TileCoord { x: 0, y: 0 }]
        );
        assert_eq!(
            document.layers()[1].tiles.pixel(255, 0),
            Some([0, 0, 255, 255])
        );
        assert!(document
            .write_rect("upper", [255, 0, 1, 1], &[0, 0, 255, 255])
            .unwrap()
            .is_none());
        assert!(document.undo().unwrap().is_some());
        document
            .write_rect("upper", [0, 0, 1, 1], &[0, 255, 0, 255])
            .unwrap();
        assert!(document.redo().unwrap().is_none());
    }

    #[test]
    fn composite_tile_respects_layer_order_alpha_and_sparse_bounds() {
        let mut document = TiledRasterDocument::new(257, 257).unwrap();
        document.add_layer("back".into(), "Back".into()).unwrap();
        document.add_layer("front".into(), "Front".into()).unwrap();
        assert!(document.composite_tile(TileCoord { x: 0, y: 0 }).is_none());
        assert!(document.composite_tile(TileCoord { x: 2, y: 0 }).is_none());
        document
            .write_rect("back", [256, 256, 1, 1], &[255, 0, 0, 255])
            .unwrap();
        document
            .write_rect("front", [256, 256, 1, 1], &[0, 0, 255, 128])
            .unwrap();
        let coord = TileCoord { x: 1, y: 1 };
        let pixels = document.composite_tile(coord).unwrap();
        assert_eq!(&pixels[..4], &[127, 0, 128, 255]);
        assert_eq!(&pixels[4..8], &[0; 4]);
        assert_eq!(&pixels[(256 * 4)..(256 * 4 + 4)], &[0; 4]);
        document.undo().unwrap();
        assert_eq!(
            &document.composite_tile(coord).unwrap()[..4],
            &[255, 0, 0, 255]
        );
    }

    #[test]
    fn upload_payload_clips_edges_and_clears_undone_or_hidden_tiles() {
        let mut document = TiledRasterDocument::new(257, 3).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        let changed = document
            .write_rect("paint", [255, 2, 2, 1], &[200, 0, 0, 255, 0, 0, 200, 255])
            .unwrap()
            .unwrap();
        let uploads = document.prepare_uploads(&changed).unwrap();
        assert_eq!(uploads.len(), 2);
        assert_eq!(uploads[0].origin, [0, 0]);
        assert_eq!(uploads[0].extent, [256, 3]);
        assert_eq!(uploads[1].origin, [256, 0]);
        assert_eq!(uploads[1].extent, [1, 3]);
        assert_eq!(uploads[1].bytes_per_row, 1024);
        assert_eq!(uploads[1].pixels.len(), TILE_BYTES);
        assert_eq!(
            &uploads[1].pixels[(2 * TILE_SIZE * 4) as usize..][..4],
            &[0, 0, 200, 255]
        );
        assert_eq!(document.prepare_full_uploads(), uploads);

        let masked = document
            .write_mask_rect("paint", [256, 2, 1, 1], &[0])
            .unwrap()
            .unwrap();
        document.set_layer_mask("paint", true, false, 1.0).unwrap();
        let masked_uploads = document.prepare_uploads(&masked).unwrap();
        assert_eq!(masked_uploads.len(), 1);
        assert!(masked_uploads[0].pixels.iter().all(|byte| *byte == 0));
        document.undo().unwrap();
        document.undo().unwrap();

        let hidden = document
            .set_layer_appearance("paint", false, 1.0)
            .unwrap()
            .unwrap();
        assert!(document
            .prepare_uploads(&hidden)
            .unwrap()
            .iter()
            .all(|upload| upload.pixels.iter().all(|byte| *byte == 0)));
        document.undo().unwrap();
        let cleared = document.undo().unwrap().unwrap();
        assert!(document
            .prepare_uploads(&cleared)
            .unwrap()
            .iter()
            .all(|upload| upload.pixels.iter().all(|byte| *byte == 0)));
        assert!(document.prepare_full_uploads().is_empty());

        let invalid = TileInvalidation {
            layer_id: "paint".into(),
            coords: vec![TileCoord { x: 0, y: 0 }, TileCoord { x: 2, y: 0 }],
        };
        assert!(document.prepare_uploads(&invalid).is_err());
    }

    #[test]
    fn changed_uploads_skip_identical_tiles_and_clear_removed_pixels() {
        let mut document = TiledRasterDocument::new(512, 256).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        document
            .write_rect("paint", [0, 0, 1, 1], &[255, 0, 0, 255])
            .unwrap();
        let before = document.clone();
        document
            .write_rect("paint", [256, 0, 1, 1], &[0, 0, 255, 255])
            .unwrap();
        let uploads = document.prepare_changed_uploads(&before).unwrap();
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].coord, TileCoord { x: 1, y: 0 });
        assert_eq!(
            document.prepare_changed_uploads(&document).unwrap().len(),
            0
        );

        let painted = document.clone();
        document.undo().unwrap();
        let cleared = document.prepare_changed_uploads(&painted).unwrap();
        assert_eq!(cleared.len(), 1);
        assert_eq!(cleared[0].coord, TileCoord { x: 1, y: 0 });
        assert!(cleared[0].pixels.iter().all(|byte| *byte == 0));

        let different_size = TiledRasterDocument::new(256, 256).unwrap();
        assert!(document.prepare_changed_uploads(&different_size).is_err());
    }

    #[test]
    fn appearance_and_reorder_invalidate_affected_tiles_and_share_history() {
        let mut document = TiledRasterDocument::new(512, 256).unwrap();
        document.add_layer("back".into(), "Back".into()).unwrap();
        document.add_layer("front".into(), "Front".into()).unwrap();
        document
            .write_rect("back", [0, 0, 1, 1], &[255, 0, 0, 255])
            .unwrap();
        document
            .write_rect("back", [256, 0, 1, 1], &[255, 0, 0, 255])
            .unwrap();
        document
            .write_rect("front", [0, 0, 1, 1], &[0, 0, 255, 255])
            .unwrap();
        let tile = TileCoord { x: 0, y: 0 };
        let other = TileCoord { x: 1, y: 0 };
        assert_eq!(
            &document.composite_tile(tile).unwrap()[..4],
            &[0, 0, 255, 255]
        );
        let changed = document
            .set_layer_appearance("front", true, 0.5)
            .unwrap()
            .unwrap();
        assert_eq!(changed.coords, [tile]);
        assert_eq!(
            &document.composite_tile(tile).unwrap()[..4],
            &[127, 0, 128, 255]
        );
        let revision = document.revision();
        assert!(document
            .set_layer_appearance("front", true, 0.5)
            .unwrap()
            .is_none());
        assert!(document
            .set_layer_appearance("front", true, f32::NAN)
            .is_err());
        assert_eq!(document.revision(), revision);
        document.set_layer_appearance("front", false, 0.5).unwrap();
        assert_eq!(
            &document.composite_tile(tile).unwrap()[..4],
            &[255, 0, 0, 255]
        );
        document.undo().unwrap();
        assert_eq!(
            &document.composite_tile(tile).unwrap()[..4],
            &[127, 0, 128, 255]
        );
        let moved = document.move_layer("front", 0).unwrap().unwrap();
        assert_eq!(moved.coords, [tile, other]);
        assert_eq!(
            &document.composite_tile(tile).unwrap()[..4],
            &[255, 0, 0, 255]
        );
        assert_eq!(document.undo().unwrap().unwrap().coords, [tile, other]);
        assert_eq!(
            &document.composite_tile(tile).unwrap()[..4],
            &[127, 0, 128, 255]
        );
        document.redo().unwrap();
        assert_eq!(document.layers()[0].id, "front");
        assert!(document.move_layer("front", 2).is_err());
    }

    #[test]
    fn sampled_brush_stroke_crosses_tiles_and_undoes_as_one_edit() {
        let mut document = TiledRasterDocument::new(512, 256).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        let dab = RasterDab {
            material: Default::default(),
            texture: 0.0,
            texture_scale: 1.0,
            x: 256.0,
            y: 30.5,
            radius: 8.0,
            hardness: 0.0,
            weight: 0.3,
        };
        let before = document.clone();
        let changed = document
            .paint_dabs("paint", &[dab, dab], [20, 80, 200])
            .unwrap()
            .unwrap();
        assert_eq!(
            changed.coords,
            [TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 0 }]
        );
        let layer = &document.layers()[0];
        assert_eq!(layer.tiles.pixel(255, 30), layer.tiles.pixel(256, 30));
        assert!(layer.tiles.pixel(255, 30).unwrap()[3] > layer.tiles.pixel(250, 30).unwrap()[3]);
        let painted = layer.tiles.pixel(255, 30);
        assert_eq!(document.undo().unwrap().unwrap().coords, changed.coords);
        assert_eq!(document.layers()[0].tiles, before.layers()[0].tiles);
        document.redo().unwrap();
        assert_eq!(document.layers()[0].tiles.pixel(255, 30), painted);
    }

    #[test]
    fn selection_clips_each_dab_before_density_accumulates() {
        use crate::selection::{SelectionOperation, SelectionRegion, SelectionShape};

        let mut document = TiledRasterDocument::new(32, 32).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        let selection = Selection {
            raster: None,
            regions: vec![
                SelectionRegion {
                    paint: None,
                    points: Vec::new(),
                    radius: 0.,
                    shape: SelectionShape::Rectangle,
                    bounds: [0.0, 0.0, 32.0, 32.0],
                    operation: SelectionOperation::Replace,
                },
                SelectionRegion {
                    paint: None,
                    points: Vec::new(),
                    radius: 0.,
                    shape: SelectionShape::Rectangle,
                    bounds: [16.0, 0.0, 16.0, 32.0],
                    operation: SelectionOperation::Subtract,
                },
            ],
        };
        let dab = RasterDab {
            material: Default::default(),
            texture: 0.0,
            texture_scale: 1.0,
            x: 16.0,
            y: 16.0,
            radius: 12.0,
            hardness: 1.0,
            weight: 1.0,
        };
        document
            .paint_dabs_clipped("paint", &[dab, dab], [0, 0, 0], Some(&selection))
            .unwrap();
        let pixels = &document.layers()[0].tiles;
        assert!(pixels.pixel(15, 16).unwrap()[3] > 0);
        assert_eq!(pixels.pixel(16, 16), Some([0, 0, 0, 0]));
        document.undo().unwrap();
        assert_eq!(document.layers()[0].tiles.allocated_tile_count(), 0);
    }

    #[test]
    fn layer_lock_rejects_edits_and_alpha_lock_preserves_coverage() {
        let mut document = TiledRasterDocument::new(32, 32).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        document
            .write_rect("paint", [5, 5, 1, 1], &[200, 0, 0, 128])
            .unwrap();
        let dab = RasterDab {
            material: Default::default(),
            texture: 0.0,
            texture_scale: 1.0,
            x: 5.5,
            y: 5.5,
            radius: 3.0,
            hardness: 1.0,
            weight: 2.0,
        };
        assert!(document.set_layer_locks("paint", true, false).unwrap());
        let before = document.clone();
        assert!(document.paint_dabs("paint", &[dab], [0, 0, 255]).is_err());
        assert!(document.write_rect("paint", [5, 5, 1, 1], &[0; 4]).is_err());
        assert_eq!(document, before);
        document.set_layer_locks("paint", false, true).unwrap();
        assert!(document.write_rect("paint", [5, 5, 1, 1], &[0; 4]).is_err());
        document.paint_dabs("paint", &[dab], [0, 0, 255]).unwrap();
        let layer = &document.layers()[0];
        let painted = layer.tiles.pixel(5, 5).unwrap();
        assert_eq!(painted[3], 128);
        assert!(painted[2] > 0);
        assert_eq!(layer.tiles.pixel(6, 5), Some([0; 4]));
        document.undo().unwrap();
        assert_eq!(
            document.layers()[0].tiles.pixel(5, 5),
            Some([200, 0, 0, 128])
        );
        document.undo().unwrap();
        assert!(document.layers()[0].locked);
        assert!(!document.layers()[0].alpha_locked);
        document.redo().unwrap();
        assert!(!document.layers()[0].locked);
        assert!(document.layers()[0].alpha_locked);
    }

    #[test]
    fn white_disabled_mask_presence_follows_settings_and_undo() {
        let mut doc = TiledRasterDocument::new(32, 32).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        assert!(!doc.layers()[0].mask_present);
        doc.set_layer_mask("paint", true, false, 1.).unwrap();
        doc.set_layer_mask("paint", false, false, 1.).unwrap();
        assert!(doc.layers()[0].mask_present);
        assert!(!doc.layers()[0].mask_enabled);
        assert_eq!(doc.layers()[0].mask.allocated_tile_count(), 0);
        assert!(
            TiledRasterDocument::from_state(doc.state())
                .unwrap()
                .layers()[0]
                .mask_present
        );
        doc.undo().unwrap();
        assert!(doc.layers()[0].mask_enabled);
        doc.undo().unwrap();
        assert!(!doc.layers()[0].mask_present);
        doc.redo().unwrap();
        doc.redo().unwrap();
        assert!(doc.layers()[0].mask_present);
        assert!(!doc.layers()[0].mask_enabled);

        let mut second = TiledRasterDocument::new(32, 32).unwrap();
        second.add_layer("paint".into(), "Paint".into()).unwrap();
        let mut settings = second.layers()[0].settings();
        settings.mask_enabled = true;
        second.set_layer_settings(settings).unwrap();
        assert!(second.layers()[0].mask_present);
        second.undo().unwrap();
        assert!(!second.layers()[0].mask_present);
        second.redo().unwrap();
        assert!(second.layers()[0].mask_present);

        let mut painted = TiledRasterDocument::new(32, 32).unwrap();
        painted.add_layer("paint".into(), "Paint".into()).unwrap();
        painted
            .write_mask_rect("paint", [0, 0, 1, 1], &[0])
            .unwrap();
        painted
            .write_mask_rect("paint", [0, 0, 1, 1], &[255])
            .unwrap();
        assert!(painted.layers()[0].mask_present);
        assert_eq!(painted.layers()[0].mask.allocated_tile_count(), 0);
        painted.undo().unwrap();
        painted.undo().unwrap();
        assert!(!painted.layers()[0].mask_present);
        painted.redo().unwrap();
        painted.redo().unwrap();
        assert!(painted.layers()[0].mask_present);
    }
    #[test]
    fn uniform_mask_changes_composite_without_editing_source_tiles() {
        let mut document = TiledRasterDocument::new(32, 32).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        document
            .write_rect("paint", [0, 0, 1, 1], &[240, 40, 20, 255])
            .unwrap();
        let coord = TileCoord { x: 0, y: 0 };
        let source = document.layers()[0].tiles.clone();
        let original = document.composite_tile(coord).unwrap();
        let revision = document.revision();
        assert!(document
            .set_layer_mask("paint", true, false, f32::NAN)
            .is_err());
        assert_eq!(document.revision(), revision);
        let invalidation = document
            .set_layer_mask("paint", true, false, 0.5)
            .unwrap()
            .unwrap();
        assert_eq!(invalidation.coords, [coord]);
        assert_eq!(document.composite_tile(coord).unwrap()[3], 128);
        assert_eq!(document.layers()[0].tiles, source);
        document.set_layer_mask("paint", true, true, 0.5).unwrap();
        assert_eq!(document.composite_tile(coord).unwrap()[3], 128);
        document.set_layer_mask("paint", true, true, 1.0).unwrap();
        assert_eq!(document.composite_tile(coord), None);
        assert_eq!(document.undo().unwrap().unwrap().coords, [coord]);
        assert_eq!(document.composite_tile(coord).unwrap()[3], 128);
        document.undo().unwrap();
        document.undo().unwrap();
        assert_eq!(document.composite_tile(coord).unwrap(), original);
        document.redo().unwrap();
        assert_eq!(document.composite_tile(coord).unwrap()[3], 128);
    }

    #[test]
    fn spatial_mask_crosses_tiles_and_undo_restores_sparse_default() {
        let mut document = TiledRasterDocument::new(512, 16).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        document
            .write_rect("paint", [255, 0, 2, 1], &[255, 0, 0, 255, 255, 0, 0, 255])
            .unwrap();
        document.set_layer_mask("paint", true, false, 1.0).unwrap();
        let revision = document.revision();
        assert!(document
            .write_mask_rect("paint", [255, 0, 2, 1], &[0])
            .is_err());
        assert_eq!(document.revision(), revision);
        let invalidation = document
            .write_mask_rect("paint", [255, 0, 2, 1], &[0, 128])
            .unwrap()
            .unwrap();
        assert_eq!(
            invalidation.coords,
            [TileCoord { x: 0, y: 0 }, TileCoord { x: 1, y: 0 }]
        );
        assert_eq!(document.layers()[0].mask.allocated_tile_count(), 2);
        assert_eq!(document.layers()[0].mask.pixel(255, 0), Some(0));
        assert_eq!(document.layers()[0].mask.pixel(256, 0), Some(128));
        let left = document.composite_tile(TileCoord { x: 0, y: 0 });
        let right = document.composite_tile(TileCoord { x: 1, y: 0 }).unwrap();
        assert_eq!(left, None);
        assert_eq!(&right[..4], &[128, 0, 0, 128]);
        document.set_layer_mask("paint", true, true, 1.0).unwrap();
        assert_eq!(
            document.composite_tile(TileCoord { x: 0, y: 0 }).unwrap()[((255 * 4) as usize) + 3],
            255
        );
        document.undo().unwrap();
        assert_eq!(
            document.undo().unwrap().unwrap().coords,
            invalidation.coords
        );
        assert_eq!(document.layers()[0].mask.allocated_tile_count(), 0);
        assert_eq!(document.layers()[0].mask.pixel(255, 0), Some(255));
        document.redo().unwrap();
        assert_eq!(document.layers()[0].mask.pixel(256, 0), Some(128));
        assert!(document
            .write_mask_rect("paint", [255, 0, 2, 1], &[255, 255])
            .unwrap()
            .is_some());
        assert_eq!(document.layers()[0].mask.allocated_tile_count(), 0);
    }

    #[test]
    fn mask_brush_shares_density_and_respects_selection_and_history() {
        use crate::selection::SelectionShape;

        let mut document = TiledRasterDocument::new(512, 32).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        document.set_layer_mask("paint", true, false, 1.0).unwrap();
        let selection = Selection::new(SelectionShape::Rectangle, [0.0, 0.0, 256.0, 32.0]);
        let dab = RasterDab {
            material: Default::default(),
            texture: 0.0,
            texture_scale: 1.0,
            x: 256.0,
            y: 15.5,
            radius: 8.0,
            hardness: 0.5,
            weight: 0.5,
        };
        let before = document.clone();
        let changed = document
            .paint_mask_dabs("paint", &[dab, dab], 0, Some(&selection))
            .unwrap()
            .unwrap();
        assert_eq!(changed.coords, [TileCoord { x: 0, y: 0 }]);
        let mask = &document.layers()[0].mask;
        assert!(mask.pixel(255, 15).unwrap() < 255);
        assert_eq!(mask.pixel(256, 15), Some(255));
        assert_eq!(mask.allocated_tile_count(), 1);
        let painted = mask.pixel(255, 15);
        document.undo().unwrap();
        assert_eq!(document.layers()[0].mask, before.layers()[0].mask);
        document.redo().unwrap();
        assert_eq!(document.layers()[0].mask.pixel(255, 15), painted);
        let revision = document.revision();
        assert!(document
            .paint_mask_dabs(
                "paint",
                &[RasterDab {
                    material: Default::default(),
                    weight: f32::NAN,
                    ..dab
                }],
                0,
                None
            )
            .is_err());
        assert_eq!(document.revision(), revision);
        document
            .paint_mask_dabs(
                "paint",
                &[RasterDab {
                    material: Default::default(),
                    weight: 10.0,
                    ..dab
                }],
                255,
                Some(&selection),
            )
            .unwrap();
        assert_eq!(document.layers()[0].mask.pixel(256, 15), Some(255));
    }

    #[test]
    fn tiled_state_round_trips_pixels_spatial_masks_and_layer_order() {
        let mut document = TiledRasterDocument::new(257, 17).unwrap();
        document.add_layer("back".into(), "Back".into()).unwrap();
        document.add_layer("front".into(), "Front".into()).unwrap();
        document
            .write_rect("back", [255, 2, 2, 1], &[200, 10, 20, 255, 5, 40, 80, 128])
            .unwrap();
        document
            .write_rect("front", [12, 3, 1, 1], &[20, 180, 60, 220])
            .unwrap();
        document.set_layer_appearance("front", false, 0.4).unwrap();
        document.set_layer_locks("front", true, true).unwrap();
        document.set_layer_mask("back", true, true, 0.75).unwrap();
        document
            .write_mask_rect("back", [255, 2, 2, 1], &[0, 128])
            .unwrap();

        let state = document.state();
        let restored = TiledRasterDocument::from_state(state.clone()).unwrap();

        assert_eq!(restored.state(), state);
        assert_eq!(restored.revision(), 0);
        assert!(restored
            .prepare_changed_uploads(&document)
            .unwrap()
            .is_empty());
        assert_eq!(restored.layers()[0].mask.pixel(255, 2), Some(0));
        assert_eq!(restored.layers()[0].mask.pixel(256, 2), Some(128));
        assert_eq!(restored.layers()[1].id, "front");
        assert!(restored.layers()[1].locked && restored.layers()[1].alpha_locked);
    }

    #[test]
    fn tiled_state_rejects_duplicate_and_invalid_edge_payloads_atomically() {
        let mut document = TiledRasterDocument::new(257, 1).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        document
            .write_rect("paint", [256, 0, 1, 1], &[20, 40, 80, 255])
            .unwrap();
        document
            .write_mask_rect("paint", [256, 0, 1, 1], &[0])
            .unwrap();
        let state = document.state();

        let mut duplicate = state.clone();
        let repeated = duplicate.layers[0].tiles[0].clone();
        duplicate.layers[0].tiles.push(repeated);
        assert!(TiledRasterDocument::from_state(duplicate).is_err());

        let mut invalid_mask = state.clone();
        invalid_mask.layers[0].mask_tiles[0].values[1] = 0;
        assert!(TiledRasterDocument::from_state(invalid_mask).is_err());

        let mut invalid_pixels = state;
        invalid_pixels.layers[0].tiles[0].pixels[7] = 255;
        assert!(TiledRasterDocument::from_state(invalid_pixels).is_err());
    }

    #[test]
    fn invalid_brush_batch_is_atomic_and_zero_weight_is_a_noop() {
        let mut document = TiledRasterDocument::new(256, 256).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        let dab = RasterDab {
            material: Default::default(),
            texture: 0.0,
            texture_scale: 1.0,
            x: 30.0,
            y: 30.0,
            radius: 6.0,
            hardness: 1.0,
            weight: 0.0,
        };
        let before = document.clone();
        assert!(document
            .paint_dabs("paint", &[dab], [0; 3])
            .unwrap()
            .is_none());
        assert!(document
            .paint_dabs(
                "paint",
                &[
                    dab,
                    RasterDab {
                        material: Default::default(),
                        texture: 0.0,
                        texture_scale: 1.0,
                        radius: f32::NAN,
                        ..dab
                    }
                ],
                [0; 3],
            )
            .is_err());
        assert_eq!(document, before);
    }

    #[test]
    fn overlapping_dabs_match_separate_same_color_strokes_within_rgba_rounding() {
        let dab = RasterDab {
            material: Default::default(),
            texture: 0.0,
            texture_scale: 1.0,
            x: 20.5,
            y: 20.5,
            radius: 8.0,
            hardness: 0.2,
            weight: 0.1,
        };
        let mut one = TiledRasterDocument::new(64, 64).unwrap();
        let mut two = TiledRasterDocument::new(64, 64).unwrap();
        for document in [&mut one, &mut two] {
            document.add_layer("paint".into(), "Paint".into()).unwrap();
        }
        one.paint_dabs("paint", &[dab, dab], [20, 80, 200]).unwrap();
        two.paint_dabs("paint", &[dab], [20, 80, 200]).unwrap();
        two.paint_dabs("paint", &[dab], [20, 80, 200]).unwrap();
        for y in 12..29 {
            for x in 12..29 {
                let combined = one.layers()[0].tiles.pixel(x, y).unwrap();
                let separate = two.layers()[0].tiles.pixel(x, y).unwrap();
                let premultiplied = |pixel: [u8; 4]| {
                    [
                        pixel[0] as u16 * pixel[3] as u16 / 255,
                        pixel[1] as u16 * pixel[3] as u16 / 255,
                        pixel[2] as u16 * pixel[3] as u16 / 255,
                        u16::from(pixel[3]),
                    ]
                };
                for (a, b) in premultiplied(combined)
                    .into_iter()
                    .zip(premultiplied(separate))
                {
                    assert!(
                        a.abs_diff(b) <= 2,
                        "pixel ({x}, {y}): {combined:?} vs {separate:?}"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod imported_layer_tests {
    use super::*;
    #[test]
    fn layer_import_is_one_history_entry_and_failed_import_is_atomic() {
        let mut document = TiledRasterDocument::new(3, 2).unwrap();
        document.add_layer("base".into(), "Base".into()).unwrap();
        document.discard_history();
        let before = document.state();
        let revision = document.revision();
        let mut tiles = SparseTiles::new(3, 2).unwrap();
        tiles.write_rect(1, 0, 1, 1, &[10, 20, 30, 128]).unwrap();
        assert!(document
            .import_layer("base".into(), "Duplicate".into(), tiles.clone())
            .is_err());
        assert!(document
            .import_layer(
                "wrong".into(),
                "Wrong".into(),
                SparseTiles::new(1, 1).unwrap()
            )
            .is_err());
        assert_eq!(document.state(), before);
        assert_eq!(document.revision(), revision);
        assert!(document.undo().unwrap().is_none());
        document
            .import_layer("new".into(), "Imported".into(), tiles.clone())
            .unwrap();
        assert_eq!(document.revision(), revision + 1);
        assert_eq!(document.layers()[1].tiles, tiles);
        let imported = document.state();
        document.undo().unwrap();
        assert_eq!(document.state(), before);
        assert!(document.undo().unwrap().is_none());
        document.redo().unwrap();
        assert_eq!(document.state(), imported);
    }
}

mod tile_pairs {
    use super::*;
    pub fn serialize<S: serde::Serializer, T: Serialize>(
        value: &BTreeMap<TileCoord, T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.iter().collect::<Vec<_>>().serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
        deserializer: D,
    ) -> Result<BTreeMap<TileCoord, T>, D::Error> {
        Ok(Vec::<(TileCoord, T)>::deserialize(deserializer)?
            .into_iter()
            .collect())
    }
}
