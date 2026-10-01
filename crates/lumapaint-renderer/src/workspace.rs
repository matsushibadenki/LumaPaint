//! Viewport-sized exterior image. The model and all export/save dimensions remain unchanged.
use super::{vector, Document, PreparedSvgLayer, Viewport};
use lumapaint_core::document::SvgLayer;

#[derive(Default)]
pub(super) struct WorkspaceCache {
    key: Option<[f32; 8]>,
    layers: Vec<(String, String, f32)>,
}

impl WorkspaceCache {
    pub fn clear(&mut self) {
        self.key = None;
        self.layers.clear();
    }
    #[cfg(test)]
    pub fn prepare(
        &mut self,
        document: &Document,
        viewport: Viewport,
        offset: [f32; 2],
    ) -> Result<Option<Vec<PreparedSvgLayer>>, String> {
        self.prepare_filtered(document, viewport, offset, &Default::default())
    }
    pub fn prepare_filtered(
        &mut self,
        document: &Document,
        viewport: Viewport,
        offset: [f32; 2],
        excluded: &std::collections::HashSet<&str>,
    ) -> Result<Option<Vec<PreparedSvgLayer>>, String> {
        let key = [
            viewport.width as f32,
            viewport.height as f32,
            viewport.scale,
            viewport.zoom,
            viewport.pan_x,
            viewport.pan_y,
            viewport.document_width,
            viewport.document_height,
        ];
        let previews: Vec<SvgLayer> = if offset != [0.0, 0.0] {
            document
                .visible_svg_layers()
                .filter(|layer| !excluded.contains(layer.id.as_str()))
                .map(|layer| {
                    document
                        .translated_vector_layer(layer, offset[0], offset[1])
                        .map(|preview| preview.unwrap_or_else(|| layer.clone()))
                })
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        let layers: Vec<&SvgLayer> = if offset == [0.0, 0.0] {
            document
                .visible_svg_layers()
                .filter(|layer| !excluded.contains(layer.id.as_str()))
                .collect()
        } else {
            previews.iter().collect()
        };
        if self.key == Some(key)
            && self.layers.len() == layers.len()
            && self
                .layers
                .iter()
                .zip(&layers)
                .all(|((id, source, opacity), layer)| {
                    *id == layer.id
                        && *source == layer.source
                        && *opacity == layer.effective_opacity()
                })
        {
            return Ok(None);
        }
        let rect = world_rect(viewport);
        // Limit additional exterior textures to 64 MiB across visible layers.
        let max_pixels = 64 * 1024 * 1024 / 4 / layers.len().max(1);
        let area = viewport.width as usize * viewport.height as usize;
        let ratio = (max_pixels as f64 / area as f64).sqrt().min(1.0);
        let size = [
            (viewport.width as f64 * ratio).floor().max(1.0) as u32,
            (viewport.height as f64 * ratio).floor().max(1.0) as u32,
        ];
        let mut prepared = Vec::with_capacity(layers.len());
        for layer in &layers {
            let mut pixels = vector::rasterize_svg_workspace(
                &layer.source,
                [document.dimensions().0, document.dimensions().1],
                size,
                rect,
            )?;
            let opacity = layer.effective_opacity();
            if opacity != 1.0 {
                for value in &mut pixels {
                    *value = (*value as f32 * opacity).round() as u8;
                }
            }
            prepared.push(PreparedSvgLayer {
                id: layer.id.clone(),
                source: layer.id.clone(),
                opacity,
                size: (size[0], size[1]),
                fully_contained: true,
                pixels,
            });
        }
        self.layers = layers
            .iter()
            .map(|layer| {
                (
                    layer.id.clone(),
                    layer.source.clone(),
                    layer.effective_opacity(),
                )
            })
            .collect();
        self.key = Some(key);
        Ok(Some(prepared))
    }
}

fn world_rect(viewport: Viewport) -> [f32; 4] {
    let first = viewport.document_point(0.0, 0.0);
    let last = viewport.document_point(
        viewport.width as f32 / viewport.scale,
        viewport.height as f32 / viewport.scale,
    );
    [first.x, first.y, last.x - first.x, last.y - first.y]
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_formats::native::NativeDocumentCodec;

    #[test]
    fn extreme_zoom_rasterizes_only_the_viewport_without_changing_the_document() {
        let mut document = Document::default();
        document.import_svg("detail".into(), r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="479.75" y="319.75" width="0.5" height="0.5" fill="red"/></svg>"#.into()).unwrap();
        let before = document.encode().unwrap();
        let viewport = Viewport::new(800.0, 500.0, 1.0, 1.0, false)
            .unwrap()
            .with_screen_zoom(640.0);
        let images = WorkspaceCache::default()
            .prepare(&document, viewport, [0.0, 0.0])
            .unwrap()
            .unwrap();
        assert_eq!(images[0].size, (800, 500));
        assert_eq!(images[0].pixels.len(), 800 * 500 * 4);
        let pixel =
            |x: usize, y: usize| &images[0].pixels[(y * 800 + x) * 4..(y * 800 + x) * 4 + 4];
        assert_eq!(pixel(400, 250), &[255, 0, 0, 255]);
        assert_eq!(pixel(200, 250), &[0, 0, 0, 0]);
        assert_eq!(pixel(600, 250), &[0, 0, 0, 0]);
        assert_eq!(document.encode().unwrap(), before);
    }

    #[test]
    fn outside_content_is_visible_without_changing_document_or_export_bounds() {
        let mut document = Document::default();
        document.import_svg("outside".into(), r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="-60" y="20" width="100" height="60" fill="red"/></svg>"#.into()).unwrap();
        let before = document.encode().unwrap();
        let viewport = Viewport::new(1024.0, 768.0, 1.0, 1.0, false).unwrap();
        let rect = world_rect(viewport);
        assert!(rect[0] < 0.0 && rect[1] < 0.0);
        let mut cache = WorkspaceCache::default();
        let prepared = cache
            .prepare(&document, viewport, [0.0, 0.0])
            .unwrap()
            .unwrap();
        let prepared = &prepared[0];
        let outside: usize = prepared
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(index, pixel)| {
                let world_x = rect[0]
                    + (*index % viewport.width as usize) as f32 * rect[2] / viewport.width as f32;
                world_x < 0.0 && pixel[3] > 0
            })
            .count();
        assert!(outside > 0);
        assert_eq!(prepared.size, (viewport.width, viewport.height));
        assert!(cache
            .prepare(&document, viewport, [0.0, 0.0])
            .unwrap()
            .is_none());
        let mut panned = viewport;
        panned.pan_x = 20.0;
        assert!(cache
            .prepare(&document, panned, [0.0, 0.0])
            .unwrap()
            .is_some());
        assert_eq!(document.encode().unwrap(), before);
        assert_eq!(document.dimensions(), (960, 640));
    }

    #[test]
    fn viewport_render_matches_document_pixels_in_overlap() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect x="-20" y="-20" width="80" height="80" fill="red" opacity=".5"/></svg>"#;
        let full = vector::rasterize_svg(source, 100, 100).unwrap();
        let expanded = vector::rasterize_svg_workspace(
            source,
            [100, 100],
            [140, 140],
            [-20.0, -20.0, 140.0, 140.0],
        )
        .unwrap();
        for y in 0..100 {
            assert_eq!(
                &expanded[((y + 20) * 140 + 20) * 4..((y + 20) * 140 + 120) * 4],
                &full.pixels[y * 100 * 4..(y + 1) * 100 * 4]
            );
        }
        assert!(expanded[..20 * 140 * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn layer_order_and_opacity_remain_separate_for_gpu_compositing() {
        let mut document = Document::default();
        for color in ["red", "blue"] {
            document.import_svg(color.into(), format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><rect x=\"-20\" width=\"100\" height=\"100\" fill=\"{color}\"/></svg>")).unwrap();
        }
        let mut state = document.document_state();
        state.svg_layers[1].opacity = 0.5;
        let document = Document::from_document_state(state).unwrap();
        let viewport = Viewport::new(1024.0, 768.0, 1.0, 1.0, false).unwrap();
        let prepared = WorkspaceCache::default()
            .prepare(&document, viewport, [0.0, 0.0])
            .unwrap()
            .unwrap();
        assert_eq!(prepared.len(), 2);
        assert_eq!(prepared[0].id, document.svg_layers().next().unwrap().id);
        assert_eq!(prepared[1].opacity, 0.5);
        assert!(prepared[0]
            .pixels
            .as_chunks::<4>()
            .0
            .contains(&[255, 0, 0, 255]));
        assert!(prepared[1]
            .pixels
            .as_chunks::<4>()
            .0
            .contains(&[0, 0, 128, 128]));
    }
}
