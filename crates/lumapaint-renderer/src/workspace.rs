//! Viewport-sized exterior image. The model and all export/save dimensions remain unchanged.
use super::{vector, Document, PreparedSvgLayer, Viewport};
use lumapaint_core::document::SvgLayer;

struct RetainedLayer {
    layer: SvgLayer,
    effects: lumapaint_core::layer_effects::LayerEffects,
    size: [u32; 2],
    pixels: Vec<u8>,
}

#[derive(Default)]
pub(super) struct WorkspaceCache {
    key: Option<[f32; 8]>,
    journal_cursor: u64,
    layers: Vec<(
        String,
        String,
        f32,
        lumapaint_core::layer_effects::LayerEffects,
    )>,
    retained: std::collections::HashMap<String, RetainedLayer>,
}

impl WorkspaceCache {
    pub fn clear(&mut self) {
        self.key = None;
        self.journal_cursor = 0;
        self.layers.clear();
        self.retained.clear();
    }
    #[cfg(test)]
    pub fn prepare(
        &mut self,
        document: &Document,
        viewport: Viewport,
        offset: [f32; 2],
    ) -> Result<Option<Vec<PreparedSvgLayer>>, String> {
        let mut prepared =
            self.prepare_filtered(document, viewport, offset, &Default::default())?;
        if let Some(images) = &mut prepared {
            for image in images {
                if image.pixels.is_empty() {
                    image.pixels = self.retained[&image.id].pixels.clone();
                }
            }
        }
        Ok(prepared)
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
                .all(|((id, source, opacity, effects), layer)| {
                    *id == layer.id
                        && *source == layer.source
                        && *opacity == layer.effective_opacity()
                        && *effects == document.layer_effects(&layer.id)
                })
        {
            return Ok(None);
        }
        let rect = world_rect(viewport);
        // Bound each retained image set to 32 MiB across visible layers.
        let max_pixels = 32 * 1024 * 1024 / 4 / layers.len().max(1);
        let area = viewport.width as usize * viewport.height as usize;
        let ratio = (max_pixels as f64 / area as f64).sqrt().min(1.0);
        let size = [
            (viewport.width as f64 * ratio).floor().max(1.0) as u32,
            (viewport.height as f64 * ratio).floor().max(1.0) as u32,
        ];
        let journal = document.scene_journal().read(self.journal_cursor);
        let (cursor, complete_history) = match journal {
            lumapaint_core::scene::JournalRead::Incremental { cursor, .. } => (cursor, true),
            lumapaint_core::scene::JournalRead::Rebuild { cursor } => (cursor, false),
        };
        let same_view = self.key == Some(key) && complete_history;
        let mut retained = std::collections::HashMap::new();
        let mut prepared = Vec::with_capacity(layers.len());
        for layer in &layers {
            let effects = document.layer_effects(&layer.id);
            let opacity = layer.effective_opacity();
            let old = self
                .retained
                .get(&layer.id)
                .filter(|old| same_view && old.size == size && old.effects == effects);
            if let Some(old) = old.filter(|old| {
                old.layer.source == layer.source && old.layer.effective_opacity() == opacity
            }) {
                prepared.push(PreparedSvgLayer {
                    id: layer.id.clone(),
                    source: layer.id.clone(),
                    opacity,
                    size: (size[0], size[1]),
                    fully_contained: true,
                    pixels: Vec::new(),
                });
                // Move unchanged payloads after traversal, without copying them.
                let _ = old;
                continue;
            }
            let mut pixels = if let Some((old, tiles)) = old.and_then(|old| {
                dirty_tiles(&old.layer, layer, document.dimensions(), size, rect)
                    .map(|tiles| (old, tiles))
            }) {
                let mut pixels = old.pixels.clone();
                for [x, y, width, height] in tiles {
                    // Render every contributor in its original order, including
                    // group masks and transparency, rather than drawing only the edit.
                    // A two-pixel gutter keeps antialiasing independent of tile edges.
                    let left = x.saturating_sub(2);
                    let top = y.saturating_sub(2);
                    let right = (x + width + 2).min(size[0]);
                    let bottom = (y + height + 2).min(size[1]);
                    let tile_rect = [
                        rect[0] + left as f32 * rect[2] / size[0] as f32,
                        rect[1] + top as f32 * rect[3] / size[1] as f32,
                        (right - left) as f32 * rect[2] / size[0] as f32,
                        (bottom - top) as f32 * rect[3] / size[1] as f32,
                    ];
                    let mut tile = vector::rasterize_svg_workspace(
                        &layer.source,
                        [document.dimensions().0, document.dimensions().1],
                        [right - left, bottom - top],
                        tile_rect,
                    )?;
                    crate::apply_layer_effects(&mut tile, &effects);
                    apply_opacity(&mut tile, opacity);
                    for row in y..y + height {
                        let from = ((row - top) * (right - left) + x - left) as usize * 4;
                        let to = (row * size[0] + x) as usize * 4;
                        pixels[to..to + width as usize * 4]
                            .copy_from_slice(&tile[from..from + width as usize * 4]);
                    }
                }
                pixels
            } else {
                let mut pixels = vector::rasterize_svg_workspace(
                    &layer.source,
                    [document.dimensions().0, document.dimensions().1],
                    size,
                    rect,
                )?;
                crate::apply_layer_effects(&mut pixels, &effects);
                apply_opacity(&mut pixels, opacity);
                pixels
            };
            retained.insert(
                layer.id.clone(),
                RetainedLayer {
                    layer: (*layer).clone(),
                    effects,
                    size,
                    pixels: pixels.clone(),
                },
            );
            prepared.push(PreparedSvgLayer {
                id: layer.id.clone(),
                source: layer.id.clone(),
                opacity,
                size: (size[0], size[1]),
                fully_contained: true,
                pixels: std::mem::take(&mut pixels),
            });
        }
        for image in &prepared {
            if image.pixels.is_empty() {
                if let Some(old) = self.retained.remove(&image.id) {
                    retained.insert(image.id.clone(), old);
                }
            }
        }
        self.retained = retained;
        self.layers = layers
            .iter()
            .map(|layer| {
                (
                    layer.id.clone(),
                    layer.source.clone(),
                    layer.effective_opacity(),
                    document.layer_effects(&layer.id),
                )
            })
            .collect();
        self.key = Some(key);
        self.journal_cursor = cursor;
        Ok(Some(prepared))
    }
}

fn apply_opacity(pixels: &mut [u8], opacity: f32) {
    if opacity != 1.0 {
        for value in pixels {
            *value = (*value as f32 * opacity).round() as u8;
        }
    }
}

/// Unknown imported CSS, filters or text metrics require a complete update.
/// Canonical portable objects have conservative bounds, including stroke/arrows.
fn dirty_tiles(
    before: &SvgLayer,
    after: &SvgLayer,
    document: (u32, u32),
    size: [u32; 2],
    rect: [f32; 4],
) -> Option<Vec<[u32; 4]>> {
    use lumapaint_core::document::vector_svg;
    if before.effective_opacity() != after.effective_opacity()
        || before.source != vector_svg(document.0, document.1, &before.vector_objects)
        || after.source != vector_svg(document.0, document.1, &after.vector_objects)
    {
        return None;
    }
    let mut dirty = std::collections::BTreeSet::new();
    let old: std::collections::HashMap<_, _> = before
        .vector_objects
        .iter()
        .enumerate()
        .map(|(i, o)| (&o.id, (i, o)))
        .collect();
    let new: std::collections::HashMap<_, _> = after
        .vector_objects
        .iter()
        .enumerate()
        .map(|(i, o)| (&o.id, (i, o)))
        .collect();
    for (id, (index, object)) in &old {
        if new
            .get(id)
            .is_some_and(|(next_index, next)| index == next_index && object == next)
        {
            continue;
        }
        mark_tiles(
            &mut dirty,
            object.conservative_drawing_bounds()?,
            size,
            rect,
        );
        if let Some((_, next)) = new.get(id) {
            mark_tiles(&mut dirty, next.conservative_drawing_bounds()?, size, rect);
        }
    }
    for (id, (_, object)) in &new {
        if !old.contains_key(id) {
            mark_tiles(
                &mut dirty,
                object.conservative_drawing_bounds()?,
                size,
                rect,
            );
        }
    }
    Some(
        dirty
            .into_iter()
            .map(|(x, y)| [x, y, 256.min(size[0] - x), 256.min(size[1] - y)])
            .collect(),
    )
}
fn mark_tiles(
    tiles: &mut std::collections::BTreeSet<(u32, u32)>,
    bounds: [f64; 4],
    size: [u32; 2],
    rect: [f32; 4],
) {
    let precision = bounds
        .iter()
        .fold(1.0_f64, |maximum, value| maximum.max(value.abs()))
        * f64::from(f32::EPSILON)
        * 8.0;
    let pixel = |v: f64, axis: usize| {
        (v - f64::from(rect[axis])) * f64::from(size[axis]) / f64::from(rect[axis + 2])
    };
    let left =
        (pixel(bounds[0] - precision, 0).floor() - 2.0).clamp(0.0, f64::from(size[0])) as u32;
    let top = (pixel(bounds[1] - precision, 1).floor() - 2.0).clamp(0.0, f64::from(size[1])) as u32;
    let right =
        (pixel(bounds[2] + precision, 0).ceil() + 2.0).clamp(0.0, f64::from(size[0])) as u32;
    let bottom =
        (pixel(bounds[3] + precision, 1).ceil() + 2.0).clamp(0.0, f64::from(size[1])) as u32;
    if right <= left || bottom <= top {
        return;
    }
    for y in (top / 256 * 256..bottom).step_by(256) {
        for x in (left / 256 * 256..right).step_by(256) {
            tiles.insert((x, y));
        }
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
    fn effects_invalidate_retained_pixels_without_changing_svg() {
        let mut d = Document::default();
        d.import_svg("image".into(),r##"<svg width="960" height="640"><rect width="960" height="640" fill="#404040"/></svg>"##.into()).unwrap();
        let id = d.svg_layers().next().unwrap().id.clone();
        let source = d.svg_layers().next().unwrap().source.clone();
        let viewport = Viewport::new(960., 640., 1., 1., false).unwrap();
        let mut cache = WorkspaceCache::default();
        let before = cache.prepare(&d, viewport, [0., 0.]).unwrap().unwrap();
        assert!(cache.prepare(&d, viewport, [0., 0.]).unwrap().is_none());
        let mut e = lumapaint_core::layer_effects::LayerEffects {
            enabled: true,
            ..Default::default()
        };
        e.values[0] = 1.;
        d.set_layer_effects(&id, e).unwrap();
        let after = cache.prepare(&d, viewport, [0., 0.]).unwrap().unwrap();
        let index = before[0]
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .position(|p| p[3] == 255)
            .unwrap()
            * 4;
        assert_eq!(&before[0].pixels[index..index + 4], &[64, 64, 64, 255]);
        assert_eq!(&after[0].pixels[index..index + 4], &[90, 90, 90, 255]);
        assert_eq!(d.svg_layers().next().unwrap().source, source);
        assert!(cache.prepare(&d, viewport, [0., 0.]).unwrap().is_none());
        d.undo();
        let undone = cache.prepare(&d, viewport, [0., 0.]).unwrap().unwrap();
        assert_eq!(undone[0].pixels, before[0].pixels);
        let sampled = crate::color_sampler::ColorSampler::default()
            .sample(&d, lumapaint_core::document::Point { x: 100., y: 100. })
            .unwrap();
        assert_eq!(sampled, Some([64, 64, 64]));
        d.redo();
        assert_eq!(
            crate::color_sampler::ColorSampler::default()
                .sample(&d, lumapaint_core::document::Point { x: 100., y: 100. })
                .unwrap(),
            Some([90, 90, 90])
        );
    }
    #[test]
    fn dirty_tiles_match_full_transparent_composition_across_tile_edges() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        let rectangle = |id: &str, x: f32, color: [u8; 4]| {
            use lumapaint_core::vector::*;
            VectorObject {
                id: id.into(),
                name: id.into(),
                opacity: 0.5,
                blend_mode: "normal".into(),
                group_path: vec![],
                clipping_group: None,
                bounds_reset: false,
                path: VectorPath {
                    data: "M0 0H70V100H0Z".into(),
                    fill_rule: FillRule::NonZero,
                },
                transform: [1., 0., 0., 1., x, 200.],
                image_frame: None,
                fill_gradient: None,
                stroke_gradient: None,
                fill: Some(VectorPaint {
                    registration: false,
                    color,
                }),
                stroke: None,
                stroke_width: 0.,
                stroke_style: Default::default(),
                live_corners: None,
                rectangle_radii: None,
                visible: true,
                kind: VectorObjectKind::Rectangle,
                control_points: vec![[0., 0.], [70., 0.], [70., 100.], [0., 100.]],
                text: None,
            }
        };
        let background = rectangle("background", 225., [0, 0, 255, 255]);
        let moving = rectangle("moving", 245., [255, 0, 0, 255]);
        document.upsert_vector_object(&layer, background).unwrap();
        document
            .upsert_vector_object(&layer, moving.clone())
            .unwrap();
        let viewport = Viewport::new(960., 640., 1., 1., false).unwrap();
        let mut cache = WorkspaceCache::default();
        cache.prepare(&document, viewport, [0., 0.]).unwrap();
        let before = document
            .svg_layers()
            .find(|l| l.id == layer)
            .unwrap()
            .clone();
        let mut moved = moving;
        moved.transform[4] += 45.;
        document.upsert_vector_object(&layer, moved).unwrap();
        let after = document.svg_layers().find(|l| l.id == layer).unwrap();
        let tiles = dirty_tiles(
            &before,
            after,
            document.dimensions(),
            [960, 640],
            world_rect(viewport),
        )
        .unwrap();
        assert!(!tiles.is_empty());
        assert!(tiles.len() < 12);
        let incremental = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let full = WorkspaceCache::default()
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let max_error = incremental[0]
            .pixels
            .iter()
            .zip(&full[0].pixels)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(max_error <= 1, "tile seam error: {max_error}");
        document.undo();
        let restored = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let full = WorkspaceCache::default()
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        assert_eq!(restored[0].pixels, full[0].pixels);
        let mut imported = before.clone();
        imported.source = imported
            .source
            .replace("<svg ", "<svg style=\"display:none\" ");
        assert!(dirty_tiles(
            &before,
            &imported,
            document.dimensions(),
            [960, 640],
            world_rect(viewport)
        )
        .is_none());
        // A changed clipping mask must erase its old contribution and include
        // unchanged content below it when the affected tiles are recomposed.
        let mut mask = document
            .svg_layers()
            .find(|l| l.id == layer)
            .unwrap()
            .vector_objects[1]
            .clone();
        let mut content = mask.clone();
        content.group_path = vec!["clip-group".into()];
        document.upsert_vector_object(&layer, content).unwrap();
        mask.id = "mask".into();
        mask.group_path = vec!["clip-group".into()];
        mask.clipping_group = Some("clip-group".into());
        document.upsert_vector_object(&layer, mask.clone()).unwrap();
        cache.prepare(&document, viewport, [0., 0.]).unwrap();
        mask.transform[4] += 45.;
        document.upsert_vector_object(&layer, mask).unwrap();
        let incremental = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let full = WorkspaceCache::default()
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let max_error = incremental[0]
            .pixels
            .iter()
            .zip(&full[0].pixels)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(max_error <= 1, "clipped tile seam error: {max_error}");
    }

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
