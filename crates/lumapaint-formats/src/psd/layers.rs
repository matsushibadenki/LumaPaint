//! Conservative RGB8 raster layer adapter. Unsupported appearance falls back as a whole.
use super::*;
use std::collections::BTreeMap;
struct Record<'a> {
    rect: [u32; 4],
    channels: Vec<(i16, usize)>,
    extra: &'a [u8],
    mask: Option<Mask>,
    opacity: u8,
    flags: u8,
}
#[derive(Clone, Copy)]
struct Mask {
    rect: [u32; 4],
    background: u8,
    flags: u8,
}
fn dimensions(rect: [u32; 4]) -> (usize, usize) {
    ((rect[3] - rect[1]) as usize, (rect[2] - rect[0]) as usize)
}
fn tile_count(rect: [u32; 4]) -> usize {
    let [top, left, bottom, right] = rect;
    if bottom == top || right == left {
        return 0;
    }
    ((right - 1) / TILE_SIZE - left / TILE_SIZE + 1) as usize
        * ((bottom - 1) / TILE_SIZE - top / TILE_SIZE + 1) as usize
}
fn unsupported() -> ImportError {
    ImportError::Unsupported("PSD raster layer features")
}
fn name(extra: &[u8]) -> Result<(String, bool, bool), ImportError> {
    let mut r = Reader {
        bytes: extra,
        at: 0,
    };
    r.section()?; // Mask metadata is validated against canvas bounds in decode.
    let ranges = r.section()?;
    if ranges.len() % 8 != 0
        || !ranges
            .as_chunks::<4>()
            .0
            .iter()
            .all(|r| *r == [0, 0, 255, 255])
    {
        return Err(unsupported());
    }
    let length = r.take(1)?[0] as usize;
    let legacy = r.take(length)?;
    r.take((4 - (length + 1) % 4) % 4)?;
    let mut unicode = None;
    let mut locked = false;
    let mut alpha_locked = false;
    while r.at < extra.len() {
        if r.take(4)? != b"8BIM" {
            return Err(ImportError::Malformed("layer extra signature"));
        }
        let key = r.take(4)?;
        let data = r.section()?;
        if data.len() % 2 != 0 {
            r.take(1)?;
        }
        match key {
            b"luni" => {
                if unicode.is_some() {
                    return Err(ImportError::Malformed("duplicate Unicode layer name"));
                }
                let mut n = Reader { bytes: data, at: 0 };
                let count = n.u32()? as usize;
                if count > 1024 {
                    return Err(ImportError::LimitExceeded("PSD layer name"));
                }
                let units = n
                    .take(count * 2)?
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|v| u16::from_be_bytes(*v))
                    .collect::<Vec<_>>();
                if n.at != data.len() && !(n.at + 2 == data.len() && data[n.at..] == [0, 0]) {
                    return Err(ImportError::Malformed("Unicode name length"));
                }
                unicode = Some(
                    String::from_utf16(&units)
                        .map_err(|_| ImportError::Malformed("Unicode layer name"))?,
                );
            }
            b"lyid" if data.len() == 4 => {}
            b"lspf" if data == [0, 0, 0, 0] || data == [0, 0, 0, 1] || data == [0, 0, 0, 7] => {
                locked = data[3] == 7;
                alpha_locked = data[3] & 1 != 0;
            }
            b"clbl" | b"infx" if data == [0, 0, 0, 0] || data == [1, 0, 0, 0] => {}
            b"knko" if data == [0, 0, 0, 0] => {}
            b"lclr" if data == [0; 8] => {}
            b"iOpa" if data == [255, 0, 0, 0] => {}
            b"lsct" if data == [0, 0, 0, 0] => {}
            _ => return Err(unsupported()),
        }
    }
    let name = if let Some(name) = unicode {
        name
    } else {
        if !legacy.is_ascii() {
            return Err(unsupported());
        }
        String::from_utf8(legacy.to_vec()).map_err(|_| unsupported())?
    };
    if name.trim().is_empty() || name.chars().count() > 255 || name.chars().any(char::is_control) {
        return Err(unsupported());
    }
    Ok((name, locked, alpha_locked))
}
// Materialize a black outside background into native sparse masks; white is implicit.
fn mask(extra: &[u8], h: Header) -> Result<Option<Mask>, ImportError> {
    let mut r = Reader {
        bytes: extra,
        at: 0,
    };
    let data = r.section()?;
    if data.is_empty() {
        return Ok(None);
    }
    if data.len() != 20 {
        return Err(unsupported());
    }
    let mut m = Reader { bytes: data, at: 0 };
    let mut rect = [0u32; 4];
    for value in &mut rect {
        let signed = m.u32()? as i32;
        if signed < 0 {
            return Err(unsupported());
        }
        *value = signed as u32;
    }
    let background = m.take(1)?[0];
    let flags = m.take(1)?[0];
    if rect[0] > rect[2] || rect[1] > rect[3] {
        return Err(ImportError::Malformed("layer mask rectangle"));
    }
    if rect[2] > h.height || rect[3] > h.width || ![0, 255].contains(&background) || flags & !6 != 0
    {
        return Err(unsupported());
    }
    if m.take(2)? != [0, 0] {
        return Err(ImportError::Malformed("layer mask padding"));
    }
    Ok(Some(Mask {
        rect,
        background,
        flags,
    }))
}
fn channel_size(record: &Record<'_>, id: i16) -> (usize, usize) {
    if id == -2 {
        dimensions(record.mask.expect("validated mask channel").rect)
    } else {
        (
            (record.rect[3] - record.rect[1]) as usize,
            (record.rect[2] - record.rect[0]) as usize,
        )
    }
}
// Return validated planar rows without retaining a second full image buffer.
fn rows(
    data: &[u8],
    width: usize,
    height: usize,
    mut visit: impl FnMut(usize, &[u8]),
) -> Result<(), ImportError> {
    let mut r = Reader { bytes: data, at: 0 };
    match r.u16()? {
        0 => {
            if data.len() - 2 != width * height {
                return Err(ImportError::Malformed("layer raw length"));
            }
            for y in 0..height {
                visit(y, r.take(width)?);
            }
        }
        1 => {
            let counts = (0..height)
                .map(|_| r.u16().map(usize::from))
                .collect::<Result<Vec<_>, _>>()?;
            for (y, size) in counts.into_iter().enumerate() {
                let row = packbits(r.take(size)?, width)?;
                visit(y, &row);
            }
            if r.at != data.len() {
                return Err(ImportError::Malformed("layer RLE length"));
            }
        }
        _ => return Err(unsupported()),
    }
    Ok(())
}
pub(super) fn decode(data: &[u8], h: Header) -> Result<Option<Vec<RasterLayerState>>, ImportError> {
    if data.is_empty() {
        return Ok(None);
    }
    let mut outer = Reader { bytes: data, at: 0 };
    let info = outer.section()?;
    if info.is_empty() {
        return Ok(None);
    }
    if !outer.section()?.is_empty() || outer.at != data.len() {
        return Err(unsupported());
    }
    let mut r = Reader { bytes: info, at: 0 };
    let count = (r.u16()? as i16).unsigned_abs() as usize;
    if count == 0 {
        return Ok(None);
    }
    if count > 16 {
        return Err(unsupported());
    }
    let mut records = Vec::new();
    let mut budget = 0usize;
    for _ in 0..count {
        let mut rect = [0u32; 4];
        for n in &mut rect {
            let value = r.u32()? as i32;
            if value < 0 {
                return Err(unsupported());
            }
            *n = value as u32;
        }
        if rect[0] > rect[2] || rect[1] > rect[3] {
            return Err(ImportError::Malformed("layer rectangle"));
        }
        if rect[2] > h.height || rect[3] > h.width {
            return Err(unsupported());
        }
        let n = r.u16()?;
        if n > 56 {
            return Err(ImportError::Malformed("layer channel count"));
        }
        let mut channels = Vec::new();
        for _ in 0..n {
            let id = r.u16()? as i16;
            let length = r.u32()? as usize;
            if ![-2, -1, 0, 1, 2].contains(&id) {
                return Err(unsupported());
            }
            if channels.iter().any(|(existing, _)| *existing == id) {
                return Err(ImportError::Malformed("duplicate layer channel"));
            }
            channels.push((id, length));
        }
        let width = (rect[3] - rect[1]) as usize;
        let height = (rect[2] - rect[0]) as usize;
        if width > 0 && height > 0 && !(0..3).all(|id| channels.iter().any(|(c, _)| *c == id)) {
            return Err(unsupported());
        }
        // Bound both CPU expansion work and tile storage across all layers, before allocation.
        budget = budget
            .checked_add(width * height * channels.iter().filter(|(id, _)| *id != -2).count())
            .ok_or(ImportError::LimitExceeded("PSD layer expansion"))?;
        if budget > MAX_INPUT_BYTES {
            return Err(ImportError::LimitExceeded("PSD layer expansion"));
        }
        if r.take(4)? != b"8BIM" {
            return Err(ImportError::Malformed("layer blend signature"));
        }
        if r.take(4)? != b"norm" {
            return Err(unsupported());
        }
        let opacity = r.take(1)?[0];
        let clipping = r.take(1)?[0];
        let flags = r.take(1)?[0];
        let filler = r.take(1)?[0];
        if clipping != 0 || flags & !15 != 0 {
            return Err(unsupported());
        }
        if filler != 0 {
            return Err(ImportError::Malformed("layer filler"));
        }
        let extra = r.section()?;
        name(extra)?;
        let mask = mask(extra, h)?;
        if mask.is_some() != channels.iter().any(|(id, _)| *id == -2) {
            return Err(ImportError::Malformed("layer mask channel"));
        }
        if let Some(mask) = mask {
            let (width, height) = dimensions(mask.rect);
            budget += width * height;
            if budget > MAX_INPUT_BYTES {
                return Err(ImportError::LimitExceeded("PSD layer expansion"));
            }
        }
        records.push(Record {
            rect,
            channels,
            opacity,
            flags,
            extra,
            mask,
        });
    }
    // Validate every channel before allocating any tiles; partial failures never enter a document.
    let start = r.at;
    for record in &records {
        for &(id, length) in &record.channels {
            let (width, height) = channel_size(record, id);
            rows(r.take(length)?, width, height, |_, _| {})?;
        }
    }
    let mut storage = 0usize;
    for record in &records {
        let [top, left, bottom, right] = record.rect;
        if let Some(mask) = record.mask {
            let rect = if mask.background == 0 {
                [0, 0, h.height, h.width]
            } else {
                mask.rect
            };
            storage += tile_count(rect) * (TILE_SIZE * TILE_SIZE) as usize;
            if storage > MAX_INPUT_BYTES {
                return Err(ImportError::LimitExceeded("PSD layer tiles"));
            }
        }
        if bottom > top && right > left {
            storage += ((right - 1) / TILE_SIZE - left / TILE_SIZE + 1) as usize
                * ((bottom - 1) / TILE_SIZE - top / TILE_SIZE + 1) as usize
                * (TILE_SIZE * TILE_SIZE * 4) as usize;
            if storage > MAX_INPUT_BYTES {
                return Err(ImportError::LimitExceeded("PSD layer tiles"));
            }
        }
    }
    r.at = start;
    let mut layers = Vec::new();
    for (index, record) in records.into_iter().enumerate() {
        let [top, left, _, _] = record.rect;
        let (name, locked, alpha_locked) = name(record.extra)?;
        let alpha = record.channels.iter().any(|(id, _)| *id == -1);
        let mut tiles = BTreeMap::<TileCoord, Vec<u8>>::new();
        let mut masks = BTreeMap::<TileCoord, Vec<u8>>::new();
        if record.mask.is_some_and(|mask| mask.background == 0) {
            for ty in 0..h.height.div_ceil(TILE_SIZE) {
                for tx in 0..h.width.div_ceil(TILE_SIZE) {
                    let mut values = vec![255; (TILE_SIZE * TILE_SIZE) as usize];
                    let width = (h.width - tx * TILE_SIZE).min(TILE_SIZE) as usize;
                    let height = (h.height - ty * TILE_SIZE).min(TILE_SIZE) as usize;
                    for row in values
                        .as_chunks_mut::<{ TILE_SIZE as usize }>()
                        .0
                        .iter_mut()
                        .take(height)
                    {
                        row[..width].fill(0);
                    }
                    masks.insert(TileCoord { x: tx, y: ty }, values);
                }
            }
        }
        for &(id, length) in &record.channels {
            let (width, height) = channel_size(&record, id);
            if id == -2 {
                let mask = record.mask.expect("validated mask channel");
                rows(r.take(length)?, width, height, |y, row| {
                    for (x, &value) in row.iter().enumerate() {
                        let dx = mask.rect[1] + x as u32;
                        let dy = mask.rect[0] + y as u32;
                        let tile = masks
                            .entry(TileCoord {
                                x: dx / TILE_SIZE,
                                y: dy / TILE_SIZE,
                            })
                            .or_insert_with(|| vec![255; (TILE_SIZE * TILE_SIZE) as usize]);
                        tile[((dy % TILE_SIZE) * TILE_SIZE + dx % TILE_SIZE) as usize] = value;
                    }
                })?;
                continue;
            }
            rows(r.take(length)?, width, height, |y, row| {
                for (x, &value) in row.iter().enumerate() {
                    let dx = left + x as u32;
                    let dy = top + y as u32;
                    let tile = tiles
                        .entry(TileCoord {
                            x: dx / TILE_SIZE,
                            y: dy / TILE_SIZE,
                        })
                        .or_insert_with(|| vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize]);
                    let at = ((dy % TILE_SIZE) * TILE_SIZE + dx % TILE_SIZE) as usize * 4;
                    if id == -1 {
                        tile[at + 3] = value;
                    } else {
                        tile[at + id as usize] = value;
                        if !alpha {
                            tile[at + 3] = 255;
                        }
                    }
                }
            })?;
        }
        layers.push(RasterLayerState {
            id: format!("psd-layer-{index}"),
            name,
            visible: record.flags & 2 == 0,
            opacity: f32::from(record.opacity) / 255.,
            locked,
            alpha_locked: alpha_locked || record.flags & 1 != 0,
            mask_enabled: record.mask.is_some_and(|mask| mask.flags & 2 == 0),
            mask_inverted: record.mask.is_some_and(|mask| mask.flags & 4 != 0),
            mask_density: 1.,
            tiles: tiles
                .into_iter()
                .map(|(coord, pixels)| RasterTileState { coord, pixels })
                .collect(),
            mask_tiles: masks
                .into_iter()
                .filter(|(_, values)| values.iter().any(|value| *value != 255))
                .map(|(coord, values)| lumapaint_core::tiles::MaskTileState { coord, values })
                .collect(),
        });
    }
    Ok(Some(layers))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(rle: bool) -> &'static [u8] {
        if rle {
            include_bytes!("../../tests/fixtures/lp-psd-layers-rle.psd")
        } else {
            include_bytes!("../../tests/fixtures/lp-psd-layers.psd")
        }
    }
    fn masked_layer(flags: u8, rle: bool, values: [u8; 2]) -> Vec<u8> {
        region_layer(flags, rle, [0, 0, 1, 2], [0, 0, 1, 2], 255, &values)
    }
    fn region_layer(
        flags: u8,
        rle: bool,
        layer_rect: [u32; 4],
        mask_rect: [u32; 4],
        background: u8,
        values: &[u8],
    ) -> Vec<u8> {
        let mut extra = 20u32.to_be_bytes().to_vec();
        for n in mask_rect {
            extra.extend(n.to_be_bytes());
        }
        extra.extend([background, flags, 0, 0]);
        extra.extend([0; 4]);
        extra.extend([1, b'A', 0, 0]);
        let (lw, lh) = dimensions(layer_rect);
        let mut channels = Vec::new();
        for value in [255, 0, 0] {
            let mut data = vec![0, 0];
            data.extend(vec![value; lw * lh]);
            channels.push(data);
        }
        let (mw, mh) = dimensions(mask_rect);
        assert_eq!(values.len(), mw * mh);
        channels.push(if rle {
            assert!(mw <= 128);
            let mut data = vec![0, 1];
            for _ in 0..mh {
                data.extend(((mw + 1) as u16).to_be_bytes());
            }
            for row in values.chunks(mw.max(1)) {
                data.push((mw - 1) as u8);
                data.extend(row);
            }
            data
        } else {
            let mut data = vec![0, 0];
            data.extend(values);
            data
        });
        let mut info = 1u16.to_be_bytes().to_vec();
        for n in layer_rect {
            info.extend(n.to_be_bytes());
        }
        info.extend(4u16.to_be_bytes());
        for (id, data) in [0i16, 1, 2, -2].into_iter().zip(&channels) {
            info.extend(id.to_be_bytes());
            info.extend((data.len() as u32).to_be_bytes());
        }
        info.extend(b"8BIMnorm");
        info.extend([255, 0, 0, 0]);
        info.extend((extra.len() as u32).to_be_bytes());
        info.extend(extra);
        for data in channels {
            info.extend(data);
        }
        if !info.len().is_multiple_of(2) {
            info.push(0);
        }
        let mut out = (info.len() as u32).to_be_bytes().to_vec();
        out.extend(info);
        out.extend([0; 4]);
        out
    }
    #[test]
    fn partial_masks_keep_background_position_inversion_and_disabled_pixels() {
        let h = Header {
            width: 4,
            height: 2,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        for rle in [false, true] {
            for background in [0, 255] {
                for flags in [0, 2, 4] {
                    let layers = decode(
                        &region_layer(
                            flags,
                            rle,
                            [0, 0, 2, 4],
                            [1, 1, 2, 3],
                            background,
                            &[64, 128],
                        ),
                        h,
                    )
                    .unwrap()
                    .unwrap();
                    let state = TiledRasterState {
                        width: 4,
                        height: 2,
                        layers,
                    };
                    let native = crate::tile_container::encode(&state).unwrap();
                    assert_eq!(crate::tile_container::decode(&native).unwrap(), state);
                    let doc =
                        lumapaint_core::tiles::TiledRasterDocument::from_state(state).unwrap();
                    let tile = doc.composite_tile(TileCoord { x: 0, y: 0 }).unwrap();
                    for y in 0..2usize {
                        for x in 0..4usize {
                            let raw = if y == 1 && x == 1 {
                                64
                            } else if y == 1 && x == 2 {
                                128
                            } else {
                                background
                            };
                            let alpha = if flags == 2 {
                                255
                            } else if flags == 4 {
                                255 - raw
                            } else {
                                raw
                            };
                            let at = (y * TILE_SIZE as usize + x) * 4;
                            assert_eq!(&tile[at..at + 4], &[alpha, 0, 0, alpha]);
                        }
                    }
                }
            }
        }
        // RGB and mask dimensions are independent, including a mask on a different row.
        let layers = decode(
            &region_layer(0, false, [1, 1, 2, 3], [0, 0, 1, 2], 0, &[255, 255]),
            h,
        )
        .unwrap()
        .unwrap();
        let doc = lumapaint_core::tiles::TiledRasterDocument::from_state(TiledRasterState {
            width: 4,
            height: 2,
            layers,
        })
        .unwrap();
        assert!(doc.composite_tile(TileCoord { x: 0, y: 0 }).is_none());
    }
    #[test]
    fn black_background_storage_is_bounded_even_for_an_empty_mask() {
        let single = region_layer(0, false, [0, 0, 1, 1], [0, 0, 0, 0], 0, &[]);
        let mut r = Reader {
            bytes: &single,
            at: 0,
        };
        let info = r.section().unwrap();
        // One record: rectangle, channel table, blend fields, extra length and data.
        let record_end = 2 + 16 + 2 + 4 * 6 + 12 + 4 + 32;
        let mut combined = 16u16.to_be_bytes().to_vec();
        for _ in 0..16 {
            combined.extend(&info[2..record_end]);
        }
        for _ in 0..16 {
            combined.extend(&info[record_end..info.len() - 1]);
        }
        let mut data = (combined.len() as u32).to_be_bytes().to_vec();
        data.extend(combined);
        data.extend([0; 4]);
        let h = Header {
            width: 8192,
            height: 8192,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        assert!(matches!(
            decode(&data, h),
            Err(ImportError::LimitExceeded("PSD layer tiles"))
        ));
    }
    #[test]
    fn partial_mask_crosses_tiles_and_rejects_invalid_bounds() {
        let h = Header {
            width: 258,
            height: 1,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        for background in [0, 255] {
            let layers = decode(
                &region_layer(
                    0,
                    false,
                    [0, 0, 1, 258],
                    [0, 255, 1, 257],
                    background,
                    &[32, 128],
                ),
                h,
            )
            .unwrap()
            .unwrap();
            let state = TiledRasterState {
                width: 258,
                height: 1,
                layers,
            };
            crate::tile_container::decode(&crate::tile_container::encode(&state).unwrap()).unwrap();
            let doc = lumapaint_core::tiles::TiledRasterDocument::from_state(state).unwrap();
            assert_eq!(
                doc.composite_tile(TileCoord { x: 0, y: 0 }).unwrap()[255 * 4 + 3],
                32
            );
            assert_eq!(
                doc.composite_tile(TileCoord { x: 1, y: 0 }).unwrap()[3],
                128
            );
            assert_eq!(
                doc.composite_tile(TileCoord { x: 1, y: 0 }).unwrap()[7],
                background
            );
        }
        let extra = |rect: [u32; 4]| {
            let mut extra = 20u32.to_be_bytes().to_vec();
            for n in rect {
                extra.extend(n.to_be_bytes());
            }
            extra.extend([255, 0, 0, 0]);
            extra
        };
        assert!(matches!(
            mask(&extra([0, 3, 1, 2]), h),
            Err(ImportError::Malformed("layer mask rectangle"))
        ));
        for rect in [[0, 0, 1, 259], [u32::MAX, 0, 1, 1]] {
            assert!(matches!(
                mask(&extra(rect), h),
                Err(ImportError::Unsupported(_))
            ));
        }
    }
    #[test]
    fn canvas_masks_keep_pixels_disable_invert_and_native_roundtrip() {
        let h = Header {
            width: 2,
            height: 1,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        for rle in [false, true] {
            for (flags, expected) in [
                (0, [0, 0, 0, 0, 128, 0, 0, 128]),
                (2, [255, 0, 0, 255, 255, 0, 0, 255]),
                (4, [255, 0, 0, 255, 127, 0, 0, 127]),
            ] {
                let layers = decode(&masked_layer(flags, rle, [0, 128]), h)
                    .unwrap()
                    .unwrap();
                assert_eq!(&layers[0].mask_tiles[0].values[..2], &[0, 128]);
                let mut psd = b"8BPS".to_vec();
                psd.extend(1u16.to_be_bytes());
                psd.extend([0; 6]);
                psd.extend(3u16.to_be_bytes());
                psd.extend(1u32.to_be_bytes());
                psd.extend(2u32.to_be_bytes());
                psd.extend(8u16.to_be_bytes());
                psd.extend(3u16.to_be_bytes());
                psd.extend([0; 8]);
                let layer_data = masked_layer(flags, rle, [0, 128]);
                psd.extend((layer_data.len() as u32).to_be_bytes());
                psd.extend(layer_data);
                psd.extend([0, 0, 255, 255, 0, 0, 0, 0]);
                let imported = PsdImporter.import(&psd, ImportOptions::default()).unwrap();
                assert!(imported.report.issues.is_empty());
                assert_eq!(imported.raster.layers, layers);
                let state = TiledRasterState {
                    width: 2,
                    height: 1,
                    layers,
                };
                let native = crate::tile_container::encode(&state).unwrap();
                assert_eq!(crate::tile_container::decode(&native).unwrap(), state);
                let doc = lumapaint_core::tiles::TiledRasterDocument::from_state(state).unwrap();
                assert_eq!(
                    &doc.composite_tile(TileCoord { x: 0, y: 0 }).unwrap()[..8],
                    &expected
                );
            }
            let layers = decode(&masked_layer(0, rle, [255, 255]), h)
                .unwrap()
                .unwrap();
            assert!(layers[0].mask_tiles.is_empty());
            lumapaint_core::tiles::TiledRasterDocument::from_state(TiledRasterState {
                width: 2,
                height: 1,
                layers,
            })
            .unwrap();
        }
        let mut malformed = masked_layer(0, false, [0, 128]);
        malformed.pop();
        assert!(decode(&malformed, h).is_err());
        for flags in [1, 8, 16] {
            assert!(matches!(
                decode(&masked_layer(flags, false, [0, 128]), h),
                Err(ImportError::Unsupported(_))
            ));
        }
    }
    #[test]
    fn raw_and_rle_layers_keep_order_unicode_position_opacity_and_visibility() {
        let mut previous = None;
        for rle in [false, true] {
            let imported = PsdImporter
                .import(fixture(rle), ImportOptions::default())
                .unwrap();
            assert!(imported.report.issues.is_empty());
            let layers = &imported.raster.layers;
            assert_eq!(layers.len(), 3);
            assert_eq!(layers[0].name, "Base");
            assert_eq!(layers[1].name, "日本語");
            assert_eq!(layers[2].name, "简体中文");
            assert!(layers[0].alpha_locked);
            assert!(layers[1].locked);
            assert!(!layers[2].visible);
            assert_eq!(layers[1].opacity, 128. / 255.);
            assert_eq!(
                &layers[1].tiles[0].pixels[..12],
                &[0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0]
            );
            if let Some(ref state) = previous {
                assert_eq!(&imported.raster, state);
            }
            previous = Some(imported.raster.clone());
            let native = crate::tile_container::encode(&imported.raster).unwrap();
            assert_eq!(
                crate::tile_container::decode(&native).unwrap(),
                imported.raster
            );
            let doc =
                lumapaint_core::tiles::TiledRasterDocument::from_state(imported.raster).unwrap();
            assert_eq!(
                &doc.composite_tile(TileCoord { x: 0, y: 0 }).unwrap()[..16],
                &[255, 0, 0, 255, 127, 0, 128, 255, 255, 0, 0, 255, 255, 0, 0, 255]
            );
        }
    }
    #[test]
    fn unsupported_blend_falls_back_to_the_entire_merged_image_with_consent() {
        let mut bytes = fixture(false).to_vec();
        let index = bytes.windows(8).position(|v| v == b"8BIMnorm").unwrap() + 4;
        bytes[index..index + 4].copy_from_slice(b"mul ");
        assert!(matches!(
            PsdImporter.import(&bytes, ImportOptions::default()),
            Err(ImportError::LossyConversionRequiresConsent(_))
        ));
        let imported = PsdImporter
            .import(&bytes, ImportOptions { allow_lossy: true })
            .unwrap();
        assert_eq!(imported.raster.layers.len(), 1);
        assert_eq!(imported.raster.layers[0].id, "psd-composite");
        assert_eq!(imported.report.issues[0].code, "psd.layersFlattened");
        assert_eq!(
            &imported.raster.layers[0].tiles[0].pixels[..8],
            &[255, 0, 0, 255, 127, 0, 128, 255]
        );
    }
    #[test]
    fn parameterized_masks_clipping_groups_and_outside_bounds_trigger_whole_image_fallback() {
        for mode in 0..4 {
            let mut bytes = fixture(false).to_vec();
            assert_eq!(&bytes[80..88], b"8BIMnorm");
            match mode {
                0 => bytes[89] = 1,
                1 => bytes[44..48].copy_from_slice(&(-1i32).to_be_bytes()),
                2 | 3 => {
                    let extra_len = u32::from_be_bytes(bytes[92..96].try_into().unwrap());
                    let (at, payload) = if mode == 2 {
                        bytes[96..100].copy_from_slice(&20u32.to_be_bytes());
                        (100, {
                            let mut data = vec![0; 20];
                            data[17] = 16; // Parameterized mask remains unsupported.
                            data
                        })
                    } else {
                        let mut tag = b"8BIMlsct".to_vec();
                        tag.extend(4u32.to_be_bytes());
                        tag.extend(1u32.to_be_bytes());
                        (96 + extra_len as usize, tag)
                    };
                    let delta = payload.len() as u32;
                    for offset in [34, 38, 92] {
                        let old = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
                        bytes[offset..offset + 4].copy_from_slice(&(old + delta).to_be_bytes());
                    }
                    bytes.splice(at..at, payload);
                }
                _ => unreachable!(),
            }
            let result = PsdImporter
                .import(&bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert_eq!(result.raster.layers.len(), 1, "mode {mode}");
            assert_eq!(result.report.issues[0].code, "psd.layersFlattened");
        }
    }
    #[test]
    fn neutral_blending_settings_and_supported_protection_are_retained() {
        let mut extra = vec![0; 8];
        extra.extend([1, b'A', 0, 0]);
        for (key, data) in [
            (b"clbl", [1, 0, 0, 0]),
            (b"infx", [0, 0, 0, 0]),
            (b"knko", [0, 0, 0, 0]),
            (b"iOpa", [255, 0, 0, 0]),
            (b"lspf", [0, 0, 0, 1]),
        ] {
            extra.extend(b"8BIM");
            extra.extend(key);
            extra.extend(4u32.to_be_bytes());
            extra.extend(data);
        }
        assert_eq!(name(&extra).unwrap(), ("A".into(), false, true));
        for (key, data) in [
            (b"knko", [1, 0, 0, 0]),
            (b"iOpa", [128, 0, 0, 0]),
            (b"lspf", [0, 0, 0, 4]),
        ] {
            let mut unsupported_extra = extra.clone();
            unsupported_extra.extend(b"8BIM");
            unsupported_extra.extend(key);
            unsupported_extra.extend(4u32.to_be_bytes());
            unsupported_extra.extend(data);
            assert!(matches!(
                name(&unsupported_extra),
                Err(ImportError::Unsupported(_))
            ));
        }
    }
    #[test]
    fn total_layer_expansion_is_bounded_before_channel_allocation() {
        let mut info = 3u16.to_be_bytes().to_vec();
        for _ in 0..3 {
            for n in [0u32, 0, 8192, 8192] {
                info.extend(n.to_be_bytes());
            }
            info.extend(3u16.to_be_bytes());
            for id in 0..3u16 {
                info.extend(id.to_be_bytes());
                info.extend(2u32.to_be_bytes());
            }
            info.extend(b"8BIMnorm");
            info.extend([255, 0, 0, 0]);
            info.extend(12u32.to_be_bytes());
            info.extend([0; 8]);
            info.extend([1, b'A', 0, 0]);
        }
        let mut data = (info.len() as u32).to_be_bytes().to_vec();
        data.extend(info);
        data.extend([0; 4]);
        let h = Header {
            channels: 3,
            width: 8192,
            height: 8192,
            depth: 8,
            color_mode: 3,
        };
        assert!(matches!(
            decode(&data, h),
            Err(ImportError::LimitExceeded("PSD layer expansion"))
        ));
    }
    #[test]
    fn malformed_layer_rows_names_and_channel_ids_are_rejected_atomically() {
        for rle in [false, true] {
            let bytes = fixture(rle);
            for end in 0..bytes.len() {
                assert!(
                    PsdImporter
                        .import(&bytes[..end], ImportOptions { allow_lossy: true })
                        .is_err(),
                    "{rle} {end}"
                );
            }
            let mut duplicate = bytes.to_vec();
            duplicate[68..70].copy_from_slice(&0i16.to_be_bytes());
            assert!(matches!(
                PsdImporter.import(&duplicate, ImportOptions { allow_lossy: true }),
                Err(ImportError::Malformed("duplicate layer channel"))
            ));
            let mut unicode = bytes.to_vec();
            let at = unicode.windows(4).position(|v| v == b"luni").unwrap() + 8;
            unicode[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());
            assert!(matches!(
                PsdImporter.import(&unicode, ImportOptions { allow_lossy: true }),
                Err(ImportError::LimitExceeded("PSD layer name"))
            ));
        }
        assert!(rows(&[0, 0, 1], 2, 1, |_, _| {}).is_err());
        assert!(rows(&[0, 1, 0, 2, 253, 7], 2, 1, |_, _| {}).is_err());
    }
}
