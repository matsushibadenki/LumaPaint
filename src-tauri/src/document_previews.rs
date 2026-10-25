//! Bounded, opaque preview resources. Full documents and pixels remain native.
#![cfg_attr(not(target_os = "macos"), allow(unused_imports))]
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
static PREVIEWS: Mutex<VecDeque<(u64, Arc<Vec<u8>>)>> = Mutex::new(VecDeque::new());
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(serde::Serialize)]
pub struct PanePreview {
    url: String,
    #[cfg(target_os = "macos")]
    view: Option<crate::canvas::platform::DocumentView>,
}
#[tauri::command]
pub async fn document_pane_preview(
    window: tauri::WebviewWindow,
    id: u64,
) -> Result<PanePreview, String> {
    #[cfg(target_os = "macos")]
    {
        let source = crate::canvas::on_main(window, move || {
            Ok((
                crate::canvas::platform::pane_preview_document(id)?,
                crate::canvas::platform::pane_document_view(id),
            ))
        })
        .await?;
        crate::diagnostic_jobs::spawn_blocking(move || {
            let (source, view) = source;
            let png = match source {
                crate::canvas::ThumbnailDocument::Standard(doc) => {
                    lumapaint_renderer::thumbnails::page_preview(&doc, 1024)?
                }
                crate::canvas::ThumbnailDocument::Tiled(doc) => {
                    lumapaint_renderer::thumbnails::tiled_page_preview(&doc, 1024)?
                }
            };
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let mut cache = PREVIEWS.lock().map_err(|e| e.to_string())?;
            while cache.len() >= 24
                || cache.iter().map(|(_, p)| p.len()).sum::<usize>() + png.len() > 32 * 1024 * 1024
            {
                if cache.pop_front().is_none() {
                    break;
                }
            }
            cache.push_back((id, Arc::new(png)));
            Ok(PanePreview {
                url: format!("document-preview://localhost/{id}"),
                view,
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, id);
        Err("Document pane previews are not supported on this platform yet".into())
    }
}
pub fn serve(request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let id = request
        .uri()
        .path()
        .trim_start_matches('/')
        .parse::<u64>()
        .ok();
    let png = PREVIEWS.lock().ok().and_then(|cache| {
        cache
            .iter()
            .find(|(token, _)| Some(*token) == id)
            .map(|(_, png)| png.clone())
    });
    let response = tauri::http::Response::builder()
        .status(if png.is_some() { 200 } else { 404 })
        .header("Content-Type", "image/png")
        .header("Cache-Control", "no-store")
        .body(png.map_or_else(Vec::new, |p| (*p).clone()))
        .unwrap();
    responder.respond(response);
}
