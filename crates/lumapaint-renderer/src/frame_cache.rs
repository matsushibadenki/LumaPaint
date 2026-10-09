//! CPU cache for independently editable text frames in one vector layer.
use super::{prepare_svg_layer, vector, PreparedSvgLayer};
use lumapaint_core::document::SvgLayer;
use std::collections::HashMap;
use std::sync::Arc;

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 4096;
const MAX_DISPLAY_TEXT_FRAMES: usize = 4096;
const MAX_COMPOSITE_BYTES: usize = 16 * 1024 * 1024;
const MAX_COMPOSITE_ENTRIES: usize = 8;

/// Premultiplied pixels bounded by actual coverage, including antialiased edges.
#[derive(Clone)]
pub(crate) struct CroppedFrame {
    pub(crate) pixels: Vec<u8>,
    pub(crate) origin: (usize, usize),
    pub(crate) width: usize,
}

pub(crate) struct PatchRows<'a> {
    pub(crate) pixels: &'a [u8],
    pub(crate) stride: u32,
    pub(crate) origin: [u32; 2],
    pub(crate) size: [u32; 2],
}

pub(crate) fn patch_rows(
    frame: &CroppedFrame,
    bounds: [usize; 4],
) -> Result<PatchRows<'_>, String> {
    let [left, top, right, bottom] = bounds;
    let stride = frame
        .width
        .checked_mul(4)
        .filter(|s| *s != 0)
        .ok_or("Invalid text patch stride")?;
    if !frame.pixels.len().is_multiple_of(stride)
        || right <= left
        || bottom <= top
        || left < frame.origin.0
        || top < frame.origin.1
        || right > frame.origin.0.saturating_add(frame.width)
        || bottom > frame.origin.1.saturating_add(frame.pixels.len() / stride)
    {
        return Err("Invalid overlapping text patch bounds".into());
    }
    let offset = ((top - frame.origin.1) * frame.width + left - frame.origin.0) * 4;
    let end = offset + (bottom - top - 1) * stride + (right - left) * 4;
    Ok(PatchRows {
        pixels: &frame.pixels[offset..end],
        stride: u32::try_from(stride).map_err(|_| "Invalid text patch stride")?,
        origin: [
            u32::try_from(left).map_err(|_| "Invalid text patch origin")?,
            u32::try_from(top).map_err(|_| "Invalid text patch origin")?,
        ],
        size: [
            u32::try_from(right - left).map_err(|_| "Invalid text patch width")?,
            u32::try_from(bottom - top).map_err(|_| "Invalid text patch height")?,
        ],
    })
}

/// Worker result for display. Export and compatibility compositing retain full pixels.
pub enum PreparedDisplayLayer {
    Full(PreparedSvgLayer),
    Text(PreparedTextLayer),
}

pub(crate) struct CompositePatch {
    pub(crate) base_source: String,
    pub(crate) bounds: [usize; 4],
}

pub struct PreparedTextLayer {
    pub(crate) tiled: Option<Arc<crate::tiled_rgba::TiledRgba>>,
    pub(crate) patch: Option<CompositePatch>,
    pub(crate) independent: bool,
    pub(crate) opacity: f32,
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) size: (u32, u32),
    pub(crate) frames: Vec<PreparedTextFrame>,
}

#[cfg_attr(test, derive(Clone))]
pub(crate) struct PreparedTextFrame {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) frame: Arc<CroppedFrame>,
}

#[cfg(test)]
impl PreparedTextLayer {
    pub(crate) fn materialize_tiled(&mut self) {
        if let Some(image) = self.tiled.take() {
            crate::performance::count(
                "text_tiled_materialized_bytes",
                image.payload_bytes() as u64,
            );
            let original = &self.frames[0].frame;
            self.frames[0].frame = Arc::new(CroppedFrame {
                pixels: image.materialize(),
                origin: original.origin,
                width: original.width,
            });
        }
    }
}

#[cfg(test)]
impl PreparedTextLayer {
    pub(crate) fn into_full(mut self) -> PreparedSvgLayer {
        self.materialize_tiled();
        let byte_count = self.size.0 as usize * self.size.1 as usize * 4;
        crate::performance::count("full_canvas_allocations", 1);
        crate::performance::count("cpu_rgba_allocated_bytes", byte_count as u64);
        let mut pixels = vec![0; byte_count];
        for frame in self.frames {
            frame.frame.composite(&mut pixels, self.size.0 as usize);
        }
        PreparedSvgLayer {
            id: self.id,
            source: self.source,
            opacity: self.opacity,
            size: self.size,
            fully_contained: true,
            pixels,
        }
    }
}

impl PreparedDisplayLayer {
    pub fn id(&self) -> &str {
        match self {
            Self::Full(layer) => &layer.id,
            Self::Text(layer) => &layer.id,
        }
    }

    /// Available CPU pixel payload; resident GPU reuse can reduce actual upload further.
    pub fn pixel_bytes(&self) -> usize {
        match self {
            Self::Full(layer) => layer.pixels.len(),
            Self::Text(layer) => layer.tiled.as_ref().map_or_else(
                || layer.frames.iter().map(|f| f.frame.len()).sum(),
                |image| image.payload_bytes(),
            ),
        }
    }
}

impl CroppedFrame {
    fn from_pixels(pixels: Vec<u8>, width: u32, height: u32) -> Self {
        let width = width as usize;
        let mut left = width;
        let mut right = 0;
        let mut top = height as usize;
        let mut bottom = 0;
        for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
            if pixel[3] != 0 {
                let x = index % width;
                let y = index / width;
                left = left.min(x);
                right = right.max(x + 1);
                top = top.min(y);
                bottom = bottom.max(y + 1);
            }
        }
        if right == 0 {
            return Self {
                pixels: Vec::new(),
                origin: (0, 0),
                width: 0,
            };
        }
        // Keep a transparent texel guard for GPU bilinear sampling at frame edges.
        left = left.saturating_sub(1);
        top = top.saturating_sub(1);
        right = (right + 1).min(width);
        bottom = (bottom + 1).min(height as usize);
        let mut cropped = Vec::with_capacity((right - left) * (bottom - top) * 4);
        for y in top..bottom {
            cropped.extend_from_slice(&pixels[(y * width + left) * 4..(y * width + right) * 4]);
        }
        Self {
            pixels: cropped,
            origin: (left, top),
            width: right - left,
        }
    }

    fn composite(&self, destination: &mut [u8], width: usize) {
        let _timer = crate::performance::time("text_frame_cpu_composite");
        if self.width == 0 {
            return;
        }
        for (row, source) in self.pixels.chunks_exact(self.width * 4).enumerate() {
            let start = ((self.origin.1 + row) * width + self.origin.0) * 4;
            source_over(&mut destination[start..start + source.len()], source);
        }
    }

    fn len(&self) -> usize {
        self.pixels.len()
    }
}

struct Entry {
    source: String,
    size: (u32, u32),
    pixels: Arc<CroppedFrame>,
    fully_contained: bool,
    last_used: u64,
}

#[derive(Clone)]
struct CompositeFrameKey {
    id: String,
    source: String,
    rect: [usize; 4],
}

fn frame_key(frame: &PreparedTextFrame) -> CompositeFrameKey {
    CompositeFrameKey {
        id: frame.id.clone(),
        source: frame.source.clone(),
        rect: [
            frame.frame.origin.0,
            frame.frame.origin.1,
            frame.frame.width,
            frame.frame.len() / (frame.frame.width * 4),
        ],
    }
}

struct CompositeEntry {
    tiled: Option<Arc<crate::tiled_rgba::TiledRgba>>,
    frames: Vec<CompositeFrameKey>,
    layer_id: String,
    source: String,
    size: (u32, u32),
    opacity: f32,
    frame: Arc<CroppedFrame>,
    last_used: u64,
}

impl CompositeEntry {
    fn bytes(&self) -> usize {
        self.layer_id.len()
            + self.source.len()
            + self.frame.len()
            + self.tiled.as_ref().map_or(0, |image| image.payload_bytes())
            + self
                .frames
                .iter()
                .map(|f| f.id.len() + f.source.len())
                .sum::<usize>()
    }
}

type FrameKey = (String, String, u64, (u32, u32));
fn glyph_entry_bytes(key: &FrameKey, entry: &Entry) -> usize {
    key.0.len() + key.1.len() + entry.source.len() + entry.pixels.len()
}

/// Accounted retained CPU payload, not total heap usage or GPU allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameCacheStats {
    pub total_retained_payload_bytes: usize,
    pub total_payload_budget_bytes: usize,
    pub composite_entries: usize,
    pub composite_retained_payload_bytes: usize,
    pub composite_payload_budget_bytes: usize,
    pub composite_evicted_entries: usize,
    pub entries: usize,
    pub retained_payload_bytes: usize,
    pub payload_budget_bytes: usize,
    pub evicted_entries: usize,
}

#[derive(Default)]
pub struct FrameRasterCache {
    memory_limit: Option<usize>,
    composites: Vec<CompositeEntry>,
    composite_bytes: usize,
    composite_evicted_entries: usize,
    entries: HashMap<FrameKey, Entry>,
    used_bytes: usize,
    clock: u64,
    evicted_entries: usize,
    /// Diagnostic count of text frames actually rasterized by this cache.
    pub rasterized_frames: usize,
    pub reused_frames: usize,
}

fn fingerprint(source: &str) -> u64 {
    // Stable within and across processes; exact source comparison still guards collisions.
    source.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn source_over(destination: &mut [u8], source: &[u8]) {
    for (destination, source) in destination
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(source.as_chunks::<4>().0)
    {
        if source == &[0, 0, 0, 0] {
            continue;
        }
        if source[3] == 255 {
            *destination = *source;
            continue;
        }
        let remaining = 255 - u32::from(source[3]);
        for channel in 0..4 {
            destination[channel] = (u32::from(source[channel])
                + (u32::from(destination[channel]) * remaining + 127) / 255)
                .min(255) as u8;
        }
    }
}

/// Correct layer opacity after source-over, once per covered row interval.
/// Merge overlapping intervals so overlapping text is never attenuated twice.
fn attenuate_covered_rows(
    pixels: &mut [u8],
    width: usize,
    opacity: f32,
    rows: &mut [Vec<(usize, usize)>],
) -> usize {
    let _timer = crate::performance::time("text_covered_opacity");
    let mut processed = 0;
    for (y, intervals) in rows.iter_mut().enumerate() {
        intervals.sort_unstable();
        let mut merged: Option<(usize, usize)> = None;
        let mut apply = |left: usize, right: usize| {
            let row = &mut pixels[(y * width + left) * 4..(y * width + right) * 4];
            processed += row.len();
            for value in row {
                *value = (f32::from(*value) * opacity).round() as u8;
            }
        };
        for &(left, right) in intervals.iter() {
            match merged {
                Some((start, end)) if left <= end => merged = Some((start, end.max(right))),
                Some((start, end)) => {
                    apply(start, end);
                    merged = Some((left, right));
                }
                None => merged = Some((left, right)),
            }
        }
        if let Some((left, right)) = merged {
            apply(left, right);
        }
    }
    crate::performance::count("text_covered_opacity_bytes", processed as u64);
    processed
}

/// Preserve encoded integer source-over order before applying layer opacity.
fn compose_cropped_frames(
    frames: &[PreparedTextFrame],
    opacity: f32,
) -> Result<CroppedFrame, String> {
    let _timer = crate::performance::time("text_overlap_cropped_composite");
    let left = frames.iter().map(|f| f.frame.origin.0).min().unwrap();
    let top = frames.iter().map(|f| f.frame.origin.1).min().unwrap();
    let right = frames
        .iter()
        .map(|f| f.frame.origin.0 + f.frame.width)
        .max()
        .unwrap();
    let bottom = frames
        .iter()
        .map(|f| f.frame.origin.1 + f.frame.len() / (f.frame.width * 4))
        .max()
        .unwrap();
    let width = right - left;
    let bytes = width
        .checked_mul(bottom - top)
        .and_then(|n| n.checked_mul(4))
        .ok_or("Invalid overlapping text bounds")?;
    let mut pixels = vec![0; bytes];
    crate::performance::count("text_overlap_cropped_allocated_bytes", bytes as u64);
    for entry in frames {
        for (row, source) in entry
            .frame
            .pixels
            .chunks_exact(entry.frame.width * 4)
            .enumerate()
        {
            let start =
                ((entry.frame.origin.1 - top + row) * width + entry.frame.origin.0 - left) * 4;
            source_over(&mut pixels[start..start + source.len()], source);
        }
    }
    if opacity != 1.0 {
        for value in &mut pixels {
            *value = (f32::from(*value) * opacity).round() as u8;
        }
    }
    Ok(CroppedFrame {
        pixels,
        origin: (left, top),
        width,
    })
}

fn composite_bounds(frames: &[PreparedTextFrame]) -> [usize; 4] {
    [
        frames.iter().map(|f| f.frame.origin.0).min().unwrap(),
        frames.iter().map(|f| f.frame.origin.1).min().unwrap(),
        frames
            .iter()
            .map(|f| f.frame.origin.0 + f.frame.width)
            .max()
            .unwrap(),
        frames
            .iter()
            .map(|f| f.frame.origin.1 + f.frame.len() / (f.frame.width * 4))
            .max()
            .unwrap(),
    ]
}

/// Changed coverage includes inserts/removals and relative order changes among
/// surviving objects. Added objects do not dirty unrelated shifted list indices.
fn changed_composite_bounds(
    old: &[CompositeFrameKey],
    new: &[CompositeFrameKey],
) -> Option<[usize; 4]> {
    let old_map: HashMap<_, _> = old.iter().map(|key| (key.id.as_str(), key)).collect();
    let new_map: HashMap<_, _> = new.iter().map(|key| (key.id.as_str(), key)).collect();
    if old_map.len() != old.len() || new_map.len() != new.len() {
        return None;
    }
    let old_order: HashMap<_, _> = old
        .iter()
        .filter(|key| new_map.contains_key(key.id.as_str()))
        .enumerate()
        .map(|(index, key)| (key.id.as_str(), index))
        .collect();
    let new_order: HashMap<_, _> = new
        .iter()
        .filter(|key| old_map.contains_key(key.id.as_str()))
        .enumerate()
        .map(|(index, key)| (key.id.as_str(), index))
        .collect();
    let mut dirty: Option<[usize; 4]> = None;
    let mut include = |rect: [usize; 4]| {
        dirty = Some(match dirty {
            None => [rect[0], rect[1], rect[0] + rect[2], rect[1] + rect[3]],
            Some(d) => [
                d[0].min(rect[0]),
                d[1].min(rect[1]),
                d[2].max(rect[0] + rect[2]),
                d[3].max(rect[1] + rect[3]),
            ],
        });
    };
    for prior in old {
        if let Some(current) = new_map.get(prior.id.as_str()) {
            if prior.source == current.source
                && prior.rect == current.rect
                && old_order.get(prior.id.as_str()) == new_order.get(prior.id.as_str())
            {
                continue;
            }
            include(prior.rect);
            include(current.rect);
        } else {
            include(prior.rect);
        }
    }
    for current in new {
        if !old_map.contains_key(current.id.as_str()) {
            include(current.rect);
        }
    }
    dirty
}

fn compose_region(frames: &[PreparedTextFrame], bounds: [usize; 4], opacity: f32) -> Vec<u8> {
    let [left, top, right, bottom] = bounds;
    let width = right - left;
    let mut pixels = vec![0; width * (bottom - top) * 4];
    for entry in frames {
        let frame = &entry.frame;
        let x0 = left.max(frame.origin.0);
        let x1 = right.min(frame.origin.0 + frame.width);
        let y0 = top.max(frame.origin.1);
        let y1 = bottom.min(frame.origin.1 + frame.len() / (frame.width * 4));
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        for y in y0..y1 {
            let source = ((y - frame.origin.1) * frame.width + x0 - frame.origin.0) * 4;
            let target = ((y - top) * width + x0 - left) * 4;
            source_over(
                &mut pixels[target..target + (x1 - x0) * 4],
                &frame.pixels[source..source + (x1 - x0) * 4],
            );
        }
    }
    if opacity != 1.0 {
        for value in &mut pixels {
            *value = (f32::from(*value) * opacity).round() as u8;
        }
    }
    pixels
}

/// Update a stable-bounds composite without reprocessing unaffected pixels.
/// Shared images remain immutable. An evicted, exclusively owned buffer may be
/// recycled; otherwise copy on write before clearing/rebuilding dirty coverage.
fn update_cropped_composite(
    previous: &mut CompositeEntry,
    frames: &[PreparedTextFrame],
    opacity: f32,
    recycle: bool,
) -> Option<(Arc<CroppedFrame>, [usize; 4])> {
    let keys: Vec<_> = frames.iter().map(frame_key).collect();
    let left = keys.iter().map(|k| k.rect[0]).min()?;
    let top = keys.iter().map(|k| k.rect[1]).min()?;
    let right = keys.iter().map(|k| k.rect[0] + k.rect[2]).max()?;
    let bottom = keys.iter().map(|k| k.rect[1] + k.rect[3]).max()?;
    let old = &previous.frame;
    if old.origin != (left, top)
        || old.width != right - left
        || old.len() != (right - left) * (bottom - top) * 4
    {
        return None;
    }
    let dirty = changed_composite_bounds(&previous.frames, &keys)?;
    let _timer = crate::performance::time("text_overlap_local_composite");
    let width = old.width;
    let mut updated = if recycle {
        std::mem::replace(
            &mut previous.frame,
            Arc::new(CroppedFrame {
                pixels: Vec::new(),
                origin: (0, 0),
                width: 0,
            }),
        )
    } else {
        Arc::clone(&previous.frame)
    };
    if Arc::strong_count(&updated) == 1 {
        crate::performance::count("text_overlap_recycled_updates", 1);
    } else {
        crate::performance::count("text_overlap_local_copy_bytes", updated.len() as u64);
    }
    let pixels = &mut Arc::make_mut(&mut updated).pixels;
    for y in dirty[1]..dirty[3] {
        let start = ((y - top) * width + dirty[0] - left) * 4;
        pixels[start..start + (dirty[2] - dirty[0]) * 4].fill(0);
    }
    for entry in frames {
        let frame = &entry.frame;
        let x0 = dirty[0].max(frame.origin.0);
        let x1 = dirty[2].min(frame.origin.0 + frame.width);
        let y0 = dirty[1].max(frame.origin.1);
        let y1 = dirty[3].min(frame.origin.1 + frame.len() / (frame.width * 4));
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        for y in y0..y1 {
            let source = ((y - frame.origin.1) * frame.width + x0 - frame.origin.0) * 4;
            let destination = ((y - top) * width + x0 - left) * 4;
            source_over(
                &mut pixels[destination..destination + (x1 - x0) * 4],
                &frame.pixels[source..source + (x1 - x0) * 4],
            );
        }
    }
    if opacity != 1.0 {
        for y in dirty[1]..dirty[3] {
            let start = ((y - top) * width + dirty[0] - left) * 4;
            for value in &mut pixels[start..start + (dirty[2] - dirty[0]) * 4] {
                *value = (f32::from(*value) * opacity).round() as u8;
            }
        }
    }
    crate::performance::count("text_overlap_local_updates", 1);
    crate::performance::count(
        "text_overlap_local_dirty_bytes",
        ((dirty[2] - dirty[0]) * (dirty[3] - dirty[1]) * 4) as u64,
    );
    Some((updated, dirty))
}

impl FrameRasterCache {
    fn payload_limit(&self) -> usize {
        self.memory_limit.unwrap_or(MAX_CACHE_BYTES)
    }
    fn composite_limit(&self) -> usize {
        self.payload_limit().min(MAX_COMPOSITE_BYTES)
    }
    pub fn set_memory_limit(&mut self, bytes: usize) {
        self.memory_limit = Some(bytes.clamp(1024 * 1024, MAX_CACHE_BYTES));
        self.reserve_payload(0, self.payload_limit());
    }

    /// Prepare contained text without a document-sized CPU buffer.
    /// Disjoint frames remain independent; overlapping frames are composed in encoded
    /// RGBA on the CPU within their union bounds to preserve exact rounding.
    pub fn prepare_display_layer(
        &mut self,
        layer: &SvgLayer,
        width: u32,
        height: u32,
    ) -> Result<PreparedDisplayLayer, String> {
        let opacity = layer.effective_opacity();
        let parts = (layer.vector_objects.len() <= MAX_DISPLAY_TEXT_FRAMES
            && layer.vector_objects.iter().all(|o| {
                o.group_path.is_empty() && o.clipping_group.is_none() && o.blend_mode == "normal"
            }))
        .then(|| layer.text_frame_sources(width, height))
        .flatten();
        if let Some(parts) = parts {
            self.clock = self.clock.wrapping_add(1);
            if let Some(entry) = self.composites.iter_mut().find(|entry| {
                entry.layer_id == layer.id
                    && entry.source == layer.source
                    && entry.size == (width, height)
                    && entry.opacity == opacity
            }) {
                entry.last_used = self.clock;
                crate::performance::count("text_overlap_composite_cache_hits", 1);
                return Ok(PreparedDisplayLayer::Text(PreparedTextLayer {
                    tiled: entry.tiled.clone(),
                    patch: None,
                    independent: false,
                    opacity,
                    id: layer.id.clone(),
                    source: layer.source.clone(),
                    size: (width, height),
                    frames: vec![PreparedTextFrame {
                        id: layer.id.clone(),
                        source: layer.source.clone(),
                        frame: Arc::clone(&entry.frame),
                    }],
                }));
            }
            let mut frames: Vec<PreparedTextFrame> = Vec::with_capacity(parts.len());
            let mut compatible = true;
            for (id, source) in parts {
                let (frame, contained) = self.frame(&layer.id, &id, &source, (width, height))?;
                if !contained {
                    crate::performance::count("fallback.text_frame_outside_canvas", 1);
                    compatible = false;
                    break;
                }
                if frame.width == 0 {
                    continue;
                }
                frames.push(PreparedTextFrame { id, source, frame });
            }
            if compatible && !frames.is_empty() {
                let mut patch = None;
                let independent = !any_frames_overlap(&frames);
                if !independent {
                    let bounds = composite_bounds(&frames);
                    if (bounds[2] - bounds[0]) * (bounds[3] - bounds[1]) * 4 >= 256 * 1024 {
                        return self.prepare_tiled_overlap(
                            layer,
                            (width, height),
                            opacity,
                            frames,
                            bounds,
                        );
                    }
                    crate::performance::count("text_overlap_composite_cache_misses", 1);
                    let candidate = self
                        .composites
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| {
                            entry.layer_id == layer.id
                                && entry.size == (width, height)
                                && entry.opacity == opacity
                        })
                        .max_by_key(|(_, entry)| entry.last_used)
                        .map(|(index, entry)| (index, entry.bytes()));
                    let local = candidate.and_then(|(index, bytes)| {
                        let pressure = self.composites.len() >= MAX_COMPOSITE_ENTRIES
                            || self.composite_bytes.saturating_add(bytes) > self.composite_limit()
                            || self
                                .used_bytes
                                .saturating_add(self.composite_bytes)
                                .saturating_add(bytes)
                                > self.payload_limit();
                        if pressure && Arc::strong_count(&self.composites[index].frame) == 1 {
                            let mut old = self.composites.swap_remove(index);
                            self.composite_bytes -= old.bytes();
                            let result = update_cropped_composite(&mut old, &frames, opacity, true)
                                .map(|(frame, bounds)| {
                                    (
                                        frame,
                                        CompositePatch {
                                            base_source: old.source.clone(),
                                            bounds,
                                        },
                                    )
                                });
                            if result.is_none() {
                                self.composite_bytes += old.bytes();
                                self.composites.push(old);
                            } else {
                                self.composite_evicted_entries += 1;
                                crate::performance::count("text_overlap_recycled_cache_entries", 1);
                            }
                            result
                        } else {
                            let old = &mut self.composites[index];
                            update_cropped_composite(old, &frames, opacity, false).map(
                                |(frame, bounds)| {
                                    (
                                        frame,
                                        CompositePatch {
                                            base_source: old.source.clone(),
                                            bounds,
                                        },
                                    )
                                },
                            )
                        }
                    });
                    let frame = match local {
                        Some((frame, update)) => {
                            patch = Some(update);
                            frame
                        }
                        None => Arc::new(compose_cropped_frames(&frames, opacity)?),
                    };
                    self.retain_composite(
                        layer,
                        (width, height),
                        opacity,
                        Arc::clone(&frame),
                        frames.iter().map(frame_key).collect(),
                    );
                    frames = vec![PreparedTextFrame {
                        id: layer.id.clone(),
                        source: layer.source.clone(),
                        frame,
                    }];
                } else if opacity != 1.0 {
                    for entry in &mut frames {
                        let _timer = crate::performance::time("text_frame_opacity");
                        let mut pixels = entry.frame.pixels.clone();
                        crate::performance::count(
                            "text_frame_opacity_copy_bytes",
                            pixels.len() as u64,
                        );
                        for value in &mut pixels {
                            *value = (f32::from(*value) * opacity).round() as u8;
                        }
                        entry.frame = Arc::new(CroppedFrame {
                            pixels,
                            origin: entry.frame.origin,
                            width: entry.frame.width,
                        });
                    }
                }
                crate::performance::count("prepared_retained_text_frames", frames.len() as u64);
                return Ok(PreparedDisplayLayer::Text(PreparedTextLayer {
                    tiled: None,
                    patch,
                    independent,
                    opacity,
                    id: layer.id.clone(),
                    source: layer.source.clone(),
                    size: (width, height),
                    frames,
                }));
            }
        }
        self.prepare_layer(layer, width, height)
            .map(PreparedDisplayLayer::Full)
    }

    fn prepare_tiled_overlap(
        &mut self,
        layer: &SvgLayer,
        size: (u32, u32),
        opacity: f32,
        frames: Vec<PreparedTextFrame>,
        bounds: [usize; 4],
    ) -> Result<PreparedDisplayLayer, String> {
        let candidate = self
            .composites
            .iter()
            .filter(|entry| {
                entry.layer_id == layer.id
                    && entry.size == size
                    && entry.opacity == opacity
                    && entry.tiled.is_some()
            })
            .max_by_key(|entry| entry.last_used);
        let mut patch = None;
        let changed_bounds = candidate.and_then(|entry| {
            let image = entry.tiled.as_ref().unwrap();
            let (width, height) = image.dimensions();
            if entry.frame.origin == (bounds[0], bounds[1])
                && (width as usize, height as usize)
                    == (bounds[2] - bounds[0], bounds[3] - bounds[1])
            {
                return None;
            }
            let keys: Vec<_> = frames.iter().map(frame_key).collect();
            changed_composite_bounds(&entry.frames, &keys).map(|dirty| (entry, dirty))
        });
        let local = candidate.and_then(|entry| {
            let image = entry.tiled.as_ref().unwrap();
            let (width, height) = image.dimensions();
            if entry.frame.origin != (bounds[0], bounds[1])
                || (width as usize, height as usize)
                    != (bounds[2] - bounds[0], bounds[3] - bounds[1])
            {
                return None;
            }
            let keys: Vec<_> = frames.iter().map(frame_key).collect();
            changed_composite_bounds(&entry.frames, &keys)
                .map(|dirty| (entry.source.clone(), Arc::clone(image), dirty))
        });
        let image = if let Some((base_source, image, dirty)) = local {
            let region = crate::tiled_rgba::Region {
                x: (dirty[0] - bounds[0]) as u32,
                y: (dirty[1] - bounds[1]) as u32,
                width: (dirty[2] - dirty[0]) as u32,
                height: (dirty[3] - dirty[1]) as u32,
            };
            let (width, height) = image.dimensions();
            let (updated, stats) = image.rebuilt(width, height, [0, 0], Some(region), |tile| {
                let x = bounds[0] + tile.x as usize;
                let y = bounds[1] + tile.y as usize;
                Ok(compose_region(
                    &frames,
                    [x, y, x + tile.width as usize, y + tile.height as usize],
                    opacity,
                ))
            })?;
            crate::performance::count("text_tiled_local_updates", 1);
            crate::performance::count("text_tiled_copied_pixel_bytes", 0);
            crate::performance::count(
                "text_tiled_dirty_allocated_bytes",
                stats.rendered_pixel_bytes as u64,
            );
            patch = Some(CompositePatch {
                base_source,
                bounds: dirty,
            });
            Arc::new(updated)
        } else if let Some((entry, dirty)) = changed_bounds {
            let left = dirty[0].max(bounds[0]);
            let top = dirty[1].max(bounds[1]);
            let right = dirty[2].min(bounds[2]);
            let bottom = dirty[3].min(bounds[3]);
            let region = (left < right && top < bottom).then(|| crate::tiled_rgba::Region {
                x: (left - bounds[0]) as u32,
                y: (top - bounds[1]) as u32,
                width: (right - left) as u32,
                height: (bottom - top) as u32,
            });
            let (image, stats) = entry.tiled.as_ref().unwrap().rebuilt(
                (bounds[2] - bounds[0]) as u32,
                (bounds[3] - bounds[1]) as u32,
                [
                    entry.frame.origin.0 as i64 - bounds[0] as i64,
                    entry.frame.origin.1 as i64 - bounds[1] as i64,
                ],
                region,
                |tile| {
                    let x = bounds[0] + tile.x as usize;
                    let y = bounds[1] + tile.y as usize;
                    Ok(compose_region(
                        &frames,
                        [x, y, x + tile.width as usize, y + tile.height as usize],
                        opacity,
                    ))
                },
            )?;
            crate::performance::count("text_tiled_rebuilt_shared_tiles", stats.shared_tiles as u64);
            crate::performance::count(
                "text_tiled_rebuilt_rendered_bytes",
                stats.rendered_pixel_bytes as u64,
            );
            Arc::new(image)
        } else {
            Arc::new(crate::tiled_rgba::TiledRgba::from_regions(
                (bounds[2] - bounds[0]) as u32,
                (bounds[3] - bounds[1]) as u32,
                |region| {
                    let left = bounds[0] + region.x as usize;
                    let top = bounds[1] + region.y as usize;
                    let pixels = compose_region(
                        &frames,
                        [
                            left,
                            top,
                            left + region.width as usize,
                            top + region.height as usize,
                        ],
                        opacity,
                    );
                    crate::performance::count("text_tiled_initial_tile_bytes", pixels.len() as u64);
                    Ok(pixels)
                },
            )?)
        };
        let placeholder = Arc::new(CroppedFrame {
            pixels: Vec::new(),
            origin: (bounds[0], bounds[1]),
            width: bounds[2] - bounds[0],
        });
        self.retain_entry(CompositeEntry {
            tiled: Some(Arc::clone(&image)),
            frames: frames.iter().map(frame_key).collect(),
            layer_id: layer.id.clone(),
            source: layer.source.clone(),
            size,
            opacity,
            frame: Arc::clone(&placeholder),
            last_used: self.clock,
        });
        Ok(PreparedDisplayLayer::Text(PreparedTextLayer {
            tiled: Some(image),
            patch,
            independent: false,
            opacity,
            id: layer.id.clone(),
            source: layer.source.clone(),
            size,
            frames: vec![PreparedTextFrame {
                id: layer.id.clone(),
                source: layer.source.clone(),
                frame: placeholder,
            }],
        }))
    }

    /// One accounted retention ceiling for both CPU text-cache categories.
    fn reserve_payload(&mut self, incoming: usize, budget: usize) {
        while self
            .used_bytes
            .saturating_add(self.composite_bytes)
            .saturating_add(incoming)
            > budget
        {
            let glyph = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, entry)| (key.clone(), entry.last_used));
            let composite = self
                .composites
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(index, entry)| (index, entry.last_used));
            match (glyph, composite) {
                (Some((key, time)), Some((_, other))) if time <= other => self.evict_glyph(&key),
                (Some((key, _)), None) => self.evict_glyph(&key),
                (_, Some((index, _))) => self.evict_composite(index),
                (None, None) => break,
            }
            crate::performance::count("text_cache_shared_budget_evictions", 1);
        }
    }

    fn evict_glyph(&mut self, key: &FrameKey) {
        let entry = self.entries.remove(key).unwrap();
        self.used_bytes -= glyph_entry_bytes(key, &entry);
        self.evicted_entries += 1;
        crate::performance::count("text_glyph_cache_evictions", 1);
    }

    fn evict_composite(&mut self, index: usize) {
        let removed = self.composites.swap_remove(index);
        self.composite_bytes -= removed.bytes();
        self.composite_evicted_entries += 1;
        crate::performance::count("text_overlap_composite_cache_evictions", 1);
    }

    fn retain_composite(
        &mut self,
        layer: &SvgLayer,
        size: (u32, u32),
        opacity: f32,
        frame: Arc<CroppedFrame>,
        frames: Vec<CompositeFrameKey>,
    ) {
        self.retain_entry(CompositeEntry {
            tiled: None,
            frames,
            layer_id: layer.id.clone(),
            source: layer.source.clone(),
            size,
            opacity,
            frame,
            last_used: self.clock,
        });
    }

    fn retain_entry(&mut self, entry: CompositeEntry) {
        let bytes = entry.bytes();
        if bytes > self.composite_limit() {
            crate::performance::count("fallback.text_overlap_composite_budget", 1);
            return;
        }
        while self.composite_bytes + bytes > self.composite_limit()
            || self.composites.len() >= MAX_COMPOSITE_ENTRIES
        {
            let index = self
                .composites
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used)
                .unwrap()
                .0;
            self.evict_composite(index);
        }
        self.reserve_payload(bytes, self.payload_limit());
        self.composites.push(entry);
        self.composite_bytes += bytes;
    }

    pub fn stats(&self) -> FrameCacheStats {
        FrameCacheStats {
            total_retained_payload_bytes: self.used_bytes + self.composite_bytes,
            total_payload_budget_bytes: self.payload_limit(),
            composite_entries: self.composites.len(),
            composite_retained_payload_bytes: self.composite_bytes,
            composite_payload_budget_bytes: self.composite_limit(),
            composite_evicted_entries: self.composite_evicted_entries,
            entries: self.entries.len(),
            retained_payload_bytes: self.used_bytes,
            payload_budget_bytes: self.payload_limit(),
            evicted_entries: self.evicted_entries,
        }
    }

    pub fn prepare_layer(
        &mut self,
        layer: &SvgLayer,
        width: u32,
        height: u32,
    ) -> Result<PreparedSvgLayer, String> {
        let _timer = crate::performance::time("frame_raster_cache_prepare");
        let Some(parts) = layer.text_frame_sources(width, height) else {
            crate::performance::count("fallback.frame_sources_incompatible", 1);
            return prepare_svg_layer(layer, width, height);
        };
        let byte_count = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("Invalid text frame raster dimensions")?;
        crate::performance::count("full_canvas_allocations", 1);
        crate::performance::count("cpu_rgba_allocated_bytes", byte_count as u64);
        let mut pixels = vec![0; byte_count];
        let mut fully_contained = true;
        let opacity = layer.effective_opacity();
        let mut covered_rows = (opacity != 1.0).then(|| vec![Vec::new(); height as usize]);
        for (object_id, source) in parts {
            let (frame, contained) = self.frame(&layer.id, &object_id, &source, (width, height))?;
            frame.composite(&mut pixels, width as usize);
            if let Some(rows) = &mut covered_rows {
                if frame.width != 0 {
                    let frame_height = frame.pixels.len() / (frame.width * 4);
                    for row in &mut rows[frame.origin.1..frame.origin.1 + frame_height] {
                        row.push((frame.origin.0, frame.origin.0 + frame.width));
                    }
                }
            }
            fully_contained &= contained;
        }
        if let Some(rows) = &mut covered_rows {
            attenuate_covered_rows(&mut pixels, width as usize, opacity, rows);
        }
        Ok(PreparedSvgLayer {
            id: layer.id.clone(),
            source: layer.source.clone(),
            opacity,
            size: (width, height),
            fully_contained,
            pixels,
        })
    }

    fn frame(
        &mut self,
        layer_id: &str,
        object_id: &str,
        source: &str,
        size: (u32, u32),
    ) -> Result<(Arc<CroppedFrame>, bool), String> {
        self.clock = self.clock.wrapping_add(1);
        let hash = fingerprint(source);
        let key = (layer_id.to_owned(), object_id.to_owned(), hash, size);
        if let Some(entry) = self.entries.get_mut(&key) {
            if entry.source == source && entry.size == size {
                entry.last_used = self.clock;
                self.reused_frames += 1;
                crate::performance::count("frame_cache_hits", 1);
                return Ok((Arc::clone(&entry.pixels), entry.fully_contained));
            }
        }
        if let Some(old) = self.entries.remove(&key) {
            self.used_bytes -= glyph_entry_bytes(&key, &old);
        }
        crate::performance::count("frame_cache_misses", 1);
        crate::performance::count("text_raster_fallbacks", 1);
        let region = vector::rasterize_svg_cropped(source, size.0, size.1)?;
        let fully_contained = region.raster.fully_contained;
        self.rasterized_frames += 1;
        let region_height = if region.width == 0 {
            0
        } else {
            region.raster.pixels.len() / (region.width * 4)
        };
        let mut frame = CroppedFrame::from_pixels(
            region.raster.pixels,
            region.width as u32,
            region_height as u32,
        );
        frame.origin.0 += region.origin.0;
        frame.origin.1 += region.origin.1;
        let pixels = Arc::new(frame);
        let entry_bytes = pixels
            .len()
            .saturating_add(source.len())
            .saturating_add(key.0.len())
            .saturating_add(key.1.len());
        if entry_bytes <= self.payload_limit() {
            while self.entries.len() >= MAX_CACHE_ENTRIES {
                let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(key, _)| key.clone())
                else {
                    break;
                };
                self.evict_glyph(&oldest);
            }
            self.reserve_payload(entry_bytes, self.payload_limit());
            self.used_bytes += entry_bytes;
            self.entries.insert(
                key,
                Entry {
                    source: source.to_owned(),
                    size,
                    pixels: Arc::clone(&pixels),
                    fully_contained,
                    last_used: self.clock,
                },
            );
        }
        Ok((pixels, fully_contained))
    }
}

fn any_frames_overlap(frames: &[PreparedTextFrame]) -> bool {
    let _timer = crate::performance::time("text_overlap_query");
    if frames.len() <= 64 {
        return frames.iter().enumerate().any(|(i, frame)| {
            frames[..i]
                .iter()
                .any(|other| frames_overlap(&frame.frame, &other.frame))
        });
    }
    let bounds = |frame: &CroppedFrame| {
        [
            frame.origin.0 as f64,
            frame.origin.1 as f64,
            (frame.origin.0 + frame.width) as f64,
            (frame.origin.1 + frame.pixels.len() / (frame.width * 4)) as f64,
        ]
    };
    let index = lumapaint_core::scene::spatial::SpatialIndex::build(
        frames
            .iter()
            .enumerate()
            .map(|(i, frame)| (i, Some(bounds(&frame.frame)))),
    );
    let mut items = Vec::new();
    let mut stack = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        let visited = index.query_into(bounds(&frame.frame), &mut items, &mut stack);
        crate::performance::count("text_overlap_visited_nodes", visited as u64);
        if items
            .iter()
            .any(|&j| j < i && frames_overlap(&frame.frame, &frames[j].frame))
        {
            return true;
        }
    }
    false
}

fn frames_overlap(a: &CroppedFrame, b: &CroppedFrame) -> bool {
    let height = |frame: &CroppedFrame| frame.pixels.len() / (frame.width * 4);
    a.origin.0 < b.origin.0 + b.width
        && b.origin.0 < a.origin.0 + a.width
        && a.origin.1 < b.origin.1 + height(b)
        && b.origin.1 < a.origin.1 + height(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::{Document, TextSettings};
    use lumapaint_core::vector::VectorText;

    fn text(content: &str, y: f32) -> TextSettings {
        TextSettings {
            id: None,
            text: VectorText {
                content: content.into(),
                font_size: 32.0,
                box_width: 500.0,
                box_height: Some(80.0),
                ..Default::default()
            },
            position: [20.0, y],
            color: [0, 0, 0],
        }
    }

    #[test]
    fn indexed_frame_overlap_matches_pairwise_including_touching_edges() {
        let mut frames: Vec<_> = (0..4096)
            .map(|i| PreparedTextFrame {
                id: i.to_string(),
                source: String::new(),
                frame: Arc::new(CroppedFrame {
                    pixels: vec![0; 4],
                    origin: (i % 64, i / 64),
                    width: 1,
                }),
            })
            .collect();
        assert!(
            !any_frames_overlap(&frames),
            "touching edges are independent"
        );
        frames[4095].frame = Arc::clone(&frames[100].frame);
        assert!(any_frames_overlap(&frames));
        for count in [0, 1, 64, 65, 100, 4096] {
            let slice = &frames[..count];
            let expected = slice.iter().enumerate().any(|(i, frame)| {
                slice[..i]
                    .iter()
                    .any(|other| frames_overlap(&frame.frame, &other.frame))
            });
            assert_eq!(any_frames_overlap(slice), expected);
        }
    }

    #[test]
    fn one_thousand_text_frames_use_cropped_display_and_reuse_rasters() {
        let mut document = Document::default();
        let mut settings = text("A", 20.);
        settings.text.font_size = 8.;
        document.set_text_object(settings).unwrap();
        let mut state = document.document_state();
        let prototype = state.svg_layers[0].vector_objects[0].clone();
        state.svg_layers[0].vector_objects = (0..1000)
            .map(|i| {
                let mut object = prototype.clone();
                object.id = format!("text-{i}");
                object.transform[4] += (i % 25) as f32 * 36.;
                object.transform[5] += (i / 25) as f32 * 13.;
                object
            })
            .collect();
        state.svg_layers[0].source = lumapaint_core::document::vector_svg(
            state.width,
            state.height,
            &state.svg_layers[0].vector_objects,
        );
        let document = Document::from_document_state(state).unwrap();
        let layer = document.svg_layers().next().unwrap();
        let (width, height) = document.dimensions();
        let mut cache = FrameRasterCache::default();
        let PreparedDisplayLayer::Text(prepared) =
            cache.prepare_display_layer(layer, width, height).unwrap()
        else {
            panic!("expected retained text")
        };
        assert!(prepared.independent);
        assert_eq!(prepared.frames.len(), 1000);
        assert_eq!(
            prepared.into_full().pixels,
            prepare_svg_layer(layer, width, height).unwrap().pixels
        );
        assert_eq!(cache.rasterized_frames, 1000);
        cache.prepare_display_layer(layer, width, height).unwrap();
        assert_eq!(cache.rasterized_frames, 1000);
        assert_eq!(cache.reused_frames, 1000);
        let mut edited = layer.clone();
        edited.vector_objects[500].fill.as_mut().unwrap().color = [200, 40, 80, 255];
        edited.source = lumapaint_core::document::vector_svg(width, height, &edited.vector_objects);
        crate::performance::take();
        let PreparedDisplayLayer::Text(changed) =
            cache.prepare_display_layer(&edited, width, height).unwrap()
        else {
            panic!("expected retained text after edit")
        };
        let metrics = crate::performance::take();
        assert!(!metrics.counts.contains_key("full_canvas_allocations"));
        assert_eq!(cache.rasterized_frames, 1001);
        assert_eq!(cache.reused_frames, 1999);
        assert_eq!(
            changed.into_full().pixels,
            prepare_svg_layer(&edited, width, height).unwrap().pixels
        );
        assert!(cache.stats().total_retained_payload_bytes <= MAX_CACHE_BYTES);
    }

    #[test]
    fn display_frames_match_full_pixels_and_retain_unchanged_frame_after_edit() {
        let mut document = Document::default();
        document.set_text_object(text("First 日本語", 50.)).unwrap();
        let layer_id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(layer_id).unwrap();
        document
            .set_text_object(text("Second 简体中文", 200.))
            .unwrap();
        let mut cache = FrameRasterCache::default();
        let layer = document.svg_layers().next().unwrap();
        let PreparedDisplayLayer::Text(first) =
            cache.prepare_display_layer(layer, 960, 640).unwrap()
        else {
            panic!("disjoint frames must use retained display")
        };
        let unchanged = Arc::clone(&first.frames[1].frame);
        assert!(
            first
                .frames
                .iter()
                .map(|frame| frame.frame.len())
                .sum::<usize>()
                < 960 * 640 * 4 / 8
        );
        let original = first.into_full().pixels;
        assert_eq!(original, prepare_svg_layer(layer, 960, 640).unwrap().pixels);
        let first = document.snapshot().text_objects[0].clone();
        document
            .set_text_object(TextSettings {
                id: Some(first.id),
                text: VectorText {
                    content: "Edited English 日本語".into(),
                    ..first.text
                },
                position: first.position,
                color: first.color,
            })
            .unwrap();
        let layer = document.svg_layers().next().unwrap();
        let PreparedDisplayLayer::Text(edited) =
            cache.prepare_display_layer(layer, 960, 640).unwrap()
        else {
            panic!("edited frames must use retained display")
        };
        assert!(Arc::ptr_eq(&unchanged, &edited.frames[1].frame));
        assert_eq!(cache.rasterized_frames, 3);
        let edited_pixels = edited.into_full().pixels;
        assert_eq!(
            edited_pixels,
            prepare_svg_layer(layer, 960, 640).unwrap().pixels
        );
        document.undo();
        let PreparedDisplayLayer::Text(restored) = cache
            .prepare_display_layer(document.svg_layers().next().unwrap(), 960, 640)
            .unwrap()
        else {
            panic!("undo must retain independent frames")
        };
        assert_eq!(restored.into_full().pixels, original);
        document.redo();
        let PreparedDisplayLayer::Text(restored) = cache
            .prepare_display_layer(document.svg_layers().next().unwrap(), 960, 640)
            .unwrap()
        else {
            panic!("redo must retain independent frames")
        };
        assert_eq!(restored.into_full().pixels, edited_pixels);
        assert_eq!(cache.rasterized_frames, 3);
    }

    #[test]
    fn opacity_intervals_merge_overlap_and_skip_uncovered_pixels() {
        let original = [12, 30, 90, 200].repeat(8 * 4);
        let mut pixels = vec![0; original.len()];
        let mut rows = vec![
            vec![(4, 6), (1, 4), (2, 5), (6, 7)],
            vec![],
            vec![(0, 1)],
            vec![],
        ];
        pixels[4..28].copy_from_slice(&original[4..28]);
        pixels[64..68].copy_from_slice(&original[64..68]);
        let mut reference = pixels.clone();
        for value in &mut reference {
            *value = (f32::from(*value) * 0.37).round() as u8;
        }
        assert_eq!(
            attenuate_covered_rows(&mut pixels, 8, 0.37, &mut rows),
            7 * 4
        );
        assert_eq!(pixels, reference);
    }

    #[test]
    #[ignore = "manual CPU opacity timing; not application latency"]
    fn covered_opacity_timing() {
        use std::time::Instant;
        let width = 2048;
        let mut pixels = vec![0; width * width * 4];
        let mut rows = vec![Vec::new(); width];
        for y in 80..160 {
            rows[y] = vec![(80, 580), (400, 800)];
            pixels[(y * width + 80) * 4..(y * width + 800) * 4].fill(120);
        }
        let mut full_times = Vec::new();
        let mut covered_times = Vec::new();
        for sample in 0..21 {
            let mut full = pixels.clone();
            let mut covered = pixels.clone();
            let mut intervals = rows.clone();
            let start = Instant::now();
            for value in &mut full {
                *value = (f32::from(*value) * 0.5).round() as u8;
            }
            let full_time = start.elapsed();
            let start = Instant::now();
            let count = attenuate_covered_rows(&mut covered, width, 0.5, &mut intervals);
            let covered_time = start.elapsed();
            assert_eq!(full, covered);
            assert_eq!(count, 80 * 720 * 4);
            if sample != 0 {
                full_times.push(full_time);
                covered_times.push(covered_time);
            }
        }
        full_times.sort();
        covered_times.sort();
        eprintln!("opacity CPU only: full_bytes={} covered_bytes={} samples=20 full_median_us={} covered_median_us={}", pixels.len(), 80*720*4, full_times[10].as_micros(), covered_times[10].as_micros());
    }

    #[test]
    fn source_over_fast_paths_preserve_integer_reference_for_every_alpha() {
        for alpha in 0..=255u32 {
            let source = [
                alpha as u8 / 2,
                alpha as u8 / 3,
                alpha as u8 / 4,
                alpha as u8,
            ];
            for destination_alpha in 0..=255u32 {
                let mut actual = [
                    destination_alpha as u8 / 2,
                    destination_alpha as u8 / 3,
                    0,
                    destination_alpha as u8,
                ];
                let mut expected = actual;
                for channel in 0..4 {
                    expected[channel] = (u32::from(source[channel])
                        + (u32::from(expected[channel]) * (255 - alpha) + 127) / 255)
                        .min(255) as u8;
                }
                source_over(&mut actual, &source);
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn faded_disjoint_frames_match_full_and_reuse_base_rasters() {
        let mut document = Document::default();
        document
            .set_text_object(text("English 日本語", 50.))
            .unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(id).unwrap();
        document.set_text_object(text("简体中文", 200.)).unwrap();
        let mut cache = FrameRasterCache::default();
        for opacity in [1.0, 0.5, 0.01, 0.0, 0.75, 1.0] {
            let mut layer = document.svg_layers().next().unwrap().clone();
            layer.opacity = opacity;
            let PreparedDisplayLayer::Text(prepared) =
                cache.prepare_display_layer(&layer, 960, 640).unwrap()
            else {
                panic!("disjoint translucent text should retain cropped preparation")
            };
            assert_eq!(
                prepared.into_full().pixels,
                prepare_svg_layer(&layer, 960, 640).unwrap().pixels
            );
            assert_eq!(
                cache.rasterized_frames, 2,
                "opacity must reuse base glyph rasters"
            );
        }
    }

    #[test]
    fn overlapping_and_faded_display_layers_use_exact_cropped_compositing() {
        let mut document = Document::default();
        document.set_text_object(text("Overlap", 50.)).unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(id).unwrap();
        let mut second = text("Overlap", 50.);
        second.color = [180, 30, 70];
        document.set_text_object(second).unwrap();
        let mut cache = FrameRasterCache::default();
        let layer = document.svg_layers().next().unwrap();
        for opacity in [1.0, 0.5, 0.01, 0.0, 0.75] {
            let mut layer = layer.clone();
            layer.opacity = opacity;
            let PreparedDisplayLayer::Text(full) =
                cache.prepare_display_layer(&layer, 960, 640).unwrap()
            else {
                panic!("overlap should use cropped encoded composition")
            };
            assert!(!full.independent);
            assert!(full.frames[0].frame.len() < 960 * 640 * 4 / 8);
            eprintln!(
                "overlap cropped opacity={opacity} payload={} full={}",
                full.frames[0].frame.len(),
                960 * 640 * 4
            );
            assert_eq!(
                full.into_full().pixels,
                prepare_svg_layer(&layer, 960, 640).unwrap().pixels
            );
            assert_eq!(cache.rasterized_frames, 2);
        }
    }

    #[test]
    fn large_overlapping_text_uses_tiles_for_shared_edit_and_undo_redo() {
        let mut state = Document::default().document_state();
        state.width = 2048;
        state.height = 2048;
        let mut document = Document::from_document_state(state).unwrap();
        let mut large = text(&"ABCDEFG 日本語 简体中文\n".repeat(8), 80.);
        large.text.font_size = 64.;
        large.text.box_width = 1200.;
        large.text.box_height = Some(900.);
        document.set_text_object(large).unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(id).unwrap();
        document
            .set_text_object(text("Small 日本語", 150.))
            .unwrap();
        let mut cache = FrameRasterCache::default();
        let PreparedDisplayLayer::Text(base) = cache
            .prepare_display_layer(document.svg_layers().next().unwrap(), 2048, 2048)
            .unwrap()
        else {
            panic!("expected text");
        };
        let original = Arc::clone(base.tiled.as_ref().expect("large overlap must use tiles"));
        let original_pixels = original.materialize();
        let original_right = base.frames[0].frame.origin.0 + original.dimensions().0 as usize;
        assert_eq!(
            base.into_full().pixels,
            prepare_svg_layer(document.svg_layers().next().unwrap(), 2048, 2048)
                .unwrap()
                .pixels
        );
        let small = document.snapshot().text_objects[1].clone();
        document
            .set_text_object(TextSettings {
                id: Some(small.id),
                text: small.text,
                position: small.position,
                color: [160, 30, 70],
            })
            .unwrap();
        if crate::performance::enabled() {
            crate::performance::take();
        }
        let layer = document.svg_layers().next().unwrap();
        let PreparedDisplayLayer::Text(updated) =
            cache.prepare_display_layer(layer, 2048, 2048).unwrap()
        else {
            panic!("expected text");
        };
        if crate::performance::enabled() {
            let metrics = crate::performance::take();
            assert_eq!(metrics.counts.get("text_tiled_local_updates"), Some(&1));
            assert!(
                metrics.counts["text_tiled_copied_pixel_bytes"]
                    < original.payload_bytes() as u64 / 2
            );
            assert!(
                metrics.counts["text_tiled_dirty_allocated_bytes"]
                    < original.payload_bytes() as u64 / 2
            );
            assert!(!metrics.counts.contains_key("text_tiled_materialized_bytes"));
            eprintln!(
                "tiled overlap edit payload={} metrics={metrics:?}",
                original.payload_bytes()
            );
        }
        assert!(updated.patch.is_some());
        let edited = Arc::clone(updated.tiled.as_ref().unwrap());
        assert_eq!(original.materialize(), original_pixels);
        assert_eq!(
            updated.into_full().pixels,
            prepare_svg_layer(layer, 2048, 2048).unwrap().pixels
        );
        document.undo();
        let PreparedDisplayLayer::Text(restored) = cache
            .prepare_display_layer(document.svg_layers().next().unwrap(), 2048, 2048)
            .unwrap()
        else {
            panic!("expected text");
        };
        assert!(Arc::ptr_eq(restored.tiled.as_ref().unwrap(), &original));
        assert_eq!(
            restored.into_full().pixels,
            prepare_svg_layer(document.svg_layers().next().unwrap(), 2048, 2048)
                .unwrap()
                .pixels
        );
        document.redo();
        let PreparedDisplayLayer::Text(redone) = cache
            .prepare_display_layer(document.svg_layers().next().unwrap(), 2048, 2048)
            .unwrap()
        else {
            panic!("expected text");
        };
        assert!(Arc::ptr_eq(redone.tiled.as_ref().unwrap(), &edited));
        assert_eq!(
            redone.into_full().pixels,
            prepare_svg_layer(document.svg_layers().next().unwrap(), 2048, 2048)
                .unwrap()
                .pixels
        );
        let mut faded = document.svg_layers().next().unwrap().clone();
        faded.opacity = 0.5;
        let PreparedDisplayLayer::Text(faded_result) =
            cache.prepare_display_layer(&faded, 2048, 2048).unwrap()
        else {
            panic!("expected text");
        };
        assert!(faded_result.tiled.is_some() && faded_result.patch.is_none());
        assert_eq!(
            faded_result.into_full().pixels,
            prepare_svg_layer(&faded, 2048, 2048).unwrap().pixels
        );
        let small = document.snapshot().text_objects[1].clone();
        document
            .set_text_object(TextSettings {
                id: Some(small.id),
                text: small.text,
                position: [(original_right - 80) as f32, 150.],
                color: small.color,
            })
            .unwrap();
        let moved = document.svg_layers().next().unwrap();
        let PreparedDisplayLayer::Text(moved_result) =
            cache.prepare_display_layer(moved, 2048, 2048).unwrap()
        else {
            panic!("expected text");
        };
        assert!(moved_result.tiled.is_some() && moved_result.patch.is_none());
        assert_eq!(
            moved_result.into_full().pixels,
            prepare_svg_layer(moved, 2048, 2048).unwrap().pixels
        );
        assert_eq!(
            cache.composite_bytes,
            cache
                .composites
                .iter()
                .map(CompositeEntry::bytes)
                .sum::<usize>()
        );
        assert!(cache.stats().total_retained_payload_bytes <= MAX_CACHE_BYTES);
    }

    #[test]
    fn patch_rows_borrows_subrectangle_with_stride_and_exact_final_row() {
        let frame = CroppedFrame {
            pixels: (0..7 * 5 * 4).map(|i| i as u8).collect(),
            origin: (10, 20),
            width: 7,
        };
        let rows = patch_rows(&frame, [12, 21, 15, 24]).unwrap();
        assert_eq!(rows.stride, 28);
        assert_eq!(rows.origin, [12, 21]);
        assert_eq!(rows.size, [3, 3]);
        assert_eq!(rows.pixels.as_ptr(), frame.pixels[36..].as_ptr());
        assert_eq!(rows.pixels.len(), 28 * 2 + 12);
        for y in 0..3 {
            assert_eq!(
                &rows.pixels[y * 28..y * 28 + 12],
                &frame.pixels[36 + y * 28..48 + y * 28]
            );
        }
        let single = patch_rows(&frame, [10, 24, 17, 25]).unwrap();
        assert_eq!(single.pixels.len(), 28);
        for bounds in [
            [12, 21, 12, 24],
            [9, 21, 15, 24],
            [12, 19, 15, 24],
            [12, 21, 18, 24],
            [12, 21, 15, 26],
        ] {
            assert!(patch_rows(&frame, bounds).is_err());
        }
        let malformed = CroppedFrame {
            pixels: vec![0; 3],
            origin: (0, 0),
            width: 1,
        };
        assert!(patch_rows(&malformed, [0, 0, 1, 1]).is_err());
    }

    #[test]
    fn recyclable_local_images_keep_allocation_and_shared_images_stay_immutable() {
        let make = |source: &str, color| PreparedTextFrame {
            id: "object".into(),
            source: source.into(),
            frame: Arc::new(CroppedFrame {
                pixels: vec![color; 16],
                origin: (2, 2),
                width: 2,
            }),
        };
        for shared in [false, true] {
            let old = vec![make("old", 20)];
            let new = vec![make("new", 30)];
            let image = Arc::new(compose_cropped_frames(&old, 0.5).unwrap());
            let address = image.pixels.as_ptr();
            let saved = image.pixels.clone();
            let external = shared.then(|| Arc::clone(&image));
            let mut previous = CompositeEntry {
                tiled: None,
                frames: old.iter().map(frame_key).collect(),
                layer_id: "layer".into(),
                source: "old".into(),
                size: (960, 640),
                opacity: 0.5,
                frame: image,
                last_used: 0,
            };
            let (result, _) = update_cropped_composite(&mut previous, &new, 0.5, true).unwrap();
            assert_eq!(
                result.pixels,
                compose_cropped_frames(&new, 0.5).unwrap().pixels
            );
            if let Some(external) = external {
                assert_eq!(external.pixels, saved);
                assert_ne!(result.pixels.as_ptr(), address);
            } else {
                assert_eq!(
                    result.pixels.as_ptr(),
                    address,
                    "exclusive buffer should keep its allocation"
                );
            }
        }
    }

    #[test]
    fn composite_entry_pressure_recycles_exclusive_cached_pixels() {
        let mut document = Document::default();
        document
            .set_text_object(text("English 日本語", 50.))
            .unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(id).unwrap();
        document.set_text_object(text("简体中文", 55.)).unwrap();
        let mut cache = FrameRasterCache::default();
        {
            let prepared = cache
                .prepare_display_layer(document.svg_layers().next().unwrap(), 960, 640)
                .unwrap();
            drop(prepared);
        }
        let address = cache.composites[0].frame.pixels.as_ptr();
        for index in 1..MAX_COMPOSITE_ENTRIES {
            let dummy = CompositeEntry {
                tiled: None,
                frames: vec![],
                layer_id: format!("dummy{index}"),
                source: "dummy".into(),
                size: (1, 1),
                opacity: 1.0,
                frame: Arc::new(CroppedFrame {
                    pixels: vec![0; 4],
                    origin: (0, 0),
                    width: 1,
                }),
                last_used: 0,
            };
            cache.composite_bytes += dummy.bytes();
            cache.composites.push(dummy);
        }
        let first = document.snapshot().text_objects[0].clone();
        document
            .set_text_object(TextSettings {
                id: Some(first.id),
                text: first.text,
                position: first.position,
                color: [160, 30, 70],
            })
            .unwrap();
        let layer = document.svg_layers().next().unwrap();
        let PreparedDisplayLayer::Text(prepared) =
            cache.prepare_display_layer(layer, 960, 640).unwrap()
        else {
            panic!("expected overlap");
        };
        assert!(prepared.patch.is_some());
        assert_eq!(prepared.frames[0].frame.pixels.as_ptr(), address);
        assert_eq!(
            prepared.into_full().pixels,
            prepare_svg_layer(layer, 960, 640).unwrap().pixels
        );
        assert_eq!(cache.composite_evicted_entries, 1);
        assert_eq!(
            cache.composite_bytes,
            cache
                .composites
                .iter()
                .map(CompositeEntry::bytes)
                .sum::<usize>()
        );
    }

    #[test]
    fn structural_overlap_updates_only_changed_coverage_for_small_and_tiled_images() {
        let make = |id: &str, x: usize, width: usize, color: [u8; 4]| PreparedTextFrame {
            id: id.into(),
            source: id.into(),
            frame: Arc::new(CroppedFrame {
                pixels: color.repeat(width * 256),
                origin: (x, 0),
                width,
            }),
        };
        let old = vec![
            make("anchor", 0, 512, [40, 20, 10, 100]),
            make("front", 40, 16, [30, 80, 10, 128]),
        ];
        let mut inserted = old.clone();
        inserted.insert(1, make("inserted", 20, 16, [80, 10, 30, 128]));
        let keys = |frames: &[PreparedTextFrame]| frames.iter().map(frame_key).collect::<Vec<_>>();
        assert_eq!(
            changed_composite_bounds(&keys(&old), &keys(&inserted)),
            Some([20, 0, 36, 256])
        );
        assert_eq!(
            changed_composite_bounds(&keys(&inserted), &keys(&old)),
            Some([20, 0, 36, 256])
        );
        assert!(changed_composite_bounds(&keys(&old), &keys(&old)).is_none());
        let duplicate = vec![old[0].clone(), old[0].clone()];
        assert!(changed_composite_bounds(&keys(&duplicate), &keys(&old)).is_none());
        let mut reordered = inserted.clone();
        reordered.swap(1, 2);
        assert_eq!(
            changed_composite_bounds(&keys(&inserted), &keys(&reordered)),
            Some([20, 0, 56, 256])
        );
        let mut document = Document::default();
        document.set_text_object(text("Anchor", 50.)).unwrap();
        let mut layer = document.svg_layers().next().unwrap().clone();
        for opacity in [0.0, 0.01, 0.5, 1.0] {
            let mut cache = FrameRasterCache::default();
            let mut prior_frames = old.clone();
            layer.source = "base".into();
            let PreparedDisplayLayer::Text(base) = cache
                .prepare_tiled_overlap(&layer, (960, 640), opacity, old.clone(), [0, 0, 512, 256])
                .unwrap()
            else {
                panic!("expected tiled text")
            };
            let original = Arc::clone(base.tiled.as_ref().unwrap());
            let saved = original.materialize();
            for (index, current) in [inserted.clone(), reordered.clone(), old.clone()]
                .into_iter()
                .enumerate()
            {
                let prior = compose_cropped_frames(&prior_frames, opacity).unwrap();
                let mut entry = CompositeEntry {
                    tiled: None,
                    frames: keys(&prior_frames),
                    layer_id: layer.id.clone(),
                    source: layer.source.clone(),
                    size: (960, 640),
                    opacity,
                    frame: Arc::new(prior),
                    last_used: 0,
                };
                let local = update_cropped_composite(&mut entry, &current, opacity, false).unwrap();
                let expected = compose_cropped_frames(&current, opacity).unwrap().pixels;
                assert_eq!(local.0.pixels, expected);
                let previous_source = layer.source.clone();
                layer.source = format!("structural-{index}");
                let PreparedDisplayLayer::Text(updated) = cache
                    .prepare_tiled_overlap(
                        &layer,
                        (960, 640),
                        opacity,
                        current.clone(),
                        [0, 0, 512, 256],
                    )
                    .unwrap()
                else {
                    panic!("expected tiled text")
                };
                assert_eq!(updated.patch.as_ref().unwrap().base_source, previous_source);
                assert_eq!(updated.tiled.as_ref().unwrap().materialize(), expected);
                assert_eq!(original.materialize(), saved);
                prior_frames = current;
            }
            let mut extended = old.clone();
            extended.push(make("extension", 500, 140, [80, 30, 10, 128]));
            layer.source = "extended".into();
            crate::performance::take();
            let PreparedDisplayLayer::Text(expanded) = cache
                .prepare_tiled_overlap(
                    &layer,
                    (960, 640),
                    opacity,
                    extended.clone(),
                    [0, 0, 640, 256],
                )
                .unwrap()
            else {
                panic!("expected tiled text")
            };
            if crate::performance::enabled() {
                let metrics = crate::performance::take();
                assert!(
                    *metrics
                        .counts
                        .get("text_tiled_rebuilt_shared_tiles")
                        .unwrap()
                        >= 6
                );
                assert!(!metrics.counts.contains_key("text_tiled_materialized_bytes"));
            }
            assert!(expanded.patch.is_none());
            assert_eq!(
                expanded.tiled.as_ref().unwrap().materialize(),
                compose_cropped_frames(&extended, opacity).unwrap().pixels
            );
            assert_eq!(original.materialize(), saved);
            layer.source = "contracted".into();
            let PreparedDisplayLayer::Text(contracted) = cache
                .prepare_tiled_overlap(&layer, (960, 640), opacity, old.clone(), [0, 0, 512, 256])
                .unwrap()
            else {
                panic!("expected tiled text")
            };
            assert_eq!(contracted.tiled.as_ref().unwrap().materialize(), saved);
        }
    }

    #[test]
    fn local_overlap_rebuilds_dirty_coverage_in_order_and_preserves_old_image() {
        let make =
            |id: &str, source: &str, x: usize, width: usize, color: [u8; 4]| PreparedTextFrame {
                id: id.into(),
                source: source.into(),
                frame: Arc::new(CroppedFrame {
                    pixels: color.repeat(width * 2),
                    origin: (x, 2),
                    width,
                }),
            };
        for opacity in [0.0, 0.01, 0.5, 1.0] {
            let old = vec![
                make("background", "base", 0, 20, [40, 20, 10, 100]),
                make("edited", "old", 2, 4, [60, 10, 20, 128]),
                make("front", "foreground", 4, 4, [30, 80, 10, 128]),
            ];
            let original = Arc::new(compose_cropped_frames(&old, opacity).unwrap());
            let saved = original.pixels.clone();
            let mut previous = CompositeEntry {
                tiled: None,
                frames: old.iter().map(frame_key).collect(),
                layer_id: "layer".into(),
                source: "old".into(),
                size: (960, 640),
                opacity,
                frame: original,
                last_used: 0,
            };
            let changed = vec![
                make("background", "base", 0, 20, [40, 20, 10, 100]),
                make("edited", "new", 4, 4, [10, 60, 40, 128]),
                make("front", "foreground", 4, 4, [30, 80, 10, 128]),
            ];
            let local = update_cropped_composite(&mut previous, &changed, opacity, false)
                .expect("stable bounds should update locally");
            let full = compose_cropped_frames(&changed, opacity).unwrap();
            assert_eq!(local.0.pixels, full.pixels);
            assert_eq!(local.1, [2, 2, 8, 4]);
            assert_eq!(previous.frame.pixels, saved);
            let mut reordered = changed;
            reordered.swap(0, 1);
            let reordered_result =
                update_cropped_composite(&mut previous, &reordered, opacity, false).unwrap();
            assert_eq!(
                reordered_result.0.pixels,
                compose_cropped_frames(&reordered, opacity).unwrap().pixels
            );
            let removed_result =
                update_cropped_composite(&mut previous, &reordered[..2], opacity, false).unwrap();
            assert_eq!(
                removed_result.0.pixels,
                compose_cropped_frames(&reordered[..2], opacity)
                    .unwrap()
                    .pixels
            );
            let larger = vec![
                make("background", "base", 0, 24, [40, 20, 10, 100]),
                make("edited", "new", 4, 4, [10, 60, 40, 128]),
                make("front", "foreground", 4, 4, [30, 80, 10, 128]),
            ];
            assert!(update_cropped_composite(&mut previous, &larger, opacity, false).is_none());
        }
    }

    #[test]
    fn overlap_patch_metadata_matches_base_and_full_result_after_color_edit() {
        let mut document = Document::default();
        document
            .set_text_object(text("English 日本語", 50.))
            .unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(id).unwrap();
        document.set_text_object(text("简体中文", 55.)).unwrap();
        let mut cache = FrameRasterCache::default();
        let layer = document.svg_layers().next().unwrap();
        let base_source = layer.source.clone();
        let PreparedDisplayLayer::Text(base) =
            cache.prepare_display_layer(layer, 960, 640).unwrap()
        else {
            panic!("expected overlap");
        };
        assert!(base.patch.is_none());
        let first = document.snapshot().text_objects[0].clone();
        document
            .set_text_object(TextSettings {
                id: Some(first.id),
                text: first.text,
                position: first.position,
                color: [160, 30, 70],
            })
            .unwrap();
        let layer = document.svg_layers().next().unwrap();
        let PreparedDisplayLayer::Text(edited) =
            cache.prepare_display_layer(layer, 960, 640).unwrap()
        else {
            panic!("expected overlap");
        };
        let patch = edited
            .patch
            .as_ref()
            .expect("stable coverage should emit patch");
        assert_eq!(patch.base_source, base_source);
        let [left, top, right, bottom] = patch.bounds;
        let frame = &edited.frames[0].frame;
        assert!(left >= frame.origin.0 && top >= frame.origin.1);
        assert!(
            right <= frame.origin.0 + frame.width
                && bottom <= frame.origin.1 + frame.len() / (frame.width * 4)
        );
        assert_eq!(
            edited.into_full().pixels,
            prepare_svg_layer(layer, 960, 640).unwrap().pixels
        );
        let mut faded = layer.clone();
        faded.opacity = 0.5;
        let PreparedDisplayLayer::Text(faded) =
            cache.prepare_display_layer(&faded, 960, 640).unwrap()
        else {
            panic!("expected overlap");
        };
        assert!(
            faded.patch.is_none(),
            "opacity change must initialize full GPU target"
        );
    }

    #[test]
    fn overlapping_composite_reuses_arc_and_invalidates_opacity_and_dimensions() {
        let mut document = Document::default();
        document
            .set_text_object(text("English 日本語 简体中文", 50.))
            .unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(id).unwrap();
        document.set_text_object(text("Overlap", 55.)).unwrap();
        let layer = document.svg_layers().next().unwrap();
        let mut cache = FrameRasterCache::default();
        let prepare = |cache: &mut FrameRasterCache, layer: &SvgLayer, size: (u32, u32)| {
            let PreparedDisplayLayer::Text(prepared) =
                cache.prepare_display_layer(layer, size.0, size.1).unwrap()
            else {
                panic!("expected cropped text");
            };
            assert!(!prepared.independent);
            let image = Arc::clone(&prepared.frames[0].frame);
            assert_eq!(
                prepared.into_full().pixels,
                prepare_svg_layer(layer, size.0, size.1).unwrap().pixels
            );
            image
        };
        let original = prepare(&mut cache, layer, (960, 640));
        let repeated = prepare(&mut cache, layer, (960, 640));
        assert!(Arc::ptr_eq(&original, &repeated));
        let mut faded = layer.clone();
        faded.opacity = 0.5;
        let different = prepare(&mut cache, &faded, (960, 640));
        assert!(!Arc::ptr_eq(&original, &different));
        assert!(Arc::ptr_eq(
            &original,
            &prepare(&mut cache, layer, (960, 640))
        ));
        let PreparedDisplayLayer::Full(resized) =
            cache.prepare_display_layer(layer, 1024, 768).unwrap()
        else {
            panic!("changed SVG viewport should use compatibility preparation");
        };
        assert_eq!(
            resized.pixels,
            prepare_svg_layer(layer, 1024, 768).unwrap().pixels
        );
        assert_eq!(cache.stats().composite_entries, 2);
        assert!(cache.stats().composite_retained_payload_bytes <= MAX_COMPOSITE_BYTES);
    }

    #[test]
    fn shared_payload_budget_evicts_oldest_across_glyphs_and_composites() {
        for composite_is_oldest in [false, true] {
            let mut cache = FrameRasterCache::default();
            let pixels = Arc::new(CroppedFrame {
                pixels: vec![30; 16],
                origin: (0, 0),
                width: 2,
            });
            let key = ("layer".into(), "object".into(), 0, (960, 640));
            let glyph = Entry {
                source: "glyph".into(),
                size: (960, 640),
                pixels: Arc::clone(&pixels),
                fully_contained: true,
                last_used: if composite_is_oldest { 2 } else { 1 },
            };
            cache.used_bytes = glyph_entry_bytes(&key, &glyph);
            cache.entries.insert(key, glyph);
            let composite = CompositeEntry {
                tiled: None,
                frames: vec![CompositeFrameKey {
                    id: "object".into(),
                    source: "glyph".into(),
                    rect: [0, 0, 2, 2],
                }],
                layer_id: "layer".into(),
                source: "composite".into(),
                size: (960, 640),
                opacity: 1.0,
                frame: Arc::clone(&pixels),
                last_used: if composite_is_oldest { 1 } else { 2 },
            };
            cache.composite_bytes = composite.bytes();
            cache.composites.push(composite);
            let total = cache.stats().total_retained_payload_bytes;
            cache.reserve_payload(4, total);
            assert!(cache.stats().total_retained_payload_bytes + 4 <= total);
            assert_eq!(cache.entries.is_empty(), !composite_is_oldest);
            assert_eq!(cache.composites.is_empty(), composite_is_oldest);
            assert_eq!(cache.evicted_entries, usize::from(!composite_is_oldest));
            assert_eq!(
                cache.composite_evicted_entries,
                usize::from(composite_is_oldest)
            );
            assert_eq!(
                pixels.pixels,
                vec![30; 16],
                "eviction must not alter outstanding images"
            );
            cache.reserve_payload(total, total);
            assert_eq!(cache.stats().total_retained_payload_bytes, 0);
        }
    }

    #[test]
    fn composite_lru_entry_limit_and_payload_accounting_are_bounded() {
        let mut document = Document::default();
        document.set_text_object(text("Cache", 50.)).unwrap();
        let layer = document.svg_layers().next().unwrap();
        let mut cache = FrameRasterCache::default();
        let frame = Arc::new(CroppedFrame {
            pixels: vec![0; 4],
            origin: (0, 0),
            width: 1,
        });
        for index in 0..MAX_COMPOSITE_ENTRIES + 1 {
            cache.clock += 1;
            cache.retain_composite(
                layer,
                (960, 640),
                index as f32 / 10.,
                Arc::clone(&frame),
                vec![],
            );
        }
        assert_eq!(cache.composites.len(), MAX_COMPOSITE_ENTRIES);
        assert_eq!(cache.composite_evicted_entries, 1);
        assert!(!cache.composites.iter().any(|entry| entry.opacity == 0.0));
        assert_eq!(
            cache.composite_bytes,
            cache
                .composites
                .iter()
                .map(CompositeEntry::bytes)
                .sum::<usize>()
        );
        let oversized = Arc::new(CroppedFrame {
            pixels: vec![0; MAX_COMPOSITE_BYTES + 1],
            origin: (0, 0),
            width: 1,
        });
        cache.retain_composite(layer, (960, 640), 1.0, oversized, vec![]);
        assert_eq!(cache.composites.len(), MAX_COMPOSITE_ENTRIES);
        assert_eq!(cache.composite_evicted_entries, 1);
    }

    #[test]
    fn overlapping_display_edits_undo_and_redo_match_full_render() {
        let mut document = Document::default();
        document
            .set_text_object(text("English 日本語", 50.))
            .unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(id).unwrap();
        let mut second = text("简体中文", 55.);
        second.color = [120, 30, 80];
        document.set_text_object(second).unwrap();
        let mut cache = FrameRasterCache::default();
        let verify = |cache: &mut FrameRasterCache, document: &Document| {
            let layer = document.svg_layers().next().unwrap();
            let PreparedDisplayLayer::Text(prepared) =
                cache.prepare_display_layer(layer, 960, 640).unwrap()
            else {
                panic!("expected cropped overlap");
            };
            assert!(!prepared.independent);
            let image = Arc::clone(&prepared.frames[0].frame);
            assert_eq!(
                prepared.into_full().pixels,
                prepare_svg_layer(layer, 960, 640).unwrap().pixels
            );
            image
        };
        let original_image = verify(&mut cache, &document);
        let first = document.snapshot().text_objects[0].clone();
        let mut updated = TextSettings {
            id: Some(first.id),
            text: first.text,
            position: first.position,
            color: first.color,
        };
        updated.text.content = "Edited 日本語 简体中文".into();
        document.set_text_object(updated).unwrap();
        let edited_image = verify(&mut cache, &document);
        assert_eq!(cache.rasterized_frames, 3);
        document.undo();
        assert!(Arc::ptr_eq(&original_image, &verify(&mut cache, &document)));
        document.redo();
        assert!(Arc::ptr_eq(&edited_image, &verify(&mut cache, &document)));
        assert_eq!(
            cache.rasterized_frames, 3,
            "Undo/Redo should reuse glyph rasters"
        );
    }

    #[test]
    #[ignore = "manual text rendering timing; not an FPS guarantee"]
    fn text_frame_render_timing() {
        use std::time::Instant;
        let mut state = Document::default().document_state();
        state.width = 2048;
        state.height = 2048;
        let mut document = Document::from_document_state(state).unwrap();
        for index in 0..8 {
            document
                .set_text_object(text(
                    &format!("Frame {index} 日本語 简体中文"),
                    20.0 + index as f32 * 140.0,
                ))
                .unwrap();
            let id = document.svg_layers().next().unwrap().id.clone();
            document.select_layer(id).unwrap();
        }
        let mut cache = FrameRasterCache::default();
        let start = Instant::now();
        cache
            .prepare_layer(document.svg_layers().next().unwrap(), 2048, 2048)
            .unwrap();
        let cold = start.elapsed();
        let start = Instant::now();
        cache
            .prepare_layer(document.svg_layers().next().unwrap(), 2048, 2048)
            .unwrap();
        let warm = start.elapsed();
        let frame = document.snapshot().text_objects[0].clone();
        let mut changed = TextSettings {
            id: Some(frame.id),
            text: frame.text,
            position: frame.position,
            color: frame.color,
        };
        changed.text.content.push_str(" edited");
        document.set_text_object(changed).unwrap();
        let start = Instant::now();
        cache
            .prepare_layer(document.svg_layers().next().unwrap(), 2048, 2048)
            .unwrap();
        eprintln!("text frames=8 canvas=2048x2048 cold_ms={:.2} warm_ms={:.2} edit_ms={:.2} rasterized={} reused={}", cold.as_secs_f64()*1000.0, warm.as_secs_f64()*1000.0, start.elapsed().as_secs_f64()*1000.0, cache.rasterized_frames, cache.reused_frames);
    }

    #[test]
    fn cropped_render_matches_full_surface_for_transforms_clipping_and_effects() {
        for source in [
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><text x="-10" y="20" font-size="24">日本語 English 简体中文</text></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><g transform="translate(160 180) rotate(37) scale(.8 1.2)"><text font-size="32" stroke="red" stroke-width="3">日本語 text</text></g></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><defs><clipPath id="c"><rect x="40" y="40" width="80" height="80"/></clipPath><filter id="b"><feGaussianBlur stdDeviation="3"/></filter></defs><g clip-path="url(#c)" opacity=".5"><text x="30" y="60" font-size="24" filter="url(#b)">Text clipping</text></g></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200"><text x="-300" y="-200">offscreen</text></svg>"#,
        ] {
            let full = vector::rasterize_svg(source, 200, 200).unwrap();
            let region = vector::rasterize_svg_cropped(source, 200, 200).unwrap();
            let frame = CroppedFrame {
                pixels: region.raster.pixels,
                origin: region.origin,
                width: region.width,
            };
            let mut restored = vec![0; full.pixels.len()];
            frame.composite(&mut restored, 200);
            assert_eq!(restored, full.pixels, "{source}");
            assert_eq!(region.raster.fully_contained, full.fully_contained);
        }
    }

    #[test]
    fn single_text_box_uses_a_small_surface_and_reuses_unchanged_content() {
        let mut state = Document::default().document_state();
        state.width = 4096;
        state.height = 4096;
        let mut document = Document::from_document_state(state).unwrap();
        document
            .set_text_object(text("English 日本語 简体中文", 80.0))
            .unwrap();
        let layer = document.svg_layers().next().unwrap();
        let sources = layer.text_frame_sources(4096, 4096).unwrap();
        let region = vector::rasterize_svg_cropped(&sources[0].1, 4096, 4096).unwrap();
        assert!(region.raster.pixels.len() < 4096 * 4096 * 4 / 64);
        let mut cache = FrameRasterCache::default();
        let first = cache.prepare_layer(layer, 4096, 4096).unwrap();
        let second = cache.prepare_layer(layer, 4096, 4096).unwrap();
        assert_eq!(first.pixels, second.pixels);
        assert_eq!(cache.rasterized_frames, 1);
        assert_eq!(cache.reused_frames, 1);
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn crop_keeps_edge_alpha_and_empty_frames() {
        let mut pixels = vec![0; 8 * 6 * 4];
        pixels[(2 * 8 + 3) * 4..(2 * 8 + 3) * 4 + 4].copy_from_slice(&[1, 0, 0, 1]);
        pixels[(5 * 8 + 7) * 4..(5 * 8 + 7) * 4 + 4].copy_from_slice(&[20, 30, 40, 255]);
        let frame = CroppedFrame::from_pixels(pixels.clone(), 8, 6);
        assert_eq!(frame.origin, (2, 1));
        assert_eq!(frame.width, 6);
        assert_eq!(frame.len(), 6 * 5 * 4);
        let mut result = vec![0; pixels.len()];
        frame.composite(&mut result, 8);
        assert_eq!(result, pixels);
        let empty = CroppedFrame::from_pixels(vec![0; 8 * 6 * 4], 8, 6);
        assert_eq!(empty.len(), 0);
        empty.composite(&mut result, 8);
        assert_eq!(result, pixels);
    }

    #[test]
    fn changing_one_frame_reuses_the_other_and_matches_full_layer() {
        let mut document = Document::default();
        document
            .set_text_object(text("First 日本語", 20.0))
            .unwrap();
        let layer_id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(layer_id).unwrap();
        document
            .set_text_object(text("Second 简体中文", 140.0))
            .unwrap();
        let mut cache = FrameRasterCache::default();
        let layer = document.svg_layers().next().unwrap();
        let full = prepare_svg_layer(layer, 960, 640).unwrap();
        let cached = cache.prepare_layer(layer, 960, 640).unwrap();
        assert_eq!(cached.pixels, full.pixels);
        assert_eq!(cache.rasterized_frames, 2);
        assert!(cache.used_bytes < 2 * 960 * 640 * 4 / 4);
        eprintln!(
            "frame cache: {} bytes versus {} full-canvas bytes",
            cache.used_bytes,
            2 * 960 * 640 * 4
        );
        cache.prepare_layer(layer, 960, 640).unwrap();
        assert_eq!(cache.rasterized_frames, 2);

        let original = document.snapshot().text_objects[0].clone();
        let mut changed = TextSettings {
            id: Some(original.id),
            text: original.text,
            position: original.position,
            color: original.color,
        };
        changed.text.content = "Changed 日本語".into();
        document.set_text_object(changed).unwrap();
        let layer = document.svg_layers().next().unwrap();
        let full = prepare_svg_layer(layer, 960, 640).unwrap();
        let cached = cache.prepare_layer(layer, 960, 640).unwrap();
        assert_eq!(cached.pixels, full.pixels);
        assert_eq!(cache.rasterized_frames, 3);

        let mut faded = layer.clone();
        faded.opacity = 0.5;
        assert_eq!(
            cache.prepare_layer(&faded, 960, 640).unwrap().pixels,
            prepare_svg_layer(&faded, 960, 640).unwrap().pixels
        );
        assert_eq!(cache.rasterized_frames, 3);

        let mut incompatible = layer.clone();
        incompatible.source.push(' ');
        assert_eq!(
            cache.prepare_layer(&incompatible, 960, 640).unwrap().pixels,
            prepare_svg_layer(&incompatible, 960, 640).unwrap().pixels
        );
        assert_eq!(cache.rasterized_frames, 3);
    }

    #[test]
    fn frame_crop_rebuilds_on_resize_and_handles_empty_text() {
        let mut document = Document::default();
        document
            .set_text_object(text("Edge 日本語", 140.0))
            .unwrap();
        let layer_id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(layer_id).unwrap();
        document.set_text_object(text("", 20.0)).unwrap();
        let layer = document.svg_layers().next().unwrap();
        let mut cache = FrameRasterCache::default();
        for (width, height) in [(960, 640), (64, 160), (960, 640)] {
            let (frame, _) = cache
                .frame(&layer.id, "resized", &layer.source, (width, height))
                .unwrap();
            let mut result = vec![0; width as usize * height as usize * 4];
            frame.composite(&mut result, width as usize);
            assert_eq!(
                result,
                prepare_svg_layer(layer, width, height).unwrap().pixels
            );
        }
        assert_eq!(cache.rasterized_frames, 2);
        let mut cache = FrameRasterCache::default();
        let original = document.snapshot().text_objects[0].clone();
        let mut settings = text("", 140.0);
        settings.id = Some(original.id);
        document.set_text_object(settings).unwrap();
        let layer = document.svg_layers().next().unwrap();
        assert!(cache
            .prepare_layer(layer, 960, 640)
            .unwrap()
            .pixels
            .iter()
            .all(|&p| p == 0));
        assert!(cache.entries.values().all(|entry| entry.pixels.len() == 0));
    }

    #[test]
    fn undo_redo_reuses_prior_frame_versions_without_rasterizing() {
        let mut document = Document::default();
        document
            .set_text_object(text("Original 日本語", 20.))
            .unwrap();
        let layer_id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(layer_id).unwrap();
        document
            .set_text_object(text("Second 简体中文", 140.))
            .unwrap();
        let mut cache = FrameRasterCache::default();
        let original = cache
            .prepare_layer(document.svg_layers().next().unwrap(), 960, 640)
            .unwrap();
        let settings = document.snapshot().text_objects[0].clone();
        document
            .set_text_object(TextSettings {
                id: Some(settings.id),
                text: VectorText {
                    content: "Edited English".into(),
                    ..settings.text
                },
                position: settings.position,
                color: settings.color,
            })
            .unwrap();
        let edited = cache
            .prepare_layer(document.svg_layers().next().unwrap(), 960, 640)
            .unwrap();
        assert_eq!(cache.rasterized_frames, 3);
        document.undo();
        assert_eq!(
            cache
                .prepare_layer(document.svg_layers().next().unwrap(), 960, 640)
                .unwrap()
                .pixels,
            original.pixels
        );
        document.redo();
        assert_eq!(
            cache
                .prepare_layer(document.svg_layers().next().unwrap(), 960, 640)
                .unwrap()
                .pixels,
            edited.pixels
        );
        assert_eq!(cache.rasterized_frames, 3);
        assert!(cache.used_bytes <= MAX_CACHE_BYTES);
    }

    #[test]
    fn empty_versions_are_bounded_and_evicted() {
        let mut cache = FrameRasterCache::default();
        for i in 0..MAX_CACHE_ENTRIES + 1 {
            let source = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"><!--{i}--></svg>");
            cache.frame("layer", "frame", &source, (1, 1)).unwrap();
        }
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        let stats = cache.stats();
        assert_eq!(stats.entries, MAX_CACHE_ENTRIES);
        assert_eq!(stats.retained_payload_bytes, cache.used_bytes);
        assert_eq!(stats.payload_budget_bytes, MAX_CACHE_BYTES);
        assert_eq!(stats.evicted_entries, 1);
        assert!(cache.used_bytes > 0);
        assert_eq!(
            cache.used_bytes,
            cache
                .entries
                .iter()
                .map(|(key, entry)| glyph_entry_bytes(key, entry))
                .sum::<usize>()
        );
    }

    #[test]
    fn overlapping_frames_match_full_composite() {
        let mut document = Document::default();
        let mut first = text("Overlap", 20.0);
        first.color = [220, 40, 20];
        document.set_text_object(first).unwrap();
        let layer_id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(layer_id).unwrap();
        let mut second = text("Overlap", 20.0);
        second.color = [20, 80, 220];
        document.set_text_object(second).unwrap();
        let layer = document.svg_layers().next().unwrap();
        let full = prepare_svg_layer(layer, 960, 640).unwrap();
        let mut cache = FrameRasterCache::default();
        let cached = cache.prepare_layer(layer, 960, 640).unwrap();
        assert_eq!(cached.pixels, full.pixels);
    }
}
