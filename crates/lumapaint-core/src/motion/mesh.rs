//! True 3D triangle geometry, with near-plane clipping and perspective UVs.
//! The native renderer keeps source textures on the GPU between frames.
use super::{Clip, Operation, Preset};
use std::fmt::Write;
#[derive(Clone, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [f32; 3],
    pub color: Option<[f32; 4]>,
}
#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub cull_back: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projected {
    pub point: [f32; 2],
    pub depth: f32,
    pub distance: f32,
    pub uv: [f32; 2],
    pub light: f32,
    pub color: Option<[f32; 4]>,
}
#[derive(Clone, Debug)]
pub struct Surface {
    pub opacity: f32,
    pub size: [f32; 2],
    pub source: String,
    pub triangles: Vec<[Projected; 3]>,
}
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub surfaces: Vec<Surface>,
}
impl Mesh {
    pub fn plane(width: f32, height: f32) -> Self {
        Self::faces(&[[
            [-width / 2., -height / 2., 0.],
            [width / 2., -height / 2., 0.],
            [width / 2., height / 2., 0.],
            [-width / 2., height / 2., 0.],
        ]])
    }
    pub fn cuboid(width: f32, height: f32, depth: f32) -> Self {
        let (x, y, z) = (width / 2., height / 2., depth / 2.);
        Self::faces(&[
            [[-x, -y, z], [x, -y, z], [x, y, z], [-x, y, z]],
            [[x, -y, -z], [-x, -y, -z], [-x, y, -z], [x, y, -z]],
            [[x, -y, z], [x, -y, -z], [x, y, -z], [x, y, z]],
            [[-x, -y, -z], [-x, -y, z], [-x, y, z], [-x, y, -z]],
            [[-x, -y, -z], [x, -y, -z], [x, -y, z], [-x, -y, z]],
            [[-x, y, z], [x, y, z], [x, y, -z], [-x, y, -z]],
        ])
    }
    pub fn cylinder(width: f32, height: f32) -> Self {
        let radius = width / std::f32::consts::PI;
        let mut mesh = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            cull_back: true,
        };
        for i in 0..=32 {
            let u = i as f32 / 32.;
            let angle = (u - 0.5) * std::f32::consts::TAU;
            for (y, v) in [(-height * 0.5, 0.), (height * 0.5, 1.)] {
                mesh.vertices.push(Vertex {
                    position: [angle.sin() * radius, y, angle.cos() * radius - radius],
                    uv: [u, v],
                    normal: [angle.sin(), 0., angle.cos()],
                    color: None,
                });
            }
            if i > 0 {
                let b = (i - 1) * 2;
                mesh.indices.extend([b, b + 2, b + 1, b + 1, b + 2, b + 3]);
            }
        }
        mesh
    }
    fn faces(faces: &[[[f32; 3]; 4]]) -> Self {
        let mut mesh = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            cull_back: faces.len() > 1,
        };
        for face in faces {
            let a = sub(face[1], face[0]);
            let b = sub(face[2], face[0]);
            let n = cross(a, b);
            let length = dot(n, n).sqrt().max(1e-6);
            let n = n.map(|v| v / length);
            let base = mesh.vertices.len() as u32;
            for (position, uv) in face.iter().zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]]) {
                mesh.vertices.push(Vertex {
                    position: *position,
                    uv,
                    normal: n,
                    color: None,
                });
            }
            mesh.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        mesh
    }
    pub fn project(
        &self,
        rotation: [f32; 3],
        translation: [f32; 3],
        center: [f32; 2],
        camera: f32,
    ) -> Vec<[Projected; 3]> {
        let vertices = self
            .vertices
            .iter()
            .map(|v| Vertex {
                position: add(rotate(v.position, rotation), translation),
                normal: rotate(v.normal, rotation),
                uv: v.uv,
                color: v.color,
            })
            .collect::<Vec<_>>();
        let mut triangles = Vec::new();
        for indices in self.indices.as_chunks::<3>().0 {
            if self.cull_back {
                let v = &vertices[indices[0] as usize];
                if dot(v.normal, sub([0., 0., camera], v.position)) <= 0. {
                    continue;
                }
            }
            let polygon = near_clip(
                &[
                    vertices[indices[0] as usize].clone(),
                    vertices[indices[1] as usize].clone(),
                    vertices[indices[2] as usize].clone(),
                ],
                camera - 8.,
            );
            for i in 1..polygon.len().saturating_sub(1) {
                let triangle = [&polygon[0], &polygon[i], &polygon[i + 1]].map(|v| {
                    let distance = (camera - v.position[2]).max(8.);
                    let scale = camera / distance;
                    Projected {
                        point: [
                            center[0] + v.position[0] * scale,
                            center[1] + v.position[1] * scale,
                        ],
                        depth: distance / (distance + camera),
                        distance,
                        uv: v.uv,
                        color: v.color,
                        light: 0.45 + 0.55 * dot(v.normal, [0.25, -0.35, 0.9]).clamp(0., 1.),
                    }
                });
                if triangle.iter().all(|v| {
                    v.point
                        .iter()
                        .chain([&v.depth, &v.distance, &v.light])
                        .all(|x| x.is_finite())
                }) {
                    triangles.push(triangle);
                }
            }
        }
        triangles.sort_by(|a, b| {
            b.iter()
                .map(|v| v.distance)
                .sum::<f32>()
                .total_cmp(&a.iter().map(|v| v.distance).sum::<f32>())
        });
        triangles
    }
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn rotate([x, y, z]: [f32; 3], angles: [f32; 3]) -> [f32; 3] {
    let [(sx, cx), (sy, cy), (sz, cz)] = angles.map(|a| a.to_radians().sin_cos());
    let (y, z) = (y * cx - z * sx, y * sx + z * cx);
    let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
    [x * cz - y * sz, x * sz + y * cz, z]
}
fn near_clip(vertices: &[Vertex], limit: f32) -> Vec<Vertex> {
    let mut out = Vec::new();
    let mut previous = vertices.last().unwrap();
    for current in vertices {
        let a = previous.position[2] <= limit;
        let b = current.position[2] <= limit;
        if a != b {
            let t = (limit - previous.position[2]) / (current.position[2] - previous.position[2]);
            out.push(Vertex {
                position: std::array::from_fn(|i| {
                    previous.position[i] + t * (current.position[i] - previous.position[i])
                }),
                uv: std::array::from_fn(|i| previous.uv[i] + t * (current.uv[i] - previous.uv[i])),
                normal: previous.normal,
                color: previous.color,
            });
        }
        if b {
            out.push(current.clone());
        }
        previous = current;
    }
    out
}
pub fn enabled(preset: &Preset) -> bool {
    preset.operations.iter().any(|op| {
        matches!(
            op,
            Operation::MeshPlane | Operation::Cube | Operation::DepthStack | Operation::Coverflow
        )
    })
}
#[allow(clippy::too_many_arguments)]
pub fn surface(
    source: String,
    bounds: [f32; 4],
    clip: &Clip,
    preset: &Preset,
    p: f32,
    index: usize,
    count: usize,
    fps: u32,
) -> Surface {
    surface_inner(source, bounds, clip, preset, p, index, count, fps, None)
}
#[allow(clippy::too_many_arguments)]
pub fn surface_for_object(
    source: String,
    bounds: [f32; 4],
    clip: &Clip,
    preset: &Preset,
    p: f32,
    index: usize,
    count: usize,
    fps: u32,
    object: &crate::vector::VectorObject,
) -> Surface {
    surface_inner(
        source,
        bounds,
        clip,
        preset,
        p,
        index,
        count,
        fps,
        Some(object),
    )
}
#[allow(clippy::too_many_arguments)]
fn surface_inner(
    source: String,
    bounds: [f32; 4],
    clip: &Clip,
    preset: &Preset,
    p: f32,
    index: usize,
    count: usize,
    fps: u32,
    object: Option<&crate::vector::VectorObject>,
) -> Surface {
    let width = (bounds[2] - bounds[0]).max(1.);
    let height = (bounds[3] - bounds[1]).max(1.);
    let center = [
        bounds[0] + width * clip.pivot[0],
        bounds[1] + height * clip.pivot[1],
    ];
    let mut rotation = [clip.rotation_x, clip.rotation_y, clip.rotation_z];
    let mut translation = [
        width * (0.5 - clip.pivot[0]),
        height * (0.5 - clip.pivot[1]),
        0.,
    ];
    let q = clip
        .curve
        .unwrap_or(preset.curve)
        .sample(p, p * clip.duration as f32 / fps as f32, clip.seed)
        .clamp(0., 1.);
    if matches!(preset.id.as_str(), "tx-layered-depth" | "tx-motext-falloff") {
        rotation[0] +=
            (std::f32::consts::TAU * p * clip.frequency).sin() * clip.angle * 0.1 * clip.strength;
        rotation[1] +=
            (std::f32::consts::TAU * p * clip.frequency).cos() * clip.angle * 0.1 * clip.strength;
    }
    let mesh = if preset.operations.contains(&Operation::Cube) {
        rotation[1] += clip.angle * p * clip.strength;
        rotation[0] += clip.angle * p * 0.37 * clip.strength;
        Mesh::cuboid(width, height, clip.mesh_depth.max(1.))
    } else {
        if preset.operations.contains(&Operation::DepthStack) {
            translation[2] = if preset.id.contains("fly-in") {
                -(clip.mesh_depth + clip.distance * 3.) * (1. - q) * clip.strength
            } else if matches!(preset.id.as_str(), "tx-layered-depth" | "tx-motext-falloff") {
                0.
            } else {
                (index as f32 - count.saturating_sub(1) as f32) * clip.mesh_depth
                    + q * clip.distance * clip.strength
            };
        } else if preset.operations.contains(&Operation::Coverflow) {
            let offset = index as f32 - (count.saturating_sub(1) as f32 * 0.5);
            rotation[1] += offset.signum() * clip.angle * (1. - q);
            translation[2] = -offset.abs() * clip.mesh_depth;
            translation[0] += offset * clip.distance * (1. - q);
        } else {
            let axis = if preset.id.contains("up")
                || preset.id.contains("fold")
                || preset.id.contains("flipX")
            {
                0
            } else {
                1
            };
            rotation[axis] += (1. - q)
                * clip.angle
                * clip.strength
                * if preset.id.contains("flip-up") {
                    -0.5
                } else {
                    1.
                };
        }
        if preset.id == "tx-cylinder-text" {
            rotation[1] += p * 360. * clip.frequency;
            Mesh::cylinder(width, height)
        } else {
            Mesh::plane(width, height)
        }
    };
    let extrusion = object
        .filter(|_| {
            clip.extruded && matches!(preset.id.as_str(), "tx-layered-depth" | "tx-motext-falloff")
        })
        .and_then(|object| super::extrude::cached(object, bounds, clip.mesh_depth));
    let mut triangles = extrusion.as_deref().unwrap_or(&mesh).project(
        rotation,
        translation,
        center,
        clip.camera_distance,
    );
    if extrusion.is_none() && matches!(preset.id.as_str(), "tx-layered-depth" | "tx-motext-falloff")
    {
        for copy in 1..=clip.copies {
            let mut offset = translation;
            offset[2] -= clip.mesh_depth * copy as f32 / clip.copies as f32;
            let mut extra = mesh.project(rotation, offset, center, clip.camera_distance);
            for vertex in extra.iter_mut().flatten() {
                vertex.light *= 1. - 0.4 * copy as f32 / clip.copies as f32;
            }
            triangles.extend(extra);
        }
        triangles.sort_by(|a, b| {
            b.iter()
                .map(|v| v.distance)
                .sum::<f32>()
                .total_cmp(&a.iter().map(|v| v.distance).sum::<f32>())
        });
    }
    // Crop the stable texture to its original bounds. No animation transform is
    // included in this source, allowing exact reuse across frames on the GPU.
    let source=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {width} {height}\">{source}</svg>",bounds[0],bounds[1]);
    Surface {
        opacity: if preset.id.contains("fly-in") || preset.id.contains("flip-up") {
            q
        } else {
            1.
        },
        size: [width, height],
        source,
        triangles,
    }
}
// A midpoint in texture/object space projects with distance-weighted screen
// coordinates. Splitting there reduces the affine fallback's projective error.
fn midpoint(a: Projected, b: Projected) -> Projected {
    let distance = (a.distance + b.distance) * 0.5;
    let camera = a.distance * (1. - a.depth) / a.depth;
    Projected {
        point: std::array::from_fn(|i| {
            (a.point[i] * a.distance + b.point[i] * b.distance) / (a.distance + b.distance)
        }),
        uv: std::array::from_fn(|i| (a.uv[i] + b.uv[i]) * 0.5),
        light: (a.light + b.light) * 0.5,
        depth: distance / (distance + camera),
        distance,
        color: a.color,
    }
}
fn refine(triangle: [Projected; 3], level: u8, budget: &mut usize, out: &mut Vec<[Projected; 3]>) {
    let edges = [(0, 1, 2), (1, 2, 0), (2, 0, 1)];
    let (edge, error) = edges
        .iter()
        .map(|&(a, b, c)| {
            let m = midpoint(triangle[a], triangle[b]);
            let error = (m.point[0] - (triangle[a].point[0] + triangle[b].point[0]) * 0.5)
                .hypot(m.point[1] - (triangle[a].point[1] + triangle[b].point[1]) * 0.5);
            ((a, b, c, m), error)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    if triangle[0].color.is_none() && error > 0.35 && level < 6 && *budget > 0 {
        *budget -= 1;
        let (a, b, c, m) = edge;
        refine([triangle[a], m, triangle[c]], level + 1, budget, out);
        refine([m, triangle[b], triangle[c]], level + 1, budget, out);
    } else {
        out.push(triangle);
    }
}
/// Bounded adaptive UV tessellation and encoded-sRGB lighting for portable SVG.
/// Native display retains perspective-correct interpolation and depth testing.
pub fn svg(surface: &Surface, tag: &str) -> String {
    let mut texture = surface
        .source
        .replacen("<svg ", "<svg preserveAspectRatio=\"none\" ", 1);
    for key in ["width", "height"] {
        let start = texture.find(&format!("{key}=\"")).unwrap() + key.len() + 2;
        let end = start + texture[start..].find('"').unwrap();
        texture.replace_range(start..end, "1");
    }
    let mut out = format!("<defs><g id=\"{tag}-texture\">{texture}</g></defs>");
    let mut triangles = Vec::new();
    let mut budget = 8192usize.saturating_sub(surface.triangles.len());
    for triangle in &surface.triangles {
        refine(*triangle, 0, &mut budget, &mut triangles);
    }
    let mut lights = std::collections::HashSet::new();
    for (i, t) in triangles.iter().enumerate() {
        if let Some(color) = t[0].color {
            let light = t.iter().map(|v| v.light).sum::<f32>() / 3.;
            let alpha = color[3];
            let rgb = color[..3]
                .iter()
                .map(|c| {
                    (c / alpha.max(0.000001) * light * 255.)
                        .round()
                        .clamp(0., 255.) as u8
                })
                .collect::<Vec<_>>();
            let _=write!(out,"<polygon points=\"{},{} {},{} {},{}\" fill=\"rgb({},{},{})\" fill-opacity=\"{alpha}\"/>",t[0].point[0],t[0].point[1],t[1].point[0],t[1].point[1],t[2].point[0],t[2].point[1],rgb[0],rgb[1],rgb[2]);
            continue;
        }
        let uv = t.map(|v| v.uv);
        let p = t.map(|v| v.point);
        let det = (uv[1][0] - uv[0][0]) * (uv[2][1] - uv[0][1])
            - (uv[2][0] - uv[0][0]) * (uv[1][1] - uv[0][1]);
        if det.abs() < 1e-6 {
            continue;
        }
        let values = std::array::from_fn::<_, 6, _>(|k| {
            let axis = k % 2;
            if k < 4 {
                let d1 = p[1][axis] - p[0][axis];
                let d2 = p[2][axis] - p[0][axis];
                if k < 2 {
                    (d1 * (uv[2][1] - uv[0][1]) - d2 * (uv[1][1] - uv[0][1])) / det
                } else {
                    ((uv[1][0] - uv[0][0]) * d2 - (uv[2][0] - uv[0][0]) * d1) / det
                }
            } else {
                0.
            }
        });
        let [a, b, c, d, _, _] = values;
        let e = p[0][0] - a * uv[0][0] - c * uv[0][1];
        let f = p[0][1] - b * uv[0][0] - d * uv[0][1];
        let light = (t.iter().map(|v| v.light).sum::<f32>() / 3. * 1000.).round() as u32;
        if lights.insert(light) {
            let value = light as f32 / 1000.;
            let _=write!(out,"<filter id=\"{tag}-light-{light}\" color-interpolation-filters=\"sRGB\"><feComponentTransfer><feFuncR type=\"linear\" slope=\"{value}\"/><feFuncG type=\"linear\" slope=\"{value}\"/><feFuncB type=\"linear\" slope=\"{value}\"/></feComponentTransfer></filter>");
        }
        let _=write!(out,"<defs><clipPath id=\"{tag}-{i}\"><polygon points=\"{},{} {},{} {},{}\"/></clipPath></defs><g clip-path=\"url(#{tag}-{i})\"><use href=\"#{tag}-texture\" filter=\"url(#{tag}-light-{light})\" transform=\"matrix({a} {b} {c} {d} {e} {f})\"/></g>",p[0][0],p[0][1],p[1][0],p[1][1],p[2][0],p[2][1]);
    }
    format!("<g opacity=\"{}\">{out}</g>", surface.opacity)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_uv_refinement_is_bounded_and_uses_projected_midpoints() {
        let coarse = Mesh::plane(120., 80.).project([0., 65., 0.], [0.; 3], [100., 100.], 180.);
        let mut result = vec![];
        let mut budget = 8192 - coarse.len();
        for t in &coarse {
            refine(*t, 0, &mut budget, &mut result);
        }
        assert!(result.len() > coarse.len() && result.len() <= 8192);
        let error = |triangles: &[[Projected; 3]]| {
            triangles
                .iter()
                .flat_map(|t| {
                    [(0, 1), (1, 2), (2, 0)].map(|(a, b)| {
                        let m = midpoint(t[a], t[b]);
                        (m.point[0] - (t[a].point[0] + t[b].point[0]) * 0.5)
                            .hypot(m.point[1] - (t[a].point[1] + t[b].point[1]) * 0.5)
                    })
                })
                .fold(0., f32::max)
        };
        assert!(error(&result) < error(&coarse) * 0.1);
        let mut budget = 1;
        let mut limited = vec![];
        refine(coarse[0], 0, &mut budget, &mut limited);
        assert_eq!(limited.len(), 2);
    }
    #[test]
    fn mesh_topology_perspective_and_near_clip() {
        let mesh = Mesh::cuboid(100., 80., 60.);
        assert_eq!(mesh.vertices.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
        let a = mesh.project([20., 35., 0.], [0.; 3], [50., 50.], 200.);
        assert!(a.len() >= 2 && a.len() <= 6);
        assert!(a.iter().flatten().all(|v| v.depth > 0. && v.depth < 1.));
        let b = Mesh::plane(100., 80.).project([20., 80., 0.], [0., 0., 190.], [50., 50.], 200.);
        assert!(!b.is_empty());
        assert!(b.iter().flatten().all(|v| v.distance >= 8.));
        assert_ne!(a, b);
    }
}
