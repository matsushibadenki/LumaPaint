//! Document-space color sampling; only three color bytes cross the host bridge.
use crate::{paint_cache::PaintCache, vector::rasterize_svg_workspace};
use lumapaint_core::{
    document::{CanvasColor, Document, Point},
    tiles::{TileCoord, TiledRasterDocument, TILE_SIZE},
};

#[derive(Default)]
pub struct ColorSampler {
    paint: Option<PaintCache>,
}

// SVG rasters and tile composites contain premultiplied RGBA.
fn over(destination: &mut [f32; 4], source: [u8; 4], opacity: f32) {
    let alpha = source[3] as f32 / 255. * opacity;
    for c in 0..4 {
        destination[c] = source[c] as f32 * opacity + destination[c] * (1. - alpha);
    }
}
fn straight(pixel: [f32; 4]) -> Option<[u8; 3]> {
    (pixel[3] > 0.).then(|| {
        std::array::from_fn(|c| (pixel[c] * 255. / pixel[3]).round().clamp(0., 255.) as u8)
    })
}
fn validate(point: Point) -> Result<(), String> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || point.x.abs() > 1_000_000.
        || point.y.abs() > 1_000_000.
    {
        return Err("Invalid sample coordinates".into());
    }
    Ok(())
}
impl ColorSampler {
    /// One document pixel at native resolution. Selection overlays and zoom are excluded.
    /// SVG uses the existing GPU-first rasterizer with its automatic CPU fallback.
    pub fn sample(&mut self, document: &Document, point: Point) -> Result<Option<[u8; 3]>, String> {
        validate(point)?;
        let (width, height) = document.dimensions();
        let inside =
            point.x >= 0. && point.y >= 0. && point.x < width as f32 && point.y < height as f32;
        let rect = [point.x.floor(), point.y.floor(), 1., 1.];
        let mut result = [0.; 4];
        if inside && document.background_visible() {
            let mut background = if document.canvas_color() == CanvasColor::White {
                [255.; 4]
            } else {
                [0.; 4]
            };
            if self.paint.is_none() {
                self.paint = Some(PaintCache::new((width, height))?);
            }
            let cache = self.paint.as_mut().unwrap();
            cache.prepare(document, false)?;
            let mut paint = cache.pixel(rect[0] as u32, rect[1] as u32);
            for c in 0..3 {
                paint[c] = (paint[c] as u16 * paint[3] as u16 / 255) as u8;
            }
            over(&mut background, paint, 1.);
            for c in 0..4 {
                result[c] = background[c] * document.paint_layer_opacity();
            }
        }
        for layer in document.visible_svg_layers() {
            let mut pixel = rasterize_svg_workspace(&layer.source, [width, height], [1, 1], rect)?;
            crate::apply_layer_effects(&mut pixel, &document.layer_effects(&layer.id));
            over(
                &mut result,
                pixel[..4].try_into().unwrap(),
                layer.effective_opacity(),
            );
        }
        if let Some(mask) = document.clipping_mask_svg() {
            let pixel = rasterize_svg_workspace(&mask, [width, height], [1, 1], rect)?;
            for value in &mut result {
                *value *= pixel[3] as f32 / 255.;
            }
        }
        Ok(straight(result))
    }
}

pub fn sample_tiled(
    document: &TiledRasterDocument,
    point: Point,
) -> Result<Option<[u8; 3]>, String> {
    validate(point)?;
    let (width, height) = document.dimensions();
    if point.x < 0. || point.y < 0. || point.x >= width as f32 || point.y >= height as f32 {
        return Ok(None);
    }
    let (x, y) = (point.x.floor() as u32, point.y.floor() as u32);
    let coord = TileCoord {
        x: x / TILE_SIZE,
        y: y / TILE_SIZE,
    };
    let Some(pixels) = document.composite_tile(coord) else {
        return Ok(None);
    };
    let index = ((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize * 4;
    Ok(straight(std::array::from_fn(|c| pixels[index + c] as f32)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::Brush;
    #[test]
    fn samples_paint_vector_opacity_and_outside_content_without_editing() {
        let mut doc = Document::default();
        doc.begin_with_pressure(
            Point { x: 20., y: 20. },
            Brush {
                size: 16.,
                hardness: 1.,
                color: [255, 0, 0],
                ..Brush::default()
            },
            1.,
        )
        .unwrap();
        doc.finish();
        let mut sampler = ColorSampler::default();
        assert_eq!(
            sampler.sample(&doc, Point { x: 20., y: 20. }).unwrap(),
            Some([255, 0, 0])
        );
        doc.import_svg("sample".into(),r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="10" y="10" width="30" height="30" fill="blue" opacity="0.5"/><rect x="-30" y="10" width="20" height="20" fill="green"/></svg>"#.into()).unwrap();
        let revision = doc.snapshot().revision;
        let mixed = sampler
            .sample(&doc, Point { x: 20., y: 20. })
            .unwrap()
            .unwrap();
        assert!(
            (126..=129).contains(&mixed[0]) && mixed[1] == 0 && (126..=129).contains(&mixed[2])
        );
        assert_eq!(
            sampler.sample(&doc, Point { x: -20., y: 20. }).unwrap(),
            Some([0, 128, 0])
        );
        assert_eq!(
            sampler.sample(&doc, Point { x: -80., y: 20. }).unwrap(),
            None
        );
        assert_eq!(doc.snapshot().revision, revision);
        assert!(sampler.sample(&doc, Point { x: f32::NAN, y: 0. }).is_err());
    }
    #[test]
    fn sampling_backing_image_and_hidden_layers_refreshes_after_undo() {
        let mut doc = Document::default();
        doc.replace_moved_pixels(r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="10" y="10" width="30" height="30" fill="rgb(18,52,86)"/></svg>"#.into(), None).unwrap();
        let mut sampler = ColorSampler::default();
        let point = Point { x: 20., y: 20. };
        assert_eq!(sampler.sample(&doc, point).unwrap(), Some([18, 52, 86]));
        doc.import_raster_layer("foreground".into(),r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="10" y="10" width="30" height="30" fill="blue"/></svg>"#.into()).unwrap();
        let id = doc.svg_layers().next().unwrap().id.clone();
        assert_eq!(sampler.sample(&doc, point).unwrap(), Some([0, 0, 255]));
        doc.toggle_layer(&id).unwrap();
        assert_eq!(sampler.sample(&doc, point).unwrap(), Some([18, 52, 86]));
        doc.toggle_layer(&id).unwrap();
        assert_eq!(sampler.sample(&doc, point).unwrap(), Some([0, 0, 255]));
        doc.undo();
        assert_eq!(sampler.sample(&doc, point).unwrap(), Some([18, 52, 86]));
        doc.undo();
        assert_eq!(sampler.sample(&doc, point).unwrap(), Some([255, 255, 255]));
    }

    #[test]
    fn tiled_sampling_unpremultiplies_and_respects_visibility() {
        let mut doc = TiledRasterDocument::new(300, 300).unwrap();
        doc.add_layer("a".into(), "A".into()).unwrap();
        doc.write_rect("a", [260, 270, 1, 1], &[200, 100, 50, 128])
            .unwrap();
        let color = sample_tiled(&doc, Point { x: 260.9, y: 270.1 })
            .unwrap()
            .unwrap();
        assert!(
            color[0].abs_diff(200) <= 1
                && color[1].abs_diff(100) <= 1
                && color[2].abs_diff(50) <= 1
        );
        doc.set_layer_appearance("a", false, 1.).unwrap();
        assert_eq!(
            sample_tiled(&doc, Point { x: 260., y: 270. }).unwrap(),
            None
        );
        assert_eq!(
            sample_tiled(&doc, Point { x: 300., y: 270. }).unwrap(),
            None
        );
    }
}
