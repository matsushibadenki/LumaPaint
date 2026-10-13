//! Bounded closed-path extrusion with nonzero/even-odd holes and explicit side walls.
use super::mesh::{Mesh, Vertex};
use crate::vector::{FillRule, VectorObject};
use std::sync::{Arc, Mutex, OnceLock};
const MAX_EDGES: usize = 2048;
const MAX_VERTICES: usize = 32768;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
type Point = [f32; 2];
type Edge = [Point; 2];
type Cache = Mutex<Vec<(String, Option<Arc<Mesh>>)>>;
#[cfg(test)]
pub fn cached(object: &VectorObject, bounds: [f32; 4], depth: f32) -> Option<Arc<Mesh>> {
    cached_beveled(object, bounds, depth, 0.)
}
pub fn cached_beveled(
    object: &VectorObject,
    bounds: [f32; 4],
    depth: f32,
    bevel: f32,
) -> Option<Arc<Mesh>> {
    if object.text.is_some()
        || object.fill.is_none()
        || object.blend_mode != "normal"
        || depth <= 0.
        || !depth.is_finite()
        || !bevel.is_finite()
        || bevel < 0.
    {
        return None;
    }
    let key = format!(
        "{}:{:?}:{:?}:{:?}:{:?}:{:?}:{}:{}",
        object.path.data,
        object.path.fill_rule,
        object.transform.map(f32::to_bits),
        bounds.map(f32::to_bits),
        depth.to_bits(),
        object.fill,
        object.opacity.to_bits(),
        bevel.to_bits()
    );
    if key.len() > 128 * 1024 {
        return None;
    }
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(mut entries) = cache.lock() {
        if let Some(i) = entries.iter().position(|(k, _)| *k == key) {
            let entry = entries.remove(i);
            let result = entry.1.clone();
            entries.push(entry);
            return result;
        }
    }
    let result = build(object, bounds, depth, bevel)
        .or_else(|| {
            (bevel > 0.)
                .then(|| build(object, bounds, depth, 0.))
                .flatten()
        })
        .map(Arc::new);
    let bytes = |entry: &(String, Option<Arc<Mesh>>)| {
        entry.0.len()
            + entry.1.as_ref().map_or(0, |m| {
                m.vertices.len() * std::mem::size_of::<Vertex>() + m.indices.len() * 4
            })
    };
    if let Ok(mut entries) = cache.lock() {
        let entry = (key, result.clone());
        let size = bytes(&entry);
        while !entries.is_empty()
            && (entries.len() >= 32
                || entries.iter().map(bytes).sum::<usize>() + size > CACHE_BYTES)
        {
            entries.remove(0);
        }
        if size <= CACHE_BYTES {
            entries.push(entry);
        }
    }
    result
}
fn x_at(edge: Edge, y: f32) -> f32 {
    let [a, b] = edge;
    a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1])
}
fn filled(edges: &[Edge], p: Point, rule: FillRule) -> bool {
    let mut winding = 0;
    for &[a, b] in edges {
        if ((a[1] <= p[1] && b[1] > p[1]) || (b[1] <= p[1] && a[1] > p[1]))
            && x_at([a, b], p[1]) > p[0]
        {
            winding += if b[1] > a[1] { 1 } else { -1 };
        }
    }
    match rule {
        FillRule::EvenOdd => winding % 2 != 0,
        FillRule::NonZero => winding != 0,
    }
}
fn face(
    mesh: &mut Mesh,
    points: &[[f32; 3]],
    uv: &[[f32; 2]],
    normal: [f32; 3],
    color: Option<[f32; 4]>,
) -> Option<()> {
    if mesh.vertices.len() + points.len() > MAX_VERTICES {
        return None;
    }
    let base = mesh.vertices.len() as u32;
    mesh.vertices
        .extend(points.iter().zip(uv).map(|(p, u)| Vertex {
            position: *p,
            uv: *u,
            normal,
            color,
        }));
    for i in 1..points.len() - 1 {
        mesh.indices
            .extend([base, base + i as u32, base + i as u32 + 1]);
    }
    Some(())
}
// Flatten in document space to a 0.2px chord tolerance. Straight segments
// contribute one edge, avoiding the generic sampler's eight-edge minimum.
fn flatten_curve(p: [Point; 4], level: u8, out: &mut Vec<Point>) -> Option<()> {
    let dx = p[3][0] - p[0][0];
    let dy = p[3][1] - p[0][1];
    let length = dx.hypot(dy);
    let flat = p[1..3].iter().all(|q| {
        let along = (q[0] - p[0][0]) * dx + (q[1] - p[0][1]) * dy;
        let cross = ((q[0] - p[0][0]) * dy - (q[1] - p[0][1]) * dx).abs();
        length > 0.00001 && cross <= 0.2 * length && along >= 0. && along <= length * length
    });
    if flat || level >= 10 {
        if out.len() >= MAX_EDGES {
            return None;
        }
        out.push(p[3]);
        return Some(());
    }
    let mid = |a: Point, b: Point| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let a = mid(p[0], p[1]);
    let b = mid(p[1], p[2]);
    let c = mid(p[2], p[3]);
    let d = mid(a, b);
    let e = mid(b, c);
    let f = mid(d, e);
    flatten_curve([p[0], a, d, f], level + 1, out)?;
    flatten_curve([f, e, c, p[3]], level + 1, out)
}
// Split boundaries at intersections before probing which side is filled.
// Collinear overlap endpoints also split edges, so internal seams disappear.
fn intersections(edges: &[Edge]) -> Option<(Vec<Edge>, Vec<f32>)> {
    let mut cuts = vec![vec![0f64, 1.]; edges.len()];
    let mut levels = vec![];
    let mut cut_count = edges.len() * 2;
    let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            let [a, b] = edges[i].map(|p| p.map(f64::from));
            let [c, d] = edges[j].map(|p| p.map(f64::from));
            let r = [b[0] - a[0], b[1] - a[1]];
            let v = [d[0] - c[0], d[1] - c[1]];
            let delta = [c[0] - a[0], c[1] - a[1]];
            let denominator = cross(r, v);
            if denominator.abs() > 1e-10 {
                let t = cross(delta, v) / denominator;
                let u = cross(delta, r) / denominator;
                if (0. ..=1.).contains(&t) && (0. ..=1.).contains(&u) {
                    cut_count += 2;
                    cuts[i].push(t);
                    cuts[j].push(u);
                    levels.push((a[1] + r[1] * t) as f32);
                }
            } else if cross(delta, r).abs() < 1e-8 {
                for (index, origin, direction, points) in [(i, a, r, [c, d]), (j, c, v, [a, b])] {
                    let axis = usize::from(direction[1].abs() > direction[0].abs());
                    if direction[axis].abs() < 1e-10 {
                        continue;
                    }
                    for point in points {
                        let t = (point[axis] - origin[axis]) / direction[axis];
                        if t > 0. && t < 1. {
                            cut_count += 1;
                            cuts[index].push(t);
                        }
                    }
                }
            }
            if levels.len() > 8192 || cut_count > 16384 {
                return None;
            }
        }
    }
    let mut split = vec![];
    for (edge, mut cuts) in edges.iter().zip(cuts) {
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
        for pair in cuts.windows(2) {
            split.push([pair[0], pair[1]].map(|t| {
                std::array::from_fn(|k| {
                    (edge[0][k] as f64 + (edge[1][k] as f64 - edge[0][k] as f64) * t) as f32
                })
            }));
            if split.len() > 8192 {
                return None;
            }
        }
    }
    Some((split, levels))
}
// Offset closed non-intersecting contours into filled material. Reject joins
// that reverse an edge or cross another boundary; narrow strokes reduce width.
fn inset(edges: &[Edge], rule: FillRule, width: f32) -> Option<Vec<Edge>> {
    let normal = |edge: Edge| {
        let [a, b] = edge;
        let l = (b[0] - a[0]).hypot(b[1] - a[1]);
        let n = [(b[1] - a[1]) / l, -(b[0] - a[0]) / l];
        let m = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let plus = filled(edges, [m[0] + n[0] * 0.001, m[1] + n[1] * 0.001], rule);
        let minus = filled(edges, [m[0] - n[0] * 0.001, m[1] - n[1] * 0.001], rule);
        if plus == minus {
            None
        } else {
            Some(if plus { n.map(|v| -v) } else { n })
        }
    };
    let normals = edges
        .iter()
        .copied()
        .map(normal)
        .collect::<Option<Vec<_>>>()?;
    let mut points = Vec::new();
    for (i, edge) in edges.iter().enumerate() {
        let previous = edges.iter().position(|e| e[1] == edge[0])?;
        let a = normals[previous];
        let b = normals[i];
        let denominator = 1. + a[0] * b[0] + a[1] * b[1];
        if denominator < 0.0001 {
            return None;
        }
        let p = std::array::from_fn(|k| edge[0][k] - width * (a[k] + b[k]) / denominator);
        if !filled(edges, p, rule) {
            return None;
        }
        points.push(p);
    }
    let result = edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let next = edges.iter().position(|a| a[0] == e[1])?;
            let segment = [points[i], points[next]];
            let old = [e[1][0] - e[0][0], e[1][1] - e[0][1]];
            let new = [segment[1][0] - segment[0][0], segment[1][1] - segment[0][1]];
            if old[0] * new[0] + old[1] * new[1] <= 0. {
                return None;
            }
            let mid = [
                (segment[0][0] + segment[1][0]) * 0.5,
                (segment[0][1] + segment[1][1]) * 0.5,
            ];
            filled(edges, mid, rule).then_some(segment)
        })
        .collect::<Option<Vec<_>>>()?;
    let (split, _) = intersections(&result)?;
    if split.len() != result.len() {
        return None;
    }
    Some(result)
}
fn build(
    object: &VectorObject,
    bounds: [f32; 4],
    depth: f32,
    requested_bevel: f32,
) -> Option<Mesh> {
    let width = bounds[2] - bounds[0];
    let height = bounds[3] - bounds[1];
    if width <= 0. || height <= 0. || !depth.is_finite() {
        return None;
    }
    let center = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    let mut edges = Vec::new();
    let mut groups: Vec<([f32; 4], Vec<Edge>)> = Vec::new();
    let [a, b, c, d, e, f] = object.transform;
    let contours = crate::bezier::cubic_contours(&object.path.data).ok()?;
    for (points, closed) in contours {
        if !closed {
            return None;
        }
        // Reject huge curves before flattened() can expand them.
        if points.len() > 4096
            || points
                .windows(2)
                .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                .sum::<f32>()
                > 8192.
        {
            return None;
        }
        let mapped = points
            .iter()
            .map(|[x, y]| [a * x + c * y + e - center[0], b * x + d * y + f - center[1]])
            .collect::<Vec<_>>();
        let mut polygon = vec![*mapped.first()?];
        let mut start = mapped[0];
        for curve in mapped[1..].as_chunks::<3>().0 {
            flatten_curve([start, curve[0], curve[1], curve[2]], 0, &mut polygon)?;
            start = curve[2];
        }
        if polygon.iter().flatten().any(|v| !v.is_finite()) {
            return None;
        }
        polygon.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) <= 0.0001);
        let first = *polygon.first()?;
        if let Some(last) = polygon.last_mut() {
            if (last[0] - first[0]).hypot(last[1] - first[1]) <= 0.0001 {
                *last = first;
            }
        }
        polygon.push(first);
        let contour = polygon
            .windows(2)
            .filter(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]) > 0.0001)
            .map(|w| [w[0], w[1]])
            .collect::<Vec<_>>();
        if requested_bevel > 0. && !contour.is_empty() {
            let mut bounds = [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ];
            for p in contour.iter().flatten() {
                bounds[0] = bounds[0].min(p[0]);
                bounds[1] = bounds[1].min(p[1]);
                bounds[2] = bounds[2].max(p[0]);
                bounds[3] = bounds[3].max(p[1]);
            }
            groups.push((bounds, contour.clone()));
        }
        edges.extend(contour);
        if edges.len() > MAX_EDGES {
            return None;
        }
    }
    if edges.is_empty() {
        return None;
    }
    // Keep counters and overlapping contours together, but do not let a sharp
    // corner in one letter reduce the bevel of every other letter in the word.
    if requested_bevel > 0. && groups.len() > 1 {
        let mut merged: Vec<([f32; 4], Vec<Edge>)> = vec![];
        for (mut box_, mut contour) in groups {
            loop {
                let overlap = merged.iter().position(|(b, _)| {
                    box_[0] <= b[2] && box_[2] >= b[0] && box_[1] <= b[3] && box_[3] >= b[1]
                });
                let Some(i) = overlap else {
                    break;
                };
                let (b, other) = merged.swap_remove(i);
                box_ = [
                    box_[0].min(b[0]),
                    box_[1].min(b[1]),
                    box_[2].max(b[2]),
                    box_[3].max(b[3]),
                ];
                contour.extend(other);
            }
            merged.push((box_, contour));
        }
        if merged.len() > 1 {
            let mut mesh = Mesh {
                vertices: vec![],
                indices: vec![],
                cull_back: true,
            };
            for (_, edges) in merged {
                let part = build_edges(object, bounds, depth, requested_bevel, edges)?;
                if mesh.vertices.len() + part.vertices.len() > MAX_VERTICES {
                    return None;
                }
                let offset = mesh.vertices.len() as u32;
                mesh.vertices.extend(part.vertices);
                mesh.indices
                    .extend(part.indices.into_iter().map(|i| i + offset));
            }
            return Some(mesh);
        }
    }
    build_edges(object, bounds, depth, requested_bevel, edges)
}
fn build_edges(
    object: &VectorObject,
    bounds: [f32; 4],
    depth: f32,
    requested_bevel: f32,
    mut edges: Vec<Edge>,
) -> Option<Mesh> {
    let width = bounds[2] - bounds[0];
    let height = bounds[3] - bounds[1];
    let mut mesh = Mesh {
        vertices: vec![],
        indices: vec![],
        cull_back: true,
    };
    let rule = object.path.fill_rule;
    let uv = |[x, y]: Point| [(x + width * 0.5) / width, (y + height * 0.5) / height];
    let original_edges = edges.clone();
    let mut bevel = requested_bevel.max(0.).min(depth * 0.499);
    let mut inset_edges = None;
    if bevel >= 0.001 && intersections(&edges)?.0.len() == edges.len() {
        for _ in 0..12 {
            if bevel < 0.001 {
                break;
            }
            if let Some(value) = inset(&edges, rule, bevel) {
                inset_edges = Some(value);
                break;
            }
            bevel *= 0.5;
        }
    }
    if let Some(value) = &inset_edges {
        edges = value.clone();
    } else {
        bevel = 0.;
    }
    let mut levels = edges.iter().flatten().map(|p| p[1]).collect::<Vec<_>>();
    let (boundaries, crossing_levels) = intersections(&edges)?;
    levels.extend(crossing_levels);
    levels.sort_by(f32::total_cmp);
    levels.dedup_by(|a, b| (*a - *b).abs() < 0.0001);
    for y in levels.windows(2) {
        let middle = (y[0] + y[1]) * 0.5;
        let mut active = edges
            .iter()
            .copied()
            .filter(|[a, b]| a[1].min(b[1]) < middle && a[1].max(b[1]) > middle)
            .collect::<Vec<_>>();
        active.sort_by(|a, b| x_at(*a, middle).total_cmp(&x_at(*b, middle)));
        // Remaining order reversals indicate numerically unresolved topology.
        if active.windows(2).any(|w| {
            x_at(w[0], y[0]) > x_at(w[1], y[0]) + 0.001
                || x_at(w[0], y[1]) > x_at(w[1], y[1]) + 0.001
        }) {
            return None;
        }
        let mut winding = 0;
        for pair in active.windows(2) {
            winding += if pair[0][1][1] > pair[0][0][1] { 1 } else { -1 };
            let inside = match rule {
                FillRule::EvenOdd => winding % 2 != 0,
                FillRule::NonZero => winding != 0,
            };
            if !inside {
                continue;
            }
            let p = [
                [x_at(pair[0], y[0]), y[0]],
                [x_at(pair[1], y[0]), y[0]],
                [x_at(pair[1], y[1]), y[1]],
                [x_at(pair[0], y[1]), y[1]],
            ];
            let front = p.map(|[x, y]| [x, y, 0.]);
            let u = p.map(uv);
            face(&mut mesh, &front, &u, [0., 0., 1.], None)?;
            let back = [p[3], p[2], p[1], p[0]];
            face(
                &mut mesh,
                &back.map(|[x, y]| [x, y, -depth]),
                &back.map(uv),
                [0., 0., -1.],
                None,
            )?;
        }
    }
    let paint = object.fill?;
    let alpha = paint.color[3] as f32 / 255. * object.opacity;
    let color = Some([
        paint.color[0] as f32 / 255. * alpha,
        paint.color[1] as f32 / 255. * alpha,
        paint.color[2] as f32 / 255. * alpha,
        alpha,
    ]);
    let mut seen = std::collections::HashSet::new();
    let wall_boundaries = if bevel > 0. {
        original_edges.clone()
    } else {
        boundaries
    };
    for (wall_index, &[a, b]) in wall_boundaries.iter().enumerate() {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = dx.hypot(dy);
        if length < 0.0001 {
            continue;
        }
        let mut endpoints = [
            a.map(|v| (v as f64 * 10000.).round() as i64),
            b.map(|v| (v as f64 * 10000.).round() as i64),
        ];
        endpoints.sort();
        if !seen.insert(endpoints) {
            continue;
        }
        let mut n = [dy / length, -dx / length, 0.];
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let epsilon = 0.01;
        let plus = filled(
            &original_edges,
            [mid[0] + n[0] * epsilon, mid[1] + n[1] * epsilon],
            rule,
        );
        let minus = filled(
            &original_edges,
            [mid[0] - n[0] * epsilon, mid[1] - n[1] * epsilon],
            rule,
        );
        if plus == minus {
            continue;
        } // Internal overlapping contours do not create walls.
        if plus {
            n[0] = -n[0];
            n[1] = -n[1];
        }
        face(
            &mut mesh,
            &[
                [a[0], a[1], -bevel],
                [b[0], b[1], -bevel],
                [b[0], b[1], -depth + bevel],
                [a[0], a[1], -depth + bevel],
            ],
            &[[0.; 2]; 4],
            n,
            color,
        )?;
        if let Some(inset) = &inset_edges {
            let [u, v] = inset[wall_index];
            for (z, wall_z, sign) in [(0., -bevel, 1.), (-depth, -depth + bevel, -1.)] {
                face(
                    &mut mesh,
                    &[
                        [u[0], u[1], z],
                        [v[0], v[1], z],
                        [b[0], b[1], wall_z],
                        [a[0], a[1], wall_z],
                    ],
                    &[[0.; 2]; 4],
                    [
                        n[0] * std::f32::consts::FRAC_1_SQRT_2,
                        n[1] * std::f32::consts::FRAC_1_SQRT_2,
                        sign * std::f32::consts::FRAC_1_SQRT_2,
                    ],
                    color,
                )?;
            }
        }
    }
    (!mesh.indices.is_empty()).then_some(mesh)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn shape(data: &str, rule: FillRule) -> VectorObject {
        let mut o: VectorObject =
            serde_json::from_value(serde_json::json!({"id":"glyph","name":"Glyph","path":{"data":"M0 0H100V100H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":{"color":[20,60,180,255]},"stroke":null,"strokeWidth":0,"visible":true})).unwrap();
        o.path.data = data.into();
        o.path.fill_rule = rule;
        o
    }
    #[test]
    fn closed_extrusion_has_hole_caps_inner_walls_and_reuses_geometry() {
        let data = "M0 0 L100 0 L100 100 L0 100 Z M30 30 L30 70 L70 70 L70 30 Z";
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let o = shape(data, rule);
            let mesh = cached(&o, [0., 0., 100., 100.], 20.).unwrap();
            assert!(mesh.vertices.iter().any(|v| v.normal == [0., 0., -1.]));
            assert!(mesh
                .vertices
                .iter()
                .any(|v| v.color.is_some() && v.position[0].abs() == 20.));
            let mut area = 0.;
            for t in mesh.indices.as_chunks::<3>().0 {
                let v = t.map(|i| &mesh.vertices[i as usize]);
                if v.iter().all(|v| v.normal == [0., 0., 1.]) {
                    let [a, b, c] = v.map(|v| v.position);
                    area +=
                        ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5;
                }
            }
            assert!(
                (area - 8400.).abs() < 0.1,
                "cap must preserve the counter: {area}"
            );
            assert!(Arc::ptr_eq(
                &mesh,
                &cached(&o, [0., 0., 100., 100.], 20.).unwrap()
            ));
        }
        assert!(cached(
            &shape("M0 0 L20 0", FillRule::NonZero),
            [0., 0., 100., 100.],
            20.
        )
        .is_none());
        let bow = cached(
            &shape("M0 0 L100 100 L0 100 L100 0 Z", FillRule::EvenOdd),
            [0., 0., 100., 100.],
            20.,
        )
        .unwrap();
        assert!((cap_area(&bow) - 5000.).abs() < 0.1);
    }
    fn cap_area(mesh: &Mesh) -> f32 {
        mesh.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let v = t.map(|i| &mesh.vertices[i as usize]);
                if !v.iter().all(|v| v.normal == [0., 0., 1.]) {
                    return 0.;
                }
                let [a, b, c] = v.map(|v| v.position);
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
            })
            .sum()
    }
    #[test]
    fn bevel_shrinks_caps_preserves_holes_and_reduces_narrow_strokes() {
        let o = shape("M0 0H100V100H0Z M30 30V70H70V30Z", FillRule::NonZero);
        let mesh = cached_beveled(&o, [0., 0., 100., 100.], 20., 4.).unwrap();
        // The exterior moves inward and the counter expands by the same width.
        assert!((cap_area(&mesh) - (92. * 92. - 48. * 48.)).abs() < 0.1);
        for sign in [-1., 1.] {
            assert!(mesh.vertices.iter().any(|v| v.color.is_some()
                && (v.normal[2] - sign * std::f32::consts::FRAC_1_SQRT_2).abs() < 0.001));
        }
        assert!(Arc::ptr_eq(
            &mesh,
            &cached_beveled(&o, [0., 0., 100., 100.], 20., 4.).unwrap()
        ));
        assert!(!Arc::ptr_eq(
            &mesh,
            &cached(&o, [0., 0., 100., 100.], 20.).unwrap()
        ));
        let thin = cached_beveled(
            &shape("M0 0H4V100H0Z", FillRule::NonZero),
            [0., 0., 4., 100.],
            20.,
            8.,
        )
        .unwrap();
        assert!(cap_area(&thin) > 0.);
        assert!(thin
            .vertices
            .iter()
            .any(|v| v.normal[2] > 0. && v.color.is_some()));
        assert!(thin
            .vertices
            .iter()
            .filter(|v| v.normal == [0., 0., 1.])
            .all(|v| v.position[0].abs() <= 1.01));
        let separate = cached_beveled(
            &shape("M0 0H100V100H0Z M120 0H124V100H120Z", FillRule::NonZero),
            [0., 0., 124., 100.],
            20.,
            4.,
        )
        .unwrap();
        assert!(
            (cap_area(&separate) - (92. * 92. + 2. * 98.)).abs() < 0.1,
            "a narrow contour must not reduce the bevel of a separate contour"
        );
        let bow = shape("M0 0L100 100L0 100L100 0Z", FillRule::EvenOdd);
        let fallback = cached_beveled(&bow, [0., 0., 100., 100.], 20., 4.).unwrap();
        assert!((cap_area(&fallback) - 5000.).abs() < 0.1);
        assert!(!fallback
            .vertices
            .iter()
            .any(|v| v.normal[2].abs() > 0. && v.color.is_some()));
        assert!(cached_beveled(&o, [0., 0., 100., 100.], 20., f32::NAN).is_none());
    }
    #[test]
    fn overlapping_boundaries_obey_fill_rule_without_internal_walls() {
        let path = "M0 0H100V100H0Z M50 0H150V100H50Z";
        for (rule, area, perimeter) in [
            (FillRule::NonZero, 15000., 500.),
            (FillRule::EvenOdd, 10000., 600.),
        ] {
            let mesh = cached(&shape(path, rule), [0., 0., 150., 100.], 20.).unwrap();
            assert!((cap_area(&mesh) - area).abs() < 0.1);
            let length: f32 = mesh
                .vertices
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|v| v[0].color.is_some())
                .map(|v| {
                    (v[1].position[0] - v[0].position[0]).hypot(v[1].position[1] - v[0].position[1])
                })
                .sum();
            assert!((length - perimeter).abs() < 0.1, "{length} != {perimeter}");
        }
    }
}
