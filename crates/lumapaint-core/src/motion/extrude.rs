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
pub fn cached(object: &VectorObject, bounds: [f32; 4], depth: f32) -> Option<Arc<Mesh>> {
    if object.text.is_some()
        || object.fill.is_none()
        || object.blend_mode != "normal"
        || depth <= 0.
    {
        return None;
    }
    let key = format!(
        "{}:{:?}:{:?}:{:?}:{:?}:{:?}:{}",
        object.path.data,
        object.path.fill_rule,
        object.transform.map(f32::to_bits),
        bounds.map(f32::to_bits),
        depth.to_bits(),
        object.fill,
        object.opacity.to_bits()
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
    let result = build(object, bounds, depth).map(Arc::new);
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
fn build(object: &VectorObject, bounds: [f32; 4], depth: f32) -> Option<Mesh> {
    let width = bounds[2] - bounds[0];
    let height = bounds[3] - bounds[1];
    if width <= 0. || height <= 0. || !depth.is_finite() {
        return None;
    }
    let center = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    let mut edges = Vec::new();
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
        polygon.push(*polygon.first()?);
        edges.extend(
            polygon
                .windows(2)
                .filter(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]) > 0.0001)
                .map(|w| [w[0], w[1]]),
        );
        if edges.len() > MAX_EDGES {
            return None;
        }
    }
    if edges.is_empty() {
        return None;
    }
    let mut mesh = Mesh {
        vertices: vec![],
        indices: vec![],
        cull_back: true,
    };
    let rule = object.path.fill_rule;
    let uv = |[x, y]: Point| [(x + width * 0.5) / width, (y + height * 0.5) / height];
    let mut levels = edges.iter().flatten().map(|p| p[1]).collect::<Vec<_>>();
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
        // Self-crossings inside a slab need more topology: use the established fallback.
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
    for &[a, b] in &edges {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let length = dx.hypot(dy);
        let mut n = [dy / length, -dx / length, 0.];
        let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        let epsilon = 0.01;
        let plus = filled(
            &edges,
            [mid[0] + n[0] * epsilon, mid[1] + n[1] * epsilon],
            rule,
        );
        let minus = filled(
            &edges,
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
                [a[0], a[1], 0.],
                [b[0], b[1], 0.],
                [b[0], b[1], -depth],
                [a[0], a[1], -depth],
            ],
            &[[0.; 2]; 4],
            n,
            color,
        )?;
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
        assert!(cached(
            &shape("M0 0 L100 100 L0 100 L100 0 Z", FillRule::EvenOdd),
            [0., 0., 100., 100.],
            20.
        )
        .is_none());
    }
}
