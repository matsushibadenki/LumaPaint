//! Immutable request snapshots are resolved before any document mutation.
use image::GenericImageView;
use lumapaint_core::{document::Point, selection::Selection};
pub struct Target {
    pub document: u64,
    pub revision: u64,
    pub layer: String,
    pub width: u32,
    pub height: u32,
    pub selection: Option<Selection>,
    pub pixels: Vec<u8>,
    pub input: Option<Vec<u8>>,
    pub bounds: [u32; 4],
}
pub struct Prepared {
    pub target: Target,
    pub source: String,
}
impl Target {
    pub fn prepare_input(&mut self, pixels: Vec<u8>) -> Result<(), String> {
        let selection = self.selection.as_ref().ok_or("Missing image selection")?;
        let (width, height) = (self.width, self.height);
        if pixels.len() != lumapaint_renderer::vector::document_rgba_len(width, height)? {
            return Err("Invalid source image".into());
        }
        let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
        let mut has_image = false;
        for y in 0..height {
            for x in 0..width {
                if selection.contains(Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                }) {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x + 1);
                    bottom = bottom.max(y + 1);
                    has_image |= pixels[((y * width + x) * 4 + 3) as usize] > 0;
                }
            }
        }
        if left >= right || top >= bottom {
            return Err("Selection is empty / 選択範囲が空です / 选区为空".into());
        }
        if !has_image {
            return Err(
                "No image pixels in selection / 選択範囲に画像がありません / 选区内没有图像像素"
                    .into(),
            );
        }
        self.bounds = [left, top, right - left, bottom - top];
        let mut crop = Vec::with_capacity((self.bounds[2] * self.bounds[3] * 4) as usize);
        for y in top..bottom {
            let start = ((y * width + left) * 4) as usize;
            for x in left..right {
                let offset = start + ((x - left) * 4) as usize;
                if selection.contains(Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                }) {
                    crop.extend_from_slice(&pixels[offset..offset + 4]);
                } else {
                    crop.extend_from_slice(&[0; 4]);
                }
            }
        }
        self.input = Some(lumapaint_renderer::vector::document_png(
            self.bounds[2],
            self.bounds[3],
            crop,
        )?);
        self.pixels = pixels;
        Ok(())
    }
    pub fn finish(mut self, bytes: &[u8]) -> Result<Prepared, String> {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|_| "Unknown AI image format")?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|_| "Cannot decode AI image")?;
        let [x, y, w, h] = self.bounds;
        let edited = self.selection.is_some();
        let (rw, rh) = if edited {
            (w, h)
        } else {
            let (iw, ih) = image.dimensions();
            let scale = (w as f64 / iw as f64).min(h as f64 / ih as f64);
            (
                (iw as f64 * scale).round().max(1.) as u32,
                (ih as f64 * scale).round().max(1.) as u32,
            )
        };
        let raster = image
            .resize_exact(rw, rh, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let ox = x + (w - rw) / 2;
        let oy = y + (h - rh) / 2;
        if !edited {
            self.pixels =
                vec![0; lumapaint_renderer::vector::document_rgba_len(self.width, self.height)?];
        }
        for dy in 0..rh {
            for dx in 0..rw {
                let px = ox + dx;
                let py = oy + dy;
                if self.selection.as_ref().is_some_and(|s| {
                    !s.contains(Point {
                        x: px as f32 + 0.5,
                        y: py as f32 + 0.5,
                    })
                }) {
                    continue;
                }
                let mut p = raster.get_pixel(dx, dy).0;
                for i in 0..3 {
                    p[i] = ((p[i] as u16 * p[3] as u16 + 127) / 255) as u8;
                }
                let offset = ((py * self.width + px) * 4) as usize;
                self.pixels[offset..offset + 4].copy_from_slice(&p);
            }
        }
        let png = lumapaint_renderer::vector::document_png(
            self.width,
            self.height,
            std::mem::take(&mut self.pixels),
        )?;
        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let source=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><image width=\"{}\" height=\"{}\" href=\"data:image/png;base64,{data}\"/></svg>",self.width,self.height,self.width,self.height);
        self.input = None;
        Ok(Prepared {
            target: self,
            source,
        })
    }
}

pub fn apply(
    prepared: Prepared,
    name: String,
    document: u64,
    doc: &mut lumapaint_core::document::Document,
) -> Result<(), String> {
    let target = prepared.target;
    if target.document != document
        || doc.revision() != target.revision
        || (target.selection.is_some()
            && (doc.snapshot().layer_id != target.layer
                || doc.selection() != target.selection.as_ref()))
    {
        return Err("Document changed during generation; result was not applied / 生成中に文書が変更されたため適用しませんでした / 生成期间文档已更改，结果未应用".into());
    }
    if target.selection.is_some() {
        doc.replace_moved_pixels(prepared.source, target.selection)
    } else {
        doc.import_raster_layer(name, prepared.source)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::{document::Document, selection::SelectionShape};
    use lumapaint_formats::native::NativeDocumentCodec;
    fn setup(edit: bool) -> (Document, Target, Vec<u8>) {
        let mut doc = Document::default();
        doc.crop_canvas([0., 0., 4., 4.]).unwrap();
        let pixels = [0, 0, 0, 0, 7, 13, 21, 64, 23, 45, 67, 128, 12, 24, 36, 255].repeat(4);
        let png = lumapaint_renderer::vector::document_png(4, 4, pixels.clone()).unwrap();
        use base64::Engine;
        let source=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"4\"><image width=\"4\" height=\"4\" href=\"data:image/png;base64,{}\"/></svg>",base64::engine::general_purpose::STANDARD.encode(png));
        doc.replace_moved_pixels(source, None).unwrap();
        let selection = edit.then(|| Selection::new(SelectionShape::Ellipse, [1., 1., 2., 2.]));
        doc.set_tool_pixel_selection(selection.clone()).unwrap();
        let target = Target {
            document: 1,
            revision: doc.revision(),
            layer: doc.snapshot().layer_id,
            width: 4,
            height: 4,
            selection,
            pixels: pixels.clone(),
            input: None,
            bounds: if edit { [1, 1, 2, 2] } else { [0, 0, 4, 4] },
        };
        (doc, target, pixels)
    }
    fn generated() -> Vec<u8> {
        lumapaint_renderer::vector::document_png(2, 2, [200, 20, 30, 255].repeat(4)).unwrap()
    }
    #[test]
    fn edits_preserve_outside_mask_and_undo_redo() {
        let (mut doc, mut target, before) = setup(true);
        target.prepare_input(before.clone()).unwrap();
        assert!(target.input.is_some());
        assert_eq!(target.bounds, [1, 1, 2, 2]);
        let selected = target.selection.clone().unwrap();
        let prepared = target.finish(&generated()).unwrap();
        let pixels = lumapaint_renderer::vector::rasterize_svg(&prepared.source, 4, 4)
            .unwrap()
            .pixels;
        for y in 0..4 {
            for x in 0..4 {
                let i = ((y * 4 + x) * 4) as usize;
                if !selected.contains(Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                }) {
                    assert_eq!(&pixels[i..i + 4], &before[i..i + 4]);
                } else {
                    assert_eq!(&pixels[i..i + 4], &[200, 20, 30, 255]);
                }
            }
        }
        let saved = doc.encode().unwrap();
        apply(prepared, "AI".into(), 1, &mut doc).unwrap();
        let after = doc.encode().unwrap();
        doc.undo();
        assert_eq!(doc.encode().unwrap(), saved);
        doc.redo();
        assert_eq!(doc.encode().unwrap(), after);
    }
    #[test]
    fn provider_input_contains_only_selected_image_pixels() {
        let (_, mut target, pixels) = setup(true);
        target.selection = Some(Selection::new(SelectionShape::Ellipse, [0., 0., 4., 4.]));
        target.prepare_input(pixels).unwrap();
        let image = image::load_from_memory(target.input.as_ref().unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(image.get_pixel(3, 0).0, [0; 4]);
        assert!(image.get_pixel(1, 1).0[3] > 0);
        assert!(target.prepare_input(vec![0; 64]).is_err());
    }
    #[test]
    fn new_layer_and_conflicts_are_atomic() {
        let (mut doc, target, _) = setup(false);
        let before = doc.encode().unwrap();
        let count = doc.snapshot().layers.len();
        apply(
            target.finish(&generated()).unwrap(),
            "Generated".into(),
            1,
            &mut doc,
        )
        .unwrap();
        assert_eq!(doc.snapshot().layers.len(), count + 1);
        let after = doc.encode().unwrap();
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.redo();
        assert_eq!(doc.encode().unwrap(), after);
        let (_, target, _) = setup(false);
        let unchanged = doc.encode().unwrap();
        assert!(apply(
            target.finish(&generated()).unwrap(),
            "Rejected".into(),
            2,
            &mut doc
        )
        .is_err());
        assert_eq!(doc.encode().unwrap(), unchanged);
        let (_, target, _) = setup(true);
        assert!(target.finish(b"invalid image").is_err());
        assert_eq!(doc.encode().unwrap(), unchanged);
    }
}
