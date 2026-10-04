use super::*;
use std::collections::BTreeSet;

/// Folder hierarchy is independent of renderer resources. Layer flags remain
/// projected for legacy rendering/editing paths, while members retain own flags.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerGroupsState {
    #[serde(default)]
    pub groups: Vec<LayerGroup>,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub members: Vec<LayerGroupMember>,
    #[serde(default)]
    pub next_id: u64,
    #[serde(skip)]
    pub selected: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerGroup {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub collapsed: bool,
    #[serde(default)]
    pub mask_enabled: bool,
    #[serde(default)]
    pub mask_inverted: bool,
    #[serde(default = "default_layer_opacity")]
    pub mask_density: f32,
    pub children: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerGroupMember {
    #[serde(default)]
    pub opacity: Option<f32>,
    pub id: String,
    pub visible: bool,
    pub locked: bool,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerGroupEdit {
    pub action: String,
    pub id: Option<String>,
    pub target: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub additive: bool,
    #[serde(default)]
    pub mask: Option<GroupMaskSettings>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupMaskSettings {
    pub enabled: bool,
    pub inverted: bool,
    pub density: f32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerGroupsSnapshot {
    pub groups: Vec<LayerGroup>,
    pub roots: Vec<String>,
    pub members: Vec<LayerGroupMember>,
    pub selected: Vec<String>,
}
impl LayerGroupsState {
    fn parent(&self, id: &str) -> Option<&LayerGroup> {
        self.groups
            .iter()
            .find(|g| g.children.iter().any(|child| child == id))
    }
    pub(super) fn ancestors(&self, id: &str) -> Vec<&LayerGroup> {
        let mut parents = vec![];
        let mut current = id;
        for _ in 0..=self.groups.len() {
            let Some(parent) = self.parent(current) else {
                break;
            };
            parents.push(parent);
            current = &parent.id;
        }
        parents
    }
    pub(super) fn flatten(&self, ids: &[String], out: &mut Vec<String>) {
        for id in ids {
            if let Some(g) = self.groups.iter().find(|g| &g.id == id) {
                self.flatten(&g.children, out);
            } else {
                out.push(id.clone());
            }
        }
    }
    pub(super) fn reconcile(&mut self, layers: &[SvgLayer]) {
        let valid: BTreeSet<_> = layers
            .iter()
            .map(|l| l.id.clone())
            .chain(self.groups.iter().map(|g| g.id.clone()))
            .collect();
        self.roots.retain(|id| valid.contains(id));
        for g in &mut self.groups {
            g.children.retain(|id| valid.contains(id));
        }
        for l in layers {
            if !self.roots.contains(&l.id) && self.parent(&l.id).is_none() {
                self.roots.insert(0, l.id.clone());
            }
        }
        self.members.retain(|m| layers.iter().any(|l| l.id == m.id));
        self.selected.retain(|id| valid.contains(id));
    }
    pub(super) fn validate(&self, layers: &[SvgLayer]) -> Result<(), String> {
        if self.groups.len() > 4096 || self.next_id > 1_000_000_000 {
            return Err("Too many layer groups".into());
        }
        let mut ids: BTreeSet<String> = layers.iter().map(|l| l.id.clone()).collect();
        for g in &self.groups {
            if g.id.is_empty()
                || g.id.len() > 128
                || !ids.insert(g.id.clone())
                || g.name.trim().is_empty()
                || g.name.chars().count() > 120
                || !g.mask_density.is_finite()
                || !(0.0..=1.0).contains(&g.mask_density)
            {
                return Err("Invalid layer group".into());
            }
        }
        let mut owned = BTreeSet::new();
        for id in self
            .roots
            .iter()
            .chain(self.groups.iter().flat_map(|g| g.children.iter()))
        {
            if !ids.contains(id) || !owned.insert(id.clone()) {
                return Err("Invalid layer group hierarchy".into());
            }
        }
        for g in &self.groups {
            if !owned.contains(&g.id) {
                return Err("Orphan layer group".into());
            }
            let mut visited = BTreeSet::new();
            let mut current = g.id.as_str();
            while let Some(parent) = self.parent(current) {
                if !visited.insert(parent.id.clone()) || visited.len() > 64 {
                    return Err("Cyclic or too deeply nested layer groups".into());
                }
                current = &parent.id;
            }
        }
        let mut members = BTreeSet::new();
        for m in &self.members {
            if m.opacity
                .is_some_and(|o| !o.is_finite() || !(0.0..=1.0).contains(&o))
                || !layers.iter().any(|l| l.id == m.id)
                || !members.insert(&m.id)
            {
                return Err("Invalid group member flags".into());
            }
        }
        for l in layers {
            if self.parent(&l.id).is_some() && !members.contains(&l.id) {
                return Err("Missing group member flags".into());
            }
        }
        Ok(())
    }
}
impl Document {
    pub fn layer_groups_snapshot(&self) -> LayerGroupsSnapshot {
        let mut state = self.layer_groups.clone();
        state.reconcile(&self.svg_layers);
        if let Some(edit) = &self.path_editing {
            state.roots.retain(|id| id != &edit.layer_id);
        }
        LayerGroupsSnapshot {
            groups: state.groups,
            roots: state.roots,
            members: state.members,
            selected: state.selected,
        }
    }
    pub(super) fn sync_layer_group_flags(&mut self) {
        self.layer_groups.reconcile(&self.svg_layers);
        for member in &mut self.layer_groups.members {
            if member.opacity.is_none() {
                member.opacity = self
                    .svg_layers
                    .iter()
                    .find(|l| l.id == member.id)
                    .map(|l| l.opacity);
            }
        }
        for l in &mut self.svg_layers {
            if let Some(m) = self.layer_groups.members.iter().find(|m| m.id == l.id) {
                let ancestors = self.layer_groups.ancestors(&l.id);
                l.visible = m.visible && ancestors.iter().all(|g| g.visible);
                l.locked = m.locked || ancestors.iter().any(|g| g.locked);
                l.opacity = m.opacity.unwrap_or(l.opacity)
                    * ancestors
                        .iter()
                        .map(|g| mask_factor(g.mask_enabled, g.mask_inverted, g.mask_density))
                        .product::<f32>();
            }
        }
        let grouped: BTreeSet<_> = self
            .layer_groups
            .groups
            .iter()
            .flat_map(|g| g.children.iter().cloned())
            .collect();
        self.layer_groups
            .members
            .retain(|m| grouped.contains(&m.id));
    }
    pub(super) fn attach_new_layer_to_selected_group(&mut self, id: &str) -> Result<(), String> {
        let parent = self
            .layer_groups
            .selected
            .iter()
            .find(|id| self.layer_groups.groups.iter().any(|g| &g.id == *id))
            .cloned();
        self.layer_groups.reconcile(&self.svg_layers);
        if let Some(parent) = parent {
            let layer = self
                .svg_layers
                .iter()
                .find(|l| l.id == id)
                .ok_or("Layer not found")?;
            self.layer_groups.members.push(LayerGroupMember {
                opacity: Some(layer.opacity),
                id: id.into(),
                visible: layer.visible,
                locked: layer.locked,
            });
            self.layer_groups.roots.retain(|root| root != id);
            self.layer_groups
                .groups
                .iter_mut()
                .find(|g| g.id == parent)
                .unwrap()
                .children
                .insert(0, id.into());
            self.sync_layer_group_flags();
            let mut order = vec![];
            self.layer_groups
                .flatten(&self.layer_groups.roots, &mut order);
            self.reorder_layers(&order)?;
        }
        Ok(())
    }
    pub(super) fn sync_layer_group_order(&mut self, order: &[String]) -> Result<(), String> {
        if self.layer_groups.groups.is_empty() {
            self.layer_groups.roots = order.to_vec();
            return Ok(());
        }
        let before = self.layer_groups.clone();
        self.layer_groups.reconcile(&self.svg_layers);
        fn sort_nodes(state: &LayerGroupsState, nodes: &mut [String], order: &[String]) {
            nodes.sort_by_key(|id| {
                let mut leaves = vec![];
                state.flatten(std::slice::from_ref(id), &mut leaves);
                leaves
                    .iter()
                    .filter_map(|leaf| order.iter().position(|s| s == leaf))
                    .min()
                    .unwrap_or(usize::MAX)
            });
        }
        let state = self.layer_groups.clone();
        sort_nodes(&state, &mut self.layer_groups.roots, order);
        for group in &mut self.layer_groups.groups {
            sort_nodes(&state, &mut group.children, order);
        }
        let mut flattened = vec![];
        self.layer_groups
            .flatten(&self.layer_groups.roots, &mut flattened);
        if flattened != order {
            self.layer_groups = before;
            return Err("Move layers through the folder panel to change their group".into());
        }
        Ok(())
    }
    pub fn edit_layer_groups(&mut self, e: LayerGroupEdit) -> Result<(), String> {
        if self.path_editing.is_some() {
            return Err("Finish path editing before editing layer groups".into());
        }
        self.layer_groups.reconcile(&self.svg_layers);
        let id = e.id.as_deref().unwrap_or("");
        if e.action == "select" {
            if id != "layer-1"
                && !self.svg_layers.iter().any(|l| l.id == id)
                && !self.layer_groups.groups.iter().any(|g| g.id == id)
            {
                return Err("Layer not found".into());
            }
            if !e.ids.is_empty() {
                if e.ids.iter().any(|id| {
                    id != "layer-1"
                        && !self.svg_layers.iter().any(|l| &l.id == id)
                        && !self.layer_groups.groups.iter().any(|g| &g.id == id)
                }) {
                    return Err("Layer not found".into());
                }
                self.layer_groups.selected = e.ids.clone();
            } else if e.additive {
                if self.layer_groups.selected.iter().any(|s| s == id) {
                    self.layer_groups.selected.retain(|s| s != id);
                } else {
                    self.layer_groups.selected.push(id.into());
                }
            } else {
                self.layer_groups.selected = vec![id.into()];
            }
            if self.svg_layers.iter().any(|l| l.id == id) || id == "layer-1" {
                let selection = self.layer_groups.selected.clone();
                self.select_layer(id.into())?;
                self.layer_groups.selected = selection;
            }
            if self.layer_groups.groups.iter().any(|g| g.id == id) {
                self.selected_layer = Some(id.into());
            }
            self.selected_vector_objects.clear();
            return Ok(());
        }
        self.finish();
        let before = self.vector_history_state();
        let result = self.apply_layer_group_edit(e);
        if let Err(error) = result {
            self.restore_vector_history(before);
            return Err(error);
        }
        self.sync_layer_group_flags();
        self.selected_vector_objects.clear();
        if let Err(error) = self.layer_groups.validate(&self.svg_layers) {
            self.restore_vector_history(before);
            return Err(error);
        }
        let mut order = vec![];
        self.layer_groups
            .flatten(&self.layer_groups.roots, &mut order);
        self.reorder_layers(&order)?;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    fn apply_layer_group_edit(&mut self, e: LayerGroupEdit) -> Result<(), String> {
        let id = e.id.as_deref().unwrap_or("");
        if matches!(
            e.action.as_str(),
            "collapse" | "visibility" | "lock" | "rename" | "mask"
        ) {
            let g = self
                .layer_groups
                .groups
                .iter_mut()
                .find(|g| g.id == id)
                .ok_or("Group not found")?;
            match e.action.as_str() {
                "collapse" => g.collapsed = !g.collapsed,
                "visibility" => g.visible = !g.visible,
                "lock" => g.locked = !g.locked,
                "mask" => {
                    let mask = e.mask.ok_or("Missing group mask settings")?;
                    g.mask_enabled = mask.enabled;
                    g.mask_inverted = mask.inverted;
                    g.mask_density = mask.density;
                }
                _ => g.name = e.name.ok_or("Missing group name")?.trim().into(),
            }
            return Ok(());
        }
        if e.action == "delete" {
            let group = self
                .layer_groups
                .groups
                .iter()
                .find(|g| g.id == id)
                .ok_or("Group not found")?;
            let mut removed = vec![group.id.clone()];
            for _ in 0..self.layer_groups.groups.len() {
                let children: Vec<_> = self
                    .layer_groups
                    .groups
                    .iter()
                    .filter(|g| removed.contains(&g.id))
                    .flat_map(|g| g.children.clone())
                    .collect();
                for child in children {
                    if !removed.contains(&child) {
                        removed.push(child);
                    }
                }
            }
            let objects: Vec<_> = self
                .svg_layers
                .iter()
                .filter(|l| removed.contains(&l.id))
                .flat_map(|l| l.vector_objects.iter().map(|o| o.id.clone()))
                .collect();
            self.svg_layers.retain(|l| !removed.contains(&l.id));
            self.locked_objects.retain(|id| !objects.contains(id));
            self.locked_artwork_layers
                .retain(|id| !removed.contains(id));
            self.layer_groups
                .groups
                .retain(|g| !removed.contains(&g.id));
            self.layer_groups.roots.retain(|id| !removed.contains(id));
            for group in &mut self.layer_groups.groups {
                group.children.retain(|id| !removed.contains(id));
            }
            self.layer_groups.selected.clear();
            self.selected_layer = None;
            return Ok(());
        }
        if e.action == "ungroup" {
            let group = self
                .layer_groups
                .groups
                .iter()
                .find(|g| g.id == id)
                .cloned()
                .ok_or("Group not found")?;
            if let Some(parent) = self
                .layer_groups
                .groups
                .iter_mut()
                .find(|g| g.children.contains(&group.id))
            {
                let index = parent.children.iter().position(|s| s == id).unwrap();
                parent
                    .children
                    .splice(index..=index, group.children.clone());
            } else {
                let index = self
                    .layer_groups
                    .roots
                    .iter()
                    .position(|s| s == id)
                    .ok_or("Group not found")?;
                self.layer_groups
                    .roots
                    .splice(index..=index, group.children.clone());
            }
            self.layer_groups.groups.retain(|g| g.id != id);
            self.selected_layer = group.children.first().cloned();
            self.layer_groups.selected = group.children;
            return Ok(());
        }
        if !matches!(
            e.action.as_str(),
            "create" | "createEmpty" | "move" | "reorder"
        ) {
            return Err("Unknown layer group action".into());
        }
        let mut ids = if e.ids.is_empty() && e.action == "create" {
            self.layer_groups.selected.clone()
        } else {
            e.ids
        };
        let action = if e.action == "createEmpty" {
            "create"
        } else {
            e.action.as_str()
        };
        // Ancestor selection subsumes descendants; preserve current stacking order.
        ids.retain(|id| id != "layer-1");
        let selected = ids.clone();
        ids.retain(|id| {
            !self
                .layer_groups
                .ancestors(id)
                .iter()
                .any(|g| selected.contains(&g.id))
        });
        let valid = |id: &String| {
            self.svg_layers.iter().any(|l| &l.id == id)
                || self.layer_groups.groups.iter().any(|g| &g.id == id)
        };
        if ids.iter().any(|id| !valid(id)) || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err("Invalid group selection".into());
        }
        let target = e.target.filter(|s| !s.is_empty());
        if let Some(target) = &target {
            if !self.layer_groups.groups.iter().any(|g| &g.id == target)
                || ids.contains(target)
                || self
                    .layer_groups
                    .ancestors(target)
                    .iter()
                    .any(|g| ids.contains(&g.id))
            {
                return Err("Cannot move a group into itself or its descendants".into());
            }
        }
        if e.action == "reorder" {
            let destination = if let Some(target) = &target {
                &mut self
                    .layer_groups
                    .groups
                    .iter_mut()
                    .find(|g| &g.id == target)
                    .unwrap()
                    .children
            } else {
                &mut self.layer_groups.roots
            };
            if ids.len() != destination.len() || destination.iter().any(|s| !ids.contains(s)) {
                return Err("Invalid folder order".into());
            }
            *destination = ids;
            return Ok(());
        }
        // Use document tree order rather than click order when grouping.
        fn tree_order(state: &LayerGroupsState, nodes: &[String], out: &mut Vec<String>) {
            for node in nodes {
                out.push(node.clone());
                if let Some(g) = state.groups.iter().find(|g| &g.id == node) {
                    tree_order(state, &g.children, out);
                }
            }
        }
        let mut order = vec![];
        tree_order(&self.layer_groups, &self.layer_groups.roots, &mut order);
        ids.sort_by_key(|id| order.iter().position(|s| s == id).unwrap_or(usize::MAX));
        for layer in &self.svg_layers {
            if ids.contains(&layer.id)
                && !self.layer_groups.members.iter().any(|m| m.id == layer.id)
            {
                self.layer_groups.members.push(LayerGroupMember {
                    opacity: Some(layer.opacity),
                    id: layer.id.clone(),
                    visible: layer.visible,
                    locked: layer.locked,
                });
            }
        }
        let insertion = self
            .layer_groups
            .roots
            .iter()
            .position(|id| ids.contains(id))
            .unwrap_or(0);
        // An omitted target means the common parent when creating a group.
        let target = if action == "create" && target.is_none() && !ids.is_empty() {
            let parent = self.layer_groups.parent(&ids[0]).map(|g| g.id.clone());
            if ids
                .iter()
                .all(|id| self.layer_groups.parent(id).map(|g| &g.id) == parent.as_ref())
            {
                parent
            } else {
                None
            }
        } else {
            target
        };
        let child_insertion = target
            .as_ref()
            .and_then(|target| self.layer_groups.groups.iter().find(|g| &g.id == target))
            .and_then(|g| g.children.iter().position(|id| ids.contains(id)))
            .unwrap_or(0);
        self.layer_groups.roots.retain(|id| !ids.contains(id));
        for g in &mut self.layer_groups.groups {
            g.children.retain(|id| !ids.contains(id));
        }
        if action == "create" {
            while valid_group_id(self, self.layer_groups.next_id) {
                self.layer_groups.next_id += 1;
            }
            let new_id = format!("layer-group-{}", self.layer_groups.next_id);
            self.layer_groups.next_id += 1;
            self.layer_groups.groups.push(LayerGroup {
                id: new_id.clone(),
                name: e.name.unwrap_or_else(|| "Group".into()),
                visible: true,
                locked: false,
                collapsed: false,
                mask_enabled: false,
                mask_inverted: false,
                mask_density: 1.,
                children: ids,
            });
            ids = vec![new_id.clone()];
            self.selected_layer = Some(new_id.clone());
            self.layer_groups.selected = vec![new_id];
        } else {
            self.layer_groups.selected = ids.clone();
        }
        if let Some(target) = target {
            let children = &mut self
                .layer_groups
                .groups
                .iter_mut()
                .find(|g| g.id == target)
                .unwrap()
                .children;
            let index = if action == "create" {
                child_insertion.min(children.len())
            } else {
                0
            };
            children.splice(index..index, ids);
        } else {
            let index = if action == "create" {
                insertion.min(self.layer_groups.roots.len())
            } else {
                0
            };
            self.layer_groups.roots.splice(index..index, ids);
        }
        Ok(())
    }
}
fn valid_group_id(d: &Document, number: u64) -> bool {
    let id = format!("layer-group-{number}");
    d.svg_layers.iter().any(|l| l.id == id) || d.layer_groups.groups.iter().any(|g| g.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(action: &str) -> LayerGroupEdit {
        LayerGroupEdit {
            action: action.into(),
            id: None,
            target: None,
            name: None,
            ids: vec![],
            additive: false,
            mask: None,
        }
    }
    fn group(d: &mut Document, ids: Vec<String>) -> String {
        let mut e = edit("create");
        e.ids = ids;
        d.edit_layer_groups(e).unwrap();
        d.layer_groups.selected[0].clone()
    }
    #[test]
    fn group_masks_preserve_child_opacity_nested_restore_and_history() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        d.svg_layers[0].opacity = 0.6;
        let inner = group(&mut d, vec![a.clone()]);
        let mut e = edit("mask");
        e.id = Some(inner.clone());
        e.mask = Some(GroupMaskSettings {
            enabled: true,
            inverted: true,
            density: 0.5,
        });
        d.edit_layer_groups(e.clone()).unwrap();
        assert!((d.svg_layers[0].effective_opacity() - 0.3).abs() < 1e-6);
        assert!((d.snapshot().layers[1].opacity - 0.6).abs() < 1e-6);
        let outer = group(&mut d, vec![inner.clone()]);
        e.id = Some(outer.clone());
        d.edit_layer_groups(e.clone()).unwrap();
        assert!((d.svg_layers[0].effective_opacity() - 0.15).abs() < 1e-6);
        let mut restored = Document::decode(&d.encode().unwrap()).unwrap();
        assert!((restored.svg_layers[0].effective_opacity() - 0.15).abs() < 1e-6);
        let before = restored.encode().unwrap();
        e.mask.as_mut().unwrap().density = 2.;
        assert!(restored.edit_layer_groups(e.clone()).is_err());
        assert_eq!(restored.encode().unwrap(), before);
        e.mask.as_mut().unwrap().density = 0.5;
        e.mask.as_mut().unwrap().enabled = false;
        restored.edit_layer_groups(e).unwrap();
        assert!((restored.svg_layers[0].effective_opacity() - 0.3).abs() < 1e-6);
        restored.undo();
        assert!((restored.svg_layers[0].effective_opacity() - 0.15).abs() < 1e-6);
        restored.redo();
        assert!((restored.svg_layers[0].effective_opacity() - 0.3).abs() < 1e-6);
        let mut move_out = edit("move");
        move_out.ids = vec![a];
        move_out.target = Some(String::new());
        restored.edit_layer_groups(move_out).unwrap();
        assert!((restored.svg_layers[0].effective_opacity() - 0.6).abs() < 1e-6);
    }
    #[test]
    fn legacy_group_files_without_masks_and_member_opacity_remain_editable() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        d.svg_layers[0].opacity = 0.4;
        let g = group(&mut d, vec![a]);
        let mut json = serde_json::to_value(d.document_state()).unwrap();
        for group in json["layerGroups"]["groups"].as_array_mut().unwrap() {
            for key in ["maskEnabled", "maskInverted", "maskDensity"] {
                group.as_object_mut().unwrap().remove(key);
            }
        }
        for member in json["layerGroups"]["members"].as_array_mut().unwrap() {
            member.as_object_mut().unwrap().remove("opacity");
        }
        let mut restored =
            Document::from_document_state(serde_json::from_value(json).unwrap()).unwrap();
        let mut e = edit("mask");
        e.id = Some(g);
        e.mask = Some(GroupMaskSettings {
            enabled: true,
            inverted: true,
            density: 0.5,
        });
        restored.edit_layer_groups(e).unwrap();
        assert!((restored.svg_layers[0].opacity - 0.2).abs() < 1e-6);
    }
    #[test]
    fn folders_preserve_order_flags_undo_and_native_roundtrip() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        let b = d.add_paint_layer().unwrap();
        d.toggle_layer(&a).unwrap();
        let g = group(&mut d, vec![a.clone(), b.clone()]);
        assert_eq!(
            d.layer_groups.groups[0].children,
            vec![b.clone(), a.clone()]
        );
        let mut e = edit("visibility");
        e.id = Some(g.clone());
        d.edit_layer_groups(e.clone()).unwrap();
        assert!(d.svg_layers.iter().all(|l| !l.visible));
        let json = serde_json::to_string(&d.document_state()).unwrap();
        let file: DocumentState = serde_json::from_str(&json).unwrap();
        let mut loaded = Document::from_document_state(file).unwrap();
        loaded.edit_layer_groups(e).unwrap();
        assert!(
            loaded
                .svg_layers
                .iter()
                .find(|l| l.id == b)
                .unwrap()
                .visible
        );
        assert!(
            !loaded
                .svg_layers
                .iter()
                .find(|l| l.id == a)
                .unwrap()
                .visible
        );
        let mut e = edit("ungroup");
        e.id = Some(g);
        loaded.edit_layer_groups(e).unwrap();
        assert!(loaded.layer_groups.groups.is_empty());
        assert!(
            !loaded
                .svg_layers
                .iter()
                .find(|l| l.id == a)
                .unwrap()
                .visible
        );
        loaded.undo();
        assert_eq!(loaded.layer_groups.groups.len(), 1);
        loaded.redo();
        assert!(loaded.layer_groups.groups.is_empty());
    }
    #[test]
    fn nesting_move_cycle_rejection_and_inherited_lock() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        let b = d.add_vector_layer().unwrap();
        let inner = group(&mut d, vec![a.clone()]);
        let outer = group(&mut d, vec![inner.clone(), b.clone()]);
        let mut e = edit("lock");
        e.id = Some(outer.clone());
        d.edit_layer_groups(e.clone()).unwrap();
        assert!(d.svg_layers.iter().all(|l| l.locked));
        d.select_layer(a.clone()).unwrap();
        assert!(d.selected_vector_target().is_err());
        let before = serde_json::to_string(&d.document_state()).unwrap();
        let mut invalid = edit("move");
        invalid.ids = vec![outer.clone()];
        invalid.target = Some(inner.clone());
        assert!(d.edit_layer_groups(invalid).is_err());
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), before);
        let mut move_out = edit("move");
        move_out.ids = vec![a.clone()];
        move_out.target = Some(String::new());
        d.edit_layer_groups(move_out).unwrap();
        assert!(!d.svg_layers.iter().find(|l| l.id == a).unwrap().locked);
        assert!(d.svg_layers.iter().find(|l| l.id == b).unwrap().locked);
        d.undo();
        assert!(d.svg_layers.iter().all(|l| l.locked));
    }
    #[test]
    fn folder_selection_range_creation_deletion_and_new_layers() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        let b = d.add_vector_layer().unwrap();
        let mut e = edit("select");
        e.id = Some(a.clone());
        d.edit_layer_groups(e.clone()).unwrap();
        e.id = Some(b.clone());
        e.additive = true;
        d.edit_layer_groups(e).unwrap();
        assert_eq!(d.layer_groups.selected.len(), 2);
        let g = group(&mut d, vec![]);
        assert_eq!(d.snapshot().layer_id, g);
        let c = d.add_vector_layer().unwrap();
        assert!(d.layer_groups.groups[0].children.contains(&c));
        let mut e = edit("delete");
        e.id = Some(g);
        d.edit_layer_groups(e).unwrap();
        assert!(d.svg_layers.is_empty());
        d.undo();
        assert_eq!(d.svg_layers.len(), 3);
        d.redo();
        assert!(d.svg_layers.is_empty());
    }
    #[test]
    fn invalid_files_names_and_flat_reordering_are_rejected() {
        let mut d = Document::default();
        let a = d.add_vector_layer().unwrap();
        let b = d.add_vector_layer().unwrap();
        let c = d.add_vector_layer().unwrap();
        let g = group(&mut d, vec![a.clone(), b.clone()]);
        let before = d.layer_groups_snapshot();
        assert!(d.reorder_layers(&[a.clone(), c, b.clone()]).is_err());
        assert_eq!(d.layer_groups_snapshot().roots, before.roots);
        let mut e = edit("rename");
        e.id = Some(g.clone());
        e.name = Some(" ".into());
        assert!(d.edit_layer_groups(e).is_err());
        let mut state = d.document_state();
        state.layer_groups.groups[0].children.push(g);
        assert!(Document::from_document_state(state).is_err());
        let mut old = serde_json::to_value(Document::default().document_state()).unwrap();
        old.as_object_mut().unwrap().remove("layerGroups");
        assert!(Document::from_document_state(serde_json::from_value(old).unwrap()).is_ok());
    }
}
