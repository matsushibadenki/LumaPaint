//! Font database and preview work stay off the AppKit/UI thread.
#[tauri::command]
pub async fn font_catalog() -> Result<Vec<lumapaint_renderer::vector::font_viewer::FontFace>, String>
{
    crate::diagnostic_jobs::spawn_blocking(|| {
        let _metrics = lumapaint_core::performance::batch("host.font_catalog_job");
        lumapaint_renderer::vector::font_viewer::catalog().to_vec()
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn font_preview(
    postscript: String,
    sample: String,
    size: f32,
    width: u32,
    height: u32,
) -> Result<tauri::ipc::Response, String> {
    crate::diagnostic_jobs::spawn_blocking(move || {
        let _metrics = lumapaint_core::performance::batch("host.font_preview_job");
        lumapaint_renderer::vector::font_viewer::preview(&postscript, &sample, size, width, height)
            .map(tauri::ipc::Response::new)
    })
    .await
    .map_err(|e| e.to_string())?
}
