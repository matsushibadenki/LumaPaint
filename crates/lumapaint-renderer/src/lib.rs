//! GPU rendering shared across desktop platforms; native view ownership lives in the host.
use lumapaint_core::document::{
    CanvasColor, Document, PaintProjectionState, Point, Selection, SelectionOperation,
    SelectionShape, Stroke, SvgLayer, HEIGHT, WIDTH,
};
use lumapaint_core::tiles::{
    RasterDab, TileCoord, TileInvalidation, TileUpload, TiledRasterDocument, TILE_SIZE,
};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex, OnceLock};
pub use wgpu;
use wgpu::util::DeviceExt;

fn render_metrics_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("LUMAPAINT_RENDER_METRICS").is_some())
}

#[derive(Default)]
struct VectorDragMetrics {
    unit_reuse: usize,
    partial_reuse: usize,
    cache_builds: usize,
    full_previews: usize,
    upload_bytes: usize,
    cache_setup_ms: f64,
    full_prepare_upload_ms: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub zoom: f32,
    pub dark: bool,
    pub pan_x: f32,
    pub pan_y: f32,
    pub document_width: f32,
    pub document_height: f32,
    pub canvas_color: CanvasColor,
}

impl Viewport {
    fn tile_pixels_per_document_pixel(&self) -> f32 {
        let width = self.width as f32 / self.scale;
        let height = self.height as f32 / self.scale;
        let fit = ((width - 48.0) / self.document_width)
            .min((height - 48.0) / self.document_height)
            .max(0.01);
        fit * self.zoom * self.scale
    }

    fn tile_filter_nearest(&self) -> bool {
        self.tile_pixels_per_document_pixel() < 0.75
    }

    /// Choose preview resolution from the effective physical pixel density,
    /// not the display's backing scale alone.
    pub fn recommended_tile_preview_scale(&self) -> u32 {
        if self.tile_pixels_per_document_pixel() >= 1.5
            && self.document_width <= 4096.0
            && self.document_height <= 4096.0
        {
            2
        } else {
            1
        }
    }

    /// The v1 brush and raster tiles have comparable edge widths only near
    /// whole physical pixels per document pixel. Keep other zooms on v1.
    pub fn compatible_tile_preview_scale(&self) -> Option<u32> {
        let density = self.tile_pixels_per_document_pixel();
        let scale = if (0.9..=1.1).contains(&density) {
            1u32
        } else if (1.8..=2.2).contains(&density) {
            2u32
        } else {
            return None;
        };
        let width = self.document_width as u32;
        let height = self.document_height as u32;
        let pixels = width
            .checked_mul(scale)?
            .checked_mul(height.checked_mul(scale)?)?;
        (pixels <= 16_777_216 && width * scale <= 8192 && height * scale <= 8192).then_some(scale)
    }

    pub fn document_point(&self, x: f32, y: f32) -> Point {
        let width = self.width as f32 / self.scale;
        let height = self.height as f32 / self.scale;
        let fit = ((width - 48.0) / self.document_width)
            .min((height - 48.0) / self.document_height)
            .max(0.01);
        Point {
            x: (x - width * 0.5 - self.pan_x) / (fit * self.zoom) + self.document_width * 0.5,
            y: (y - height * 0.5 - self.pan_y) / (fit * self.zoom) + self.document_height * 0.5,
        }
    }
    pub fn zoom_around(mut self, x: f32, y: f32, zoom: f32) -> Self {
        let anchor = self.document_point(x, y);
        let width = self.width as f32 / self.scale;
        let height = self.height as f32 / self.scale;
        let fit = ((width - 48.0) / self.document_width)
            .min((height - 48.0) / self.document_height)
            .max(0.01);
        self.zoom = zoom;
        self.pan_x = (x - width * 0.5 - (anchor.x - self.document_width * 0.5) * fit * zoom)
            .clamp(-8192.0, 8192.0);
        self.pan_y = (y - height * 0.5 - (anchor.y - self.document_height * 0.5) * fit * zoom)
            .clamp(-8192.0, 8192.0);
        self
    }
    pub fn new(width: f64, height: f64, scale: f64, zoom: f64, dark: bool) -> Result<Self, String> {
        if ![width, height, scale, zoom].iter().all(|v| v.is_finite())
            || width <= 0.0
            || height <= 0.0
            || !(0.5..=8.0).contains(&scale)
            || !(0.25..=4.0).contains(&zoom)
        {
            return Err("Invalid viewport dimensions, scale, or zoom".into());
        }
        let width = (width * scale).round().max(1.0);
        let height = (height * scale).round().max(1.0);
        if width > 8192.0 || height > 8192.0 {
            return Err("Viewport exceeds the 8192-pixel preview limit".into());
        }
        Ok(Self {
            width: width as u32,
            height: height as u32,
            scale: scale as f32,
            zoom: zoom as f32,
            dark,
            pan_x: 0.0,
            pan_y: 0.0,
            document_width: WIDTH,
            document_height: HEIGHT,
            canvas_color: CanvasColor::White,
        })
    }

    pub fn with_pan(mut self, x: f32, y: f32) -> Result<Self, String> {
        if !x.is_finite() || !y.is_finite() || x.abs() > 8192.0 || y.abs() > 8192.0 {
            return Err("Invalid canvas pan offset".into());
        }
        self.pan_x = x;
        self.pan_y = y;
        Ok(self)
    }

    pub fn with_document(
        mut self,
        width: u32,
        height: u32,
        canvas_color: CanvasColor,
    ) -> Result<Self, String> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("Invalid document dimensions".into());
        }
        self.document_width = width as f32;
        self.document_height = height as f32;
        self.canvas_color = canvas_color;
        Ok(self)
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    viewport: [f32; 4],
    appearance: [f32; 4],
    document: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Segment {
    ends: [f32; 4],
    color: [f32; 4],
    radius: f32,
    hardness: f32,
    weight: f32,
    padding: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SelectionGpuRegion {
    bounds: [f32; 4],
    info: [f32; 4],
}

fn create_selection_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Selection regions"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}
fn selection_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    selection: Option<&Selection>,
) -> wgpu::BindGroup {
    let regions: Vec<_> = selection
        .map(|selection| {
            selection
                .regions
                .iter()
                .map(|region| SelectionGpuRegion {
                    bounds: region.bounds,
                    info: [
                        match region.operation {
                            SelectionOperation::Replace => 0.0,
                            SelectionOperation::Add => 1.0,
                            SelectionOperation::Subtract => 2.0,
                            SelectionOperation::Invert => 3.0,
                        },
                        if region.shape == SelectionShape::Ellipse {
                            1.0
                        } else {
                            0.0
                        },
                        0.0,
                        0.0,
                    ],
                })
                .collect()
        })
        .unwrap_or_else(|| {
            vec![SelectionGpuRegion {
                bounds: [0.0; 4],
                info: [4.0, 0.0, 0.0, 0.0],
            }]
        });
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Selection regions"),
        contents: bytemuck::cast_slice(&regions),
        usage: wgpu::BufferUsages::STORAGE,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Selection regions"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

fn linear_color(color: [u8; 3]) -> [f32; 4] {
    let rgb = color.map(|channel| {
        let value = f32::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    [rgb[0], rgb[1], rgb[2], 1.0]
}

// Integrate a radial brush tip along arc length, independent of input event density.
// The carry across polyline vertices is essential: restarting dabs at every
// vertex would reintroduce dark knots. A partial final interval has less weight.
fn segments(stroke: &Stroke, opacity: f32) -> Vec<Segment> {
    let points = smooth_points(&stroke.points, &stroke.pressures);
    dabs_along_path(&points, stroke.brush, opacity)
}

/// Use the same curve and arc-length sampler as the current GPU brush for a
/// tile-backed stroke.
pub fn sampled_raster_dabs(stroke: &Stroke) -> Result<Vec<RasterDab>, String> {
    stroke.brush.validate()?;
    if stroke.points.is_empty()
        || stroke.points.len() > 65_536
        || stroke.points.iter().any(|point| {
            !point.x.is_finite()
                || !point.y.is_finite()
                || point.x.abs() >= 100_000.0
                || point.y.abs() >= 100_000.0
        })
        || (!stroke.pressures.is_empty() && stroke.pressures.len() != stroke.points.len())
        || stroke
            .pressures
            .iter()
            .any(|pressure| !pressure.is_finite() || !(0.0..=1.0).contains(pressure))
    {
        return Err("Invalid stroke samples".into());
    }
    let points = smooth_points(&stroke.points, &stroke.pressures);
    let base_radius = stroke.brush.size * 0.5;
    let spacing = (base_radius * 0.08).min(1.0 + 3.0 * (1.0 - stroke.brush.hardness));
    let length: f64 = points
        .windows(2)
        .map(|pair| f64::from((pair[1].0.x - pair[0].0.x).hypot(pair[1].0.y - pair[0].0.y)))
        .sum();
    if !length.is_finite() || length > f64::from(spacing) * 65_535.0 {
        return Err("Stroke exceeds tile dab limit".into());
    }
    let dabs = dabs_along_path(&points, stroke.brush, 1.0)
        .into_iter()
        .map(|segment| RasterDab {
            x: segment.ends[0],
            y: segment.ends[1],
            radius: segment.radius,
            hardness: segment.hardness,
            weight: segment.weight,
        })
        .collect::<Vec<_>>();
    if dabs.len() > 65_536 {
        return Err("Stroke exceeds tile dab limit".into());
    }
    Ok(dabs)
}

pub fn paint_stroke_into_tiles(
    document: &mut TiledRasterDocument,
    layer_id: &str,
    stroke: &Stroke,
) -> Result<Option<TileInvalidation>, String> {
    document.paint_dabs_clipped(
        layer_id,
        &sampled_raster_dabs(stroke)?,
        stroke.brush.color,
        stroke.selection.as_ref(),
    )
}

/// Rasterize a retained v1 stroke at an integer preview resolution. Sampling
/// stays in document coordinates so dab density and pressure do not change.
pub fn paint_stroke_into_tiles_at_scale(
    document: &mut TiledRasterDocument,
    layer_id: &str,
    stroke: &Stroke,
    scale: u32,
) -> Result<Option<TileInvalidation>, String> {
    if !(1..=2).contains(&scale) {
        return Err("Unsupported tile preview scale".into());
    }
    let factor = scale as f32;
    let mut dabs = sampled_raster_dabs(stroke)?;
    for dab in &mut dabs {
        dab.x *= factor;
        dab.y *= factor;
        dab.radius *= factor;
    }
    let selection = stroke.selection.as_ref().map(|selection| {
        let mut scaled = selection.clone();
        for region in &mut scaled.regions {
            for value in &mut region.bounds {
                *value *= factor;
            }
        }
        scaled
    });
    if let Some(selection) = &selection {
        selection.validate()?;
    }
    document.paint_dabs_clipped(layer_id, &dabs, stroke.brush.color, selection.as_ref())
}

/// Conservative tile bounds for one retained stroke. The two-pixel margin
/// matches the raster dab's coverage loop, including edge antialiasing.
pub fn stroke_candidate_tile_coords_at_scale(
    stroke: &Stroke,
    dimensions: (u32, u32),
    scale: u32,
) -> Result<Vec<TileCoord>, String> {
    if !(1..=2).contains(&scale) {
        return Err("Unsupported tile preview scale".into());
    }
    let width = dimensions
        .0
        .checked_mul(scale)
        .ok_or("Tile preview width overflow")?;
    let height = dimensions
        .1
        .checked_mul(scale)
        .ok_or("Tile preview height overflow")?;
    let factor = scale as f32;
    let mut coords = BTreeSet::new();
    for dab in sampled_raster_dabs(stroke)? {
        if dab.weight == 0.0 {
            continue;
        }
        let x = dab.x * factor;
        let y = dab.y * factor;
        let radius = dab.radius * factor;
        let left = (x - radius - 2.0).floor().max(0.0) as u32;
        let top = (y - radius - 2.0).floor().max(0.0) as u32;
        let right = (x + radius + 2.0).ceil().max(0.0).min(width as f32) as u32;
        let bottom = (y + radius + 2.0).ceil().max(0.0).min(height as f32) as u32;
        if left >= right || top >= bottom {
            continue;
        }
        for tile_y in top / TILE_SIZE..=(bottom - 1) / TILE_SIZE {
            for tile_x in left / TILE_SIZE..=(right - 1) / TILE_SIZE {
                coords.insert(TileCoord {
                    x: tile_x,
                    y: tile_y,
                });
            }
        }
    }
    Ok(coords.into_iter().collect())
}

/// Paint a sampled stroke into a layer mask using an explicit grayscale target.
/// Zero hides the layer and 255 reveals it when the mask is enabled.
pub fn paint_stroke_into_mask(
    document: &mut TiledRasterDocument,
    layer_id: &str,
    stroke: &Stroke,
    value: u8,
) -> Result<Option<TileInvalidation>, String> {
    document.paint_mask_dabs(
        layer_id,
        &sampled_raster_dabs(stroke)?,
        value,
        stroke.selection.as_ref(),
    )
}

/// Reconstruct the committed v1 paint layer as tiles for migration checks.
/// SVG/vector layers and an active pointer stroke remain on the v1 renderer.
/// This does not alter the source document or its save format.
pub fn project_committed_paint_layer(source: &Document) -> Result<TiledRasterDocument, String> {
    project_committed_paint_layer_at_scale(source, 1)
}

/// Rebuild a temporary high-resolution preview from retained strokes. The
/// returned tile dimensions are `source.dimensions() * scale`; it is not a
/// replacement for the project's document pixel dimensions or save format.
pub fn project_committed_paint_layer_at_scale(
    source: &Document,
    scale: u32,
) -> Result<TiledRasterDocument, String> {
    if !(1..=2).contains(&scale) {
        return Err("Unsupported tile preview scale".into());
    }
    let (width, height) = source.dimensions();
    let mut projected = TiledRasterDocument::new(
        width
            .checked_mul(scale)
            .ok_or("Tile preview width overflow")?,
        height
            .checked_mul(scale)
            .ok_or("Tile preview height overflow")?,
    )?;
    let snapshot = source.snapshot();
    let paint = snapshot.layers.first().ok_or("Missing v1 paint layer")?;
    projected.add_layer(paint.id.clone(), paint.name.clone())?;
    for stroke in source.committed_paint_strokes() {
        paint_stroke_into_tiles_at_scale(&mut projected, &paint.id, stroke, scale)?;
        projected.discard_history();
    }
    projected.set_layer_mask(
        &paint.id,
        paint.mask_enabled,
        paint.mask_inverted,
        paint.mask_density,
    )?;
    projected.set_layer_appearance(&paint.id, paint.visible, paint.opacity)?;
    projected.set_layer_locks(&paint.id, paint.locked, paint.alpha_locked)?;
    projected.discard_history();
    Ok(projected)
}

/// Apply paint-layer settings to a retained projection without replaying its
/// strokes. Only the allocated tiles can change their composite pixels.
pub fn update_projected_paint_appearance(
    projected: &mut TiledRasterDocument,
    state: PaintProjectionState,
) -> Result<Vec<TileUpload>, String> {
    let layer = projected
        .layers()
        .first()
        .ok_or("Missing projected paint layer")?;
    let invalidation = TileInvalidation {
        layer_id: layer.id.clone(),
        coords: layer.tiles.allocated_coords().collect(),
    };
    let before = projected.prepare_uploads(&invalidation)?;
    let (visible, opacity, mask_enabled, mask_inverted, mask_density) = state.tile_appearance();
    projected.set_layer_mask(
        &invalidation.layer_id,
        mask_enabled,
        mask_inverted,
        mask_density,
    )?;
    projected.set_layer_appearance(&invalidation.layer_id, visible, opacity)?;
    let (locked, alpha_locked) = state.locks();
    projected.set_layer_locks(&invalidation.layer_id, locked, alpha_locked)?;
    projected.discard_history();
    let after = projected.prepare_uploads(&invalidation)?;
    Ok(after
        .into_iter()
        .zip(before)
        .filter_map(|(next, old)| (next.pixels != old.pixels).then_some(next))
        .collect())
}

/// GPU storage for the tile document's premultiplied RGBA8 composite.
/// The current v1 frame renderer does not sample this texture yet.
pub struct TileTexture {
    texture: wgpu::Texture,
    width: u32,
    height: u32,
}

/// A tile transfer batch whose coordinates, extents and payload lengths were
/// checked before it reached the UI thread.
pub struct ValidatedTileUploads {
    dimensions: (u32, u32),
    uploads: Vec<TileUpload>,
}

impl ValidatedTileUploads {
    pub fn new(dimensions: (u32, u32), uploads: Vec<TileUpload>) -> Result<Self, String> {
        validate_tile_uploads(dimensions.0, dimensions.1, &uploads)?;
        Ok(Self {
            dimensions,
            uploads,
        })
    }

    pub fn len(&self) -> usize {
        self.uploads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.uploads.is_empty()
    }
}

enum TileUploadSource<'a> {
    Raw(&'a [TileUpload]),
    Validated(&'a ValidatedTileUploads),
}

impl TileTexture {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let limit = device.limits().max_texture_dimension_2d;
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || width > limit
            || height > limit
        {
            return Err("Invalid tile texture dimensions".into());
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Tile composite"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Clear tile composite"),
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
        }
        queue.submit([encoder.finish()]);
        Ok(Self {
            texture,
            width,
            height,
        })
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }

    /// Validate the full batch before submitting any texture writes.
    pub fn upload(&self, queue: &wgpu::Queue, uploads: &[TileUpload]) -> Result<(), String> {
        validate_tile_uploads(self.width, self.height, uploads)?;
        self.upload_unchecked(queue, uploads);
        Ok(())
    }

    fn upload_validated(
        &self,
        queue: &wgpu::Queue,
        batch: &ValidatedTileUploads,
    ) -> Result<(), String> {
        if batch.dimensions != (self.width, self.height) {
            return Err("Tile upload dimensions changed".into());
        }
        self.upload_unchecked(queue, &batch.uploads);
        Ok(())
    }

    fn upload_unchecked(&self, queue: &wgpu::Queue, uploads: &[TileUpload]) {
        for upload in uploads {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: upload.origin[0],
                        y: upload.origin[1],
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &upload.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(upload.bytes_per_row),
                    rows_per_image: Some(TILE_SIZE),
                },
                wgpu::Extent3d {
                    width: upload.extent[0],
                    height: upload.extent[1],
                    depth_or_array_layers: 1,
                },
            );
        }
    }
}

fn validate_tile_uploads(width: u32, height: u32, uploads: &[TileUpload]) -> Result<(), String> {
    for upload in uploads {
        let x = upload
            .coord
            .x
            .checked_mul(TILE_SIZE)
            .ok_or("Tile origin overflow")?;
        let y = upload
            .coord
            .y
            .checked_mul(TILE_SIZE)
            .ok_or("Tile origin overflow")?;
        if x >= width
            || y >= height
            || upload.origin != [x, y]
            || upload.extent != [(width - x).min(TILE_SIZE), (height - y).min(TILE_SIZE)]
            || upload.bytes_per_row != TILE_SIZE * 4
            || upload.pixels.len() != (TILE_SIZE * TILE_SIZE * 4) as usize
        {
            return Err("Invalid tile upload".into());
        }
    }
    Ok(())
}

fn dabs_along_path(
    points: &[(Point, f32)],
    brush: lumapaint_core::document::Brush,
    opacity: f32,
) -> Vec<Segment> {
    let base_radius = brush.size * 0.5;
    // Soft tips tolerate wider sampling. Avoid thousands of tiny half-float
    // additions on wide brushes; hard edges keep a one-pixel upper bound.
    let spacing = (base_radius * 0.08).min(1.0 + 3.0 * (1.0 - brush.hardness));
    let color = linear_color(brush.color);
    let mut result = Vec::new();
    let mut emit = |p: Point, pressure: f32, length: f32| {
        let radius = base_radius * pressure.max(0.05);
        result.push(Segment {
            ends: [p.x, p.y, p.x, p.y],
            color,
            radius,
            hardness: brush.hardness,
            // Optical depth per unit arc length; the shader converts it to alpha.
            weight: length / radius * opacity,
            padding: 0.0,
        })
    };
    let mut traversed = 0.0;
    let mut next = spacing * 0.5;
    for pair in points.windows(2) {
        let (a, pressure_a) = pair[0];
        let (b, pressure_b) = pair[1];
        let length = (b.x - a.x).hypot(b.y - a.y);
        if length <= f32::EPSILON {
            continue;
        }
        while next < traversed + length {
            let t = (next - traversed) / length;
            emit(
                Point {
                    x: a.x + (b.x - a.x) * t,
                    y: a.y + (b.y - a.y) * t,
                },
                pressure_a + (pressure_b - pressure_a) * t,
                spacing,
            );
            next += spacing;
        }
        traversed += length;
    }
    if let Some((last, pressure)) = points.last() {
        if traversed < spacing * 0.5 {
            // A click still produces a round mark.
            emit(*last, *pressure, base_radius);
        } else {
            // Correct the last quadrature cell instead of adding a full end dab.
            let covered = next - spacing * 0.5;
            let remainder = traversed - covered;
            if remainder > 0.0 {
                emit(*last, *pressure, remainder);
            } else if let Some(last_dab) = result.last_mut() {
                last_dab.weight += remainder / base_radius;
            }
        }
    }
    result
}

fn smooth_points(points: &[Point], pressures: &[f32]) -> Vec<(Point, f32)> {
    let pressure_at = |index: usize| pressures.get(index).copied().unwrap_or(1.0);
    if points.len() <= 2 {
        return points
            .iter()
            .enumerate()
            .map(|(index, point)| (*point, pressure_at(index)))
            .collect();
    }

    let mut result = vec![(points[0], pressure_at(0))];
    for index in 0..points.len() - 1 {
        let p0 = points[index.saturating_sub(1)];
        let p1 = points[index];
        let p2 = points[index + 1];
        let p3 = points[(index + 2).min(points.len() - 1)];
        let distance = (p2.x - p1.x).hypot(p2.y - p1.y);
        let subdivisions = (distance / 1.5).ceil().clamp(1.0, 64.0) as usize;
        for step in 1..=subdivisions {
            let t = step as f32 / subdivisions as f32;
            let t2 = t * t;
            let t3 = t2 * t;
            result.push((
                Point {
                    x: 0.5
                        * ((2.0 * p1.x)
                            + (-p0.x + p2.x) * t
                            + (2.0 * p0.x - 5.0 * p1.x + 4.0 * p2.x - p3.x) * t2
                            + (-p0.x + 3.0 * p1.x - 3.0 * p2.x + p3.x) * t3),
                    y: 0.5
                        * ((2.0 * p1.y)
                            + (-p0.y + p2.y) * t
                            + (2.0 * p0.y - 5.0 * p1.y + 4.0 * p2.y - p3.y) * t2
                            + (-p0.y + 3.0 * p1.y - 3.0 * p2.y + p3.y) * t3),
                },
                pressure_at(index) + (pressure_at(index + 1) - pressure_at(index)) * t,
            ));
        }
    }
    result
}

const BRUSH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

fn create_brush_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
) -> wgpu::RenderPipeline {
    let brush_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Round brush"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}",
                include_str!("selection_common.wgsl"),
                include_str!("brush.wgsl")
            )
            .into(),
        ),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Round brush pipeline"), layout: Some(layout),
            vertex: wgpu::VertexState {
                module: &brush_shader, entry_point: Some("vs_main"), compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Segment>() as u64, step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32, 3 => Float32, 4 => Float32] }],
            },
            primitive: Default::default(), depth_stencil: None, multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &brush_shader, entry_point: Some("fs_main"), compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: BRUSH_FORMAT, blend: Some(wgpu::BlendState {
color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Max },
alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
}), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview: None, cache: None,
        })
}

fn create_selection_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Selection outline"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}",
                include_str!("selection_common.wgsl"),
                include_str!("selection.wgsl")
            )
            .into(),
        ),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Selection outline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

fn create_brush_composite(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Brush mask texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let brush_composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Brush composite layout"),
        bind_group_layouts: &[&bind_layout],
        push_constant_ranges: &[],
    });
    let brush_composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Brush mask composite"),
        source: wgpu::ShaderSource::Wgsl(include_str!("brush_composite.wgsl").into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Brush composite pipeline"),
        layout: Some(&brush_composite_layout),
        vertex: wgpu::VertexState {
            module: &brush_composite_shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &brush_composite_shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });
    (bind_layout, pipeline)
}

pub mod vector;

fn rasterize_svg(source: &str, width: u32, height: u32) -> Result<Vec<u8>, String> {
    vector::rasterize_svg(source, width, height).map(|raster| raster.pixels)
}

pub fn validate_svg(source: &str) -> Result<(), String> {
    rasterize_svg(source, WIDTH as u32, HEIGHT as u32).map(|_| ())
}

/// The host must keep the native surface target alive until this renderer is dropped.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    selection_pipeline: wgpu::RenderPipeline,
    selection_layout: wgpu::BindGroupLayout,
    brush_pipeline: wgpu::RenderPipeline,
    brush_composite_pipeline: wgpu::RenderPipeline,
    brush_bind_layout: wgpu::BindGroupLayout,
    brush_sampler: wgpu::Sampler,
    svg_pipeline: wgpu::RenderPipeline,
    tile_pipeline: wgpu::RenderPipeline,
    svg_bind_layout: wgpu::BindGroupLayout,
    svg_sampler: wgpu::Sampler,
    tile_nearest_sampler: wgpu::Sampler,
    tile_preview: Option<TileGpuPreview>,
    svg_cache: HashMap<String, CachedSvg>,
    drag_cache: HashMap<String, DragLayerCache>,
    uniform_layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    device_error: Arc<Mutex<Option<String>>>,
    pub adapter_name: String,
    pub backend: String,
}

struct TileGpuPreview {
    texture: TileTexture,
    document_dimensions: (u32, u32),
    linear_bind_group: wgpu::BindGroup,
    nearest_bind_group: wgpu::BindGroup,
    visible: bool,
}

fn create_svg_pipeline(
    device: &wgpu::Device,
    bind_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let svg_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("SVG texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let svg_pipeline = create_textured_layer_pipeline(
        device,
        bind_layout,
        &svg_bind_layout,
        format,
        include_str!("svg.wgsl"),
        "SVG layer",
    );
    (svg_bind_layout, svg_pipeline)
}

fn create_textured_layer_pipeline(
    device: &wgpu::Device,
    viewport_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    source: &str,
    label: &str,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[viewport_layout, texture_layout],
        push_constant_ranges: &[],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

struct CachedSvg {
    fully_contained: bool,
    source: String,
    opacity: f32,
    size: (u32, u32),
    _texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

struct DragLayerCache {
    source: String,
    selected_ids: Vec<String>,
    size: (u32, u32),
    opacity: f32,
    /// Empty means the preview needs the full-raster fallback.
    runs: Vec<DragRun>,
    bytes: usize,
}

struct DragRun {
    selected: bool,
    cached: CachedSvg,
    bounds: Option<[i32; 4]>,
}

fn alpha_bounds(pixels: &[u8], size: (u32, u32)) -> Option<[i32; 4]> {
    let mut result: Option<[i32; 4]> = None;
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        let x = (index % size.0 as usize) as i32;
        let y = (index / size.0 as usize) as i32;
        match &mut result {
            Some([left, top, right, bottom]) => {
                *left = (*left).min(x);
                *top = (*top).min(y);
                *right = (*right).max(x + 1);
                *bottom = (*bottom).max(y + 1);
            }
            None => result = Some([x, y, x + 1, y + 1]),
        }
    }
    result
}

fn bounds_are_separate(bounds: &[(bool, Option<[i32; 4]>)], offset: [f32; 2]) -> bool {
    for (index, (first_selected, first_bounds)) in bounds.iter().enumerate() {
        let Some(a) = first_bounds else { continue };
        let (ax, ay) = if *first_selected {
            (offset[0], offset[1])
        } else {
            (0.0, 0.0)
        };
        for (second_selected, second_bounds) in bounds.iter().skip(index + 1) {
            let Some(b) = second_bounds else { continue };
            let (bx, by) = if *second_selected {
                (offset[0], offset[1])
            } else {
                (0.0, 0.0)
            };
            // Include the sampling footprint at fractional zoom and drag offsets.
            if (a[0] as f32 + ax) < (b[2] as f32 + bx + 2.0)
                && (a[2] as f32 + ax + 2.0) > (b[0] as f32 + bx)
                && (a[1] as f32 + ay) < (b[3] as f32 + by + 2.0)
                && (a[3] as f32 + ay + 2.0) > (b[1] as f32 + by)
            {
                return false;
            }
        }
    }
    true
}

fn drag_runs_are_separate(runs: &[DragRun], offset: [f32; 2]) -> bool {
    let bounds = runs
        .iter()
        .map(|run| (run.selected, run.bounds))
        .collect::<Vec<_>>();
    bounds_are_separate(&bounds, offset)
}

/// CPU-only SVG result. The host can prepare this away from the AppKit/GPU owner.
pub struct PreparedSvgLayer {
    pub id: String,
    pub source: String,
    pub opacity: f32,
    pub size: (u32, u32),
    pub fully_contained: bool,
    pub pixels: Vec<u8>,
}

pub fn prepare_svg_layer(
    layer: &SvgLayer,
    width: u32,
    height: u32,
) -> Result<PreparedSvgLayer, String> {
    let raster = vector::rasterize_svg(&layer.source, width, height)?;
    let opacity = layer.effective_opacity();
    let mut pixels = raster.pixels;
    if opacity != 1.0 {
        for value in &mut pixels {
            *value = (f32::from(*value) * opacity).round() as u8;
        }
    }
    Ok(PreparedSvgLayer {
        id: layer.id.clone(),
        source: layer.source.clone(),
        opacity,
        size: (width, height),
        fully_contained: raster.fully_contained,
        pixels,
    })
}

impl Renderer {
    /// Opt-in tile paint preview. The host must only install a tile document
    /// matching the current v1 document and clear it when that source changes.
    pub fn install_tiled_preview(&mut self, tiles: &TiledRasterDocument) -> Result<(), String> {
        self.install_tiled_preview_at_scale(tiles, tiles.dimensions(), 1)
    }

    /// Install a temporary tile preview sampled at 1x or 2x while retaining
    /// the source document's logical dimensions and pointer coordinates.
    pub fn install_tiled_preview_at_scale(
        &mut self,
        tiles: &TiledRasterDocument,
        document_dimensions: (u32, u32),
        scale: u32,
    ) -> Result<(), String> {
        self.install_tiled_preview_uploads_at_scale(
            tiles.dimensions(),
            document_dimensions,
            scale,
            &tiles.prepare_full_uploads(),
        )
    }

    /// Accept already-composited tile pixels so the host can prepare them off the UI thread.
    pub fn install_tiled_preview_uploads_at_scale(
        &mut self,
        tile_dimensions: (u32, u32),
        document_dimensions: (u32, u32),
        scale: u32,
        uploads: &[TileUpload],
    ) -> Result<(), String> {
        self.install_tiled_preview_upload_source(
            tile_dimensions,
            document_dimensions,
            scale,
            TileUploadSource::Raw(uploads),
        )
    }

    /// Install a worker-validated batch without repeating per-tile checks on
    /// the UI thread.
    pub fn install_tiled_preview_batch_at_scale(
        &mut self,
        document_dimensions: (u32, u32),
        scale: u32,
        batch: &ValidatedTileUploads,
    ) -> Result<(), String> {
        self.install_tiled_preview_upload_source(
            batch.dimensions,
            document_dimensions,
            scale,
            TileUploadSource::Validated(batch),
        )
    }

    fn install_tiled_preview_upload_source(
        &mut self,
        tile_dimensions: (u32, u32),
        document_dimensions: (u32, u32),
        scale: u32,
        uploads: TileUploadSource<'_>,
    ) -> Result<(), String> {
        if !(1..=2).contains(&scale)
            || document_dimensions.0.checked_mul(scale) != Some(tile_dimensions.0)
            || document_dimensions.1.checked_mul(scale) != Some(tile_dimensions.1)
        {
            return Err("Tile preview dimensions do not match the document scale".into());
        }
        let (width, height) = tile_dimensions;
        let texture = TileTexture::new(&self.device, &self.queue, width, height)?;
        match uploads {
            TileUploadSource::Raw(uploads) => texture.upload(&self.queue, uploads)?,
            TileUploadSource::Validated(batch) => texture.upload_validated(&self.queue, batch)?,
        }
        let view = texture.texture().create_view(&Default::default());
        let make_bind_group = |sampler: &wgpu::Sampler| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Tile paint preview"),
                layout: &self.svg_bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        self.tile_preview = Some(TileGpuPreview {
            texture,
            document_dimensions,
            linear_bind_group: make_bind_group(&self.svg_sampler),
            nearest_bind_group: make_bind_group(&self.tile_nearest_sampler),
            visible: true,
        });
        Ok(())
    }

    pub fn update_tiled_preview(
        &mut self,
        tiles: &TiledRasterDocument,
        invalidation: &TileInvalidation,
    ) -> Result<(), String> {
        self.update_tiled_preview_uploads(tiles.dimensions(), &tiles.prepare_uploads(invalidation)?)
    }

    /// Upload only pixels prepared for the changed tiles on a worker thread.
    pub fn update_tiled_preview_uploads(
        &mut self,
        tile_dimensions: (u32, u32),
        uploads: &[TileUpload],
    ) -> Result<(), String> {
        let preview = self
            .tile_preview
            .as_ref()
            .ok_or("No tile preview installed")?;
        if tile_dimensions != (preview.texture.width, preview.texture.height) {
            return Err("Tile preview dimensions changed".into());
        }
        preview.texture.upload(&self.queue, uploads)
    }

    pub fn update_tiled_preview_batch(
        &mut self,
        batch: &ValidatedTileUploads,
    ) -> Result<(), String> {
        let preview = self
            .tile_preview
            .as_ref()
            .ok_or("No tile preview installed")?;
        preview.texture.upload_validated(&self.queue, batch)
    }

    pub fn clear_tiled_preview(&mut self) {
        self.tile_preview = None;
    }

    /// Keep uploaded tile data while the active stroke uses the v1 brush path.
    pub fn set_tiled_preview_visible(&mut self, visible: bool) {
        if let Some(preview) = &mut self.tile_preview {
            preview.visible = visible;
        }
    }

    /// Layers that still need CPU preparation for the committed document frame.
    pub fn missing_svg_layers(&self, document: &Document) -> Vec<SvgLayer> {
        let size = document.dimensions();
        document
            .visible_svg_layers()
            .filter(|layer| {
                !self.svg_cache.get(&layer.id).is_some_and(|cached| {
                    cached.source == layer.source
                        && cached.opacity == layer.effective_opacity()
                        && cached.size == size
                })
            })
            .cloned()
            .collect()
    }

    /// GPU-only half of SVG preparation. The host checks the document generation
    /// before installing a result produced by a worker.
    pub fn install_prepared_svg(&mut self, prepared: PreparedSvgLayer) -> Result<(), String> {
        let id = prepared.id.clone();
        let cached = self.make_cached_svg(prepared)?;
        self.svg_cache.insert(id, cached);
        Ok(())
    }

    fn make_cached_svg(&self, prepared: PreparedSvgLayer) -> Result<CachedSvg, String> {
        let (width, height) = prepared.size;
        let expected = usize::try_from(width)
            .ok()
            .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("Invalid prepared SVG dimensions")?;
        if width == 0
            || height == 0
            || width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
            || prepared.pixels.len() != expected
        {
            return Err("Invalid prepared SVG pixels".into());
        }
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("SVG layer texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &prepared.pixels,
        );
        let view = texture.create_view(&Default::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("SVG layer"),
            layout: &self.svg_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.svg_sampler),
                },
            ],
        });
        Ok(CachedSvg {
            fully_contained: prepared.fully_contained,
            source: prepared.source,
            opacity: prepared.opacity,
            size: prepared.size,
            _texture: texture,
            bind_group,
        })
    }

    fn prepare_drag_cache(
        &self,
        document: &Document,
        layer: &SvgLayer,
        size: (u32, u32),
    ) -> Result<Option<DragLayerCache>, String> {
        let Some(runs) = document.vector_drag_runs(layer) else {
            return Ok(None);
        };
        let unsupported = || DragLayerCache {
            source: layer.source.clone(),
            selected_ids: document.selected_vector_ids().to_vec(),
            size,
            opacity: layer.effective_opacity(),
            runs: Vec::new(),
            bytes: 0,
        };
        const MAX_DRAG_RUNS: usize = 8;
        const MAX_DRAG_CACHE_BYTES: usize = 64 * 1024 * 1024;
        let estimated_bytes = (size.0 as usize)
            .checked_mul(size.1 as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| bytes.checked_mul(runs.len()));
        let in_use: usize = self.drag_cache.values().map(|cache| cache.bytes).sum();
        if runs.len() > MAX_DRAG_RUNS
            || estimated_bytes
                .is_none_or(|bytes| bytes > MAX_DRAG_CACHE_BYTES.saturating_sub(in_use))
        {
            return Ok(Some(unsupported()));
        }
        let mut prepared = Vec::with_capacity(runs.len());
        for (selected, run) in &runs {
            let raster = prepare_svg_layer(run, size.0, size.1)?;
            if *selected && !raster.fully_contained {
                return Ok(Some(unsupported()));
            }
            prepared.push((*selected, raster));
        }
        let runs = prepared
            .into_iter()
            .map(|(selected, raster)| {
                let bounds = alpha_bounds(&raster.pixels, size);
                self.make_cached_svg(raster).map(|cached| DragRun {
                    selected,
                    cached,
                    bounds,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(DragLayerCache {
            source: layer.source.clone(),
            selected_ids: document.selected_vector_ids().to_vec(),
            size,
            opacity: layer.effective_opacity(),
            runs,
            bytes: estimated_bytes.unwrap_or(0),
        }))
    }

    pub async fn new(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        viewport: Viewport,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| e.to_string())?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("LumaPaint preview device"),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let capabilities = surface.get_capabilities(&adapter);
        let device_error = Arc::new(Mutex::new(None));
        let errors = Arc::clone(&device_error);
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            if let Ok(mut slot) = errors.lock() {
                *slot = Some(error.to_string());
            }
        }));
        let errors = Arc::clone(&device_error);
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut slot) = errors.lock() {
                *slot = Some(format!("GPU device lost ({reason:?}): {message}"));
            }
        });
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .ok_or("No sRGB surface format available")?;
        let mut config = surface
            .get_default_config(&adapter, viewport.width, viewport.height)
            .ok_or("No compatible surface configuration")?;
        config.format = format;
        config.present_mode = wgpu::PresentMode::Fifo;
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shared viewport layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Canvas layout"),
            bind_group_layouts: &[&bind_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("GPU preview pattern"),
            source: wgpu::ShaderSource::Wgsl(include_str!("preview.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("LumaPaint preview pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let selection_layout = create_selection_layout(&device);
        let paint_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Selected paint layout"),
            bind_group_layouts: &[&bind_layout, &selection_layout],
            push_constant_ranges: &[],
        });
        let brush_pipeline = create_brush_pipeline(&device, &paint_layout);
        let selection_pipeline = create_selection_pipeline(&device, &paint_layout, format);
        let (brush_bind_layout, brush_composite_pipeline) = create_brush_composite(&device, format);
        let brush_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let (svg_bind_layout, svg_pipeline) = create_svg_pipeline(&device, &bind_layout, format);
        let tile_pipeline = create_textured_layer_pipeline(
            &device,
            &bind_layout,
            &svg_bind_layout,
            format,
            include_str!("tile.wgsl"),
            "Tile paint layer",
        );
        let svg_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let tile_nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        if let Some(error) = device.pop_error_scope().await {
            return Err(error.to_string());
        }
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Preview viewport"),
            contents: bytemuck::bytes_of(&Self::uniforms(viewport)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Preview viewport"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        surface.configure(&device, &config);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            selection_pipeline,
            selection_layout,
            brush_pipeline,
            brush_composite_pipeline,
            brush_bind_layout,
            brush_sampler,
            svg_pipeline,
            tile_pipeline,
            svg_bind_layout,
            svg_sampler,
            tile_nearest_sampler,
            tile_preview: None,
            svg_cache: HashMap::new(),
            drag_cache: HashMap::new(),
            uniform_layout: bind_layout,
            uniforms,
            bind_group,
            device_error,
            adapter_name: info.name,
            backend: format!("{:?}", info.backend),
        })
    }

    fn uniforms(viewport: Viewport) -> Uniforms {
        Uniforms {
            viewport: [
                viewport.width as f32,
                viewport.height as f32,
                viewport.scale,
                viewport.zoom,
            ],
            appearance: [
                if viewport.dark { 1.0 } else { 0.0 },
                viewport.pan_x,
                viewport.pan_y,
                if viewport.canvas_color == CanvasColor::Transparent {
                    1.0
                } else {
                    0.0
                },
            ],
            document: [viewport.document_width, viewport.document_height, 0.0, 0.0],
        }
    }

    pub fn render(&mut self, viewport: Viewport, document: &Document) -> Result<(), String> {
        self.render_vector_drag(viewport, document, [0.0, 0.0])
    }

    /// Draw a committed frame without blocking the UI on missing SVG layers.
    /// The host installs matching CPU results and requests another frame.
    pub fn render_deferred(
        &mut self,
        viewport: Viewport,
        document: &Document,
    ) -> Result<(), String> {
        self.render_vector_drag_inner(viewport, document, [0.0, 0.0], true)
    }

    /// Transient drag offset in document pixels; no document edit or text shaping per frame
    /// is needed when the selected layer's full content already fits in its cached texture.
    pub fn render_vector_drag(
        &mut self,
        viewport: Viewport,
        document: &Document,
        offset: [f32; 2],
    ) -> Result<(), String> {
        self.render_vector_drag_inner(viewport, document, offset, false)
    }

    fn render_vector_drag_inner(
        &mut self,
        viewport: Viewport,
        document: &Document,
        offset: [f32; 2],
        defer_svg: bool,
    ) -> Result<(), String> {
        if offset
            .iter()
            .any(|v| !v.is_finite() || v.abs() >= 100_000.0)
        {
            return Err("Invalid vector drag offset".into());
        }
        if let Some(error) = self
            .device_error
            .lock()
            .map_err(|e| e.to_string())?
            .as_ref()
        {
            return Err(error.clone());
        }
        if viewport.width > self.device.limits().max_texture_dimension_2d
            || viewport.height > self.device.limits().max_texture_dimension_2d
        {
            return Err("Viewport exceeds GPU texture limits".into());
        }
        if self.config.width != viewport.width || self.config.height != viewport.height {
            self.config.width = viewport.width;
            self.config.height = viewport.height;
            self.surface.configure(&self.device, &self.config);
        }
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                self.surface
                    .get_current_texture()
                    .map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        };
        let uniforms = Self::uniforms(viewport);
        if self
            .tile_preview
            .as_ref()
            .is_some_and(|preview| preview.document_dimensions != document.dimensions())
        {
            self.tile_preview = None;
        }
        let vector_overlays = document.vector_overlay_selections_at_offset(offset[0], offset[1]);
        let outline_selections = document
            .selection()
            .into_iter()
            .chain(vector_overlays.iter())
            .map(|selection| {
                selection_bind_group(&self.device, &self.selection_layout, Some(selection))
            })
            .collect::<Vec<_>>();
        let stroke_selections: Vec<_> = document
            .visible_strokes()
            .map(|stroke| {
                selection_bind_group(
                    &self.device,
                    &self.selection_layout,
                    stroke.selection.as_ref(),
                )
            })
            .collect();
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        let view = frame.texture.create_view(&Default::default());
        let stroke_segments: Vec<_> = if self
            .tile_preview
            .as_ref()
            .is_some_and(|preview| preview.visible)
        {
            Vec::new()
        } else {
            document
                .visible_strokes()
                .map(|stroke| segments(stroke, document.paint_layer_opacity()))
                .collect()
        };
        self.svg_cache
            .retain(|id, _| document.svg_layers().any(|layer| &layer.id == id));
        let (width, height) = document.dimensions();
        let mut translated_layers = std::collections::HashSet::new();
        let mut partial_layers = std::collections::HashSet::new();
        let mut drag_metrics = VectorDragMetrics::default();
        if offset == [0.0, 0.0] {
            self.drag_cache.clear();
        }
        for original in document.visible_svg_layers() {
            let dragging = offset != [0.0, 0.0];
            if dragging
                && document.vector_layer_moves_as_unit(original)
                && self.svg_cache.get(&original.id).is_some_and(|cached| {
                    cached.fully_contained
                        && cached.source == original.source
                        && cached.opacity == original.effective_opacity()
                        && cached.size == (width, height)
                })
            {
                translated_layers.insert(original.id.as_str());
                drag_metrics.unit_reuse += 1;
                continue;
            }
            if dragging && original.vector_layer && original.effective_opacity() == 1.0 {
                let cache_valid = self.drag_cache.get(&original.id).is_some_and(|cache| {
                    cache.source == original.source
                        && cache.selected_ids == document.selected_vector_ids()
                        && cache.size == (width, height)
                        && cache.opacity == original.effective_opacity()
                });
                if !cache_valid {
                    self.drag_cache.remove(&original.id);
                    let started = std::time::Instant::now();
                    if let Some(cache) =
                        self.prepare_drag_cache(document, original, (width, height))?
                    {
                        if !cache.runs.is_empty() {
                            drag_metrics.cache_builds += 1;
                            drag_metrics.upload_bytes += cache.bytes;
                        }
                        self.drag_cache.insert(original.id.clone(), cache);
                    }
                    drag_metrics.cache_setup_ms += started.elapsed().as_secs_f64() * 1000.0;
                }
                if self.drag_cache.get(&original.id).is_some_and(|cache| {
                    !cache.runs.is_empty() && drag_runs_are_separate(&cache.runs, offset)
                }) {
                    partial_layers.insert(original.id.as_str());
                    drag_metrics.partial_reuse += 1;
                    continue;
                }
            }
            let preview = if dragging {
                document.translated_vector_layer(original, offset[0], offset[1])?
            } else {
                None
            };
            if preview.is_some() {
                drag_metrics.full_previews += 1;
            }
            let layer = preview.as_ref().unwrap_or(original);
            if self.svg_cache.get(&layer.id).is_some_and(|cached| {
                cached.source == layer.source
                    && cached.opacity == layer.effective_opacity()
                    && cached.size == (width, height)
            }) {
                continue;
            }
            if defer_svg {
                self.svg_cache.remove(&layer.id);
                continue;
            }
            let started = std::time::Instant::now();
            let prepared = prepare_svg_layer(layer, width, height)?;
            drag_metrics.upload_bytes += prepared.pixels.len();
            self.install_prepared_svg(prepared)?;
            drag_metrics.full_prepare_upload_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        let translated_uniforms = if translated_layers.is_empty() && partial_layers.is_empty() {
            None
        } else {
            let mut moved = uniforms;
            moved.document[2] = offset[0];
            moved.document[3] = offset[1];
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Vector drag offset"),
                    contents: bytemuck::bytes_of(&moved),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Vector drag uniforms"),
                layout: &self.uniform_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            }))
        };
        let brush_buffers: Vec<_> = stroke_segments
            .iter()
            .map(|segments| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Brush dabs"),
                        contents: bytemuck::cast_slice(segments),
                        usage: wgpu::BufferUsages::VERTEX,
                    })
            })
            .collect();
        let brush_mask = (!brush_buffers.is_empty()).then(|| {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Brush paint mask"),
                size: wgpu::Extent3d {
                    width: viewport.width,
                    height: viewport.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: BRUSH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let mask_view = texture.create_view(&Default::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Brush paint mask"),
                layout: &self.brush_bind_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&mask_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.brush_sampler),
                    },
                ],
            });
            (texture, mask_view, bind_group)
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Preview render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(preview) = self.tile_preview.as_ref().filter(|preview| preview.visible) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Tile paint preview pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.tile_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            let tile_bind_group = if viewport.tile_filter_nearest() {
                &preview.nearest_bind_group
            } else {
                &preview.linear_bind_group
            };
            pass.set_bind_group(1, tile_bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        if let Some((_mask, mask_view, mask_bind_group)) = &brush_mask {
            for ((buffer, segments), selection_group) in brush_buffers
                .iter()
                .zip(stroke_segments.iter())
                .zip(&stroke_selections)
            {
                let mut mask_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Brush paint pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: mask_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                mask_pass.set_pipeline(&self.brush_pipeline);
                mask_pass.set_bind_group(0, &self.bind_group, &[]);
                mask_pass.set_bind_group(1, selection_group, &[]);
                mask_pass.set_vertex_buffer(0, buffer.slice(..));
                mask_pass.draw(0..6, 0..segments.len() as u32);
                drop(mask_pass);

                let mut composite_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Stroke composite pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                composite_pass.set_pipeline(&self.brush_composite_pipeline);
                composite_pass.set_bind_group(0, mask_bind_group, &[]);
                composite_pass.draw(0..6, 0..1);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SVG layer render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            for layer in document.visible_svg_layers() {
                if partial_layers.contains(layer.id.as_str()) {
                    if let Some(cache) = self.drag_cache.get(&layer.id) {
                        for run in &cache.runs {
                            pass.set_pipeline(&self.svg_pipeline);
                            pass.set_bind_group(
                                0,
                                if run.selected {
                                    translated_uniforms.as_ref().unwrap_or(&self.bind_group)
                                } else {
                                    &self.bind_group
                                },
                                &[],
                            );
                            pass.set_bind_group(1, &run.cached.bind_group, &[]);
                            pass.draw(0..6, 0..1);
                        }
                    }
                    continue;
                }
                let Some(cached) = self.svg_cache.get(&layer.id) else {
                    continue;
                };
                let bind_group = &cached.bind_group;
                pass.set_pipeline(&self.svg_pipeline);
                let uniforms = if translated_layers.contains(layer.id.as_str()) {
                    translated_uniforms.as_ref().unwrap_or(&self.bind_group)
                } else {
                    &self.bind_group
                };
                pass.set_bind_group(0, uniforms, &[]);
                pass.set_bind_group(1, bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
            for selection_group in &outline_selections {
                pass.set_pipeline(&self.selection_pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_bind_group(1, selection_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        if offset != [0.0, 0.0] && render_metrics_enabled() {
            eprintln!(
                "LumaPaint vector-drag unit_reuse={} partial_reuse={} cache_builds={} full_previews={} upload_bytes={} cache_setup_ms={:.2} full_prepare_upload_ms={:.2}",
                drag_metrics.unit_reuse,
                drag_metrics.partial_reuse,
                drag_metrics.cache_builds,
                drag_metrics.full_previews,
                drag_metrics.upload_bytes,
                drag_metrics.cache_setup_ms,
                drag_metrics.full_prepare_upload_ms,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_upload_validation_rejects_partial_bad_batches_before_gpu_writes() {
        let mut document = TiledRasterDocument::new(257, 3).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        let changed = document
            .write_rect("paint", [255, 2, 2, 1], &[200, 0, 0, 255, 0, 0, 200, 255])
            .unwrap()
            .unwrap();
        let uploads = document.prepare_uploads(&changed).unwrap();
        assert!(validate_tile_uploads(257, 3, &uploads).is_ok());
        assert_eq!(
            ValidatedTileUploads::new((257, 3), uploads.clone())
                .unwrap()
                .len(),
            2
        );
        let mut invalid = uploads.clone();
        invalid[1].extent = [2, 3];
        assert!(validate_tile_uploads(257, 3, &invalid).is_err());
        assert!(ValidatedTileUploads::new((257, 3), invalid.clone()).is_err());
        invalid[1] = uploads[1].clone();
        invalid[1].pixels.pop();
        assert!(validate_tile_uploads(257, 3, &invalid).is_err());
        assert!(ValidatedTileUploads::new((257, 3), invalid).is_err());
    }

    #[test]
    fn tile_dabs_share_gpu_curve_spacing_radius_hardness_and_pressure() {
        let stroke = Stroke {
            brush: lumapaint_core::document::Brush {
                size: 32.0,
                hardness: 0.3,
                color: [20, 80, 200],
            },
            points: vec![
                Point { x: 20.0, y: 20.0 },
                Point { x: 80.0, y: 30.0 },
                Point { x: 120.0, y: 80.0 },
            ],
            pressures: vec![0.2, 0.8, 0.4],
            selection: None,
        };
        let gpu = segments(&stroke, 1.0);
        let tiled = sampled_raster_dabs(&stroke).unwrap();
        assert_eq!(gpu.len(), tiled.len());
        assert!(tiled.len() > stroke.points.len());
        for (segment, dab) in gpu.iter().zip(&tiled) {
            assert_eq!([dab.x, dab.y], [segment.ends[0], segment.ends[1]]);
            assert_eq!(dab.radius, segment.radius);
            assert_eq!(dab.hardness, segment.hardness);
            assert_eq!(dab.weight, segment.weight);
        }
        let mut document = TiledRasterDocument::new(128, 128).unwrap();
        document.add_layer("paint".into(), "Paint".into()).unwrap();
        let changed = paint_stroke_into_tiles(&mut document, "paint", &stroke)
            .unwrap()
            .unwrap();
        assert!(!changed.coords.is_empty());
        assert!(document.layers()[0].tiles.allocated_tile_count() > 0);
        document.undo().unwrap();
        assert_eq!(document.layers()[0].tiles.allocated_tile_count(), 0);
        let mask_change = paint_stroke_into_mask(&mut document, "paint", &stroke, 0)
            .unwrap()
            .unwrap();
        assert_eq!(mask_change.coords, changed.coords);
        assert!(document.layers()[0].mask.pixel(20, 20).unwrap() < 255);
        document.undo().unwrap();
        assert_eq!(document.layers()[0].mask.allocated_tile_count(), 0);
    }

    #[test]
    fn committed_v1_paint_projects_hidden_pixels_and_layer_settings_without_mutation() {
        use lumapaint_core::document::{Brush, LayerSettings};

        let mut source = Document::default();
        source.begin_selection(Point { x: 10.0, y: 10.0 }, SelectionShape::Rectangle);
        source.extend_selection(Point { x: 40.0, y: 40.0 }, true);
        source
            .begin(
                Point { x: 30.0, y: 30.0 },
                Brush {
                    size: 20.0,
                    hardness: 0.5,
                    color: [180, 40, 20],
                },
            )
            .unwrap();
        source.finish();
        source.deselect();
        source
            .set_layer_settings(LayerSettings {
                id: "layer-1".into(),
                name: "Imported paint".into(),
                opacity: 0.5,
                locked: true,
                alpha_locked: true,
                mask_enabled: true,
                mask_inverted: false,
                mask_density: 0.5,
            })
            .unwrap();
        source.toggle_visibility();
        let source_revision = source.revision();
        let mut projected = project_committed_paint_layer(&source).unwrap();
        let layer = &projected.layers()[0];
        assert_eq!(layer.name, "Imported paint");
        assert!(!layer.visible);
        assert!(layer.locked && layer.alpha_locked);
        assert_eq!(layer.effective_opacity(), 0.25);
        assert!(layer.tiles.pixel(30, 30).unwrap()[3] > 0);
        assert_eq!(layer.tiles.pixel(45, 30), Some([0; 4]));
        assert_eq!(
            projected.composite_tile(lumapaint_core::tiles::TileCoord { x: 0, y: 0 }),
            None
        );
        assert!(projected.undo().unwrap().is_none());
        assert_eq!(source.revision(), source_revision);
        assert_eq!(source.committed_paint_strokes().count(), 1);
    }

    #[test]
    fn doubled_tile_projection_scales_paint_and_stored_selection_together() {
        use lumapaint_core::document::Brush;
        let mut source = Document::default();
        source.begin_selection(Point { x: 10.0, y: 10.0 }, SelectionShape::Rectangle);
        source.extend_selection(Point { x: 40.0, y: 40.0 }, true);
        source
            .begin(
                Point { x: 39.0, y: 30.0 },
                Brush {
                    size: 24.0,
                    hardness: 1.0,
                    color: [0, 0, 0],
                },
            )
            .unwrap();
        source.finish();
        let projected = project_committed_paint_layer_at_scale(&source, 2).unwrap();
        assert_eq!(projected.dimensions(), (1920, 1280));
        let tiles = &projected.layers()[0].tiles;
        assert!(tiles.pixel(76, 60).unwrap()[3] > 0);
        assert_eq!(tiles.pixel(82, 60), Some([0; 4]));
        assert!(project_committed_paint_layer_at_scale(&source, 3).is_err());
    }

    #[test]
    fn appended_stroke_matches_full_projection_at_both_resolutions() {
        use lumapaint_core::document::Brush;
        let mut source = Document::default();
        let brush = Brush {
            size: 32.0,
            hardness: 0.4,
            color: [30, 90, 180],
        };
        source.begin(Point { x: 240.0, y: 240.0 }, brush).unwrap();
        source.extend(Point { x: 300.0, y: 300.0 }).unwrap();
        source.finish();
        let before = source.paint_projection_state();
        for scale in [1, 2] {
            let mut incremental = project_committed_paint_layer_at_scale(&source, scale).unwrap();
            let mut edited = source.clone();
            edited.begin_selection(Point { x: 265.0, y: 220.0 }, SelectionShape::Rectangle);
            edited.extend_selection(Point { x: 330.0, y: 320.0 }, true);
            edited.begin(Point { x: 250.0, y: 300.0 }, brush).unwrap();
            edited.extend(Point { x: 320.0, y: 240.0 }).unwrap();
            edited.finish();
            assert!(before.can_append_one(edited.paint_projection_state()));
            let stroke = edited.committed_paint_strokes().last().unwrap();
            let changed =
                paint_stroke_into_tiles_at_scale(&mut incremental, "layer-1", stroke, scale)
                    .unwrap()
                    .unwrap();
            assert!(!changed.coords.is_empty());
            let rebuilt = project_committed_paint_layer_at_scale(&edited, scale).unwrap();
            assert_eq!(
                incremental.prepare_full_uploads(),
                rebuilt.prepare_full_uploads()
            );
        }
    }

    #[test]
    fn undone_stroke_bounds_cover_every_changed_tile_at_both_resolutions() {
        use lumapaint_core::document::Brush;
        let mut source = Document::default();
        let brush = Brush {
            size: 24.0,
            hardness: 0.25,
            color: [20, 40, 80],
        };
        source.begin(Point { x: 80.0, y: 80.0 }, brush).unwrap();
        source.finish();
        source.begin(Point { x: 255.0, y: 280.0 }, brush).unwrap();
        source.extend(Point { x: 520.0, y: 350.0 }).unwrap();
        source.finish();
        for scale in [1, 2] {
            let before = project_committed_paint_layer_at_scale(&source, scale).unwrap();
            let mut undone = source.clone();
            undone.undo();
            let removed = undone.last_undone_paint_stroke().unwrap();
            let coords =
                stroke_candidate_tile_coords_at_scale(removed, source.dimensions(), scale).unwrap();
            let after = project_committed_paint_layer_at_scale(&undone, scale).unwrap();
            let all = after.prepare_changed_uploads(&before).unwrap();
            let bounded = after
                .prepare_changed_uploads_in_coords(&before, &coords)
                .unwrap();
            assert!(!all.is_empty());
            assert_eq!(bounded, all);
            assert!(!coords.contains(&TileCoord { x: 0, y: 0 }));
        }
    }

    #[test]
    fn appearance_updates_match_full_projection_without_replaying_strokes() {
        use lumapaint_core::document::{Brush, LayerSettings};
        let mut source = Document::default();
        source
            .begin(Point { x: 260.0, y: 260.0 }, Brush::default())
            .unwrap();
        source.finish();
        for scale in [1, 2] {
            let mut edited = source.clone();
            let mut projected = project_committed_paint_layer_at_scale(&edited, scale).unwrap();
            for (opacity, mask_enabled, mask_density, visible, expect_empty) in [
                (0.5, false, 1.0, true, false),
                (0.5, true, 0.4, true, false),
                (0.5, true, 0.4, false, false),
                (0.25, true, 0.4, false, true),
                (0.25, true, 0.4, true, false),
            ] {
                edited
                    .set_layer_settings(LayerSettings {
                        id: "layer-1".into(),
                        name: "Layer 1".into(),
                        opacity,
                        locked: false,
                        alpha_locked: false,
                        mask_enabled,
                        mask_inverted: false,
                        mask_density,
                    })
                    .unwrap();
                if edited.snapshot().layers[0].visible != visible {
                    edited.toggle_visibility();
                }
                let uploads = update_projected_paint_appearance(
                    &mut projected,
                    edited.paint_projection_state(),
                )
                .unwrap();
                assert_eq!(uploads.is_empty(), expect_empty);
                let rebuilt = project_committed_paint_layer_at_scale(&edited, scale).unwrap();
                assert_eq!(
                    projected.prepare_full_uploads(),
                    rebuilt.prepare_full_uploads()
                );
            }
        }
    }

    #[test]
    fn partial_drag_cache_is_used_only_while_raster_runs_stay_separate() {
        let bounds = [
            (false, Some([10, 10, 20, 20])),
            (true, Some([40, 10, 50, 20])),
            (false, Some([70, 10, 80, 20])),
        ];
        assert!(bounds_are_separate(&bounds, [0.0, 0.0]));
        assert!(!bounds_are_separate(&bounds, [-25.0, 0.0]));
        assert!(!bounds_are_separate(&bounds, [25.0, 0.0]));
        assert!(bounds_are_separate(&bounds, [0.0, 25.0]));
        let mut pixels = vec![0; 4 * 4 * 4];
        pixels[4 * (2 * 4 + 1) + 3] = 255;
        assert_eq!(alpha_bounds(&pixels, (4, 4)), Some([1, 2, 2, 3]));
    }

    fn separated_drag_document() -> Document {
        use lumapaint_core::vector::{
            FillRule, VectorObject, VectorObjectKind, VectorPaint, VectorPath,
        };
        let mut document = Document::default();
        let layer_id = document.add_vector_layer().unwrap();
        for (id, x, color) in [
            ("moving", 20.0, [255, 0, 0, 255]),
            ("still", 150.0, [0, 0, 255, 255]),
        ] {
            document
                .upsert_vector_object(
                    &layer_id,
                    VectorObject {
                        id: id.into(),
                        name: id.into(),
                        text: None,
                        path: VectorPath {
                            data: "M 0 0 H 30 V 30 H 0 Z".into(),
                            fill_rule: FillRule::NonZero,
                        },
                        transform: [1.0, 0.0, 0.0, 1.0, x, 20.0],
                        fill: Some(VectorPaint { color }),
                        stroke: None,
                        stroke_width: 0.0,
                        visible: true,
                        kind: VectorObjectKind::Rectangle,
                        control_points: vec![[0.0, 0.0], [30.0, 30.0]],
                    },
                )
                .unwrap();
        }
        document
            .select_vector_objects(vec!["moving".into()])
            .unwrap();
        document
    }

    #[test]
    fn separated_vector_drag_runs_match_full_preview_pixels() {
        let document = separated_drag_document();
        let layer_id = document.svg_layers().next().unwrap().id.clone();
        let layer = document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap();
        let runs = document.vector_drag_runs(layer).unwrap();
        let full = document
            .translated_vector_layer(layer, 15.0, 0.0)
            .unwrap()
            .unwrap();
        let expected = vector::rasterize_svg(&full.source, 960, 640)
            .unwrap()
            .pixels;
        let mut combined = vec![0u8; expected.len()];
        for (selected, part) in runs {
            let raster = vector::rasterize_svg(&part.source, 960, 640).unwrap();
            for y in 0..640usize {
                for x in 0..960usize {
                    let source = (y * 960 + x) * 4;
                    if raster.pixels[source + 3] == 0 {
                        continue;
                    }
                    let target_x = x + if selected { 15 } else { 0 };
                    let target = (y * 960 + target_x) * 4;
                    assert_eq!(combined[target + 3], 0);
                    combined[target..target + 4]
                        .copy_from_slice(&raster.pixels[source..source + 4]);
                }
            }
        }
        assert_eq!(combined, expected);
    }

    #[test]
    #[ignore = "Diagnostic CPU benchmark; run explicitly with --ignored --nocapture"]
    fn vector_drag_raster_cost_diagnostic() {
        let document = separated_drag_document();
        let layer = document.svg_layers().next().unwrap();
        let offsets = (0..24).map(|step| [step as f32 * 3.0, 0.0]);
        let started = std::time::Instant::now();
        let mut full_bytes = 0usize;
        for offset in offsets {
            let preview = document
                .translated_vector_layer(layer, offset[0], offset[1])
                .unwrap()
                .unwrap();
            full_bytes += vector::rasterize_svg(&preview.source, 960, 640)
                .unwrap()
                .pixels
                .len();
        }
        let full_time = started.elapsed();
        let started = std::time::Instant::now();
        let cached_bytes: usize = document
            .vector_drag_runs(layer)
            .unwrap()
            .iter()
            .map(|(_, run)| {
                vector::rasterize_svg(&run.source, 960, 640)
                    .unwrap()
                    .pixels
                    .len()
            })
            .sum();
        let cache_time = started.elapsed();
        assert!(cached_bytes < full_bytes);
        eprintln!(
            "24 drag offsets: full CPU raster {:.2} ms, {} MiB; split cache setup {:.2} ms, {} MiB",
            full_time.as_secs_f64() * 1000.0,
            full_bytes / (1024 * 1024),
            cache_time.as_secs_f64() * 1000.0,
            cached_bytes / (1024 * 1024)
        );
    }

    #[test]
    fn prepared_svg_keeps_identity_and_applies_effective_opacity() {
        let layer = SvgLayer {
            id: "layer-1".into(),
            name: "Sample".into(),
            visible: true,
            opacity: 0.5,
            locked: false,
            alpha_locked: false,
            mask_enabled: true,
            mask_inverted: false,
            mask_density: 0.5,
            source: "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><rect width='8' height='8' fill='red'/></svg>".into(),
            paint_layer: false,
            vector_layer: false,
            vector_objects: Vec::new(),
        };
        let prepared = prepare_svg_layer(&layer, 8, 8).unwrap();
        assert_eq!(prepared.id, layer.id);
        assert_eq!(prepared.source, layer.source);
        assert_eq!(prepared.opacity, 0.25);
        assert_eq!(prepared.size, (8, 8));
        assert_eq!(prepared.pixels.len(), 8 * 8 * 4);
        assert_eq!(&prepared.pixels[4 * (4 * 8 + 4)..][..4], &[64, 0, 0, 64]);
    }

    #[test]
    fn brush_curve_interpolation_preserves_endpoints_and_adds_smooth_samples() {
        let points = [
            Point { x: 0.0, y: 10.0 },
            Point { x: 10.0, y: 0.0 },
            Point { x: 20.0, y: 10.0 },
        ];
        let smoothed = smooth_points(&points, &[0.25, 0.5, 1.0]);
        assert!(smoothed.len() > points.len());
        assert_eq!((smoothed[0].0.x, smoothed[0].0.y), (0.0, 10.0));
        assert_eq!(smoothed[0].1, 0.25);
        let last = smoothed.last().unwrap();
        assert!((last.0.x - 20.0).abs() < 0.001);
        assert!((last.0.y - 10.0).abs() < 0.001);
        assert!((last.1 - 1.0).abs() < 0.001);
        assert!(smoothed.iter().any(|point| point.0.y < 1.0));
    }

    #[test]
    fn pressure_changes_brush_radius_along_the_stroke() {
        let stroke = Stroke {
            brush: lumapaint_core::document::Brush {
                size: 40.0,
                ..Default::default()
            },
            points: vec![Point { x: 10.0, y: 10.0 }, Point { x: 110.0, y: 10.0 }],
            pressures: vec![0.25, 1.0],
            selection: None,
        };
        let dabs = segments(&stroke, 1.0);
        assert!(dabs.first().unwrap().radius < 6.0);
        assert!(dabs.last().unwrap().radius > 19.0);
    }

    #[test]
    fn document_center_is_stable_at_retina_and_zoom() {
        let viewport = Viewport::new(800.0, 500.0, 2.0, 2.0, true).unwrap();
        let point = viewport.document_point(400.0, 250.0);
        assert_eq!((point.x, point.y), (480.0, 320.0));
    }

    #[test]
    fn pan_offset_moves_document_and_pointer_mapping_together() {
        let viewport = Viewport::new(800.0, 500.0, 2.0, 2.0, true)
            .unwrap()
            .with_pan(40.0, -20.0)
            .unwrap();
        let point = viewport.document_point(440.0, 230.0);
        assert!((point.x - WIDTH * 0.5).abs() < 0.001);
        assert!((point.y - HEIGHT * 0.5).abs() < 0.001);
    }

    #[test]
    fn zoom_around_keeps_clicked_document_point_under_pointer() {
        let viewport = Viewport::new(800.0, 500.0, 2.0, 1.0, true)
            .unwrap()
            .with_pan(35.0, -18.0)
            .unwrap();
        let anchor = (270.0, 190.0);
        let before = viewport.document_point(anchor.0, anchor.1);
        let zoomed = viewport.zoom_around(anchor.0, anchor.1, 1.25);
        let after = zoomed.document_point(anchor.0, anchor.1);
        assert!((before.x - after.x).abs() < 0.001);
        assert!((before.y - after.y).abs() < 0.001);
        assert_eq!(zoomed.zoom, 1.25);
    }

    #[test]
    fn custom_document_size_controls_pointer_mapping() {
        let viewport = Viewport::new(800.0, 500.0, 1.0, 1.0, false)
            .unwrap()
            .with_document(434, 418, CanvasColor::White)
            .unwrap();
        let point = viewport.document_point(400.0, 250.0);
        assert_eq!((point.x, point.y), (217.0, 209.0));
    }

    #[test]
    fn retina_dimensions_preserve_logical_size() {
        let viewport = Viewport::new(641.5, 300.0, 2.0, 1.0, false).unwrap();
        assert_eq!((viewport.width, viewport.height), (1283, 600));
        assert_eq!(viewport.scale, 2.0);
    }

    #[test]
    fn tile_filter_accounts_for_fit_zoom_and_retina_scale() {
        let base = Viewport::new(1024.0, 768.0, 1.0, 960.0 / 976.0, false).unwrap();
        assert!(!base.tile_filter_nearest());
        let reduced = Viewport::new(1024.0, 768.0, 1.0, 0.5, false).unwrap();
        assert!(reduced.tile_filter_nearest());
        let retina = Viewport::new(512.0, 384.0, 2.0, 1.0, false).unwrap();
        assert!(!retina.tile_filter_nearest());
        assert_eq!(base.recommended_tile_preview_scale(), 1);
        assert_eq!(reduced.recommended_tile_preview_scale(), 1);
        assert_eq!(retina.recommended_tile_preview_scale(), 1);
        assert_eq!(base.compatible_tile_preview_scale(), Some(1));
        assert_eq!(reduced.compatible_tile_preview_scale(), None);
        assert_eq!(retina.compatible_tile_preview_scale(), Some(1));
        let enlarged = Viewport::new(1024.0, 768.0, 1.0, 2.0, false).unwrap();
        assert_eq!(enlarged.recommended_tile_preview_scale(), 2);
        assert_eq!(enlarged.compatible_tile_preview_scale(), Some(2));
    }

    #[test]
    fn invalid_and_oversized_viewports_are_rejected_before_allocation() {
        for width in [0.0, -1.0, f64::NAN, f64::INFINITY, 8193.0] {
            assert!(Viewport::new(width, 300.0, 1.0, 1.0, false).is_err());
        }
        assert!(Viewport::new(5000.0, 300.0, 2.0, 1.0, false).is_err());
        assert!(Viewport::new(300.0, 300.0, 0.0, 1.0, false).is_err());
        assert!(Viewport::new(300.0, 300.0, 1.0, 0.0, false).is_err());
    }

    #[test]
    fn svg_validation_rasterizes_vector_content() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><circle cx="20" cy="10" r="8" fill="#538fd2"/></svg>"##;
        let pixels = rasterize_svg(svg, WIDTH as u32, HEIGHT as u32).unwrap();
        assert_eq!(pixels.len(), WIDTH as usize * HEIGHT as usize * 4);
        assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
        assert!(validate_svg("not svg").is_err());
    }
}

#[cfg(test)]
mod gpu_tests;
