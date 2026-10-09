//! Isolated channel edits preserve every unselected component. Source and brush
//! coverage remain in Rust; previews reuse the normal incremental brush raster.
use crate::{pixel_paint::PixelPaintPreview, vector, Document, TileUpload};
use lumapaint_core::brush_blend::{composite_gray, BrushBlendMode};

pub struct ChannelPaint {
    size: (u32, u32),
    channel: usize,
    value: u8,
    blend_mode: BrushBlendMode,
    original: Vec<u8>,
    pixels: Vec<u8>,
    coverage: PixelPaintPreview,
}
impl ChannelPaint {
    pub fn new(size: (u32, u32), source: Vec<u8>, channel: u32, value: u8) -> Result<Self, String> {
        if !(1..=4).contains(&channel) || source.len() != vector::document_rgba_len(size.0, size.1)?
        {
            return Err("Invalid channel paint source".into());
        }
        Ok(Self {
            size,
            channel: channel as usize - 1,
            value,
            blend_mode: BrushBlendMode::Normal,
            pixels: source.clone(),
            original: source,
            coverage: PixelPaintPreview::new(size)?,
        })
    }
    pub fn with_blend_mode(mut self, mode: BrushBlendMode) -> Self {
        self.blend_mode = mode;
        self
    }
    pub fn update(&mut self, stroke: &Document) -> Result<Vec<TileUpload>, String> {
        self.coverage.update(stroke)?;
        let mut uploads = self.coverage.take_uploads();
        for upload in &mut uploads {
            for y in 0..upload.extent[1] as usize {
                for x in 0..upload.extent[0] as usize {
                    let local = y * upload.bytes_per_row as usize + x * 4;
                    let global = ((upload.origin[1] as usize + y) * self.size.0 as usize
                        + upload.origin[0] as usize
                        + x)
                        * 4;
                    let source: [u8; 4] = self.original[global..global + 4].try_into().unwrap();
                    let out = paint_pixel_blended(
                        source,
                        self.channel,
                        self.value,
                        upload.pixels[local + 3],
                        self.blend_mode,
                        [upload.origin[0] + x as u32, upload.origin[1] + y as u32],
                    );
                    self.pixels[global..global + 4].copy_from_slice(&out);
                    upload.pixels[local..local + 4].copy_from_slice(&out);
                }
            }
        }
        Ok(uploads)
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}
fn paint_pixel(source: [u8; 4], channel: usize, value: u8, coverage: u8) -> [u8; 4] {
    paint_pixel_blended(
        source,
        channel,
        value,
        coverage,
        BrushBlendMode::Normal,
        [0; 2],
    )
}
fn paint_pixel_blended(
    source: [u8; 4],
    channel: usize,
    value: u8,
    coverage: u8,
    mode: BrushBlendMode,
    position: [u32; 2],
) -> [u8; 4] {
    let mix = |old| composite_gray(old, value, f32::from(coverage) / 255., mode, position);
    let mut result = source;
    if channel == 3 {
        result[3] = mix(source[3]);
        for c in 0..3 {
            result[c] = (u32::from(source[c]) * u32::from(result[3]) + u32::from(source[3]) / 2)
                .checked_div(u32::from(source[3]))
                .unwrap_or(0)
                .min(255) as u8;
        }
    } else if source[3] > 0 {
        let straight = ((u32::from(source[channel]) * 255 + u32::from(source[3]) / 2)
            / u32::from(source[3]))
        .min(255) as u8;
        result[channel] = ((u32::from(mix(straight)) * u32::from(source[3]) + 127) / 255) as u8;
    }
    result
}

pub fn clear(
    pixels: &mut [u8],
    size: (u32, u32),
    channel: u32,
    selection: Option<&lumapaint_core::selection::Selection>,
) -> Result<(), String> {
    if !(1..=4).contains(&channel) || pixels.len() != vector::document_rgba_len(size.0, size.1)? {
        return Err("Invalid channel source".into());
    }
    for (i, p) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let point = lumapaint_core::document::Point {
            x: (i as u32 % size.0) as f32 + 0.5,
            y: (i as u32 / size.0) as f32 + 0.5,
        };
        if selection.is_none_or(|s| s.contains(point)) {
            let out = paint_pixel(*p, channel as usize - 1, 0, 255);
            p.copy_from_slice(&out);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::{Brush, Point};
    #[test]
    fn brush_channel_modes_preserve_unselected_values() {
        let source = [32, 64, 96, 128];
        for channel in 0..3 {
            let out =
                paint_pixel_blended(source, channel, 128, 255, BrushBlendMode::Multiply, [0; 2]);
            assert!(out[channel].abs_diff(source[channel] / 2) <= 1);
            for i in 0..4 {
                if i != channel {
                    assert_eq!(out[i], source[i]);
                }
            }
        }
        assert_eq!(
            paint_pixel_blended(source, 3, 255, 128, BrushBlendMode::Clear, [0; 2]),
            [16, 32, 48, 64]
        );
        assert_eq!(
            paint_pixel_blended(source, 1, 255, 255, BrushBlendMode::Behind, [0; 2]),
            source
        );
    }
    #[test]
    fn only_selected_rgb_component_changes_and_alpha_remains_exact() {
        for channel in 0..3 {
            let source = [32, 64, 96, 128];
            let out = paint_pixel(source, channel, 255, 128);
            for c in 0..4 {
                if c != channel {
                    assert_eq!(out[c], source[c]);
                }
            }
            assert!(out[channel] > source[channel]);
        }
        assert_eq!(paint_pixel([0; 4], 1, 255, 255), [0; 4]);
        assert_eq!(paint_pixel([32, 64, 96, 128], 3, 0, 255), [0; 4]);
        assert_eq!(
            paint_pixel([32, 64, 96, 128], 3, 255, 255),
            [64, 128, 191, 255]
        );
    }
    #[test]
    fn channel_brush_preview_obeys_selection_and_does_not_accumulate_on_refresh() {
        let mut state = Document::default().document_state();
        state.width = 32;
        state.height = 32;
        let mut doc = Document::from_document_state(state)
            .unwrap()
            .channel_brush_workspace();
        doc.set_tool_pixel_selection(Some(lumapaint_core::selection::Selection::new(
            lumapaint_core::selection::SelectionShape::Rectangle,
            [16., 0., 16., 32.],
        )))
        .unwrap();
        doc.begin(
            Point { x: 16., y: 16. },
            Brush {
                color: [255; 3],
                size: 8.,
                hardness: 1.,
                ..Default::default()
            },
        )
        .unwrap();
        let mut paint =
            ChannelPaint::new((32, 32), [40, 80, 120, 255].repeat(32 * 32), 2, 0).unwrap();
        paint.update(&doc).unwrap();
        let at = (16 * 32 + 16) * 4;
        assert_eq!(&paint.pixels()[at..at + 4], &[40, 0, 120, 255]);
        assert_eq!(&paint.pixels()[..4], &[40, 80, 120, 255]);
        let clipped = (16 * 32 + 14) * 4;
        assert_eq!(&paint.pixels()[clipped..clipped + 4], &[40, 80, 120, 255]);
        let before = paint.pixels().to_vec();
        paint.update(&doc).unwrap();
        assert_eq!(paint.pixels(), before);
    }
}
