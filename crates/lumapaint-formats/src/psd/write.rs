//! Bounded RGB8 PSD/PSB writing. Pixel data stays in Rust; no renderer dependency.
use super::*;
use crate::export::{ExportError, ExportOptions, ExportedDocument, FormatId};
use std::collections::{BTreeMap, BTreeSet};
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;
struct Output(Vec<u8>);
impl Output {
    fn bytes(&mut self, bytes: &[u8]) -> Result<(), ExportError> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > MAX_OUTPUT_BYTES)
        {
            return Err(invalid("PSD/PSB export exceeds 128 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn length(&mut self, value: usize, large: bool) -> Result<(), ExportError> {
        if large {
            self.bytes(&(value as u64).to_be_bytes())
        } else {
            self.bytes(
                &u32::try_from(value)
                    .map_err(|_| invalid("PSD section length"))?
                    .to_be_bytes(),
            )
        }
    }
    fn patch(&mut self, position: usize, value: usize, large: bool) {
        let size = if large { 8 } else { 4 };
        let bytes = (value as u64).to_be_bytes();
        self.0[position..position + size].copy_from_slice(&bytes[8 - size..]);
    }
}
fn invalid(message: &str) -> ExportError {
    ExportError::InvalidDocument(message.into())
}
fn issue(report: &mut ConversionReport, code: &'static str) {
    if !report.issues.iter().any(|i| i.code == code) {
        report.issues.push(ConversionIssue {
            code,
            tier: CompatibilityTier::B,
        });
    }
}
fn bounds(layer: &RasterLayerState, width: u32, height: u32) -> [i32; 4] {
    let mut rect = [i64::from(height), i64::from(width), 0, 0];
    let mut any = false;
    for tile in &layer.tiles {
        any = true;
        rect[0] = rect[0].min(i64::from(tile.coord.y * TILE_SIZE));
        rect[1] = rect[1].min(i64::from(tile.coord.x * TILE_SIZE));
        rect[2] = rect[2].max(i64::from(((tile.coord.y + 1) * TILE_SIZE).min(height)));
        rect[3] = rect[3].max(i64::from(((tile.coord.x + 1) * TILE_SIZE).min(width)));
    }
    // Use exact nonzero bounds, including RGB under zero alpha, so signed
    // tile rounding cannot overflow PSD's signed coordinates.
    for tile in &layer.off_canvas_tiles {
        for (index, pixel) in tile.pixels.as_chunks::<4>().0.iter().enumerate() {
            if *pixel == [0; 4] {
                continue;
            }
            let x =
                i64::from(tile.x) * i64::from(TILE_SIZE) + (index as i64 % i64::from(TILE_SIZE));
            let y =
                i64::from(tile.y) * i64::from(TILE_SIZE) + (index as i64 / i64::from(TILE_SIZE));
            if !any {
                rect = [y, x, y + 1, x + 1];
                any = true;
            } else {
                rect[0] = rect[0].min(y);
                rect[1] = rect[1].min(x);
                rect[2] = rect[2].max(y + 1);
                rect[3] = rect[3].max(x + 1);
            }
        }
    }
    if any {
        rect.map(|v| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
    } else {
        [0; 4]
    }
}
fn mask_bounds(layer: &RasterLayerState, width: u32, height: u32) -> [i32; 4] {
    let canvas = [0, 0, height as i32, width as i32];
    let background = (255. * layer.mask_density).round() as u8;
    if layer.off_canvas_tiles.is_empty() || background == 0 || background == 255 {
        return canvas;
    }
    let rect = bounds(layer, width, height);
    [
        canvas[0].min(rect[0]),
        canvas[1].min(rect[1]),
        canvas[2].max(rect[2]),
        canvas[3].max(rect[3]),
    ]
}
fn rect_area(rect: [i32; 4]) -> u64 {
    ((i64::from(rect[2]) - i64::from(rect[0])) as u64)
        .saturating_mul((i64::from(rect[3]) - i64::from(rect[1])) as u64)
}
fn extra(
    layer: &RasterLayerState,
    mask_rect: [i32; 4],
    has_mask: bool,
    mask_background: u8,
) -> Result<Vec<u8>, ExportError> {
    let mut out = Output(Vec::new());
    if has_mask {
        out.length(20, false)?;
        for value in mask_rect {
            out.bytes(&value.to_be_bytes())?;
        }
        out.bytes(&[
            mask_background,
            u8::from(!layer.mask_linked)
                + u8::from(!layer.mask_enabled) * 2
                + u8::from(layer.mask_inverted) * 4,
            0,
            0,
        ])?;
    } else {
        out.length(0, false)?;
    }
    out.length(0, false)?; // Default blending ranges.
    out.bytes(&[5, b'L', b'a', b'y', b'e', b'r', 0, 0])?;
    let units: Vec<u16> = layer.name.encode_utf16().collect();
    out.bytes(b"8BIMluni")?;
    out.length(4 + units.len() * 2, false)?;
    out.length(units.len(), false)?;
    for unit in units {
        out.bytes(&unit.to_be_bytes())?;
    }
    out.bytes(b"8BIMlspf")?;
    out.length(4, false)?;
    out.bytes(&[
        0,
        0,
        0,
        if layer.locked {
            7
        } else {
            u8::from(layer.alpha_locked)
        },
    ])?;
    Ok(out.0)
}
fn resolution_resource(resolution: RasterResolution) -> Vec<u8> {
    let display = |unit| match unit {
        ResolutionDisplayUnit::PixelsPerInch => 1u16,
        ResolutionDisplayUnit::PixelsPerCentimeter => 2,
    };
    let dimension = |unit| match unit {
        PrintDimensionUnit::Inches => 1u16,
        PrintDimensionUnit::Centimeters => 2,
        PrintDimensionUnit::Points => 3,
        PrintDimensionUnit::Picas => 4,
        PrintDimensionUnit::Columns => 5,
    };
    let mut bytes = b"8BIM".to_vec();
    bytes.extend(1005u16.to_be_bytes());
    bytes.extend([0, 0]);
    bytes.extend(16u32.to_be_bytes());
    for (ppi, unit, size_unit) in [
        (
            resolution.x_ppi,
            resolution.x_display,
            resolution.width_display,
        ),
        (
            resolution.y_ppi,
            resolution.y_display,
            resolution.height_display,
        ),
    ] {
        bytes.extend(((ppi * 65536.).round() as u32).to_be_bytes());
        bytes.extend(display(unit).to_be_bytes());
        bytes.extend(dimension(size_unit).to_be_bytes());
    }
    bytes
}
/// Normal RGB8 raster layers only. Quantized opacity/mask settings require consent.
pub fn write(
    state: &TiledRasterState,
    large: bool,
    options: ExportOptions,
) -> Result<ExportedDocument, ExportError> {
    let source_bytes = state
        .layers
        .iter()
        .flat_map(|l| {
            l.tiles
                .iter()
                .map(|t| t.pixels.len())
                .chain(l.mask_tiles.iter().map(|t| t.values.len()))
                .chain(l.off_canvas_tiles.iter().map(|t| t.pixels.len()))
        })
        .try_fold(0usize, |total, size| total.checked_add(size))
        .ok_or_else(|| invalid("Raster payload size"))?;
    if source_bytes > MAX_SOURCE_BYTES
        || state.width > MAX_DIMENSION
        || state.height > MAX_DIMENSION
    {
        return Err(invalid("PSD/PSB export source exceeds limits"));
    }
    // Reuse the independent model's complete validation before trusting pixel buffers.
    lumapaint_core::tiles::TiledRasterDocument::from_state(state.clone())
        .map_err(ExportError::InvalidDocument)?;
    // A single ordinary mask can be represented as a PSD pixel mask. Keep
    // the original image pixels and mask link rather than baking its alpha.
    // Outside mask geometry is not yet represented by the native mask tiles.
    let can_convert = |layer: &RasterLayerState| {
        !layer.effects.enabled
            && layer.effects.mask.is_some()
            && !layer.mask_present
            && !layer.mask_enabled
            && layer.mask_tiles.is_empty()
            && layer.off_canvas_tiles.is_empty()
    };
    let converted_mask_ids: BTreeSet<_> = state
        .layers
        .iter()
        .filter(|layer| can_convert(layer))
        .map(|layer| layer.id.clone())
        .collect();
    let mask_rasterized = !converted_mask_ids.is_empty();
    let converted;
    let state = if mask_rasterized {
        let area = u64::from(state.width) * u64::from(state.height);
        let count = state.layers.iter().filter(|l| can_convert(l)).count() as u64;
        if area.saturating_mul(count) > MAX_SOURCE_BYTES as u64 {
            return Err(invalid("PSD mask conversion exceeds the memory limit. / PSDマスク変換がメモリ上限を超えました。 / PSD蒙版转换超出内存限制。"));
        }
        converted = {
            let mut copy = state.clone();
            for layer in &mut copy.layers {
                if !can_convert(layer) {
                    continue;
                }
                let original = layer.effects.mask.take().expect("checked mask");
                let mut raw = original.clone();
                raw.enabled = true;
                raw.inverted = false;
                raw.density = 1.;
                let mut masks = Vec::new();
                for ty in 0..copy.height.div_ceil(TILE_SIZE) {
                    for tx in 0..copy.width.div_ceil(TILE_SIZE) {
                        let mut values = vec![255; (TILE_SIZE * TILE_SIZE) as usize];
                        let mut changed = false;
                        for y in 0..(copy.height - ty * TILE_SIZE).min(TILE_SIZE) {
                            for x in 0..(copy.width - tx * TILE_SIZE).min(TILE_SIZE) {
                                let value = (raw.coverage([
                                    (tx * TILE_SIZE + x) as f32 + 0.5,
                                    (ty * TILE_SIZE + y) as f32 + 0.5,
                                ]) * 255.)
                                    .round()
                                    .clamp(0., 255.)
                                    as u8;
                                values[(y * TILE_SIZE + x) as usize] = value;
                                changed |= value != 255;
                            }
                        }
                        if changed {
                            masks.push(lumapaint_core::tiles::MaskTileState {
                                coord: TileCoord { x: tx, y: ty },
                                values,
                            });
                        }
                    }
                }
                layer.mask_present = true;
                layer.mask_linked = original.linked;
                layer.mask_enabled = original.enabled;
                layer.mask_inverted = original.inverted;
                layer.mask_density = original.density;
                layer.mask_tiles = masks;
            }
            copy
        };
        &converted
    } else {
        state
    };
    // Baking only touches canvas tiles. Refuse when it would leave outside
    // pixels in a different appearance; PSD cannot represent the effect here.
    if state
        .layers
        .iter()
        .any(|layer| layer.effects.active() && !layer.off_canvas_tiles.is_empty())
    {
        return Err(invalid("Effects on off-canvas pixels cannot yet be exported. / キャンバス外の画素に適用した効果はまだ書き出せません。 / 暂不支持导出应用于画布外像素的效果。"));
    }
    let disabled_effect_mask_lost = state.layers.iter().any(|layer| {
        layer
            .effects
            .mask
            .as_ref()
            .is_some_and(|mask| !mask.enabled)
    });
    let effects_baked = state.layers.iter().any(|l| l.effects.active());
    let mut baked;
    let state = if effects_baked {
        for layer in &state.layers {
            layer
                .effects
                .validate()
                .map_err(ExportError::InvalidDocument)?;
        }
        baked = state.clone();
        for layer in &mut baked.layers {
            if layer.effects.active() {
                let effects = layer.effects.prepare();
                for tile in &mut layer.tiles {
                    for (index, pixel) in tile.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate()
                    {
                        let edge = lumapaint_core::tiles::TILE_SIZE;
                        *pixel = effects.apply_at(
                            *pixel,
                            [
                                (tile.coord.x * edge + index as u32 % edge) as f32 + 0.5,
                                (tile.coord.y * edge + index as u32 / edge) as f32 + 0.5,
                            ],
                        );
                    }
                }
            }
            layer.effects = Default::default();
        }
        &baked
    } else {
        state
    };
    let canvas_area = u64::from(state.width) * u64::from(state.height);
    let estimated_pixels = state.layers.iter().fold(canvas_area * 4, |sum, layer| {
        let rect = bounds(layer, state.width, state.height);
        let area = ((i64::from(rect[2]) - i64::from(rect[0])) as u64)
            .saturating_mul((i64::from(rect[3]) - i64::from(rect[1])) as u64);
        let mask = layer.mask_present
            || !layer.mask_tiles.is_empty()
            || layer.mask_enabled
            || layer.mask_inverted
            || layer.mask_density != 1.;
        sum.saturating_add(area.saturating_mul(4))
            .saturating_add(if mask {
                rect_area(mask_bounds(layer, state.width, state.height))
            } else {
                0
            })
    });
    if estimated_pixels.saturating_add(32768) > MAX_OUTPUT_BYTES as u64 {
        return Err(invalid("PSD/PSB export exceeds 128 MiB"));
    }
    let mut report = ConversionReport::default();
    if effects_baked {
        issue(&mut report, "psd.layerEffectsBaked");
    }
    if mask_rasterized {
        issue(&mut report, "psd.effectMaskRasterized");
    }
    if disabled_effect_mask_lost {
        report.issues.push(ConversionIssue {
            code: "psd.disabledEffectMaskNotPreserved",
            tier: CompatibilityTier::C,
        });
    }
    if state.resolution.is_some_and(|r| {
        [r.x_ppi, r.y_ppi]
            .iter()
            .any(|v| (*v * 65536.).round() / 65536. != *v)
    }) {
        issue(&mut report, "psd.resolutionQuantized");
    }
    for layer in &state.layers {
        let opacity = (layer.opacity * 255.).round() as u8;
        if (layer.opacity - f32::from(opacity) / 255.).abs() > 1e-7 {
            issue(&mut report, "psd.opacityQuantized");
        }
        if layer.mask_density != 1. {
            issue(&mut report, "psd.nativeMaskDensityBaked");
        }
        if layer.locked && !layer.alpha_locked {
            issue(&mut report, "psd.protectionExpanded");
        }
    }
    if !options.allow_lossy && !report.issues.is_empty() {
        return Err(ExportError::LossyConversionRequiresConsent(report));
    }
    let mut out = Output(Vec::new());
    out.bytes(b"8BPS")?;
    out.bytes(&(if large { 2u16 } else { 1u16 }).to_be_bytes())?;
    out.bytes(&[0; 6])?;
    out.bytes(&4u16.to_be_bytes())?;
    out.bytes(&state.height.to_be_bytes())?;
    out.bytes(&state.width.to_be_bytes())?;
    out.bytes(&8u16.to_be_bytes())?;
    out.bytes(&3u16.to_be_bytes())?;
    out.length(0, false)?;
    let mut resources = state
        .resolution
        .map(resolution_resource)
        .unwrap_or_default();
    if let Some(profile) = &state.icc_profile {
        resources.extend(b"8BIM");
        resources.extend(1039u16.to_be_bytes());
        resources.extend([0, 0]);
        resources.extend((profile.len() as u32).to_be_bytes());
        resources.extend(profile);
        if profile.len() % 2 != 0 {
            resources.push(0);
        }
    }
    out.length(resources.len(), false)?;
    out.bytes(&resources)?;
    let section = out.0.len();
    out.length(0, large)?;
    let info = out.0.len();
    out.length(0, large)?;
    let info_start = out.0.len();
    out.bytes(&(-(state.layers.len() as i16)).to_be_bytes())?;
    let records: Vec<_> = state
        .layers
        .iter()
        .map(|layer| {
            let mask = layer.mask_present
                || !layer.mask_tiles.is_empty()
                || layer.mask_enabled
                || layer.mask_inverted
                || layer.mask_density != 1.;
            (
                layer,
                bounds(layer, state.width, state.height),
                mask,
                mask_bounds(layer, state.width, state.height),
            )
        })
        .collect();
    for &(layer, rect, mask, mask_rect) in &records {
        for value in rect {
            out.bytes(&value.to_be_bytes())?;
        }
        out.bytes(&(if mask { 5u16 } else { 4u16 }).to_be_bytes())?;
        let area = (i64::from(rect[2]) - i64::from(rect[0])) as usize
            * (i64::from(rect[3]) - i64::from(rect[1])) as usize;
        for id in [0i16, 1, 2, -1] {
            out.bytes(&id.to_be_bytes())?;
            out.length(area + 2, large)?;
        }
        if mask {
            out.bytes(&(-2i16).to_be_bytes())?;
            out.length(rect_area(mask_rect) as usize + 2, large)?;
        }
        out.bytes(b"8BIM")?;
        out.bytes(blend_key(layer.blend_mode))?;
        out.bytes(&[
            (layer.opacity * 255.).round() as u8,
            0,
            u8::from(!layer.visible) * 2 + u8::from(layer.alpha_locked),
            0,
        ])?;
        let background = if converted_mask_ids.contains(&layer.id) {
            0
        } else if layer.mask_density == 1. {
            255
        } else if (255. * layer.mask_density).round() < 1. {
            0
        } else {
            255
        };
        let data = extra(layer, mask_rect, mask, background)?;
        out.length(data.len(), false)?;
        out.bytes(&data)?;
    }
    for &(layer, rect, mask, mask_rect) in &records {
        let tiles: BTreeMap<_, _> = layer
            .tiles
            .iter()
            .map(|t| (t.coord, t.pixels.as_slice()))
            .collect();
        let outside: BTreeMap<_, _> = layer
            .off_canvas_tiles
            .iter()
            .map(|t| ((t.x, t.y), t.pixels.as_slice()))
            .collect();
        let masks: BTreeMap<_, _> = layer
            .mask_tiles
            .iter()
            .map(|t| (t.coord, t.values.as_slice()))
            .collect();
        for channel in 0..4 {
            out.bytes(&[0, 0])?;
            for y in rect[0]..rect[2] {
                let mut row =
                    Vec::with_capacity((i64::from(rect[3]) - i64::from(rect[1])) as usize);
                for x in rect[1]..rect[3] {
                    let edge = TILE_SIZE as i32;
                    let index = ((y.rem_euclid(edge) as u32) * TILE_SIZE
                        + x.rem_euclid(edge) as u32) as usize
                        * 4;
                    let value = if x >= 0
                        && y >= 0
                        && (x as u32) < state.width
                        && (y as u32) < state.height
                    {
                        let coord = TileCoord {
                            x: x as u32 / TILE_SIZE,
                            y: y as u32 / TILE_SIZE,
                        };
                        tiles.get(&coord).map_or(0, |t| t[index + channel])
                    } else {
                        outside
                            .get(&(x.div_euclid(edge), y.div_euclid(edge)))
                            .map_or(0, |t| t[index + channel])
                    };
                    row.push(value);
                }
                out.bytes(&row)?;
            }
        }
        if mask {
            out.bytes(&[0, 0])?;
            for y in mask_rect[0]..mask_rect[2] {
                let mut row = Vec::with_capacity(
                    (i64::from(mask_rect[3]) - i64::from(mask_rect[1])) as usize,
                );
                for x in mask_rect[1]..mask_rect[3] {
                    let value = if x >= 0
                        && y >= 0
                        && (x as u32) < state.width
                        && (y as u32) < state.height
                    {
                        let coord = TileCoord {
                            x: x as u32 / TILE_SIZE,
                            y: y as u32 / TILE_SIZE,
                        };
                        let index = (((y as u32) % TILE_SIZE) * TILE_SIZE + (x as u32) % TILE_SIZE)
                            as usize;
                        masks.get(&coord).map_or(255, |m| m[index])
                    } else {
                        255
                    };
                    row.push((f32::from(value) * layer.mask_density).round() as u8);
                }
                out.bytes(&row)?;
            }
        }
    }
    if !(out.0.len() - info_start).is_multiple_of(2) {
        out.bytes(&[0])?;
    }
    out.patch(info, out.0.len() - info_start, large);
    out.length(0, false)?; // Global mask.
    out.patch(section, out.0.len() - info, large);
    out.bytes(&[0, 0])?;
    let columns = state.width.div_ceil(TILE_SIZE);
    // Planar output requires revisiting tile rows for each channel. The cache is one tile row.
    for channel in 0..4 {
        for ty in 0..state.height.div_ceil(TILE_SIZE) {
            let tiles: Vec<_> = (0..columns)
                .map(|tx| {
                    lumapaint_core::tiles::composite_raster_layers_tile(
                        &state.layers,
                        TileCoord { x: tx, y: ty },
                    )
                })
                .collect();
            for y in 0..(state.height - ty * TILE_SIZE).min(TILE_SIZE) {
                let mut row = Vec::with_capacity(state.width as usize);
                for x in 0..state.width {
                    let index = (y * TILE_SIZE + x % TILE_SIZE) as usize * 4;
                    let pixel = tiles[(x / TILE_SIZE) as usize]
                        .as_ref()
                        .map_or([0; 4], |t| t[index..index + 4].try_into().expect("RGBA"));
                    row.push(if channel == 3 {
                        pixel[3]
                    } else {
                        pixel[channel].saturating_add(255 - pixel[3])
                    });
                }
                out.bytes(&row)?;
            }
        }
    }
    Ok(ExportedDocument {
        format: if large { FormatId::Psb } else { FormatId::Psd },
        media_type: "image/vnd.adobe.photoshop",
        bytes: out.0,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_off_canvas_pixels_survive_native_psd_and_psb() {
        let mut doc = lumapaint_core::tiles::TiledRasterDocument::new(4, 2).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        doc.write_rect("paint", [1, 1, 1, 1], &[40, 60, 80, 255])
            .unwrap();
        let mut state = doc.state();
        let mut negative = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
        let at = ((TILE_SIZE - 1) * TILE_SIZE + TILE_SIZE - 1) as usize * 4;
        negative[at..at + 4].copy_from_slice(&[17, 35, 91, 0]);
        let mut positive = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
        let at = (2 * TILE_SIZE + 4) as usize * 4;
        positive[at..at + 4].copy_from_slice(&[7, 33, 127, 181]);
        state.layers[0].off_canvas_tiles = vec![
            lumapaint_core::tiles::OffCanvasTileState {
                x: -1,
                y: -1,
                pixels: negative,
            },
            lumapaint_core::tiles::OffCanvasTileState {
                x: 0,
                y: 0,
                pixels: positive,
            },
        ];
        let mut invalid = state.clone();
        let duplicate = invalid.layers[0].off_canvas_tiles[0].clone();
        invalid.layers[0].off_canvas_tiles.push(duplicate);
        assert!(crate::tile_container::encode(&invalid).is_err());
        let mut guarded = state.clone();
        guarded.layers[0].mask_present = true;
        let mut live = lumapaint_core::tiles::TiledRasterDocument::from_state(guarded).unwrap();
        let pixels = live.layers()[0].tiles.clone();
        let mask = live.layers()[0].mask.clone();
        assert!(live
            .transform_sparse_mask("paint", Some(pixels), mask)
            .is_err());
        assert_eq!(
            live.state().layers[0].off_canvas_tiles,
            state.layers[0].off_canvas_tiles
        );
        let native = crate::tile_container::encode(&state).unwrap();
        let restored = crate::tile_container::decode(&native).unwrap();
        assert_eq!(
            restored.layers[0].off_canvas_tiles,
            state.layers[0].off_canvas_tiles
        );
        for large in [false, true] {
            let saved = write(&restored, large, ExportOptions::default()).unwrap();
            let imported = PsdImporter
                .import(&saved.bytes, ImportOptions::default())
                .unwrap();
            assert_eq!(imported.raster.layers.len(), 1, "{:?}", imported.report);
            assert_eq!(
                imported.raster.layers[0].off_canvas_tiles,
                state.layers[0].off_canvas_tiles
            );
        }
    }

    #[test]
    fn photoshop_resaved_off_canvas_pixels_round_trip_psd_and_psb() {
        for (large, bytes) in [
            (
                false,
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-offcanvas-simple.psd")
                    .as_slice(),
            ),
            (
                true,
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-offcanvas-simple.psb")
                    .as_slice(),
            ),
        ] {
            let input = PsdImporter
                .import(bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert_eq!(input.raster.layers.len(), 1, "{:?}", input.report);
            assert!(!input.raster.layers[0].off_canvas_tiles.is_empty());
            let saved = write(&input.raster, large, ExportOptions { allow_lossy: true }).unwrap();
            let output = PsdImporter
                .import(&saved.bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert_eq!(
                output.raster.layers[0].off_canvas_tiles,
                input.raster.layers[0].off_canvas_tiles
            );
            assert_eq!(output.raster.layers[0].tiles, input.raster.layers[0].tiles);
        }
    }

    #[test]
    fn disabled_effect_masks_are_reported_and_outside_effects_are_not_partially_baked() {
        use lumapaint_core::layer_mask::{LayerMask, MaskKind};
        let mut doc = lumapaint_core::tiles::TiledRasterDocument::new(2, 2).unwrap();
        doc.add_layer("a".into(), "A".into()).unwrap();
        doc.write_rect("a", [0, 0, 1, 1], &[100, 50, 20, 255])
            .unwrap();
        let mut state = doc.state();
        let mut mask = LayerMask::from_selection(MaskKind::Pixel, None, 2, 2).unwrap();
        mask.enabled = false;
        state.layers[0].effects.mask = Some(mask);
        let mut pixels = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
        let at = (TILE_SIZE as usize - 1) * 4;
        pixels[at..at + 4].copy_from_slice(&[10, 20, 30, 255]);
        state.layers[0]
            .off_canvas_tiles
            .push(lumapaint_core::tiles::OffCanvasTileState {
                x: -1,
                y: 0,
                pixels,
            });
        for large in [false, true] {
            let Err(error) = write(&state, large, ExportOptions::default()) else {
                panic!("disabled mask must require consent");
            };
            assert!(
                matches!(error, ExportError::LossyConversionRequiresConsent(ref report) if report.issues.iter().any(|i| i.code == "psd.disabledEffectMaskNotPreserved"))
            );
            let saved = write(&state, large, ExportOptions { allow_lossy: true }).unwrap();
            assert!(saved
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.disabledEffectMaskNotPreserved"));
        }
        state.layers[0].effects.mask.as_mut().unwrap().enabled = true;
        for large in [false, true] {
            assert!(matches!(
                write(&state, large, ExportOptions { allow_lossy: true }),
                Err(ExportError::InvalidDocument(_))
            ));
        }
    }

    #[test]
    fn transformed_effect_mask_exports_as_editable_psd_pixel_mask() {
        use lumapaint_core::{
            layer_mask::{LayerMask, MaskKind},
            selection::{Selection, SelectionShape},
        };
        for enabled in [true, false] {
            let mut doc = lumapaint_core::tiles::TiledRasterDocument::new(3, 2).unwrap();
            doc.add_layer("a".into(), "A".into()).unwrap();
            doc.write_rect("a", [0, 0, 3, 2], &[200, 100, 50, 255].repeat(6))
                .unwrap();
            let mut state = doc.state();
            let selection = Selection::new(SelectionShape::Rectangle, [0., 0., 1., 2.]);
            let mut mask =
                LayerMask::from_selection(MaskKind::Pixel, Some(&selection), 3, 2).unwrap();
            mask.transform = [1., 0., 0., 1., 1., 0.];
            mask.enabled = enabled;
            mask.linked = false;
            state.layers[0].effects.mask = Some(mask);
            for large in [false, true] {
                assert!(matches!(
                    write(&state, large, ExportOptions::default()),
                    Err(ExportError::LossyConversionRequiresConsent(_))
                ));
                let saved = write(&state, large, ExportOptions { allow_lossy: true }).unwrap();
                assert!(saved
                    .report
                    .issues
                    .iter()
                    .any(|i| i.code == "psd.effectMaskRasterized"));
                // A moved PSD mask exposes its background; it must match the
                // original ordinary mask's zero coverage outside the canvas.
                let mut reader = Reader {
                    bytes: &saved.bytes,
                    at: 26,
                };
                reader.section().unwrap();
                reader.section().unwrap();
                let layers = reader.sized_section(large).unwrap();
                let mut outer = Reader {
                    bytes: layers,
                    at: 0,
                };
                let info = outer.sized_section(large).unwrap();
                let mut record = Reader { bytes: info, at: 2 };
                record.take(16).unwrap();
                let channels = record.u16().unwrap();
                for _ in 0..channels {
                    record.take(2).unwrap();
                    record.length(large).unwrap();
                }
                record.take(12).unwrap();
                let extra = record.section().unwrap();
                let mut extra_reader = Reader {
                    bytes: extra,
                    at: 0,
                };
                let mask = extra_reader.section().unwrap();
                assert_eq!(mask[16], 0);
                let imported = PsdImporter
                    .import(&saved.bytes, ImportOptions { allow_lossy: true })
                    .unwrap();
                assert_eq!(imported.raster.layers.len(), 1, "{:?}", imported.report);
                let layer = &imported.raster.layers[0];
                assert!(layer.mask_present);
                assert_eq!(layer.mask_enabled, enabled);
                assert!(!layer.mask_linked);
                let live = lumapaint_core::tiles::TiledRasterDocument::from_state(imported.raster)
                    .unwrap();
                assert_eq!(live.layers()[0].mask.pixel(0, 0), Some(0));
                assert_eq!(live.layers()[0].mask.pixel(1, 0), Some(255));
                assert_eq!(live.layers()[0].mask.pixel(2, 0), Some(0));
            }
        }
    }

    fn resolution() -> RasterResolution {
        RasterResolution {
            x_ppi: 300.5,
            y_ppi: 150.25,
            x_display: ResolutionDisplayUnit::PixelsPerCentimeter,
            y_display: ResolutionDisplayUnit::PixelsPerInch,
            width_display: PrintDimensionUnit::Centimeters,
            height_display: PrintDimensionUnit::Points,
        }
    }
    #[test]
    fn effects_export_requires_consent_bakes_appearance_and_native_preserves_source() {
        let mut document = lumapaint_core::tiles::TiledRasterDocument::new(1, 1).unwrap();
        document
            .add_layer("image".into(), "日本語画像".into())
            .unwrap();
        document
            .write_rect("image", [0, 0, 1, 1], &[64, 32, 16, 255])
            .unwrap();
        document
            .set_layer_effects(
                "image",
                lumapaint_core::layer_effects::LayerEffects {
                    enabled: true,
                    values: [1., 0., 0., 0., 0., 0., 0., 0., 0., 0.],
                    mixer: [[20., 30., 10.]; 8],
                    grading: [[30., 15., 2.], [200., 10., -5.], [270., 20., 3.]],
                    grading_blend: 75.,
                    grading_balance: -35.,
                    curves: [
                        vec![[0.08, 0.], [0.4, 0.65], [0.95, 1.]],
                        vec![[0., 0.], [1., 1.]],
                        vec![[0., 0.], [1., 1.]],
                        vec![[0., 0.], [1., 1.]],
                    ],
                    ..Default::default()
                },
            )
            .unwrap();
        let source = document.state();
        let native = crate::tile_container::encode(&source).unwrap();
        assert_eq!(crate::tile_container::decode(&native).unwrap(), source);
        for large in [false, true] {
            assert!(matches!(
                write(&source, large, ExportOptions::default()),
                Err(ExportError::LossyConversionRequiresConsent(_))
            ));
            let exported = write(&source, large, ExportOptions { allow_lossy: true }).unwrap();
            assert!(exported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.layerEffectsBaked"));
            let loaded = crate::psd::PsdImporter
                .import(&exported.bytes, crate::ImportOptions::default())
                .unwrap()
                .raster;
            assert_eq!(
                &loaded.layers[0].tiles[0].pixels[..4],
                &source.layers[0].effects.apply([64, 32, 16, 255])
            );
            assert!(!loaded.layers[0].effects.enabled);
            assert_eq!(&source.layers[0].tiles[0].pixels[..4], &[64, 32, 16, 255]);
        }
    }
    #[test]
    fn white_disabled_mask_survives_native_psd_and_psb() {
        let mut doc = lumapaint_core::tiles::TiledRasterDocument::new(8, 8).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        doc.write_rect("paint", [0, 0, 1, 1], &[50, 100, 150, 255])
            .unwrap();
        doc.set_layer_mask("paint", true, false, 1.).unwrap();
        doc.set_layer_mask("paint", false, false, 1.).unwrap();
        let state = doc.state();
        let native = crate::tile_container::encode(&state).unwrap();
        assert_eq!(crate::tile_container::decode(&native).unwrap(), state);
        for large in [false, true] {
            let saved = write(&state, large, ExportOptions::default()).unwrap();
            let imported = PsdImporter
                .import(&saved.bytes, ImportOptions::default())
                .unwrap();
            let layer = &imported.raster.layers[0];
            assert!(layer.mask_present);
            assert!(!layer.mask_enabled);
            assert!(layer.mask_tiles.is_empty());
        }
    }
    #[test]
    fn supported_blends_round_trip_with_native_pixels_and_psd_preview_guard() {
        let mut state = PsdImporter
            .import(
                include_bytes!("../../tests/fixtures/lp-psd-layers.psd"),
                ImportOptions::default(),
            )
            .unwrap()
            .raster;
        for mode in [
            RasterBlendMode::Multiply,
            RasterBlendMode::Screen,
            RasterBlendMode::Overlay,
            RasterBlendMode::HardLight,
            RasterBlendMode::Darken,
            RasterBlendMode::Lighten,
            RasterBlendMode::Difference,
            RasterBlendMode::Exclusion,
        ] {
            state.layers[1].blend_mode = mode;
            state.layers[1].opacity = 128. / 255.;
            state.layers[1].mask_present = true;
            state.layers[1].mask_enabled = true;
            let mut values = vec![255; (TILE_SIZE * TILE_SIZE) as usize];
            for y in 0..state.height as usize {
                values[y * TILE_SIZE as usize..y * TILE_SIZE as usize + state.width as usize]
                    .fill(128);
            }
            state.layers[1].mask_tiles = vec![lumapaint_core::tiles::MaskTileState {
                coord: TileCoord { x: 0, y: 0 },
                values,
            }];
            let encoded = crate::tile_container::encode(&state).unwrap();
            assert_eq!(crate::tile_container::decode(&encoded).unwrap(), state);
            for large in [false, true] {
                let saved = write(&state, large, ExportOptions::default()).unwrap();
                assert!(saved.report.issues.is_empty());
                let imported = PsdImporter
                    .import(&saved.bytes, ImportOptions::default())
                    .unwrap();
                assert_eq!(imported.raster, state);
                // A stale/tampered merged preview must not allow silently different editable layers.
                let mut mismatch = saved.bytes.clone();
                let last = mismatch.len() - 1;
                mismatch[last] ^= 255;
                assert!(matches!(
                    PsdImporter.import(&mismatch, ImportOptions::default()),
                    Err(ImportError::LossyConversionRequiresConsent(_))
                ));
            }
        }
    }
    #[test]
    fn resolution_survives_psd_psb_native_save_and_live_restore() {
        let mut state = PsdImporter
            .import(
                include_bytes!("../../tests/fixtures/lp-psd-layers.psd"),
                ImportOptions::default(),
            )
            .unwrap()
            .raster;
        state.resolution = Some(resolution());
        let native = crate::tile_container::encode(&state).unwrap();
        let restored = crate::tile_container::decode(&native).unwrap();
        assert_eq!(restored, state);
        let live = lumapaint_core::tiles::TiledRasterDocument::from_state(restored).unwrap();
        assert_eq!(live.resolution(), state.resolution);
        assert_eq!(live.state(), state);
        for large in [false, true] {
            let saved = write(&live.state(), large, ExportOptions::default()).unwrap();
            assert!(saved.report.issues.is_empty());
            // Independent expected bytes: 300.5 and 150.25 signed 16.16 ppi.
            assert_eq!(
                &saved.bytes[34..62],
                &[
                    56, 66, 73, 77, 3, 237, 0, 0, 0, 0, 0, 16, 1, 44, 128, 0, 0, 2, 0, 2, 0, 150,
                    64, 0, 0, 1, 0, 3
                ]
            );
            let loaded = PsdImporter
                .import(&saved.bytes, ImportOptions::default())
                .unwrap();
            assert_eq!(loaded.raster, state);
            let r = loaded.raster.resolution.unwrap();
            assert_eq!(r.size_inches(601, 601), [2., 4.]);
        }
    }
    #[test]
    fn fractional_resolution_rounding_requires_consent() {
        let mut state = PsdImporter
            .import(
                include_bytes!("../../tests/fixtures/lp-psd-layers.psd"),
                ImportOptions::default(),
            )
            .unwrap()
            .raster;
        let mut r = resolution();
        r.x_ppi = 300.123456789;
        state.resolution = Some(r);
        assert!(matches!(
            write(&state, false, ExportOptions::default()),
            Err(ExportError::LossyConversionRequiresConsent(_))
        ));
        let output = write(&state, true, ExportOptions { allow_lossy: true }).unwrap();
        assert_eq!(output.report.issues[0].code, "psd.resolutionQuantized");
        let r2 = PsdImporter
            .import(&output.bytes, ImportOptions::default())
            .unwrap()
            .raster
            .resolution
            .unwrap();
        assert!((r2.x_ppi - r.x_ppi).abs() <= 0.5 / 65536.);
        assert_eq!(r2.y_ppi, r.y_ppi);
    }
    #[test]
    fn invalid_resolution_and_duplicate_resource_are_rejected() {
        let resource = resolution_resource(resolution());
        for (position, value) in [(0, 0u32), (0, 0x80000000)] {
            let mut payload = resource[12..].to_vec();
            payload[position..position + 4].copy_from_slice(&value.to_be_bytes());
            assert!(read_resolution(&payload).is_err());
        }
        for pos in [4, 6, 12, 14] {
            let mut payload = resource[12..].to_vec();
            payload[pos..pos + 2].fill(0);
            assert!(read_resolution(&payload).is_err());
        }
        assert!(read_resolution(&resource[12..27]).is_err());
        let mut duplicated = resource.clone();
        duplicated.extend(resource);
        assert!(resources(&duplicated, &mut ConversionReport::default()).is_err());
        let mut invalid = resolution();
        invalid.y_ppi = f64::NAN;
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn unicode_raster_layers_round_trip_in_psd_and_psb() {
        let original = PsdImporter
            .import(
                include_bytes!("../../tests/fixtures/lp-psd-layers.psd"),
                ImportOptions::default(),
            )
            .unwrap()
            .raster;
        for large in [false, true] {
            let saved = write(&original, large, ExportOptions::default()).unwrap();
            let loaded = PsdImporter
                .import(&saved.bytes, ImportOptions::default())
                .unwrap();
            assert_eq!(loaded.raster, original);
            assert_eq!(
                read_header(&saved.bytes).unwrap().version,
                if large { 2 } else { 1 }
            );
        }
    }
    #[test]
    fn unlinked_sparse_mask_round_trips_psd_and_psb() {
        let mut doc = lumapaint_core::tiles::TiledRasterDocument::new(300, 4).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        doc.write_rect("paint", [257, 1, 1, 1], &[20, 30, 40, 255])
            .unwrap();
        doc.write_mask_rect("paint", [2, 1, 1, 1], &[0]).unwrap();
        doc.set_layer_mask("paint", true, false, 1.).unwrap();
        doc.set_sparse_mask_linked("paint", false).unwrap();
        let state = doc.state();
        for large in [false, true] {
            let saved = write(&state, large, ExportOptions::default()).unwrap();
            assert!(matches!(
                PsdImporter.import(&saved.bytes, ImportOptions::default()),
                Err(ImportError::LossyConversionRequiresConsent(report))
                    if report.issues.iter().any(|issue| issue.code == "psd.maskOriginAmbiguous")
            ));
            let loaded = PsdImporter
                .import(&saved.bytes, ImportOptions { allow_lossy: true })
                .unwrap()
                .raster;
            assert!(!loaded.layers[0].mask_linked);
            let restored = lumapaint_core::tiles::TiledRasterDocument::from_state(loaded).unwrap();
            assert_eq!(restored.layers()[0].mask.pixel(2, 1), Some(0));
            assert_eq!(
                restored.layers()[0].tiles.pixel(257, 1),
                Some([20, 30, 40, 255])
            );
        }
    }

    #[test]
    fn mask_density_and_opacity_quantization_are_reported_and_preserve_pixels() {
        let mut source = PsdImporter
            .import(
                include_bytes!("../../tests/fixtures/lp-psd-layers.psd"),
                ImportOptions::default(),
            )
            .unwrap()
            .raster;
        let layer = &mut source.layers[1];
        layer.opacity = 0.5;
        layer.mask_present = true;
        layer.mask_enabled = true;
        layer.mask_inverted = true;
        layer.mask_density = 0.73;
        let mut mask = vec![255; (TILE_SIZE * TILE_SIZE) as usize];
        mask[1] = 37;
        layer.mask_tiles.push(lumapaint_core::tiles::MaskTileState {
            coord: TileCoord { x: 0, y: 0 },
            values: mask,
        });
        assert!(matches!(
            write(&source, false, ExportOptions::default()),
            Err(ExportError::LossyConversionRequiresConsent(_))
        ));
        for large in [false, true] {
            let saved = write(&source, large, ExportOptions { allow_lossy: true }).unwrap();
            assert!(saved
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.opacityQuantized"));
            assert!(saved
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.nativeMaskDensityBaked"));
            let loaded = PsdImporter
                .import(&saved.bytes, ImportOptions::default())
                .unwrap()
                .raster;
            let expected = lumapaint_core::tiles::composite_raster_layers_tile(
                &source.layers,
                TileCoord { x: 0, y: 0 },
            )
            .unwrap();
            let actual = lumapaint_core::tiles::composite_raster_layers_tile(
                &loaded.layers,
                TileCoord { x: 0, y: 0 },
            )
            .unwrap();
            assert!(expected
                .iter()
                .zip(&actual)
                .all(|(a, b)| a.abs_diff(*b) <= 1));
        }
    }
    #[test]
    fn fractional_mask_density_extends_over_off_canvas_pixels() {
        let mut doc = lumapaint_core::tiles::TiledRasterDocument::new(2, 1).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        doc.write_rect("paint", [0, 0, 1, 1], &[20, 30, 40, 255])
            .unwrap();
        let mut state = doc.state();
        let mut right = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
        right[0..4].copy_from_slice(&[20, 30, 40, 255]);
        let mut left = vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize];
        left[(TILE_SIZE as usize - 1) * 4..TILE_SIZE as usize * 4]
            .copy_from_slice(&[20, 30, 40, 255]);
        let layer = &mut state.layers[0];
        layer
            .off_canvas_tiles
            .push(lumapaint_core::tiles::OffCanvasTileState {
                x: 1,
                y: 0,
                pixels: right,
            });
        layer
            .off_canvas_tiles
            .push(lumapaint_core::tiles::OffCanvasTileState {
                x: -1,
                y: 0,
                pixels: left,
            });
        layer.mask_present = true;
        for (density, expected_rect, background, expected_sample) in
            [(0.5, [0, -1, 1, 257], 255, 128), (0., [0, 0, 1, 2], 0, 0)]
        {
            for enabled in [false, true] {
                for inverted in [false, true] {
                    state.layers[0].mask_density = density;
                    state.layers[0].mask_enabled = enabled;
                    state.layers[0].mask_inverted = inverted;
                    for large in [false, true] {
                        let saved =
                            write(&state, large, ExportOptions { allow_lossy: true }).unwrap();
                        assert!(saved
                            .report
                            .issues
                            .iter()
                            .any(|i| i.code == "psd.nativeMaskDensityBaked"));
                        let mut reader = Reader {
                            bytes: &saved.bytes,
                            at: 26,
                        };
                        reader.section().unwrap();
                        reader.section().unwrap();
                        let section_len = reader.length(large).unwrap();
                        let section = reader.take(section_len).unwrap();
                        let mut section_reader = Reader {
                            bytes: section,
                            at: 0,
                        };
                        let info_len = section_reader.length(large).unwrap();
                        let info = section_reader.take(info_len).unwrap();
                        let mut record = Reader {
                            bytes: info,
                            at: 2 + 16,
                        };
                        let channels = record.u16().unwrap();
                        let mut lengths = Vec::new();
                        for _ in 0..channels {
                            record.take(2).unwrap();
                            lengths.push(record.length(large).unwrap());
                        }
                        record.take(12).unwrap();
                        let extra = record.section().unwrap();
                        for (index, expected) in expected_rect.iter().enumerate() {
                            assert_eq!(
                                i32::from_be_bytes(
                                    extra[4 + index * 4..8 + index * 4].try_into().unwrap()
                                ),
                                *expected
                            );
                        }
                        assert_eq!(extra[20], background);
                        for length in lengths.iter().take(4) {
                            record.take(*length).unwrap();
                        }
                        let mask = record.take(lengths[4]).unwrap();
                        assert_eq!(&mask[..2], &[0, 0]);
                        assert!(mask[2..].iter().all(|v| *v == expected_sample));
                        if density == 0.5 {
                            let imported = PsdImporter
                                .import(&saved.bytes, ImportOptions { allow_lossy: true })
                                .unwrap();
                            assert!(imported
                                .report
                                .issues
                                .iter()
                                .any(|i| i.code == "psd.maskOutsideCanvasClipped"));
                            assert!(!imported
                                .report
                                .issues
                                .iter()
                                .any(|i| i.code == "psd.layersFlattened"));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn transparent_canvas_rgb_round_trips_without_loss() {
        let mut doc = lumapaint_core::tiles::TiledRasterDocument::new(2, 1).unwrap();
        doc.add_layer("paint".into(), "Paint".into()).unwrap();
        doc.write_rect("paint", [0, 0, 1, 1], &[11, 22, 33, 255])
            .unwrap();
        for large in [false, true] {
            let mut file = write(&doc.state(), large, ExportOptions::default())
                .unwrap()
                .bytes;
            let red_second_pixel = {
                let mut reader = Reader {
                    bytes: &file,
                    at: 26,
                };
                reader.section().unwrap();
                reader.section().unwrap();
                reader.length(large).unwrap();
                reader.length(large).unwrap();
                let info_start = reader.at;
                let mut record = Reader {
                    bytes: &file,
                    at: info_start + 2 + 16,
                };
                let channels = record.u16().unwrap();
                for _ in 0..channels {
                    record.take(2).unwrap();
                    record.length(large).unwrap();
                }
                record.take(12).unwrap();
                record.section().unwrap();
                record.at + 2 + 1 // RAW marker and second pixel in red plane.
            };
            file[red_second_pixel] = 90;
            let result = PsdImporter.import(&file, ImportOptions::default()).unwrap();
            assert!(!result
                .report
                .issues
                .iter()
                .any(|issue| issue.code == "psd.transparentRgbDiscarded"));
            assert_eq!(result.raster.layers.len(), 1);
            assert!(!result
                .report
                .issues
                .iter()
                .any(|issue| issue.code == "psd.layersFlattened"));
            let native = crate::tile_container::encode(&result.raster).unwrap();
            let restored = lumapaint_core::tiles::TiledRasterDocument::from_state(
                crate::tile_container::decode(&native).unwrap(),
            )
            .unwrap();
            assert_eq!(
                restored.layers()[0].tiles.pixel(0, 0),
                Some([11, 22, 33, 255])
            );
            assert_eq!(restored.layers()[0].tiles.pixel(1, 0), Some([90, 0, 0, 0]));
            let again = write(&restored.state(), large, ExportOptions::default()).unwrap();
            let read = PsdImporter
                .import(&again.bytes, ImportOptions::default())
                .unwrap();
            let reread =
                lumapaint_core::tiles::TiledRasterDocument::from_state(read.raster).unwrap();
            assert_eq!(reread.layers()[0].tiles.pixel(1, 0), Some([90, 0, 0, 0]));
        }
    }

    #[test]
    fn invalid_raster_payloads_cannot_be_written() {
        let mut state = PsdImporter
            .import(
                include_bytes!("../../tests/fixtures/lp-psd-layers.psd"),
                ImportOptions::default(),
            )
            .unwrap()
            .raster;
        state.layers[0].tiles[0].pixels.pop();
        assert!(write(&state, false, ExportOptions { allow_lossy: true }).is_err());
    }
}
