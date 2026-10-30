//! Conservative RGB8 raster layer adapter. Unsupported appearance falls back as a whole.
use super::*;
use std::collections::BTreeMap;
struct Record<'a> {
    rect: [i64; 4],
    channels: Vec<(i16, usize)>,
    extra: &'a [u8],
    mask: Option<Mask>,
    blend_mode: RasterBlendMode,
    opacity: u8,
    flags: u8,
}
#[derive(Clone, Copy)]
struct Mask {
    rect: [i64; 4],
    background: u8,
    flags: u8,
    density: u8,
    ambiguous_origin: bool,
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
#[cfg(test)]
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
// Inspect only bounded layer-record metadata after the full decoder has already
// chosen a flattened fallback. Unknown layer features must not hide a later
// text block, but malformed structures are never guessed from raw byte patterns.
pub(super) fn contains_text_metadata(data: &[u8], large: bool) -> Result<bool, ImportError> {
    if data.is_empty() {
        return Ok(false);
    }
    let mut outer = Reader { bytes: data, at: 0 };
    let info = outer.sized_section(large)?;
    if info.is_empty() {
        return Ok(false);
    }
    let mut records = Reader { bytes: info, at: 0 };
    let count = (records.u16()? as i16).unsigned_abs() as usize;
    if count > 4096 {
        return Err(ImportError::LimitExceeded("PSD layer metadata scan"));
    }
    for _ in 0..count {
        records.take(16)?;
        let channels = records.u16()? as usize;
        if channels > 1024 {
            return Err(ImportError::LimitExceeded("PSD layer metadata channels"));
        }
        for _ in 0..channels {
            records.take(2)?;
            records.length(large)?;
        }
        records.take(12)?; // Blend signature, key, opacity, clipping, flags and filler.
        let extra = records.section()?;
        let mut metadata = Reader {
            bytes: extra,
            at: 0,
        };
        metadata.section()?;
        metadata.section()?;
        let name_len = metadata.take(1)?[0] as usize;
        metadata.take(name_len + (4 - (name_len + 1) % 4) % 4)?;
        while metadata.at < extra.len() {
            let (key, _) = tag(&mut metadata, large, 2)?;
            if key == b"TySh" {
                return Ok(true);
            }
        }
    }
    Ok(false)
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
            b"TySh" => return Err(ImportError::Unsupported("PSD editable text layer")),
            b"lmgm" if data == [0, 0, 0, 0] => {
                *discarded = true;
            }
            b"lyvr" if data == [0, 0, 0, 70] => {
                *discarded = true;
            }
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
fn mask(
    extra: &[u8],
    _h: Header,
    layer_rect: [i64; 4],
    prefer_relative: bool,
) -> Result<Option<Mask>, ImportError> {
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
    // Both interpretations can fit in the document; neither the flag nor
    // the rectangle alone distinguishes old relative files from Photoshop's
    // current document-space encoding.
    let relative = signed.map(|v| v);
    let translated = [
        relative[0] + layer_rect[0],
        relative[1] + layer_rect[1],
        relative[2] + layer_rect[0],
        relative[3] + layer_rect[1],
    ];
    // Compare plausible origins against the composite instead of guessing.
    let ambiguous_origin = flags & 1 != 0
        && layer_rect[..2] != [0, 0]
        && relative != translated
        && relative[0] < relative[2]
        && relative[1] < relative[3];
    if ambiguous_origin && prefer_relative {
        signed = translated;
    }
    let rect = signed;
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
        ambiguous_origin,
    }))
}
fn channel_size(record: &Record<'_>, id: i16) -> (usize, usize) {
    if id == -2 {
        let rect = record.mask.expect("validated mask channel").rect;
        ((rect[3] - rect[1]) as usize, (rect[2] - rect[0]) as usize)
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
#[cfg(test)]
pub(super) fn decode_with_metadata(
    data: &[u8],
    h: Header,
    density_baked: &mut bool,
    discarded: &mut bool,
    clipped: &mut bool,
    ambiguous: &mut bool,
) -> Result<Option<Vec<RasterLayerState>>, ImportError> {
    decode_with_metadata_mode(
        data,
        h,
        density_baked,
        discarded,
        clipped,
        ambiguous,
        (0, &mut 0),
    )
}

#[cfg(test)]
pub(super) fn decode_with_metadata_mode(
    data: &[u8],
    h: Header,
    density_baked: &mut bool,
    discarded: &mut bool,
    clipped: &mut bool,
    ambiguous: &mut bool,
    mask_choice: (u16, &mut u16),
) -> Result<Option<Vec<RasterLayerState>>, ImportError> {
    decode_with_metadata_mode_and_clipped(
        data,
        h,
        density_baked,
        discarded,
        clipped,
        ambiguous,
        mask_choice,
        &mut false,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn decode_with_metadata_mode_and_clipped(
    data: &[u8],
    h: Header,
    density_baked: &mut bool,
    discarded: &mut bool,
    clipped: &mut bool,
    ambiguous: &mut bool,
    mask_choice: (u16, &mut u16),
    mask_clipped: &mut bool,
) -> Result<Option<Vec<RasterLayerState>>, ImportError> {
    let (relative_mask_bits, ambiguous_mask_bits) = mask_choice;
    *density_baked = false;
    if data.is_empty() {
        return Ok(None);
    }
    let mut outer = Reader { bytes: data, at: 0 };
    let info = outer.sized_section(h.version == 2)?;
    if info.is_empty() {
        return Ok(None);
    }
    let global_mask = outer.section()?;
    if !global_mask.is_empty() {
        // Photoshop emits this default per-layer mask display setting for
        // raster masks. It is editor metadata, not the layer mask pixels.
        // Accept only the observed default and require merged-preview checks.
        if global_mask != [0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 50, 128, 0, 0, 0] {
            return Err(unsupported());
        }
        *discarded = true;
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
    for layer_index in 0..count {
        let mut rect = [0i64; 4];
        for n in &mut rect {
            *n = i64::from(r.u32()? as i32);
        }
        if rect[0] > rect[2] || rect[1] > rect[3] {
            return Err(ImportError::Malformed("layer rectangle"));
        }
        if rect[0] < 0
            || rect[1] < 0
            || rect[2] > i64::from(h.height)
            || rect[3] > i64::from(h.width)
        {
            *clipped = true;
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
        let expanded = width
            .checked_mul(height)
            .and_then(|area| area.checked_mul(channels.iter().filter(|(id, _)| *id != -2).count()))
            .ok_or(ImportError::LimitExceeded("PSD layer expansion"))?;
        budget = budget
            .checked_add(expanded)
            .ok_or(ImportError::LimitExceeded("PSD layer expansion"))?;
        if budget > MAX_INPUT_BYTES {
            return Err(ImportError::LimitExceeded("PSD layer expansion"));
        }
        if r.take(4)? != b"8BIM" {
            return Err(ImportError::Malformed("layer blend signature"));
        }
        let blend_mode = read_blend_mode(r.take(4)?).ok_or_else(unsupported)?;
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
        let mask = mask(
            extra,
            h,
            rect,
            relative_mask_bits & (1u16 << layer_index) != 0,
        )?;
        if mask.is_some_and(|m| {
            m.rect[0] < 0
                || m.rect[1] < 0
                || m.rect[2] > i64::from(h.height)
                || m.rect[3] > i64::from(h.width)
        }) {
            *mask_clipped = true;
        }
        if mask.is_some_and(|m| m.ambiguous_origin) {
            *ambiguous = true;
            *ambiguous_mask_bits |= 1u16 << layer_index;
        }
        if mask.is_some() != channels.iter().any(|(id, _)| *id == -2) {
            return Err(ImportError::Malformed("layer mask channel"));
        }
        if let Some(mask) = mask {
            let width = (mask.rect[3] - mask.rect[1]) as usize;
            let height = (mask.rect[2] - mask.rect[0]) as usize;
            let area = width
                .checked_mul(height)
                .ok_or(ImportError::LimitExceeded("PSD layer expansion"))?;
            budget = budget
                .checked_add(area)
                .ok_or(ImportError::LimitExceeded("PSD layer expansion"))?;
            if budget > MAX_INPUT_BYTES {
                return Err(ImportError::LimitExceeded("PSD layer expansion"));
            }
        }
        records.push(Record {
            rect,
            channels,
            blend_mode,
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
                [
                    mask.rect[0].clamp(0, i64::from(h.height)) as u32,
                    mask.rect[1].clamp(0, i64::from(h.width)) as u32,
                    mask.rect[2].clamp(0, i64::from(h.height)) as u32,
                    mask.rect[3].clamp(0, i64::from(h.width)) as u32,
                ]
            };
            storage += tile_count(rect) * (TILE_SIZE * TILE_SIZE) as usize;
            if storage > MAX_INPUT_BYTES {
                return Err(ImportError::LimitExceeded("PSD layer tiles"));
            }
        }
        if bottom > top
            && right > left
            && (top < 0 || left < 0 || bottom > i64::from(h.height) || right > i64::from(h.width))
        {
            let edge = i64::from(TILE_SIZE);
            let columns = (right - 1).div_euclid(edge) - left.div_euclid(edge) + 1;
            let rows = (bottom - 1).div_euclid(edge) - top.div_euclid(edge) + 1;
            let all_tiles = usize::try_from(columns)
                .ok()
                .and_then(|x| usize::try_from(rows).ok().and_then(|y| x.checked_mul(y)))
                .ok_or(ImportError::LimitExceeded("PSD layer tiles"))?;
            let bytes = all_tiles
                .checked_mul((TILE_SIZE * TILE_SIZE * 4) as usize)
                .ok_or(ImportError::LimitExceeded("PSD layer tiles"))?;
            storage = storage
                .checked_add(bytes)
                .ok_or(ImportError::LimitExceeded("PSD layer tiles"))?;
            if storage > MAX_INPUT_BYTES {
                return Err(ImportError::LimitExceeded("PSD layer tiles"));
            }
        }
        let top = top.clamp(0, i64::from(h.height)) as u32;
        let left = left.clamp(0, i64::from(h.width)) as u32;
        let bottom = bottom.clamp(0, i64::from(h.height)) as u32;
        let right = right.clamp(0, i64::from(h.width)) as u32;
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
        let mut outside = BTreeMap::<(i32, i32), Vec<u8>>::new();
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
                        let dx = mask.rect[1] + x as i64;
                        let dy = mask.rect[0] + y as i64;
                        if dx < 0 || dy < 0 || dx >= i64::from(h.width) || dy >= i64::from(h.height)
                        {
                            continue;
                        }
                        let (dx, dy) = (dx as u32, dy as u32);
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
                    let dx = left + x as i64;
                    let dy = top + y as i64;
                    if dx < 0 || dy < 0 || dx >= i64::from(h.width) || dy >= i64::from(h.height) {
                        let edge = i64::from(TILE_SIZE);
                        let coord = (dx.div_euclid(edge) as i32, dy.div_euclid(edge) as i32);
                        let tile = outside
                            .entry(coord)
                            .or_insert_with(|| vec![0; (TILE_SIZE * TILE_SIZE * 4) as usize]);
                        let at = (dy.rem_euclid(edge) * edge + dx.rem_euclid(edge)) as usize * 4;
                        if id == -1 {
                            tile[at + 3] = value;
                        } else {
                            tile[at + id as usize] = value;
                            if !alpha {
                                tile[at + 3] = 255;
                            }
                        }
                        continue;
                    }
                    let (dx, dy) = (dx as u32, dy as u32);
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
        tiles.retain(|_, pixels| pixels.iter().any(|&value| value != 0));
        layers.push(RasterLayerState {
            effects: Default::default(),
            blend_mode: record.blend_mode,
            id: format!("psd-layer-{index}"),
            name,
            visible: record.flags & 2 == 0,
            opacity: f32::from(record.opacity) / 255.,
            locked,
            alpha_locked: alpha_locked || record.flags & 1 != 0,
            mask_present: record.mask.is_some(),
            mask_linked: record.mask.is_none_or(|mask| mask.flags & 1 == 0),
            mask_enabled: record.mask.is_some_and(|mask| mask.flags & 2 == 0),
            mask_inverted: record
                .mask
                .is_some_and(|mask| mask.flags & 4 != 0 && mask.density == 255),
            mask_density: 1.,
            tiles: tiles
                .into_iter()
                .map(|(coord, pixels)| RasterTileState { coord, pixels })
                .collect(),
            off_canvas_tiles: outside
                .into_iter()
                .filter(|(_, pixels)| pixels.iter().any(|&v| v != 0))
                .map(|((x, y), pixels)| lumapaint_core::tiles::OffCanvasTileState { x, y, pixels })
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
    decode_with_metadata(data, h, density_baked, &mut false, &mut false, &mut false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn photoshop_transparent_rgb_is_retained_inside_and_outside_canvas() {
        for bytes in [
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-transparent-rgb.psd").as_slice(),
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-transparent-rgb.psb").as_slice(),
        ] {
            let imported = PsdImporter
                .import(bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert!(!imported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.layersFlattened"));
            let doc =
                lumapaint_core::tiles::TiledRasterDocument::from_state(imported.raster).unwrap();
            assert_eq!(doc.layers()[0].tiles.pixel(0, 0), Some([190, 10, 90, 0]));
            let outside = &doc.layers()[0].off_canvas_tiles[0];
            assert_eq!((outside.x, outside.y), (-1, 0));
            assert_eq!(&outside.pixels[1020..1024], &[31, 95, 180, 0]);
            for large in [false, true] {
                let output = super::write::write(
                    &doc.state(),
                    large,
                    crate::export::ExportOptions::default(),
                )
                .unwrap();
                let result = PsdImporter
                    .import(&output.bytes, ImportOptions { allow_lossy: true })
                    .unwrap();
                let restored =
                    lumapaint_core::tiles::TiledRasterDocument::from_state(result.raster).unwrap();
                assert_eq!(
                    restored.layers()[0].tiles.pixel(0, 0),
                    Some([190, 10, 90, 0])
                );
                assert_eq!(
                    &restored.layers()[0].off_canvas_tiles[0].pixels[1020..1024],
                    &[31, 95, 180, 0]
                );
            }
        }
    }
    #[test]
    fn photoshop_2026_mask_link_fixtures_expose_observed_flag() {
        for (data, large, expected) in [
            (
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-linked.psd").as_slice(),
                false,
                0,
            ),
            (
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-unlinked.psd")
                    .as_slice(),
                false,
                1,
            ),
            (
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-linked.psb").as_slice(),
                true,
                0,
            ),
            (
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-unlinked.psb")
                    .as_slice(),
                true,
                1,
            ),
        ] {
            let mut file = Reader {
                bytes: data,
                at: 26,
            };
            file.section().unwrap();
            file.section().unwrap();
            let section = file.sized_section(large).unwrap();
            let mut outer = Reader {
                bytes: section,
                at: 0,
            };
            let info = outer.sized_section(large).unwrap();
            let mut records = Reader { bytes: info, at: 0 };
            let count = (records.u16().unwrap() as i16).unsigned_abs();
            let mut flags = Vec::new();
            for _ in 0..count {
                records.take(16).unwrap();
                let channels = records.u16().unwrap();
                for _ in 0..channels {
                    records.take(2).unwrap();
                    records.length(large).unwrap();
                }
                records.take(12).unwrap();
                let extra = records.section().unwrap();
                let mut reader = Reader {
                    bytes: extra,
                    at: 0,
                };
                let mask = reader.section().unwrap();
                if !mask.is_empty() {
                    flags.push(mask[17] & 1);
                }
            }
            assert_eq!(flags, [expected]);
        }
    }

    #[test]
    fn photoshop_link_fixtures_import_link_state() {
        for (bytes, linked) in [
            (
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-linked.psd").as_slice(),
                true,
            ),
            (
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-unlinked.psd")
                    .as_slice(),
                false,
            ),
            (
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-linked.psb").as_slice(),
                true,
            ),
            (
                include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-unlinked.psb")
                    .as_slice(),
                false,
            ),
        ] {
            let mut reader = Reader { bytes, at: 26 };
            reader.section().unwrap();
            reader.section().unwrap();
            let section = reader
                .sized_section(read_header(bytes).unwrap().version == 2)
                .unwrap();
            let raw = decode_with_metadata(
                section,
                read_header(bytes).unwrap(),
                &mut false,
                &mut false,
                &mut false,
                &mut false,
            );
            assert!(raw.is_ok(), "decode: {:?}", raw.err());
            let image = PsdImporter
                .import(bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            let mask_layer = image
                .raster
                .layers
                .iter()
                .find(|layer| layer.mask_present)
                .unwrap_or_else(|| panic!("Photoshop mask not retained: {:?}", image.report));
            assert_eq!(mask_layer.mask_linked, linked);
        }
    }

    #[test]
    fn photoshop_mask_moved_outside_canvas_retains_visible_mask_with_consent() {
        for bytes in [
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-effect-mask-moved-outside.psd")
                .as_slice(),
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-effect-mask-moved-outside.psb")
                .as_slice(),
        ] {
            assert!(matches!(
                PsdImporter.import(bytes, ImportOptions::default()),
                Err(ImportError::LossyConversionRequiresConsent(_))
            ));
            let result = PsdImporter
                .import(bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert!(result
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.maskOutsideCanvasClipped"));
            assert!(!result
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.layersFlattened"));
            assert_eq!(result.raster.layers.len(), 1);
            assert!(result.raster.layers[0].mask_present);
            assert!(!result.raster.layers[0].mask_linked);
            let live =
                lumapaint_core::tiles::TiledRasterDocument::from_state(result.raster).unwrap();
            assert_eq!(live.layers()[0].mask.pixel(20, 50), Some(0));
            assert_eq!(live.layers()[0].mask.pixel(50, 50), Some(255));
            let large = read_header(bytes).unwrap().version == 2;
            let saved = super::write::write(
                &live.state(),
                large,
                crate::export::ExportOptions { allow_lossy: true },
            )
            .unwrap();
            let again = PsdImporter
                .import(&saved.bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert!(!again
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.layersFlattened"));
            let restored =
                lumapaint_core::tiles::TiledRasterDocument::from_state(again.raster).unwrap();
            assert!(!restored.layers()[0].mask_linked);
            assert_eq!(restored.layers()[0].mask.pixel(20, 50), Some(0));
            assert_eq!(restored.layers()[0].mask.pixel(50, 50), Some(255));
        }
    }

    #[test]
    fn photoshop_offset_psb_retains_mask_position_and_link() {
        for bytes in [
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-offset.psb").as_slice(),
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-offset-roundtrip.psd")
                .as_slice(),
        ] {
            let image = PsdImporter
                .import(bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            let layer = image
                .raster
                .layers
                .iter()
                .find(|l| l.mask_present)
                .expect("Photoshop mask retained");
            assert!(!layer.mask_linked);
            let doc = lumapaint_core::tiles::TiledRasterDocument::from_state(image.raster).unwrap();
            assert_eq!(doc.layers()[0].mask.pixel(2, 30), Some(0));
            assert_eq!(doc.layers()[0].mask.pixel(257, 30), Some(255));
            assert_eq!(
                doc.layers()[0].tiles.pixel(257, 30),
                Some([255, 255, 255, 255])
            );
        }
    }

    #[test]
    fn photoshop_off_canvas_layer_falls_back_if_composite_differs() {
        for bytes in [
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-mask-offset.psd").as_slice(),
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-offcanvas.psb").as_slice(),
        ] {
            let mut reader = Reader { bytes, at: 26 };
            reader.section().unwrap();
            reader.section().unwrap();
            let header = read_header(bytes).unwrap();
            let section = reader.sized_section(header.version == 2).unwrap();
            let mut clipped = false;
            let layers = decode_with_metadata(
                section,
                header,
                &mut false,
                &mut false,
                &mut clipped,
                &mut false,
            )
            .unwrap()
            .unwrap();
            assert!(clipped);
            assert_eq!(layers.len(), 8);
            let layer = layers.iter().find(|l| l.name == "Layer 6").unwrap();
            assert!(!layer.mask_linked);
            assert!(!layer.tiles.is_empty());
            assert!(matches!(
                PsdImporter.import(bytes, ImportOptions::default()),
                Err(ImportError::LossyConversionRequiresConsent(_))
            ));
            let imported = PsdImporter
                .import(bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert!(imported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.layersFlattened"));
            assert!(!imported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.offCanvasPixelsClipped"));
        }
    }

    #[test]
    fn photoshop_negative_origin_psd_psb_keep_in_canvas_layer_pixels() {
        for bytes in [
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-negative-layer.psb").as_slice(),
            include_bytes!("../../tests/fixtures/lp-photoshop-2026-negative-layer.psd").as_slice(),
        ] {
            let header = read_header(bytes).unwrap();
            let mut reader = Reader { bytes, at: 26 };
            reader.section().unwrap();
            reader.section().unwrap();
            let section = reader.sized_section(header.version == 2).unwrap();
            let mut clipped = false;
            let layers = decode_with_metadata(
                section,
                header,
                &mut false,
                &mut false,
                &mut clipped,
                &mut false,
            )
            .unwrap()
            .unwrap();
            assert!(clipped);
            assert_eq!(layers.len(), 8);
            let layer = layers.iter().find(|l| l.name == "Layer 6").unwrap();
            assert!(!layer.mask_linked);
            assert!(!layer.tiles.is_empty());
            let state = TiledRasterState {
                width: header.width,
                height: header.height,
                resolution: None,
                icc_profile: None,
                layers,
            };
            lumapaint_core::tiles::TiledRasterDocument::from_state(state).unwrap();
            let imported = PsdImporter
                .import(bytes, ImportOptions { allow_lossy: true })
                .unwrap();
            assert!(
                imported
                    .report
                    .issues
                    .iter()
                    .any(|issue| issue.code == "psd.layersFlattened"),
                "{:?}",
                imported.report
            );
        }
    }

    #[test]
    fn negative_layer_origin_decodes_visible_pixels_with_clipping_report() {
        let h = Header {
            version: 1,
            width: 4,
            height: 2,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        let mut bytes = region_layer(0, false, [0, 0, 2, 4], [0, 0, 1, 2], 255, &[0, 128]);
        for (idx, coordinate) in [-1i32, -1, 1, 3].iter().enumerate() {
            bytes[6 + idx * 4..10 + idx * 4].copy_from_slice(&coordinate.to_be_bytes());
        }
        let mut clipped = false;
        let layers =
            decode_with_metadata(&bytes, h, &mut false, &mut false, &mut clipped, &mut false)
                .unwrap()
                .unwrap();
        assert!(clipped);
        let state = TiledRasterState {
            width: 4,
            height: 2,
            resolution: None,
            icc_profile: None,
            layers,
        };
        let doc = lumapaint_core::tiles::TiledRasterDocument::from_state(state).unwrap();
        assert_eq!(doc.layers()[0].tiles.pixel(0, 0), Some([255, 0, 0, 255]));
        assert_eq!(doc.layers()[0].tiles.pixel(2, 0), Some([255, 0, 0, 255]));
        assert_eq!(doc.layers()[0].tiles.pixel(3, 0), Some([0, 0, 0, 0]));
        assert_eq!(doc.layers()[0].tiles.pixel(0, 1), Some([0, 0, 0, 0]));
    }

    #[test]
    fn positive_off_canvas_pixels_are_retained_by_decoder() {
        let h = Header {
            version: 1,
            width: 4,
            height: 2,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        let bytes = region_layer(0, false, [0, 1, 2, 5], [0, 0, 1, 2], 255, &[0, 128]);
        let mut clipped = false;
        let layers =
            decode_with_metadata(&bytes, h, &mut false, &mut false, &mut clipped, &mut false)
                .unwrap()
                .unwrap();
        assert!(clipped);
        let state = TiledRasterState {
            width: 4,
            height: 2,
            resolution: None,
            icc_profile: None,
            layers,
        };
        let doc = lumapaint_core::tiles::TiledRasterDocument::from_state(state).unwrap();
        assert_eq!(doc.layers()[0].tiles.pixel(0, 0), Some([0, 0, 0, 0]));
        assert!(doc.layers()[0].tiles.pixel(3, 0).unwrap()[3] > 0);
        assert!(!doc.layers()[0].off_canvas_tiles.is_empty());
    }

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
    fn merged_preview_selects_mixed_mask_origins_per_layer() {
        fn parts(section: &[u8]) -> (Vec<u8>, Vec<u8>) {
            let mut outer = Reader {
                bytes: section,
                at: 0,
            };
            let info = outer.section().unwrap();
            let mut r = Reader { bytes: info, at: 2 };
            r.take(16).unwrap();
            let count = r.u16().unwrap();
            for _ in 0..count {
                r.take(2).unwrap();
                r.length(false).unwrap();
            }
            r.take(12).unwrap();
            r.section().unwrap();
            (info[2..r.at].to_vec(), info[r.at..].to_vec())
        }
        for (size, fully_outside, selected) in
            [(4, false, 1), (4, true, 1), (513, false, 1), (4, false, 0)]
        {
            let h = Header {
                version: 1,
                width: size,
                height: size,
                channels: 3,
                depth: 8,
                color_mode: 3,
            };
            let first = if fully_outside {
                region_layer(1, false, [1, 1, 2, 4], [1, 3, 2, 4], 255, &[0])
            } else if size == 4 {
                region_layer(1, false, [1, 1, 2, 3], [0, 0, 1, 4], 255, &[0, 128, 255, 0])
            } else {
                region_layer(1, false, [1, 1, 2, 3], [0, 0, 1, 2], 255, &[0, 128])
            };
            let second = region_layer(1, false, [2, 1, 3, 3], [0, 0, 1, 2], 255, &[0, 128]);
            let (r1, p1) = parts(&first);
            let (r2, p2) = parts(&second);
            let mut info = (-2i16).to_be_bytes().to_vec();
            info.extend(r1);
            info.extend(r2);
            info.extend(p1);
            info.extend(p2);
            if !info.len().is_multiple_of(2) {
                info.push(0);
            }
            let mut section = (info.len() as u32).to_be_bytes().to_vec();
            section.extend(info);
            section.extend([0; 4]);
            let mut bits = 0;
            let layers = decode_with_metadata_mode(
                &section,
                h,
                &mut false,
                &mut false,
                &mut false,
                &mut false,
                (selected, &mut bits),
            )
            .unwrap()
            .unwrap();
            assert_eq!(bits, 3);
            let state = TiledRasterState {
                width: size,
                height: size,
                resolution: None,
                icc_profile: None,
                layers,
            };
            let saved = super::write::write(&state, false, crate::export::ExportOptions::default())
                .unwrap();
            let mut file = saved.bytes;
            let mut cursor = Reader {
                bytes: &file,
                at: 26,
            };
            cursor.section().unwrap();
            cursor.section().unwrap();
            let start = cursor.at;
            let end =
                start + 4 + u32::from_be_bytes(file[start..start + 4].try_into().unwrap()) as usize;
            let mut replacement = (section.len() as u32).to_be_bytes().to_vec();
            replacement.extend(section);
            file.splice(start..end, replacement);
            if selected == 0 {
                PsdImporter.import(&file, ImportOptions::default()).unwrap();
            }
            let imported = PsdImporter
                .import(&file, ImportOptions { allow_lossy: true })
                .unwrap();
            assert_eq!(
                imported
                    .report
                    .issues
                    .iter()
                    .any(|i| i.code == "psd.maskOriginRelativeInferred"),
                selected != 0,
                "{:?}",
                imported.report
            );
            assert!(
                !imported
                    .report
                    .issues
                    .iter()
                    .any(|i| i.code == "psd.maskOriginAmbiguous"),
                "{:?}",
                imported.report
            );
            assert!(!imported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.layersFlattened"));
            if size == 4 && selected != 0 {
                assert!(imported
                    .report
                    .issues
                    .iter()
                    .any(|i| i.code == "psd.maskOutsideCanvasClipped"));
            }
            let doc =
                lumapaint_core::tiles::TiledRasterDocument::from_state(imported.raster).unwrap();
            assert_eq!(
                doc.layers()[0].mask.pixel(1, 1),
                Some(if fully_outside || selected == 0 {
                    255
                } else {
                    0
                })
            );
            assert_eq!(doc.layers()[1].mask.pixel(0, 0), Some(0));
            assert_eq!(doc.layers()[1].mask.pixel(1, 2), Some(255));
        }
    }

    #[test]
    fn invisible_mask_origin_remains_ambiguous_after_preview_match() {
        fn parts(section: &[u8]) -> (Vec<u8>, Vec<u8>) {
            let mut outer = Reader {
                bytes: section,
                at: 0,
            };
            let info = outer.section().unwrap();
            let mut r = Reader { bytes: info, at: 2 };
            r.take(16).unwrap();
            let count = r.u16().unwrap();
            for _ in 0..count {
                r.take(2).unwrap();
                r.length(false).unwrap();
            }
            r.take(12).unwrap();
            r.section().unwrap();
            (info[2..r.at].to_vec(), info[r.at..].to_vec())
        }
        for (size, zero_opacity, selected) in
            [(4, false, 0), (4, false, 1), (4, true, 0), (4, true, 1)]
        {
            let h = Header {
                version: 1,
                width: size,
                height: size,
                channels: 3,
                depth: 8,
                color_mode: 3,
            };
            let first = region_layer(1, false, [1, 1, 2, 3], [0, 0, 1, 2], 255, &[0, 128]);
            let second = region_layer(1, false, [2, 1, 3, 3], [0, 0, 1, 2], 255, &[0, 128]);
            let (r1, p1) = parts(&first);
            let (mut r2, p2) = parts(&second);
            let channels = u16::from_be_bytes([r2[16], r2[17]]) as usize;
            if zero_opacity {
                r2[18 + channels * 6 + 8] = 0;
            } else {
                r2[18 + channels * 6 + 10] |= 2;
            }

            let mut info = (-2i16).to_be_bytes().to_vec();
            info.extend(r1);
            info.extend(r2);
            info.extend(p1);
            info.extend(p2);
            if !info.len().is_multiple_of(2) {
                info.push(0);
            }
            let mut section = (info.len() as u32).to_be_bytes().to_vec();
            section.extend(info);
            section.extend([0; 4]);
            let mut bits = 0;
            let layers = decode_with_metadata_mode(
                &section,
                h,
                &mut false,
                &mut false,
                &mut false,
                &mut false,
                (selected, &mut bits),
            )
            .unwrap()
            .unwrap();
            assert_eq!(bits, 3);
            let state = TiledRasterState {
                width: size,
                height: size,
                resolution: None,
                icc_profile: None,
                layers,
            };
            let saved = super::write::write(&state, false, crate::export::ExportOptions::default())
                .unwrap();
            let mut file = saved.bytes;
            let mut cursor = Reader {
                bytes: &file,
                at: 26,
            };
            cursor.section().unwrap();
            cursor.section().unwrap();
            let start = cursor.at;
            let end =
                start + 4 + u32::from_be_bytes(file[start..start + 4].try_into().unwrap()) as usize;
            let mut replacement = (section.len() as u32).to_be_bytes().to_vec();
            replacement.extend(section);
            file.splice(start..end, replacement);
            let imported = PsdImporter
                .import(&file, ImportOptions { allow_lossy: true })
                .unwrap();
            assert_eq!(
                imported
                    .report
                    .issues
                    .iter()
                    .any(|i| i.code == "psd.maskOriginRelativeInferred"),
                selected != 0,
                "{:?}",
                imported.report
            );
            assert!(!imported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.layersFlattened"));
            assert!(
                imported
                    .report
                    .issues
                    .iter()
                    .any(|i| i.code == "psd.maskOriginAmbiguous"),
                "{:?}",
                imported.report
            );

            let doc =
                lumapaint_core::tiles::TiledRasterDocument::from_state(imported.raster).unwrap();
            assert_eq!(
                doc.layers()[0].mask.pixel(1, 1),
                Some(if selected == 0 { 255 } else { 0 })
            );
            assert!(if zero_opacity {
                doc.layers()[1].opacity == 0.0
            } else {
                !doc.layers()[1].visible
            });
        }
    }

    #[test]
    fn mixed_many_mask_origins_use_early_tile_rejection() {
        let h = Header {
            version: 1,
            width: 4097,
            height: 4097,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        let mut records = Vec::new();
        let mut payload = Vec::new();
        for y in 1..=6 {
            let section = region_layer(1, false, [y, 1, y + 1, 3], [0, 0, 1, 2], 255, &[0, 128]);
            let mut outer = Reader {
                bytes: &section,
                at: 0,
            };
            let info = outer.section().unwrap();
            let mut r = Reader { bytes: info, at: 2 };
            r.take(16).unwrap();
            let count = r.u16().unwrap();
            for _ in 0..count {
                r.take(2).unwrap();
                r.length(false).unwrap();
            }
            r.take(12).unwrap();
            r.section().unwrap();
            records.extend_from_slice(&info[2..r.at]);
            payload.extend_from_slice(&info[r.at..]);
        }
        let mut info = (-6i16).to_be_bytes().to_vec();
        info.extend(records);
        info.extend(payload);
        if !info.len().is_multiple_of(2) {
            info.push(0);
        }
        let mut section = (info.len() as u32).to_be_bytes().to_vec();
        section.extend(info);
        section.extend([0; 4]);
        let mut bits = 0;
        let layers = decode_with_metadata_mode(
            &section,
            h,
            &mut false,
            &mut false,
            &mut false,
            &mut false,
            (0b100001, &mut bits),
        )
        .unwrap()
        .unwrap();
        assert_eq!(bits, 0b111111);
        let state = TiledRasterState {
            width: h.width,
            height: h.height,
            resolution: None,
            icc_profile: None,
            layers,
        };
        // Bake the sparse masks only for constructing the merged preview;
        // writing full-canvas mask channels for this synthetic document would
        // exceed the exporter's 128 MiB limit.
        let mut preview = state.clone();
        for layer in &mut preview.layers {
            for tile in &mut layer.tiles {
                if let Some(mask) = layer
                    .mask_tiles
                    .iter()
                    .find(|mask| mask.coord == tile.coord)
                {
                    for (pixel, &value) in tile
                        .pixels
                        .as_chunks_mut::<4>()
                        .0
                        .iter_mut()
                        .zip(&mask.values)
                    {
                        pixel[3] = ((u16::from(pixel[3]) * u16::from(value) + 127) / 255) as u8;
                        if pixel[3] == 0 {
                            *pixel = [0; 4];
                        }
                    }
                }
            }
            layer.tiles.retain(|tile| {
                tile.pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] != 0)
            });
            layer.mask_tiles.clear();
            layer.mask_present = false;
            layer.mask_enabled = false;
        }
        let mut file =
            super::write::write(&preview, false, crate::export::ExportOptions::default())
                .unwrap()
                .bytes;
        let mut cursor = Reader {
            bytes: &file,
            at: 26,
        };
        cursor.section().unwrap();
        cursor.section().unwrap();
        let start = cursor.at;
        let end =
            start + 4 + u32::from_be_bytes(file[start..start + 4].try_into().unwrap()) as usize;
        let mut replacement = (section.len() as u32).to_be_bytes().to_vec();
        replacement.extend(section);
        file.splice(start..end, replacement);
        let imported = PsdImporter
            .import(&file, ImportOptions { allow_lossy: true })
            .unwrap();
        assert!(
            imported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.maskOriginRelativeInferred"),
            "{:?}",
            imported.report
        );
        assert_eq!(imported.raster.layers.len(), 6);
    }

    #[test]
    fn merged_preview_selects_legacy_relative_mask_when_absolute_differs() {
        let h = Header {
            version: 1,
            width: 4,
            height: 2,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        let mut section = region_layer(1, false, [1, 1, 2, 3], [0, 0, 1, 2], 255, &[0, 128]);
        let mut ambiguous = false;
        let layers = decode_with_metadata_mode(
            &section,
            h,
            &mut false,
            &mut false,
            &mut false,
            &mut ambiguous,
            (u16::MAX, &mut 0),
        )
        .unwrap()
        .unwrap();
        assert!(ambiguous);
        let state = TiledRasterState {
            width: 4,
            height: 2,
            resolution: None,
            icc_profile: None,
            layers,
        };
        let saved =
            super::write::write(&state, false, crate::export::ExportOptions::default()).unwrap();
        let mut file = saved.bytes;
        let mut cursor = Reader {
            bytes: &file,
            at: 26,
        };
        cursor.section().unwrap();
        cursor.section().unwrap();
        let start = cursor.at;
        let end =
            start + 4 + u32::from_be_bytes(file[start..start + 4].try_into().unwrap()) as usize;
        // The merged image uses alpha; preserve the negative layer count.
        section[4..6].copy_from_slice(&(-1i16).to_be_bytes());
        let mut replacement = (section.len() as u32).to_be_bytes().to_vec();
        replacement.extend(section);
        file.splice(start..end, replacement);
        let imported = PsdImporter
            .import(&file, ImportOptions { allow_lossy: true })
            .unwrap();
        assert!(
            imported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.maskOriginRelativeInferred"),
            "{:?}",
            imported.report
        );
        assert!(!imported
            .report
            .issues
            .iter()
            .any(|i| i.code == "psd.layersFlattened"));
        let doc = lumapaint_core::tiles::TiledRasterDocument::from_state(imported.raster).unwrap();
        assert_eq!(doc.layers()[0].mask.pixel(1, 1), Some(0));
        assert_eq!(doc.layers()[0].mask.pixel(0, 0), Some(255));
    }

    #[test]
    fn ambiguous_legacy_mask_origin_is_marked_for_preview_verification() {
        let h = Header {
            version: 1,
            width: 4,
            height: 2,
            channels: 3,
            depth: 8,
            color_mode: 3,
        };
        let bytes = region_layer(1, false, [1, 1, 2, 3], [0, 0, 1, 2], 255, &[0, 128]);
        let mut ambiguous = false;
        let layers = decode_with_metadata(
            &bytes,
            h,
            &mut false,
            &mut false,
            &mut false,
            &mut ambiguous,
        )
        .unwrap()
        .unwrap();
        assert!(ambiguous);
        assert!(!layers[0].mask_linked);
        let state = TiledRasterState {
            width: 4,
            height: 2,
            resolution: None,
            icc_profile: None,
            layers,
        };
        let doc = lumapaint_core::tiles::TiledRasterDocument::from_state(state).unwrap();
        assert_eq!(doc.layers()[0].mask.pixel(0, 0), Some(0));
    }

    #[test]
    fn unlinked_masks_use_document_coordinates_and_resolve_negative_legacy_offsets() {
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
            let mut relative = decode(&bytes, h, &mut false).unwrap().unwrap();
            assert!(!relative[0].mask_linked);
            relative[0].mask_linked = true;
            let absolute = decode(
                &region_layer(0, rle, [1, 1, 2, 3], [0, 0, 1, 2], 255, &[0, 128]),
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
            let mut resolved = decode_with_metadata_mode(
                &negative,
                h,
                &mut false,
                &mut false,
                &mut false,
                &mut false,
                (1, &mut 0),
            )
            .unwrap()
            .unwrap();
            assert!(!resolved[0].mask_linked);
            resolved[0].mask_linked = true;
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
                        resolution: None,
                        icc_profile: None,
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
                        resolution: None,
                        icc_profile: None,
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
                        resolution: None,
                        icc_profile: None,
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
            resolution: None,
            icc_profile: None,
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
                resolution: None,
                icc_profile: None,
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
            mask(&extra([0, 3, 1, 2]), h, [0; 4], false),
            Err(ImportError::Malformed("layer mask rectangle"))
        ));
        for rect in [[0, 0, 1, 259], [u32::MAX, 0, 1, 1]] {
            assert!(mask(&extra(rect), h, [0; 4], false).unwrap().is_some());
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
                    resolution: None,
                    icc_profile: None,
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
                resolution: None,
                icc_profile: None,
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
        bytes[index..index + 4].copy_from_slice(b"hue ");
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
    fn photoshop_text_metadata_requires_consent_before_flattening() {
        // A synthetic type-tool block verifies the compatibility boundary; it
        // does not stand in for a real Photoshop text descriptor fixture.
        let mut bytes = fixture(false).to_vec();
        let extra_len = u32::from_be_bytes(bytes[92..96].try_into().unwrap());
        let mut block = b"8BIMTySh".to_vec();
        block.extend(4u32.to_be_bytes());
        block.extend([0u8; 4]);
        for offset in [34, 38, 92] {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
            bytes[offset..offset + 4].copy_from_slice(&(length + block.len() as u32).to_be_bytes());
        }
        bytes.splice(96 + extra_len as usize..96 + extra_len as usize, block);
        let Err(ImportError::LossyConversionRequiresConsent(report)) =
            PsdImporter.import(&bytes, ImportOptions::default())
        else {
            panic!("text metadata must require consent");
        };
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "psd.textNotEditable"));
        let imported = PsdImporter
            .import(&bytes, ImportOptions { allow_lossy: true })
            .unwrap();
        assert_eq!(imported.raster.layers.len(), 1);
        assert_eq!(imported.raster.layers[0].id, "psd-composite");
        assert!(imported
            .report
            .issues
            .iter()
            .any(|issue| issue.code == "psd.layersFlattened"));
        assert!(imported
            .report
            .issues
            .iter()
            .any(|issue| issue.code == "psd.textNotEditable"));

        // An earlier unsupported blend must not hide the subsequent text tag.
        let blend = bytes.windows(8).position(|v| v == b"8BIMnorm").unwrap() + 4;
        bytes[blend..blend + 4].copy_from_slice(b"hue ");
        let Err(ImportError::LossyConversionRequiresConsent(report)) =
            PsdImporter.import(&bytes, ImportOptions::default())
        else {
            panic!("unsupported blend and text require consent");
        };
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "psd.textNotEditable"));
    }
    #[test]
    fn text_metadata_scan_does_not_match_embedded_payload_bytes() {
        let mut bytes = fixture(false).to_vec();
        let extra_len = u32::from_be_bytes(bytes[92..96].try_into().unwrap());
        let mut block = b"8BIMzzzz".to_vec();
        block.extend(8u32.to_be_bytes());
        block.extend(b"8BIMTySh");
        for offset in [34, 38, 92] {
            let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
            bytes[offset..offset + 4].copy_from_slice(&(length + block.len() as u32).to_be_bytes());
        }
        bytes.splice(96 + extra_len as usize..96 + extra_len as usize, block);
        let Err(ImportError::LossyConversionRequiresConsent(report)) =
            PsdImporter.import(&bytes, ImportOptions::default())
        else {
            panic!("unknown metadata must require consent");
        };
        assert!(!report
            .issues
            .iter()
            .any(|issue| issue.code == "psd.textNotEditable"));
    }
    #[test]
    fn unsupported_masks_clipping_and_groups_fall_back_and_invalid_bounds_are_rejected() {
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
            if mode == 1 {
                // Only the rectangle was altered; channel lengths no longer match it.
                assert!(matches!(
                    PsdImporter.import(&bytes, ImportOptions { allow_lossy: true }),
                    Err(ImportError::Malformed("layer raw length"))
                ));
                continue;
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
