//! PSD v1 opaque RGB8 merged-image adapter, based on Adobe's published specification.
//! Layer records, alpha channels, profiles, and higher depths are deliberately not decoded yet.
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
        if h.color_mode != 3 || h.depth != 8 || h.channels != 3 {
            return Err(ImportError::Unsupported(
                "only opaque RGB8 with three channels",
            ));
        }
        let mut r = Reader { bytes, at: 26 };
        if !r.section()?.is_empty() {
            return Err(ImportError::Malformed(
                "RGB color-mode section must be empty",
            ));
        }
        let mut report = ConversionReport::default();
        resources(r.section()?, &mut report)?;
        if !r.section()?.is_empty() {
            report.issues.push(ConversionIssue {
                code: "psd.layersFlattened",
                tier: CompatibilityTier::C,
            });
        }
        let compression = r.u16()?;
        if compression > 1 {
            return Err(ImportError::Unsupported("ZIP compression"));
        }
        let rows = h.height as usize * 3;
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
        if !options.allow_lossy && !report.issues.is_empty() {
            return Err(ImportError::LossyConversionRequiresConsent(report));
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
        // Decode one planar scanline at a time into native tiles, avoiding a second full image buffer.
        for channel in 0..3usize {
            for y in 0..h.height {
                let decoded;
                let row = if compression == 0 {
                    r.take(h.width as usize)?
                } else {
                    decoded = packbits(
                        r.take(counts[channel * h.height as usize + y as usize])?,
                        h.width as usize,
                    )?;
                    &decoded
                };
                for (x, &value) in row.iter().enumerate() {
                    let tile =
                        &mut tiles[(y / TILE_SIZE * columns + x as u32 / TILE_SIZE) as usize];
                    let at = ((y % TILE_SIZE) * TILE_SIZE + x as u32 % TILE_SIZE) as usize * 4;
                    tile.pixels[at + channel] = value;
                    tile.pixels[at + 3] = 255;
                }
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
        for (offset, value) in [(4, 2u16), (12, 4), (22, 16), (24, 4)] {
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
        bytes[at - 4..at].copy_from_slice(&4u32.to_be_bytes());
        bytes.splice(at..at, [0; 4]);
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
