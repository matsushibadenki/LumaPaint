//! Bounded RGB8 PSD/PSB writing. Pixel data stays in Rust; no renderer dependency.
use super::*;
use crate::export::{ExportError, ExportOptions, ExportedDocument, FormatId};
use std::collections::BTreeMap;
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
fn bounds(layer: &RasterLayerState, width: u32, height: u32) -> [u32; 4] {
    let mut rect = [height, width, 0, 0];
    for tile in &layer.tiles {
        rect[0] = rect[0].min(tile.coord.y * TILE_SIZE);
        rect[1] = rect[1].min(tile.coord.x * TILE_SIZE);
        rect[2] = rect[2].max(((tile.coord.y + 1) * TILE_SIZE).min(height));
        rect[3] = rect[3].max(((tile.coord.x + 1) * TILE_SIZE).min(width));
    }
    if layer.tiles.is_empty() {
        [0; 4]
    } else {
        rect
    }
}
fn extra(
    layer: &RasterLayerState,
    state: &TiledRasterState,
    has_mask: bool,
) -> Result<Vec<u8>, ExportError> {
    let mut out = Output(Vec::new());
    if has_mask {
        out.length(20, false)?;
        for value in [0, 0, state.height, state.width] {
            out.bytes(&value.to_be_bytes())?;
        }
        out.bytes(&[
            255,
            u8::from(!layer.mask_enabled) * 2 + u8::from(layer.mask_inverted) * 4,
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
    let canvas_area = u64::from(state.width) * u64::from(state.height);
    let estimated_pixels = state.layers.iter().fold(canvas_area * 4, |sum, layer| {
        let rect = bounds(layer, state.width, state.height);
        let area = u64::from(rect[2] - rect[0]) * u64::from(rect[3] - rect[1]);
        let mask = !layer.mask_tiles.is_empty()
            || layer.mask_enabled
            || layer.mask_inverted
            || layer.mask_density != 1.;
        sum + area * 4 + if mask { canvas_area } else { 0 }
    });
    if estimated_pixels + 32768 > MAX_OUTPUT_BYTES as u64 {
        return Err(invalid("PSD/PSB export exceeds 128 MiB"));
    }
    let mut report = ConversionReport::default();
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
    let resources = state
        .resolution
        .map(resolution_resource)
        .unwrap_or_default();
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
            let mask = !layer.mask_tiles.is_empty()
                || layer.mask_enabled
                || layer.mask_inverted
                || layer.mask_density != 1.;
            (layer, bounds(layer, state.width, state.height), mask)
        })
        .collect();
    for &(layer, rect, mask) in &records {
        for value in rect {
            out.bytes(&value.to_be_bytes())?;
        }
        out.bytes(&(if mask { 5u16 } else { 4u16 }).to_be_bytes())?;
        let area = (rect[2] - rect[0]) as usize * (rect[3] - rect[1]) as usize;
        for id in [0i16, 1, 2, -1] {
            out.bytes(&id.to_be_bytes())?;
            out.length(area + 2, large)?;
        }
        if mask {
            out.bytes(&(-2i16).to_be_bytes())?;
            out.length(state.width as usize * state.height as usize + 2, large)?;
        }
        out.bytes(b"8BIM")?;
        out.bytes(blend_key(layer.blend_mode))?;
        out.bytes(&[
            (layer.opacity * 255.).round() as u8,
            0,
            u8::from(!layer.visible) * 2 + u8::from(layer.alpha_locked),
            0,
        ])?;
        let data = extra(layer, state, mask)?;
        out.length(data.len(), false)?;
        out.bytes(&data)?;
    }
    for &(layer, rect, mask) in &records {
        let tiles: BTreeMap<_, _> = layer
            .tiles
            .iter()
            .map(|t| (t.coord, t.pixels.as_slice()))
            .collect();
        let masks: BTreeMap<_, _> = layer
            .mask_tiles
            .iter()
            .map(|t| (t.coord, t.values.as_slice()))
            .collect();
        for channel in 0..4 {
            out.bytes(&[0, 0])?;
            for y in rect[0]..rect[2] {
                let mut row = Vec::with_capacity((rect[3] - rect[1]) as usize);
                for x in rect[1]..rect[3] {
                    let coord = TileCoord {
                        x: x / TILE_SIZE,
                        y: y / TILE_SIZE,
                    };
                    let index = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize * 4;
                    row.push(tiles.get(&coord).map_or(0, |t| t[index + channel]));
                }
                out.bytes(&row)?;
            }
        }
        if mask {
            out.bytes(&[0, 0])?;
            for y in 0..state.height {
                let mut row = Vec::with_capacity(state.width as usize);
                for x in 0..state.width {
                    let coord = TileCoord {
                        x: x / TILE_SIZE,
                        y: y / TILE_SIZE,
                    };
                    let index = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize;
                    let value = masks.get(&coord).map_or(255, |m| m[index]);
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
            RasterBlendMode::Darken,
            RasterBlendMode::Lighten,
            RasterBlendMode::Difference,
            RasterBlendMode::Exclusion,
        ] {
            state.layers[1].blend_mode = mode;
            state.layers[1].opacity = 128. / 255.;
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
