//! Retained gradient stops and vector clipping paths; no raster frame transfer.
use super::{exact_rectangle, rectangle_pixel_aligned, segments};
use crate::Viewport;
use lumapaint_core::{
    document::SvgLayer,
    gradient::{Gradient, GradientKind},
    vector::{FillRule, VectorObject, VectorPath},
};
use resvg::{tiny_skia, usvg};
use wgpu::util::DeviceExt;
struct Clip {
    id: String,
    path: VectorPath,
    center: [f64; 2],
    bounds: [f64; 4],
    start: usize,
    count: usize,
    rectangular: bool,
    bitmap_key: Option<[f64; 8]>,
}
pub(super) struct Paint {
    gradient: Option<Gradient>,
    clips: Vec<Clip>,
    pub uniform: wgpu::Buffer,
    pub stops: wgpu::Buffer,
    pub clip_uniforms: wgpu::Buffer,
    pub clip_curves: wgpu::Buffer,
    pub masks: wgpu::Texture,
    pub mask_view: wgpu::TextureView,
    mask_size: [u32; 2],
    pub mask_upload_bytes: usize,
    values: [f32; 16],
    last_values: Option<[f32; 16]>,
    last_clips: Vec<[f32; 20]>,
    initialized: bool,
}
impl Paint {
    pub fn precise(&self, layer: &SvgLayer, indices: &[usize], v: Viewport) -> bool {
        self.clips.iter().zip(indices).all(|(clip, &i)| {
            let o = &layer.vector_objects[i];
            let extent = (clip.bounds[2] - clip.bounds[0]).max(clip.bounds[3] - clip.bounds[1]);
            let matrix_scale = o.transform[..4]
                .iter()
                .map(|v| f64::from(*v).powi(2))
                .sum::<f64>()
                .sqrt();
            let error = extent
                * f64::from(f32::EPSILON)
                * 8.
                * matrix_scale
                * f64::from(v.screen_zoom() * v.scale);
            error.is_finite() && error <= 0.25
        })
    }
    pub fn bytes(&self) -> u64 {
        self.uniform.size()
            + self.stops.size()
            + self.clip_uniforms.size()
            + self.clip_curves.size()
            + u64::from(self.mask_size[0])
                * u64::from(self.mask_size[1])
                * self.clips.len().max(1) as u64
    }
    pub fn cost(&self) -> usize {
        self.clips
            .iter()
            .map(|c| if c.rectangular { c.count } else { 1 })
            .sum::<usize>()
            + if self.gradient.is_some() { 12 } else { 0 }
    }
    pub fn normal_clips_supported(
        &self,
        layer: &SvgLayer,
        indices: &[usize],
        v: Viewport,
        selected: &[String],
        offset: [f32; 2],
    ) -> bool {
        self.clips
            .iter()
            .zip(indices)
            .enumerate()
            .all(|(j, (clip, &i))| {
                let mask = &layer.vector_objects[i];
                self.last_clips[j][18] > 1.5
                    || clip.rectangular
                        && mask.transform[1] == 0.
                        && mask.transform[2] == 0.
                        && rectangle_pixel_aligned(
                            mask,
                            v,
                            if selected.contains(&mask.id) {
                                offset
                            } else {
                                [0.; 2]
                            },
                            clip.bounds,
                        )
            })
    }
    pub fn matches(
        &self,
        o: &VectorObject,
        layer: &SvgLayer,
        indices: &[usize],
        v: Viewport,
    ) -> bool {
        self.mask_size
            == if indices.is_empty() {
                [1, 1]
            } else {
                [v.width.clamp(1, 1024), v.height.clamp(1, 1024)]
            }
            && self.gradient.as_ref() == o.fill_gradient.as_ref().or(o.stroke_gradient.as_ref())
            && self.clips.len() == indices.len()
            && self.clips.iter().zip(indices).all(|(c, &i)| {
                c.id == layer.vector_objects[i].id && c.path == layer.vector_objects[i].path
            })
    }
    pub fn new(
        device: &wgpu::Device,
        o: &VectorObject,
        center: [f64; 2],
        layer: &SvgLayer,
        indices: &[usize],
        v: Viewport,
    ) -> Option<Self> {
        if indices.len() > 8 {
            return None;
        }
        let gradient = o
            .fill_gradient
            .as_ref()
            .or(o.stroke_gradient.as_ref())
            .cloned();
        let mut values = [0.; 16];
        let mut stops = Vec::<[f32; 4]>::new();
        if let Some(g) = &gradient {
            g.validate().ok()?;
            if g.dither || g.pixel_style.is_some() {
                return None;
            }
            let mut bounds = lumapaint_core::stroke::path_bounds(&o.path.data)?;
            let padding = o.stroke_width.max(1.) * 0.5;
            if bounds[0] == bounds[2] {
                bounds[0] -= padding;
                bounds[2] += padding;
            }
            if bounds[1] == bounds[3] {
                bounds[1] -= padding;
                bounds[3] += padding;
            }
            let [a, b, c, d, e, f] = g.geometry_in_bounds(bounds).map(f64::from);
            let det = a * d - b * c;
            if !det.is_finite() || det.abs() < 1e-12 {
                return None;
            }
            values[..4].copy_from_slice(&[
                (d / det) as f32,
                (-c / det) as f32,
                (-b / det) as f32,
                (a / det) as f32,
            ]);
            values[4..8].copy_from_slice(&[
                (e - center[0]) as f32,
                (f - center[1]) as f32,
                if g.kind == GradientKind::Linear {
                    1.
                } else {
                    2.
                },
                0.,
            ]);
            for (position, color) in g.svg_stops() {
                stops.push(color.map(|v| f32::from(v) / 255.));
                stops.push([position, 0., 0., 0.]);
            }
            values[7] = (stops.len() / 2) as f32;
        }
        if stops.is_empty() {
            stops.push([0.; 4]);
        }
        let mut clips = Vec::new();
        let mut curves = Vec::<[f32; 12]>::new();
        for &i in indices {
            let mask = &layer.vector_objects[i];
            let mut raw = mask.clone();
            raw.stroke = None;
            raw.fill_gradient = None;
            raw.stroke_gradient = None;
            let (data, center, bounds) = segments(&raw)?;
            clips.push(Clip {
                id: mask.id.clone(),
                path: mask.path.clone(),
                center,
                bounds,
                start: curves.len(),
                count: data.len(),
                rectangular: exact_rectangle(&data),
                bitmap_key: None,
            });
            curves.extend(data);
        }
        if curves.is_empty() {
            curves.push([0.; 12]);
        }
        values[9] = clips.len() as f32;
        let uniform = crate::gpu_metrics::create_buffer!(
            device,
            &wgpu::BufferDescriptor {
                label: Some("Native paint transform"),
                size: 64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false
            }
        );
        let stops = crate::gpu_metrics::create_buffer_init!(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Retained gradient stops"),
                contents: bytemuck::cast_slice(&stops),
                usage: wgpu::BufferUsages::STORAGE
            }
        );
        let clip_curves = crate::gpu_metrics::create_buffer_init!(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Retained clipping curves"),
                contents: bytemuck::cast_slice(&curves),
                usage: wgpu::BufferUsages::STORAGE
            }
        );
        let clip_uniforms = crate::gpu_metrics::create_buffer!(
            device,
            &wgpu::BufferDescriptor {
                label: Some("Clip transforms"),
                size: (clips.len().max(1) * 80) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false
            }
        );
        let mask_size = if clips.is_empty() {
            [1, 1]
        } else {
            [v.width.clamp(1, 1024), v.height.clamp(1, 1024)]
        };
        let masks = crate::gpu_metrics::create_texture!(
            device,
            &wgpu::TextureDescriptor {
                label: Some("Retained compatibility clip masks"),
                size: wgpu::Extent3d {
                    width: mask_size[0],
                    height: mask_size[1],
                    depth_or_array_layers: clips.len().max(1) as u32
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[]
            }
        );
        let mask_view = masks.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let last_clips = vec![[0.; 20]; clips.len()];
        Some(Self {
            gradient,
            masks,
            mask_view,
            mask_size,
            mask_upload_bytes: 0,
            clips,
            uniform,
            stops,
            clip_uniforms,
            clip_curves,
            values,
            last_values: None,
            last_clips,
            initialized: false,
        })
    }
    pub fn update(
        &mut self,
        queue: &wgpu::Queue,
        o: &VectorObject,
        layer: &SvgLayer,
        indices: &[usize],
        v: Viewport,
        drag: (&[String], [f32; 2]),
    ) -> Option<usize> {
        let (selected, offset) = drag;
        self.mask_upload_bytes = 0;
        let mut uploaded = 0;
        self.values[8] = o.opacity;
        if self.last_values != Some(self.values) {
            crate::gpu_metrics::write_buffer!(
                queue,
                &self.uniform,
                0,
                bytemuck::cast_slice(&self.values)
            );
            self.last_values = Some(self.values);
            uploaded += 64;
        }
        for (j, (clip, &i)) in self.clips.iter_mut().zip(indices).enumerate() {
            let mask = &layer.vector_objects[i];
            let [a, b, c, d, e, f] = mask.transform.map(f64::from);
            let s = f64::from(v.screen_zoom() * v.scale);
            let det = (a * d - b * c) * s;
            if !det.is_finite() || det.abs() < 1e-12 {
                continue;
            }
            let delta = if selected.contains(&mask.id) {
                offset
            } else {
                [0.; 2]
            };
            let origin = [
                (a * clip.center[0] + c * clip.center[1] + e + f64::from(delta[0])
                    - f64::from(v.document_width) * 0.5)
                    * s
                    + f64::from(v.width) * 0.5
                    + f64::from(v.pan_x * v.scale),
                (b * clip.center[0] + d * clip.center[1] + f + f64::from(delta[1])
                    - f64::from(v.document_height) * 0.5)
                    * s
                    + f64::from(v.height) * 0.5
                    + f64::from(v.pan_y * v.scale),
            ];
            let mut u = [0.; 20];
            u[..8].copy_from_slice(&[
                origin[0] as f32,
                origin[1] as f32,
                v.width as f32,
                v.height as f32,
                (d / det) as f32,
                (-c / det) as f32,
                (-b / det) as f32,
                (a / det) as f32,
            ]);
            u[16..20].copy_from_slice(&[
                clip.count as f32,
                f32::from(mask.path.fill_rule == FillRule::EvenOdd),
                f32::from(clip.rectangular && b == 0. && c == 0.),
                clip.start as f32,
            ]);
            let analytic = clip.rectangular
                && b == 0.
                && c == 0.
                && rectangle_pixel_aligned(mask, v, delta, clip.bounds);
            if !analytic {
                let mut lo = [f64::INFINITY; 2];
                let mut hi = [f64::NEG_INFINITY; 2];
                for x in [clip.bounds[0], clip.bounds[2]] {
                    for y in [clip.bounds[1], clip.bounds[3]] {
                        let p = [
                            origin[0] + ((x - clip.center[0]) * a + (y - clip.center[1]) * c) * s,
                            origin[1] + ((x - clip.center[0]) * b + (y - clip.center[1]) * d) * s,
                        ];
                        for k in 0..2 {
                            lo[k] = lo[k].min(p[k]);
                            hi[k] = hi[k].max(p[k]);
                        }
                    }
                }
                let left = (lo[0].floor() - 1.).clamp(0., f64::from(v.width));
                let top = (lo[1].floor() - 1.).clamp(0., f64::from(v.height));
                let width = ((hi[0].ceil() + 1.).clamp(left, f64::from(v.width)) - left) as u32;
                let height = ((hi[1].ceil() + 1.).clamp(top, f64::from(v.height)) - top) as u32;
                if width > self.mask_size[0] || height > self.mask_size[1] {
                    crate::performance::count("fallback.native_clip_mask_size", 1);
                    return None;
                }
                u[8..12].copy_from_slice(&[left as f32, top as f32, width as f32, height as f32]);
                u[18] = 2.;
                let matrix = [
                    a * s,
                    b * s,
                    c * s,
                    d * s,
                    origin[0] - (a * clip.center[0] + c * clip.center[1]) * s - left,
                    origin[1] - (b * clip.center[0] + d * clip.center[1]) * s - top,
                ];
                let key = [
                    matrix[0],
                    matrix[1],
                    matrix[2],
                    matrix[3],
                    matrix[4],
                    matrix[5],
                    f64::from(width),
                    f64::from(height),
                ];
                if width > 0 && height > 0 && clip.bitmap_key != Some(key) {
                    let _timer = crate::performance::time("native_clip_mask_raster");
                    let path = clip
                        .path
                        .data
                        .replace('&', "&amp;")
                        .replace('"', "&quot;")
                        .replace('<', "&lt;");
                    let rule = if clip.path.fill_rule == FillRule::EvenOdd {
                        "evenodd"
                    } else {
                        "nonzero"
                    };
                    let [ma, mb, mc, md, me, mf] = matrix;
                    let source=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><path d=\"{path}\" transform=\"matrix({ma} {mb} {mc} {md} {me} {mf})\" fill=\"white\" fill-rule=\"{rule}\"/></svg>");
                    let tree = usvg::Tree::from_str(&source, &usvg::Options::default()).ok()?;
                    let mut image = tiny_skia::Pixmap::new(width, height)?;
                    resvg::render(&tree, tiny_skia::Transform::identity(), &mut image.as_mut());
                    let alpha: Vec<u8> = image
                        .data()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| p[3])
                        .collect();
                    crate::performance::count("native_clip_mask_svg_parses", 1);
                    crate::gpu_metrics::write_texture!(
                        queue,
                        wgpu::TexelCopyTextureInfo {
                            texture: &self.masks,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: 0,
                                y: 0,
                                z: j as u32
                            },
                            aspect: wgpu::TextureAspect::All
                        },
                        &alpha,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(width),
                            rows_per_image: Some(height)
                        },
                        wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1
                        }
                    );
                    self.mask_upload_bytes += alpha.len();
                    crate::performance::count("native_clip_mask_rasterizations", 1);
                    crate::performance::count("native_clip_mask_upload_bytes", alpha.len() as u64);
                    clip.bitmap_key = Some(key);
                }
            }
            if !self.initialized || self.last_clips[j] != u {
                crate::gpu_metrics::write_buffer!(
                    queue,
                    &self.clip_uniforms,
                    (j * 80) as u64,
                    bytemuck::cast_slice(&u)
                );
                self.last_clips[j] = u;
                uploaded += 80;
            }
        }
        self.initialized = true;
        Some(uploaded)
    }
}
