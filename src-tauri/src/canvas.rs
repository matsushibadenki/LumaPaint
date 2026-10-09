use lumapaint_core::document::{
    Brush, ColorMode, ColorProfile, DocumentSettings, DocumentSnapshot, LayerSettings, TextSettings,
};
use lumapaint_core::vector::{PathOperation, VectorObject};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
#[path = "canvas_macos.rs"]
pub(crate) mod platform;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanvasRequest {
    #[serde(default)]
    pub overlays: Vec<[f64; 4]>,
    #[serde(default)]
    pub overlay: Option<[f64; 4]>,
    #[serde(default)]
    pub channel: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub zoom: f64,
    #[serde(default)]
    pub absolute_zoom: bool,
    // Part of the shared IPC schema; native zoom synchronization is macOS-only.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    #[serde(default)]
    pub zoom_revision: Option<u64>,
    // Shared IPC setting consumed by the macOS native canvas renderer.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    #[serde(default)]
    pub pasteboard_color: Option<[u8; 3]>,
    pub dark: bool,
    pub visible: bool,
    #[serde(default)]
    pub brush: Brush,
    #[serde(default)]
    pub tool: CanvasTool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CanvasTool {
    #[default]
    Brush,
    Eraser,
    Blur,
    Sharpen,
    Smudge,
    CloneStamp,
    PaintBucket,
    Lasso,
    PolygonLasso,
    MagneticLasso,
    SelectionBrush,
    Eyedropper,
    Gradient,
    Crop,
    Rectangle,
    Ellipse,
    VectorSelect,
    VectorDirectSelect,
    VectorScale,
    VectorRotate,
    VectorPen,
    VectorPencil,
    VectorAnchorAdd,
    VectorAnchorDelete,
    VectorAnchorConvert,
    VectorRectangle,
    VectorEllipse,
    ImageFrameRectangle,
    ImageFrameEllipse,
    Text,
    TextVertical,
    TextFrame,
    TextFrameVertical,
    ZoomIn,
    ZoomOut,
    Hand,
}

impl CanvasRequest {
    fn validate(&self) -> Result<(), String> {
        if self.overlays.len() > 10
            || self
                .overlays
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 65536.)
        {
            return Err("Invalid panel overlay bounds".into());
        }
        if self.channel > 8 {
            return Err("Invalid display channel".into());
        }
        if self
            .overlay
            .is_some_and(|rect| rect.iter().any(|v| !v.is_finite() || v.abs() > 65536.0))
        {
            return Err("Invalid overlay bounds".into());
        }
        self.brush.validate()?;
        if ![self.x, self.y, self.width, self.height, self.zoom]
            .iter()
            .all(|v| v.is_finite())
            || self.x.abs() > 32768.0
            || self.y.abs() > 32768.0
            || !(0.0..=16384.0).contains(&self.width)
            || !(0.0..=16384.0).contains(&self.height)
            || !(if self.absolute_zoom {
                self.zoom == 0.0 || (0.0313..=640.0).contains(&self.zoom)
            } else {
                (0.000001..=64000.0).contains(&self.zoom)
            })
        {
            return Err("Invalid canvas bounds or zoom".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RulerViewport {
    pub ruler_origin: [f32; 2],
    pub width: f32,
    pub height: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub zoom: f32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasInfo {
    pub status: &'static str,
    pub backend: String,
    pub adapter_name: String,
    pub physical_width: u32,
    pub physical_height: u32,
    pub scale_factor: f64,
    pub zoom: Option<f32>,
    pub ruler_viewport: Option<RulerViewport>,
    pub document: Option<DocumentSnapshot>,
}

impl CanvasInfo {
    fn inactive(status: &'static str) -> Self {
        Self {
            status,
            backend: String::new(),
            adapter_name: String::new(),
            physical_width: 0,
            physical_height: 0,
            scale_factor: 1.0,
            zoom: None,
            ruler_viewport: None,
            document: None,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentTabSnapshot {
    pub id: u64,
    pub file_name: Option<String>,
    pub dirty: bool,
    pub format: &'static str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentWorkspaceSnapshot {
    pub active_id: Option<u64>,
    pub active: Option<DocumentSnapshot>,
    pub documents: Vec<DocumentTabSnapshot>,
}

#[tauri::command]
pub async fn document_workspace(
    window: tauri::WebviewWindow,
) -> Result<DocumentWorkspaceSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, || Ok(platform::workspace_snapshot())).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(DocumentWorkspaceSnapshot {
            active_id: None,
            active: None,
            documents: vec![],
        })
    }
}

#[tauri::command]
pub async fn new_document(
    window: tauri::WebviewWindow,
    settings: lumapaint_core::document::NewDocumentSettings,
) -> Result<DocumentWorkspaceSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::new_document(settings)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn switch_document(
    window: tauri::WebviewWindow,
    id: u64,
) -> Result<DocumentWorkspaceSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::switch_document(id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn close_document(
    window: tauri::WebviewWindow,
    id: u64,
) -> Result<DocumentWorkspaceSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::close_document(id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn sync_canvas(
    window: tauri::WebviewWindow,
    request: CanvasRequest,
) -> Result<CanvasInfo, String> {
    if !crate::editor_windows::is_editor(window.label()) {
        return Err("Unknown editor window".into());
    }
    request.validate()?;
    #[cfg(target_os = "macos")]
    {
        // AppKit and surface creation must run on the main thread. Waiting happens off-thread.
        let label = crate::modal_windows::owner(&window)?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .with_webview(move |webview| {
                let _session = match platform::SessionGuard::enter(&label) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                };
                let result = platform::sync(webview.inner(), request);
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        crate::diagnostic_jobs::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())??
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Keep the IPC contract buildable while native hosts are implemented separately.
        // The native host is not implemented yet, but consume platform-only fields so
        // Windows/Linux CI still validates the complete IPC request without dead code.
        let _ = (request.dark, request.visible, request.tool);
        Ok(CanvasInfo::inactive("unsupported"))
    }
}

#[tauri::command]
pub async fn reset_canvas_pan(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::reset_pan).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(())
    }
}

#[tauri::command]
pub async fn finish_canvas_path(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::finish_open_pen).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(())
    }
}

pub fn close_window(label: &str) {
    #[cfg(target_os = "macos")]
    platform::window_sessions::close_window(label);
    #[cfg(not(target_os = "macos"))]
    let _ = label;
}
pub fn confirm_window(label: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        platform::window_sessions::confirm_window(label)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = label;
        true
    }
}

pub fn initialize(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    platform::initialize(app.clone());
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocumentAction {
    CreateTrimMarks,
    RegistrationFill,
    RegistrationStroke,
    Undo,
    Redo,
    ToggleLayer,
    SelectAll,
    Deselect,
    InvertSelection,
    DeleteSelectedObjects,
    ClearLayer,
    LockSelection,
    LockArtworkAbove,
    LockOtherLayers,
    UnlockAllObjects,
    HideSelection,
    HideArtworkAbove,
    HideOtherLayers,
    ShowAllObjects,
    Copy,
    Cut,
    Paste,
}

#[tauri::command]
pub async fn edit_document(
    window: tauri::WebviewWindow,
    action: DocumentAction,
) -> Result<DocumentSnapshot, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let label = crate::modal_windows::owner(&window)?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _session = match platform::SessionGuard::enter(&label) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                };
                let _ = tx.send(platform::edit(action));
            })
            .map_err(|e| e.to_string())?;
        crate::diagnostic_jobs::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())??
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = action;
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn toggle_layer(
    window: tauri::WebviewWindow,
    id: String,
) -> Result<DocumentSnapshot, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::toggle_layer(id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn create_screentone_layer(
    window: tauri::WebviewWindow,
    tone: lumapaint_core::screentone::Screentone,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::create_screentone_layer(tone)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, tone);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn create_layer_mask(
    window: tauri::WebviewWindow,
    id: String,
    kind: lumapaint_core::layer_mask::MaskKind,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        let job = on_main(window.clone(), move || {
            platform::prepare_layer_mask(id, kind)
        })
        .await?;
        let job = crate::diagnostic_jobs::spawn_blocking(move || platform::render_layer_mask(job))
            .await
            .map_err(|e| e.to_string())??;
        on_main(window, move || platform::commit_layer_mask(job)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, kind);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn transform_layer_mask(
    window: tauri::WebviewWindow,
    id: String,
    matrix: [f32; 6],
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        let job = on_main(window.clone(), move || {
            platform::prepare_mask_transform(id, matrix)
        })
        .await?;
        let job =
            crate::diagnostic_jobs::spawn_blocking(move || platform::render_mask_transform(job))
                .await
                .map_err(|e| e.to_string())??;
        on_main(window, move || platform::commit_mask_transform(job)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, matrix);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn set_layer_effects(
    window: tauri::WebviewWindow,
    id: String,
    effects: lumapaint_core::layer_effects::LayerEffects,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_layer_effects(id, effects)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, effects);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn set_raster_blend_mode(
    window: tauri::WebviewWindow,
    id: String,
    mode: lumapaint_core::tiles::RasterBlendMode,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_raster_blend_mode(id, mode)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, mode);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn set_layer_settings(
    window: tauri::WebviewWindow,
    settings: LayerSettings,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_layer_settings(settings)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn delete_layer(
    window: tauri::WebviewWindow,
    id: String,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::delete_layer(id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Native document editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn select_channel(
    window: tauri::WebviewWindow,
    channel: u32,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::select_channel(channel)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, channel);
        Err("Native editing is unavailable".into())
    }
}
#[tauri::command]
pub async fn select_layer_target(
    window: tauri::WebviewWindow,
    id: String,
    target: lumapaint_core::layer_mask::LayerEditTarget,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::select_layer_target(id, target)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, target);
        Err("Native editing is unavailable".into())
    }
}
#[tauri::command]
pub async fn select_layer(
    window: tauri::WebviewWindow,
    id: String,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::select_layer(id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Native editing is unavailable".into())
    }
}
#[tauri::command]
pub async fn add_paint_layer(window: tauri::WebviewWindow) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::add_paint_layer).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Native document editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn add_vector_layer(window: tauri::WebviewWindow) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::add_vector_layer).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Native document editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn set_text_object(
    window: tauri::WebviewWindow,
    settings: TextSettings,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_text_object(settings)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Native document editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn upsert_vector_object(
    window: tauri::WebviewWindow,
    layer_id: String,
    object: VectorObject,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::upsert_vector_object(layer_id, object)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, layer_id, object);
        Err("Native document editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn set_vector_stroke_width(
    window: tauri::WebviewWindow,
    width: f32,
    color: [u8; 3],
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::set_vector_stroke_width(width, color)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, width, color);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub fn stroke_preview(
    width: f32,
    style: lumapaint_core::stroke::StrokeStyle,
) -> Result<String, String> {
    style.validate()?;
    if !width.is_finite() || !(0.0..=4096.0).contains(&width) {
        return Err("Invalid stroke width".into());
    }
    let data = "M32 48L88 22L144 48L208 22";
    style.validate_geometry(data, width.min(12.))?;
    let geometry = lumapaint_core::stroke::selection_geometry(
        data,
        width.min(12.),
        &style,
        [1., 0., 0., 1., 0., 0.],
        false,
    );
    let mut bounds = [0_f64, 0., 240., 72.];
    for p in geometry.edges.into_iter().flatten() {
        bounds[0] = bounds[0].min(p[0] - 4.);
        bounds[1] = bounds[1].min(p[1] - 4.);
        bounds[2] = bounds[2].max(p[0] + 4.);
        bounds[3] = bounds[3].max(p[1] + 4.);
    }
    let [x, y, right, bottom] = bounds;
    let (view_width, view_height) = (right - x, bottom - y);
    let body = lumapaint_core::stroke::svg_stroke(
        data,
        data,
        "nonzero",
        width.min(12.),
        &style,
        "currentColor",
        0,
    );
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x} {y} {view_width} {view_height}">{body}</svg>"#
    ))
}

#[tauri::command]
pub async fn set_vector_stroke_style(
    window: tauri::WebviewWindow,
    patch: lumapaint_core::stroke::StrokeStylePatch,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_vector_stroke_style(patch)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, patch);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn select_vector_objects(
    window: tauri::WebviewWindow,
    ids: Vec<String>,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::select_vector_objects(ids)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, ids);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn set_vector_object_visibility(
    window: tauri::WebviewWindow,
    layer_id: String,
    object_id: String,
    visible: bool,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::set_vector_object_visibility(layer_id, object_id, visible)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, layer_id, object_id, visible);
        Err("Native document editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn arrange_selected_vectors(
    window: tauri::WebviewWindow,
    action: String,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::arrange_selected_vectors(action)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, action);
        Err("Native editing is unavailable".into())
    }
}
#[tauri::command]
pub async fn select_arrange_layer(
    window: tauri::WebviewWindow,
    id: String,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::select_arrange_layer(id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Native editing is unavailable".into())
    }
}
#[tauri::command]
pub async fn reorder_vector_objects(
    window: tauri::WebviewWindow,
    layer_id: String,
    ids: Vec<String>,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::reorder_vector_objects(layer_id, ids)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, layer_id, ids);
        Err("Native document editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn combine_selected_vectors(
    window: tauri::WebviewWindow,
    operation: PathOperation,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::combine_selected_vectors(operation)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, operation);
        Err("Vector path operations are not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn pathfinder_vectors(
    window: tauri::WebviewWindow,
    operation: lumapaint_core::vector::PathfinderOperation,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::pathfinder_vectors(operation)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, operation);
        Err("Vector operations are unavailable on this platform".into())
    }
}

#[tauri::command]
pub async fn make_compound_shape(
    window: tauri::WebviewWindow,
    operation: lumapaint_core::vector::PathfinderOperation,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::make_compound_shape(operation)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, operation);
        Err("Vector operations are unavailable on this platform".into())
    }
}
#[tauri::command]
pub async fn edit_compound_shape(
    window: tauri::WebviewWindow,
    edit: lumapaint_core::document::CompoundShapeEdit,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::edit_compound_shape(edit)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, edit);
        Err("Vector operations are unavailable on this platform".into())
    }
}

#[tauri::command]
pub async fn open_document_view(
    window: tauri::WebviewWindow,
    id: u64,
    target: String,
) -> Result<DocumentWorkspaceSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::open_document_view(id, &target)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, target);
        Err("Document views are unavailable on this platform".into())
    }
}

#[tauri::command]
pub async fn move_document_to_window(
    window: tauri::WebviewWindow,
    id: u64,
    target: String,
) -> Result<DocumentWorkspaceSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::move_document_to_window(id, &target)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, target);
        Err("Document transfer is unavailable on this platform".into())
    }
}

#[tauri::command]
pub async fn group_selected_vectors(
    window: tauri::WebviewWindow,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::group_selected_vectors).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Vector grouping is pending on this platform / このプラットフォームのグループ機能は保留中です / 此平台的矢量分组功能尚待实现".into())
    }
}

#[tauri::command]
pub async fn ungroup_selected_vectors(
    window: tauri::WebviewWindow,
    all: bool,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::ungroup_selected_vectors(all)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, all);
        Err("Vector grouping is pending on this platform / このプラットフォームのグループ機能は保留中です / 此平台的矢量分组功能尚待实现".into())
    }
}

#[tauri::command]
pub async fn edit_selected_paths(
    window: tauri::WebviewWindow,
    action: lumapaint_core::vector::PathEditAction,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::edit_selected_paths(action)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, action);
        Err("Path editing is pending on this platform / このプラットフォームのパス編集は保留中です / 此平台的路径编辑尚待实现".into())
    }
}
#[tauri::command]
pub async fn reorder_layers(
    window: tauri::WebviewWindow,
    ids: Vec<String>,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::reorder_layers(ids)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, ids);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn set_color_mode(
    window: tauri::WebviewWindow,
    mode: ColorMode,
) -> Result<DocumentSnapshot, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let label = crate::modal_windows::owner(&window)?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _session = match platform::SessionGuard::enter(&label) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                };
                let _ = tx.send(platform::set_color_mode(mode));
            })
            .map_err(|e| e.to_string())?;
        crate::diagnostic_jobs::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())??
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = mode;
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn set_bit_depth(
    window: tauri::WebviewWindow,
    depth: u8,
) -> Result<DocumentSnapshot, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let label = crate::modal_windows::owner(&window)?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _session = match platform::SessionGuard::enter(&label) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                };
                let _ = tx.send(platform::set_bit_depth(depth));
            })
            .map_err(|e| e.to_string())?;
        crate::diagnostic_jobs::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())??
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = depth;
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn set_color_profile(
    window: tauri::WebviewWindow,
    profile: ColorProfile,
) -> Result<DocumentSnapshot, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let label = crate::modal_windows::owner(&window)?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _session = match platform::SessionGuard::enter(&label) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                };
                let _ = tx.send(platform::set_color_profile(profile));
            })
            .map_err(|e| e.to_string())?;
        crate::diagnostic_jobs::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())??
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = profile;
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn set_document_settings(
    window: tauri::WebviewWindow,
    settings: DocumentSettings,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_document_settings(settings)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_preview_rejects_invalid_style_and_position() {
        let mut settings = TextSettings {
            id: None,
            text: lumapaint_core::vector::VectorText {
                content: "Preview".into(),
                ..Default::default()
            },
            position: [0., 0.],
            color: [0, 0, 0],
        };
        let valid = |settings: &TextSettings| {
            ToolPreviewRequest::Text {
                settings: Box::new(settings.clone()),
            }
            .validate()
        };
        assert!(valid(&settings).is_ok());
        settings.position[0] = f32::NAN;
        assert!(valid(&settings).is_err());
        settings.position = [0., 100_000.];
        assert!(valid(&settings).is_err());
        settings.position = [0., 0.];
        settings.text.font_size = -1.;
        assert!(valid(&settings).is_err());
    }

    #[test]
    fn rejects_non_finite_and_unbounded_native_frames() {
        let mut request = CanvasRequest {
            pasteboard_color: None,
            overlays: Vec::new(),
            overlay: None,
            channel: 0,
            x: 0.0,
            y: 0.0,
            width: 640.0,
            height: 480.0,
            zoom: 1.0,
            absolute_zoom: false,
            zoom_revision: None,
            dark: false,
            visible: true,
            brush: Brush::default(),
            tool: CanvasTool::Brush,
        };
        assert!(request.validate().is_ok());
        request.tool = CanvasTool::VectorSelect;
        assert!(request.validate().is_ok());
        request.x = f64::NAN;
        assert!(request.validate().is_err());
        request.x = 0.0;
        request.width = 16385.0;
        assert!(request.validate().is_err());
        request.width = 0.0;
        request.visible = false;
        assert!(request.validate().is_ok());
        request.absolute_zoom = true;
        for zoom in [0.0, 0.0313, 1.0, 640.0] {
            request.zoom = zoom;
            assert!(request.validate().is_ok());
        }
        for zoom in [-1.0, 0.001, 640.01, f64::NAN, f64::INFINITY] {
            request.zoom = zoom;
            assert!(request.validate().is_err());
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileAction {
    Export,
    Open,
    Save,
    SaveAs,
}

#[tauri::command]
pub async fn project_action(
    window: tauri::WebviewWindow,
    action: FileAction,
) -> Result<DocumentSnapshot, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let label = crate::modal_windows::owner(&window)?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _session = match platform::SessionGuard::enter(&label) {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        return;
                    }
                };
                let _ = tx.send(platform::file_action(action));
            })
            .map_err(|e| e.to_string())?;
        crate::diagnostic_jobs::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())??
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = action;
        Err("Native project editing is not supported on this platform yet".into())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDropResult {
    workspace: DocumentWorkspaceSnapshot,
    errors: Vec<String>,
}

#[tauri::command]
pub async fn drop_files(
    window: tauri::WebviewWindow,
    paths: Vec<String>,
    target_id: Option<u64>,
) -> Result<FileDropResult, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let context = on_main(window.clone(), move || {
            platform::file_drop::target_context(target_id)
        })
        .await?;
        let mut errors = Vec::new();
        // Keep order; one failed file must not discard other successful imports.
        for path in paths {
            let display = std::path::Path::new(&path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let result = crate::diagnostic_jobs::spawn_blocking(move || {
                platform::file_drop::prepare(path.into(), target_id.is_some())
                    .and_then(|prepared| platform::file_drop::fit_layer(prepared, context))
            })
            .await
            .map_err(|e| e.to_string())?;
            let result = match result {
                Ok(prepared) => {
                    on_main(window.clone(), move || {
                        platform::file_drop::apply(prepared, target_id)
                    })
                    .await
                }
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                errors.push(format!("{display}: {error}"));
            }
        }
        let workspace = on_main(window, || Ok(platform::workspace_snapshot())).await?;
        Ok(FileDropResult { workspace, errors })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, paths, target_id);
        Err("Native file drop is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn finish_raster_import(
    window: tauri::WebviewWindow,
    commit: bool,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::finish_raster_import(commit)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, commit);
        Err("Image placement unavailable".into())
    }
}
#[tauri::command]
pub async fn import_raster_layer(
    window: tauri::WebviewWindow,
    format: String,
) -> Result<DocumentSnapshot, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let chosen_format = format.clone();
        let path = on_main(window.clone(), move || {
            platform::pick_raster_file(chosen_format)
        })
        .await?;
        let Some(path) = path else {
            return on_main(window, platform::raster_import_snapshot).await;
        };
        let (name, bytes, info) = crate::diagnostic_jobs::spawn_blocking(move || -> Result<_,String> {
            // Embedded raster data shares the current document storage limit.
            if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 3 * 1024 * 1024 - 1024 {
                return Err("Image exceeds the current 3 MiB import limit / 現在の読み込み上限は約3MiBです / 当前导入上限约为3MiB".into());
            }
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            let info = lumapaint_renderer::vector::imported_raster_info(&bytes, &format)?;
            let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            Ok((name, bytes, info))
        }).await.map_err(|e| e.to_string())??;
        on_main(window, move || {
            platform::apply_raster_import(name, bytes, info)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, format);
        Err("Image import is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn place_image(
    window: tauri::WebviewWindow,
    id: Option<String>,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        let (target, document_id, revision) =
            on_main(window.clone(), move || platform::frame_place_context(id)).await?;
        let path = on_main(window.clone(), move || {
            platform::pick_raster_file("all".into())
        })
        .await?;
        let Some(path) = path else {
            return on_main(window, platform::raster_import_snapshot).await;
        };
        let image =
            crate::diagnostic_jobs::spawn_blocking(move || crate::image_frames::load(&path))
                .await
                .map_err(|e| e.to_string())??;
        on_main(window, move || {
            platform::place_frame_image(target, image, document_id, revision)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Image frames are pending on this platform".into())
    }
}
#[tauri::command]
pub async fn image_links(
    window: tauri::WebviewWindow,
) -> Result<Vec<crate::image_frames::ImageLink>, String> {
    #[cfg(target_os = "macos")]
    {
        let images = on_main(window, platform::frame_links).await?;
        crate::diagnostic_jobs::spawn_blocking(move || {
            images
                .into_iter()
                .map(|(id, image)| crate::image_frames::status(id, image))
                .collect()
        })
        .await
        .map_err(|e| e.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(Vec::new())
    }
}
#[tauri::command]
pub async fn image_frame_action(
    window: tauri::WebviewWindow,
    id: String,
    action: String,
    values: Option<Vec<f32>>,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        if action == "update" || action == "relink" {
            let target = id.clone();
            let (mut image, document_id, revision) = on_main(window.clone(), move || {
                platform::frame_image_context(&target)
            })
            .await?;
            let path = if action == "relink" {
                on_main(window.clone(), move || {
                    platform::pick_raster_file("all".into())
                })
                .await?
            } else {
                image.source_path.take().map(std::path::PathBuf::from)
            };
            let Some(path) = path else {
                return on_main(window, platform::raster_import_snapshot).await;
            };
            let image =
                crate::diagnostic_jobs::spawn_blocking(move || crate::image_frames::load(&path))
                    .await
                    .map_err(|e| e.to_string())??;
            on_main(window, move || {
                platform::update_frame_image(id, image, document_id, revision)
            })
            .await
        } else {
            on_main(window, move || {
                platform::edit_image_frame(&id, &action, values.unwrap_or_default())
            })
            .await
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, action, values);
        Err("Image frames are pending on this platform".into())
    }
}

#[tauri::command]
pub async fn manage_image_links(
    window: tauri::WebviewWindow,
    ids: Vec<String>,
    action: String,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        if ids.is_empty()
            || ids.len() > 10000
            || !["update", "relink", "embed"].contains(&action.as_str())
        {
            return Err("Invalid link command".into());
        }
        let targets = ids.clone();
        let contexts = on_main(window.clone(), move || {
            targets
                .iter()
                .map(|id| platform::frame_image_context(id))
                .collect::<Result<Vec<_>, String>>()
        })
        .await?;
        let document_id = contexts[0].1;
        let revision = contexts[0].2;
        let replacement = if action == "relink" {
            let path = on_main(window.clone(), move || {
                platform::pick_raster_file("all".into())
            })
            .await?;
            let Some(path) = path else {
                return on_main(window, platform::raster_import_snapshot).await;
            };
            Some(path)
        } else {
            None
        };
        let updates = crate::diagnostic_jobs::spawn_blocking(move || {
            let mut cache = std::collections::HashMap::new();
            let mut updates = Vec::new();
            for (id, (image, _, _)) in ids.into_iter().zip(contexts) {
                let image = if action == "embed" {
                    None
                } else {
                    let path = replacement
                        .clone()
                        .or_else(|| image.source_path.map(std::path::PathBuf::from))
                        .ok_or("Embedded image has no source")?;
                    let loaded = if let Some(image) = cache.get(&path) {
                        image
                    } else {
                        cache.insert(path.clone(), crate::image_frames::load(&path)?);
                        cache.get(&path).unwrap()
                    };
                    Some(loaded.clone())
                };
                updates.push((id, image));
            }
            Ok::<_, String>(updates)
        })
        .await
        .map_err(|e| e.to_string())??;
        on_main(window, move || {
            platform::manage_frame_images(updates, document_id, revision)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, ids, action);
        Err("Image links are pending on this platform".into())
    }
}

#[tauri::command]
pub async fn import_svg_layer(window: tauri::WebviewWindow) -> Result<DocumentSnapshot, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::import_svg_layer).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("SVG import is not supported on this platform yet".into())
    }
}

pub fn confirm_discard() -> bool {
    #[cfg(target_os = "macos")]
    {
        platform::window_sessions::confirm_all()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn shutdown() {
    #[cfg(target_os = "macos")]
    {
        platform::window_sessions::shutdown_all();
        platform::cleanup_memory();
    }
}

#[cfg(target_os = "macos")]
pub(crate) async fn on_main<T: Send + 'static>(
    window: tauri::WebviewWindow,
    action: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if crate::modal_windows::owner(&window).is_err() {
        return Err("Unknown editor window".into());
    }
    let label = crate::modal_windows::owner(&window)?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            let _session = match platform::SessionGuard::enter(&label) {
                Ok(session) => session,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    return;
                }
            };
            let _ = tx.send(action());
        })
        .map_err(|e| e.to_string())?;
    crate::diagnostic_jobs::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())??
}
#[tauri::command]
pub async fn recovery_info(window: tauri::WebviewWindow) -> Result<Option<RecoveryInfo>, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::recovery_info).await.map(Some)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(None)
    }
}
#[cfg(target_os = "macos")]
type RecoveryInfo = crate::recovery::Info;
#[cfg(not(target_os = "macos"))]
type RecoveryInfo = ();

#[tauri::command]
pub async fn restore_recovery(
    window: tauri::WebviewWindow,
    id: String,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::restore_recovery(id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Recovery is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn delete_recovery(
    window: tauri::WebviewWindow,
    id: String,
) -> Result<RecoveryInfo, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::delete_recovery(id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Recovery is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn delete_all_recoveries(window: tauri::WebviewWindow) -> Result<RecoveryInfo, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::delete_all_recoveries).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Recovery is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn retry_recovery(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::retry_recovery).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(())
    }
}

#[tauri::command]
pub async fn text_fonts(window: tauri::WebviewWindow) -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::text_fonts).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(vec![])
    }
}
#[tauri::command]
pub async fn begin_text_edit(
    window: tauri::WebviewWindow,
    settings: TextSettings,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::begin_text_edit(settings)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Inline editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn set_text_edit_color(
    window: tauri::WebviewWindow,
    id: Option<String>,
    color: [u8; 3],
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_text_edit_color(id, color)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id, color);
        Err("Inline editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn update_text_edit(
    window: tauri::WebviewWindow,
    settings: TextSettings,
    patch: Option<lumapaint_core::vector::TextStylePatch>,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::update_text_edit(settings, patch)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings, patch);
        Err("Inline editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn finish_text_edit(
    window: tauri::WebviewWindow,
    commit: bool,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::finish_text_edit(commit)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, commit);
        Err("Inline editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn clipping_path(
    window: tauri::WebviewWindow,
    action: String,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::clipping_path(&action)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, action);
        Err("Clipping paths are pending on this platform / このプラットフォームでは保留中です / 此平台尚待实现".into())
    }
}

#[tauri::command]
pub async fn compound_path(
    window: tauri::WebviewWindow,
    release: bool,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::compound_path(release)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, release);
        Err("Compound paths are pending on this platform / このプラットフォームでは保留中です / 此平台尚待实现".into())
    }
}

#[tauri::command]
pub async fn edit_transform_panel(
    window: tauri::WebviewWindow,
    edit: lumapaint_core::document::TransformPanelEdit,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::edit_transform_panel(edit)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, edit);
        Err("Transforms are pending on this platform / この環境の変形は未対応です / 此平台暂不支持变换".into())
    }
}

#[tauri::command]
pub async fn transform_objects(
    window: tauri::WebviewWindow,
    action: String,
    values: [f32; 4],
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::transform_objects(&action, values)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, action, values);
        Err("Transforms are pending on this platform".into())
    }
}

#[tauri::command]
pub async fn set_vector_appearance(
    window: tauri::WebviewWindow,
    ids: Vec<String>,
    opacity: Option<f32>,
    blend_mode: Option<String>,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::set_vector_appearance(&ids, opacity, blend_mode)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, ids, opacity, blend_mode);
        Err("Vector appearance is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn set_vector_paint(
    window: tauri::WebviewWindow,
    ids: Vec<String>,
    target: String,
    color: Option<[u8; 3]>,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::set_vector_paint(&ids, &target, color)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, ids, target, color);
        Err("Vector paint is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn outline_text(window: tauri::WebviewWindow) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::outline_text).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Text outlines are pending on this platform".into())
    }
}

#[tauri::command]
pub async fn outline_view(
    window: tauri::WebviewWindow,
    value: Option<bool>,
) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::outline_view(value)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, value);
        Err("Outline view is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn text_writing_mode(
    window: tauri::WebviewWindow,
    mode: lumapaint_core::vector::WritingMode,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::text_writing_mode(mode)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, mode);
        Err("Writing direction is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn saved_path_action(
    window: tauri::WebviewWindow,
    action: String,
    id: Option<String>,
    name: String,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::saved_path_action(action, id, name)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, action, id, name);
        Err("Saved paths are pending on this platform".into())
    }
}

#[cfg(target_os = "macos")]
pub enum ThumbnailDocument {
    Standard(Box<lumapaint_core::document::Document>),
    Tiled(Box<lumapaint_core::tiles::TiledRasterDocument>),
}
#[derive(Serialize)]
pub struct ThumbnailSet {
    layers: Vec<(String, String)>,
    channels: Vec<String>,
}
#[tauri::command]
pub async fn panel_thumbnails(window: tauri::WebviewWindow) -> Result<ThumbnailSet, String> {
    #[cfg(target_os = "macos")]
    {
        let document = on_main(window, platform::thumbnail_document).await?;
        crate::diagnostic_jobs::spawn_blocking(move || {
            use base64::Engine;
            let previews = match document {
                ThumbnailDocument::Standard(document) => {
                    lumapaint_renderer::thumbnails::render(&document)?
                }
                ThumbnailDocument::Tiled(document) => {
                    lumapaint_renderer::thumbnails::render_tiled(&document)?
                }
            };
            let encode = |png: Vec<u8>| {
                format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(png)
                )
            };
            Ok(ThumbnailSet {
                layers: previews
                    .layers
                    .into_iter()
                    .map(|(id, png)| (id, encode(png)))
                    .collect(),
                channels: previews.channels.into_iter().map(encode).collect(),
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Panel thumbnails are pending on this platform".into())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectControlInfo {
    pub id: String,
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub anchor: bool,
    pub radius: Option<f32>,
}
#[derive(Serialize)]
pub struct DirectEditResult {
    pub snapshot: DocumentSnapshot,
    pub preview: String,
}
#[tauri::command]
pub async fn direct_control_info(
    window: tauri::WebviewWindow,
) -> Result<Vec<DirectControlInfo>, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::direct_control_info).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(vec![])
    }
}
#[tauri::command]
pub async fn edit_direct_controls(
    window: tauri::WebviewWindow,
    mode: String,
    values: Vec<f32>,
    preview: bool,
    expected: Vec<(String, usize)>,
    revision: u64,
) -> Result<DirectEditResult, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::edit_direct_controls(mode, values, preview, expected, revision)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, mode, values, preview, expected, revision);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

pub fn discard_recoveries() {
    #[cfg(target_os = "macos")]
    platform::window_sessions::discard_all();
}

pub(crate) async fn prepare_modal(
    window: tauri::WebviewWindow,
) -> Result<DocumentWorkspaceSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::prepare_modal).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        document_workspace(window).await
    }
}

#[tauri::command]
pub async fn apply_gradient(
    window: tauri::WebviewWindow,
    ids: Vec<String>,
    target: String,
    gradient: lumapaint_core::gradient::Gradient,
) -> Result<DocumentSnapshot, String> {
    gradient.validate()?;
    #[cfg(target_os = "macos")]
    {
        if target == "gradientLayer" {
            on_main(window, move || {
                platform::create_gradient_fill_layer(gradient)
            })
            .await
        } else if target == "pixels" {
            let job = on_main(window.clone(), move || {
                platform::prepare_pixel_gradient(&ids)
            })
            .await?;
            let job = crate::diagnostic_jobs::spawn_blocking(move || {
                platform::render_pixel_gradient(job, gradient)
            })
            .await
            .map_err(|e| e.to_string())??;
            on_main(window, move || platform::commit_pixel_gradient(job)).await
        } else {
            on_main(window, move || {
                platform::apply_gradient(&ids, &target, gradient)
            })
            .await
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, ids, target, gradient);
        Err("Native canvas is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn set_gradient_tool(
    window: tauri::WebviewWindow,
    target: String,
    gradient: lumapaint_core::gradient::Gradient,
) -> Result<(), String> {
    gradient.validate()?;
    if !["fill", "stroke"].contains(&target.as_str()) {
        return Err("Invalid gradient target".into());
    }
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::configure_gradient_tool(gradient, &target)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, target, gradient);
        Err("Native gradient tool is pending on this platform".into())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOptionsSnapshot {
    pub crop_bounds: Option<[f32; 4]>,
    gradient: lumapaint_core::gradient::Gradient,
    gradient_target: String,
    brush: Brush,
    zoom: f32,
}
#[tauri::command]
pub async fn tool_options(window: tauri::WebviewWindow) -> Result<ToolOptionsSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::tool_options).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("Native tool settings are pending on this platform".into())
    }
}
#[tauri::command]
pub async fn set_tool_brush(window: tauri::WebviewWindow, brush: Brush) -> Result<(), String> {
    brush.validate()?;
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_tool_brush(brush)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, brush);
        Err("Native tool settings are pending on this platform".into())
    }
}
#[tauri::command]
pub async fn set_tool_zoom(window: tauri::WebviewWindow, zoom: f32) -> Result<(), String> {
    if !zoom.is_finite() || !(0.0313..=640.).contains(&zoom) {
        return Err("Invalid zoom".into());
    }
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_tool_zoom(zoom)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, zoom);
        Err("Native tool settings are pending on this platform".into())
    }
}
#[tauri::command]
pub async fn numeric_tool(
    window: tauri::WebviewWindow,
    tool: CanvasTool,
    bounds: [f32; 4],
) -> Result<DocumentSnapshot, String> {
    if !matches!(
        tool,
        CanvasTool::Crop
            | CanvasTool::Rectangle
            | CanvasTool::Ellipse
            | CanvasTool::VectorRectangle
            | CanvasTool::VectorEllipse
            | CanvasTool::ImageFrameRectangle
            | CanvasTool::ImageFrameEllipse
    ) || bounds.iter().any(|v| !v.is_finite() || v.abs() > 65536.)
        || bounds[2] <= 0.
        || bounds[3] <= 0.
    {
        return Err("Invalid tool dimensions / ツールの寸法が不正です / 工具尺寸无效".into());
    }
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::numeric_tool(tool, bounds)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, tool, bounds);
        Err("Native tool operations are pending on this platform".into())
    }
}

#[tauri::command]
pub async fn sample_tool_point(window: tauri::WebviewWindow, x: f32, y: f32) -> Result<(), String> {
    if !x.is_finite() || !y.is_finite() || x.abs() > 65536. || y.abs() > 65536. {
        return Err("Invalid sampling coordinates".into());
    }
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::sample_tool_point(x, y)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, x, y);
        Err("Native sampling is pending on this platform".into())
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ToolPreviewRequest {
    Numeric { tool: CanvasTool, bounds: [f32; 4] },
    Text { settings: Box<TextSettings> },
    Transform { action: String, values: [f32; 4] },
    Brush { tool: CanvasTool, brush: Brush },
    Zoom { zoom: f32 },
    Sample { x: f32, y: f32 },
}
impl ToolPreviewRequest {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Numeric { tool, bounds } => {
                if !matches!(
                    tool,
                    CanvasTool::Crop
                        | CanvasTool::Rectangle
                        | CanvasTool::Ellipse
                        | CanvasTool::VectorRectangle
                        | CanvasTool::VectorEllipse
                        | CanvasTool::ImageFrameRectangle
                        | CanvasTool::ImageFrameEllipse
                ) || bounds.iter().any(|v| !v.is_finite() || v.abs() > 65536.)
                    || bounds[2] <= 0.
                    || bounds[3] <= 0.
                {
                    return Err("Invalid preview bounds".into());
                }
            }
            Self::Brush { tool, brush } => {
                if !matches!(
                    tool,
                    CanvasTool::Brush
                        | CanvasTool::Eraser
                        | CanvasTool::VectorPen
                        | CanvasTool::VectorPencil
                ) {
                    return Err("Invalid preview tool".into());
                }
                brush.validate()?;
            }
            Self::Zoom { zoom } => {
                if !zoom.is_finite() || !(0.0313..=640.).contains(zoom) {
                    return Err("Invalid preview zoom".into());
                }
            }
            Self::Transform { action, values } => {
                if ![
                    "move",
                    "rotate",
                    "scale",
                    "reflect",
                    "shear",
                    "individual",
                    "reset",
                ]
                .contains(&action.as_str())
                    || values.iter().any(|v| !v.is_finite())
                {
                    return Err("Invalid preview transform".into());
                }
            }
            Self::Sample { x, y } => {
                if !x.is_finite() || !y.is_finite() || x.abs() > 65536. || y.abs() > 65536. {
                    return Err("Invalid preview sample".into());
                }
            }
            Self::Text { settings } => {
                settings.text.validate()?;
                if settings
                    .position
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() >= 100_000.)
                {
                    return Err("Invalid text position".into());
                }
            }
        }
        Ok(())
    }
}
#[tauri::command]
pub async fn tool_preview(
    window: tauri::WebviewWindow,
    request: Option<ToolPreviewRequest>,
) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    if let Some(request) = &request {
        request.validate()?;
    }
    #[cfg(target_os = "macos")]
    {
        let caller = window.clone();
        on_main(window, move || {
            crate::modal_windows::owner(&caller)?;
            platform::tool_preview(request)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        if request.is_none() {
            Ok(())
        } else {
            Err("Native preview is pending on this platform".into())
        }
    }
}
#[cfg(target_os = "macos")]
pub(crate) fn clear_window_preview(label: &str) {
    platform::clear_window_preview(label);
}

#[tauri::command]
pub async fn edit_pages(
    window: tauri::WebviewWindow,
    edit: lumapaint_core::document::PageEdit,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::edit_pages(edit)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, edit);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn page_thumbnails(
    window: tauri::WebviewWindow,
    indices: Vec<usize>,
) -> Result<Vec<(String, String)>, String> {
    if indices.len() > 64 {
        return Err("At most 64 page thumbnails per request".into());
    }
    #[cfg(target_os = "macos")]
    {
        let documents =
            on_main(window, move || platform::page_thumbnail_documents(indices)).await?;
        crate::diagnostic_jobs::spawn_blocking(move || {
            use base64::Engine;
            documents
                .into_iter()
                .map(|(id, doc)| {
                    let png = lumapaint_renderer::thumbnails::page_preview(&doc, 128)?;
                    Ok((
                        id,
                        format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(png)
                        ),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, indices);
        Err("Page thumbnails are unavailable on this platform".into())
    }
}

#[tauri::command]
pub async fn edit_guides(
    window: tauri::WebviewWindow,
    edit: lumapaint_core::document::GuideEdit,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::edit_guides(edit)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, edit);
        Err("Native document editing is not supported on this platform yet".into())
    }
}
#[tauri::command]
pub async fn ruler_guide(
    window: tauri::WebviewWindow,
    axis: String,
    position: f32,
    phase: u8,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::ruler_guide(axis, position, phase)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, axis, position, phase);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn ruler_origin(
    window: tauri::WebviewWindow,
    point: [f32; 2],
    phase: u8,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::ruler_origin(point, phase)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, point, phase);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn edit_layer_groups(
    window: tauri::WebviewWindow,
    edit: lumapaint_core::document::LayerGroupEdit,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::edit_layer_groups(edit)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, edit);
        Err("Native document editing is not supported on this platform yet".into())
    }
}

#[cfg(target_os = "macos")]
pub(crate) async fn apply_pdf_import(
    window: tauri::WebviewWindow,
    token: u64,
    decoded: lumapaint_formats::io::ReadDocument,
    target: Option<u64>,
    name: String,
) -> Result<bool, String> {
    let owner = crate::modal_windows::owner(&window)?;
    let app = tauri::Manager::app_handle(&window).clone();
    on_main(window, move || {
        if !crate::pdf_import::is_pending(&app, &owner, token) {
            return Err("PDF import was cancelled".into());
        }
        let applied = platform::apply_pdf_import(decoded, target, name)?;
        if applied {
            crate::pdf_import::discard(&app, &owner);
        }
        Ok(applied)
    })
    .await
}

#[cfg(target_os = "macos")]
pub(crate) fn open_psd(owner: &str, prepared: crate::psd_import::Prepared) -> Result<(), String> {
    let _session = platform::SessionGuard::enter(owner)?;
    platform::open_psd(prepared)
}

#[tauri::command]
pub async fn crop_action(window: tauri::WebviewWindow, confirm: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::crop_action(confirm)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, confirm);
        Err("Native canvas unavailable".into())
    }
}

#[tauri::command]
pub async fn clone_stamp_settings(
    window: tauri::WebviewWindow,
) -> Result<lumapaint_core::clone_stamp::Settings, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::clone_stamp_settings).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(Default::default())
    }
}
#[tauri::command]
pub async fn set_clone_stamp_settings(
    window: tauri::WebviewWindow,
    settings: lumapaint_core::clone_stamp::Settings,
) -> Result<(), String> {
    settings.validate()?;
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_clone_stamp_settings(settings)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Native painting is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn retouch_settings(
    window: tauri::WebviewWindow,
) -> Result<lumapaint_core::retouch::Settings, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::retouch_settings).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(Default::default())
    }
}
#[tauri::command]
pub async fn set_retouch_settings(
    window: tauri::WebviewWindow,
    settings: lumapaint_core::retouch::Settings,
) -> Result<(), String> {
    settings.validate()?;
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::set_retouch_settings(settings)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Native painting is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn paint_bucket_settings(
    window: tauri::WebviewWindow,
) -> Result<lumapaint_core::paint_bucket::Settings, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::paint_bucket_settings).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(Default::default())
    }
}
#[tauri::command]
pub async fn set_paint_bucket_settings(
    window: tauri::WebviewWindow,
    settings: lumapaint_core::paint_bucket::Settings,
) -> Result<(), String> {
    settings.validate()?;
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::set_paint_bucket_settings(settings)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Native painting is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn selection_tool_settings(
    window: tauri::WebviewWindow,
) -> Result<lumapaint_core::selection_tools::Settings, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::selection_tool_settings).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(Default::default())
    }
}
#[tauri::command]
pub async fn set_selection_tool_settings(
    window: tauri::WebviewWindow,
    settings: lumapaint_core::selection_tools::Settings,
) -> Result<(), String> {
    settings.validate()?;
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || {
            platform::set_selection_tool_settings(settings)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, settings);
        Err("Native selection is pending on this platform".into())
    }
}
#[tauri::command]
pub async fn selection_path_action(
    window: tauri::WebviewWindow,
    confirm: bool,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::selection_path_action(confirm)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, confirm);
        Err("Native selection is pending on this platform".into())
    }
}

#[tauri::command]
pub async fn vector_selection_action(
    window: tauri::WebviewWindow,
    request: lumapaint_core::document::VectorSelectionRequest,
) -> Result<DocumentSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, move || platform::vector_selection_action(request)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, request);
        Err("Native vector editing is pending on this platform".into())
    }
}
