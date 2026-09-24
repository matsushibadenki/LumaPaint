//! Cubic paths store an initial anchor followed by control/control/anchor triples.
use std::fmt::Write;

pub fn path_data(points: &[[f32; 2]], closed: bool) -> Result<String, String> {
    if points.is_empty() || !(points.len() - 1).is_multiple_of(3) {
        return Err("Invalid cubic control points".into());
    }
    let mut path = format!("M {} {}", points[0][0], points[0][1]);
    for segment in points[1..].as_chunks::<3>().0 {
        let _ = write!(
            path,
            " C {} {} {} {} {} {}",
            segment[0][0],
            segment[0][1],
            segment[1][0],
            segment[1][1],
            segment[2][0],
            segment[2][1]
        );
    }
    if closed {
        path.push_str(" Z");
    }
    Ok(path)
}

pub fn flattened(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let Some(&first) = points.first() else {
        return vec![];
    };
    let mut result = vec![first];
    let mut start = first;
    for segment in points[1..].as_chunks::<3>().0 {
        // Bound sampling by the control polygon length, including tight curves.
        let length: f32 = [start, segment[0], segment[1], segment[2]]
            .windows(2)
            .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
            .sum();
        let steps = (length / 2.0).ceil().clamp(8.0, 4096.0) as usize;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            let u = 1.0 - t;
            result.push(std::array::from_fn(|axis| {
                u * u * u * start[axis]
                    + 3.0 * u * u * t * segment[0][axis]
                    + 3.0 * u * t * t * segment[1][axis]
                    + t * t * t * segment[2][axis]
            }));
        }
        start = segment[2];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cubic_serialization_and_sampling_preserve_curve() {
        let points = [[0., 0.], [0., 100.], [100., 100.], [100., 0.]];
        assert_eq!(
            path_data(&points, true).unwrap(),
            "M 0 0 C 0 100 100 100 100 0 Z"
        );
        let samples = flattened(&points);
        assert!(samples
            .iter()
            .any(|p| (p[0] - 50.).abs() < 1. && (p[1] - 75.).abs() < 1.));
        assert_eq!(samples.last(), Some(&[100., 0.]));
        assert!(path_data(&points[..3], false).is_err());
    }
}

use crate::vector::{VectorObject, VectorObjectKind};

fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t)
}

pub fn evaluate(segment: &[[f32; 2]; 4], t: f32) -> [f32; 2] {
    lerp(
        lerp(
            lerp(segment[0], segment[1], t),
            lerp(segment[1], segment[2], t),
            t,
        ),
        lerp(
            lerp(segment[1], segment[2], t),
            lerp(segment[2], segment[3], t),
            t,
        ),
        t,
    )
}

pub fn world_point(object: &VectorObject, p: [f32; 2]) -> [f32; 2] {
    let [a, b, c, d, e, f] = object.transform;
    [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
}

pub fn local_point(object: &VectorObject, p: [f32; 2]) -> Result<[f32; 2], String> {
    let [a, b, c, d, e, f] = object.transform;
    let determinant = a * d - b * c;
    if determinant.abs() < 0.000001 {
        return Err("Vector transform is not invertible".into());
    }
    Ok([
        (d * (p[0] - e) - c * (p[1] - f)) / determinant,
        (-b * (p[0] - e) + a * (p[1] - f)) / determinant,
    ])
}

/// Convert pencil polylines to equivalent cubic segments before anchor editing.
pub fn editable(object: &VectorObject) -> Option<VectorObject> {
    let mut result = object.clone();
    match object.kind {
        VectorObjectKind::Rectangle | VectorObjectKind::Ellipse
            if object.control_points.len() == 2 =>
        {
            let [x1, y1] = object.control_points[0];
            let [x2, y2] = object.control_points[1];
            let (left, right, top, bottom) = (x1.min(x2), x1.max(x2), y1.min(y2), y1.max(y2));
            result.control_points = if object.kind == VectorObjectKind::Rectangle {
                vec![
                    [left, top],
                    [left, top],
                    [right, top],
                    [right, top],
                    [right, top],
                    [right, bottom],
                    [right, bottom],
                    [right, bottom],
                    [left, bottom],
                    [left, bottom],
                    [left, bottom],
                    [left, top],
                    [left, top],
                ]
            } else {
                let (cx, cy, rx, ry) = (
                    (left + right) / 2.,
                    (top + bottom) / 2.,
                    (right - left) / 2.,
                    (bottom - top) / 2.,
                );
                let k = 0.552_284_8;
                vec![
                    [right, cy],
                    [right, cy + k * ry],
                    [cx + k * rx, bottom],
                    [cx, bottom],
                    [cx - k * rx, bottom],
                    [left, cy + k * ry],
                    [left, cy],
                    [left, cy - k * ry],
                    [cx - k * rx, top],
                    [cx, top],
                    [cx + k * rx, top],
                    [right, cy - k * ry],
                    [right, cy],
                ]
            };
            result.kind = VectorObjectKind::Bezier;
            result.path.data = path_data(&result.control_points, true).ok()?;
        }
        VectorObjectKind::Bezier => {}
        VectorObjectKind::Path if object.control_points.len() >= 2 => {
            let points = &object.control_points;
            let mut controls = vec![points[0]];
            for pair in points.windows(2) {
                controls.extend([pair[0], pair[1], pair[1]]);
            }
            if object.path.data.trim_end().ends_with(['Z', 'z']) && points.first() != points.last()
            {
                controls.extend([*points.last()?, points[0], points[0]]);
            }
            result.kind = VectorObjectKind::Bezier;
            result.control_points = controls;
        }
        _ => return None,
    }
    Some(result)
}

pub fn control_hit(
    object: &VectorObject,
    point: [f32; 2],
    tolerance: f32,
    handles: bool,
) -> Option<usize> {
    // Prefer anchors when a collapsed handle occupies the same position.
    (0..object.control_points.len())
        .step_by(3)
        .chain((0..object.control_points.len()).filter(|index| handles && !index.is_multiple_of(3)))
        .find(|&index| {
            distance(world_point(object, object.control_points[index]), point) <= tolerance
        })
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

pub fn segment_hit(object: &VectorObject, point: [f32; 2], tolerance: f32) -> Option<(usize, f32)> {
    let mut best = None;
    let mut best_distance = tolerance;
    for start in (0..object.control_points.len().saturating_sub(3)).step_by(3) {
        let segment =
            std::array::from_fn(|i| world_point(object, object.control_points[start + i]));
        let length: f32 = segment.windows(2).map(|p| distance(p[0], p[1])).sum();
        let steps = (length / tolerance.max(0.5)).ceil().clamp(16., 4096.) as usize;
        let mut closest = (f32::INFINITY, 0.);
        for index in 0..=steps {
            let t = index as f32 / steps as f32;
            let d = distance(evaluate(&segment, t), point);
            if d < closest.0 {
                closest = (d, t);
            }
        }
        let mut lo = (closest.1 - 1. / steps as f32).max(0.);
        let mut hi = (closest.1 + 1. / steps as f32).min(1.);
        for _ in 0..18 {
            let a = lo + (hi - lo) / 3.;
            let b = hi - (hi - lo) / 3.;
            if distance(evaluate(&segment, a), point) < distance(evaluate(&segment, b), point) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let t = (lo + hi) * 0.5;
        let d = distance(evaluate(&segment, t), point);
        if d < best_distance && t > 0.001 && t < 0.999 {
            best_distance = d;
            best = Some((start, t));
        }
    }
    best
}

/// De Casteljau subdivision preserves the exact shape of the curve.
pub fn insert(object: &mut VectorObject, start: usize, t: f32) -> Result<(), String> {
    if !start.is_multiple_of(3)
        || start + 3 >= object.control_points.len()
        || !(0.0..1.0).contains(&t)
        || t == 0.0
    {
        return Err("Invalid curve segment".into());
    }
    let p = &object.control_points[start..start + 4];
    let a = lerp(p[0], p[1], t);
    let b = lerp(p[1], p[2], t);
    let c = lerp(p[2], p[3], t);
    let d = lerp(a, b, t);
    let e = lerp(b, c, t);
    let anchor = lerp(d, e, t);
    let end = p[3];
    object
        .control_points
        .splice(start + 1..start + 4, [a, d, anchor, e, c, end]);
    rebuild(object)
}

pub fn remove(object: &mut VectorObject, mut index: usize) -> Result<(), String> {
    let closed = object.path.data.trim_end().ends_with(['Z', 'z']);
    let count = (object.control_points.len() - 1) / 3 + usize::from(!closed);
    if !index.is_multiple_of(3) || index >= object.control_points.len() {
        return Err("Invalid anchor".into());
    }
    if count <= if closed { 3 } else { 2 } {
        return Err("Keep at least two anchors (three for a closed path). / 開いたパスは2点、閉じたパスは3点以上必要です。 / 开放路径至少保留两个锚点，闭合路径至少保留三个。".into());
    }
    let points = &mut object.control_points;
    let last = points.len() - 1;
    if closed && (index == 0 || index == last) {
        let mut rotated = points[3..].to_vec();
        rotated.extend_from_slice(&points[1..4]);
        *points = rotated;
        index = last - 3;
    }
    if !closed && index == 0 {
        points.drain(..3);
    } else if !closed && index == last {
        points.truncate(points.len() - 3);
    } else {
        points.drain(index - 1..index + 2);
    }
    rebuild(object)
}

/// Click collapses the handles into a corner; dragging creates symmetric handles.
/// Dragging an existing handle changes just that handle, producing a cusp.
pub fn convert(
    object: &mut VectorObject,
    index: usize,
    handle: Option<[f32; 2]>,
) -> Result<(), String> {
    let last = object
        .control_points
        .len()
        .checked_sub(1)
        .ok_or("Missing anchor")?;
    if index > last {
        return Err("Invalid anchor".into());
    }
    if !index.is_multiple_of(3) {
        if let Some(handle) = handle {
            object.control_points[index] = handle;
        }
    } else {
        let anchor = object.control_points[index];
        let outgoing = handle.unwrap_or(anchor);
        let incoming = [2. * anchor[0] - outgoing[0], 2. * anchor[1] - outgoing[1]];
        if index > 0 {
            object.control_points[index - 1] = incoming;
        }
        if index < last {
            object.control_points[index + 1] = outgoing;
        }
        if object.path.data.trim_end().ends_with(['Z', 'z']) && (index == 0 || index == last) {
            object.control_points[1] = outgoing;
            object.control_points[last - 1] = incoming;
        }
    }
    rebuild(object)
}

fn rebuild(object: &mut VectorObject) -> Result<(), String> {
    object.path.data = path_data(
        &object.control_points,
        object.path.data.trim_end().ends_with(['Z', 'z']),
    )?;
    object.validate()
}

#[cfg(test)]
mod anchor_tests {
    use super::*;
    use crate::vector::{FillRule, VectorPaint, VectorPath};
    fn curve() -> VectorObject {
        let points = vec![[0., 0.], [0., 100.], [100., 100.], [100., 0.]];
        VectorObject {
            id: "curve".into(),
            name: "Curve".into(),
            text: None,
            path: VectorPath {
                data: path_data(&points, false).unwrap(),
                fill_rule: FillRule::NonZero,
            },
            control_points: points,
            transform: [1., 0., 0., 1., 0., 0.],
            fill: None,
            stroke: Some(VectorPaint {
                color: [0, 0, 0, 255],
            }),
            stroke_width: 1.,
            visible: true,
            kind: VectorObjectKind::Bezier,
        }
    }
    #[test]
    fn inserted_anchor_preserves_all_curve_samples_and_transformed_hit() {
        let mut object = curve();
        object.transform = [2., 0., 0., 3., 30., 40.];
        let original: [[f32; 2]; 4] = object.control_points.clone().try_into().unwrap();
        let click = world_point(&object, evaluate(&original, 0.37));
        let (index, t) = segment_hit(&object, click, 3.).unwrap();
        assert!((t - 0.37).abs() < 0.001);
        insert(&mut object, index, t).unwrap();
        assert_eq!(object.control_points.len(), 7);
        for step in 0..=100 {
            let u = step as f32 / 100.;
            let (offset, v) = if u <= t {
                (0, u / t)
            } else {
                (3, (u - t) / (1. - t))
            };
            let segment = std::array::from_fn(|i| object.control_points[offset + i]);
            assert!(distance(evaluate(&original, u), evaluate(&segment, v)) < 0.001);
        }
        assert!(segment_hit(&object, [2000., 2000.], 3.).is_none());
    }
    #[test]
    fn corners_smooth_handles_and_independent_handles() {
        let mut object = curve();
        insert(&mut object, 0, 0.5).unwrap();
        let anchor = object.control_points[3];
        convert(&mut object, 3, None).unwrap();
        assert_eq!(object.control_points[2], anchor);
        assert_eq!(object.control_points[4], anchor);
        convert(&mut object, 3, Some([anchor[0] + 20., anchor[1] + 10.])).unwrap();
        assert_eq!(object.control_points[2], [anchor[0] - 20., anchor[1] - 10.]);
        let opposite = object.control_points[2];
        convert(&mut object, 4, Some([90., 60.])).unwrap();
        assert_eq!(object.control_points[2], opposite);
        assert_eq!(object.control_points[4], [90., 60.]);
    }
    #[test]
    fn closed_start_deletion_keeps_closure_and_minimum_anchor_count() {
        let mut object = curve();
        object.control_points = vec![
            [0., 0.],
            [0., 0.],
            [10., 0.],
            [10., 0.],
            [10., 0.],
            [10., 10.],
            [10., 10.],
            [10., 10.],
            [0., 10.],
            [0., 10.],
            [0., 10.],
            [0., 0.],
            [0., 0.],
        ];
        object.path.data = path_data(&object.control_points, true).unwrap();
        remove(&mut object, 0).unwrap();
        assert_eq!(object.control_points.len(), 10);
        assert_eq!(object.control_points.first(), object.control_points.last());
        assert_eq!(object.control_points[0], [10., 0.]);
        assert!(object.path.data.ends_with(" Z"));
        let before = object.clone();
        assert!(remove(&mut object, 0).is_err());
        assert_eq!(object, before);
        convert(&mut object, 0, Some([15., 0.])).unwrap();
        assert_eq!(object.control_points[1], [15., 0.]);
        assert_eq!(object.control_points[8], [5., 0.]);
    }
    #[test]
    fn open_endpoint_deletion_and_pencil_conversion() {
        let mut pencil = curve();
        pencil.kind = VectorObjectKind::Path;
        pencil.control_points = vec![[0., 0.], [10., 10.], [20., 0.], [30., 10.]];
        pencil.path.data = "M 0 0 L 10 10 L 20 0 L 30 10".into();
        let mut object = editable(&pencil).unwrap();
        assert_eq!(object.kind, VectorObjectKind::Bezier);
        remove(&mut object, 0).unwrap();
        assert_eq!(object.control_points[0], [10., 10.]);
        let last = object.control_points.len() - 1;
        remove(&mut object, last).unwrap();
        assert_eq!(object.control_points.last(), Some(&[20., 0.]));
        assert!(remove(&mut object, 0).is_err());
    }
}

/// Move anchors with their attached handles, or move an individual direction handle.
pub fn translate_controls(
    object: &mut VectorObject,
    indices: &[usize],
    delta: [f32; 2],
    break_smooth: bool,
) -> Result<(), String> {
    let original = object.control_points.clone();
    let last = original.len().checked_sub(1).ok_or("Missing controls")?;
    let closed = object.path.data.trim_end().ends_with(['Z', 'z']);
    let local_zero = local_point(object, [0., 0.])?;
    let local_end = local_point(object, delta)?;
    let offset = [local_end[0] - local_zero[0], local_end[1] - local_zero[1]];
    let mut move_indices = std::collections::BTreeSet::new();
    for &index in indices {
        if index > last {
            return Err("Invalid control index".into());
        }
        move_indices.insert(index);
        if index % 3 == 0 {
            if index > 0 {
                move_indices.insert(index - 1);
            }
            if index < last {
                move_indices.insert(index + 1);
            }
            if closed && (index == 0 || index == last) {
                move_indices.extend([0, 1, last - 1, last]);
            }
        }
    }
    for &index in &move_indices {
        object.control_points[index] = [
            original[index][0] + offset[0],
            original[index][1] + offset[1],
        ];
    }
    if !break_smooth {
        for &index in indices.iter().filter(|&&index| !index.is_multiple_of(3)) {
            let anchor = if index % 3 == 1 { index - 1 } else { index + 1 };
            if move_indices.contains(&anchor) {
                continue;
            }
            let opposite = if index % 3 == 1 {
                if anchor > 0 {
                    Some(anchor - 1)
                } else if closed {
                    Some(last - 1)
                } else {
                    None
                }
            } else if anchor < last {
                Some(anchor + 1)
            } else if closed {
                Some(1)
            } else {
                None
            };
            let Some(opposite) = opposite.filter(|i| !move_indices.contains(i)) else {
                continue;
            };
            let a = original[anchor];
            let v = [original[index][0] - a[0], original[index][1] - a[1]];
            let w = [original[opposite][0] - a[0], original[opposite][1] - a[1]];
            let length = w[0].hypot(w[1]);
            let smooth = (v[0] * w[1] - v[1] * w[0]).abs()
                <= 0.001 * (v[0].hypot(v[1]) * length).max(1.)
                && v[0] * w[0] + v[1] * w[1] < 0.;
            let moved = object.control_points[index];
            let direction = [moved[0] - a[0], moved[1] - a[1]];
            let magnitude = direction[0].hypot(direction[1]);
            if smooth && magnitude > 0.00001 {
                object.control_points[opposite] = [
                    a[0] - direction[0] / magnitude * length,
                    a[1] - direction[1] / magnitude * length,
                ];
            }
        }
    }
    rebuild(object)
}

#[cfg(test)]
mod direct_tests {
    use super::*;
    use crate::vector::{FillRule, VectorPaint, VectorPath};
    fn shape() -> VectorObject {
        VectorObject {
            id: "shape".into(),
            name: "Shape".into(),
            text: None,
            path: VectorPath {
                data: "M 0 0 H 100 V 100 H 0 Z".into(),
                fill_rule: FillRule::NonZero,
            },
            transform: [2., 0., 0., 3., 10., 20.],
            fill: Some(VectorPaint {
                color: [0, 0, 0, 255],
            }),
            stroke: None,
            stroke_width: 0.,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[0., 0.], [100., 100.]],
        }
    }
    #[test]
    fn transformed_anchors_move_once_with_handles_and_closed_seam() {
        let mut object = editable(&shape()).unwrap();
        translate_controls(&mut object, &[0, 3], [20., 30.], false).unwrap();
        assert_eq!(object.control_points[0], [10., 10.]);
        assert_eq!(object.control_points[1], [10., 10.]);
        assert_eq!(object.control_points[2], [110., 10.]);
        assert_eq!(object.control_points[3], [110., 10.]);
        assert_eq!(object.control_points.last(), Some(&[10., 10.]));
        assert_eq!(object.control_points[6], [100., 100.]);
    }
    #[test]
    fn smooth_handles_keep_tangent_unless_option_breaks_it() {
        let mut source = shape();
        source.kind = VectorObjectKind::Ellipse;
        source.transform = [1., 0., 0., 1., 0., 0.];
        let original = editable(&source).unwrap();
        let mut smooth = original.clone();
        let mut corner = original.clone();
        translate_controls(&mut smooth, &[1], [10., 10.], false).unwrap();
        translate_controls(&mut corner, &[1], [10., 10.], true).unwrap();
        assert_eq!(corner.control_points[11], original.control_points[11]);
        assert_ne!(smooth.control_points[11], original.control_points[11]);
        let anchor = smooth.control_points[0];
        let a = [
            smooth.control_points[1][0] - anchor[0],
            smooth.control_points[1][1] - anchor[1],
        ];
        let b = [
            smooth.control_points[11][0] - anchor[0],
            smooth.control_points[11][1] - anchor[1],
        ];
        assert!((a[0] * b[1] - a[1] * b[0]).abs() < 0.001);
    }
}
