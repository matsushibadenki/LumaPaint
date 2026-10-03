//! Numeric transforms stay in the document model; the panel only sends commands.
use super::*;
use std::fmt::Write;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransformPanelInfo {
    pub corners: [[f32; 2]; 4],
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
    pub shear: f32,
    pub rectangle: bool,
    /// Top-left, top-right, bottom-right, bottom-left; document units (pixels).
    pub radii: [f32; 4],
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TransformPanelEdit {
    pub field: String,
    pub values: [f32; 4],
    pub reference: [f32; 2],
    pub proportional: bool,
    pub scale_corners: bool,
    pub scale_strokes: bool,
    pub revision: u64,
    pub ids: Vec<String>,
}

fn basis(corners: [[f32; 2]; 4]) -> [f32; 4] {
    [
        corners[1][0] - corners[0][0],
        corners[1][1] - corners[0][1],
        corners[3][0] - corners[0][0],
        corners[3][1] - corners[0][1],
    ]
}
fn determinant(m: [f32; 4]) -> f32 {
    m[0] * m[3] - m[1] * m[2]
}
fn multiply(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
    ]
}
fn inverse(m: [f32; 4]) -> Result<[f32; 4], String> {
    let det = determinant(m);
    if det.abs() < 1e-8 {
        return Err("The selection has no area / 選択の寸法がゼロです / 所选对象尺寸为零".into());
    }
    Ok([m[3] / det, -m[1] / det, -m[2] / det, m[0] / det])
}
fn object_scale(o: &VectorObject) -> f32 {
    determinant([
        o.transform[0],
        o.transform[1],
        o.transform[2],
        o.transform[3],
    ])
    .abs()
    .sqrt()
    .max(0.00001)
}

fn panel_box(document: &Document) -> Option<[[f32; 2]; 4]> {
    let ids = document.expand_group_selection(&document.selected_vector_objects);
    let selected: Vec<_> = document
        .svg_layers
        .iter()
        .filter(|l| l.vector_layer && l.visible && !l.locked)
        .flat_map(|l| &l.vector_objects)
        .filter(|o| o.visible && ids.contains(&o.id))
        .collect();
    let first = *selected.first()?;
    let [a, b, c, d, e, f] = first.transform;
    let inv = inverse([a, b, c, d]).ok()?;
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for o in selected {
        for p in vector_geometry_extrema(o, |p| Point {
            x: inv[0] * (p.x - e) + inv[2] * (p.y - f),
            y: inv[1] * (p.x - e) + inv[3] * (p.y - f),
        }) {
            bounds[0] = bounds[0].min(p.x);
            bounds[1] = bounds[1].min(p.y);
            bounds[2] = bounds[2].max(p.x);
            bounds[3] = bounds[3].max(p.y);
        }
    }
    if bounds.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let [x, y, r, bottom] = bounds;
    Some(
        [[x, y], [r, y], [r, bottom], [x, bottom]]
            .map(|[x, y]| [a * x + c * y + e, b * x + d * y + f]),
    )
}

impl Document {
    pub fn transform_panel_info(&self) -> Option<TransformPanelInfo> {
        if self.selected_vector_objects.is_empty() {
            return None;
        }
        let ids = self.expand_group_selection(&self.selected_vector_objects);
        if self.svg_layers.iter().any(|layer| {
            layer.vector_objects.iter().any(|object| {
                ids.contains(&object.id)
                    && (layer.locked || !layer.visible || !layer.vector_layer || !object.visible)
            })
        }) {
            return None;
        }

        let corners = panel_box(self)?;
        let m = basis(corners);
        let width = m[0].hypot(m[1]);
        let height = m[2].hypot(m[3]);
        let selected: Vec<_> = self
            .svg_layers
            .iter()
            .flat_map(|l| &l.vector_objects)
            .filter(|o| self.selected_vector_objects.contains(&o.id))
            .collect();
        let rectangle = selected.len() == 1
            && selected[0].kind == VectorObjectKind::Rectangle
            && matches!(selected[0].control_points.len(), 2 | 4);
        let radii = if rectangle {
            selected[0]
                .rectangle_radii
                .unwrap_or([0.; 4])
                .map(|r| r * object_scale(selected[0]))
        } else {
            [0.; 4]
        };
        Some(TransformPanelInfo {
            corners,
            width,
            height,
            rotation: m[1].atan2(m[0]).to_degrees(),
            shear: (m[0] * m[2] + m[1] * m[3])
                .atan2(determinant(m).abs())
                .to_degrees(),
            rectangle,
            radii,
        })
    }

    pub fn edit_transform_panel(&mut self, edit: TransformPanelEdit) -> Result<(), String> {
        if edit.revision != self.revision || edit.ids != self.selected_vector_objects {
            return Err("Selection changed; try again / 選択が変更されました。再操作してください / 选择已更改，请重试".into());
        }
        if edit
            .values
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 100_000.)
            || edit
                .reference
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.).contains(v))
        {
            return Err("Invalid transform values / 変形の数値が無効です / 变换数值无效".into());
        }
        let info = self
            .transform_panel_info()
            .ok_or("Select an object / オブジェクトを選択してください / 请选择对象")?;
        let m = basis(info.corners);
        let [u, v] = edit.reference;
        let anchor = [
            info.corners[0][0] + m[0] * u + m[2] * v,
            info.corners[0][1] + m[1] * u + m[3] * v,
        ];
        let mut linear = [1., 0., 0., 1.];
        let mut offset = [0., 0.];
        match edit.field.as_str() {
            "x" => offset[0] = edit.values[0] - anchor[0],
            "y" => offset[1] = edit.values[0] - anchor[1],
            "width" | "height" => {
                let old = if edit.field == "width" {
                    info.width
                } else {
                    info.height
                };
                if old < 0.0001 || edit.values[0] <= 0. || edit.values[0] > 100_000. {
                    return Err("Invalid dimensions / 寸法が無効です / 尺寸无效".into());
                }
                let ratio = edit.values[0] / old;
                let (sx, sy) = if edit.proportional {
                    (ratio, ratio)
                } else if edit.field == "width" {
                    (ratio, 1.)
                } else {
                    (1., ratio)
                };
                let axes = if info.height < 0.0001 {
                    [
                        m[0] / info.width,
                        m[1] / info.width,
                        -m[1] / info.width,
                        m[0] / info.width,
                    ]
                } else if info.width < 0.0001 {
                    [
                        m[3] / info.height,
                        -m[2] / info.height,
                        m[2] / info.height,
                        m[3] / info.height,
                    ]
                } else {
                    m
                };
                linear = multiply(
                    [axes[0] * sx, axes[1] * sx, axes[2] * sy, axes[3] * sy],
                    inverse(axes)?,
                );
            }
            "rotation" => {
                let (sn, cs) = (edit.values[0] - info.rotation).to_radians().sin_cos();
                linear = [cs, sn, -sn, cs];
            }
            "shear" => {
                if edit.values[0].abs() >= 89. {
                    return Err("Shear must be between −89° and 89° / シアーは−89°〜89°未満です / 倾斜角必须介于−89°和89°之间".into());
                }
                let rotation = info.rotation.to_radians();
                let theta = edit.values[0].to_radians();
                let sign = determinant(m).signum();
                let vx = info.height * theta.sin();
                let vy = info.height * theta.cos() * sign;
                let desired = [
                    m[0],
                    m[1],
                    rotation.cos() * vx - rotation.sin() * vy,
                    rotation.sin() * vx + rotation.cos() * vy,
                ];
                linear = multiply(desired, inverse(m)?);
            }
            "corners" if info.rectangle => {
                if edit.values.iter().any(|v| !(0.0..=4096.).contains(v)) {
                    return Err("Invalid corner radius / 角半径が無効です / 圆角半径无效".into());
                }
            }
            _ => {
                return Err("Unknown transform property / 変形項目が不明です / 未知变换属性".into())
            }
        }
        if edit.field != "corners"
            && linear
                .iter()
                .zip([1., 0., 0., 1.])
                .all(|(a, b)| (*a - b).abs() < 1e-6)
            && offset.iter().all(|v| v.abs() < 1e-6)
        {
            return Ok(());
        }
        let scale = determinant(linear).abs().sqrt();
        if !scale.is_finite() || scale < 0.00001 {
            return Err("Invalid transform scale / 変形の倍率が無効です / 变换比例无效".into());
        }
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            let mut changed = false;
            for o in &mut layer.vector_objects {
                if !selected.contains(&o.id) {
                    continue;
                }
                if layer.locked || !layer.visible || !o.visible || !layer.vector_layer {
                    return Err("Selected object is locked or hidden / 選択オブジェクトがロックまたは非表示です / 所选对象已锁定或隐藏".into());
                }
                let [a, b, c, d, e, f] = o.transform;
                o.transform = [
                    linear[0] * a + linear[2] * b,
                    linear[1] * a + linear[3] * b,
                    linear[0] * c + linear[2] * d,
                    linear[1] * c + linear[3] * d,
                    anchor[0]
                        + linear[0] * (e - anchor[0])
                        + linear[2] * (f - anchor[1])
                        + offset[0],
                    anchor[1]
                        + linear[1] * (e - anchor[0])
                        + linear[3] * (f - anchor[1])
                        + offset[1],
                ];
                if !edit.scale_strokes && (scale - 1.).abs() > 1e-6 {
                    o.stroke_width /= scale;
                    for dash in &mut o.stroke_style.dash_array {
                        *dash /= scale;
                    }
                    o.stroke_style.dash_offset /= scale;
                }
                if edit.field == "corners" {
                    o.rectangle_radii = Some(edit.values.map(|r| r / object_scale(o)));
                    rebuild_rectangle(o)?;
                } else if !edit.scale_corners && (scale - 1.).abs() > 1e-6 {
                    if let Some(radii) = &mut o.rectangle_radii {
                        for r in radii {
                            *r /= scale;
                        }
                        rebuild_rectangle(o)?;
                    }
                    if let Some(corners) = &mut o.live_corners {
                        let radius = corners.radius;
                        let anchors = corners.anchors.clone();
                        *o = crate::bezier::round_corners(o, &anchors, radius)?;
                    }
                }
                o.bounds_reset = false;
                o.validate()?;
                changed = true;
            }
            if changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data / ベクターデータが上限を超えています / 矢量数据超过上限".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
}

/// Build round corners in world coordinates, then return geometry to object space.
pub(super) fn rebuild_rectangle(o: &mut VectorObject) -> Result<(), String> {
    if o.kind != VectorObjectKind::Rectangle || !matches!(o.control_points.len(), 2 | 4) {
        return Err("Select a rectangle / 長方形を選択してください / 请选择矩形".into());
    }
    let radii = o
        .rectangle_radii
        .unwrap_or([0.; 4])
        .map(|r| r * object_scale(o));
    let controls = if o.control_points.len() == 2 {
        let p = o.control_points[0];
        let q = o.control_points[1];
        let x = p[0].min(q[0]);
        let y = p[1].min(q[1]);
        let r = p[0].max(q[0]);
        let b = p[1].max(q[1]);
        vec![[x, y], [r, y], [r, b], [x, b]]
    } else {
        o.control_points.clone()
    };
    let world: Vec<_> = controls
        .iter()
        .map(|p| crate::bezier::world_point(o, *p))
        .collect();
    let inv = inverse([
        o.transform[0],
        o.transform[1],
        o.transform[2],
        o.transform[3],
    ])?;
    let local = |p: [f32; 2]| {
        let x = p[0] - o.transform[4];
        let y = p[1] - o.transform[5];
        [inv[0] * x + inv[2] * y, inv[1] * x + inv[3] * y]
    };
    let mut effective_radii = [0.; 4];
    let mut parts = Vec::new();
    for i in 0..4 {
        let p = world[i];
        let prev = world[(i + 3) % 4];
        let next = world[(i + 1) % 4];
        let l1 = (prev[0] - p[0]).hypot(prev[1] - p[1]);
        let l2 = (next[0] - p[0]).hypot(next[1] - p[1]);
        if l1 < 0.0001 || l2 < 0.0001 {
            return Err("Rectangle has no area / 長方形の寸法がゼロです / 矩形尺寸为零".into());
        }
        let u = [(prev[0] - p[0]) / l1, (prev[1] - p[1]) / l1];
        let v = [(next[0] - p[0]) / l2, (next[1] - p[1]) / l2];
        let angle = (u[0] * v[0] + u[1] * v[1]).clamp(-1., 1.).acos();
        let tangent = (angle * 0.5).tan();
        let distance = (radii[i] / tangent).min(l1 * 0.5).min(l2 * 0.5);
        let radius = distance * tangent;
        effective_radii[i] = radius / object_scale(o);
        let h = 4. / 3. * ((std::f32::consts::PI - angle) * 0.25).tan() * radius;
        let incoming = [p[0] + u[0] * distance, p[1] + u[1] * distance];
        let outgoing = [p[0] + v[0] * distance, p[1] + v[1] * distance];
        parts.push([
            local(incoming),
            local([incoming[0] - u[0] * h, incoming[1] - u[1] * h]),
            local([outgoing[0] - v[0] * h, outgoing[1] - v[1] * h]),
            local(outgoing),
        ]);
    }
    let mut data = String::new();
    for (i, p) in parts.iter().enumerate() {
        let _ = write!(
            data,
            "{} {} {} C {} {} {} {} {} {} ",
            if i == 0 { "M" } else { "L" },
            p[0][0],
            p[0][1],
            p[1][0],
            p[1][1],
            p[2][0],
            p[2][1],
            p[3][0],
            p[3][1]
        );
    }
    data.push('Z');
    o.rectangle_radii = Some(effective_radii);
    o.path.data = data;
    o.live_corners = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle() -> Document {
        let mut d = Document::default();
        let layer = d.add_vector_layer().unwrap();
        d.select_layer(layer.clone()).unwrap();
        let o = VectorObject {
            id: "transform-rect".into(),
            name: "Rectangle".into(),
            path: VectorPath {
                data: "M 10 20 H 110 V 80 H 10 Z".into(),
                fill_rule: crate::vector::FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                color: [20, 30, 40, 255],
            }),
            stroke: Some(VectorPaint {
                color: [0, 0, 0, 255],
            }),
            stroke_width: 4.,
            stroke_style: Default::default(),
            live_corners: None,
            rectangle_radii: None,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[10., 20.], [110., 80.]],
            text: None,
            opacity: 1.,
            blend_mode: "normal".into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
        };
        d.upsert_vector_object(&layer, o).unwrap();
        d.select_vector_objects(vec!["transform-rect".into()])
            .unwrap();
        d
    }
    fn edit(
        d: &mut Document,
        field: &str,
        values: [f32; 4],
        reference: [f32; 2],
        proportional: bool,
        scale_corners: bool,
        scale_strokes: bool,
    ) -> Result<(), String> {
        d.edit_transform_panel(TransformPanelEdit {
            field: field.into(),
            values,
            reference,
            proportional,
            scale_corners,
            scale_strokes,
            revision: d.revision,
            ids: d.selected_vector_objects.clone(),
        })
    }
    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 0.002, "{a} != {b}");
    }
    #[test]
    fn numeric_dimensions_anchor_rotation_and_shear_are_absolute() {
        let mut d = rectangle();
        let info = d.transform_panel_info().unwrap();
        close(info.width, 100.);
        close(info.height, 60.);
        assert!(info.rectangle);
        edit(
            &mut d,
            "width",
            [200., 0., 0., 0.],
            [1., 1.],
            false,
            true,
            true,
        )
        .unwrap();
        let info = d.transform_panel_info().unwrap();
        close(info.width, 200.);
        close(info.corners[2][0], 110.);
        close(info.corners[2][1], 80.);
        edit(
            &mut d,
            "rotation",
            [30., 0., 0., 0.],
            [1., 1.],
            false,
            true,
            true,
        )
        .unwrap();
        let info = d.transform_panel_info().unwrap();
        close(info.rotation, 30.);
        close(info.corners[2][0], 110.);
        close(info.corners[2][1], 80.);
        edit(
            &mut d,
            "shear",
            [20., 0., 0., 0.],
            [1., 1.],
            false,
            true,
            true,
        )
        .unwrap();
        let info = d.transform_panel_info().unwrap();
        close(info.shear, 20.);
        close(info.width, 200.);
        close(info.height, 60.);
        close(info.corners[2][0], 110.);
        close(info.corners[2][1], 80.);
        edit(&mut d, "x", [50., 0., 0., 0.], [1., 1.], false, true, true).unwrap();
        close(d.transform_panel_info().unwrap().corners[2][0], 50.);
    }
    #[test]
    fn corners_strokes_and_proportions_survive_editing_undo_and_serialization() {
        let mut d = rectangle();
        edit(
            &mut d,
            "corners",
            [4., 8., 12., 16.],
            [0., 0.],
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(d.transform_panel_info().unwrap().radii, [4., 8., 12., 16.]);
        let object = d
            .svg_layers
            .iter()
            .flat_map(|l| &l.vector_objects)
            .find(|o| o.id == "transform-rect")
            .unwrap();
        let serialized = serde_json::to_string(object).unwrap();
        let restored: VectorObject = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.rectangle_radii, object.rectangle_radii);
        assert!(restored.path.data.contains("C"));
        assert!(crate::bezier::editable(&restored)
            .unwrap()
            .path
            .data
            .contains("C"));
        edit(
            &mut d,
            "width",
            [200., 0., 0., 0.],
            [0., 0.],
            true,
            false,
            false,
        )
        .unwrap();
        let info = d.transform_panel_info().unwrap();
        close(info.width, 200.);
        close(info.height, 120.);
        for (a, b) in info.radii.into_iter().zip([4., 8., 12., 16.]) {
            close(a, b);
        }
        let object = d
            .svg_layers
            .iter()
            .flat_map(|l| &l.vector_objects)
            .find(|o| o.id == "transform-rect")
            .unwrap();
        close(object.stroke_width, 2.);
        d.undo();
        close(d.transform_panel_info().unwrap().width, 100.);
        d.redo();
        close(d.transform_panel_info().unwrap().width, 200.);
        edit(
            &mut d,
            "width",
            [400., 0., 0., 0.],
            [0., 0.],
            true,
            true,
            true,
        )
        .unwrap();
        for (a, b) in d
            .transform_panel_info()
            .unwrap()
            .radii
            .into_iter()
            .zip([8., 16., 24., 32.])
        {
            close(a, b);
        }
    }
    #[test]
    fn invalid_or_stale_edits_are_atomic() {
        let mut d = rectangle();
        let revision = d.revision;
        let source = d.svg_layers[0].source.clone();
        assert!(edit(&mut d, "width", [0.; 4], [0., 0.], false, true, true).is_err());
        assert!(edit(
            &mut d,
            "shear",
            [90., 0., 0., 0.],
            [0., 0.],
            false,
            true,
            true
        )
        .is_err());
        assert!(edit(
            &mut d,
            "corners",
            [-1., 0., 0., 0.],
            [0., 0.],
            false,
            true,
            true
        )
        .is_err());
        assert!(d
            .edit_transform_panel(TransformPanelEdit {
                field: "x".into(),
                values: [100., 0., 0., 0.],
                reference: [0., 0.],
                proportional: false,
                scale_corners: true,
                scale_strokes: true,
                revision: revision + 1,
                ids: d.selected_vector_objects.clone()
            })
            .is_err());
        assert_eq!(d.revision, revision);
        assert_eq!(d.svg_layers[0].source, source);
    }
}
