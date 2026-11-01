//! Named visibility/opacity configurations. No artwork or renderer resources are stored.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerComp {
    pub id: String,
    pub name: String,
    pub visibility: BTreeMap<String, bool>,
    /// Empty in legacy visibility-only comps; applying those preserves opacity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub opacity: BTreeMap<String, f32>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerCompSummary {
    pub id: String,
    pub name: String,
    pub matches: bool,
    pub count: usize,
    pub difference: LayerCompDifference,
}

/// Bounded read-only comparison metadata. Values are current -> saved.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerCompDifference {
    pub layer_count: usize,
    pub visibility_count: usize,
    pub opacity_count: usize,
    pub uncaptured_count: usize,
    pub details: Vec<LayerCompDifferenceDetail>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerCompDifferenceDetail {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<(Option<bool>, bool)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<(Option<f32>, f32)>,
}

impl LayerGroupsState {
    pub(super) fn validate_layer_comps(&self, layers: &[SvgLayer]) -> Result<(), String> {
        if self.comps.len() > 32 || self.next_comp_id > 1_000_000_000 {
            return Err("Too many layer comps".into());
        }
        if self.comps.is_empty() {
            return Ok(());
        }
        let opacity_ids: BTreeSet<&str> = layers
            .iter()
            .map(|layer| layer.id.as_str())
            .chain(std::iter::once("layer-1"))
            .collect();
        let valid: BTreeSet<&str> = opacity_ids
            .iter()
            .copied()
            .chain(self.groups.iter().map(|group| group.id.as_str()))
            .collect();
        let mut ids = BTreeSet::new();
        for comp in &self.comps {
            if comp.id.is_empty()
                || comp.id.len() > 128
                || !ids.insert(&comp.id)
                || !valid_name(&comp.name)
                || comp.visibility.len() > 16_384
                || !comp.visibility.contains_key("layer-1")
                || comp.opacity.iter().any(|(id, value)| {
                    !value.is_finite()
                        || !(0.0..=1.0).contains(value)
                        || !comp.visibility.contains_key(id)
                        || !opacity_ids.contains(id.as_str())
                })
                || comp
                    .visibility
                    .keys()
                    .any(|id| !valid.contains(id.as_str()))
            {
                return Err("Invalid layer comp".into());
            }
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 120 && !name.chars().any(char::is_control)
}

impl Document {
    fn own_layer_visibility(&self) -> BTreeMap<String, bool> {
        std::iter::once(("layer-1".into(), self.visible))
            .chain(
                self.svg_layers
                    .iter()
                    .filter(|l| {
                        self.path_editing
                            .as_ref()
                            .is_none_or(|p| p.layer_id != l.id)
                    })
                    .map(|l| {
                        (
                            l.id.clone(),
                            self.layer_groups
                                .members
                                .iter()
                                .find(|m| m.id == l.id)
                                .map_or(l.visible, |m| m.visible),
                        )
                    }),
            )
            .chain(
                self.layer_groups
                    .groups
                    .iter()
                    .map(|g| (g.id.clone(), g.visible)),
            )
            .collect()
    }

    fn own_layer_opacity(&self) -> BTreeMap<String, f32> {
        std::iter::once(("layer-1".into(), self.layer_opacity))
            .chain(
                self.svg_layers
                    .iter()
                    .filter(|layer| {
                        self.path_editing
                            .as_ref()
                            .is_none_or(|path| path.layer_id != layer.id)
                    })
                    .map(|layer| {
                        let opacity = self
                            .layer_groups
                            .members
                            .iter()
                            .find(|member| member.id == layer.id)
                            .and_then(|member| member.opacity)
                            .unwrap_or(layer.opacity);
                        (layer.id.clone(), opacity)
                    }),
            )
            .collect()
    }

    pub(super) fn layer_comp_summaries(&self) -> Vec<LayerCompSummary> {
        if self.layer_groups.comps.is_empty() {
            return Vec::new();
        }
        let current = self.own_layer_visibility();
        let opacity = self.own_layer_opacity();
        let names: BTreeMap<&str, &str> = std::iter::once(("layer-1", self.layer_name.as_str()))
            .chain(
                self.svg_layers
                    .iter()
                    .map(|layer| (layer.id.as_str(), layer.name.as_str())),
            )
            .chain(
                self.layer_groups
                    .groups
                    .iter()
                    .map(|group| (group.id.as_str(), group.name.as_str())),
            )
            .collect();
        self.layer_groups
            .comps
            .iter()
            .map(|comp| {
                let mut difference = LayerCompDifference {
                    uncaptured_count: current
                        .keys()
                        .filter(|id| !comp.visibility.contains_key(*id))
                        .count(),
                    ..Default::default()
                };
                for (id, saved) in &comp.visibility {
                    let visible = current.get(id).copied();
                    let visibility = (visible != Some(*saved)).then_some((visible, *saved));
                    let alpha = opacity.get(id).copied();
                    let opacity = comp
                        .opacity
                        .get(id)
                        .filter(|saved| alpha != Some(**saved))
                        .map(|saved| (alpha, *saved));
                    difference.visibility_count += usize::from(visibility.is_some());
                    difference.opacity_count += usize::from(opacity.is_some());
                    if visibility.is_some() || opacity.is_some() {
                        difference.layer_count += 1;
                        // Counts cover all layers; avoid sending an unbounded list per comp.
                        if difference.details.len() < 8 {
                            difference.details.push(LayerCompDifferenceDetail {
                                id: id.clone(),
                                name: names.get(id.as_str()).copied().unwrap_or(id).into(),
                                visibility,
                                opacity,
                            });
                        }
                    }
                }
                LayerCompSummary {
                    id: comp.id.clone(),
                    name: comp.name.clone(),
                    count: comp.visibility.len(),
                    matches: difference.layer_count == 0,
                    difference,
                }
            })
            .collect()
    }

    /// Deletion removes references in the same history transaction, preventing
    /// reused legacy layer IDs from acquiring an unrelated visibility setting.
    pub(super) fn prune_layer_comp_references(&mut self) {
        if self.layer_groups.comps.is_empty() {
            return;
        }
        let valid: BTreeSet<_> = self
            .svg_layers
            .iter()
            .map(|l| l.id.as_str())
            .chain(self.layer_groups.groups.iter().map(|g| g.id.as_str()))
            .chain(std::iter::once("layer-1"))
            .collect();
        for comp in &mut self.layer_groups.comps {
            comp.visibility.retain(|id, _| valid.contains(id.as_str()));
            comp.opacity.retain(|id, _| valid.contains(id.as_str()));
        }
    }

    pub(super) fn edit_layer_comp(&mut self, edit: LayerGroupEdit) -> Result<(), String> {
        let id = edit.id.as_deref().unwrap_or("");
        let mut comps = self.layer_groups.comps.clone();
        let mut next_id = self.layer_groups.next_comp_id;
        let name = edit.name.as_deref().unwrap_or("").trim();
        if matches!(edit.action.as_str(), "compCreate" | "compRename") && !valid_name(name) {
            return Err("Invalid layer comp name".into());
        }
        match edit.action.as_str() {
            "compCreate" => {
                if comps.len() >= 32 {
                    return Err("A page can contain up to 32 layer comps".into());
                }
                loop {
                    next_id += 1;
                    if next_id > 1_000_000_000 {
                        return Err("Layer comp ID limit reached".into());
                    }
                    if !comps.iter().any(|c| c.id == format!("comp-{next_id}")) {
                        break;
                    }
                }
                comps.push(LayerComp {
                    id: format!("comp-{next_id}"),
                    name: name.into(),
                    visibility: self.own_layer_visibility(),
                    opacity: self.own_layer_opacity(),
                });
            }
            "compRename" | "compUpdate" | "compDelete" | "compApply" => {
                let index = comps
                    .iter()
                    .position(|c| c.id == id)
                    .ok_or("Layer comp not found")?;
                match edit.action.as_str() {
                    "compRename" => comps[index].name = name.into(),
                    "compUpdate" => {
                        comps[index].visibility = self.own_layer_visibility();
                        comps[index].opacity = self.own_layer_opacity();
                    }
                    "compDelete" => {
                        comps.remove(index);
                    }
                    _ => {}
                }
            }
            _ => return Err("Unknown layer comp action".into()),
        }
        let apply = edit.action == "compApply";
        if (!apply && comps == self.layer_groups.comps)
            || (apply && {
                let comp = comps.iter().find(|comp| comp.id == id).unwrap();
                let visibility = self.own_layer_visibility();
                let opacity = self.own_layer_opacity();
                comp.visibility
                    .iter()
                    .all(|(id, saved)| visibility.get(id) == Some(saved))
                    && comp
                        .opacity
                        .iter()
                        .all(|(id, saved)| opacity.get(id) == Some(saved))
            })
        {
            return Ok(());
        }
        self.finish();
        let mut before = self.vector_history_state();
        if apply {
            let comp = comps.iter().find(|c| c.id == id).unwrap();
            let visibility = &comp.visibility;
            if let Some(value) = comp.opacity.get("layer-1") {
                before.background_opacity = Some(self.layer_opacity);
                self.layer_opacity = *value;
            }
            if let Some(value) = visibility.get("layer-1") {
                self.visible = *value;
            }
            for group in &mut self.layer_groups.groups {
                if let Some(value) = visibility.get(&group.id) {
                    group.visible = *value;
                }
            }
            for layer in &mut self.svg_layers {
                if let Some(value) = comp.opacity.get(&layer.id) {
                    if let Some(member) = self
                        .layer_groups
                        .members
                        .iter_mut()
                        .find(|m| m.id == layer.id)
                    {
                        member.opacity = Some(*value);
                    } else {
                        layer.opacity = *value;
                    }
                }
                if let Some(value) = visibility.get(&layer.id) {
                    if let Some(member) = self
                        .layer_groups
                        .members
                        .iter_mut()
                        .find(|m| m.id == layer.id)
                    {
                        member.visible = *value;
                    } else {
                        layer.visible = *value;
                    }
                }
            }
            self.sync_layer_group_flags();
        }
        self.layer_groups.comps = comps;
        self.layer_groups.next_comp_id = next_id;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(action: &str, id: &str, name: &str) -> LayerGroupEdit {
        LayerGroupEdit {
            action: action.into(),
            id: Some(id.into()),
            name: Some(name.into()),
            target: None,
            ids: vec![],
            additive: false,
            mask: None,
            filter: None,
        }
    }
    fn apply(d: &mut Document, action: &str, id: &str, name: &str) {
        d.edit_layer_groups(edit(action, id, name)).unwrap();
    }
    #[test]
    fn comp_difference_reports_current_to_saved_without_mutation() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        d.svg_layers[0].name = "線画 / Ink / 线稿".into();
        apply(&mut d, "compCreate", "", "Reference");
        d.toggle_layer(&a).unwrap();
        d.svg_layers[0].opacity = 0.4;
        d.layer_opacity = 0.6;
        let added = d.add_vector_layer().unwrap();
        let before = serde_json::to_value(d.document_state()).unwrap();
        let revision = d.revision;
        let selection = d.selected_layer.clone();
        let summary = d.layer_comp_summaries();
        let diff = &summary[0].difference;
        assert!(!summary[0].matches);
        assert_eq!(
            (
                diff.layer_count,
                diff.visibility_count,
                diff.opacity_count,
                diff.uncaptured_count
            ),
            (2, 1, 2, 1)
        );
        let detail = diff.details.iter().find(|detail| detail.id == a).unwrap();
        assert_eq!(detail.name, "線画 / Ink / 线稿");
        assert_eq!(detail.visibility, Some((Some(false), true)));
        assert_eq!(detail.opacity, Some((Some(0.4), 1.0)));
        assert_eq!(
            diff.details
                .iter()
                .find(|detail| detail.id == "layer-1")
                .unwrap()
                .opacity,
            Some((Some(0.6), 1.0))
        );
        assert_eq!(before, serde_json::to_value(d.document_state()).unwrap());
        assert_eq!(revision, d.revision);
        assert_eq!(selection, d.selected_layer);
        apply(&mut d, "compApply", "comp-1", "");
        assert!(d.layer_comp_summaries()[0].matches);
        assert_eq!(d.layer_comp_summaries()[0].difference.uncaptured_count, 1);
        assert!(
            d.svg_layers
                .iter()
                .find(|layer| layer.id == added)
                .unwrap()
                .visible
        );
        d.undo();
        assert_eq!(d.layer_comp_summaries()[0].difference.layer_count, 2);
        d.redo();
        assert_eq!(d.layer_comp_summaries()[0].difference.layer_count, 0);
    }

    #[test]
    fn comp_difference_uses_own_group_flags_and_legacy_capture_scope() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        d.svg_layers[0].opacity = 0.6;
        let mut group = edit("create", "", "Folder");
        group.ids = vec![a.clone()];
        d.edit_layer_groups(group).unwrap();
        let id = d.layer_groups.groups[0].id.clone();
        apply(&mut d, "visibility", &id, "");
        apply(&mut d, "compCreate", "", "Hidden parent");
        let mut mask = edit("mask", &id, "");
        mask.mask = Some(GroupMaskSettings {
            enabled: true,
            inverted: true,
            density: 0.5,
        });
        d.edit_layer_groups(mask).unwrap();
        assert!(d.layer_comp_summaries()[0].matches);
        apply(&mut d, "visibility", &id, "");
        let summary = d.layer_comp_summaries();
        assert_eq!(summary[0].difference.layer_count, 1);
        assert_eq!(summary[0].difference.details[0].id, id);
        assert_eq!(
            summary[0].difference.details[0].visibility,
            Some((Some(true), false))
        );
        assert_eq!(summary[0].difference.opacity_count, 0);
        d.layer_groups.comps[0].opacity.clear();
        d.layer_groups.members[0].opacity = Some(0.2);
        d.sync_layer_group_flags();
        assert_eq!(d.layer_comp_summaries()[0].difference.opacity_count, 0);
        apply(&mut d, "visibility", &id, "");
        assert!(d.layer_comp_summaries()[0].matches);
        d.delete_layer(&a).unwrap();
        assert_eq!(d.layer_comp_summaries()[0].difference.layer_count, 0);
        assert!(d.layer_comp_summaries()[0].difference.details.is_empty());
    }

    #[test]
    fn comp_difference_caps_details_but_counts_all_changed_layers() {
        let mut d = Document::default();
        for _ in 0..16 {
            d.add_vector_layer().unwrap();
        }
        apply(&mut d, "compCreate", "", "Reference");
        for layer in &mut d.svg_layers {
            layer.visible = false;
            layer.opacity = 0.25;
        }
        for member in &mut d.layer_groups.members {
            member.visible = false;
            member.opacity = Some(0.25);
        }
        let summary = d.layer_comp_summaries();
        let diff = &summary[0].difference;
        assert_eq!(
            (diff.layer_count, diff.visibility_count, diff.opacity_count),
            (16, 16, 16)
        );
        assert_eq!(diff.details.len(), 8);
        assert!(diff
            .details
            .iter()
            .all(|detail| detail.visibility.is_some() && detail.opacity.is_some()));
        let mut restored = Document::from_document_state(d.document_state()).unwrap();
        assert_eq!(
            restored.layer_comp_summaries()[0].difference.layer_count,
            16
        );
        apply(&mut restored, "compUpdate", "comp-1", "");
        assert!(restored.layer_comp_summaries()[0].matches);
        restored.undo();
        assert_eq!(
            restored.layer_comp_summaries()[0].difference.layer_count,
            16
        );
    }

    #[test]
    fn layer_comp_opacity_preserves_own_values_masks_artwork_and_history() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        d.layer_opacity = 0.35;
        d.svg_layers[0].opacity = 0.6;
        let mut group = edit("create", "", "Folder");
        group.ids = vec![a.clone()];
        d.edit_layer_groups(group).unwrap();
        let group_id = d.layer_groups.groups[0].id.clone();
        let mut mask = edit("mask", &group_id, "");
        mask.mask = Some(GroupMaskSettings {
            enabled: true,
            inverted: true,
            density: 0.5,
        });
        d.edit_layer_groups(mask).unwrap();
        let source = d.svg_layers[0].source.clone();
        let selected = d.selected_layer.clone();
        apply(&mut d, "compCreate", "", "Soft / 薄く / 淡色");
        assert_eq!(d.layer_groups.comps[0].opacity[&a], 0.6);
        assert!(!d.layer_groups.comps[0].opacity.contains_key(&group_id));
        d.layer_opacity = 0.9;
        d.layer_groups.members[0].opacity = Some(0.2);
        d.sync_layer_group_flags();
        d.toggle_layer("layer-1").unwrap();
        assert!(!d.layer_comp_summaries()[0].matches);
        apply(&mut d, "compApply", "comp-1", "");
        assert_eq!(d.layer_opacity, 0.35);
        assert_eq!(d.layer_groups.members[0].opacity, Some(0.6));
        assert!((d.svg_layers[0].opacity - 0.3).abs() < 1e-6);
        assert!(d.visible && d.layer_comp_summaries()[0].matches);
        assert_eq!(d.selected_layer, selected);
        assert_eq!(d.svg_layers[0].source, source);
        assert_eq!(d.layer_groups.groups[0].mask_density, 0.5);
        d.undo();
        assert_eq!(d.layer_opacity, 0.9);
        assert_eq!(d.layer_groups.members[0].opacity, Some(0.2));
        assert!(!d.visible);
        d.redo();
        assert_eq!(d.layer_opacity, 0.35);
        assert_eq!(d.layer_groups.members[0].opacity, Some(0.6));
        assert!(d.visible);
        let revision = d.revision;
        apply(&mut d, "compApply", "comp-1", "");
        assert_eq!(d.revision, revision);
        // Later mask edits remain independent; a comp compares own opacity.
        d.layer_groups.groups[0].mask_density = 0.75;
        d.sync_layer_group_flags();
        assert!(d.layer_comp_summaries()[0].matches);
        let mut restored = Document::from_document_state(d.document_state()).unwrap();
        assert!(restored.layer_comp_summaries()[0].matches);
        restored.delete_layer(&a).unwrap();
        assert!(!restored.layer_groups.comps[0].opacity.contains_key(&a));
        restored.undo();
        assert_eq!(restored.layer_groups.comps[0].opacity[&a], 0.6);
    }

    #[test]
    fn legacy_comp_opacity_is_unchanged_until_update_and_new_layers_are_untouched() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        apply(&mut d, "compCreate", "", "Legacy");
        let mut state = serde_json::to_value(d.document_state()).unwrap();
        state["layerGroups"]["comps"][0]
            .as_object_mut()
            .unwrap()
            .remove("opacity");
        let mut d = Document::from_document_state(serde_json::from_value(state).unwrap()).unwrap();
        d.layer_opacity = 0.4;
        d.svg_layers[0].opacity = 0.2;
        d.toggle_layer(&a).unwrap();
        apply(&mut d, "compApply", "comp-1", "");
        assert_eq!(d.layer_opacity, 0.4);
        assert_eq!(d.svg_layers[0].opacity, 0.2);
        assert!(d.layer_comp_summaries()[0].matches);
        d.layer_opacity = 0.7;
        d.undo();
        assert_eq!(d.layer_opacity, 0.7);
        d.redo();
        assert_eq!(d.layer_opacity, 0.7);
        d.layer_opacity = 0.4;
        apply(&mut d, "compUpdate", "comp-1", "");
        assert_eq!(d.layer_groups.comps[0].opacity[&a], 0.2);
        d.undo();
        assert!(d.layer_groups.comps[0].opacity.is_empty());
        d.redo();
        assert_eq!(d.layer_groups.comps[0].opacity[&a], 0.2);
        let b = d.add_vector_layer().unwrap();
        d.svg_layers.iter_mut().find(|l| l.id == b).unwrap().opacity = 0.65;
        d.layer_opacity = 0.8;
        apply(&mut d, "compApply", "comp-1", "");
        assert_eq!(d.layer_opacity, 0.4);
        assert_eq!(
            d.svg_layers.iter().find(|l| l.id == b).unwrap().opacity,
            0.65
        );
        // Root opacity is optional history data: unrelated vector Undo preserves it.
        d.layer_opacity = 0.7;
        apply(&mut d, "compRename", "comp-1", "New name");
        d.layer_opacity = 0.9;
        d.undo();
        assert_eq!(d.layer_opacity, 0.9);
    }

    #[test]
    fn comp_opacity_validation_rejects_invalid_values_and_references() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        let mut group = edit("create", "", "Folder");
        group.ids = vec![a.clone()];
        d.edit_layer_groups(group).unwrap();
        let group_id = d.layer_groups.groups[0].id.clone();
        apply(&mut d, "compCreate", "", "A");
        for (id, value) in [
            ("layer-1", -0.1),
            ("layer-1", 1.1),
            ("layer-1", f32::NAN),
            ("layer-1", f32::INFINITY),
            ("missing", 0.5),
            (&group_id, 0.5),
        ] {
            let mut state = d.document_state();
            state.layer_groups.comps[0].opacity.insert(id.into(), value);
            assert!(
                Document::from_document_state(state).is_err(),
                "{id}: {value}"
            );
        }
        let mut state = d.document_state();
        state.layer_groups.comps[0].visibility.remove(&a);
        assert!(Document::from_document_state(state).is_err());
        for value in [0.0, 1.0] {
            let mut state = d.document_state();
            state.layer_groups.comps[0].opacity.insert(a.clone(), value);
            assert!(Document::from_document_state(state).is_ok());
        }
    }

    #[test]
    fn layer_comps_visibility_and_history_preserve_artwork_and_selection() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        apply(&mut d, "compCreate", "", "線画 / Ink / 线稿");
        let id = d.layer_groups.comps[0].id.clone();
        let source = d.svg_layers[0].source.clone();
        d.toggle_layer(&a).unwrap();
        d.toggle_layer("layer-1").unwrap();
        let selected = d.selected_layer.clone();
        assert!(!d.layer_comp_summaries()[0].matches);
        let revision = d.revision;
        apply(&mut d, "compApply", &id, "");
        assert_eq!(d.revision, revision + 1);
        assert!(d.visible && d.svg_layers[0].visible);
        assert_eq!(selected, d.selected_layer);
        assert_eq!(source, d.svg_layers[0].source);
        d.undo();
        assert!(!d.visible && !d.svg_layers[0].visible);
        d.redo();
        assert!(d.visible && d.svg_layers[0].visible);
        let revision = d.revision;
        apply(&mut d, "compApply", &id, "");
        assert_eq!(d.revision, revision);
        apply(&mut d, "compRename", &id, "仕上げ");
        d.undo();
        assert_eq!(d.layer_groups.comps[0].name, "線画 / Ink / 线稿");
        d.redo();
        assert_eq!(d.layer_groups.comps[0].name, "仕上げ");
        d.toggle_layer(&a).unwrap();
        apply(&mut d, "compUpdate", &id, "");
        assert!(!d.layer_groups.comps[0].visibility[&a]);
        d.undo();
        assert!(d.layer_groups.comps[0].visibility[&a]);
        apply(&mut d, "compDelete", &id, "");
        assert!(d.layer_groups.comps.is_empty());
        d.undo();
        assert_eq!(d.layer_groups.comps.len(), 1);
    }
    #[test]
    fn layer_comps_nested_own_visibility_and_new_deleted_layers() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        let mut e = edit("create", "", "folder");
        e.ids = vec![a.clone()];
        d.edit_layer_groups(e).unwrap();
        let group = d.layer_groups.groups[0].id.clone();
        apply(&mut d, "visibility", &group, "");
        assert!(!d.svg_layers[0].visible);
        apply(&mut d, "compCreate", "", "Hidden group / visible child");
        let id = d.layer_groups.comps[0].id.clone();
        assert!(d.layer_groups.comps[0].visibility[&a]);
        assert!(!d.layer_groups.comps[0].visibility[&group]);
        apply(&mut d, "visibility", &group, "");
        d.toggle_layer(&a).unwrap();
        apply(&mut d, "compApply", &id, "");
        apply(&mut d, "visibility", &group, "");
        assert!(d.svg_layers[0].visible);
        d.layer_groups.selected.clear();
        let new = d.add_vector_layer().unwrap();
        d.toggle_layer(&new).unwrap();
        apply(&mut d, "compApply", &id, "");
        assert!(!d.svg_layers.iter().find(|l| l.id == new).unwrap().visible);
        d.delete_layer(&a).unwrap();
        assert!(!d.layer_groups.comps[0].visibility.contains_key(&a));
        d.undo();
        assert!(d.layer_groups.comps[0].visibility.contains_key(&a));
        d.redo();
        let reused = d.add_vector_layer().unwrap();
        assert!(!d.layer_groups.comps[0].visibility.contains_key(&reused));
        apply(&mut d, "compApply", &id, "");
        assert!(
            d.svg_layers
                .iter()
                .find(|l| l.id == reused)
                .unwrap()
                .visible
        );
    }
    #[test]
    fn layer_comps_group_deletion_and_undo_restore_references() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        let mut e = edit("create", "", "folder");
        e.ids = vec![a.clone()];
        d.edit_layer_groups(e).unwrap();
        let group = d.layer_groups.groups[0].id.clone();
        apply(&mut d, "compCreate", "", "A");
        apply(&mut d, "delete", &group, "");
        assert!(!d.layer_groups.comps[0].visibility.contains_key(&group));
        assert!(!d.layer_groups.comps[0].visibility.contains_key(&a));
        d.undo();
        assert!(d.layer_groups.comps[0].visibility.contains_key(&group));
        assert!(d.layer_groups.comps[0].visibility.contains_key(&a));
        d.redo();
        assert!(d.layer_groups.groups.is_empty());
    }
    #[test]
    fn layer_comps_are_page_local_and_round_trip_inactive_pages() {
        let mut d = Document::default();
        apply(&mut d, "compCreate", "", "Page A");
        let page_edit = |action: &str, index: Option<usize>| PageEdit {
            action: action.into(),
            index,
            facing: None,
            binding: None,
        };
        d.edit_pages(page_edit("add", None)).unwrap();
        d.edit_pages(page_edit("select", Some(1))).unwrap();
        assert!(d.layer_groups.comps.is_empty());
        apply(&mut d, "compCreate", "", "Page B");
        let mut restored = Document::from_document_state(d.document_state()).unwrap();
        assert_eq!(restored.layer_groups.comps[0].name, "Page B");
        restored.edit_pages(page_edit("select", Some(0))).unwrap();
        assert_eq!(restored.layer_groups.comps[0].name, "Page A");
        restored.edit_pages(page_edit("select", Some(1))).unwrap();
        assert_eq!(restored.layer_groups.comps[0].name, "Page B");
    }
    #[test]
    fn layer_comps_validation_atomicity_recovery_and_legacy_state() {
        let mut d = Document::default();
        apply(&mut d, "compCreate", "", "A");
        let original = serde_json::to_value(d.document_state()).unwrap();
        let revision = d.revision;
        for e in [
            edit("compRename", "comp-1", "  "),
            edit("compApply", "missing", ""),
            edit("compCreate", "", "a\nb"),
            edit("compWhatever", "", "A"),
        ] {
            assert!(d.edit_layer_groups(e).is_err());
            assert_eq!(original, serde_json::to_value(d.document_state()).unwrap());
            assert_eq!(revision, d.revision);
        }
        let mut decoded =
            Document::from_document_state(serde_json::from_value(original.clone()).unwrap())
                .unwrap();
        assert_eq!(decoded.layer_groups.comps[0].name, "A");
        assert_eq!(
            original,
            serde_json::to_value(d.clone_for_recovery().document_state()).unwrap()
        );
        let mut invalid = decoded.document_state();
        invalid.layer_groups.comps[0]
            .visibility
            .insert("missing".into(), true);
        assert!(Document::from_document_state(invalid).is_err());
        for i in 1..32 {
            apply(&mut decoded, "compCreate", "", &format!("Comp {i}"));
        }
        assert!(decoded
            .edit_layer_groups(edit("compCreate", "", "overflow"))
            .is_err());
        let mut old = original;
        old["layerGroups"].as_object_mut().unwrap().remove("comps");
        old["layerGroups"]
            .as_object_mut()
            .unwrap()
            .remove("nextCompId");
        assert!(
            Document::from_document_state(serde_json::from_value(old).unwrap())
                .unwrap()
                .layer_groups
                .comps
                .is_empty()
        );
    }
}
