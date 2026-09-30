//! CPU cache for independently editable text frames in one vector layer.
use super::{prepare_svg_layer, vector, PreparedSvgLayer};
use lumapaint_core::document::SvgLayer;
use std::collections::HashMap;
use std::sync::Arc;

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 512;

/// Premultiplied pixels bounded by actual coverage, including antialiased edges.
struct CroppedFrame {
    pixels: Vec<u8>,
    origin: (usize, usize),
    width: usize,
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

type FrameKey = (String, String, u64, (u32, u32));

/// Accounted retained CPU payload, not total heap usage or GPU allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameCacheStats {
    pub entries: usize,
    pub retained_payload_bytes: usize,
    pub payload_budget_bytes: usize,
    pub evicted_entries: usize,
}

#[derive(Default)]
pub struct FrameRasterCache {
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
        let remaining = 255 - u32::from(source[3]);
        for channel in 0..4 {
            destination[channel] = (u32::from(source[channel])
                + (u32::from(destination[channel]) * remaining + 127) / 255)
                .min(255) as u8;
        }
    }
}

impl FrameRasterCache {
    pub fn stats(&self) -> FrameCacheStats {
        FrameCacheStats {
            entries: self.entries.len(),
            retained_payload_bytes: self.used_bytes,
            payload_budget_bytes: MAX_CACHE_BYTES,
            evicted_entries: self.evicted_entries,
        }
    }

    pub fn prepare_layer(
        &mut self,
        layer: &SvgLayer,
        width: u32,
        height: u32,
    ) -> Result<PreparedSvgLayer, String> {
        let Some(parts) = layer.text_frame_sources(width, height) else {
            return prepare_svg_layer(layer, width, height);
        };
        let byte_count = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("Invalid text frame raster dimensions")?;
        let mut pixels = vec![0; byte_count];
        let mut fully_contained = true;
        for (object_id, source) in parts {
            let (frame, contained) = self.frame(&layer.id, &object_id, &source, (width, height))?;
            frame.composite(&mut pixels, width as usize);
            fully_contained &= contained;
        }
        let opacity = layer.effective_opacity();
        if opacity != 1.0 {
            for value in &mut pixels {
                *value = (f32::from(*value) * opacity).round() as u8;
            }
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
                return Ok((Arc::clone(&entry.pixels), entry.fully_contained));
            }
        }
        if let Some(old) = self.entries.remove(&key) {
            self.used_bytes -= old.pixels.len() + old.source.len();
        }
        let raster = vector::rasterize_svg(source, size.0, size.1)?;
        self.rasterized_frames += 1;
        let pixels = Arc::new(CroppedFrame::from_pixels(raster.pixels, size.0, size.1));
        let entry_bytes = pixels.len().saturating_add(source.len());
        if entry_bytes <= MAX_CACHE_BYTES {
            while self.used_bytes + entry_bytes > MAX_CACHE_BYTES
                || self.entries.len() >= MAX_CACHE_ENTRIES
            {
                let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(key, _)| key.clone())
                else {
                    break;
                };
                let old = self.entries.remove(&oldest).unwrap();
                self.evicted_entries += 1;
                self.used_bytes -= old.pixels.len() + old.source.len();
            }
            self.used_bytes += entry_bytes;
            self.entries.insert(
                key,
                Entry {
                    source: source.to_owned(),
                    size,
                    pixels: Arc::clone(&pixels),
                    fully_contained: raster.fully_contained,
                    last_used: self.clock,
                },
            );
        }
        Ok((pixels, raster.fully_contained))
    }
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
    fn crop_keeps_edge_alpha_and_empty_frames() {
        let mut pixels = vec![0; 8 * 6 * 4];
        pixels[(2 * 8 + 3) * 4..(2 * 8 + 3) * 4 + 4].copy_from_slice(&[1, 0, 0, 1]);
        pixels[(5 * 8 + 7) * 4..(5 * 8 + 7) * 4 + 4].copy_from_slice(&[20, 30, 40, 255]);
        let frame = CroppedFrame::from_pixels(pixels.clone(), 8, 6);
        assert_eq!(frame.origin, (3, 2));
        assert_eq!(frame.width, 5);
        assert_eq!(frame.len(), 5 * 4 * 4);
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
                .values()
                .map(|e| e.source.len() + e.pixels.len())
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
