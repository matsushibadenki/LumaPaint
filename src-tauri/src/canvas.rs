use lumapaint_core::document::{
    Brush, ColorMode, ColorProfile, DocumentSettings, DocumentSnapshot, LayerSettings, TextSettings,
};
use lumapaint_core::vector::{PathOperation, VectorObject};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
#[path = "canvas_macos.rs"]
mod platform;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanvasRequest {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub zoom: f64,
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
    Rectangle,
    Ellipse,
    VectorSelect,
    VectorDirectSelect,
    VectorPen,
    VectorPencil,
    VectorAnchorAdd,
    VectorAnchorDelete,
    VectorAnchorConvert,
    VectorRectangle,
    VectorEllipse,
    Text,
    ZoomIn,
    ZoomOut,
    Hand,
}

impl CanvasRequest {
    fn validate(&self) -> Result<(), String> {
        self.brush.validate()?;
        if ![self.x, self.y, self.width, self.height, self.zoom]
            .iter()
            .all(|v| v.is_finite())
            || self.x.abs() > 32768.0
            || self.y.abs() > 32768.0
            || !(0.0..=16384.0).contains(&self.width)
            || !(0.0..=16384.0).contains(&self.height)
            || !(0.25..=4.0).contains(&self.zoom)
        {
            return Err("Invalid canvas bounds or zoom".into());
        }
        Ok(())
    }
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
) -> Result<DocumentWorkspaceSnapshot, String> {
    #[cfg(target_os = "macos")]
    {
        on_main(window, platform::new_document).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
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
    if window.label() != "main" {
        return Err("Canvas is only available in the main window".into());
    }
    request.validate()?;
    #[cfg(target_os = "macos")]
    {
        // AppKit and surface creation must run on the main thread. Waiting happens off-thread.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .with_webview(move |webview| {
                let result = platform::sync(webview.inner(), request);
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
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

pub fn destroy() {
    #[cfg(target_os = "macos")]
    platform::destroy();
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
    Undo,
    Redo,
    ToggleLayer,
    SelectAll,
    Deselect,
    InvertSelection,
    DeleteSelectedObjects,
    ClearLayer,
    Copy,
    Cut,
    Paste,
}

#[tauri::command]
pub async fn edit_document(
    window: tauri::WebviewWindow,
    action: DocumentAction,
) -> Result<DocumentSnapshot, String> {
    if window.label() != "main" {
        return Err("Document is only available in the main window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _ = tx.send(platform::edit(action));
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
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
    if window.label() != "main" {
        return Err("Document is only available in the main window".into());
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
    if window.label() != "main" {
        return Err("Document is only available in the main window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _ = tx.send(platform::set_color_mode(mode));
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
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
    if window.label() != "main" {
        return Err("Document is only available in the main window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _ = tx.send(platform::set_bit_depth(depth));
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
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
    if window.label() != "main" {
        return Err("Document is only available in the main window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _ = tx.send(platform::set_color_profile(profile));
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
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
    fn rejects_non_finite_and_unbounded_native_frames() {
        let mut request = CanvasRequest {
            x: 0.0,
            y: 0.0,
            width: 640.0,
            height: 480.0,
            zoom: 1.0,
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
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileAction {
    Open,
    Save,
    SaveAs,
}

#[tauri::command]
pub async fn project_action(
    window: tauri::WebviewWindow,
    action: FileAction,
) -> Result<DocumentSnapshot, String> {
    if window.label() != "main" {
        return Err("Project is only available in the main window".into());
    }
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let _ = tx.send(platform::file_action(action));
            })
            .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())??
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = action;
        Err("Native project editing is not supported on this platform yet".into())
    }
}

#[tauri::command]
pub async fn import_svg_layer(window: tauri::WebviewWindow) -> Result<DocumentSnapshot, String> {
    if window.label() != "main" {
        return Err("Project is only available in the main window".into());
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
        platform::confirm_discard()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn shutdown() {
    #[cfg(target_os = "macos")]
    platform::shutdown();
}

pub fn discard_recovery() {
    #[cfg(target_os = "macos")]
    platform::discard_recovery();
}

#[cfg(target_os = "macos")]
async fn on_main<T: Send + 'static>(
    window: tauri::WebviewWindow,
    action: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    if window.label() != "main" {
        return Err("Recovery is only available in the main window".into());
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            let _ = tx.send(action());
        })
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
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
