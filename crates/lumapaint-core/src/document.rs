//! Session document: a stable layer identity and retained brush strokes.
//! This is deliberately independent of pixels, AppKit, and the renderer.
use serde::{Deserialize, Serialize};

pub const WIDTH: f32 = 960.0;
pub const HEIGHT: f32 = 640.0;
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
    fn on_page(self) -> bool {
        self.x >= 0.0 && self.y >= 0.0 && self.x < WIDTH && self.y < HEIGHT
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub brush: Brush,
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SvgLayer {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub source: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerSnapshot {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub visible: bool,
    pub stroke_count: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSnapshot {
    pub width: u32,
    pub height: u32,
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
    strokes: Vec<Stroke>,
    redo: Vec<Stroke>,
    active: Option<Stroke>,
    visible: bool,
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
            strokes: Vec::new(),
            redo: Vec::new(),
            active: None,
            visible: true,
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
            name: "Layer 1".into(),
            kind: "paint",
            visible: self.visible,
            stroke_count: self.strokes.len(),
        }];
        layers.extend(self.svg_layers.iter().map(|layer| LayerSnapshot {
            id: layer.id.clone(),
            name: layer.name.clone(),
            kind: "svg",
            visible: layer.visible,
            stroke_count: 0,
        }));
        DocumentSnapshot {
            width: WIDTH as u32,
            height: HEIGHT as u32,
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
            width: WIDTH as u32,
            height: HEIGHT as u32,
            layer_visible: self.visible,
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
            || file.width != WIDTH as u32
            || file.height != HEIGHT as u32
        {
            return Err("Unsupported project format, version, or dimensions".into());
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
            if stroke.points.is_empty() || stroke.points.iter().any(|p| !p.valid()) {
                return Err("Invalid stroke points".into());
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
            strokes: file.strokes,
            svg_layers: file.svg_layers,
            visible: file.layer_visible,
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
        brush.validate()?;
        if !point.valid() {
            return Err("Invalid pointer position".into());
        }
        self.finish();
        if !self.visible || !point.on_page() {
            return Ok(false);
        }
        if self.point_count >= MAX_POINTS {
            return Err("Session stroke limit reached. Undo a stroke to continue.".into());
        }
        self.active = Some(Stroke {
            brush,
            points: vec![point],
        });
        Ok(true)
    }
    pub fn extend(&mut self, point: Point) -> Result<(), String> {
        if !point.valid() {
            return Err("Invalid pointer position".into());
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
    pub fn import_svg(&mut self, name: String, source: String) -> Result<(), String> {
        self.finish();
        let id = format!("svg-layer-{}", self.svg_layers.len() + 1);
        let layer = SvgLayer {
            id,
            name,
            visible: true,
            source,
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
    pub fn visible_strokes(&self) -> impl Iterator<Item = &Stroke> {
        self.strokes
            .iter()
            .chain(self.active.iter())
            .filter(|_| self.visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stroke(doc: &mut Document) {
        doc.begin(Point { x: 30.0, y: 40.0 }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 100.0, y: 140.0 }).unwrap();
        doc.finish();
    }
    #[test]
    fn undo_redo_restores_stroke_data_without_image_copies() {
        let mut doc = Document::default();
        stroke(&mut doc);
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
    width: u32,
    height: u32,
    layer_visible: bool,
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
}
