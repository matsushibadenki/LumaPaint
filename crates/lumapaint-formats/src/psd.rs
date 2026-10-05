//! PSD v1 RGB8 merged-image adapter, based on Adobe's published specification.
//! Normal RGB8 raster layers can be retained; effects, masks, profiles and higher depths remain partial.
mod layers;
use crate::*;
use lumapaint_core::tiles::{
    RasterLayerState, RasterTileState, TileCoord, TiledRasterState, TILE_SIZE,
};

pub const MAX_INPUT_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_DIMENSION: u32 = 8192;
pub struct PsdImporter;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub channels: u16,
    pub width: u32,
    pub height: u32,
    pub depth: u16,
    pub color_mode: u16,
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ImportError> {
        let end = self
            .at
            .checked_add(length)
            .ok_or(ImportError::Malformed("section length overflow"))?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(ImportError::Malformed("truncated section"))?;
        self.at = end;
        Ok(value)
    }
    fn u16(&mut self) -> Result<u16, ImportError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, ImportError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn section(&mut self) -> Result<&'a [u8], ImportError> {
        let size = self.u32()? as usize;
        self.take(size)
    }
}
pub fn read_header(bytes: &[u8]) -> Result<Header, ImportError> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != b"8BPS" {
        return Err(ImportError::Malformed("PSD signature"));
    }
    match r.u16()? {
        1 => {}
        2 => return Err(ImportError::Unsupported("PSB")),
        _ => return Err(ImportError::Malformed("PSD version")),
    }
    if r.take(6)?.iter().any(|b| *b != 0) {
        return Err(ImportError::Malformed("reserved header bytes"));
    }
    let channels = r.u16()?;
    let height = r.u32()?;
    let width = r.u32()?;
    let depth = r.u16()?;
    let color_mode = r.u16()?;
    if !(1..=56).contains(&channels)
        || !(1..=30000).contains(&width)
        || !(1..=30000).contains(&height)
        || ![1, 8, 16, 32].contains(&depth)
        || ![0, 1, 2, 3, 4, 7, 8, 9].contains(&color_mode)
    {
        return Err(ImportError::Malformed("PSD header values"));
    }
    Ok(Header {
        channels,
        width,
        height,
        depth,
        color_mode,
    })
}
fn packbits(data: &[u8], width: usize) -> Result<Vec<u8>, ImportError> {
    let mut r = Reader { bytes: data, at: 0 };
    let mut out = Vec::with_capacity(width);
    while r.at < data.len() {
        let control = r.take(1)?[0] as i8;
        if control == -128 {
            continue;
        }
        let length = if control >= 0 {
            control as usize + 1
        } else {
            (1 - i16::from(control)) as usize
        };
        if out.len() + length > width {
            return Err(ImportError::Malformed("RLE row overrun"));
        }
        if control >= 0 {
            out.extend_from_slice(r.take(length)?);
        } else {
            let value = r.take(1)?[0];
            out.resize(out.len() + length, value);
        }
    }
    if out.len() != width {
        return Err(ImportError::Malformed("RLE row underrun"));
    }
    Ok(out)
}
fn resources(data: &[u8], report: &mut ConversionReport) -> Result<(), ImportError> {
    let mut r = Reader { bytes: data, at: 0 };
    while r.at < data.len() {
        if r.take(4)? != b"8BIM" {
            return Err(ImportError::Malformed("resource signature"));
        }
        let id = r.u16()?;
        let name_len = r.take(1)?[0] as usize;
        r.take(name_len)?;
        if !(name_len + 1).is_multiple_of(2) {
            r.take(1)?;
        }
        let size = r.u32()? as usize;
        r.take(size)?;
        if !size.is_multiple_of(2) {
            r.take(1)?;
        }
        let code = match id {
            1005 => "psd.resolutionNotPreserved",
            1039 => "psd.iccProfileNotPreserved",
            _ => "psd.resourceNotPreserved",
        };
        if !report.issues.iter().any(|issue| issue.code == code) {
            report.issues.push(ConversionIssue {
                code,
                tier: CompatibilityTier::D,
            });
        }
    }
    Ok(())
}
// Negative layer count identifies the first extra channel as merged transparency.
// Validate the container and record lengths before trusting that marker.
fn merged_transparency(data: &[u8]) -> Result<bool, ImportError> {
    if data.is_empty() {
        return Ok(false);
    }
    let mut outer = Reader { bytes: data, at: 0 };
    let info = outer.section()?;
    let mut transparency = false;
    if !info.is_empty() {
        let mut r = Reader { bytes: info, at: 0 };
        let count = r.u16()? as i16;
        transparency = count < 0;
        let mut payload = 0usize;
        for _ in 0..count.unsigned_abs() {
            r.take(16)?; // Rectangle; merged decoding does not interpret individual layers.
            let channels = r.u16()?;
            if channels > 56 {
                return Err(ImportError::Malformed("layer channel count"));
            }
            for _ in 0..channels {
                r.u16()?;
                let length = r.u32()? as usize;
                payload = payload
                    .checked_add(length)
                    .ok_or(ImportError::Malformed("layer payload overflow"))?;
            }
            if r.take(4)? != b"8BIM" {
                return Err(ImportError::Malformed("layer blend signature"));
            }
            r.take(8)?; // Blend key, opacity, clipping, flags, filler.
            r.section()?; // Bounded layer extra data, discarded with the layer report.
        }
        r.take(payload)?;
        if r.at != info.len() && !(r.at + 1 == info.len() && info[r.at] == 0) {
            return Err(ImportError::Malformed("layer info payload length"));
        }
    }
    outer.section()?; // Global mask, discarded along with the layer metadata.
                      // Additional layer metadata is intentionally discarded; the outer section bounds it.
    Ok(transparency)
}
fn remove_white_matte(color: u8, alpha: u8) -> u8 {
    if alpha == 0 {
        return 0;
    }
    let numerator = (i32::from(color) + i32::from(alpha) - 255).max(0) * 255;
    ((numerator + i32::from(alpha) / 2) / i32::from(alpha)).min(255) as u8
}
impl DocumentImporter for PsdImporter {
    fn probe(&self, bytes: &[u8]) -> bool {
        bytes.starts_with(b"8BPS")
    }
    fn import(
        &self,
        bytes: &[u8],
        options: ImportOptions,
    ) -> Result<ImportedDocument, ImportError> {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(ImportError::LimitExceeded("PSD input"));
        }
        let h = read_header(bytes)?;
        if h.width > MAX_DIMENSION || h.height > MAX_DIMENSION {
            return Err(ImportError::LimitExceeded("native dimensions"));
        }
        if h.color_mode != 3 || h.depth != 8 || h.channels < 3 {
            return Err(ImportError::Unsupported("only RGB8 composite images"));
        }
        let mut r = Reader { bytes, at: 26 };
        if !r.section()?.is_empty() {
            return Err(ImportError::Malformed(
                "RGB color-mode section must be empty",
            ));
        }
        let mut report = ConversionReport::default();
        resources(r.section()?, &mut report)?;
        let layer_data = r.section()?;
        let transparency = merged_transparency(layer_data)?;
        if transparency && h.channels < 4 {
            return Err(ImportError::Malformed(
                "missing merged transparency channel",
            ));
        }
        if h.channels > if transparency { 4 } else { 3 } {
            report.issues.push(ConversionIssue {
                code: "psd.extraChannelsNotPreserved",
                tier: CompatibilityTier::D,
            });
        }
        let compression = r.u16()?;
        if compression > 2 {
            return Err(ImportError::Unsupported("ZIP prediction compression"));
        }
        let rows = h.height as usize * usize::from(h.channels);
        if h.width as usize * rows > MAX_INPUT_BYTES {
            return Err(ImportError::LimitExceeded("PSD decoded channels"));
        }
        let counts = if compression == 1 {
            (0..rows)
                .map(|_| r.u16().map(usize::from))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![]
        };
        let remaining = bytes.len() - r.at;
        if compression == 0 {
            if remaining != h.width as usize * rows {
                return Err(ImportError::Malformed("composite payload length"));
            }
        } else if compression == 2 {
            layers::zip_rows(&bytes[r.at..], h.width as usize, rows, |_, _| {})?;
        } else {
            let total = counts
                .iter()
                .try_fold(0usize, |sum, &size| sum.checked_add(size))
                .ok_or(ImportError::Malformed("RLE length overflow"))?;
            if total != remaining {
                return Err(ImportError::Malformed("RLE payload length"));
            }
            // Validate expansion before allocating the native canvas.
            let mut check = Reader { bytes, at: r.at };
            for &size in &counts {
                packbits(check.take(size)?, h.width as usize)?;
            }
        }
        let mut density_baked = false;
        let retained = match layers::decode(layer_data, h, &mut density_baked) {
            Ok(layers) => layers,
            Err(ImportError::Unsupported(_)) => None,
            Err(error) => return Err(error),
        };
        if retained.is_none() && !layer_data.is_empty() {
            report.issues.push(ConversionIssue {
                code: "psd.layersFlattened",
                tier: CompatibilityTier::C,
            });
        }
        if retained.is_some() && density_baked {
            report.issues.push(ConversionIssue {
                code: "psd.maskDensityBaked",
                tier: CompatibilityTier::B,
            });
        }
        if !options.allow_lossy && !report.issues.is_empty() {
            return Err(ImportError::LossyConversionRequiresConsent(report));
        }
        if let Some(layers) = retained {
            return Ok(ImportedDocument {
                raster: TiledRasterState {
                    width: h.width,
                    height: h.height,
                    layers,
                },
                report,
            });
        }
        let columns = h.width.div_ceil(TILE_SIZE);
        let tile_rows = h.height.div_ceil(TILE_SIZE);
        let mut tiles: Vec<_> = (0..tile_rows)
            .flat_map(|y| {
                (0..columns).map(move |x| RasterTileState {
                    coord: TileCoord { x, y },
                    pixels: vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize],
                })
            })
            .collect();
        // Decode one planar scanline into tiles; ZIP also keeps only a scanline buffer.
        let mut apply = |index: usize, row: &[u8]| {
            let channel = index / h.height as usize;
            let y = (index % h.height as usize) as u32;
            if channel > 2 && !(channel == 3 && transparency) {
                return;
            }
            for (x, &value) in row.iter().enumerate() {
                let tile = &mut tiles[(y / TILE_SIZE * columns + x as u32 / TILE_SIZE) as usize];
                let at = ((y % TILE_SIZE) * TILE_SIZE + x as u32 % TILE_SIZE) as usize * 4;
                if channel == 3 {
                    for color in &mut tile.pixels[at..at + 3] {
                        *color = remove_white_matte(*color, value);
                    }
                    tile.pixels[at + 3] = value;
                } else {
                    tile.pixels[at + channel] = value;
                    tile.pixels[at + 3] = 255;
                }
            }
        };
        if compression == 2 {
            layers::zip_rows(&bytes[r.at..], h.width as usize, rows, &mut apply)?;
            r.at = bytes.len();
        } else if compression == 0 {
            for index in 0..rows {
                apply(index, r.take(h.width as usize)?);
            }
        } else {
            for (index, size) in counts.into_iter().enumerate() {
                let row = packbits(r.take(size)?, h.width as usize)?;
                apply(index, &row);
            }
        }
        if r.at != bytes.len() {
            return Err(ImportError::Malformed("trailing composite data"));
        }
        let raster = TiledRasterState {
            width: h.width,
            height: h.height,
            layers: vec![RasterLayerState {
                id: "psd-composite".into(),
                name: "PSD composite".into(),
                visible: true,
                opacity: 1.,
                locked: false,
                alpha_locked: false,
                mask_enabled: false,
                mask_inverted: false,
                mask_density: 1.,
                tiles,
                mask_tiles: vec![],
            }],
        };
        Ok(ImportedDocument { raster, report })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(width: u32, height: u32, rle: bool) -> Vec<u8> {
        let mut bytes = b"8BPS".to_vec();
        bytes.extend(1u16.to_be_bytes());
        bytes.extend([0; 6]);
        bytes.extend(3u16.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes.extend(8u16.to_be_bytes());
        bytes.extend(3u16.to_be_bytes());
        bytes.extend([0; 12]);
        bytes.extend(u16::from(rle).to_be_bytes());
        if rle {
            for _ in 0..height * 3 {
                bytes.extend(2u16.to_be_bytes());
            }
        }
        for channel in 0..3 {
            for _ in 0..height {
                if rle {
                    assert!(width <= 128);
                    bytes.extend([(1 - width as i16) as i8 as u8, 20 + channel]);
                } else {
                    bytes.extend(vec![20 + channel; width as usize]);
                }
            }
        }
        bytes
    }
    fn zip_composite(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut r = Reader { bytes, at: 26 };
        r.section().unwrap();
        r.section().unwrap();
        r.section().unwrap();
        let at = r.at;
        assert_eq!(r.u16().unwrap(), 0);
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&bytes[r.at..]).unwrap();
        let mut zip = bytes[..at].to_vec();
        zip.extend(2u16.to_be_bytes());
        zip.extend(encoder.finish().unwrap());
        zip
    }
    #[test]
    fn zip_composite_matches_raw_edges_transparency_and_rejects_corruption() {
        for bytes in [
            fixture(257, 2, false),
            include_bytes!("../tests/fixtures/lp-psd-alpha.psd").to_vec(),
        ] {
            let zip = zip_composite(&bytes);
            let raw = PsdImporter
                .import(&bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            let result = PsdImporter
                .import(&zip, ImportOptions { allow_lossy: true })
                .unwrap();
            assert_eq!(raw.raster, result.raster);
            assert_eq!(raw.report, result.report);
            lumapaint_core::tiles::TiledRasterDocument::from_state(result.raster).unwrap();
            for end in 0..zip.len() {
                assert!(PsdImporter
                    .import(&zip[..end], ImportOptions { allow_lossy: true })
                    .is_err());
            }
            let mut corrupt = zip.clone();
            *corrupt.last_mut().unwrap() ^= 1;
            assert!(PsdImporter
                .import(&corrupt, ImportOptions { allow_lossy: true })
                .is_err());
            let mut trailing = zip;
            trailing.push(0);
            assert!(PsdImporter
                .import(&trailing, ImportOptions { allow_lossy: true })
                .is_err());
        }
    }
    #[test]
    fn raw_and_packbits_produce_identical_native_tiles_and_save_roundtrip() {
        let raw = PsdImporter
            .import(&fixture(3, 2, false), ImportOptions::default())
            .unwrap();
        let rle = PsdImporter
            .import(&fixture(3, 2, true), ImportOptions::default())
            .unwrap();
        assert_eq!(raw.raster, rle.raster);
        assert!(raw.report.issues.is_empty());
        assert_eq!(
            &raw.raster.layers[0].tiles[0].pixels[..4],
            &[20, 21, 22, 255]
        );
        let native = crate::tile_container::encode(&raw.raster).unwrap();
        assert_eq!(crate::tile_container::decode(&native).unwrap(), raw.raster);
    }
    #[test]
    fn edge_tiles_have_zero_padding() {
        let result = PsdImporter
            .import(&fixture(257, 2, false), ImportOptions::default())
            .unwrap();
        assert_eq!(result.raster.layers[0].tiles.len(), 2);
        let edge = &result.raster.layers[0].tiles[1].pixels;
        assert_eq!(&edge[..4], &[20, 21, 22, 255]);
        assert_eq!(&edge[4..8], &[0; 4]);
        lumapaint_core::tiles::TiledRasterDocument::from_state(result.raster).unwrap();
    }
    #[test]
    fn every_truncation_and_trailing_data_is_rejected() {
        for rle in [false, true] {
            let valid = fixture(3, 2, rle);
            for end in 0..valid.len() {
                assert!(
                    PsdImporter
                        .import(&valid[..end], ImportOptions::default())
                        .is_err(),
                    "{end}"
                );
            }
            let mut extra = valid;
            extra.push(0);
            assert!(PsdImporter
                .import(&extra, ImportOptions::default())
                .is_err());
        }
    }
    #[test]
    fn incompatible_headers_are_not_coerced_into_rgb8() {
        for (offset, value) in [(4, 2u16), (12, 2), (22, 16), (24, 4)] {
            let mut bytes = fixture(3, 2, false);
            bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
            assert!(matches!(
                PsdImporter.import(&bytes, ImportOptions::default()),
                Err(ImportError::Unsupported(_))
            ));
        }
        let mut large = fixture(1, 1, false);
        large[18..22].copy_from_slice(&8193u32.to_be_bytes());
        assert!(matches!(
            PsdImporter.import(&large, ImportOptions::default()),
            Err(ImportError::LimitExceeded(_))
        ));
        let mut reserved = fixture(1, 1, false);
        reserved[6] = 1;
        assert!(read_header(&reserved).is_err());
        let mut channel = fixture(1, 1, false);
        channel[12..14].copy_from_slice(&0u16.to_be_bytes());
        assert!(read_header(&channel).is_err());
    }
    #[test]
    fn lossy_resources_and_layers_require_explicit_opt_in() {
        let mut bytes = fixture(3, 2, false);
        // Resource signature, ID, empty Pascal name, payload length, payload.
        let mut resource = b"8BIM".to_vec();
        resource.extend(1039u16.to_be_bytes());
        resource.extend([0, 0]);
        resource.extend(2u32.to_be_bytes());
        resource.extend([1, 2]);
        bytes[30..34].copy_from_slice(&(resource.len() as u32).to_be_bytes());
        bytes.splice(34..34, resource);
        let at = bytes.len() - 20; // Layer section is the four bytes before compression and 18 pixel bytes.
        bytes[at - 4..at].copy_from_slice(&8u32.to_be_bytes());
        bytes.splice(at..at, [0; 8]);
        let Err(ImportError::LossyConversionRequiresConsent(report)) =
            PsdImporter.import(&bytes, ImportOptions::default())
        else {
            panic!("expected explicit consent");
        };
        assert!(report
            .issues
            .iter()
            .any(|i| i.code == "psd.layersFlattened" && i.tier == CompatibilityTier::C));
        assert!(report
            .issues
            .iter()
            .any(|i| i.code == "psd.iccProfileNotPreserved" && i.tier == CompatibilityTier::D));
        let result = PsdImporter
            .import(&bytes, ImportOptions { allow_lossy: true })
            .unwrap();
        assert_eq!(result.report, report);
    }
    fn alpha_fixture(rle: bool, saved_channel: bool) -> Vec<u8> {
        let mut bytes = include_bytes!("../tests/fixtures/lp-psd-alpha.psd").to_vec();
        if saved_channel {
            bytes[42..44].copy_from_slice(&1i16.to_be_bytes());
        }
        if rle {
            let at = bytes.len() - 18;
            let pixels = bytes[at + 2..].to_vec();
            bytes.truncate(at);
            bytes.extend(1u16.to_be_bytes());
            for _ in 0..4 {
                bytes.extend(5u16.to_be_bytes());
            }
            for row in pixels.as_chunks::<4>().0 {
                bytes.push(3);
                bytes.extend(row);
            }
        }
        bytes
    }
    #[test]
    fn merged_transparency_unmattes_rgb_and_round_trips_native_tiles() {
        let mut previous = None;
        for rle in [false, true] {
            let bytes = alpha_fixture(rle, false);
            assert!(matches!(
                PsdImporter.import(&bytes, ImportOptions::default()),
                Err(ImportError::LossyConversionRequiresConsent(_))
            ));
            let result = PsdImporter
                .import(&bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert_eq!(
                &result.raster.layers[0].tiles[0].pixels[..16],
                &[255, 0, 0, 128, 0, 0, 255, 64, 0, 255, 0, 255, 0, 0, 0, 0]
            );
            if let Some(ref old) = previous {
                assert_eq!(&result.raster, old);
            }
            previous = Some(result.raster.clone());
            let encoded = crate::tile_container::encode(&result.raster).unwrap();
            assert_eq!(
                crate::tile_container::decode(&encoded).unwrap(),
                result.raster
            );
            let doc =
                lumapaint_core::tiles::TiledRasterDocument::from_state(result.raster).unwrap();
            assert_eq!(
                &doc.composite_tile(TileCoord { x: 0, y: 0 }).unwrap()[..16],
                &[128, 0, 0, 128, 0, 0, 64, 64, 0, 255, 0, 255, 0, 0, 0, 0]
            );
        }
    }
    #[test]
    fn saved_alpha_channels_do_not_become_image_transparency() {
        for rle in [false, true] {
            let result = PsdImporter
                .import(
                    &alpha_fixture(rle, true),
                    ImportOptions { allow_lossy: true },
                )
                .unwrap();
            assert_eq!(
                &result.raster.layers[0].tiles[0].pixels[..4],
                &[255, 127, 127, 255]
            );
            assert!(result.report.issues.iter().any(
                |i| i.code == "psd.extraChannelsNotPreserved" && i.tier == CompatibilityTier::D
            ));
        }
    }
    #[test]
    fn additional_saved_channel_keeps_the_first_merged_alpha_channel() {
        for rle in [false, true] {
            let mut bytes = alpha_fixture(rle, false);
            bytes[12..14].copy_from_slice(&5u16.to_be_bytes());
            if rle {
                let layer_size = u32::from_be_bytes(bytes[34..38].try_into().unwrap()) as usize;
                let at = 38 + layer_size + 2 + 8;
                bytes.splice(at..at, 5u16.to_be_bytes());
                bytes.extend([3, 77, 77, 77, 77]);
            } else {
                bytes.extend([77; 4]);
            }
            let result = PsdImporter
                .import(&bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert_eq!(
                &result.raster.layers[0].tiles[0].pixels[..8],
                &[255, 0, 0, 128, 0, 0, 255, 64]
            );
            assert!(result
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.extraChannelsNotPreserved"));
        }
    }
    #[test]
    fn transparency_rejects_truncation_bad_layer_lengths_and_missing_alpha() {
        for rle in [false, true] {
            let bytes = alpha_fixture(rle, false);
            for end in 0..bytes.len() {
                assert!(
                    PsdImporter
                        .import(&bytes[..end], ImportOptions { allow_lossy: true })
                        .is_err(),
                    "{rle} {end}"
                );
            }
            let mut bad = bytes.clone();
            bad[12..14].copy_from_slice(&3u16.to_be_bytes());
            assert!(matches!(
                PsdImporter.import(&bad, ImportOptions { allow_lossy: true }),
                Err(ImportError::Malformed(
                    "missing merged transparency channel"
                ))
            ));
            let mut bad = bytes.clone();
            bad[38..42].copy_from_slice(&u32::MAX.to_be_bytes());
            assert!(PsdImporter
                .import(&bad, ImportOptions { allow_lossy: true })
                .is_err());
            let mut bad = bytes;
            bad.push(0);
            assert!(PsdImporter
                .import(&bad, ImportOptions { allow_lossy: true })
                .is_err());
        }
        let mut budget = alpha_fixture(false, false);
        budget[12..14].copy_from_slice(&56u16.to_be_bytes());
        budget[14..18].copy_from_slice(&8192u32.to_be_bytes());
        budget[18..22].copy_from_slice(&8192u32.to_be_bytes());
        assert!(matches!(
            PsdImporter.import(&budget, ImportOptions { allow_lossy: true }),
            Err(ImportError::LimitExceeded("PSD decoded channels"))
        ));
    }
    #[test]
    fn packbits_enforces_exact_row_expansion() {
        assert_eq!(
            packbits(&[128, 2, 10, 20, 30, 128], 3).unwrap(),
            vec![10, 20, 30]
        );
        assert_eq!(packbits(&[254, 7], 3).unwrap(), vec![7; 3]);
        for input in [&[2, 1, 2][..], &[253, 7], &[0, 7], &[128]] {
            assert!(packbits(input, 3).is_err());
        }
    }
    #[test]
    fn signature_probe_does_not_claim_validation() {
        assert!(PsdImporter.probe(b"8BPS"));
        assert!(read_header(b"8BPS").is_err());
        assert!(!PsdImporter.probe(b"%PDF"));
    }
}
