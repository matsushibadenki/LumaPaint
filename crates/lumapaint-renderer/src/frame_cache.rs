//! CPU cache for independently editable text frames in one vector layer.
use super::{prepare_svg_layer, vector, PreparedSvgLayer};
use lumapaint_core::document::SvgLayer;
use std::collections::HashMap;
use std::sync::Arc;

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_ENTRIES: usize = 512;

/// Premultiplied pixels bounded by actual coverage, including antialiased edges.
pub(crate) struct CroppedFrame {
    pub(crate) pixels: Vec<u8>,
    pub(crate) origin: (usize, usize),
    pub(crate) width: usize,
}

/// Worker result for display. Export and compatibility compositing retain full pixels.
pub enum PreparedDisplayLayer {
    Full(PreparedSvgLayer),
    Text(PreparedTextLayer),
}

pub struct PreparedTextLayer {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) size: (u32, u32),
    pub(crate) frames: Vec<PreparedTextFrame>,
}

pub(crate) struct PreparedTextFrame {
    pub(crate) id: String,
    pub(crate) source: String,
    pub(crate) frame: Arc<CroppedFrame>,
}

#[cfg(test)]
impl PreparedTextLayer {
    pub(crate) fn into_full(self) -> PreparedSvgLayer {
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
            opacity: 1.0,
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
            Self::Text(layer) => layer.frames.iter().map(|f| f.frame.len()).sum(),
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
    /// Prepare independent, disjoint, contained text frames without a document-sized buffer.
    /// Disjointness is required because CPU encoded-RGBA and GPU linear blending differ.
    pub fn prepare_display_layer(
        &mut self,
        layer: &SvgLayer,
        width: u32,
        height: u32,
    ) -> Result<PreparedDisplayLayer, String> {
        let parts = (layer.effective_opacity() == 1.0
            && layer.vector_objects.len() <= 64
            && layer.vector_objects.iter().all(|o| {
                o.group_path.is_empty() && o.clipping_group.is_none() && o.blend_mode == "normal"
            }))
        .then(|| layer.text_frame_sources(width, height))
        .flatten();
        if let Some(parts) = parts {
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
                if frames
                    .iter()
                    .any(|other| frames_overlap(&other.frame, &frame))
                {
                    crate::performance::count("fallback.text_frame_overlap", 1);
                    compatible = false;
                    break;
                }
                frames.push(PreparedTextFrame { id, source, frame });
            }
            if compatible && !frames.is_empty() {
                crate::performance::count("prepared_retained_text_frames", frames.len() as u64);
                return Ok(PreparedDisplayLayer::Text(PreparedTextLayer {
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
                crate::performance::count("frame_cache_hits", 1);
                return Ok((Arc::clone(&entry.pixels), entry.fully_contained));
            }
        }
        if let Some(old) = self.entries.remove(&key) {
            self.used_bytes -= old.pixels.len() + old.source.len();
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
                    fully_contained,
                    last_used: self.clock,
                },
            );
        }
        Ok((pixels, fully_contained))
    }
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
    fn overlapping_and_faded_display_layers_keep_compatibility_compositing() {
        let mut document = Document::default();
        document.set_text_object(text("Overlap", 50.)).unwrap();
        let id = document.svg_layers().next().unwrap().id.clone();
        document.select_layer(id).unwrap();
        let mut second = text("Overlap", 50.);
        second.color = [180, 30, 70];
        document.set_text_object(second).unwrap();
        let mut cache = FrameRasterCache::default();
        let layer = document.svg_layers().next().unwrap();
        for opacity in [1.0, 0.5] {
            let mut layer = layer.clone();
            layer.opacity = opacity;
            let PreparedDisplayLayer::Full(full) =
                cache.prepare_display_layer(&layer, 960, 640).unwrap()
            else {
                panic!("dependent compositing must stay full")
            };
            assert_eq!(
                full.pixels,
                prepare_svg_layer(&layer, 960, 640).unwrap().pixels
            );
        }
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
