fn snap_default() -> bool {
    true
}
use super::*;
fn default_nudge() -> [f32; 2] {
    [1., 10.]
}
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
    #[serde(default)]
    pub origin: [f32; 2],
    #[serde(default = "default_nudge")]
    pub nudge: [f32; 2],
    #[serde(default = "snap_default")]
    pub snap: bool,
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
            origin: [0., 0.],
            nudge: default_nudge(),
            snap: true,
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
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GuideLayout {
    version: u32,
    items: Vec<Guide>,
}
impl Document {
    /// Layout coordinates are absolute document pixels; artwork is never included.
    pub fn encode_guide_layout(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&GuideLayout {
            version: 1,
            items: self.guides.items.clone(),
        })
        .map_err(|e| e.to_string())
    }
}
impl GuidesState {
    pub(super) fn validate(&self) -> Result<(), String> {
        if self
            .origin
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            return Err("Invalid ruler origin".into());
        }
        if self
            .nudge
            .iter()
            .any(|v| !v.is_finite() || *v <= 0. || *v > 1_000_000.)
        {
            return Err("Invalid guide keyboard increment".into());
        }
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
    pub fn select_guide_additive(&mut self, id: String) {
        if self.guides.locked
            || !self.guides.visible
            || !self.guides.items.iter().any(|g| g.id == id)
        {
            return;
        }
        if self.guides.selected.contains(&id) {
            self.guides.selected.retain(|v| v != &id);
        } else {
            self.guides.selected.push(id);
        }
        self.selected_vector_objects.clear();
        self.selection = None;
    }
    /// Snap a translation using bounds anchors and optional text baseline endpoints.
    pub fn snap_guide_translation(
        &self,
        bounds: [f32; 4],
        offset: [f32; 2],
        baseline: &[[f32; 2]],
        tolerance: f32,
    ) -> ([f32; 2], Vec<[f32; 2]>) {
        let [x, y, r, b] = bounds;
        let mut anchors = baseline.to_vec();
        for px in [x, (x + r) * 0.5, r] {
            for py in [y, (y + b) * 0.5, b] {
                anchors.push([px, py]);
            }
        }
        let mut corrections = [0.; 2];
        let mut distances = [tolerance; 2];
        let mut markers = [None; 2];
        for a in anchors {
            let point = [a[0] + offset[0], a[1] + offset[1]];
            let snapped = self.snap_to_guides(point, tolerance);
            for i in 0..2 {
                let delta = snapped[i] - point[i];
                let exact = self.guides.snap
                    && self.guides.visible
                    && self.guides.items.iter().any(|g| {
                        g.axis.as_deref() == Some(if i == 0 { "vertical" } else { "horizontal" })
                            && g.position == point[i]
                    });
                if (delta != 0. || exact) && delta.abs() < distances[i] {
                    distances[i] = delta.abs();
                    corrections[i] = delta;
                    markers[i] = Some(snapped);
                }
            }
        }
        (
            [offset[0] + corrections[0], offset[1] + corrections[1]],
            markers.into_iter().flatten().collect(),
        )
    }
    pub fn snap_to_guides(&self, point: [f32; 2], tolerance: f32) -> [f32; 2] {
        if !self.guides.snap || !self.guides.visible || !tolerance.is_finite() || tolerance <= 0. {
            return point;
        }
        let mut result = point;
        let mut distance = [tolerance; 2];
        for g in &self.guides.items {
            if let Some(axis) = g.axis.as_deref() {
                let i = if axis == "vertical" { 0 } else { 1 };
                let d = (point[i] - g.position).abs();
                if d < distance[i] {
                    distance[i] = d;
                    result[i] = g.position;
                }
            } else {
                for [a, b] in g
                    .objects
                    .iter()
                    .flat_map(|o| crate::stroke::path_edges(&o.path.data, o.transform))
                {
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let t = (((point[0] - a[0]) * dx + (point[1] - a[1]) * dy)
                        / (dx * dx + dy * dy).max(0.0001))
                    .clamp(0., 1.);
                    let q = [a[0] + dx * t, a[1] + dy * t];
                    let d = (q[0] - point[0]).hypot(q[1] - point[1]);
                    if d < distance[0].min(distance[1]) {
                        result = q;
                        distance = [d; 2];
                    }
                }
            }
        }
        result
    }
    /// Select guide lines crossing a marquee, including transformed path guides.
    pub fn select_guides_in_rect(
        &mut self,
        start: Point,
        end: Point,
        additive: bool,
    ) -> Result<(), String> {
        if !start.valid() || !end.valid() {
            return Err("Invalid selection rectangle".into());
        }
        let min = [start.x.min(end.x), start.y.min(end.y)];
        let max = [start.x.max(end.x), start.y.max(end.y)];
        let mut ids = if additive {
            self.guides.selected.clone()
        } else {
            Vec::new()
        };
        if !self.guides.locked
            && self.guides.visible
            && max[0] - min[0] >= 1.
            && max[1] - min[1] >= 1.
        {
            for g in &self.guides.items {
                let hit = match g.axis.as_deref() {
                    Some("vertical") => (min[0]..=max[0]).contains(&g.position),
                    Some("horizontal") => (min[1]..=max[1]).contains(&g.position),
                    _ => g
                        .objects
                        .iter()
                        .flat_map(|o| crate::stroke::path_edges(&o.path.data, o.transform))
                        .any(|[a, b]| segment_intersects_rect(a, b, min, max)),
                };
                if hit && !ids.contains(&g.id) {
                    ids.push(g.id.clone());
                }
            }
        }
        self.guides.selected = ids;
        if !self.guides.selected.is_empty() {
            self.selected_vector_objects.clear();
            self.selection = None;
        }
        Ok(())
    }
    /// Artwork has priority; Shift extends an existing guide selection independently.
    pub fn select_layout_in_rect(
        &mut self,
        start: Point,
        end: Point,
        additive: bool,
    ) -> Result<(), String> {
        if additive && !self.guides.selected.is_empty() {
            return self.select_guides_in_rect(start, end, true);
        }
        self.select_vectors_in_rect(start, end, additive)?;
        if self.selected_vector_objects.is_empty() {
            self.select_guides_in_rect(start, end, false)?;
        }
        Ok(())
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
        if matches!(
            e.action.as_str(),
            "selectAll" | "invertSelection" | "deselect"
        ) {
            if e.action == "deselect" {
                self.guides.selected.clear();
                return Ok(());
            }
            if self.guides.locked || !self.guides.visible {
                return Err("Unlock and show guides before selecting them".into());
            }
            self.guides.selected = self
                .guides
                .items
                .iter()
                .filter(|g| e.action == "selectAll" || !self.guides.selected.contains(&g.id))
                .map(|g| g.id.clone())
                .collect();
            if !self.guides.selected.is_empty() {
                self.selected_vector_objects.clear();
                self.selection = None;
            }
            return Ok(());
        }
        self.finish();
        let before = self.vector_history_state();
        match e.action.as_str() {
            "thirds" | "quarters" | "margins" | "importLayout" => {
                let mut items = if e.action == "importLayout" {
                    let source = e.id.as_deref().ok_or("Missing guide layout")?;
                    if source.len() > 8 * 1024 * 1024 {
                        return Err("Guide layout is too large".into());
                    }
                    let layout: GuideLayout =
                        serde_json::from_str(source).map_err(|e| e.to_string())?;
                    if layout.version != 1 {
                        return Err("Unsupported guide layout version".into());
                    }
                    let candidate = GuidesState {
                        items: layout.items.clone(),
                        ..GuidesState::default()
                    };
                    candidate.validate()?;
                    layout.items
                } else {
                    let mut positions = vec![];
                    if e.action == "margins" {
                        for (axis, extent) in [
                            ("vertical", self.width as f32),
                            ("horizontal", self.height as f32),
                        ] {
                            positions.push((axis, extent * 0.1));
                            positions.push((axis, extent * 0.9));
                        }
                    } else {
                        let divisions = if e.action == "thirds" { 3 } else { 4 };
                        for (axis, extent) in [
                            ("vertical", self.width as f32),
                            ("horizontal", self.height as f32),
                        ] {
                            for i in 1..divisions {
                                positions.push((axis, extent * i as f32 / divisions as f32));
                            }
                        }
                    }
                    positions
                        .into_iter()
                        .map(|(axis, position)| Guide {
                            id: String::new(),
                            axis: Some(axis.into()),
                            position,
                            layer_id: None,
                            objects: vec![],
                        })
                        .collect()
                };
                if self.guides.items.len() + items.len() > 4096 {
                    return Err("Too many guides".into());
                }
                for g in &mut items {
                    while self
                        .guides
                        .items
                        .iter()
                        .any(|existing| existing.id == format!("guide-{}", self.guides.next_id))
                    {
                        self.guides.next_id += 1;
                    }
                    g.id = format!("guide-{}", self.guides.next_id);
                    g.layer_id = None;
                    for (i, object) in g.objects.iter_mut().enumerate() {
                        object.id = format!("guide-layout-{}-{i}", self.guides.next_id);
                    }
                    self.guides.next_id += 1;
                }
                self.guides.items.extend(items);
                self.guides.visible = true;
            }
            "increments" => {
                self.guides.nudge = e.delta.ok_or("Missing guide keyboard increments")?
            }
            "snap" => self.guides.snap = !self.guides.snap,
            "origin" => self.guides.origin = e.delta.ok_or("Missing ruler origin")?,
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
            "position" => {
                if self.guides.locked {
                    return Err("Guides are locked".into());
                }
                let position = e.position.ok_or("Missing guide position")?;
                if !position.is_finite() || position.abs() > 1_000_000. {
                    return Err("Invalid guide position".into());
                }
                let axis = e
                    .axis
                    .filter(|a| a == "vertical" || a == "horizontal")
                    .ok_or("Invalid guide axis")?;
                let selected: Vec<_> = self
                    .guides
                    .items
                    .iter()
                    .filter(|g| self.guides.selected.contains(&g.id))
                    .collect();
                if selected.is_empty() || selected.iter().any(|g| g.axis.as_ref() != Some(&axis)) {
                    return Err("Select ruler guides with the same orientation".into());
                }
                for g in &mut self.guides.items {
                    if self.guides.selected.contains(&g.id) {
                        g.position = position;
                    }
                }
            }
            "duplicate" | "moveSelected" => {
                if self.guides.locked {
                    return Err("Guides are locked".into());
                }
                let delta = e.delta.ok_or("Missing guide offset")?;
                if delta.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {
                    return Err("Invalid offset".into());
                }
                let mut moved: Vec<_> = self
                    .guides
                    .items
                    .iter()
                    .filter(|g| self.guides.selected.contains(&g.id))
                    .cloned()
                    .collect();
                if moved.is_empty() {
                    return Err("Select guides first".into());
                }
                let duplicate = e.action == "duplicate";
                if !duplicate
                    && moved.iter().all(|g| match g.axis.as_deref() {
                        Some("horizontal") => delta[1] == 0.,
                        Some("vertical") => delta[0] == 0.,
                        _ => delta == [0., 0.],
                    })
                {
                    return Ok(());
                }
                for g in &mut moved {
                    if duplicate {
                        g.id = format!("guide-{}", self.guides.next_id);
                        for (index, o) in g.objects.iter_mut().enumerate() {
                            o.id = format!("guide-copy-{}-{index}", self.guides.next_id);
                        }
                        self.guides.next_id += 1;
                    }
                    match g.axis.as_deref() {
                        Some("horizontal") => g.position += delta[1],
                        Some("vertical") => g.position += delta[0],
                        _ => {
                            for o in &mut g.objects {
                                o.transform[4] += delta[0];
                                o.transform[5] += delta[1];
                            }
                        }
                    }
                }
                if !duplicate {
                    self.guides
                        .items
                        .retain(|g| !self.guides.selected.contains(&g.id));
                }
                self.guides.selected = moved.iter().map(|g| g.id.clone()).collect();
                self.guides.items.extend(moved);
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
    pub origin: [f32; 2],
    pub nudge: [f32; 2],
    pub snap: bool,
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
            origin: self.origin,
            nudge: self.nudge,
            snap: self.snap,
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

// Liang–Barsky clipping handles segments crossing a rectangle with both endpoints outside.
fn segment_intersects_rect(a: [f32; 2], b: [f32; 2], min: [f32; 2], max: [f32; 2]) -> bool {
    let delta = [b[0] - a[0], b[1] - a[1]];
    let mut range = [0_f32, 1_f32];
    for i in 0..2 {
        if delta[i].abs() < f32::EPSILON {
            if a[i] < min[i] || a[i] > max[i] {
                return false;
            }
        } else {
            let t0 = (min[i] - a[i]) / delta[i];
            let t1 = (max[i] - a[i]) / delta[i];
            range[0] = range[0].max(t0.min(t1));
            range[1] = range[1].min(t0.max(t1));
            if range[0] > range[1] {
                return false;
            }
        }
    }
    true
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
    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }
    #[test]
    fn layouts_append_roundtrip_and_undo_atomically() {
        let mut d = Document::default();
        d.edit_guides(edit("thirds")).unwrap();
        assert_eq!(d.guides.items.len(), 4);
        let source = d.encode_guide_layout().unwrap();
        let first = d.guides.items[0].clone();
        let mut import = edit("importLayout");
        import.id = Some(source);
        d.edit_guides(import.clone()).unwrap();
        assert_eq!(d.guides.items.len(), 8);
        assert_ne!(d.guides.items[4].id, first.id);
        assert_eq!(d.guides.items[4].position, first.position);
        d.undo();
        assert_eq!(d.guides.items.len(), 4);
        import.id = Some(r#"{"version":2,"items":[]}"#.into());
        assert!(d.edit_guides(import).is_err());
        assert_eq!(d.guides.items.len(), 4);
        d.edit_guides(edit("quarters")).unwrap();
        assert_eq!(d.guides.items.len(), 10);
        d.edit_guides(edit("margins")).unwrap();
        assert_eq!(d.guides.items.len(), 14);
        d.undo();
        assert_eq!(d.guides.items.len(), 10);
    }

    #[test]
    fn guide_keyboard_increments_roundtrip_validate_and_undo() {
        let mut d = Document::default();
        assert_eq!(d.guides.nudge, [1., 10.]);
        let mut e = edit("increments");
        e.delta = Some([0.5, 5.]);
        d.edit_guides(e.clone()).unwrap();
        assert_eq!(d.guides.nudge, [0.5, 5.]);
        d.undo();
        assert_eq!(d.guides.nudge, [1., 10.]);
        d.redo();
        assert_eq!(d.guides.nudge, [0.5, 5.]);
        let restored = Document::from_document_state(d.document_state()).unwrap();
        assert_eq!(restored.guides.nudge, [0.5, 5.]);
        for delta in [
            [0., 5.],
            [-1., 5.],
            [f32::NAN, 5.],
            [1., f32::INFINITY],
            [1., 1_000_001.],
        ] {
            e.delta = Some(delta);
            assert!(d.edit_guides(e.clone()).is_err());
            assert_eq!(d.guides.nudge, [0.5, 5.]);
        }
        let mut old = serde_json::to_value(GuidesState::default()).unwrap();
        old.as_object_mut().unwrap().remove("nudge");
        let old: GuidesState = serde_json::from_value(old).unwrap();
        assert_eq!(old.nudge, [1., 10.]);
    }
    #[test]
    fn guide_selection_commands_are_transient_and_respect_visibility_and_locks() {
        let mut d = Document::default();
        for position in [10., 20., 30.] {
            let mut e = edit("add");
            e.axis = Some("vertical".into());
            e.position = Some(position);
            d.edit_guides(e).unwrap();
        }
        let revision = d.revision();
        assert!(d.edit_guides(edit("selectAll")).is_err());
        assert_eq!(d.revision(), revision);
        d.edit_guides(edit("lock")).unwrap();
        let revision = d.revision();
        d.select_guide(Some(d.guides.items[0].id.clone()));
        d.edit_guides(edit("invertSelection")).unwrap();
        assert_eq!(d.guides.selected.len(), 2);
        d.edit_guides(edit("selectAll")).unwrap();
        assert_eq!(d.guides.selected.len(), 3);
        d.edit_guides(edit("invertSelection")).unwrap();
        assert!(d.guides.selected.is_empty());
        d.edit_guides(edit("selectAll")).unwrap();
        d.edit_guides(edit("deselect")).unwrap();
        assert!(d.guides.selected.is_empty());
        assert_eq!(d.revision(), revision);
        d.edit_guides(edit("visibility")).unwrap();
        assert!(d.edit_guides(edit("invertSelection")).is_err());
        let restored = Document::from_document_state(d.document_state()).unwrap();
        assert!(restored.guides.selected.is_empty());
        assert_eq!(restored.guides.items.len(), 3);
    }
    #[test]
    fn guide_marquee_selects_crossings_additively_and_respects_locks() {
        let mut d = Document::default();
        for (axis, position) in [("vertical", 20.), ("horizontal", 40.), ("vertical", 80.)] {
            let mut e = edit("add");
            e.axis = Some(axis.into());
            e.position = Some(position);
            d.edit_guides(e).unwrap();
        }
        let revision = d.revision();
        d.select_guides_in_rect(p(0., 0.), p(50., 50.), false)
            .unwrap();
        assert!(d.guides.selected.is_empty());
        d.edit_guides(edit("lock")).unwrap();
        let revision_unlocked = d.revision();
        d.select_layout_in_rect(p(50., 50.), p(0., 0.), false)
            .unwrap();
        assert_eq!(d.guides.selected.len(), 2);
        assert_eq!(d.revision(), revision_unlocked);
        assert!(d.selected_vector_ids().is_empty());
        d.select_layout_in_rect(p(70., 60.), p(90., 90.), true)
            .unwrap();
        assert_eq!(d.guides.selected.len(), 3);
        d.select_guides_in_rect(p(0., 0.), p(0., 0.), false)
            .unwrap();
        assert!(d.guides.selected.is_empty());
        assert!(d
            .select_guides_in_rect(p(f32::NAN, 0.), p(20., 20.), true)
            .is_err());
        assert!(d.revision() > revision);
        d.edit_guides(edit("visibility")).unwrap();
        d.select_guides_in_rect(p(0., 0.), p(100., 100.), false)
            .unwrap();
        assert!(d.guides.selected.is_empty());
    }
    #[test]
    fn path_guide_marquee_tests_segments_not_just_bounding_boxes() {
        assert!(segment_intersects_rect(
            [-100., 10.],
            [100., 10.],
            [0., 0.],
            [20., 20.]
        ));
        assert!(!segment_intersects_rect(
            [0., 30.],
            [30., 0.],
            [0., 0.],
            [10., 10.]
        ));
        assert!(segment_intersects_rect(
            [5., 5.],
            [5., 5.],
            [0., 0.],
            [10., 10.]
        ));
        let mut d = Document::default();
        d.add_vector_layer().unwrap();
        let o:VectorObject=serde_json::from_value(serde_json::json!({"id":"diagonal","name":"Guide","path":{"data":"M0 30 L30 0","fillRule":"nonZero"},"transform":[1,0,0,1,100,100],"fill":null,"stroke":{"color":[0,0,0,255]},"strokeWidth":1,"visible":true})).unwrap();
        d.svg_layers[0].vector_objects.push(o);
        d.select_vector_objects(vec!["diagonal".into()]).unwrap();
        d.edit_guides(edit("make")).unwrap();
        d.edit_guides(edit("lock")).unwrap();
        d.select_guides_in_rect(p(100., 100.), p(110., 110.), false)
            .unwrap();
        assert!(d.guides.selected.is_empty());
        d.select_guides_in_rect(p(110., 110.), p(120., 120.), false)
            .unwrap();
        assert_eq!(d.guides.selected.len(), 1);
    }
    #[test]
    fn guide_keyboard_offsets_preserve_artwork_and_skip_wrong_axis() {
        let mut d = Document::default();
        let mut e = edit("add");
        e.axis = Some("vertical".into());
        e.position = Some(100.);
        d.edit_guides(e).unwrap();
        d.edit_guides(edit("lock")).unwrap();
        d.select_guide(Some(d.guides.items[0].id.clone()));
        let revision = d.revision();
        let mut e = edit("moveSelected");
        e.delta = Some([0., 1.]);
        d.edit_guides(e.clone()).unwrap();
        assert_eq!(d.revision(), revision);
        e.delta = Some([1., 0.]);
        d.edit_guides(e.clone()).unwrap();
        assert_eq!(d.guides.items[0].position, 101.);
        e.delta = Some([10., 0.]);
        d.edit_guides(e).unwrap();
        assert_eq!(d.guides.items[0].position, 111.);
        d.undo();
        assert_eq!(d.guides.items[0].position, 101.);
        d.undo();
        assert_eq!(d.guides.items[0].position, 100.);
        assert_eq!(d.visible_svg_layers().count(), 0);
    }
    #[test]
    fn numeric_guide_positions_validate_orientation_and_undo() {
        let mut d = Document::default();
        for position in [10., 30.] {
            let mut e = edit("add");
            e.axis = Some("vertical".into());
            e.position = Some(position);
            d.edit_guides(e).unwrap();
        }
        d.edit_guides(edit("lock")).unwrap();
        let ids: Vec<_> = d.guides.items.iter().map(|g| g.id.clone()).collect();
        for id in ids {
            d.select_guide_additive(id);
        }
        let mut e = edit("position");
        e.axis = Some("vertical".into());
        e.position = Some(-12.5);
        d.edit_guides(e.clone()).unwrap();
        assert!(d.guides.items.iter().all(|g| g.position == -12.5));
        d.undo();
        assert_eq!(d.guides.items[0].position, 10.);
        assert_eq!(d.guides.items[1].position, 30.);
        d.redo();
        e.axis = Some("horizontal".into());
        assert!(d.edit_guides(e.clone()).is_err());
        assert!(d.guides.items.iter().all(|g| g.position == -12.5));
        e.axis = Some("vertical".into());
        e.position = Some(f32::INFINITY);
        assert!(d.edit_guides(e).is_err());
        let mut e = edit("position");
        e.axis = Some("vertical".into());
        e.position = Some(20.);
        d.edit_guides(edit("lock")).unwrap();
        assert!(d.edit_guides(e).is_err());
    }
    #[test]
    fn selected_guides_move_duplicate_and_rollback_atomically() {
        let mut d = Document::default();
        for (axis, position) in [("vertical", 100.), ("horizontal", 80.)] {
            let mut e = edit("add");
            e.axis = Some(axis.into());
            e.position = Some(position);
            d.edit_guides(e).unwrap();
        }
        d.edit_guides(edit("lock")).unwrap();
        let ids: Vec<_> = d.guides.items.iter().map(|g| g.id.clone()).collect();
        for id in &ids {
            d.select_guide_additive(id.clone());
        }
        let mut e = edit("moveSelected");
        e.delta = Some([10., 20.]);
        d.edit_guides(e.clone()).unwrap();
        assert_eq!(d.guides.items[0].position, 110.);
        assert_eq!(d.guides.items[1].position, 100.);
        d.undo();
        assert_eq!(d.guides.items[0].position, 100.);
        d.redo();
        e.action = "duplicate".into();
        d.edit_guides(e.clone()).unwrap();
        assert_eq!(d.guides.items.len(), 4);
        assert!(d.guides.selected.iter().all(|id| !ids.contains(id)));
        e.delta = Some([f32::NAN, 0.]);
        assert!(d.edit_guides(e).is_err());
        assert_eq!(d.guides.items.len(), 4);
        d.undo();
        assert_eq!(d.guides.items.len(), 2);
        let restored = Document::from_document_state(d.document_state()).unwrap();
        assert_eq!(restored.guides.items.len(), 2);
    }
    #[test]
    fn translation_snaps_bounds_and_baselines_at_screen_tolerance() {
        let mut d = Document::default();
        for (axis, position) in [("vertical", 100.), ("horizontal", 80.)] {
            let mut e = edit("add");
            e.axis = Some(axis.into());
            e.position = Some(position);
            d.edit_guides(e).unwrap();
        }
        let (offset, markers) =
            d.snap_guide_translation([10., 10., 50., 40.], [47., 12.], &[[20., 65.]], 6.);
        assert_eq!(offset, [50., 15.]);
        assert_eq!(markers.len(), 2);
        assert_eq!(
            d.snap_guide_translation([10., 10., 50., 40.], [47., 12.], &[[20., 65.]], 2.)
                .0,
            [47., 12.]
        );
        d.edit_guides(edit("visibility")).unwrap();
        assert_eq!(
            d.snap_guide_translation([10., 10., 50., 40.], [47., 12.], &[[20., 65.]], 6.)
                .0,
            [47., 12.]
        );
    }
    #[test]
    fn guide_snapping_and_ruler_origin_preserve_geometry_and_undo() {
        let mut d = Document::default();
        let mut e = edit("add");
        e.axis = Some("vertical".into());
        e.position = Some(100.);
        d.edit_guides(e).unwrap();
        assert_eq!(d.snap_to_guides([103., 45.], 6.), [100., 45.]);
        assert_eq!(d.snap_to_guides([110., 45.], 6.), [110., 45.]);
        let mut e = edit("origin");
        e.delta = Some([20., 30.]);
        d.edit_guides(e).unwrap();
        assert_eq!(d.guides.origin, [20., 30.]);
        assert_eq!(d.guides.items[0].position, 100.);
        d.undo();
        assert_eq!(d.guides.origin, [0., 0.]);
        d.redo();
        assert_eq!(d.guides.origin, [20., 30.]);
        d.edit_guides(edit("snap")).unwrap();
        assert_eq!(d.snap_to_guides([103., 45.], 6.), [103., 45.]);
        let restored = Document::from_document_state(d.document_state()).unwrap();
        assert_eq!(restored.guides.origin, [20., 30.]);
        assert!(!restored.guides.snap);
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
