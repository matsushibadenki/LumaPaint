use super::*;
use crate::image_frame::{inverse, multiply};
use crate::layer_effects::LayerEffects;

impl Document {
    pub fn layer_edit_target(&self) -> crate::layer_mask::LayerEditTarget {
        use crate::layer_mask::LayerEditTarget;
        let target = self
            .layer_edit_selection
            .as_ref()
            .filter(|(id, _)| id == self.selected_layer_id())
            .map_or(LayerEditTarget::Content, |(_, t)| *t);
        if target == LayerEditTarget::Mask
            && self
                .layer_effects
                .get(self.selected_layer_id())
                .is_none_or(|e| e.mask.is_none())
        {
            LayerEditTarget::None
        } else {
            target
        }
    }
    pub fn select_layer_target(
        &mut self,
        id: String,
        target: crate::layer_mask::LayerEditTarget,
    ) -> Result<(), String> {
        if target == crate::layer_mask::LayerEditTarget::Mask
            && self.layer_effects.get(&id).is_none_or(|e| e.mask.is_none())
        {
            return Err("Layer mask not found".into());
        }
        self.select_layer(id.clone())?;
        self.end_path_editing();
        self.layer_edit_selection = Some((id, target));
        if target == crate::layer_mask::LayerEditTarget::Mask && self.editing_channel != 0 {
            self.editing_channel = 4;
        }
        Ok(())
    }

    /// Only a uniform transform of the entire layer carries its linked mask.
    pub(super) fn linked_mask_updates(
        &self,
        next: &[SvgLayer],
    ) -> Result<Vec<(String, LayerEffects)>, String> {
        let mut updates = Vec::new();
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        for old in &self.svg_layers {
            let Some(effects) = self
                .layer_effects
                .get(&old.id)
                .filter(|e| e.mask.as_ref().is_some_and(|m| m.linked))
            else {
                continue;
            };
            let Some(new) = next.iter().find(|l| l.id == old.id) else {
                continue;
            };
            if !old.vector_objects.iter().all(|o| selected.contains(&o.id))
                || old.vector_objects.is_empty()
                || old.vector_objects.len() != new.vector_objects.len()
            {
                continue;
            }
            let first = &old.vector_objects[0];
            let Some(after) = new.vector_objects.iter().find(|o| o.id == first.id) else {
                continue;
            };
            let Ok(inv) = inverse(first.transform) else {
                continue;
            };
            let delta = multiply(after.transform, inv);
            let close = |a: [f32; 6], b: [f32; 6]| {
                a.iter()
                    .zip(b)
                    .all(|(a, b)| (*a - b).abs() <= 2e-6 * (1. + a.abs().max(b.abs())))
            };
            if close(delta, [1., 0., 0., 1., 0., 0.]) {
                continue;
            }
            if !old.vector_objects.iter().all(|o| {
                new.vector_objects
                    .iter()
                    .find(|n| n.id == o.id)
                    .is_some_and(|n| close(multiply(delta, o.transform), n.transform))
            }) {
                continue;
            }
            let mut effects = effects.clone();
            effects.mask.as_mut().unwrap().transform_by(delta)?;
            updates.push((old.id.clone(), effects));
        }
        Ok(updates)
    }
    pub fn selected_vectors_have_mask(&self) -> bool {
        self.svg_layers.iter().any(|l| {
            self.layer_effects
                .get(&l.id)
                .is_some_and(|e| e.mask.is_some())
                && l.vector_objects
                    .iter()
                    .any(|o| self.selected_vector_objects.contains(&o.id))
        })
    }
    /// The image and mask share the source replacement's single Undo entry.
    pub fn replace_transformed_pixels(
        &mut self,
        source: String,
        selection: Option<Selection>,
        matrix: [f32; 6],
        whole_layer: bool,
    ) -> Result<(), String> {
        let id = self
            .selected_layer
            .as_deref()
            .unwrap_or("layer-1")
            .to_owned();
        let mut effects = self.layer_effects(&id);
        if whole_layer {
            if let Some(mask) = effects.mask.as_mut().filter(|m| m.linked) {
                mask.transform_by(matrix)?;
            }
        }
        self.replace_moved_pixels(source, selection)?;
        if whole_layer && effects.mask.as_ref().is_some_and(|m| m.linked) {
            self.layer_effects.insert(id, effects);
        }
        Ok(())
    }
    /// Transform from the mask side. A linked mask also transforms the whole layer.
    /// The host supplies a raw raster source only when root strokes need flattening.
    pub fn transform_layer_mask(
        &mut self,
        id: &str,
        matrix: [f32; 6],
        root_source: Option<String>,
    ) -> Result<(), String> {
        crate::layer_mask::validate_transform(matrix)?;
        if matrix == [1., 0., 0., 1., 0., 0.] {
            return Ok(());
        }
        let mut effects = self.layer_effects(id);
        let mask = effects.mask.as_mut().ok_or("Layer mask not found")?;
        mask.transform_by(matrix)?;
        let linked = mask.linked;
        let mut layers = self.svg_layers.clone();
        let mut paint = None;
        if id == "layer-1" {
            if self.layer_locked || !self.visible {
                return Err("Layer is locked or hidden".into());
            }
            if linked {
                if self.layer_alpha_locked {
                    return Err("Layer transparency is locked".into());
                }
                if !self.strokes.is_empty() && root_source.is_none() {
                    return Err("Raw paint source is required".into());
                }
                let source = root_source
                    .or_else(|| self.paint_source.clone())
                    .unwrap_or_else(|| empty_vector_svg(self.width, self.height));
                paint = Some(transformed_source(
                    &source,
                    self.width,
                    self.height,
                    matrix,
                )?);
            }
        } else {
            let layer = layers
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or("Layer not found")?;
            if layer.locked || !layer.visible {
                return Err("Layer is locked or hidden".into());
            }
            if linked {
                if layer.vector_layer {
                    for o in &mut layer.vector_objects {
                        if self.object_is_locked(&o.id) {
                            return Err("Object is locked".into());
                        }
                        o.transform = multiply(matrix, o.transform);
                        o.bounds_reset = false;
                        o.validate()?;
                    }
                    layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                } else {
                    if layer.alpha_locked {
                        return Err("Layer transparency is locked".into());
                    }
                    layer.source =
                        transformed_source(&layer.source, self.width, self.height, matrix)?;
                }
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much image data".into());
        }
        self.finish();
        let mut before = self.vector_history_state();
        if let Some(source) = paint {
            before.strokes = Some(std::mem::take(&mut self.strokes));
            self.point_count = 0;
            self.paint_source = Some(source);
        }
        self.svg_layers = layers;
        self.layer_effects.insert(id.into(), effects);
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
}
fn transformed_source(
    source: &str,
    width: u32,
    height: u32,
    m: [f32; 6],
) -> Result<String, String> {
    let source = unclipped_pixel_source(source, width, height)?;
    let [a, b, c, d, e, f] = m;
    let result = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><g transform="matrix({a} {b} {c} {d} {e} {f})">{source}</g></svg>"#
    );
    if result.len() > MAX_SVG_BYTES {
        return Err("Pixel image exceeds project limit".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        layer_mask::{LayerMask, MaskKind},
        selection::SelectionShape,
    };
    fn fixture(vector: bool, kind: MaskKind) -> (Document, String) {
        let mut d = Document::default();
        let id = if vector {
            d.add_vector_layer().unwrap()
        } else {
            d.add_paint_layer().unwrap()
        };
        d.select_layer(id.clone()).unwrap();
        if vector {
            let object = VectorObject {
                live_corners: None,
                rectangle_radii: None,
                opacity: 1.0,
                blend_mode: "normal".into(),
                text: None,
                id: "rectangle-1".into(),
                name: "Rectangle".into(),
                group_path: Vec::new(),
                clipping_group: None,
                bounds_reset: false,
                path: VectorPath {
                    data: "M 10 10 H 30 V 40 H 10 Z".into(),
                    fill_rule: crate::vector::FillRule::NonZero,
                },
                transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                image_frame: None,
                fill_gradient: None,
                stroke_gradient: None,
                fill: Some(VectorPaint {
                    registration: false,
                    color: [0, 0, 0, 255],
                }),
                stroke: None,
                stroke_style: Default::default(),
                stroke_width: 0.0,
                visible: true,
                kind: crate::vector::VectorObjectKind::Rectangle,
                control_points: vec![[10.0, 10.0], [30.0, 40.0]],
            };

            d.upsert_vector_object(&id, object).unwrap();
            d.select_vector_objects(vec!["rectangle-1".into()]).unwrap();
        } else {
            d.replace_moved_pixels(empty_vector_svg(16, 16), None)
                .unwrap();
        }
        let s = Selection::new(SelectionShape::Rectangle, [2., 3., 8., 7.]);
        d.set_layer_effects(
            &id,
            LayerEffects {
                mask: Some(LayerMask::from_selection(kind, Some(&s), 16, 16).unwrap()),
                ..Default::default()
            },
        )
        .unwrap();
        (d, id)
    }
    fn set_link(d: &mut Document, id: &str, linked: bool) {
        let mut e = d.layer_effects(id);
        e.mask.as_mut().unwrap().linked = linked;
        d.set_layer_effects(id, e).unwrap();
    }
    fn transform(d: &Document, id: &str) -> [f32; 6] {
        d.layer_effects(id).mask.unwrap().transform
    }
    #[test]
    fn mask_link_four_combinations_move_undo_and_relink() {
        for vector in [false, true] {
            for kind in [MaskKind::Pixel, MaskKind::Vector] {
                for linked in [false, true] {
                    let (mut d, id) = fixture(vector, kind);
                    set_link(&mut d, &id, linked);
                    let before = d.encode().unwrap();
                    if vector {
                        d.move_selected_vectors(12., -4.).unwrap();
                    } else {
                        let source = d
                            .translated_pixel_layer_source(12., -4., false)
                            .unwrap()
                            .unwrap();
                        d.replace_transformed_pixels(
                            source,
                            None,
                            [1., 0., 0., 1., 12., -4.],
                            true,
                        )
                        .unwrap();
                    }
                    assert_eq!(
                        transform(&d, &id),
                        if linked {
                            [1., 0., 0., 1., 12., -4.]
                        } else {
                            [1., 0., 0., 1., 0., 0.]
                        }
                    );
                    let after = d.encode().unwrap();
                    d.undo();
                    assert_eq!(d.encode().unwrap(), before);
                    d.redo();
                    assert_eq!(d.encode().unwrap(), after);
                    set_link(&mut d, &id, !linked);
                    assert_eq!(
                        transform(&d, &id),
                        if linked {
                            [1., 0., 0., 1., 12., -4.]
                        } else {
                            [1., 0., 0., 1., 0., 0.]
                        }
                    );
                    let loaded = Document::from_document_state(
                        serde_json::from_slice(&serde_json::to_vec(&d.document_state()).unwrap())
                            .unwrap(),
                    )
                    .unwrap();
                    assert_eq!(loaded.layer_effects(&id), d.layer_effects(&id));
                }
            }
        }
    }
    #[test]
    fn mask_side_affine_transform_is_atomic_for_all_combinations() {
        for vector in [false, true] {
            for kind in [MaskKind::Pixel, MaskKind::Vector] {
                for linked in [false, true] {
                    let (mut d, id) = fixture(vector, kind);
                    set_link(&mut d, &id, linked);
                    let source = d.svg_layers[0].source.clone();
                    let before = d.encode().unwrap();
                    let m = [0., 2., -2., 0., 25., 8.];
                    d.transform_layer_mask(&id, m, None).unwrap();
                    assert_eq!(transform(&d, &id), m);
                    assert_eq!(source == d.svg_layers[0].source, !linked);
                    let mask = d.layer_effects(&id).mask.unwrap();
                    assert_eq!(mask.coverage([17., 14.]), 1.);
                    assert_eq!(mask.coverage([4., 4.]), 0.);
                    let after = d.encode().unwrap();
                    d.undo();
                    assert_eq!(d.encode().unwrap(), before);
                    d.redo();
                    assert_eq!(d.encode().unwrap(), after);
                }
            }
        }
    }
    #[test]
    fn partial_vector_move_does_not_carry_mask_and_full_affine_does() {
        let (mut d, id) = fixture(true, MaskKind::Vector);
        let mut second = d.svg_layers[0].vector_objects[0].clone();
        second.id = "second".into();
        d.upsert_vector_object(&id, second).unwrap();
        d.select_vector_objects(vec!["rectangle-1".into()]).unwrap();
        d.move_selected_vectors(3., 4.).unwrap();
        assert_eq!(transform(&d, &id), [1., 0., 0., 1., 0., 0.]);
        d.select_vector_objects(vec!["rectangle-1".into(), "second".into()])
            .unwrap();
        let m = [1.5, 0.3, 0.2, 2., 7., -4.];
        d.affine_selected_vectors(m).unwrap();
        assert!(transform(&d, &id)
            .iter()
            .zip(m)
            .all(|(a, b)| (a - b).abs() < 0.0001));
    }
    #[test]
    fn invalid_and_locked_mask_transform_leaves_document_intact() {
        let (mut d, id) = fixture(true, MaskKind::Pixel);
        let before = d.encode().unwrap();
        assert!(d.transform_layer_mask(&id, [0.; 6], None).is_err());
        assert_eq!(d.encode().unwrap(), before);
        d.svg_layers[0].locked = true;
        let before = d.encode().unwrap();
        assert!(d
            .transform_layer_mask(&id, [1., 0., 0., 1., 3., 4.], None)
            .is_err());
        assert_eq!(d.encode().unwrap(), before);
    }
    #[test]
    fn old_masks_default_linked_and_preserve_coverage_when_toggled() {
        let (mut d, id) = fixture(false, MaskKind::Pixel);
        let mut json = serde_json::to_value(d.layer_effects(&id).mask.unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("linked");
        json.as_object_mut().unwrap().remove("transform");
        let mask: LayerMask = serde_json::from_value(json).unwrap();
        assert!(mask.linked);
        assert_eq!(mask.transform, [1., 0., 0., 1., 0., 0.]);
        set_link(&mut d, &id, false);
        let current = d.layer_effects(&id).mask.unwrap();
        assert_eq!(mask.content, current.content);
        assert_eq!(mask.coverage([3., 4.]), current.coverage([3., 4.]));
    }
    #[test]
    fn linked_mask_preview_has_no_history_and_disabled_mask_still_follows() {
        let (mut d, id) = fixture(true, MaskKind::Pixel);
        let mut effects = d.layer_effects(&id);
        effects.mask.as_mut().unwrap().enabled = false;
        d.set_layer_effects(&id, effects).unwrap();
        let original = d.encode().unwrap();
        let mut preview = d.clone_for_rendering();
        preview.move_selected_vectors_preview(9., 2.).unwrap();
        assert!(preview.vector_undo.is_empty());
        assert_eq!(transform(&preview, &id), [1., 0., 0., 1., 9., 2.]);
        assert_eq!(d.encode().unwrap(), original);
    }
    #[test]
    fn root_pixel_mask_and_content_transform_undo_together() {
        let mut d = Document::default();
        d.replace_moved_pixels(empty_vector_svg(16, 16), None)
            .unwrap();
        let mask = LayerMask::from_selection(MaskKind::Vector, None, 16, 16).unwrap();
        d.set_layer_effects(
            "layer-1",
            LayerEffects {
                mask: Some(mask),
                ..Default::default()
            },
        )
        .unwrap();
        let before = d.encode().unwrap();
        d.transform_layer_mask("layer-1", [1., 0., 0., 1., 5., 6.], None)
            .unwrap();
        assert!(d.paint_source().unwrap().contains("matrix(1 0 0 1 5 6)"));
        let after = d.encode().unwrap();
        d.undo();
        assert_eq!(d.encode().unwrap(), before);
        d.redo();
        assert_eq!(d.encode().unwrap(), after);
    }
}
