//! Object locks belong to the document, independently of rendering or SVG paint.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectLockAction {
    Selection,
    ArtworkAbove,
    OtherLayers,
    UnlockAll,
}

impl Document {
    pub fn object_is_locked(&self, id: &str) -> bool {
        self.locked_objects.contains(id)
    }
    pub fn has_locked_objects(&self) -> bool {
        !self.locked_objects.is_empty() || !self.locked_artwork_layers.is_empty()
    }
    pub(super) fn ensure_object_unlocked(&self, id: &str) -> Result<(), String> {
        if self.object_is_locked(id) {
            Err("Object is locked / オブジェクトはロックされています / 对象已锁定".into())
        } else {
            Ok(())
        }
    }

    /// Renderer-resolved imported geometry keeps instance IDs; locks never rewrite SVG.
    fn lock_targets(&self) -> Vec<(String, Vec<VectorObject>)> {
        self.svg_layers
            .iter()
            .filter(|l| l.visible && !l.locked)
            .map(|l| {
                let objects = if l.vector_layer {
                    l.vector_objects.clone()
                } else if !l.paint_layer {
                    self.imported_svg_objects(&l.source, &l.id)
                } else {
                    Vec::new()
                };
                (l.id.clone(), objects)
            })
            .collect()
    }

    fn normalized_locked_sources(&self) -> Result<Vec<(usize, String)>, String> {
        let Some(backend) = &self.svg_geometry_backend else {
            return Ok(Vec::new());
        };
        let mut sources = Vec::new();
        let mut bytes = self
            .svg_layers
            .iter()
            .map(|layer| layer.source.len())
            .sum::<usize>();
        for (index, layer) in self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| !layer.vector_layer && !layer.paint_layer)
        {
            if !self
                .imported_svg_objects(&layer.source, &layer.id)
                .iter()
                .any(|object| self.object_is_locked(&object.id))
            {
                continue;
            }
            let source = backend.stabilize_ids(&layer.source)?;
            validate_svg_edit_source(&source)?;
            bytes = bytes - layer.source.len() + source.len();
            if bytes > MAX_SVG_TOTAL_BYTES {
                return Err("Project contains too much image data".into());
            }
            sources.push((index, source));
        }
        Ok(sources)
    }

    pub fn lock_objects(&mut self, action: ObjectLockAction) -> Result<(), String> {
        if self.has_active_saved_path() {
            return Err(
                "Finish path editing first / パスの編集を終了してください / 请先结束路径编辑"
                    .into(),
            );
        }
        self.finish();
        let selected: BTreeSet<String> = self.selected_vector_objects.iter().cloned().collect();
        let before = self.vector_history_state();
        let old_locks = self.locked_objects.clone();
        let old_layer_locks = self.locked_artwork_layers.clone();
        let mut changed = false;
        match action {
            ObjectLockAction::Selection | ObjectLockAction::ArtworkAbove => {
                let targets = self.lock_targets();
                if !selected.is_empty()
                    && selected.iter().any(|id| {
                        !targets.iter().any(|(_, objects)| {
                            objects
                                .iter()
                                .any(|o| &o.id == id && o.visible && !self.object_is_locked(id))
                        })
                    })
                {
                    return Err("Selection changed or is locked / 選択対象が変更されたかロックされています / 选区已更改或已锁定".into());
                }
                let mut ids = selected.clone();
                if matches!(action, ObjectLockAction::Selection) {
                    for (_, objects) in &targets {
                        let roots: BTreeSet<_> = objects
                            .iter()
                            .filter(|object| selected.contains(&object.id))
                            .filter_map(|object| object.group_path.first())
                            .collect();
                        for root in roots {
                            if objects
                                .iter()
                                .filter(|object| {
                                    object.visible && object.group_path.first() == Some(root)
                                })
                                .all(|object| selected.contains(&object.id))
                            {
                                ids.extend(
                                    objects
                                        .iter()
                                        .filter(|object| object.group_path.first() == Some(root))
                                        .map(|object| object.id.clone()),
                                );
                            }
                        }
                    }
                }
                if matches!(action, ObjectLockAction::ArtworkAbove) {
                    ids.clear();
                    for (_, objects) in &targets {
                        let selected_bounds: Vec<_> = objects
                            .iter()
                            .enumerate()
                            .filter(|(_, o)| selected.contains(&o.id))
                            .filter_map(|(index, o)| {
                                artwork_bounds(o).map(|bounds| (index, bounds))
                            })
                            .collect();
                        for (index, object) in objects.iter().enumerate() {
                            if !object.visible
                                || selected.contains(&object.id)
                                || self.object_is_locked(&object.id)
                            {
                                continue;
                            }
                            if selected_bounds.iter().any(|(below, bounds)| {
                                index > *below
                                    && object.intersects_selection(
                                        [
                                            bounds[0],
                                            bounds[1],
                                            bounds[2] - bounds[0],
                                            bounds[3] - bounds[1],
                                        ],
                                        false,
                                    )
                            }) {
                                // A group acts as one artwork unit in normal object selection.
                                if let Some(root) = object.group_path.first() {
                                    ids.extend(
                                        objects
                                            .iter()
                                            .filter(|o| {
                                                o.group_path.first() == Some(root)
                                                    && !selected.contains(&o.id)
                                            })
                                            .map(|o| o.id.clone()),
                                    );
                                } else {
                                    ids.insert(object.id.clone());
                                }
                            }
                        }
                    }
                }
                if selected.is_empty() {
                    if matches!(action, ObjectLockAction::ArtworkAbove) {
                        return Ok(());
                    }
                    let id = self
                        .selected_layer
                        .clone()
                        .unwrap_or_else(|| "layer-1".into());
                    if id == "layer-1" {
                        if self.layer_locked || !self.visible || self.selection.is_none() {
                            return Ok(());
                        }
                        self.layer_locked = true;
                    } else {
                        let Some(layer) = self
                            .svg_layers
                            .iter_mut()
                            .find(|l| l.id == id && !l.vector_layer && l.visible && !l.locked)
                        else {
                            return Ok(());
                        };
                        layer.locked = true;
                    }
                    self.locked_artwork_layers.insert(id);
                    self.selection = None;
                    changed = true;
                } else {
                    self.locked_objects.extend(ids);
                    changed = self.locked_objects != old_locks;
                    self.selected_vector_objects
                        .retain(|id| !self.locked_objects.contains(id));
                }
            }
            ObjectLockAction::OtherLayers => {
                let targets = self.lock_targets();
                let mut keep: BTreeSet<String> = targets
                    .iter()
                    .filter(|(_, objects)| objects.iter().any(|o| selected.contains(&o.id)))
                    .map(|(id, _)| id.clone())
                    .collect();
                if keep.is_empty() {
                    keep.insert(
                        self.selected_layer
                            .clone()
                            .unwrap_or_else(|| "layer-1".into()),
                    );
                }
                if !keep.contains("layer-1") {
                    changed |= !self.layer_locked;
                    self.layer_locked = true;
                    // The explicit layer command promotes a whole-image object lock to a layer lock.
                    changed |= self.locked_artwork_layers.remove("layer-1");
                }
                for layer in &mut self.svg_layers {
                    if !keep.contains(&layer.id) {
                        changed |= !layer.locked;
                        layer.locked = true;
                        changed |= self.locked_artwork_layers.remove(&layer.id);
                    }
                }
            }
            ObjectLockAction::UnlockAll => {
                if !self.has_locked_objects() {
                    return Ok(());
                }
                self.locked_objects.clear();
                for id in &self.locked_artwork_layers {
                    if id == "layer-1" {
                        self.layer_locked = false;
                    } else if let Some(layer) = self.svg_layers.iter_mut().find(|l| &l.id == id) {
                        layer.locked = false;
                    }
                }
                self.locked_artwork_layers.clear();
                // Illustrator selects the newly unlocked visible artwork; layer locks remain.
                let selectable: BTreeSet<_> = self
                    .lock_targets()
                    .into_iter()
                    .flat_map(|(_, objects)| objects)
                    .filter(|o| o.visible)
                    .map(|o| o.id)
                    .collect();
                self.selected_vector_objects =
                    old_locks.intersection(&selectable).cloned().collect();
                changed = true;
            }
        }
        if changed || self.locked_artwork_layers != old_layer_locks {
            // Anonymous SVG nodes need stable metadata before another path can be removed.
            if matches!(
                action,
                ObjectLockAction::Selection | ObjectLockAction::ArtworkAbove
            ) {
                let normalized = self.normalized_locked_sources();
                match normalized {
                    Ok(sources) => {
                        for (index, source) in sources {
                            self.svg_layers[index].source = source;
                        }
                    }
                    Err(error) => {
                        self.restore_vector_history(before);
                        return Err(error);
                    }
                }
            }
            self.selection_anchor = None;
            self.record_vector_edit(before);
            self.revision += 1;
        }
        Ok(())
    }
}

fn artwork_bounds(object: &VectorObject) -> Option<[f32; 4]> {
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for point in vector_geometry_extrema(object, |point| point)
        .into_iter()
        .chain(
            object
                .stroke_boundary_points()
                .into_iter()
                .map(|[x, y]| Point { x, y }),
        )
    {
        bounds[0] = bounds[0].min(point.x);
        bounds[1] = bounds[1].min(point.y);
        bounds[2] = bounds[2].max(point.x);
        bounds[3] = bounds[3].max(point.y);
    }
    bounds.iter().all(|v| v.is_finite()).then_some(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle(id: &str, x: f32, y: f32) -> VectorObject {
        VectorObject {
            opacity: 1.,
            blend_mode: "normal".into(),
            id: id.into(),
            name: id.into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: format!("M {x} {y} h 60 v 60 h -60 Z"),
                fill_rule: crate::vector::FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            fill: Some(VectorPaint {
                color: [255, 0, 0, 255],
            }),
            stroke: None,
            stroke_width: 0.,
            stroke_style: Default::default(),
            live_corners: None,
            rectangle_radii: None,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[x, y], [x + 60., y], [x + 60., y + 60.], [x, y + 60.]],
            text: None,
        }
    }
    fn scene() -> (Document, String) {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        for (id, x) in [
            ("back", 0.),
            ("selected", 20.),
            ("front", 40.),
            ("far", 400.),
        ] {
            doc.upsert_vector_object(&layer, rectangle(id, x, 20.))
                .unwrap();
        }
        doc.select_layer(layer.clone()).unwrap();
        doc.select_vector_objects(vec!["selected".into()]).unwrap();
        (doc, layer)
    }
    #[test]
    fn object_lock_preserves_rendering_rejects_edits_and_round_trips_history() {
        let (mut doc, layer) = scene();
        let original = doc.svg_layers[0].source.clone();
        doc.lock_objects(ObjectLockAction::Selection).unwrap();
        assert!(doc.selected_vector_objects.is_empty());
        assert!(doc.object_is_locked("selected"));
        assert_eq!(doc.svg_layers[0].source, original);
        assert!(doc.snapshot().layers[1].objects[1].locked);
        assert_eq!(
            doc.vector_at(Point { x: 25., y: 25. }, 0.).as_deref(),
            Some("back")
        );
        assert!(doc.select_vector_objects(vec!["selected".into()]).is_err());
        assert!(doc
            .upsert_vector_object(&layer, rectangle("selected", 200., 20.))
            .is_err());
        assert!(!doc
            .direct_objects()
            .iter()
            .any(|(_, object)| object.id == "selected"));
        let state = serde_json::to_vec(&doc.document_state()).unwrap();
        let loaded =
            Document::from_document_state(serde_json::from_slice(&state).unwrap()).unwrap();
        assert!(loaded.object_is_locked("selected"));
        doc.undo();
        assert!(!doc.object_is_locked("selected"));
        assert_eq!(doc.selected_vector_ids(), ["selected"]);
        doc.redo();
        assert!(doc.object_is_locked("selected"));
        doc.lock_objects(ObjectLockAction::UnlockAll).unwrap();
        assert!(!doc.has_locked_objects());
        assert_eq!(doc.selected_vector_ids(), ["selected"]);
        doc.undo();
        assert!(doc.object_is_locked("selected"));
    }
    #[test]
    fn above_locks_only_overlapping_front_artwork_in_the_same_layer() {
        let (mut doc, _) = scene();
        let other = doc.add_vector_layer().unwrap();
        doc.upsert_vector_object(&other, rectangle("other-layer", 40., 20.))
            .unwrap();
        doc.select_vector_objects(vec!["selected".into()]).unwrap();
        doc.lock_objects(ObjectLockAction::ArtworkAbove).unwrap();
        assert!(doc.object_is_locked("front"));
        for id in ["selected", "back", "far", "other-layer"] {
            assert!(!doc.object_is_locked(id), "{id}");
        }
        assert_eq!(doc.selected_vector_ids(), ["selected"]);
        let revision = doc.revision();
        doc.lock_objects(ObjectLockAction::ArtworkAbove).unwrap();
        assert_eq!(doc.revision(), revision);
        doc.undo();
        assert!(!doc.object_is_locked("front"));
    }
    #[test]
    fn other_layers_preserves_all_selected_layers_and_unlock_all_preserves_layer_locks() {
        let (mut doc, _) = scene();
        let other = doc.add_vector_layer().unwrap();
        doc.upsert_vector_object(&other, rectangle("other", 40., 20.))
            .unwrap();
        doc.select_vector_objects(vec!["selected".into(), "other".into()])
            .unwrap();
        doc.lock_objects(ObjectLockAction::OtherLayers).unwrap();
        assert!(doc.layer_locked);
        assert!(doc.svg_layers.iter().all(|layer| !layer.locked));
        doc.undo();
        assert!(!doc.layer_locked);
        doc.redo();
        assert!(doc.layer_locked);
        doc.lock_objects(ObjectLockAction::Selection).unwrap();
        doc.lock_objects(ObjectLockAction::UnlockAll).unwrap();
        assert!(doc.layer_locked);
        assert!(!doc.has_locked_objects());
        assert_eq!(doc.selected_vector_ids().len(), 2);
    }
    #[test]
    fn grouped_selection_locks_all_members_and_clear_preserves_locked_artwork() {
        let (mut doc, layer) = scene();
        for id in ["selected", "front"] {
            let mut object = doc.svg_layers[0]
                .vector_objects
                .iter()
                .find(|object| object.id == id)
                .unwrap()
                .clone();
            object.group_path = vec!["g".into()];
            doc.upsert_vector_object(&layer, object).unwrap();
        }
        doc.select_vector_objects(vec!["selected".into()]).unwrap();
        assert_eq!(doc.selected_vector_ids().len(), 2);
        doc.lock_objects(ObjectLockAction::Selection).unwrap();
        assert!(doc.object_is_locked("selected") && doc.object_is_locked("front"));
        doc.clear_selected_layer().unwrap();
        assert_eq!(doc.svg_layers[0].vector_objects.len(), 2);
        assert!(doc.svg_layers[0].source.contains("g"));
        doc.lock_objects(ObjectLockAction::UnlockAll).unwrap();
        assert_eq!(doc.selected_vector_ids().len(), 2);
    }
    #[test]
    fn selected_pixel_artwork_lock_has_undo_and_unlock_all() {
        let mut doc = Document::default();
        doc.select_all();
        doc.lock_objects(ObjectLockAction::Selection).unwrap();
        assert!(doc.layer_locked && doc.has_locked_objects());
        assert!(doc.selection.is_none());
        assert!(!doc
            .begin(Point { x: 10., y: 10. }, Brush::default())
            .unwrap());
        let mut loaded = Document::from_document_state(doc.document_state()).unwrap();
        loaded.lock_objects(ObjectLockAction::UnlockAll).unwrap();
        assert!(!loaded.layer_locked);
        doc.undo();
        assert!(!doc.layer_locked);
        assert!(doc.selection.is_some());
        doc.redo();
        assert!(doc.layer_locked);
        doc.lock_objects(ObjectLockAction::UnlockAll).unwrap();
        assert!(!doc.layer_locked);
    }
}
