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
    #[serde(default)]
    pub overlay: Option<[f64; 4]>,
    #[serde(default)]
    pub channel: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub zoom: f64,
    // Part of the shared IPC schema; native zoom synchronization is macOS-only.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    #[serde(default)]
    pub zoom_revision: Option<u64>,
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
    VectorScale,
    VectorRotate,
    VectorPen,
    VectorPencil,
    VectorAnchorAdd,
    VectorAnchorDelete,
    VectorAnchorConvert,
    VectorRectangle,
    VectorEllipse,
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
            overlay: None,
            channel: 0,
            x: 0.0,
            y: 0.0,
            width: 640.0,
            height: 480.0,
            zoom: 1.0,
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
    if window.label() != "main" {
        return Err("Import is only available in the main window".into());
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
        let (name, bytes, info) = tauri::async_runtime::spawn_blocking(move || -> Result<_,String> {
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
        tauri::async_runtime::spawn_blocking(move || {
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
