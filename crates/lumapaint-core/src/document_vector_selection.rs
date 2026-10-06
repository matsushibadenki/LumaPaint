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
            "same" => {
                if !SAME_CRITERIA.contains(&criterion) {
                    return Err("Unknown same-object criterion".into());
                }
                if reference.is_empty() {
                    return Err("Select a reference object / 基準のオブジェクトを選択してください / 请选择参考对象".into());
                }
                c.iter()
                    .filter(|(l, o)| reference.iter().any(|(rl, r)| same(criterion, l, o, rl, r)))
                    .map(|(_, o)| o.id.clone())
                    .collect()
            }
            "object" => {
                if !OBJECT_CRITERIA.contains(&criterion) {
                    return Err("Unknown object criterion".into());
                }
                let layers: std::collections::BTreeSet<_> =
                    reference.iter().map(|(l, _)| l.id.as_str()).collect();
                c.iter()
                    .filter(|(l, o)| match criterion {
                        "sameLayers" => layers.contains(l.id.as_str()),
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
                && same("strokeWidth", l, a, rl, b)
                && a.stroke_style == b.stroke_style
        }
        "shape" => a.text.is_none() && b.text.is_none() && shape(a) == shape(b),
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
fn shape(o: &VectorObject) -> String {
    if o.control_points.is_empty() {
        return format!("{:?}:{}", o.kind, o.path.data);
    }
    let x = o
        .control_points
        .iter()
        .map(|p| p[0])
        .fold(f32::INFINITY, f32::min);
    let y = o
        .control_points
        .iter()
        .map(|p| p[1])
        .fold(f32::INFINITY, f32::min);
    format!(
        "{:?}:{:?}",
        o.kind,
        o.control_points
            .iter()
            .map(|p| [
                ((p[0] - x) * 1000.).round() as i64,
                ((p[1] - y) * 1000.).round() as i64
            ])
            .collect::<Vec<_>>()
    )
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
            "fontStyle" => format!("{}:{}:{}", s.font_family, s.bold, s.italic),
            "fontStyleSize" => format!("{}:{}:{}:{}", s.font_family, s.bold, s.italic, s.font_size),
            "fontSize" => s.font_size.to_string(),
            "textFill" | "textFillStroke" => format!("{}:{:?}", s.no_color, s.color),
            "textStroke" => String::new(),
            _ => String::new(),
        })
        .collect();
    keys.sort();
    keys.dedup();
    keys
}
fn stray(o: &VectorObject) -> bool {
    if o.text.is_some() {
        return false;
    }
    let mut moves = 0;
    for command in svgtypes::PathParser::from(o.path.data.as_str()) {
        match command {
            Ok(svgtypes::PathSegment::MoveTo { .. }) => moves += 1,
            _ => return false,
        }
    }
    moves == 1
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
