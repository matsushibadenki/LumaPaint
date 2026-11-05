//! Retain a whole pixel layer in GPU memory during translation. No document
//! mutation, image decoding, SVG generation or pixel upload on warm frames.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Request<'a> {
    pub layer: &'a str,
    pub offset: [f32; 2],
}

pub(super) struct Cache {
    layer: String,
    density: f32,
    revision: u64,
    journal: u64,
    bounds: [f32; 4],
    identity: object_cache_state::SourceIdentity,
    pub image: ObjectLayerCache,
}
impl Cache {
    pub fn matches(&self, document: &Document) -> bool {
        self.image.document_size == document.dimensions()
            && document.visible_svg_layers().any(|layer| {
                layer.id == self.layer
                    && layer.effective_opacity() == self.image.opacity
                    && self.identity == object_cache_state::SourceIdentity::capture(document, layer)
            })
    }
}

fn eligible(document: &Document, layer: &SvgLayer, copy: bool) -> bool {
    !layer.vector_layer
        && layer.vector_objects.is_empty()
        && !document.has_document_clipping()
        && !document.has_active_saved_path()
        && !document.has_layer_effects(&layer.id)
        // Copying transparent pixels needs the source's original group blending
        // color space; retain the exact compositing fallback for Option-drag.
        && !copy
}

impl Renderer {
    fn share_pixel_display(
        &mut self,
        document: &Document,
        layer: &SvgLayer,
        bounds: [f32; 4],
    ) -> Option<ObjectLayerCache> {
        let size = document.dimensions();
        let [x, y, w, h] = bounds;
        if x < 0. || y < 0. || x + w > size.0 as f32 || y + h > size.1 as f32 {
            return None;
        }
        let displayed = self.svg_cache.get(&layer.id)?;
        if displayed.size != size
            || displayed.opacity != layer.effective_opacity()
            || !displayed.matches_source(document, layer)
        {
            return None;
        }
        let bytes = size.0 as usize * size.1 as usize * 4;
        let in_use: usize = self.object_cache.values().map(|cache| cache.bytes).sum();
        if bytes + in_use > 64 * 1024 * 1024 {
            return None;
        }
        let rectangle = [0., 0., size.0 as f32, size.1 as f32];
        let buffer = crate::gpu_metrics::create_buffer_init!(
            self.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Retained pixel bounds"),
                contents: bytemuck::cast_slice(&rectangle),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }
        );
        let geometry = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Retained pixel bounds"),
            layout: &self.object_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        // Transfer ownership: the async SVG worker must not overwrite a texture
        // that the gesture still displays at its original local coordinates.
        let cached = self.svg_cache.remove(&layer.id)?;
        performance::count("pixel_drag_shared_display_images", 1);
        Some(ObjectLayerCache {
            state: object_cache_state::ObjectCacheState::new(document, layer, false),
            objects: Vec::new(),
            source: String::new(),
            selected_ids: Vec::new(),
            opacity: layer.effective_opacity(),
            document_size: size,
            bytes,
            upload_bytes: 0,
            runs: vec![ObjectRun {
                object_id: None,
                selected: false,
                cached,
                geometry,
                geometry_buffer: buffer,
                rectangle,
            }],
        })
    }
    pub fn pixel_drag_bounds(&self, document: &Document, layer_id: &str) -> Option<[f32; 4]> {
        self.pixel_drag_cache
            .as_ref()
            .filter(|cache| cache.layer == layer_id && cache.matches(document))
            .map(|cache| cache.bounds)
    }

    /// Certify a host-committed whole-layer translation against the gesture's
    /// original document/revision. All other edits invalidate the retained image.
    pub fn commit_pixel_drag(
        &mut self,
        document: &Document,
        layer_id: &str,
        previous_revision: u64,
        offset: [f32; 2],
    ) {
        let Some(cache) = self.pixel_drag_cache.as_mut() else {
            return;
        };
        let Some(layer) = document.visible_svg_layers().find(|l| l.id == layer_id) else {
            return;
        };
        if cache.layer != layer_id
            || cache.revision != previous_revision
            || cache.journal != document.scene_journal().instance_id()
            || document.revision() != previous_revision.wrapping_add(1)
            || !eligible(document, layer, false)
            || offset.iter().any(|v| !v.is_finite() || v.abs() >= 100_000.)
        {
            return;
        }
        for run in &mut cache.image.runs {
            run.rectangle[0] += offset[0];
            run.rectangle[1] += offset[1];
            crate::gpu_metrics::write_buffer!(
                self.queue,
                &run.geometry_buffer,
                0,
                bytemuck::cast_slice(&run.rectangle),
            );
        }
        cache.bounds[0] += offset[0];
        cache.bounds[1] += offset[1];
        cache.identity = object_cache_state::SourceIdentity::capture(document, layer);
        cache.revision = document.revision();
        performance::count("pixel_drag_commit_reuses", 1);
    }

    pub(super) fn retained_pixel_layer(
        &self,
        document: &Document,
        viewport: Viewport,
    ) -> Option<String> {
        let cache = self.pixel_drag_cache.as_ref()?;
        if self.channel != 0
            || self.outline_view
            || !cache.matches(document)
            || cache.density != (viewport.screen_zoom() * viewport.scale).ceil().max(1.)
        {
            return None;
        }
        let layer = document
            .visible_svg_layers()
            .find(|layer| layer.id == cache.layer)?;
        eligible(document, layer, false).then(|| layer.id.clone())
    }
    /// Returns false for dependent effects or oversized content so the host can
    /// use its exact compatibility preview. The cache has the existing 64 MiB
    /// object-image budget and preserves off-artboard pixels and layer ordering.
    pub fn render_pixel_drag(
        &mut self,
        viewport: Viewport,
        document: &Document,
        layer_id: &str,
        offset: [f32; 2],
        copy: bool,
    ) -> Result<bool, String> {
        if offset.iter().any(|v| !v.is_finite() || v.abs() >= 100_000.) {
            return Err("Invalid pixel drag offset".into());
        }
        // Opt-in A/B measurement of the unchanged compatibility path.
        static BASELINE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if self.outline_view
            || self.channel != 0
            || *BASELINE.get_or_init(|| {
                performance::enabled()
                    && std::env::var_os("LUMAPAINT_PIXEL_DRAG_BASELINE").is_some()
            })
        {
            return Ok(false);
        }
        let Some(layer) = document
            .visible_svg_layers()
            .find(|layer| layer.id == layer_id)
        else {
            return Ok(false);
        };
        if !eligible(document, layer, copy) {
            return Ok(false);
        }
        let density = (viewport.screen_zoom() * viewport.scale).ceil().max(1.);
        if !self.pixel_drag_cache.as_ref().is_some_and(|cache| {
            cache.layer == layer_id && cache.density == density && cache.matches(document)
        }) {
            self.pixel_drag_cache = None;
            // Legacy nested SVG viewports are unwrapped by the compatibility
            // move path. Reusing their clipped display would lose hidden pixels.
            if layer.source.match_indices("<svg").nth(1).is_some() {
                return Ok(false);
            }
            let _timer = performance::time("pixel_drag_cache_prepare_host");
            let bounds =
                vector::pixel_source_document_bounds(&layer.source, document.dimensions())?;
            let shared = (density == 1.)
                .then(|| self.share_pixel_display(document, layer, bounds))
                .flatten();
            let Ok(image) = shared.map(Ok).unwrap_or_else(|| {
                self.prepare_object_cache_at_density(document, layer, false, density)
            }) else {
                return Ok(false);
            };
            if image.runs.is_empty() {
                return Ok(false);
            }
            performance::count("pixel_drag_cache_builds", 1);
            performance::count("pixel_drag_upload_bytes", image.upload_bytes as u64);
            self.pixel_drag_cache = Some(Cache {
                layer: layer_id.into(),
                density,
                revision: document.revision(),
                journal: document.scene_journal().instance_id(),
                bounds,
                identity: object_cache_state::SourceIdentity::capture(document, layer),
                image,
            });
        }
        // Selection-only revisions do not change the image, but the eventual
        // commit must be certified against this gesture's current revision.
        self.pixel_drag_cache.as_mut().unwrap().revision = document.revision();
        performance::count("pixel_drag_retained_frames", 1);
        // Other layers are rendered against the unmodified document; only the
        // selected image receives a transient GPU offset at its original depth.
        self.render_vector_drag_inner(
            viewport,
            document,
            [0., 0.],
            false,
            Some(Request {
                layer: layer_id,
                offset,
            }),
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> Document {
        let mut d = Document::default();
        d.import_raster_layer("Image".into(), r##"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="-20" y="40" width="160" height="120" fill="#d03090" fill-opacity=".5"/><rect x="80" y="70" width="120" height="80" fill="#1080c0"/></svg>"##.into()).unwrap();
        d
    }

    #[test]
    fn pixel_drag_texture_matches_translation_copy_and_offcanvas_return() {
        let mut d = document();
        let source = d.svg_layers().last().unwrap().source.clone();
        // Exercise the same object-local texture and rectangle used by the GPU,
        // at normal and HiDPI sampling, including pixels initially off canvas.
        for density in [1, 2] {
            let w = 960 * density;
            let h = 640 * density;
            let (pixels, rect) = vector::rasterize_svg_object(&source, (w, h)).unwrap();
            for (dx, dy) in [(0, 0), (45, 27), (-150, -80), (930, 600), (0, 0)] {
                for copy in [false, true] {
                    let moved = d
                        .translated_pixel_layer_source(dx as f32, dy as f32, copy)
                        .unwrap()
                        .unwrap();
                    let expected_source = if dx == 0 && dy == 0 { &source } else { &moved };
                    let expected = vector::rasterize_svg(expected_source, w, h).unwrap().pixels;
                    let mut actual = vec![0u8; expected.len()];
                    let shifts = if copy && (dx != 0 || dy != 0) {
                        vec![(0, 0), (dx, dy)]
                    } else {
                        vec![(dx, dy)]
                    };
                    for (x, y) in shifts {
                        let left = rect[0] as i32 + x * density as i32;
                        let top = rect[1] as i32 + y * density as i32;
                        let rw = rect[2] as usize;
                        for py in 0..rect[3] as i32 {
                            for px in 0..rw as i32 {
                                let tx = left + px;
                                let ty = top + py;
                                if tx < 0 || ty < 0 || tx >= w as i32 || ty >= h as i32 {
                                    continue;
                                }
                                let src = (py as usize * rw + px as usize) * 4;
                                let dst = (ty as usize * w as usize + tx as usize) * 4;
                                let inv = 255 - u32::from(pixels[src + 3]);
                                for c in 0..4 {
                                    actual[dst + c] = (u32::from(pixels[src + c])
                                        + (u32::from(actual[dst + c]) * inv + 127) / 255)
                                        .min(255)
                                        as u8;
                                }
                            }
                        }
                    }
                    assert!(
                        actual
                            .iter()
                            .zip(&expected)
                            .all(|(a, b)| a.abs_diff(*b) <= 2),
                        "{density}x ({dx},{dy}) copy={copy}"
                    );
                }
            }
        }
        let id = d.svg_layers().last().unwrap().id.clone();
        let initial =
            object_cache_state::SourceIdentity::capture(&d, d.svg_layers().last().unwrap());
        let moved = d
            .translated_pixel_layer_source(40., 30., false)
            .unwrap()
            .unwrap();
        d.replace_transformed_pixels(moved.clone(), None, [1., 0., 0., 1., 40., 30.], true)
            .unwrap();
        assert_ne!(
            initial,
            object_cache_state::SourceIdentity::capture(&d, d.svg_layers().last().unwrap())
        );
        d.undo();
        assert_eq!(d.svg_layers().find(|l| l.id == id).unwrap().source, source);
        d.redo();
        assert_eq!(d.svg_layers().find(|l| l.id == id).unwrap().source, moved);
    }

    #[test]
    fn pixel_drag_rejects_dependent_composition() {
        let d = document();
        let mut layer = d.svg_layers().last().unwrap().clone();
        assert!(eligible(&d, &layer, false));
        assert!(!eligible(&d, &layer, true));
        layer.opacity = 0.5;
        assert!(eligible(&d, &layer, false));
        assert!(!eligible(&d, &layer, true));
        layer.vector_layer = true;
        assert!(!eligible(&d, &layer, false));
        let mut d = document();
        let id = d.svg_layers().last().unwrap().id.clone();
        let mut effects = d.layer_effects(&id);
        effects.enabled = true;
        effects.values[0] = 1.;
        // An active correction/mask must use the exact composition fallback.
        d.set_layer_effects(&id, effects).unwrap();
        assert!(!eligible(&d, d.svg_layers().last().unwrap(), false));
    }
}
