//! Incremental Rust-owned paint preview for image-backed pixel layers.
use super::*;

pub struct PixelPaintPreview {
    cache: paint_cache::PaintCache,
    pixels: Vec<u8>,
    uploads: Vec<TileUpload>,
}

impl PixelPaintPreview {
    pub fn new(size: (u32, u32)) -> Result<Self, String> {
        let bytes = vector::document_rgba_len(size.0, size.1)?;
        Ok(Self {
            cache: paint_cache::PaintCache::new(size)?,
            uploads: Vec::new(),
            pixels: vec![0; bytes],
        })
    }

    pub fn update(&mut self, document: &Document) -> Result<(), String> {
        if self.cache.dimensions() != document.dimensions() {
            return Err("Paint dimensions changed".into());
        }
        let stride = document.dimensions().0 as usize * 4;
        self.uploads = self.cache.prepare(document, false)?;
        for upload in &self.uploads {
            let count = upload.extent[0] as usize * 4;
            for row in 0..upload.extent[1] as usize {
                let src = row * upload.bytes_per_row as usize;
                let dst =
                    (upload.origin[1] as usize + row) * stride + upload.origin[0] as usize * 4;
                self.pixels[dst..dst + count].copy_from_slice(&upload.pixels[src..src + count]);
            }
        }
        Ok(())
    }

    pub fn take_uploads(&mut self) -> Vec<TileUpload> {
        std::mem::take(&mut self.uploads)
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::{Brush, CanvasColor, DocumentSettings, DocumentUnit};

    #[test]
    fn a4_preview_transfers_changed_tiles_and_preserves_curve() {
        let mut document = Document::default();
        document
            .set_document_settings(DocumentSettings {
                name: "A4".into(),
                width: 2480,
                height: 3508,
                unit: DocumentUnit::Pixels,
                resolution: 300,
                artboards: false,
                canvas_color: CanvasColor::Transparent,
                pixel_aspect_ratio: 1.0,
            })
            .unwrap();
        let point = |step: usize| {
            let angle = step as f32 * std::f32::consts::TAU / 68.;
            Point {
                x: 900. + angle.cos() * 300.,
                y: 2500. + angle.sin() * 300.,
            }
        };
        document
            .begin(
                point(0),
                Brush {
                    size: 20.,
                    hardness: 1.,
                    ..Default::default()
                },
            )
            .unwrap();
        let mut preview = PixelPaintPreview::new(document.dimensions()).unwrap();
        let full_bytes = 2480 * 3508 * 4;
        let mut transferred = 0;
        let mut times = Vec::new();
        for step in 0..69 {
            document.extend_with_pressure(point(step), 1.).unwrap();
            let start = std::time::Instant::now();
            preview.update(&document).unwrap();
            times.push(start.elapsed());
            let uploads = preview.take_uploads();
            let bytes: usize = uploads.iter().map(|tile| tile.pixels.len()).sum();
            assert!(
                bytes < full_bytes / 8,
                "small brush update must not transfer the whole A4 image"
            );
            transferred += bytes;
        }
        let mut reference = PixelPaintPreview::new(document.dimensions()).unwrap();
        reference.update(&document).unwrap();
        assert_eq!(preview.pixels(), reference.pixels());
        preview.update(&document).unwrap();
        assert!(preview.take_uploads().is_empty());
        // Erasing previously painted pixels must upload transparent replacements.
        document.finish();
        document
            .begin_eraser(
                point(0),
                Brush {
                    size: 60.,
                    hardness: 1.,
                    ..Default::default()
                },
                1.,
            )
            .unwrap();
        preview.update(&document).unwrap();
        assert!(!preview.take_uploads().is_empty());
        let offset = (2500 * 2480 + 1200) * 4;
        assert_eq!(preview.pixels()[offset + 3], 0);
        times.sort();
        eprintln!("A4 69 curve updates: {:.2} MiB transferred vs {:.2} MiB full frames; CPU median {:.2} ms, max {:.2} ms", transferred as f64 / 1048576., full_bytes as f64 * 69. / 1048576., times[34].as_secs_f64() * 1000., times[68].as_secs_f64() * 1000.);
    }
}
