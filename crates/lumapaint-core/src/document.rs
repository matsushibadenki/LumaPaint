#[path = "animation.rs"]
pub mod animation;
#[path = "document_channels.rs"]
mod document_channels;
#[path = "document_layer_masks.rs"]
mod document_layer_masks;
#[path = "document_vector_selection.rs"]
mod vector_selection;
pub use vector_selection::{
    SavedVectorSelection, SavedVectorSelectionSummary, VectorSelectionRequest,
};
#[path = "document_clone_stamp.rs"]
mod clone_stamp;
#[path = "document_compound_shapes.rs"]
mod compound_shapes;
#[path = "document_crop.rs"]
mod crop;
pub use compound_shapes::{CompoundShape, CompoundShapeEdit, CompoundShapeSnapshot};
#[path = "document_layer_groups.rs"]
mod layer_groups;
pub use layer_groups::{GroupMaskSettings, LayerGroupEdit, LayerGroupsSnapshot, LayerGroupsState};
#[path = "document_guides.rs"]
mod guides;
pub use guides::{Guide, GuideEdit, GuidesSnapshot, GuidesState};
#[path = "document_pages.rs"]
mod pages;
pub use pages::{
    PageBinding, PageBookState, PageEdit, PageSetup, PageState, PageSummary, PagesSnapshot,
};
// Session document: a stable layer identity and retained brush strokes.
// This is deliberately independent of pixels, AppKit, and the renderer.
#[path = "document_image_frames.rs"]
mod image_frames;
#[path = "object_lock.rs"]
mod object_lock;
#[path = "object_visibility.rs"]
mod object_visibility;
#[path = "transform_panel.rs"]
mod transform_panel;
use crate::selection::SelectionGesture;
pub use crate::selection::{
    Selection, SelectionMode, SelectionOperation, SelectionRegion, SelectionShape,
};
use crate::vector::{
    PathEditAction, PathOperation, PortablePathGeometry, VectorObject, VectorObjectKind,
    VectorPaint, VectorPath, VectorText,
};
pub use object_lock::ObjectLockAction;
pub use object_visibility::ObjectVisibilityAction;
use serde::{Deserialize, Serialize};
pub use transform_panel::{TransformPanelEdit, TransformPanelInfo};

pub const WIDTH: f32 = 960.0;
pub const HEIGHT: f32 = 640.0;
pub const MAX_DOCUMENT_DIMENSION: u32 = 8192;
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
    Grayscale,
    Lab,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorProfile {
    #[default]
    Srgb,
    DisplayP3,
    AdobeRgb1998,
    JapanColor2001Coated,
    GrayD65,
    LabD50,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentUnit {
    #[default]
    Pixels,
    Inches,
    Centimeters,
    Millimeters,
    Points,
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

#[derive(Clone, Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NewDocumentGuideLayout {
    Print {
        bleed_mm: f32,
    },
    Manga {
        trim_width_mm: f32,
        trim_height_mm: f32,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewDocumentSettings {
    #[serde(default)]
    pub guide_layout: Option<NewDocumentGuideLayout>,
    #[serde(default)]
    pub pages: Option<PageSetup>,
    pub document: DocumentSettings,
    pub color_mode: ColorMode,
    pub color_profile: ColorProfile,
    pub bit_depth: u8,
}

impl ColorProfile {
    fn mode(self) -> ColorMode {
        match self {
            Self::Srgb | Self::DisplayP3 | Self::AdobeRgb1998 => ColorMode::Rgb,
            Self::JapanColor2001Coated => ColorMode::Cmyk,
            Self::GrayD65 => ColorMode::Grayscale,
            Self::LabD50 => ColorMode::Lab,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Brush {
    #[serde(default = "default_hardness")]
    pub opacity: f32,
    #[serde(default = "default_hardness")]
    pub flow: f32,
    #[serde(default = "default_hardness")]
    pub alpha: f32,
    #[serde(default)]
    pub smoothing: f32,
    #[serde(default)]
    pub blend_mode: crate::brush_blend::BrushBlendMode,
    #[serde(default)]
    pub no_color: bool,
    #[serde(default)]
    pub envelope: BrushEnvelope,
    #[serde(default)]
    pub simulation: BrushSimulation,
    pub size: f32,
    #[serde(default = "default_hardness")]
    pub hardness: f32,
    pub color: [u8; 3],
}

/// Distance-based ADSR. Zero-length stages are skipped; no timing data is required.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrushEnvelope {
    pub enabled: bool,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub hold: f32,
    pub release: f32,
    pub dryness: f32,
}

impl Default for BrushEnvelope {
    fn default() -> Self {
        Self {
            enabled: false,
            attack: 20.0,
            decay: 100.0,
            sustain: 0.6,
            hold: 200.0,
            release: 200.0,
            dryness: 0.5,
        }
    }
}

impl BrushEnvelope {
    pub fn level(self, mut distance: f32) -> f32 {
        if !self.enabled {
            return 1.0;
        }
        distance = distance.max(0.0);
        if distance < self.attack {
            return distance / self.attack;
        }
        distance -= self.attack;
        if distance < self.decay {
            return 1.0 - (1.0 - self.sustain) * distance / self.decay;
        }
        distance -= self.decay;
        if distance < self.hold {
            return self.sustain;
        }
        distance -= self.hold;
        if distance < self.release {
            return self.sustain * (1.0 - distance / self.release);
        }
        0.0
    }

    fn validate(self) -> Result<(), String> {
        if [self.attack, self.decay, self.hold, self.release]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=10000.0).contains(v))
            || [self.sustain, self.dryness]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("Invalid brush envelope".into());
        }
        Ok(())
    }
}

const fn default_hardness() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrushSimulation {
    #[default]
    Round,
    Ink,
    Pencil,
    DryBrush,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            opacity: 1.,
            flow: 1.,
            alpha: 1.,
            smoothing: 0.,
            blend_mode: Default::default(),
            no_color: false,
            size: 16.0,
            envelope: BrushEnvelope::default(),
            simulation: BrushSimulation::Round,
            hardness: default_hardness(),
            color: [32, 32, 32],
        }
    }
}

impl Brush {
    pub fn needs_compositing(self) -> bool {
        self.opacity != 1.
            || self.alpha != 1.
            || self.blend_mode != crate::brush_blend::BrushBlendMode::Normal
    }
    pub fn validate(&self) -> Result<(), String> {
        self.envelope.validate()?;
        if [self.opacity, self.flow, self.alpha, self.smoothing]
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("Invalid brush opacity, flow, alpha or smoothing".into());
        }
        if !self.size.is_finite() || !(1.0..=MAX_BRUSH_SIZE).contains(&self.size) {
            return Err("Brush size must be between 1 and 512 px".into());
        }
        if !self.hardness.is_finite() || !(0.0..=1.0).contains(&self.hardness) {
            return Err("Brush hardness must be between 0 and 1".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

// Allow only floating-point roundoff from local/world coordinate conversion.
fn path_guide_color(id: &str) -> [u8; 4] {
    const COLORS: [[u8; 4]; 6] = [
        [48, 144, 255, 255],
        [255, 91, 104, 255],
        [65, 205, 92, 255],
        [183, 112, 255, 255],
        [244, 164, 48, 255],
        [40, 192, 202, 255],
    ];
    let index = id
        .rsplit('-')
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .map(|n| n.saturating_sub(1))
        .unwrap_or_else(|| {
            id.bytes().fold(0usize, |sum, b| {
                sum.wrapping_mul(31).wrapping_add(b as usize)
            })
        });
    COLORS[index % COLORS.len()]
}

fn coincident_path_endpoints(a: [f32; 2], b: [f32; 2]) -> bool {
    (0..2).all(|axis| {
        let tolerance = 1e-5_f32.max(a[axis].abs().max(b[axis].abs()) * f32::EPSILON * 4.);
        (a[axis] - b[axis]).abs() <= tolerance
    })
}

fn distance2(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    #[serde(default)]
    pub eraser: bool,
    #[serde(default)]
    pub clear: bool,
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

    /// Independent text frames can be rasterized once per object while retaining
    /// their order. Imported or mixed SVG keeps the full-layer compatibility path.
    pub fn text_frame_sources(&self, width: u32, height: u32) -> Option<Vec<(String, String)>> {
        if !self.vector_layer
            || self.vector_objects.is_empty()
            || self
                .vector_objects
                .iter()
                .any(|object| object.text.is_none())
            || self.source != vector_svg(width, height, &self.vector_objects)
        {
            return None;
        }
        Some(
            self.vector_objects
                .iter()
                .filter(|object| object.visible)
                .map(|object| {
                    (
                        object.id.clone(),
                        vector_svg(width, height, std::slice::from_ref(object)),
                    )
                })
                .collect(),
        )
    }
}

const fn default_layer_opacity() -> f32 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerObjectSnapshot {
    pub image_frame: Option<crate::image_frame::ImageFrameSummary>,
    pub locked: bool,
    pub opacity: f32,
    pub blend_mode: String,
    pub fill_gradient: Option<crate::gradient::Gradient>,
    pub stroke_gradient: Option<crate::gradient::Gradient>,
    pub fill_color: Option<[u8; 4]>,
    pub stroke_color: Option<[u8; 4]>,
    pub stroke_width: f32,
    pub stroke_style: crate::stroke::StrokeStyle,
    pub stroke_contours: Vec<bool>,
    pub id: String,
    pub name: String,
    pub group_path: Vec<String>,
    pub clipping_mask: bool,
    pub kind: VectorObjectKind,
    pub visible: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effects: Option<crate::layer_effects::LayerEffects>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raster_blend_mode: Option<crate::tiles::RasterBlendMode>,
    pub guide_color: [u8; 4],
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
    pub saved_vector_selections: Vec<SavedVectorSelectionSummary>,
    pub can_reselect_vectors: bool,
    pub compound_shapes: Vec<CompoundShapeSnapshot>,
    pub layer_groups: LayerGroupsSnapshot,
    pub guides: GuidesSnapshot,
    pub pages: PagesSnapshot,
    pub has_hidden_objects: bool,
    pub has_locked_objects: bool,
    pub selected_bounds: Option<[f32; 4]>,
    pub transform_panel: Option<TransformPanelInfo>,
    pub active_saved_path: Option<String>,
    pub saved_paths: Vec<SavedPathSnapshot>,
    pub selection: Option<Selection>,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub unit: DocumentUnit,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raster_resolution: Option<crate::tiles::RasterResolution>,
    pub resolution: u32,
    pub artboards: bool,
    pub canvas_color: CanvasColor,
    pub pixel_aspect_ratio: f32,
    pub layer_id: String,
    pub editing_channel: u32,
    pub layer_edit_target: crate::layer_mask::LayerEditTarget,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedPath {
    pub id: String,
    pub name: String,
    pub objects: Vec<VectorObject>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPathSnapshot {
    pub guide_color: [u8; 4],
    pub id: String,
    pub name: String,
    pub components: usize,
    pub clipping: bool,
}

#[derive(Clone, Copy)]
enum HistoryKind {
    Page,
    Stroke,
    Vector,
}

#[derive(Clone, Serialize, Deserialize)]
struct PathEditing {
    object_ids: Vec<(String, String)>,
    id: String,
    layer_id: String,
    previous_layer: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct VectorHistoryState {
    paint_bucket_settings: crate::paint_bucket::Settings,
    animation: std::sync::Arc<animation::Animation>,
    crop_dimensions: Option<(u32, u32)>,
    compound_shapes: Vec<CompoundShape>,
    layer_groups: LayerGroupsState,
    guides: GuidesState,
    background_visible: bool,
    locked_objects: std::collections::BTreeSet<String>,
    locked_artwork_layers: std::collections::BTreeSet<String>,
    background_locked: bool,
    path_editing: Option<PathEditing>,
    saved_paths: Vec<SavedPath>,
    clipping_path_id: Option<String>,
    selected_layer: Option<String>,
    layers: Vec<SvgLayer>,
    selection: Vec<String>,
    strokes: Option<Vec<Stroke>>,
    layer_effects: std::collections::BTreeMap<String, crate::layer_effects::LayerEffects>,
    paint_source: Option<String>,
    pixel_selection: Option<Selection>,
}

/// Diagnostic counters for the last successful translation or its Undo/Redo.
/// Byte counts cover retained transform entries and source patch movement, not
/// allocator overhead, validation temporaries or process-wide memory.
#[derive(Clone, Copy, Debug, Default)]
pub struct TranslationMetrics {
    pub elapsed_us: u64,
    pub journal_us: u64,
    pub edited_objects: usize,
    pub scanned_objects: usize,
    pub history_transform_bytes: usize,
    pub full_layer_snapshots: usize,
    pub svg_generations: usize,
    pub svg_patch_bytes: usize,
    pub svg_source_search_bytes: usize,
    pub estimated_source_relocation_bytes: usize,
}

/// Ordinary translations retain only the edited objects. Other edit kinds keep
/// the existing complete-state contract until their delta transactions migrate.
#[derive(Clone, Serialize, Deserialize)]
enum VectorHistoryEntry {
    #[serde(skip)]
    Stored(crate::history_storage::StoredHistory),
    PaintBucket(crate::paint_bucket::Settings),
    Animation(std::sync::Arc<animation::Animation>),
    SavedSelections(std::sync::Arc<Vec<SavedVectorSelection>>),
    PixelSelection(Option<Selection>),
    State(Box<VectorHistoryState>),
    Objects {
        edits: Vec<ObjectHistoryEntry>,
        selection: Vec<String>,
        selected_layer: Option<String>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
struct ObjectHistoryEntry {
    layer: usize,
    position: usize,
    transform: [f32; 6],
    bounds: Option<[f64; 4]>,
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
    effects_key: u64,
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
            && self.effects_key == next.effects_key
    }

    pub fn same_effects(self, next: Self) -> bool {
        self.effects_key == next.effects_key
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
    paint_bucket_settings: crate::paint_bucket::Settings,
    animation: std::sync::Arc<animation::Animation>,
    saved_vector_selections: std::sync::Arc<Vec<SavedVectorSelection>>,
    previous_vector_selection: Vec<String>,
    compound_shapes: Vec<CompoundShape>,
    scene_journal: crate::scene::Journal,
    scene_picking: crate::scene::picking::PickingState,
    layer_groups: LayerGroupsState,
    guides: GuidesState,
    locked_objects: std::collections::BTreeSet<String>,
    locked_artwork_layers: std::collections::BTreeSet<String>,
    svg_geometry_backend: Option<std::sync::Arc<dyn crate::svg_backend::SvgGeometryBackend>>,
    path_editing: Option<PathEditing>,
    saved_paths: Vec<SavedPath>,
    clipping_path_id: Option<String>,
    selection: Option<Selection>,
    selection_anchor: Option<SelectionGesture>,
    name: String,
    width: u32,
    height: u32,
    unit: DocumentUnit,
    resolution: u32,
    artboards: bool,
    pages: Option<pages::PageBook>,
    page_undo: Vec<pages::PageHistory>,
    page_redo: Vec<pages::PageHistory>,
    canvas_color: CanvasColor,
    pixel_aspect_ratio: f32,
    layer_effects: std::collections::BTreeMap<String, crate::layer_effects::LayerEffects>,
    paint_source: Option<String>,
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
    editing_channel: u32,
    layer_edit_selection: Option<(String, crate::layer_mask::LayerEditTarget)>,
    selected_vector_objects: Vec<String>,
    vector_undo: Vec<VectorHistoryEntry>,
    vector_redo: Vec<VectorHistoryEntry>,
    undo_order: Vec<HistoryKind>,
    redo_order: Vec<HistoryKind>,
    color_mode: ColorMode,
    color_profile: ColorProfile,
    bit_depth: u8,
    revision: u64,
    point_count: usize,
    saved_revision: u64,
    text_change_generation: u64,
    translation_metrics: TranslationMetrics,
    noncanonical_vector_sources: std::collections::BTreeSet<String>,
    transform_slots: TransformSlotCache,
    file_name: Option<String>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            paint_bucket_settings: Default::default(),
            animation: Default::default(),
            saved_vector_selections: std::sync::Arc::new(Vec::new()),
            previous_vector_selection: Vec::new(),
            compound_shapes: Vec::new(),
            scene_journal: crate::scene::Journal::default(),
            scene_picking: crate::scene::picking::PickingState::default(),
            layer_groups: LayerGroupsState::default(),
            guides: GuidesState::default(),
            locked_objects: Default::default(),
            locked_artwork_layers: Default::default(),
            svg_geometry_backend: None,
            path_editing: None,
            saved_paths: Vec::new(),
            clipping_path_id: None,
            selection: None,
            selection_anchor: None,
            name: "Untitled-1".into(),
            width: WIDTH as u32,
            height: HEIGHT as u32,
            unit: DocumentUnit::Pixels,
            resolution: 72,
            artboards: false,
            pages: None,
            page_undo: Vec::new(),
            page_redo: Vec::new(),
            canvas_color: CanvasColor::White,
            pixel_aspect_ratio: 1.0,
            layer_effects: Default::default(),
            paint_source: None,
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
            editing_channel: 0,
            layer_edit_selection: None,
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
            text_change_generation: 0,
            translation_metrics: TranslationMetrics::default(),
            noncanonical_vector_sources: Default::default(),
            transform_slots: TransformSlotCache::default(),
            file_name: None,
        }
    }
}

impl Document {
    /// Independent current-page artwork for transient rendering and sampling.
    /// Omits Undo/Redo, inactive pages, picking/transform caches and change logs.
    /// This is not a project backup: use the original document for saving/editing.
    pub fn clone_for_rendering(&self) -> Self {
        Self {
            paint_bucket_settings: self.paint_bucket_settings.clone(),
            animation: self.animation.clone(),
            compound_shapes: self.compound_shapes.clone(),
            layer_groups: self.layer_groups.clone(),
            guides: self.guides.clone(),
            locked_objects: self.locked_objects.clone(),
            locked_artwork_layers: self.locked_artwork_layers.clone(),
            svg_geometry_backend: self.svg_geometry_backend.clone(),
            path_editing: self.path_editing.clone(),
            saved_paths: self.saved_paths.clone(),
            clipping_path_id: self.clipping_path_id.clone(),
            selection: self.selection.clone(),
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            unit: self.unit,
            resolution: self.resolution,
            artboards: self.artboards,
            canvas_color: self.canvas_color,
            pixel_aspect_ratio: self.pixel_aspect_ratio,
            layer_effects: self.layer_effects.clone(),
            paint_source: self.paint_source.clone(),
            strokes: self.strokes.clone(),
            active: self.active.clone(),
            visible: self.visible,
            layer_name: self.layer_name.clone(),
            layer_opacity: self.layer_opacity,
            layer_locked: self.layer_locked,
            layer_alpha_locked: self.layer_alpha_locked,
            layer_mask_enabled: self.layer_mask_enabled,
            layer_mask_inverted: self.layer_mask_inverted,
            layer_mask_density: self.layer_mask_density,
            svg_layers: self.svg_layers.clone(),
            selected_layer: self.selected_layer.clone(),
            editing_channel: self.editing_channel,
            layer_edit_selection: self.layer_edit_selection.clone(),
            selected_vector_objects: self.selected_vector_objects.clone(),
            color_mode: self.color_mode,
            color_profile: self.color_profile,
            bit_depth: self.bit_depth,
            revision: self.revision,
            point_count: self.point_count,
            saved_revision: self.saved_revision,
            text_change_generation: self.text_change_generation,
            noncanonical_vector_sources: self.noncanonical_vector_sources.clone(),
            file_name: self.file_name.clone(),
            ..Self::default()
        }
    }
    /// Attach or detach the optional SVG runtime adapter. This is not saved or added to history.
    pub fn set_svg_geometry_backend(
        &mut self,
        backend: Option<std::sync::Arc<dyn crate::svg_backend::SvgGeometryBackend>>,
    ) {
        self.svg_geometry_backend = backend;
    }
    pub fn has_svg_geometry_backend(&self) -> bool {
        self.svg_geometry_backend.is_some()
    }
    fn imported_svg_objects(&self, source: &str, layer_id: &str) -> Vec<VectorObject> {
        self.svg_geometry_backend
            .as_ref()
            .map_or_else(Vec::new, |backend| {
                backend.objects(source, layer_id, [self.width as f32, self.height as f32])
            })
    }

    /// Pixel dimensions without allocating a UI snapshot or cloning text/layer metadata.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn canvas_color(&self) -> CanvasColor {
        self.canvas_color
    }

    pub fn scene_journal(&self) -> &crate::scene::Journal {
        &self.scene_journal
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Lightweight edit target; does not build object, text or page UI summaries.
    pub fn selected_layer_id(&self) -> &str {
        self.selected_layer
            .as_deref()
            .filter(|id| {
                *id == "layer-1"
                    || self.svg_layers.iter().any(|layer| layer.id == *id)
                    || self.layer_groups.groups.iter().any(|group| group.id == *id)
            })
            .unwrap_or("layer-1")
    }

    pub fn selected_layer_alpha_locked(&self) -> bool {
        let id = self.selected_layer_id();
        if id == "layer-1" {
            self.layer_alpha_locked
        } else {
            self.svg_layers
                .iter()
                .find(|layer| layer.id == id)
                .is_some_and(|layer| layer.alpha_locked)
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision || self.active.is_some()
    }

    /// Current visible brush input without scanning committed stroke history.
    pub fn active_paint_stroke(&self) -> Option<&Stroke> {
        self.active.as_ref().filter(|_| self.visible)
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
            effects_key: {
                use std::hash::Hasher;
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                let neutral = crate::layer_effects::LayerEffects::default();
                let effects = self.layer_effects.get("layer-1").unwrap_or(&neutral);
                hash.write_u8(u8::from(effects.enabled));
                if let Some(mask) = &effects.mask {
                    mask.hash_state(&mut hash);
                }
                for smooth in effects.curve_smooth {
                    hash.write_u8(u8::from(smooth));
                }
                for value in effects
                    .values
                    .iter()
                    .chain(effects.mixer.iter().flatten())
                    .chain(effects.grading.iter().flatten())
                    .chain(effects.curves.iter().flatten().flatten())
                    .chain([effects.grading_blend, effects.grading_balance].iter())
                {
                    hash.write_u32(value.to_bits());
                }
                hash.finish()
            },
        }
    }

    pub fn snapshot(&self) -> DocumentSnapshot {
        let mut layers = vec![LayerSnapshot {
            effects: Some(
                self.layer_effects
                    .get("layer-1")
                    .map(|e| e.snapshot())
                    .unwrap_or_default(),
            ),
            raster_blend_mode: None,
            guide_color: self.layer_guide_color("layer-1"),
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
        layers.extend(
            self.svg_layers
                .iter()
                .filter(|layer| !self.is_path_edit_layer(layer))
                .map(|layer| {
                    let imported = if !layer.vector_layer
                        && self
                            .selected_vector_objects
                            .iter()
                            .any(|id| id.starts_with(&format!("{}::", layer.id)))
                    {
                        self.imported_svg_objects(&layer.source, &layer.id)
                            .into_iter()
                            .filter(|o| self.selected_vector_objects.contains(&o.id))
                            .collect::<Vec<_>>()
                    } else {
                        Vec::new()
                    };
                    LayerSnapshot {
                        effects: Some(
                            self.layer_effects
                                .get(&layer.id)
                                .map(|e| e.snapshot())
                                .unwrap_or_default(),
                        ),
                        raster_blend_mode: None,
                        guide_color: self.layer_guide_color(&layer.id),
                        objects: layer
                            .vector_objects
                            .iter()
                            .chain(imported.iter())
                            .map(|object| LayerObjectSnapshot {
                                image_frame: object.image_frame.as_ref().map(|f| f.summary()),
                                locked: self.object_is_locked(&object.id),
                                opacity: object.opacity,
                                blend_mode: object.blend_mode.clone(),
                                fill_gradient: object.fill_gradient.clone(),
                                stroke_gradient: object.stroke_gradient.clone(),
                                fill_color: object.fill.map(|p| p.color),
                                stroke_color: object.stroke.map(|p| p.color),
                                stroke_style: object.stroke_style.clone(),
                                stroke_contours: crate::stroke::contour_closed(&object.path.data),
                                stroke_width: if object.stroke.is_some() {
                                    object.stroke_width
                                } else {
                                    0.0
                                },
                                id: object.id.clone(),
                                name: object.name.clone(),
                                group_path: object.group_path.clone(),
                                clipping_mask: object.clipping_group.is_some(),
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
                        visible: self
                            .layer_groups
                            .members
                            .iter()
                            .find(|m| m.id == layer.id)
                            .map_or(layer.visible, |m| m.visible),
                        opacity: self
                            .layer_groups
                            .members
                            .iter()
                            .find(|m| m.id == layer.id)
                            .and_then(|m| m.opacity)
                            .unwrap_or(layer.opacity),
                        locked: self
                            .layer_groups
                            .members
                            .iter()
                            .find(|m| m.id == layer.id)
                            .map_or(layer.locked, |m| m.locked),
                        alpha_locked: layer.alpha_locked,
                        mask_enabled: layer.mask_enabled,
                        mask_inverted: layer.mask_inverted,
                        mask_density: layer.mask_density,
                        deletable: true,
                        stroke_count: 0,
                    }
                }),
        );
        DocumentSnapshot {
            saved_vector_selections: self
                .saved_vector_selections
                .iter()
                .map(|s| SavedVectorSelectionSummary {
                    name: s.name.clone(),
                    count: s.object_ids.len(),
                })
                .collect(),
            can_reselect_vectors: !self.previous_vector_selection.is_empty(),
            compound_shapes: self.compound_shape_snapshot(),
            layer_groups: self.layer_groups_snapshot(),
            guides: self.guides.snapshot(),
            has_locked_objects: self.has_locked_objects(),
            has_hidden_objects: self.has_hidden_objects(),
            selected_bounds: (self.layer_edit_target()
                == crate::layer_mask::LayerEditTarget::Content)
                .then(|| self.selected_vector_bounds())
                .flatten(),
            transform_panel: (self.layer_edit_target()
                == crate::layer_mask::LayerEditTarget::Content)
                .then(|| self.transform_panel_info())
                .flatten(),
            active_saved_path: self.path_editing.as_ref().map(|edit| edit.id.clone()),
            saved_paths: self
                .saved_paths
                .iter()
                .map(|p| SavedPathSnapshot {
                    guide_color: path_guide_color(&p.id),
                    id: p.id.clone(),
                    name: p.name.clone(),
                    components: p.objects.len(),
                    clipping: self.clipping_path_id.as_ref() == Some(&p.id),
                })
                .collect(),
            selection: self.selection.clone(),
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            unit: self.unit,
            raster_resolution: None,
            resolution: self.resolution,
            artboards: self.artboards,
            pages: self.pages_snapshot(),
            canvas_color: self.canvas_color,
            pixel_aspect_ratio: self.pixel_aspect_ratio,
            layer_id: self.selected_layer_id().to_owned(),
            layer_edit_target: self.layer_edit_target(),
            editing_channel: self.editing_channel,
            layer_visible: self
                .selected_layer
                .as_ref()
                .and_then(|id| self.layer_groups.groups.iter().find(|g| &g.id == id))
                .map_or(self.visible, |g| g.visible),
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
                                editable: !layer.locked
                                    && layer.visible
                                    && object.visible
                                    && !self.object_is_locked(&object.id),
                            }
                        })
                    })
                })
                .collect(),
            selected_vector_objects: self.selected_vector_ids().to_vec(),
            can_undo: !self.undo_order.is_empty(),
            can_redo: !self.redo_order.is_empty(),
            revision: self.revision,
            dirty: self.is_dirty(),
            file_name: self.file_name.clone(),
        }
    }
    /// Owned, engine-independent state. Runtime services, selections and history are excluded.
    /// Callers that need an in-progress stroke should finish a clone before taking this state.
    pub fn document_state(&self) -> DocumentState {
        let mut layer_groups = self.layer_groups.clone();
        layer_groups.reconcile(&self.svg_layers);
        if let Some(edit) = &self.path_editing {
            layer_groups.roots.retain(|id| id != &edit.layer_id);
        }
        DocumentState {
            paint_bucket_settings: self.paint_bucket_settings(),
            animation: self.animation.as_ref().clone(),
            saved_vector_selections: self.saved_vector_selections.as_ref().clone(),
            pixel_selection: self.selection.clone(),
            compound_shapes: self.compound_shapes.clone(),
            layer_groups,
            guides: self.guides.clone(),
            locked_objects: self.locked_objects.clone(),
            locked_artwork_layers: self.locked_artwork_layers.clone(),
            saved_paths: self.saved_paths.clone(),
            clipping_path_id: self.clipping_path_id.clone(),
            name: Some(self.name.clone()),
            width: self.width,
            height: self.height,
            unit: Some(self.unit),
            resolution: Some(self.resolution),
            artboards: Some(self.artboards),
            pages: self.pages_state(),
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
            layer_effects: self
                .layer_effects
                .iter()
                .filter(|(id, _)| {
                    id.as_str() == "layer-1" || self.svg_layers.iter().any(|l| &l.id == *id)
                })
                .map(|(id, e)| (id.clone(), e.clone()))
                .collect(),
            paint_source: self.paint_source.clone(),
            strokes: self.strokes.clone(),
            svg_layers: self
                .svg_layers
                .iter()
                .filter(|layer| !self.is_path_edit_layer(layer))
                .cloned()
                .collect(),
        }
    }

    /// Validate and restore model state; file signatures and codecs belong to the I/O layer.
    pub fn from_document_state(mut file: DocumentState) -> Result<Self, String> {
        file.animation.validate(file.width, file.height)?;
        if let Some(selection) = &file.pixel_selection {
            selection.validate()?;
        }
        if file.width == 0
            || file.height == 0
            || file.width > MAX_DOCUMENT_DIMENSION
            || file.height > MAX_DOCUMENT_DIMENSION
        {
            return Err("Unsupported document dimensions".into());
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
            ColorMode::Grayscale => ColorProfile::GrayD65,
            ColorMode::Lab => ColorProfile::LabD50,
        });
        if color_profile.mode() != file.color_mode {
            return Err("Color profile is incompatible with the document color mode".into());
        }
        let mut count = 0;
        for stroke in &file.strokes {
            stroke.brush.validate()?;
            if stroke.clear && (!stroke.eraser || stroke.selection.is_none()) {
                return Err("Invalid clear stroke".into());
            }
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
        if file.layer_effects.len() > MAX_SVG_LAYERS + 1 {
            return Err("Too many layer effects".into());
        }
        for (id, effects) in &file.layer_effects {
            effects.validate()?;
            if id != "layer-1" && !file.svg_layers.iter().any(|l| &l.id == id) {
                return Err("Unknown effect layer".into());
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
        if file
            .paint_source
            .as_ref()
            .is_some_and(|s| s.len() > MAX_SVG_BYTES || !s.contains("<svg"))
        {
            return Err("Invalid paint image".into());
        }
        file.paint_bucket_settings.validate()?;
        if file
            .paint_bucket_settings
            .reference_layers
            .iter()
            .any(|id| id != "layer-1" && !file.svg_layers.iter().any(|layer| &layer.id == id))
        {
            return Err("Invalid paint bucket reference layers".into());
        }
        file.layer_groups.validate(&file.svg_layers)?;
        file.guides.validate()?;
        file.guides.selected.clear();
        validate_saved_paths(&file.saved_paths, file.clipping_path_id.as_deref())?;
        if file.locked_objects.len() > 65536
            || file.locked_artwork_layers.len() > 17
            || file
                .locked_objects
                .iter()
                .chain(&file.locked_artwork_layers)
                .any(|id| id.is_empty() || id.len() > 256)
        {
            return Err("Invalid object locks".into());
        }
        let pages = file
            .pages
            .clone()
            .map(pages::PageBook::from_state)
            .transpose()?;
        vector_selection::validate_saved(&file.saved_vector_selections)?;
        compound_shapes::validate_compound_shapes(&file.compound_shapes, &file.svg_layers)?;
        let noncanonical_vector_sources = file
            .svg_layers
            .iter()
            .filter(|l| {
                l.vector_layer && l.source != vector_svg(file.width, file.height, &l.vector_objects)
            })
            .map(|l| l.id.clone())
            .collect();
        let stroke_count = file.strokes.len();
        Ok(Self {
            paint_bucket_settings: file.paint_bucket_settings,
            animation: std::sync::Arc::new(file.animation),
            saved_vector_selections: std::sync::Arc::new(file.saved_vector_selections),
            previous_vector_selection: Vec::new(),
            selection: file.pixel_selection,
            compound_shapes: file.compound_shapes,
            layer_groups: file.layer_groups,
            guides: file.guides,
            locked_objects: file.locked_objects,
            locked_artwork_layers: file.locked_artwork_layers,
            path_editing: None,
            saved_paths: file.saved_paths,
            clipping_path_id: file.clipping_path_id,
            name: file.name.unwrap_or_else(|| "Untitled-1".into()),
            width: file.width,
            height: file.height,
            unit: file.unit.unwrap_or_default(),
            resolution: file.resolution.unwrap_or(72),
            artboards: file.artboards.unwrap_or(false),
            pages,
            page_undo: Vec::new(),
            page_redo: Vec::new(),
            canvas_color: file.canvas_color.unwrap_or_default(),
            pixel_aspect_ratio: file.pixel_aspect_ratio.unwrap_or(1.0),
            layer_effects: file.layer_effects,
            paint_source: file.paint_source,
            strokes: file.strokes,
            svg_layers: file.svg_layers,
            noncanonical_vector_sources,
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
    pub fn color_mode(&self) -> ColorMode {
        self.color_mode
    }

    pub fn set_color_mode(&mut self, mode: ColorMode) {
        self.finish();
        if self.color_mode != mode {
            self.color_mode = mode;
            self.editing_channel = 0;
            self.color_profile = match mode {
                ColorMode::Rgb => ColorProfile::Srgb,
                ColorMode::Cmyk => ColorProfile::JapanColor2001Coated,
                ColorMode::Grayscale => ColorProfile::GrayD65,
                ColorMode::Lab => ColorProfile::LabD50,
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
    /// Validate the complete preset before installing it in a workspace.
    pub fn from_preset(settings: NewDocumentSettings) -> Result<Self, String> {
        let mut document = Self::default();
        document.set_document_settings(settings.document)?;
        document.set_color_mode(settings.color_mode);
        document.set_color_profile(settings.color_profile)?;
        document.set_bit_depth(settings.bit_depth)?;
        document.select_layer("layer-1".into())?;
        if let Some(layout) = settings.guide_layout {
            let factor = document.resolution as f32 / 25.4;
            let (w, h) = (document.width as f32, document.height as f32);
            let (x, y, right, bottom) = match layout {
                NewDocumentGuideLayout::Print { bleed_mm } => {
                    if !bleed_mm.is_finite() || !(0.0..=100.0).contains(&bleed_mm) {
                        return Err("Invalid bleed size".into());
                    }
                    let bleed = bleed_mm * factor;
                    (-bleed, -bleed, w + bleed, h + bleed)
                }
                NewDocumentGuideLayout::Manga {
                    trim_width_mm,
                    trim_height_mm,
                } => {
                    if [trim_width_mm, trim_height_mm]
                        .iter()
                        .any(|v| !v.is_finite() || *v <= 0.)
                    {
                        return Err("Invalid manga trim size".into());
                    }
                    let (tw, th) = if w > h {
                        (
                            trim_width_mm.max(trim_height_mm),
                            trim_width_mm.min(trim_height_mm),
                        )
                    } else {
                        (
                            trim_width_mm.min(trim_height_mm),
                            trim_width_mm.max(trim_height_mm),
                        )
                    };
                    let (tw, th) = (tw * factor, th * factor);
                    if tw > w + 0.5 || th > h + 0.5 {
                        return Err("Manga trim size exceeds paper size".into());
                    }
                    ((w - tw) / 2., (h - th) / 2., (w + tw) / 2., (h + th) / 2.)
                }
            };
            for (axis, position) in [
                ("vertical", x),
                ("vertical", right),
                ("horizontal", y),
                ("horizontal", bottom),
            ] {
                document.guides.items.push(Guide {
                    id: format!("guide-{}", document.guides.next_id),
                    axis: Some(axis.into()),
                    position,
                    layer_id: None,
                    objects: vec![],
                });
                document.guides.next_id += 1;
            }
            document.guides.validate()?;
        }
        if let Some(pages) = settings.pages {
            document.setup_pages(pages)?;
        }
        Ok(document)
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
        if brush.no_color {
            return Ok(false);
        }
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
            eraser: false,
            clear: false,
            brush,
            points: vec![point],
            pressures: vec![pressure],
            selection: self.selection.clone(),
        });
        Ok(true)
    }
    pub fn begin_eraser(
        &mut self,
        point: Point,
        brush: Brush,
        pressure: f32,
    ) -> Result<bool, String> {
        if self.layer_alpha_locked {
            return Err("透明ピクセル保護を解除してください。\nDisable alpha lock to erase.\n请关闭透明像素锁定。".into());
        }
        let started = self.begin_with_pressure(
            point,
            Brush {
                no_color: false,
                ..brush
            },
            pressure,
        )?;
        if started {
            if let Some(stroke) = &mut self.active {
                stroke.eraser = true;
            }
        }
        Ok(started)
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
    pub fn restore_history(&mut self, redo: bool) -> Result<(), String> {
        let kind = if redo {
            self.redo_order.last()
        } else {
            self.undo_order.last()
        };
        if !matches!(kind, Some(HistoryKind::Vector)) {
            return Ok(());
        }
        let history = if redo {
            &mut self.vector_redo
        } else {
            &mut self.vector_undo
        };
        if let Some(VectorHistoryEntry::Stored(blob)) = history.last() {
            let restored =
                serde_json::from_slice(&blob.0.read()?).map_err(|error| error.to_string())?;
            *history.last_mut().unwrap() = restored;
        }
        Ok(())
    }
    pub fn manage_history(
        &mut self,
        store: &dyn crate::history_storage::HistoryStore,
        budget: usize,
        states: usize,
    ) -> Result<usize, String> {
        while self.undo_order.len() + self.redo_order.len() > states && !self.undo_order.is_empty()
        {
            match self.undo_order.remove(0) {
                HistoryKind::Vector => {
                    self.vector_undo.remove(0);
                }
                HistoryKind::Page => {
                    self.page_undo.remove(0);
                }
                HistoryKind::Stroke => {} // Committed strokes are artwork, not discardable history.
            }
        }
        while self.undo_order.len() + self.redo_order.len() > states {
            match self.redo_order.remove(0) {
                HistoryKind::Vector => {
                    self.vector_redo.remove(0);
                }
                HistoryKind::Page => {
                    self.page_redo.remove(0);
                }
                HistoryKind::Stroke => {
                    self.redo.remove(0);
                }
            }
        }
        let mut retained = self
            .vector_undo
            .iter()
            .chain(&self.vector_redo)
            .map(history_bytes)
            .sum::<usize>();
        for history in [&mut self.vector_undo, &mut self.vector_redo] {
            let cold = history.len().saturating_sub(2);
            for entry in history.iter_mut().take(cold) {
                if retained <= budget {
                    break;
                }
                let size = history_bytes(entry);
                if size == 0 {
                    continue;
                }
                let bytes = serde_json::to_vec(entry).map_err(|error| error.to_string())?;
                let blob = store.write(&bytes)?; // Never replace an entry until the write succeeds.
                *entry = VectorHistoryEntry::Stored(blob);
                retained = retained.saturating_sub(size);
            }
        }
        Ok(retained)
    }

    pub fn undo(&mut self) {
        if self.restore_history(false).is_err() {
            return;
        }
        self.finish();
        match self.undo_order.pop() {
            Some(HistoryKind::Page) => self.undo_page(),
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
                    let current = self.exchange_vector_history(previous);
                    self.vector_redo.push(current);
                    self.redo_order.push(HistoryKind::Vector);
                    self.revision += 1;
                }
            }
            None => {}
        }
    }
    pub fn redo(&mut self) {
        if self.restore_history(true).is_err() {
            return;
        }
        self.finish();
        match self.redo_order.pop() {
            Some(HistoryKind::Page) => self.redo_page(),
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
                    let current = self.exchange_vector_history(next);
                    self.vector_undo.push(current);
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
        let before = self.svg_layers.clone();
        if let Some(member) = self.layer_groups.members.iter_mut().find(|m| m.id == id) {
            member.visible = !member.visible;
            self.sync_layer_group_flags();
            self.scene_journal.layers_changed(&before, &self.svg_layers);
            self.revision += 1;
            return Ok(());
        }
        if id == "layer-1" {
            self.visible = !self.visible;
        } else if let Some(layer) = self.svg_layers.iter_mut().find(|layer| layer.id == id) {
            layer.visible = !layer.visible;
        } else {
            return Err("Layer not found".into());
        }
        self.scene_journal.layers_changed(&before, &self.svg_layers);
        self.revision += 1;
        Ok(())
    }
    pub fn layer_effects(&self, id: &str) -> crate::layer_effects::LayerEffects {
        self.layer_effects.get(id).cloned().unwrap_or_default()
    }
    pub fn has_layer_effects(&self, id: &str) -> bool {
        self.layer_effects.get(id).is_some_and(|e| e.active())
    }
    pub fn set_layer_effects(
        &mut self,
        id: &str,
        mut effects: crate::layer_effects::LayerEffects,
    ) -> Result<(), String> {
        effects.retain_mask_content(&self.layer_effects(id))?;
        effects.validate()?;
        let locked = if id == "layer-1" {
            self.layer_locked
        } else {
            self.svg_layers
                .iter()
                .find(|l| l.id == id)
                .ok_or("Layer not found")?
                .locked
        };
        if self.layer_effects(id) == effects {
            return Ok(());
        }
        if locked {
            return Err("Layer is locked".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.layer_effects.insert(id.into(), effects);
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    pub fn set_layer_settings(&mut self, settings: LayerSettings) -> Result<(), String> {
        self.finish();
        let before = self.svg_layers.clone();
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
        let was_locked = if settings.id == "layer-1" {
            self.layer_locked
        } else {
            self.svg_layers
                .iter()
                .find(|layer| layer.id == settings.id)
                .ok_or("Layer not found")?
                .locked
        };
        if was_locked != settings.locked {
            self.locked_artwork_layers.remove(&settings.id);
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
        if let Some(member) = self
            .layer_groups
            .members
            .iter_mut()
            .find(|m| m.id == settings.id)
        {
            member.locked = settings.locked;
            member.opacity = Some(settings.opacity);
            self.sync_layer_group_flags();
        }
        self.scene_journal.layers_changed(&before, &self.svg_layers);
        self.revision += 1;
        Ok(())
    }
    pub fn cut_paint_selection(&mut self) -> Result<(), String> {
        if self.selected_layer.as_deref().unwrap_or("layer-1") != "layer-1"
            || self.layer_locked
            || self.layer_alpha_locked
            || !self.visible
        {
            return Err("Unlock and show the paint layer first / ピクセルレイヤーのロックを解除してください / 请先解锁并显示像素图层".into());
        }
        let selection = self
            .selection
            .clone()
            .ok_or("Select a pixel area first / 範囲を選択してください / 请先选择区域")?;
        self.finish();
        if self.point_count >= MAX_POINTS {
            return Err("Session stroke limit reached".into());
        }
        self.active = Some(Stroke {
            eraser: true,
            clear: true,
            brush: Brush::default(),
            points: vec![Point { x: 0., y: 0. }],
            pressures: vec![1.],
            selection: Some(selection),
        });
        self.finish();
        Ok(())
    }

    pub fn paint_source(&self) -> Option<&str> {
        self.paint_source.as_deref()
    }
    /// Isolated paint workspace for an additional pixel layer. The caller commits
    /// the resulting image atomically, so previews never alter project history.
    pub fn selected_pixel_paint_workspace(&self) -> Result<Option<Self>, String> {
        self.pixel_paint_workspace(false)
    }
    fn pixel_paint_workspace(&self, preserve_alpha: bool) -> Result<Option<Self>, String> {
        let id = self.selected_layer.as_deref().unwrap_or("layer-1");
        if id == "layer-1" {
            return Ok(None);
        }
        let layer = self
            .svg_layers
            .iter()
            .find(|layer| layer.id == id)
            .ok_or("Layer not found")?;
        if layer.vector_layer || !layer.paint_layer {
            return Err(
                "ピクセルレイヤーを選択してください。\nSelect a pixel layer.\n请选择像素图层。"
                    .into(),
            );
        }
        if layer.locked || !layer.visible || (layer.alpha_locked && !preserve_alpha) {
            return Err("ピクセルレイヤーを表示し、ロックと透明ピクセル保護を解除してください。\nShow the pixel layer and disable layer and alpha locks.\n请显示像素图层并关闭图层锁定和透明像素锁定。".into());
        }
        Ok(Some(Self {
            width: self.width,
            height: self.height,
            paint_source: (layer.source != empty_vector_svg(self.width, self.height)
                && layer.source
                    != r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#)
                .then(|| layer.source.clone()),
            selection: self.selection.clone(),
            canvas_color: CanvasColor::Transparent,
            ..Self::default()
        }))
    }

    pub fn selected_layer_is_vector(&self) -> bool {
        self.svg_layers.iter().any(|layer| {
            layer.vector_layer
                && (Some(&layer.id) == self.selected_layer.as_ref()
                    || (self.selected_layer.is_none()
                        && layer.visible
                        && !layer.locked
                        && !layer.vector_objects.is_empty()))
        })
    }
    pub fn translate_pixel_selection(&mut self, selection: Option<Selection>, dx: f32, dy: f32) {
        self.selection = selection.map(|s| s.translated(dx, dy));
    }
    /// Move a whole image layer without decompressing/re-encoding its pixels.
    pub fn translated_pixel_layer_source(
        &self,
        dx: f32,
        dy: f32,
        copy: bool,
    ) -> Result<Option<String>, String> {
        if !dx.is_finite() || !dy.is_finite() || dx.abs() > 100_000. || dy.abs() > 100_000. {
            return Err("Invalid image translation".into());
        }
        let Some(layer) = self
            .svg_layers
            .iter()
            .find(|l| Some(&l.id) == self.selected_layer.as_ref())
        else {
            return Ok(None);
        };
        if layer.vector_layer {
            return Ok(None);
        }
        if layer.locked || layer.alpha_locked || !layer.visible {
            return Err("Unlock and show the pixel layer / ピクセルレイヤーのロックを解除し表示してください / 请解锁并显示像素图层".into());
        }
        let source = layer
            .source
            .find("<svg")
            .map(|i| &layer.source[i..])
            .ok_or("Invalid image source")?;
        // A nested document-sized SVG creates a new clipping viewport. Older
        // moves accumulated these viewports, permanently hiding off-canvas pixels.
        // Keep the coordinate system and attributes, but make these wrappers groups.
        let source = unclipped_pixel_source(source, self.width, self.height)?;
        let content = if copy {
            // One encoded image payload, referenced twice. IDs must be unique across repeated copies.
            let mut serial = self.revision;
            while source.contains(&format!("pixel-copy-{serial}")) {
                serial += 1;
            }
            format!("<defs><g id=\"pixel-copy-{serial}\">{source}</g></defs><use href=\"#pixel-copy-{serial}\"/><use href=\"#pixel-copy-{serial}\" transform=\"translate({dx} {dy})\"/>")
        } else {
            format!("<g transform=\"translate({dx} {dy})\">{source}</g>")
        };
        Ok(Some(format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">{content}</svg>",
            self.width, self.height
        )))
    }

    pub fn replace_moved_pixels(
        &mut self,
        source: String,
        selection: Option<Selection>,
    ) -> Result<(), String> {
        self.replace_pixel_source(source, selection, false)
    }

    /// Retouch workers must preserve every destination alpha byte before commit.
    /// This entry point permits the alpha lock, while retaining all other guards.
    pub fn replace_retouch_pixels(
        &mut self,
        source: String,
        selection: Option<Selection>,
    ) -> Result<(), String> {
        self.replace_pixel_source(source, selection, true)
    }

    fn replace_pixel_source(
        &mut self,
        source: String,
        selection: Option<Selection>,
        preserve_alpha: bool,
    ) -> Result<(), String> {
        if source.len() > MAX_SVG_BYTES || !source.contains("<svg") {
            return Err("Pixel image exceeds project limit".into());
        }
        if let Some(selection) = &selection {
            selection.validate()?;
        }
        self.finish();
        if self.selected_layer.as_deref().unwrap_or("layer-1") == "layer-1" {
            if self.layer_locked || (self.layer_alpha_locked && !preserve_alpha) || !self.visible {
                return Err("Unlock and show the pixel layer / ピクセルレイヤーのロックを解除してください / 请解锁并显示像素图层".into());
            }
            let mut before = self.vector_history_state();
            before.strokes = Some(std::mem::take(&mut self.strokes));
            self.point_count = 0;
            self.paint_source = Some(source);
            self.selection = selection;
            self.record_vector_edit(before);
            self.revision += 1;
        } else {
            self.replace_image_source(source, preserve_alpha)?;
            self.selection = selection;
        }
        Ok(())
    }

    pub fn replace_selected_image(&mut self, source: String) -> Result<(), String> {
        self.replace_image_source(source, false)
    }
    fn replace_image_source(&mut self, source: String, preserve_alpha: bool) -> Result<(), String> {
        self.replace_image_source_inner(source, preserve_alpha, true)
    }
    fn replace_image_source_inner(
        &mut self,
        source: String,
        preserve_alpha: bool,
        record_history: bool,
    ) -> Result<(), String> {
        let id = self
            .selected_layer
            .as_ref()
            .ok_or("Select an image layer")?;
        let index = self
            .svg_layers
            .iter()
            .position(|layer| &layer.id == id)
            .ok_or("Image layer not found")?;
        let mut layer = self.svg_layers[index].clone();
        if layer.locked
            || (layer.alpha_locked && !preserve_alpha)
            || !layer.visible
            || layer.vector_layer
        {
            return Err("Select an unlocked image layer / ロックされていない画像レイヤーを選択してください / 请选择未锁定的图像图层".into());
        }
        layer.source = source;
        validate_svg_layer(&layer)?;
        let total = self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, l)| l.source.len())
            .sum::<usize>()
            + layer.source.len();
        if total > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much image data".into());
        }
        if record_history {
            let before = self.vector_history_state();
            self.svg_layers[index] = layer;
            self.record_vector_edit(before);
        } else {
            self.scene_journal.layers_changed(
                std::slice::from_ref(&self.svg_layers[index]),
                std::slice::from_ref(&layer),
            );
            self.svg_layers[index] = layer;
        }
        self.revision += 1;
        Ok(())
    }

    pub fn paste_content(
        &mut self,
        source: Option<String>,
        mut objects: Vec<VectorObject>,
    ) -> Result<(), String> {
        self.finish();
        if self.svg_layers.len() >= MAX_SVG_LAYERS
            || objects.len() > 4096
            || (source.is_some() == !objects.is_empty())
        {
            return Err("Invalid clipboard content or layer limit reached".into());
        }
        let mut serial = 1;
        while self
            .svg_layers
            .iter()
            .any(|layer| layer.id == format!("pasted-layer-{serial}"))
        {
            serial += 1;
        }
        let id = format!("pasted-layer-{serial}");
        for (index, object) in objects.iter_mut().enumerate() {
            let mut suffix = index;
            while self.svg_layers.iter().any(|layer| {
                layer
                    .vector_objects
                    .iter()
                    .any(|object| object.id == format!("pasted-{serial}-{suffix}"))
            }) {
                suffix += 4096;
            }
            object.id = format!("pasted-{serial}-{suffix}");
            object.validate()?;
        }
        let vector = source.is_none();
        let source = source.unwrap_or_else(|| vector_svg(self.width, self.height, &objects));
        let layer = SvgLayer {
            id: id.clone(),
            name: format!("Pasted {serial}"),
            visible: true,
            opacity: 1.,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
            source,
            paint_layer: !vector,
            vector_layer: vector,
            vector_objects: objects,
        };
        validate_svg_layer(&layer)?;
        if self
            .svg_layers
            .iter()
            .map(|layer| layer.source.len())
            .sum::<usize>()
            + layer.source.len()
            > MAX_SVG_TOTAL_BYTES
        {
            return Err("Project contains too much clipboard data".into());
        }
        let before = self.vector_history_state();
        self.selected_vector_objects = layer
            .vector_objects
            .iter()
            .map(|object| object.id.clone())
            .collect();
        self.svg_layers.push(layer);
        self.selected_layer = Some(id);
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn clear_selected_layer(&mut self) -> Result<(), String> {
        let id = self
            .selected_layer
            .as_deref()
            .unwrap_or("layer-1")
            .to_owned();
        let locked = if id == "layer-1" {
            self.layer_locked
        } else {
            self.svg_layers
                .iter()
                .find(|layer| layer.id == id)
                .ok_or("Layer not found")?
                .locked
        };
        if locked {
            return Err(
                "レイヤーのロックを解除してください。\nUnlock the layer first.\n请先解锁图层。"
                    .into(),
            );
        }
        self.finish();
        let mut before = self.vector_history_state();
        if id == "layer-1" {
            if self.strokes.is_empty() && self.paint_source.is_none() {
                return Ok(());
            }
            self.paint_source = None;
            before.strokes = Some(std::mem::take(&mut self.strokes));
            self.point_count = 0;
        } else {
            let imported_locked = self
                .svg_layers
                .iter()
                .find(|layer| layer.id == id && !layer.vector_layer)
                .is_some_and(|layer| {
                    self.imported_svg_objects(&layer.source, &layer.id)
                        .iter()
                        .any(|object| self.object_is_locked(&object.id))
                });
            if imported_locked {
                return Err("Unlock the artwork first / アートワークのロックを解除してください / 请先解锁图稿".into());
            }
            let layer = self
                .svg_layers
                .iter_mut()
                .find(|layer| layer.id == id)
                .unwrap();
            let empty = vector_svg(self.width, self.height, &[]);
            if layer.source == empty && layer.vector_objects.is_empty() {
                return Ok(());
            }
            self.selected_vector_objects
                .retain(|id| !layer.vector_objects.iter().any(|object| &object.id == id));
            if layer.vector_layer {
                layer
                    .vector_objects
                    .retain(|object| self.locked_objects.contains(&object.id));
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
            } else {
                layer.vector_objects.clear();
                layer.source = empty;
            }
        }
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn delete_layer(&mut self, id: &str) -> Result<(), String> {
        self.finish();
        if id == "layer-1" {
            return Err("The paint layer cannot be deleted yet".into());
        }
        let removed_ids: std::collections::BTreeSet<_> = self
            .svg_layers
            .iter()
            .find(|layer| layer.id == id)
            .map(|layer| {
                if layer.vector_layer {
                    layer
                        .vector_objects
                        .iter()
                        .map(|object| object.id.clone())
                        .collect()
                } else {
                    self.imported_svg_objects(&layer.source, &layer.id)
                        .into_iter()
                        .map(|object| object.id)
                        .collect()
                }
            })
            .unwrap_or_default();
        let before_state = self.vector_history_state();
        let before = self.svg_layers.len();
        self.svg_layers.retain(|layer| layer.id != id);
        if self.svg_layers.len() == before {
            return Err("Layer not found".into());
        }
        self.locked_objects.retain(|id| !removed_ids.contains(id));
        self.locked_artwork_layers.remove(id);
        self.layer_groups.reconcile(&self.svg_layers);
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
        let before = self.vector_history_state();
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
            source: empty_vector_svg(self.width, self.height),
            paint_layer: true,
            vector_layer: false,
            vector_objects: vec![],
        });
        self.attach_new_layer_to_selected_group(&id)?;
        self.record_vector_edit(before);
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
        self.sync_layer_group_order(ids_top_to_bottom)?;
        let previous = self.svg_layers.clone();
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
        self.scene_journal
            .layers_changed(&previous, &self.svg_layers);
        self.revision += 1;
        Ok(())
    }
    pub fn select_layer(&mut self, id: String) -> Result<(), String> {
        if !self
            .path_editing
            .as_ref()
            .is_some_and(|edit| edit.layer_id == id)
        {
            self.end_path_editing();
        }
        if id != "layer-1" && !self.svg_layers.iter().any(|layer| layer.id == id) {
            return Err("Layer not found".into());
        }
        self.finish();
        self.layer_groups.selected = vec![id.clone()];
        self.selected_layer = Some(id);
        self.layer_edit_selection = None;
        self.selected_vector_objects.clear();
        Ok(())
    }

    /// Initialize a freshly loaded view without clearing an existing selection.
    pub fn initialize_layer_selection(&mut self) {
        if self.selected_layer.is_none() {
            self.selected_layer = Some("layer-1".into());
            if self.layer_groups.selected.is_empty() {
                self.layer_groups.selected.push("layer-1".into());
            }
        }
    }

    pub fn selected_vector_target(&self) -> Result<Option<String>, String> {
        if self.layer_edit_target() != crate::layer_mask::LayerEditTarget::Content {
            return Err("Select the layer content thumbnail / レイヤー本体のサムネイルを選択してください / 请选择图层内容缩略图".into());
        }
        let Some(id) = &self.selected_layer else {
            return Ok(None);
        };
        let layer = self.svg_layers.iter().find(|layer| &layer.id == id);
        match layer {
            Some(layer) if layer.vector_layer && !layer.locked && layer.visible => Ok(Some(id.clone())),
            _ => Err("選択レイヤーにはテキスト・パスを書き込めません。表示中のロックされていないベクターレイヤーを選択してください。\nSelect an unlocked, visible vector layer for text and paths.\n请为文字和路径选择可见且未锁定的矢量图层。".into()),
        }
    }

    pub fn import_raster_layer(&mut self, name: String, source: String) -> Result<(), String> {
        if self.path_editing.is_some() {
            return Err("Finish path editing before importing / パスの編集を終了してから読み込んでください / 请先结束路径编辑".into());
        }
        self.paste_content(Some(source), Vec::new())?;
        // The paste history already contains the state before this new layer.
        self.svg_layers.last_mut().unwrap().name = name;
        Ok(())
    }

    /// Import into a new, independently undoable layer, regardless of current selection.
    pub fn import_svg_as_layer(&mut self, name: String, source: String) -> Result<(), String> {
        if self.path_editing.is_some() {
            return Err("Finish path editing before importing / パスの編集を終了してから読み込んでください / 请先结束路径编辑".into());
        }
        self.paste_content(Some(source), Vec::new())?;
        let layer = self.svg_layers.last_mut().unwrap();
        layer.name = name;
        layer.paint_layer = false;
        Ok(())
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
        let before = self.svg_layers.clone();
        self.svg_layers.push(layer);
        self.scene_journal.layers_changed(&before, &self.svg_layers);
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
        self.attach_new_layer_to_selected_group(&id)?;
        self.revision += 1;
        Ok(id)
    }
    pub fn text_edit_preview(&self, id: Option<&str>) -> Result<Self, String> {
        let mut preview = self.clone_for_rendering();
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
            if layer.locked
                || !layer.visible
                || !object.visible
                || self.object_is_locked(&object.id)
            {
                return Err("Text layer is locked or hidden".into());
            }
            object.visible = false;
            layer.source = vector_svg(preview.width, preview.height, &layer.vector_objects);
        }
        Ok(preview)
    }

    /// Update an isolated, retained preview of an existing text object. No
    /// history is recorded: callers must commit via set_text_object instead.
    /// Only the affected layer is staged; failures leave the preview intact.
    pub fn update_text_preview(&mut self, mut settings: TextSettings) -> Result<(), String> {
        if self.path_editing.is_some() || self.active.is_some() {
            return Err("Finish the active edit before text preview".into());
        }
        let hide = settings.text.content.trim().is_empty() && settings.text.box_height.is_none();
        if !hide {
            settings.text.validate()?;
        }
        if settings
            .position
            .iter()
            .any(|v| !v.is_finite() || v.abs() >= 100_000.)
        {
            return Err("Invalid text position".into());
        }
        let id = settings
            .id
            .as_deref()
            .ok_or("Existing text object required")?;
        self.ensure_object_unlocked(id)?;
        let (index, position) = self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| layer.vector_layer)
            .find_map(|(i, layer)| {
                layer
                    .vector_objects
                    .iter()
                    .position(|o| o.id == id && o.text.is_some())
                    .map(|p| (i, p))
            })
            .ok_or("Text object not found")?;
        let previous = &self.svg_layers[index];
        if previous.locked || !previous.visible {
            return Err("Text layer is locked or hidden".into());
        }
        let mut updated = previous.clone();
        let object = &mut updated.vector_objects[position];
        let original = object.text.as_ref().unwrap();
        settings.text.change_generation = original.change_generation;
        settings.text.updated_at_ms = original.updated_at_ms;
        object.visible = !hide;
        if !hide {
            object.control_points = settings.text.control_points();
            object.text = Some(settings.text);
            object.transform[4..6].copy_from_slice(&settings.position);
            object.fill = Some(VectorPaint {
                registration: false,
                color: [settings.color[0], settings.color[1], settings.color[2], 255],
            });
        }
        object.validate()?;
        updated.source = vector_svg(self.width, self.height, &updated.vector_objects);
        if updated.source.len()
            + self
                .svg_layers
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != index)
                .map(|(_, l)| l.source.len())
                .sum::<usize>()
            > MAX_SVG_TOTAL_BYTES
        {
            return Err("Project contains too much vector data".into());
        }
        validate_svg_layer(&updated)?;
        self.scene_journal.layers_changed(
            std::slice::from_ref(previous),
            std::slice::from_ref(&updated),
        );
        self.transform_slots.0.remove(&updated.id);
        self.noncanonical_vector_sources.remove(&updated.id);
        self.svg_layers[index] = updated;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn text_at(&self, point: [f32; 2]) -> Option<String> {
        self.svg_layers
            .iter()
            .rev()
            .filter(|layer| layer.visible && !layer.locked)
            .flat_map(|layer| layer.vector_objects.iter().rev())
            .find(|object| {
                !self.object_is_locked(&object.id)
                    && object.text.is_some()
                    && object.hit_test(point, 0.0)
            })
            .map(|object| object.id.clone())
    }

    pub fn set_text_object(&mut self, mut settings: TextSettings) -> Result<(), String> {
        let _timer = crate::performance::time("document_text_edit");
        crate::performance::count("document_text_edit_calls", 1);
        if self.path_editing.is_some() {
            return Err(
                "Paths accept geometry only / パスには文字を追加できません / 路径不支持文字".into(),
            );
        }
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
            self.ensure_object_unlocked(&id)?;
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
            if let Some(text) = &object.text {
                settings.text.change_generation = text.change_generation;
                settings.text.updated_at_ms = text.updated_at_ms;
            }
            if object.text.as_ref() == Some(&settings.text)
                && object.transform[4..] == settings.position
                && object.fill
                    == Some(crate::vector::VectorPaint {
                        registration: false,
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
                registration: false,
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
        }) || self
            .reserved_compound_ids()
            .contains(&format!("text-{serial}"))
        {
            serial += 1;
        }
        let id = format!("text-{serial}");
        let object = VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: id.clone(),
            name: format!("Text {serial}"),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
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
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(crate::vector::VectorPaint {
                registration: false,
                color: [r, g, b, 255],
            }),
            stroke: None,
            stroke_style: Default::default(),
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
        if self
            .compound_shapes
            .iter()
            .any(|s| s.operands.iter().any(|o| o.id == object.id))
        {
            return Err("Object identity is reserved by a compound shape".into());
        }
        self.ensure_object_unlocked(&object.id)?;
        if self
            .path_editing
            .as_ref()
            .is_some_and(|edit| edit.layer_id != layer_id || object.text.is_some())
        {
            return Err("Select the active saved path / アクティブな保存パスを選択してください / 请选择活动的保存路径".into());
        }
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
    pub fn set_selected_stroke_width(&mut self, width: f32, color: [u8; 3]) -> Result<(), String> {
        if !width.is_finite() || !(0.0..=4096.0).contains(&width) {
            return Err("Invalid stroke width".into());
        }
        let mut layers = self.svg_layers.clone();
        let mut changed = false;
        let mut found = false;
        for layer in &mut layers {
            let mut layer_changed = false;
            for object in &mut layer.vector_objects {
                if !self.selected_vector_objects.contains(&object.id) {
                    continue;
                }
                if !layer.visible
                    || layer.locked
                    || !object.visible
                    || self.object_is_locked(&object.id)
                    || object.kind == VectorObjectKind::Text
                {
                    return Err("Select unlocked visible vector paths / ロックされていない表示中のパスを選択してください / 请选择未锁定的可见路径".into());
                }
                found = true;
                if object.stroke_width == width && (object.stroke.is_some() || width == 0.0) {
                    continue;
                }
                object.stroke_width = width;
                if width > 0.0 && object.stroke.is_none() {
                    object.stroke = Some(VectorPaint {
                        registration: false,
                        color: [color[0], color[1], color[2], 255],
                    });
                }
                object.validate()?;
                layer_changed = true;
            }
            if layer_changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
                changed = true;
            }
        }
        if !found {
            return Err(
                "Select a vector path / ベクターパスを選択してください / 请选择矢量路径".into(),
            );
        }
        if layers.iter().map(|layer| layer.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        if changed {
            let before = self.vector_history_state();
            self.svg_layers = layers;
            self.record_vector_edit(before);
            self.revision += 1;
        }
        Ok(())
    }

    pub fn set_selected_stroke_style(
        &mut self,
        patch: crate::stroke::StrokeStylePatch,
    ) -> Result<(), String> {
        let mut layers = self.svg_layers.clone();
        let mut changed = false;
        let mut found = false;
        for layer in &mut layers {
            let mut layer_changed = false;
            for object in &mut layer.vector_objects {
                if !self.selected_vector_objects.contains(&object.id) {
                    continue;
                }
                if !layer.visible
                    || layer.locked
                    || !object.visible
                    || self.object_is_locked(&object.id)
                    || object.kind == VectorObjectKind::Text
                {
                    return Err("Select unlocked visible vector paths / ロックされていない表示中のパスを選択してください / 请选择未锁定的可见路径".into());
                }
                found = true;
                let mut style = object.stroke_style.clone();
                style.apply(&patch);
                style.validate()?;
                if style == object.stroke_style {
                    continue;
                }
                object.stroke_style = style;
                object.validate()?;
                layer_changed = true;
            }
            if layer_changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
                changed = true;
            }
        }
        if !found {
            return Err(
                "Select a vector path / ベクターパスを選択してください / 请选择矢量路径".into(),
            );
        }
        if layers.iter().map(|layer| layer.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        if changed {
            let before = self.vector_history_state();
            self.svg_layers = layers;
            self.record_vector_edit(before);
            self.revision += 1;
        }
        Ok(())
    }

    pub fn select_vector_objects(&mut self, ids: Vec<String>) -> Result<(), String> {
        if ids.len() > 4096 {
            return Err("Too many selected vector objects".into());
        }
        let mut unique = Vec::with_capacity(ids.len());
        for id in ids {
            let exists = self.svg_layers.iter().any(|layer| {
                self.can_edit_path_layer(layer)
                    && layer.vector_layer
                    && layer.visible
                    && !layer.locked
                    && layer.vector_objects.iter().any(|object| {
                        object.id == id && object.visible && !self.object_is_locked(&object.id)
                    })
            });
            if !exists {
                return Err("Vector object not found".into());
            }
            if !unique.contains(&id) {
                unique.push(id);
            }
        }
        self.guides.selected.clear();
        if !self.selected_vector_objects.is_empty() {
            self.previous_vector_selection = self.selected_vector_objects.clone();
        }
        self.selected_vector_objects = self.expand_group_selection(&unique);
        Ok(())
    }

    fn expand_group_selection(&self, ids: &[String]) -> Vec<String> {
        let mut expanded = Vec::new();
        for layer in self.svg_layers.iter().filter(|layer| {
            self.can_edit_path_layer(layer) && layer.vector_layer && layer.visible && !layer.locked
        }) {
            let roots: Vec<&str> = layer
                .vector_objects
                .iter()
                .filter(|object| ids.contains(&object.id))
                .filter_map(|object| object.group_path.first().map(String::as_str))
                .collect();
            for object in &layer.vector_objects {
                let selected = ids.contains(&object.id)
                    || object
                        .group_path
                        .first()
                        .is_some_and(|root| roots.contains(&root.as_str()));
                if selected
                    && object.visible
                    && !self.object_is_locked(&object.id)
                    && !expanded.contains(&object.id)
                {
                    expanded.push(object.id.clone());
                }
            }
        }
        expanded
    }

    pub fn compound_path<F>(&mut self, release: bool, operation: F) -> Result<(), String>
    where
        F: FnOnce(&[VectorObject]) -> Result<Vec<PortablePathGeometry>, String>,
    {
        let selected = &self.selected_vector_objects;
        let indices: Vec<_> = self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.vector_objects.iter().any(|o| selected.contains(&o.id)))
            .map(|(i, _)| i)
            .collect();
        if indices.len() != 1 {
            return Err("Select paths on one vector layer / 同じベクターレイヤーのパスを選択してください / 请选择同一矢量图层中的路径".into());
        }
        let index = indices[0];
        let mut layer = self.svg_layers[index].clone();
        let objects: Vec<_> = layer
            .vector_objects
            .iter()
            .filter(|o| selected.contains(&o.id))
            .cloned()
            .collect();
        if layer.locked
            || !layer.visible
            || objects.iter().any(|o| {
                !o.visible
                    || o.text.is_some()
                    || o.clipping_group.is_some()
                    || !o.group_path.is_empty()
            })
        {
            return Err("Select visible ungrouped paths on an unlocked layer / ロックされていないレイヤーの、グループ外の表示中パスを選択してください / 请选择未锁定图层中可见且未分组的路径".into());
        }
        if (!release && objects.len() < 2)
            || (release && (objects.len() != 1 || objects[0].kind != VectorObjectKind::Compound))
        {
            return Err("Select two paths to make, or one compound path to release / 作成は2つ以上のパス、削除は1つの複合パスを選択してください / 建立需选择至少两条路径，释放需选择一条复合路径".into());
        }
        let geometry = operation(&objects)?;
        if geometry.is_empty()
            || (!release && geometry.len() != 1)
            || geometry.len() > 4096
            || layer.vector_objects.len() - objects.len() + geometry.len() > 4096
        {
            return Err("Invalid compound path result".into());
        }
        let insertion = layer
            .vector_objects
            .iter()
            .position(|o| selected.contains(&o.id))
            .unwrap();
        let mut output = Vec::new();
        for (n, (path, points)) in geometry.into_iter().enumerate() {
            let mut object = objects[0].clone();
            if n > 0 {
                let mut suffix = n;
                loop {
                    let candidate = format!("compound-{}-{suffix}", self.revision + 1);
                    if !self
                        .svg_layers
                        .iter()
                        .any(|l| l.vector_objects.iter().any(|o| o.id == candidate))
                    {
                        object.id = candidate;
                        break;
                    }
                    suffix += 4096;
                }
            }
            object.path = path;
            object.control_points = points;
            object.transform = [1., 0., 0., 1., 0., 0.];
            object.kind = VectorObjectKind::Compound;
            object.validate()?;
            output.push(object);
        }
        let ids = output.iter().map(|o| o.id.clone()).collect();
        layer.vector_objects.retain(|o| !selected.contains(&o.id));
        layer.vector_objects.splice(insertion..insertion, output);
        layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
        validate_svg_layer(&layer)?;
        if self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, l)| l.source.len())
            .sum::<usize>()
            + layer.source.len()
            > MAX_SVG_TOTAL_BYTES
        {
            return Err("Project contains too much vector data".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers[index] = layer;
        self.selected_vector_objects = ids;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn set_selected_vector_appearance(
        &mut self,
        ids: &[String],
        opacity: Option<f32>,
        blend_mode: Option<String>,
    ) -> Result<(), String> {
        if ids.is_empty()
            || ids.len() != self.selected_vector_objects.len()
            || ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
            || !ids
                .iter()
                .all(|id| self.selected_vector_objects.contains(id))
        {
            return Err("Selection changed / 選択が変更されました / 选区已更改".into());
        }
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            let mut changed = false;
            for object in &mut layer.vector_objects {
                if !ids.contains(&object.id) {
                    continue;
                }
                if layer.locked
                    || !layer.visible
                    || !object.visible
                    || self.object_is_locked(&object.id)
                    || object.text.is_some()
                {
                    return Err("Select visible paths on unlocked layers / ロックされていない表示中のパスを選択してください / 请选择未锁定且可见的路径".into());
                }
                if let Some(value) = opacity {
                    object.opacity = value;
                }
                if let Some(value) = &blend_mode {
                    object.blend_mode = value.clone();
                }
                object.validate()?;
                changed = true;
            }
            if changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers
            .iter()
            .zip(&self.svg_layers)
            .all(|(a, b)| a.vector_objects == b.vector_objects)
        {
            return Ok(());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn create_trim_marks(&mut self) -> Result<(), String> {
        let error = "Select one visible unlocked rectangle / 表示中でロックされていない四角形を1つ選択してください / 请选择一个可见且未锁定的矩形";
        if self.path_editing.is_some() || self.selected_vector_objects.len() != 1 {
            return Err(error.into());
        }
        let id = &self.selected_vector_objects[0];
        let layer_index = self
            .svg_layers
            .iter()
            .position(|l| l.vector_objects.iter().any(|o| &o.id == id))
            .ok_or(error)?;
        let layer = &self.svg_layers[layer_index];
        let rectangle = layer.vector_objects.iter().find(|o| &o.id == id).unwrap();
        if !layer.vector_layer
            || !layer.visible
            || layer.locked
            || !rectangle.visible
            || self.object_is_locked(id)
            || rectangle.kind != VectorObjectKind::Rectangle
        {
            return Err(error.into());
        }
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for p in vector_geometry_extrema(rectangle, |p| p) {
            bounds[0] = bounds[0].min(p.x);
            bounds[1] = bounds[1].min(p.y);
            bounds[2] = bounds[2].max(p.x);
            bounds[3] = bounds[3].max(p.y);
        }
        if bounds.iter().any(|v| !v.is_finite()) || bounds[2] <= bounds[0] || bounds[3] <= bounds[1]
        {
            return Err(error.into());
        }
        let [x, y, r, b] = bounds;
        let gap = 3. * self.resolution as f32 / 25.4;
        let length = 5. * self.resolution as f32 / 25.4;
        let mut path = String::new();
        for yy in [y, b, y - gap, b + gap] {
            for (start, end) in [(x - gap - length, x - gap), (r + gap, r + gap + length)] {
                path.push_str(&format!("M{start} {yy}L{end} {yy}"));
            }
        }
        for xx in [x, r, x - gap, r + gap] {
            for (start, end) in [(y - gap - length, y - gap), (b + gap, b + gap + length)] {
                path.push_str(&format!("M{xx} {start}L{xx} {end}"));
            }
        }
        // Center crosses stay outside the bleed, with a 1 mm clearance.
        let clearance = self.resolution as f32 / 25.4;
        let half = length / 2.;
        let cx = (x + r) / 2.;
        let cy = (y + b) / 2.;
        for yy in [y - gap - clearance - half, b + gap + clearance + half] {
            path.push_str(&format!("M{} {yy}L{} {yy}", cx - half, cx + half));
            path.push_str(&format!("M{cx} {}L{cx} {}", yy - half, yy + half));
        }
        for xx in [x - gap - clearance - half, r + gap + clearance + half] {
            path.push_str(&format!("M{xx} {}L{xx} {}", cy - half, cy + half));
            path.push_str(&format!("M{} {cy}L{} {cy}", xx - half, xx + half));
        }
        let mut marks = rectangle.clone();
        let mut serial = 1;
        loop {
            marks.id = format!("trim-marks-{serial}");
            if !self
                .svg_layers
                .iter()
                .any(|l| l.vector_objects.iter().any(|o| o.id == marks.id))
            {
                break;
            }
            serial += 1;
        }
        marks.name = "Trim marks / トリムマーク / 裁切标记".into();
        marks.kind = VectorObjectKind::Path;
        marks.path.data = path;
        marks.transform = [1., 0., 0., 1., 0., 0.];
        marks.fill = None;
        marks.fill_gradient = None;
        marks.stroke_gradient = None;
        marks.stroke = Some(VectorPaint {
            registration: true,
            color: [0, 0, 0, 255],
        });
        marks.stroke_width = self.resolution as f32 / 72. * 0.25;
        marks.stroke_style = Default::default();
        marks.group_path.clear();
        marks.clipping_group = None;
        marks.text = None;
        marks.image_frame = None;
        marks.live_corners = None;
        marks.rectangle_radii = None;
        marks.control_points.clear();
        marks.opacity = 1.;
        marks.blend_mode = "normal".into();
        marks.bounds_reset = false;
        marks.validate()?;
        let mut layers = self.svg_layers.clone();
        layers[layer_index].vector_objects.push(marks.clone());
        layers[layer_index].source =
            vector_svg(self.width, self.height, &layers[layer_index].vector_objects);
        validate_svg_layer(&layers[layer_index])?;
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.selected_vector_objects = vec![marks.id];
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn set_selected_vector_paint(
        &mut self,
        ids: &[String],
        target: &str,
        color: Option<[u8; 3]>,
    ) -> Result<(), String> {
        let registration = matches!(target, "registrationFill" | "registrationStroke");
        let target = match target {
            "registrationFill" => "fill",
            "registrationStroke" => "stroke",
            value => value,
        };
        let color = if registration { Some([0, 0, 0]) } else { color };
        if ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
            || ids.is_empty()
            || ids.len() != self.selected_vector_objects.len()
            || !ids
                .iter()
                .all(|id| self.selected_vector_objects.contains(id))
        {
            return Err("Selection changed / 選択が変更されました / 选区已更改".into());
        }
        if !["fill", "stroke", "swap"].contains(&target) {
            return Err("Invalid paint target".into());
        }
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            let mut changed = false;
            for object in &mut layer.vector_objects {
                if !ids.contains(&object.id) {
                    continue;
                }
                if layer.locked
                    || !layer.visible
                    || !object.visible
                    || self.object_is_locked(&object.id)
                    || object.text.is_some()
                {
                    return Err("Select visible paths on unlocked layers / ロックされていない表示中のパスを選択してください / 请选择未锁定且可见的路径".into());
                }
                if target == "swap" {
                    std::mem::swap(&mut object.fill, &mut object.stroke);
                    std::mem::swap(&mut object.fill_gradient, &mut object.stroke_gradient);
                } else {
                    let paint = if target == "fill" {
                        &mut object.fill
                    } else {
                        &mut object.stroke
                    };
                    if target == "fill" {
                        object.fill_gradient = None;
                    } else {
                        object.stroke_gradient = None;
                    }
                    *paint = color.map(|[r, g, b]| VectorPaint {
                        registration,
                        color: [
                            r,
                            g,
                            b,
                            if registration {
                                255
                            } else {
                                paint.map_or(255, |p| p.color[3])
                            },
                        ],
                    });
                }
                if object.stroke.is_some() && object.stroke_width <= 0. {
                    object.stroke_width = 1.;
                }
                object.validate()?;
                changed = true;
            }
            if changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|layer| layer.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    /// An editable gradient fill above the pixel artwork. The original pixels
    /// remain untouched; selection geometry becomes a saved vector mask.
    pub fn add_gradient_fill_layer(
        &mut self,
        gradient: crate::gradient::Gradient,
        engine: &dyn crate::vector::VectorPathEngine,
    ) -> Result<String, String> {
        self.add_procedural_fill_layer(gradient, engine, None)
    }

    pub fn add_screentone_layer(
        &mut self,
        mut tone: crate::screentone::Screentone,
        engine: &dyn crate::vector::VectorPathEngine,
    ) -> Result<String, String> {
        tone.validate()?;
        tone.dpi = self.resolution as f32;
        let gradient = crate::gradient::Gradient {
            geometry: None,
            pixel_style: None,
            kind: crate::gradient::GradientKind::Linear,
            angle: 0.,
            aspect: 1.,
            dither: false,
            method: crate::gradient::GradientMethod::Classic,
            stops: vec![
                crate::gradient::GradientStop {
                    position: 0.,
                    color: [0, 0, 0, 255],
                    midpoint: 0.5,
                },
                crate::gradient::GradientStop {
                    position: 1.,
                    color: [0, 0, 0, 255],
                    midpoint: 0.5,
                },
            ],
        };
        self.add_procedural_fill_layer(gradient, engine, Some(tone))
    }
    fn add_procedural_fill_layer(
        &mut self,
        gradient: crate::gradient::Gradient,
        engine: &dyn crate::vector::VectorPathEngine,
        tone: Option<crate::screentone::Screentone>,
    ) -> Result<String, String> {
        let title = if tone.is_some() {
            "Screentone"
        } else {
            "Gradient Fill"
        };
        let prefix = if tone.is_some() {
            "tone-layer"
        } else {
            "gradient-layer"
        };
        gradient.validate()?;
        if gradient.pixel_style.is_some() {
            return Err("Gradient fill layers currently support linear and radial / グラデーションレイヤーは線形・円形に対応しています / 渐变填充图层目前支持线性和径向".into());
        }
        if self.svg_layers.len() >= MAX_SVG_LAYERS {
            return Err("Layer capacity exceeded".into());
        }
        let rectangle = |x: f32, y: f32, w: f32, h: f32| VectorPath {
            data: format!("M{x} {y}h{w}v{h}h{}Z", -w),
            fill_rule: crate::vector::FillRule::NonZero,
        };
        let mut path = rectangle(0., 0., self.width as f32, self.height as f32);
        if let Some(selection) = &self.selection {
            for region in &selection.regions {
                let [x, y, w, h] = region.bounds;
                let shape = match region.shape {
                    SelectionShape::Polygon | SelectionShape::Stroke => VectorPath {
                        data: region.path_data(),
                        fill_rule: if region.shape == SelectionShape::Polygon {
                            crate::vector::FillRule::EvenOdd
                        } else {
                            crate::vector::FillRule::NonZero
                        },
                    },
                    SelectionShape::Rectangle => rectangle(x, y, w, h),
                    SelectionShape::Ellipse => VectorPath {
                        data: format!(
                            "M{} {}a{} {} 0 1 0 {} 0a{} {} 0 1 0 {} 0Z",
                            x + w,
                            y + h * 0.5,
                            w * 0.5,
                            h * 0.5,
                            -w,
                            w * 0.5,
                            h * 0.5,
                            w
                        ),
                        fill_rule: crate::vector::FillRule::NonZero,
                    },
                };
                path = match region.operation {
                    SelectionOperation::Replace => shape,
                    SelectionOperation::Add => {
                        engine.combine(&path, &shape, PathOperation::Union)?
                    }
                    SelectionOperation::Subtract => {
                        engine.combine(&path, &shape, PathOperation::Difference)?
                    }
                    SelectionOperation::Intersect => {
                        engine.combine(&path, &shape, PathOperation::Intersection)?
                    }
                    SelectionOperation::Invert => {
                        engine.combine(&shape, &path, PathOperation::Difference)?
                    }
                };
            }
        }
        let bounds = crate::stroke::path_bounds(&path.data)
            .ok_or("Empty gradient fill selection".to_string())?;
        if bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
            return Err("Empty gradient fill selection".into());
        }
        let mut serial = 1;
        while self
            .svg_layers
            .iter()
            .any(|l| l.id == format!("{prefix}-{serial}"))
        {
            serial += 1;
        }
        let id = format!("{prefix}-{serial}");
        let object_id = format!("{id}-fill");
        let object = VectorObject {
            id: object_id.clone(),
            name: title.into(),
            path,
            transform: [1., 0., 0., 1., 0., 0.],
            opacity: 1.,
            blend_mode: "normal".into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            fill: Some(VectorPaint {
                registration: false,
                color: gradient.stops[0].color,
            }),
            stroke: None,
            stroke_width: 0.,
            stroke_style: Default::default(),
            fill_gradient: if tone.is_some() { None } else { Some(gradient) },
            stroke_gradient: None,
            live_corners: None,
            rectangle_radii: None,
            image_frame: None,
            visible: true,
            kind: VectorObjectKind::Compound,
            control_points: vec![[bounds[0], bounds[1]], [bounds[2], bounds[3]]],
            text: None,
        };
        object.validate()?;
        let objects = vec![object];
        let source = vector_svg(self.width, self.height, &objects);
        let layer = SvgLayer {
            id: id.clone(),
            name: title.into(),
            source,
            vector_objects: objects,
            vector_layer: true,
            paint_layer: false,
            visible: true,
            locked: false,
            opacity: 1.,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
        };
        validate_svg_layer(&layer)?;
        if self
            .svg_layers
            .iter()
            .map(|l| l.source.len())
            .sum::<usize>()
            + layer.source.len()
            > MAX_SVG_TOTAL_BYTES
        {
            return Err("Vector capacity exceeded".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers.push(layer);
        if let Some(tone) = tone {
            self.layer_effects.insert(
                id.clone(),
                crate::layer_effects::LayerEffects {
                    enabled: true,
                    screentone: Some(tone),
                    ..Default::default()
                },
            );
        }
        self.layer_groups.reconcile(&self.svg_layers);
        self.selected_layer = Some(id.clone());
        self.selected_vector_objects = vec![object_id];
        self.selection = None;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(id)
    }

    pub fn set_selected_vector_gradient(
        &mut self,
        ids: &[String],
        target: &str,
        gradient: crate::gradient::Gradient,
    ) -> Result<(), String> {
        self.set_vector_gradient(ids, target, gradient, false)
    }

    /// Canvas drag geometry arrives in document space, independent of object transforms.
    pub fn set_dragged_vector_gradient(
        &mut self,
        ids: &[String],
        target: &str,
        gradient: crate::gradient::Gradient,
    ) -> Result<(), String> {
        self.set_vector_gradient(ids, target, gradient, true)
    }

    fn set_vector_gradient(
        &mut self,
        ids: &[String],
        target: &str,
        gradient: crate::gradient::Gradient,
        document_space: bool,
    ) -> Result<(), String> {
        gradient.validate()?;
        if gradient.pixel_style.is_some() {
            return Err("This gradient style is available for pixels only / この種類はピクセル専用です / 此类型仅用于像素".into());
        }
        if target == "stroke" && gradient.dither {
            return Err(
                "Dither applies only to fills / ディザは塗りにのみ適用できます / 仿色仅用于填色"
                    .into(),
            );
        }
        if ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len()
            || ids.is_empty()
            || ids.len() != self.selected_vector_objects.len()
            || !ids
                .iter()
                .all(|id| self.selected_vector_objects.contains(id))
        {
            return Err("Selection changed / 選択が変更されました / 选区已更改".into());
        }
        if !["fill", "stroke"].contains(&target) {
            return Err("Invalid paint target".into());
        }
        let available: std::collections::HashSet<String> = self
            .svg_layers
            .iter()
            .flat_map(|layer| {
                if layer.vector_layer {
                    layer
                        .vector_objects
                        .iter()
                        .map(|o| o.id.clone())
                        .collect::<Vec<_>>()
                } else if ids
                    .iter()
                    .any(|id| id.starts_with(&format!("{}::", layer.id)))
                {
                    self.imported_svg_objects(&layer.source, &layer.id)
                        .into_iter()
                        .map(|o| o.id)
                        .collect()
                } else {
                    Vec::new()
                }
            })
            .collect();
        if !ids.iter().all(|id| available.contains(id)) {
            return Err("Gradient object missing".into());
        }
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            if !layer.vector_layer {
                let objects = self.imported_svg_objects(&layer.source, &layer.id);
                let mut changes = Vec::new();
                for object in objects.iter().filter(|object| ids.contains(&object.id)) {
                    if layer.locked
                        || !layer.visible
                        || !object.visible
                        || self.object_is_locked(&object.id)
                    {
                        return Err("SVG object is locked or hidden / SVGがロックまたは非表示です / SVG 已锁定或隐藏".into());
                    }
                    let mut gradient = gradient.clone();
                    if document_space {
                        gradient.geometry = gradient.geometry.map(|g| {
                            let [a, b, c, d, e, f] = object.transform;
                            let det = a * d - b * c;
                            let [ga, gb, gc, gd, ge, gf] = g;
                            [
                                (d * ga - c * gb) / det,
                                (-b * ga + a * gb) / det,
                                (d * gc - c * gd) / det,
                                (-b * gc + a * gd) / det,
                                (d * (ge - e) - c * (gf - f)) / det,
                                (-b * (ge - e) + a * (gf - f)) / det,
                            ]
                        });
                        gradient.validate()?;
                    }
                    changes.push((object.id.clone(), target.to_owned(), gradient));
                }
                if !changes.is_empty() {
                    layer.source = self
                        .svg_geometry_backend
                        .as_ref()
                        .ok_or("SVG adapter unavailable")?
                        .set_gradients(
                            &layer.source,
                            &layer.id,
                            [self.width as f32, self.height as f32],
                            &changes,
                        )?;
                    validate_svg_layer(layer)?;
                    validate_svg_edit_source(&layer.source)?;
                }
                continue;
            }
            let mut changed = false;
            for object in &mut layer.vector_objects {
                if !ids.contains(&object.id) {
                    continue;
                }
                if layer.locked
                    || !layer.visible
                    || !object.visible
                    || self.object_is_locked(&object.id)
                    || object.text.is_some()
                {
                    return Err("Select visible paths on unlocked layers / ロックされていない表示中のパスを選択してください / 请选择未锁定且可见的路径".into());
                }
                let mut gradient = gradient.clone();
                if document_space {
                    if let Some([ga, gb, gc, gd, ge, gf]) = gradient.geometry {
                        let [a, b, c, d, e, f] = object.transform;
                        let det = a * d - b * c;
                        if det.abs() < 1e-10 {
                            return Err("Cannot edit a singular transform".into());
                        }
                        gradient.geometry = Some([
                            (d * ga - c * gb) / det,
                            (-b * ga + a * gb) / det,
                            (d * gc - c * gd) / det,
                            (-b * gc + a * gd) / det,
                            (d * (ge - e) - c * (gf - f)) / det,
                            (-b * (ge - e) + a * (gf - f)) / det,
                        ]);
                        gradient.validate()?;
                    }
                }
                if target == "fill" {
                    object.fill_gradient = Some(gradient.clone());
                    object.fill = Some(VectorPaint {
                        registration: false,
                        color: gradient.stops[0].color,
                    });
                } else {
                    object.stroke_gradient = Some(gradient.clone());
                    object.stroke = Some(VectorPaint {
                        registration: false,
                        color: gradient.stops[0].color,
                    });
                }
                if object.stroke.is_some() && object.stroke_width <= 0. {
                    object.stroke_width = 1.;
                }
                object.validate()?;
                changed = true;
            }
            if changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|layer| layer.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        if layers
            .iter()
            .zip(&self.svg_layers)
            .all(|(a, b)| a.source == b.source && a.vector_objects == b.vector_objects)
        {
            return Ok(());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn clipping_path(&mut self, action: &str) -> Result<(), String> {
        let selected = self.selected_vector_objects.clone();
        let indices: Vec<_> = self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.vector_objects.iter().any(|o| selected.contains(&o.id)))
            .map(|(i, _)| i)
            .collect();
        if indices.len() != 1 {
            return Err("Select objects on one vector layer / 同じベクターレイヤーで選択してください / 请在同一矢量图层中选择对象".into());
        }
        let index = indices[0];
        let mut layer = self.svg_layers[index].clone();
        if !layer.visible || layer.locked {
            return Err(
                "Layer is locked or hidden / レイヤーがロックまたは非表示です / 图层已锁定或隐藏"
                    .into(),
            );
        }
        if action == "create" {
            let members: Vec<_> = layer
                .vector_objects
                .iter()
                .filter(|o| selected.contains(&o.id))
                .collect();
            if members.len() < 2 || members.iter().any(|o| !o.visible) {
                return Err("Select at least two visible objects / 表示中のオブジェクトを2つ以上選択してください / 请选择至少两个可见对象".into());
            }
            let mask = members.last().unwrap();
            if mask.text.is_some()
                || !mask.group_path.is_empty()
                || mask.path.data.trim().is_empty()
            {
                return Err("The frontmost object must be an ungrouped path / 最前面にはグループ外のパスを選択してください / 最前面的对象必须是未分组路径".into());
            }
            let mask_id = mask.id.clone();
            let group = format!("clip-{}", self.revision + 1);
            if layer
                .vector_objects
                .iter()
                .any(|o| o.group_path.contains(&group))
            {
                return Err("Clipping group ID conflict".into());
            }
            let front = layer
                .vector_objects
                .iter()
                .rposition(|o| selected.contains(&o.id))
                .unwrap();
            let insertion = layer.vector_objects[..front]
                .iter()
                .filter(|o| !selected.contains(&o.id))
                .count();
            let mut grouped = Vec::new();
            layer.vector_objects.retain_mut(|o| {
                if !selected.contains(&o.id) {
                    return true;
                }
                o.group_path.insert(0, group.clone());
                if o.id == mask_id {
                    o.clipping_group = Some(group.clone());
                }
                grouped.push(o.clone());
                false
            });
            layer.vector_objects.splice(insertion..insertion, grouped);
        } else if action == "release" || action == "edit" {
            let masks: Vec<_> = layer
                .vector_objects
                .iter()
                .filter(|o| {
                    o.clipping_group.as_ref().is_some_and(|g| {
                        layer
                            .vector_objects
                            .iter()
                            .any(|m| selected.contains(&m.id) && m.group_path.contains(g))
                    })
                })
                .map(|o| (o.id.clone(), o.clipping_group.clone().unwrap()))
                .collect();
            if masks.len() != 1 {
                return Err("Select one clipping group / クリッピンググループを1つ選択してください / 请选择一个剪切组".into());
            }
            let (mask_id, group) = &masks[0];
            if action == "edit" {
                self.selected_vector_objects = vec![mask_id.clone()];
                return Ok(());
            }
            for o in &mut layer.vector_objects {
                o.group_path.retain(|g| g != group);
                if o.id == *mask_id {
                    o.clipping_group = None;
                }
            }
        } else {
            return Err("Unknown clipping action".into());
        }
        layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
        validate_svg_layer(&layer)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers[index] = layer;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn group_selected_vectors(&mut self) -> Result<(), String> {
        if self.selected_vector_objects.len() < 2 {
            return Err("Select at least two vector objects / 2つ以上のベクターオブジェクトを選択してください / 请选择至少两个矢量对象".into());
        }
        let selected = self.selected_vector_objects.clone();
        let matching_layers: Vec<usize> = self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(_, layer)| {
                layer.vector_layer
                    && layer
                        .vector_objects
                        .iter()
                        .any(|object| selected.contains(&object.id))
            })
            .map(|(index, _)| index)
            .collect();
        if matching_layers.len() != 1 {
            return Err("Grouped objects must be on one vector layer / 同じベクターレイヤーのオブジェクトを選択してください / 分组对象必须位于同一矢量图层".into());
        }
        let layer_index = matching_layers[0];
        let layer = &self.svg_layers[layer_index];
        if layer.locked || !layer.visible {
            return Err("Vector layer is locked or hidden / ベクターレイヤーがロックまたは非表示です / 矢量图层已锁定或隐藏".into());
        }
        let selected_count = layer
            .vector_objects
            .iter()
            .filter(|object| object.visible && selected.contains(&object.id))
            .count();
        if selected_count != selected.len() {
            return Err("Grouped objects must be visible on one unlocked vector layer / 表示中でロックされていない1つのベクターレイヤーから選択してください / 请选择同一可见且未锁定矢量图层中的对象".into());
        }
        let mut entities = Vec::new();
        for object in layer
            .vector_objects
            .iter()
            .filter(|object| selected.contains(&object.id))
        {
            let entity = object.group_path.first().unwrap_or(&object.id);
            if !entities.contains(entity) {
                entities.push(entity.clone());
            }
        }
        if entities.len() < 2 {
            return Err("Select at least two separate objects or groups / 2つ以上の別々のオブジェクトまたはグループを選択してください / 请选择至少两个独立对象或组".into());
        }
        let mut suffix = 1u64;
        let group_id = loop {
            let candidate = format!("group-{}-{suffix}", self.revision + 1);
            if !self.svg_layers.iter().any(|layer| {
                layer
                    .vector_objects
                    .iter()
                    .any(|object| object.group_path.contains(&candidate))
            }) {
                break candidate;
            }
            suffix += 1;
        };
        let mut updated = self.svg_layers[layer_index].clone();
        let frontmost = updated
            .vector_objects
            .iter()
            .rposition(|object| selected.contains(&object.id))
            .ok_or("Vector objects not found")?;
        let insertion = updated.vector_objects[..frontmost]
            .iter()
            .filter(|object| !selected.contains(&object.id))
            .count();
        let mut members = Vec::with_capacity(selected.len());
        updated.vector_objects.retain_mut(|object| {
            if selected.contains(&object.id) {
                object.group_path.insert(0, group_id.clone());
                members.push(object.clone());
                false
            } else {
                true
            }
        });
        updated.vector_objects.splice(insertion..insertion, members);
        updated.source = vector_svg(self.width, self.height, &updated.vector_objects);
        validate_svg_layer(&updated)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers[layer_index] = updated;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn ungroup_selected_vectors(&mut self, all: bool) -> Result<(), String> {
        let selected = self.selected_vector_objects.clone();
        let mut roots: Vec<(usize, String)> = Vec::new();
        for (layer_index, layer) in self.svg_layers.iter().enumerate() {
            for object in &layer.vector_objects {
                if selected.contains(&object.id) {
                    if let Some(root) = object.group_path.first() {
                        let target = (layer_index, root.clone());
                        if !roots.contains(&target) {
                            roots.push(target);
                        }
                    }
                }
            }
        }
        if roots.is_empty() {
            return Err("Select a group / グループを選択してください / 请选择组".into());
        }
        if roots.iter().any(|(index, _)| {
            self.svg_layers
                .get(*index)
                .is_none_or(|layer| layer.locked || !layer.visible)
        }) {
            return Err("Selected groups must be on visible unlocked vector layers / 表示中でロックされていないベクターレイヤーのグループを選択してください / 请选择可见且未锁定矢量图层中的组".into());
        }
        self.finish();
        let before = self.vector_history_state();
        for (layer_index, layer) in self.svg_layers.iter_mut().enumerate() {
            for object in &mut layer.vector_objects {
                if object.group_path.first().is_some_and(|root| {
                    roots
                        .iter()
                        .any(|(index, target)| *index == layer_index && target == root)
                }) {
                    if all {
                        object.group_path.clear();
                    } else {
                        object.group_path.remove(0);
                    }
                }
            }
        }
        for layer in &mut self.svg_layers {
            for object in &mut layer.vector_objects {
                if object
                    .clipping_group
                    .as_ref()
                    .is_some_and(|g| !object.group_path.contains(g))
                {
                    object.clipping_group = None;
                }
            }
            if layer.vector_layer {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
            }
        }
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn edit_selected_paths(&mut self, action: PathEditAction) -> Result<(), String> {
        if self.selected_vector_objects.is_empty() {
            return Err(
                "Select one or more paths / 1つ以上のパスを選択してください / 请选择一个或多个路径"
                    .into(),
            );
        }
        let selected = self.selected_vector_objects.clone();
        let mut layers = self.svg_layers.clone();
        let mut changed = false;
        for layer in &mut layers {
            let mut layer_changed = false;
            for object in &mut layer.vector_objects {
                if !selected.contains(&object.id) {
                    continue;
                }
                if layer.locked
                    || !layer.visible
                    || !object.visible
                    || self.object_is_locked(&object.id)
                {
                    return Err("Selected path is hidden or locked / 選択したパスが非表示またはロックされています / 所选路径已隐藏或锁定".into());
                }
                if object.kind == VectorObjectKind::Text
                    || object.kind == VectorObjectKind::Compound
                {
                    return Err("This object is not an editable path / このオブジェクトは編集可能なパスではありません / 此对象不是可编辑路径".into());
                }
                match action {
                    PathEditAction::Join
                    | PathEditAction::Average
                    | PathEditAction::Outline
                    | PathEditAction::Offset
                    | PathEditAction::DivideBelow
                    | PathEditAction::SplitGrid => {
                        return Err("This path operation requires the platform path engine".into())
                    }
                    PathEditAction::Reverse => crate::bezier::reverse(object)?,
                    PathEditAction::Simplify => crate::bezier::simplify(object, 2.0)?,
                    PathEditAction::Smooth => crate::bezier::smooth(object)?,
                    PathEditAction::AddAnchors => crate::bezier::subdivide_all(object)?,
                    PathEditAction::RemoveAnchors => {
                        crate::bezier::remove_alternate_anchors(object)?
                    }
                    PathEditAction::CleanUp => crate::bezier::clean_up(object)?,
                }
                layer_changed = true;
                changed = true;
            }
            if layer_changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if !changed {
            return Err(
                "Selected path was not found / 選択したパスが見つかりません / 找不到所选路径"
                    .into(),
            );
        }
        if layers.iter().map(|layer| layer.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn split_selected_paths(
        &mut self,
        mut split: impl FnMut(&VectorObject) -> Result<Vec<PortablePathGeometry>, String>,
    ) -> Result<(), String> {
        let selected = self.selected_vector_objects.clone();
        if selected.is_empty() {
            return Err("Select one or more paths".into());
        }
        let mut layers = self.svg_layers.clone();
        let mut selected_pieces = Vec::new();
        let mut serial = self.revision + 1;
        for layer in &mut layers {
            if layer.locked || !layer.visible {
                continue;
            }
            let mut output = Vec::with_capacity(layer.vector_objects.len());
            let mut changed = false;
            for object in &layer.vector_objects {
                if !selected.contains(&object.id) {
                    output.push(object.clone());
                    continue;
                }
                for (index, (path, points)) in split(object)?.into_iter().enumerate() {
                    let mut piece = object.clone();
                    piece.id = format!("split-{serial}-{index}");
                    piece.name = format!("{} {}", object.name, index + 1);
                    piece.path = path;
                    piece.control_points = points;
                    piece.transform = [1., 0., 0., 1., 0., 0.];
                    piece.kind = VectorObjectKind::Compound;
                    piece.validate()?;
                    selected_pieces.push(piece.id.clone());
                    output.push(piece);
                }
                serial += 1;
                changed = true;
            }
            if changed {
                layer.vector_objects = output;
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if selected_pieces.is_empty() {
            return Err("The selected path could not be split".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.selected_vector_objects = selected_pieces;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn average_vector_controls(&mut self, controls: &[(String, usize)]) -> Result<(), String> {
        if controls.len() < 2 {
            return Err("Select at least two anchor points / 2つ以上のアンカーポイントを選択してください / 请选择至少两个锚点".into());
        }
        let mut world = Vec::with_capacity(controls.len());
        for (id, index) in controls {
            let object = self
                .svg_layers
                .iter()
                .flat_map(|layer| &layer.vector_objects)
                .find(|object| &object.id == id)
                .ok_or("Anchor path not found")?;
            let edited = crate::bezier::editable(object).ok_or("Path cannot be edited")?;
            let point = *edited
                .control_points
                .get(*index)
                .ok_or("Anchor point not found")?;
            world.push(crate::bezier::world_point(&edited, point));
        }
        let average = world.iter().fold([0., 0.], |sum, point| {
            [sum[0] + point[0], sum[1] + point[1]]
        });
        let average = [
            average[0] / world.len() as f32,
            average[1] / world.len() as f32,
        ];
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            for object in &mut layer.vector_objects {
                let selected: Vec<_> = controls
                    .iter()
                    .zip(&world)
                    .filter(|((id, _), _)| id == &object.id)
                    .map(|((_, index), point)| (*index, *point))
                    .collect();
                if selected.is_empty() {
                    continue;
                }
                if layer.locked
                    || !layer.visible
                    || !object.visible
                    || self.object_is_locked(&object.id)
                {
                    return Err("Selected path is hidden or locked".into());
                }
                let mut edited = crate::bezier::editable(object).ok_or("Path cannot be edited")?;
                for (index, point) in selected {
                    crate::bezier::translate_controls(
                        &mut edited,
                        &[index],
                        [average[0] - point[0], average[1] - point[1]],
                        false,
                    )?;
                }
                *object = edited;
            }
            layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
            validate_svg_layer(layer)?;
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn join_selected_paths(&mut self) -> Result<(), String> {
        self.join_path_endpoints(&[])
    }

    pub fn join_path_endpoints(&mut self, controls: &[(String, usize)]) -> Result<(), String> {
        const ENDPOINT_ERROR: &str = "Select two endpoints of open paths / 開いたパスの端点を2つ選択してください / 请选择开放路径的两个端点";
        if !controls.is_empty() {
            if controls.len() != 2 || controls[0] == controls[1] {
                return Err(ENDPOINT_ERROR.into());
            }
            for (id, index) in controls {
                let object = self
                    .svg_layers
                    .iter()
                    .filter(|layer| {
                        self.can_edit_path_layer(layer) && layer.visible && !layer.locked
                    })
                    .flat_map(|layer| &layer.vector_objects)
                    .find(|object| {
                        &object.id == id
                            && object.visible
                            && self.selected_vector_objects.contains(id)
                    })
                    .and_then(crate::bezier::editable)
                    .ok_or(ENDPOINT_ERROR)?;
                if object.path.data.trim_end().ends_with(['z', 'Z'])
                    || object.control_points.len() < 4
                    || (*index != 0 && *index != object.control_points.len() - 1)
                {
                    return Err(ENDPOINT_ERROR.into());
                }
            }
            if controls[0].0 == controls[1].0 {
                let layer = self
                    .svg_layers
                    .iter()
                    .find(|layer| {
                        self.can_edit_path_layer(layer)
                            && layer
                                .vector_objects
                                .iter()
                                .any(|object| object.id == controls[0].0)
                    })
                    .ok_or(ENDPOINT_ERROR)?;
                let layer_id = layer.id.clone();
                let mut object = crate::bezier::editable(
                    layer
                        .vector_objects
                        .iter()
                        .find(|object| object.id == controls[0].0)
                        .unwrap(),
                )
                .ok_or(ENDPOINT_ERROR)?;
                let first = object.control_points[0];
                let last = *object.control_points.last().unwrap();
                if coincident_path_endpoints(
                    crate::bezier::world_point(&object, first),
                    crate::bezier::world_point(&object, last),
                ) {
                    let end = object.control_points.len() - 1;
                    // Closed cubic paths repeat the first anchor as the final
                    // segment endpoint; it is one logical, editable anchor.
                    object.control_points[end] = first;
                    for axis in 0..2 {
                        object.control_points[end - 1][axis] += first[axis] - last[axis];
                    }
                } else {
                    object.control_points.extend([last, first, first]);
                }
                object.path.data = crate::bezier::path_data(&object.control_points, true)?;
                return self.upsert_vector_object(&layer_id, object);
            }
        }
        if self.selected_vector_objects.len() != 2 {
            return Err("Select exactly two open paths / 2本の開いたパスを選択してください / 请选择两条开放路径".into());
        }
        let selected = self.selected_vector_objects.clone();
        let layer_index = self
            .svg_layers
            .iter()
            .position(|layer| {
                layer
                    .vector_objects
                    .iter()
                    .filter(|object| selected.contains(&object.id))
                    .count()
                    == 2
            })
            .ok_or("Paths must be on the same vector layer")?;
        let layer = &self.svg_layers[layer_index];
        if layer.locked || !layer.visible {
            return Err("Vector layer is hidden or locked".into());
        }
        let positions: Vec<_> = layer
            .vector_objects
            .iter()
            .enumerate()
            .filter(|(_, object)| selected.contains(&object.id))
            .map(|(index, _)| index)
            .collect();
        let mut first = crate::bezier::editable(&layer.vector_objects[positions[0]])
            .ok_or("Path cannot be edited")?;
        let mut second = crate::bezier::editable(&layer.vector_objects[positions[1]])
            .ok_or("Path cannot be edited")?;
        if first.path.data.trim_end().ends_with(['Z', 'z'])
            || second.path.data.trim_end().ends_with(['Z', 'z'])
        {
            return Err(
                "Join requires open paths / 連結できるのは開いたパスです / 只能连接开放路径".into(),
            );
        }
        for object in [&mut first, &mut second] {
            object.control_points = object
                .control_points
                .iter()
                .map(|point| crate::bezier::world_point(object, *point))
                .collect();
            object.transform = [1., 0., 0., 1., 0., 0.];
        }
        let endpoints = [
            (
                false,
                false,
                distance2(
                    *first.control_points.last().unwrap(),
                    second.control_points[0],
                ),
            ),
            (
                false,
                true,
                distance2(
                    *first.control_points.last().unwrap(),
                    *second.control_points.last().unwrap(),
                ),
            ),
            (
                true,
                false,
                distance2(first.control_points[0], second.control_points[0]),
            ),
            (
                true,
                true,
                distance2(
                    first.control_points[0],
                    *second.control_points.last().unwrap(),
                ),
            ),
        ];
        let (reverse_first, reverse_second) = if controls.is_empty() {
            let (a, b, _) = *endpoints.iter().min_by(|a, b| a.2.total_cmp(&b.2)).unwrap();
            (a, b)
        } else {
            let first_index = controls
                .iter()
                .find(|(id, _)| id == &first.id)
                .ok_or(ENDPOINT_ERROR)?
                .1;
            let second_index = controls
                .iter()
                .find(|(id, _)| id == &second.id)
                .ok_or(ENDPOINT_ERROR)?
                .1;
            (first_index == 0, second_index != 0)
        };
        if reverse_first {
            crate::bezier::reverse(&mut first)?;
        }
        if reverse_second {
            crate::bezier::reverse(&mut second)?;
        }
        let end = *first.control_points.last().unwrap();
        let start = second.control_points[0];
        if coincident_path_endpoints(end, start) {
            // Keep the incoming handle from the first curve and the outgoing
            // handle from the second, with a single shared anchor between them.
            for axis in 0..2 {
                second.control_points[1][axis] += end[axis] - start[axis];
            }
        } else {
            first.control_points.extend([end, start, start]);
        }
        first
            .control_points
            .extend_from_slice(&second.control_points[1..]);
        first.path.data = crate::bezier::path_data(&first.control_points, false)?;
        first.kind = VectorObjectKind::Bezier;
        first.validate()?;
        let mut updated = layer.clone();
        updated.vector_objects[positions[0]] = first;
        updated.vector_objects.remove(positions[1]);
        updated.source = vector_svg(self.width, self.height, &updated.vector_objects);
        validate_svg_layer(&updated)?;
        self.finish();
        let before = self.vector_history_state();
        let joined_id = updated.vector_objects[positions[0]].id.clone();
        self.svg_layers[layer_index] = updated;
        self.selected_vector_objects = vec![joined_id];
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    fn is_path_edit_layer(&self, layer: &SvgLayer) -> bool {
        self.path_editing
            .as_ref()
            .is_some_and(|edit| edit.layer_id == layer.id)
    }
    pub fn can_edit_path_layer(&self, layer: &SvgLayer) -> bool {
        self.path_editing.is_none() || self.is_path_edit_layer(layer)
    }
    fn sync_saved_path(&mut self) {
        if let Some(edit) = &self.path_editing {
            if let (Some(layer), Some(path)) = (
                self.svg_layers.iter().find(|l| l.id == edit.layer_id),
                self.saved_paths.iter_mut().find(|p| p.id == edit.id),
            ) {
                path.objects = layer
                    .vector_objects
                    .iter()
                    .cloned()
                    .map(|mut object| {
                        if let Some((_, stored_id)) = edit
                            .object_ids
                            .iter()
                            .find(|(editing_id, _)| editing_id == &object.id)
                        {
                            object.id = stored_id.clone();
                        }
                        object
                    })
                    .collect();
            }
        }
    }
    fn end_path_editing(&mut self) {
        self.sync_saved_path();
        if let Some(edit) = self.path_editing.take() {
            let previous = self.svg_layers.clone();
            self.svg_layers.retain(|layer| layer.id != edit.layer_id);
            self.scene_journal
                .layers_changed(&previous, &self.svg_layers);
            self.selected_layer = edit.previous_layer;
            self.selected_vector_objects.clear();
        }
    }
    fn activate_saved_path(&mut self, id: &str) -> Result<(), String> {
        let mut path = self
            .saved_paths
            .iter()
            .find(|p| p.id == id)
            .ok_or("Path not found")?
            .clone();
        self.finish();
        self.end_path_editing();
        let mut object_ids = Vec::new();
        let object_count = path.objects.len().max(1);
        for (index, object) in path.objects.iter_mut().enumerate() {
            let stored_id = object.id.clone();
            let mut serial = index;
            loop {
                let editing_id = format!("{}-object-{serial}", path.id);
                if !self
                    .svg_layers
                    .iter()
                    .flat_map(|l| &l.vector_objects)
                    .any(|o| o.id == editing_id)
                    && !object_ids.iter().any(|(id, _)| id == &editing_id)
                {
                    object.id = editing_id.clone();
                    object_ids.push((editing_id, stored_id));
                    break;
                }
                serial += object_count;
            }
        }
        let mut layer_id = format!("edit-{}", path.id);
        while self.svg_layers.iter().any(|l| l.id == layer_id) {
            layer_id.push('_');
        }
        self.path_editing = Some(PathEditing {
            object_ids,
            id: path.id,
            layer_id: layer_id.clone(),
            previous_layer: self.selected_layer.clone(),
        });
        self.selected_layer = Some(layer_id.clone());
        self.selected_vector_objects.clear();
        let previous = self.svg_layers.clone();
        self.svg_layers.push(SvgLayer {
            id: layer_id,
            name: path.name,
            visible: true,
            opacity: 1.,
            locked: false,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
            source: vector_svg(self.width, self.height, &path.objects),
            paint_layer: false,
            vector_layer: true,
            vector_objects: path.objects,
        });
        self.scene_journal
            .layers_changed(&previous, &self.svg_layers);
        Ok(())
    }
    pub fn append_vector_guide(
        &mut self,
        layer_id: &str,
        object: VectorObject,
    ) -> Result<(), String> {
        if self.path_editing.is_none()
            && self
                .svg_layers
                .iter()
                .any(|l| l.id == layer_id && l.vector_layer)
        {
            return self.upsert_vector_object(layer_id, object);
        }
        object.validate()?;
        let previous = self.svg_layers.clone();
        self.svg_layers.push(SvgLayer {
            id: format!("guide-{}", self.svg_layers.len()),
            name: "Path guide".into(),
            visible: true,
            opacity: 1.,
            locked: true,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
            source: vector_svg(self.width, self.height, std::slice::from_ref(&object)),
            paint_layer: false,
            vector_layer: true,
            vector_objects: vec![object],
        });
        self.scene_journal
            .layers_changed(&previous, &self.svg_layers);
        Ok(())
    }
    pub fn has_active_saved_path(&self) -> bool {
        self.path_editing.is_some()
    }
    /// Saved paths are editing guides, never artwork or export content.
    pub fn layer_guide_color(&self, id: &str) -> [u8; 4] {
        let id = self
            .path_editing
            .as_ref()
            .filter(|edit| edit.layer_id == id)
            .map_or(id, |edit| edit.id.as_str());
        path_guide_color(id)
    }

    pub fn saved_path_edit_view(&self, width: f32) -> Self {
        let mut doc = self.clone_for_rendering();
        let Some(edit) = &self.path_editing else {
            return doc;
        };
        let Some(mut layer) = self
            .svg_layers
            .iter()
            .find(|layer| layer.id == edit.layer_id)
            .cloned()
        else {
            return doc;
        };
        doc.svg_layers.retain(|layer| layer.id != edit.layer_id);
        for object in &mut layer.vector_objects {
            if let Some(mut curve) = crate::bezier::editable(object) {
                curve.control_points = curve
                    .control_points
                    .iter()
                    .map(|p| crate::bezier::world_point(&curve, *p))
                    .collect();
                curve.transform = [1., 0., 0., 1., 0., 0.];
                if let Ok(data) = crate::bezier::path_data(
                    &curve.control_points,
                    curve.path.data.trim_end().ends_with(['Z', 'z']),
                ) {
                    curve.path.data = data;
                    *object = curve;
                }
            }
            object.fill_gradient = None;
            object.stroke_gradient = None;
            object.fill = None;
            object.stroke = Some(crate::vector::VectorPaint {
                registration: false,
                color: self.layer_guide_color(&edit.layer_id),
            });
            object.stroke_width = width;
            object.stroke_style = Default::default();
        }
        layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
        doc.svg_layers.push(layer);
        doc.path_editing = None;
        doc
    }

    pub fn saved_path_action(
        &mut self,
        action: &str,
        id: Option<&str>,
        name: &str,
    ) -> Result<(), String> {
        if action == "activate" {
            return self.activate_saved_path(id.ok_or("Path not found")?);
        }
        if action == "deactivate" {
            self.end_path_editing();
            return Ok(());
        }
        let mut paths = self.saved_paths.clone();
        let mut clip = self.clipping_path_id.clone();
        match action {
            "new" => {
                let mut serial = 1;
                while paths.iter().any(|p| p.id == format!("saved-path-{serial}")) {
                    serial += 1;
                }
                paths.push(SavedPath {
                    id: format!("saved-path-{serial}"),
                    name: name.trim().to_string(),
                    objects: Vec::new(),
                });
            }
            "create" | "update" => {
                let objects: Vec<_> = self
                    .svg_layers
                    .iter()
                    .filter(|l| l.visible && l.vector_layer)
                    .flat_map(|l| &l.vector_objects)
                    .filter(|o| o.visible && self.selected_vector_objects.contains(&o.id))
                    .cloned()
                    .collect();
                if objects.is_empty()
                    || objects
                        .iter()
                        .any(|o| o.text.is_some() || !o.path.data.trim_end().ends_with(['z', 'Z']))
                {
                    return Err("Select closed vector paths / 閉じたベクターパスを選択してください / 请选择闭合矢量路径".into());
                }
                if action == "update" {
                    paths
                        .iter_mut()
                        .find(|p| Some(p.id.as_str()) == id)
                        .ok_or("Path not found")?
                        .objects = objects;
                } else {
                    let mut serial = 1;
                    while paths.iter().any(|p| p.id == format!("saved-path-{serial}")) {
                        serial += 1;
                    }
                    paths.push(SavedPath {
                        id: format!("saved-path-{serial}"),
                        name: name.trim().to_string(),
                        objects,
                    });
                }
            }
            "rename" => {
                paths
                    .iter_mut()
                    .find(|p| Some(p.id.as_str()) == id)
                    .ok_or("Path not found")?
                    .name = name.trim().to_string()
            }
            "delete" => {
                if !paths.iter().any(|p| Some(p.id.as_str()) == id) {
                    return Err("Path not found".into());
                }
                paths.retain(|p| Some(p.id.as_str()) != id);
                if clip.as_deref() == id {
                    clip = None;
                }
            }
            "clip" => {
                let path = paths
                    .iter()
                    .find(|p| Some(p.id.as_str()) == id)
                    .ok_or("Path not found")?;
                if path.objects.is_empty()
                    || path
                        .objects
                        .iter()
                        .any(|o| !o.path.data.trim_end().ends_with(['z', 'Z']))
                {
                    return Err("Close the path before clipping / 閉じたパスを描いてから指定してください / 请先绘制闭合路径".into());
                }
                clip = id.map(str::to_string);
            }
            "release" => clip = None,
            _ => return Err("Unknown path operation".into()),
        }
        validate_saved_paths(&paths, clip.as_deref())?;
        self.finish();
        let before = self.vector_history_state();
        if action == "delete"
            && self
                .path_editing
                .as_ref()
                .is_some_and(|edit| Some(edit.id.as_str()) == id)
        {
            self.end_path_editing();
        }
        self.saved_paths = paths;
        self.clipping_path_id = clip;
        self.record_vector_edit(before);
        self.revision += 1;
        if action == "new" {
            let id = self.saved_paths.last().unwrap().id.clone();
            self.activate_saved_path(&id)?;
        }
        Ok(())
    }

    pub fn clipping_mask_svg(&self) -> Option<String> {
        let path = self
            .saved_paths
            .iter()
            .find(|path| Some(&path.id) == self.clipping_path_id.as_ref())?;
        let objects: Vec<_> = path
            .objects
            .iter()
            .cloned()
            .map(|mut object| {
                object.fill_gradient = None;
                object.stroke_gradient = None;
                object.fill = Some(crate::vector::VectorPaint {
                    registration: false,
                    color: [255; 4],
                });
                object.stroke = None;
                object
            })
            .collect();
        Some(vector_svg(self.width, self.height, &objects))
    }

    pub fn has_document_clipping(&self) -> bool {
        self.clipping_path_id.is_some()
    }

    /// Coverage outside the document clip is presented as transparency, after
    /// every paint/image/vector layer. This transient layer is never serialized.
    pub fn clipping_view(&self) -> Self {
        use std::fmt::Write;
        let mut doc = self.clone_for_rendering();
        let Some(path) = self
            .saved_paths
            .iter()
            .find(|p| Some(&p.id) == self.clipping_path_id.as_ref())
        else {
            return doc;
        };
        let mut shapes = String::new();
        for object in &path.objects {
            let [a, b, c, d, e, f] = object.transform;
            let rule = match object.path.fill_rule {
                crate::vector::FillRule::EvenOdd => "evenodd",
                _ => "nonzero",
            };
            let _ = write!(
                shapes,
                r#"<path d="{}" transform="matrix({a} {b} {c} {d} {e} {f})" fill="black" fill-rule="{rule}"/>"#,
                escape_xml(&object.path.data)
            );
        }
        let (w, h) = (self.width, self.height);
        let source = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><defs><mask id="outside" maskUnits="userSpaceOnUse" x="0" y="0" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="white"/>{shapes}</mask><pattern id="checker" width="16" height="16" patternUnits="userSpaceOnUse"><rect width="16" height="16" fill="#ededed"/><path d="M0 0H8V8H0ZM8 8H16V16H8Z" fill="#cccccc"/></pattern></defs><rect width="{w}" height="{h}" fill="url(#checker)" mask="url(#outside)"/></svg>"##
        );
        let mut id = "document-clip-preview".to_string();
        while doc.svg_layers.iter().any(|l| l.id == id) {
            id.push('_');
        }
        let index = doc
            .svg_layers
            .iter()
            .position(|layer| {
                layer.name == "Path guide" && layer.locked && layer.id.starts_with("guide-")
            })
            .unwrap_or(doc.svg_layers.len());
        doc.svg_layers.insert(
            index,
            SvgLayer {
                id,
                name: "Clipping preview".into(),
                visible: true,
                opacity: 1.,
                locked: true,
                alpha_locked: false,
                mask_enabled: false,
                mask_inverted: false,
                mask_density: 1.,
                source,
                paint_layer: false,
                vector_layer: false,
                vector_objects: Vec::new(),
            },
        );
        doc
    }

    pub fn set_text_writing_mode(
        &mut self,
        mode: crate::vector::WritingMode,
        mut reflow: impl FnMut(&mut crate::vector::VectorText, [u8; 3]) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut layers = self.svg_layers.clone();
        let mut changed = false;
        for layer in &mut layers {
            let mut layer_changed = false;
            for object in &mut layer.vector_objects {
                if !self.selected_vector_objects.contains(&object.id) {
                    continue;
                }
                let Some(text) = &mut object.text else {
                    continue;
                };
                if layer.locked
                    || !layer.visible
                    || !object.visible
                    || self.object_is_locked(&object.id)
                {
                    return Err("Unlock and show text / 文字のロックを解除し表示してください / 请解锁并显示文字".into());
                }
                if text.writing_mode == mode {
                    continue;
                }
                // Point text is anchored to its first glyph, independent of the
                // container width used by a vertical layout engine.
                let anchor = text.point_text.then(|| text.control_points()[0]);
                text.writing_mode = mode;
                if text.point_text && mode == crate::vector::WritingMode::Vertical {
                    text.box_width = (text.font_size * text.line_height).clamp(16., 8192.);
                }
                text.clear_measured_layout();
                let color = object
                    .fill
                    .map_or([0, 0, 0], |p| [p.color[0], p.color[1], p.color[2]]);
                reflow(text, color)?;
                object.control_points = text.control_points();
                if let Some(anchor) = anchor {
                    let next = object.control_points[0];
                    let [a, b, c, d, _, _] = object.transform;
                    let delta = [anchor[0] - next[0], anchor[1] - next[1]];
                    object.transform[4] += a * delta[0] + c * delta[1];
                    object.transform[5] += b * delta[0] + d * delta[1];
                }
                layer_changed = true;
            }
            if layer_changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
                changed = true;
            }
        }
        if changed {
            self.finish();
            let before = self.vector_history_state();
            self.svg_layers = layers;
            self.record_vector_edit(before);
            self.revision += 1;
        }
        Ok(())
    }

    /// Transient inspection projection; never stored in project data or history.
    pub fn outline_view(&self, offset: [f32; 2], line_width: f32) -> Self {
        let mut preview = self.clone_for_rendering();
        if offset != [0., 0.] {
            let _ = preview.move_selected_vectors_preview(offset[0], offset[1]);
        }
        for layer in &mut preview.svg_layers {
            if !layer.vector_layer {
                continue;
            }
            // Outline mode shows the editable centerline, without generated stroke outlines or arrowheads.
            for object in &mut layer.vector_objects {
                object.stroke_style = Default::default();
            }
            layer.source = vector_svg(preview.width, preview.height, &layer.vector_objects);
            // CSS covers vector geometry and shaped text. Clipping and
            // masks are disabled only in this projection so their paths can be inspected.
            let style = format!(
                r#"<style>path,rect,circle,ellipse,line,polyline,polygon,text,tspan {{ fill: none !important; stroke: #222 !important; stroke-width: {line_width} !important; stroke-opacity: 1 !important; stroke-dasharray: none !important; }} * {{ clip-path: none !important; mask: none !important; filter: none !important; }}</style>"#
            );
            if let Some(end) = layer.source.rfind("</svg>") {
                layer.source.insert_str(end, &style);
            }
        }
        preview
    }

    pub fn outline_selected_text(
        &mut self,
        mut outline: impl FnMut(&str, &VectorObject) -> Result<Vec<VectorObject>, String>,
    ) -> Result<(), String> {
        let selected = self.selected_vector_objects.clone();
        let mut layers = self.svg_layers.clone();
        let mut ids = Vec::new();
        let mut count = 0;
        let mut occupied: std::collections::HashSet<String> = layers
            .iter()
            .flat_map(|l| {
                l.vector_objects
                    .iter()
                    .flat_map(|o| std::iter::once(o.id.clone()).chain(o.group_path.iter().cloned()))
            })
            .collect();
        occupied.extend(self.reserved_compound_ids());
        let mut serial = 0;
        let mut fresh = || loop {
            serial += 1;
            let id = format!("outline-{}-{serial}", self.revision);
            if occupied.insert(id.clone()) {
                break id;
            }
        };
        for layer in &mut layers {
            let mut objects = Vec::new();
            let mut changed = false;
            for object in &layer.vector_objects {
                if !selected.contains(&object.id) || object.text.is_none() {
                    objects.push(object.clone());
                    if selected.contains(&object.id) {
                        ids.push(object.id.clone());
                    }
                    continue;
                }
                if layer.locked
                    || !layer.visible
                    || !object.visible
                    || self.object_is_locked(&object.id)
                {
                    return Err("Unlock and show selected text / 選択した文字のロックを解除し表示してください / 请解锁并显示所选文字".into());
                }
                let mut source_object = object.clone();
                source_object.group_path.clear();
                source_object.clipping_group = None;
                let source = vector_svg(self.width, self.height, &[source_object]);
                let replacements = outline(&source, object)?;
                if replacements.is_empty() {
                    return Err("No outlineable glyphs / アウトライン化できる文字がありません / 没有可转换的字形".into());
                }
                let group = fresh();
                for mut replacement in replacements {
                    replacement.id = fresh();
                    replacement.group_path = object.group_path.clone();
                    replacement.group_path.push(group.clone());
                    replacement.text = None;
                    replacement.clipping_group = None;
                    replacement.validate()?;
                    ids.push(replacement.id.clone());
                    objects.push(replacement);
                }
                count += 1;
                changed = true;
            }
            if changed {
                if objects.len() > 4096 {
                    return Err(
                        "Too many outlines / アウトライン数が上限を超えています / 轮廓数量超出限制"
                            .into(),
                    );
                }
                layer.vector_objects = objects;
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if count == 0 {
            return Err("Select text / テキストを選択してください / 请选择文字".into());
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much outline data".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.selected_vector_objects = ids;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    pub fn replace_selected_path_geometry(
        &mut self,
        mut replace: impl FnMut(&VectorObject) -> Result<(VectorPath, Vec<[f32; 2]>), String>,
        outline: bool,
    ) -> Result<(), String> {
        if self.selected_vector_objects.is_empty() {
            return Err(
                "Select one or more paths / 1つ以上のパスを選択してください / 请选择一个或多个路径"
                    .into(),
            );
        }
        let selected = self.selected_vector_objects.clone();
        let mut layers = self.svg_layers.clone();
        let mut changed = false;
        for layer in &mut layers {
            let mut layer_changed = false;
            for object in &mut layer.vector_objects {
                if !selected.contains(&object.id) {
                    continue;
                }
                if layer.locked
                    || !layer.visible
                    || !object.visible
                    || self.object_is_locked(&object.id)
                {
                    return Err("Selected path is hidden or locked / 選択したパスが非表示またはロックされています / 所选路径已隐藏或锁定".into());
                }
                let (path, points) = replace(object)?;
                object.path = path;
                object.control_points = points;
                object.transform = [1., 0., 0., 1., 0., 0.];
                object.kind = VectorObjectKind::Compound;
                if outline {
                    if object.stroke.is_some() {
                        object.fill_gradient = object.stroke_gradient.take();
                    }
                    object.fill = object.stroke.or(object.fill);
                    object.stroke = None;
                    object.stroke_gradient = None;
                    object.stroke_width = 0.;
                }
                object.validate()?;
                layer_changed = true;
                changed = true;
            }
            if layer_changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if !changed {
            return Err("Selected path was not found".into());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
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

    /// Arrange whole top-level groups in painter order (back to front).
    pub fn arrange_selected_vectors(&mut self, action: &str) -> Result<(), String> {
        if !matches!(
            action,
            "front" | "forward" | "backward" | "back" | "moveToLayer"
        ) {
            return Err("Unknown arrange action".into());
        }
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        if selected.is_empty() {
            return Err("Select objects / オブジェクトを選択してください / 请选择对象".into());
        }
        for layer in &self.svg_layers {
            if layer
                .vector_objects
                .iter()
                .any(|o| selected.contains(&o.id))
                && (!self.can_edit_path_layer(layer)
                    || !layer.vector_layer
                    || layer.locked
                    || !layer.visible)
            {
                return Err("Select unlocked, visible vector layers / 表示中のロックされていないベクターレイヤーを選択してください / 请选择可见且未锁定的矢量图层".into());
            }
        }
        let mut layers = self.svg_layers.clone();
        if action == "moveToLayer" {
            let target = layers.iter().position(|l| Some(&l.id) == self.selected_layer.as_ref()
                && l.vector_layer && l.visible && !l.locked && !self.is_path_edit_layer(l))
                .filter(|_| self.path_editing.is_none())
                .ok_or("Select an unlocked vector destination layer / 移動先のロックされていないベクターレイヤーを選択してください / 请选择未锁定的目标矢量图层")?;
            let mut moving = Vec::new();
            for layer in &mut layers {
                layer.vector_objects.retain(|object| {
                    if selected.contains(&object.id) {
                        moving.push(object.clone());
                        false
                    } else {
                        true
                    }
                });
            }
            layers[target].vector_objects.extend(moving);
        } else {
            for layer in &mut layers {
                if !layer
                    .vector_objects
                    .iter()
                    .any(|o| selected.contains(&o.id))
                {
                    continue;
                }
                let mut units: Vec<Vec<VectorObject>> = Vec::new();
                for object in &layer.vector_objects {
                    let same_group = object.group_path.first().is_some_and(|root| {
                        units
                            .last()
                            .is_some_and(|unit| unit[0].group_path.first() == Some(root))
                    });
                    if same_group {
                        units.last_mut().unwrap().push(object.clone());
                    } else {
                        units.push(vec![object.clone()]);
                    }
                }
                let chosen =
                    |unit: &Vec<VectorObject>| unit.iter().any(|o| selected.contains(&o.id));
                match action {
                    "front" => units.sort_by_key(|unit| chosen(unit)),
                    "back" => units.sort_by_key(|unit| !chosen(unit)),
                    "forward" => {
                        for i in (0..units.len().saturating_sub(1)).rev() {
                            if chosen(&units[i]) && !chosen(&units[i + 1]) {
                                units.swap(i, i + 1);
                            }
                        }
                    }
                    "backward" => {
                        for i in 1..units.len() {
                            if chosen(&units[i]) && !chosen(&units[i - 1]) {
                                units.swap(i, i - 1);
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                layer.vector_objects = units.into_iter().flatten().collect();
            }
        }
        let mut changed = false;
        for (layer, original) in layers.iter_mut().zip(&self.svg_layers) {
            if layer.vector_objects != original.vector_objects {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
                changed = true;
            }
        }
        if !changed {
            return Ok(());
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.selected_vector_objects = selected;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    /// Layer-panel activation can choose a destination without losing artwork selection.
    pub fn select_layer_preserving_objects(&mut self, id: String) -> Result<(), String> {
        let keep = self.path_editing.is_none()
            && self
                .svg_layers
                .iter()
                .any(|l| l.id == id && l.vector_layer && l.visible && !l.locked);
        let selected = self.selected_vector_objects.clone();
        self.select_layer(id)?;
        if keep {
            self.selected_vector_objects = selected;
        }
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
    /// Apply a portable pathfinder result atomically, in layer stacking order.
    pub fn pathfinder_selected_vectors(
        &mut self,
        operation: crate::vector::PathfinderOperation,
        compute: impl FnOnce(
            &[VectorObject],
            crate::vector::PathfinderOperation,
        ) -> Result<Vec<VectorObject>, String>,
    ) -> Result<(), String> {
        if !(2..=64).contains(&self.selected_vector_objects.len()) {
            return Err("Select 2 to 64 vector shapes".into());
        }
        let selected = &self.selected_vector_objects;
        let matches: Vec<_> = self
            .svg_layers
            .iter()
            .enumerate()
            .flat_map(|(li, layer)| {
                layer
                    .vector_objects
                    .iter()
                    .enumerate()
                    .filter_map(move |(oi, object)| {
                        selected.contains(&object.id).then_some((li, oi))
                    })
            })
            .collect();
        if matches.len() != selected.len() {
            return Err("Selected vector shapes are missing".into());
        }
        let cross_layer = matches.iter().any(|(li, _)| *li != matches[0].0);
        if cross_layer && self.svg_layers.len() >= 16 {
            return Err("Too many layers for a Pathfinder result layer".into());
        }
        let li = matches.last().unwrap().0;
        let objects: Vec<_> = matches
            .iter()
            .map(|(source_layer, oi)| {
                let source = &self.svg_layers[*source_layer];
                let mut object = source.vector_objects[*oi].clone();
                if cross_layer {
                    object.opacity *= source.effective_opacity();
                    for group in &mut object.group_path {
                        *group = format!("{}:{group}", source.id);
                    }
                    if let Some(group) = &mut object.clipping_group {
                        *group = format!("{}:{group}", source.id);
                    }
                }
                object
            })
            .collect();
        if matches
            .iter()
            .any(|(li, _)| self.svg_layers[*li].locked || !self.svg_layers[*li].visible)
            || objects.iter().any(|object| {
                !object.visible
                    || self.object_is_locked(&object.id)
                    || (object.fill.is_none()
                        && object.text.is_none()
                        && object.clipping_group.is_none())
            })
        {
            return Err("Select visible filled shapes on an unlocked layer".into());
        }
        for (li, oi) in &matches {
            let layer = &self.svg_layers[*li];
            let object = &layer.vector_objects[*oi];
            if let Some(group) = object.group_path.first().or(object.clipping_group.as_ref()) {
                if layer.vector_objects.iter().any(|o| {
                    (o.group_path.contains(group) || o.clipping_group.as_ref() == Some(group))
                        && !selected.contains(&o.id)
                }) {
                    return Err("Select the entire group or clipping group".into());
                }
            }
        }
        let mut results = compute(&objects, operation)?;
        if results.len() > 4096 {
            return Err("Pathfinder result exceeds 4096 objects".into());
        }
        let mut ids: std::collections::HashSet<String> = self
            .svg_layers
            .iter()
            .flat_map(|layer| &layer.vector_objects)
            .map(|object| object.id.clone())
            .collect();
        ids.extend(self.reserved_compound_ids());
        for (index, object) in results.iter_mut().enumerate() {
            let base = format!("pf-{}-{}", self.revision, index);
            let mut id = base.clone();
            let mut suffix = 0;
            while ids.contains(&id) {
                suffix += 1;
                id = format!("{base}-{suffix}");
            }
            ids.insert(id.clone());
            object.id = id;
            object.validate()?;
        }
        let mut layers = self.svg_layers.clone();
        let root = self
            .layer_groups
            .ancestors(&self.svg_layers[li].id)
            .last()
            .map_or_else(|| self.svg_layers[li].id.clone(), |g| g.id.clone());
        let mut root_leaves = Vec::new();
        self.layer_groups
            .flatten(std::slice::from_ref(&root), &mut root_leaves);
        let result_index = self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(_, l)| root_leaves.contains(&l.id))
            .map(|(i, _)| i + 1)
            .max()
            .unwrap_or(li + 1);
        let mut result_layer_id = None;
        if cross_layer {
            for layer in &mut layers {
                if layer
                    .vector_objects
                    .iter()
                    .any(|o| selected.contains(&o.id))
                {
                    layer.vector_objects.retain(|o| !selected.contains(&o.id));
                    layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                }
            }
            if !results.is_empty() {
                let mut id = format!("pathfinder-layer-{}", self.revision);
                while layers.iter().any(|l| l.id == id) {
                    id.push('x');
                }
                result_layer_id = Some(id.clone());
                layers.insert(
                    result_index,
                    SvgLayer {
                        id,
                        name: "Pathfinder".into(),
                        visible: true,
                        opacity: 1.,
                        locked: false,
                        alpha_locked: false,
                        mask_enabled: false,
                        mask_inverted: false,
                        mask_density: 1.,
                        source: vector_svg(self.width, self.height, &results),
                        paint_layer: false,
                        vector_layer: true,
                        vector_objects: results.clone(),
                    },
                );
            }
        } else {
            let insertion = matches.last().unwrap().1;
            let mut merged = Vec::new();
            for (oi, object) in layers[li].vector_objects.drain(..).enumerate() {
                if !selected.contains(&object.id) {
                    merged.push(object);
                }
                if oi == insertion {
                    merged.extend(results.iter().cloned());
                }
            }
            layers[li].vector_objects = merged;
            layers[li].source = vector_svg(self.width, self.height, &layers[li].vector_objects);
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        for layer in &layers {
            validate_svg_layer(layer)?;
        }
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.layer_groups.reconcile(&self.svg_layers);
        if let Some(id) = result_layer_id {
            self.layer_groups.roots.retain(|r| r != &id);
            let index = self
                .layer_groups
                .roots
                .iter()
                .position(|r| r == &root)
                .unwrap_or(0);
            self.layer_groups.roots.insert(index, id);
        }
        self.selected_layer = self
            .svg_layers
            .iter()
            .find(|l| {
                l.vector_objects
                    .iter()
                    .any(|o| results.iter().any(|r| r.id == o.id))
            })
            .map(|l| l.id.clone());
        self.selected_vector_objects = results.iter().map(|object| object.id.clone()).collect();
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }

    /// One oriented bounding box shared by the selected objects.
    // Preserve painter order: the first selected object's affine basis defines
    // the oriented selection box. Selection order itself must not change it.
    fn selected_drawing_objects(&self, editable_only: bool) -> Vec<&VectorObject> {
        if self.selected_vector_objects.is_empty() {
            return Vec::new();
        }
        let mut index = self.scene_picking.lock();
        index.retain(&self.svg_layers);
        let mut objects = Vec::new();
        for layer in self.svg_layers.iter().filter(|layer| {
            layer.visible
                && !layer.locked
                && (!editable_only || (layer.vector_layer && self.can_edit_path_layer(layer)))
        }) {
            let (positions, _) = index.positions(
                layer,
                &self.scene_journal,
                self.revision,
                &self.selected_vector_objects,
            );
            objects.extend(
                positions
                    .into_iter()
                    .map(|i| &layer.vector_objects[i])
                    .filter(|o| o.visible),
            );
        }
        objects
    }

    pub fn selected_vector_box(&self) -> Option<[[f32; 2]; 4]> {
        let objects = self.selected_drawing_objects(false);
        if let Some(first) = objects.first().filter(|o| !o.bounds_reset) {
            let [a, b, c, d, e, f] = first.transform;
            let det = a * d - b * c;
            if det.abs() > 1e-8 && objects.iter().all(|o| !o.bounds_reset) {
                let mut bounds = [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ];
                for object in &objects {
                    let inverse = |p: Point| Point {
                        x: (d * (p.x - e) - c * (p.y - f)) / det,
                        y: (-b * (p.x - e) + a * (p.y - f)) / det,
                    };
                    let mut drawn = vector_geometry_extrema(object, inverse);
                    drawn.extend(
                        object
                            .stroke_boundary_points()
                            .into_iter()
                            .map(|p| inverse(Point { x: p[0], y: p[1] })),
                    );
                    for p in drawn {
                        let (x, y) = (p.x, p.y);
                        bounds[0] = bounds[0].min(x);
                        bounds[1] = bounds[1].min(y);
                        bounds[2] = bounds[2].max(x);
                        bounds[3] = bounds[3].max(y);
                    }
                }
                if bounds.iter().all(|v| v.is_finite()) {
                    let [x, y, r, bottom] = bounds;
                    return Some(
                        [[x, y], [r, y], [r, bottom], [x, bottom]]
                            .map(|[x, y]| [a * x + c * y + e, b * x + d * y + f]),
                    );
                }
            }
        }
        self.selected_vector_bounds().map(|mut bounds| {
            for object in objects {
                for p in object.stroke_boundary_points() {
                    bounds[0] = bounds[0].min(p[0]);
                    bounds[1] = bounds[1].min(p[1]);
                    bounds[2] = bounds[2].max(p[0]);
                    bounds[3] = bounds[3].max(p[1]);
                }
            }
            let [x, y, r, b] = bounds;
            [[x, y], [r, y], [r, b], [x, b]]
        })
    }

    pub fn transform_selected_vectors(
        &mut self,
        action: &str,
        values: [f32; 4],
    ) -> Result<bool, String> {
        if values.iter().any(|v| !v.is_finite() || v.abs() > 100_000.) {
            return Err("Invalid transform values".into());
        }
        let Some([x, y, r, b]) = self.selected_vector_bounds() else {
            return Err("Select objects / オブジェクトを選択してください / 请选择对象".into());
        };
        if ![
            "move",
            "rotate",
            "reflect",
            "scale",
            "shear",
            "individual",
            "reset",
        ]
        .contains(&action)
        {
            return Err("Unknown transform".into());
        }
        if matches!(action, "scale" | "individual")
            && (!(0.01..=100.).contains(&values[0]) || !(0.01..=100.).contains(&values[1]))
        {
            return Err(
                "Scale must be 1–10000% / 倍率は1〜10000%です / 缩放比例应为1–10000%".into(),
            );
        }
        if action == "shear" && values[0].abs() >= 89. {
            return Err("Shear angle must be between -89 and 89 degrees".into());
        }
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        let mut layers = self.svg_layers.clone();
        for l in &mut layers {
            let mut changed = false;
            for o in &mut l.vector_objects {
                if !selected.contains(&o.id) {
                    continue;
                }
                if l.locked || !l.visible || !o.visible || self.object_is_locked(&o.id) {
                    return Err("Selected object is locked or hidden".into());
                }
                if action == "reset" {
                    o.bounds_reset = true;
                    changed = true;
                    continue;
                }
                let mut center = [(x + r) * 0.5, (y + b) * 0.5];
                if action == "individual" {
                    let points: Vec<_> = self
                        .svg_layers
                        .iter()
                        .filter(|source| source.id == l.id)
                        .flat_map(|source| &source.vector_objects)
                        .filter(|member| {
                            o.group_path.first().map_or(member.id == o.id, |root| {
                                member.group_path.first() == Some(root)
                            })
                        })
                        .flat_map(transformed_control_points)
                        .collect();
                    let minx = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
                    let maxx = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
                    let miny = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
                    let maxy = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
                    center = [(minx + maxx) * 0.5, (miny + maxy) * 0.5];
                }
                let [mut a, mut b, mut c, mut d] = [1., 0., 0., 1.];
                let mut offset = [0., 0.];
                match action {
                    "move" => offset = [values[0], values[1]],
                    "scale" => {
                        a = values[0];
                        d = values[1];
                    }
                    "rotate" | "individual" => {
                        let (sn, cs) = values[if action == "rotate" { 0 } else { 2 }]
                            .to_radians()
                            .sin_cos();
                        let sx = if action == "individual" {
                            values[0]
                        } else {
                            1.
                        };
                        let sy = if action == "individual" {
                            values[1]
                        } else {
                            1.
                        };
                        a = cs * sx;
                        b = sn * sx;
                        c = -sn * sy;
                        d = cs * sy;
                    }
                    "reflect" => {
                        let (sn, cs) = (2. * values[0].to_radians()).sin_cos();
                        a = cs;
                        b = sn;
                        c = sn;
                        d = -cs;
                    }
                    "shear" => c = values[0].to_radians().tan(),
                    _ => {}
                }
                let [oa, ob, oc, od, oe, of] = o.transform;
                o.transform = [
                    a * oa + c * ob,
                    b * oa + d * ob,
                    a * oc + c * od,
                    b * oc + d * od,
                    center[0] + a * (oe - center[0]) + c * (of - center[1]) + offset[0],
                    center[1] + b * (oe - center[0]) + d * (of - center[1]) + offset[1],
                ];
                o.bounds_reset = false;
                o.validate()?;
                changed = true;
            }
            if changed {
                l.source = vector_svg(self.width, self.height, &l.vector_objects);
                validate_svg_layer(l)?;
            }
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        let mask_updates = self.linked_mask_updates(&layers)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.layer_effects.extend(mask_updates);
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(true)
    }

    pub fn selected_vector_bounds(&self) -> Option<[f32; 4]> {
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for object in self.selected_drawing_objects(true) {
            for p in vector_geometry_extrema(object, |p| p) {
                bounds[0] = bounds[0].min(p.x);
                bounds[1] = bounds[1].min(p.y);
                bounds[2] = bounds[2].max(p.x);
                bounds[3] = bounds[3].max(p.y);
            }
        }
        bounds.iter().all(|v| v.is_finite()).then_some(bounds)
    }

    pub fn affine_selected_vectors(&mut self, matrix: [f32; 6]) -> Result<bool, String> {
        self.affine_vectors(matrix, false, true)
    }
    pub fn resize_selected_image_frames(
        &mut self,
        matrix: [f32; 6],
        scale_content: bool,
    ) -> Result<bool, String> {
        self.affine_vectors(matrix, !scale_content, true)
    }
    fn affine_vectors(
        &mut self,
        matrix: [f32; 6],
        preserve_frame_content: bool,
        record_history: bool,
    ) -> Result<bool, String> {
        if matrix.iter().any(|v| !v.is_finite())
            || (matrix[0] * matrix[3] - matrix[1] * matrix[2]).abs() < 0.000001
        {
            return Err("Invalid object transform".into());
        }
        if matrix == [1., 0., 0., 1., 0., 0.] || self.selected_vector_objects.is_empty() {
            return Ok(false);
        }
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            let mut changed = false;
            for o in &mut layer.vector_objects {
                if !selected.contains(&o.id) {
                    continue;
                }
                if layer.locked || !layer.visible || !o.visible || self.object_is_locked(&o.id) {
                    return Err("Selected object is locked or hidden / 選択対象がロックまたは非表示です / 所选对象已锁定或隐藏".into());
                }
                let [a, b, c, d, e, f] = matrix;
                let [oa, ob, oc, od, oe, of] = o.transform;
                o.transform = [
                    a * oa + c * ob,
                    b * oa + d * ob,
                    a * oc + c * od,
                    b * oc + d * od,
                    a * oe + c * of + e,
                    b * oe + d * of + f,
                ];
                if preserve_frame_content {
                    if let Some(frame) = &mut o.image_frame {
                        if let Some(content) = frame.content_transform {
                            frame.content_transform = Some(crate::image_frame::multiply(
                                crate::image_frame::multiply(
                                    crate::image_frame::inverse(o.transform)?,
                                    [oa, ob, oc, od, oe, of],
                                ),
                                content,
                            ));
                        }
                    }
                }
                o.bounds_reset = false;
                o.validate()?;
                changed = true;
            }
            if changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        let mask_updates = self.linked_mask_updates(&layers)?;
        self.finish();
        let before = record_history.then(|| self.vector_history_state());
        if !record_history {
            self.scene_journal.layers_changed(&self.svg_layers, &layers);
        }
        self.svg_layers = layers;
        self.layer_effects.extend(mask_updates);
        if let Some(before) = before {
            self.record_vector_edit(before);
        } else {
            self.sync_saved_path();
        }
        self.revision += 1;
        Ok(true)
    }

    pub fn scale_selected_vectors(&mut self, center: [f32; 2], scale: f32) -> Result<bool, String> {
        if !scale.is_finite()
            || !(0.01..=100.).contains(&scale)
            || center.iter().any(|v| !v.is_finite())
        {
            return Err("Invalid object scale".into());
        }
        if (scale - 1.).abs() < f32::EPSILON || self.selected_vector_objects.is_empty() {
            return Ok(false);
        }
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            let mut changed = false;
            for o in &mut layer.vector_objects {
                if !selected.contains(&o.id) {
                    continue;
                }
                if layer.locked || !layer.visible || !o.visible || self.object_is_locked(&o.id) {
                    return Err("Selected object is locked or hidden / 選択対象がロックまたは非表示です / 所选对象已锁定或隐藏".into());
                }
                for v in &mut o.transform[..4] {
                    *v *= scale;
                }
                o.transform[4] = center[0] + (o.transform[4] - center[0]) * scale;
                o.transform[5] = center[1] + (o.transform[5] - center[1]) * scale;
                o.bounds_reset = false;
                o.validate()?;
                changed = true;
            }
            if changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        let mask_updates = self.linked_mask_updates(&layers)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.layer_effects.extend(mask_updates);
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(true)
    }

    pub fn rotate_selected_vectors(
        &mut self,
        center: [f32; 2],
        angle: f32,
    ) -> Result<bool, String> {
        if !angle.is_finite() || angle.abs() > 100_000. || center.iter().any(|v| !v.is_finite()) {
            return Err("Invalid object rotation".into());
        }
        if angle.abs() < f32::EPSILON || self.selected_vector_objects.is_empty() {
            return Ok(false);
        }
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        let mut layers = self.svg_layers.clone();
        for layer in &mut layers {
            let mut changed = false;
            for o in &mut layer.vector_objects {
                if !selected.contains(&o.id) {
                    continue;
                }
                if layer.locked || !layer.visible || !o.visible || self.object_is_locked(&o.id) {
                    return Err("Selected object is locked or hidden / 選択対象がロックまたは非表示です / 所选对象已锁定或隐藏".into());
                }
                let (sin, cos) = angle.sin_cos();
                let [a, b, c, d, e, f] = o.transform;
                o.transform = [
                    cos * a - sin * b,
                    sin * a + cos * b,
                    cos * c - sin * d,
                    sin * c + cos * d,
                    center[0] + cos * (e - center[0]) - sin * (f - center[1]),
                    center[1] + sin * (e - center[0]) + cos * (f - center[1]),
                ];
                o.bounds_reset = false;
                o.validate()?;
                changed = true;
            }
            if changed {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
            }
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        let mask_updates = self.linked_mask_updates(&layers)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.layer_effects.extend(mask_updates);
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(true)
    }

    pub fn select_vectors_in_rect(
        &mut self,
        start: Point,
        end: Point,
        additive: bool,
    ) -> Result<(), String> {
        if !start.valid() || !end.valid() {
            return Err("Invalid selection rectangle".into());
        }
        let min = [start.x.min(end.x), start.y.min(end.y)];
        let max = [start.x.max(end.x), start.y.max(end.y)];
        let mut ids = if additive {
            self.selected_vector_objects.clone()
        } else {
            Vec::new()
        };
        if max[0] - min[0] >= 1. && max[1] - min[1] >= 1. {
            for layer in self
                .svg_layers
                .iter()
                .filter(|l| self.can_edit_path_layer(l) && l.vector_layer && l.visible && !l.locked)
            {
                for object in layer.vector_objects.iter().filter(|o| {
                    o.visible && !self.object_is_locked(&o.id) && !o.control_points.is_empty()
                }) {
                    if object.intersects_selection(
                        [min[0], min[1], max[0] - min[0], max[1] - min[1]],
                        false,
                    ) && !ids.contains(&object.id)
                    {
                        ids.push(object.id.clone());
                    }
                }
            }
        }
        self.select_vector_objects(ids)
    }

    /// Read-only picking avoids cloning the document and its history for a click.
    pub fn vector_at(&self, point: Point, tolerance: f32) -> Option<String> {
        if !point.valid() || !tolerance.is_finite() || !(0.0..=256.0).contains(&tolerance) {
            return None;
        }
        let x = f64::from(point.x);
        let y = f64::from(point.y);
        // Cover the rounding allowance used by the f32 narrow-phase test.
        let tolerance64 =
            f64::from(tolerance) + x.abs().max(y.abs()).max(1.) * f64::from(f32::EPSILON) * 4.;
        let bounds = [
            x - tolerance64,
            y - tolerance64,
            x + tolerance64,
            y + tolerance64,
        ];
        let mut index = self.scene_picking.lock();
        index.retain(&self.svg_layers);
        for layer in self.svg_layers.iter().rev().filter(|layer| {
            self.can_edit_path_layer(layer) && layer.vector_layer && layer.visible && !layer.locked
        }) {
            for position in index
                .candidates(layer, &self.scene_journal, self.revision, bounds)
                .into_iter()
                .rev()
            {
                let object = &layer.vector_objects[position];
                if !self.object_is_locked(&object.id)
                    && object.hit_test([point.x, point.y], tolerance)
                {
                    return Some(object.id.clone());
                }
            }
        }
        None
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
        let previous = self.selected_vector_objects.clone();
        let hit = self.vector_at(point, tolerance);
        if let Some(id) = hit.as_ref() {
            if !additive {
                self.selected_vector_objects.clear();
            }
            for grouped in self.expand_group_selection(std::slice::from_ref(id)) {
                if !self.selected_vector_objects.contains(&grouped) {
                    self.selected_vector_objects.push(grouped);
                }
            }
        } else if !additive {
            self.selected_vector_objects.clear();
        }
        if previous != self.selected_vector_objects && !previous.is_empty() {
            self.previous_vector_selection = previous;
        }
        hit
    }
    pub fn duplicate_selected_vectors(&mut self, dx: f32, dy: f32) -> Result<bool, String> {
        if !dx.is_finite() || !dy.is_finite() || dx.abs() >= 100_000. || dy.abs() >= 100_000. {
            return Err("Invalid duplicate offset".into());
        }
        let selected = self.expand_group_selection(&self.selected_vector_objects);
        if selected.is_empty() {
            return Ok(false);
        }
        let mut layers = self.svg_layers.clone();
        let mut ids = Vec::new();
        let mut serial = 0u64;
        let mut compound_mapping = std::collections::HashMap::new();
        let mut groups = std::collections::HashMap::new();
        let mut occupied: std::collections::HashSet<String> = self
            .svg_layers
            .iter()
            .flat_map(|l| l.vector_objects.iter())
            .flat_map(|o| std::iter::once(o.id.clone()).chain(o.group_path.iter().cloned()))
            .collect();
        occupied.extend(self.reserved_compound_ids());
        let mut fresh = || loop {
            serial += 1;
            let id = format!("duplicate-{}-{serial}", self.revision + 1);
            if occupied.insert(id.clone()) {
                break id;
            }
        };
        for layer in &mut layers {
            let originals: Vec<_> = layer
                .vector_objects
                .iter()
                .filter(|o| selected.contains(&o.id))
                .cloned()
                .collect();
            if originals.is_empty() {
                continue;
            }
            if layer.locked || !layer.visible || originals.iter().any(|o| !o.visible) {
                return Err("Select visible objects on unlocked layers / 表示中でロックされていないオブジェクトを選択してください / 请选择可见且未锁定的对象".into());
            }
            if layer.vector_objects.len() + originals.len() > 4096 {
                return Err("Too many vector objects".into());
            }
            let insertion = layer
                .vector_objects
                .iter()
                .rposition(|o| selected.contains(&o.id))
                .unwrap()
                + 1;
            let mut copies = Vec::new();
            for mut object in originals {
                let original_id = object.id.clone();
                object.id = fresh();
                compound_mapping.insert(original_id, object.id.clone());
                for group in &mut object.group_path {
                    *group = groups
                        .entry((layer.id.clone(), group.clone()))
                        .or_insert_with(&mut fresh)
                        .clone();
                }
                if let Some(group) = &mut object.clipping_group {
                    *group = groups
                        .get(&(layer.id.clone(), group.clone()))
                        .ok_or("Missing clipping group")?
                        .clone();
                }
                object.transform[4] += dx;
                object.transform[5] += dy;
                object.validate()?;
                ids.push(object.id.clone());
                copies.push(object);
            }
            layer.vector_objects.splice(insertion..insertion, copies);
            layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
            validate_svg_layer(layer)?;
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much vector data".into());
        }
        let recipes = self.duplicate_compound_recipes(&compound_mapping, &mut fresh);
        let mut candidate_recipes = self.compound_shapes.clone();
        candidate_recipes.extend(recipes);
        compound_shapes::validate_compound_shapes(&candidate_recipes, &layers)?;
        self.finish();
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.compound_shapes = candidate_recipes;
        self.selected_vector_objects = ids;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(true)
    }

    pub fn translation_metrics(&self) -> TranslationMetrics {
        self.translation_metrics
    }
    pub fn move_selected_vectors(&mut self, dx: f32, dy: f32) -> Result<bool, String> {
        self.move_selected_vectors_inner(dx, dy, true)
    }
    /// Translate an isolated rendering fork without creating editing history.
    /// Call only on a transient preview; commit through move_selected_vectors.
    pub fn move_selected_vectors_preview(&mut self, dx: f32, dy: f32) -> Result<bool, String> {
        self.move_selected_vectors_inner(dx, dy, false)
    }
    fn move_selected_vectors_inner(
        &mut self,
        dx: f32,
        dy: f32,
        record_history: bool,
    ) -> Result<bool, String> {
        if self.selected_vectors_have_mask() {
            if !dx.is_finite() || !dy.is_finite() || dx.abs() >= 100_000. || dy.abs() >= 100_000. {
                return Err("Invalid vector translation".into());
            }
            return self.affine_vectors([1., 0., 0., 1., dx, dy], false, record_history);
        }
        let _timer = crate::performance::time("document_vector_move");
        crate::performance::count(
            if record_history {
                "document_vector_move_commits"
            } else {
                "document_vector_move_previews"
            },
            1,
        );
        if !dx.is_finite() || !dy.is_finite() || dx.abs() >= 100_000.0 || dy.abs() >= 100_000.0 {
            return Err("Invalid vector translation".into());
        }
        if dx.abs() < f32::EPSILON && dy.abs() < f32::EPSILON {
            return Ok(false);
        }
        let started = std::time::Instant::now();
        let mut metrics = TranslationMetrics::default();
        // Saved-path editing still uses the complete-state transaction because
        // its stored path is a second authoritative copy of the edited objects.
        let use_complete_history = record_history
            && (self.path_editing.is_some()
                || self.svg_layers.iter().any(|layer| {
                    self.noncanonical_vector_sources.contains(&layer.id)
                        && layer
                            .vector_objects
                            .iter()
                            .any(|o| self.selected_vector_objects.contains(&o.id))
                }));
        let previous = use_complete_history.then(|| self.vector_history_state());
        let mut edits = Vec::new();
        for (layer_index, layer) in self.svg_layers.iter().enumerate() {
            if !layer.vector_layer || layer.locked {
                continue;
            }
            let (positions, scanned) = self.scene_picking.lock().positions(
                layer,
                &self.scene_journal,
                self.revision,
                &self.selected_vector_objects,
            );
            metrics.scanned_objects += scanned;
            for position in positions {
                let object = &layer.vector_objects[position];
                object.validate()?;
                let mut transform = object.transform;
                transform[4] += dx;
                transform[5] += dy;
                if transform.iter().any(|value| !value.is_finite()) {
                    return Err("Invalid vector translation".into());
                }
                edits.push(ObjectHistoryEntry {
                    layer: layer_index,
                    position,
                    transform,
                    bounds: object.conservative_drawing_bounds(),
                });
            }
        }
        if edits.is_empty() {
            return Ok(false);
        }
        // All coordinates validate before touching the document. Prepare every
        // source patch before committing; size failure restores all transforms.
        let touched: std::collections::BTreeSet<_> = edits.iter().map(|e| e.layer).collect();
        for edit in &mut edits {
            std::mem::swap(
                &mut self.svg_layers[edit.layer].vector_objects[edit.position].transform,
                &mut edit.transform,
            );
        }
        let mut updates = Vec::new();
        for &index in &touched {
            let layer = &self.svg_layers[index];
            let positions: Vec<_> = edits
                .iter()
                .filter(|e| e.layer == index)
                .map(|e| e.position)
                .collect();
            let patches = (!self.noncanonical_vector_sources.contains(&layer.id))
                .then(|| {
                    self.transform_slots
                        .patches(layer, &positions, &mut metrics)
                })
                .flatten();
            let (patches, source) = match patches {
                Some(patches) => (Some(patches), None),
                None => (
                    None,
                    Some(vector_svg(self.width, self.height, &layer.vector_objects)),
                ),
            };
            let length = source.as_ref().map_or_else(
                || {
                    let patches = patches.as_ref().unwrap();
                    layer.source.len() - patches.iter().map(|(r, _)| r.len()).sum::<usize>()
                        + patches.iter().map(|(_, s)| s.len()).sum::<usize>()
                },
                String::len,
            );
            if length > MAX_SVG_BYTES {
                for edit in &mut edits {
                    std::mem::swap(
                        &mut self.svg_layers[edit.layer].vector_objects[edit.position].transform,
                        &mut edit.transform,
                    );
                }
                return Err("SVG must be no larger than 4 MiB".into());
            }
            updates.push((index, patches, source));
        }
        for (index, patches, source) in updates {
            let layer = &mut self.svg_layers[index];
            if let Some(source) = source {
                metrics.svg_generations += 1;
                layer.source = source;
            } else {
                let (patch, relocation) =
                    apply_transform_patches(&mut layer.source, patches.unwrap());
                metrics.svg_patch_bytes += patch;
                metrics.estimated_source_relocation_bytes += relocation;
            }
        }
        metrics.edited_objects = edits.len();
        metrics.history_transform_bytes = if record_history {
            edits.len() * std::mem::size_of::<ObjectHistoryEntry>()
        } else {
            0
        };
        let journal_started = std::time::Instant::now();
        if let Some(previous) = previous {
            metrics.full_layer_snapshots = previous.layers.len();
            metrics.history_transform_bytes = 0;
            self.record_vector_edit(previous);
        } else if record_history {
            self.notify_object_history(&edits);
            self.vector_undo.push(VectorHistoryEntry::Objects {
                edits,
                selection: self.selected_vector_objects.clone(),
                selected_layer: self.selected_layer.clone(),
            });
            self.vector_redo.clear();
            self.redo.clear();
            self.redo_order.clear();
            self.undo_order.push(HistoryKind::Vector);
        } else {
            self.notify_object_history(&edits);
            self.sync_saved_path();
        }
        for index in touched {
            self.noncanonical_vector_sources
                .remove(&self.svg_layers[index].id);
        }
        metrics.journal_us = journal_started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        metrics.elapsed_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        self.translation_metrics = metrics;
        self.revision += 1;
        Ok(true)
    }
    pub fn delete_selected_vector_objects(&mut self) -> Result<bool, String> {
        if !self.guides.selected.is_empty() {
            self.edit_guides(GuideEdit {
                action: "delete".into(),
                id: None,
                axis: None,
                position: None,
                delta: None,
            })?;
            return Ok(true);
        }
        self.finish();
        if self.selected_vector_objects.is_empty() {
            return Ok(false);
        }
        let selected = self.selected_vector_objects.clone();
        if self.svg_layers.iter().any(|layer| {
            layer.vector_layer
                && layer.locked
                && layer
                    .vector_objects
                    .iter()
                    .any(|object| selected.contains(&object.id))
        }) {
            return Err("Selected vector object is on a locked layer".into());
        }
        let before = self.vector_history_state();
        let mut changed = false;
        for layer in self
            .svg_layers
            .iter_mut()
            .filter(|layer| layer.vector_layer)
        {
            let previous_len = layer.vector_objects.len();
            layer
                .vector_objects
                .retain(|object| !selected.contains(&object.id));
            if layer.vector_objects.len() != previous_len {
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                validate_svg_layer(layer)?;
                changed = true;
            }
        }
        if changed {
            self.selected_vector_objects.clear();
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
        if self.layer_edit_target() == crate::layer_mask::LayerEditTarget::Content {
            &self.selected_vector_objects
        } else {
            &[]
        }
    }

    /// Consecutive visible objects stay in painter order when a partial drag is
    /// composed from stationary and translated textures. Group opacity requires
    /// compositing the whole layer first, so those layers use the regular path.
    pub fn vector_drag_runs(&self, layer: &SvgLayer) -> Option<Vec<(bool, SvgLayer)>> {
        if layer
            .vector_objects
            .iter()
            .any(|o| o.clipping_group.is_some() || o.blend_mode != "normal")
            || !layer.vector_layer
            || !layer.visible
            || layer.locked
            || layer.effective_opacity() != 1.0
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
            let mut points = vector_geometry_extrema(object, |p| p);
            points.extend(
                object
                    .stroke_boundary_points()
                    .into_iter()
                    .map(|p| Point { x: p[0], y: p[1] }),
            );
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
            .filter(|layer| {
                self.can_edit_path_layer(layer)
                    && layer.vector_layer
                    && layer.visible
                    && !layer.locked
            })
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
    pub fn direct_objects(&self) -> Vec<(String, VectorObject)> {
        self.svg_layers
            .iter()
            .filter(|l| self.can_edit_path_layer(l) && l.visible && !l.locked && !l.paint_layer)
            .flat_map(|l| {
                let objects = if l.vector_layer {
                    l.vector_objects
                        .iter()
                        .filter(|o| o.visible && !self.object_is_locked(&o.id))
                        .filter_map(crate::bezier::editable)
                        .collect::<Vec<_>>()
                } else {
                    self.imported_svg_objects(&l.source, &l.id)
                };
                objects
                    .into_iter()
                    .filter(|o| o.visible && !self.object_is_locked(&o.id))
                    .map(|o| (l.id.clone(), o))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
    pub fn select_direct_objects(&mut self, ids: Vec<String>) -> Result<(), String> {
        let targets = self.direct_objects();
        if ids.len() > 4096
            || ids
                .iter()
                .any(|id| !targets.iter().any(|(_, o)| &o.id == id))
        {
            return Err("Direct path not found".into());
        }
        self.selected_vector_objects = ids
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(())
    }
    fn edit_direct_controls(
        &mut self,
        controls: &[(String, usize)],
        mut edit: impl FnMut(&VectorObject, &[usize]) -> Result<Option<VectorObject>, String>,
    ) -> Result<(), String> {
        if controls.is_empty() {
            return Ok(());
        }
        let mut layers = self.svg_layers.clone();
        let mut found = 0;
        for layer in &mut layers {
            if !self.can_edit_path_layer(layer)
                || layer.locked
                || !layer.visible
                || layer.paint_layer
            {
                continue;
            }
            if layer.vector_layer {
                for object in &mut layer.vector_objects {
                    let indices: Vec<_> = controls
                        .iter()
                        .filter(|(id, _)| id == &object.id)
                        .map(|(_, i)| *i)
                        .collect();
                    if indices.is_empty() {
                        continue;
                    }
                    if !object.visible || self.object_is_locked(&object.id) {
                        return Err("Path is hidden".into());
                    }
                    let source = crate::bezier::editable(object).ok_or("Path cannot be edited")?;
                    if indices
                        .iter()
                        .any(|i| !crate::bezier::control_indices(&source).contains(i))
                    {
                        return Err("Invalid controls".into());
                    }
                    found += indices.len();
                    if let Some(edited) = edit(&source, &indices)? {
                        *object = edited;
                    } else {
                        object.path.data.clear();
                    }
                }
                layer.vector_objects.retain(|o| !o.path.data.is_empty());
                layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
            } else {
                let targets = self.imported_svg_objects(&layer.source, &layer.id);
                let mut changes = Vec::new();
                for target in targets {
                    let indices: Vec<_> = controls
                        .iter()
                        .filter(|(id, _)| id == &target.id)
                        .map(|(_, i)| *i)
                        .collect();
                    if indices.is_empty() {
                        continue;
                    }
                    if indices
                        .iter()
                        .any(|i| !crate::bezier::control_indices(&target).contains(i))
                    {
                        return Err("Invalid SVG controls".into());
                    }
                    found += indices.len();
                    let edited = edit(&target, &indices)?;
                    changes.push(crate::svg_backend::SvgEdit {
                        object_id: target.id,
                        object: edited,
                    });
                }
                if !changes.is_empty() {
                    let backend = self
                        .svg_geometry_backend
                        .as_ref()
                        .ok_or("SVG geometry backend unavailable")?;
                    layer.source = backend.apply(
                        &layer.source,
                        &layer.id,
                        [self.width as f32, self.height as f32],
                        &changes,
                    )?;
                    validate_svg_edit_source(&layer.source)?;
                }
            }
            validate_svg_layer(layer)?;
        }
        if found != controls.len() {
            return Err("Select visible, unlocked path controls / 表示中のロックされていない節点を選択してください / 请选择可见且未锁定的路径节点".into());
        }
        if layers.iter().map(|l| l.source.len()).sum::<usize>() > MAX_SVG_TOTAL_BYTES {
            return Err("Too much vector data".into());
        }
        if layers
            .iter()
            .zip(&self.svg_layers)
            .all(|(a, b)| a.source == b.source && a.vector_objects == b.vector_objects)
        {
            return Ok(());
        }
        let before = self.vector_history_state();
        self.svg_layers = layers;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    pub fn delete_vector_anchors(&mut self, controls: &[(String, usize)]) -> Result<(), String> {
        let anchors: Vec<_> = controls
            .iter()
            .filter(|(_, i)| i.is_multiple_of(3))
            .cloned()
            .collect();
        self.edit_direct_controls(&anchors, crate::bezier::split_deleted)
    }
    pub fn move_vector_controls(
        &mut self,
        controls: &[(String, usize)],
        delta: [f32; 2],
        break_smooth: bool,
    ) -> Result<(), String> {
        if delta.iter().any(|v| !v.is_finite()) {
            return Err("Invalid control movement".into());
        }
        if delta == [0., 0.] {
            return Ok(());
        }
        self.edit_direct_controls(controls, |o, indices| {
            let mut edited = o.clone();
            crate::bezier::translate_controls(&mut edited, indices, delta, break_smooth)?;
            Ok(Some(edited))
        })
    }
    pub fn set_direct_coordinates(
        &mut self,
        controls: &[(String, usize)],
        position: [f32; 2],
    ) -> Result<(), String> {
        if controls.len() != 1
            || position
                .iter()
                .any(|v| !v.is_finite() || v.abs() >= 100_000.)
        {
            return Err("Select one control for absolute coordinates / 絶対座標の編集は1点を選択してください / 编辑绝对坐标请选择一个控制点".into());
        }
        let o = self
            .direct_objects()
            .into_iter()
            .find(|(_, o)| o.id == controls[0].0)
            .ok_or("Path missing")?
            .1;
        let p = *o
            .control_points
            .get(controls[0].1)
            .ok_or("Control missing")?;
        let p = crate::bezier::world_point(&o, p);
        self.move_vector_controls(controls, [position[0] - p[0], position[1] - p[1]], false)
    }
    pub fn set_direct_corner_radius(
        &mut self,
        controls: &[(String, usize)],
        radius: f32,
    ) -> Result<(), String> {
        self.edit_direct_controls(controls, |o, indices| {
            Ok(Some(crate::bezier::round_corners(o, indices, radius)?))
        })
    }
    pub fn set_live_corner_values(
        &mut self,
        id: &str,
        values: &[(usize, f32)],
    ) -> Result<(), String> {
        self.edit_direct_controls(&[(id.to_owned(), 0)], |o, _| {
            Ok(Some(crate::bezier::round_corner_values(o, values)?))
        })
    }
    pub fn direct_preview_svg(&self, ids: &[String]) -> String {
        let objects: Vec<_> = self
            .direct_objects()
            .into_iter()
            .filter(|(_, o)| ids.contains(&o.id))
            .map(|(_, mut o)| {
                o.fill_gradient = None;
                o.stroke_gradient = None;
                o.stroke = Some(crate::vector::VectorPaint {
                    registration: false,
                    color: [70, 150, 255, 255],
                });
                o.stroke_width = 1.;
                o.stroke_style = Default::default();
                o.fill = None;
                o
            })
            .collect();
        let points: Vec<_> = objects
            .iter()
            .flat_map(|o| {
                crate::bezier::control_indices(o)
                    .into_iter()
                    .map(|i| crate::bezier::world_point(o, o.control_points[i]))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut svg = vector_svg(self.width, self.height, &objects);
        if !points.is_empty() {
            let min_x = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
            let min_y = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
            let max_x = points
                .iter()
                .map(|p| p[0])
                .fold(f32::NEG_INFINITY, f32::max);
            let max_y = points
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max);
            let pad = ((max_x - min_x).max(max_y - min_y) * 0.1).max(4.);
            svg = svg.replacen(
                &format!("viewBox=\"0 0 {} {}\"", self.width, self.height),
                &format!(
                    "viewBox=\"{} {} {} {}\"",
                    min_x - pad,
                    min_y - pad,
                    (max_x - min_x).max(1.) + pad * 2.,
                    (max_y - min_y).max(1.) + pad * 2.
                ),
                1,
            );
        }
        svg.replace("<path ", "<path vector-effect=\"non-scaling-stroke\" ")
    }
    pub fn direct_object_preview(
        &mut self,
        layer_id: &str,
        object: VectorObject,
    ) -> Result<(), String> {
        if self
            .svg_layers
            .iter()
            .any(|l| l.id == layer_id && l.vector_layer)
        {
            return self.upsert_vector_object(layer_id, object);
        }
        let layer = self
            .svg_layers
            .iter_mut()
            .find(|l| l.id == layer_id)
            .ok_or("SVG layer missing")?;
        let backend = self
            .svg_geometry_backend
            .as_ref()
            .ok_or("SVG geometry backend unavailable")?;
        let updated = backend.apply(
            &layer.source,
            layer_id,
            [self.width as f32, self.height as f32],
            &[crate::svg_backend::SvgEdit {
                object_id: object.id.clone(),
                object: Some(object),
            }],
        )?;
        let mut candidate = layer.clone();
        candidate.source = updated;
        validate_svg_layer(&candidate)?;
        validate_svg_edit_source(&candidate.source)?;
        *layer = candidate;
        Ok(())
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
        let previous = *control;
        *control = local;
        if object.kind == VectorObjectKind::Bezier && index.is_multiple_of(3) {
            let delta = [local[0] - previous[0], local[1] - previous[1]];
            let last = object.control_points.len() - 1;
            let closed = object.path.data.trim_end().ends_with('Z');
            let mut neighbors = vec![];
            if index > 0 {
                neighbors.push(index - 1);
            }
            if index < last {
                neighbors.push(index + 1);
            }
            if closed && (index == 0 || index == last) {
                object.control_points[if index == 0 { last } else { 0 }] = local;
                neighbors.push(if index == 0 { last - 1 } else { 1 });
            }
            for neighbor in neighbors {
                object.control_points[neighbor][0] += delta[0];
                object.control_points[neighbor][1] += delta[1];
            }
        }
        rebuild_vector_path(object)?;
        object.validate()?;
        let layer = &mut self.svg_layers[layer_index];
        layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
        validate_svg_layer(layer)?;
        self.record_vector_edit(before);
        self.revision += 1;
        Ok(())
    }
    fn notify_object_history(&mut self, edits: &[ObjectHistoryEntry]) {
        for edit in edits {
            let layer = &self.svg_layers[edit.layer];
            let object = &layer.vector_objects[edit.position];
            self.scene_journal.object_transformed(
                &layer.id,
                &object.id,
                edit.bounds,
                object.conservative_drawing_bounds(),
            );
        }
    }
    fn exchange_vector_history(&mut self, entry: VectorHistoryEntry) -> VectorHistoryEntry {
        match entry {
            VectorHistoryEntry::Stored(_) => unreachable!("history was restored before exchange"),
            VectorHistoryEntry::PaintBucket(previous) => VectorHistoryEntry::PaintBucket(
                std::mem::replace(&mut self.paint_bucket_settings, previous),
            ),
            VectorHistoryEntry::Animation(previous) => {
                VectorHistoryEntry::Animation(std::mem::replace(&mut self.animation, previous))
            }
            VectorHistoryEntry::SavedSelections(previous) => VectorHistoryEntry::SavedSelections(
                std::mem::replace(&mut self.saved_vector_selections, previous),
            ),
            VectorHistoryEntry::PixelSelection(previous) => {
                VectorHistoryEntry::PixelSelection(std::mem::replace(&mut self.selection, previous))
            }
            VectorHistoryEntry::State(previous) => {
                let mut current = self.vector_history_state();
                if previous.crop_dimensions.is_some() {
                    current.crop_dimensions = Some((self.width, self.height));
                }
                if previous.strokes.is_some() {
                    current.strokes = Some(self.strokes.clone());
                }
                self.scene_journal
                    .layers_changed(&current.layers, &previous.layers);
                self.restore_vector_history(*previous);
                VectorHistoryEntry::State(Box::new(current))
            }
            VectorHistoryEntry::Objects {
                mut edits,
                mut selection,
                mut selected_layer,
            } => {
                let started = std::time::Instant::now();
                let mut metrics = TranslationMetrics {
                    edited_objects: edits.len(),
                    history_transform_bytes: edits.len()
                        * std::mem::size_of::<ObjectHistoryEntry>(),
                    ..Default::default()
                };
                for edit in &mut edits {
                    edit.bounds = self.svg_layers[edit.layer].vector_objects[edit.position]
                        .conservative_drawing_bounds();
                    std::mem::swap(
                        &mut self.svg_layers[edit.layer].vector_objects[edit.position].transform,
                        &mut edit.transform,
                    );
                }
                let touched: std::collections::BTreeSet<_> =
                    edits.iter().map(|e| e.layer).collect();
                for index in touched {
                    let layer = &mut self.svg_layers[index];
                    let positions: Vec<_> = edits
                        .iter()
                        .filter(|e| e.layer == index)
                        .map(|e| e.position)
                        .collect();
                    if let Some(patches) =
                        self.transform_slots
                            .patches(layer, &positions, &mut metrics)
                    {
                        let (patch, relocation) =
                            apply_transform_patches(&mut layer.source, patches);
                        metrics.svg_patch_bytes += patch;
                        metrics.estimated_source_relocation_bytes += relocation;
                    } else {
                        metrics.svg_generations += 1;
                        layer.source = vector_svg(self.width, self.height, &layer.vector_objects);
                    }
                }
                let journal_started = std::time::Instant::now();
                self.notify_object_history(&edits);
                metrics.journal_us =
                    journal_started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                metrics.elapsed_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
                self.translation_metrics = metrics;
                std::mem::swap(&mut self.selected_vector_objects, &mut selection);
                std::mem::swap(&mut self.selected_layer, &mut selected_layer);
                VectorHistoryEntry::Objects {
                    edits,
                    selection,
                    selected_layer,
                }
            }
        }
    }
    fn vector_history_state(&self) -> VectorHistoryState {
        VectorHistoryState {
            paint_bucket_settings: self.paint_bucket_settings.clone(),
            animation: self.animation.clone(),
            crop_dimensions: None,
            compound_shapes: self.compound_shapes.clone(),
            layer_groups: self.layer_groups.clone(),
            guides: self.guides.clone(),
            background_visible: self.visible,
            locked_objects: self.locked_objects.clone(),
            locked_artwork_layers: self.locked_artwork_layers.clone(),
            background_locked: self.layer_locked,
            path_editing: self.path_editing.clone(),
            saved_paths: self.saved_paths.clone(),
            clipping_path_id: self.clipping_path_id.clone(),
            selected_layer: self.selected_layer.clone(),
            layers: self.svg_layers.clone(),
            selection: self.selected_vector_objects.clone(),
            strokes: None,
            layer_effects: self.layer_effects.clone(),
            paint_source: self.paint_source.clone(),
            pixel_selection: self.selection.clone(),
        }
    }
    fn restore_vector_history(&mut self, state: VectorHistoryState) {
        self.paint_bucket_settings = state.paint_bucket_settings;
        self.animation = state.animation;
        if let Some((width, height)) = state.crop_dimensions {
            self.width = width;
            self.height = height;
        }
        self.transform_slots.0.clear();
        self.compound_shapes = state.compound_shapes;
        self.layer_groups = state.layer_groups;
        self.guides = state.guides;
        self.visible = state.background_visible;
        self.locked_objects = state.locked_objects;
        self.locked_artwork_layers = state.locked_artwork_layers;
        self.layer_locked = state.background_locked;
        let editing = self.path_editing.as_ref().map(|edit| edit.id.clone());
        self.path_editing = state.path_editing;
        self.saved_paths = state.saved_paths;
        self.clipping_path_id = state.clipping_path_id;
        self.layer_effects = state.layer_effects;
        self.paint_source = state.paint_source;
        self.selection = state.pixel_selection;
        if let Some(strokes) = state.strokes {
            self.point_count = strokes.iter().map(|stroke| stroke.points.len()).sum();
            self.strokes = strokes;
        }
        self.selected_layer = state.selected_layer;
        self.svg_layers = state.layers;
        self.noncanonical_vector_sources = self
            .svg_layers
            .iter()
            .filter(|l| {
                l.vector_layer && l.source != vector_svg(self.width, self.height, &l.vector_objects)
            })
            .map(|l| l.id.clone())
            .collect();
        self.selected_vector_objects = state.selection;
        // Undo changes path data, not which panel the user is editing.
        if self.path_editing.as_ref().map(|edit| &edit.id) != editing.as_ref() {
            self.end_path_editing();
            if let Some(id) = editing.filter(|id| self.saved_paths.iter().any(|p| &p.id == id)) {
                let _ = self.activate_saved_path(&id);
            }
        }
    }
    fn record_vector_edit(&mut self, previous: VectorHistoryState) {
        self.transform_slots.0.clear();
        let previous_objects: std::collections::HashMap<_, _> = previous
            .layers
            .iter()
            .flat_map(|layer| &layer.vector_objects)
            .map(|object| (object.id.as_str(), object))
            .collect();
        self.text_change_generation = self
            .text_change_generation
            .max(
                previous
                    .layers
                    .iter()
                    .flat_map(|layer| &layer.vector_objects)
                    .filter_map(|object| object.text.as_ref())
                    .map(|text| text.change_generation)
                    .max()
                    .unwrap_or(0),
            )
            .saturating_add(1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| {
                duration.as_millis().min(u64::MAX as u128) as u64
            });
        for layer in &mut self.svg_layers {
            for object in &mut layer.vector_objects {
                if object.text.is_none() {
                    continue;
                }
                let old = previous_objects.get(object.id.as_str()).copied();
                let metadata = old
                    .and_then(|old| old.text.as_ref())
                    .map(|text| (text.change_generation, text.updated_at_ms));
                // Native previews and IPC callers cannot overwrite engine-owned metadata.
                let text = object.text.as_mut().unwrap();
                (text.change_generation, text.updated_at_ms) = metadata.unwrap_or((0, 0));
                if old != Some(object) {
                    let text = object.text.as_mut().unwrap();
                    text.change_generation = self.text_change_generation;
                    text.updated_at_ms = now.max(text.updated_at_ms);
                }
            }
        }
        self.sync_saved_path();
        self.compound_shapes.retain(|recipe| {
            let unchanged_recipe = previous.compound_shapes.iter().any(|old| old == recipe);
            if !unchanged_recipe {
                return true;
            }
            let old = previous
                .layers
                .iter()
                .flat_map(|l| &l.vector_objects)
                .find(|o| o.id == recipe.id);
            let now = self
                .svg_layers
                .iter()
                .flat_map(|l| &l.vector_objects)
                .find(|o| o.id == recipe.id);
            match (old, now) {
                (Some(old), Some(now)) => {
                    old.path == now.path && old.control_points == now.control_points
                }
                _ => true,
            }
        });
        self.prune_compound_shapes();
        self.paint_bucket_settings
            .reference_layers
            .retain(|id| id == "layer-1" || self.svg_layers.iter().any(|layer| &layer.id == id));
        self.scene_journal
            .layers_changed(&previous.layers, &self.svg_layers);
        self.vector_undo
            .push(VectorHistoryEntry::State(Box::new(previous)));
        self.vector_redo.clear();
        self.redo.clear();
        self.redo_order.clear();
        self.undo_order.push(HistoryKind::Vector);
    }
    pub fn visible_svg_layers(&self) -> impl Iterator<Item = &SvgLayer> {
        self.svg_layers
            .iter()
            .filter(|layer| layer.visible && !self.is_path_edit_layer(layer))
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
            vector_layer: None,
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
        let vector_layer = self
            .svg_layers
            .iter()
            .find(|layer| Some(&layer.id) == self.selected_layer.as_ref() && layer.vector_layer);
        if vector_layer.is_some_and(|layer| layer.locked || !layer.visible) {
            return Err("Select an unlocked visible layer / ロックされていない表示中のレイヤーを選択してください / 请选择未锁定且可见的图层".into());
        }
        let vector_layer = vector_layer.map(|layer| layer.id.clone());
        let moving = vector_layer.is_none()
            && mode == SelectionMode::Replace
            && self.selection.as_ref().is_some_and(|s| s.contains(point));
        if vector_layer.is_none() && mode != SelectionMode::Replace {
            if let Some(selection) = &self.selection {
                selection.ensure_capacity()?;
            }
        }
        self.selection_anchor = Some(SelectionGesture {
            vector_layer,
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
        if let Some(gesture) = self
            .selection_anchor
            .as_ref()
            .filter(|g| g.vector_layer.is_some())
            .cloned()
        {
            let layer_id = gesture.vector_layer.as_ref().unwrap();
            let bounds = [
                gesture.start.x.min(point.x),
                gesture.start.y.min(point.y),
                (gesture.start.x - point.x).abs(),
                (gesture.start.y - point.y).abs(),
            ];
            let region = Selection::new(gesture.shape, bounds);
            self.selection = (bounds[2] >= 1. && bounds[3] >= 1.).then_some(region.clone());
            if finish {
                let mut hits = Vec::new();
                if self.selected_layer.as_ref() == Some(layer_id)
                    && bounds[2] >= 1.
                    && bounds[3] >= 1.
                {
                    if let Some(layer) = self
                        .svg_layers
                        .iter()
                        .find(|l| &l.id == layer_id && l.visible && !l.locked)
                    {
                        for object in &layer.vector_objects {
                            if object.visible
                                && !self.object_is_locked(&object.id)
                                && !object.control_points.is_empty()
                                && object.intersects_selection(
                                    bounds,
                                    gesture.shape == SelectionShape::Ellipse,
                                )
                            {
                                hits.push(object.id.clone());
                            }
                        }
                    }
                }
                if self.selected_layer.as_ref() == Some(layer_id) {
                    let hits = self.expand_group_selection(&hits);
                    self.selected_vector_objects = match gesture.mode {
                        SelectionMode::Replace => hits,
                        SelectionMode::Add => {
                            let mut ids = self.selected_vector_objects.clone();
                            for id in hits {
                                if !ids.contains(&id) {
                                    ids.push(id);
                                }
                            }
                            ids
                        }
                        SelectionMode::Intersect => self
                            .selected_vector_objects
                            .iter()
                            .filter(|id| hits.contains(id))
                            .cloned()
                            .collect(),
                        SelectionMode::Subtract => self
                            .selected_vector_objects
                            .iter()
                            .filter(|id| !hits.contains(id))
                            .cloned()
                            .collect(),
                    };
                }
                self.selection = gesture.base;
                self.selection_anchor = None;
            }
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
                        points: Vec::new(),
                        radius: 0.,
                        shape: gesture.shape,
                        bounds,
                        operation: if gesture.mode == SelectionMode::Add {
                            SelectionOperation::Add
                        } else if gesture.mode == SelectionMode::Intersect {
                            SelectionOperation::Intersect
                        } else {
                            SelectionOperation::Subtract
                        },
                    };
                    self.selection = Some(match gesture.mode {
                        SelectionMode::Replace => Selection::new(gesture.shape, bounds),
                        SelectionMode::Add | SelectionMode::Intersect => match &gesture.base {
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
        self.guides.selected.clear();
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
    fn lightweight_edit_state_matches_ui_after_selection_and_history_changes() {
        let mut doc = Document::default();
        let check = |doc: &Document| {
            let snapshot = doc.snapshot();
            assert_eq!(doc.selected_layer_id(), snapshot.layer_id);
            assert_eq!(doc.is_dirty(), snapshot.dirty);
            let alpha = snapshot
                .layers
                .iter()
                .find(|layer| layer.id == snapshot.layer_id)
                .is_some_and(|layer| layer.alpha_locked);
            assert_eq!(doc.selected_layer_alpha_locked(), alpha);
        };
        check(&doc);
        doc.begin(Point { x: 20., y: 20. }, Brush::default())
            .unwrap();
        check(&doc);
        doc.finish();
        let layer = doc.add_paint_layer().unwrap();
        doc.svg_layers
            .iter_mut()
            .find(|l| l.id == layer)
            .unwrap()
            .alpha_locked = true;
        check(&doc);
        doc.undo();
        check(&doc);
        doc.redo();
        check(&doc);
        doc.selected_layer = Some("missing-layer".into());
        check(&doc);
        assert_eq!(doc.selected_layer_id(), "layer-1");
    }

    #[test]
    fn retained_text_updates_are_atomic_without_history_or_unrelated_layer_copies() {
        for mode in [
            crate::vector::WritingMode::Horizontal,
            crate::vector::WritingMode::Vertical,
        ] {
            let mut doc = Document::default();
            let other = doc.add_vector_layer().unwrap();
            doc.set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: "日本語 ABC 简体中文".into(),
                    writing_mode: mode,
                    box_height: Some(180.),
                    ..Default::default()
                },
                position: [40., 50.],
                color: [12, 34, 56],
            })
            .unwrap();
            doc.affine_selected_vectors([1.2, 0., 0., 1.5, 5., 9.])
                .unwrap();
            let text = doc.snapshot().text_objects.remove(0);
            let mut settings = TextSettings {
                id: Some(text.id),
                text: text.text,
                position: text.position,
                color: text.color,
            };
            let before = doc.document_state();
            let mut preview = doc.clone();
            let history = preview.vector_undo.len();
            let journal = preview.scene_journal.instance_id();
            let other_ptr = preview
                .svg_layers
                .iter()
                .find(|l| l.id == other)
                .unwrap()
                .source
                .as_ptr();
            for content in ["変更 123", "日本語\n简体中文 ABC", "Undo相当"] {
                settings.text.content = content.into();
                settings.text.clear_measured_layout();
                preview.update_text_preview(settings.clone()).unwrap();
                let mut expected = doc.clone();
                expected.set_text_object(settings.clone()).unwrap();
                assert_eq!(
                    preview.svg_layers.last().unwrap().source,
                    expected.svg_layers.last().unwrap().source
                );
                assert_eq!(preview.vector_undo.len(), history);
                assert_eq!(preview.scene_journal.instance_id(), journal);
                assert_eq!(
                    preview
                        .svg_layers
                        .iter()
                        .find(|l| l.id == other)
                        .unwrap()
                        .source
                        .as_ptr(),
                    other_ptr
                );
            }
            let saved = serde_json::to_vec(&preview.document_state()).unwrap();
            let revision = preview.revision();
            let cursor = preview.scene_journal.cursor();
            let mut invalid = settings.clone();
            invalid.position[0] = f32::NAN;
            assert!(preview.update_text_preview(invalid).is_err());
            assert_eq!(
                serde_json::to_vec(&preview.document_state()).unwrap(),
                saved
            );
            assert_eq!(preview.revision(), revision);
            assert_eq!(preview.scene_journal.cursor(), cursor);
            settings.text.content.clear();
            settings.text.box_height = None;
            preview.update_text_preview(settings.clone()).unwrap();
            assert!(!preview.svg_layers.last().unwrap().vector_objects[0].visible);
            settings.text.content = "復帰".into();
            preview.update_text_preview(settings).unwrap();
            assert!(preview.svg_layers.last().unwrap().vector_objects[0].visible);
            assert_eq!(
                serde_json::to_vec(&doc.document_state()).unwrap(),
                serde_json::to_vec(&before).unwrap()
            );
        }
    }

    fn move_fixture(count: usize) -> Document {
        let mut document = Document::default();
        document.add_vector_layer().unwrap();
        let prototype = VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            text: None,
            id: "rectangle-1".into(),
            name: "Rectangle".into(),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: "M 10 10 H 30 V 40 H 10 Z".into(),
                fill_rule: crate::vector::FillRule::NonZero,
            },
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                registration: false,
                color: [0, 0, 0, 255],
            }),
            stroke: None,
            stroke_style: Default::default(),
            stroke_width: 0.0,
            visible: true,
            kind: crate::vector::VectorObjectKind::Rectangle,
            control_points: vec![[10.0, 10.0], [30.0, 40.0]],
        };
        let objects: Vec<_> = (0..count)
            .map(|i| {
                let mut object = prototype.clone();
                object.id = format!("object-{i}");
                object
            })
            .collect();
        document.svg_layers[0].vector_objects = objects;
        document.svg_layers[0].source = vector_svg(
            document.width,
            document.height,
            &document.svg_layers[0].vector_objects,
        );
        document.selected_vector_objects = vec!["object-0".into()];
        document
    }
    #[test]
    fn translation_delta_history_is_constant_and_undo_restores_canonical_source() {
        for count in [1_000, 4_096] {
            let mut d = move_fixture(count);
            let source = d.svg_layers[0].source.clone();
            let untouched = d.svg_layers[0].vector_objects[1].clone();
            let cursor = d.scene_journal.cursor();
            assert!(d.move_selected_vectors(5., 7.).unwrap());
            let Some(VectorHistoryEntry::Objects { edits, .. }) = d.vector_undo.last() else {
                panic!("translation must use delta history");
            };
            assert_eq!(edits.len(), 1);
            assert_eq!(d.translation_metrics().svg_generations, 0);
            assert_eq!(
                d.translation_metrics().history_transform_bytes,
                std::mem::size_of::<ObjectHistoryEntry>()
            );
            assert_eq!(d.svg_layers[0].vector_objects[1], untouched);
            assert_eq!(
                d.svg_layers[0].source,
                vector_svg(d.width, d.height, &d.svg_layers[0].vector_objects)
            );
            let crate::scene::JournalRead::Incremental { changes, .. } =
                d.scene_journal.read(cursor)
            else {
                panic!("unexpected journal gap");
            };
            assert_eq!(changes.len(), 2);
            assert_eq!(changes[1].target.object.as_deref(), Some("object-0"));
            assert!(changes[1].changes.transform && !changes[1].changes.geometry);
            assert_ne!(changes[1].before_bounds, changes[1].after_bounds);
            let moved = d.svg_layers[0].source.clone();
            d.selected_vector_objects.clear();
            d.undo();
            assert_eq!(d.svg_layers[0].source, source);
            assert_eq!(d.translation_metrics().svg_generations, 0);
            assert_eq!(d.selected_vector_objects, ["object-0"]);
            d.redo();
            assert_eq!(d.svg_layers[0].source, moved);
            d.undo();
            assert_eq!(d.svg_layers[0].source, source);
        }
    }
    #[test]
    fn indexed_selection_bounds_preserve_order_and_follow_history() {
        let mut d = move_fixture(4096);
        d.select_vector_objects(vec!["object-4095".into(), "object-0".into()])
            .unwrap();
        let ids: Vec<_> = d
            .selected_drawing_objects(false)
            .iter()
            .map(|o| o.id.as_str())
            .collect();
        assert_eq!(ids, ["object-0", "object-4095"]);
        let bounds = d.selected_vector_box().unwrap();
        d.move_selected_vectors(12., -8.).unwrap();
        let moved = d.selected_vector_box().unwrap();
        for (before, after) in bounds.into_iter().zip(moved) {
            assert!((after[0] - before[0] - 12.).abs() < 0.001);
            assert!((after[1] - before[1] + 8.).abs() < 0.001);
        }
        d.undo();
        assert_eq!(d.selected_vector_box(), Some(bounds));
        d.redo();
        assert_eq!(d.selected_vector_box(), Some(moved));
        d.svg_layers[0].locked = true;
        assert!(d.selected_drawing_objects(false).is_empty());
        assert_eq!(d.selected_vector_box(), None);
        d.svg_layers[0].locked = false;
        d.select_vector_objects(Vec::new()).unwrap();
        assert_eq!(d.selected_vector_box(), None);
    }
    #[test]
    fn trim_marks_are_registration_geometry_with_atomic_history() {
        let mut d = move_fixture(1);
        let before = d.svg_layers[0].source.clone();
        d.create_trim_marks().unwrap();
        let marks = d.svg_layers[0].vector_objects.last().unwrap();
        assert!(marks.stroke.unwrap().registration);
        assert_eq!(marks.path.data.matches('M').count(), 24);
        assert_eq!(marks.stroke_width, 0.25);
        let contours = crate::bezier::cubic_contours(&marks.path.data).unwrap();
        assert_eq!(contours.len(), 24);
        assert!((contours[0].0[0][0] - (10. - 8. * 72. / 25.4)).abs() < 0.001);
        let mm = 72. / 25.4;
        let near = |actual: f32, expected: f32| assert!((actual - expected).abs() < 0.001);
        // Inner/outer corner pairs are separated by exactly 3 mm.
        near(contours[0].0[0][1], 10.);
        near(contours[4].0[0][1], 10. - 3. * mm);
        near(contours[8].0[0][0], 10.);
        near(contours[12].0[0][0], 10. - 3. * mm);
        // Top/bottom and left/right crosses are centered and clear of the bleed.
        near(contours[16].0[0][0], 20. - 2.5 * mm);
        near(contours[16].0.last().unwrap()[0], 20. + 2.5 * mm);
        near(contours[17].0.last().unwrap()[1], 10. - 4. * mm);
        near(contours[19].0[0][1], 40. + 4. * mm);
        near(contours[20].0[0][1], 25. - 2.5 * mm);
        near(contours[21].0.last().unwrap()[0], 10. - 4. * mm);
        near(contours[23].0[0][0], 30. + 4. * mm);
        let after = d.svg_layers[0].source.clone();
        let loaded = Document::decode(&d.encode().unwrap()).unwrap();
        assert!(
            loaded.svg_layers[0]
                .vector_objects
                .last()
                .unwrap()
                .stroke
                .unwrap()
                .registration
        );
        d.undo();
        assert_eq!(d.svg_layers[0].source, before);
        d.redo();
        assert_eq!(d.svg_layers[0].source, after);
        d.undo();
        d.svg_layers[0].locked = true;
        let revision = d.revision;
        assert!(d.create_trim_marks().is_err());
        assert_eq!(d.revision, revision);
        assert_eq!(d.svg_layers[0].source, before);
    }
    #[test]
    fn trim_marks_keep_physical_dimensions_at_print_resolution() {
        let mut screen = move_fixture(1);
        screen.create_trim_marks().unwrap();
        let a = crate::bezier::cubic_contours(
            &screen.svg_layers[0]
                .vector_objects
                .last()
                .unwrap()
                .path
                .data,
        )
        .unwrap();
        let mut print = move_fixture(1);
        print.resolution = 300;
        let scale = 300. / 72.;
        print.svg_layers[0].vector_objects[0].transform = [scale, 0., 0., scale, 0., 0.];
        print.create_trim_marks().unwrap();
        let marks = print.svg_layers[0].vector_objects.last().unwrap();
        assert!((marks.stroke_width / scale - 0.25).abs() < 0.0001);
        let b = crate::bezier::cubic_contours(&marks.path.data).unwrap();
        assert_eq!(a.len(), b.len());
        for ((aa, ac), (bb, bc)) in a.iter().zip(&b) {
            assert_eq!(ac, bc);
            assert_eq!(aa.len(), bb.len());
            for (ap, bp) in aa.iter().zip(bb) {
                for axis in 0..2 {
                    assert!((ap[axis] - bp[axis] / scale).abs() < 0.001);
                }
            }
        }
    }
    #[test]
    fn no_paint_keeps_geometry_selection_and_atomic_history() {
        let mut d = move_fixture(1);
        let id = d.selected_vector_objects[0].clone();
        let bounds = d.selected_vector_bounds();
        let before = d.svg_layers[0].source.clone();
        d.set_selected_vector_paint(std::slice::from_ref(&id), "fill", None)
            .unwrap();
        let object = &d.svg_layers[0].vector_objects[0];
        assert!(object.fill.is_none() && object.stroke.is_none());
        assert_eq!(d.selected_vector_bounds(), bounds);
        assert_eq!(d.selected_vector_objects, vec![id]);
        let saved = d.encode().unwrap();
        let restored = Document::decode(&saved).unwrap();
        assert!(restored.svg_layers[0].vector_objects[0].fill.is_none());
        assert!(restored.svg_layers[0].source.contains("fill=\"none\""));
        d.undo();
        assert_eq!(d.svg_layers[0].source, before);
        d.redo();
        assert!(d.svg_layers[0].vector_objects[0].fill.is_none());
    }
    #[test]
    fn none_brush_does_not_edit_and_eraser_remains_independent() {
        let mut d = Document::default();
        let before = d.encode().unwrap();
        let brush = Brush {
            no_color: true,
            ..Default::default()
        };
        assert!(!d.begin(Point { x: 10., y: 10. }, brush).unwrap());
        assert_eq!(d.encode().unwrap(), before);
        assert!(!d.has_active_stroke());
        assert!(d.begin_eraser(Point { x: 10., y: 10. }, brush, 1.).unwrap());
        let old: Brush =
            serde_json::from_value(serde_json::json!({"size":16,"hardness":1,"color":[1,2,3]}))
                .unwrap();
        assert!(!old.no_color);
    }
    #[test]
    fn text_none_is_local_and_preserves_measured_geometry() {
        let mut text = VectorText {
            content: "あ😀中".into(),
            ..Default::default()
        };
        text.line_baselines = vec![48.];
        text.character_origins = vec![vec![0., 48., 96.]];
        let color = [10, 20, 30];
        text.apply_style(
            1,
            3,
            &crate::vector::TextStylePatch {
                no_color: Some(true),
                ..Default::default()
            },
            color,
        )
        .unwrap();
        assert!(!text.style_at(0, color).no_color);
        assert!(text.style_at(1, color).no_color);
        assert!(!text.style_at(3, color).no_color);
        assert_eq!(text.character_origins, vec![vec![0., 48., 96.]]);
        let mut d = Document::default();
        d.set_text_object(TextSettings {
            id: None,
            text: text.clone(),
            position: [10., 20.],
            color,
        })
        .unwrap();
        assert!(d.svg_layers[0].source.contains("fill=\"none\""));
        let restored = Document::decode(&d.encode().unwrap()).unwrap();
        assert!(
            restored.svg_layers[0].vector_objects[0]
                .text
                .as_ref()
                .unwrap()
                .style_at(1, color)
                .no_color
        );
        text.apply_style(
            1,
            3,
            &crate::vector::TextStylePatch {
                color: Some([200, 0, 0]),
                ..Default::default()
            },
            color,
        )
        .unwrap();
        assert!(!text.style_at(1, color).no_color);
        assert_eq!(text.style_at(1, color).color, [200, 0, 0]);
    }
    #[test]
    fn warm_translation_uses_index_and_constant_history_payload() {
        let mut payload = None;
        for count in [1000, 4096] {
            let mut d = move_fixture(count);
            d.move_selected_vectors(1., 1.).unwrap();
            d.undo();
            d.move_selected_vectors(1., 1.).unwrap();
            let m = d.translation_metrics();
            assert_eq!(m.scanned_objects, 0);
            assert_eq!(m.full_layer_snapshots, 0);
            assert_eq!(m.svg_generations, 0);
            assert_eq!(m.edited_objects, 1);
            assert_eq!(
                m.svg_patch_bytes,
                slot_matrix([1., 0., 0., 1., 1., 1.]).len()
            );
            assert_eq!(m.svg_source_search_bytes, 0);
            assert_eq!(m.estimated_source_relocation_bytes, 0);
            if let Some(bytes) = payload {
                assert_eq!(m.history_transform_bytes, bytes);
            }
            payload = Some(m.history_transform_bytes);
        }
    }
    #[test]
    fn padded_translation_values_round_trip_f32_and_keep_slot_length() {
        let baseline = slot_matrix([1., 0., 0., 1., 0., 0.]).len();
        let mut bits = 1u32;
        let special = [
            0.,
            -0.,
            f32::MAX,
            f32::MIN,
            f32::MIN_POSITIVE,
            f32::from_bits(1),
            -f32::from_bits(1),
        ];
        for value in special.into_iter().chain((0..10_000).filter_map(|_| {
            bits = bits.wrapping_mul(1664525).wrapping_add(1013904223);
            let value = f32::from_bits(bits);
            value.is_finite().then_some(value)
        })) {
            let matrix = slot_matrix([1., 0., 0., 1., value, -value]);
            assert_eq!(matrix.len(), baseline);
            let transform = format!("matrix({matrix})");
            let token = svgtypes::TransformListParser::from(transform.as_str())
                .next()
                .unwrap()
                .unwrap();
            let svgtypes::TransformListToken::Matrix { e, f, .. } = token else {
                panic!("expected matrix");
            };
            assert_eq!((e as f32).to_bits(), value.to_bits());
            assert_eq!((f as f32).to_bits(), (-value).to_bits());
        }
    }
    #[test]
    fn last_object_moves_do_not_search_or_relocate_unrelated_source() {
        for count in [1000, 4096] {
            let mut d = move_fixture(count);
            let id = format!("object-{}", count - 1);
            d.select_vector_objects(vec![id]).unwrap();
            d.move_selected_vectors(9., -9.).unwrap();
            d.undo();
            let pointer = d.svg_layers[0].source.as_ptr();
            let capacity = d.svg_layers[0].source.capacity();
            for (dx, dy) in [
                (0.0001, -0.0001),
                (99999., -99999.),
                (-99999., 99999.),
                (-123.25, 4.5),
            ] {
                d.move_selected_vectors(dx, dy).unwrap();
                let m = d.translation_metrics();
                assert_eq!(m.scanned_objects, 0);
                assert_eq!(m.svg_source_search_bytes, 0);
                assert_eq!(m.estimated_source_relocation_bytes, 0);
                assert_eq!(m.svg_generations, 0);
                assert_eq!(d.svg_layers[0].source.as_ptr(), pointer);
                assert_eq!(d.svg_layers[0].source.capacity(), capacity);
                assert_eq!(
                    d.svg_layers[0].source,
                    vector_svg(d.width, d.height, &d.svg_layers[0].vector_objects)
                );
                d.undo();
                assert_eq!(d.translation_metrics().svg_source_search_bytes, 0);
                assert_eq!(d.translation_metrics().estimated_source_relocation_bytes, 0);
                d.redo();
                assert_eq!(d.translation_metrics().svg_source_search_bytes, 0);
                assert_eq!(d.translation_metrics().estimated_source_relocation_bytes, 0);
                d.undo();
            }
        }
    }
    #[test]
    fn transform_slot_index_is_discarded_on_structure_changes_and_forks() {
        let mut d = move_fixture(3);
        d.move_selected_vectors(1., 1.).unwrap();
        assert!(!d.transform_slots.0.is_empty());
        let mut fork = d.clone();
        assert!(fork.transform_slots.0.is_empty());
        fork.move_selected_vectors(2., 2.).unwrap();
        assert!(fork.translation_metrics().svg_source_search_bytes > 0);
        assert_ne!(fork.svg_layers[0].source, d.svg_layers[0].source);
        let layer_id = d.svg_layers[0].id.clone();
        let mut object = d.svg_layers[0].vector_objects[1].clone();
        object.name = "changed".into();
        d.upsert_vector_object(&layer_id, object).unwrap();
        assert!(d.transform_slots.0.is_empty());
        d.move_selected_vectors(2., 2.).unwrap();
        assert_eq!(
            d.svg_layers[0].source,
            vector_svg(d.width, d.height, &d.svg_layers[0].vector_objects)
        );
        d.undo();
        d.undo();
        assert!(d.transform_slots.0.is_empty());
        d.move_selected_vectors(3., 3.).unwrap();
        assert_eq!(
            d.svg_layers[0].source,
            vector_svg(d.width, d.height, &d.svg_layers[0].vector_objects)
        );
    }
    #[test]
    fn legacy_source_at_capacity_rejects_migration_without_partial_edit() {
        let mut d = move_fixture(4);
        for object in &mut d.svg_layers[0].vector_objects[..3] {
            object
                .path
                .data
                .push_str(&" ".repeat(1024 * 1024 - object.path.data.len()));
        }
        let len = vector_svg(d.width, d.height, &d.svg_layers[0].vector_objects).len();
        d.svg_layers[0].vector_objects[3]
            .path
            .data
            .push_str(&" ".repeat(MAX_SVG_BYTES + 16 - len));
        let canonical = vector_svg(d.width, d.height, &d.svg_layers[0].vector_objects);
        assert_eq!(canonical.len(), MAX_SVG_BYTES + 16);
        let legacy = canonical.replace(&slot_matrix([1., 0., 0., 1., 0., 0.]), "1 0 0 1 0 0");
        assert!(legacy.len() < MAX_SVG_BYTES);
        d.svg_layers[0].source = legacy;
        let state = d.document_state();
        let mut d = Document::from_document_state(state).unwrap();
        d.select_vector_objects(vec!["object-0".into(), "object-3".into()])
            .unwrap();
        let source = d.svg_layers[0].source.clone();
        let revision = d.revision;
        let cursor = d.scene_journal.cursor();
        let history = d.undo_order.len();
        assert!(d.move_selected_vectors(5., 7.).is_err());
        assert_eq!(d.svg_layers[0].source, source);
        assert!(d.svg_layers[0]
            .vector_objects
            .iter()
            .all(|o| o.transform == [1., 0., 0., 1., 0., 0.]));
        assert_eq!(d.revision, revision);
        assert_eq!(d.scene_journal.cursor(), cursor);
        assert_eq!(d.undo_order.len(), history);
    }
    #[test]
    fn noncanonical_loaded_svg_uses_safe_history_and_preserves_undo_source() {
        let d = move_fixture(2);
        let mut state = d.document_state();
        // A marker in imported source is not proof that its structure matches LP.
        state.svg_layers[0].source = state.svg_layers[0]
            .source
            .replace("opacity=\"1\"", "opacity=\"0.5\"");
        let source = state.svg_layers[0].source.clone();
        let mut d = Document::from_document_state(state).unwrap();
        d.select_vector_objects(vec!["object-0".into()]).unwrap();
        d.move_selected_vectors(10., 10.).unwrap();
        assert_eq!(d.translation_metrics().svg_generations, 1);
        assert!(matches!(
            d.vector_undo.last(),
            Some(VectorHistoryEntry::State(_))
        ));
        d.undo();
        assert_eq!(d.svg_layers[0].source, source);
    }
    #[test]
    fn failed_multi_object_translation_does_not_mutate_earlier_objects_or_history() {
        let mut d = move_fixture(2);
        d.selected_vector_objects.push("object-1".into());
        // Simulate a rejected object after a valid candidate; no first-object
        // mutation may leak before all selected candidates pass validation.
        d.svg_layers[0].vector_objects[1].transform[4] = f32::NAN;
        let source = d.svg_layers[0].source.clone();
        let before = d.svg_layers[0].vector_objects[0].transform;
        let revision = d.revision;
        let cursor = d.scene_journal.cursor();
        let history = d.undo_order.len();
        assert!(d.move_selected_vectors(10., 20.).is_err());
        assert_eq!(d.svg_layers[0].vector_objects[0].transform, before);
        assert_eq!(d.svg_layers[0].source, source);
        assert_eq!(d.revision, revision);
        assert_eq!(d.scene_journal.cursor(), cursor);
        assert_eq!(d.undo_order.len(), history);
    }
    #[test]
    fn oversized_source_failure_rolls_back_translation_before_history_commit() {
        let mut d = move_fixture(2);
        let remaining = MAX_SVG_BYTES - d.svg_layers[0].source.len();
        let at = d.svg_layers[0]
            .source
            .find("></svg>")
            .unwrap_or(d.svg_layers[0].source.len() - 6);
        d.svg_layers[0]
            .source
            .insert_str(at, &" ".repeat(remaining + 1));
        let source = d.svg_layers[0].source.clone();
        let transform = d.svg_layers[0].vector_objects[0].transform;
        let cursor = d.scene_journal.cursor();
        let history = d.undo_order.len();
        assert!(d.move_selected_vectors(99999., 99999.).is_err());
        assert_eq!(d.svg_layers[0].source, source);
        assert_eq!(d.svg_layers[0].vector_objects[0].transform, transform);
        assert_eq!(d.scene_journal.cursor(), cursor);
        assert_eq!(d.undo_order.len(), history);
    }

    #[test]
    fn ordinary_layer_effects_snapshot_history_and_saved_state() {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        let source = d.svg_layers().next().unwrap().source.clone();
        assert!(d.snapshot().layers.iter().all(|l| l.effects.is_some()));
        let e = crate::layer_effects::LayerEffects {
            enabled: true,
            grading_balance: 25.,
            ..Default::default()
        };
        d.set_layer_effects(&id, e.clone()).unwrap();
        assert_eq!(d.svg_layers().next().unwrap().source, source);
        assert_eq!(
            d.snapshot()
                .layers
                .iter()
                .find(|l| l.id == id)
                .unwrap()
                .effects,
            Some(e.clone())
        );
        let revision = d.revision();
        d.set_layer_effects(&id, e.clone()).unwrap();
        assert_eq!(d.revision(), revision);
        let saved = d.document_state();
        let loaded = Document::from_document_state(saved.clone()).unwrap();
        assert_eq!(loaded.layer_effects(&id), e);
        d.undo();
        assert!(!d.has_layer_effects(&id));
        d.redo();
        assert_eq!(d.layer_effects(&id), e);
        let before = d.paint_projection_state();
        d.set_layer_effects("layer-1", e.clone()).unwrap();
        assert!(!before.same_effects(d.paint_projection_state()));
        let mut invalid = e;
        invalid.values[0] = f32::NAN;
        let revision = d.revision();
        assert!(d.set_layer_effects("layer-1", invalid).is_err());
        assert_eq!(d.revision(), revision);
        let mut old = serde_json::to_value(saved).unwrap();
        old.as_object_mut().unwrap().remove("layerEffects");
        assert!(
            Document::from_document_state(serde_json::from_value(old).unwrap())
                .unwrap()
                .snapshot()
                .layers
                .iter()
                .all(|l| !l.effects.as_ref().unwrap().enabled)
        );
    }
    #[test]
    fn switching_point_text_direction_preserves_first_glyph_in_world_coordinates() {
        use crate::vector::{VectorText, WritingMode};
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        document.select_layer(layer).unwrap();
        document
            .set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    point_text: true,
                    content: "あいうえお".into(),
                    box_width: 480.,
                    layout_bounds: Some([0., 2., 200., 50.]),
                    scale_x: 1.5,
                    scale_y: 0.8,
                    rotation: 20.,
                    ..Default::default()
                },
                position: [50., 60.],
                color: [0, 0, 0],
            })
            .unwrap();
        let object = document
            .svg_layers
            .iter_mut()
            .flat_map(|layer| &mut layer.vector_objects)
            .find(|object| object.text.is_some())
            .unwrap();
        object.transform = [1.2, 0.2, -0.1, 1.1, 50., 60.];
        let anchor =
            crate::bezier::world_point(object, object.text.as_ref().unwrap().control_points()[0]);
        for mode in [WritingMode::Vertical, WritingMode::Horizontal] {
            document
                .set_text_writing_mode(mode, |text, _| {
                    text.layout_bounds = Some(if mode == WritingMode::Vertical {
                        [text.box_width - 50., 0., 50., 200.]
                    } else {
                        [0., 2., 200., 50.]
                    });
                    Ok(())
                })
                .unwrap();
            let object = document
                .svg_layers
                .iter()
                .flat_map(|layer| &layer.vector_objects)
                .find(|object| object.text.is_some())
                .unwrap();
            let next = crate::bezier::world_point(object, object.control_points[0]);
            assert!((anchor[0] - next[0]).abs() < 0.001);
            assert!((anchor[1] - next[1]).abs() < 0.001);
            assert!(object.text.as_ref().unwrap().box_height.is_none());
        }
        document.undo();
        assert_eq!(
            document.snapshot().text_objects[0].text.writing_mode,
            WritingMode::Vertical
        );
        document.undo();
        assert_eq!(document.snapshot().text_objects[0].text.box_width, 480.);
    }

    #[test]
    fn shared_selection_recognizes_legacy_vector_target_but_honors_pixel_layer() {
        let mut doc = Document::default();
        assert!(!doc.selected_layer_is_vector());
        doc.set_text_object(TextSettings {
            id: None,
            text: crate::vector::VectorText {
                content: "Legacy".into(),
                ..Default::default()
            },
            position: [20., 20.],
            color: [0, 0, 0],
        })
        .unwrap();
        assert!(doc.selected_layer.is_none());
        assert!(doc.selected_layer_is_vector());
        let layer = doc.svg_layers[0].id.clone();
        doc.select_layer("layer-1".into()).unwrap();
        assert!(!doc.selected_layer_is_vector());
        doc.select_layer(layer).unwrap();
        assert!(doc.selected_layer_is_vector());
    }

    #[test]
    fn moved_background_image_round_trips_and_restores_strokes_and_selection() {
        let mut doc = Document::default();
        doc.begin(Point { x: 20., y: 20. }, Brush::default())
            .unwrap();
        doc.finish();
        doc.begin_selection_edit(
            Point { x: 1., y: 1. },
            SelectionShape::Rectangle,
            SelectionMode::Replace,
        )
        .unwrap();
        doc.extend_selection(Point { x: 40., y: 40. }, true);
        let original = doc.selection.clone();
        let source="<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><rect x=\"30\" y=\"30\" width=\"10\" height=\"10\" fill=\"red\"/></svg>".to_owned();
        let moved = original.clone().map(|s| s.translated(10., 10.));
        doc.replace_moved_pixels(source.clone(), moved.clone())
            .unwrap();
        assert!(doc.strokes.is_empty());
        assert_eq!(doc.paint_source(), Some(source.as_str()));
        assert_eq!(
            Document::decode(&doc.encode().unwrap())
                .unwrap()
                .paint_source(),
            Some(source.as_str())
        );
        doc.undo();
        assert_eq!(doc.strokes.len(), 1);
        assert!(doc.paint_source().is_none());
        assert_eq!(doc.selection, original);
        doc.redo();
        assert!(doc.strokes.is_empty());
        assert_eq!(doc.selection, moved);
        doc.clear_selected_layer().unwrap();
        assert!(doc.paint_source().is_none());
        doc.undo();
        assert_eq!(doc.paint_source(), Some(source.as_str()));
    }

    #[test]
    fn selection_area_routes_by_layer_and_preserves_pixel_mask() {
        let mut doc = Document::default();
        doc.begin_selection_edit(
            Point { x: 1., y: 1. },
            SelectionShape::Rectangle,
            SelectionMode::Replace,
        )
        .unwrap();
        doc.extend_selection(Point { x: 10., y: 10. }, true);
        let mask = doc.selection.clone();
        let layer = doc.add_vector_layer().unwrap();
        doc.select_layer(layer.clone()).unwrap();
        doc.set_text_object(TextSettings {
            id: None,
            text: crate::vector::VectorText {
                content: "A".into(),
                box_width: 20.,
                box_height: Some(20.),
                ..Default::default()
            },
            position: [50., 50.],
            color: [0, 0, 0],
        })
        .unwrap();
        let id = doc.selected_vector_objects[0].clone();
        doc.select_vector_objects(Vec::new()).unwrap();
        for shape in [SelectionShape::Rectangle, SelectionShape::Ellipse] {
            doc.begin_selection_edit(Point { x: 0., y: 0. }, shape, SelectionMode::Replace)
                .unwrap();
            doc.extend_selection(Point { x: 150., y: 150. }, true);
            assert_eq!(doc.selected_vector_objects, vec![id.clone()]);
            assert_eq!(doc.selection, mask);
            doc.begin_selection_edit(Point { x: 0., y: 0. }, shape, SelectionMode::Subtract)
                .unwrap();
            doc.extend_selection(Point { x: 150., y: 150. }, true);
            assert!(doc.selected_vector_objects.is_empty());
            doc.begin_selection_edit(Point { x: 0., y: 0. }, shape, SelectionMode::Add)
                .unwrap();
            doc.extend_selection(Point { x: 150., y: 150. }, true);
            assert_eq!(doc.selected_vector_objects, vec![id.clone()]);
        }
        doc.begin_selection_edit(
            Point { x: 200., y: 200. },
            SelectionShape::Rectangle,
            SelectionMode::Replace,
        )
        .unwrap();
        doc.extend_selection(Point { x: 220., y: 220. }, false);
        doc.cancel_selection_gesture();
        assert_eq!(doc.selection, mask);
        assert_eq!(doc.selected_vector_objects, vec![id]);
        doc.select_layer("layer-1".into()).unwrap();
        doc.begin_selection_edit(
            Point { x: 200., y: 200. },
            SelectionShape::Ellipse,
            SelectionMode::Replace,
        )
        .unwrap();
        doc.extend_selection(Point { x: 220., y: 220. }, true);
        assert_ne!(doc.selection, mask);
        assert!(doc.selected_vector_objects.is_empty());
        doc.select_layer(layer).unwrap();
    }

    #[test]
    fn vector_paints_update_selection_preserve_alpha_and_undo() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        let shape = |id: &str, x: f32| VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: id.into(),
            name: id.into(),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
            text: None,
            path: VectorPath {
                data: "M 0 0 H 20 V 20 H 0 Z".into(),
                fill_rule: crate::vector::FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., x, 10.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                registration: false,
                color: [40, 80, 160, 255],
            }),
            stroke: None,
            stroke_style: Default::default(),
            stroke_width: 0.,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[0., 0.], [20., 20.]],
        };
        let mut legacy = serde_json::to_value(shape("legacy", 0.)).unwrap();
        legacy.as_object_mut().unwrap().remove("opacity");
        legacy.as_object_mut().unwrap().remove("blendMode");
        let old: VectorObject = serde_json::from_value(legacy).unwrap();
        assert_eq!(old.opacity, 1.);
        assert_eq!(old.blend_mode, "normal");
        document
            .upsert_vector_object(&layer, shape("a", 0.))
            .unwrap();
        document
            .upsert_vector_object(&layer, shape("b", 30.))
            .unwrap();
        let ids = vec!["a".into(), "b".into()];
        document.select_vector_objects(ids.clone()).unwrap();
        assert!(document
            .set_selected_vector_paint(&["a".into(), "a".into()], "fill", None)
            .is_err());
        document
            .set_selected_vector_paint(&ids, "stroke", Some([200, 30, 10]))
            .unwrap();
        for object in &document.svg_layers[0].vector_objects {
            assert_eq!(object.stroke.unwrap().color, [200, 30, 10, 255]);
            assert_eq!(object.stroke_width, 1.);
        }
        document
            .set_selected_vector_paint(&ids, "swap", None)
            .unwrap();
        assert_eq!(
            document.snapshot().layers[1].objects[0].fill_color,
            Some([200, 30, 10, 255])
        );
        document
            .set_selected_vector_paint(&ids, "fill", None)
            .unwrap();
        assert!(document.svg_layers[0].vector_objects[0].fill.is_none());
        document.undo();
        assert_eq!(
            document.svg_layers[0].vector_objects[0].fill.unwrap().color,
            [200, 30, 10, 255]
        );
        document.svg_layers[0].vector_objects[0]
            .fill
            .as_mut()
            .unwrap()
            .color[3] = 100;
        document
            .set_selected_vector_paint(&ids, "fill", Some([1, 2, 3]))
            .unwrap();
        assert_eq!(
            document.svg_layers[0].vector_objects[0].fill.unwrap().color,
            [1, 2, 3, 100]
        );
        document.select_vector_objects(vec!["a".into()]).unwrap();
        assert!(document
            .set_selected_vector_paint(&ids, "fill", None)
            .is_err());
    }

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
    fn clear_layer_preserves_identity_and_interleaved_history() {
        let mut doc = Document::default();
        doc.begin(Point { x: 20.0, y: 20.0 }, Brush::default())
            .unwrap();
        doc.finish();
        let before = doc.encode().unwrap();
        doc.clear_selected_layer().unwrap();
        assert_eq!(doc.snapshot().stroke_count, 0);
        assert_eq!(doc.snapshot().layer_id, "layer-1");
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.undo();
        assert_eq!(doc.snapshot().stroke_count, 0);
        doc.redo();
        doc.redo();
        assert_eq!(doc.snapshot().stroke_count, 0);
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.layer_locked = true;
        assert!(doc.clear_selected_layer().is_err());
        assert_eq!(doc.snapshot().stroke_count, 1);

        let id = doc.add_paint_layer().unwrap();
        doc.select_layer(id.clone()).unwrap();
        doc.svg_layers[0].source =
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>"
                .into();
        let original = doc.svg_layers[0].clone();
        doc.clear_selected_layer().unwrap();
        assert_eq!(doc.svg_layers.len(), 1);
        assert_eq!(doc.snapshot().layer_id, id);
        assert!(!doc.svg_layers[0].source.contains("<rect"));
        assert_eq!(doc.snapshot().stroke_count, 1);
        doc.undo();
        assert_eq!(doc.svg_layers[0].source, original.source);
        doc.redo();
        assert!(!doc.svg_layers[0].source.contains("<rect"));
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
    fn new_document_preset_validates_and_round_trips() {
        let preset = NewDocumentSettings {
            guide_layout: None,
            pages: None,
            document: DocumentSettings {
                name: "A4 print".into(),
                width: 2480,
                height: 3508,
                unit: DocumentUnit::Millimeters,
                resolution: 300,
                artboards: true,
                canvas_color: CanvasColor::Transparent,
                pixel_aspect_ratio: 1.0,
            },
            color_mode: ColorMode::Cmyk,
            color_profile: ColorProfile::JapanColor2001Coated,
            bit_depth: 16,
        };
        let mut print = preset.clone();
        print.guide_layout = Some(NewDocumentGuideLayout::Print { bleed_mm: 3. });
        let mut print = Document::from_preset(print).unwrap();
        assert_eq!(print.guides.items.len(), 4);
        assert!(print.guides.visible && print.guides.locked);
        let bleed = 3. * 300. / 25.4;
        assert!((print.guides.items[0].position + bleed).abs() < 0.001);
        assert!((print.guides.items[1].position - 2480. - bleed).abs() < 0.001);
        let loaded = Document::decode(&print.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.guides.items[0].position,
            print.guides.items[0].position
        );
        let mut manga = preset.clone();
        manga.guide_layout = Some(NewDocumentGuideLayout::Manga {
            trim_width_mm: 148.,
            trim_height_mm: 210.,
        });
        manga.pages = Some(PageSetup {
            count: 2,
            facing: true,
            binding: PageBinding::RightToLeft,
        });
        let mut manga = Document::from_preset(manga.clone()).unwrap();
        assert!((manga.guides.items[0].position - (2480. - 148. * 300. / 25.4) / 2.).abs() < 0.001);
        let loaded = Document::decode(&manga.encode().unwrap()).unwrap();
        assert_eq!(loaded.guides.items.len(), 4);
        let mut invalid = preset.clone();
        invalid.guide_layout = Some(NewDocumentGuideLayout::Manga {
            trim_width_mm: 1000.,
            trim_height_mm: 1000.,
        });
        assert!(Document::from_preset(invalid).is_err());
        let mut invalid = preset.clone();
        invalid.guide_layout = Some(NewDocumentGuideLayout::Print { bleed_mm: f32::NAN });
        assert!(Document::from_preset(invalid).is_err());
        let mut doc = Document::from_preset(preset.clone()).unwrap();
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap().snapshot();
        assert_eq!(
            (loaded.width, loaded.height, loaded.resolution),
            (2480, 3508, 300)
        );
        assert_eq!(loaded.color_mode, ColorMode::Cmyk);
        assert_eq!(loaded.color_profile, ColorProfile::JapanColor2001Coated);
        assert_eq!(loaded.bit_depth, 16);
        assert_eq!(loaded.canvas_color, CanvasColor::Transparent);
        assert!(loaded.artboards);
        let mut invalid = preset.clone();
        invalid.document.width = 8193;
        assert!(Document::from_preset(invalid).is_err());
        let mut invalid = preset.clone();
        invalid.color_profile = ColorProfile::Srgb;
        assert!(Document::from_preset(invalid).is_err());
        let mut invalid = preset;
        invalid.bit_depth = 12;
        assert!(Document::from_preset(invalid).is_err());
    }

    #[test]
    fn brush_envelope_stages_validation_and_persistence() {
        let envelope = BrushEnvelope {
            enabled: true,
            attack: 10.0,
            decay: 20.0,
            sustain: 0.5,
            hold: 30.0,
            release: 40.0,
            dryness: 0.75,
        };
        for (distance, expected) in [
            (0.0, 0.0),
            (5.0, 0.5),
            (10.0, 1.0),
            (20.0, 0.75),
            (30.0, 0.5),
            (60.0, 0.5),
            (80.0, 0.25),
            (100.0, 0.0),
            (500.0, 0.0),
        ] {
            assert_eq!(envelope.level(distance), expected);
        }
        let zero = BrushEnvelope {
            attack: 0.0,
            decay: 0.0,
            hold: 0.0,
            release: 0.0,
            ..envelope
        };
        assert_eq!(zero.level(0.0), 0.0);
        let mut brush = Brush {
            envelope,
            ..Brush::default()
        };
        let old: Brush = serde_json::from_str(r#"{"size":16,"color":[0,0,0]}"#).unwrap();
        assert!(!old.envelope.enabled);
        let mut doc = Document::default();
        doc.begin(Point { x: 10.0, y: 10.0 }, brush).unwrap();
        doc.finish();
        let mut loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.visible_strokes().next().unwrap().brush.envelope,
            envelope
        );
        loaded.undo();
        loaded.redo();
        assert_eq!(
            loaded.visible_strokes().next().unwrap().brush.envelope,
            envelope
        );
        brush.envelope.release = f32::NAN;
        assert!(brush.validate().is_err());
        brush.envelope = BrushEnvelope {
            sustain: 1.1,
            ..envelope
        };
        assert!(brush.validate().is_err());
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
            no_color: false,
            simulation: Default::default(),
            envelope: Default::default(),
            size: 512.0,
            hardness: 1.0,
            color: [0, 0, 0],
            ..Default::default()
        }
        .validate()
        .is_ok());
        assert!(Brush {
            no_color: false,
            simulation: Default::default(),
            envelope: Default::default(),
            size: 513.0,
            hardness: 1.0,
            color: [0, 0, 0],
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(Brush {
            no_color: false,
            simulation: Default::default(),
            envelope: Default::default(),
            size: 16.0,
            hardness: 1.01,
            color: [0, 0, 0],
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}

/// Portable document data shared by renderers and independent I/O adapters.
/// This is model state, not a file envelope or a renderer display list.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentState {
    #[serde(
        default,
        skip_serializing_if = "crate::paint_bucket::Settings::is_default"
    )]
    pub paint_bucket_settings: crate::paint_bucket::Settings,
    #[serde(default)]
    pub animation: animation::Animation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_vector_selections: Vec<SavedVectorSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pixel_selection: Option<Selection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compound_shapes: Vec<CompoundShape>,
    #[serde(default)]
    pub guides: GuidesState,
    #[serde(default)]
    pub layer_groups: LayerGroupsState,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub locked_objects: std::collections::BTreeSet<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub locked_artwork_layers: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub saved_paths: Vec<SavedPath>,
    #[serde(default)]
    pub clipping_path_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub unit: Option<DocumentUnit>,
    #[serde(default)]
    pub resolution: Option<u32>,
    #[serde(default)]
    pub artboards: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<PageBookState>,
    #[serde(default)]
    pub canvas_color: Option<CanvasColor>,
    #[serde(default)]
    pub pixel_aspect_ratio: Option<f32>,
    pub layer_visible: bool,
    #[serde(default)]
    pub layer_name: Option<String>,
    #[serde(default)]
    pub layer_opacity: Option<f32>,
    #[serde(default)]
    pub layer_locked: Option<bool>,
    #[serde(default)]
    pub layer_alpha_locked: Option<bool>,
    #[serde(default)]
    pub layer_mask_enabled: Option<bool>,
    #[serde(default)]
    pub layer_mask_inverted: Option<bool>,
    #[serde(default)]
    pub layer_mask_density: Option<f32>,
    #[serde(default)]
    pub color_mode: ColorMode,
    #[serde(default)]
    pub color_profile: Option<ColorProfile>,
    #[serde(default = "default_bit_depth")]
    pub bit_depth: u8,
    #[serde(default)]
    pub paint_source: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub layer_effects: std::collections::BTreeMap<String, crate::layer_effects::LayerEffects>,
    pub strokes: Vec<Stroke>,
    #[serde(default)]
    pub svg_layers: Vec<SvgLayer>,
}

fn validate_saved_paths(paths: &[SavedPath], clip: Option<&str>) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    if paths.len() > 64 {
        return Err("Too many saved paths".into());
    }
    let mut bytes = 0;
    for path in paths {
        if path.id.is_empty()
            || path.id.len() > 64
            || !ids.insert(path.id.as_str())
            || path.name.trim().is_empty()
            || path.name.len() > 255
            || path.objects.len() > 4096
        {
            return Err("Invalid saved path".into());
        }
        for object in &path.objects {
            object.validate()?;
            if object.text.is_some() {
                return Err("Clipping paths must be closed".into());
            }
            bytes += object.path.data.len();
        }
    }
    if bytes > MAX_SVG_TOTAL_BYTES || clip.is_some_and(|id| !ids.contains(id)) {
        return Err("Invalid clipping path reference or size".into());
    }
    Ok(())
}

fn unclipped_pixel_source(source: &str, width: u32, height: u32) -> Result<String, String> {
    let tree = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let mut replacements = Vec::new();
    for node in tree
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "svg")
    {
        let dimension = |name| node.attribute(name).and_then(|s| s.parse::<f32>().ok());
        if dimension("width") != Some(width as f32)
            || dimension("height") != Some(height as f32)
            || ["viewBox", "x", "y"]
                .iter()
                .any(|name| node.attribute(*name).is_some())
        {
            continue;
        }
        let range = node.range();
        let tag = &source[range.clone()];
        if !tag.starts_with("<svg") {
            continue;
        }
        replacements.push(range.start + 1..range.start + 4);
        if let Some(end) = tag.rfind("</svg") {
            replacements.push(range.start + end + 2..range.start + end + 5);
        }
    }
    replacements.sort_by_key(|r| r.start);
    let mut result = source.to_string();
    for range in replacements.into_iter().rev() {
        result.replace_range(range, "g");
    }
    Ok(result)
}

// Validate the portable result ourselves rather than relying on a particular SVG backend.
fn validate_svg_edit_source(source: &str) -> Result<(), String> {
    if source.len() > MAX_SVG_BYTES {
        return Err("SVG edit exceeds source limit".into());
    }
    let xml =
        roxmltree::Document::parse(source).map_err(|e| format!("Invalid SVG edit result: {e}"))?;
    if xml.root_element().tag_name().name() != "svg" {
        return Err("SVG edit result must have an SVG root".into());
    }
    Ok(())
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
    if layer.source.len() > MAX_SVG_BYTES
        || (!layer.source.contains("<svg")
            && !roxmltree::Document::parse(&layer.source)
                .is_ok_and(|xml| xml.root_element().tag_name().name() == "svg"))
    {
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

type TransformPatch = (std::ops::Range<usize>, String);
/// Derived indices are not copied with document forks or stored in history.
#[derive(Default)]
struct TransformSlotCache(std::collections::HashMap<String, TransformSlotIndex>);
impl Clone for TransformSlotCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}
struct TransformSlotIndex {
    source_key: (usize, usize),
    slots: std::collections::HashMap<usize, std::ops::Range<usize>>,
}
impl TransformSlotCache {
    fn patches(
        &mut self,
        layer: &SvgLayer,
        positions: &[usize],
        metrics: &mut TranslationMetrics,
    ) -> Option<Vec<TransformPatch>> {
        let source_key = (layer.source.as_ptr() as usize, layer.source.len());
        if self
            .0
            .get(&layer.id)
            .is_none_or(|index| index.source_key != source_key)
        {
            metrics.svg_source_search_bytes += layer.source.len();
            let mut slots = std::collections::HashMap::new();
            const PREFIX: &str = "data-lp-transform=\"";
            for (offset, _) in layer.source.match_indices(PREFIX) {
                let tail = &layer.source[offset + PREFIX.len()..];
                let identifier_end = tail.find('"')?;
                let position: usize = tail[..identifier_end].parse().ok()?;
                const MATRIX: &str = "\" transform=\"matrix(";
                if !tail[identifier_end..].starts_with(MATRIX) {
                    return None;
                }
                let start = offset + PREFIX.len() + identifier_end + MATRIX.len();
                let end = start + layer.source[start..].find(')')?;
                if slots.insert(position, start..end).is_some() {
                    return None;
                }
            }
            self.0
                .insert(layer.id.clone(), TransformSlotIndex { source_key, slots });
        }
        let index = &self.0[&layer.id];
        let mut patches = Vec::with_capacity(positions.len());
        for &position in positions {
            let object = &layer.vector_objects[position];
            if !object.visible && object.clipping_group.is_none() {
                continue;
            }
            let range = index.slots.get(&position)?.clone();
            let replacement = slot_matrix(object.transform);
            // Foreign/older slots use canonical regeneration, never shift a
            // cached offset table after changing its layout.
            if range.len() != replacement.len() {
                return None;
            }
            patches.push((range, replacement));
        }
        patches.sort_by_key(|(range, _)| range.start);
        Some(patches)
    }
}
/// 10 significant decimal digits round-trip every finite f32. Padding only the
/// two translation components keeps ordinary moves the same byte length even
/// across zero, signs, decimal points and exponent changes.
fn slot_matrix([a, b, c, d, e, f]: [f32; 6]) -> String {
    format!("{a} {b} {c} {d} {e:18.9e} {f:18.9e}")
}
fn apply_transform_patches(source: &mut String, patches: Vec<TransformPatch>) -> (usize, usize) {
    let mut patched = 0;
    let mut relocated = 0;
    for (range, replacement) in patches.into_iter().rev() {
        patched += replacement.len();
        if range.len() != replacement.len() {
            relocated += source.len() - range.end;
        }
        if source.len() - range.len() + replacement.len() > source.capacity() {
            relocated += source.len();
        }
        source.replace_range(range, &replacement);
    }
    (patched, relocated)
}

fn empty_vector_svg(width: u32, height: u32) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"/>"#
    )
}

pub fn vector_svg(width: u32, height: u32, objects: &[VectorObject]) -> String {
    use std::fmt::Write;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    for (i, mask) in objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.clipping_group.is_some())
    {
        let matrix = slot_matrix(mask.transform);
        let rule = if mask.path.fill_rule == crate::vector::FillRule::EvenOdd {
            "evenodd"
        } else {
            "nonzero"
        };
        let _ = write!(
            svg,
            r#"<defs><clipPath id="clip-{i}" clipPathUnits="userSpaceOnUse"><path data-lp-transform="{i}" transform="matrix({matrix})" d="{}" clip-rule="{rule}"/></clipPath></defs>"#,
            escape_xml(&mask.path.data)
        );
    }
    for (object_index, object) in objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.visible)
    {
        if object.clipping_group.is_some() {
            continue;
        }
        let _ = write!(
            svg,
            r#"<g opacity="{}" style="mix-blend-mode:{}">"#,
            object.opacity, object.blend_mode
        );
        let mut clips = 1;
        for (i, mask) in objects.iter().enumerate() {
            if mask
                .clipping_group
                .as_ref()
                .is_some_and(|g| object.group_path.contains(g))
            {
                let _ = write!(svg, r#"<g clip-path="url(#clip-{i})">"#);
                clips += 1;
            }
        }
        let matrix = slot_matrix(object.transform);
        let _ = write!(
            svg,
            r#"<g data-lp-transform="{object_index}" transform="matrix({matrix})">"#
        );
        clips += 1;
        let [a, b, c, d, e, f] = [1., 0., 0., 1., 0., 0.];
        let fill = object
            .fill
            .map_or_else(|| "none".into(), |paint| rgba_hex(paint.color));
        let stroke = object
            .stroke
            .map_or_else(|| "none".into(), |paint| rgba_hex(paint.color));
        let mut fill = fill;
        let mut stroke = stroke;
        for (target, gradient) in [
            ("fill", &object.fill_gradient),
            ("stroke", &object.stroke_gradient),
        ] {
            if let Some(gradient) = gradient.as_ref().filter(|_| {
                if target == "fill" {
                    object.fill.is_some()
                } else {
                    object.stroke.is_some()
                }
            }) {
                let id = format!("lp-gradient-{object_index}-{target}");
                svg.push_str("<defs>");
                let mut bounds =
                    crate::stroke::path_bounds(&object.path.data).unwrap_or([0., 0., 1., 1.]);
                let padding = object.stroke_width.max(1.) * 0.5;
                if bounds[0] == bounds[2] {
                    bounds[0] -= padding;
                    bounds[2] += padding;
                }
                if bounds[1] == bounds[3] {
                    bounds[1] -= padding;
                    bounds[3] += padding;
                }
                svg.push_str(&gradient.svg_definition_in_bounds(&id, bounds));
                svg.push_str("</defs>");
                if target == "fill" {
                    fill = format!("url(#{id})");
                } else {
                    stroke = format!("url(#{id})");
                }
            }
        }
        if let Some(frame) = &object.image_frame {
            if let Some(image) = &frame.image {
                if fill != "none" {
                    let data = escape_xml(&object.path.data);
                    let rule = if object.path.fill_rule == crate::vector::FillRule::EvenOdd {
                        "evenodd"
                    } else {
                        "nonzero"
                    };
                    let _ = write!(
                        svg,
                        r#"<path transform="matrix({a} {b} {c} {d} {e} {f})" d="{data}" fill="{fill}" fill-rule="{rule}"/>"#
                    );
                    fill = "none".into();
                }
                let matrix = crate::image_frame::multiply(
                    frame.content_transform.unwrap_or([1., 0., 0., 1., 0., 0.]),
                    image.orientation_transform,
                );
                let [ia, ib, ic, id, ie, iff] = matrix;
                let data = escape_xml(&object.path.data);
                let rule = if object.path.fill_rule == crate::vector::FillRule::EvenOdd {
                    "evenodd"
                } else {
                    "nonzero"
                };
                let _ = write!(
                    svg,
                    r#"<g transform="matrix({a} {b} {c} {d} {e} {f})"><defs><clipPath id="lp-image-frame-{object_index}"><path d="{data}" clip-rule="{rule}"/></clipPath></defs><g clip-path="url(#lp-image-frame-{object_index})"><image width="{}" height="{}" transform="matrix({ia} {ib} {ic} {id} {ie} {iff})" href="{}"/></g></g>"#,
                    image.encoded_width, image.encoded_height, image.data_uri
                );
            }
        }
        if let Some(text) = &object.text {
            let anchor = match text.alignment {
                crate::vector::TextAlignment::Left => "start",
                crate::vector::TextAlignment::Center => "middle",
                crate::vector::TextAlignment::Right => "end",
                crate::vector::TextAlignment::Justify => "start",
            };
            let _ = write!(
                svg,
                r#"<g transform="matrix({a} {b} {c} {d} {e} {f}) rotate({}) scale({} {})">"#,
                text.rotation, text.scale_x, text.scale_y
            );
            if text.writing_mode == crate::vector::WritingMode::Vertical
                && text.has_vertical_character_options()
            {
                let color = object
                    .fill
                    .map_or([0, 0, 0], |p| [p.color[0], p.color[1], p.color[2]]);
                write_vertical_character_options(&mut svg, text, color, object_index);
                svg.push_str("</g>");
                for _ in 0..clips {
                    svg.push_str("</g>");
                }
                continue;
            }
            if text.runs.iter().any(|run| {
                run.style.scale_x != 1.0 || run.style.scale_y != 1.0 || run.style.rotation != 0.0
            }) {
                let color = object.fill.map_or([0, 0, 0], |paint| {
                    [paint.color[0], paint.color[1], paint.color[2]]
                });
                write_transformed_text(&mut svg, text, color, object_index);
                svg.push_str("</g>");
                for _ in 0..clips {
                    svg.push_str("</g>");
                }
                continue;
            }
            if text.writing_mode == crate::vector::WritingMode::Vertical {
                let mut vertical = text.clone();
                if vertical.line_baselines.is_empty() {
                    vertical.reflow_vertical();
                }
                if let Some(height) = text.box_height {
                    let _ = write!(
                        svg,
                        r#"<clipPath id="vertical-{object_index}"><rect width="{}" height="{height}"/></clipPath><g clip-path="url(#vertical-{object_index})">"#,
                        text.box_width
                    );
                }
                let color = object
                    .fill
                    .map_or([0, 0, 0], |p| [p.color[0], p.color[1], p.color[2]]);
                for (column, (start, line, _)) in vertical.visual_lines().into_iter().enumerate() {
                    let x = text.line_baselines.get(column).map_or(
                        text.box_width
                            - text.font_size * 0.5
                            - column as f32 * text.font_size * text.line_height,
                        |baseline| text.box_width - baseline,
                    );
                    if !text_line_fits_frame(text, start, line, x, color) {
                        continue;
                    }
                    let y = text.line_origins.get(column).copied().unwrap_or(
                        text.indent_left + if column == 0 { text.indent_first } else { 0. },
                    );
                    let _ = write!(
                        svg,
                        r#"<text writing-mode="tb" x="{x}" y="{y}" xml:space="preserve">"#
                    );
                    let starts = text.style_segment_starts(start, line, color);
                    for (index, offset) in starts.iter().enumerate() {
                        let end = starts
                            .get(index + 1)
                            .copied()
                            .unwrap_or(start + line.encode_utf16().count());
                        if let Some(content) = utf16_slice(line, offset - start, end - start) {
                            let char_start = utf16_slice(line, 0, offset - start)
                                .map_or(0, |prefix| prefix.chars().count());
                            let positions =
                                text.character_origins.get(column).and_then(|positions| {
                                    positions.get(char_start..char_start + content.chars().count())
                                });
                            let origin = text
                                .style_segment_origins
                                .get(column)
                                .and_then(|origins| origins.get(index))
                                .copied();
                            let mut span = String::new();
                            write_text_segment(
                                &mut span,
                                &text.style_at(*offset, color),
                                content,
                                origin,
                                None,
                                positions,
                            );
                            // Inline progression is Y in vertical writing mode.
                            svg.push_str(&span.replace(" x=", " y="));
                        }
                    }
                    svg.push_str("</text>");
                }
                if text.box_height.is_some() {
                    svg.push_str("</g>");
                }
                svg.push_str("</g>");
                for _ in 0..clips {
                    svg.push_str("</g>");
                }
                continue;
            }
            if let Some(frame_height) = text.box_height {
                let _ = write!(
                    svg,
                    r#"<clipPath id="text-frame-{object_index}" clipPathUnits="userSpaceOnUse"><rect width="{}" height="{frame_height}"/></clipPath>"#,
                    text.box_width
                );
                let _ = write!(
                    svg,
                    r#"<text text-anchor="{anchor}" xml:space="preserve" clip-path="url(#text-frame-{object_index})">"#
                );
            } else {
                let _ = write!(svg, r#"<text text-anchor="{anchor}" xml:space="preserve">"#);
            }
            let color = object.fill.map_or([0, 0, 0], |paint| {
                [paint.color[0], paint.color[1], paint.color[2]]
            });
            // Character-level font changes must also move the first baseline.
            // The box's base font size may still be its original default.
            let mut y = text.style_at(0, color).font_size + text.space_before;
            let mut paragraph_number = 0usize;
            for (index, (start, line, hard_break_before)) in
                text.visual_lines().into_iter().enumerate()
            {
                if index == 0 || hard_break_before {
                    paragraph_number += 1;
                }
                if index > 0 {
                    y += text.font_size * text.line_height;
                    if hard_break_before {
                        y += text.space_before + text.space_after;
                    }
                }
                let baseline = text.line_baselines.get(index).copied().unwrap_or(y);
                if !text_line_fits_frame(text, start, line, baseline, color) {
                    continue;
                }
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
                    crate::vector::TextAlignment::Justify => text.indent_left + first_indent,
                };
                let measured_origin = text.line_origins.get(index).copied();
                let x = measured_origin.unwrap_or(fallback_x);
                if index == 0 || hard_break_before {
                    let marker = match text.list_style {
                        crate::vector::ParagraphListStyle::None => None,
                        crate::vector::ParagraphListStyle::Bullets => Some("•".to_string()),
                        crate::vector::ParagraphListStyle::Numbers => {
                            Some(format!("{paragraph_number}."))
                        }
                    };
                    if let Some(marker) = marker {
                        let marker_x = (x - text.font_size * 1.1).max(0.0);
                        let _ = write!(
                            svg,
                            r#"<tspan x="{marker_x}" y="{baseline}" text-anchor="start">{marker}</tspan>"#
                        );
                    }
                }
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
            for _ in 0..clips {
                svg.push_str("</g>");
            }
            continue;
        }
        let rule = match object.path.fill_rule {
            crate::vector::FillRule::NonZero => "nonzero",
            crate::vector::FillRule::EvenOdd => "evenodd",
        };
        let data = escape_xml(&object.path.data);
        let appearance = &object.stroke_style;
        let dither = object
            .fill_gradient
            .as_ref()
            .filter(|g| g.dither && object.fill.is_some());
        if let Some(gradient) = dither {
            let id = format!("lp-dither-{object_index}");
            svg.push_str("<defs>");
            svg.push_str(&gradient.svg_dither_filter(&id));
            svg.push_str("</defs>");
            let _ = write!(
                svg,
                r#"<g transform="matrix({a} {b} {c} {d} {e} {f})"><path d="{data}" fill="{fill}" fill-rule="{rule}" filter="url(#{id})"/>"#
            );
            svg.push_str(&crate::stroke::svg_stroke(
                &data,
                &object.path.data,
                rule,
                object.stroke_width,
                &object.stroke_style,
                &stroke,
                object_index,
            ));
            svg.push_str("</g>");
        } else if appearance.profile == crate::stroke::WidthProfile::Uniform
            && appearance.alignment == crate::stroke::StrokeAlignment::Center
            && appearance.start_arrow == crate::stroke::Arrowhead::None
            && appearance.end_arrow == crate::stroke::Arrowhead::None
        {
            let _ = write!(
                svg,
                r#"<path d="{data}" transform="matrix({a} {b} {c} {d} {e} {f})" fill="{fill}" stroke="{stroke}" stroke-width="{}" fill-rule="{rule}" {}/>"#,
                object.stroke_width,
                appearance.attributes()
            );
        } else {
            let _ = write!(
                svg,
                r#"<g transform="matrix({a} {b} {c} {d} {e} {f})"><path d="{data}" fill="{fill}" fill-rule="{rule}"/>"#
            );
            svg.push_str(&crate::stroke::svg_stroke(
                &data,
                &object.path.data,
                rule,
                object.stroke_width,
                &object.stroke_style,
                &stroke,
                object_index,
            ));
            svg.push_str("</g>");
        }
        for _ in 0..clips {
            svg.push_str("</g>");
        }
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

/// Suppress an entire overflowing row/column instead of clipping part of its em box.
/// Coordinates are local to the text frame, before the object transform.
fn text_line_fits_frame(
    text: &crate::vector::VectorText,
    start: usize,
    line: &str,
    baseline: f32,
    color: [u8; 3],
) -> bool {
    let Some(height) = text.box_height.filter(|_| !text.point_text) else {
        return true;
    };
    let vertical = text.writing_mode == crate::vector::WritingMode::Vertical;
    let limit = if vertical { text.box_width } else { height };
    let mut offset = start;
    for character in line.chars() {
        let style = text.style_at(offset, color);
        offset += character.len_utf16();
        let angle = style.rotation.to_radians();
        let w = style.font_size * style.scale_x;
        let h = style.font_size * style.scale_y;
        let (low, high) = if vertical {
            let half = (w * angle.cos().abs() + h * angle.sin().abs()) * 0.5;
            (
                baseline + style.baseline_shift - half,
                baseline + style.baseline_shift + half,
            )
        } else {
            let cross = h * angle.cos().abs() + w * angle.sin().abs();
            (
                baseline - style.baseline_shift - cross * 0.8,
                baseline - style.baseline_shift + cross * 0.2,
            )
        };
        if low < -0.001 || high > limit + 0.001 {
            return false;
        }
    }
    true
}

fn write_vertical_character_options(
    svg: &mut String,
    original: &crate::vector::VectorText,
    color: [u8; 3],
    object_index: usize,
) {
    use std::fmt::Write;
    let mut text = original.clone();
    if text.line_baselines.is_empty() {
        text.reflow_vertical();
    }
    text.normalize_vertical_group_positions(color);
    if let Some(height) = text.box_height {
        let _ = write!(
            svg,
            r#"<clipPath id="vc-{object_index}"><rect width="{}" height="{height}"/></clipPath><g clip-path="url(#vc-{object_index})">"#,
            text.box_width
        );
    }
    let lines = text.visual_lines();
    let units = text.vertical_units(color);
    let breaks: std::collections::HashSet<_> = text.soft_breaks.iter().copied().collect();
    let mut column = 0usize;
    let mut cursor = text.indent_left + text.indent_first + text.space_before;
    for (start, content, style) in units {
        if content.contains('\n') {
            column += 1;
            cursor = text.indent_left + text.indent_first + text.space_before;
            continue;
        }
        if breaks.contains(&start) {
            column += 1;
            cursor = text.indent_left;
        }
        let native_column = lines
            .iter()
            .position(|(line_start, line, _)| {
                start >= *line_start && start < *line_start + line.encode_utf16().count()
            })
            .unwrap_or(column);
        let x = text.line_baselines.get(native_column).map_or(
            text.box_width
                - text.font_size * 0.5
                - column as f32 * text.font_size * text.line_height,
            |baseline| text.box_width - baseline,
        );
        if let Some((line_start, line, _)) = lines.get(native_column) {
            if !text_line_fits_frame(&text, *line_start, line, x, color) {
                continue;
            }
        }
        let size = style.font_size;
        let char_index = lines
            .get(native_column)
            .and_then(|(line_start, line, _)| utf16_slice(line, 0, start - line_start))
            .map_or(0, |prefix| prefix.chars().count());
        let y = text
            .character_origins
            .get(native_column)
            .and_then(|positions| positions.get(char_index))
            .copied()
            .unwrap_or(cursor);
        let horizontal = style.tate_chu_yoko || style.upright_latin(content);
        let _ = write!(
            svg,
            r#"<g transform="translate({x} {y}) rotate({}) scale({} {})"><text xml:space="preserve"{}{}>"#,
            style.rotation,
            style.scale_x,
            style.scale_y,
            if horizontal {
                r#" text-anchor="middle" y="0""#
            } else {
                r#" writing-mode="tb" x="0" y="0""#
            },
            if horizontal {
                format!(r#" transform="translate(0 {})""#, size * 0.85)
            } else {
                String::new()
            }
        );
        // A horizontal group is fitted into the em square, centered on its column.
        let mut span = String::new();
        write_text_segment(
            &mut span,
            &style,
            content,
            Some(0.),
            if style.tate_chu_yoko && content.chars().count() > 1 {
                Some(size)
            } else {
                None
            },
            None,
        );
        if style.tate_chu_yoko {
            span = span.replace(
                "lengthAdjust=\"spacing\"",
                "lengthAdjust=\"spacingAndGlyphs\"",
            );
        }
        svg.push_str(&span);
        svg.push_str("</text></g>");
        cursor += size
            * style.scale_y
            * if style.tate_chu_yoko {
                1.
            } else if style.upright_latin(content) {
                (1. + style.tracking / 1000.).max(1.)
            } else {
                1. + style.tracking / 1000.
            };
    }
    if text.box_height.is_some() {
        svg.push_str("</g>");
    }
}

fn write_transformed_text(
    svg: &mut String,
    text: &crate::vector::VectorText,
    color: [u8; 3],
    object_index: usize,
) {
    use std::fmt::Write;
    use unicode_segmentation::UnicodeSegmentation;
    let vertical = text.writing_mode == crate::vector::WritingMode::Vertical;
    if let Some(height) = text.box_height {
        let _ = write!(
            svg,
            r#"<clipPath id="run-frame-{object_index}"><rect width="{}" height="{height}"/></clipPath><g clip-path="url(#run-frame-{object_index})">"#,
            text.box_width
        );
    }
    let mut paragraph_number = 0;
    for (index, (start, line, hard_break_before)) in text.visual_lines().into_iter().enumerate() {
        let cross = text.line_baselines.get(index).copied().unwrap_or(
            (if vertical {
                text.font_size * 0.5
            } else {
                text.font_size + text.space_before
            }) + index as f32 * text.font_size * text.line_height,
        );
        let baseline = if vertical {
            text.box_width - cross
        } else {
            cross
        };
        if !text_line_fits_frame(text, start, line, baseline, color) {
            continue;
        }
        let mut offset = 0;
        let mut character_index = 0;
        let mut cursor = text
            .line_origins
            .get(index)
            .copied()
            .unwrap_or(text.indent_left);
        if !vertical && (index == 0 || hard_break_before) {
            paragraph_number += 1;
            let marker = match text.list_style {
                crate::vector::ParagraphListStyle::None => None,
                crate::vector::ParagraphListStyle::Bullets => Some("•".to_string()),
                crate::vector::ParagraphListStyle::Numbers => Some(format!("{paragraph_number}.")),
            };
            if let Some(marker) = marker {
                let x = (cursor - text.font_size * 1.1).max(0.0);
                let _ = write!(svg, r#"<text x="{x}" y="{cross}" xml:space="preserve">"#);
                write_text_segment(svg, &text.base_style(color), &marker, Some(x), None, None);
                svg.push_str("</text>");
            }
        }
        let fallback: Vec<_> = line
            .graphemes(true)
            .map(|content| {
                let style = text.style_at(start + offset, color);
                let position = text
                    .character_origins
                    .get(index)
                    .and_then(|positions| positions.get(character_index))
                    .copied()
                    .unwrap_or(cursor);
                let cluster = crate::vector::TextGlyphCluster {
                    start: offset,
                    end: offset + content.encode_utf16().count(),
                    x: position,
                };
                offset = cluster.end;
                character_index += content.chars().count();
                cursor += style.font_size
                    * if vertical {
                        style.scale_y
                    } else {
                        style.scale_x
                    };
                cluster
            })
            .collect();
        let clusters = text
            .glyph_clusters
            .get(index)
            .filter(|clusters| !clusters.is_empty())
            .unwrap_or(&fallback);
        for cluster in clusters {
            let Some(content) = utf16_slice(line, cluster.start, cluster.end) else {
                continue;
            };
            let style = text.style_at(start + cluster.start, color);
            let (x, y) = if vertical {
                (text.box_width - cross, cluster.x)
            } else {
                (cluster.x, cross)
            };
            let _ = write!(
                svg,
                r#"<g transform="translate({x} {y}) rotate({}) scale({} {})"><text x="0" y="0" xml:space="preserve"{}>"#,
                style.rotation,
                style.scale_x,
                style.scale_y,
                if vertical {
                    r#" writing-mode="tb""#
                } else {
                    ""
                }
            );
            let mut local = style.clone();
            local.baseline_shift /= if vertical {
                style.scale_x
            } else {
                style.scale_y
            };
            write_text_segment(svg, &local, content, Some(0.0), None, None);
            svg.push_str("</text></g>");
        }
    }
    if text.box_height.is_some() {
        svg.push_str("</g>");
    }
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
    let paint = if style.no_color {
        "none".into()
    } else {
        format!("#{r:02x}{g:02x}{b:02x}")
    };
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
        r##"<tspan{x_attribute}{width_attribute} fill="{paint}" font-family="{}" font-size="{}" font-weight="{}" font-style="{}" font-kerning="{}" letter-spacing="{}" baseline-shift="{}" text-decoration="{decoration}">{}</tspan>"##,
        escape_xml(family),
        style.font_size,
        if style.bold { 700 } else { 400 },
        if style.italic { "italic" } else { "normal" },
        if style.kerning == crate::vector::KerningMode::Metrics {
            "auto"
        } else {
            "none"
        },
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

/// Bounds use curve extrema in the box's coordinate system, never the
/// off-curve handles. Transform controls before solving to support rotated,
/// sheared, and grouped objects without enlarging their oriented boxes.
fn vector_geometry_extrema(object: &VectorObject, map: impl Fn(Point) -> Point) -> Vec<Point> {
    let points: Vec<_> = transformed_control_points(object)
        .into_iter()
        .map(map)
        .collect();
    if object.kind != VectorObjectKind::Bezier
        || points.len() < 4
        || !(points.len() - 1).is_multiple_of(3)
    {
        return points;
    }
    let mut extrema = Vec::new();
    for segment in points.windows(4).step_by(3) {
        extrema.extend([segment[0], segment[3]]);
        for axis in 0..2 {
            let v: Vec<f64> = segment
                .iter()
                .map(|p| if axis == 0 { p.x as f64 } else { p.y as f64 })
                .collect();
            let a = -v[0] + 3. * v[1] - 3. * v[2] + v[3];
            let b = 2. * (v[0] - 2. * v[1] + v[2]);
            let c = v[1] - v[0];
            let mut roots = Vec::new();
            if a.abs() < 1e-12 {
                if b.abs() > 1e-12 {
                    roots.push(-c / b);
                }
            } else {
                let discriminant = b * b - 4. * a * c;
                if discriminant >= 0. {
                    let q = -0.5 * (b + discriminant.sqrt().copysign(b));
                    if q != 0. {
                        roots.extend([q / a, c / q]);
                    } else {
                        roots.push(-b / (2. * a));
                    }
                }
            }
            for t in roots.into_iter().filter(|t| *t > 0. && *t < 1.) {
                let u = 1. - t;
                let evaluate = |values: [f32; 4]| -> f32 {
                    (u * u * u * values[0] as f64
                        + 3. * u * u * t * values[1] as f64
                        + 3. * u * t * t * values[2] as f64
                        + t * t * t * values[3] as f64) as f32
                };
                extrema.push(Point {
                    x: evaluate([segment[0].x, segment[1].x, segment[2].x, segment[3].x]),
                    y: evaluate([segment[0].y, segment[1].y, segment[2].y, segment[3].y]),
                });
            }
        }
    }
    extrema
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
    if object.kind == VectorObjectKind::Rectangle && object.rectangle_radii.is_some() {
        return transform_panel::rebuild_rectangle(object);
    }
    use crate::vector::VectorObjectKind;
    use std::fmt::Write;
    object.path.data = match object.kind {
        VectorObjectKind::Text => return Err("Edit text through text settings".into()),
        VectorObjectKind::Compound => {
            return Err("Compound paths cannot be rebuilt from control points".into())
        }
        VectorObjectKind::Bezier => crate::bezier::path_data(
            &object.control_points,
            object.path.data.trim_end().ends_with('Z'),
        )?,
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
                    no_color: false,
                    simulation: Default::default(),
                    envelope: Default::default(),
                    size: 37.0,
                    hardness: 0.35,
                    color: [12, 34, 56],
                    ..Default::default()
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
                    live_corners: None,
                    rectangle_radii: None,
                    opacity: 1.0,
                    blend_mode: "normal".into(),
                    text: None,
                    id: "shape-1".into(),
                    name: "Blue triangle".into(),
                    group_path: Vec::new(),
                    clipping_group: None,
                    bounds_reset: false,
                    path: VectorPath {
                        data: "M 10 10 L 80 10 L 40 70 Z".into(),
                        fill_rule: FillRule::EvenOdd,
                    },
                    transform: [1.0, 0.0, 0.0, 1.0, 5.0, 6.0],
                    image_frame: None,
                    fill_gradient: None,
                    stroke_gradient: None,
                    fill: Some(VectorPaint {
                        registration: false,
                        color: [10, 80, 220, 255],
                    }),
                    stroke: Some(VectorPaint {
                        registration: false,
                        color: [0, 0, 0, 128],
                    }),
                    stroke_style: Default::default(),
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
                        live_corners: None,
                        rectangle_radii: None,
                        opacity: 1.0,
                        blend_mode: "normal".into(),
                        text: None,
                        id: id.into(),
                        name: id.into(),
                        group_path: Vec::new(),
                        clipping_group: None,
                        bounds_reset: false,
                        path: VectorPath {
                            data: "M 10 10 H 60 V 60 H 10 Z".into(),
                            fill_rule: FillRule::NonZero,
                        },
                        transform: [1.0, 0.0, 0.0, 1.0, index as f32 * 10.0, 0.0],
                        image_frame: None,
                        fill_gradient: None,
                        stroke_gradient: None,
                        fill: Some(VectorPaint {
                            registration: false,
                            color: [50, 80, 120, 255],
                        }),
                        stroke: None,
                        stroke_style: Default::default(),
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
                    live_corners: None,
                    rectangle_radii: None,
                    opacity: 1.0,
                    blend_mode: "normal".into(),
                    text: None,
                    id: "shape-1".into(),
                    name: "Shape".into(),
                    group_path: Vec::new(),
                    clipping_group: None,
                    bounds_reset: false,
                    path: VectorPath {
                        data: "M 0 0 L 20 0 L 20 20 Z".into(),
                        fill_rule: FillRule::NonZero,
                    },
                    transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    image_frame: None,
                    fill_gradient: None,
                    stroke_gradient: None,
                    fill: Some(VectorPaint {
                        registration: false,
                        color: [255, 0, 0, 255],
                    }),
                    stroke: None,
                    stroke_style: Default::default(),
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

        assert!(document.delete_selected_vector_objects().unwrap());
        assert!(document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .vector_objects
            .is_empty());
        assert!(document.snapshot().selected_vector_objects.is_empty());
        document.undo();
        assert_eq!(
            document
                .svg_layers()
                .find(|layer| layer.id == layer_id)
                .unwrap()
                .vector_objects[0]
                .id,
            "shape-1"
        );
        assert_eq!(document.snapshot().selected_vector_objects, ["shape-1"]);
    }

    #[test]
    fn vector_hit_testing_selects_front_object_and_move_is_undoable() {
        let mut document = Document::default();
        let layer_id = document.add_vector_layer().unwrap();
        document
            .upsert_vector_object(
                &layer_id,
                VectorObject {
                    live_corners: None,
                    rectangle_radii: None,
                    opacity: 1.0,
                    blend_mode: "normal".into(),
                    text: None,
                    id: "rectangle-1".into(),
                    name: "Rectangle".into(),
                    group_path: Vec::new(),
                    clipping_group: None,
                    bounds_reset: false,
                    path: VectorPath {
                        data: "M 10 10 H 30 V 40 H 10 Z".into(),
                        fill_rule: FillRule::NonZero,
                    },
                    transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
                    image_frame: None,
                    fill_gradient: None,
                    stroke_gradient: None,
                    fill: Some(VectorPaint {
                        registration: false,
                        color: [0, 0, 0, 255],
                    }),
                    stroke: None,
                    stroke_style: Default::default(),
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
    fn vector_groups_select_move_nest_ungroup_and_round_trip() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        let shape = |id: &str, x: f32| VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: id.into(),
            name: id.into(),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
            text: None,
            path: VectorPath {
                data: "M 0 0 H 20 V 20 H 0 Z".into(),
                fill_rule: FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., x, 10.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                registration: false,
                color: [40, 80, 160, 255],
            }),
            stroke: None,
            stroke_style: Default::default(),
            stroke_width: 0.,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[0., 0.], [20., 20.]],
        };
        for (id, x) in [("a", 10.), ("b", 40.), ("c", 70.)] {
            document.upsert_vector_object(&layer, shape(id, x)).unwrap();
        }
        document
            .select_vector_objects(vec!["a".into(), "b".into()])
            .unwrap();
        document.group_selected_vectors().unwrap();
        let grouped = document.snapshot();
        let a_group = grouped.layers[1].objects[0].group_path.clone();
        assert_eq!(a_group.len(), 1);
        assert_eq!(grouped.layers[1].objects[1].group_path, a_group);
        assert!(grouped.layers[1].objects[2].group_path.is_empty());

        document.select_vector_at(Point { x: 15., y: 15. }, 1., false);
        assert_eq!(document.selected_vector_ids(), &["a", "b"]);
        document.move_selected_vectors(5., 7.).unwrap();
        let objects = &document
            .svg_layers()
            .find(|item| item.id == layer)
            .unwrap()
            .vector_objects;
        assert_eq!(&objects[0].transform[4..], &[15., 17.]);
        assert_eq!(&objects[1].transform[4..], &[45., 17.]);
        assert_eq!(&objects[2].transform[4..], &[70., 10.]);

        document
            .select_vector_objects(vec!["a".into(), "c".into()])
            .unwrap();
        assert_eq!(document.selected_vector_ids(), &["a", "b", "c"]);
        document.group_selected_vectors().unwrap();
        let nested = document.snapshot();
        assert_eq!(nested.layers[1].objects[0].group_path.len(), 2);
        assert_eq!(nested.layers[1].objects[1].group_path.len(), 2);
        assert_eq!(nested.layers[1].objects[2].group_path.len(), 1);
        let encoded = document.encode().unwrap();
        let restored = Document::decode(&encoded).unwrap();
        assert_eq!(
            restored.snapshot().layers[1].objects,
            nested.layers[1].objects
        );

        document.ungroup_selected_vectors(false).unwrap();
        let once = document.snapshot();
        assert_eq!(once.layers[1].objects[0].group_path.len(), 1);
        assert_eq!(once.layers[1].objects[1].group_path.len(), 1);
        assert!(once.layers[1].objects[2].group_path.is_empty());
        document.undo();
        assert_eq!(
            document.snapshot().layers[1].objects,
            nested.layers[1].objects
        );
        document.redo();
        document.ungroup_selected_vectors(true).unwrap();
        assert!(document.snapshot().layers[1]
            .objects
            .iter()
            .all(|item| item.group_path.is_empty()));
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
    fn simulated_brush_round_trip_history_and_legacy_default() {
        let old: Brush =
            serde_json::from_str(r#"{"size":16,"hardness":1,"color":[0,0,0]}"#).unwrap();
        assert_eq!(old.simulation, BrushSimulation::Round);
        for simulation in [
            BrushSimulation::Ink,
            BrushSimulation::Pencil,
            BrushSimulation::DryBrush,
        ] {
            let mut document = Document::default();
            document
                .begin(
                    Point { x: 20., y: 30. },
                    Brush {
                        simulation,
                        ..Brush::default()
                    },
                )
                .unwrap();
            document.finish();
            let loaded = Document::decode(&document.encode().unwrap()).unwrap();
            assert_eq!(
                loaded.visible_strokes().next().unwrap().brush.simulation,
                simulation
            );
            document.undo();
            assert_eq!(document.visible_strokes().count(), 0);
            document.redo();
            assert_eq!(
                document.visible_strokes().next().unwrap().brush.simulation,
                simulation
            );
        }
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
    fn grayscale_and_lab_modes_round_trip_and_validate_profiles() {
        for (mode, profile) in [
            (ColorMode::Grayscale, ColorProfile::GrayD65),
            (ColorMode::Lab, ColorProfile::LabD50),
        ] {
            let mut doc = Document::default();
            doc.set_color_mode(mode);
            assert_eq!(doc.color_mode(), mode);
            assert_eq!(doc.snapshot().color_profile, profile);
            assert!(doc.set_color_profile(ColorProfile::Srgb).is_err());
            let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
            assert_eq!(loaded.color_mode(), mode);
            assert_eq!(loaded.snapshot().color_profile, profile);
            doc.set_color_mode(ColorMode::Rgb);
            assert_eq!(doc.snapshot().color_profile, ColorProfile::Srgb);
        }
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
    #[test]
    fn fixed_text_frame_keeps_empty_content_bounds_and_clipping_across_save_and_undo() {
        let mut doc = Document::default();
        let mut frame = settings();
        frame.text.content.clear();
        frame.text.box_width = 160.0;
        frame.text.box_height = Some(80.0);
        doc.set_text_object(frame).unwrap();
        let created = doc.snapshot().text_objects[0].clone();
        assert_eq!(
            created.text.control_points(),
            vec![[0.0, 0.0], [160.0, 0.0], [160.0, 80.0], [0.0, 80.0]]
        );
        assert_eq!(
            doc.text_at([created.position[0] + 20.0, created.position[1] + 20.0]),
            Some(created.id.clone())
        );
        let mut edited = TextSettings {
            id: Some(created.id.clone()),
            text: created.text.clone(),
            position: created.position,
            color: created.color,
        };
        edited.text.content = "First line\nSecond line\nThird line".into();
        doc.set_text_object(edited).unwrap();
        let svg = &doc.svg_layers().next().unwrap().source;
        assert!(svg.contains("clipPathUnits=\"userSpaceOnUse\""));
        assert!(svg.contains("clip-path=\"url(#text-frame-0)\""));
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.snapshot().text_objects[0].text.box_height,
            Some(80.0)
        );
        doc.undo();
        assert!(doc.snapshot().text_objects[0].text.content.is_empty());
        doc.redo();
        assert_eq!(
            doc.snapshot().text_objects[0].text.content,
            "First line\nSecond line\nThird line"
        );
    }
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
    fn raster_import_creates_named_pixel_layer_and_round_trips_history() {
        let mut doc = Document::default();
        let vector = doc.add_vector_layer().unwrap();
        doc.select_layer(vector).unwrap();
        let count = doc.svg_layers.len();
        let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"20\"><rect width=\"10\" height=\"20\" fill=\"red\"/></svg>";
        doc.import_raster_layer("Photo".into(), source.into())
            .unwrap();
        let added = doc.svg_layers.last().unwrap();
        assert_eq!(added.name, "Photo");
        assert!(added.paint_layer && !added.vector_layer);
        assert_eq!(doc.selected_layer.as_ref(), Some(&added.id));
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.svg_layers.last().unwrap().name, "Photo");
        doc.undo();
        assert_eq!(doc.svg_layers.len(), count);
        doc.redo();
        assert_eq!(doc.svg_layers.last().unwrap().name, "Photo");
        let before = doc.encode().unwrap();
        assert!(doc
            .import_raster_layer("Bad".into(), "not an image".into())
            .is_err());
        assert_eq!(doc.encode().unwrap(), before);
    }

    #[test]
    fn arrange_objects_preserves_groups_order_and_atomic_history() {
        let mut doc = Document::default();
        let source = doc.add_vector_layer().unwrap();
        doc.select_layer(source.clone()).unwrap();
        let mut ids = Vec::new();
        for name in ["a", "b", "c", "d", "e"] {
            let mut text = settings();
            text.text.content = name.into();
            doc.set_text_object(text).unwrap();
            ids.push(doc.selected_vector_objects[0].clone());
        }
        let order = |doc: &Document, layer: &str| -> Vec<String> {
            doc.svg_layers
                .iter()
                .find(|l| l.id == layer)
                .unwrap()
                .vector_objects
                .iter()
                .map(|o| o.id.clone())
                .collect()
        };
        // The unselected b/c group is one stacking unit.
        doc.select_vector_objects(vec![ids[1].clone(), ids[2].clone()])
            .unwrap();
        doc.group_selected_vectors().unwrap();
        doc.select_vector_objects(vec![ids[0].clone(), ids[3].clone()])
            .unwrap();
        doc.arrange_selected_vectors("forward").unwrap();
        assert_eq!(
            order(&doc, &source),
            [
                ids[1].clone(),
                ids[2].clone(),
                ids[0].clone(),
                ids[4].clone(),
                ids[3].clone()
            ]
        );
        doc.undo();
        assert_eq!(order(&doc, &source), ids);
        doc.redo();
        doc.arrange_selected_vectors("backward").unwrap();
        assert_eq!(order(&doc, &source), ids);
        doc.arrange_selected_vectors("front").unwrap();
        assert_eq!(
            order(&doc, &source),
            [
                ids[1].clone(),
                ids[2].clone(),
                ids[4].clone(),
                ids[0].clone(),
                ids[3].clone()
            ]
        );
        let revision = doc.revision;
        doc.arrange_selected_vectors("front").unwrap();
        assert_eq!(doc.revision, revision);
        doc.arrange_selected_vectors("back").unwrap();
        assert_eq!(
            order(&doc, &source),
            [
                ids[0].clone(),
                ids[3].clone(),
                ids[1].clone(),
                ids[2].clone(),
                ids[4].clone()
            ]
        );
        doc.select_vector_objects(vec![ids[1].clone()]).unwrap();
        let target = doc.add_vector_layer().unwrap();
        // Adding a layer itself does not replace the object selection.
        doc.select_vector_objects(vec![ids[1].clone()]).unwrap();
        doc.select_layer_preserving_objects(target.clone()).unwrap();
        assert_eq!(doc.selected_vector_objects.len(), 2);
        let original = order(&doc, &source);
        doc.arrange_selected_vectors("moveToLayer").unwrap();
        assert_eq!(order(&doc, &target), [ids[1].clone(), ids[2].clone()]);
        assert_eq!(order(&doc, &source).len(), 3);
        assert!(doc
            .svg_layers
            .iter()
            .find(|l| l.id == target)
            .unwrap()
            .vector_objects
            .iter()
            .all(|o| !o.group_path.is_empty()));
        doc.undo();
        assert_eq!(order(&doc, &source), original);
        assert!(order(&doc, &target).is_empty());
        doc.redo();
        assert_eq!(order(&doc, &target).len(), 2);
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(order(&loaded, &target), order(&doc, &target));
        assert_eq!(order(&loaded, &source), order(&doc, &source));
        // Convert the moved group into a clipping group and move its mask selection.
        doc.ungroup_selected_vectors(true).unwrap();
        let mut mask = doc
            .svg_layers
            .iter()
            .find(|l| l.id == target)
            .unwrap()
            .vector_objects[1]
            .clone();
        mask.kind = crate::vector::VectorObjectKind::Rectangle;
        mask.text = None;
        mask.control_points = vec![[0., 0.], [100., 100.]];
        mask.path.data = "M 0 0 H 100 V 100 H 0 Z".into();
        doc.upsert_vector_object(&target, mask).unwrap();
        doc.select_vector_objects(vec![ids[1].clone(), ids[2].clone()])
            .unwrap();
        doc.clipping_path("create").unwrap();
        doc.clipping_path("edit").unwrap();
        assert_eq!(doc.selected_vector_objects.len(), 1);
        doc.select_layer_preserving_objects(source.clone()).unwrap();
        doc.arrange_selected_vectors("moveToLayer").unwrap();
        assert!(order(&doc, &target).is_empty());
        assert_eq!(doc.selected_vector_objects.len(), 2);
        let source_layer = doc.svg_layers.iter().find(|l| l.id == source).unwrap();
        assert!(source_layer.source.contains("clip-path"));
        assert_eq!(
            source_layer
                .vector_objects
                .iter()
                .filter(|o| o.clipping_group.is_some())
                .count(),
            1
        );
        doc.undo();
        assert_eq!(order(&doc, &target).len(), 2);
        doc.select_layer_preserving_objects(target.clone()).unwrap();
        // Reject a locked destination without removing anything from its source.
        doc.select_vector_objects(vec![ids[0].clone()]).unwrap();
        doc.svg_layers
            .iter_mut()
            .find(|l| l.id == target)
            .unwrap()
            .locked = true;
        let before = order(&doc, &source);
        assert!(doc.arrange_selected_vectors("moveToLayer").is_err());
        assert_eq!(order(&doc, &source), before);
        assert!(doc.arrange_selected_vectors("invalid").is_err());
        doc.select_layer_preserving_objects("layer-1".into())
            .unwrap();
        assert!(doc.selected_vector_objects.is_empty());
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
    fn first_line_uses_character_font_size_when_base_size_differs() {
        let mut edit = settings();
        edit.text.content = "Hello".into();
        edit.text.font_size = 48.0;
        edit.text.line_height = 15.0 / 48.0;
        let mut style = edit.text.base_style(edit.color);
        style.font_size = 15.0;
        edit.text.runs = vec![crate::vector::TextRun {
            start: 0,
            end: 5,
            style,
        }];
        let mut doc = Document::default();
        doc.set_text_object(edit).unwrap();
        assert!(doc.svg_layers[0].source.contains("y=\"15\""));
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
        next.text = doc.snapshot().text_objects[0].text.clone();
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
    fn paragraph_details_render_and_round_trip_without_changing_source_text() {
        use crate::vector::{KinsokuMode, MojikumiMode, ParagraphListStyle, TextAlignment};
        for list_style in [ParagraphListStyle::Bullets, ParagraphListStyle::Numbers] {
            let mut edit = settings();
            edit.text.content = "First paragraph\n第二段落".into();
            edit.text.alignment = TextAlignment::Justify;
            edit.text.list_style = list_style;
            edit.text.kinsoku = KinsokuMode::Strict;
            edit.text.mojikumi = MojikumiMode::Japanese;
            edit.text.hyphenation = true;
            edit.text.indent_left = 24.0;
            edit.text.indent_right = 12.0;
            edit.text.indent_first = 8.0;
            edit.text.space_before = 6.0;
            edit.text.space_after = 9.0;
            let mut doc = Document::default();
            doc.set_text_object(edit.clone()).unwrap();
            let svg = &doc.svg_layers[0].source;
            match list_style {
                ParagraphListStyle::Bullets => assert_eq!(svg.matches(">•</tspan>").count(), 2),
                ParagraphListStyle::Numbers => {
                    assert!(svg.contains(">1.</tspan>"));
                    assert!(svg.contains(">2.</tspan>"));
                }
                ParagraphListStyle::None => unreachable!(),
            }
            assert_eq!(
                doc.snapshot().text_objects[0].text.content,
                edit.text.content
            );
            let restored = Document::decode(&doc.encode().unwrap()).unwrap();
            edit.text = doc.snapshot().text_objects[0].text.clone();
            assert_eq!(restored.snapshot().text_objects[0].text, edit.text);
        }
        let legacy: VectorText = serde_json::from_str(r#"{"content":"Legacy"}"#).unwrap();
        assert_eq!(legacy.list_style, ParagraphListStyle::None);
        assert_eq!(legacy.kinsoku, KinsokuMode::None);
        assert_eq!(legacy.mojikumi, MojikumiMode::None);
        assert!(!legacy.hyphenation);
    }

    #[test]
    fn vertical_options_preserve_native_positions_and_unselected_content() {
        let mut edit = settings();
        edit.text.content = "あAい12う".into();
        edit.text.font_size = 48.;
        edit.text.writing_mode = crate::vector::WritingMode::Vertical;
        edit.text
            .apply_style(
                1,
                2,
                &crate::vector::TextStylePatch {
                    rotate_latin: Some(false),
                    ..Default::default()
                },
                edit.color,
            )
            .unwrap();
        edit.text
            .apply_style(
                3,
                5,
                &crate::vector::TextStylePatch {
                    tate_chu_yoko: Some(true),
                    ..Default::default()
                },
                edit.color,
            )
            .unwrap();
        edit.text.line_baselines = vec![13.];
        edit.text.line_origins = vec![7.];
        edit.text.character_origins = vec![vec![7., 60., 110., 160., 180., 208.]];
        let mut doc = Document::default();
        doc.set_text_object(edit.clone()).unwrap();
        let svg = &doc.svg_layers[0].source;
        let x = edit.text.box_width - 13.;
        for y in [7., 60., 110., 160., 208.] {
            assert!(
                svg.contains(&format!("translate({x} {y})")),
                "missing native pen position {y}"
            );
        }
        assert_eq!(doc.snapshot().text_objects[0].text.content, "あAい12う");
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.snapshot().text_objects[0].text.character_origins,
            edit.text.character_origins
        );
    }
    #[test]
    fn vertical_character_options_render_and_survive_history_and_save() {
        let mut edit = settings();
        edit.text.content = "あ12ABC".into();
        edit.text.writing_mode = crate::vector::WritingMode::Vertical;
        edit.text
            .apply_style(
                1,
                3,
                &crate::vector::TextStylePatch {
                    tate_chu_yoko: Some(true),
                    ..Default::default()
                },
                edit.color,
            )
            .unwrap();
        edit.text
            .apply_style(
                3,
                6,
                &crate::vector::TextStylePatch {
                    rotate_latin: Some(false),
                    ..Default::default()
                },
                edit.color,
            )
            .unwrap();
        let mut doc = Document::default();
        doc.set_text_object(edit).unwrap();
        let svg = doc.svg_layers[0].source.clone();
        assert!(svg.contains("lengthAdjust=\"spacingAndGlyphs\""));
        assert!(svg.contains(">12</tspan>"));
        assert!(svg.contains("text-anchor=\"middle\""));
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.svg_layers[0].source, svg);
        let text = &loaded.snapshot().text_objects[0].text;
        assert!(text.style_at(1, [0, 0, 0]).tate_chu_yoko);
        assert!(!text.style_at(3, [0, 0, 0]).rotate_latin);
        doc.undo();
        assert!(doc.snapshot().text_objects.is_empty());
        doc.redo();
        assert_eq!(doc.svg_layers[0].source, svg);
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
    fn text_edit_metadata_is_owned_by_document_and_restored_with_history() {
        let mut doc = Document::default();
        doc.set_text_object(settings()).unwrap();
        let original = doc.snapshot().text_objects[0].clone();
        assert!(original.text.change_generation > 0);
        assert!(original.text.updated_at_ms > 0);
        let source = doc.svg_layers[0].source.clone();
        let mut second = settings();
        second.position[0] += 200.0;
        doc.set_text_object(second).unwrap();
        assert_eq!(doc.snapshot().text_objects[0].text, original.text);
        let untouched = doc.snapshot().text_objects[1].text.clone();
        let mut edit = settings();
        edit.id = Some(original.id.clone());
        edit.text.content = "日本語 English 简体中文 👩‍🎨".into();
        edit.text.change_generation = u64::MAX;
        edit.text.updated_at_ms = u64::MAX;
        doc.set_text_object(edit.clone()).unwrap();
        let changed = doc.snapshot().text_objects[0].text.clone();
        assert!(changed.change_generation > original.text.change_generation);
        assert!(changed.change_generation < u64::MAX);
        assert!(changed.updated_at_ms < u64::MAX);
        assert_eq!(doc.snapshot().text_objects[1].text, untouched);
        doc.undo();
        assert_eq!(doc.snapshot().text_objects[0].text, original.text);
        doc.redo();
        assert_eq!(doc.snapshot().text_objects[0].text, changed);
        let mut loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.snapshot().text_objects[0].text, changed);
        edit.text.content = "New branch".into();
        loaded.set_text_object(edit.clone()).unwrap();
        let generation = loaded.snapshot().text_objects[0].text.change_generation;
        loaded.undo();
        edit.text.content = "Another branch".into();
        loaded.set_text_object(edit).unwrap();
        assert!(loaded.snapshot().text_objects[0].text.change_generation > generation);
        // Metadata alone never enters SVG or affects the raster-cache content key.
        doc.undo();
        doc.svg_layers[0].vector_objects[0]
            .text
            .as_mut()
            .unwrap()
            .updated_at_ms += 1;
        assert_eq!(
            vector_svg(
                doc.width,
                doc.height,
                &doc.svg_layers[0].vector_objects[..1]
            ),
            source
        );
        let mut legacy = serde_json::to_value(&original.text).unwrap();
        legacy.as_object_mut().unwrap().remove("changeGeneration");
        legacy.as_object_mut().unwrap().remove("updatedAtMs");
        let legacy: VectorText = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.change_generation, 0);
        assert_eq!(legacy.updated_at_ms, 0);
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
    fn selected_character_color_preserves_other_styles_in_text_and_frames() {
        use crate::vector::TextStylePatch;
        for box_height in [None, Some(180.0)] {
            let mut doc = Document::default();
            let mut edit = settings();
            edit.text.content = "A日😀中B".into();
            edit.text.box_height = box_height;
            edit.text
                .apply_style(
                    1,
                    5,
                    &TextStylePatch {
                        bold: Some(true),
                        font_size: Some(24.0),
                        ..Default::default()
                    },
                    edit.color,
                )
                .unwrap();
            doc.set_text_object(edit.clone()).unwrap();
            edit.id = Some(doc.snapshot().text_objects[0].id.clone());
            let original = doc.snapshot().text_objects[0].text.clone();
            // Native selection offsets use UTF-16: the emoji occupies two units.
            for color in [[255, 0, 0], [0, 80, 255]] {
                edit.text
                    .apply_style(
                        2,
                        5,
                        &TextStylePatch {
                            color: Some(color),
                            ..Default::default()
                        },
                        edit.color,
                    )
                    .unwrap();
                for offset in [0, 1, 5] {
                    assert_eq!(
                        edit.text.style_at(offset, edit.color),
                        original.style_at(offset, edit.color)
                    );
                }
                for offset in [2, 4] {
                    let mut expected = original.style_at(offset, edit.color);
                    expected.color = color;
                    assert_eq!(edit.text.style_at(offset, edit.color), expected);
                }
            }
            doc.set_text_object(edit.clone()).unwrap();
            let saved = Document::decode(&doc.encode().unwrap()).unwrap();
            edit.text = doc.snapshot().text_objects[0].text.clone();
            assert_eq!(saved.snapshot().text_objects[0].text, edit.text);
            let svg = vector_svg(960, 640, &doc.svg_layers[0].vector_objects);
            assert!(svg.contains("fill=\"#0050ff\""));
            assert!(svg.contains("😀中</tspan>"));
            doc.undo();
            assert_eq!(doc.snapshot().text_objects[0].text, original);
            doc.redo();
            assert_eq!(doc.snapshot().text_objects[0].text, edit.text);
            // An insertion color must not recolor existing text.
            let before = edit.text.clone();
            edit.text
                .apply_style(
                    2,
                    2,
                    &TextStylePatch {
                        color: Some([0, 255, 0]),
                        ..Default::default()
                    },
                    edit.color,
                )
                .unwrap();
            assert_eq!(edit.text, before);
        }
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

#[cfg(test)]
mod stroke_width_tests {
    use super::*;
    #[test]
    fn stroke_style_patch_is_atomic_persisted_and_keeps_unrelated_settings() {
        use crate::stroke::*;
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        for (id, cap) in [("a", LineCap::Round), ("b", LineCap::Square)] {
            doc.upsert_vector_object(
                &layer,
                VectorObject {
                    live_corners: None,
                    rectangle_radii: None,
                    id: id.into(),
                    name: id.into(),
                    opacity: 1.,
                    blend_mode: "normal".into(),
                    group_path: vec![],
                    clipping_group: None,
                    bounds_reset: false,
                    path: VectorPath {
                        data: "M20 50C40 10 80 90 120 50".into(),
                        fill_rule: crate::vector::FillRule::NonZero,
                    },
                    transform: [1., 0., 0., 1., 0., 0.],
                    image_frame: None,
                    fill_gradient: None,
                    stroke_gradient: None,
                    fill: None,
                    stroke: Some(VectorPaint {
                        registration: false,
                        color: [10, 20, 30, 255],
                    }),
                    stroke_width: 8.,
                    stroke_style: StrokeStyle {
                        cap,
                        ..Default::default()
                    },
                    visible: true,
                    kind: VectorObjectKind::Path,
                    control_points: vec![[20., 50.], [120., 50.]],
                    text: None,
                },
            )
            .unwrap();
        }
        doc.select_vector_objects(vec!["a".into(), "b".into()])
            .unwrap();
        let before = doc.encode().unwrap();
        let patch = || StrokeStylePatch {
            profile: Some(WidthProfile::Custom),
            width_curve: Some(vec![
                crate::stroke::WidthStop {
                    position: 0.,
                    width: 0.5,
                    slope: 1.,
                },
                crate::stroke::WidthStop {
                    position: 1.,
                    width: 1.5,
                    slope: 1.,
                },
            ]),
            start_arrow_scale: Some(0.75),
            end_arrow_scale: Some(2.),
            contour_alignments: Some(vec![crate::stroke::StrokeAlignment::Outside]),
            dash_array: Some(vec![12., 5.]),
            end_arrow: Some(Arrowhead::Triangle),
            ..Default::default()
        };
        doc.set_selected_stroke_style(patch()).unwrap();
        assert_eq!(
            doc.svg_layers[0].vector_objects[0].stroke_style.cap,
            LineCap::Round
        );
        assert_eq!(
            doc.svg_layers[0].vector_objects[1].stroke_style.cap,
            LineCap::Square
        );
        let outline = doc.outline_view([0., 0.], 1.);
        assert!(!outline.svg_layers[0].source.contains("scale(8)"));
        assert!(outline.svg_layers[0]
            .vector_objects
            .iter()
            .all(|o| o.stroke_style == StrokeStyle::default()));
        assert_eq!(
            doc.svg_layers[0].vector_objects[0].stroke_style.profile,
            WidthProfile::Custom
        );
        let after = doc.encode().unwrap();
        let revision = doc.revision;
        doc.set_selected_stroke_style(patch()).unwrap();
        assert_eq!(doc.revision, revision);
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.redo();
        assert_eq!(doc.encode().unwrap(), after);
        let loaded = Document::decode(&after).unwrap();
        assert_eq!(
            loaded.svg_layers[0].vector_objects,
            doc.svg_layers[0].vector_objects
        );
        assert_eq!(
            doc.snapshot().layers[1].objects[0].stroke_style,
            doc.svg_layers[0].vector_objects[0].stroke_style
        );
        doc.svg_layers[0].vector_objects[1].visible = false;
        let locked = doc.encode().unwrap();
        assert!(doc
            .set_selected_stroke_style(StrokeStylePatch {
                join: Some(LineJoin::Bevel),
                ..Default::default()
            })
            .is_err());
        assert_eq!(doc.encode().unwrap(), locked);
        doc.svg_layers[0].vector_objects[1].visible = true;
        for invalid in [f32::NAN, 0., 101.] {
            assert!(doc
                .set_selected_stroke_style(StrokeStylePatch {
                    miter_limit: Some(invalid),
                    ..Default::default()
                })
                .is_err());
            assert_eq!(doc.encode().unwrap(), after);
        }
        let mut legacy: serde_json::Value = serde_json::from_slice(&after).unwrap();
        for object in legacy["svgLayers"][0]["vectorObjects"]
            .as_array_mut()
            .unwrap()
        {
            object.as_object_mut().unwrap().remove("strokeStyle");
        }
        let legacy = Document::decode(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(
            legacy.svg_layers[0].vector_objects[0].stroke_style,
            StrokeStyle::default()
        );
    }
    #[test]
    fn selected_stroke_width_is_atomic_undoable_and_keeps_geometry() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        let object = VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: "path-a".into(),
            name: "Path".into(),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
            text: None,
            path: VectorPath {
                data: "M 0 0 L 100 100".into(),
                fill_rule: crate::vector::FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: None,
            stroke: Some(VectorPaint {
                registration: false,
                color: [10, 20, 30, 255],
            }),
            stroke_style: Default::default(),
            stroke_width: 1.,
            visible: true,
            kind: VectorObjectKind::Path,
            control_points: vec![[0., 0.], [100., 100.]],
        };
        document
            .upsert_vector_object(&layer, object.clone())
            .unwrap();
        let mut second = object.clone();
        second.id = "path-b".into();
        second.stroke_width = 3.;
        document.upsert_vector_object(&layer, second).unwrap();
        document
            .select_vector_objects(vec!["path-a".into(), "path-b".into()])
            .unwrap();
        document
            .set_selected_stroke_width(12., [255, 0, 0])
            .unwrap();
        assert!(document.svg_layers[0]
            .vector_objects
            .iter()
            .all(|object| object.stroke_width == 12.));
        assert_eq!(document.svg_layers[0].vector_objects[0].path, object.path);
        assert_eq!(
            document.svg_layers[0].vector_objects[0].stroke,
            object.stroke
        );
        assert!(document.svg_layers[0]
            .source
            .contains("stroke-width=\"12\""));
        document.undo();
        assert_eq!(document.svg_layers[0].vector_objects[0].stroke_width, 1.);
        assert_eq!(document.svg_layers[0].vector_objects[1].stroke_width, 3.);
        document.redo();
        assert_eq!(document.snapshot().layers[1].objects[0].stroke_width, 12.);
        let revision = document.revision;
        document.svg_layers[0].locked = true;
        assert!(document.set_selected_stroke_width(5., [0, 0, 0]).is_err());
        assert_eq!(document.revision, revision);
        assert!(document.svg_layers[0]
            .vector_objects
            .iter()
            .all(|object| object.stroke_width == 12.));
        document.svg_layers[0].locked = false;
        assert!(document
            .set_selected_stroke_width(f32::NAN, [0, 0, 0])
            .is_err());
        document.set_selected_stroke_width(0., [0, 0, 0]).unwrap();
        assert_eq!(document.svg_layers[0].vector_objects[0].stroke_width, 0.);
    }
}

#[cfg(test)]
mod clipboard_tests {
    use super::*;
    #[test]
    fn paste_uses_one_history_entry_and_restores_selection_and_layer() {
        let mut document = Document::default();
        document.select_layer("layer-1".into()).unwrap();
        document.paste_content(Some("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\"/></svg>".into()),vec![]).unwrap();
        let id = document.snapshot().layer_id;
        assert_ne!(id, "layer-1");
        assert_eq!(document.svg_layers.len(), 1);
        document.undo();
        assert_eq!(document.svg_layers.len(), 0);
        assert_eq!(document.snapshot().layer_id, "layer-1");
        document.redo();
        assert_eq!(document.svg_layers.len(), 1);
        assert_eq!(document.snapshot().layer_id, id);
        let restored = Document::decode(&document.encode().unwrap()).unwrap();
        assert_eq!(restored.svg_layers[0].source, document.svg_layers[0].source);
    }
    #[test]
    fn cut_retains_selection_and_is_undoable_and_persisted() {
        let mut document = Document::default();
        document
            .begin(Point { x: 20., y: 20. }, Brush::default())
            .unwrap();
        document.finish();
        document.select_all();
        document.cut_paint_selection().unwrap();
        assert_eq!(document.strokes.len(), 2);
        assert!(document.strokes[1].clear);
        document.undo();
        assert_eq!(document.strokes.len(), 1);
        document.redo();
        assert_eq!(document.strokes.len(), 2);
        assert_eq!(document.point_count, 2);
        let restored = Document::decode(&document.encode().unwrap()).unwrap();
        assert!(restored.strokes[1].clear);
        document.layer_locked = true;
        assert!(document.cut_paint_selection().is_err());
        assert_eq!(document.strokes.len(), 2);
    }
}

#[cfg(test)]
mod transform_menu_tests {
    use super::*;
    #[test]
    fn transforms_follow_oriented_box_and_reset_without_changing_artwork() {
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Transform".into(),
                box_width: 100.,
                ..Default::default()
            },
            position: [100., 100.],
            color: [0, 0, 0],
        })
        .unwrap();
        let original = doc.svg_layers().next().unwrap().vector_objects[0].clone();
        doc.transform_selected_vectors("rotate", [45., 0., 0., 0.])
            .unwrap();
        let corners = doc.selected_vector_box().unwrap();
        assert!((corners[1][1] - corners[0][1]).abs() > 1.);
        let artwork = doc.svg_layers().next().unwrap().source.clone();
        doc.transform_selected_vectors("reset", [0.; 4]).unwrap();
        let reset = doc.selected_vector_box().unwrap();
        assert_eq!(reset[0][1], reset[1][1]);
        assert_eq!(doc.svg_layers().next().unwrap().source, artwork);
        doc.undo();
        assert_eq!(doc.selected_vector_box().unwrap(), corners);
        doc.undo();
        assert_eq!(doc.svg_layers().next().unwrap().vector_objects[0], original);
        for (action, values) in [
            ("move", [20., 30., 0., 0.]),
            ("reflect", [90., 0., 0., 0.]),
            ("scale", [2., 0.5, 0., 0.]),
            ("shear", [20., 0., 0., 0.]),
            ("individual", [1.5, 0.5, 30., 0.]),
        ] {
            doc.transform_selected_vectors(action, values).unwrap();
            let changed = doc.svg_layers().next().unwrap().vector_objects[0].clone();
            assert_ne!(changed.transform, original.transform);
            let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
            assert_eq!(
                loaded.svg_layers().next().unwrap().vector_objects[0],
                changed
            );
            doc.undo();
            assert_eq!(doc.svg_layers().next().unwrap().vector_objects[0], original);
            doc.redo();
            assert_eq!(doc.svg_layers().next().unwrap().vector_objects[0], changed);
            doc.undo();
        }
        assert!(doc
            .transform_selected_vectors("scale", [0., 1., 0., 0.])
            .is_err());
        assert!(doc
            .transform_selected_vectors("shear", [90., 0., 0., 0.])
            .is_err());
        assert_eq!(doc.svg_layers().next().unwrap().vector_objects[0], original);
    }
}

#[cfg(test)]
mod independent_saved_path_tests {
    use super::*;
    use crate::vector::{FillRule, VectorObjectKind, VectorPaint, VectorPath};

    fn square(id: &str) -> VectorObject {
        VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: id.into(),
            name: "Square".into(),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: "M 10 10 H 80 V 80 H 10 Z".into(),
                fill_rule: FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                registration: false,
                color: [255, 0, 0, 255],
            }),
            stroke: None,
            stroke_style: Default::default(),
            stroke_width: 0.,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[10., 10.], [80., 10.], [80., 80.], [10., 80.]],
            text: None,
        }
    }

    #[test]
    fn preview_translation_matches_commit_without_history_or_source_mutation() {
        for variant in 0..3 {
            let mut doc = Document::default();
            let layer = if variant == 2 {
                doc.saved_path_action("new", None, "Cutout").unwrap();
                doc.selected_vector_target().unwrap().unwrap()
            } else {
                doc.add_vector_layer().unwrap()
            };
            doc.upsert_vector_object(&layer, square("moving")).unwrap();
            doc.select_vector_objects(vec!["moving".into()]).unwrap();
            if variant == 1 {
                doc.noncanonical_vector_sources.insert(layer);
            }
            let original = serde_json::to_value(doc.document_state()).unwrap();
            let mut expected = doc.clone();
            expected.move_selected_vectors(5., 7.).unwrap();
            let mut preview = doc.clone_for_rendering();
            assert!(preview.move_selected_vectors_preview(5., 7.).unwrap());
            assert_eq!(
                serde_json::to_value(preview.document_state()).unwrap(),
                serde_json::to_value(expected.document_state()).unwrap()
            );
            assert!(preview.vector_undo.is_empty());
            assert!(preview.undo_order.is_empty());
            assert_eq!(preview.translation_metrics.history_transform_bytes, 0);
            assert_eq!(preview.translation_metrics.full_layer_snapshots, 0);
            let state = serde_json::to_value(preview.document_state()).unwrap();
            let revision = preview.revision();
            let cursor = preview.scene_journal.cursor();
            assert!(preview.move_selected_vectors_preview(f32::NAN, 2.).is_err());
            assert_eq!(
                serde_json::to_value(preview.document_state()).unwrap(),
                state
            );
            assert_eq!(preview.revision(), revision);
            assert_eq!(preview.scene_journal.cursor(), cursor);
            let outline = doc.outline_view([5., 7.], 1.);
            assert!(outline.vector_undo.is_empty());
            assert!(outline.undo_order.is_empty());
            assert_eq!(
                outline.svg_layers[0].vector_objects[0].transform[4..],
                [5., 7.]
            );
            assert_eq!(
                serde_json::to_value(doc.document_state()).unwrap(),
                original
            );
            expected.undo();
            assert_eq!(
                serde_json::to_value(expected.document_state()).unwrap(),
                original
            );
        }
    }
    #[test]
    fn independent_paths_isolate_tools_persistence_clipping_and_history() {
        let mut doc = Document::default();
        let artwork = doc.add_vector_layer().unwrap();
        doc.upsert_vector_object(&artwork, square("artwork"))
            .unwrap();
        doc.select_layer(artwork.clone()).unwrap();
        let original = doc.svg_layers[0].clone();
        doc.saved_path_action("new", None, "Cutout").unwrap();
        let id = doc.snapshot().active_saved_path.unwrap();
        let target = doc.selected_vector_target().unwrap().unwrap();
        assert_eq!(doc.snapshot().layers.len(), 2);
        assert!(doc.saved_path_action("clip", Some(&id), "").is_err());
        assert!(doc
            .select_vector_at(Point { x: 30., y: 30. }, 2., false)
            .is_none());
        assert!(doc.select_vector_objects(vec!["artwork".into()]).is_err());
        assert!(doc.upsert_vector_object(&artwork, square("wrong")).is_err());
        doc.upsert_vector_object(&target, square("cutout")).unwrap();
        assert_eq!(doc.saved_paths[0].objects.len(), 1);
        assert_eq!(
            doc.select_vector_at(Point { x: 30., y: 30. }, 2., false)
                .as_deref(),
            Some("cutout")
        );
        doc.move_selected_vectors(5., 7.).unwrap();
        assert_eq!(doc.saved_paths[0].objects[0].transform[4..], [5., 7.]);
        assert_eq!(doc.svg_layers[0].source, original.source);
        assert_eq!(doc.visible_svg_layers().count(), 1);
        doc.saved_path_action("clip", Some(&id), "").unwrap();
        let mut preview = doc.clone();
        preview
            .append_vector_guide(&target, square("guide"))
            .unwrap();
        assert_eq!(preview.saved_paths[0].objects.len(), 1);
        let view = doc.clipping_view().saved_path_edit_view(1.);
        let guide = view.visible_svg_layers().last().unwrap();
        assert_eq!(guide.vector_objects[0].fill, None);
        assert_eq!(
            guide.vector_objects[0].stroke.unwrap().color,
            [48, 144, 255, 255]
        );
        let decoded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert!(!decoded.has_active_saved_path());
        assert_eq!(decoded.svg_layers.len(), 1);
        assert_eq!(decoded.saved_paths[0].objects.len(), 1);
        assert!(decoded.has_document_clipping());
        doc.select_layer(artwork).unwrap();
        assert!(!doc.has_active_saved_path());
        doc.undo(); // clipping assignment
        doc.undo(); // path movement
        assert!(!doc.has_active_saved_path());
        assert_eq!(doc.saved_paths[0].objects[0].transform[4..], [0., 0.]);
        doc.redo();
        assert!(!doc.has_active_saved_path());
        assert_eq!(doc.saved_paths[0].objects[0].transform[4..], [5., 7.]);
        doc.saved_path_action("activate", Some(&id), "").unwrap();
        assert!(doc.selected_layer_is_vector());
        doc.saved_path_action("delete", Some(&id), "").unwrap();
        assert!(!doc.has_active_saved_path());
        assert!(doc.saved_paths.is_empty());
        doc.undo();
        assert_eq!(doc.saved_paths.len(), 1);
        assert!(!doc.has_active_saved_path());
    }

    fn open_curve(id: &str, start: f32, end: f32) -> VectorObject {
        let mut object = square(id);
        object.kind = VectorObjectKind::Bezier;
        object.control_points = vec![[start, 0.], [start, 30.], [end, 30.], [end, 0.]];
        object.path.data = crate::bezier::path_data(&object.control_points, false).unwrap();
        object
    }

    #[test]
    fn arrow_only_pick_and_selection_boxes_follow_drawn_bounds() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let mut object = open_curve("arrow", 0., 100.);
        object.fill = None;
        object.stroke = Some(VectorPaint {
            registration: false,
            color: [0, 0, 0, 255],
        });
        object.stroke_width = 10.;
        object.stroke_style.end_arrow = crate::stroke::Arrowhead::Circle;
        object.stroke_style.end_arrow_scale = Some(2.);
        doc.upsert_vector_object(&layer, object.clone()).unwrap();
        doc.select_vector_objects(vec!["arrow".into()]).unwrap();
        assert!(object.hit_test([120., 0.], 0.));
        assert!(object.intersects_selection([118., -2., 4., 4.], false));
        let box_points = doc.selected_vector_box().unwrap();
        assert!((box_points[1][0] - 130.).abs() < 0.01);
        assert!((box_points[0][1] + 30.).abs() < 0.01);
        object.bounds_reset = true;
        doc.upsert_vector_object(&layer, object).unwrap();
        assert!((doc.selected_vector_box().unwrap()[1][0] - 130.).abs() < 0.01);
        let overlay = doc.vector_overlay_selections();
        assert!(!overlay.is_empty());
    }
    #[test]
    fn selected_endpoints_close_saved_path_and_support_undo_and_clipping() {
        let mut doc = Document::default();
        doc.saved_path_action("new", None, "Outline").unwrap();
        let id = doc.snapshot().active_saved_path.unwrap();
        let layer = doc.selected_vector_target().unwrap().unwrap();
        doc.upsert_vector_object(&layer, open_curve("curve", 10., 100.))
            .unwrap();
        doc.select_vector_objects(vec!["curve".into()]).unwrap();
        let before = doc.encode().unwrap();
        doc.join_path_endpoints(&[("curve".into(), 0), ("curve".into(), 3)])
            .unwrap();
        let closed = &doc.saved_paths[0].objects[0];
        assert!(closed.path.data.ends_with('Z'));
        assert_eq!(closed.control_points.len(), 7);
        assert_eq!(closed.control_points[0], closed.control_points[6]);
        assert_eq!(doc.snapshot().layers.len(), 1);
        let after = doc.encode().unwrap();
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.redo();
        assert_eq!(doc.encode().unwrap(), after);
        doc.saved_path_action("clip", Some(&id), "").unwrap();
        assert!(Document::decode(&doc.encode().unwrap())
            .unwrap()
            .has_document_clipping());
    }

    #[test]
    fn explicit_join_uses_selected_ends_instead_of_nearest_ends() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        doc.upsert_vector_object(&layer, open_curve("a", 0., 10.))
            .unwrap();
        let mut second = open_curve("b", 11., 20.);
        second.transform[4] = 100.;
        doc.upsert_vector_object(&layer, second).unwrap();
        doc.select_vector_objects(vec!["a".into(), "b".into()])
            .unwrap();
        doc.join_path_endpoints(&[("a".into(), 0), ("b".into(), 3)])
            .unwrap();
        let object = &doc.svg_layers[0].vector_objects[0];
        assert_eq!(doc.svg_layers[0].vector_objects.len(), 1);
        assert_eq!(object.control_points[0], [10., 0.]);
        assert_eq!(object.control_points[3], [0., 0.]);
        assert_eq!(object.control_points[6], [120., 0.]);
        assert_eq!(*object.control_points.last().unwrap(), [111., 0.]);
        assert!(!object.path.data.ends_with('Z'));
    }

    #[test]
    fn bezier_bounding_boxes_follow_curve_instead_of_handles() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let mut curve = open_curve("curve", 0., 100.);
        curve.control_points = vec![[0., 0.], [0., 100.], [100., 100.], [100., 0.]];
        curve.path.data = crate::bezier::path_data(&curve.control_points, false).unwrap();
        doc.upsert_vector_object(&layer, curve.clone()).unwrap();
        doc.select_vector_objects(vec!["curve".into()]).unwrap();
        assert_eq!(doc.selected_vector_bounds().unwrap(), [0., 0., 100., 75.]);
        assert_eq!(
            doc.selected_vector_box().unwrap(),
            [[0., 0.], [100., 0.], [100., 75.], [0., 75.]]
        );
        let original = doc.encode().unwrap();
        let angle = 0.7_f32;
        curve.transform = [
            angle.cos(),
            angle.sin(),
            -angle.sin(),
            angle.cos(),
            25.,
            40.,
        ];
        doc.upsert_vector_object(&layer, curve.clone()).unwrap();
        let corners = doc.selected_vector_box().unwrap();
        for (actual, local) in
            corners
                .into_iter()
                .zip([[0., 0.], [100., 0.], [100., 75.], [0., 75.]])
        {
            let expected = crate::bezier::world_point(&curve, local);
            assert!(
                (actual[0] - expected[0]).abs() < 0.001 && (actual[1] - expected[1]).abs() < 0.001
            );
        }
        let bounds = doc.selected_vector_bounds().unwrap();
        for point in crate::bezier::flattened(&curve.control_points) {
            let [x, y] = crate::bezier::world_point(&curve, point);
            assert!(
                x >= bounds[0] - 0.001
                    && x <= bounds[2] + 0.001
                    && y >= bounds[1] - 0.001
                    && y <= bounds[3] + 0.001
            );
        }
        doc.transform_selected_vectors("reset", [0.; 4]).unwrap();
        assert_eq!(
            doc.selected_vector_box().unwrap(),
            [
                [bounds[0], bounds[1]],
                [bounds[2], bounds[1]],
                [bounds[2], bounds[3]],
                [bounds[0], bounds[3]]
            ]
        );
        doc.undo();
        doc.undo();
        assert_eq!(doc.encode().unwrap(), original);
    }

    #[test]
    fn average_then_join_merges_endpoints_and_preserves_both_handles() {
        for reverse in [false, true] {
            let mut doc = Document::default();
            doc.saved_path_action("new", None, "Path").unwrap();
            let layer = doc.selected_vector_target().unwrap().unwrap();
            doc.upsert_vector_object(&layer, open_curve("a", 0., 10.))
                .unwrap();
            let mut b = open_curve("b", 20., 30.);
            b.transform = [1.3, 0.2, -0.4, 1.1, 3.7, 2.3];
            doc.upsert_vector_object(&layer, b).unwrap();
            doc.select_vector_objects(vec!["a".into(), "b".into()])
                .unwrap();
            let controls = vec![
                ("a".into(), if reverse { 0 } else { 3 }),
                ("b".into(), if reverse { 3 } else { 0 }),
            ];
            doc.average_vector_controls(&controls).unwrap();
            let before = doc.encode().unwrap();
            let objects = &doc.saved_paths[0].objects;
            let incoming = crate::bezier::world_point(
                &objects[0],
                objects[0].control_points[if reverse { 1 } else { 2 }],
            );
            let outgoing = crate::bezier::world_point(
                &objects[1],
                objects[1].control_points[if reverse { 2 } else { 1 }],
            );
            doc.join_path_endpoints(&controls).unwrap();
            let joined = &doc.saved_paths[0].objects[0];
            assert_eq!(doc.saved_paths[0].objects.len(), 1);
            assert_eq!(joined.control_points.len(), 7); // three anchors, two curves
            assert_eq!(joined.path.data.matches(" C ").count(), 2);
            assert!(coincident_path_endpoints(
                joined.control_points[2],
                incoming
            ));
            assert!(coincident_path_endpoints(
                joined.control_points[4],
                outgoing
            ));
            let after = doc.encode().unwrap();
            assert_eq!(
                Document::decode(&after).unwrap().saved_paths[0].objects[0]
                    .control_points
                    .len(),
                7
            );
            doc.undo();
            assert_eq!(doc.encode().unwrap(), before);
            doc.redo();
            assert_eq!(doc.encode().unwrap(), after);
        }
    }

    #[test]
    fn coincident_ends_close_without_an_extra_segment_but_distinct_ends_stay_distinct() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let mut curve = open_curve("a", 1., 1. + 1e-6);
        curve.transform = [1.3, 0.2, -0.4, 1.1, 3.7, 2.3];
        doc.upsert_vector_object(&layer, curve).unwrap();
        doc.select_vector_objects(vec!["a".into()]).unwrap();
        doc.join_path_endpoints(&[("a".into(), 0), ("a".into(), 3)])
            .unwrap();
        let closed = &doc.svg_layers[0].vector_objects[0];
        assert_eq!(closed.control_points.len(), 4);
        assert_eq!(closed.control_points[0], closed.control_points[3]);
        assert!(closed.path.data.ends_with('Z'));
        assert!(!coincident_path_endpoints([1., 0.], [1.001, 0.]));
    }

    #[test]
    fn joining_handles_duplicates_or_closed_paths_is_atomic() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        doc.upsert_vector_object(&layer, open_curve("a", 0., 10.))
            .unwrap();
        doc.select_vector_objects(vec!["a".into()]).unwrap();
        let before = doc.encode().unwrap();
        for indices in [vec![0], vec![0, 0], vec![0, 1], vec![0, 99], vec![0, 1, 3]] {
            let controls = indices
                .into_iter()
                .map(|i| ("a".into(), i))
                .collect::<Vec<_>>();
            assert!(doc.join_path_endpoints(&controls).is_err());
            assert_eq!(doc.encode().unwrap(), before);
        }
        doc.join_path_endpoints(&[("a".into(), 0), ("a".into(), 3)])
            .unwrap();
        let closed = doc.encode().unwrap();
        assert!(doc
            .join_path_endpoints(&[("a".into(), 0), ("a".into(), 6)])
            .is_err());
        assert_eq!(doc.encode().unwrap(), closed);
    }

    #[test]
    fn path_colors_match_snapshots_and_remain_stable_after_reload() {
        let mut doc = Document::default();
        let first = doc.add_vector_layer().unwrap();
        let second = doc.add_vector_layer().unwrap();
        assert_ne!(
            doc.layer_guide_color(&first),
            doc.layer_guide_color(&second)
        );
        doc.saved_path_action("new", None, "One").unwrap();
        doc.saved_path_action("new", None, "Two").unwrap();
        let snapshot = doc.snapshot();
        let active_layer = doc.selected_vector_target().unwrap().unwrap();
        assert_eq!(
            doc.layer_guide_color(&active_layer),
            snapshot.saved_paths[1].guide_color
        );
        assert_ne!(
            snapshot.saved_paths[0].guide_color,
            snapshot.saved_paths[1].guide_color
        );
        doc.upsert_vector_object(&active_layer, square("shape"))
            .unwrap();
        let view = doc.saved_path_edit_view(1.);
        assert_eq!(
            view.svg_layers().last().unwrap().vector_objects[0]
                .stroke
                .unwrap()
                .color,
            snapshot.saved_paths[1].guide_color
        );
        let restored = Document::decode(&doc.encode().unwrap()).unwrap().snapshot();
        assert_eq!(
            restored.saved_paths[1].guide_color,
            snapshot.saved_paths[1].guide_color
        );
        assert_eq!(
            restored.layers[2].guide_color,
            snapshot.layers[2].guide_color
        );
    }

    #[test]
    fn empty_and_open_paths_round_trip_without_becoming_artwork() {
        let mut doc = Document::default();
        doc.saved_path_action("new", None, "Draft").unwrap();
        let id = doc.snapshot().active_saved_path.unwrap();
        assert!(Document::decode(&doc.encode().unwrap())
            .unwrap()
            .saved_paths[0]
            .objects
            .is_empty());
        let mut open = square("open");
        open.path.data = "M 10 10 L 80 80".into();
        let target = doc.selected_vector_target().unwrap().unwrap();
        doc.upsert_vector_object(&target, open).unwrap();
        assert!(doc.saved_path_action("clip", Some(&id), "").is_err());
        let decoded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(decoded.saved_paths[0].objects.len(), 1);
        assert!(decoded.svg_layers.is_empty());
    }
}

#[cfg(test)]
impl Document {
    pub fn encode(&mut self) -> Result<Vec<u8>, String> {
        self.finish();
        let mut value = serde_json::to_value(self.document_state()).map_err(|e| e.to_string())?;
        value["format"] = "LumaPaint".into();
        value["version"] = 1.into();
        serde_json::to_vec(&value).map_err(|e| e.to_string())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Project exceeds 8 MiB".into());
        }
        let mut value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let fields = value.as_object_mut().ok_or("Invalid project")?;
        if fields.remove("format") != Some("LumaPaint".into())
            || fields.remove("version") != Some(1.into())
        {
            return Err("Unsupported project format or version".into());
        }
        Self::from_document_state(serde_json::from_value(value).map_err(|e| e.to_string())?)
    }
}

#[cfg(test)]
mod gradient_document_tests {
    use super::*;
    use crate::gradient::*;
    fn fixture() -> (Document, String, Gradient) {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let object:VectorObject=serde_json::from_value(serde_json::json!({"id":"gradient-path","name":"Gradient path","path":{"data":"M0 0H100V100H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":{"color":[255,0,0,255]},"stroke":null,"strokeWidth":0,"visible":true,"controlPoints":[[0,0],[100,100]]})).unwrap();
        doc.upsert_vector_object(&layer, object).unwrap();
        doc.select_vector_objects(vec!["gradient-path".into()])
            .unwrap();
        let g = Gradient {
            geometry: None,
            pixel_style: None,
            kind: GradientKind::Radial,
            angle: 45.,
            aspect: 0.5,
            dither: false,
            method: GradientMethod::Classic,
            stops: vec![
                GradientStop {
                    position: 0.,
                    color: [255, 255, 0, 255],
                    midpoint: 0.3,
                },
                GradientStop {
                    position: 1.,
                    color: [255, 0, 0, 128],
                    midpoint: 0.5,
                },
            ],
        };
        (doc, layer, g)
    }
    #[test]
    fn dragged_geometry_uses_inverse_object_transform_and_round_trips() {
        let (mut doc, layer, mut g) = fixture();
        let mut object = doc
            .svg_layers()
            .find(|l| l.id == layer)
            .unwrap()
            .vector_objects[0]
            .clone();
        object.transform = [2., 0., 0., 3., 40., 60.];
        doc.upsert_vector_object(&layer, object).unwrap();
        let ids = vec!["gradient-path".into()];
        g.geometry = Some([100., 0., 0., 50., 80., 90.]);
        doc.set_dragged_vector_gradient(&ids, "fill", g).unwrap();
        let stored = doc
            .snapshot()
            .layers
            .into_iter()
            .find(|l| l.id == layer)
            .unwrap()
            .objects[0]
            .fill_gradient
            .clone()
            .unwrap();
        assert_eq!(stored.geometry, Some([50., 0., 0., 50. / 3., 20., 10.]));
        let decoded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            decoded
                .snapshot()
                .layers
                .into_iter()
                .find(|l| l.id == layer)
                .unwrap()
                .objects[0]
                .fill_gradient,
            Some(stored)
        );
        doc.undo();
        assert!(doc
            .snapshot()
            .layers
            .into_iter()
            .find(|l| l.id == layer)
            .unwrap()
            .objects[0]
            .fill_gradient
            .is_none());
    }
    #[test]
    fn gradients_persist_undo_and_replace_with_solid() {
        let (mut doc, _, g) = fixture();
        let ids = vec!["gradient-path".into()];
        doc.set_selected_vector_gradient(&ids, "fill", g.clone())
            .unwrap();
        assert!(doc
            .svg_layers()
            .next()
            .unwrap()
            .source
            .contains("radialGradient"));
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            loaded
                .snapshot()
                .layers
                .iter()
                .flat_map(|l| &l.objects)
                .find(|o| o.id == ids[0])
                .unwrap()
                .fill_gradient,
            Some(g.clone())
        );
        let revision = doc.revision();
        doc.set_selected_vector_gradient(&ids, "fill", g.clone())
            .unwrap();
        assert_eq!(doc.revision(), revision);
        doc.undo();
        assert!(doc.svg_layers().next().unwrap().vector_objects[0]
            .fill_gradient
            .is_none());
        doc.redo();
        assert_eq!(
            doc.svg_layers().next().unwrap().vector_objects[0].fill_gradient,
            Some(g)
        );
        doc.set_selected_vector_paint(&ids, "fill", Some([10, 20, 30]))
            .unwrap();
        assert!(doc.svg_layers().next().unwrap().vector_objects[0]
            .fill_gradient
            .is_none());
        doc.undo();
        assert!(doc.svg_layers().next().unwrap().vector_objects[0]
            .fill_gradient
            .is_some());
    }
    #[test]
    fn invalid_or_stale_selection_is_atomic_and_stroke_gets_width() {
        let (mut doc, _, mut g) = fixture();
        let ids = vec!["gradient-path".into()];
        let bytes = doc.encode().unwrap();
        g.angle = f32::NAN;
        assert!(doc.set_selected_vector_gradient(&ids, "fill", g).is_err());
        assert_eq!(doc.encode().unwrap(), bytes);
        let (_, _, g) = fixture();
        assert!(doc
            .set_selected_vector_gradient(&["missing".into()], "fill", g.clone())
            .is_err());
        assert_eq!(doc.encode().unwrap(), bytes);
        doc.set_selected_vector_gradient(&ids, "stroke", g.clone())
            .unwrap();
        let o = &doc.svg_layers().next().unwrap().vector_objects[0];
        assert_eq!(o.stroke_gradient, Some(g));
        assert_eq!(o.stroke_width, 1.);
    }
}

impl Document {
    pub fn set_path_selection(
        &mut self,
        points: Vec<Point>,
        radius: f32,
        mode: SelectionMode,
    ) -> Result<(), String> {
        if points.len() < if radius > 0. { 1 } else { 3 } {
            return Ok(());
        }
        let padding = radius;
        let minx = points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min) - padding;
        let miny = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) - padding;
        let maxx = points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max) + padding;
        let maxy = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max) + padding;
        let mut region = SelectionRegion {
            shape: if radius > 0. {
                SelectionShape::Stroke
            } else {
                SelectionShape::Polygon
            },
            bounds: [
                minx,
                miny,
                (maxx - minx).max(0.001),
                (maxy - miny).max(0.001),
            ],
            points,
            radius,
            operation: SelectionOperation::Replace,
        };
        let mut result = match mode {
            SelectionMode::Replace => Selection {
                regions: Vec::new(),
            },
            SelectionMode::Add | SelectionMode::Intersect => {
                self.selection.clone().unwrap_or(Selection {
                    regions: Vec::new(),
                })
            }
            SelectionMode::Subtract => self.selection.clone().unwrap_or_else(|| {
                Selection::new(
                    SelectionShape::Rectangle,
                    [0., 0., self.width as f32, self.height as f32],
                )
            }),
        };
        result.ensure_capacity()?;
        region.operation = if result.regions.is_empty() {
            SelectionOperation::Replace
        } else if mode == SelectionMode::Intersect {
            SelectionOperation::Intersect
        } else if mode == SelectionMode::Subtract {
            SelectionOperation::Subtract
        } else {
            SelectionOperation::Add
        };
        result.regions.push(region);
        result.validate()?;
        self.selection = Some(result);
        Ok(())
    }
}
impl Document {
    pub fn commit_path_selection(&mut self, selection: Option<Selection>) -> Result<bool, String> {
        if let Some(s) = &selection {
            s.validate()?;
        }
        if self.selection == selection {
            return Ok(false);
        }
        self.finish();
        let previous = std::mem::replace(&mut self.selection, selection);
        self.vector_undo
            .push(VectorHistoryEntry::PixelSelection(previous));
        self.vector_redo.clear();
        self.redo.clear();
        self.redo_order.clear();
        self.undo_order.push(HistoryKind::Vector);
        self.revision += 1;
        Ok(true)
    }
    pub fn set_tool_pixel_selection(&mut self, selection: Option<Selection>) -> Result<(), String> {
        if let Some(s) = &selection {
            s.validate()?;
        }
        self.selection = selection;
        Ok(())
    }
}
impl Document {
    pub fn selection_sampling_document(&self, all_layers: bool) -> Self {
        let mut result = self.clone_for_rendering();
        result.finish();
        result.selection = None;
        if !all_layers {
            if let Some(id) = &self.selected_layer {
                result.paint_source = None;
                result.strokes.clear();
                result.point_count = 0;
                result.svg_layers.retain(|l| &l.id == id);
            } else {
                result.svg_layers.clear();
            }
        }
        result
    }
}

#[cfg(test)]
mod text_frame_overflow_tests {
    use super::text_line_fits_frame;
    use crate::vector::{VectorText, WritingMode};
    #[test]
    fn hides_incomplete_rows_and_columns_but_keeps_exact_fit() {
        let mut text = VectorText {
            font_size: 20.,
            box_width: 50.,
            box_height: Some(50.),
            ..VectorText::default()
        };
        assert!(text_line_fits_frame(&text, 0, "日本語abc中文", 46., [0; 3]));
        assert!(!text_line_fits_frame(
            &text,
            0,
            "日本語abc中文",
            47.,
            [0; 3]
        ));
        text.writing_mode = WritingMode::Vertical;
        assert!(text_line_fits_frame(&text, 0, "日本語", 10., [0; 3]));
        assert!(!text_line_fits_frame(&text, 0, "日本語", 9., [0; 3]));
        text.box_height = None;
        assert!(text_line_fits_frame(&text, 0, "日本語", -10., [0; 3]));
    }
}

#[cfg(test)]
mod text_frame_svg_tests {
    use super::{Document, TextSettings};
    use crate::vector::{VectorText, WritingMode};
    #[test]
    fn overflowing_last_line_stays_editable_and_returns_when_frame_grows() {
        for mode in [WritingMode::Horizontal, WritingMode::Vertical] {
            let mut doc = Document::default();
            let mut text = VectorText {
                content: "Visible\nHidden".into(),
                font_size: 20.,
                box_width: 200.,
                box_height: Some(50.),
                writing_mode: mode,
                line_baselines: if mode == WritingMode::Horizontal {
                    vec![20., 48.]
                } else {
                    vec![10., 195.]
                },
                ..VectorText::default()
            };
            doc.set_text_object(TextSettings {
                id: None,
                text: text.clone(),
                position: [0., 0.],
                color: [0; 3],
            })
            .unwrap();
            let created = doc.snapshot().text_objects[0].clone();
            let svg = &doc.svg_layers().next().unwrap().source;
            assert!(svg.contains("Visible"), "{svg}");
            assert!(!svg.contains(">Hidden<"), "{svg}");
            assert_eq!(created.text.content, text.content);
            if mode == WritingMode::Horizontal {
                text.box_height = Some(80.);
            } else {
                text.box_width = 240.;
            }
            doc.set_text_object(TextSettings {
                id: Some(created.id),
                text,
                position: [0., 0.],
                color: [0; 3],
            })
            .unwrap();
            assert!(doc.svg_layers().next().unwrap().source.contains(">Hidden<"));
        }
    }
}

fn history_bytes(entry: &VectorHistoryEntry) -> usize {
    match entry {
        VectorHistoryEntry::Stored(_) => 0,
        VectorHistoryEntry::State(state) => {
            state
                .layers
                .iter()
                .map(|layer| layer.source.len() + layer.vector_objects.len() * 512)
                .sum::<usize>()
                + state.paint_source.as_ref().map_or(0, String::len)
                + state.strokes.as_ref().map_or(0, |strokes| {
                    strokes
                        .iter()
                        .map(|stroke| stroke.points.len() * 32)
                        .sum::<usize>()
                })
        }
        VectorHistoryEntry::Objects { edits, .. } => edits.len() * 80,
        _ => 4096,
    }
}
