//! Geometric alignment, stable grouped units and atomic translation history.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum AlignmentTarget {
    #[default]
    Selection,
    KeyObject,
    Artboard,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AlignAction {
    Left,
    HorizontalCenter,
    Right,
    Center,
    Top,
    VerticalCenter,
    Bottom,
    DistributeTop,
    DistributeVerticalCenter,
    DistributeBottom,
    DistributeLeft,
    DistributeHorizontalCenter,
    DistributeRight,
    SpaceVertical,
    SpaceHorizontal,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlignmentSettings {
    pub target: AlignmentTarget,
    pub key_object: Option<String>,
    pub spacing: f32,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlignmentRequest {
    pub action: Option<AlignAction>,
    pub target: Option<AlignmentTarget>,
    pub key_object: Option<String>,
    pub spacing: Option<f32>,
}
struct Unit {
    ids: Vec<String>,
    bounds: [f32; 4],
}
fn error() -> String {
    "Check selection, key object or spacing / 選択・キーオブジェクト・間隔を確認してください / 请检查选择、关键对象或间距".into()
}
fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}
fn bounds(o: &VectorObject) -> Result<[f32; 4], String> {
    let mut points = vector_geometry_extrema(o, |p| p);
    if o.kind == VectorObjectKind::Ellipse && !o.control_points.is_empty() {
        let mut b = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for p in &o.control_points {
            b = union(b, [p[0], p[1], p[0], p[1]]);
        }
        let [a, c, d, e, x, y] = o.transform;
        let rx = (b[2] - b[0]) / 2.;
        let ry = (b[3] - b[1]) / 2.;
        let cx = (b[0] + b[2]) / 2.;
        let cy = (b[1] + b[3]) / 2.;
        let mx = a * cx + d * cy + x;
        let my = c * cx + e * cy + y;
        let ex = (a * rx).hypot(d * ry);
        let ey = (c * rx).hypot(e * ry);
        return Ok([mx - ex, my - ey, mx + ex, my + ey]);
    }
    if points.is_empty() {
        points = crate::stroke::path_edges(&o.path.data, o.transform)
            .into_iter()
            .flat_map(|edge| edge.map(|p| Point { x: p[0], y: p[1] }))
            .collect();
    }
    let mut b = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for p in points {
        b = union(b, [p.x, p.y, p.x, p.y]);
    }
    if b.iter().any(|v| !v.is_finite()) {
        return Err(error());
    }
    Ok(b)
}
impl Document {
    pub fn alignment_settings(&self) -> AlignmentSettings {
        let mut state = self.alignment.clone();
        if !state
            .key_object
            .as_ref()
            .is_some_and(|id| self.selected_vector_objects.contains(id))
        {
            state.key_object = None;
        }
        state
    }
    pub fn edit_alignment(&mut self, request: AlignmentRequest) -> Result<bool, String> {
        let mut settings = self.alignment_settings();
        if let Some(target) = request.target {
            settings.target = target;
            if target == AlignmentTarget::KeyObject && settings.key_object.is_none() {
                settings.key_object = self.selected_vector_objects.first().cloned();
            }
        }
        if let Some(id) = request.key_object {
            if !self.selected_vector_objects.contains(&id) {
                return Err(error());
            }
            settings.key_object = Some(id);
            settings.target = AlignmentTarget::KeyObject;
        }
        if let Some(spacing) = request.spacing {
            if !spacing.is_finite() || spacing.abs() > 65536. {
                return Err(error());
            }
            settings.spacing = spacing;
        }
        let Some(action) = request.action else {
            self.alignment = settings;
            return Ok(false);
        };
        if self.path_editing.is_some() {
            return Err("Leave saved-path editing before alignment / パス編集を終了してください / 请先结束路径编辑".into());
        }
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        if selected.is_empty() || selected.len() > 2048 {
            return Err(error());
        }
        let selected: BTreeSet<_> = selected.into_iter().collect();
        let mut groups: BTreeMap<(usize, Vec<String>), (usize, usize)> = BTreeMap::new();
        for (li, layer) in self.svg_layers.iter().enumerate() {
            for o in &layer.vector_objects {
                for depth in 1..=o.group_path.len() {
                    let count = groups
                        .entry((li, o.group_path[..depth].to_vec()))
                        .or_default();
                    count.0 += 1;
                    count.1 += usize::from(selected.contains(&o.id));
                }
            }
        }
        let mut units: Vec<Unit> = vec![];
        let mut indices: BTreeMap<(usize, Vec<String>), usize> = BTreeMap::new();
        let mut found = 0;
        for (li, layer) in self.svg_layers.iter().enumerate() {
            for o in &layer.vector_objects {
                if !selected.contains(&o.id) {
                    continue;
                }
                found += 1;
                if !layer.visible
                    || layer.locked
                    || !layer.vector_layer
                    || !o.visible
                    || self.object_is_locked(&o.id)
                {
                    return Err(error());
                }
                o.validate()?;
                let b = bounds(o)?;
                let group = (1..=o.group_path.len()).find_map(|depth| {
                    let key = (li, o.group_path[..depth].to_vec());
                    let count = groups[&key];
                    (count.0 == count.1).then_some(key)
                });
                if let Some(group) = group {
                    if let Some(&index) = indices.get(&group) {
                        units[index].bounds = union(units[index].bounds, b);
                        units[index].ids.push(o.id.clone());
                    } else {
                        indices.insert(group, units.len());
                        units.push(Unit {
                            ids: vec![o.id.clone()],
                            bounds: b,
                        });
                    }
                } else {
                    units.push(Unit {
                        ids: vec![o.id.clone()],
                        bounds: b,
                    });
                }
            }
        }
        if found != selected.len() || units.is_empty() {
            return Err(error());
        }
        let key = settings
            .key_object
            .as_ref()
            .and_then(|id| units.iter().position(|u| u.ids.contains(id)));
        let selection = units
            .iter()
            .skip(1)
            .fold(units[0].bounds, |b, u| union(b, u.bounds));
        let reference = match settings.target {
            AlignmentTarget::Selection => selection,
            AlignmentTarget::Artboard => [0., 0., self.width as f32, self.height as f32],
            AlignmentTarget::KeyObject => units[key.ok_or_else(error)?].bounds,
        };
        let mut deltas = vec![[0., 0.]; units.len()];
        let feature = |b: [f32; 4], axis: usize, anchor: usize| match anchor {
            0 => b[axis],
            1 => (b[axis] + b[axis + 2]) / 2.,
            _ => b[axis + 2],
        };
        let alignment = match action {
            AlignAction::Left => Some((0, 0)),
            AlignAction::HorizontalCenter => Some((0, 1)),
            AlignAction::Right => Some((0, 2)),
            AlignAction::Top => Some((1, 0)),
            AlignAction::VerticalCenter => Some((1, 1)),
            AlignAction::Bottom => Some((1, 2)),
            _ => None,
        };
        if let Some((axis, anchor)) = alignment {
            for (i, u) in units.iter().enumerate() {
                deltas[i][axis] =
                    feature(reference, axis, anchor) - feature(u.bounds, axis, anchor);
            }
        } else if matches!(action, AlignAction::Center) {
            for (i, u) in units.iter().enumerate() {
                for (axis, delta) in deltas[i].iter_mut().enumerate() {
                    *delta = feature(reference, axis, 1) - feature(u.bounds, axis, 1);
                }
            }
        } else {
            let (axis, anchor, spacing) = match action {
                AlignAction::DistributeTop => (1, 0, false),
                AlignAction::DistributeVerticalCenter => (1, 1, false),
                AlignAction::DistributeBottom => (1, 2, false),
                AlignAction::DistributeLeft => (0, 0, false),
                AlignAction::DistributeHorizontalCenter => (0, 1, false),
                AlignAction::DistributeRight => (0, 2, false),
                AlignAction::SpaceVertical => (1, 0, true),
                AlignAction::SpaceHorizontal => (0, 0, true),
                _ => unreachable!(),
            };
            let minimum = if spacing || settings.target == AlignmentTarget::Artboard {
                2
            } else {
                3
            };
            if units.len() < minimum {
                return Err(error());
            }
            let mut order: Vec<_> = (0..units.len()).collect();
            order.sort_by(|a, b| {
                feature(units[*a].bounds, axis, anchor)
                    .total_cmp(&feature(units[*b].bounds, axis, anchor))
                    .then(a.cmp(b))
            });
            if spacing && settings.target == AlignmentTarget::KeyObject {
                let key = key.unwrap();
                let position = order.iter().position(|i| *i == key).unwrap();
                let mut cursor = units[key].bounds[axis];
                for &i in order[..position].iter().rev() {
                    cursor -= settings.spacing + units[i].bounds[axis + 2] - units[i].bounds[axis];
                    deltas[i][axis] = cursor - units[i].bounds[axis];
                }
                cursor = units[key].bounds[axis + 2];
                for &i in &order[position + 1..] {
                    cursor += settings.spacing;
                    deltas[i][axis] = cursor - units[i].bounds[axis];
                    cursor += units[i].bounds[axis + 2] - units[i].bounds[axis];
                }
            } else {
                let first = order[0];
                let last = *order.last().unwrap();
                let (start, end) = if settings.target == AlignmentTarget::Artboard {
                    let start = feature(reference, axis, 0)
                        + if anchor == 1 {
                            (units[first].bounds[axis + 2] - units[first].bounds[axis]) / 2.
                        } else if anchor == 2 {
                            units[first].bounds[axis + 2] - units[first].bounds[axis]
                        } else {
                            0.
                        };
                    let end = reference[axis + 2]
                        - if anchor == 1 {
                            (units[last].bounds[axis + 2] - units[last].bounds[axis]) / 2.
                        } else if anchor == 0 {
                            units[last].bounds[axis + 2] - units[last].bounds[axis]
                        } else {
                            0.
                        };
                    (start, end)
                } else {
                    (
                        feature(units[first].bounds, axis, anchor),
                        feature(units[last].bounds, axis, anchor),
                    )
                };
                if spacing {
                    let gap = (reference[axis + 2]
                        - reference[axis]
                        - units
                            .iter()
                            .map(|u| u.bounds[axis + 2] - u.bounds[axis])
                            .sum::<f32>())
                        / (units.len() - 1) as f32;
                    let mut cursor = reference[axis];
                    for &i in &order {
                        deltas[i][axis] = cursor - units[i].bounds[axis];
                        cursor += units[i].bounds[axis + 2] - units[i].bounds[axis] + gap;
                    }
                } else {
                    let step = (end - start) / (units.len() - 1) as f32;
                    for (pos, &i) in order.iter().enumerate() {
                        deltas[i][axis] =
                            start + step * pos as f32 - feature(units[i].bounds, axis, anchor);
                    }
                    if settings.target == AlignmentTarget::KeyObject {
                        let correction = deltas[key.unwrap()][axis];
                        for delta in &mut deltas {
                            delta[axis] -= correction;
                        }
                    }
                }
            }
        }
        if deltas
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() >= 100000.)
        {
            return Err(error());
        }
        let offsets = units
            .into_iter()
            .zip(deltas)
            .flat_map(|(unit, delta)| unit.ids.into_iter().map(move |id| (id, delta)))
            .collect();
        let changed = self.move_vectors_with_offsets(0., 0., true, Some(&offsets))?;
        self.alignment = settings;
        Ok(changed)
    }
    pub(super) fn alignment_mask_translation(
        &mut self,
        offsets: &BTreeMap<String, [f32; 2]>,
    ) -> Result<bool, String> {
        if offsets
            .values()
            .all(|d| d[0].abs() < f32::EPSILON && d[1].abs() < f32::EPSILON)
        {
            return Ok(false);
        }
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            let mut changed = false;
            for o in &mut layer.vector_objects {
                if let Some(delta) = offsets.get(&o.id) {
                    o.transform[4] += delta[0];
                    o.transform[5] += delta[1];
                    o.validate()?;
                    changed = true;
                }
            }
            if changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err(error());
        }
        let masks = self.linked_mask_updates(&layers)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.layer_effects.extend(masks);
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(id: &str, x: f32, y: f32, w: f32, h: f32) -> VectorObject {
        VectorObject {
            id: id.into(),
            name: id.into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            path: crate::vector::VectorPath {
                data: format!("M0 0H{w}V{h}H0Z"),
                fill_rule: crate::vector::FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., x, y],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(crate::vector::VectorPaint {
                registration: false,
                color: [40, 120, 200, 255],
            }),
            stroke: None,
            stroke_style: Default::default(),
            stroke_width: 0.,
            live_corners: None,
            rectangle_radii: None,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[0., 0.], [w, h]],
            text: None,
            opacity: 1.,
            blend_mode: "normal".into(),
        }
    }
    fn doc() -> Document {
        let mut d = Document::default();
        d.add_vector_layer().unwrap();
        let mut state = d.document_state();
        state.svg_layers[0].vector_objects = vec![
            rect("a", 10., 20., 20., 10.),
            rect("b", 70., 50., 40., 30.),
            rect("c", 150., 80., 10., 20.),
        ];
        state.svg_layers[0].source = vector_svg(
            state.width,
            state.height,
            &state.svg_layers[0].vector_objects,
        );
        let mut d = Document::from_document_state(state).unwrap();
        d.select_vector_objects(vec!["a".into(), "b".into(), "c".into()])
            .unwrap();
        d
    }
    fn positions(d: &Document) -> Vec<[f32; 2]> {
        d.svg_layers[0]
            .vector_objects
            .iter()
            .map(|o| [o.transform[4], o.transform[5]])
            .collect()
    }
    fn run(d: &mut Document, action: AlignAction) {
        d.edit_alignment(AlignmentRequest {
            action: Some(action),
            ..Default::default()
        })
        .unwrap();
    }
    #[test]
    fn alignments_are_atomic_and_noops_have_no_history() {
        let actions = [
            (AlignAction::Left, 0, vec![10., 10., 10.]),
            (AlignAction::HorizontalCenter, 0, vec![75., 65., 80.]),
            (AlignAction::Right, 0, vec![140., 120., 150.]),
            (AlignAction::Top, 1, vec![20., 20., 20.]),
            (AlignAction::VerticalCenter, 1, vec![55., 45., 50.]),
            (AlignAction::Bottom, 1, vec![90., 70., 80.]),
        ];
        for (action, axis, expected) in actions {
            let mut d = doc();
            let before = positions(&d);
            run(&mut d, action);
            assert_eq!(
                positions(&d).iter().map(|p| p[axis]).collect::<Vec<_>>(),
                expected
            );
            let revision = d.revision();
            run(&mut d, action);
            assert_eq!(d.revision(), revision);
            d.undo();
            assert_eq!(positions(&d), before);
            d.redo();
            assert_eq!(
                positions(&d).iter().map(|p| p[axis]).collect::<Vec<_>>(),
                expected
            );
        }
    }
    #[test]
    fn fixed_key_spacing_and_board_center() {
        let mut d = doc();
        let revision = d.revision();
        d.edit_alignment(AlignmentRequest {
            target: Some(AlignmentTarget::KeyObject),
            key_object: Some("b".into()),
            spacing: Some(12.),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(d.revision(), revision);
        run(&mut d, AlignAction::SpaceHorizontal);
        assert_eq!(positions(&d), vec![[38., 20.], [70., 50.], [122., 80.]]);
        d.undo();
        assert_eq!(positions(&d)[1], [70., 50.]);
        d.edit_alignment(AlignmentRequest {
            target: Some(AlignmentTarget::Artboard),
            ..Default::default()
        })
        .unwrap();
        d.select_vector_objects(vec!["a".into()]).unwrap();
        run(&mut d, AlignAction::Center);
        assert_eq!(
            positions(&d)[0],
            [d.width as f32 / 2. - 10., d.height as f32 / 2. - 5.]
        );
    }
    #[test]
    fn distribution_and_groups_keep_relative_geometry() {
        let mut d = doc();
        run(&mut d, AlignAction::DistributeHorizontalCenter);
        assert_eq!(positions(&d)[1][0], 67.5);
        d.undo();
        run(&mut d, AlignAction::DistributeVerticalCenter);
        assert_eq!(positions(&d)[1][1], 42.5);
        d.undo();
        d.svg_layers[0].vector_objects[0].group_path = vec!["group".into()];
        d.svg_layers[0].vector_objects[1].group_path = vec!["group".into()];
        run(&mut d, AlignAction::Top);
        assert_eq!(positions(&d), vec![[10., 20.], [70., 50.], [150., 20.]]);
    }
    #[test]
    fn invalid_key_locked_selection_and_spacing_are_rejected() {
        let mut d = doc();
        let before = positions(&d);
        assert!(d
            .edit_alignment(AlignmentRequest {
                key_object: Some("missing".into()),
                action: Some(AlignAction::Left),
                ..Default::default()
            })
            .is_err());
        assert!(d
            .edit_alignment(AlignmentRequest {
                spacing: Some(f32::NAN),
                ..Default::default()
            })
            .is_err());
        d.svg_layers[0].locked = true;
        assert!(d
            .edit_alignment(AlignmentRequest {
                action: Some(AlignAction::Left),
                ..Default::default()
            })
            .is_err());
        assert_eq!(positions(&d), before);
    }
}
