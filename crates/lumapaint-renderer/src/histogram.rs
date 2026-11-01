//! Bounded, alpha-weighted 8-bit preview histograms, before layer adjustments.
use lumapaint_core::tiles::SparseTiles;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Histogram {
    /// Composite RGB, red, green, blue. Every channel has 256 bins.
    pub channels: Vec<Vec<u64>>,
    pub samples: u64,
}
impl Histogram {
    fn new() -> Self {
        Self {
            channels: vec![vec![0; 256]; 4],
            samples: 0,
        }
    }
    fn add(&mut self, pixel: [u8; 4]) {
        let alpha = u64::from(pixel[3]);
        if alpha == 0 {
            return;
        }
        self.samples += 1;
        for (index, value) in pixel[..3].iter().enumerate() {
            self.channels[index + 1][*value as usize] += alpha;
            self.channels[0][*value as usize] += alpha;
        }
    }
}
pub fn from_premultiplied(pixels: &[u8]) -> Histogram {
    let mut result = Histogram::new();
    for pixel in pixels.as_chunks::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            continue;
        }
        let mut straight = [0; 4];
        straight[3] = pixel[3];
        for channel in 0..3 {
            straight[channel] =
                ((u32::from(pixel[channel]) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
        result.add(straight);
    }
    result
}
/// Four stratified samples per proxy cell. Work stays bounded for huge canvases;
/// Straight RGBA, at most 65,536 reads; avoids copying an entire tiled layer.
pub fn from_tiles(tiles: &SparseTiles, width: u32, height: u32) -> Histogram {
    let mut result = Histogram::new();
    if width == 0 || height == 0 {
        return result;
    }
    let scale = (128. / f64::from(width.max(height))).min(1.);
    let w = (f64::from(width) * scale).round().max(1.) as u32;
    let h = (f64::from(height) * scale).round().max(1.) as u32;
    for y in 0..h {
        for x in 0..w {
            for (dx, dy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let px = ((f64::from(x) + dx) * f64::from(width) / f64::from(w)) as u32;
                let py = ((f64::from(y) + dy) * f64::from(height) / f64::from(h)) as u32;
                result.add(
                    tiles
                        .pixel(px.min(width - 1), py.min(height - 1))
                        .unwrap_or([0; 4]),
                );
            }
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_weighted_channels_ignore_transparent_pixels_and_unpremultiply() {
        let h = from_premultiplied(&[128, 0, 0, 128, 0, 255, 0, 255, 0, 0, 0, 0]);
        assert_eq!(h.samples, 2);
        assert_eq!(h.channels[1][255], 128);
        assert_eq!(h.channels[2][255], 255);
        assert_eq!(h.channels[3][0], 383);
        assert_eq!(h.channels[0].iter().sum::<u64>(), 3 * 383);
        assert_eq!(h.channels[1].iter().sum::<u64>(), 383);
        assert_eq!(from_premultiplied(&[0; 16]).samples, 0);
    }
    #[test]
    fn tiled_histogram_uses_straight_alpha_and_bounded_sampling() {
        let mut tiles = SparseTiles::new(2, 1).unwrap();
        tiles
            .write_rect(0, 0, 2, 1, &[255, 0, 0, 128, 0, 255, 0, 0])
            .unwrap();
        let h = from_tiles(&tiles, 2, 1);
        assert_eq!(h.samples, 4);
        assert_eq!(h.channels[1][255], 512);
        assert_eq!(h.channels[2][255], 0);
        assert_eq!(h.channels[0].iter().sum::<u64>(), 1536);
        let empty = SparseTiles::new(8192, 8192).unwrap();
        assert_eq!(from_tiles(&empty, 8192, 8192).samples, 0);
    }
    #[test]
    fn isolated_histogram_ignores_effects_other_layers_visibility_and_opacity() {
        use lumapaint_core::{
            document::{CanvasColor, Document},
            layer_effects::LayerEffects,
        };
        let mut state = Document::default().document_state();
        state.width = 2;
        state.height = 1;
        let mut doc = Document::from_document_state(state).unwrap();
        doc.import_svg("Input".into(), r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1"><path d="M0 0H1V1H0Z" fill="rgb(64,128,192)"/></svg>"#.into()).unwrap();
        let id = doc.svg_layers().next().unwrap().id.clone();
        doc.import_svg("Other".into(), r#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1"><path d="M0 0H2V1H0Z" fill="red"/></svg>"#.into()).unwrap();
        let mut state = doc.document_state();
        state.svg_layers[0].visible = false;
        state.svg_layers[0].opacity = 0.1;
        let mut effects = LayerEffects {
            enabled: true,
            ..Default::default()
        };
        effects.values[0] = 5.;
        state.layer_effects.insert(id.clone(), effects);
        let doc = Document::from_document_state(state).unwrap();
        let before = serde_json::to_vec(&doc.document_state()).unwrap();
        let view = doc.effect_histogram_view(&id).unwrap();
        let h = crate::thumbnails::effect_histogram(&view).unwrap();
        assert_eq!(h.samples, 1);
        assert_eq!(h.channels[1][64], 255);
        assert_eq!(h.channels[2][128], 255);
        assert_eq!(h.channels[3][192], 255);
        let white =
            crate::thumbnails::effect_histogram(&doc.effect_histogram_view("layer-1").unwrap())
                .unwrap();
        assert_eq!(white.samples, 2);
        assert_eq!(white.channels[1][255], 510);
        assert!(doc.effect_histogram_view("missing").is_err());
        assert_eq!(before, serde_json::to_vec(&doc.document_state()).unwrap());
        let mut state = doc.document_state();
        state.canvas_color = Some(CanvasColor::Transparent);
        let transparent = Document::from_document_state(state).unwrap();
        assert_eq!(
            crate::thumbnails::effect_histogram(
                &transparent.effect_histogram_view("layer-1").unwrap()
            )
            .unwrap()
            .samples,
            0
        );
    }
}
