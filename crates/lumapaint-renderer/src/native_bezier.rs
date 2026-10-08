//! Bounded native cubic fills, exact opaque rectangles and rectangular straight strokes.
use crate::native_composite as composite;
#[path = "native_paint.rs"]
mod paint;
use crate::{create_layer_pipeline, Viewport};
use lumapaint_core::{
    document::SvgLayer,
    vector::{FillRule, VectorObject, VectorPath},
};
use skia_safe::{Path, PathVerb};
use std::collections::{HashMap, HashSet};
use wgpu::util::DeviceExt;

pub(crate) struct Geometry {
    layer_id: String,
    paint: paint::Paint,
    path: VectorPath,
    stroke_key: Option<(f32, lumapaint_core::stroke::StrokeStyle)>,
    exact_rectangle: bool,
    rectangular_stroke: bool,
    straight_stroke: bool,
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
    pub source_layer_scans: usize,
    pub svg_generations: usize,
    pub bvh_builds: usize,
    pub bvh_refits: usize,
    pub spatial_queries: usize,
    pub spatial_query_reuses: usize,
    pub geometry_builds: usize,
    pub geometry_upload_bytes: u64,
    pub uniform_upload_bytes: usize,
    pub clip_mask_upload_bytes: usize,
}
pub(crate) struct Cache {
    pub pipeline: wgpu::RenderPipeline,
    encoded_pipeline: wgpu::RenderPipeline,
    composite_pipeline: composite::Pipeline,
    composites: HashMap<String, composite::Composite>,
    pub metrics: PreparationMetrics,
    layout: wgpu::BindGroupLayout,
    pub shapes: HashMap<String, Geometry>,
    layer_shapes: HashMap<String, HashSet<String>>,
    resident_bytes: u64,
    ready: HashSet<String>,
    live_cursor: Option<u64>,
    live_revision: Option<u64>,
    journal_id: Option<u64>,
    layer_sources: Vec<(String, usize, usize)>,
    verified: HashMap<String, VerifiedLayer>,
    draw_lists: HashMap<String, Vec<usize>>,
    query_keys: HashMap<String, QueryKey>,
    query_stack: Vec<usize>,
    normal_zoom_probe: Option<(String, VectorPath, bool, [f64; 4])>,
}
#[derive(Clone, Copy, PartialEq)]
struct QueryKey {
    bounds: [f64; 4],
    source: (usize, usize),
    journal_id: u64,
    sequence: u64,
}
struct VerifiedLayer {
    source_key: (usize, usize),
    compatible: bool,
    fallback_reasons: HashMap<&'static str, u64>,
    spatial: lumapaint_core::scene::spatial::SpatialIndex,
    positions: HashMap<String, usize>,
    local_bounds: Vec<Option<[f64; 4]>>,
    clip_indices: Vec<Vec<usize>>,
    object_ids: Vec<String>,
    cursor: u64,
    last_refits: usize,
    journal_id: u64,
    layer_sequence: u64,
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
        if self.source_key == source_key && self.layer_sequence == journal.layer_sequence(&layer.id)
        {
            self.cursor = journal.cursor();
            crate::performance::count("native_unchanged_layer_journal_skips", 1);
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
        let mut has_changes = false;
        let mut has_objects = false;
        for change in changes.iter().filter(|c| c.target.layer == layer.id) {
            has_changes = true;
            if change.removed
                || change.changes.structure
                || change.changes.style
                || change.changes.visibility
            {
                return false;
            }
            if let Some(id) = &change.target.object {
                has_objects = true;
                if change.changes.geometry || !change.changes.transform {
                    return false;
                }
                let Some(&position) = self.positions.get(id) else {
                    return false;
                };
                let object = &layer.vector_objects[position];
                if object.id != *id || !supported(object) {
                    return false;
                }
            }
        }
        if !has_changes && self.source_key == source_key {
            self.cursor = cursor;
            self.layer_sequence = journal.layer_sequence(&layer.id);
            return true;
        }
        if !has_objects {
            return false;
        }
        for change in changes.iter().filter(|c| c.target.layer == layer.id) {
            let Some(id) = &change.target.object else {
                continue;
            };
            let position = self.positions[id];
            let object = &layer.vector_objects[position];
            self.last_refits += 1;
            if !self.spatial.refit(
                position,
                transformed_drawing_bounds(object, self.local_bounds[position]),
            ) {
                return false;
            }
        }
        self.source_key = source_key;
        self.cursor = cursor;
        self.layer_sequence = journal.layer_sequence(&layer.id);
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
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
        let encoded_pipeline = crate::create_layer_pipeline_with_fragment(
            device,
            &[&layout],
            wgpu::TextureFormat::Rgba8Unorm,
            include_str!("native_bezier.wgsl"),
            "Native encoded geometry",
            "fs_encoded",
        );
        let composite_pipeline = composite::Pipeline::new(device, format);
        Self {
            pipeline,
            encoded_pipeline,
            composite_pipeline,
            composites: HashMap::new(),
            metrics: PreparationMetrics::default(),
            layout,
            shapes: HashMap::new(),
            layer_shapes: HashMap::new(),
            resident_bytes: 0,
            ready: HashSet::new(),
            live_cursor: None,
            live_revision: None,
            journal_id: None,
            layer_sources: Vec::new(),
            verified: HashMap::new(),
            draw_lists: HashMap::new(),
            query_keys: HashMap::new(),
            query_stack: Vec::new(),
            normal_zoom_probe: None,
        }
    }
    pub fn begin_frame(&mut self) {
        self.metrics = PreparationMetrics::default();
        self.ready.clear();
    }
    pub fn ready(&self, layer: &SvgLayer) -> bool {
        self.ready.contains(&layer.id)
            && self.composites.get(&layer.id).map_or_else(
                || layer.effective_opacity() == 1.,
                |c| c.opacity == layer.effective_opacity(),
            )
    }
    pub fn synchronize(&mut self, document: &lumapaint_core::document::Document) {
        use lumapaint_core::scene::JournalRead;
        let journal_id = document.scene_journal().instance_id();
        if self.journal_id != Some(journal_id) {
            self.verified.clear();
            self.live_cursor = None;
            self.live_revision = None;
            self.journal_id = Some(journal_id);
        }
        let cursor = document.scene_journal().cursor();
        // Document owns its SVG layers and exposes immutable iteration only.
        // Source edits advance revision; mutable forks get a fresh journal ID.
        // Consequently a stable journal/revision pair needs no source traversal.
        let revision = document.revision();
        if self.live_cursor == Some(cursor) && self.live_revision == Some(revision) {
            return;
        }
        let same_sources = document
            .svg_layers()
            .map(|l| {
                self.metrics.source_layer_scans += 1;
                crate::performance::count("native_source_layers_scanned", 1);
                (l.id.as_str(), l.source.as_ptr() as usize, l.source.len())
            })
            .eq(self
                .layer_sources
                .iter()
                .map(|(id, pointer, length)| (id.as_str(), *pointer, *length)));
        if self.live_cursor == Some(cursor) && same_sources {
            self.live_revision = Some(revision);
            return;
        }
        let rebuild = match self.live_cursor {
            None => true,
            Some(previous) => match document.scene_journal().read(previous) {
                JournalRead::Rebuild { .. } => true,
                JournalRead::Incremental { changes, .. } => {
                    for change in &changes {
                        if change.removed {
                            if let Some(id) = &change.target.object {
                                if let Some(geometry) = self.shapes.remove(id) {
                                    self.resident_bytes =
                                        self.resident_bytes.saturating_sub(geometry.bytes());
                                    if let Some(ids) = self.layer_shapes.get_mut(&geometry.layer_id)
                                    {
                                        ids.remove(id);
                                        if ids.is_empty() {
                                            self.layer_shapes.remove(&geometry.layer_id);
                                        }
                                    }
                                }
                            } else if let Some(ids) = self.layer_shapes.remove(&change.target.layer)
                            {
                                self.composites.remove(&change.target.layer);
                                for id in ids {
                                    if let Some(g) = self.shapes.remove(&id) {
                                        self.resident_bytes =
                                            self.resident_bytes.saturating_sub(g.bytes());
                                    }
                                }
                            }
                        }
                        if change.removed || change.changes.structure {
                            self.verified.remove(&change.target.layer);
                            self.draw_lists.remove(&change.target.layer);
                            self.query_keys.remove(&change.target.layer);
                        }
                    }
                    changes.is_empty() && !same_sources
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
            self.resident_bytes = self.shapes.values().map(Geometry::bytes).sum();
            self.layer_shapes.clear();
            for (id, geometry) in &self.shapes {
                self.layer_shapes
                    .entry(geometry.layer_id.clone())
                    .or_default()
                    .insert(id.clone());
            }
            self.verified.retain(|id, _| layers.contains(id));
            self.draw_lists.retain(|id, _| layers.contains(id));
            self.composites.retain(|id, _| layers.contains(id));
            self.query_keys.retain(|id, _| layers.contains(id));
        }
        self.live_cursor = Some(cursor);
        self.live_revision = Some(revision);
        if !same_sources {
            crate::performance::count("native_source_snapshot_builds", 1);
            self.layer_sources.clear();
            self.layer_sources.extend(
                document
                    .svg_layers()
                    .map(|l| (l.id.clone(), l.source.as_ptr() as usize, l.source.len())),
            );
        }
    }
    /// Production routing: qualified rectangles and straight strokes use
    /// exact pixel coverage at normal zoom. Cubic fills retain their high-zoom policy.
    pub fn prepare_display(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &SvgLayer,
        viewport: Viewport,
        scene: (&lumapaint_core::scene::Journal, &[String]),
        offset: [f32; 2],
    ) -> bool {
        if viewport.screen_zoom() <= 1.5 {
            let preflight = self
                .verified
                .get(&layer.id)
                .is_none_or(|v| !v.compatible || v.journal_id != scene.0.instance_id());
            let mut has_objects = !preflight;
            for object in layer
                .vector_objects
                .iter()
                .filter(|o| preflight && o.clipping_group.is_none())
            {
                has_objects = true;
                if !supported(object) || object.transform[1] != 0. || object.transform[2] != 0. {
                    crate::performance::count("fallback.native_normal_zoom_quality", 1);
                    return false;
                }
                let object_offset = if scene.1.contains(&object.id) {
                    offset
                } else {
                    [0.; 2]
                };
                if object.stroke.is_none() {
                    // Cubic/arc paths retain their existing rejection before parsing.
                    // Bound the one-entry classification cache independently of GPU caches.
                    if object.path.data.len() > 4096
                        || object.path.data.bytes().any(|byte| {
                            matches!(
                                byte,
                                b'C' | b'c' | b'Q' | b'q' | b'S' | b's' | b'T' | b't' | b'A' | b'a'
                            )
                        })
                    {
                        crate::performance::count("fallback.native_normal_zoom_quality", 1);
                        return false;
                    }
                    let retained = self
                        .shapes
                        .get(&object.id)
                        .filter(|shape| shape.path == object.path)
                        .map(|shape| (shape.exact_rectangle, shape.bounds));
                    let (exact, bounds) = retained.unwrap_or_else(|| {
                        let unchanged =
                            self.normal_zoom_probe
                                .as_ref()
                                .is_some_and(|(id, path, _, _)| {
                                    *id == object.id && *path == object.path
                                });
                        if !unchanged {
                            let (exact, bounds) = segments(object)
                                .map_or((false, [0.; 4]), |(data, _, bounds)| {
                                    (exact_rectangle(&data), bounds)
                                });
                            self.normal_zoom_probe =
                                Some((object.id.clone(), object.path.clone(), exact, bounds));
                        }
                        let probe = self.normal_zoom_probe.as_ref().unwrap();
                        (probe.2, probe.3)
                    });
                    if !exact || !rectangle_pixel_aligned(object, viewport, object_offset, bounds) {
                        crate::performance::count("fallback.native_normal_zoom_quality", 1);
                        return false;
                    }
                }
            }
            if !has_objects {
                return false;
            }
            if !self.prepare(device, queue, layer, viewport, scene, offset) {
                return false;
            }
            if self
                .draw_lists
                .get(&layer.id)
                .into_iter()
                .flatten()
                .all(|&i| {
                    let object = &layer.vector_objects[i];
                    self.shapes.get(&object.id).is_some_and(|shape| {
                        let clips = &self.verified[&layer.id].clip_indices
                            [self.verified[&layer.id].positions[&object.id]];
                        shape
                            .paint
                            .normal_clips_supported(layer, clips, viewport, scene.1, offset)
                            && if object.stroke.is_some() {
                                shape.rectangular_stroke
                            } else {
                                shape.exact_rectangle
                                    && object.transform[1] == 0.
                                    && object.transform[2] == 0.
                                    && rectangle_pixel_aligned(
                                        object,
                                        viewport,
                                        if scene.1.contains(&object.id) {
                                            offset
                                        } else {
                                            [0.; 2]
                                        },
                                        shape.bounds,
                                    )
                            }
                    })
                })
            {
                return true;
            }
            self.ready.remove(&layer.id);
            crate::performance::count("fallback.native_normal_zoom_quality", 1);
            return false;
        }
        if !self.prepare(device, queue, layer, viewport, scene, offset) {
            return false;
        }
        if self
            .draw_lists
            .get(&layer.id)
            .into_iter()
            .flatten()
            .any(|&i| {
                let o = &layer.vector_objects[i];
                o.stroke.is_some() && !self.shapes.get(&o.id).is_some_and(|g| g.straight_stroke)
            })
        {
            self.ready.remove(&layer.id);
            crate::performance::count("fallback.native_stroke_quality", 1);
            return false;
        }
        true
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
        let previous_upload = (
            self.metrics.geometry_builds,
            self.metrics.uniform_upload_bytes,
        );
        let (journal, selected) = scene;
        if !layer.vector_layer
            || !layer.effective_opacity().is_finite()
            || layer.vector_objects.is_empty()
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
            let eligible = layer.vector_objects.iter().all(supported)
                && (layer
                    .vector_objects
                    .iter()
                    .filter(|o| o.clipping_group.is_none())
                    .count()
                    == 1
                    || layer
                        .vector_objects
                        .iter()
                        .filter(|o| o.clipping_group.is_none())
                        .all(|object| object.stroke.is_none()));
            let mut verified = VerifiedLayer {
                source_key: (layer.source.as_ptr() as usize, layer.source.len()),
                compatible: false,
                fallback_reasons: HashMap::new(),
                spatial: Default::default(),
                positions: HashMap::new(),
                local_bounds: Vec::new(),
                cursor: journal.cursor(),
                clip_indices: Vec::new(),
                object_ids: Vec::new(),
                last_refits: 0,
                journal_id: journal.instance_id(),
                layer_sequence: journal.layer_sequence(&layer.id),
            };
            if eligible {
                self.metrics.svg_generations += 1;
                let expected = lumapaint_core::document::vector_svg(
                    viewport.document_width as u32,
                    viewport.document_height as u32,
                    &layer.vector_objects,
                );
                verified.compatible = expected == layer.source;
                if verified.compatible {
                    self.metrics.bvh_builds += 1;
                    verified.local_bounds = layer
                        .vector_objects
                        .iter()
                        .map(|object| segments(object).map(|(_, _, bounds)| bounds))
                        .collect();
                    verified.spatial = lumapaint_core::scene::spatial::SpatialIndex::build(
                        layer.vector_objects.iter().enumerate().map(|(i, object)| {
                            (
                                i,
                                transformed_drawing_bounds(object, verified.local_bounds[i]),
                            )
                        }),
                    );
                    verified.object_ids =
                        layer.vector_objects.iter().map(|o| o.id.clone()).collect();
                    verified.clip_indices = layer
                        .vector_objects
                        .iter()
                        .map(|o| {
                            layer
                                .vector_objects
                                .iter()
                                .enumerate()
                                .filter_map(|(i, mask)| {
                                    mask.clipping_group
                                        .as_ref()
                                        .filter(|g| o.group_path.contains(g))
                                        .map(|_| i)
                                })
                                .collect()
                        })
                        .collect();
                    verified.positions = layer
                        .vector_objects
                        .iter()
                        .enumerate()
                        .map(|(i, object)| (object.id.clone(), i))
                        .collect();
                }
            }
            if !verified.compatible {
                for object in &layer.vector_objects {
                    if let Some(reason) = unsupported_reason(object) {
                        *verified.fallback_reasons.entry(reason).or_default() += 1;
                    }
                }
                if layer.vector_objects.len() > 1
                    && layer
                        .vector_objects
                        .iter()
                        .any(|object| object.stroke.is_some())
                {
                    verified
                        .fallback_reasons
                        .insert("fallback.native_stroke_compositing", 1);
                }
                if eligible {
                    verified
                        .fallback_reasons
                        .insert("fallback.native_source_mismatch", 1);
                }
            }
            self.verified.insert(layer.id.clone(), verified);
        }
        self.metrics.bvh_refits += self.verified[&layer.id].last_refits;
        if !self.verified[&layer.id].compatible {
            for (&reason, &count) in &self.verified[&layer.id].fallback_reasons {
                crate::performance::count(reason, count);
            }
            return false;
        }
        let verified = &self.verified[&layer.id];
        let query_key = QueryKey {
            bounds: viewport_query_bounds(viewport),
            source: verified.source_key,
            journal_id: verified.journal_id,
            sequence: verified.layer_sequence,
        };
        let previous_candidates = self.draw_lists.remove(&layer.id);
        let reuse_query = offset == [0., 0.]
            && previous_candidates.is_some()
            && self.query_keys.get(&layer.id) == Some(&query_key);
        let mut candidates = previous_candidates.unwrap_or_default();
        if !reuse_query {
            self.metrics.spatial_queries += 1;
            let _timer = crate::performance::time("native_spatial_query");
            let visited = self.verified[&layer.id].spatial.query_into(
                query_key.bounds,
                &mut candidates,
                &mut self.query_stack,
            );
            crate::performance::count("native_spatial_visited_nodes", visited as u64);
        } else {
            self.metrics.spatial_query_reuses += 1;
            crate::performance::count("native_spatial_query_reuses", 1);
        }
        if offset != [0., 0.] {
            self.query_keys.remove(&layer.id);
        }
        // Consult retained ID positions instead of scanning every offscreen object.
        if offset != [0., 0.] {
            candidates.extend(
                selected
                    .iter()
                    .filter_map(|id| self.verified[&layer.id].positions.get(id).copied()),
            );
            candidates.sort_unstable();
            candidates.dedup();
        }
        candidates.retain(|&i| {
            layer.vector_objects[i].visible && layer.vector_objects[i].clipping_group.is_none()
        });
        let mut work = 0usize;
        for &i in &candidates {
            let o = &layer.vector_objects[i];
            let clip_indices = &self.verified[&layer.id].clip_indices[i];
            let valid = self.shapes.get(&o.id).is_some_and(|g| {
                g.paint.matches(o, layer, clip_indices, viewport)
                    && g.layer_id == layer.id
                    && g.path == o.path
                    && g.stroke_key.as_ref().is_none_or(|(width, style)| {
                        o.stroke.is_some() && *width == o.stroke_width && *style == o.stroke_style
                    })
                    && (g.stroke_key.is_some() == o.stroke.is_some())
            });
            if !valid {
                if self.shapes.len() >= 8192 {
                    return false;
                }
                let Some(g) = Geometry::new(device, &self.layout, layer, o, clip_indices, viewport)
                else {
                    crate::performance::count("fallback.native_geometry_unrepresentable", 1);
                    return false;
                };
                let previous_bytes = self.shapes.get(&o.id).map_or(0, Geometry::bytes);
                let retained_bytes = self
                    .resident_bytes
                    .saturating_sub(previous_bytes)
                    .saturating_add(g.bytes());
                if retained_bytes > 64 * 1024 * 1024 {
                    crate::performance::count("fallback.native_resource_budget", 1);
                    return false;
                }
                self.resident_bytes = retained_bytes;
                self.metrics.geometry_builds += 1;
                self.metrics.geometry_upload_bytes +=
                    g._curves.size() + g.paint.stops.size() + g.paint.clip_curves.size();
                if let Some(previous) = self.shapes.insert(o.id.clone(), g) {
                    if previous.layer_id != layer.id {
                        if let Some(ids) = self.layer_shapes.get_mut(&previous.layer_id) {
                            ids.remove(&o.id);
                            if ids.is_empty() {
                                self.layer_shapes.remove(&previous.layer_id);
                            }
                        }
                    }
                }
                self.layer_shapes
                    .entry(layer.id.clone())
                    .or_default()
                    .insert(o.id.clone());
            }
            let g = self.shapes.get_mut(&o.id).unwrap();
            if !g.paint.precise(layer, clip_indices, viewport) {
                crate::performance::count("fallback.native_clip_precision", 1);
                return false;
            }
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
            let Some(paint_uploaded) =
                g.paint
                    .update(queue, o, layer, clip_indices, viewport, (selected, offset))
            else {
                return false;
            };
            self.metrics.uniform_upload_bytes += paint_uploaded;
            self.metrics.clip_mask_upload_bytes += g.paint.mask_upload_bytes;
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
        let geometry_changed = previous_upload
            != (
                self.metrics.geometry_builds,
                self.metrics.uniform_upload_bytes,
            );
        let isolated = candidates.len() > 1 || layer.effective_opacity() != 1.;
        if isolated {
            let size = [viewport.width, viewport.height];
            let bytes = u64::from(size[0]) * u64::from(size[1]) * 4;
            if bytes > 64 * 1024 * 1024 || size.contains(&0) {
                return false;
            }
            if self
                .composites
                .get(&layer.id)
                .is_none_or(|c| c.size != size)
            {
                self.composites.remove(&layer.id);
                let current: u64 = self
                    .composites
                    .values()
                    .map(|c| u64::from(c.size[0]) * u64::from(c.size[1]) * 4)
                    .sum();
                if current + bytes > 64 * 1024 * 1024 {
                    self.composites.retain(|id, _| self.ready.contains(id));
                    let current: u64 = self
                        .composites
                        .values()
                        .map(|c| u64::from(c.size[0]) * u64::from(c.size[1]) * 4)
                        .sum();
                    if current + bytes > 64 * 1024 * 1024 {
                        return false;
                    }
                }
                self.composites.insert(
                    layer.id.clone(),
                    self.composite_pipeline.target(device, size),
                );
            }
            let c = self.composites.get_mut(&layer.id).unwrap();
            if c.opacity != layer.effective_opacity() {
                c.opacity = layer.effective_opacity();
                crate::gpu_metrics::write_buffer!(
                    queue,
                    &c.uniform,
                    0,
                    bytemuck::cast_slice(&[c.opacity, 0., 0., 0.])
                );
                self.metrics.uniform_upload_bytes += 16;
            }
            if !reuse_query || geometry_changed {
                c.dirty.set(true);
            }
        } else {
            self.composites.remove(&layer.id);
        }
        self.draw_lists.insert(layer.id.clone(), candidates);
        if offset == [0., 0.] {
            if let Some(key) = self.query_keys.get_mut(&layer.id) {
                *key = query_key;
            } else {
                self.query_keys.insert(layer.id.clone(), query_key);
            }
        }
        self.ready.insert(layer.id.clone());
        true
    }
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        for id in &self.ready {
            let Some(c) = self.composites.get(id).filter(|c| c.dirty.get()) else {
                continue;
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Native encoded layer pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &c.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.encoded_pipeline);
            for &i in self.draw_lists.get(id).into_iter().flatten() {
                let object_id = self.verified[id].object_ids.get(i);
                if let Some(g) = object_id.and_then(|id| self.shapes.get(id)) {
                    pass.set_bind_group(0, &g.binding, &[]);
                    pass.draw_indirect(&g.indirect, 0);
                }
            }
            crate::performance::count("native_isolated_layer_redraws", 1);
            c.dirty.set(false);
        }
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, layer: &SvgLayer) {
        if let Some(c) = self.composites.get(&layer.id) {
            pass.set_pipeline(&self.composite_pipeline.draw);
            pass.set_bind_group(0, &c.binding, &[]);
            pass.draw(0..6, 0..1);
            return;
        }
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
#[cfg(test)]
fn native_drawing_bounds(object: &VectorObject) -> Option<[f64; 4]> {
    transformed_drawing_bounds(object, segments(object).map(|(_, _, bounds)| bounds))
}
fn transformed_drawing_bounds(object: &VectorObject, local: Option<[f64; 4]>) -> Option<[f64; 4]> {
    let local = local?;
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
#[cfg(test)]
fn viewport_candidates(
    index: &lumapaint_core::scene::spatial::SpatialIndex,
    viewport: Viewport,
) -> Vec<usize> {
    index.query(viewport_query_bounds(viewport)).items
}
fn viewport_query_bounds(viewport: Viewport) -> [f64; 4] {
    let first = viewport.document_point(-2. / viewport.scale, -2. / viewport.scale);
    let last = viewport.document_point(
        (viewport.width as f32 + 2.) / viewport.scale,
        (viewport.height as f32 + 2.) / viewport.scale,
    );
    [
        f64::from(first.x),
        f64::from(first.y),
        f64::from(last.x),
        f64::from(last.y),
    ]
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
    } else if o.clipping_group.is_none()
        && o.stroke.is_some()
        && (o.fill.is_some() || !native_stroke_style(o))
    {
        Some("native_ineligible.stroke")
    } else if o.clipping_group.is_none() && o.fill.is_none() && o.stroke.is_none() {
        Some("native_ineligible.no_fill")
    } else if o
        .fill_gradient
        .as_ref()
        .or(o.stroke_gradient.as_ref())
        .is_some_and(|g| g.validate().is_err() || g.dither || g.pixel_style.is_some())
    {
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
fn stroke_key(o: &VectorObject) -> Option<(f32, lumapaint_core::stroke::StrokeStyle)> {
    o.stroke.map(|_| (o.stroke_width, o.stroke_style.clone()))
}
fn simple_stroke_style(o: &VectorObject) -> bool {
    use lumapaint_core::stroke::{LineCap, LineJoin};
    native_stroke_style(o)
        && o.stroke_style.cap != LineCap::Round
        && o.stroke_style.join != LineJoin::Round
        && (o.stroke_style.cap != LineCap::Square
            || o.stroke_style
                .dash_array
                .iter()
                .enumerate()
                .all(|(i, &gap)| {
                    (o.stroke_style.dash_array.len().is_multiple_of(2) && i.is_multiple_of(2))
                        || gap >= o.stroke_width
                }))
}
fn native_stroke_style(o: &VectorObject) -> bool {
    use lumapaint_core::stroke::{Arrowhead, StrokeAlignment, WidthProfile};
    o.stroke_width.is_finite()
        && o.stroke_width > 0.
        && o.transform[1] == 0.
        && o.transform[2] == 0.
        && o.opacity == 1.
        && o.stroke.is_some_and(|paint| paint.color[3] == 255)
        && o.stroke_style.validate().is_ok()
        && o.stroke_style.alignment == StrokeAlignment::Center
        && o.stroke_style.contour_alignments.is_empty()
        && o.stroke_style.profile == WidthProfile::Uniform
        && o.stroke_style.start_arrow == Arrowhead::None
        && o.stroke_style.end_arrow == Arrowhead::None
}

fn normalize_stroke_rectangles(path: &Path) -> Option<Path> {
    fn flush(
        points: &mut Vec<skia_safe::Point>,
        builder: &mut skia_safe::PathBuilder,
        count: &mut usize,
    ) -> Option<()> {
        if points.is_empty() {
            return Some(());
        }
        if points.len() < 4 {
            return None;
        }
        let left = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let top = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let right = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
        let bottom = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        if left >= right || top >= bottom {
            return None;
        }
        let mut area = 0.;
        for (i, p) in points.iter().enumerate() {
            let q = points[(i + 1) % points.len()];
            if (p.x != left && p.x != right && p.y != top && p.y != bottom)
                || (p.x != q.x && p.y != q.y)
            {
                return None;
            }
            area += f64::from(p.x) * f64::from(q.y) - f64::from(p.y) * f64::from(q.x);
        }
        let expected = 2. * f64::from(right - left) * f64::from(bottom - top);
        if (area.abs() - expected).abs() > expected * 1e-6 {
            return None;
        }
        builder.add_rect(skia_safe::Rect::new(left, top, right, bottom), None, None);
        points.clear();
        *count += 1;
        if *count > 8 {
            return None;
        }
        Some(())
    }
    let mut builder = skia_safe::PathBuilder::default();
    let mut points = Vec::new();
    let mut count = 0;
    for item in path.iter() {
        match item.verb() {
            PathVerb::Move => {
                flush(&mut points, &mut builder, &mut count)?;
                points.push(item.points()[0]);
            }
            PathVerb::Line => points.push(item.points()[1]),
            PathVerb::Close => {
                flush(&mut points, &mut builder, &mut count)?;
            }
            _ => return None,
        }
    }
    flush(&mut points, &mut builder, &mut count)?;
    (count > 0).then(|| builder.into())
}

// a/b contain the cubic controls; parameters = [conic weight, conic flag, 0, 0].
// Conics keep their rational form rather than introducing zoom-dependent tessellation.
type CurveGeometry = (Vec<[f32; 12]>, [f64; 2], [f64; 4]);
fn rectangle_pixel_aligned(
    o: &VectorObject,
    v: Viewport,
    offset: [f32; 2],
    bounds: [f64; 4],
) -> bool {
    let [a, _, _, d, e, f] = o.transform.map(f64::from);
    let scale = f64::from(v.screen_zoom()) * f64::from(v.scale);
    [bounds[0], bounds[2]].into_iter().all(|x| {
        let x = (a * x + e + f64::from(offset[0]) - f64::from(v.document_width) * 0.5) * scale
            + f64::from(v.width) * 0.5
            + f64::from(v.pan_x) * f64::from(v.scale);
        x.is_finite() && (x - x.round()).abs() <= 1e-6
    }) && [bounds[1], bounds[3]].into_iter().all(|y| {
        let y = (d * y + f + f64::from(offset[1]) - f64::from(v.document_height) * 0.5) * scale
            + f64::from(v.height) * 0.5
            + f64::from(v.pan_y) * f64::from(v.scale);
        y.is_finite() && (y - y.round()).abs() <= 1e-6
    })
}
fn exact_rectangle(data: &[[f32; 12]]) -> bool {
    if data.len() != 4 {
        return false;
    }
    let lo = [
        data.iter().map(|s| s[0]).fold(f32::INFINITY, f32::min),
        data.iter().map(|s| s[1]).fold(f32::INFINITY, f32::min),
    ];
    let hi = [
        data.iter().map(|s| s[0]).fold(f32::NEG_INFINITY, f32::max),
        data.iter().map(|s| s[1]).fold(f32::NEG_INFINITY, f32::max),
    ];
    if lo[0] >= hi[0] || lo[1] >= hi[1] {
        return false;
    }
    let mut area = 0f64;
    for (i, s) in data.iter().enumerate() {
        if s[9] != 0.
            || s[6..8] != data[(i + 1) % 4][..2]
            || (s[0] != lo[0] && s[0] != hi[0])
            || (s[1] != lo[1] && s[1] != hi[1])
            || !((s[0] == s[6] && s[2] == s[0] && s[4] == s[0])
                || (s[1] == s[7] && s[3] == s[1] && s[5] == s[1]))
            || s[..8].as_chunks::<2>().0.iter().any(|p| {
                p[0] < s[0].min(s[6])
                    || p[0] > s[0].max(s[6])
                    || p[1] < s[1].min(s[7])
                    || p[1] > s[1].max(s[7])
            })
        {
            return false;
        }
        area += f64::from(s[0]) * f64::from(s[7]) - f64::from(s[1]) * f64::from(s[6]);
    }
    let expected = 2. * f64::from(hi[0] - lo[0]) * f64::from(hi[1] - lo[1]);
    (area.abs() - expected).abs() <= expected * 1e-6
}
fn segments(o: &VectorObject) -> Option<CurveGeometry> {
    crate::performance::count("native_path_parses", 1);
    let mut path = Path::from_svg(&o.path.data)?;
    if o.stroke.is_some() {
        let _outline_timer = crate::performance::time("native_stroke_outline");
        use lumapaint_core::stroke::{LineCap, LineJoin};
        if o.fill.is_some() || !native_stroke_style(o) {
            return None;
        }
        if let Some(shortest) = o.stroke_style.dash_array.iter().copied().reduce(f32::min) {
            let length: f32 = path
                .points()
                .windows(2)
                .map(|pair| (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y))
                .sum();
            if !length.is_finite() || length / shortest > 128. {
                return None;
            }
        }
        let mut paint = skia_safe::Paint::default();
        paint
            .set_style(skia_safe::paint::Style::Stroke)
            .set_stroke_width(o.stroke_width)
            .set_stroke_miter(o.stroke_style.miter_limit)
            .set_stroke_cap(match o.stroke_style.cap {
                LineCap::Round => skia_safe::paint::Cap::Round,
                LineCap::Square => skia_safe::paint::Cap::Square,
                _ => skia_safe::paint::Cap::Butt,
            })
            .set_stroke_join(match o.stroke_style.join {
                LineJoin::Round => skia_safe::paint::Join::Round,
                LineJoin::Bevel => skia_safe::paint::Join::Bevel,
                _ => skia_safe::paint::Join::Miter,
            });
        if !o.stroke_style.dash_array.is_empty() {
            let mut intervals = o.stroke_style.dash_array.clone();
            if !intervals.len().is_multiple_of(2) {
                intervals.extend_from_within(..);
            }
            paint.set_path_effect(skia_safe::PathEffect::dash(
                &intervals,
                o.stroke_style.dash_offset,
            )?);
        }
        let mut builder = skia_safe::PathBuilder::default();
        // Fixed precision keeps retained outlines independent of viewport zoom.
        // The 32-segment admission limit still applies to the resulting outline.
        if !skia_safe::path_utils::fill_path_with_paint(
            &path,
            &paint,
            &mut builder,
            None,
            Some(skia_safe::Matrix::scale((16., 16.))),
        ) {
            return None;
        }
        let outline: Path = builder.into();
        path = if simple_stroke_style(o) {
            normalize_stroke_rectangles(&outline).unwrap_or(outline)
        } else {
            outline
        };
    }
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
            0.,
            0.,
            0.,
            0.,
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
                    0.,
                    0.,
                    0.,
                    0.,
                ]);
                last = Some(c);
            }
            PathVerb::Cubic => {
                let a = local(p[0]);
                let b = local(p[1]);
                let c = local(p[2]);
                let d = local(p[3]);
                result.push([
                    a[0], a[1], b[0], b[1], c[0], c[1], d[0], d[1], 0., 0., 0., 0.,
                ]);
                last = Some(d);
            }
            PathVerb::Conic => {
                let weight = item.conic_weight();
                if !weight.is_finite() || weight <= 0. || weight > 1. {
                    return None;
                }
                let a = local(p[0]);
                let b = local(p[1]);
                let c = local(p[2]);
                result.push([
                    a[0], a[1], b[0], b[1], c[0], c[1], c[0], c[1], weight, 1., 0., 0.,
                ]);
                last = Some(c);
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
    if o.stroke.is_some()
        && simple_stroke_style(o)
        && result.len().is_multiple_of(4)
        && result.as_chunks::<4>().0.iter().all(|r| exact_rectangle(r))
    {
        if !result.len().is_multiple_of(4) {
            return None;
        }
        for rectangle in result.as_chunks::<4>().0 {
            let lo = [
                rectangle.iter().map(|s| s[0]).fold(f32::INFINITY, f32::min),
                rectangle.iter().map(|s| s[1]).fold(f32::INFINITY, f32::min),
            ];
            let hi = [
                rectangle
                    .iter()
                    .map(|s| s[0])
                    .fold(f32::NEG_INFINITY, f32::max),
                rectangle
                    .iter()
                    .map(|s| s[1])
                    .fold(f32::NEG_INFINITY, f32::max),
            ];
            if lo[0] >= hi[0] || lo[1] >= hi[1] {
                return None;
            }
            for (i, segment) in rectangle.iter().enumerate() {
                if !(segment[0] == lo[0] || segment[0] == hi[0])
                    || !(segment[1] == lo[1] || segment[1] == hi[1])
                    || segment[6..8] != rectangle[(i + 1) % 4][..2]
                {
                    return None;
                }
            }
        }
    }
    Some((result, center, bounds))
}
impl Geometry {
    fn bytes(&self) -> u64 {
        self._curves.size() + self.uniform.size() + self.indirect.size() + self.paint.bytes()
    }

    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        layer: &SvgLayer,
        o: &VectorObject,
        clips: &[usize],
        viewport: Viewport,
    ) -> Option<Self> {
        let (data, center, bounds) = segments(o)?;
        let paint = paint::Paint::new(device, o, center, layer, clips, viewport)?;
        let curves = crate::gpu_metrics::create_buffer_init!(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Resident cubic control points"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::STORAGE,
            }
        );
        let uniform = crate::gpu_metrics::create_buffer!(
            device,
            &wgpu::BufferDescriptor {
                label: Some("Rebased shape display"),
                size: 80,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }
        );
        let indirect = crate::gpu_metrics::create_buffer_init!(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Native shape indirect drawing"),
                contents: bytemuck::cast_slice(&[6u32, 1, 0, 0]),
                usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
            }
        );
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: paint.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: paint.stops.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: paint.clip_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: paint.clip_curves.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&paint.mask_view),
                },
            ],
        });
        let additional_cost = paint.cost();
        Some(Self {
            layer_id: layer.id.clone(),
            paint,
            path: o.path.clone(),
            stroke_key: stroke_key(o),
            straight_stroke: o.stroke.is_some()
                && (simple_stroke_style(o)
                    || (o.stroke_style.cap == lumapaint_core::stroke::LineCap::Round
                        && o.stroke_style.dash_array.is_empty()
                        && o.transform[0] == o.transform[3]
                        && o.transform[0] > 0.))
                && Path::from_svg(&o.path.data).is_some_and(|p| {
                    let points = p.points();
                    points.len() == 2
                        && p.verbs() == [PathVerb::Move, PathVerb::Line]
                        && (points[0].x == points[1].x || points[0].y == points[1].y)
                }),
            exact_rectangle: exact_rectangle(&data),
            rectangular_stroke: o.stroke.is_some()
                && simple_stroke_style(o)
                && data.len().is_multiple_of(4)
                && data.as_chunks::<4>().0.iter().all(|r| exact_rectangle(r)),
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
                    if p[9] == 0. && line.iter().zip(&p[2..6]).all(|(a, b)| (a - b).abs() < 1e-4) {
                        1
                    } else {
                        35
                    }
                })
                .sum::<usize>()
                .saturating_add(additional_cost),
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
        let fill = o.fill.or(o.stroke).unwrap().color;
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
            if o.stroke.is_none() && o.path.fill_rule == FillRule::EvenOdd {
                1.
            } else {
                0.
            },
            if self.rectangular_stroke
                || (self.exact_rectangle
                    && b == 0.
                    && c == 0.
                    && rectangle_pixel_aligned(o, v, offset, self.bounds))
            {
                1.
            } else {
                0.
            },
            f32::from(o.stroke.is_some() || b != 0. || c != 0. || a != d || a <= 0.),
        ];
        if self.last_uniform != Some(uniform) {
            crate::gpu_metrics::write_buffer!(
                queue,
                &self.uniform,
                0,
                bytemuck::cast_slice(&uniform)
            );
            self.last_uniform = Some(uniform);
        }
        let visible = bounds[2] > bounds[0] && bounds[3] > bounds[1];
        let indirect = [6u32, u32::from(visible), 0, 0];
        if self.last_indirect != Some(indirect) {
            crate::gpu_metrics::write_buffer!(
                queue,
                &self.indirect,
                0,
                bytemuck::cast_slice(&indirect)
            );
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
    fn rectangle_coverage_rejects_nonrectangular_or_curved_contours() {
        for path in ["M0 0H20V10H0Z", "M20 10H0V0H20Z"] {
            assert!(exact_rectangle(&segments(&object(path)).unwrap().0));
        }
        for path in [
            "M0 0L20 0L15 10L0 10Z",
            "M0 0L20 10L20 0L0 10Z",
            "M0 0C0 -5 20 -5 20 0V10H0Z",
            "M0 0H20V10H0ZM5 2H15V8H5Z",
        ] {
            assert!(
                !exact_rectangle(&segments(&object(path)).unwrap().0),
                "{path}"
            );
        }
    }
    fn stroke_object() -> VectorObject {
        let mut o = object("M20 64H108");
        o.stroke = o.fill.take();
        o.stroke_width = 8.;
        o
    }
    #[test]
    fn native_linear_stroke_bounds_caps_dashes_and_fallback_are_exact() {
        use lumapaint_core::stroke::{LineCap, StrokeAlignment};
        let mut o = stroke_object();
        assert!(supported(&o));
        assert_eq!(segments(&o).unwrap().2, [20., 60., 108., 68.]);
        o.stroke_style.cap = LineCap::Square;
        assert_eq!(segments(&o).unwrap().2, [16., 60., 112., 68.]);
        o.stroke_style.cap = LineCap::Butt;
        o.stroke_style.dash_array = vec![8.];
        let (curves, _, bounds) = segments(&o).unwrap();
        assert_eq!(curves.len(), 24);
        assert_eq!(bounds, [20., 60., 108., 68.]);
        o.stroke_style.cap = LineCap::Round;
        assert!(supported(&o));
        o.stroke_style.cap = LineCap::Butt;
        o.stroke_style.alignment = StrokeAlignment::Inside;
        assert!(!supported(&o));
        o.stroke_style = Default::default();
        o.transform[1] = 0.5;
        assert!(!supported(&o));
        o.transform[1] = 0.;
        o.stroke.as_mut().unwrap().color[3] = 128;
        assert!(!supported(&o));
        o.stroke.as_mut().unwrap().color[3] = 255;
        assert!(
            normalize_stroke_rectangles(&Path::from_svg("M0 0H10V5H5V10H0Z").unwrap()).is_none()
        );
        o.path.data = "M20 64C40 0 80 128 108 64".into();
        assert!(
            segments(&o).is_some(),
            "bounded curve outlines are retained without SVG rasterization"
        );
        o.path.data = "M0 0H10000".into();
        o.stroke_style.dash_array = vec![1., 1.];
        assert!(
            segments(&o).is_none(),
            "native curve/work budget remains bounded"
        );
    }
    #[test]
    fn round_strokes_keep_exact_rational_arcs_and_polyline_joins() {
        use lumapaint_core::stroke::{LineCap, LineJoin};
        let mut o = stroke_object();
        o.stroke_style.cap = LineCap::Round;
        o.stroke_style.join = LineJoin::Round;
        let (curves, _, bounds) = segments(&o).unwrap();
        assert_eq!(bounds, [16., 60., 112., 68.]);
        assert!(curves.iter().any(|s| s[9] == 1.));
        assert!(curves
            .iter()
            .filter(|s| s[9] == 1.)
            .all(|s| s[8] > 0. && s[8] <= 1.));
        assert!(curves.len() <= 32);
        o.path.data = "M24 104L64 24L104 104".into();
        assert!(segments(&o).is_some());
        o.path.data = "M24 104C24 16 104 16 104 104".into();
        assert!(segments(&o).is_some());
        o.stroke_style.dash_array = vec![0.1, 0.1];
        assert!(
            segments(&o).is_none(),
            "dense strokes still use compatibility rendering"
        );
    }

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
            fallback_reasons: HashMap::new(),
            spatial: lumapaint_core::scene::spatial::SpatialIndex::build([(
                0,
                native_drawing_bounds(&layer.vector_objects[0]),
            )]),
            positions: [("curve".into(), 0)].into(),
            local_bounds: vec![segments(&layer.vector_objects[0]).map(|(_, _, bounds)| bounds)],
            cursor: d.scene_journal().cursor(),
            clip_indices: Vec::new(),
            object_ids: Vec::new(),
            last_refits: 0,
            journal_id: d.scene_journal().instance_id(),
            layer_sequence: d.scene_journal().layer_sequence(&layer.id),
        };
        let original = crate::vector::rasterize_svg(&layer.source, 64, 64).unwrap();
        assert!(d.move_selected_vectors(30., 0.).unwrap());
        let layer = d.svg_layers().next().unwrap();
        crate::performance::take();
        assert!(verified.refresh(layer, d.scene_journal()));
        assert!(!crate::performance::take()
            .counts
            .contains_key("native_path_parses"));
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
    fn stroke_rotation_invalidates_transform_only_native_certification() {
        let mut document = lumapaint_core::document::Document::default();
        let id = document.add_vector_layer().unwrap();
        document.upsert_vector_object(&id, stroke_object()).unwrap();
        document
            .select_vector_objects(vec!["curve".into()])
            .unwrap();
        let layer = document.svg_layers().next().unwrap();
        let bounds = segments(&layer.vector_objects[0]).unwrap().2;
        let mut verified = VerifiedLayer {
            source_key: (layer.source.as_ptr() as usize, layer.source.len()),
            compatible: true,
            fallback_reasons: HashMap::new(),
            spatial: lumapaint_core::scene::spatial::SpatialIndex::build([(0, Some(bounds))]),
            positions: [("curve".into(), 0)].into(),
            local_bounds: vec![Some(bounds)],
            cursor: document.scene_journal().cursor(),
            clip_indices: Vec::new(),
            object_ids: Vec::new(),
            last_refits: 0,
            journal_id: document.scene_journal().instance_id(),
            layer_sequence: document.scene_journal().layer_sequence(&layer.id),
        };
        document
            .rotate_selected_vectors([64., 64.], std::f32::consts::FRAC_PI_4)
            .unwrap();
        assert!(!verified.refresh(
            document.svg_layers().next().unwrap(),
            document.scene_journal()
        ));
        assert_eq!(verified.last_refits, 0);
        assert_eq!(verified.local_bounds, vec![Some(bounds)]);
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
        assert!(segments(&object("M0 0A10 10 0 0 1 20 0Z"))
            .unwrap()
            .0
            .iter()
            .any(|s| s[9] == 1.));
        let mut stroked = o;
        stroked.stroke = stroked.fill;
        assert!(!supported(&stroked));
    }
    #[test]
    fn unrelated_edits_reuse_cached_fallback_but_source_visibility_and_forks_invalidate() {
        let mut document = lumapaint_core::document::Document::default();
        let id = document.add_vector_layer().unwrap();
        document
            .upsert_vector_object(&id, object("M0 0L10 0L10 10Z"))
            .unwrap();
        let layer = document.svg_layers().find(|l| l.id == id).unwrap();
        let mut cached = VerifiedLayer {
            source_key: (layer.source.as_ptr() as usize, layer.source.len()),
            compatible: false,
            fallback_reasons: [("fallback.native_source_mismatch", 1)].into(),
            spatial: Default::default(),
            positions: Default::default(),
            local_bounds: Vec::new(),
            cursor: document.scene_journal().cursor(),
            clip_indices: Vec::new(),
            object_ids: Vec::new(),
            last_refits: 0,
            journal_id: document.scene_journal().instance_id(),
            layer_sequence: document.scene_journal().layer_sequence(&id),
        };
        let old_cursor = cached.cursor;
        document.add_vector_layer().unwrap();
        let layer = document.svg_layers().find(|l| l.id == id).unwrap();
        assert!(document.scene_journal().cursor() > old_cursor);
        assert!(cached.refresh(layer, document.scene_journal()));
        assert!(!cached.compatible);
        assert_eq!(cached.cursor, document.scene_journal().cursor());
        assert_eq!(cached.last_refits, 0);
        let mut changed_source = layer.clone();
        changed_source.source.push(' ');
        assert!(!cached.refresh(&changed_source, document.scene_journal()));
        let fork = document.clone_for_rendering();
        assert!(!cached.refresh(layer, fork.scene_journal()));
        document.toggle_layer(&id).unwrap();
        assert!(!cached.refresh(
            document.svg_layers().find(|l| l.id == id).unwrap(),
            document.scene_journal()
        ));
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
            cache.encode(&mut encoder);
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
        // Normal-zoom policy must reject the unqualified cubic before parsing,
        // verification SVG generation, BVH building or GPU geometry allocation.
        assert!(!cache.prepare_display(
            &device,
            &queue,
            &layer,
            v,
            (&lumapaint_core::scene::Journal::default(), &[]),
            [0., 0.]
        ));
        assert_eq!(cache.metrics.svg_generations, 0);
        assert_eq!(cache.metrics.bvh_builds, 0);
        assert_eq!(cache.metrics.geometry_builds, 0);
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
        // Diagnostic quality gate: screen-space compatibility raster versus native
        // analytic AA. Report edge errors separately; mean error alone can hide them.
        let journal = lumapaint_core::scene::Journal::default();
        for zoom in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
            let probe_view = v.with_screen_zoom(zoom);
            let mut probe_cache = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            assert!(probe_cache.prepare(
                &device,
                &queue,
                &layer,
                probe_view,
                (&journal, &[]),
                [0., 0.]
            ));
            let actual = render(&probe_cache, &layer);
            assert!(
                actual
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[..3].iter().all(|&c| c <= p[3].saturating_add(1))),
                "Encoded premultiplied RGB must not exceed alpha"
            );
            let mut reference_object = o.clone();
            reference_object.transform = [zoom, 0., 0., zoom, 64. * (1. - zoom), 64. * (1. - zoom)];
            let source = lumapaint_core::document::vector_svg(128, 128, &[reference_object]);
            let reference = crate::vector::with_compatibility_renderer(|| {
                crate::vector::rasterize_svg(&source, 128, 128)
            })
            .unwrap();
            let diffs: Vec<_> = actual
                .as_chunks::<4>()
                .0
                .iter()
                .zip(reference.pixels.as_chunks::<4>().0.iter())
                .map(|(a, b)| a[3].abs_diff(b[3]))
                .collect();
            let sum: usize = diffs.iter().map(|&d| usize::from(d)).sum();
            let over_one = diffs.iter().filter(|&&d| d > 1).count();
            let max = diffs.iter().max().copied().unwrap();
            eprintln!("native_quality zoom={zoom} alpha_mean={:.6} alpha_max={max} pixels_over_one={over_one}", sum as f64 / diffs.len() as f64);
            // This is a diagnostic, not authorization to change normal-zoom routing.
            assert_eq!(actual.len(), reference.pixels.len());
        }
        // Phase 6: shared SVG stop interpolation, affine paint and nested masks.
        for kind in [
            lumapaint_core::gradient::GradientKind::Linear,
            lumapaint_core::gradient::GradientKind::Radial,
        ] {
            for method in [
                lumapaint_core::gradient::GradientMethod::Classic,
                lumapaint_core::gradient::GradientMethod::Linear,
                lumapaint_core::gradient::GradientMethod::Perceptual,
            ] {
                for clipped in [false, true] {
                    let mut painted = object("M40 40H88V88H40Z");
                    painted.fill_gradient = Some(lumapaint_core::gradient::Gradient {
                        geometry: if clipped {
                            Some([31., 6., -4., 21., 48., 42.])
                        } else {
                            None
                        },
                        pixel_style: None,
                        kind,
                        angle: 31.,
                        aspect: 0.7,
                        dither: false,
                        method,
                        stops: vec![
                            lumapaint_core::gradient::GradientStop {
                                position: 0.,
                                color: [255, 20, 80, 255],
                                midpoint: 0.27,
                            },
                            lumapaint_core::gradient::GradientStop {
                                position: 0.55,
                                color: [10, 240, 40, 128],
                                midpoint: 0.73,
                            },
                            lumapaint_core::gradient::GradientStop {
                                position: 1.,
                                color: [30, 40, 255, 255],
                                midpoint: 0.5,
                            },
                        ],
                    });
                    painted.opacity = 0.7;
                    let mut probe = layer.clone();
                    probe.vector_objects = vec![painted];
                    if clipped {
                        probe.vector_objects[0].group_path = vec!["outer".into(), "inner".into()];
                        for (id, path) in
                            [("outer", "M44 44H84V84H44Z"), ("inner", "M48 48H80V80H48Z")]
                        {
                            let mut mask = object(path);
                            mask.id = id.into();
                            mask.group_path = vec![id.into()];
                            mask.clipping_group = Some(id.into());
                            mask.fill = None;
                            probe.vector_objects.push(mask);
                        }
                    }
                    probe.source =
                        lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
                    let mut retained = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
                    for zoom in [1., 2.] {
                        retained.begin_frame();
                        assert!(retained.prepare_display(
                            &device,
                            &queue,
                            &probe,
                            v.with_screen_zoom(zoom),
                            (&journal, &[]),
                            [0., 0.]
                        ));
                        let actual = render(&retained, &probe);
                        let reference_objects: Vec<_> = probe
                            .vector_objects
                            .iter()
                            .cloned()
                            .map(|mut o| {
                                let [a, b, c, d, e, f] = o.transform;
                                o.transform = [
                                    a * zoom,
                                    b * zoom,
                                    c * zoom,
                                    d * zoom,
                                    e * zoom + 64. * (1. - zoom),
                                    f * zoom + 64. * (1. - zoom),
                                ];
                                o
                            })
                            .collect();
                        let source =
                            lumapaint_core::document::vector_svg(128, 128, &reference_objects);
                        let reference = crate::vector::with_compatibility_renderer(|| {
                            crate::vector::rasterize_svg(&source, 128, 128)
                        })
                        .unwrap();
                        let max = actual
                            .iter()
                            .zip(&reference.pixels)
                            .map(|(a, b)| a.abs_diff(*b))
                            .max()
                            .unwrap();
                        let mean = actual
                            .iter()
                            .zip(&reference.pixels)
                            .map(|(a, b)| usize::from(a.abs_diff(*b)))
                            .sum::<usize>() as f64
                            / actual.len() as f64;
                        eprintln!("native_paint kind={kind:?} method={method:?} clipped={clipped} zoom={zoom} rgba_max={max} rgba_mean={mean:.6}");
                        assert!(max <= 3, "gradient/clip mismatch {max}");
                        if kind == lumapaint_core::gradient::GradientKind::Linear
                            && method == lumapaint_core::gradient::GradientMethod::Classic
                            && !clipped
                            && zoom == 2.
                        {
                            let upload = device.create_texture(&wgpu::TextureDescriptor {
                                label: Some("Reference SVG upload benchmark"),
                                size: wgpu::Extent3d {
                                    width: 128,
                                    height: 128,
                                    depth_or_array_layers: 1,
                                },
                                mip_level_count: 1,
                                sample_count: 1,
                                dimension: wgpu::TextureDimension::D2,
                                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                                usage: wgpu::TextureUsages::COPY_DST
                                    | wgpu::TextureUsages::TEXTURE_BINDING,
                                view_formats: &[],
                            });
                            let mut before = Vec::new();
                            let mut after = Vec::new();
                            let mut reuses = 0;
                            for _ in 0..50 {
                                let started = std::time::Instant::now();
                                let raster = crate::vector::with_compatibility_renderer(|| {
                                    crate::vector::rasterize_svg(&source, 128, 128)
                                })
                                .unwrap();
                                queue.write_texture(
                                    upload.as_image_copy(),
                                    &raster.pixels,
                                    wgpu::TexelCopyBufferLayout {
                                        offset: 0,
                                        bytes_per_row: Some(512),
                                        rows_per_image: Some(128),
                                    },
                                    wgpu::Extent3d {
                                        width: 128,
                                        height: 128,
                                        depth_or_array_layers: 1,
                                    },
                                );
                                before.push(started.elapsed().as_nanos());
                                retained.begin_frame();
                                let started = std::time::Instant::now();
                                assert!(retained.prepare_display(
                                    &device,
                                    &queue,
                                    &probe,
                                    v.with_screen_zoom(zoom),
                                    (&journal, &[]),
                                    [0., 0.]
                                ));
                                after.push(started.elapsed().as_nanos());
                                assert_eq!(retained.metrics.geometry_builds, 0);
                                assert_eq!(retained.metrics.uniform_upload_bytes, 0);
                                assert_eq!(retained.metrics.svg_generations, 0);
                                reuses += retained.metrics.spatial_query_reuses;
                            }
                            before.sort_unstable();
                            after.sort_unstable();
                            eprintln!("native_paint_bench samples=50 before_cpu_ns_p50={} p95={} p99={} after_cpu_ns_p50={} p95={} p99={} before_rgba_upload_per_sample=65536 after_upload=0 query_reuses={reuses}",before[25],before[47],before[49],after[25],after[47],after[49]);
                            assert_eq!(reuses, 50);
                            queue.submit([]);
                            let mut moving_refs = reference_objects.clone();
                            let selected = vec![probe.vector_objects[0].id.clone()];
                            let mut before_move = Vec::new();
                            let mut after_move = Vec::new();
                            let mut moved_bytes = 0;
                            for i in 0..50 {
                                let delta = if i % 2 == 0 { [1., 0.] } else { [0., 0.] };
                                moving_refs[0].transform[4] = 64. * (1. - zoom) + delta[0] * zoom;
                                let started = std::time::Instant::now();
                                let moving_source =
                                    lumapaint_core::document::vector_svg(128, 128, &moving_refs);
                                let raster = crate::vector::with_compatibility_renderer(|| {
                                    crate::vector::rasterize_svg(&moving_source, 128, 128)
                                })
                                .unwrap();
                                queue.write_texture(
                                    upload.as_image_copy(),
                                    &raster.pixels,
                                    wgpu::TexelCopyBufferLayout {
                                        offset: 0,
                                        bytes_per_row: Some(512),
                                        rows_per_image: Some(128),
                                    },
                                    wgpu::Extent3d {
                                        width: 128,
                                        height: 128,
                                        depth_or_array_layers: 1,
                                    },
                                );
                                before_move.push(started.elapsed().as_nanos());
                                retained.begin_frame();
                                let started = std::time::Instant::now();
                                assert!(retained.prepare_display(
                                    &device,
                                    &queue,
                                    &probe,
                                    v.with_screen_zoom(zoom),
                                    (&journal, &selected),
                                    delta
                                ));
                                after_move.push(started.elapsed().as_nanos());
                                assert_eq!(retained.metrics.geometry_builds, 0);
                                assert_eq!(retained.metrics.svg_generations, 0);
                                assert_eq!(retained.metrics.geometry_upload_bytes, 0);
                                moved_bytes += retained.metrics.uniform_upload_bytes;
                            }
                            before_move.sort_unstable();
                            after_move.sort_unstable();
                            eprintln!("native_paint_move_bench samples=50 before_cpu_ns_p50={} p95={} p99={} after_cpu_ns_p50={} p95={} p99={} before_rgba_upload_per_sample=65536 after_uniform_total={moved_bytes} geometry_rebuilds=0 svg_generations=0",before_move[25],before_move[47],before_move[49],after_move[25],after_move[47],after_move[49]);
                            queue.submit([]);
                        }
                        retained.begin_frame();
                        assert!(retained.prepare_display(
                            &device,
                            &queue,
                            &probe,
                            v.with_screen_zoom(zoom),
                            (&journal, &[]),
                            [0., 0.]
                        ));
                        assert_eq!(retained.metrics.geometry_builds, 0);
                        assert_eq!(retained.metrics.uniform_upload_bytes, 0);
                        assert_eq!(render(&retained, &probe), actual);
                    }
                }
            }
        }
        for opacity in [1., 0.6] {
            let mut first = object("M40 40H76V76H40Z");
            first.id = "lower".into();
            first.opacity = 0.7;
            first.group_path = vec!["art".into()];
            let mut second = object("M52 52H88V88H52Z");
            second.id = "upper".into();
            second.opacity = 0.55;
            second.group_path = vec!["art".into()];
            second.fill.as_mut().unwrap().color = [240, 80, 20, 255];
            let mut probe = layer.clone();
            probe.opacity = opacity;
            probe.vector_objects = vec![first, second];
            probe.source = lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
            let mut retained = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            for zoom in [1., 2.] {
                for reversed in [false, true] {
                    if reversed {
                        probe.vector_objects.reverse();
                        probe.source =
                            lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
                    }
                    assert!(retained.prepare_display(
                        &device,
                        &queue,
                        &probe,
                        v.with_screen_zoom(zoom),
                        (&journal, &[]),
                        [0., 0.]
                    ));
                    assert!(retained.ready(&probe));
                    let actual = render(&retained, &probe);
                    let transformed: Vec<_> = probe
                        .vector_objects
                        .iter()
                        .cloned()
                        .map(|mut o| {
                            o.transform =
                                [zoom, 0., 0., zoom, 64. * (1. - zoom), 64. * (1. - zoom)];
                            o
                        })
                        .collect();
                    let source = lumapaint_core::document::vector_svg(128, 128, &transformed)
                        .replacen('>', &format!("><g opacity=\"{opacity}\">"), 1)
                        .replace("</svg>", "</g></svg>");
                    let reference = crate::vector::with_compatibility_renderer(|| {
                        crate::vector::rasterize_svg(&source, 128, 128)
                    })
                    .unwrap();
                    let max = actual
                        .iter()
                        .zip(&reference.pixels)
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap();
                    eprintln!("native_group opacity={opacity} reversed={reversed} zoom={zoom} rgba_max={max}");
                    assert!(max <= 2, "group composition mismatch {max}");
                    retained.begin_frame();
                    assert!(retained.prepare_display(
                        &device,
                        &queue,
                        &probe,
                        v.with_screen_zoom(zoom),
                        (&journal, &[]),
                        [0., 0.]
                    ));
                    assert!(!retained.composites[&probe.id].dirty.get());
                    assert_eq!(retained.metrics.uniform_upload_bytes, 0);
                    assert_eq!(render(&retained, &probe), actual);
                }
            }
        }
        for kind in [
            lumapaint_core::gradient::GradientKind::Linear,
            lumapaint_core::gradient::GradientKind::Radial,
        ] {
            let mut stroke = stroke_object();
            stroke.stroke_gradient = Some(lumapaint_core::gradient::Gradient {
                geometry: None,
                pixel_style: None,
                kind,
                angle: 25.,
                aspect: 0.75,
                dither: false,
                method: lumapaint_core::gradient::GradientMethod::Classic,
                stops: vec![
                    lumapaint_core::gradient::GradientStop {
                        position: 0.,
                        color: [255, 50, 10, 255],
                        midpoint: 0.35,
                    },
                    lumapaint_core::gradient::GradientStop {
                        position: 1.,
                        color: [20, 60, 255, 128],
                        midpoint: 0.5,
                    },
                ],
            });
            let mut probe = layer.clone();
            probe.vector_objects = vec![stroke.clone()];
            probe.source = lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
            let mut retained = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            for zoom in [1., 2.] {
                assert!(retained.prepare_display(
                    &device,
                    &queue,
                    &probe,
                    v.with_screen_zoom(zoom),
                    (&journal, &[]),
                    [0., 0.]
                ));
                let actual = render(&retained, &probe);
                let mut reference_object = stroke.clone();
                reference_object.transform =
                    [zoom, 0., 0., zoom, 64. * (1. - zoom), 64. * (1. - zoom)];
                let source = lumapaint_core::document::vector_svg(128, 128, &[reference_object]);
                let reference = crate::vector::with_compatibility_renderer(|| {
                    crate::vector::rasterize_svg(&source, 128, 128)
                })
                .unwrap();
                let max = actual
                    .iter()
                    .zip(&reference.pixels)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                eprintln!("native_stroke_gradient kind={kind:?} zoom={zoom} rgba_max={max}");
                assert!(max <= 3);
            }
        }
        // Clip shape, transform and paint updates must retain unrelated geometry.
        for mask_path in [
            "M44 44H84V84H44Z M52 52H76V76H52Z",
            "M44 64A20 20 0 1 0 84 64A20 20 0 1 0 44 64Z",
        ] {
            let mut painted = object("M36 36H92V92H36Z");
            painted.group_path = vec!["clip".into()];
            let mut mask = object(mask_path);
            mask.id = "mask".into();
            mask.group_path = vec!["clip".into()];
            mask.clipping_group = Some("clip".into());
            mask.fill = None;
            let mut probe = layer.clone();
            probe.vector_objects = vec![painted, mask];
            probe.source = lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
            let mut retained = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            assert!(retained.prepare_display(
                &device,
                &queue,
                &probe,
                v.with_screen_zoom(1.),
                (&journal, &[]),
                [0., 0.]
            ));
            let normal = render(&retained, &probe);
            let reference = crate::vector::with_compatibility_renderer(|| {
                crate::vector::rasterize_svg(&probe.source, 128, 128)
            })
            .unwrap();
            assert!(normal
                .iter()
                .zip(&reference.pixels)
                .all(|(a, b)| a.abs_diff(*b) <= 1));
            assert!(normal
                .as_chunks::<4>()
                .0
                .iter()
                .zip(reference.pixels.as_chunks::<4>().0)
                .all(|(a, b)| a[3] == b[3]));
            retained.begin_frame();
            for delta in [[0., 0.], [3., 2.], [0.25, 0.5], [0., 0.]] {
                assert!(retained.prepare_display(
                    &device,
                    &queue,
                    &probe,
                    v.with_screen_zoom(2.),
                    (&journal, &["mask".into()]),
                    delta
                ));
                let actual = render(&retained, &probe);
                let refs: Vec<_> = probe
                    .vector_objects
                    .iter()
                    .cloned()
                    .map(|mut o| {
                        let shift = if o.id == "mask" { delta } else { [0., 0.] };
                        o.transform = [2., 0., 0., 2., -64. + 2. * shift[0], -64. + 2. * shift[1]];
                        o
                    })
                    .collect();
                let source = lumapaint_core::document::vector_svg(128, 128, &refs);
                let reference = crate::vector::with_compatibility_renderer(|| {
                    crate::vector::rasterize_svg(&source, 128, 128)
                })
                .unwrap();
                let diffs: Vec<_> = actual
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(reference.pixels.as_chunks::<4>().0)
                    .map(|(a, b)| a[3].abs_diff(b[3]))
                    .collect();
                let mean = diffs.iter().map(|&v| usize::from(v)).sum::<usize>() as f64
                    / diffs.len() as f64;
                let max = diffs.iter().max().unwrap();
                eprintln!("native_clip path={mask_path} offset={delta:?} alpha_mean={mean:.6} alpha_max={max}");
                assert_eq!(
                    *max, 0,
                    "clip edge coverage must match the compatibility mask"
                );
                retained.begin_frame();
                assert!(retained.prepare_display(
                    &device,
                    &queue,
                    &probe,
                    v.with_screen_zoom(2.),
                    (&journal, &["mask".into()]),
                    delta
                ));
                assert_eq!(retained.metrics.geometry_builds, 0);
                assert_eq!(retained.metrics.clip_mask_upload_bytes, 0);
                assert_eq!(render(&retained, &probe), actual);
            }
            let mut huge = v;
            huge.width = 8192;
            huge.height = 8192;
            probe.opacity = 0.5;
            retained.begin_frame();
            assert!(
                !retained.prepare(&device, &queue, &probe, huge, (&journal, &[]), [0., 0.]),
                "oversized isolation target must fall back"
            );
        }
        {
            let mut probe = layer.clone();
            probe.vector_objects = vec![object("M40 40H88V88H40Z")];
            for i in 0..8 {
                let id = format!("clip-{i}");
                probe.vector_objects[0].group_path.push(id.clone());
                let mut mask = object("M48 48H80V80H48Z");
                mask.id = id.clone();
                mask.group_path = vec![id.clone()];
                mask.clipping_group = Some(id);
                mask.fill = None;
                probe.vector_objects.push(mask);
            }
            probe.source = lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
            let mut retained = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            assert!(retained.prepare_display(
                &device,
                &queue,
                &probe,
                v.with_screen_zoom(1.),
                (&journal, &[]),
                [0., 0.]
            ));
            let actual = render(&retained, &probe);
            let reference = crate::vector::with_compatibility_renderer(|| {
                crate::vector::rasterize_svg(&probe.source, 128, 128)
            })
            .unwrap();
            assert_eq!(actual, reference.pixels);
            let mut extra = probe.vector_objects[1].clone();
            extra.id = "extra-mask".into();
            extra.group_path = vec!["extra".into()];
            extra.clipping_group = Some("extra".into());
            probe.vector_objects[0].group_path.push("extra".into());
            probe.vector_objects.push(extra);
            probe.source = lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
            retained.begin_frame();
            assert!(
                !retained.prepare_display(
                    &device,
                    &queue,
                    &probe,
                    v.with_screen_zoom(1.),
                    (&journal, &[]),
                    [0., 0.]
                ),
                "too many masks must fall back safely"
            );
            assert!(!retained.ready.contains(&probe.id));
        }
        for path in [
            "M56 64L72 64",
            "M24 104L64 24L104 104",
            "M24 104C24 16 104 16 104 104",
            "M52 72C52 48 76 48 76 72",
        ] {
            let mut stroke = stroke_object();
            stroke.path.data = path.into();
            stroke.stroke_style.cap = lumapaint_core::stroke::LineCap::Round;
            stroke.stroke_style.join = lumapaint_core::stroke::LineJoin::Round;
            let mut probe = layer.clone();
            probe.vector_objects = vec![stroke.clone()];
            probe.source = lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
            let mut retained = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            assert!(!retained.prepare_display(
                &device,
                &queue,
                &probe,
                v.with_screen_zoom(1.),
                (&journal, &[]),
                [0., 0.]
            ));
            assert!(!retained.ready.contains(&probe.id));
            for zoom in [2., 4.] {
                retained.begin_frame();
                let admitted = retained.prepare_display(
                    &device,
                    &queue,
                    &probe,
                    v.with_screen_zoom(zoom),
                    (&journal, &[]),
                    [0., 0.],
                );
                assert_eq!(admitted, path == "M56 64L72 64");
                assert!(retained.prepare(
                    &device,
                    &queue,
                    &probe,
                    v.with_screen_zoom(zoom),
                    (&journal, &[]),
                    [0., 0.]
                ));
                let actual = render(&retained, &probe);
                let mut reference_object = stroke.clone();
                reference_object.transform =
                    [zoom, 0., 0., zoom, 64. * (1. - zoom), 64. * (1. - zoom)];
                let source = lumapaint_core::document::vector_svg(128, 128, &[reference_object]);
                let reference = crate::vector::with_compatibility_renderer(|| {
                    crate::vector::rasterize_svg(&source, 128, 128)
                })
                .unwrap();
                let diffs: Vec<_> = actual
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(reference.pixels.as_chunks::<4>().0.iter())
                    .map(|(a, b)| a[3].abs_diff(b[3]))
                    .collect();
                let mean = diffs.iter().map(|&d| usize::from(d)).sum::<usize>() as f64
                    / diffs.len() as f64;
                eprintln!(
                    "conic_stroke path={path} zoom={zoom} alpha_mean={mean:.6} alpha_max={}",
                    diffs.iter().max().unwrap()
                );
                if path == "M56 64L72 64" {
                    assert!(mean < 0.2, "round cap coverage regression: {mean}");
                }
                if path == "M52 72C52 48 76 48 76 72" {
                    assert!(mean < if zoom == 2. { 0.35 } else { 0.4 });
                    assert!(
                        diffs.iter().copied().max().unwrap() <= if zoom == 2. { 72 } else { 64 }
                    );
                    assert!(retained.shapes["curve"].count <= 32);
                }
                assert!(actual
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[..3].iter().all(|&c| c <= p[3].saturating_add(1))));
                retained.begin_frame();
                assert!(retained.prepare(
                    &device,
                    &queue,
                    &probe,
                    v.with_screen_zoom(zoom),
                    (&journal, &[]),
                    [0., 0.]
                ));
                assert_eq!(retained.metrics.geometry_builds, 0);
                assert_eq!(render(&retained, &probe), actual);
            }
        }
        for path in ["M20 20H108V108H20Z", "M108 108V20H20V108Z"] {
            let rectangle = object(path);
            let mut probe = layer.clone();
            probe.vector_objects = vec![rectangle.clone()];
            probe.source = lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
            let mut retained = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            for zoom in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
                retained.begin_frame();
                assert!(retained.prepare_display(
                    &device,
                    &queue,
                    &probe,
                    v.with_screen_zoom(zoom),
                    (&journal, &[]),
                    [0., 0.]
                ));
                let mut reference_object = rectangle.clone();
                reference_object.transform =
                    [zoom, 0., 0., zoom, 64. * (1. - zoom), 64. * (1. - zoom)];
                let source = lumapaint_core::document::vector_svg(128, 128, &[reference_object]);
                let reference = crate::vector::with_compatibility_renderer(|| {
                    crate::vector::rasterize_svg(&source, 128, 128)
                })
                .unwrap();
                assert_eq!(
                    render(&retained, &probe),
                    reference.pixels,
                    "rectangle zoom={zoom}"
                );
            }
            for offset in [[8., 4.], [0.25, 0.5], [0., 0.]] {
                retained.begin_frame();
                crate::performance::take();
                let prepared = retained.prepare_display(
                    &device,
                    &queue,
                    &probe,
                    v,
                    (&journal, std::slice::from_ref(&rectangle.id)),
                    offset,
                );
                if offset == [0.25, 0.5] {
                    assert!(
                        !prepared,
                        "fractional rectangle keeps compatibility quality"
                    );
                    assert_eq!(retained.metrics.geometry_builds, 0);
                    continue;
                }
                assert!(prepared);
                assert_eq!(retained.metrics.geometry_builds, 0);
                assert_eq!(retained.metrics.svg_generations, 0);
                assert_eq!(
                    crate::performance::take()
                        .counts
                        .get("native_path_parses")
                        .copied()
                        .unwrap_or(0),
                    0
                );
                let mut moved = rectangle.clone();
                moved.transform[4] += offset[0];
                moved.transform[5] += offset[1];
                let source = lumapaint_core::document::vector_svg(128, 128, &[moved]);
                let reference = crate::vector::with_compatibility_renderer(|| {
                    crate::vector::rasterize_svg(&source, 128, 128)
                })
                .unwrap();
                assert_eq!(
                    render(&retained, &probe),
                    reference.pixels,
                    "rectangle drag"
                );
            }
        }
        // Exact stroke fixtures have integer-aligned screen bounds and opaque color.
        // Test the production path, including the dashed-path effect and transform reuse.
        for path in ["M20 64H108", "M64 20V108"] {
            for cap in [
                lumapaint_core::stroke::LineCap::Butt,
                lumapaint_core::stroke::LineCap::Square,
            ] {
                for dash in [vec![], vec![8.]] {
                    let mut stroke = stroke_object();
                    stroke.path = object(path).path;
                    stroke.stroke_style.cap = cap;
                    stroke.stroke_style.dash_array = dash;
                    let mut probe = layer.clone();
                    probe.vector_objects = vec![stroke.clone()];
                    probe.source =
                        lumapaint_core::document::vector_svg(128, 128, &probe.vector_objects);
                    let journal = lumapaint_core::scene::Journal::default();
                    let mut stroke_cache = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
                    assert!(stroke_cache.prepare(
                        &device,
                        &queue,
                        &probe,
                        v,
                        (&journal, &[]),
                        [0., 0.]
                    ));
                    let actual = render(&stroke_cache, &probe);
                    let reference = crate::vector::with_compatibility_renderer(|| {
                        crate::vector::rasterize_svg(&probe.source, 128, 128)
                    })
                    .unwrap();
                    assert_eq!(
                        actual, reference.pixels,
                        "stroke cap={cap:?} dash={:?}",
                        stroke.stroke_style.dash_array
                    );
                    for zoom in [0.25, 0.5, 1.0, 1.5, 2.0, 4.0] {
                        let view = v.with_screen_zoom(zoom);
                        stroke_cache.begin_frame();
                        assert!(stroke_cache.prepare_display(
                            &device,
                            &queue,
                            &probe,
                            view,
                            (&journal, &[]),
                            [0., 0.]
                        ));
                        assert_eq!(stroke_cache.metrics.geometry_builds, 0);
                        assert_eq!(stroke_cache.metrics.geometry_upload_bytes, 0);
                        assert_eq!(stroke_cache.metrics.svg_generations, 0);
                        assert_eq!(stroke_cache.metrics.bvh_builds, 0);
                        let mut reference_stroke = stroke.clone();
                        reference_stroke.transform =
                            [zoom, 0., 0., zoom, 64. * (1. - zoom), 64. * (1. - zoom)];
                        let source =
                            lumapaint_core::document::vector_svg(128, 128, &[reference_stroke]);
                        let reference = crate::vector::with_compatibility_renderer(|| {
                            crate::vector::rasterize_svg(&source, 128, 128)
                        })
                        .unwrap();
                        assert_eq!(
                            render(&stroke_cache, &probe),
                            reference.pixels,
                            "stroke normal zoom={zoom} cap={cap:?}"
                        );
                    }
                    stroke_cache.begin_frame();
                    assert!(stroke_cache.prepare_display(
                        &device,
                        &queue,
                        &probe,
                        v,
                        (&journal, &[]),
                        [0., 0.]
                    ));
                    let geometry_address =
                        &stroke_cache.shapes["curve"]._curves as *const _ as usize;
                    stroke_cache.begin_frame();
                    crate::performance::take();
                    assert!(stroke_cache.prepare(
                        &device,
                        &queue,
                        &probe,
                        v,
                        (&journal, &["curve".into()]),
                        [0., 10.]
                    ));
                    assert_eq!(stroke_cache.metrics.geometry_builds, 0);
                    assert_eq!(
                        &stroke_cache.shapes["curve"]._curves as *const _ as usize,
                        geometry_address
                    );
                    assert!(!crate::performance::take()
                        .counts
                        .contains_key("native_path_parses"));
                    let mut moved = stroke;
                    moved.transform[5] = 10.;
                    let source = lumapaint_core::document::vector_svg(128, 128, &[moved]);
                    let reference = crate::vector::with_compatibility_renderer(|| {
                        crate::vector::rasterize_svg(&source, 128, 128)
                    })
                    .unwrap();
                    assert_eq!(render(&stroke_cache, &probe), reference.pixels);
                    stroke_cache.begin_frame();
                    assert!(stroke_cache.prepare(
                        &device,
                        &queue,
                        &probe,
                        v,
                        (&journal, &["curve".into()]),
                        [0.5, 10.5]
                    ));
                    let mut fractional = probe.vector_objects[0].clone();
                    fractional.transform[4] = 0.5;
                    fractional.transform[5] = 10.5;
                    let source = lumapaint_core::document::vector_svg(128, 128, &[fractional]);
                    let reference = crate::vector::with_compatibility_renderer(|| {
                        crate::vector::rasterize_svg(&source, 128, 128)
                    })
                    .unwrap();
                    let actual = render(&stroke_cache, &probe);
                    let differences: Vec<_> = actual
                        .iter()
                        .zip(&reference.pixels)
                        .map(|(a, b)| a.abs_diff(*b))
                        .collect();
                    assert!(
                        differences.iter().all(|&d| d <= 1),
                        "fractional stroke rounding error: {:?}",
                        differences.iter().max()
                    );
                    assert_eq!(stroke_cache.metrics.geometry_builds, 0);
                }
            }
        }
        let mut composite_probe = layer.clone();
        let first_stroke = stroke_object();
        let mut second_stroke = first_stroke.clone();
        second_stroke.id = "second-stroke".into();
        composite_probe.vector_objects = vec![first_stroke, second_stroke];
        composite_probe.source =
            lumapaint_core::document::vector_svg(128, 128, &composite_probe.vector_objects);
        let mut composite_cache = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
        assert!(!composite_cache.prepare(
            &device,
            &queue,
            &composite_probe,
            v,
            (&journal, &[]),
            [0., 0.]
        ));
        assert_eq!(composite_cache.metrics.geometry_builds, 0);
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
        let stable_journal = lumapaint_core::scene::Journal::default();
        assert!(cache.prepare(&device, &queue, &layer, v, (&stable_journal, &[]), [0., 0.]));
        cache.begin_frame();
        assert!(cache.prepare(&device, &queue, &layer, v, (&stable_journal, &[]), [0., 0.]));
        assert_eq!(cache.metrics.spatial_queries, 0);
        assert_eq!(cache.metrics.spatial_query_reuses, 1);
        assert_eq!(cache.draw_lists[&layer.id], vec![0]);
        assert!(!cache.shapes.contains_key("outside"));
        assert_eq!(render(&cache, &layer), pixels);
        cache.begin_frame();
        assert!(cache.prepare(
            &device,
            &queue,
            &layer,
            v,
            (&stable_journal, &["outside".into()]),
            [-10_000., 0.]
        ));
        assert_eq!(cache.draw_lists[&layer.id], vec![0, 1]);
        assert!(cache.shapes.contains_key("outside"));
        cache.begin_frame();
        assert!(cache.prepare(&device, &queue, &layer, v, (&stable_journal, &[]), [0., 0.]));
        assert_eq!(cache.metrics.spatial_queries, 1);
        assert_eq!(
            cache.draw_lists[&layer.id],
            vec![0],
            "dragged offscreen objects must not remain in a reused query"
        );
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
        let source_snapshot_pointer = cache.layer_sources.as_ptr();
        let source_id_pointer = cache.layer_sources[0].0.as_ptr();
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
            assert_eq!(cache.metrics.source_layer_scans, 0);
            assert_eq!(cache.layer_sources.as_ptr(), source_snapshot_pointer);
            assert_eq!(cache.layer_sources[0].0.as_ptr(), source_id_pointer);
            assert_eq!(cache.metrics.svg_generations, 0);
            assert_eq!(cache.metrics.bvh_builds, 0);
            assert_eq!(cache.metrics.bvh_refits, 0);
            assert_eq!(cache.metrics.geometry_builds, 0);
            assert_eq!(cache.metrics.geometry_upload_bytes, 0);
            assert_eq!(cache.metrics.uniform_upload_bytes, 0);
        }
        for count in [1, 16] {
            let mut state = document.document_state();
            state.layer_groups = Default::default();
            state.svg_layers = (0..count)
                .map(|i| {
                    let mut layer = document.svg_layers().next().unwrap().clone();
                    layer.id = format!("bench-layer-{i}");
                    for object in &mut layer.vector_objects {
                        object.id = format!("bench-shape-{i}");
                    }
                    layer.source =
                        lumapaint_core::document::vector_svg(128, 128, &layer.vector_objects);
                    layer
                })
                .collect();
            let bench = lumapaint_core::document::Document::from_document_state(state).unwrap();
            let mut bench_cache = Cache::new(&device, wgpu::TextureFormat::Rgba8UnormSrgb);
            bench_cache.synchronize(&bench);
            bench_cache.begin_frame();
            let iterations = 20_000;
            let start = std::time::Instant::now();
            for _ in 0..iterations {
                std::hint::black_box(
                    std::hint::black_box(&bench)
                        .svg_layers()
                        .map(|l| (l.id.as_str(), l.source.as_ptr() as usize, l.source.len()))
                        .eq(bench_cache
                            .layer_sources
                            .iter()
                            .map(|(id, pointer, length)| (id.as_str(), *pointer, *length))),
                );
            }
            let before = start.elapsed();
            let start = std::time::Instant::now();
            for _ in 0..iterations {
                bench_cache.synchronize(std::hint::black_box(&bench));
            }
            let after = start.elapsed();
            assert_eq!(bench_cache.metrics.source_layer_scans, 0);
            assert_eq!(bench_cache.metrics.live_id_scans, 0);
            eprintln!("native idle synchronization layers={count} iterations={iterations} old_source_walk_host_ns={} new_revision_check_host_ns={} source_layers_scanned=0", before.as_nanos(), after.as_nanos());
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
        let layer_id = document.svg_layers().next().unwrap().id.clone();
        assert!(document.delete_selected_vector_objects().unwrap());
        cache.begin_frame();
        cache.synchronize(&document);
        assert_eq!(cache.metrics.live_id_scans, 0);
        assert!(!cache.shapes.contains_key("curve"));
        assert!(!cache.layer_shapes.contains_key(&layer_id));
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
        assert_eq!(cache.metrics.live_id_scans, 0);
        assert_eq!(render(&cache, restored), moved_pixels);
        document.delete_layer(&layer_id).unwrap();
        cache.begin_frame();
        cache.synchronize(&document);
        assert_eq!(cache.metrics.live_id_scans, 0);
        assert!(!cache.shapes.contains_key("curve"));
        assert!(!cache.layer_shapes.contains_key(&layer_id));
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
        let fallback_journal = lumapaint_core::scene::Journal::default();
        cache.begin_frame();
        crate::performance::take();
        assert!(!cache.prepare(
            &device,
            &queue,
            &layer,
            v,
            (&fallback_journal, &[]),
            [0., 0.]
        ));
        assert_eq!(cache.metrics.svg_generations, 0);
        assert_eq!(cache.metrics.bvh_builds, 0);
        assert!(!crate::performance::take()
            .counts
            .contains_key("native_path_parses"));
        let reasons_capacity = cache.verified[&layer.id].fallback_reasons.capacity();
        assert_eq!(
            cache.verified[&layer.id].fallback_reasons["native_ineligible.stroke"],
            1
        );
        for _ in 0..3 {
            cache.begin_frame();
            crate::performance::take();
            assert!(!cache.prepare(
                &device,
                &queue,
                &layer,
                v,
                (&fallback_journal, &[]),
                [0., 0.]
            ));
            assert_eq!(cache.metrics.svg_generations, 0);
            assert_eq!(cache.metrics.bvh_builds, 0);
            assert_eq!(
                cache.verified[&layer.id].fallback_reasons.capacity(),
                reasons_capacity
            );
            let stats = crate::performance::take();
            assert!(!stats.counts.contains_key("native_path_parses"));
            if crate::performance::enabled() {
                assert_eq!(stats.counts.get("native_ineligible.stroke"), Some(&1));
            }
        }
    }
}
