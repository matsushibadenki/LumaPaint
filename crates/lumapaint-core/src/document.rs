//! Session document: a stable layer identity and retained brush strokes.
//! This is deliberately independent of pixels, AppKit, and the renderer.
use crate::selection::SelectionGesture;
pub use crate::selection::{
    Selection, SelectionMode, SelectionOperation, SelectionRegion, SelectionShape,
};
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
pub struct LayerSnapshot {
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
    pub layer_id: &'static str,
    pub layer_visible: bool,
    pub color_mode: ColorMode,
    pub color_profile: ColorProfile,
    pub bit_depth: u8,
    pub stroke_count: usize,
    pub layers: Vec<LayerSnapshot>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub revision: u64,
    pub dirty: bool,
    pub file_name: Option<String>,
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
            layer_name: "Layer 1".into(),
            layer_opacity: 1.0,
            layer_locked: false,
            layer_alpha_locked: false,
            layer_mask_enabled: false,
            layer_mask_inverted: false,
            layer_mask_density: 1.0,
            svg_layers: Vec::new(),
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
    pub fn snapshot(&self) -> DocumentSnapshot {
        let mut layers = vec![LayerSnapshot {
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
        layers.extend(self.svg_layers.iter().map(|layer| LayerSnapshot {
            id: layer.id.clone(),
            name: layer.name.clone(),
            kind: if layer.paint_layer { "paint" } else { "svg" },
            visible: layer.visible,
            opacity: layer.opacity,
            locked: layer.locked,
            alpha_locked: layer.alpha_locked,
            mask_enabled: layer.mask_enabled,
            mask_inverted: layer.mask_inverted,
            mask_density: layer.mask_density,
            deletable: true,
            stroke_count: 0,
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
            layer_id: "layer-1",
            layer_visible: self.visible,
            color_mode: self.color_mode,
            color_profile: self.color_profile,
            bit_depth: self.bit_depth,
            stroke_count: self.strokes.len(),
            layers,
            can_undo: !self.strokes.is_empty(),
            can_redo: !self.redo.is_empty(),
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
            layer_name: file.layer_name.unwrap_or_else(|| "Layer 1".into()),
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
            self.revision += 1;
        }
    }
    pub fn undo(&mut self) {
        self.finish();
        if let Some(stroke) = self.strokes.pop() {
            self.point_count -= stroke.points.len();
            self.redo.push(stroke);
            self.revision += 1;
        }
    }
    pub fn redo(&mut self) {
        self.finish();
        if let Some(stroke) = self.redo.pop() {
            self.point_count += stroke.points.len();
            self.strokes.push(stroke);
            self.revision += 1;
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
        let before = self.svg_layers.len();
        self.svg_layers.retain(|layer| layer.id != id);
        if self.svg_layers.len() == before {
            return Err("Layer not found".into());
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
    pub fn import_svg(&mut self, name: String, source: String) -> Result<(), String> {
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
    pub fn visible_svg_layers(&self) -> impl Iterator<Item = &SvgLayer> {
        self.svg_layers.iter().filter(|layer| layer.visible)
    }
    pub fn svg_layers(&self) -> impl Iterator<Item = &SvgLayer> {
        self.svg_layers.iter()
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

fn mask_factor(enabled: bool, inverted: bool, density: f32) -> f32 {
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
        assert!(doc
            .extend(Point {
                x: f32::NAN,
                y: 20.0
            })
            .is_err());
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
