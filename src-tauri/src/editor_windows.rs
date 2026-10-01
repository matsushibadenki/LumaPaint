//! Local editor windows. Rust owns their sessions; the WebviewWindow is a view.
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

static NEXT_WINDOW: AtomicU64 = AtomicU64::new(1);

pub(crate) fn is_editor(label: &str) -> bool {
    label == "main"
        || label.strip_prefix("editor-").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(crate) fn create(app: &AppHandle) -> Result<String, String> {
    let id = NEXT_WINDOW.fetch_add(1, Ordering::Relaxed);
    let label = format!("editor-{id}");
    // App URL and geometry are fixed here, never supplied by the WebView.
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(format!("LumaPaint — {}", id + 1))
        .inner_size(1200.0, 800.0)
        .min_inner_size(640.0, 480.0)
        .center()
        .build()
        .map_err(|error| error.to_string())?;
    Ok(label)
}

#[tauri::command]
pub async fn new_editor_window(window: WebviewWindow) -> Result<String, String> {
    if !is_editor(window.label()) {
        return Err("Unknown editor window".into());
    }
    // Window creation from a synchronous command can deadlock on Windows.
    // All native window work is dispatched explicitly to the UI thread.
    let app = window.app_handle().clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            let _ = sender.send(create(&app));
        })
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv().map_err(|error| error.to_string()))
        .await
        .map_err(|error| error.to_string())??
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_local_editor_labels_are_accepted() {
        for label in ["main", "editor-1", "editor-12345"] {
            assert!(is_editor(label));
        }
        for label in [
            "",
            "editor-",
            "editor-other",
            "settings",
            "editor-1/path",
            "editor-1\n",
        ] {
            assert!(!is_editor(label));
        }
    }
}
