use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Guide {
    pub id: String,
    pub axis: Option<String>,
    pub position: f32,
    pub layer_id: Option<String>,
    pub objects: Vec<VectorObject>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuidesState {
    pub visible: bool,
    pub locked: bool,
    pub next_id: u64,
    pub items: Vec<Guide>,
    #[serde(skip)]
    pub selected: Vec<String>,
}
impl Default for GuidesState {
    fn default() -> Self {
        Self {
            visible: true,
            locked: true,
            next_id: 1,
            items: vec![],
            selected: vec![],
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GuideEdit {
    pub action: String,
    pub id: Option<String>,
    pub axis: Option<String>,
    pub position: Option<f32>,
    pub delta: Option<[f32; 2]>,
}
impl GuidesState {
    pub(super) fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        if self.items.len() > 4096 || self.next_id > 1_000_000_000 {
            return Err("Too many guides".into());
        }
        for g in &self.items {
            if g.id.is_empty()
                || g.id.len() > 128
                || !ids.insert(&g.id)
                || !g.position.is_finite()
                || g.position.abs() > 1_000_000.
                || !matches!(
                    g.axis.as_deref(),
                    None | Some("horizontal") | Some("vertical")
                )
                || (g.axis.is_none() && g.objects.is_empty())
            {
                return Err("Invalid guide".into());
            }
            for o in &g.objects {
                o.validate()?;
            }
        }
        Ok(())
    }
}
impl Document {
    pub fn guides(&self) -> &GuidesState {
        &self.guides
    }
    pub fn select_guide(&mut self, id: Option<String>) {
        self.guides.selected.clear();
        if !self.guides.locked && self.guides.visible {
            if let Some(id) = id.filter(|id| self.guides.items.iter().any(|g| &g.id == id)) {
                self.guides.selected.push(id);
                self.selected_vector_objects.clear();
                self.selection = None;
            }
        }
    }
    pub fn guide_hit(&self, p: [f32; 2], tolerance: f32) -> Option<String> {
        if self.guides.locked || !self.guides.visible {
            return None;
        }
        self.guides
            .items
            .iter()
            .rev()
            .find(|g| match g.axis.as_deref() {
                Some("horizontal") => (p[1] - g.position).abs() <= tolerance,
                Some("vertical") => (p[0] - g.position).abs() <= tolerance,
                _ => g
                    .objects
                    .iter()
                    .flat_map(|o| crate::stroke::path_edges(&o.path.data, o.transform))
                    .any(|[a, b]| {
                        let dx = b[0] - a[0];
                        let dy = b[1] - a[1];
                        let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy)
                            / (dx * dx + dy * dy).max(0.0001))
                        .clamp(0., 1.);
                        (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy) <= tolerance
                    }),
            })
            .map(|g| g.id.clone())
    }
    pub fn edit_guides(&mut self, e: GuideEdit) -> Result<(), String> {
        if e.action == "select" {
            self.select_guide(e.id);
            return Ok(());
        }
        self.finish();
        let before = self.vector_history_state();
        match e.action.as_str() {
            "visibility" => {
                self.guides.visible = !self.guides.visible;
                self.guides.selected.clear();
            }
            "lock" => {
                self.guides.locked = !self.guides.locked;
                self.guides.selected.clear();
            }
            "clear" => {
                self.guides.items.clear();
                self.guides.selected.clear();
            }
            "add" => {
                let position = e.position.ok_or("Missing guide position")?;
                let axis = e
                    .axis
                    .filter(|a| a == "horizontal" || a == "vertical")
                    .ok_or("Invalid guide axis")?;
                self.guides.items.push(Guide {
                    id: format!("ruler-guide-{}", self.guides.next_id),
                    axis: Some(axis),
                    position,
                    layer_id: None,
                    objects: vec![],
                });
                self.guides.next_id += 1;
                self.guides.visible = true;
            }
            "move" => {
                if self.guides.locked {
                    return Err("Guides are locked".into());
                }
                let g = self
                    .guides
                    .items
                    .iter_mut()
                    .find(|g| Some(&g.id) == e.id.as_ref())
                    .ok_or("Guide not found")?;
                if g.axis.is_some() {
                    g.position = e.position.ok_or("Missing guide position")?;
                } else {
                    let [dx, dy] = e.delta.ok_or("Missing guide offset")?;
                    if !dx.is_finite()
                        || !dy.is_finite()
                        || dx.abs() > 1_000_000.
                        || dy.abs() > 1_000_000.
                    {
                        return Err("Invalid offset".into());
                    }
                    for o in &mut g.objects {
                        o.transform[4] += dx;
                        o.transform[5] += dy;
                    }
                }
            }
            "delete" => {
                if self.guides.locked {
                    return Err("Guides are locked".into());
                }
                self.guides
                    .items
                    .retain(|g| !self.guides.selected.contains(&g.id));
                self.guides.selected.clear();
            }
            "make" => {
                if self.path_editing.is_some() {
                    return Err("Finish path editing first".into());
                }
                let ids = self.selected_vector_objects.clone();
                let mut items = vec![];
                for l in &mut self.svg_layers {
                    if !l.vector_layer
                        || l.locked
                        || !l.visible
                        || self.locked_artwork_layers.contains(&l.id)
                    {
                        continue;
                    }
                    let objects: Vec<_> = l
                        .vector_objects
                        .iter()
                        .filter(|o| {
                            ids.contains(&o.id)
                                && o.visible
                                && !self.locked_objects.contains(&o.id)
                                && o.text.is_none()
                                && o.image_frame.is_none()
                                && o.clipping_group.is_none()
                        })
                        .cloned()
                        .collect();
                    if objects.is_empty() {
                        continue;
                    }
                    l.vector_objects
                        .retain(|o| !objects.iter().any(|g| g.id == o.id));
                    l.source = vector_svg(self.width, self.height, &l.vector_objects);
                    items.push(Guide {
                        id: format!("object-guide-{}", self.guides.next_id),
                        axis: None,
                        position: 0.,
                        layer_id: Some(l.id.clone()),
                        objects,
                    });
                    self.guides.next_id += 1;
                }
                if items.is_empty() {
                    return Err("Select editable paths to make guides".into());
                }
                self.guides.items.extend(items);
                self.selected_vector_objects.clear();
                self.guides.visible = true;
            }
            "release" => {
                if self.guides.locked {
                    return Err("Unlock guides before releasing them".into());
                }
                let items: Vec<_> = self
                    .guides
                    .items
                    .iter()
                    .filter(|g| self.guides.selected.contains(&g.id))
                    .cloned()
                    .collect();
                if items.is_empty() {
                    return Err("Select guides to release".into());
                }
                let needs_layer = items.iter().any(|g| {
                    g.axis.is_some()
                        || !self.svg_layers.iter().any(|l| {
                            Some(&l.id) == g.layer_id.as_ref()
                                && l.vector_layer
                                && !l.locked
                                && l.visible
                                && !self.locked_artwork_layers.contains(&l.id)
                        })
                });
                if needs_layer && self.svg_layers.len() >= MAX_SVG_LAYERS {
                    return Err("Too many layers".into());
                }
                let mut released_ids = Vec::new();
                let mut layer = SvgLayer {
                    id: format!("released-guides-{}", self.guides.next_id),
                    name: "Released guides".into(),
                    visible: true,
                    opacity: 1.,
                    locked: false,
                    alpha_locked: false,
                    mask_enabled: false,
                    mask_inverted: false,
                    mask_density: 1.,
                    source: String::new(),
                    paint_layer: false,
                    vector_layer: true,
                    vector_objects: vec![],
                };
                for g in &items {
                    if let Some(axis) = &g.axis {
                        let data = if axis == "horizontal" {
                            format!("M0 {} H{}", g.position, self.width)
                        } else {
                            format!("M{} 0 V{}", g.position, self.height)
                        };
                        let mut o:VectorObject=serde_json::from_value(serde_json::json!({"id":g.id,"name":"Released guide","path":{"data":data,"fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":null,"stroke":{"color":[0,0,0,255]},"strokeWidth":1,"visible":true})).map_err(|e|e.to_string())?;
                        o.id = format!("released-{}-{}", self.guides.next_id, g.id);
                        layer.vector_objects.push(o);
                    } else {
                        if let Some(target) = self.svg_layers.iter_mut().find(|l| {
                            Some(&l.id) == g.layer_id.as_ref()
                                && l.vector_layer
                                && !l.locked
                                && l.visible
                                && !self.locked_artwork_layers.contains(&l.id)
                        }) {
                            for o in &g.objects {
                                let mut o = o.clone();
                                if target.vector_objects.iter().any(|v| v.id == o.id) {
                                    o.id = format!("released-{}-{}", self.guides.next_id, o.id);
                                }
                                released_ids.push(o.id.clone());
                                target.vector_objects.push(o);
                            }
                            target.source =
                                vector_svg(self.width, self.height, &target.vector_objects);
                            self.selected_layer = Some(target.id.clone());
                        } else {
                            for o in &g.objects {
                                let mut o = o.clone();
                                o.id = format!("released-{}-{}", self.guides.next_id, o.id);
                                layer.vector_objects.push(o);
                            }
                        }
                    }
                }
                self.guides.next_id += 1;
                if !layer.vector_objects.is_empty() {
                    layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                    released_ids.extend(layer.vector_objects.iter().map(|o| o.id.clone()));
                    self.selected_layer = Some(layer.id.clone());
                    self.svg_layers.push(layer);
                }
                self.selected_vector_objects = released_ids;
                self.guides
                    .items
                    .retain(|g| !self.guides.selected.contains(&g.id));
                self.guides.selected.clear();
            }
            _ => return Err("Unknown guide action".into()),
        }
        if let Err(err) = self.guides.validate() {
            self.restore_vector_history(before);
            return Err(err);
        }
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidesSnapshot {
    pub visible: bool,
    pub locked: bool,
    pub next_id: u64,
    pub selected: Vec<String>,
    pub items: Vec<GuideSummary>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuideSummary {
    pub id: String,
    pub axis: Option<String>,
    pub position: f32,
    pub layer_id: Option<String>,
}
impl GuidesState {
    pub fn snapshot(&self) -> GuidesSnapshot {
        GuidesSnapshot {
            visible: self.visible,
            locked: self.locked,
            next_id: self.next_id,
            selected: self.selected.clone(),
            items: self
                .items
                .iter()
                .map(|g| GuideSummary {
                    id: g.id.clone(),
                    axis: g.axis.clone(),
                    position: g.position,
                    layer_id: g.layer_id.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(action: &str) -> GuideEdit {
        GuideEdit {
            action: action.into(),
            id: None,
            axis: None,
            position: None,
            delta: None,
        }
    }
    #[test]
    fn ruler_guides_validate_roundtrip_and_undo_without_entering_artwork() {
        let mut d = Document::default();
        let mut e = edit("add");
        e.axis = Some("vertical".into());
        e.position = Some(-20.);
        d.edit_guides(e.clone()).unwrap();
        assert!(d.guides.locked);
        assert!(d.guide_hit([-20., 50.], 4.).is_none());
        d.edit_guides(edit("lock")).unwrap();
        let id = d.guide_hit([-20., 50.], 4.).unwrap();
        d.select_guide(Some(id.clone()));
        e = edit("move");
        e.id = Some(id.clone());
        e.position = Some(100.);
        d.edit_guides(e.clone()).unwrap();
        assert_eq!(d.guides.items[0].position, 100.);
        d.undo();
        assert_eq!(d.guides.items[0].position, -20.);
        d.redo();
        assert_eq!(d.guides.items[0].position, 100.);
        let state = d.document_state();
        let mut restored = Document::from_document_state(state).unwrap();
        assert_eq!(restored.guides.items.len(), 1);
        assert!(restored.guides.selected.is_empty());
        assert_eq!(restored.visible_svg_layers().count(), 0);
        e.position = Some(f32::NAN);
        assert!(d.edit_guides(e).is_err());
        assert_eq!(d.guides.items[0].position, 100.);
        restored.select_guide(Some(id));
        restored.edit_guides(edit("release")).unwrap();
        assert!(restored.guides.items.is_empty());
        assert_eq!(restored.visible_svg_layers().count(), 1);
        restored.undo();
        assert_eq!(restored.guides.items.len(), 1);
        d.edit_guides(edit("clear")).unwrap();
        assert!(d.guides.items.is_empty());
        d.undo();
        assert_eq!(d.guides.items.len(), 1);
    }
    #[test]
    fn object_guides_preserve_paths_and_paints_on_release() {
        let mut d = Document::default();
        d.add_vector_layer().unwrap();
        // Create the same portable shape consumed by the rectangle tool.
        let o:VectorObject=serde_json::from_value(serde_json::json!({"id":"shape","name":"Shape","path":{"data":"M10 10 H90 V90 H10 Z","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":{"color":[255,0,0,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
        d.svg_layers[0].vector_objects.push(o.clone());
        d.svg_layers[0].source = vector_svg(d.width, d.height, &d.svg_layers[0].vector_objects);
        d.select_vector_objects(vec!["shape".into()]).unwrap();
        d.edit_guides(edit("make")).unwrap();
        assert!(d.svg_layers[0].vector_objects.is_empty());
        assert_eq!(d.guides.items[0].objects[0], o);
        d.edit_guides(edit("lock")).unwrap();
        let id = d.guide_hit([10., 40.], 2.).unwrap();
        d.select_guide(Some(id));
        d.edit_guides(edit("release")).unwrap();
        let released = d.svg_layers.last().unwrap().vector_objects.first().unwrap();
        assert_eq!(released.path, o.path);
        assert_eq!(released.fill, o.fill);
    }
}
