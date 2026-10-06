//! Sparse RGBA8 raster tiles. A fully transparent tile has no allocation.
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
    pub layers: Vec<RasterLayerState>,
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
    pub mask_enabled: bool,
    pub mask_inverted: bool,
    pub mask_density: f32,
    pub tiles: Vec<RasterTileState>,
    pub mask_tiles: Vec<MaskTileState>,
}

/// Composite validated snapshot layers with the same CPU kernel as the live document.
/// This borrows pixel payloads; import verification does not clone a full document.
pub fn composite_raster_layers_tile(
    layers: &[RasterLayerState],
    coord: TileCoord,
) -> Option<Vec<u8>> {
    composite_sources(layers.iter().filter_map(|layer| {
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
    }))
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
fn composite_sources<'a>(sources: impl Iterator<Item = CompositeSource<'a>>) -> Option<Vec<u8>> {
    let mut sources = sources.peekable();
    sources.peek()?;
    let mut result = vec![0u8; TILE_BYTES];
    for source in sources {
        let effects = source.effects.enabled.then(|| source.effects.prepare());
        for (index, (target, pixel)) in result
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(source.pixels.as_chunks::<4>().0.iter())
            .enumerate()
        {
            let adjusted = effects.as_ref().map_or(*pixel, |e| e.apply(*pixel));
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaskTileState {
    pub coord: TileCoord,
    pub values: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TileChange {
    pub coord: TileCoord,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// A mask tile is absent when every pixel is fully visible (255).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SparseMask {
    width: u32,
    height: u32,
    tiles: BTreeMap<TileCoord, Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
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
        value: u8,
        selection: Option<&Selection>,
    ) -> Result<Vec<MaskChange>, String> {
        let density = dab_density(self.width, self.height, dabs, selection)?;
        let mut changes = Vec::new();
        for (coord, coverage) in density {
            let before = self.tiles.get(&coord).cloned();
            let mut after = before
                .clone()
                .unwrap_or_else(|| vec![255; (TILE_SIZE * TILE_SIZE) as usize]);
            for (pixel, depth) in after.iter_mut().zip(coverage) {
                if depth > 0.0 {
                    let alpha = 1.0 - (-depth).exp();
                    *pixel = (f32::from(value) * alpha + f32::from(*pixel) * (1.0 - alpha))
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SparseTiles {
    width: u32,
    height: u32,
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
                // RGB in transparent pixels has no visible or stored meaning.
                for pixel in after.as_chunks_mut::<4>().0 {
                    if pixel[3] == 0 {
                        pixel[..3].fill(0);
                    }
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
        self.paint_dabs_with_alpha_lock(dabs, color, selection, false)
    }

    fn paint_dabs_with_alpha_lock(
        &mut self,
        dabs: &[RasterDab],
        color: [u8; 3],
        selection: Option<&Selection>,
        alpha_locked: bool,
    ) -> Result<Vec<TileChange>, String> {
        let density = dab_density(self.width, self.height, dabs, selection)?;
        let mut changes = Vec::new();
        for (coord, coverage) in density {
            let before = self.tiles.get(&coord).cloned();
            let mut after = before.clone().unwrap_or_else(|| vec![0; TILE_BYTES]);
            for (pixel, depth) in after.as_chunks_mut::<4>().0.iter_mut().zip(coverage) {
                if depth <= 0.0 || (alpha_locked && pixel[3] == 0) {
                    continue;
                }
                let source_alpha = 1.0 - (-depth).exp();
                if alpha_locked {
                    for channel in 0..3 {
                        pixel[channel] = (f32::from(color[channel]) * source_alpha
                            + f32::from(pixel[channel]) * (1.0 - source_alpha))
                            .round()
                            .clamp(0.0, 255.0) as u8;
                    }
                    continue;
                }
                let old_alpha = f32::from(pixel[3]) / 255.0;
                let output_alpha = source_alpha + old_alpha * (1.0 - source_alpha);
                let byte_alpha = (output_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
                if byte_alpha == 0 {
                    pixel.fill(0);
                    continue;
                }
                for channel in 0..3 {
                    let old = f32::from(pixel[channel]) / 255.0;
                    let ink = f32::from(color[channel]) / 255.0;
                    pixel[channel] = (((ink * source_alpha
                        + old * old_alpha * (1.0 - source_alpha))
                        / output_alpha)
                        * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
                pixel[3] = byte_alpha;
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

fn valid_tile_payload(coord: TileCoord, bytes: &[u8], width: u32, height: u32) -> bool {
    if bytes.len() != TILE_BYTES {
        return false;
    }
    let mut painted = false;
    for (index, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let x = coord.x * TILE_SIZE + (index as u32 % TILE_SIZE);
        let y = coord.y * TILE_SIZE + (index as u32 / TILE_SIZE);
        if x >= width || y >= height || pixel[3] == 0 {
            if *pixel != [0, 0, 0, 0] {
                return false;
            }
        } else {
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

#[derive(Clone, Debug, PartialEq)]
pub struct RasterLayer {
    pub effects: crate::layer_effects::LayerEffects,
    pub blend_mode: RasterBlendMode,
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub locked: bool,
    pub alpha_locked: bool,
    pub mask_enabled: bool,
    pub mask_inverted: bool,
    pub mask_density: f32,
    pub mask: SparseMask,
    pub tiles: SparseTiles,
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

#[derive(Clone, Debug, PartialEq)]
enum RasterEdit {
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
    },
    Pixels {
        layer_id: String,
        changes: Vec<TileChange>,
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
        before: (bool, bool, f32),
        after: (bool, bool, f32),
    },
    MaskPixels {
        layer_id: String,
        changes: Vec<MaskChange>,
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
        if !self.texture.is_finite()
            || !(0.0..=2.0).contains(&self.texture)
            || !self.texture_scale.is_finite()
            || !(1.0..=2.0).contains(&self.texture_scale)
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

fn dab_density(
    width: u32,
    height: u32,
    dabs: &[RasterDab],
    selection: Option<&Selection>,
) -> Result<BTreeMap<TileCoord, Vec<f32>>, String> {
    dab_density_on_tiles(width, height, dabs, selection, None)
}

fn dab_density_on_tiles(
    width: u32,
    height: u32,
    dabs: &[RasterDab],
    selection: Option<&Selection>,
    occupied: Option<&BTreeMap<TileCoord, Vec<u8>>>,
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
    for dab in dabs {
        if dab.weight == 0.0 {
            continue;
        }
        let fringe = 2.0 * dab.texture_scale;
        let left = ((dab.x - dab.radius - fringe).floor() as i32).max(0) as u32;
        let top = ((dab.y - dab.radius - fringe).floor() as i32).max(0) as u32;
        let right = ((dab.x + dab.radius + fringe).ceil() as i32)
            .min(width as i32)
            .max(0) as u32;
        let bottom = ((dab.y + dab.radius + fringe).ceil() as i32)
            .min(height as i32)
            .max(0) as u32;
        let inner = dab.radius * dab.hardness;
        for y in top..bottom {
            for tile_x in left / TILE_SIZE..right.div_ceil(TILE_SIZE) {
                let coord = TileCoord {
                    x: tile_x,
                    y: y / TILE_SIZE,
                };
                if occupied.is_some_and(|tiles| !tiles.contains_key(&coord)) {
                    continue;
                }
                let coverage = density
                    .entry(coord)
                    .or_insert_with(|| vec![0.0; TILE_BYTES / 4]);
                for x in left.max(tile_x * TILE_SIZE)..right.min((tile_x + 1) * TILE_SIZE) {
                    if selection.is_some_and(|selection| {
                        !selection.contains(Point {
                            x: x as f32 + 0.5,
                            y: y as f32 + 0.5,
                        })
                    }) {
                        continue;
                    }
                    let dx = x as f32 + 0.5 - dab.x;
                    let dy = y as f32 + 0.5 - dab.y;
                    let distance = dx.hypot(dy);
                    // Share the GPU brush's document-pixel radial profile. A 2x
                    // preview must scale the fringe as well as the tip radius.
                    let aa =
                        ((dx.abs() + dy.abs()) / distance.max(0.001)).max(0.01) * dab.texture_scale;
                    let edge = (inner + aa).max(dab.radius + aa);
                    let t = ((distance - inner) / (edge - inner)).clamp(0.0, 1.0);
                    let profile = 1.0 - t * t * (3.0 - 2.0 * t);
                    if profile <= 0.0 {
                        continue;
                    }
                    let index = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize;
                    coverage[index] += (4.0 + 12.0 * dab.hardness)
                        * dab.weight
                        * profile
                        * brush_texture(
                            (x as f32 + 0.5) / dab.texture_scale,
                            (y as f32 + 0.5) / dab.texture_scale,
                            dab.texture,
                        );
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
    resolution: Option<RasterResolution>,
    width: u32,
    height: u32,
    layers: Vec<RasterLayer>,
    undo: Vec<RasterEdit>,
    redo: Vec<RasterEdit>,
    revision: u64,
}

impl TiledRasterDocument {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        SparseTiles::new(width, height)?;
        Ok(Self {
            resolution: None,
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
            layer.mask_enabled = saved.mask_enabled;
            layer.mask_inverted = saved.mask_inverted;
            layer.mask_density = saved.mask_density;
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
        composite_sources(self.layers.iter().filter_map(|layer| {
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
        }))
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
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
            mask: SparseMask::new(self.width, self.height),
            tiles: SparseTiles::new(self.width, self.height)?,
        });
        self.revision += 1;
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
        layer.apply_settings(&settings);
        let layer_id = settings.id.clone();
        self.record_edit(RasterEdit::Settings {
            before,
            after: settings,
        });
        Ok(Some(TileInvalidation { layer_id, coords }))
    }

    pub fn set_layer_effects(
        &mut self,
        id: &str,
        effects: crate::layer_effects::LayerEffects,
    ) -> Result<Option<TileInvalidation>, String> {
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
        let before = (layer.mask_enabled, layer.mask_inverted, layer.mask_density);
        let after = (enabled, inverted, density);
        if before == after {
            return Ok(None);
        }
        let coords = layer.tiles.allocated_coords().collect();
        (layer.mask_enabled, layer.mask_inverted, layer.mask_density) = after;
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
        self.record_edit(RasterEdit::MaskPixels {
            layer_id: layer_id.into(),
            changes,
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
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if layer.locked || !layer.visible {
            return Err("Raster layer is hidden or locked".into());
        }
        let changes = layer.mask.paint_dabs(dabs, value, selection)?;
        if changes.is_empty() {
            return Ok(None);
        }
        let coords = changes.iter().map(|change| change.coord).collect();
        self.record_edit(RasterEdit::MaskPixels {
            layer_id: layer_id.into(),
            changes,
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
                if selection.contains(crate::document::Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                }) {
                    pixel.fill(0);
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
        )?;
        let mut changes = Vec::new();
        for (coord, coverage) in density {
            let Some(before) = layer.tiles.tiles.get(&coord).cloned() else {
                continue;
            };
            let mut after = before.clone();
            for (pixel, depth) in after.as_chunks_mut::<4>().0.iter_mut().zip(coverage) {
                pixel[3] = (f32::from(pixel[3]) * (-depth).exp()).round() as u8;
                if pixel[3] == 0 {
                    pixel.fill(0);
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
        let layer = self
            .layers
            .iter_mut()
            .find(|layer| layer.id == layer_id)
            .ok_or("Unknown raster layer")?;
        if !layer.visible || layer.locked {
            return Err("Raster layer is hidden or locked".into());
        }
        let changes =
            layer
                .tiles
                .paint_dabs_with_alpha_lock(dabs, color, selection, layer.alpha_locked)?;
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

    fn record_edit(&mut self, edit: RasterEdit) {
        self.undo.push(edit);
        self.redo.clear();
        self.revision += 1;
    }

    fn apply_history(&mut self, undo: bool) -> Result<Option<TileInvalidation>, String> {
        let source = if undo { &self.undo } else { &self.redo };
        let Some(edit) = source.last() else {
            return Ok(None);
        };
        let invalidation = match edit {
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
            RasterEdit::Settings { before, after } => {
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
                if layer.settings() != *expected {
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
                if (layer.mask_enabled, layer.mask_inverted, layer.mask_density)
                    != if undo { *after } else { *before }
                {
                    return Err("Raster mask changed outside history".into());
                }
                let coords = layer.tiles.allocated_coords().collect();
                (layer.mask_enabled, layer.mask_inverted, layer.mask_density) =
                    if undo { *before } else { *after };
                TileInvalidation {
                    layer_id: layer_id.clone(),
                    coords,
                }
            }
            RasterEdit::MaskPixels { layer_id, changes } => {
                self.layers
                    .iter_mut()
                    .find(|layer| layer.id == *layer_id)
                    .ok_or("Raster history layer is missing")?
                    .mask
                    .apply_changes(changes, undo)?;
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
    fn sparse_eraser_density_matches_full_density_without_allocating_empty_tiles() {
        let coord = TileCoord { x: 0, y: 0 };
        let occupied = BTreeMap::from([(coord, vec![255; TILE_BYTES])]);
        let dabs = [RasterDab {
            texture: 0.0,
            texture_scale: 1.0,
            x: TILE_SIZE as f32,
            y: TILE_SIZE as f32,
            radius: 80.0,
            hardness: 0.35,
            weight: 1.0,
        }];
        let full = dab_density(TILE_SIZE * 2, TILE_SIZE * 2, &dabs, None).unwrap();
        let sparse =
            dab_density_on_tiles(TILE_SIZE * 2, TILE_SIZE * 2, &dabs, None, Some(&occupied))
                .unwrap();
        assert_eq!(sparse.len(), 1);
        assert_eq!(sparse[&coord], full[&coord]);
        let empty = BTreeMap::new();
        assert!(
            dab_density_on_tiles(TILE_SIZE * 2, TILE_SIZE * 2, &dabs, None, Some(&empty))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn eraser_removes_alpha_and_undo_restores_pixels() {
        let mut doc = TiledRasterDocument::new(32, 32).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        doc.write_rect("paint", [10, 10, 1, 1], &[200, 50, 20, 255])
            .unwrap();
        let dab = RasterDab {
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
        doc.undo().unwrap();
        assert_eq!(
            doc.layers()[0].tiles.pixel(10, 10),
            Some([200, 50, 20, 255])
        );
        doc.redo().unwrap();
        assert_eq!(doc.layers()[0].tiles.allocated_tile_count(), 0);
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
    fn transparent_write_releases_tile_and_noop_has_no_history() {
        let mut tiles = SparseTiles::new(256, 256).unwrap();
        assert!(tiles
            .write_rect(0, 0, 1, 1, &[0, 0, 0, 0])
            .unwrap()
            .is_empty());
        tiles.write_rect(0, 0, 1, 1, &[255, 0, 0, 255]).unwrap();
        let changes = tiles.write_rect(0, 0, 1, 1, &[200, 1, 2, 0]).unwrap();
        assert_eq!(tiles.allocated_tile_count(), 0);
        tiles.apply_changes(&changes, true).unwrap();
        assert_eq!(tiles.pixel(0, 0), Some([255, 0, 0, 255]));
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
            regions: vec![
                SelectionRegion {
                    points: Vec::new(),
                    radius: 0.,
                    shape: SelectionShape::Rectangle,
                    bounds: [0.0, 0.0, 32.0, 32.0],
                    operation: SelectionOperation::Replace,
                },
                SelectionRegion {
                    points: Vec::new(),
                    radius: 0.,
                    shape: SelectionShape::Rectangle,
                    bounds: [16.0, 0.0, 16.0, 32.0],
                    operation: SelectionOperation::Subtract,
                },
            ],
        };
        let dab = RasterDab {
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
