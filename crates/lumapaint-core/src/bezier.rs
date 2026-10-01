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
    if object.kind == VectorObjectKind::Rectangle && object.rectangle_radii.is_some() {
        let mut source = object.clone();
        source.kind = VectorObjectKind::Compound;
        source.rectangle_radii = None;
        return editable(&source);
    }
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
        VectorObjectKind::Compound => {
            pack(&mut result, cubic_contours(&object.path.data).ok()?).ok()?;
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

/// Parse SVG commands (including quadratic curves and arcs) into cubic contours.
pub type CubicContour = (Vec<[f32; 2]>, bool);
pub fn cubic_contours(data: &str) -> Result<Vec<CubicContour>, String> {
    use svgtypes::SimplePathSegment::*;
    let mut result = Vec::new();
    let mut points = Vec::new();
    let mut closed = false;
    let mut at = [0., 0.];
    for item in svgtypes::SimplifyingPathParser::from(data) {
        match item.map_err(|e| e.to_string())? {
            MoveTo { x, y } => {
                if !points.is_empty() {
                    result.push((std::mem::take(&mut points), closed));
                }
                at = [x as f32, y as f32];
                points.push(at);
                closed = false;
            }
            LineTo { x, y } => {
                let end = [x as f32, y as f32];
                points.extend([at, end, end]);
                at = end;
            }
            CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                at = [x as f32, y as f32];
                points.extend([[x1 as f32, y1 as f32], [x2 as f32, y2 as f32], at]);
            }
            Quadratic { x1, y1, x, y } => {
                let q = [x1 as f32, y1 as f32];
                let end = [x as f32, y as f32];
                points.extend([lerp(at, q, 2. / 3.), lerp(end, q, 2. / 3.), end]);
                at = end;
            }
            ClosePath => {
                let first = *points.first().ok_or("Missing contour start")?;
                if at != first {
                    points.extend([at, first, first]);
                }
                at = first;
                closed = true;
            }
        }
        if points.len() > 65_536 || result.len() > 4096 {
            return Err("Too many path controls".into());
        }
    }
    if !points.is_empty() {
        result.push((points, closed));
    }
    if result.is_empty() || result.iter().map(|(p, _)| p.len() + 2).sum::<usize>() > 65_536 {
        return Err("Invalid path contours".into());
    }
    Ok(result)
}

/// Contours use two inert padding controls so every anchor retains a multiple-of-three index.
fn pack(object: &mut VectorObject, contours: Vec<(Vec<[f32; 2]>, bool)>) -> Result<(), String> {
    let single = contours.len() == 1;
    let mut data = String::new();
    let mut points = Vec::new();
    for (p, closed) in contours {
        if !points.is_empty() {
            points.extend([points[0]; 2]);
        }
        data.push_str(&path_data(&p, closed)?);
        data.push(' ');
        points.extend(p);
    }
    object.kind = if single && object.kind != VectorObjectKind::Compound {
        VectorObjectKind::Bezier
    } else {
        VectorObjectKind::Compound
    };
    object.path.data = data.trim_end().into();
    object.control_points = points;
    object.validate()
}

pub fn contour_ranges(object: &VectorObject) -> Vec<(usize, usize, bool)> {
    if object.kind != VectorObjectKind::Compound {
        return vec![(
            0,
            object.control_points.len(),
            object.path.data.trim_end().ends_with(['Z', 'z']),
        )];
    }
    let mut start = 0;
    cubic_contours(&object.path.data)
        .unwrap_or_default()
        .into_iter()
        .map(|(p, c)| {
            let range = (start, start + p.len(), c);
            start += p.len() + 2;
            range
        })
        .collect()
}

pub fn anchor_indices(object: &VectorObject) -> Vec<usize> {
    contour_ranges(object)
        .into_iter()
        .flat_map(|(start, end, closed)| {
            (start..end.saturating_sub(usize::from(closed))).step_by(3)
        })
        .collect()
}
pub fn segment_indices(object: &VectorObject) -> Vec<usize> {
    contour_ranges(object)
        .into_iter()
        .flat_map(|(start, end, _)| (start..end.saturating_sub(3)).step_by(3))
        .collect()
}
pub fn control_indices(object: &VectorObject) -> Vec<usize> {
    contour_ranges(object)
        .into_iter()
        .flat_map(|(start, end, _)| start..end)
        .collect()
}
pub fn canonical_anchor(object: &VectorObject, index: usize) -> usize {
    contour_ranges(object)
        .into_iter()
        .find(|&(s, e, _)| index >= s && index < e)
        .map_or(
            index,
            |(s, e, c)| if c && index == e - 1 { s } else { index },
        )
}

fn edit_contour(
    object: &mut VectorObject,
    index: usize,
    edit: impl FnOnce(&mut VectorObject, usize) -> Result<(), String>,
) -> Result<(), String> {
    object.live_corners = None;
    let mut contours = cubic_contours(&object.path.data)?;
    let mut offset = 0;
    for (p, closed) in &mut contours {
        if index >= offset && index < offset + p.len() {
            let mut part = object.clone();
            part.kind = VectorObjectKind::Bezier;
            part.control_points = p.clone();
            part.path.data = path_data(p, *closed)?;
            edit(&mut part, index - offset)?;
            *p = part.control_points;
            return pack(object, contours);
        }
        offset += p.len() + 2;
    }
    Err("Invalid contour control".into())
}

/// Delete selected anchors and their incident edges without inventing connecting segments.
pub fn split_deleted(
    object: &VectorObject,
    indices: &[usize],
) -> Result<Option<VectorObject>, String> {
    let object = editable(object).ok_or("Path cannot be edited")?;
    let selected: std::collections::BTreeSet<_> = indices.iter().copied().collect();
    let mut output = Vec::new();
    for (start, end, closed) in contour_ranges(&object) {
        let p = &object.control_points[start..end];
        let n = (p.len() - 1) / 3 + usize::from(!closed);
        let deleted: Vec<_> = (0..n)
            .map(|i| {
                selected.contains(&(start + i * 3))
                    || (closed && i == 0 && selected.contains(&(end - 1)))
            })
            .collect();
        if !deleted.iter().any(|d| *d) {
            output.push((p.to_vec(), closed));
            continue;
        }
        let mut run = Vec::new();
        let first = if closed {
            (deleted.iter().position(|d| *d).unwrap() + 1) % n
        } else {
            0
        };
        for step in 0..n {
            let i = (first + step) % n;
            if deleted[i] {
                if run.len() >= 4 {
                    output.push((std::mem::take(&mut run), false));
                } else {
                    run.clear();
                }
            } else if run.is_empty() {
                run.push(p[i * 3]);
            } else {
                let previous = (i + n - 1) % n;
                run.extend([p[previous * 3 + 1], p[previous * 3 + 2], p[i * 3]]);
            }
        }
        if run.len() >= 4 {
            output.push((run, false));
        }
    }
    if output.is_empty() {
        return Ok(None);
    }
    let mut result = object;
    result.live_corners = None;
    pack(&mut result, output)?;
    Ok(Some(result))
}

pub fn control_hit(
    object: &VectorObject,
    point: [f32; 2],
    tolerance: f32,
    handles: bool,
) -> Option<usize> {
    // Prefer anchors when a collapsed handle occupies the same position.
    anchor_indices(object)
        .into_iter()
        .chain(
            control_indices(object)
                .into_iter()
                .filter(|index| handles && !index.is_multiple_of(3)),
        )
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
    for start in segment_indices(object) {
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
    if object.kind == VectorObjectKind::Compound {
        return edit_contour(object, start, |p, i| insert(p, i, t));
    }
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
    if object.kind == VectorObjectKind::Compound {
        return edit_contour(object, index, remove);
    }
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
    if object.kind == VectorObjectKind::Compound {
        return edit_contour(object, index, |p, i| convert(p, i, handle));
    }
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
    object.live_corners = None;
    object.path.data = path_data(
        &object.control_points,
        object.path.data.trim_end().ends_with(['Z', 'z']),
    )?;
    object.validate()
}

fn edit_all_contours(
    object: &mut VectorObject,
    mut edit: impl FnMut(&mut VectorObject) -> Result<(), String>,
) -> Result<(), String> {
    let mut contours = cubic_contours(&object.path.data)?;
    let mut result = object.clone();
    result.live_corners = None;
    for (points, closed) in &mut contours {
        let mut part = result.clone();
        part.kind = VectorObjectKind::Bezier;
        part.control_points = points.clone();
        part.path.data = path_data(points, *closed)?;
        edit(&mut part)?;
        *points = part.control_points;
    }
    pack(&mut result, contours)?;
    *object = result;
    Ok(())
}

pub fn reverse(object: &mut VectorObject) -> Result<(), String> {
    if object.kind == VectorObjectKind::Compound {
        return edit_all_contours(object, reverse);
    }
    let mut edited = editable(object).ok_or("Path cannot be edited")?;
    let original = edited.control_points.clone();
    let mut reversed = vec![*original.last().ok_or("Missing path points")?];
    for segment in original.windows(4).step_by(3).rev() {
        reversed.extend([segment[2], segment[1], segment[0]]);
    }
    edited.control_points = reversed;
    rebuild(&mut edited)?;
    *object = edited;
    Ok(())
}

pub fn subdivide_all(object: &mut VectorObject) -> Result<(), String> {
    if object.kind == VectorObjectKind::Compound {
        return edit_all_contours(object, subdivide_all);
    }
    let mut edited = editable(object).ok_or("Path cannot be edited")?;
    let starts: Vec<_> = (0..edited.control_points.len().saturating_sub(3))
        .step_by(3)
        .collect();
    for start in starts.into_iter().rev() {
        insert(&mut edited, start, 0.5)?;
    }
    *object = edited;
    Ok(())
}

pub fn remove_alternate_anchors(object: &mut VectorObject) -> Result<(), String> {
    if object.kind == VectorObjectKind::Compound {
        return edit_all_contours(object, remove_alternate_anchors);
    }
    let mut edited = editable(object).ok_or("Path cannot be edited")?;
    let closed = edited.path.data.trim_end().ends_with(['Z', 'z']);
    let anchors = (edited.control_points.len() - 1) / 3 + usize::from(!closed);
    let minimum = if closed { 3 } else { 2 };
    let mut indices: Vec<_> = (3..edited.control_points.len().saturating_sub(1))
        .step_by(6)
        .take(anchors.saturating_sub(minimum))
        .collect();
    for index in indices.drain(..).rev() {
        remove(&mut edited, index)?;
    }
    *object = edited;
    Ok(())
}

fn point_line_distance(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length2 = delta[0] * delta[0] + delta[1] * delta[1];
    if length2 <= f32::EPSILON {
        return distance(point, start);
    }
    let t = (((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1]) / length2)
        .clamp(0.0, 1.0);
    distance(point, [start[0] + delta[0] * t, start[1] + delta[1] * t])
}

fn rdp(points: &[[f32; 2]], tolerance: f32) -> Vec<[f32; 2]> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut furthest = (0usize, 0.0f32);
    for (index, point) in points[1..points.len() - 1].iter().enumerate() {
        let value = point_line_distance(*point, points[0], points[points.len() - 1]);
        if value > furthest.1 {
            furthest = (index + 1, value);
        }
    }
    if furthest.1 <= tolerance {
        return vec![points[0], points[points.len() - 1]];
    }
    let mut left = rdp(&points[..=furthest.0], tolerance);
    left.pop();
    left.extend(rdp(&points[furthest.0..], tolerance));
    left
}

fn rebuild_polyline(
    object: &mut VectorObject,
    points: &[[f32; 2]],
    closed: bool,
) -> Result<(), String> {
    if points.len() < if closed { 3 } else { 2 } {
        return Err("Path has too few anchors".into());
    }
    let mut controls = vec![points[0]];
    for pair in points.windows(2) {
        controls.extend([pair[0], pair[1], pair[1]]);
    }
    if closed && points.last() != points.first() {
        controls.extend([*points.last().unwrap(), points[0], points[0]]);
    }
    object.control_points = controls;
    object.kind = VectorObjectKind::Bezier;
    object.path.data = path_data(&object.control_points, closed)?;
    object.validate()
}

pub fn simplify(object: &mut VectorObject, tolerance: f32) -> Result<(), String> {
    if object.kind == VectorObjectKind::Compound {
        return edit_all_contours(object, |p| simplify(p, tolerance));
    }
    let edited = editable(object).ok_or("Path cannot be edited")?;
    let closed = edited.path.data.trim_end().ends_with(['Z', 'z']);
    let mut samples = flattened(&edited.control_points);
    if closed && samples.last() == samples.first() {
        samples.pop();
    }
    let simplified = rdp(&samples, tolerance.max(0.01));
    let mut result = edited;
    rebuild_polyline(&mut result, &simplified, closed)?;
    *object = result;
    Ok(())
}

pub fn smooth(object: &mut VectorObject) -> Result<(), String> {
    if object.kind == VectorObjectKind::Compound {
        return edit_all_contours(object, smooth);
    }
    let mut edited = editable(object).ok_or("Path cannot be edited")?;
    let closed = edited.path.data.trim_end().ends_with(['Z', 'z']);
    let last = edited.control_points.len() - 1;
    let anchors: Vec<_> = (0..=last)
        .step_by(3)
        .map(|index| edited.control_points[index])
        .collect();
    let unique = if closed {
        &anchors[..anchors.len() - 1]
    } else {
        &anchors[..]
    };
    if unique.len() < 2 {
        return Err("Path has too few anchors".into());
    }
    for index in 0..unique.len() {
        let control = index * 3;
        let previous = if index > 0 {
            unique[index - 1]
        } else if closed {
            unique[unique.len() - 1]
        } else {
            unique[index]
        };
        let next = if index + 1 < unique.len() {
            unique[index + 1]
        } else if closed {
            unique[0]
        } else {
            unique[index]
        };
        let tangent = [(next[0] - previous[0]) / 6.0, (next[1] - previous[1]) / 6.0];
        if control > 0 {
            edited.control_points[control - 1] =
                [unique[index][0] - tangent[0], unique[index][1] - tangent[1]];
        }
        if control < last {
            edited.control_points[control + 1] =
                [unique[index][0] + tangent[0], unique[index][1] + tangent[1]];
        }
    }
    if closed {
        edited.control_points[last] = edited.control_points[0];
        edited.control_points[last - 1] = [
            edited.control_points[0][0] - (unique[1][0] - unique[unique.len() - 1][0]) / 6.0,
            edited.control_points[0][1] - (unique[1][1] - unique[unique.len() - 1][1]) / 6.0,
        ];
    }
    rebuild(&mut edited)?;
    *object = edited;
    Ok(())
}

pub fn clean_up(object: &mut VectorObject) -> Result<(), String> {
    simplify(object, 0.05)
}

#[cfg(test)]
mod anchor_tests {
    use super::*;
    use crate::vector::{FillRule, VectorPaint, VectorPath};
    fn curve() -> VectorObject {
        let points = vec![[0., 0.], [0., 100.], [100., 100.], [100., 0.]];
        VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: "curve".into(),
            name: "Curve".into(),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
            text: None,
            path: VectorPath {
                data: path_data(&points, false).unwrap(),
                fill_rule: FillRule::NonZero,
            },
            control_points: points,
            transform: [1., 0., 0., 1., 0., 0.],
            fill_gradient: None,
            stroke_gradient: None,
            fill: None,
            stroke: Some(VectorPaint {
                color: [0, 0, 0, 255],
            }),
            stroke_style: Default::default(),
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

    #[test]
    fn whole_path_edits_preserve_valid_geometry() {
        let original = curve();
        let mut reversed = original.clone();
        reverse(&mut reversed).unwrap();
        assert_eq!(
            reversed.control_points.first(),
            original.control_points.last()
        );
        reverse(&mut reversed).unwrap();
        assert_eq!(reversed.control_points, original.control_points);

        let mut subdivided = original.clone();
        subdivide_all(&mut subdivided).unwrap();
        assert_eq!(subdivided.control_points.len(), 7);
        remove_alternate_anchors(&mut subdivided).unwrap();
        assert_eq!(subdivided.control_points.len(), 4);

        let mut smoothed = editable(&original).unwrap();
        smooth(&mut smoothed).unwrap();
        smoothed.validate().unwrap();
        simplify(&mut smoothed, 2.0).unwrap();
        smoothed.validate().unwrap();
        clean_up(&mut smoothed).unwrap();
        smoothed.validate().unwrap();
    }
}

/// Move anchors with their attached handles, or move an individual direction handle.
pub fn translate_controls(
    object: &mut VectorObject,
    indices: &[usize],
    delta: [f32; 2],
    break_smooth: bool,
) -> Result<(), String> {
    object.live_corners = None;
    if object.kind == VectorObjectKind::Compound {
        let mut contours = cubic_contours(&object.path.data)?;
        let mut offset = 0;
        let mut found = 0;
        for (p, c) in &mut contours {
            let local: Vec<_> = indices
                .iter()
                .filter(|&&i| i >= offset && i < offset + p.len())
                .map(|&i| i - offset)
                .collect();
            found += local.len();
            let mut part = object.clone();
            part.kind = VectorObjectKind::Bezier;
            part.control_points = p.clone();
            part.path.data = path_data(p, *c)?;
            translate_controls(&mut part, &local, delta, break_smooth)?;
            *p = part.control_points;
            offset += p.len() + 2;
        }
        if found != indices.len() {
            return Err("Invalid contour controls".into());
        }
        return pack(object, contours);
    }
    object.live_corners = None;
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
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: "shape".into(),
            name: "Shape".into(),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
            text: None,
            path: VectorPath {
                data: "M 0 0 H 100 V 100 H 0 Z".into(),
                fill_rule: FillRule::NonZero,
            },
            transform: [2., 0., 0., 3., 10., 20.],
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                color: [0, 0, 0, 255],
            }),
            stroke: None,
            stroke_style: Default::default(),
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

/// The unrounded path is persisted so a live radius can be changed without cumulative loss.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiveCorners {
    pub source: String,
    pub anchors: Vec<usize>,
    pub radius: f32,
}
impl LiveCorners {
    pub fn validate(&self) -> Result<(), String> {
        if !self.radius.is_finite()
            || !(0.0..=4096.).contains(&self.radius)
            || self.source.len() > 1024 * 1024
            || self.anchors.len() > 65_536
        {
            return Err("Invalid live corners".into());
        }
        let contours = cubic_contours(&self.source)?;
        let mut valid = Vec::new();
        let mut offset = 0;
        for (p, c) in contours {
            valid.extend((offset..offset + p.len() - usize::from(c)).step_by(3));
            offset += p.len() + 2;
        }
        let valid: std::collections::BTreeSet<_> = valid.into_iter().collect();
        if self.anchors.is_empty() || self.anchors.iter().any(|i| !valid.contains(i)) {
            return Err("Invalid corner anchors".into());
        }
        Ok(())
    }
}

pub fn round_corners(
    object: &VectorObject,
    indices: &[usize],
    radius: f32,
) -> Result<VectorObject, String> {
    if !radius.is_finite() || !(0.0..=4096.).contains(&radius) {
        return Err("Invalid corner radius".into());
    }
    let (source, indices) = object
        .live_corners
        .as_ref()
        .map_or((object.path.data.clone(), indices.to_vec()), |c| {
            (c.source.clone(), c.anchors.clone())
        });
    let mut original = object.clone();
    original.live_corners = None;
    original.kind = VectorObjectKind::Compound;
    original.path.data = source.clone();
    let original = editable(&original).ok_or("Invalid corner source")?;
    let aliases: std::collections::BTreeMap<_, _> = contour_ranges(&original)
        .into_iter()
        .filter(|(_, _, c)| *c)
        .map(|(s, e, _)| (e - 1, s))
        .collect();
    let anchors: Vec<_> = indices
        .iter()
        .map(|i| aliases.get(i).copied().unwrap_or(*i))
        .collect();
    let valid: std::collections::BTreeSet<_> = anchor_indices(&original).into_iter().collect();
    if anchors.is_empty() || anchors.iter().any(|i| !valid.contains(i)) {
        return Err("Select corner anchors / 角の節点を選択してください / 请选择角点".into());
    }
    let selected: std::collections::BTreeSet<_> = anchors.iter().copied().collect();
    let mut contours = Vec::new();
    for (start, end, closed) in contour_ranges(&original) {
        let p = &original.control_points[start..end];
        if !anchors.iter().any(|i| *i >= start && *i < end) || radius == 0. {
            contours.push((p.to_vec(), closed));
            continue;
        }
        let world: Vec<_> = p.iter().map(|&p| world_point(&original, p)).collect();
        for segment in world.windows(4).step_by(3) {
            if point_line_distance(segment[1], segment[0], segment[3]) > 0.001
                || point_line_distance(segment[2], segment[0], segment[3]) > 0.001
            {
                return Err("Live corners require straight edges / ライブコーナーは直線の角に対応しています / 实时圆角需要直线边".into());
            }
        }
        let n = (p.len() - 1) / 3 + usize::from(!closed);
        let mut corners = Vec::new();
        for i in 0..n {
            let at = world[i * 3];
            let mut incoming = at;
            let mut outgoing = at;
            let mut c1 = at;
            let mut c2 = at;
            if selected.contains(&(start + i * 3)) && (closed || (i > 0 && i + 1 < n)) {
                let prev = world[((i + n - 1) % n) * 3];
                let next = world[((i + 1) % n) * 3];
                let l1 = distance(prev, at);
                let l2 = distance(next, at);
                if l1 > 0.0001 && l2 > 0.0001 {
                    let u = [(prev[0] - at[0]) / l1, (prev[1] - at[1]) / l1];
                    let v = [(next[0] - at[0]) / l2, (next[1] - at[1]) / l2];
                    let angle = (u[0] * v[0] + u[1] * v[1]).clamp(-1., 1.).acos();
                    if angle > 0.001 && angle < std::f32::consts::PI - 0.001 {
                        let tangent = (angle / 2.).tan();
                        let d = (radius / tangent).min(l1 / 2.).min(l2 / 2.);
                        let effective = d * tangent;
                        let h = 4. / 3. * ((std::f32::consts::PI - angle) / 4.).tan() * effective;
                        incoming = [at[0] + u[0] * d, at[1] + u[1] * d];
                        outgoing = [at[0] + v[0] * d, at[1] + v[1] * d];
                        c1 = [incoming[0] - u[0] * h, incoming[1] - u[1] * h];
                        c2 = [outgoing[0] - v[0] * h, outgoing[1] - v[1] * h];
                    }
                }
            }
            corners.push((incoming, c1, c2, outgoing));
        }
        let mut rounded = vec![corners[0].0];
        for (i, &(a, c1, c2, b)) in corners.iter().enumerate() {
            if i > 0 {
                rounded.extend([*rounded.last().unwrap(), a, a]);
            }
            if a != b {
                rounded.extend([c1, c2, b]);
            }
        }
        if closed {
            let first = rounded[0];
            rounded.extend([*rounded.last().unwrap(), first, first]);
        }
        let local = rounded
            .into_iter()
            .map(|p| local_point(&original, p))
            .collect::<Result<Vec<_>, _>>()?;
        contours.push((local, closed));
    }
    let mut result = original;
    result.kind = object.kind;
    pack(&mut result, contours)?;
    result.live_corners = Some(LiveCorners {
        source,
        anchors,
        radius,
    });
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod contour_edit_tests {
    use super::*;
    use crate::vector::{FillRule, VectorPaint, VectorPath};
    pub fn object(data: &str) -> VectorObject {
        editable(&VectorObject {
            live_corners: None,
            rectangle_radii: None,
            id: "test".into(),
            name: "Test".into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            text: None,
            path: VectorPath {
                data: data.into(),
                fill_rule: FillRule::EvenOdd,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                color: [0, 0, 0, 255],
            }),
            stroke: None,
            stroke_width: 1.,
            stroke_style: Default::default(),
            opacity: 1.,
            blend_mode: "normal".into(),
            visible: true,
            kind: VectorObjectKind::Compound,
            control_points: vec![],
        })
        .unwrap()
    }
    #[test]
    fn compound_edit_keeps_hole_seams_and_never_selects_padding() {
        let mut o = object("M0 0H100V100H0Z M30 30V70H70V30Z");
        let ranges = contour_ranges(&o);
        assert_eq!(ranges.len(), 2);
        let second = ranges[1].0;
        let original = o.control_points.clone();
        assert!(!control_indices(&o).contains(&(second - 1)));
        translate_controls(&mut o, &[second], [10., 5.], false).unwrap();
        assert_eq!(&o.control_points[..ranges[0].1], &original[..ranges[0].1]);
        assert_eq!(o.control_points[second], [40., 35.]);
        assert_eq!(o.control_points[ranges[1].1 - 1], [40., 35.]);
        assert_eq!(canonical_anchor(&o, ranges[1].1 - 1), second);
        assert_eq!(o.path.fill_rule, FillRule::EvenOdd);
        assert_eq!(segment_indices(&o).len(), 8);
        insert(&mut o, second, 0.5).unwrap();
        assert_eq!(segment_indices(&o).len(), 9);
        o.validate().unwrap();
    }
    #[test]
    fn svg_quadratic_arc_relative_commands_become_cubics() {
        let o = object("m10 10q20 40 40 0a20 20 0 0 1 40 0m20 0l10 20");
        assert_eq!(contour_ranges(&o).len(), 2);
        assert!(segment_indices(&o).len() >= 4);
        assert!(cubic_contours("M0 0 Q invalid").is_err());
    }
    #[test]
    fn deleting_middle_anchor_retains_only_original_incident_free_edges() {
        let o = object("M0 0C1 2 8 2 10 0C12 1 18 1 20 0C22 3 28 3 30 0C32 4 38 4 40 0");
        let split = split_deleted(&o, &[6]).unwrap().unwrap();
        let ranges = contour_ranges(&split);
        assert_eq!(ranges.len(), 2);
        assert!(ranges.iter().all(|r| !r.2));
        assert_eq!(&split.control_points[0..4], &o.control_points[0..4]);
        assert_eq!(
            &split.control_points[ranges[1].0..ranges[1].1],
            &o.control_points[9..13]
        );
        assert_eq!(segment_indices(&split).len(), 2);
        assert!(split_deleted(&o, &[0, 3, 6, 9, 12]).unwrap().is_none());
    }
    #[test]
    fn deleting_closed_seam_opens_path_and_preserves_other_contour() {
        let o = object("M0 0H100V100H0Z M200 0L300 0");
        let split = split_deleted(&o, &[0]).unwrap().unwrap();
        let parts = cubic_contours(&split.path.data).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(!parts[0].1);
        assert_eq!(parts[0].0[0], [100., 0.]);
        assert_eq!(parts[0].0.last(), Some(&[0., 100.]));
        assert_eq!(parts[1].0, cubic_contours(&o.path.data).unwrap()[1].0);
    }
    #[test]
    fn corners_are_reversible_and_use_world_radius_after_transform() {
        let mut o = object("M0 0H100V100H0Z");
        o.transform = [2., 0., 0., 3., 10., 20.];
        let rounded = round_corners(&o, &[3], 20.).unwrap();
        let p: Vec<_> = rounded
            .control_points
            .iter()
            .map(|&p| world_point(&rounded, p))
            .collect();
        assert!(p.contains(&[190., 20.]));
        assert!(p.contains(&[210., 40.]));
        let larger = round_corners(&rounded, &[3], 40.).unwrap();
        assert_eq!(larger.live_corners.as_ref().unwrap().source, o.path.data);
        let restored = round_corners(&larger, &[3], 0.).unwrap();
        assert_eq!(restored.path.data, o.path.data);
        assert_eq!(restored.control_points, o.control_points);
        let encoded = serde_json::to_string(&larger).unwrap();
        let decoded: VectorObject = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, larger);
        let mut moved = larger;
        translate_controls(&mut moved, &[0], [1., 1.], false).unwrap();
        assert!(moved.live_corners.is_none());
    }
    #[test]
    fn corners_clamp_adjacent_edges_and_reject_curves_handles_and_bad_radius() {
        let o = object("M0 0H10V10H0Z");
        let rounded = round_corners(&o, &[0, 3, 6, 9], 1000.).unwrap();
        rounded.validate().unwrap();
        for p in &rounded.control_points {
            assert!((0.0..=10.).contains(&p[0]) && (0.0..=10.).contains(&p[1]));
        }
        assert!(round_corners(&o, &[1], 5.).is_err());
        assert!(round_corners(&o, &[3], f32::NAN).is_err());
        let curve = object("M0 0C0 100 100 100 100 0L200 0");
        assert!(round_corners(&curve, &[3], 5.).is_err());
    }
}

#[cfg(test)]
mod contour_operation_tests {
    use super::contour_edit_tests::object;
    use super::*;
    #[test]
    fn reversing_and_subdividing_compound_paths_keep_independent_contours() {
        let mut o = object("M100 100L200 100 M300 300L400 300");
        let original = o.clone();
        assert!(!o.hit_test([0., 0.], 1.));
        reverse(&mut o).unwrap();
        assert_eq!(contour_ranges(&o).len(), 2);
        assert_eq!(cubic_contours(&o.path.data).unwrap()[0].0[0], [200., 100.]);
        reverse(&mut o).unwrap();
        assert_eq!(o.control_points, original.control_points);
        subdivide_all(&mut o).unwrap();
        assert_eq!(segment_indices(&o).len(), 4);
        remove_alternate_anchors(&mut o).unwrap();
        assert_eq!(segment_indices(&o).len(), 2);
        smooth(&mut o).unwrap();
        assert_eq!(contour_ranges(&o).len(), 2);
    }
}
