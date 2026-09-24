//! CPU cache for independently editable text frames in one vector layer.
use super::{prepare_svg_layer, vector, PreparedSvgLayer};
use lumapaint_core::document::SvgLayer;
use std::collections::HashMap;
use std::sync::Arc;

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;

struct Entry {
    source: String,
    fingerprint: u64,
    size: (u32, u32),
    pixels: Arc<[u8]>,
    fully_contained: bool,
    last_used: u64,
}

#[derive(Default)]
pub struct FrameRasterCache {
    entries: HashMap<(String, String), Entry>,
    used_bytes: usize,
    clock: u64,
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
            source_over(&mut pixels, &frame);
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
    ) -> Result<(Arc<[u8]>, bool), String> {
        self.clock = self.clock.wrapping_add(1);
        let key = (layer_id.to_owned(), object_id.to_owned());
        let hash = fingerprint(source);
        if let Some(entry) = self.entries.get_mut(&key) {
            if entry.fingerprint == hash && entry.source == source && entry.size == size {
                entry.last_used = self.clock;
                self.reused_frames += 1;
                return Ok((Arc::clone(&entry.pixels), entry.fully_contained));
            }
        }
        if let Some(old) = self.entries.remove(&key) {
            self.used_bytes -= old.pixels.len();
        }
        let raster = vector::rasterize_svg(source, size.0, size.1)?;
        self.rasterized_frames += 1;
        let pixels: Arc<[u8]> = raster.pixels.into();
        if pixels.len() <= MAX_CACHE_BYTES {
            while self.used_bytes + pixels.len() > MAX_CACHE_BYTES {
                let Some(oldest) = self
                    .entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(key, _)| key.clone())
                else {
                    break;
                };
                self.used_bytes -= self.entries.remove(&oldest).unwrap().pixels.len();
            }
            self.used_bytes += pixels.len();
            self.entries.insert(
                key,
                Entry {
                    source: source.to_owned(),
                    fingerprint: hash,
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
