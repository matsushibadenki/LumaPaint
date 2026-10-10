//! Viewport-sized exterior image. The model and all export/save dimensions remain unchanged.
use super::{vector, Document, PreparedSvgLayer, Viewport};
use lumapaint_core::document::SvgLayer;
use std::sync::Arc;

type SharedPreparedLayer = PreparedSvgLayer<Arc<Vec<u8>>>;

struct RetainedLayer {
    layer: SvgLayer,
    effects: lumapaint_core::layer_effects::LayerEffects,
    size: [u32; 2],
    pixels: Arc<Vec<u8>>,
    source_pixels: Arc<Vec<u8>>,
    source_revision: u64,
    deferred: bool,
}

pub(super) struct WorkspaceImage {
    pub image: SharedPreparedLayer,
    pub effect: Option<(lumapaint_core::layer_effects::LayerEffects, u64, [f32; 4])>,
}

// A failed GPU effect must never adjust the raw pixels retained by the workspace.
pub(super) fn apply_cpu_fallback(
    image: &mut SharedPreparedLayer,
    effects: &lumapaint_core::layer_effects::LayerEffects,
    mapping: [f32; 4],
) {
    let pixels = Arc::make_mut(&mut image.pixels);
    crate::screentone::apply_cpu(pixels, effects, image.size.0, mapping);
    apply_opacity(pixels, image.opacity);
}

#[derive(Default)]
pub(super) struct WorkspaceCache {
    key: Option<[f32; 8]>,
    journal_cursor: u64,
    gpu_display: bool,
    layers: Vec<String>,
    retained: std::collections::HashMap<String, RetainedLayer>,
    #[cfg(test)]
    rasterizations: usize,
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
    ) -> Result<Option<Vec<SharedPreparedLayer>>, String> {
        let mut prepared = self
            .prepare_filtered(document, viewport, offset, &Default::default(), None)?
            .map(|images| {
                images
                    .into_iter()
                    .map(|item| item.image)
                    .collect::<Vec<_>>()
            });
        if let Some(images) = &mut prepared {
            for image in images {
                if image.pixels.is_empty() {
                    let retained = &self.retained[&image.id];
                    image.pixels = if retained.pixels.is_empty() {
                        retained.source_pixels.clone()
                    } else {
                        retained.pixels.clone()
                    };
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
        gpu_limits: Option<&wgpu::Limits>,
    ) -> Result<Option<Vec<WorkspaceImage>>, String> {
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
        if self.gpu_display == gpu_limits.is_some()
            && self.key == Some(key)
            && self.layers.len() == layers.len()
            && self.layers.iter().zip(&layers).all(|(id, layer)| {
                *id == layer.id
                    && self.retained.get(id).is_some_and(|old| {
                        old.layer.source == layer.source
                            && old.layer.effective_opacity() == layer.effective_opacity()
                            && old.effects == document.layer_effects(&layer.id)
                    })
            })
        {
            return Ok(None);
        }
        let rect = world_rect(viewport);
        // Bound raw and adjusted retained images to 32 MiB each (64 MiB total).
        let max_pixels = 32 * 1024 * 1024 / 4 / layers.len().max(1);
        let area = viewport.width as usize * viewport.height as usize;
        let ratio = (max_pixels as f64 / area as f64).sqrt().min(1.0);
        let size = [
            (viewport.width as f64 * ratio).floor().max(1.0) as u32,
            (viewport.height as f64 * ratio).floor().max(1.0) as u32,
        ];
        // Only range availability is needed here, not the event payloads.
        let journal = document.scene_journal();
        let cursor = journal.cursor();
        let complete_history = journal.read_borrowed(self.journal_cursor).is_some();
        lumapaint_core::performance::count("workspace_journal_range_checks", 1);
        let same_view =
            self.gpu_display == gpu_limits.is_some() && self.key == Some(key) && complete_history;
        self.gpu_display = gpu_limits.is_some();
        // A failed raster must not leave a valid key after moving old payloads.
        self.key = None;
        let mut retained = std::collections::HashMap::new();
        let mut prepared = Vec::with_capacity(layers.len());
        for layer in &layers {
            let effects = document.layer_effects(&layer.id);
            let opacity = layer.effective_opacity();
            let deferred = effects.active()
                && gpu_limits.is_some_and(|limits| {
                    super::effects_display::layout((size[0], size[1]), limits).is_some()
                });
            // Unadjusted CPU display also needs only the raw retained image.
            let raw_display = deferred || (!effects.active() && opacity == 1.0);
            let old = self
                .retained
                .remove(&layer.id)
                .filter(|old| same_view && old.size == size && old.deferred == deferred);
            if old.as_ref().is_some_and(|old| {
                old.layer.source == layer.source
                    && old.layer.effective_opacity() == opacity
                    && old.effects == effects
            }) {
                prepared.push(WorkspaceImage {
                    effect: None,
                    image: PreparedSvgLayer {
                        id: layer.id.clone(),
                        source: layer.id.clone(),
                        opacity,
                        size: (size[0], size[1]),
                        fully_contained: true,
                        pixels: Default::default(),
                    },
                });
                retained.insert(layer.id.clone(), old.unwrap());
                continue;
            }
            let same_source = old
                .as_ref()
                .is_some_and(|old| old.layer.source == layer.source);
            let tiles = old
                .as_ref()
                .filter(|old| old.effects == effects && old.layer.effective_opacity() == opacity)
                .and_then(|old| dirty_tiles(&old.layer, layer, document.dimensions(), size, rect));
            let source_revision = if same_source {
                old.as_ref().unwrap().source_revision
            } else {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            };
            let (source_pixels, mut pixels, retained_layer) = if same_source {
                // Effects/opacity changes never feed adjusted pixels back into the filter.
                let old = old.unwrap();
                let mut retained_layer = old.layer;
                retained_layer.opacity = layer.opacity;
                retained_layer.mask_enabled = layer.mask_enabled;
                retained_layer.mask_inverted = layer.mask_inverted;
                retained_layer.mask_density = layer.mask_density;
                if retained_layer.vector_objects != layer.vector_objects {
                    retained_layer.vector_objects = layer.vector_objects.clone();
                } else {
                    lumapaint_core::performance::count(
                        "workspace_retained_object_snapshot_reuses",
                        1,
                    );
                }
                lumapaint_core::performance::count(
                    "workspace_retained_source_bytes_reused",
                    retained_layer.source.len() as u64,
                );
                let source_pixels = old.source_pixels;
                let mut pixels = old.pixels;
                if !raw_display {
                    // Every adjusted byte is replaced; do not copy the old shared image.
                    if Arc::get_mut(&mut pixels).is_none() {
                        pixels = Default::default();
                    }
                    let pixels = Arc::make_mut(&mut pixels);
                    pixels.resize(source_pixels.len(), 0);
                    pixels.copy_from_slice(&source_pixels);
                    if effects.screentone.is_some() || effects.mask.is_some() {
                        crate::screentone::apply(pixels, &effects, size[0], rect);
                    } else {
                        crate::apply_layer_effects_source(pixels, &effects, source_revision);
                    }
                    apply_opacity(pixels, opacity);
                }
                (source_pixels, pixels, retained_layer)
            } else if let Some(tiles) = tiles {
                let old = old.unwrap();
                let mut source_pixels = old.source_pixels;
                let mut pixels = old.pixels;
                let source_output = Arc::make_mut(&mut source_pixels);
                let mut adjusted_output = (!raw_display).then(|| Arc::make_mut(&mut pixels));
                for [x, y, width, height] in tiles {
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
                    #[cfg(test)]
                    {
                        self.rasterizations += 1;
                    }
                    let mut tile = vector::rasterize_svg_workspace(
                        &layer.source,
                        [document.dimensions().0, document.dimensions().1],
                        [right - left, bottom - top],
                        tile_rect,
                    )?;
                    for row in y..y + height {
                        let from = ((row - top) * (right - left) + x - left) as usize * 4;
                        let to = (row * size[0] + x) as usize * 4;
                        source_output[to..to + width as usize * 4]
                            .copy_from_slice(&tile[from..from + width as usize * 4]);
                    }
                    if !raw_display {
                        crate::screentone::apply(&mut tile, &effects, right - left, tile_rect);
                        apply_opacity(&mut tile, opacity);
                    }
                    if !raw_display {
                        for row in y..y + height {
                            let from = ((row - top) * (right - left) + x - left) as usize * 4;
                            let to = (row * size[0] + x) as usize * 4;
                            adjusted_output.as_deref_mut().unwrap()[to..to + width as usize * 4]
                                .copy_from_slice(&tile[from..from + width as usize * 4]);
                        }
                    }
                }
                (source_pixels, pixels, (*layer).clone())
            } else {
                #[cfg(test)]
                {
                    self.rasterizations += 1;
                }
                let source_pixels = vector::rasterize_svg_workspace(
                    &layer.source,
                    [document.dimensions().0, document.dimensions().1],
                    size,
                    rect,
                )?;
                let mut pixels = if raw_display {
                    Vec::new()
                } else {
                    source_pixels.clone()
                };
                if !raw_display {
                    if effects.screentone.is_some() || effects.mask.is_some() {
                        crate::screentone::apply(&mut pixels, &effects, size[0], rect);
                    } else {
                        crate::apply_layer_effects_source(&mut pixels, &effects, source_revision);
                    }
                    apply_opacity(&mut pixels, opacity);
                }
                (Arc::new(source_pixels), Arc::new(pixels), (*layer).clone())
            };
            let upload_pixels = if raw_display {
                lumapaint_core::performance::count(
                    if deferred {
                        "workspace_deferred_duplicate_bytes_avoided"
                    } else {
                        "workspace_cpu_raw_duplicate_bytes_avoided"
                    },
                    source_pixels.len() as u64,
                );
                pixels = Default::default();
                Arc::clone(&source_pixels)
            } else {
                Arc::clone(&pixels)
            };
            lumapaint_core::performance::count(
                "workspace_upload_bytes_shared",
                upload_pixels.len() as u64,
            );
            retained.insert(
                layer.id.clone(),
                RetainedLayer {
                    layer: retained_layer,
                    effects: effects.clone(),
                    deferred,
                    size,
                    source_pixels,
                    source_revision,
                    pixels,
                },
            );
            prepared.push(WorkspaceImage {
                effect: deferred.then_some((
                    effects,
                    source_revision,
                    [
                        rect[0],
                        rect[1],
                        rect[2] / size[0] as f32,
                        rect[3] / size[1] as f32,
                    ],
                )),
                image: PreparedSvgLayer {
                    id: layer.id.clone(),
                    source: layer.id.clone(),
                    opacity,
                    size: (size[0], size[1]),
                    fully_contained: true,
                    pixels: upload_pixels,
                },
            });
        }
        self.retained = retained;
        self.layers = layers.iter().map(|layer| layer.id.clone()).collect();
        lumapaint_core::performance::count(
            "workspace_snapshot_source_bytes_avoided",
            layers.iter().map(|layer| layer.source.len() as u64).sum(),
        );
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
    fn deferred_cpu_fallback_preserves_shared_source_and_matches_cpu_preparation() {
        let mut doc = Document::default();
        doc.import_svg("image".into(), r##"<svg width="960" height="640"><rect width="960" height="640" fill="#404040"/></svg>"##.into()).unwrap();
        let id = doc.svg_layers().next().unwrap().id.clone();
        let mut effects = lumapaint_core::layer_effects::LayerEffects {
            enabled: true,
            ..Default::default()
        };
        effects.values[0] = 1.;
        doc.set_layer_effects(&id, effects).unwrap();
        doc.set_layer_settings(lumapaint_core::document::LayerSettings {
            id: id.clone(),
            name: "image".into(),
            opacity: 0.5,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
        })
        .unwrap();
        let viewport = Viewport::new(960., 640., 1., 1., false).unwrap();
        let mut cache = WorkspaceCache::default();
        let mut item = cache
            .prepare_filtered(
                &doc,
                viewport,
                [0., 0.],
                &Default::default(),
                Some(&wgpu::Limits::default()),
            )
            .unwrap()
            .unwrap()
            .remove(0);
        let raw = Arc::clone(&cache.retained[&id].source_pixels);
        let original = raw.as_ref().clone();
        assert!(Arc::ptr_eq(&raw, &item.image.pixels));
        let (effects, _, mapping) = item.effect.take().unwrap();
        apply_cpu_fallback(&mut item.image, &effects, mapping);
        assert_eq!(raw.as_ref(), &original);
        assert!(!Arc::ptr_eq(&raw, &item.image.pixels));
        let fresh = WorkspaceCache::default()
            .prepare(&doc, viewport, [0., 0.])
            .unwrap()
            .unwrap()
            .remove(0);
        assert_eq!(item.image.pixels, fresh.pixels);
        cache.clear();
        assert_eq!(raw.as_ref(), &original);
    }

    #[test]
    fn deferred_effects_keep_raw_pixels_and_invalidate_on_mode_or_effect_changes() {
        let mut doc = Document::default();
        doc.import_svg("image".into(), r##"<svg width="960" height="640"><rect width="960" height="640" fill="#404040"/></svg>"##.into()).unwrap();
        let id = doc.svg_layers().next().unwrap().id.clone();
        let viewport = Viewport::new(960., 640., 1., 1., false).unwrap();
        let mut effects = lumapaint_core::layer_effects::LayerEffects {
            enabled: true,
            ..Default::default()
        };
        effects.values[0] = 1.;
        doc.set_layer_effects(&id, effects.clone()).unwrap();
        let limits = wgpu::Limits::default();
        let mut cache = WorkspaceCache::default();
        let prepare = |cache: &mut WorkspaceCache, doc: &Document| {
            cache.prepare_filtered(doc, viewport, [0., 0.], &Default::default(), Some(&limits))
        };
        let first = prepare(&mut cache, &doc).unwrap().unwrap().remove(0);
        let revision = first.effect.unwrap().1;
        let rasters = cache.rasterizations;
        let raw = first.image.pixels;
        assert!(cache.retained[&id].pixels.is_empty());
        assert_eq!(cache.retained[&id].source_pixels, raw);
        assert!(Arc::ptr_eq(&cache.retained[&id].source_pixels, &raw));
        let index = raw
            .as_chunks::<4>()
            .0
            .iter()
            .position(|p| p[3] == 255)
            .unwrap()
            * 4;
        assert_eq!(&raw[index..index + 4], &[64, 64, 64, 255]);
        assert!(prepare(&mut cache, &doc).unwrap().is_none());
        effects.values[0] = 2.;
        doc.set_layer_effects(&id, effects).unwrap();
        let changed = prepare(&mut cache, &doc).unwrap().unwrap().remove(0);
        assert_eq!(changed.effect.unwrap().1, revision);
        assert_eq!(changed.image.pixels, raw);
        assert!(Arc::ptr_eq(&changed.image.pixels, &raw));
        assert_eq!(cache.rasterizations, rasters);
        let cpu = cache
            .prepare(&doc, viewport, [0., 0.])
            .unwrap()
            .unwrap()
            .remove(0);
        assert_ne!(cpu.pixels, raw);
        let direct = prepare(&mut cache, &doc).unwrap().unwrap().remove(0);
        assert!(direct.effect.is_some());
        assert_eq!(direct.image.pixels, raw);
        doc.set_layer_effects(&id, Default::default()).unwrap();
        let disabled = prepare(&mut cache, &doc).unwrap().unwrap().remove(0);
        assert!(disabled.effect.is_none());
        assert_eq!(disabled.image.pixels, raw);
    }

    #[test]
    fn retained_snapshot_detects_same_length_source_changes() {
        let mut document = Document::default();
        document.import_svg("image".into(), r#"<svg width="960" height="640"><rect width="960" height="640" fill="red"/></svg>"#.into()).unwrap();
        let viewport = Viewport::new(96., 64., 1., 1., false).unwrap();
        let mut cache = WorkspaceCache::default();
        let before = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        assert!(cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .is_none());
        let mut state = document.document_state();
        let source = &mut state.svg_layers[0].source;
        let length = source.len();
        *source = source.replace("red", "tan");
        assert_eq!(source.len(), length);
        let changed = Document::from_document_state(state).unwrap();
        let after = cache
            .prepare(&changed, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        assert_ne!(before[0].pixels, after[0].pixels);
        let fresh = WorkspaceCache::default()
            .prepare(&changed, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        assert_eq!(after[0].pixels, fresh[0].pixels);
        assert!(cache
            .prepare(&changed, viewport, [0., 0.])
            .unwrap()
            .is_none());
    }

    #[test]
    fn effects_invalidate_retained_pixels_without_changing_svg() {
        let mut d = Document::default();
        d.import_svg("image".into(),r##"<svg width="960" height="640"><rect width="960" height="640" fill="#404040"/></svg>"##.into()).unwrap();
        let id = d.svg_layers().next().unwrap().id.clone();
        let source = d.svg_layers().next().unwrap().source.clone();
        let viewport = Viewport::new(960., 640., 1., 1., false).unwrap();
        let mut cache = WorkspaceCache::default();
        lumapaint_core::performance::take();
        let before = cache.prepare(&d, viewport, [0., 0.]).unwrap().unwrap();
        let before_bytes = before[0].pixels.as_ref().clone();
        assert!(Arc::ptr_eq(
            &cache.retained[&id].source_pixels,
            &before[0].pixels
        ));
        let stats = lumapaint_core::performance::take();
        if lumapaint_core::performance::enabled() {
            assert_eq!(
                stats.counts["workspace_snapshot_source_bytes_avoided"],
                source.len() as u64
            );
        }
        assert_eq!(cache.layers.as_slice(), std::slice::from_ref(&id));
        assert!(cache.retained[&id].pixels.is_empty());
        assert_eq!(cache.retained[&id].source_pixels, before[0].pixels);
        if lumapaint_core::performance::enabled() {
            assert_eq!(
                stats.counts["workspace_cpu_raw_duplicate_bytes_avoided"],
                before[0].pixels.len() as u64
            );
        }
        let rasters = cache.rasterizations;
        let revision = cache.retained[&id].source_revision;
        let source_pointer = cache.retained[&id].layer.source.as_ptr();
        assert!(cache.prepare(&d, viewport, [0., 0.]).unwrap().is_none());
        let mut e = lumapaint_core::layer_effects::LayerEffects {
            enabled: true,
            ..Default::default()
        };
        e.values[0] = 1.;
        d.set_layer_effects(&id, e).unwrap();
        lumapaint_core::performance::take();
        let after = cache.prepare(&d, viewport, [0., 0.]).unwrap().unwrap();
        let stats = lumapaint_core::performance::take();
        if lumapaint_core::performance::enabled() {
            assert_eq!(
                stats.counts["workspace_retained_source_bytes_reused"],
                source.len() as u64
            );
            assert_eq!(stats.counts["workspace_retained_object_snapshot_reuses"], 1);
            assert_eq!(
                stats.counts["workspace_upload_bytes_shared"],
                after[0].pixels.len() as u64
            );
        }
        assert!(Arc::ptr_eq(&cache.retained[&id].pixels, &after[0].pixels));
        assert_eq!(before[0].pixels.as_ref(), &before_bytes);
        assert_eq!(cache.retained[&id].layer.source.as_ptr(), source_pointer);
        assert_eq!(cache.rasterizations, rasters);
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
        assert_eq!(cache.rasterizations, rasters);
        assert_eq!(undone[0].pixels, before[0].pixels);
        assert!(cache.retained[&id].pixels.is_empty());
        let sampled = crate::color_sampler::ColorSampler::default()
            .sample(&d, lumapaint_core::document::Point { x: 100., y: 100. })
            .unwrap();
        assert_eq!(sampled, Some([64, 64, 64]));
        d.redo();
        let redone = cache.prepare(&d, viewport, [0., 0.]).unwrap().unwrap();
        assert_eq!(cache.rasterizations, rasters);
        assert!(redone[0].pixels == after[0].pixels);
        assert_eq!(
            crate::color_sampler::ColorSampler::default()
                .sample(&d, lumapaint_core::document::Point { x: 100., y: 100. })
                .unwrap(),
            Some([90, 90, 90])
        );
        d.set_layer_settings(lumapaint_core::document::LayerSettings {
            id: id.clone(),
            name: "image".into(),
            opacity: 0.5,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
        })
        .unwrap();
        let translucent = cache.prepare(&d, viewport, [0., 0.]).unwrap().unwrap();
        assert_eq!(cache.rasterizations, rasters);
        let full = WorkspaceCache::default()
            .prepare(&d, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        assert!(translucent[0].pixels == full[0].pixels);
        assert_eq!(cache.retained[&id].source_revision, revision);
        assert_eq!(cache.retained[&id].layer.source.as_ptr(), source_pointer);
        cache.clear();
        cache.prepare(&d, viewport, [0., 0.]).unwrap();
        assert_ne!(cache.retained[&id].source_revision, revision);
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
        let original = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let original_bytes = original[0].pixels.as_ref().clone();
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
        lumapaint_core::performance::take();
        let incremental = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        assert_eq!(original[0].pixels.as_ref(), &original_bytes);
        assert!(Arc::ptr_eq(
            &cache.retained[&layer].source_pixels,
            &incremental[0].pixels
        ));
        let stats = lumapaint_core::performance::take();
        if lumapaint_core::performance::enabled() {
            assert_eq!(stats.counts["workspace_journal_range_checks"], 1);
            assert!(!stats
                .counts
                .contains_key("scene_journal_read_cloned_events"));
        }
        let full = WorkspaceCache::default()
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let max_error = incremental[0]
            .pixels
            .iter()
            .zip(full[0].pixels.iter())
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
        document.redo();
        cache.journal_cursor = document.scene_journal().cursor() + 1;
        let before_rasterizations = cache.rasterizations;
        let rebuilt = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let fresh = WorkspaceCache::default()
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        assert_eq!(cache.rasterizations, before_rasterizations + 1);
        assert_eq!(rebuilt[0].pixels, fresh[0].pixels);

        let mut effects = lumapaint_core::layer_effects::LayerEffects {
            enabled: true,
            ..Default::default()
        };
        effects.values[0] = 1.;
        document.set_layer_effects(&layer, effects).unwrap();
        let limits = wgpu::Limits::default();
        cache
            .prepare_filtered(
                &document,
                viewport,
                [0., 0.],
                &Default::default(),
                Some(&limits),
            )
            .unwrap();
        document
            .select_vector_objects(vec!["moving".into()])
            .unwrap();
        document.move_selected_vectors(5., 0.).unwrap();
        let incremental = cache
            .prepare_filtered(
                &document,
                viewport,
                [0., 0.],
                &Default::default(),
                Some(&limits),
            )
            .unwrap()
            .unwrap();
        let fresh = WorkspaceCache::default()
            .prepare_filtered(
                &document,
                viewport,
                [0., 0.],
                &Default::default(),
                Some(&limits),
            )
            .unwrap()
            .unwrap();
        assert!(cache.retained[&layer].pixels.is_empty());
        assert!(incremental[0]
            .image
            .pixels
            .iter()
            .zip(fresh[0].image.pixels.iter())
            .all(|(a, b)| a.abs_diff(*b) <= 1));
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
            .zip(full[0].pixels.iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(max_error <= 1, "clipped tile seam error: {max_error}");
        let rasters = cache.rasterizations;
        let mut effects = lumapaint_core::layer_effects::LayerEffects {
            enabled: true,
            ..Default::default()
        };
        effects.values[9] = -20.;
        document.set_layer_effects(&layer, effects).unwrap();
        let adjusted = cache
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        assert_eq!(cache.rasterizations, rasters);
        let full = WorkspaceCache::default()
            .prepare(&document, viewport, [0., 0.])
            .unwrap()
            .unwrap();
        let max_error = adjusted[0]
            .pixels
            .iter()
            .zip(full[0].pixels.iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(
            max_error <= 1,
            "raw cache after clipped tile edit: {max_error}"
        );
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
