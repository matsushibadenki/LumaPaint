use crate::{Glyph, Snapshot};
use anyhow::{Result, ensure};
use kurbo::{BezPath, Shape};
use rustybuzz::ttf_parser::{Face, GlyphId, OutlineBuilder};

impl Snapshot {
    pub(crate) fn outline_scale(&self, face: &Face<'_>) -> f64 {
        self.font_size as f64 / f64::from(face.units_per_em())
    }

    pub(crate) fn checked_outline(
        &self,
        face: &Face<'_>,
        paragraph: usize,
        glyph: &Glyph,
        scale: f64,
    ) -> Result<Option<BezPath>> {
        let id = GlyphId(glyph.id);
        ensure!(
            !face.is_color_glyph(id)
                && face.glyph_svg_image(id).is_none()
                && face.glyph_raster_image(id, u16::MAX).is_none(),
            "unsupported color or image glyph {}",
            glyph.id
        );

        let mut outline = GlyphOutline::default();
        let bounds = face.outline_glyph(id, &mut outline);
        // Empty glyphs such as spaces have no visible outline.
        if outline.0.is_empty() {
            return Ok(None);
        }
        ensure!(bounds.is_some(), "invalid outline for glyph {}", glyph.id);
        let ink = outline.0.bounding_box();
        let (x, y) = (glyph.x as f64, glyph.y as f64);
        ensure!(
            (0.0..=self.width as f64).contains(&(x + ink.x0 * scale))
                && (0.0..=self.width as f64).contains(&(x + ink.x1 * scale))
                && (0.0..=self.height as f64).contains(&(y - ink.y1 * scale))
                && (0.0..=self.height as f64).contains(&(y - ink.y0 * scale)),
            "glyph {} in paragraph {paragraph} at byte {} extends outside the page",
            glyph.id,
            glyph.cluster
        );
        Ok(Some(outline.0))
    }
}

#[derive(Default)]
struct GlyphOutline(BezPath);

impl OutlineBuilder for GlyphOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to((x1, y1), (x, y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.curve_to((x1, y1), (x2, y2), (x, y));
    }

    fn close(&mut self) {
        self.0.close_path();
    }
}
