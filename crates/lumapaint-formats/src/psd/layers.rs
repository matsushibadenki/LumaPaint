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
    density: u8,
}
impl Mask {
    fn value(self, value: u8) -> u8 {
        if self.density == 255 {
            return value;
        }
        let value = if self.flags & 4 != 0 {
            255 - value
        } else {
            value
        };
        (255 - ((255 - u32::from(value)) * u32::from(self.density) + 127) / 255) as u8
    }
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
fn tag<'a>(
    r: &mut Reader<'a>,
    large: bool,
    alignment: usize,
) -> Result<(&'a [u8], &'a [u8]), ImportError> {
    let signature = r.take(4)?;
    if signature != b"8BIM" && signature != b"8B64" {
        return Err(ImportError::Malformed("layer extra signature"));
    }
    let key = r.take(4)?;
    let wide = signature == b"8B64"
        || (large
            && matches!(
                key,
                b"LMsk"
                    | b"Lr16"
                    | b"Lr32"
                    | b"Layr"
                    | b"Mt16"
                    | b"Mt32"
                    | b"Mtrn"
                    | b"Alph"
                    | b"FMsk"
                    | b"lnk2"
                    | b"FEid"
                    | b"FXid"
                    | b"PxSD"
            ));
    let data = r.sized_section(wide)?;
    r.take((alignment - data.len() % alignment) % alignment)?;
    Ok((key, data))
}
fn metadata(data: &[u8]) -> Result<(), ImportError> {
    let mut r = Reader { bytes: data, at: 0 };
    let count = r.u32()?;
    if count > 1024 {
        return Err(ImportError::LimitExceeded("PSD metadata items"));
    }
    for _ in 0..count {
        if r.take(4)? != b"8BIM" {
            return Err(ImportError::Malformed("PSD metadata signature"));
        }
        r.take(4)?;
        if r.take(1)?[0] > 1 || r.take(3)? != [0; 3] {
            return Err(ImportError::Malformed("PSD metadata flags"));
        }
        r.section()?;
    }
    if r.at != data.len() {
        return Err(ImportError::Malformed("PSD metadata length"));
    }
    Ok(())
}
#[cfg(test)]
fn name(extra: &[u8], large: bool) -> Result<(String, bool, bool), ImportError> {
    name_with_metadata(extra, large, &mut false)
}
fn name_with_metadata(
    extra: &[u8],
    large: bool,
    discarded: &mut bool,
) -> Result<(String, bool, bool), ImportError> {
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
        let (key, data) = tag(&mut r, large, 2)?;
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
            b"shmd" => {
                metadata(data)?;
                *discarded = true;
            }
            b"fxrp" if data.len() == 16 => {
                for value in data.as_chunks::<8>().0 {
                    if !f64::from_be_bytes(*value).is_finite() {
                        return Err(ImportError::Malformed("PSD reference point"));
                    }
                }
                *discarded = true;
            }
            // Partial position/composite locks are conservatively projected to a full lock.
            b"lspf" if data.len() == 4 && data[..3] == [0; 3] && data[3] <= 15 => {
                locked = data[3] & 14 != 0;
                alpha_locked = data[3] & 1 != 0;
                *discarded |= ![0, 1, 7].contains(&data[3]);
            }
            b"lyid" if data.len() == 4 => {}
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
fn mask(extra: &[u8], h: Header, layer_rect: [u32; 4]) -> Result<Option<Mask>, ImportError> {
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
    let mut signed = [0i64; 4];
    for value in &mut signed {
        *value = i64::from(m.u32()? as i32);
    }
    let background = m.take(1)?[0];
    let flags = m.take(1)?[0];
    if signed[0] > signed[2] || signed[1] > signed[3] {
        return Err(ImportError::Malformed("layer mask rectangle"));
    }
    if ![0, 255].contains(&background) || flags & !23 != 0 {
        return Err(unsupported());
    }
    if flags & 1 != 0 {
        for (index, value) in signed.iter_mut().enumerate() {
            *value += i64::from(layer_rect[if index % 2 == 0 { 0 } else { 1 }]);
        }
    }
    if signed.iter().any(|value| *value < 0)
        || signed[2] > i64::from(h.height)
        || signed[3] > i64::from(h.width)
    {
        return Err(unsupported());
    }
    let rect = signed.map(|value| value as u32);
    let density = if flags & 16 != 0 {
        let parameters = m.take(1)?[0];
        match parameters {
            1 => m.take(1)?[0],
            0 => {
                if m.take(1)? != [0] {
                    return Err(ImportError::Malformed("layer mask padding"));
                }
                255
            }
            _ => return Err(unsupported()),
        }
    } else {
        if m.take(2)? != [0, 0] {
            return Err(ImportError::Malformed("layer mask padding"));
        }
        255
    };
    Ok(Some(Mask {
        rect,
        background,
        flags,
        density,
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
// A single bounded scanline buffer; require the zlib end marker and exact input/output.
pub(super) fn zip_rows(
    data: &[u8],
    width: usize,
    height: usize,
    visit: impl FnMut(usize, &[u8]),
) -> Result<(), ImportError> {
    zip_rows_with_prediction(data, width, height, false, visit)
}
pub(super) fn zip_rows_with_prediction(
    data: &[u8],
    width: usize,
    height: usize,
    prediction: bool,
    mut visit: impl FnMut(usize, &[u8]),
) -> Result<(), ImportError> {
    use flate2::{Decompress, FlushDecompress, Status};
    if width > MAX_DIMENSION as usize
        || (width == 0 && height > MAX_DIMENSION as usize)
        || width
            .checked_mul(height)
            .is_none_or(|size| size > MAX_INPUT_BYTES)
    {
        return Err(ImportError::LimitExceeded("PSD ZIP expansion"));
    }
    let mut decoder = Decompress::new(true);
    let mut ended = false;
    let mut row = vec![0; width];
    let step = |decoder: &mut Decompress, output: &mut [u8]| -> Result<Status, ImportError> {
        let before = (decoder.total_in(), decoder.total_out());
        let status = decoder
            .decompress(&data[before.0 as usize..], output, FlushDecompress::None)
            .map_err(|_| ImportError::Malformed("layer ZIP data"))?;
        if status != Status::StreamEnd && before == (decoder.total_in(), decoder.total_out()) {
            return Err(ImportError::Malformed("truncated layer ZIP"));
        }
        Ok(status)
    };
    for y in 0..height {
        let mut written = 0;
        while written < width {
            if ended {
                return Err(ImportError::Malformed("layer ZIP length"));
            }
            let before = decoder.total_out();
            ended = step(&mut decoder, &mut row[written..])? == Status::StreamEnd;
            written += (decoder.total_out() - before) as usize;
        }
        if prediction {
            let mut previous = 0u8;
            for value in &mut row {
                previous = previous.wrapping_add(*value);
                *value = previous;
            }
        }
        visit(y, &row);
    }
    while !ended {
        let mut extra = [0];
        let before = decoder.total_out();
        ended = step(&mut decoder, &mut extra)? == Status::StreamEnd;
        if decoder.total_out() != before {
            return Err(ImportError::Malformed("layer ZIP length"));
        }
    }
    if decoder.total_in() as usize != data.len() {
        return Err(ImportError::Malformed("trailing layer ZIP data"));
    }
    Ok(())
}
// Return validated planar rows without retaining a second full image buffer.
fn rows(
    data: &[u8],
    width: usize,
    height: usize,
    large: bool,
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
                .map(|_| r.row_length(large))
                .collect::<Result<Vec<_>, _>>()?;
            for (y, size) in counts.into_iter().enumerate() {
                let row = packbits(r.take(size)?, width)?;
                visit(y, &row);
            }
            if r.at != data.len() {
                return Err(ImportError::Malformed("layer RLE length"));
            }
        }
        2 => zip_rows(&data[2..], width, height, visit)?,
        3 => zip_rows_with_prediction(&data[2..], width, height, true, visit)?,
        _ => return Err(unsupported()),
    }
    Ok(())
}
pub(super) fn decode_with_metadata(
    data: &[u8],
    h: Header,
    density_baked: &mut bool,
    discarded: &mut bool,
) -> Result<Option<Vec<RasterLayerState>>, ImportError> {
    *density_baked = false;
    if data.is_empty() {
        return Ok(None);
    }
    let mut outer = Reader { bytes: data, at: 0 };
    let info = outer.sized_section(h.version == 2)?;
    if info.is_empty() {
        return Ok(None);
    }
    if !outer.section()?.is_empty() {
        return Err(unsupported());
    }
    while outer.at < data.len() {
        let (key, bytes) = tag(&mut outer, h.version == 2, 4)?;
        match key {
            b"Patt" if bytes.is_empty() => {}
            b"CAI " | b"GenI" | b"OCIO" | b"cinf" if (4..=65536).contains(&bytes.len()) => {
                *discarded = true;
            }
            b"FMsk" if bytes.len() == 12 => {
                *discarded = true;
            }
            _ => return Err(unsupported()),
        }
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
            let length = r.length(h.version == 2)?;
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
        name_with_metadata(extra, h.version == 2, discarded)?;
        let mask = mask(extra, h, rect)?;
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
            rows(r.take(length)?, width, height, h.version == 2, |_, _| {})?;
        }
    }
    let mut storage = 0usize;
    for record in &records {
        let [top, left, bottom, right] = record.rect;
        if let Some(mask) = record.mask {
            let rect = if mask.value(mask.background) != 255 {
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
    let records_density_baked = records
        .iter()
        .any(|record| record.mask.is_some_and(|mask| mask.density != 255));
    r.at = start;
    let mut layers = Vec::new();
    for (index, record) in records.into_iter().enumerate() {
        let [top, left, _, _] = record.rect;
        let (name, locked, alpha_locked) =
            name_with_metadata(record.extra, h.version == 2, discarded)?;
        let alpha = record.channels.iter().any(|(id, _)| *id == -1);
        let mut tiles = BTreeMap::<TileCoord, Vec<u8>>::new();
        let mut masks = BTreeMap::<TileCoord, Vec<u8>>::new();
        if let Some(mask) = record
            .mask
            .filter(|mask| mask.value(mask.background) != 255)
        {
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
                        row[..width].fill(mask.value(mask.background));
                    }
                    masks.insert(TileCoord { x: tx, y: ty }, values);
                }
            }
        }
        for &(id, length) in &record.channels {
            let (width, height) = channel_size(&record, id);
            if id == -2 {
                let mask = record.mask.expect("validated mask channel");
                rows(r.take(length)?, width, height, h.version == 2, |y, row| {
                    for (x, &value) in row.iter().enumerate() {
                        let dx = mask.rect[1] + x as u32;
                        let dy = mask.rect[0] + y as u32;
                        let tile = masks
                            .entry(TileCoord {
                                x: dx / TILE_SIZE,
                                y: dy / TILE_SIZE,
                            })
                            .or_insert_with(|| vec![255; (TILE_SIZE * TILE_SIZE) as usize]);
                        tile[((dy % TILE_SIZE) * TILE_SIZE + dx % TILE_SIZE) as usize] =
                            mask.value(value);
                    }
                })?;
                continue;
            }
            rows(r.take(length)?, width, height, h.version == 2, |y, row| {
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
            mask_inverted: record
                .mask
                .is_some_and(|mask| mask.flags & 4 != 0 && mask.density == 255),
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
    *density_baked = records_density_baked;
    Ok(Some(layers))
}

#[cfg(test)]
fn decode(
    data: &[u8],
    h: Header,
    density_baked: &mut bool,
) -> Result<Option<Vec<RasterLayerState>>, ImportError> {
    decode_with_metadata(data, h, density_baked, &mut false)
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
    fn zipped(data: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }
    fn prediction_bytes(data: &[u8], width: usize) -> Vec<u8> {
        let mut encoded = Vec::new();
        for row in data.chunks(width.max(1)) {
            let mut previous = 0u8;
            for &value in row {
                encoded.push(value.wrapping_sub(previous));
                previous = value;
            }
        }
        encoded
    }
    #[test]
    fn prediction_wraps_unsigned_bytes_and_resets_for_each_row() {
        // Independent golden differences: wraparound, decreasing samples and row reset.
        let encoded = zipped(&[250, 11, 255, 7, 249, 255]);
        let mut result = Vec::new();
        zip_rows_with_prediction(&encoded, 3, 2, true, |_, row| result.extend_from_slice(row))
            .unwrap();
        assert_eq!(result, [250, 5, 4, 7, 0, 255]);
        for end in 0..encoded.len() {
            assert!(zip_rows_with_prediction(&encoded[..end], 3, 2, true, |_, _| {}).is_err());
        }
        let mut corrupt = encoded.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(zip_rows_with_prediction(&corrupt, 3, 2, true, |_, _| {}).is_err());
        let mut trailing = encoded;
        trailing.push(0);
        assert!(zip_rows_with_prediction(&trailing, 3, 2, true, |_, _| {}).is_err());
        let mut one = Vec::new();
        zip_rows_with_prediction(&zipped(&[100, 200]), 1, 2, true, |_, row| {
            one.extend_from_slice(row)
        })
        .unwrap();
        assert_eq!(one, [100, 200]);
        zip_rows_with_prediction(&zipped(&[]), 0, 0, true, |_, _| {}).unwrap();
    }
    #[test]
    fn zip_streams_require_exact_size_checksum_end_marker_and_no_trailing_bytes() {
        let encoded = zipped(&[1, 2, 3, 4, 5, 6]);
        let mut decoded = Vec::<u8>::new();
        zip_rows(&encoded, 3, 2, |_, row| decoded.extend_from_slice(row)).unwrap();
        assert_eq!(decoded, [1, 2, 3, 4, 5, 6]);
        for end in 0..encoded.len() {
            assert!(zip_rows(&encoded[..end], 3, 2, |_, _| {}).is_err());
        }
        for (width, height) in [(2, 2), (4, 2), (3, 3)] {
            assert!(zip_rows(&encoded, width, height, |_, _| {}).is_err());
        }
        let mut corrupt = encoded.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(zip_rows(&corrupt, 3, 2, |_, _| {}).is_err());
        let mut trailing = encoded.clone();
        trailing.extend(zipped(&[]));
        assert!(zip_rows(&trailing, 3, 2, |_, _| {}).is_err());
        assert!(matches!(
            zip_rows(&encoded, 8193, 1, |_, _| {}),
            Err(ImportError::LimitExceeded(_))
        ));
        zip_rows(&zipped(&[]), 0, 0, |_, _| {}).unwrap();
    }
    #[test]
    fn zip_layers_and_composite_match_raw_and_preserve_native_state() {
        let raw = PsdImporter
            .import(fixture(false), ImportOptions::default())
            .unwrap();
        for bytes in [
            include_bytes!("../../tests/fixtures/lp-psd-layers-zip.psd").as_slice(),
            include_bytes!("../../tests/fixtures/lp-psd-layers-prediction.psd").as_slice(),
        ] {
            let zip = PsdImporter.import(bytes, ImportOptions::default()).unwrap();
            assert_eq!(raw.raster, zip.raster);
            assert_eq!(raw.report, zip.report);
            assert_eq!(
                crate::tile_container::decode(&crate::tile_container::encode(&zip.raster).unwrap())
                    .unwrap(),
                zip.raster
            );
        }
    }
    #[test]
    fn zip_masks_with_density_match_raw_before_any_document_is_created() {
        let h = Header {
            version: 1,
            width: 4,
            height: 2,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        let mut raw = region_layer(20, false, [0, 0, 2, 4], [1, 1, 2, 3], 255, &[32, 128]);
        raw[86..88].copy_from_slice(&[1, 128]);
        for prediction in [false, true] {
            let mut outer = Reader { bytes: &raw, at: 0 };
            let info = outer.section().unwrap();
            let mut r = Reader {
                bytes: info,
                at: 18,
            };
            let n = r.u16().unwrap();
            let mut channels = Vec::new();
            for _ in 0..n {
                let id = r.u16().unwrap() as i16;
                let at = r.at;
                let size = r.u32().unwrap() as usize;
                channels.push((id, at, size));
            }
            r.take(12).unwrap();
            r.section().unwrap();
            let mut encoded = info[..r.at].to_vec();
            for (id, at, size) in channels {
                let channel = r.take(size).unwrap();
                assert_eq!(&channel[..2], &[0, 0]);
                let mut zip = vec![0, if prediction { 3 } else { 2 }];
                let samples = if prediction {
                    prediction_bytes(&channel[2..], if id == -2 { 2 } else { 4 })
                } else {
                    channel[2..].to_vec()
                };
                zip.extend(zipped(&samples));
                encoded[at..at + 4].copy_from_slice(&(zip.len() as u32).to_be_bytes());
                encoded.extend(zip);
            }
            if !encoded.len().is_multiple_of(2) {
                encoded.push(0);
            }
            let mut zip = (encoded.len() as u32).to_be_bytes().to_vec();
            zip.extend(encoded);
            zip.extend([0; 4]);
            assert_eq!(
                decode(&raw, h, &mut false).unwrap(),
                decode(&zip, h, &mut false).unwrap()
            );
        }
    }
    #[test]
    fn relative_masks_resolve_signed_offsets_without_moving_layer_pixels() {
        let h = Header {
            version: 1,
            width: 4,
            height: 2,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        for rle in [false, true] {
            let bytes = region_layer(1, rle, [1, 1, 2, 3], [0, 0, 1, 2], 255, &[0, 128]);
            let relative = decode(&bytes, h, &mut false).unwrap().unwrap();
            let absolute = decode(
                &region_layer(0, rle, [1, 1, 2, 3], [1, 1, 2, 3], 255, &[0, 128]),
                h,
                &mut false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(relative, absolute);
            let mut negative = bytes.clone();
            for (index, value) in [-1i32, -1, 0, 1].into_iter().enumerate() {
                negative[68 + index * 4..72 + index * 4].copy_from_slice(&value.to_be_bytes());
            }
            let resolved = decode(&negative, h, &mut false).unwrap().unwrap();
            let expected = decode(
                &region_layer(0, rle, [1, 1, 2, 3], [0, 0, 1, 2], 255, &[0, 128]),
                h,
                &mut false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(resolved, expected);
        }
    }
    #[test]
    fn density_preserves_photoshop_coverage_and_requires_conversion_consent() {
        let h = Header {
            version: 1,
            width: 2,
            height: 1,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        for rle in [false, true] {
            for (density, normal, inverted) in [
                (0, [255, 255], [255, 255]),
                (128, [127, 191], [255, 191]),
                (255, [0, 128], [255, 127]),
            ] {
                for (flags, expected) in [(16, normal), (20, inverted), (18, [255, 255])] {
                    let mut data = masked_layer(flags, rle, [0, 128]);
                    data[86..88].copy_from_slice(&[1, density]);
                    let mut baked = false;
                    let layers = decode(&data, h, &mut baked).unwrap().unwrap();
                    assert_eq!(baked, density != 255);
                    let state = TiledRasterState {
                        width: 2,
                        height: 1,
                        layers,
                    };
                    assert_eq!(
                        crate::tile_container::decode(
                            &crate::tile_container::encode(&state).unwrap()
                        )
                        .unwrap(),
                        state
                    );
                    let doc =
                        lumapaint_core::tiles::TiledRasterDocument::from_state(state).unwrap();
                    let tile = doc.composite_tile(TileCoord { x: 0, y: 0 }).unwrap();
                    assert_eq!([tile[3], tile[7]], expected);
                    let mut psd = b"8BPS".to_vec();
                    psd.extend(1u16.to_be_bytes());
                    psd.extend([0; 6]);
                    psd.extend(3u16.to_be_bytes());
                    psd.extend(1u32.to_be_bytes());
                    psd.extend(2u32.to_be_bytes());
                    psd.extend(8u16.to_be_bytes());
                    psd.extend(3u16.to_be_bytes());
                    psd.extend([0; 8]);
                    psd.extend((data.len() as u32).to_be_bytes());
                    psd.extend(data);
                    psd.extend([0, 0, 255, 255, 0, 0, 0, 0]);
                    if baked {
                        assert!(matches!(
                            PsdImporter.import(&psd, ImportOptions::default()),
                            Err(ImportError::LossyConversionRequiresConsent(_))
                        ));
                    }
                    let imported = PsdImporter
                        .import(&psd, ImportOptions { allow_lossy: true })
                        .unwrap();
                    assert_eq!(
                        imported
                            .report
                            .issues
                            .iter()
                            .any(|i| i.code == "psd.maskDensityBaked"),
                        baked
                    );
                    assert_eq!(imported.raster.layers.len(), 1);
                    assert_eq!(imported.raster.layers[0].id, "psd-layer-0");
                }
            }
        }
    }
    #[test]
    fn partial_density_includes_the_outside_background_and_rejects_other_parameters() {
        let h = Header {
            version: 1,
            width: 4,
            height: 2,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        for background in [0, 255] {
            for flags in [16, 20] {
                let mut data = region_layer(
                    flags,
                    false,
                    [0, 0, 2, 4],
                    [1, 1, 2, 3],
                    background,
                    &[0, 128],
                );
                data[86..88].copy_from_slice(&[1, 128]);
                let layers = decode(&data, h, &mut false).unwrap().unwrap();
                let doc =
                    lumapaint_core::tiles::TiledRasterDocument::from_state(TiledRasterState {
                        width: 4,
                        height: 2,
                        layers,
                    })
                    .unwrap();
                let tile = doc.composite_tile(TileCoord { x: 0, y: 0 }).unwrap();
                let outside = if (background == 255) != (flags == 20) {
                    255
                } else {
                    127
                };
                assert_eq!(tile[3], outside);
                assert_eq!(
                    tile[(TILE_SIZE as usize + 1) * 4 + 3],
                    if flags == 20 { 255 } else { 127 }
                );
                assert_eq!(tile[(TILE_SIZE as usize + 2) * 4 + 3], 191);
                for parameter in [2, 4, 8, 128] {
                    data[86] = parameter;
                    assert!(matches!(
                        decode(&data, h, &mut false),
                        Err(ImportError::Unsupported(_))
                    ));
                }
            }
        }
    }
    #[test]
    fn partial_masks_keep_background_position_inversion_and_disabled_pixels() {
        let h = Header {
            version: 1,
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
                        &mut false,
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
            &mut false,
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
            version: 1,
            width: 8192,
            height: 8192,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        assert!(matches!(
            decode(&data, h, &mut false),
            Err(ImportError::LimitExceeded("PSD layer tiles"))
        ));
    }
    #[test]
    fn partial_mask_crosses_tiles_and_rejects_invalid_bounds() {
        let h = Header {
            version: 1,
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
                &mut false,
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
            mask(&extra([0, 3, 1, 2]), h, [0; 4]),
            Err(ImportError::Malformed("layer mask rectangle"))
        ));
        for rect in [[0, 0, 1, 259], [u32::MAX, 0, 1, 1]] {
            assert!(matches!(
                mask(&extra(rect), h, [0; 4]),
                Err(ImportError::Unsupported(_))
            ));
        }
    }
    #[test]
    fn canvas_masks_keep_pixels_disable_invert_and_native_roundtrip() {
        let h = Header {
            version: 1,
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
                let layers = decode(&masked_layer(flags, rle, [0, 128]), h, &mut false)
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
            let layers = decode(&masked_layer(0, rle, [255, 255]), h, &mut false)
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
        assert!(decode(&malformed, h, &mut false).is_err());
        for flags in [8, 32] {
            assert!(matches!(
                decode(&masked_layer(flags, false, [0, 128]), h, &mut false),
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
                            data[17] = 32; // Unknown mask flag remains unsupported.
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
        assert_eq!(name(&extra, false).unwrap(), ("A".into(), false, true));
        for (key, data) in [
            (b"knko", [1, 0, 0, 0]),
            (b"iOpa", [128, 0, 0, 0]),
            (b"lspf", [0, 0, 0, 16]),
        ] {
            let mut unsupported_extra = extra.clone();
            unsupported_extra.extend(b"8BIM");
            unsupported_extra.extend(key);
            unsupported_extra.extend(4u32.to_be_bytes());
            unsupported_extra.extend(data);
            assert!(matches!(
                name(&unsupported_extra, false),
                Err(ImportError::Unsupported(_))
            ));
        }
    }
    #[test]
    fn partial_protection_and_reference_points_report_projection_without_unlocking() {
        for flags in [2u8, 4, 5, 8, 13] {
            let mut extra = vec![0; 8];
            extra.extend([1, b'A', 0, 0]);
            extra.extend(b"8BIMlspf");
            extra.extend(4u32.to_be_bytes());
            extra.extend([0, 0, 0, flags]);
            extra.extend(b"8BIMfxrp");
            extra.extend(16u32.to_be_bytes());
            extra.extend(4.5f64.to_be_bytes());
            extra.extend((-2.0f64).to_be_bytes());
            let mut discarded = false;
            let (_, locked, alpha_locked) =
                name_with_metadata(&extra, false, &mut discarded).unwrap();
            assert!(locked && discarded);
            assert_eq!(alpha_locked, flags & 1 != 0);
            let size = extra.len();
            extra[size - 8..].copy_from_slice(&f64::NAN.to_be_bytes());
            assert!(name_with_metadata(&extra, false, &mut false).is_err());
        }
    }
    #[test]
    fn metadata_container_rejects_truncation_invalid_flags_and_excessive_counts() {
        let mut data = 1u32.to_be_bytes().to_vec();
        data.extend(b"8BIMcust");
        data.extend([1, 0, 0, 0]);
        data.extend(4u32.to_be_bytes());
        data.extend([0; 4]);
        metadata(&data).unwrap();
        for end in 0..data.len() {
            assert!(metadata(&data[..end]).is_err());
        }
        let mut bad = data.clone();
        bad[12] = 2;
        assert!(metadata(&bad).is_err());
        bad = data;
        bad[..4].copy_from_slice(&1025u32.to_be_bytes());
        assert!(matches!(metadata(&bad), Err(ImportError::LimitExceeded(_))));
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
            version: 1,
            width: 8192,
            height: 8192,
            depth: 8,
            color_mode: 3,
        };
        assert!(matches!(
            decode(&data, h, &mut false),
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
        assert!(rows(&[0, 0, 1], 2, 1, false, |_, _| {}).is_err());
        assert!(rows(&[0, 1, 0, 2, 253, 7], 2, 1, false, |_, _| {}).is_err());
    }
}
