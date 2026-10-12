//! True 3D triangle geometry, with near-plane clipping and perspective UVs.
//! The native renderer keeps source textures on the GPU between frames.
use super::{Clip, Operation, Preset};
use std::fmt::Write;
#[derive(Clone, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projected {
    pub point: [f32; 2],
    pub depth: f32,
    pub distance: f32,
    pub uv: [f32; 2],
    pub light: f32,
}
#[derive(Clone, Debug)]
pub struct Surface {
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
    fn faces(faces: &[[[f32; 3]; 4]]) -> Self {
        let mut mesh = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
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
            })
            .collect::<Vec<_>>();
        let mut triangles = Vec::new();
        for indices in self.indices.chunks_exact(3) {
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
                        light: 0.45 + 0.55 * dot(v.normal, [0.25, -0.35, 0.9]).max(0.).min(1.),
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
    let mesh = if preset.operations.contains(&Operation::Cube) {
        rotation[1] += clip.angle * p * clip.strength;
        rotation[0] += clip.angle * p * 0.37 * clip.strength;
        Mesh::cuboid(width, height, clip.mesh_depth.max(1.))
    } else {
        if preset.operations.contains(&Operation::DepthStack) {
            translation[2] = (index as f32 - count.saturating_sub(1) as f32) * clip.mesh_depth
                + q * clip.distance * clip.strength;
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
            rotation[axis] += (1. - q) * clip.angle * clip.strength;
        }
        Mesh::plane(width, height)
    };
    let triangles = mesh.project(rotation, translation, center, clip.camera_distance);
    // Crop the stable texture to its original bounds. No animation transform is
    // included in this source, allowing exact reuse across frames on the GPU.
    let source=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {width} {height}\">{source}</svg>",bounds[0],bounds[1]);
    Surface {
        size: [width, height],
        source,
        triangles,
    }
}
/// Portable raster/export fallback, tessellating perspective UV interpolation.
/// Native display uses the original triangles and perspective-correct GPU UVs.
pub fn svg(surface: &Surface, tag: &str) -> String {
    let mut texture = surface.source.clone();
    for key in ["width", "height"] {
        let start = texture.find(&format!("{key}=\"")).unwrap() + key.len() + 2;
        let end = start + texture[start..].find('"').unwrap();
        texture.replace_range(start..end, "1");
    }
    let mut out = format!("<defs><g id=\"{tag}-texture\">{texture}</g></defs>");
    for (i, t) in surface.triangles.iter().enumerate() {
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
        let _=write!(out,"<defs><clipPath id=\"{tag}-{i}\"><polygon points=\"{},{} {},{} {},{}\"/></clipPath></defs><g clip-path=\"url(#{tag}-{i})\"><use href=\"#{tag}-texture\" transform=\"matrix({a} {b} {c} {d} {e} {f})\"/></g>",p[0][0],p[0][1],p[1][0],p[1][1],p[2][0],p[2][1]);
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mesh_topology_perspective_and_near_clip() {
        let mesh = Mesh::cuboid(100., 80., 60.);
        assert_eq!(mesh.vertices.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
        let a = mesh.project([20., 35., 0.], [0.; 3], [50., 50.], 200.);
        assert_eq!(a.len(), 12);
        assert!(a.iter().flatten().all(|v| v.depth > 0. && v.depth < 1.));
        let b = mesh.project([20., 80., 0.], [0., 0., 190.], [50., 50.], 200.);
        assert!(!b.is_empty());
        assert!(b.iter().flatten().all(|v| v.distance >= 8.));
        assert_ne!(a, b);
    }
}
