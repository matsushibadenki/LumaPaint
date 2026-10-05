//! Rust-owned pending PDF bytes. Webviews receive page metadata and selection only.
use serde::Serialize;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
#[cfg(target_os = "macos")]
use tauri::Emitter;
use tauri::{Manager, WebviewWindow};
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Info {
    token: u64,
    name: String,
    pages: Vec<lumapaint_formats::pdf::PageInfo>,
    as_layer: bool,
}
#[derive(Clone)]
pub(crate) struct Pending {
    pub info: Info,
    pub bytes: Arc<[u8]>,
    pub format: lumapaint_formats::export::FormatId,
    pub target: Option<u64>,
    applying: bool,
}
#[derive(Default)]
pub(crate) struct Imports(Mutex<HashMap<String, Pending>>);
#[cfg(target_os = "macos")]
static NEXT: AtomicU64 = AtomicU64::new(1);
pub(crate) fn discard(app: &tauri::AppHandle, owner: &str) {
    app.state::<Imports>().0.lock().unwrap().remove(owner);
}
#[cfg(target_os = "macos")]
pub(crate) fn prepare(
    app: tauri::AppHandle,
    owner: String,
    path: std::path::PathBuf,
    target: Option<u64>,
) {
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> Result<Pending, String> {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .map_err(|e| e.to_string())?
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let pages = lumapaint_formats::pdf::inspect(&bytes).map_err(|e| e.to_string())?;
            let format = if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ai"))
            {
                lumapaint_formats::export::FormatId::Illustrator
            } else {
                lumapaint_formats::export::FormatId::Pdf
            };
            Ok(Pending {
                info: Info {
                    token: NEXT.fetch_add(1, Ordering::Relaxed),
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    pages,
                    as_layer: target.is_some(),
                },
                bytes: bytes.into(),
                format,
                target,
                applying: false,
            })
        })();
        let callback_app = app.clone();
        let _=app.run_on_main_thread(move|| {
            if callback_app.get_webview_window(&owner).is_none() {return;}
            let result=result.and_then(|pending| {
                let info=pending.info.clone();
                let imports=callback_app.state::<Imports>();let mut values=imports.0.lock().unwrap();
                if values.contains_key(&owner) || values.len()>=4 {return Err("PDF import is already pending / PDFの読み込みが保留中です / PDF导入正在等待".into());}
                values.insert(owner.clone(),pending);drop(values);
                if let Err(error)=callback_app.emit_to(&owner,"pdf-import-request",info.token) {discard(&callback_app,&owner);return Err(error.to_string());}
                Ok(())
            });
            if let Err(error)=result {let _=callback_app.emit_to(&owner,"canvas-error",error);}
        });
    });
}
#[tauri::command]
pub(crate) fn pdf_import_context(window: WebviewWindow) -> Result<Info, String> {
    let owner = crate::modal_windows::owner(&window)?;
    window
        .app_handle()
        .state::<Imports>()
        .0
        .lock()
        .unwrap()
        .get(&owner)
        .map(|p| p.info.clone())
        .ok_or("PDF import is no longer pending".into())
}
#[tauri::command]
pub(crate) async fn pdf_import_apply(
    window: WebviewWindow,
    token: u64,
    page_index: u32,
    dpi: u32,
    all_pages: bool,
) -> Result<bool, String> {
    let owner = crate::modal_windows::owner(&window)?;
    let app = window.app_handle().clone();
    let pending = app
        .state::<Imports>()
        .begin(&owner, token, page_index, dpi)?;
    if all_pages && pending.target.is_some() {
        app.state::<Imports>().release(&owner, token);
        return Err("All pages must be opened as a document / 全ページは文書として開いてください / 所有页面必须作为文档打开".into());
    }
    #[cfg(target_os = "macos")]
    let result = async {
        let decoded = tauri::async_runtime::spawn_blocking(move || {
            let options = lumapaint_formats::io::ReadOptions {
                page_index,
                raster_dpi: dpi,
                allow_lossy: true,
                ..Default::default()
            };
            let decoded = if all_pages {
                lumapaint_formats::pdf::read_all(&pending.bytes, options).map(|mut decoded| {
                    if pending.format == lumapaint_formats::export::FormatId::Illustrator {
                        decoded
                            .report
                            .issues
                            .push(lumapaint_formats::ConversionIssue {
                                code: "ai.pdf_compatible_only",
                                tier: lumapaint_formats::CompatibilityTier::B,
                            });
                    }
                    decoded
                })
            } else {
                lumapaint_formats::io::read_document(
                    pending.format,
                    pending.info.name.clone(),
                    &pending.bytes,
                    options,
                )
            };
            decoded.map(|d| (d, pending.target, pending.info.name))
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        crate::canvas::apply_pdf_import(window, token, decoded.0, decoded.1, decoded.2).await
    }
    .await;
    #[cfg(not(target_os = "macos"))]
    let result = {
        let _ = (pending.bytes, pending.format, pending.target);
        Err("Native document editing is not supported on this platform yet".into())
    };
    app.state::<Imports>().release(&owner, token);
    result
}
#[cfg(target_os = "macos")]
pub(crate) fn is_pending(app: &tauri::AppHandle, owner: &str, token: u64) -> bool {
    app.state::<Imports>()
        .0
        .lock()
        .unwrap()
        .get(owner)
        .is_some_and(|p| p.info.token == token)
}

#[tauri::command]
pub(crate) fn pdf_import_cancel(window: WebviewWindow, token: u64) -> Result<(), String> {
    let owner = crate::modal_windows::owner(&window)?;
    window.app_handle().state::<Imports>().cancel(&owner, token);
    Ok(())
}

impl Imports {
    fn begin(&self, owner: &str, token: u64, page_index: u32, dpi: u32) -> Result<Pending, String> {
        let mut values = self.0.lock().unwrap();
        let pending = values
            .get_mut(owner)
            .filter(|p| p.info.token == token)
            .ok_or("PDF import is no longer pending")?;
        if pending.applying {
            return Err("PDF import is already running".into());
        }
        if page_index as usize >= pending.info.pages.len() || !(36..=1200).contains(&dpi) {
            return Err("Invalid PDF page or resolution".into());
        }
        pending.applying = true;
        Ok(pending.clone())
    }
    fn release(&self, owner: &str, token: u64) {
        if let Some(pending) = self
            .0
            .lock()
            .unwrap()
            .get_mut(owner)
            .filter(|p| p.info.token == token)
        {
            pending.applying = false;
        }
    }
    fn cancel(&self, owner: &str, token: u64) {
        let mut values = self.0.lock().unwrap();
        if values.get(owner).is_some_and(|p| p.info.token == token) {
            values.remove(owner);
        }
    }
}
#[cfg(any(target_os = "macos", test))]
pub(crate) fn layer_source(
    page: &lumapaint_core::document::Document,
    destination: &lumapaint_core::document::DocumentSnapshot,
) -> Result<String, String> {
    let source = &page
        .svg_layers()
        .next()
        .ok_or("PDF page has no layer")?
        .source;
    let scale =
        destination.resolution as f32 / page.document_state().resolution.unwrap_or(72) as f32;
    Ok(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><g transform=\"scale({scale})\">{source}</g></svg>",destination.width,destination.height))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_cancel_cannot_release_another_window_or_newer_request() {
        let imports = Imports::default();
        let bytes: Arc<[u8]> = vec![1, 2, 3].into();
        let weak = Arc::downgrade(&bytes);
        imports.0.lock().unwrap().insert(
            "editor-2".into(),
            Pending {
                info: Info {
                    token: 2,
                    name: "file".into(),
                    pages: vec![],
                    as_layer: false,
                },
                bytes,
                format: lumapaint_formats::export::FormatId::Pdf,
                target: None,
                applying: false,
            },
        );
        imports.cancel("main", 2);
        imports.cancel("editor-2", 1);
        assert!(weak.upgrade().is_some());
        imports.cancel("editor-2", 2);
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn overlapping_apply_is_rejected_and_errors_can_be_retried() {
        let imports = Imports::default();
        imports.0.lock().unwrap().insert(
            "main".into(),
            Pending {
                info: Info {
                    token: 2,
                    name: "file".into(),
                    pages: vec![lumapaint_formats::pdf::PageInfo {
                        index: 0,
                        width_points: 100.,
                        height_points: 80.,
                    }],
                    as_layer: false,
                },
                bytes: vec![].into(),
                format: lumapaint_formats::export::FormatId::Pdf,
                target: None,
                applying: false,
            },
        );
        assert!(imports.begin("main", 2, 1, 144).is_err());
        assert!(imports.begin("main", 2, 0, 0).is_err());
        assert!(imports.begin("main", 2, 0, 144).is_ok());
        assert!(imports.begin("main", 2, 0, 144).is_err());
        imports.release("main", 1);
        assert!(imports.begin("main", 2, 0, 144).is_err());
        imports.release("main", 2);
        assert!(imports.begin("main", 2, 0, 72).is_ok());
    }
    #[test]
    fn pdf_layer_preserves_physical_size_and_can_be_undone() {
        let page = lumapaint_formats::svg::import(
            "page".into(),
            "<svg width=\"120\" height=\"80\"><rect width=\"120\" height=\"80\"/></svg>".into(),
        )
        .unwrap();
        let mut state = page.document_state();
        state.resolution = Some(144);
        let page = lumapaint_core::document::Document::from_document_state(state).unwrap();
        let mut destination = lumapaint_core::document::Document::default();
        let count = destination.svg_layers().count();
        let source = layer_source(&page, &destination.snapshot()).unwrap();
        let scale = destination.snapshot().resolution as f32 / 144.;
        assert!(source.contains(&format!("scale({scale})")));
        destination
            .import_raster_layer("PDF page 2".into(), source)
            .unwrap();
        assert_eq!(destination.svg_layers().count(), count + 1);
        destination.undo();
        assert_eq!(destination.svg_layers().count(), count);
    }
}
