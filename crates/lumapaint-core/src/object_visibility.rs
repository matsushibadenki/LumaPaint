//! Visibility commands are one document transaction, independent of rendering.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ObjectVisibilityAction {
    Selection,
    ArtworkAbove,
    OtherLayers,
    ShowAll,
}

impl Document {
    pub fn has_hidden_objects(&self) -> bool {
        !self.visible
            || self.layer_groups.groups.iter().any(|g| !g.visible)
            || self.svg_layers.iter().any(|layer| {
                !layer.visible
                    || layer.vector_objects.iter().any(|object| !object.visible)
                    || self
                        .svg_geometry_backend
                        .as_ref()
                        .is_some_and(|backend| backend.has_hidden_objects(&layer.source))
            })
    }

    pub fn hide_objects(&mut self, action: ObjectVisibilityAction) -> Result<(), String> {
        if self.has_active_saved_path() {
            return Err(
                "Finish path editing first / パスの編集を終了してください / 请先结束路径编辑"
                    .into(),
            );
        }
        self.finish();
        if matches!(action, ObjectVisibilityAction::ShowAll) && !self.has_hidden_objects() {
            return Ok(());
        }
        let before = self.vector_history_state();
        let selected: BTreeSet<_> = self.selected_vector_objects.iter().cloned().collect();
        let objects: Vec<_> = self
            .svg_layers
            .iter()
            .map(|layer| {
                if layer.vector_layer {
                    layer.vector_objects.clone()
                } else if !layer.paint_layer {
                    self.imported_svg_objects(&layer.source, &layer.id)
                } else {
                    Vec::new()
                }
            })
            .collect();
        let mut layers = self.svg_layers.clone();
        let mut visible = self.visible;
        let mut changed = matches!(action, ObjectVisibilityAction::ShowAll)
            && self.layer_groups.groups.iter().any(|g| !g.visible);
        let mut revealed = BTreeSet::new();
        let keep: BTreeSet<_> = self
            .svg_layers
            .iter()
            .zip(&objects)
            .filter(|(_, objects)| objects.iter().any(|object| selected.contains(&object.id)))
            .map(|(layer, _)| layer.id.clone())
            .collect();
        let active_layer = self.selected_layer.as_deref().unwrap_or("layer-1");
        for ((layer, original), objects) in layers.iter_mut().zip(&self.svg_layers).zip(&objects) {
            let other = if keep.is_empty() {
                layer.id != active_layer
            } else {
                !keep.contains(&layer.id)
            };
            match action {
                ObjectVisibilityAction::OtherLayers if other => {
                    changed |= layer.visible;
                    layer.visible = false;
                }
                ObjectVisibilityAction::ShowAll => {
                    changed |= !layer.visible;
                    layer.visible = true;
                }
                _ => {}
            }
            let mut ids: BTreeSet<String> = BTreeSet::new();
            if matches!(
                action,
                ObjectVisibilityAction::Selection | ObjectVisibilityAction::ArtworkAbove
            ) && original.visible
                && !original.locked
            {
                if matches!(action, ObjectVisibilityAction::Selection) {
                    ids.extend(
                        objects
                            .iter()
                            .filter(|o| {
                                selected.contains(&o.id)
                                    && o.visible
                                    && !self.object_is_locked(&o.id)
                            })
                            .map(|o| o.id.clone()),
                    );
                } else if let Some(first) = objects.iter().position(|o| selected.contains(&o.id)) {
                    ids.extend(
                        objects
                            .iter()
                            .skip(first + 1)
                            .filter(|o| {
                                !selected.contains(&o.id)
                                    && o.visible
                                    && !self.object_is_locked(&o.id)
                            })
                            .map(|o| o.id.clone()),
                    );
                    let roots: BTreeSet<_> = objects
                        .iter()
                        .filter(|o| ids.contains(&o.id))
                        .filter_map(|o| o.group_path.first())
                        .collect();
                    ids.extend(
                        objects
                            .iter()
                            .filter(|o| {
                                o.group_path
                                    .first()
                                    .is_some_and(|root| roots.contains(root))
                                    && !selected.contains(&o.id)
                                    && !self.object_is_locked(&o.id)
                            })
                            .map(|o| o.id.clone()),
                    );
                }
                // A selected placed image (including a raster layer) is a single artwork unit.
                if selected.is_empty()
                    && matches!(action, ObjectVisibilityAction::Selection)
                    && layer.id == active_layer
                    && !layer.vector_layer
                {
                    changed |= layer.visible;
                    layer.visible = false;
                }
            }
            if layer.vector_layer {
                let mut object_changed = false;
                for object in &mut layer.vector_objects {
                    if matches!(action, ObjectVisibilityAction::ShowAll) {
                        if (!object.visible || !original.visible)
                            && !original.locked
                            && !self.object_is_locked(&object.id)
                        {
                            revealed.insert(object.id.clone());
                        }
                        object_changed |= !object.visible;
                        object.visible = true;
                    } else if ids.contains(&object.id) {
                        object_changed |= object.visible;
                        object.visible = false;
                    }
                }
                if object_changed {
                    layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                    changed = true;
                }
            } else if !layer.paint_layer {
                if let Some(backend) = &self.svg_geometry_backend {
                    let source = if matches!(action, ObjectVisibilityAction::ShowAll) {
                        for object in objects.iter().filter(|o| {
                            (!o.visible || !original.visible)
                                && !original.locked
                                && !self.object_is_locked(&o.id)
                        }) {
                            revealed.insert(object.id.clone());
                        }
                        backend.show_all_objects(&layer.source)?
                    } else if !ids.is_empty() {
                        backend.hide_objects(
                            &layer.source,
                            &layer.id,
                            [self.width as f32, self.height as f32],
                            &ids.into_iter().collect::<Vec<_>>(),
                        )?
                    } else {
                        layer.source.clone()
                    };
                    changed |= source != layer.source;
                    layer.source = source;
                }
            }
            validate_svg_layer(layer)?;
        }
        match action {
            ObjectVisibilityAction::OtherLayers
                if if keep.is_empty() {
                    active_layer != "layer-1"
                } else {
                    !keep.contains("layer-1")
                } =>
            {
                changed |= visible;
                visible = false;
            }
            ObjectVisibilityAction::ShowAll => {
                changed |= !visible;
                visible = true;
            }
            ObjectVisibilityAction::Selection
                if selected.is_empty()
                    && active_layer == "layer-1"
                    && self.selection.is_some()
                    && !self.layer_locked =>
            {
                changed |= visible;
                visible = false;
            }
            _ => {}
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much image data".into());
        }
        if !changed {
            return Ok(());
        }
        for member in &mut self.layer_groups.members {
            if let Some(layer) = layers.iter().find(|l| l.id == member.id) {
                let previous = before.layers.iter().find(|l| l.id == member.id);
                if matches!(action, ObjectVisibilityAction::ShowAll)
                    || previous.is_some_and(|l| l.visible != layer.visible)
                {
                    member.visible = layer.visible;
                }
            }
        }
        if matches!(action, ObjectVisibilityAction::ShowAll) {
            for group in &mut self.layer_groups.groups {
                group.visible = true;
            }
        }
        self.svg_layers = layers;
        self.sync_layer_group_flags();
        self.visible = visible;
        let selectable: BTreeSet<_> = self
            .svg_layers
            .iter()
            .filter(|l| l.visible && !l.locked)
            .flat_map(|l| {
                if l.vector_layer {
                    l.vector_objects.clone()
                } else {
                    self.imported_svg_objects(&l.source, &l.id)
                }
            })
            .filter(|o| o.visible && !self.object_is_locked(&o.id))
            .map(|o| o.id)
            .collect();
        self.selected_vector_objects
            .retain(|id| selectable.contains(id));
        self.selected_vector_objects.extend(
            revealed
                .intersection(&selectable)
                .filter(|id| !selected.contains(*id))
                .cloned(),
        );
        if !self.visible
            || self
                .selected_layer
                .as_ref()
                .is_some_and(|id| self.svg_layers.iter().any(|l| &l.id == id && !l.visible))
        {
            self.selection = None;
        }
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
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
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
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
    fn selection_round_trips_history_and_native_state_without_selecting_hidden_objects() {
        let (mut doc, layer) = scene();
        doc.hide_objects(ObjectVisibilityAction::Selection).unwrap();
        assert!(doc.has_hidden_objects());
        assert!(doc.selected_vector_ids().is_empty());
        assert!(!doc.svg_layers[0].source.contains("id=\"selected\""));
        assert!(doc.select_vector_objects(vec!["selected".into()]).is_err());
        let mut loaded = Document::from_document_state(doc.document_state()).unwrap();
        loaded
            .hide_objects(ObjectVisibilityAction::ShowAll)
            .unwrap();
        assert!(loaded.svg_layers[0]
            .vector_objects
            .iter()
            .all(|o| o.visible));
        assert_eq!(loaded.selected_vector_ids(), ["selected"]);
        doc.undo();
        assert!(!doc.has_hidden_objects());
        assert_eq!(doc.selected_vector_ids(), ["selected"]);
        doc.redo();
        assert!(doc.selected_vector_ids().is_empty());
        doc.hide_objects(ObjectVisibilityAction::ShowAll).unwrap();
        assert_eq!(doc.selected_vector_ids(), ["selected"]);
        let revision = doc.revision();
        doc.hide_objects(ObjectVisibilityAction::ShowAll).unwrap();
        assert_eq!(doc.revision(), revision);
        assert!(doc.svg_layers.iter().any(|l| l.id == layer));
    }
    #[test]
    fn above_includes_nonoverlapping_front_artwork_but_excludes_other_layers_and_locks() {
        let (mut doc, _) = scene();
        let other = doc.add_vector_layer().unwrap();
        doc.upsert_vector_object(&other, rectangle("other", 40., 20.))
            .unwrap();
        doc.select_vector_objects(vec!["front".into()]).unwrap();
        doc.lock_objects(ObjectLockAction::Selection).unwrap();
        doc.select_vector_objects(vec!["selected".into()]).unwrap();
        doc.hide_objects(ObjectVisibilityAction::ArtworkAbove)
            .unwrap();
        let visibility: std::collections::BTreeMap<_, _> = doc
            .svg_layers
            .iter()
            .flat_map(|l| &l.vector_objects)
            .map(|o| (o.id.as_str(), o.visible))
            .collect();
        assert!(!visibility["far"]);
        for id in ["back", "selected", "front", "other"] {
            assert!(visibility[id], "{id}");
        }
        assert_eq!(doc.selected_vector_ids(), ["selected"]);
        doc.hide_objects(ObjectVisibilityAction::ShowAll).unwrap();
        assert!(doc.object_is_locked("front"));
    }
    #[test]
    fn other_layers_keeps_all_selected_layers_and_show_all_preserves_locks() {
        let (mut doc, _) = scene();
        let other = doc.add_vector_layer().unwrap();
        doc.upsert_vector_object(&other, rectangle("other", 40., 20.))
            .unwrap();
        doc.select_vector_objects(vec!["selected".into(), "other".into()])
            .unwrap();
        doc.hide_objects(ObjectVisibilityAction::OtherLayers)
            .unwrap();
        assert!(!doc.visible);
        assert!(doc.svg_layers.iter().all(|l| l.visible));
        doc.undo();
        assert!(doc.visible);
        doc.redo();
        assert!(!doc.visible);
        doc.layer_locked = true;
        doc.hide_objects(ObjectVisibilityAction::ShowAll).unwrap();
        assert!(doc.visible && doc.layer_locked);
    }
    #[test]
    fn grouped_and_pixel_selection_hide_as_single_artwork_and_clear_selection() {
        let (mut doc, layer) = scene();
        for object in &mut doc.svg_layers[0].vector_objects {
            if ["selected", "front"].contains(&object.id.as_str()) {
                object.group_path = vec!["g".into()];
            }
        }
        doc.select_vector_objects(vec!["selected".into()]).unwrap();
        doc.hide_objects(ObjectVisibilityAction::Selection).unwrap();
        assert!(
            doc.svg_layers[0]
                .vector_objects
                .iter()
                .filter(|o| !o.visible)
                .count()
                == 2
        );
        doc.select_layer("layer-1".into()).unwrap();
        doc.select_all();
        doc.hide_objects(ObjectVisibilityAction::Selection).unwrap();
        assert!(!doc.visible && doc.selection.is_none());
        doc.undo();
        assert!(doc.visible && doc.selection.is_some());
        doc.select_layer(layer).unwrap();
    }
}
