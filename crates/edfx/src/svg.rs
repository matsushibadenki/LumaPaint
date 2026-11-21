use crate::Document;
use anyhow::{Result, ensure};
use rustybuzz::ttf_parser::Face;
use std::{io::Write, path::Path};

impl Document {
    /// Exports the saved glyph outlines and rejects any ink outside the page.
    pub fn export_svg(&self, path: &Path) -> Result<()> {
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut output = tempfile::NamedTempFile::new_in(directory)?;
        self.write_svg(&mut output)?;
        output.as_file().sync_all()?;
        output.persist(path)?;
        Ok(())
    }

    pub fn write_svg(&self, mut output: impl Write) -> Result<()> {
        let font = self.font_bytes()?;
        let face = Face::parse(&font, self.font.face_index)?;
        let tables = face.tables();
        ensure!(
            tables.glyf.is_some() || tables.cff.is_some() || tables.cff2.is_some(),
            "SVG export requires an outline font"
        );
        let face = &face;
        let page = self.snapshot();
        let scale = page.outline_scale(face);
        let (width, height) = (page.width, page.height);
        writeln!(
            output,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}pt" height="{}pt" viewBox="0 0 {width} {height}">"#,
            width as f64 / 1000.0,
            height as f64 / 1000.0,
        )?;
        for line in &page.lines {
            for glyph in &line.glyphs {
                let Some(outline) = page.checked_outline(face, line.paragraph, glyph, scale)?
                else {
                    continue;
                };
                let (x, y) = (glyph.x, glyph.y);
                writeln!(
                    output,
                    r#"  <path transform="translate({x} {y}) scale({scale} {})" d="{}"/>"#,
                    -scale,
                    outline.to_svg(),
                )?;
            }
        }
        writeln!(output, "</svg>")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Locale, Paragraph};
    use std::fs;

    #[test]
    fn exports_saved_coordinates_and_renders_each_paragraph() {
        let mut document = fixture();
        // A second layout pass would put this glyph back in its original position.
        let glyph = &mut document.snapshot.lines[0].glyphs[0];
        glyph.x += 8000;
        let (saved_x, saved_y) = (glyph.x, glyph.y);
        document.sha256 = document.digest().unwrap();

        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("page.edfx");
        let output = directory.path().join("page.svg");
        document.save(&input).unwrap();
        Document::load(&input).unwrap().export_svg(&output).unwrap();

        let svg = fs::read_to_string(output).unwrap();
        assert!(svg.contains(&format!("translate({saved_x} {saved_y})")));
        assert!(!svg.contains("<text"));
        let tree = resvg::usvg::Tree::from_str(&svg, &Default::default()).unwrap();
        let size = tree.size().to_int_size();
        let mut image = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut image.as_mut(),
        );
        let width = image.width();
        for line in &document.snapshot().lines {
            let baseline = line.baseline as f64 / 1000.0 * 96.0 / 72.0;
            let y = baseline as u32;
            let ink_pixels = (y - 17..y + 5)
                .flat_map(|row| (0..width).map(move |column| (column, row)))
                .filter(|&(x, y)| image.pixel(x, y).unwrap().alpha() != 0)
                .count();
            assert!(
                ink_pixels > 50,
                "no rendered line for paragraph {}",
                line.paragraph
            );
        }
    }

    #[test]
    fn detects_ink_overflow_and_keeps_existing_output() {
        let mut document = fixture();
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("page.svg");
        fs::write(&output, "existing SVG").unwrap();
        let (width, height) = (document.snapshot.width, document.snapshot.height);
        let glyph = &mut document.snapshot.lines[0].glyphs[0];
        let (original_x, original_y) = (glyph.x, glyph.y);
        for (x, y) in [
            (width - 1, original_y),
            (original_x, 1),
            (original_x, height + 12_000),
        ] {
            let glyph = &mut document.snapshot.lines[0].glyphs[0];
            (glyph.x, glyph.y) = (x, y);
            let error = document.export_svg(&output).unwrap_err();
            assert!(error.to_string().contains("extends outside the page"));
            assert_eq!(fs::read_to_string(&output).unwrap(), "existing SVG");
        }
    }

    fn fixture() -> Document {
        let font = include_bytes!("../fixtures/boundary-test.ttf");
        let paragraphs = [Locale::En, Locale::Ja, Locale::ZhHans]
            .into_iter()
            .map(|locale| Paragraph {
                locale,
                text: "AAAA".into(),
            })
            .collect();
        Document::new(paragraphs, font, 0).unwrap()
    }
}
