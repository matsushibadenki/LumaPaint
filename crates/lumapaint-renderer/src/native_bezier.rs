//! Bounded native cubic fill renderer. Unsupported appearance stays with Skia.
use crate::{create_layer_pipeline, Viewport};
use lumapaint_core::{
    document::SvgLayer,
    vector::{FillRule, VectorObject, VectorPath},
};
use skia_safe::{Path, PathVerb};
use std::collections::{HashMap, HashSet};
use wgpu::util::DeviceExt;

pub(crate) struct Geometry {
    path: VectorPath,
    center: [f64; 2],
    bounds: [f64; 4],
    count: usize,
    cost: usize,
    _curves: wgpu::Buffer,
    uniform: wgpu::Buffer,
    indirect: wgpu::Buffer,
    binding: wgpu::BindGroup,
    last_uniform: Option<[f32; 20]>,
    last_indirect: Option<[u32; 4]>,
}
#[derive(Default, Debug)]
pub(crate) struct PreparationMetrics {
    pub live_id_scans: usize,
    pub svg_generations: usize,
    pub bvh_builds: usize,
    pub bvh_refits: usize,
    pub geometry_builds: usize,
    pub geometry_upload_bytes: u64,
    pub uniform_upload_bytes: usize,
}
pub(crate) struct Cache {
    pub pipeline: wgpu::RenderPipeline,
    pub metrics: PreparationMetrics,
    layout: wgpu::BindGroupLayout,
    pub shapes: HashMap<String, Geometry>,
    ready: HashSet<String>,
    live_cursor: Option<u64>,
    journal_id: Option<u64>,
    layer_sources: Vec<(String, usize, usize)>,
    verified: HashMap<String, VerifiedLayer>,
    draw_lists: HashMap<String, Vec<usize>>,
}
struct VerifiedLayer {
    source_key: (usize, usize),
    compatible: bool,
    spatial: lumapaint_core::scene::spatial::SpatialIndex,
    positions: HashMap<String, usize>,
    cursor: u64,
    last_refits: usize,
    journal_id: u64,
}
impl VerifiedLayer {
    /// Only a certified transform-only change can reuse SVG compatibility and
    /// geometry. All other changes return to the complete compatibility check.
    fn refresh(&mut self, layer: &SvgLayer, journal: &lumapaint_core::scene::Journal) -> bool {
        self.last_refits = 0;
        if self.journal_id != journal.instance_id() {
            return false;
        }
        let source_key = (layer.source.as_ptr() as usize, layer.source.len());
        if self.source_key == source_key && self.cursor == journal.cursor() {
            return true;
        }
        if !self.compatible || self.positions.len() != layer.vector_objects.len() {
            return false;
        }
        let lumapaint_core::scene::JournalRead::Incremental { changes, cursor } =
            journal.read(self.cursor)
        else {
            return false;
        };
        let changes: Vec<_> = changes
            .iter()
            .filter(|c| c.target.layer == layer.id)
            .collect();
        let objects: Vec<_> = changes
            .iter()
            .filter(|c| c.target.object.is_some())
            .collect();
        if changes.is_empty() && self.source_key == source_key {
            self.cursor = cursor;
            return true;
        }
        if objects.is_empty()
            || changes.iter().any(|c| {
                c.removed || c.changes.structure || c.changes.style || c.changes.visibility
            })
            || objects
                .iter()
                .any(|c| c.changes.geometry || !c.changes.transform)
        {
            return false;
        }
        for change in objects {
            let Some(&position) = self.positions.get(change.target.object.as_ref().unwrap()) else {
                return false;
            };
            let object = &layer.vector_objects[position];
            self.last_refits += 1;
            if object.id != *change.target.object.as_ref().unwrap()
                || !self.spatial.refit(position, native_drawing_bounds(object))
            {
                return false;
            }
        }
        self.source_key = source_key;
        self.cursor = cursor;
        true
    }
}
impl Cache {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Native cubic geometry"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = create_layer_pipeline(
            device,
            &[&layout],
            format,
            include_str!("native_bezier.wgsl"),
            "Native cubic analytic coverage",
        );
        Self {
            pipeline,
            metrics: PreparationMetrics::default(),
            layout,
            shapes: HashMap::new(),
            ready: HashSet::new(),
            live_cursor: None,
            journal_id: None,
            layer_sources: Vec::new(),
            verified: HashMap::new(),
            draw_lists: HashMap::new(),
        }
    }
    pub fn begin_frame(&mut self) {
        self.metrics = PreparationMetrics::default();
        self.ready.clear();
        self.draw_lists.clear();
    }
    pub fn ready(&self, layer: &SvgLayer) -> bool {
        layer.effective_opacity() == 1. && self.ready.contains(&layer.id)
    }
    pub fn synchronize(&mut self, document: &lumapaint_core::document::Document) {
        use lumapaint_core::scene::JournalRead;
        let journal_id = document.scene_journal().instance_id();
        if self.journal_id != Some(journal_id) {
            self.verified.clear();
            self.live_cursor = None;
            self.journal_id = Some(journal_id);
        }
        let sources: Vec<_> = document
            .svg_layers()
            .map(|l| (l.id.clone(), l.source.as_ptr() as usize, l.source.len()))
            .collect();
        let cursor = document.scene_journal().cursor();
        if self.live_cursor == Some(cursor) && self.layer_sources == sources {
            return;
        }
        let rebuild = match self.live_cursor {
            None => true,
            Some(previous) => match document.scene_journal().read(previous) {
                JournalRead::Rebuild { .. } => true,
                JournalRead::Incremental { changes, .. } => {
                    changes.iter().any(|c| c.removed || c.changes.structure)
                        || (changes.is_empty() && sources != self.layer_sources)
                }
            },
        };
        if rebuild {
            let live: HashSet<_> = document
                .svg_layers()
                .flat_map(|l| &l.vector_objects)
                .map(|o| o.id.clone())
                .collect();
            self.metrics.live_id_scans += live.len();
            let layers: HashSet<_> = document.svg_layers().map(|l| l.id.clone()).collect();
            self.shapes.retain(|id, _| live.contains(id));
            self.verified.retain(|id, _| layers.contains(id));
        }
        self.live_cursor = Some(cursor);
        self.layer_sources = sources;
    }
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &SvgLayer,
        viewport: Viewport,
        scene: (&lumapaint_core::scene::Journal, &[String]),
        offset: [f32; 2],
    ) -> bool {
        let _timer = crate::performance::time("native_geometry_prepare");
        let (journal, selected) = scene;
        if !layer.vector_layer || layer.effective_opacity() != 1. || layer.vector_objects.is_empty()
        {
            return false;
        }
        let current = self.verified.get_mut(&layer.id);
        let rebuild = {
            let _timer = crate::performance::time("native_journal_validate_refit");
            current.is_none_or(|entry| !entry.refresh(layer, journal))
        };
        if rebuild {
            let _timer = crate::performance::time("native_layer_verify_bvh_build");
            self.metrics.svg_generations += 1;
            self.metrics.bvh_builds += 1;
            let expected = lumapaint_core::document::vector_svg(
                viewport.document_width as u32,
                viewport.document_height as u32,
                &layer.vector_objects,
            );
            self.verified.insert(
                layer.id.clone(),
                VerifiedLayer {
                    source_key: (layer.source.as_ptr() as usize, layer.source.len()),
                    compatible: expected == layer.source
                        && layer.vector_objects.iter().all(supported),
                    spatial: lumapaint_core::scene::spatial::SpatialIndex::build(
                        layer
                            .vector_objects
                            .iter()
                            .enumerate()
                            .map(|(i, o)| (i, native_drawing_bounds(o))),
                    ),
                    positions: layer
                        .vector_objects
                        .iter()
                        .enumerate()
                        .map(|(i, o)| (o.id.clone(), i))
                        .collect(),
                    cursor: journal.cursor(),
                    last_refits: 0,
                    journal_id: journal.instance_id(),
                },
            );
        }
        self.metrics.bvh_refits += self.verified[&layer.id].last_refits;
        if !self.verified[&layer.id].compatible {
            if crate::performance::enabled() {
                for object in &layer.vector_objects {
                    if let Some(reason) = unsupported_reason(object) {
                        crate::performance::count(reason, 1);
                    }
                }
            }
            return false;
        }
        let mut candidates = {
            let _timer = crate::performance::time("native_spatial_query");
            viewport_candidates(&self.verified[&layer.id].spatial, viewport)
        };
        // A drag can bring a selected object into view from outside its stored bounds.
        if offset != [0., 0.] {
            candidates.extend(
                layer
                    .vector_objects
                    .iter()
                    .enumerate()
                    .filter(|(_, object)| selected.contains(&object.id))
                    .map(|(i, _)| i),
            );
            candidates.sort_unstable();
            candidates.dedup();
        }
        candidates.retain(|&i| layer.vector_objects[i].visible);
        let mut work = 0usize;
        for &i in &candidates {
            let o = &layer.vector_objects[i];
            let valid = self.shapes.get(&o.id).is_some_and(|g| g.path == o.path);
            if !valid {
                if self.shapes.len() >= 8192 {
                    return false;
                }
                let Some(g) = Geometry::new(device, &self.layout, o) else {
                    return false;
                };
                self.metrics.geometry_builds += 1;
                self.metrics.geometry_upload_bytes += g._curves.size();
                self.shapes.insert(o.id.clone(), g);
            }
            let g = self.shapes.get_mut(&o.id).unwrap();
            // Choose the native representation by its projected precision, not
            // zoom alone. Very large curves fall back to viewport rasterization.
            let extent = (g.bounds[2] - g.bounds[0]).max(g.bounds[3] - g.bounds[1]);
            let matrix_scale = o.transform[..4]
                .iter()
                .map(|v| f64::from(*v).powi(2))
                .sum::<f64>()
                .sqrt();
            let precision = extent
                * f64::from(f32::EPSILON)
                * 8.
                * matrix_scale
                * f64::from(viewport.screen_zoom() * viewport.scale);
            if !precision.is_finite() || precision > 0.25 {
                return false;
            }
            let previous_uniform = g.last_uniform;
            let previous_indirect = g.last_indirect;
            work = work.saturating_add(g.update(
                queue,
                o,
                viewport,
                if selected.contains(&o.id) {
                    offset
                } else {
                    [0., 0.]
                },
            ));
            if previous_uniform != g.last_uniform {
                self.metrics.uniform_upload_bytes += std::mem::size_of::<[f32; 20]>();
            }
            if previous_indirect != g.last_indirect {
                self.metrics.uniform_upload_bytes += std::mem::size_of::<[u32; 4]>();
            }
            if work > 32_000_000 {
                return false;
            }
        }
        self.draw_lists.insert(layer.id.clone(), candidates);
        self.ready.insert(layer.id.clone());
        true
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, layer: &SvgLayer) {
        pass.set_pipeline(&self.pipeline);
        for &i in self.draw_lists.get(&layer.id).into_iter().flatten() {
            if let Some(g) = self.shapes.get(&layer.vector_objects[i].id) {
                pass.set_bind_group(0, &g.binding, &[]);
                pass.draw_indirect(&g.indirect, 0);
            }
        }
    }
}
// Derive bounds from the actual rendered path, rather than editable control
// points, which can be absent for imported paths.
fn native_drawing_bounds(object: &VectorObject) -> Option<[f64; 4]> {
    let (_, _, local) = segments(object)?;
    let [a, b, c, d, e, f] = object.transform.map(f64::from);
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for x in [local[0], local[2]] {
        for y in [local[1], local[3]] {
            let point = [a * x + c * y + e, b * x + d * y + f];
            if !point.iter().all(|v| v.is_finite()) {
                return None;
            }
            bounds[0] = bounds[0].min(point[0]);
            bounds[1] = bounds[1].min(point[1]);
            bounds[2] = bounds[2].max(point[0]);
            bounds[3] = bounds[3].max(point[1]);
        }
    }
    Some(bounds)
}
fn viewport_candidates(
    index: &lumapaint_core::scene::spatial::SpatialIndex,
    viewport: Viewport,
) -> Vec<usize> {
    // The shader's analytic AA reaches beyond the exact path by a screen pixel.
    let first = viewport.document_point(-2. / viewport.scale, -2. / viewport.scale);
    let last = viewport.document_point(
        (viewport.width as f32 + 2.) / viewport.scale,
        (viewport.height as f32 + 2.) / viewport.scale,
    );
    index
        .query([
            f64::from(first.x),
            f64::from(first.y),
            f64::from(last.x),
            f64::from(last.y),
        ])
        .items
}
fn supported(o: &VectorObject) -> bool {
    unsupported_reason(o).is_none()
}

// First incompatible feature; reasons describe native eligibility, not final render routing.
fn unsupported_reason(o: &VectorObject) -> Option<&'static str> {
    let determinant = f64::from(o.transform[0]) * f64::from(o.transform[3])
        - f64::from(o.transform[1]) * f64::from(o.transform[2]);
    if o.text.is_some() {
        Some("native_ineligible.text")
    } else if o.image_frame.is_some() {
        Some("native_ineligible.image_frame")
    } else if !o.group_path.is_empty() {
        Some("native_ineligible.group")
    } else if o.clipping_group.is_some() {
        Some("native_ineligible.clip")
    } else if o.fill.is_none() {
        Some("native_ineligible.no_fill")
    } else if o.stroke.is_some() {
        Some("native_ineligible.stroke")
    } else if o.fill_gradient.is_some() || o.stroke_gradient.is_some() {
        Some("native_ineligible.gradient")
    } else if o.live_corners.is_some() || o.rectangle_radii.is_some() {
        Some("native_ineligible.live_shape")
    } else if o.blend_mode != "normal" {
        Some("native_ineligible.blend")
    } else if determinant.is_nan() || determinant.abs() <= 1e-12 {
        Some("native_ineligible.singular_transform")
    } else {
        None
    }
}
type CurveGeometry = (Vec<[f32; 8]>, [f64; 2], [f64; 4]);
fn segments(o: &VectorObject) -> Option<CurveGeometry> {
    let path = Path::from_svg(&o.path.data)?;
    let points = path.points();
    if points.is_empty() {
        return None;
    }
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in points {
        bounds[0] = bounds[0].min(f64::from(p.x));
        bounds[1] = bounds[1].min(f64::from(p.y));
        bounds[2] = bounds[2].max(f64::from(p.x));
        bounds[3] = bounds[3].max(f64::from(p.y));
    }
    let center = [(bounds[0] + bounds[2]) * 0.5, (bounds[1] + bounds[3]) * 0.5];
    let local = |p: skia_safe::Point| {
        [
            (f64::from(p.x) - center[0]) as f32,
            (f64::from(p.y) - center[1]) as f32,
        ]
    };
    let mut result = Vec::new();
    let mut first = None;
    let mut last = None;
    let line = |a: [f32; 2], b: [f32; 2]| {
        [
            a[0],
            a[1],
            a[0] + (b[0] - a[0]) / 3.,
            a[1] + (b[1] - a[1]) / 3.,
            a[0] + (b[0] - a[0]) * 2. / 3.,
            a[1] + (b[1] - a[1]) * 2. / 3.,
            b[0],
            b[1],
        ]
    };
    for item in path.iter() {
        let p = item.points();
        match item.verb() {
            PathVerb::Move => {
                if let (Some(a), Some(b)) = (last, first) {
                    if a != b {
                        result.push(line(a, b));
                    }
                }
                first = Some(local(p[0]));
                last = first;
            }
            PathVerb::Line => {
                result.push(line(local(p[0]), local(p[1])));
                last = Some(local(p[1]));
            }
            PathVerb::Quad => {
                let a = local(p[0]);
                let b = local(p[1]);
                let c = local(p[2]);
                result.push([
                    a[0],
                    a[1],
                    a[0] + (b[0] - a[0]) * 2. / 3.,
                    a[1] + (b[1] - a[1]) * 2. / 3.,
                    c[0] + (b[0] - c[0]) * 2. / 3.,
                    c[1] + (b[1] - c[1]) * 2. / 3.,
                    c[0],
                    c[1],
                ]);
                last = Some(c);
            }
            PathVerb::Cubic => {
                let a = local(p[0]);
                let b = local(p[1]);
                let c = local(p[2]);
                let d = local(p[3]);
                result.push([a[0], a[1], b[0], b[1], c[0], c[1], d[0], d[1]]);
                last = Some(d);
            }
            PathVerb::Close => {
                if let (Some(a), Some(b)) = (last, first) {
                    if a != b {
                        result.push(line(a, b));
                    }
                }
                last = None;
                first = None;
            }
            _ => return None,
        }
        if result.len() > 32 {
            return None;
        }
    }
    if let (Some(a), Some(b)) = (last, first) {
        if a != b {
            result.push(line(a, b));
        }
    }
    if result.is_empty() || result.len() > 32 {
        return None;
    }
    Some((result, center, bounds))
}
impl Geometry {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        o: &VectorObject,
    ) -> Option<Self> {
        let (data, center, bounds) = segments(o)?;
        let curves = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Resident cubic control points"),
            contents: bytemuck::cast_slice(&data),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Rebased shape display"),
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let indirect = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Native shape indirect drawing"),
            contents: bytemuck::cast_slice(&[6u32, 1, 0, 0]),
            usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Native curve binding"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: curves.as_entire_binding(),
                },
            ],
        });
        Some(Self {
            path: o.path.clone(),
            center,
            bounds,
            count: data.len(),
            cost: data
                .iter()
                .map(|p| {
                    let line = [
                        p[0] + (p[6] - p[0]) / 3.,
                        p[1] + (p[7] - p[1]) / 3.,
                        p[0] + (p[6] - p[0]) * 2. / 3.,
                        p[1] + (p[7] - p[1]) * 2. / 3.,
                    ];
                    if line.iter().zip(&p[2..6]).all(|(a, b)| (a - b).abs() < 1e-4) {
                        1
                    } else {
                        35
                    }
                })
                .sum(),
            _curves: curves,
            uniform,
            indirect,
            binding,
            last_uniform: None,
            last_indirect: None,
        })
    }
    fn update(
        &mut self,
        queue: &wgpu::Queue,
        o: &VectorObject,
        v: Viewport,
        offset: [f32; 2],
    ) -> usize {
        let [a, b, c, d, e, f] = o.transform.map(f64::from);
        let scale = f64::from(v.screen_zoom()) * f64::from(v.scale);
        let screen = |x: f64, y: f64| {
            [
                (a * x + c * y + e + f64::from(offset[0]) - f64::from(v.document_width) * 0.5)
                    * scale
                    + f64::from(v.width) * 0.5
                    + f64::from(v.pan_x) * f64::from(v.scale),
                (b * x + d * y + f + f64::from(offset[1]) - f64::from(v.document_height) * 0.5)
                    * scale
                    + f64::from(v.height) * 0.5
                    + f64::from(v.pan_y) * f64::from(v.scale),
            ]
        };
        let origin = screen(self.center[0], self.center[1]);
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for [x, y] in [
            [self.bounds[0], self.bounds[1]],
            [self.bounds[2], self.bounds[1]],
            [self.bounds[0], self.bounds[3]],
            [self.bounds[2], self.bounds[3]],
        ] {
            let p = screen(x, y);
            bounds[0] = bounds[0].min(p[0]);
            bounds[1] = bounds[1].min(p[1]);
            bounds[2] = bounds[2].max(p[0]);
            bounds[3] = bounds[3].max(p[1]);
        }
        bounds[0] = (bounds[0] - 1.).max(0.);
        bounds[1] = (bounds[1] - 1.).max(0.);
        bounds[2] = (bounds[2] + 1.).min(f64::from(v.width));
        bounds[3] = (bounds[3] + 1.).min(f64::from(v.height));
        let det = (a * d - b * c) * scale;
        let fill = o.fill.unwrap().color;
        let linear = |value: u8| {
            let x = f32::from(value) / 255.;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        let uniform = [
            origin[0] as f32,
            origin[1] as f32,
            v.width as f32,
            v.height as f32,
            (d / det) as f32,
            (-c / det) as f32,
            (-b / det) as f32,
            (a / det) as f32,
            bounds[0] as f32,
            bounds[1] as f32,
            (bounds[2] - bounds[0]).max(0.) as f32,
            (bounds[3] - bounds[1]).max(0.) as f32,
            linear(fill[0]),
            linear(fill[1]),
            linear(fill[2]),
            f32::from(fill[3]) / 255. * o.opacity,
            self.count as f32,
            if o.path.fill_rule == FillRule::EvenOdd {
                1.
            } else {
                0.
            },
            0.,
            0.,
        ];
        if self.last_uniform != Some(uniform) {
            queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&uniform));
            self.last_uniform = Some(uniform);
        }
        let visible = bounds[2] > bounds[0] && bounds[3] > bounds[1];
        let indirect = [6u32, u32::from(visible), 0, 0];
        if self.last_indirect != Some(indirect) {
            queue.write_buffer(&self.indirect, 0, bytemuck::cast_slice(&indirect));
            self.last_indirect = Some(indirect);
        }
        ((bounds[2] - bounds[0]).max(0.) * (bounds[3] - bounds[1]).max(0.)).ceil() as usize
            * self.cost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transform_journal_refits_native_bounds_and_undo_restores_pixels() {
        let mut d = lumapaint_core::document::Document::default();
        let id = d.add_vector_layer().unwrap();
        d.upsert_vector_object(&id, object("M0 0H20V20H0Z"))
            .unwrap();
        d.select_vector_objects(vec!["curve".into()]).unwrap();
        let layer = d.svg_layers().next().unwrap();
        let mut verified = VerifiedLayer {
            source_key: (layer.source.as_ptr() as usize, layer.source.len()),
            compatible: true,
            spatial: lumapaint_core::scene::spatial::SpatialIndex::build([(
                0,
                native_drawing_bounds(&layer.vector_objects[0]),
            )]),
            positions: [("curve".into(), 0)].into(),
            cursor: d.scene_journal().cursor(),
            last_refits: 0,
            journal_id: d.scene_journal().instance_id(),
        };
        let original = crate::vector::rasterize_svg(&layer.source, 64, 64).unwrap();
        assert!(d.move_selected_vectors(30., 0.).unwrap());
        let layer = d.svg_layers().next().unwrap();
        assert!(verified.refresh(layer, d.scene_journal()));
        assert!(verified.spatial.query([0., 0., 20., 20.]).items.is_empty());
        assert_eq!(verified.spatial.query([30., 0., 50., 20.]).items, [0]);
        let (width, height) = d.dimensions();
        let full = lumapaint_core::document::vector_svg(width, height, &layer.vector_objects);
        assert_eq!(
            crate::vector::rasterize_svg(&layer.source, 64, 64)
                .unwrap()
                .pixels,
            crate::vector::rasterize_svg(&full, 64, 64).unwrap().pixels
        );
        d.undo();
        let layer = d.svg_layers().next().unwrap();
        assert!(verified.refresh(layer, d.scene_journal()));
        assert_eq!(verified.spatial.query([0., 0., 20., 20.]).items, [0]);
        assert_eq!(
            crate::vector::rasterize_svg(&layer.source, 64, 64)
                .unwrap()
                .pixels,
            original.pixels
        );
        d.redo();
        assert!(verified.refresh(d.svg_layers().next().unwrap(), d.scene_journal()));
        assert_eq!(verified.spatial.query([30., 0., 50., 20.]).items, [0]);
    }
    #[test]
    fn viewport_query_preserves_order_unknown_bounds_and_edge_coverage() {
        let viewport = Viewport::new(960., 640., 1., 1., false).unwrap();
        let index = lumapaint_core::scene::spatial::SpatialIndex::build([
            (3, Some([2000., 0., 2005., 5.])),
            (2, Some([-1., 20., -0.5, 30.])),
            (1, Some([100., 100., 150., 150.])),
            (0, None),
        ]);
        assert_eq!(viewport_candidates(&index, viewport), vec![0, 1, 2]);
        let mut panned = viewport;
        panned.pan_x = -1500.;
        assert_eq!(viewport_candidates(&index, panned), vec![0, 3]);
    }
    fn object(path: &str) -> VectorObject {
        VectorObject {
            id: "curve".into(),
            name: "Curve".into(),
            opacity: 1.,
            blend_mode: "normal".into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: path.into(),
                fill_rule: FillRule::EvenOdd,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(lumapaint_core::vector::VectorPaint {
                registration: false,
                color: [30, 80, 255, 255],
            }),
            stroke: None,
            stroke_width: 0.,
            stroke_style: Default::default(),
            live_corners: None,
            rectangle_radii: None,
            visible: true,
            kind: lumapaint_core::vector::VectorObjectKind::Path,
            control_points: vec![],
            text: None,
        }
    }
    #[test]
    fn portable_curves_and_unsupported_fallback() {
        let o = object("M0 0Q20 40 40 0C50 -20 60 20 70 0L70 50H0Z");
        let (data, center, bounds) = segments(&o).unwrap();
        assert_eq!(data.len(), 5);
        assert!(center.iter().all(|v| v.is_finite()));
        assert_eq!(bounds, [0., -20., 70., 50.]);
        assert!(segments(&object("M0 0A10 10 0 0 1 20 0Z")).is_none());
        let mut stroked = o;
        stroked.stroke = stroked.fill;
        assert!(!supported(&stroked));
    }
    #[test]
    #[ignore = "requires a real GPU"]
    fn gpu_native_cubic_matches_fill_and_reuses_resident_geometry() {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let mut cache = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        let v = Viewport {
            pasteboard_color: None,
            width: 128,
            height: 128,
            scale: 1.,
            zoom: 1.6,
            dark: false,
            pan_x: 0.,
            pan_y: 0.,
            document_width: 128.,
            document_height: 128.,
            canvas_color: lumapaint_core::document::CanvasColor::White,
        }
        .with_screen_zoom(1.);
        let o = object("M20 20C0 60 40 110 64 108C100 110 128 40 108 20Z M48 48H80V80H48Z");
        let mut layer = SvgLayer {
            id: "layer".into(),
            name: "layer".into(),
            visible: true,
            opacity: 1.,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
            source: String::new(),
            paint_layer: false,
            vector_layer: true,
            vector_objects: vec![o.clone()],
        };
        layer.source = lumapaint_core::document::vector_svg(128, 128, &layer.vector_objects);
        let render = |cache: &Cache, layer: &SvgLayer| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 128,
                    height: 128,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 128 * 512,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let view = texture.create_view(&Default::default());
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                cache.draw(&mut pass, layer);
            }
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(512),
                        rows_per_image: Some(128),
                    },
                },
                wgpu::Extent3d {
                    width: 128,
                    height: 128,
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, |r| r.unwrap());
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            let pixels = buffer
                .slice(..)
                .get_mapped_range()
                .expect("GPU buffer mapped after successful map callback")
                .to_vec();
            buffer.unmap();
            pixels
        };
        assert!(cache.prepare(
            &device,
            &queue,
            &layer,
            v,
            (&lumapaint_core::scene::Journal::default(), &[]),
            [0., 0.]
        ));
        let geometry = &cache.shapes["curve"]._curves as *const _ as usize;
        let pixels = render(&cache, &layer);
        let path = Path::from_svg(&o.path.data)
            .unwrap()
            .with_fill_type(skia_safe::PathFillType::EvenOdd);
        let mut mismatches = 0;
        for y in 0..128 {
            for x in 0..128 {
                let inside = path.contains((x as f32 + 0.5, y as f32 + 0.5));
                let alpha = pixels[(y * 128 + x) * 4 + 3];
                if (alpha > 127) != inside {
                    mismatches += 1;
                }
            }
        }
        assert!(
            mismatches <= 8,
            "Native cubic fill mismatches: {mismatches}"
        );
        let compatibility = crate::vector::with_compatibility_renderer(|| {
            crate::vector::rasterize_svg(&layer.source, 128, 128)
        })
        .unwrap();
        let error: usize = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(compatibility.pixels.as_chunks::<4>().0.iter())
            .map(|(a, b)| usize::from(a[3].abs_diff(b[3])))
            .sum();
        assert!(error < 128 * 128 * 2, "Cubic AA alpha error: {error}");
        let mut outside = o.clone();
        outside.id = "outside".into();
        outside.transform[4] = 10_000.;
        layer.vector_objects.push(outside);
        layer.source = lumapaint_core::document::vector_svg(128, 128, &layer.vector_objects);
        cache.begin_frame();
        assert!(cache.prepare(
            &device,
            &queue,
            &layer,
            v,
            (&lumapaint_core::scene::Journal::default(), &[]),
            [0., 0.]
        ));
        assert_eq!(cache.draw_lists[&layer.id], vec![0]);
        assert!(!cache.shapes.contains_key("outside"));
        assert_eq!(render(&cache, &layer), pixels);
        cache.begin_frame();
        assert!(cache.prepare(
            &device,
            &queue,
            &layer,
            v,
            (
                &lumapaint_core::scene::Journal::default(),
                &["outside".into()]
            ),
            [-10_000., 0.]
        ));
        assert_eq!(cache.draw_lists[&layer.id], vec![0, 1]);
        assert!(cache.shapes.contains_key("outside"));
        layer.vector_objects.pop();
        layer.source = lumapaint_core::document::vector_svg(128, 128, &layer.vector_objects);
        // Pan and extremely high zoom alter display uniforms, never curve allocation.
        let mut high = v.with_screen_zoom(640.);
        high.pan_x = 100.;
        assert!(cache.prepare(
            &device,
            &queue,
            &layer,
            high,
            (&lumapaint_core::scene::Journal::default(), &[]),
            [0., 0.]
        ));
        assert_eq!(
            &cache.shapes["curve"]._curves as *const _ as usize,
            geometry
        );
        // A real document transaction must update only one BVH leaf and reuse
        // resident geometry. Compare incremental output to a fresh full cache.
        let mut state = lumapaint_core::document::Document::default().document_state();
        state.width = 128;
        state.height = 128;
        state.svg_layers = vec![layer.clone()];
        let mut document = lumapaint_core::document::Document::from_document_state(state).unwrap();
        document
            .select_vector_objects(vec!["curve".into()])
            .unwrap();
        cache.begin_frame();
        cache.synchronize(&document);
        let original_layer = document.svg_layers().next().unwrap();
        assert!(cache.prepare(
            &device,
            &queue,
            original_layer,
            v,
            (document.scene_journal(), &[]),
            [0., 0.]
        ));
        let original_pixels = render(&cache, original_layer);
        let retained_geometry = &cache.shapes["curve"]._curves as *const _ as usize;
        for _ in 0..2 {
            cache.begin_frame();
            cache.synchronize(&document);
            assert!(cache.prepare(
                &device,
                &queue,
                document.svg_layers().next().unwrap(),
                v,
                (document.scene_journal(), &[]),
                [0., 0.]
            ));
            assert_eq!(cache.metrics.live_id_scans, 0);
            assert_eq!(cache.metrics.svg_generations, 0);
            assert_eq!(cache.metrics.bvh_builds, 0);
            assert_eq!(cache.metrics.bvh_refits, 0);
            assert_eq!(cache.metrics.geometry_builds, 0);
            assert_eq!(cache.metrics.geometry_upload_bytes, 0);
            assert_eq!(cache.metrics.uniform_upload_bytes, 0);
        }
        document.move_selected_vectors(10., 5.).unwrap();
        cache.begin_frame();
        cache.synchronize(&document);
        let moved_layer = document.svg_layers().next().unwrap();
        assert!(cache.prepare(
            &device,
            &queue,
            moved_layer,
            v,
            (document.scene_journal(), &[]),
            [0., 0.]
        ));
        assert_eq!(cache.metrics.live_id_scans, 0);
        assert_eq!(cache.metrics.svg_generations, 0);
        assert_eq!(cache.metrics.bvh_builds, 0);
        assert_eq!(cache.metrics.bvh_refits, 1);
        assert_eq!(
            &cache.shapes["curve"]._curves as *const _ as usize,
            retained_geometry
        );
        let moved_pixels = render(&cache, moved_layer);
        let mut full_cache = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert!(full_cache.prepare(
            &device,
            &queue,
            moved_layer,
            v,
            (document.scene_journal(), &[]),
            [0., 0.]
        ));
        assert_eq!(moved_pixels, render(&full_cache, moved_layer));
        document.undo();
        cache.begin_frame();
        cache.synchronize(&document);
        let restored = document.svg_layers().next().unwrap();
        assert!(cache.prepare(
            &device,
            &queue,
            restored,
            v,
            (document.scene_journal(), &[]),
            [0., 0.]
        ));
        assert_eq!(render(&cache, restored), original_pixels);
        assert_eq!(cache.metrics.bvh_refits, 1);
        assert_eq!(cache.metrics.svg_generations, 0);
        document.redo();
        cache.begin_frame();
        cache.synchronize(&document);
        let restored = document.svg_layers().next().unwrap();
        assert!(cache.prepare(
            &device,
            &queue,
            restored,
            v,
            (document.scene_journal(), &[]),
            [0., 0.]
        ));
        assert_eq!(render(&cache, restored), moved_pixels);
        assert_eq!(cache.metrics.bvh_refits, 1);
        assert_eq!(cache.metrics.svg_generations, 0);
        let canonical = layer.source.clone();
        layer.source = layer
            .source
            .replace("</svg>", "<style>path { display:none }</style></svg>");
        cache.begin_frame();
        assert!(!cache.prepare(
            &device,
            &queue,
            &layer,
            v,
            (&lumapaint_core::scene::Journal::default(), &[]),
            [0., 0.]
        ));
        assert!(!cache.ready(&layer));
        layer.source = canonical;
        layer.vector_objects[0].stroke = layer.vector_objects[0].fill;
        assert!(!cache.prepare(
            &device,
            &queue,
            &layer,
            v,
            (&lumapaint_core::scene::Journal::default(), &[]),
            [0., 0.]
        ));
    }
}
