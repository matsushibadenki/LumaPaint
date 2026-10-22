//! Object-selection queries and named stable-ID selections. No SVG generation.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedVectorSelection {
    pub name: String,
    pub object_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedVectorSelectionSummary {
    pub name: String,
    pub count: usize,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorSelectionRequest {
    pub action: String,
    pub criterion: Option<String>,
    pub name: Option<String>,
    pub new_name: Option<String>,
    #[serde(default)]
    pub bulk: Option<BulkVectorSelection>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkVectorSelection {
    pub criteria: Vec<String>,
    pub match_any: bool,
    pub scope: String,
}
pub(super) fn validate_saved(items: &[SavedVectorSelection]) -> Result<(), String> {
    if items.len() > 128 {
        return Err("Too many saved selections".into());
    }
    let mut names = std::collections::BTreeSet::new();
    for item in items {
        valid_name(&item.name)?;
        if !names.insert(&item.name)
            || item.object_ids.len() > 4096
            || item
                .object_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 200)
            || item
                .object_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != item.object_ids.len()
        {
            return Err("Invalid saved selection".into());
        }
    }
    Ok(())
}
fn valid_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty()
        || name != name.trim()
        || name.chars().count() > 100
        || name.chars().any(char::is_control)
    {
        Err("Invalid selection name / 選択名が不正です / 选择名称无效".into())
    } else {
        Ok(())
    }
}
impl Document {
    fn selection_attribute_matches(
        &self,
        key: &str,
        layer: &SvgLayer,
        object: &VectorObject,
        reference_layer: &SvgLayer,
        reference: &VectorObject,
    ) -> bool {
        if ["graphicStyle", "symbolInstance", "linkedText"].contains(&key) {
            return self.vector_assets.same(key, &object.id, &reference.id);
        }
        let effects_equal = || {
            self.layer_effects.get(&layer.id).filter(|e| e.active())
                == self
                    .layer_effects
                    .get(&reference_layer.id)
                    .filter(|e| e.active())
        };
        if key == "effects" {
            return effects_equal();
        }
        same(key, layer, object, reference_layer, reference)
            && (key != "appearance" || effects_equal())
    }
    pub fn selected_direction_handles(&self) -> Result<Vec<(String, usize)>, String> {
        let mut controls = Vec::new();
        for (_, object) in self
            .selection_candidates()
            .into_iter()
            .filter(|(_, o)| self.selected_vector_objects.contains(&o.id))
        {
            let Some(editable) = crate::bezier::editable(object) else {
                continue;
            };
            for start in crate::bezier::segment_indices(&editable) {
                for (index, anchor) in [(start + 1, start), (start + 2, start + 3)] {
                    if editable.control_points[index] != editable.control_points[anchor] {
                        controls.push((object.id.clone(), index));
                        if controls.len() > 65536 {
                            return Err(
                                "Too many controls / 選択するハンドルが多すぎます / 控制点过多"
                                    .into(),
                            );
                        }
                    }
                }
            }
        }
        Ok(controls)
    }
    pub fn transform_vector_controls_matrix(
        &mut self,
        controls: &[(String, usize)],
        matrix: [f32; 6],
    ) -> Result<(), String> {
        if matrix.iter().any(|v| !v.is_finite() || v.abs() > 100000.) {
            return Err("Invalid control transform".into());
        }
        if matrix == [1., 0., 0., 1., 0., 0.] {
            return Ok(());
        }
        self.edit_direct_controls(controls, |object, indices| {
            let mut edited = object.clone();
            crate::bezier::transform_controls(&mut edited, indices, matrix)?;
            Ok(Some(edited))
        })
    }
    pub fn transform_vector_controls(
        &mut self,
        controls: &[(String, usize)],
        action: &str,
        values: [f32; 4],
    ) -> Result<(), String> {
        if controls.is_empty() || values.iter().any(|v| !v.is_finite() || v.abs() > 100000.) {
            return Err("Invalid control transform".into());
        }
        if matches!(action, "scale" | "individual")
            && (!(0.01..=100.).contains(&values[0]) || !(0.01..=100.).contains(&values[1]))
        {
            return Err(
                "Scale must be 1–10000% / 倍率は1〜10000%です / 缩放比例应为1–10000%".into(),
            );
        }
        if action == "shear" && values[0].abs() >= 89. {
            return Err("Invalid shear angle".into());
        }
        let unchanged = match action {
            "move" => values[0] == 0. && values[1] == 0.,
            "scale" => values[0] == 1. && values[1] == 1.,
            "rotate" | "shear" => values[0] == 0.,
            "individual" => values[0] == 1. && values[1] == 1. && values[2] == 0.,
            _ => false,
        };
        if unchanged {
            return Ok(());
        }
        let objects = self.direct_objects();
        let mut points = Vec::new();
        for (id, index) in controls {
            let o = objects
                .iter()
                .find(|(_, o)| &o.id == id)
                .map(|(_, o)| o)
                .ok_or("Path missing")?;
            let p = *o
                .control_points
                .get(*index)
                .ok_or("Invalid control index")?;
            points.push(crate::bezier::world_point(o, p));
        }
        let center = |points: &[[f32; 2]]| {
            [
                (points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min)
                    + points
                        .iter()
                        .map(|p| p[0])
                        .fold(f32::NEG_INFINITY, f32::max))
                    * 0.5,
                (points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min)
                    + points
                        .iter()
                        .map(|p| p[1])
                        .fold(f32::NEG_INFINITY, f32::max))
                    * 0.5,
            ]
        };
        let origin = center(&points);
        self.edit_direct_controls(controls, |object, indices| {
            let origin = if action == "individual" {
                center(
                    &indices
                        .iter()
                        .map(|&i| crate::bezier::world_point(object, object.control_points[i]))
                        .collect::<Vec<_>>(),
                )
            } else {
                origin
            };
            let [mut a, mut b, mut c, mut d] = [1., 0., 0., 1.];
            let mut offset = [0., 0.];
            match action {
                "move" => offset = [values[0], values[1]],
                "scale" => {
                    a = values[0];
                    d = values[1];
                }
                "rotate" | "individual" => {
                    let (sn, cs) = values[if action == "rotate" { 0 } else { 2 }]
                        .to_radians()
                        .sin_cos();
                    let sx = if action == "individual" {
                        values[0]
                    } else {
                        1.
                    };
                    let sy = if action == "individual" {
                        values[1]
                    } else {
                        1.
                    };
                    a = cs * sx;
                    b = sn * sx;
                    c = -sn * sy;
                    d = cs * sy;
                }
                "reflect" => {
                    let (sn, cs) = (2. * values[0].to_radians()).sin_cos();
                    a = cs;
                    b = sn;
                    c = sn;
                    d = -cs;
                }
                "shear" => c = values[0].to_radians().tan(),
                _ => return Err("Unsupported control transform".into()),
            }
            let matrix = [
                a,
                b,
                c,
                d,
                origin[0] - a * origin[0] - c * origin[1] + offset[0],
                origin[1] - b * origin[0] - d * origin[1] + offset[1],
            ];
            let mut edited = object.clone();
            crate::bezier::transform_controls(&mut edited, indices, matrix)?;
            Ok(Some(edited))
        })
    }
    fn selection_candidates(&self) -> Vec<(&SvgLayer, &VectorObject)> {
        self.svg_layers
            .iter()
            .filter(|l| l.vector_layer && l.visible && !l.locked && self.can_edit_path_layer(l))
            .flat_map(|l| {
                l.vector_objects
                    .iter()
                    .filter(|o| o.visible && !self.object_is_locked(&o.id))
                    .map(move |o| (l, o))
            })
            .collect()
    }
    pub fn vector_selection_action(
        &mut self,
        request: VectorSelectionRequest,
    ) -> Result<(), String> {
        let c = self.selection_candidates();
        let selected = &self.selected_vector_objects;
        let reference: Vec<_> = c
            .iter()
            .filter(|(_, o)| selected.contains(&o.id))
            .copied()
            .collect();
        let criterion = request.criterion.as_deref().unwrap_or("");
        let ids = match request.action.as_str() {
            "all" => c.iter().map(|(_, o)| o.id.clone()).collect(),
            "artboard" => c
                .iter()
                .filter(|(_, o)| {
                    o.intersects_selection([0., 0., self.width as f32, self.height as f32], false)
                })
                .map(|(_, o)| o.id.clone())
                .collect(),
            "deselect" => Vec::new(),
            "invert" => c
                .iter()
                .filter(|(_, o)| !selected.contains(&o.id))
                .map(|(_, o)| o.id.clone())
                .collect(),
            "reselect" => c
                .iter()
                .filter(|(_, o)| self.previous_vector_selection.contains(&o.id))
                .map(|(_, o)| o.id.clone())
                .collect(),
            "above" | "below" => {
                let positions: Vec<_> = c
                    .iter()
                    .enumerate()
                    .filter(|(_, (_, o))| selected.contains(&o.id))
                    .map(|(i, _)| i)
                    .collect();
                let index = if request.action == "above" {
                    positions.last().and_then(|i| c.get(i + 1))
                } else {
                    positions
                        .first()
                        .and_then(|i| i.checked_sub(1))
                        .and_then(|i| c.get(i))
                };
                let Some((_, o)) = index else {
                    return Ok(());
                };
                vec![o.id.clone()]
            }
            "bulk" => {
                let options = request
                    .bulk
                    .as_ref()
                    .ok_or("Missing bulk selection conditions")?;
                if options.criteria.is_empty()
                    || options.criteria.len() > SAME_CRITERIA.len()
                    || options
                        .criteria
                        .iter()
                        .any(|k| !SAME_CRITERIA.contains(&k.as_str()))
                    || !["page", "artboard", "sameLayers"].contains(&options.scope.as_str())
                {
                    return Err("Invalid bulk selection conditions / 一括選択の条件が不正です / 批量选择条件无效".into());
                }
                if reference.is_empty() {
                    return Err("Select reference objects / 基準のオブジェクトを選択してください / 请选择参考对象".into());
                }
                let layers: std::collections::BTreeSet<_> =
                    reference.iter().map(|(l, _)| l.id.as_str()).collect();
                c.iter()
                    .filter(|(l, o)| {
                        (options.scope != "sameLayers" || layers.contains(l.id.as_str()))
                            && (options.scope != "artboard"
                                || o.intersects_selection(
                                    [0., 0., self.width as f32, self.height as f32],
                                    false,
                                ))
                            && reference.iter().any(|(rl, r)| {
                                if options.match_any {
                                    options
                                        .criteria
                                        .iter()
                                        .any(|k| self.selection_attribute_matches(k, l, o, rl, r))
                                } else {
                                    options
                                        .criteria
                                        .iter()
                                        .all(|k| self.selection_attribute_matches(k, l, o, rl, r))
                                }
                            })
                    })
                    .map(|(_, o)| o.id.clone())
                    .collect()
            }
            "same" => {
                if !SAME_CRITERIA.contains(&criterion) {
                    return Err("Unknown same-object criterion".into());
                }
                if reference.is_empty() {
                    return Err("Select a reference object / 基準のオブジェクトを選択してください / 请选择参考对象".into());
                }
                if ["graphicStyle", "symbolInstance", "linkedText"].contains(&criterion)
                    && !reference
                        .iter()
                        .any(|(_, r)| self.vector_assets.same(criterion, &r.id, &r.id))
                {
                    return Err("No linked reference / 関連付けられたオブジェクトを選択してください / 请选择已关联的参考对象".into());
                }
                let asset_members =
                    if ["graphicStyle", "symbolInstance", "linkedText"].contains(&criterion) {
                        self.vector_assets.matching_ids(
                            criterion,
                            &reference
                                .iter()
                                .map(|(_, o)| o.id.as_str())
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    };
                let shapes: std::collections::BTreeSet<_> = if criterion == "shape" {
                    reference
                        .iter()
                        .filter(|(_, o)| o.text.is_none())
                        .map(|(_, o)| shape(o))
                        .filter(|key| !key.is_empty())
                        .collect()
                } else {
                    Default::default()
                };
                c.iter()
                    .filter(|(l, o)| {
                        if let Some(members) = &asset_members {
                            return members.contains(o.id.as_str());
                        }
                        if criterion == "shape" {
                            return o.text.is_none() && shapes.contains(&shape(o));
                        }
                        reference
                            .iter()
                            .any(|(rl, r)| self.selection_attribute_matches(criterion, l, o, rl, r))
                    })
                    .map(|(_, o)| o.id.clone())
                    .collect()
            }
            "object" => {
                if criterion == "segments" {
                    let handles = self.selected_direction_handles()?;
                    if handles.is_empty() {
                        return Err("Select paths with direction handles / 方向ハンドルを持つパスを選択してください / 请选择带方向手柄的路径".into());
                    }
                    let selected: std::collections::BTreeSet<_> =
                        handles.iter().map(|(id, _)| id.as_str()).collect();
                    let ids = c
                        .iter()
                        .filter(|(_, o)| selected.contains(o.id.as_str()))
                        .map(|(_, o)| o.id.clone())
                        .collect();
                    return self.apply_vector_selection(ids);
                }
                if !OBJECT_CRITERIA.contains(&criterion) {
                    return Err("Unknown object criterion".into());
                }
                let layers: std::collections::BTreeSet<_> =
                    reference.iter().map(|(l, _)| l.id.as_str()).collect();
                c.iter()
                    .filter(|(l, o)| match criterion {
                        "sameLayers" => layers.contains(l.id.as_str()),
                        "bristleBrushStrokes" => {
                            brushed(o)
                                && o.stroke_style.brush == crate::stroke::VectorBrush::Bristle
                        }
                        "pencilBrushStrokes" => {
                            brushed(o) && o.stroke_style.brush == crate::stroke::VectorBrush::Pencil
                        }
                        "brushStrokes" => brushed(o),
                        "clippingMasks" => o.clipping_group.is_some(),
                        "strayPoints" => stray(o),
                        "text" => o.text.is_some(),
                        "pointText" => o.text.as_ref().is_some_and(|t| t.point_text),
                        "areaText" => o.text.as_ref().is_some_and(|t| !t.point_text),
                        _ => false,
                    })
                    .map(|(_, o)| o.id.clone())
                    .collect()
            }
            "load" => {
                let name = request.name.as_deref().unwrap_or("");
                let saved = self
                    .saved_vector_selections
                    .iter()
                    .find(|s| s.name == name)
                    .ok_or("Saved selection not found / 保存された選択が見つかりません / 找不到已保存的选择")?;
                c.iter()
                    .filter(|(_, o)| saved.object_ids.contains(&o.id))
                    .map(|(_, o)| o.id.clone())
                    .collect()
            }
            "save" | "rename" | "delete" | "update" => {
                let name = request.name.as_deref().unwrap_or("");
                valid_name(name)?;
                let mut next = self.saved_vector_selections.as_ref().clone();
                let index = next.iter().position(|s| s.name == name);
                match request.action.as_str() {
                    "save" => {
                        if index.is_some() {
                            return Err("Selection name already exists / 同じ選択名が存在します / 选择名称已存在".into());
                        }
                        if reference.is_empty() {
                            return Err("Select objects first / オブジェクトを選択してください / 请先选择对象".into());
                        }
                        next.push(SavedVectorSelection {
                            name: name.into(),
                            object_ids: reference.iter().map(|(_, o)| o.id.clone()).collect(),
                        });
                    }
                    "rename" => {
                        let new_name = request.new_name.as_deref().unwrap_or("");
                        valid_name(new_name)?;
                        if new_name != name && next.iter().any(|s| s.name == new_name) {
                            return Err("Selection name already exists / 同じ選択名が存在します / 选择名称已存在".into());
                        }
                        next[index.ok_or("Saved selection not found / 保存された選択が見つかりません / 找不到已保存的选择")?].name = new_name.into();
                    }
                    "delete" => {
                        next.remove(index.ok_or("Saved selection not found / 保存された選択が見つかりません / 找不到已保存的选择")?);
                    }
                    "update" => {
                        if reference.is_empty() {
                            return Err("Select objects first / オブジェクトを選択してください / 请先选择对象".into());
                        }
                        next[index.ok_or("Saved selection not found / 保存された選択が見つかりません / 找不到已保存的选择")?].object_ids =
                            reference.iter().map(|(_, o)| o.id.clone()).collect();
                    }
                    _ => unreachable!(),
                }
                validate_saved(&next)?;
                if next.as_slice() == self.saved_vector_selections.as_slice() {
                    return Ok(());
                }
                self.finish();
                let old =
                    std::mem::replace(&mut self.saved_vector_selections, std::sync::Arc::new(next));
                self.vector_undo
                    .push(VectorHistoryEntry::SavedSelections(old));
                self.undo_order.push(HistoryKind::Vector);
                self.vector_redo.clear();
                self.redo_order.clear();
                self.revision += 1;
                return Ok(());
            }
            _ => return Err("Unknown vector-selection action".into()),
        };
        self.apply_vector_selection(ids)
    }
    pub(super) fn apply_vector_selection(&mut self, ids: Vec<String>) -> Result<(), String> {
        if ids.len() > 4096 {
            return Err("Too many selected vector objects".into());
        }
        if ids != self.selected_vector_objects {
            if !self.selected_vector_objects.is_empty() {
                self.previous_vector_selection = self.selected_vector_objects.clone();
            }
            self.selected_vector_objects = ids;
        }
        self.guides.selected.clear();
        self.selection = None;
        self.selection_anchor = None;
        Ok(())
    }
}
const SAME_CRITERIA: &[&str] = &[
    "appearance",
    "graphicStyle",
    "symbolInstance",
    "linkedText",
    "effects",
    "blendMode",
    "fillStroke",
    "fill",
    "opacity",
    "stroke",
    "strokeWidth",
    "shape",
    "shapeText",
    "fontFamily",
    "fontStyle",
    "fontStyleSize",
    "fontSize",
    "textFill",
    "textStroke",
    "textFillStroke",
];
const OBJECT_CRITERIA: &[&str] = &[
    "sameLayers",
    "segments",
    "bristleBrushStrokes",
    "pencilBrushStrokes",
    "brushStrokes",
    "clippingMasks",
    "strayPoints",
    "text",
    "pointText",
    "areaText",
];
fn paint_equal(a: &VectorObject, b: &VectorObject, fill: bool) -> bool {
    if fill {
        a.fill == b.fill && a.fill_gradient == b.fill_gradient
    } else {
        a.stroke == b.stroke && a.stroke_gradient == b.stroke_gradient
    }
}
fn same(k: &str, l: &SvgLayer, a: &VectorObject, rl: &SvgLayer, b: &VectorObject) -> bool {
    match k {
        "fill" => paint_equal(a, b, true),
        "stroke" => paint_equal(a, b, false),
        "fillStroke" => paint_equal(a, b, true) && paint_equal(a, b, false),
        "opacity" => {
            (a.opacity * l.effective_opacity() - b.opacity * rl.effective_opacity()).abs() < 0.00001
        }
        "blendMode" => a.blend_mode == b.blend_mode,
        "strokeWidth" => (a.stroke_width - b.stroke_width).abs() < 0.00001,
        "appearance" => {
            same("fillStroke", l, a, rl, b)
                && same("opacity", l, a, rl, b)
                && same("blendMode", l, a, rl, b)
                && (a.stroke.is_none() && a.stroke_gradient.is_none()
                    || same("strokeWidth", l, a, rl, b) && a.stroke_style == b.stroke_style)
                && (a.text.is_none()
                    || b.text.is_none()
                    || text_keys(a, "textFillStroke") == text_keys(b, "textFillStroke"))
        }
        "shape" => {
            a.text.is_none() && b.text.is_none() && !a.path.data.is_empty() && shape(a) == shape(b)
        }
        "shapeText" => {
            same("shape", l, a, rl, b)
                || a.text.is_some()
                    && b.text.is_some()
                    && a.text.as_ref().map(|t| &t.content) == b.text.as_ref().map(|t| &t.content)
                    && text_keys(a, "fontStyleSize") == text_keys(b, "fontStyleSize")
        }
        "fontFamily" | "fontStyle" | "fontStyleSize" | "fontSize" | "textFill" | "textStroke"
        | "textFillStroke" => {
            a.text.is_some()
                && b.text.is_some()
                && text_keys(a, k) == text_keys(b, k)
                && (!matches!(k, "textStroke" | "textFillStroke") || paint_equal(a, b, false))
        }
        _ => false,
    }
}
// Compare rendered local geometry, rather than stale editing control points.
// Absolute position and object transform do not define a shape's local outline.
fn shape(o: &VectorObject) -> Vec<i64> {
    use svgtypes::SimplePathSegment::*;
    let mut origin = None;
    let mut key = Vec::new();
    for segment in svgtypes::SimplifyingPathParser::from(o.path.data.as_str()) {
        let Ok(segment) = segment else {
            return vec![];
        };
        let (kind, points) = match segment {
            MoveTo { x, y } => {
                origin.get_or_insert([x, y]);
                (0, vec![[x, y]])
            }
            LineTo { x, y } => (1, vec![[x, y]]),
            CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => (2, vec![[x1, y1], [x2, y2], [x, y]]),
            Quadratic { x1, y1, x, y } => (3, vec![[x1, y1], [x, y]]),
            ClosePath => (4, vec![]),
        };
        key.push(kind);
        let [ox, oy] = origin.unwrap_or([0., 0.]);
        for [x, y] in points {
            key.extend([
                ((x - ox) * 1000.).round() as i64,
                ((y - oy) * 1000.).round() as i64,
            ]);
        }
    }
    key
}
fn text_keys(o: &VectorObject, k: &str) -> Vec<String> {
    let Some(t) = &o.text else {
        return vec![];
    };
    let color = o
        .fill
        .map_or([0, 0, 0], |p| [p.color[0], p.color[1], p.color[2]]);
    let covered: usize = t.runs.iter().map(|r| r.end - r.start).sum();
    let mut styles = if covered < t.content.encode_utf16().count() || t.runs.is_empty() {
        vec![t.base_style(color)]
    } else {
        vec![]
    };
    styles.extend(t.runs.iter().map(|r| r.style.clone()));
    let mut keys: Vec<_> = styles
        .iter()
        .map(|s| match k {
            "fontFamily" => s.font_family.clone(),
            "fontStyle" => format!(
                "{}:{}:{}:{:?}",
                s.font_family, s.bold, s.italic, s.font_axes
            ),
            "fontStyleSize" => format!(
                "{}:{}:{}:{}:{:?}",
                s.font_family, s.bold, s.italic, s.font_size, s.font_axes
            ),
            "fontSize" => s.font_size.to_string(),
            "textFill" | "textFillStroke" => {
                if s.no_color {
                    "none".into()
                } else {
                    format!(
                        "{:?}:{:?}:{}",
                        s.color,
                        o.fill_gradient,
                        o.fill.map_or(255, |p| p.color[3])
                    )
                }
            }
            "textStroke" => String::new(),
            _ => String::new(),
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys
}
fn brushed(o: &VectorObject) -> bool {
    o.text.is_none()
        && (o.stroke.is_some() || o.stroke_gradient.is_some())
        && o.stroke_style.brush != crate::stroke::VectorBrush::Basic
}
fn stray(o: &VectorObject) -> bool {
    if o.text.is_some() {
        return false;
    }
    let mut moves = 0;
    for command in svgtypes::PathParser::from(o.path.data.as_str()) {
        match command {
            Ok(svgtypes::PathSegment::MoveTo { .. }) => moves += 1,
            Ok(svgtypes::PathSegment::ClosePath { .. }) if moves > 0 => {}
            _ => return false,
        }
    }
    moves > 0
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(
        action: &str,
        criterion: Option<&str>,
        name: Option<&str>,
    ) -> VectorSelectionRequest {
        VectorSelectionRequest {
            action: action.into(),
            criterion: criterion.map(str::to_owned),
            name: name.map(str::to_owned),
            new_name: None,
            bulk: None,
        }
    }
    fn object(id: &str, color: Option<[u8; 4]>) -> VectorObject {
        VectorObject {
            image_frame: None,
            opacity: 1.,
            blend_mode: "normal".into(),
            id: id.into(),
            name: id.into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: "M10 10H30V40H10Z".into(),
                fill_rule: crate::vector::FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            fill_gradient: None,
            stroke_gradient: None,
            fill: color.map(|color| VectorPaint {
                registration: false,
                color,
            }),
            stroke: None,
            stroke_width: 0.,
            stroke_style: Default::default(),
            live_corners: None,
            rectangle_radii: None,
            visible: true,
            kind: crate::vector::VectorObjectKind::Rectangle,
            control_points: vec![[10., 10.], [30., 40.]],
            text: None,
        }
    }
    fn document() -> Document {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        for (i, c) in [
            Some([255, 0, 0, 255]),
            Some([255, 0, 0, 255]),
            Some([0, 0, 255, 255]),
            None,
        ]
        .into_iter()
        .enumerate()
        {
            d.upsert_vector_object(&id, object(&format!("v{i}"), c))
                .unwrap();
        }
        d.vector_selection_action(request("deselect", None, None))
            .unwrap();
        d
    }
    fn bulk(criteria: &[&str], match_any: bool, scope: &str) -> VectorSelectionRequest {
        let mut q = request("bulk", None, None);
        q.bulk = Some(BulkVectorSelection {
            criteria: criteria.iter().map(|s| s.to_string()).collect(),
            match_any,
            scope: scope.into(),
        });
        q
    }
    #[test]
    fn bulk_conditions_match_one_reference_and_support_any() {
        let mut d = document();
        d.svg_layers[0].vector_objects[1].opacity = 0.5;
        d.svg_layers[0].vector_objects[2].opacity = 0.5;
        d.selected_vector_objects = vec!["v0".into(), "v2".into()];
        let revision = d.revision();
        let before = serde_json::to_value(d.document_state().svg_layers).unwrap();
        d.vector_selection_action(bulk(&["fill", "opacity"], false, "page"))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v2"]);
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(bulk(&["fill", "opacity"], true, "page"))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1", "v3"]);
        assert_eq!(d.revision(), revision);
        assert_eq!(
            serde_json::to_value(d.document_state().svg_layers).unwrap(),
            before
        );
    }
    #[test]
    fn bulk_scopes_and_invalid_queries_preserve_selection() {
        let mut d = document();
        d.svg_layers[0].vector_objects[1].transform[4] = 2000.;
        let layer = d.add_vector_layer().unwrap();
        d.upsert_vector_object(&layer, object("other", Some([255, 0, 0, 255])))
            .unwrap();
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(bulk(&["fill"], false, "sameLayers"))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1"]);
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(bulk(&["fill"], false, "artboard"))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "other"]);
        d.locked_objects.insert("other".into());
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(bulk(&["fill"], false, "page"))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1"]);
        let before = d.selected_vector_ids().to_vec();
        for query in [
            bulk(&[], false, "page"),
            bulk(&["unknown"], false, "page"),
            bulk(&["fill"], false, "invalid"),
        ] {
            assert!(d.vector_selection_action(query).is_err());
            assert_eq!(d.selected_vector_ids(), before);
        }
        d.selected_vector_objects.clear();
        assert!(d
            .vector_selection_action(bulk(&["fill"], false, "page"))
            .is_err());
    }
    #[test]
    fn same_geometry_uses_path_outlines_and_fonts_use_variable_axes() {
        let mut d = document();
        d.svg_layers[0].vector_objects[1].path.data = "m100 100h20v30h-20z".into();
        d.svg_layers[0].vector_objects[2].path.data = "M10 10Q20 0 30 10V40H10Z".into();
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("shape"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1", "v3"]);
        for (i, o) in d.svg_layers[0].vector_objects.iter_mut().enumerate() {
            o.text = Some(VectorText {
                content: "A".into(),
                ..Default::default()
            });
            o.text
                .as_mut()
                .unwrap()
                .font_axes
                .insert("wght".into(), if i == 1 { 700. } else { 400. });
        }
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("fontFamily"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids().len(), 4);
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("fontStyle"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v2", "v3"]);
    }
    #[test]
    fn object_queries_distinguish_brush_kinds_masks_strays_and_text() {
        use crate::stroke::{StrokeStylePatch, VectorBrush};
        let mut d = document();
        d.selected_vector_objects = vec!["v0".into()];
        d.set_selected_stroke_style(StrokeStylePatch {
            brush: Some(VectorBrush::Pencil),
            ..Default::default()
        })
        .unwrap();
        d.selected_vector_objects = vec!["v1".into()];
        d.set_selected_stroke_style(StrokeStylePatch {
            brush: Some(VectorBrush::Bristle),
            ..Default::default()
        })
        .unwrap();
        let revision = d.revision();
        let before = serde_json::to_vec(&d.document_state()).unwrap();
        for (criterion, expected) in [
            ("pencilBrushStrokes", vec!["v0"]),
            ("bristleBrushStrokes", vec!["v1"]),
            ("brushStrokes", vec!["v0", "v1"]),
        ] {
            d.vector_selection_action(request("object", Some(criterion), None))
                .unwrap();
            assert_eq!(d.selected_vector_ids(), expected);
        }
        assert_eq!(d.revision(), revision);
        assert_eq!(serde_json::to_vec(&d.document_state()).unwrap(), before);
        let state =
            serde_json::from_slice(&serde_json::to_vec(&d.document_state()).unwrap()).unwrap();
        let mut loaded = Document::from_document_state(state).unwrap();
        loaded
            .vector_selection_action(request("object", Some("bristleBrushStrokes"), None))
            .unwrap();
        assert_eq!(loaded.selected_vector_ids(), ["v1"]);
        d.undo();
        d.vector_selection_action(request("object", Some("bristleBrushStrokes"), None))
            .unwrap();
        assert!(d.selected_vector_ids().is_empty());
        d.redo();
        d.svg_layers[0].vector_objects[1].visible = false;
        d.vector_selection_action(request("object", Some("brushStrokes"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0"]);
        d.svg_layers[0].vector_objects[2].path.data = "M10 10Z M20 20".into();
        d.vector_selection_action(request("object", Some("strayPoints"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v2"]);
        d.svg_layers[0].vector_objects[3].text = Some(VectorText::default());
        d.vector_selection_action(request("object", Some("text"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v3"]);
    }
    #[test]
    fn segments_select_direction_handles_and_transform_without_moving_anchors() {
        let mut d = document();
        let layer = d.svg_layers[0].id.clone();
        let mut ellipse = object("ellipse", Some([255, 0, 0, 255]));
        ellipse.kind = VectorObjectKind::Ellipse;
        ellipse.path.data = "M10 25 A10 15 0 1 0 30 25 A10 15 0 1 0 10 25Z".into();
        ellipse.control_points = vec![[10., 10.], [30., 40.]];
        d.upsert_vector_object(&layer, ellipse).unwrap();
        d.selected_vector_objects = vec!["ellipse".into()];
        let before = serde_json::to_vec(&d.document_state()).unwrap();
        let revision = d.revision();
        let handles = d.selected_direction_handles().unwrap();
        assert_eq!(handles.len(), 8);
        assert!(handles.iter().all(|(_, i)| !i.is_multiple_of(3)));
        d.vector_selection_action(request("object", Some("segments"), None))
            .unwrap();
        assert_eq!(d.revision(), revision);
        assert_eq!(serde_json::to_vec(&d.document_state()).unwrap(), before);
        d.transform_vector_controls(&handles, "scale", [1., 1., 0., 0.])
            .unwrap();
        assert_eq!(d.revision(), revision);
        let original = d
            .direct_objects()
            .into_iter()
            .find(|(_, o)| o.id == "ellipse")
            .unwrap()
            .1;
        d.transform_vector_controls(&handles, "scale", [1.1, 1.1, 0., 0.])
            .unwrap();
        let edited = d
            .direct_objects()
            .into_iter()
            .find(|(_, o)| o.id == "ellipse")
            .unwrap()
            .1;
        for i in crate::bezier::anchor_indices(&original) {
            assert_eq!(original.control_points[i], edited.control_points[i]);
        }
        assert_ne!(original.path.data, edited.path.data);
        d.undo();
        assert_eq!(serde_json::to_vec(&d.document_state()).unwrap(), before);
        d.locked_objects.insert("ellipse".into());
        assert!(d.selected_direction_handles().unwrap().is_empty());
        assert!(d
            .vector_selection_action(request("object", Some("segments"), None))
            .is_err());
    }
    #[test]
    fn appearance_ignores_unused_stroke_settings() {
        let mut d = document();
        d.svg_layers[0].vector_objects[1].stroke_width = 12.;
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("appearance"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1"]);
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("strokeWidth"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v2", "v3"]);
    }
    #[test]
    fn appearance_includes_layer_effects_and_text_paints() {
        let mut d = document();
        let layer = d.add_vector_layer().unwrap();
        d.upsert_vector_object(&layer, object("effect", Some([255, 0, 0, 255])))
            .unwrap();
        let mut effects = crate::layer_effects::LayerEffects {
            enabled: true,
            ..Default::default()
        };
        effects.values[0] = 1.;
        d.set_layer_effects(&layer, effects).unwrap();
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("appearance"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1"]);
        d.selected_vector_objects = vec!["effect".into()];
        d.vector_selection_action(request("same", Some("effects"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["effect"]);
        for o in &mut d.svg_layers[0].vector_objects {
            o.text = Some(VectorText {
                content: "A".into(),
                ..Default::default()
            });
        }
        let t = d.svg_layers[0].vector_objects[1].text.as_mut().unwrap();
        let mut style = t.base_style([0, 0, 255]);
        style.color = [0, 0, 255];
        t.runs = vec![crate::vector::TextRun {
            start: 0,
            end: 1,
            style,
        }];
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("appearance"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0"]);
    }
    #[test]
    fn queries_preserve_svg_and_revision_and_skip_hidden_locked_objects() {
        let mut d = document();
        d.svg_layers[0].vector_objects[1].visible = false;
        d.locked_objects.insert("v2".into());
        let before = d.document_state();
        let revision = d.revision();
        d.vector_selection_action(request("all", None, None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v3"]);
        d.vector_selection_action(request("invert", None, None))
            .unwrap();
        assert!(d.selected_vector_ids().is_empty());
        d.vector_selection_action(request("reselect", None, None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v3"]);
        assert_eq!(d.revision(), revision);
        assert_eq!(
            serde_json::to_value(d.document_state().svg_layers).unwrap(),
            serde_json::to_value(before.svg_layers).unwrap()
        );
    }
    #[test]
    fn same_fill_selects_exact_members_and_none_is_a_real_attribute() {
        let mut d = document();
        for o in &mut d.svg_layers[0].vector_objects {
            o.group_path = vec!["g".into()];
        }
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("fill"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1"]);
        d.selected_vector_objects = vec!["v3".into()];
        d.vector_selection_action(request("same", Some("fill"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v3"]);
        let selected = d.selected_vector_ids().to_vec();
        assert!(d
            .vector_selection_action(request("same", Some("bad"), None))
            .is_err());
        assert_eq!(d.selected_vector_ids(), selected);
    }
    #[test]
    fn artboard_and_stacking_are_selection_operations() {
        let mut d = document();
        d.svg_layers[0].vector_objects[3].transform[4] = 2000.;
        d.vector_selection_action(request("artboard", None, None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1", "v2"]);
        d.selected_vector_objects = vec!["v1".into()];
        d.vector_selection_action(request("above", None, None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v2"]);
        d.vector_selection_action(request("below", None, None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v1"]);
    }
    #[test]
    fn named_selection_round_trip_undo_update_and_missing_members() {
        let mut d = document();
        d.selected_vector_objects = vec!["v0".into(), "v2".into()];
        d.vector_selection_action(request("save", None, Some("制作対象")))
            .unwrap();
        let rev = d.revision();
        let state = d.document_state();
        let mut decoded = Document::from_document_state(
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap(),
        )
        .unwrap();
        decoded
            .vector_selection_action(request("load", None, Some("制作対象")))
            .unwrap();
        assert_eq!(decoded.selected_vector_ids(), ["v0", "v2"]);
        let copy = d.clone();
        assert!(std::sync::Arc::ptr_eq(
            &d.saved_vector_selections,
            &copy.saved_vector_selections
        ));
        let before = d.saved_vector_selections.clone();
        assert!(d
            .vector_selection_action(request("save", None, Some("制作対象")))
            .is_err());
        assert_eq!(d.revision(), rev);
        assert_eq!(d.saved_vector_selections, before);
        d.undo();
        assert!(d.saved_vector_selections.is_empty());
        d.redo();
        assert_eq!(d.saved_vector_selections, before);
        d.selected_vector_objects = vec!["v1".into()];
        d.vector_selection_action(request("update", None, Some("制作対象")))
            .unwrap();
        assert_eq!(d.saved_vector_selections[0].object_ids, ["v1"]);
        d.undo();
        assert_eq!(d.saved_vector_selections, before);
        d.svg_layers[0].vector_objects.retain(|o| o.id != "v0");
        d.locked_objects.insert("v2".into());
        d.vector_selection_action(request("load", None, Some("制作対象")))
            .unwrap();
        assert!(d.selected_vector_ids().is_empty());
    }
    #[test]
    fn text_queries_use_effective_runs_and_distinguish_point_and_area() {
        let mut d = document();
        for (i, o) in d.svg_layers[0]
            .vector_objects
            .iter_mut()
            .enumerate()
            .take(3)
        {
            let t = VectorText {
                content: "日本語".into(),
                point_text: i < 2,
                font_family: if i == 2 { "serif" } else { "sans-serif" }.into(),
                ..VectorText::default()
            };
            o.kind = crate::vector::VectorObjectKind::Text;
            o.text = Some(t);
        }
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("fontFamily"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v1"]);
        d.vector_selection_action(request("object", Some("areaText"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v2"]);
        let o = &mut d.svg_layers[0].vector_objects[0];
        let t = o.text.as_mut().unwrap();
        let mut style = t.base_style([255, 0, 0]);
        style.font_family = "serif".into();
        t.runs = vec![crate::vector::TextRun {
            start: 0,
            end: 3,
            style,
        }];
        d.selected_vector_objects = vec!["v0".into()];
        d.vector_selection_action(request("same", Some("fontFamily"), None))
            .unwrap();
        assert_eq!(d.selected_vector_ids(), ["v0", "v2"]);
    }
    #[test]
    fn saved_selection_names_and_counts_validate_before_restore() {
        let mut d = document();
        d.selected_vector_objects = vec!["v0".into()];
        assert!(d
            .vector_selection_action(request("save", None, Some(" \n")))
            .is_err());
        let mut state = d.document_state();
        state.saved_vector_selections = vec![
            SavedVectorSelection {
                name: "same".into(),
                object_ids: vec!["v0".into()]
            };
            2
        ];
        assert!(Document::from_document_state(state).is_err());
    }
}
