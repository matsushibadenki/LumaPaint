//! Session document: a stable layer identity and retained brush strokes.
//! This is deliberately independent of pixels, AppKit, and the renderer.
use crate::selection::SelectionGesture;
pub use crate::selection::{
    Selection, SelectionMode, SelectionOperation, SelectionRegion, SelectionShape,
};
use crate::vector::{PathOperation, VectorObject, VectorObjectKind, VectorPath, VectorText};
use serde::{Deserialize, Serialize};

pub const WIDTH: f32 = 960.0;
pub const HEIGHT: f32 = 640.0;
pub(crate) const MAX_DOCUMENT_DIMENSION: u32 = 8192;
const MAX_POINTS: usize = 65_536;
pub const MAX_BRUSH_SIZE: f32 = 512.0;
const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;
const MAX_SVG_TOTAL_BYTES: usize = 6 * 1024 * 1024;
const MAX_SVG_LAYERS: usize = 16;
const DEFAULT_BIT_DEPTH: u8 = 8;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorMode {
    #[default]
    Rgb,
    Cmyk,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorProfile {
    #[default]
    Srgb,
    DisplayP3,
    AdobeRgb1998,
    JapanColor2001Coated,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentUnit {
    #[default]
    Pixels,
    Inches,
    Centimeters,
    Millimeters,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CanvasColor {
    #[default]
    White,
    Transparent,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentSettings {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub unit: DocumentUnit,
    pub resolution: u32,
    pub artboards: bool,
    pub canvas_color: CanvasColor,
    pub pixel_aspect_ratio: f32,
}

impl ColorProfile {
    fn mode(self) -> ColorMode {
        match self {
            Self::Srgb | Self::DisplayP3 | Self::AdobeRgb1998 => ColorMode::Rgb,
            Self::JapanColor2001Coated => ColorMode::Cmyk,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Brush {
    pub size: f32,
    #[serde(default = "default_hardness")]
    pub hardness: f32,
    pub color: [u8; 3],
}

const fn default_hardness() -> f32 {
    1.0
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            size: 16.0,
            hardness: default_hardness(),
            color: [32, 32, 32],
        }
    }
}

impl Brush {
    pub fn validate(&self) -> Result<(), String> {
        if !self.size.is_finite() || !(1.0..=MAX_BRUSH_SIZE).contains(&self.size) {
            return Err("Brush size must be between 1 and 512 px".into());
        }
        if !self.hardness.is_finite() || !(0.0..=1.0).contains(&self.hardness) {
            return Err("Brush hardness must be between 0 and 1".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    fn valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.x.abs() < 100_000.0
            && self.y.abs() < 100_000.0
    }
    fn on_page(self, width: u32, height: u32) -> bool {
        self.x >= 0.0 && self.y >= 0.0 && self.x < width as f32 && self.y < height as f32
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub brush: Brush,
    pub points: Vec<Point>,
    #[serde(default)]
    pub pressures: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SvgLayer {
    pub id: String,
    pub name: String,
    pub visible: bool,
    #[serde(default = "default_layer_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub alpha_locked: bool,
    #[serde(default)]
    pub mask_enabled: bool,
    #[serde(default)]
    pub mask_inverted: bool,
    #[serde(default = "default_layer_opacity")]
    pub mask_density: f32,
    pub source: String,
    #[serde(default)]
    pub paint_layer: bool,
    #[serde(default)]
    pub vector_layer: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vector_objects: Vec<VectorObject>,
}
impl SvgLayer {
    pub fn effective_opacity(&self) -> f32 {
        self.opacity * mask_factor(self.mask_enabled, self.mask_inverted, self.mask_density)
    }
}

const fn default_layer_opacity() -> f32 {
    1.0
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerSettings {
    pub id: String,
    pub name: String,
    pub opacity: f32,
    pub locked: bool,
    #[serde(default)]
    pub alpha_locked: bool,
    #[serde(default)]
    pub mask_enabled: bool,
    #[serde(default)]
    pub mask_inverted: bool,
    #[serde(default = "default_layer_opacity")]
    pub mask_density: f32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerObjectSnapshot {
    pub id: String,
    pub name: String,
    pub kind: VectorObjectKind,
    pub visible: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerSnapshot {
    pub objects: Vec<LayerObjectSnapshot>,
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub visible: bool,
    pub opacity: f32,
    pub locked: bool,
    pub alpha_locked: bool,
    pub mask_enabled: bool,
    pub mask_inverted: bool,
    pub mask_density: f32,
    pub deletable: bool,
    pub stroke_count: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextObjectSnapshot {
    pub id: String,
    pub layer_id: String,
    pub text: VectorText,
    pub position: [f32; 2],
    pub color: [u8; 3],
    pub editable: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextSettings {
    pub id: Option<String>,
    pub text: VectorText,
    pub position: [f32; 2],
    pub color: [u8; 3],
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSnapshot {
    pub selection: Option<Selection>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub unit: DocumentUnit,
    pub resolution: u32,
    pub artboards: bool,
    pub canvas_color: CanvasColor,
    pub pixel_aspect_ratio: f32,
    pub layer_id: String,
    pub layer_visible: bool,
    pub color_mode: ColorMode,
    pub color_profile: ColorProfile,
    pub bit_depth: u8,
    pub stroke_count: usize,
    pub layers: Vec<LayerSnapshot>,
    pub selected_vector_objects: Vec<String>,
    pub text_objects: Vec<TextObjectSnapshot>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub revision: u64,
    pub dirty: bool,
    pub file_name: Option<String>,
}

#[derive(Clone, Copy)]
enum HistoryKind {
    Stroke,
    Vector,
}

#[derive(Clone)]
struct VectorHistoryState {
    layers: Vec<SvgLayer>,
    selection: Vec<String>,
}

/// Minimal state needed to decide whether a projected paint tile cache can be
/// reused or extended after a document revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintProjectionState {
    pub stroke_count: usize,
    visible: bool,
    opacity: u32,
    locked: bool,
    alpha_locked: bool,
    mask_enabled: bool,
    mask_inverted: bool,
    mask_density: u32,
}

impl PaintProjectionState {
    /// A document edit did not change the projected paint pixels. Lock flags
    /// affect future edits but do not alter already committed pixels.
    pub fn same_composite(self, next: Self) -> bool {
        self.stroke_count == next.stroke_count
            && self.visible == next.visible
            && self.opacity == next.opacity
            && self.mask_enabled == next.mask_enabled
            && self.mask_inverted == next.mask_inverted
            && self.mask_density == next.mask_density
    }

    pub fn locks(self) -> (bool, bool) {
        (self.locked, self.alpha_locked)
    }

    pub fn tile_appearance(self) -> (bool, f32, bool, bool, f32) {
        (
            self.visible,
            f32::from_bits(self.opacity),
            self.mask_enabled,
            self.mask_inverted,
            f32::from_bits(self.mask_density),
        )
    }

    pub fn same_appearance(self, next: Self) -> bool {
        Self {
            stroke_count: next.stroke_count,
            ..self
        } == next
    }

    pub fn can_append_one(self, next: Self) -> bool {
        self.visible
            && !self.locked
            && !self.alpha_locked
            && self.stroke_count.checked_add(1) == Some(next.stroke_count)
            && self.same_appearance(next)
    }
}

#[derive(Clone)]
pub struct Document {
    selection: Option<Selection>,
    selection_anchor: Option<SelectionGesture>,
    name: String,
    width: u32,
    height: u32,
    unit: DocumentUnit,
    resolution: u32,
    artboards: bool,
    canvas_color: CanvasColor,
    pixel_aspect_ratio: f32,
    strokes: Vec<Stroke>,
    redo: Vec<Stroke>,
    active: Option<Stroke>,
    visible: bool,
    layer_name: String,
    layer_opacity: f32,
    layer_locked: bool,
    layer_alpha_locked: bool,
    layer_mask_enabled: bool,
    layer_mask_inverted: bool,
    layer_mask_density: f32,
    svg_layers: Vec<SvgLayer>,
    selected_layer: Option<String>,
    selected_vector_objects: Vec<String>,
    vector_undo: Vec<VectorHistoryState>,
    vector_redo: Vec<VectorHistoryState>,
    undo_order: Vec<HistoryKind>,
    redo_order: Vec<HistoryKind>,
    color_mode: ColorMode,
    color_profile: ColorProfile,
    bit_depth: u8,
    revision: u64,
    point_count: usize,
    saved_revision: u64,
    file_name: Option<String>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            selection: None,
            selection_anchor: None,
            name: "Untitled-1".into(),
            width: WIDTH as u32,
            height: HEIGHT as u32,
            unit: DocumentUnit::Pixels,
            resolution: 72,
            artboards: false,
            canvas_color: CanvasColor::White,
            pixel_aspect_ratio: 1.0,
            strokes: Vec::new(),
            redo: Vec::new(),
            active: None,
            visible: true,
            layer_name: "Background".into(),
            layer_opacity: 1.0,
            layer_locked: false,
            layer_alpha_locked: false,
            layer_mask_enabled: false,
            layer_mask_inverted: false,
            layer_mask_density: 1.0,
            svg_layers: Vec::new(),
            selected_layer: None,
            selected_vector_objects: Vec::new(),
            vector_undo: Vec::new(),
            vector_redo: Vec::new(),
            undo_order: Vec::new(),
            redo_order: Vec::new(),
            color_mode: ColorMode::default(),
            color_profile: ColorProfile::default(),
            bit_depth: DEFAULT_BIT_DEPTH,
            revision: 0,
            point_count: 0,
            saved_revision: 0,
            file_name: None,
        }
    }
}

impl Document {
    /// Pixel dimensions without allocating a UI snapshot or cloning text/layer metadata.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn has_active_stroke(&self) -> bool {
        self.active.is_some()
    }

    pub fn paint_projection_state(&self) -> PaintProjectionState {
        PaintProjectionState {
            stroke_count: self.strokes.len(),
            visible: self.visible,
            opacity: self.layer_opacity.to_bits(),
            locked: self.layer_locked,
            alpha_locked: self.layer_alpha_locked,
            mask_enabled: self.layer_mask_enabled,
            mask_inverted: self.layer_mask_inverted,
            mask_density: self.layer_mask_density.to_bits(),
        }
    }

    pub fn snapshot(&self) -> DocumentSnapshot {
        let mut layers = vec![LayerSnapshot {
            objects: Vec::new(),
            id: "layer-1".into(),
            name: self.layer_name.clone(),
            kind: "paint",
            visible: self.visible,
            opacity: self.layer_opacity,
            locked: self.layer_locked,
            alpha_locked: self.layer_alpha_locked,
            mask_enabled: self.layer_mask_enabled,
            mask_inverted: self.layer_mask_inverted,
            mask_density: self.layer_mask_density,
            deletable: false,
            stroke_count: self.strokes.len(),
        }];
        layers.extend(self.svg_layers.iter().map(|layer| {
            LayerSnapshot {
                objects: layer
                    .vector_objects
                    .iter()
                    .map(|object| LayerObjectSnapshot {
                        id: object.id.clone(),
                        name: object.name.clone(),
                        kind: object.kind,
                        visible: object.visible,
                    })
                    .collect(),
                id: layer.id.clone(),
                name: layer.name.clone(),
                kind: if layer.paint_layer {
                    "paint"
                } else if layer.vector_layer {
                    "vector"
                } else {
                    "svg"
                },
                visible: layer.visible,
                opacity: layer.opacity,
                locked: layer.locked,
                alpha_locked: layer.alpha_locked,
                mask_enabled: layer.mask_enabled,
                mask_inverted: layer.mask_inverted,
                mask_density: layer.mask_density,
                deletable: true,
                stroke_count: 0,
            }
        }));
        DocumentSnapshot {
            selection: self.selection.clone(),
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            unit: self.unit,
            resolution: self.resolution,
            artboards: self.artboards,
            canvas_color: self.canvas_color,
            pixel_aspect_ratio: self.pixel_aspect_ratio,
            layer_id: self
                .selected_layer
                .clone()
                .filter(|id| id == "layer-1" || self.svg_layers.iter().any(|layer| &layer.id == id))
                .unwrap_or_else(|| "layer-1".into()),
            layer_visible: self.visible,
            color_mode: self.color_mode,
            color_profile: self.color_profile,
            bit_depth: self.bit_depth,
            stroke_count: self.strokes.len(),
            layers,
            text_objects: self
                .svg_layers
                .iter()
                .filter(|layer| layer.vector_layer)
                .flat_map(|layer| {
                    layer.vector_objects.iter().filter_map(move |object| {
                        object.text.as_ref().map(|text| {
                            let color = object.fill.map_or([0, 0, 0, 255], |fill| fill.color);
                            TextObjectSnapshot {
                                id: object.id.clone(),
                                layer_id: layer.id.clone(),
                                text: text.clone(),
                                position: [object.transform[4], object.transform[5]],
                                color: [color[0], color[1], color[2]],
                                editable: !layer.locked && layer.visible && object.visible,
                            }
                        })
                    })
                })
                .collect(),
            selected_vector_objects: self.selected_vector_objects.clone(),
            can_undo: !self.undo_order.is_empty(),
            can_redo: !self.redo_order.is_empty(),
            revision: self.revision,
            dirty: self.revision != self.saved_revision || self.active.is_some(),
            file_name: self.file_name.clone(),
        }
    }
    pub fn encode(&mut self) -> Result<Vec<u8>, String> {
        self.finish();
        serde_json::to_vec(&ProjectFile {
            format: "LumaPaint".into(),
            version: 1,
            name: Some(self.name.clone()),
            width: self.width,
            height: self.height,
            unit: Some(self.unit),
            resolution: Some(self.resolution),
            artboards: Some(self.artboards),
            canvas_color: Some(self.canvas_color),
            pixel_aspect_ratio: Some(self.pixel_aspect_ratio),
            layer_visible: self.visible,
            layer_name: Some(self.layer_name.clone()),
            layer_opacity: Some(self.layer_opacity),
            layer_locked: Some(self.layer_locked),
            layer_alpha_locked: Some(self.layer_alpha_locked),
            layer_mask_enabled: Some(self.layer_mask_enabled),
            layer_mask_inverted: Some(self.layer_mask_inverted),
            layer_mask_density: Some(self.layer_mask_density),
            color_mode: self.color_mode,
            color_profile: Some(self.color_profile),
            bit_depth: self.bit_depth,
            strokes: self.strokes.clone(),
            svg_layers: self.svg_layers.clone(),
        })
        .map_err(|e| e.to_string())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err("Project exceeds 8 MiB".into());
        }
        let file: ProjectFile = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if file.format != "LumaPaint"
            || file.version != 1
            || file.width == 0
            || file.height == 0
            || file.width > MAX_DOCUMENT_DIMENSION
            || file.height > MAX_DOCUMENT_DIMENSION
        {
            return Err("Unsupported project format, version, or dimensions".into());
        }
        if file
            .name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty() || name.chars().count() > 120)
            || !(1..=1200).contains(&file.resolution.unwrap_or(72))
            || !file.pixel_aspect_ratio.unwrap_or(1.0).is_finite()
            || !(0.1..=10.0).contains(&file.pixel_aspect_ratio.unwrap_or(1.0))
            || !file.layer_opacity.unwrap_or(1.0).is_finite()
            || !(0.0..=1.0).contains(&file.layer_opacity.unwrap_or(1.0))
            || !file.layer_mask_density.unwrap_or(1.0).is_finite()
            || !(0.0..=1.0).contains(&file.layer_mask_density.unwrap_or(1.0))
        {
            return Err("Invalid document settings".into());
        }
        validate_bit_depth(file.bit_depth)?;
        let color_profile = file.color_profile.unwrap_or(match file.color_mode {
            ColorMode::Rgb => ColorProfile::Srgb,
            ColorMode::Cmyk => ColorProfile::JapanColor2001Coated,
        });
        if color_profile.mode() != file.color_mode {
            return Err("Color profile is incompatible with the document color mode".into());
        }
        let mut count = 0;
        for stroke in &file.strokes {
            stroke.brush.validate()?;
            if let Some(selection) = &stroke.selection {
                selection.validate()?;
            }
            if stroke.points.is_empty() || stroke.points.iter().any(|p| !p.valid()) {
                return Err("Invalid stroke points".into());
            }
            if (!stroke.pressures.is_empty() && stroke.pressures.len() != stroke.points.len())
                || stroke
                    .pressures
                    .iter()
                    .any(|pressure| !pressure.is_finite() || !(0.0..=1.0).contains(pressure))
            {
                return Err("Invalid stroke pressure data".into());
            }
            count += stroke.points.len();
            if count > MAX_POINTS {
                return Err("Too many stroke points".into());
            }
        }
        for layer in &file.svg_layers {
            validate_svg_layer(layer)?;
        }
        if file.svg_layers.len() > MAX_SVG_LAYERS
            || file
                .svg_layers
                .iter()
                .map(|layer| layer.source.len())
                .sum::<usize>()
                > MAX_SVG_TOTAL_BYTES
        {
            return Err("Project contains too many SVG layers or SVG data".into());
        }
        let stroke_count = file.strokes.len();
        Ok(Self {
            name: file.name.unwrap_or_else(|| "Untitled-1".into()),
            width: file.width,
            height: file.height,
            unit: file.unit.unwrap_or_default(),
            resolution: file.resolution.unwrap_or(72),
            artboards: file.artboards.unwrap_or(false),
            canvas_color: file.canvas_color.unwrap_or_default(),
            pixel_aspect_ratio: file.pixel_aspect_ratio.unwrap_or(1.0),
            strokes: file.strokes,
            svg_layers: file.svg_layers,
            visible: file.layer_visible,
            layer_name: file.layer_name.unwrap_or_else(|| "Background".into()),
            layer_opacity: file.layer_opacity.unwrap_or(1.0),
            layer_locked: file.layer_locked.unwrap_or(false),
            layer_alpha_locked: file.layer_alpha_locked.unwrap_or(false),
            layer_mask_enabled: file.layer_mask_enabled.unwrap_or(false),
            layer_mask_inverted: file.layer_mask_inverted.unwrap_or(false),
            layer_mask_density: file.layer_mask_density.unwrap_or(1.0),
            color_mode: file.color_mode,
            color_profile,
            bit_depth: file.bit_depth,
            point_count: count,
            undo_order: vec![HistoryKind::Stroke; stroke_count],
            ..Self::default()
        })
    }
    pub fn mark_saved(&mut self, name: String) {
        self.saved_revision = self.revision;
        self.file_name = Some(name);
    }
    pub fn set_color_mode(&mut self, mode: ColorMode) {
        self.finish();
        if self.color_mode != mode {
            self.color_mode = mode;
            self.color_profile = match mode {
                ColorMode::Rgb => ColorProfile::Srgb,
                ColorMode::Cmyk => ColorProfile::JapanColor2001Coated,
            };
            self.revision += 1;
        }
    }
    pub fn set_color_profile(&mut self, profile: ColorProfile) -> Result<(), String> {
        if profile.mode() != self.color_mode {
            return Err("Color profile is incompatible with the document color mode".into());
        }
        self.finish();
        if self.color_profile != profile {
            self.color_profile = profile;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn set_bit_depth(&mut self, depth: u8) -> Result<(), String> {
        validate_bit_depth(depth)?;
        self.finish();
        if self.bit_depth != depth {
            self.bit_depth = depth;
            self.revision += 1;
        }
        Ok(())
    }
    pub fn set_document_settings(&mut self, settings: DocumentSettings) -> Result<(), String> {
        let name = settings.name.trim();
        if name.is_empty() || name.chars().count() > 120 {
            return Err("Document name must contain 1 to 120 characters".into());
        }
        if settings.width == 0
            || settings.height == 0
            || settings.width > MAX_DOCUMENT_DIMENSION
            || settings.height > MAX_DOCUMENT_DIMENSION
            || !(1..=1200).contains(&settings.resolution)
            || !settings.pixel_aspect_ratio.is_finite()
            || !(0.1..=10.0).contains(&settings.pixel_aspect_ratio)
        {
            return Err("Invalid document settings".into());
        }
        self.finish();
        if self.width != settings.width || self.height != settings.height {
            self.deselect();
        }
        self.name = name.into();
        self.width = settings.width;
        self.height = settings.height;
        self.unit = settings.unit;
        self.resolution = settings.resolution;
        self.artboards = settings.artboards;
        self.canvas_color = settings.canvas_color;
        self.pixel_aspect_ratio = settings.pixel_aspect_ratio;
        self.revision += 1;
        Ok(())
    }
    pub fn replace_loaded(&mut self, mut document: Self, name: String) {
        document.revision = self.revision + 1;
        document.mark_saved(name);
        *self = document;
    }
    /// Recovery never restores a writable source path or marks work as manually saved.
    pub fn replace_recovered(&mut self, mut document: Self) {
        document.revision = self.revision + 1;
        document.saved_revision = 0;
        document.file_name = None;
        *self = document;
    }
    pub fn begin(&mut self, point: Point, brush: Brush) -> Result<bool, String> {
        self.begin_with_pressure(point, brush, 1.0)
    }
    pub fn begin_with_pressure(
        &mut self,
        point: Point,
        brush: Brush,
        pressure: f32,
    ) -> Result<bool, String> {
        brush.validate()?;
        if self
            .selected_layer
            .as_deref()
            .is_some_and(|id| id != "layer-1")
        {
            return Err("ブラシは基礎ペイントレイヤーを選択してください。\nSelect the base paint layer for brush strokes.\n请为画笔选择基础绘画图层。".into());
        }
        if !point.valid() || !pressure.is_finite() || !(0.0..=1.0).contains(&pressure) {
            return Err("Invalid pointer position".into());
        }
        self.finish();
        if !self.visible
            || self.layer_locked
            || !point.on_page(self.width, self.height)
            || (self.layer_alpha_locked && !self.point_has_paint_alpha(point))
        {
            return Ok(false);
        }
        if self.point_count >= MAX_POINTS {
            return Err("Session stroke limit reached. Undo a stroke to continue.".into());
        }
        self.active = Some(Stroke {
            brush,
            points: vec![point],
            pressures: vec![pressure],
            selection: self.selection.clone(),
        });
        Ok(true)
    }
    pub fn extend(&mut self, point: Point) -> Result<(), String> {
        self.extend_with_pressure(point, 1.0)
    }
    pub fn extend_with_pressure(&mut self, point: Point, pressure: f32) -> Result<(), String> {
        if !point.valid() || !pressure.is_finite() || !(0.0..=1.0).contains(&pressure) {
            return Err("Invalid pointer position".into());
        }
        if self.layer_alpha_locked && !self.point_has_paint_alpha(point) {
            return Ok(());
        }
        if let Some(stroke) = self.active.as_mut() {
            if self.point_count + stroke.points.len() >= MAX_POINTS {
                return Err("Session stroke limit reached. Undo a stroke to continue.".into());
            }
            if let Some(last) = stroke.points.last() {
                if (point.x - last.x).hypot(point.y - last.y) < 0.35 {
                    return Ok(());
                }
            }
            stroke.points.push(point);
            stroke.pressures.push(pressure);
        }
        Ok(())
    }
    pub fn finish(&mut self) {
        if let Some(stroke) = self.active.take() {
            self.point_count += stroke.points.len();
            self.strokes.push(stroke);
            self.redo.clear();
            self.vector_redo.clear();
            self.redo_order.clear();
            self.undo_order.push(HistoryKind::Stroke);
            self.revision += 1;
        }
    }
    pub fn undo(&mut self) {
        self.finish();
        match self.undo_order.pop() {
            Some(HistoryKind::Stroke) => {
                if let Some(stroke) = self.strokes.pop() {
                    self.point_count -= stroke.points.len();
                    self.redo.push(stroke);
                    self.redo_order.push(HistoryKind::Stroke);
                    self.revision += 1;
                }
            }
            Some(HistoryKind::Vector) => {
                if let Some(previous) = self.vector_undo.pop() {
                    self.vector_redo.push(self.vector_history_state());
                    self.restore_vector_history(previous);
                    self.redo_order.push(HistoryKind::Vector);
                    self.revision += 1;
                }
            }
            None => {}
        }
    }
    pub fn redo(&mut self) {
        self.finish();
        match self.redo_order.pop() {
            Some(HistoryKind::Stroke) => {
                if let Some(stroke) = self.redo.pop() {
                    self.point_count += stroke.points.len();
                    self.strokes.push(stroke);
                    self.undo_order.push(HistoryKind::Stroke);
                    self.revision += 1;
                }
            }
            Some(HistoryKind::Vector) => {
                if let Some(next) = self.vector_redo.pop() {
                    self.vector_undo.push(self.vector_history_state());
                    self.restore_vector_history(next);
                    self.undo_order.push(HistoryKind::Vector);
                    self.revision += 1;
                }
            }
            None => {}
        }
    }
    pub fn toggle_visibility(&mut self) {
        self.finish();
        self.visible = !self.visible;
        self.revision += 1;
    }
    pub fn toggle_layer(&mut self, id: &str) -> Result<(), String> {
        self.finish();
        if id == "layer-1" {
            self.visible = !self.visible;
        } else if let Some(layer) = self.svg_layers.iter_mut().find(|layer| layer.id == id) {
            layer.visible = !layer.visible;
        } else {
            return Err("Layer not found".into());
        }
        self.revision += 1;
        Ok(())
    }
    pub fn set_layer_settings(&mut self, settings: LayerSettings) -> Result<(), String> {
        self.finish();
        let name = settings.name.trim();
        if name.is_empty()
            || name.chars().count() > 120
            || !settings.opacity.is_finite()
            || !(0.0..=1.0).contains(&settings.opacity)
            || !settings.mask_density.is_finite()
            || !(0.0..=1.0).contains(&settings.mask_density)
        {
            return Err("Invalid layer settings".into());
        }
        if settings.id == "layer-1" {
            self.layer_name = name.into();
            self.layer_opacity = settings.opacity;
            self.layer_locked = settings.locked;
            self.layer_alpha_locked = settings.alpha_locked;
            self.layer_mask_enabled = settings.mask_enabled;
            self.layer_mask_inverted = settings.mask_inverted;
            self.layer_mask_density = settings.mask_density;
        } else if let Some(layer) = self
            .svg_layers
            .iter_mut()
            .find(|layer| layer.id == settings.id)
        {
            layer.name = name.into();
            layer.opacity = settings.opacity;
            layer.locked = settings.locked;
            layer.alpha_locked = settings.alpha_locked;
            layer.mask_enabled = settings.mask_enabled;
            layer.mask_inverted = settings.mask_inverted;
            layer.mask_density = settings.mask_density;
        } else {
            return Err("Layer not found".into());
        }
        self.revision += 1;
        Ok(())
    }
    pub fn delete_layer(&mut self, id: &str) -> Result<(), String> {
        self.finish();
        if id == "layer-1" {
            return Err("The paint layer cannot be deleted yet".into());
        }
        let before_state = self.vector_history_state();
        let before = self.svg_layers.len();
        let was_vector = self
            .svg_layers
            .iter()
            .any(|layer| layer.id == id && layer.vector_layer);
        self.svg_layers.retain(|layer| layer.id != id);
        if self.svg_layers.len() == before {
            return Err("Layer not found".into());
        }
        if was_vector {
            let layers = &self.svg_layers;
            self.selected_vector_objects.retain(|selected| {
                layers.iter().any(|layer| {
                    layer
                        .vector_objects
                        .iter()
                        .any(|object| &object.id == selected)
                })
            });
            self.record_vector_edit(before_state);
        }
        self.revision += 1;
        Ok(())
    }
    pub fn add_paint_layer(&mut self) -> Result<String, String> {
        self.finish();
        if self.svg_layers.len() >= MAX_SVG_LAYERS {
            return Err("A project can contain up to 17 layers".into());
        }
        let next = self
            .svg_layers
            .iter()
            .filter(|layer| layer.paint_layer)
            .count()
            + 2;
        let mut serial = next;
        while self
            .svg_layers
            .iter()
            .any(|layer| layer.id == format!("paint-layer-{serial}"))
        {
            serial += 1;
        }
        let id = format!("paint-layer-{serial}");
        self.svg_layers.push(SvgLayer {
            id: id.clone(),
            name: format!("Layer {serial}"),
            visible: true,
            opacity: 1.0,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
            source: r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#.into(),
            paint_layer: true,
            vector_layer: false,
            vector_objects: vec![],
        });
        self.revision += 1;
        Ok(id)
    }
    pub fn reorder_layers(&mut self, ids_top_to_bottom: &[String]) -> Result<(), String> {
        if ids_top_to_bottom.len() != self.svg_layers.len()
            || ids_top_to_bottom.iter().any(|id| id == "layer-1")
            || self
                .svg_layers
                .iter()
                .any(|layer| !ids_top_to_bottom.contains(&layer.id))
        {
            return Err("Layer order does not match the document".into());
        }
        if self
            .svg_layers
            .iter()
            .rev()
            .map(|layer| &layer.id)
            .eq(ids_top_to_bottom.iter())
        {
            return Ok(());
        }
        self.finish();
        let mut reordered = Vec::with_capacity(self.svg_layers.len());
        for id in ids_top_to_bottom.iter().rev() {
            let index = self
                .svg_layers
                .iter()
                .position(|layer| &layer.id == id)
                .ok_or("Layer not found")?;
            reordered.push(self.svg_layers[index].clone());
        }
        self.svg_layers = reordered;
        self.revision += 1;
        Ok(())
    }
    pub fn select_layer(&mut self, id: String) -> Result<(), String> {
        if id != "layer-1" && !self.svg_layers.iter().any(|layer| layer.id == id) {
            return Err("Layer not found".into());
        }
        self.finish();
        self.selected_layer = Some(id);
        self.selected_vector_objects.clear();
        Ok(())
    }

    pub fn selected_vector_target(&self) -> Result<Option<String>, String> {
        let Some(id) = &self.selected_layer else {
            return Ok(None);
        };
        let layer = self.svg_layers.iter().find(|layer| &layer.id == id);
        match layer {
            Some(layer) if layer.vector_layer && !layer.locked && layer.visible => Ok(Some(id.clone())),
            _ => Err("選択レイヤーにはテキスト・パスを書き込めません。表示中のロックされていないベクターレイヤーを選択してください。\nSelect an unlocked, visible vector layer for text and paths.\n请为文字和路径选择可见且未锁定的矢量图层。".into()),
        }
    }

    pub fn import_svg(&mut self, name: String, source: String) -> Result<(), String> {
        if let Some(id) = self.selected_layer.clone() {
            let index = self.svg_layers.iter().position(|layer| layer.id == id)
                .ok_or("選択レイヤーには画像を追加できません。画像レイヤーを選択してください。\nSelect an image layer to add an image.\n请选择图像图层以添加图像。")?;
            let layer = &self.svg_layers[index];
            if layer.vector_layer || layer.locked || !layer.visible {
                return Err("選択レイヤーには画像を追加できません。表示中のロックされていない画像レイヤーを選択してください。\nSelect an unlocked, visible image layer.\n请选择可见且未锁定的图像图层。".into());
            }
            let mut updated = layer.clone();
            let incoming = source
                .find("<svg")
                .map(|start| &source[start..])
                .ok_or("Invalid SVG image")?;
            let existing = layer
                .source
                .find("<svg")
                .map(|start| &layer.source[start..])
                .ok_or("Invalid layer image")?;
            updated.source = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">{}{}</svg>",
                self.width, self.height, existing, incoming
            );
            validate_svg_layer(&updated)?;
            if updated.source.len()
                + self
                    .svg_layers
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .map(|(_, layer)| layer.source.len())
                    .sum::<usize>()
                > MAX_SVG_TOTAL_BYTES
            {
                return Err("Project contains too much image data".into());
            }
            self.finish();
            let before = self.vector_history_state();
            self.svg_layers[index] = updated;
            self.record_vector_edit(before);
            self.revision += 1;
            return Ok(());
        }
        self.finish();
        let id = format!("svg-layer-{}", self.svg_layers.len() + 1);
        let layer = SvgLayer {
            id,
            name,
            visible: true,
            opacity: 1.0,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
            source,
            paint_layer: false,
            vector_layer: false,
            vector_objects: vec![],
        };
        validate_svg_layer(&layer)?;
        if self.svg_layers.len() >= MAX_SVG_LAYERS
            || self
                .svg_layers
                .iter()
                .map(|item| item.source.len())
                .sum::<usize>()
                + layer.source.len()
                > MAX_SVG_TOTAL_BYTES
        {
            return Err("A project can contain up to 16 SVG layers and 6 MiB of SVG data".into());
        }
        self.svg_layers.push(layer);
        self.revision += 1;
        Ok(())
    }
    pub fn add_vector_layer(&mut self) -> Result<String, String> {
        self.finish();
        if self.svg_layers.len() >= MAX_SVG_LAYERS {
            return Err("A project can contain up to 17 layers".into());
        }
        let mut serial = self
            .svg_layers
            .iter()
            .filter(|layer| layer.vector_layer)
            .count()
            + 1;
        while self
            .svg_layers
            .iter()
            .any(|layer| layer.id == format!("vector-layer-{serial}"))
        {
            serial += 1;
        }
        let id = format!("vector-layer-{serial}");
        let before = self.vector_history_state();
        self.svg_layers.push(SvgLayer {
            id: id.clone(),
            name: format!("Vector Layer {serial}"),
            visible: true,
            opacity: 1.0,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
            source: empty_vector_svg(self.width, self.height),
            paint_layer: false,
            vector_layer: true,
            vector_objects: vec![],
        });
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(id)
    }
    pub fn text_edit_preview(&self, id: Option<&str>) -> Result<Self, String> {
        let mut preview = self.clone();
        preview.selected_vector_objects.clear();
        if let Some(id) = id {
            let layer = preview
                .svg_layers
                .iter_mut()
                .find(|layer| {
                    layer
                        .vector_objects
                        .iter()
                        .any(|object| object.id == id && object.text.is_some())
                })
                .ok_or("Text object not found")?;
            let object = layer
                .vector_objects
                .iter_mut()
                .find(|object| object.id == id)
                .unwrap();
            if layer.locked || !layer.visible || !object.visible {
                return Err("Text layer is locked or hidden".into());
            }
            object.visible = false;
            layer.source = vector_svg(preview.width, preview.height, &layer.vector_objects);
        }
        Ok(preview)
    }

    pub fn text_at(&self, point: [f32; 2]) -> Option<String> {
        self.svg_layers
            .iter()
            .rev()
            .filter(|layer| layer.visible)
            .flat_map(|layer| layer.vector_objects.iter().rev())
            .find(|object| object.text.is_some() && object.hit_test(point, 0.0))
            .map(|object| object.id.clone())
    }

    pub fn set_text_object(&mut self, settings: TextSettings) -> Result<(), String> {
        settings.text.validate()?;
        if settings
            .position
            .iter()
            .any(|v| !v.is_finite() || v.abs() >= 100_000.0)
        {
            return Err("Invalid text position".into());
        }
        let [r, g, b] = settings.color;
        if let Some(id) = settings.id {
            let (layer_id, mut object) = self
                .svg_layers
                .iter()
                .filter(|layer| layer.vector_layer)
                .find_map(|layer| {
                    layer
                        .vector_objects
                        .iter()
                        .find(|object| object.id == id && object.text.is_some())
                        .map(|object| (layer.id.clone(), object.clone()))
                })
                .ok_or("Text object not found")?;
            if object.text.as_ref() == Some(&settings.text)
                && object.transform[4..] == settings.position
                && object.fill
                    == Some(crate::vector::VectorPaint {
                        color: [r, g, b, 255],
                    })
            {
                return Ok(());
            }
            object.control_points = settings.text.control_points();
            object.text = Some(settings.text);
            object.transform[4] = settings.position[0];
            object.transform[5] = settings.position[1];
            object.fill = Some(crate::vector::VectorPaint {
                color: [r, g, b, 255],
            });
            return self.upsert_vector_object(&layer_id, object);
        }
        if self.svg_layers.len() >= MAX_SVG_LAYERS {
            return Err("A project can contain up to 17 layers".into());
        }
        let mut serial = 1;
        while self.svg_layers.iter().any(|layer| {
            layer.id == format!("text-layer-{serial}")
                || layer
                    .vector_objects
                    .iter()
                    .any(|object| object.id == format!("text-{serial}"))
        }) {
            serial += 1;
        }
        let id = format!("text-{serial}");
        let object = VectorObject {
            id: id.clone(),
            name: format!("Text {serial}"),
            path: crate::vector::VectorPath {
                data: "M0 0".into(),
                fill_rule: crate::vector::FillRule::NonZero,
            },
            transform: [
                1.0,
                0.0,
                0.0,
                1.0,
                settings.position[0],
                settings.position[1],
            ],
            fill: Some(crate::vector::VectorPaint {
                color: [r, g, b, 255],
            }),
            stroke: None,
            stroke_width: 0.0,
            visible: true,
            kind: VectorObjectKind::Text,
            control_points: settings.text.control_points(),
            text: Some(settings.text),
        };
        object.validate()?;
        if let Some(target) = self.selected_vector_target()? {
            self.upsert_vector_object(&target, object)?;
            self.selected_vector_objects = vec![id];
            return Ok(());
        }
        let source = vector_svg(self.width, self.height, std::slice::from_ref(&object));
        if source.len()
            + self
                .svg_layers
                .iter()
                .map(|layer| layer.source.len())
                .sum::<usize>()
            > MAX_SVG_TOTAL_BYTES
        {
            return Err("Project contains too much vector data".into());
        }
        let layer = SvgLayer {
            id: format!("text-layer-{serial}"),
            name: object.name.clone(),
            visible: true,
            opacity: 1.0,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
            source,
            paint_layer: false,
            vector_layer: true,
            vector_objects: vec![object],
        };
        validate_svg_layer(&layer)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers.push(layer);
        self.selected_vector_objects = vec![id];
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn upsert_vector_object(
        &mut self,
        layer_id: &str,
        object: VectorObject,
    ) -> Result<(), String> {
        self.finish();
        object.validate()?;
        let before = self.vector_history_state();
        let layer_index = self
            .svg_layers
            .iter()
            .position(|layer| layer.id == layer_id && layer.vector_layer)
            .ok_or("Vector layer not found")?;
        let layer = &self.svg_layers[layer_index];
        if layer.locked {
            return Err("Vector layer is locked".into());
        }
        let mut objects = layer.vector_objects.clone();
        if let Some(existing) = objects.iter_mut().find(|item| item.id == object.id) {
            *existing = object;
        } else {
            if objects.len() >= 4096 {
                return Err("A vector layer can contain up to 4096 objects".into());
            }
            objects.push(object);
        }
        let source = vector_svg(self.width, self.height, &objects);
        let total_source_len = source.len()
            + self
                .svg_layers
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != layer_index)
                .map(|(_, item)| item.source.len())
                .sum::<usize>();
        if total_source_len > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        let mut updated_layer = self.svg_layers[layer_index].clone();
        updated_layer.vector_objects = objects;
        updated_layer.source = source;
        validate_svg_layer(&updated_layer)?;
        self.svg_layers[layer_index] = updated_layer;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    pub fn select_vector_objects(&mut self, ids: Vec<String>) -> Result<(), String> {
        if ids.len() > 4096 {
            return Err("Too many selected vector objects".into());
        }
        let mut unique = Vec::with_capacity(ids.len());
        for id in ids {
            let exists = self.svg_layers.iter().any(|layer| {
                layer.vector_layer && layer.vector_objects.iter().any(|object| object.id == id)
            });
            if !exists {
                return Err("Vector object not found".into());
            }
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        self.selected_vector_objects = unique;
        Ok(())
    }

    pub fn set_vector_object_visibility(
        &mut self,
        layer_id: &str,
        object_id: &str,
        visible: bool,
    ) -> Result<(), String> {
        self.finish();
        let before = self.vector_history_state();
        let layer_index = self
            .svg_layers
            .iter()
            .position(|layer| layer.id == layer_id && layer.vector_layer)
            .ok_or("Vector layer not found")?;
        if self.svg_layers[layer_index].locked {
            return Err("Vector layer is locked".into());
        }
        let mut updated = self.svg_layers[layer_index].clone();
        let object = updated
            .vector_objects
            .iter_mut()
            .find(|object| object.id == object_id)
            .ok_or("Vector object not found")?;
        if object.visible == visible {
            return Ok(());
        }
        object.visible = visible;
        updated.source = vector_svg(self.width, self.height, &updated.vector_objects);
        validate_svg_layer(&updated)?;
        self.svg_layers[layer_index] = updated;
        if !visible {
            self.selected_vector_objects.retain(|id| id != object_id);
        }
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn reorder_vector_objects(
        &mut self,
        layer_id: &str,
        ids_top_to_bottom: &[String],
    ) -> Result<(), String> {
        self.finish();
        let layer_index = self
            .svg_layers
            .iter()
            .position(|layer| layer.id == layer_id && layer.vector_layer)
            .ok_or("Vector layer not found")?;
        let layer = &self.svg_layers[layer_index];
        if layer.locked {
            return Err("Vector layer is locked".into());
        }
        if ids_top_to_bottom.len() != layer.vector_objects.len()
            || layer
                .vector_objects
                .iter()
                .any(|object| !ids_top_to_bottom.contains(&object.id))
        {
            return Err("Vector object order does not match the layer".into());
        }
        if layer
            .vector_objects
            .iter()
            .rev()
            .map(|object| &object.id)
            .eq(ids_top_to_bottom.iter())
        {
            return Ok(());
        }
        let before = self.vector_history_state();
        let mut updated = self.svg_layers[layer_index].clone();
        updated.vector_objects = ids_top_to_bottom
            .iter()
            .rev()
            .map(|id| {
                layer
                    .vector_objects
                    .iter()
                    .find(|object| &object.id == id)
                    .cloned()
                    .ok_or_else(|| "Vector object not found".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        updated.source = vector_svg(self.width, self.height, &updated.vector_objects);
        validate_svg_layer(&updated)?;
        self.svg_layers[layer_index] = updated;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    /// Apply one boolean operation to two filled shapes on the same editable layer.
    /// Their layer order determines the operands: difference is back minus front.
    pub fn combine_selected_vectors(
        &mut self,
        operation: PathOperation,
        combine: impl FnOnce(
            &VectorObject,
            &VectorObject,
            PathOperation,
        ) -> Result<(VectorPath, Vec<[f32; 2]>), String>,
    ) -> Result<(), String> {
        if self.selected_vector_objects.len() != 2 {
            return Err("Select exactly two vector shapes".into());
        }
        let selected = &self.selected_vector_objects;
        let matches: Vec<_> =
            self.svg_layers
                .iter()
                .enumerate()
                .flat_map(|(layer_index, layer)| {
                    layer.vector_objects.iter().enumerate().filter_map(
                        move |(object_index, object)| {
                            selected
                                .contains(&object.id)
                                .then_some((layer_index, object_index))
                        },
                    )
                })
                .collect();
        if matches.len() != 2 || matches[0].0 != matches[1].0 {
            return Err("Select two shapes on the same vector layer".into());
        }
        let layer_index = matches[0].0;
        let layer = &self.svg_layers[layer_index];
        let back = &layer.vector_objects[matches[0].1];
        let front = &layer.vector_objects[matches[1].1];
        if layer.locked || !layer.visible || !back.visible || !front.visible {
            return Err("Selected shapes must be visible and unlocked".into());
        }
        if back.kind == VectorObjectKind::Text
            || front.kind == VectorObjectKind::Text
            || back.fill.is_none()
            || front.fill != back.fill
            || back.stroke.is_some()
            || front.stroke.is_some()
        {
            return Err("Select two filled shapes with the same color and no stroke".into());
        }
        let (path, points) = combine(back, front, operation)?;
        if path.data.len() > 1024 * 1024
            || points.len() > 65_536
            || points
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
        {
            return Err("Invalid vector operation result".into());
        }
        let mut updated = layer.clone();
        let back_id = back.id.clone();
        updated.vector_objects.remove(matches[1].1);
        if path.data.is_empty() {
            updated.vector_objects.remove(matches[0].1);
        } else {
            let object = &mut updated.vector_objects[matches[0].1];
            object.path = path;
            object.transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
            object.kind = VectorObjectKind::Compound;
            object.control_points = points;
            object.validate()?;
        }
        updated.source = vector_svg(self.width, self.height, &updated.vector_objects);
        let total = updated.source.len()
            + self
                .svg_layers
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != layer_index)
                .map(|(_, layer)| layer.source.len())
                .sum::<usize>();
        if total > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        validate_svg_layer(&updated)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers[layer_index] = updated;
        self.selected_vector_objects = if self.svg_layers[layer_index]
            .vector_objects
            .iter()
            .any(|object| object.id == back_id)
        {
            vec![back_id]
        } else {
            vec![]
        };
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    pub fn select_vector_at(
        &mut self,
        point: Point,
        tolerance: f32,
        additive: bool,
    ) -> Option<String> {
        if !point.valid() || !tolerance.is_finite() || !(0.0..=256.0).contains(&tolerance) {
            return None;
        }
        let hit = self
            .svg_layers
            .iter()
            .rev()
            .filter(|layer| layer.vector_layer && layer.visible && !layer.locked)
            .flat_map(|layer| layer.vector_objects.iter().rev())
            .find(|object| object.hit_test([point.x, point.y], tolerance))
            .map(|object| object.id.clone());
        if let Some(id) = hit.as_ref() {
            if !additive {
                self.selected_vector_objects.clear();
            }
            if !self.selected_vector_objects.contains(id) {
                self.selected_vector_objects.push(id.clone());
            }
        } else if !additive {
            self.selected_vector_objects.clear();
        }
        hit
    }
    pub fn move_selected_vectors(&mut self, dx: f32, dy: f32) -> Result<bool, String> {
        if !dx.is_finite() || !dy.is_finite() || dx.abs() >= 100_000.0 || dy.abs() >= 100_000.0 {
            return Err("Invalid vector translation".into());
        }
        if dx.abs() < f32::EPSILON && dy.abs() < f32::EPSILON {
            return Ok(false);
        }
        let before = self.vector_history_state();
        let selected = self.selected_vector_objects.clone();
        let mut changed = false;
        for layer in self
            .svg_layers
            .iter_mut()
            .filter(|layer| layer.vector_layer && !layer.locked)
        {
            let mut layer_changed = false;
            for object in &mut layer.vector_objects {
                if selected.contains(&object.id) {
                    object.transform[4] += dx;
                    object.transform[5] += dy;
                    object.validate()?;
                    layer_changed = true;
                    changed = true;
                }
            }
            if layer_changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if changed {
            self.record_vector_edit(before);
            self.revision += 1;
        }
        Ok(changed)
    }
    /// Temporary layer content for a drag. The document and its undo history stay untouched.
    pub fn translated_vector_layer(
        &self,
        layer: &SvgLayer,
        dx: f32,
        dy: f32,
    ) -> Result<Option<SvgLayer>, String> {
        if !dx.is_finite() || !dy.is_finite() || dx.abs() >= 100_000.0 || dy.abs() >= 100_000.0 {
            return Err("Invalid vector translation".into());
        }
        if !layer.vector_layer
            || layer.locked
            || !layer.visible
            || !layer
                .vector_objects
                .iter()
                .any(|object| object.visible && self.selected_vector_objects.contains(&object.id))
        {
            return Ok(None);
        }
        let mut preview = layer.clone();
        for object in &mut preview.vector_objects {
            if object.visible && self.selected_vector_objects.contains(&object.id) {
                object.transform[4] += dx;
                object.transform[5] += dy;
                object.validate()?;
            }
        }
        preview.source = vector_svg(self.width, self.height, &preview.vector_objects);
        validate_svg_layer(&preview)?;
        Ok(Some(preview))
    }

    /// Whole selected layers can reuse their existing texture during a translation.
    pub fn vector_layer_moves_as_unit(&self, layer: &SvgLayer) -> bool {
        layer.vector_layer
            && layer.visible
            && !layer.locked
            && layer.vector_objects.iter().any(|object| object.visible)
            && layer
                .vector_objects
                .iter()
                .filter(|object| object.visible)
                .all(|object| self.selected_vector_objects.contains(&object.id))
    }

    pub fn selected_vector_ids(&self) -> &[String] {
        &self.selected_vector_objects
    }

    /// Consecutive visible objects stay in painter order when a partial drag is
    /// composed from stationary and translated textures. Group opacity requires
    /// compositing the whole layer first, so those layers use the regular path.
    pub fn vector_drag_runs(&self, layer: &SvgLayer) -> Option<Vec<(bool, SvgLayer)>> {
        if !layer.vector_layer || !layer.visible || layer.locked || layer.effective_opacity() != 1.0
        {
            return None;
        }
        let mut groups: Vec<(bool, Vec<VectorObject>)> = Vec::new();
        for object in layer.vector_objects.iter().filter(|object| object.visible) {
            let selected = self.selected_vector_objects.contains(&object.id);
            if let Some((last_selected, objects)) = groups.last_mut() {
                if *last_selected == selected {
                    objects.push(object.clone());
                    continue;
                }
            }
            groups.push((selected, vec![object.clone()]));
        }
        if !groups.iter().any(|(selected, _)| *selected)
            || !groups.iter().any(|(selected, _)| !selected)
        {
            return None;
        }
        Some(
            groups
                .into_iter()
                .map(|(selected, objects)| {
                    let mut run = layer.clone();
                    run.source = vector_svg(self.width, self.height, &objects);
                    run.vector_objects = objects;
                    (selected, run)
                })
                .collect(),
        )
    }

    pub fn vector_overlay_selections(&self) -> Vec<Selection> {
        self.vector_overlay_selections_at_offset(0.0, 0.0)
    }
    pub fn vector_overlay_selections_at_offset(&self, dx: f32, dy: f32) -> Vec<Selection> {
        let selected = &self.selected_vector_objects;
        let mut overlays = Vec::new();
        for (object, movable) in self
            .svg_layers
            .iter()
            .filter(|layer| layer.vector_layer && layer.visible)
            .flat_map(|layer| {
                layer
                    .vector_objects
                    .iter()
                    .map(move |object| (object, !layer.locked))
            })
            .filter(|(object, _)| {
                selected.contains(&object.id) && !object.control_points.is_empty()
            })
        {
            let mut points = transformed_control_points(object);
            if movable {
                for point in &mut points {
                    point.x += dx;
                    point.y += dy;
                }
            }
            let min_x = points
                .iter()
                .map(|point| point.x)
                .fold(f32::INFINITY, f32::min);
            let max_x = points
                .iter()
                .map(|point| point.x)
                .fold(f32::NEG_INFINITY, f32::max);
            let min_y = points
                .iter()
                .map(|point| point.y)
                .fold(f32::INFINITY, f32::min);
            let max_y = points
                .iter()
                .map(|point| point.y)
                .fold(f32::NEG_INFINITY, f32::max);
            overlays.push(Selection::new(
                SelectionShape::Rectangle,
                [
                    min_x,
                    min_y,
                    (max_x - min_x).max(1.0),
                    (max_y - min_y).max(1.0),
                ],
            ));
            if object.kind == VectorObjectKind::Text || object.kind == VectorObjectKind::Compound {
                continue;
            }
            for point in points {
                if overlays.len() >= 128 {
                    break;
                }
                overlays.push(Selection::new(
                    SelectionShape::Ellipse,
                    [point.x - 3.0, point.y - 3.0, 6.0, 6.0],
                ));
            }
            if overlays.len() >= 128 {
                break;
            }
        }
        overlays
    }
    pub fn selected_control_at(&self, point: Point, tolerance: f32) -> Option<(String, usize)> {
        self.svg_layers
            .iter()
            .rev()
            .filter(|layer| layer.vector_layer && layer.visible && !layer.locked)
            .flat_map(|layer| layer.vector_objects.iter().rev())
            .filter(|object| {
                self.selected_vector_objects.contains(&object.id)
                    && object.kind != VectorObjectKind::Text
                    && object.kind != VectorObjectKind::Compound
            })
            .find_map(|object| {
                transformed_control_points(object)
                    .iter()
                    .enumerate()
                    .find(|(_, control)| {
                        (point.x - control.x).hypot(point.y - control.y) <= tolerance
                    })
                    .map(|(index, _)| (object.id.clone(), index))
            })
    }
    pub fn move_vector_control(
        &mut self,
        id: &str,
        index: usize,
        point: Point,
    ) -> Result<(), String> {
        if !point.valid() {
            return Err("Invalid control point".into());
        }
        let before = self.vector_history_state();
        let layer_index = self
            .svg_layers
            .iter()
            .position(|layer| {
                layer.vector_layer
                    && !layer.locked
                    && layer.vector_objects.iter().any(|object| object.id == id)
            })
            .ok_or("Vector object not found")?;
        let object_index = self.svg_layers[layer_index]
            .vector_objects
            .iter()
            .position(|object| object.id == id)
            .ok_or("Vector object not found")?;
        let object = &mut self.svg_layers[layer_index].vector_objects[object_index];
        if object.kind == VectorObjectKind::Compound {
            return Err("Compound path control points are not directly editable".into());
        }
        let [a, b, c, d, e, f] = object.transform;
        let determinant = a * d - b * c;
        if determinant.abs() < 0.000_001 {
            return Err("Vector transform is not invertible".into());
        }
        let px = point.x - e;
        let py = point.y - f;
        let local = [
            (d * px - c * py) / determinant,
            (-b * px + a * py) / determinant,
        ];
        let control = object
            .control_points
            .get_mut(index)
            .ok_or("Control point not found")?;
        *control = local;
        rebuild_vector_path(object)?;
        object.validate()?;
        let layer = &mut self.svg_layers[layer_index];
        layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
        validate_svg_layer(layer)?;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    fn vector_history_state(&self) -> VectorHistoryState {
        VectorHistoryState {
            layers: self.svg_layers.clone(),
            selection: self.selected_vector_objects.clone(),
        }
    }
    fn restore_vector_history(&mut self, state: VectorHistoryState) {
        self.svg_layers = state.layers;
        self.selected_vector_objects = state.selection;
    }
    fn record_vector_edit(&mut self, previous: VectorHistoryState) {
        self.vector_undo.push(previous);
        self.vector_redo.clear();
        self.redo.clear();
        self.redo_order.clear();
        self.undo_order.push(HistoryKind::Vector);
    }
    pub fn visible_svg_layers(&self) -> impl Iterator<Item = &SvgLayer> {
        self.svg_layers.iter().filter(|layer| layer.visible)
    }
    pub fn svg_layers(&self) -> impl Iterator<Item = &SvgLayer> {
        self.svg_layers.iter()
    }
    pub fn editable_vector_layer_id(&self) -> Option<String> {
        self.svg_layers
            .iter()
            .rev()
            .find(|layer| layer.vector_layer && !layer.locked && layer.visible)
            .map(|layer| layer.id.clone())
    }
    /// The canvas fill belongs to the fixed background layer.
    pub fn background_visible(&self) -> bool {
        self.visible
    }

    pub fn paint_layer_opacity(&self) -> f32 {
        self.layer_opacity
            * mask_factor(
                self.layer_mask_enabled,
                self.layer_mask_inverted,
                self.layer_mask_density,
            )
    }
    pub fn visible_strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.strokes
            .iter()
            .chain(self.active.iter())
            .filter(|_| self.visible)
    }
    /// Retained paint strokes, including those on a temporarily hidden layer.
    /// The active, uncommitted stroke is intentionally excluded.
    pub fn committed_paint_strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.strokes.iter()
    }

    /// The stroke most recently removed by Undo, when it is the next Redo action.
    pub fn last_undone_paint_stroke(&self) -> Option<&Stroke> {
        matches!(self.redo_order.last(), Some(HistoryKind::Stroke))
            .then(|| self.redo.last())
            .flatten()
    }
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }
    /// Start a fresh shape. Interactive editing should use begin_selection_edit.
    pub fn begin_selection(&mut self, point: Point, shape: SelectionShape) {
        self.finish();
        self.selection_anchor = None;
        if !point.valid() || !point.on_page(self.width, self.height) {
            return;
        }
        self.selection_anchor = Some(SelectionGesture {
            start: point,
            shape,
            mode: SelectionMode::Replace,
            moving: false,
            base: self.selection.clone(),
        });
    }
    pub fn begin_selection_edit(
        &mut self,
        point: Point,
        shape: SelectionShape,
        mode: SelectionMode,
    ) -> Result<(), String> {
        self.finish();
        self.selection_anchor = None;
        if !point.valid() || !point.on_page(self.width, self.height) {
            return Ok(());
        }
        let moving = mode == SelectionMode::Replace
            && self.selection.as_ref().is_some_and(|s| s.contains(point));
        if mode != SelectionMode::Replace {
            if let Some(selection) = &self.selection {
                selection.ensure_capacity()?;
            }
        }
        self.selection_anchor = Some(SelectionGesture {
            start: point,
            shape,
            mode,
            moving,
            base: self.selection.clone(),
        });
        Ok(())
    }
    pub fn extend_selection(&mut self, point: Point, finish: bool) {
        if !point.valid() {
            return;
        }
        if let Some(gesture) = &self.selection_anchor {
            let start = gesture.start;
            if gesture.moving {
                self.selection = gesture
                    .base
                    .as_ref()
                    .map(|selection| selection.translated(point.x - start.x, point.y - start.y));
            } else {
                let x = point.x.clamp(0.0, self.width as f32);
                let y = point.y.clamp(0.0, self.height as f32);
                let bounds = [
                    start.x.min(x),
                    start.y.min(y),
                    (start.x - x).abs(),
                    (start.y - y).abs(),
                ];
                if bounds[2] >= 1.0 && bounds[3] >= 1.0 {
                    let region = SelectionRegion {
                        shape: gesture.shape,
                        bounds,
                        operation: if gesture.mode == SelectionMode::Add {
                            SelectionOperation::Add
                        } else {
                            SelectionOperation::Subtract
                        },
                    };
                    self.selection = Some(match gesture.mode {
                        SelectionMode::Replace => Selection::new(gesture.shape, bounds),
                        SelectionMode::Add => match &gesture.base {
                            Some(base) => {
                                let mut result = base.clone();
                                result.regions.push(region);
                                result
                            }
                            None => Selection::new(gesture.shape, bounds),
                        },
                        SelectionMode::Subtract => {
                            let mut result = gesture.base.clone().unwrap_or_else(|| {
                                Selection::new(
                                    SelectionShape::Rectangle,
                                    [0.0, 0.0, self.width as f32, self.height as f32],
                                )
                            });
                            result.regions.push(region);
                            result
                        }
                    });
                } else {
                    self.selection = if gesture.mode == SelectionMode::Replace {
                        None
                    } else {
                        gesture.base.clone()
                    };
                }
            }
        }
        if finish {
            self.selection_anchor = None;
        }
    }
    pub fn cancel_selection_gesture(&mut self) -> bool {
        if let Some(gesture) = self.selection_anchor.take() {
            self.selection = gesture.base;
            true
        } else {
            false
        }
    }
    pub fn deselect(&mut self) {
        self.finish();
        self.selection = None;
        self.selection_anchor = None;
    }
    pub fn select_all(&mut self) {
        self.deselect();
        self.selection = Some(Selection::new(
            SelectionShape::Rectangle,
            [0.0, 0.0, self.width as f32, self.height as f32],
        ));
    }
    pub fn invert_selection(&mut self) -> Result<(), String> {
        self.finish();
        self.selection_anchor = None;
        if let Some(selection) = self.selection.as_mut() {
            selection.invert(self.width, self.height)?;
        }
        Ok(())
    }
    fn point_has_paint_alpha(&self, point: Point) -> bool {
        self.strokes.iter().any(|stroke| {
            if stroke
                .selection
                .as_ref()
                .is_some_and(|selection| !selection.contains(point))
            {
                return false;
            }
            let pressure = |index: usize| {
                stroke
                    .pressures
                    .get(index)
                    .copied()
                    .unwrap_or(1.0)
                    .max(0.05)
            };
            if stroke.points.len() == 1 {
                let radius = stroke.brush.size * pressure(0) * 0.5;
                return (point.x - stroke.points[0].x).hypot(point.y - stroke.points[0].y)
                    <= radius;
            }
            stroke.points.windows(2).enumerate().any(|(index, pair)| {
                let a = pair[0];
                let b = pair[1];
                let delta = Point {
                    x: b.x - a.x,
                    y: b.y - a.y,
                };
                let length_squared = delta.x * delta.x + delta.y * delta.y;
                let t = if length_squared > 0.0 {
                    (((point.x - a.x) * delta.x + (point.y - a.y) * delta.y) / length_squared)
                        .clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let nearest = Point {
                    x: a.x + delta.x * t,
                    y: a.y + delta.y * t,
                };
                let radius = stroke.brush.size
                    * (pressure(index) + (pressure(index + 1) - pressure(index)) * t)
                    * 0.5;
                (point.x - nearest.x).hypot(point.y - nearest.y) <= radius
            })
        })
    }
}

pub(crate) fn mask_factor(enabled: bool, inverted: bool, density: f32) -> f32 {
    if !enabled {
        1.0
    } else if inverted {
        1.0 - density
    } else {
        density
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_layer_routes_text_and_rejects_incompatible_image_without_mutation() {
        let mut document = Document::default();
        let first = document.add_vector_layer().unwrap();
        let second = document.add_vector_layer().unwrap();
        document.select_layer(first.clone()).unwrap();
        document
            .set_text_object(TextSettings {
                id: None,
                text: crate::vector::VectorText {
                    content: "Selected".into(),
                    ..Default::default()
                },
                position: [10.0, 20.0],
                color: [0, 0, 0],
            })
            .unwrap();
        assert_eq!(
            document
                .svg_layers()
                .find(|layer| layer.id == first)
                .unwrap()
                .vector_objects
                .len(),
            1
        );
        assert!(document
            .svg_layers()
            .find(|layer| layer.id == second)
            .unwrap()
            .vector_objects
            .is_empty());
        let snapshot = document.snapshot();
        let children = &snapshot
            .layers
            .iter()
            .find(|layer| layer.id == first)
            .unwrap()
            .objects;
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind, VectorObjectKind::Text);
        assert_eq!(children[0].id, snapshot.text_objects[0].id);
        assert!(snapshot
            .layers
            .iter()
            .find(|layer| layer.id == second)
            .unwrap()
            .objects
            .is_empty());
        document
            .select_vector_objects(vec![children[0].id.clone()])
            .unwrap();
        assert_eq!(
            document.snapshot().selected_vector_objects,
            vec![children[0].id.clone()]
        );
        document
            .set_vector_object_visibility(&first, &children[0].id, false)
            .unwrap();
        let hidden = document.snapshot();
        assert!(
            !hidden
                .layers
                .iter()
                .find(|layer| layer.id == first)
                .unwrap()
                .objects[0]
                .visible
        );
        assert!(hidden.selected_vector_objects.is_empty());
        document.undo();
        assert!(
            document
                .snapshot()
                .layers
                .iter()
                .find(|layer| layer.id == first)
                .unwrap()
                .objects[0]
                .visible
        );
        let revision = document.revision();
        assert!(document
            .import_svg(
                "Image".into(),
                "<svg xmlns=\"http://www.w3.org/2000/svg\"/>".into()
            )
            .is_err());
        assert_eq!(document.revision(), revision);
        document.select_layer("layer-1".into()).unwrap();
        assert!(document.selected_vector_target().is_err());
    }

    #[test]
    fn selected_image_layer_retains_existing_image_and_undo() {
        let mut document = Document::default();
        let id = document.add_paint_layer().unwrap();
        document.select_layer(id.clone()).unwrap();
        let before = document
            .svg_layers()
            .find(|layer| layer.id == id)
            .unwrap()
            .source
            .clone();
        document.import_svg("Image".into(), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\"/></svg>".into()).unwrap();
        assert_eq!(document.svg_layers().count(), 1);
        assert!(document
            .svg_layers()
            .next()
            .unwrap()
            .source
            .contains("<rect"));
        document.undo();
        assert_eq!(document.svg_layers().next().unwrap().source, before);
    }

    #[test]
    fn selection_drag_normalizes_clamps_and_does_not_dirty_document() {
        let mut doc = Document::default();
        doc.begin_selection(Point { x: 200.0, y: 300.0 }, SelectionShape::Rectangle);
        doc.extend_selection(Point { x: -20.0, y: 20.0 }, true);
        assert_eq!(
            doc.selection().unwrap().regions[0].bounds,
            [0.0, 20.0, 200.0, 280.0]
        );
        assert!(!doc.snapshot().dirty);
        assert_eq!(doc.snapshot().stroke_count, 0);
        doc.begin_selection(Point { x: 200.0, y: 300.0 }, SelectionShape::Ellipse);
        doc.extend_selection(
            Point {
                x: 9999.0,
                y: 9999.0,
            },
            true,
        );
        assert_eq!(
            doc.selection().unwrap().regions[0].bounds,
            [200.0, 300.0, WIDTH - 200.0, HEIGHT - 300.0]
        );
        doc.begin_selection(Point { x: 20.0, y: 20.0 }, SelectionShape::Rectangle);
        doc.extend_selection(Point { x: 20.0, y: 20.0 }, true);
        assert!(doc.selection().is_none());
    }

    #[test]
    fn selection_is_per_document_and_stroke_clip_survives_deselect_undo_and_reload() {
        let mut doc = Document::default();
        doc.begin_selection(Point { x: 20.0, y: 20.0 }, SelectionShape::Ellipse);
        doc.extend_selection(Point { x: 120.0, y: 100.0 }, true);
        let selection = doc.selection().cloned();
        assert!(Document::default().selection().is_none());
        doc.begin(Point { x: 10.0, y: 50.0 }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 150.0, y: 50.0 }).unwrap();
        doc.finish();
        doc.deselect();
        doc.undo();
        doc.redo();
        assert_eq!(doc.visible_strokes().next().unwrap().selection, selection);
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert!(loaded.selection().is_none());
        assert_eq!(
            loaded.visible_strokes().next().unwrap().selection,
            selection
        );
        doc.begin(Point { x: 10.0, y: 50.0 }, Brush::default())
            .unwrap();
        doc.finish();
        assert!(doc.visible_strokes().last().unwrap().selection.is_none());
    }

    #[test]
    fn inverted_ellipse_selects_outside_and_invalid_saved_bounds_are_rejected() {
        let mut doc = Document::default();
        doc.begin_selection(Point { x: 20.0, y: 20.0 }, SelectionShape::Ellipse);
        doc.extend_selection(Point { x: 120.0, y: 100.0 }, true);
        assert!(doc
            .selection()
            .unwrap()
            .contains(Point { x: 70.0, y: 60.0 }));
        assert!(!doc
            .selection()
            .unwrap()
            .contains(Point { x: 21.0, y: 21.0 }));
        doc.invert_selection().unwrap();
        assert!(!doc
            .selection()
            .unwrap()
            .contains(Point { x: 70.0, y: 60.0 }));
        assert!(doc
            .selection()
            .unwrap()
            .contains(Point { x: 21.0, y: 21.0 }));
        stroke(&mut doc);
        let mut json: serde_json::Value = serde_json::from_slice(&doc.encode().unwrap()).unwrap();
        json["strokes"][0]["selection"]["regions"][0]["bounds"][2] = serde_json::json!(0);
        assert!(Document::decode(&serde_json::to_vec(&json).unwrap()).is_err());
        doc.select_all();
        assert_eq!(
            doc.selection().unwrap().regions[0].bounds,
            [0.0, 0.0, WIDTH, HEIGHT]
        );
    }
    fn stroke(doc: &mut Document) {
        doc.begin(Point { x: 30.0, y: 40.0 }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 100.0, y: 140.0 }).unwrap();
        doc.finish();
    }
    #[test]
    fn undo_redo_restores_stroke_data_without_image_copies() {
        let mut doc = Document::default();
        doc.begin(Point { x: 30.0, y: 40.0 }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 100.0, y: 140.0 }).unwrap();
        doc.finish();
        doc.undo();
        assert_eq!(doc.visible_strokes().count(), 0);
        assert!(doc.snapshot().can_redo);
        doc.redo();
        assert_eq!(doc.visible_strokes().next().unwrap().points[1].x, 100.0);
        assert_eq!(doc.snapshot().stroke_count, 1);
    }
    #[test]
    fn new_stroke_after_undo_invalidates_redo() {
        let mut doc = Document::default();
        stroke(&mut doc);
        doc.undo();
        stroke(&mut doc);
        assert!(!doc.snapshot().can_redo);
    }
    #[test]
    fn tile_append_requires_one_stroke_and_unchanged_paint_settings() {
        let mut doc = Document::default();
        let initial = doc.paint_projection_state();
        stroke(&mut doc);
        assert!(initial.can_append_one(doc.paint_projection_state()));
        let previous = doc.paint_projection_state();
        doc.toggle_visibility();
        assert!(!previous.can_append_one(doc.paint_projection_state()));
        doc.toggle_visibility();
        doc.undo();
        assert!(!previous.can_append_one(doc.paint_projection_state()));
    }

    #[test]
    fn tile_composite_is_unchanged_by_lock_and_vector_edits() {
        let mut doc = Document::default();
        stroke(&mut doc);
        let painted = doc.paint_projection_state();
        doc.set_layer_settings(LayerSettings {
            id: "layer-1".into(),
            name: "Paint".into(),
            opacity: 1.0,
            locked: true,
            alpha_locked: true,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
        })
        .unwrap();
        assert!(painted.same_composite(doc.paint_projection_state()));
        assert!(!painted.same_appearance(doc.paint_projection_state()));
        doc.add_vector_layer().unwrap();
        assert!(painted.same_composite(doc.paint_projection_state()));
        doc.toggle_visibility();
        assert!(!painted.same_composite(doc.paint_projection_state()));
    }
    #[test]
    fn hidden_layer_and_outside_page_reject_new_strokes() {
        let mut doc = Document::default();
        assert!(!doc
            .begin(Point { x: -1.0, y: 4.0 }, Brush::default())
            .unwrap());
        stroke(&mut doc);
        doc.toggle_visibility();
        assert_eq!(doc.visible_strokes().count(), 0);
        assert!(!doc
            .begin(Point { x: 10.0, y: 10.0 }, Brush::default())
            .unwrap());
        doc.toggle_visibility();
        assert_eq!(doc.visible_strokes().count(), 1);
    }
    #[test]
    fn invalid_samples_do_not_corrupt_active_stroke() {
        let mut doc = Document::default();
        doc.begin(Point { x: 10.0, y: 10.0 }, Brush::default())
            .unwrap();
        assert!(doc.has_active_stroke());
        assert!(doc
            .extend(Point {
                x: f32::NAN,
                y: 20.0
            })
            .is_err());
        doc.finish();
        assert!(!doc.has_active_stroke());
        doc.finish();
        assert_eq!(doc.visible_strokes().next().unwrap().points.len(), 1);
    }
    #[test]
    fn pressure_is_validated_and_preserved() {
        let mut doc = Document::default();
        doc.begin_with_pressure(Point { x: 10.0, y: 10.0 }, Brush::default(), 0.25)
            .unwrap();
        doc.extend_with_pressure(Point { x: 40.0, y: 40.0 }, 0.8)
            .unwrap();
        doc.finish();
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.visible_strokes().next().unwrap().pressures,
            [0.25, 0.8]
        );
        assert!(doc
            .begin_with_pressure(Point { x: 10.0, y: 10.0 }, Brush::default(), 1.1)
            .is_err());
    }

    #[test]
    fn document_settings_change_bounds_and_round_trip() {
        let mut doc = Document::default();
        doc.set_document_settings(DocumentSettings {
            name: "Poster".into(),
            width: 434,
            height: 418,
            unit: DocumentUnit::Pixels,
            resolution: 72,
            artboards: true,
            canvas_color: CanvasColor::Transparent,
            pixel_aspect_ratio: 1.0,
        })
        .unwrap();
        assert!(!doc
            .begin(Point { x: 500.0, y: 100.0 }, Brush::default())
            .unwrap());
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        let snapshot = loaded.snapshot();
        assert_eq!(
            (snapshot.name.as_str(), snapshot.width, snapshot.height),
            ("Poster", 434, 418)
        );
        assert!(snapshot.artboards);
        assert_eq!(snapshot.canvas_color, CanvasColor::Transparent);
    }

    #[test]
    fn brush_accepts_sizes_up_to_512_px() {
        assert!(Brush {
            size: 512.0,
            hardness: 1.0,
            color: [0, 0, 0]
        }
        .validate()
        .is_ok());
        assert!(Brush {
            size: 513.0,
            hardness: 1.0,
            color: [0, 0, 0]
        }
        .validate()
        .is_err());
        assert!(Brush {
            size: 16.0,
            hardness: 1.01,
            color: [0, 0, 0]
        }
        .validate()
        .is_err());
    }
}

/// v1 is a bounded, stroke-based exchange format, not the future tiled project container.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectFile {
    format: String,
    version: u32,
    #[serde(default)]
    name: Option<String>,
    width: u32,
    height: u32,
    #[serde(default)]
    unit: Option<DocumentUnit>,
    #[serde(default)]
    resolution: Option<u32>,
    #[serde(default)]
    artboards: Option<bool>,
    #[serde(default)]
    canvas_color: Option<CanvasColor>,
    #[serde(default)]
    pixel_aspect_ratio: Option<f32>,
    layer_visible: bool,
    #[serde(default)]
    layer_name: Option<String>,
    #[serde(default)]
    layer_opacity: Option<f32>,
    #[serde(default)]
    layer_locked: Option<bool>,
    #[serde(default)]
    layer_alpha_locked: Option<bool>,
    #[serde(default)]
    layer_mask_enabled: Option<bool>,
    #[serde(default)]
    layer_mask_inverted: Option<bool>,
    #[serde(default)]
    layer_mask_density: Option<f32>,
    #[serde(default)]
    color_mode: ColorMode,
    #[serde(default)]
    color_profile: Option<ColorProfile>,
    #[serde(default = "default_bit_depth")]
    bit_depth: u8,
    strokes: Vec<Stroke>,
    #[serde(default)]
    svg_layers: Vec<SvgLayer>,
}

fn validate_svg_layer(layer: &SvgLayer) -> Result<(), String> {
    if layer.id.is_empty() || layer.id.len() > 64 || layer.name.is_empty() || layer.name.len() > 255
    {
        return Err("Invalid SVG layer name or identifier".into());
    }
    if !layer.opacity.is_finite()
        || !(0.0..=1.0).contains(&layer.opacity)
        || !layer.mask_density.is_finite()
        || !(0.0..=1.0).contains(&layer.mask_density)
    {
        return Err("Invalid SVG layer opacity".into());
    }
    if layer.source.len() > MAX_SVG_BYTES || !layer.source.contains("<svg") {
        return Err("SVG must contain an <svg> root and be no larger than 4 MiB".into());
    }
    if layer.paint_layer && layer.vector_layer {
        return Err("A layer cannot be both paint and vector".into());
    }
    if !layer.vector_layer && !layer.vector_objects.is_empty() {
        return Err("Only vector layers can contain vector objects".into());
    }
    for object in &layer.vector_objects {
        object.validate()?;
    }
    Ok(())
}

fn empty_vector_svg(width: u32, height: u32) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"/>"#
    )
}

fn vector_svg(width: u32, height: u32, objects: &[VectorObject]) -> String {
    use std::fmt::Write;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    for object in objects.iter().filter(|object| object.visible) {
        let [a, b, c, d, e, f] = object.transform;
        let fill = object
            .fill
            .map_or_else(|| "none".into(), |paint| rgba_hex(paint.color));
        let stroke = object
            .stroke
            .map_or_else(|| "none".into(), |paint| rgba_hex(paint.color));
        if let Some(text) = &object.text {
            let anchor = match text.alignment {
                crate::vector::TextAlignment::Left => "start",
                crate::vector::TextAlignment::Center => "middle",
                crate::vector::TextAlignment::Right => "end",
            };
            let _ = write!(
                svg,
                r#"<g transform="matrix({a} {b} {c} {d} {e} {f}) rotate({}) scale({} {})"><text text-anchor="{anchor}" xml:space="preserve">"#,
                text.rotation, text.scale_x, text.scale_y
            );
            let color = object.fill.map_or([0, 0, 0], |paint| {
                [paint.color[0], paint.color[1], paint.color[2]]
            });
            let mut y = text.font_size + text.space_before;
            for (index, (start, line, hard_break_before)) in
                text.visual_lines().into_iter().enumerate()
            {
                if index > 0 {
                    y += text.font_size * text.line_height;
                    if hard_break_before {
                        y += text.space_before + text.space_after;
                    }
                }
                let baseline = text.line_baselines.get(index).copied().unwrap_or(y);
                let first_indent = if index == 0 || hard_break_before {
                    text.indent_first
                } else {
                    0.0
                };
                let fallback_x = match text.alignment {
                    crate::vector::TextAlignment::Left => text.indent_left + first_indent,
                    crate::vector::TextAlignment::Center => {
                        (text.box_width + text.indent_left - text.indent_right + first_indent) * 0.5
                    }
                    crate::vector::TextAlignment::Right => text.box_width - text.indent_right,
                };
                let measured_origin = text.line_origins.get(index).copied();
                let x = measured_origin.unwrap_or(fallback_x);
                let segment_starts = text.style_segment_starts(start, line, color);
                let segment_origins = text
                    .style_segment_origins
                    .get(index)
                    .filter(|origins| origins.len() == segment_starts.len() && origins.len() > 1);
                let character_origins = text
                    .character_origins
                    .get(index)
                    .filter(|origins| origins.len() == line.chars().count());
                let segment_widths = segment_origins.zip(text.line_widths.get(index)).and_then(
                    |(origins, line_width)| {
                        let mut widths: Vec<f32> =
                            origins.windows(2).map(|pair| pair[1] - pair[0]).collect();
                        widths.push(origins[0] + line_width - origins[origins.len() - 1]);
                        widths
                            .iter()
                            .all(|width| width.is_finite() && (0.0..100_000.0).contains(width))
                            .then_some(widths)
                    },
                );
                let _ = write!(svg, r#"<tspan x="{x}" y="{baseline}""#);
                if measured_origin.is_some() {
                    svg.push_str(r#" text-anchor="start""#);
                }
                if let Some(width) = text.line_widths.get(index).filter(|width| {
                    **width > 0.0 && segment_starts.len() == 1 && character_origins.is_none()
                }) {
                    let _ = write!(svg, r#" textLength="{width}" lengthAdjust="spacing""#);
                }
                svg.push('>');
                if let Some(clusters) = text
                    .glyph_clusters
                    .get(index)
                    .filter(|clusters| !clusters.is_empty())
                {
                    for cluster in clusters {
                        if let Some(content) = utf16_slice(line, cluster.start, cluster.end) {
                            let style = text.style_at(start + cluster.start, color);
                            write_text_segment(
                                &mut svg,
                                &style,
                                content,
                                Some(cluster.x),
                                None,
                                None,
                            );
                        }
                    }
                    svg.push_str("</tspan>");
                    continue;
                }
                let mut segment = String::new();
                let mut previous = None;
                let mut offset = start;
                let mut segment_index = 0;
                let mut character_index = 0;
                let mut segment_character_start = 0;
                for c in line.chars() {
                    let style = text.style_at(offset, color);
                    if previous.as_ref().is_some_and(|old| old != &style) {
                        write_text_segment(
                            &mut svg,
                            previous.as_ref().unwrap(),
                            &segment,
                            segment_origins
                                .and_then(|origins| origins.get(segment_index))
                                .copied(),
                            segment_widths
                                .as_ref()
                                .and_then(|widths| widths.get(segment_index))
                                .copied(),
                            character_origins
                                .map(|origins| &origins[segment_character_start..character_index]),
                        );
                        segment.clear();
                        segment_index += 1;
                        segment_character_start = character_index;
                    }
                    segment.push(c);
                    previous = Some(style);
                    offset += c.len_utf16();
                    character_index += 1;
                }
                if let Some(style) = previous {
                    write_text_segment(
                        &mut svg,
                        &style,
                        &segment,
                        segment_origins
                            .and_then(|origins| origins.get(segment_index))
                            .copied(),
                        segment_widths
                            .as_ref()
                            .and_then(|widths| widths.get(segment_index))
                            .copied(),
                        character_origins
                            .map(|origins| &origins[segment_character_start..character_index]),
                    );
                }
                svg.push_str("</tspan>");
            }
            svg.push_str("</text></g>");
            continue;
        }
        let rule = match object.path.fill_rule {
            crate::vector::FillRule::NonZero => "nonzero",
            crate::vector::FillRule::EvenOdd => "evenodd",
        };
        let _ = write!(
            svg,
            r#"<path d="{}" transform="matrix({a} {b} {c} {d} {e} {f})" fill="{fill}" stroke="{stroke}" stroke-width="{}" fill-rule="{rule}"/>"#,
            escape_xml(&object.path.data),
            object.stroke_width
        );
    }
    svg.push_str("</svg>");
    svg
}

fn utf16_slice(value: &str, start: usize, end: usize) -> Option<&str> {
    if start > end {
        return None;
    }
    let mut utf16 = 0;
    let mut start_byte = (start == 0).then_some(0);
    let mut end_byte = (end == 0).then_some(0);
    for (byte, character) in value.char_indices() {
        if utf16 == start {
            start_byte = Some(byte);
        }
        if utf16 == end {
            end_byte = Some(byte);
            break;
        }
        utf16 += character.len_utf16();
    }
    if start_byte.is_none() && utf16 == start {
        start_byte = Some(value.len());
    }
    if end_byte.is_none() && utf16 == end {
        end_byte = Some(value.len());
    }
    Some(&value[start_byte?..end_byte?])
}

fn write_text_segment(
    svg: &mut String,
    style: &crate::vector::TextStyle,
    content: &str,
    x: Option<f32>,
    width: Option<f32>,
    character_origins: Option<&[f32]>,
) {
    use std::fmt::Write;
    let family = match style.font_family.as_str() {
        "serif" => "Hiragino Mincho ProN, Songti SC, Noto Serif CJK JP, serif",
        "monospace" => "Menlo, Consolas, Noto Sans Mono CJK JP, monospace",
        "sans-serif" => "Hiragino Sans, PingFang SC, Noto Sans CJK JP, Arial, sans-serif",
        family => family,
    };
    let decoration = match (style.underline, style.strikethrough) {
        (true, true) => "underline line-through",
        (true, false) => "underline",
        (false, true) => "line-through",
        _ => "none",
    };
    let [r, g, b] = style.color;
    let x_attribute = character_origins
        .filter(|origins| !origins.is_empty())
        .map_or_else(
            || x.map_or_else(String::new, |x| format!(r#" x="{x}""#)),
            |origins| {
                let positions = origins
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(r#" x="{positions}""#)
            },
        );
    let width_attribute = width
        .filter(|_| character_origins.is_none())
        .filter(|_| content.chars().count() > 1)
        .map_or_else(String::new, |width| {
            format!(r#" textLength="{width}" lengthAdjust="spacing""#)
        });
    let _ = write!(
        svg,
        r##"<tspan{x_attribute}{width_attribute} fill="#{r:02x}{g:02x}{b:02x}" font-family="{}" font-size="{}" font-weight="{}" font-style="{}" letter-spacing="{}" baseline-shift="{}" text-decoration="{decoration}">{}</tspan>"##,
        escape_xml(family),
        style.font_size,
        if style.bold { 700 } else { 400 },
        if style.italic { "italic" } else { "normal" },
        style.tracking * style.font_size / 1000.0,
        style.baseline_shift,
        escape_xml(content)
    );
}

fn rgba_hex([r, g, b, a]: [u8; 4]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn transformed_control_points(object: &VectorObject) -> Vec<Point> {
    let [a, b, c, d, e, f] = object.transform;
    object
        .control_points
        .iter()
        .map(|[x, y]| Point {
            x: a * x + c * y + e,
            y: b * x + d * y + f,
        })
        .collect()
}

fn rebuild_vector_path(object: &mut VectorObject) -> Result<(), String> {
    use crate::vector::VectorObjectKind;
    use std::fmt::Write;
    object.path.data = match object.kind {
        VectorObjectKind::Text => return Err("Edit text through text settings".into()),
        VectorObjectKind::Compound => {
            return Err("Compound paths cannot be rebuilt from control points".into())
        }
        VectorObjectKind::Path => {
            let Some(first) = object.control_points.first() else {
                return Err("Path has no control points".into());
            };
            let mut data = format!("M {} {}", first[0], first[1]);
            for point in object.control_points.iter().skip(1) {
                let _ = write!(data, " L {} {}", point[0], point[1]);
            }
            data
        }
        VectorObjectKind::Rectangle => {
            let [first, second] = object.control_points.as_slice() else {
                return Err("Rectangle requires two control points".into());
            };
            let x = first[0].min(second[0]);
            let y = first[1].min(second[1]);
            let width = (second[0] - first[0]).abs();
            let height = (second[1] - first[1]).abs();
            format!("M {x} {y} H {} V {} H {x} Z", x + width, y + height)
        }
        VectorObjectKind::Ellipse => {
            let [first, second] = object.control_points.as_slice() else {
                return Err("Ellipse requires two control points".into());
            };
            let cx = (first[0] + second[0]) * 0.5;
            let cy = (first[1] + second[1]) * 0.5;
            let rx = (second[0] - first[0]).abs() * 0.5;
            let ry = (second[1] - first[1]).abs() * 0.5;
            format!(
                "M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z",
                cx - rx,
                cx + rx,
                cx - rx
            )
        }
    };
    Ok(())
}

const fn default_bit_depth() -> u8 {
    DEFAULT_BIT_DEPTH
}

fn validate_bit_depth(depth: u8) -> Result<(), String> {
    if [8, 16, 32].contains(&depth) {
        Ok(())
    } else {
        Err("Bit depth must be 8, 16, or 32 bits".into())
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use crate::vector::{FillRule, VectorObject, VectorPaint, VectorPath};
    #[test]
    fn round_trip_preserves_hidden_strokes_and_brush() {
        let mut source = Document::default();
        source
            .begin(
                Point { x: 20.0, y: 40.0 },
                Brush {
                    size: 37.0,
                    hardness: 0.35,
                    color: [12, 34, 56],
                },
            )
            .unwrap();
        source.finish();
        source.toggle_visibility();
        source.set_color_mode(ColorMode::Cmyk);
        source.set_bit_depth(16).unwrap();
        let bytes = source.encode().unwrap();
        let mut loaded = Document::decode(&bytes).unwrap();
        assert!(!loaded.snapshot().layer_visible);
        assert_eq!(loaded.snapshot().color_mode, ColorMode::Cmyk);
        assert_eq!(
            loaded.snapshot().color_profile,
            ColorProfile::JapanColor2001Coated
        );
        assert_eq!(loaded.snapshot().bit_depth, 16);
        loaded.toggle_visibility();
        let stroke = loaded.visible_strokes().next().unwrap();
        assert_eq!(stroke.brush.color, [12, 34, 56]);
        assert_eq!(stroke.brush.size, 37.0);
        assert_eq!(stroke.brush.hardness, 0.35);
        assert_eq!(stroke.points[0].y, 40.0);
    }

    #[test]
    fn editable_vector_layer_round_trips_and_updates_render_source() {
        let mut document = Document::default();
        let layer_id = document.add_vector_layer().unwrap();
        document
            .upsert_vector_object(
                &layer_id,
                VectorObject {
                    text: None,
                    id: "shape-1".into(),
                    name: "Blue triangle".into(),
                    path: VectorPath {
                        data: "M 10 10 L 80 10 L 40 70 Z".into(),
                        fill_rule: FillRule::EvenOdd,
                    },
                    transform: [1.0, 0.0, 0.0, 1.0, 5.0, 6.0],
                    fill: Some(VectorPaint {
                        color: [10, 80, 220, 255],
                    }),
                    stroke: Some(VectorPaint {
                        color: [0, 0, 0, 128],
                    }),
                    stroke_width: 2.0,
                    visible: true,
                    kind: crate::vector::VectorObjectKind::Path,
                    control_points: vec![[10.0, 10.0], [80.0, 10.0], [40.0, 70.0]],
                },
            )
            .unwrap();

        let bytes = document.encode().unwrap();
        let decoded = Document::decode(&bytes).unwrap();
        let layer = decoded
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap();
        assert!(layer.vector_layer);
        assert_eq!(layer.vector_objects.len(), 1);
        assert!(layer.source.contains("fill=\"#0a50dcff\""));
        assert!(layer.source.contains("fill-rule=\"evenodd\""));
        assert_eq!(decoded.snapshot().layers[1].kind, "vector");
    }

    #[test]
    fn partial_vector_drag_runs_preserve_painter_order_without_editing_document() {
        let mut document = Document::default();
        let layer_id = document.add_vector_layer().unwrap();
        for (index, id) in ["back", "middle", "front"].into_iter().enumerate() {
            document
                .upsert_vector_object(
                    &layer_id,
                    VectorObject {
                        text: None,
                        id: id.into(),
                        name: id.into(),
                        path: VectorPath {
                            data: "M 10 10 H 60 V 60 H 10 Z".into(),
                            fill_rule: FillRule::NonZero,
                        },
                        transform: [1.0, 0.0, 0.0, 1.0, index as f32 * 10.0, 0.0],
                        fill: Some(VectorPaint {
                            color: [50, 80, 120, 255],
                        }),
                        stroke: None,
                        stroke_width: 0.0,
                        visible: true,
                        kind: crate::vector::VectorObjectKind::Rectangle,
                        control_points: vec![[10.0, 10.0], [60.0, 60.0]],
                    },
                )
                .unwrap();
        }
        document
            .select_vector_objects(vec!["middle".into()])
            .unwrap();
        let layer = document
            .svg_layers()
            .find(|item| item.id == layer_id)
            .unwrap();
        let original = layer.source.clone();
        let revision = document.revision();
        let runs = document.vector_drag_runs(layer).unwrap();
        assert_eq!(
            runs.iter()
                .map(|(selected, _)| *selected)
                .collect::<Vec<_>>(),
            vec![false, true, false]
        );
        assert_eq!(
            runs.iter()
                .map(|(_, run)| run.vector_objects[0].id.as_str())
                .collect::<Vec<_>>(),
            vec!["back", "middle", "front"]
        );
        assert_eq!(layer.source, original);
        assert_eq!(document.revision(), revision);
        let mut faded = layer.clone();
        faded.opacity = 0.5;
        assert!(document.vector_drag_runs(&faded).is_none());
    }

    #[test]
    fn vector_edits_share_ordered_undo_redo_with_brush_strokes() {
        let mut document = Document::default();
        let layer_id = document.add_vector_layer().unwrap();
        document
            .upsert_vector_object(
                &layer_id,
                VectorObject {
                    text: None,
                    id: "shape-1".into(),
                    name: "Shape".into(),
                    path: VectorPath {
                        data: "M 0 0 L 20 0 L 20 20 Z".into(),
                        fill_rule: FillRule::NonZero,
                    },
                    transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    fill: Some(VectorPaint {
                        color: [255, 0, 0, 255],
                    }),
                    stroke: None,
                    stroke_width: 0.0,
                    visible: true,
                    kind: crate::vector::VectorObjectKind::Path,
                    control_points: vec![[0.0, 0.0], [20.0, 0.0], [20.0, 20.0]],
                },
            )
            .unwrap();
        document
            .select_vector_objects(vec!["shape-1".into(), "shape-1".into()])
            .unwrap();
        assert_eq!(document.snapshot().selected_vector_objects, ["shape-1"]);

        document
            .begin(Point { x: 2.0, y: 2.0 }, Brush::default())
            .unwrap();
        document.finish();
        document.undo();
        assert_eq!(document.snapshot().stroke_count, 0);
        assert_eq!(document.snapshot().selected_vector_objects, ["shape-1"]);

        document.undo();
        let layer = document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap();
        assert!(layer.vector_objects.is_empty());
        assert!(document.snapshot().selected_vector_objects.is_empty());

        document.redo();
        assert_eq!(document.snapshot().selected_vector_objects, ["shape-1"]);
        document.redo();
        assert_eq!(document.snapshot().stroke_count, 1);
    }

    #[test]
    fn vector_hit_testing_selects_front_object_and_move_is_undoable() {
        let mut document = Document::default();
        let layer_id = document.add_vector_layer().unwrap();
        document
            .upsert_vector_object(
                &layer_id,
                VectorObject {
                    text: None,
                    id: "rectangle-1".into(),
                    name: "Rectangle".into(),
                    path: VectorPath {
                        data: "M 10 10 H 30 V 40 H 10 Z".into(),
                        fill_rule: FillRule::NonZero,
                    },
                    transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    fill: Some(VectorPaint {
                        color: [0, 0, 0, 255],
                    }),
                    stroke: None,
                    stroke_width: 0.0,
                    visible: true,
                    kind: crate::vector::VectorObjectKind::Rectangle,
                    control_points: vec![[10.0, 10.0], [30.0, 40.0]],
                },
            )
            .unwrap();
        assert_eq!(
            document.select_vector_at(Point { x: 20.0, y: 20.0 }, 2.0, false),
            Some("rectangle-1".into())
        );
        assert!(document.move_selected_vectors(12.0, -4.0).unwrap());
        let moved = &document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .vector_objects[0];
        assert_eq!(&moved.transform[4..], &[12.0, -4.0]);

        document.undo();
        let restored = &document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .vector_objects[0];
        assert_eq!(&restored.transform[4..], &[0.0, 0.0]);
        assert_eq!(document.snapshot().selected_vector_objects, ["rectangle-1"]);
        assert_eq!(document.vector_overlay_selections().len(), 3);
        assert_eq!(
            document.selected_control_at(Point { x: 10.0, y: 10.0 }, 1.0),
            Some(("rectangle-1".into(), 0))
        );
        document
            .move_vector_control("rectangle-1", 0, Point { x: 5.0, y: 6.0 })
            .unwrap();
        let edited = &document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .vector_objects[0];
        assert_eq!(edited.control_points[0], [5.0, 6.0]);
        assert!(edited.path.data.contains("M 5 6"));
        document.undo();
        let restored_control = &document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .vector_objects[0];
        assert_eq!(restored_control.control_points[0], [10.0, 10.0]);
    }

    #[test]
    fn old_strokes_default_to_full_hardness() {
        let mut doc = Document::default();
        doc.begin(Point { x: 1.0, y: 1.0 }, Brush::default())
            .unwrap();
        doc.finish();
        let mut value: serde_json::Value = serde_json::from_slice(&doc.encode().unwrap()).unwrap();
        value["strokes"][0]["brush"]
            .as_object_mut()
            .unwrap()
            .remove("hardness");
        let loaded = Document::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(loaded.visible_strokes().next().unwrap().brush.hardness, 1.0);
    }
    #[test]
    fn rejects_unknown_versions_invalid_brush_and_truncated_files() {
        let mut doc = Document::default();
        doc.begin(Point { x: 1.0, y: 1.0 }, Brush::default())
            .unwrap();
        let bytes = doc.encode().unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["version"] = 99.into();
        assert!(Document::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        value["version"] = 1.into();
        value["strokes"][0]["brush"]["size"] = 999.into();
        assert!(Document::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(Document::decode(&bytes[..bytes.len() - 1]).is_err());
    }
    #[test]
    fn save_and_load_track_dirty_and_monotonic_revision() {
        let mut doc = Document::default();
        doc.begin(Point { x: 1.0, y: 1.0 }, Brush::default())
            .unwrap();
        let bytes = doc.encode().unwrap();
        assert!(doc.snapshot().dirty);
        doc.mark_saved("test.lumapaint".into());
        assert!(!doc.snapshot().dirty);
        doc.undo();
        assert!(doc.snapshot().dirty);
        let revision = doc.snapshot().revision;
        doc.replace_loaded(Document::decode(&bytes).unwrap(), "test.lumapaint".into());
        assert!(doc.snapshot().revision > revision);
        assert!(!doc.snapshot().dirty);
        assert!(!doc.snapshot().can_redo);
    }
    #[test]
    fn color_mode_changes_revision_and_old_files_default_to_rgb() {
        let mut doc = Document::default();
        let initial_revision = doc.snapshot().revision;
        doc.set_color_mode(ColorMode::Cmyk);
        assert_eq!(doc.snapshot().color_mode, ColorMode::Cmyk);
        assert!(doc.snapshot().dirty);
        assert!(doc.snapshot().revision > initial_revision);

        let mut value: serde_json::Value = serde_json::from_slice(&doc.encode().unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("colorMode");
        value.as_object_mut().unwrap().remove("colorProfile");
        let loaded = Document::decode(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(loaded.snapshot().color_mode, ColorMode::Rgb);
        assert_eq!(loaded.snapshot().color_profile, ColorProfile::Srgb);
        assert_eq!(loaded.snapshot().bit_depth, DEFAULT_BIT_DEPTH);
    }
    #[test]
    fn bit_depth_changes_revision_and_rejects_unknown_depths() {
        let mut doc = Document::default();
        let revision = doc.snapshot().revision;
        doc.set_bit_depth(32).unwrap();
        assert_eq!(doc.snapshot().bit_depth, 32);
        assert!(doc.snapshot().revision > revision);
        assert!(doc.set_bit_depth(12).is_err());
        assert_eq!(doc.snapshot().bit_depth, 32);
    }
    #[test]
    fn color_profile_follows_mode_and_rejects_incompatible_profiles() {
        let mut doc = Document::default();
        doc.set_color_profile(ColorProfile::DisplayP3).unwrap();
        assert_eq!(doc.snapshot().color_profile, ColorProfile::DisplayP3);
        assert!(doc
            .set_color_profile(ColorProfile::JapanColor2001Coated)
            .is_err());
        doc.set_color_mode(ColorMode::Cmyk);
        assert_eq!(
            doc.snapshot().color_profile,
            ColorProfile::JapanColor2001Coated
        );
        assert!(doc.set_color_profile(ColorProfile::Srgb).is_err());
    }

    #[test]
    fn svg_layers_round_trip_and_toggle_independently() {
        let mut doc = Document::default();
        doc.import_svg(
            "logo".into(),
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#.into(),
        )
        .unwrap();
        assert_eq!(doc.snapshot().layers.len(), 2);
        assert_eq!(doc.snapshot().layers[1].kind, "svg");
        doc.toggle_layer("svg-layer-1").unwrap();
        assert!(!doc.snapshot().layers[1].visible);

        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.snapshot().layers[1].name, "logo");
        assert!(!loaded.snapshot().layers[1].visible);
        assert_eq!(loaded.svg_layers().count(), 1);
    }

    #[test]
    fn layer_settings_are_persisted_and_locked_paint_rejects_strokes() {
        let mut doc = Document::default();
        doc.set_layer_settings(LayerSettings {
            id: "layer-1".into(),
            name: "Sketch".into(),
            opacity: 0.4,
            locked: true,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
        })
        .unwrap();
        assert!(!doc
            .begin(Point { x: 20.0, y: 20.0 }, Brush::default())
            .unwrap());
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        let layer = &loaded.snapshot().layers[0];
        assert_eq!(layer.name, "Sketch");
        assert_eq!(layer.opacity, 0.4);
        assert!(layer.locked);
    }

    #[test]
    fn imported_svg_layer_can_be_renamed_and_deleted() {
        let mut doc = Document::default();
        doc.import_svg(
            "logo".into(),
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#.into(),
        )
        .unwrap();
        doc.set_layer_settings(LayerSettings {
            id: "svg-layer-1".into(),
            name: "Mark".into(),
            opacity: 0.5,
            locked: true,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.0,
        })
        .unwrap();
        assert_eq!(doc.snapshot().layers[1].name, "Mark");
        doc.delete_layer("svg-layer-1").unwrap();
        assert_eq!(doc.snapshot().layers.len(), 1);
        assert!(doc.delete_layer("layer-1").is_err());
    }

    #[test]
    fn mask_changes_effective_opacity_and_alpha_lock_clips_new_strokes() {
        let mut doc = Document::default();
        doc.begin(Point { x: 30.0, y: 40.0 }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 100.0, y: 140.0 }).unwrap();
        doc.finish();
        doc.set_layer_settings(LayerSettings {
            id: "layer-1".into(),
            name: "Paint".into(),
            opacity: 0.8,
            locked: false,
            alpha_locked: true,
            mask_enabled: true,
            mask_inverted: false,
            mask_density: 0.5,
        })
        .unwrap();
        assert_eq!(doc.paint_layer_opacity(), 0.4);
        assert!(doc
            .begin(Point { x: 60.0, y: 80.0 }, Brush::default())
            .unwrap());
        doc.finish();
        assert!(!doc
            .begin(Point { x: 900.0, y: 600.0 }, Brush::default())
            .unwrap());
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        let layer = &loaded.snapshot().layers[0];
        assert!(layer.alpha_locked && layer.mask_enabled);
        assert_eq!(layer.mask_density, 0.5);
    }

    #[test]
    fn adding_a_paint_layer_creates_a_persisted_layer_record() {
        let mut doc = Document::default();
        let id = doc.add_paint_layer().unwrap();
        let second = doc.add_paint_layer().unwrap();
        doc.reorder_layers(&[id.clone(), second.clone()]).unwrap();
        let snapshot = doc.snapshot();
        let layer = snapshot.layers.iter().find(|layer| layer.id == id).unwrap();
        assert_eq!(layer.kind, "paint");
        assert!(layer.deletable);
        assert_eq!(snapshot.layers[1].id, second);
        assert_eq!(snapshot.layers[2].id, id);
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert!(loaded.snapshot().layers.iter().any(|layer| layer.id == id));
        assert!(doc.reorder_layers(&["layer-1".into(), id]).is_err());
    }

    #[test]
    fn layer_order_moves_across_multiple_rows_and_survives_save() {
        let mut doc = Document::default();
        let a = doc.add_paint_layer().unwrap();
        let b = doc.add_paint_layer().unwrap();
        doc.import_svg(
            "Logo".into(),
            r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#.into(),
        )
        .unwrap();
        let c = doc.snapshot().layers.last().unwrap().id.clone();
        for order in [
            vec![a.clone(), c.clone(), b.clone()],
            vec![c.clone(), b.clone(), a.clone()],
        ] {
            doc.reorder_layers(&order).unwrap();
            let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
            assert_eq!(
                loaded
                    .svg_layers
                    .iter()
                    .rev()
                    .map(|layer| layer.id.clone())
                    .collect::<Vec<_>>(),
                order
            );
        }
        let revision = doc.snapshot().revision;
        doc.reorder_layers(&[c.clone(), b.clone(), a.clone()])
            .unwrap();
        assert_eq!(doc.snapshot().revision, revision);
        for invalid in [
            vec![a.clone(), a.clone(), b.clone()],
            vec!["unknown".into(), a.clone(), b.clone()],
            vec![a.clone()],
        ] {
            assert!(doc.reorder_layers(&invalid).is_err());
            assert_eq!(doc.snapshot().revision, revision);
            assert_eq!(
                doc.svg_layers
                    .iter()
                    .map(|layer| layer.id.clone())
                    .collect::<Vec<_>>(),
                vec![a.clone(), b.clone(), c.clone()]
            );
        }
    }
}

#[cfg(test)]
mod text_tests {
    use super::*;
    fn settings() -> TextSettings {
        TextSettings {
            id: None,
            text: VectorText {
                content: "日本語 & <标题>\nHello".into(),
                font_family: "sans-serif".into(),
                font_size: 36.0,
                line_height: 1.5,
                bold: true,
                ..VectorText::default()
            },
            position: [40.0, 50.0],
            color: [24, 80, 160],
        }
    }

    #[test]
    fn vector_object_reorder_changes_stack_and_is_undoable() {
        let mut document = Document::default();
        let layer_id = document.add_vector_layer().unwrap();
        document.select_layer(layer_id.clone()).unwrap();
        let mut first = settings();
        first.text.content = "Back".into();
        document.set_text_object(first).unwrap();
        let mut second = settings();
        second.text.content = "Front".into();
        document.set_text_object(second).unwrap();
        let original: Vec<_> = document
            .snapshot()
            .layers
            .into_iter()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .objects
            .into_iter()
            .rev()
            .map(|object| object.id)
            .collect();
        let reordered = vec![original[1].clone(), original[0].clone()];
        document
            .reorder_vector_objects(&layer_id, &reordered)
            .unwrap();
        let current: Vec<_> = document
            .snapshot()
            .layers
            .into_iter()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .objects
            .into_iter()
            .rev()
            .map(|object| object.id)
            .collect();
        assert_eq!(current, reordered);
        document.undo();
        let restored: Vec<_> = document
            .snapshot()
            .layers
            .into_iter()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .objects
            .into_iter()
            .rev()
            .map(|object| object.id)
            .collect();
        assert_eq!(restored, original);
    }
    #[test]
    fn soft_wraps_preserve_source_offsets_styles_and_saved_layout() {
        let mut edit = settings();
        edit.text.content = "A😀BC\n日本語".into();
        edit.text.soft_breaks = vec![3, 8];
        edit.text.box_width = 120.0;
        edit.text.indent_first = 12.0;
        edit.text
            .apply_style(
                3,
                4,
                &crate::vector::TextStylePatch {
                    color: Some([255, 0, 0]),
                    ..Default::default()
                },
                edit.color,
            )
            .unwrap();
        assert_eq!(
            edit.text.visual_lines(),
            vec![
                (0, "A😀", false),
                (3, "BC", false),
                (6, "日本", true),
                (8, "語", false)
            ]
        );
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        let svg = &doc.svg_layers[0].source;
        assert_eq!(svg.matches("<tspan x=").count(), 4);
        assert!(svg.contains("A😀</tspan>"));
        assert!(svg.contains("fill=\"#ff0000\""));
        assert!(svg.contains("x=\"12\" y=\"36\""));
        assert!(svg.contains("x=\"0\" y=\"90\""));
        assert_eq!(
            Document::decode(&doc.encode().unwrap())
                .unwrap()
                .snapshot()
                .text_objects[0]
                .text
                .soft_breaks,
            vec![3, 8]
        );
        let mut bad = edit;
        for breaks in [vec![2], vec![3, 3], vec![5], vec![6]] {
            bad.text.soft_breaks = breaks;
            assert!(bad.text.validate().is_err());
        }
    }
    #[test]
    fn measured_text_bounds_drive_selection_and_survive_save() {
        let mut edit = settings();
        edit.text.content = "Visible".into();
        edit.text.layout_bounds = Some([10.0, 8.0, 80.0, 24.0]);
        edit.text.rotation = 45.0;
        edit.position = [100.0, 100.0];
        let points = edit.text.control_points();
        let center = [
            100.0 + points.iter().map(|point| point[0]).sum::<f32>() / 4.0,
            100.0 + points.iter().map(|point| point[1]).sum::<f32>() / 4.0,
        ];
        let min_x = points
            .iter()
            .map(|point| point[0])
            .fold(f32::INFINITY, f32::min);
        let min_y = points
            .iter()
            .map(|point| point[1])
            .fold(f32::INFINITY, f32::min);
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        assert_eq!(
            doc.text_at(center),
            Some(doc.snapshot().text_objects[0].id.clone())
        );
        assert_eq!(
            doc.text_at([100.0 + min_x + 1.0, 100.0 + min_y + 1.0]),
            None
        );
        assert_eq!(
            Document::decode(&doc.encode().unwrap())
                .unwrap()
                .snapshot()
                .text_objects[0]
                .text
                .layout_bounds,
            edit.text.layout_bounds
        );
        for bounds in [[0.0, 0.0, 0.0, 5.0], [0.0, 0.0, 5.0, f32::NAN]] {
            edit.text.layout_bounds = Some(bounds);
            assert!(edit.text.validate().is_err());
        }
    }
    #[test]
    fn measured_line_baselines_drive_svg_and_survive_save() {
        let mut edit = settings();
        edit.text.content = "Large\nSmall".into();
        edit.text.line_baselines = vec![38.0, 104.5];
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        let svg = &doc.svg_layers[0].source;
        assert!(svg.contains("y=\"38\""));
        assert!(svg.contains("y=\"104.5\""));
        let restored = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            restored.snapshot().text_objects[0].text.line_baselines,
            vec![38.0, 104.5]
        );
        for baselines in [vec![38.0], vec![38.0, 38.0], vec![38.0, f32::NAN]] {
            edit.text.line_baselines = baselines;
            assert!(edit.text.validate().is_err());
        }
    }
    #[test]
    fn measured_line_widths_drive_svg_without_stretching_glyphs() {
        let mut edit = settings();
        edit.text.content = "Wide\nNarrow".into();
        edit.text.line_widths = vec![125.5, 71.0];
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        let svg = &doc.svg_layers[0].source;
        assert!(svg.contains("textLength=\"125.5\" lengthAdjust=\"spacing\""));
        assert!(svg.contains("textLength=\"71\" lengthAdjust=\"spacing\""));
        let restored = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            restored.snapshot().text_objects[0].text.line_widths,
            vec![125.5, 71.0]
        );
        for widths in [vec![125.5], vec![125.5, f32::NAN], vec![-1.0, 71.0]] {
            edit.text.line_widths = widths;
            assert!(edit.text.validate().is_err());
        }
    }
    #[test]
    fn measured_line_origins_override_legacy_alignment() {
        let mut edit = settings();
        edit.text.content = "Centered\nRight".into();
        edit.text.alignment = crate::vector::TextAlignment::Center;
        edit.text.line_origins = vec![42.0, 75.5];
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        let svg = &doc.svg_layers[0].source;
        assert!(svg.contains("x=\"42\" y=\"36\" text-anchor=\"start\""));
        assert!(svg.contains("x=\"75.5\" y=\"90\" text-anchor=\"start\""));
        let restored = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            restored.snapshot().text_objects[0].text.line_origins,
            vec![42.0, 75.5]
        );
        for origins in [vec![42.0], vec![42.0, f32::NAN]] {
            edit.text.line_origins = origins;
            assert!(edit.text.validate().is_err());
        }
    }
    #[test]
    fn measured_character_origins_drive_svg_and_survive_save() {
        let mut edit = settings();
        edit.text.content = "A😀字".into();
        edit.text.line_origins = vec![3.0];
        edit.text.line_widths = vec![90.0];
        edit.text.character_origins = vec![vec![3.0, 25.5, 64.0]];
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        let svg = &doc.svg_layers[0].source;
        assert!(svg.contains("x=\"3 25.5 64\""));
        assert!(!svg.contains("textLength=\"90\""));
        let restored = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            restored.snapshot().text_objects[0].text.character_origins,
            edit.text.character_origins
        );
        for origins in [
            vec![vec![3.0, 25.5]],
            vec![vec![3.0, f32::NAN, 64.0]],
            vec![vec![3.0, 64.0, 25.5]],
        ] {
            edit.text.character_origins = origins;
            assert!(edit.text.validate().is_err());
        }
    }
    #[test]
    fn measured_glyph_clusters_keep_combining_sequences_together() {
        let mut edit = settings();
        edit.text.content = "e\u{301}x".into();
        edit.text.line_origins = vec![4.0];
        edit.text.glyph_clusters = vec![vec![
            crate::vector::TextGlyphCluster {
                start: 0,
                end: 2,
                x: 4.0,
            },
            crate::vector::TextGlyphCluster {
                start: 2,
                end: 3,
                x: 31.5,
            },
        ]];
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        let svg = &doc.svg_layers[0].source;
        assert!(svg.contains("x=\"4\""));
        assert!(svg.contains("x=\"31.5\""));
        assert!(svg.contains("e\u{301}</tspan>"), "{svg}");
        let restored = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            restored.snapshot().text_objects[0].text.glyph_clusters,
            edit.text.glyph_clusters
        );
        edit.text.glyph_clusters[0][0].end = 1;
        assert!(edit.text.validate().is_err());
    }
    #[test]
    fn measured_style_segment_origins_are_saved_and_bound_to_runs() {
        let mut edit = settings();
        edit.text.content = "ABCD".into();
        edit.text
            .apply_style(
                2,
                4,
                &crate::vector::TextStylePatch {
                    bold: Some(false),
                    ..Default::default()
                },
                edit.color,
            )
            .unwrap();
        edit.text.style_segment_origins = vec![vec![4.0, 81.0]];
        edit.text.line_widths = vec![130.0];
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        let svg = &doc.svg_layers[0].source;
        assert!(svg.contains("<tspan x=\"4\" textLength="));
        assert!(svg.contains("<tspan x=\"81\" textLength="));
        assert!(svg.contains("x=\"4\" textLength=\"77\" lengthAdjust=\"spacing\""));
        assert!(svg.contains("x=\"81\" textLength=\"53\" lengthAdjust=\"spacing\""));
        let restored = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            restored.snapshot().text_objects[0]
                .text
                .style_segment_origins,
            vec![vec![4.0, 81.0]]
        );
        for segments in [vec![vec![]], vec![vec![f32::NAN]]] {
            edit.text.style_segment_origins = segments;
            assert!(edit.text.validate().is_err());
        }
    }
    #[test]
    fn inline_preview_preserves_original_and_commits_as_one_edit() {
        let mut doc = Document::default();
        doc.set_text_object(settings()).unwrap();
        let original = doc.encode().unwrap();
        let object = doc.snapshot().text_objects[0].clone();
        let preview = doc.text_edit_preview(Some(&object.id)).unwrap();
        assert!(!preview.svg_layers[0].source.contains("<text"));
        assert_eq!(original, doc.encode().unwrap());
        let mut next = settings();
        next.id = Some(object.id);
        let revision = doc.snapshot().revision;
        doc.set_text_object(next.clone()).unwrap();
        assert_eq!(revision, doc.snapshot().revision);
        next.text.content = "Inline 日本語".into();
        next.text.tracking = 80.0;
        next.text.italic = true;
        doc.set_text_object(next).unwrap();
        doc.undo();
        assert_eq!(original, doc.encode().unwrap());
        doc.redo();
        assert_eq!(doc.snapshot().text_objects[0].text.content, "Inline 日本語");
    }

    #[test]
    fn typography_round_trip_legacy_defaults_and_invalid_values() {
        let legacy: VectorText = serde_json::from_str(r#"{"content":"Legacy","fontFamily":"sans-serif","fontSize":24,"lineHeight":1.4,"bold":false}"#).unwrap();
        assert_eq!(legacy.scale_x, 1.0);
        assert_eq!(legacy.alignment, crate::vector::TextAlignment::Left);
        legacy.validate().unwrap();
        let mut next = settings();
        next.text.font_family = "Font \"Name\" & More".into();
        next.text.tracking = 120.0;
        next.text.scale_x = 1.2;
        next.text.rotation = 15.0;
        next.text.alignment = crate::vector::TextAlignment::Right;
        next.text.underline = true;
        next.text.space_before = 8.0;
        let mut doc = Document::default();
        doc.set_text_object(next.clone()).unwrap();
        let source = &doc.svg_layers[0].source;
        assert!(source.contains("&quot;Name&quot; &amp; More"));
        assert!(source.contains("text-anchor=\"end\""));
        assert!(source.contains("text-decoration=\"underline\""));
        let encoded = doc.encode().unwrap();
        assert_eq!(
            Document::decode(&encoded).unwrap().snapshot().text_objects[0].text,
            next.text
        );
        next.id = Some(doc.snapshot().text_objects[0].id.clone());
        for value in [f32::NAN, f32::INFINITY, -1.0, 5.0] {
            let mut invalid = next.clone();
            invalid.text.scale_x = value;
            assert!(doc.set_text_object(invalid).is_err());
            assert_eq!(encoded, doc.encode().unwrap());
        }
        next.text.indent_left = next.text.box_width;
        assert!(doc.set_text_object(next).is_err());
    }

    #[test]
    fn text_round_trip_edit_move_and_atomic_history() {
        let mut doc = Document::default();
        doc.set_text_object(settings()).unwrap();
        let text = doc.snapshot().text_objects[0].clone();
        assert!(doc.svg_layers[0].source.contains("&amp; &lt;标题&gt;"));
        assert_eq!(
            doc.select_vector_at(Point { x: 50.0, y: 70.0 }, 3.0, false),
            Some(text.id.clone())
        );
        assert!(doc
            .selected_control_at(Point { x: 40.0, y: 50.0 }, 3.0)
            .is_none());
        doc.move_selected_vectors(10.0, 15.0).unwrap();
        assert_eq!(doc.snapshot().text_objects[0].position, [50.0, 65.0]);
        doc.undo();
        let mut edit = settings();
        edit.id = Some(text.id);
        edit.text.content = "Edited".into();
        doc.set_text_object(edit).unwrap();
        doc.undo();
        assert_eq!(
            doc.snapshot().text_objects[0].text.content,
            settings().text.content
        );
        doc.redo();
        assert_eq!(doc.snapshot().text_objects[0].text.content, "Edited");
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.snapshot().text_objects[0].text.content, "Edited");
        doc.undo();
        doc.undo();
        assert!(doc.snapshot().text_objects.is_empty());
        assert_eq!(doc.snapshot().layers.len(), 1);
        doc.redo();
        assert_eq!(doc.snapshot().text_objects.len(), 1);
    }
    #[test]
    fn translated_text_preview_preserves_document_styles_history_and_other_objects() {
        let mut doc = Document::default();
        let mut input = settings();
        input
            .text
            .apply_style(
                0,
                1,
                &crate::vector::TextStylePatch {
                    bold: Some(true),
                    ..Default::default()
                },
                input.color,
            )
            .unwrap();
        doc.set_text_object(input.clone()).unwrap();
        let selected = doc.snapshot().text_objects[0].id.clone();
        input.position = [500.0, 300.0];
        doc.set_text_object(input).unwrap();
        doc.select_vector_objects(vec![selected]).unwrap();
        let before = doc.encode().unwrap();
        let revision = doc.snapshot().revision;
        let layer = doc.svg_layers[0].clone();
        assert!(doc.vector_layer_moves_as_unit(&layer));
        let preview = doc
            .translated_vector_layer(&layer, 25.0, -10.0)
            .unwrap()
            .unwrap();
        assert_eq!(
            preview.vector_objects[0].transform[4],
            layer.vector_objects[0].transform[4] + 25.0
        );
        assert_eq!(
            preview.vector_objects[0].transform[5],
            layer.vector_objects[0].transform[5] - 10.0
        );
        assert_eq!(preview.vector_objects[0].text, layer.vector_objects[0].text);
        assert_ne!(preview.source, layer.source);
        assert!(!doc.vector_layer_moves_as_unit(&doc.svg_layers[1]));
        assert!(doc
            .translated_vector_layer(&doc.svg_layers[1], 25.0, -10.0)
            .unwrap()
            .is_none());
        assert_eq!(doc.encode().unwrap(), before);
        assert_eq!(doc.snapshot().revision, revision);
        let overlay = doc.vector_overlay_selections()[0].regions[0].bounds;
        let moved = doc.vector_overlay_selections_at_offset(25.0, -10.0)[0].regions[0].bounds;
        assert_eq!(
            moved,
            [overlay[0] + 25.0, overlay[1] - 10.0, overlay[2], overlay[3]]
        );
        let mut locked = layer.clone();
        locked.locked = true;
        assert!(!doc.vector_layer_moves_as_unit(&locked));
        assert!(doc
            .translated_vector_layer(&locked, 25.0, -10.0)
            .unwrap()
            .is_none());
        assert!(doc.translated_vector_layer(&layer, f32::NAN, 0.0).is_err());
    }

    #[test]
    fn character_styles_round_trip_svg_and_atomic_document_history() {
        let mut doc = Document::default();
        let mut edit = settings();
        edit.text.content = "A日😀&<B".into();
        doc.set_text_object(edit.clone()).unwrap();
        edit.id = Some(doc.snapshot().text_objects[0].id.clone());
        edit.text
            .apply_style(
                1,
                4,
                &crate::vector::TextStylePatch {
                    font_size: Some(80.0),
                    bold: Some(true),
                    color: Some([255, 0, 0]),
                    underline: Some(true),
                    ..Default::default()
                },
                edit.color,
            )
            .unwrap();
        doc.set_text_object(edit.clone()).unwrap();
        let styled = doc.snapshot().text_objects[0].text.clone();
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.snapshot().text_objects[0].text, styled);
        let svg = vector_svg(960, 640, &doc.svg_layers[0].vector_objects);
        assert!(svg.contains("font-size=\"80\""));
        assert!(svg.contains("fill=\"#ff0000\""));
        assert!(svg.contains("日😀</tspan>"));
        assert!(svg.contains("&amp;&lt;B</tspan>"));
        let revision = doc.snapshot().revision;
        doc.set_text_object(edit).unwrap();
        assert_eq!(doc.snapshot().revision, revision);
        doc.undo();
        assert!(doc.snapshot().text_objects[0].text.runs.is_empty());
        doc.redo();
        assert_eq!(doc.snapshot().text_objects[0].text, styled);
    }
    #[test]
    fn invalid_or_locked_text_edits_leave_document_unchanged() {
        let mut doc = Document::default();
        let mut invalid = settings();
        invalid.text.font_size = f32::NAN;
        assert!(doc.set_text_object(invalid).is_err());
        assert_eq!(doc.snapshot().revision, 0);
        doc.set_text_object(settings()).unwrap();
        doc.svg_layers[0].locked = true;
        let revision = doc.snapshot().revision;
        let mut edit = settings();
        edit.id = Some(doc.snapshot().text_objects[0].id.clone());
        edit.text.content = "Rejected".into();
        assert!(doc.set_text_object(edit).is_err());
        assert_eq!(doc.snapshot().revision, revision);
        assert_eq!(
            doc.snapshot().text_objects[0].text.content,
            settings().text.content
        );
        assert!(!doc.snapshot().text_objects[0].editable);
    }
}
