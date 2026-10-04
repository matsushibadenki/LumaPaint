//! Document-modal WebviewWindows. Dialogs refer to an existing Rust editor
//! session and never allocate their own document or GPU renderer.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    Settings,
    NewDocument,
    ColorSettings,
    Transform,
    DirectControls,
    ImportImage,
    PdfImport,
    ToolSettings,
}
impl Kind {
    fn geometry(self) -> (f64, f64) {
        match self {
            Self::NewDocument => (1000., 660.),
            Self::Settings => (760., 480.),
            Self::ColorSettings => (620., 380.),
            Self::DirectControls => (440., 600.),
            Self::Transform => (400., 360.),
            Self::ImportImage => (480., 360.),
            Self::PdfImport => (480., 400.),
            Self::ToolSettings => (440., 580.),
        }
    }
    fn title(self, locale: &str) -> &str {
        let titles = match self {
            Self::Settings => ["Settings", "設定", "设置"],
            Self::NewDocument => ["New Document", "新規ドキュメント", "新建文档"],
            Self::ColorSettings => ["Color Settings", "カラー設定", "颜色设置"],
            Self::Transform => ["Transform", "変形", "变换"],
            Self::DirectControls => [
                "Controls & Live Corners",
                "節点・ライブコーナー",
                "节点与实时圆角",
            ],
            Self::ImportImage => ["Import", "読み込み", "导入"],
            Self::PdfImport => ["PDF Page", "PDFページ", "PDF页面"],
            Self::ToolSettings => ["Tool Settings", "ツール設定", "工具设置"],
        };
        titles[match locale {
            "ja" => 1,
            "zh-CN" => 2,
            _ => 0,
        }]
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Request {
    request_id: String,
    kind: Kind,
    locale: String,
    theme: String,
    action: Option<String>,
}
impl Request {
    fn validate(&self) -> Result<(), String> {
        if self.request_id.is_empty()
            || self.request_id.len() > 64
            || !self
                .request_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !["en", "ja", "zh-CN"].contains(&self.locale.as_str())
            || !["system", "light", "dark"].contains(&self.theme.as_str())
            || matches!(self.kind, Kind::ToolSettings)
                && !self.action.as_deref().is_some_and(|tool| {
                    serde_json::from_value::<crate::canvas::CanvasTool>(serde_json::Value::String(
                        tool.into(),
                    ))
                    .is_ok()
                })
            || matches!(self.kind, Kind::Transform)
                && !self.action.as_deref().is_some_and(|action| {
                    [
                        "move",
                        "rotate",
                        "reflect",
                        "scale",
                        "shear",
                        "individual",
                        "reset",
                    ]
                    .contains(&action)
                })
        {
            return Err("Invalid modal window request".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Context {
    request_id: String,
    kind: Kind,
    locale: String,
    theme: String,
    action: Option<String>,
    document: Option<lumapaint_core::document::DocumentSnapshot>,
}
struct Entry {
    owner: String,
    context: Context,
    shown: bool,
    busy: bool,
}
#[derive(Default)]
pub(crate) struct Modals(Mutex<HashMap<String, Entry>>);
impl Modals {
    fn owner(&self, label: &str) -> Option<String> {
        self.0
            .lock()
            .unwrap()
            .get(label)
            .map(|entry| entry.owner.clone())
    }
}
static NEXT_MODAL: AtomicU64 = AtomicU64::new(1);

pub(crate) fn owner(window: &WebviewWindow) -> Result<String, String> {
    if crate::editor_windows::is_editor(window.label()) {
        return Ok(window.label().into());
    }
    window
        .app_handle()
        .state::<Modals>()
        .owner(window.label())
        .ok_or_else(|| "Unknown document window".into())
}
#[cfg(target_os = "macos")]
pub(crate) fn blocked(_app: &AppHandle, _owner: &str) -> bool {
    // Settings windows are modeless; the editor remains visible and interactive.
    false
}

pub(crate) fn focus(app: &AppHandle, owner: Option<&str>) -> bool {
    let label = app
        .state::<Modals>()
        .0
        .lock()
        .unwrap()
        .iter()
        .find(|(_, entry)| owner.is_none_or(|owner| entry.owner == owner))
        .map(|(label, _)| label.clone());
    if let Some(window) = label.and_then(|label| app.get_webview_window(&label)) {
        let _ = window.set_focus();
        true
    } else {
        false
    }
}

#[tauri::command]
pub(crate) async fn open_modal_window(
    window: WebviewWindow,
    request: Request,
) -> Result<String, String> {
    request.validate()?;
    if !crate::editor_windows::is_editor(window.label()) {
        return Err("A dialog requires an editor parent".into());
    }
    // Commit inline text and finish interactions before taking the dialog's view snapshot.
    let workspace = crate::canvas::prepare_modal(window.clone()).await?;
    let owner = window.label().to_string();
    let parent = window.clone();
    main(window, move || {
        let app = parent.app_handle();
        {
            let modals = app.state::<Modals>();
            let entries = modals.0.lock().unwrap();
            if let Some((label, entry)) = entries.iter().find(|(_, entry)| entry.owner == owner) {
                if entry.context.request_id == request.request_id {
                    return Ok(label.clone());
                }
                return Err("This editor already has an open dialog".into());
            }
        }
        let label = format!("modal-{}", NEXT_MODAL.fetch_add(1, Ordering::Relaxed));
        let (width, height) = request.kind.geometry();
        let title = request.kind.title(&request.locale);
        let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
            .title(title)
            .inner_size(width, height)
            .resizable(false)
            .minimizable(false)
            .maximizable(false)
            .skip_taskbar(true)
            .decorations(true)
            .visible(false)
            .center();
        #[cfg(not(target_os = "macos"))]
        let builder = builder.parent(&parent).map_err(|error| error.to_string())?;
        app.state::<Modals>().0.lock().unwrap().insert(
            label.clone(),
            Entry {
                owner,
                context: Context {
                    request_id: request.request_id,
                    kind: request.kind,
                    locale: request.locale,
                    theme: request.theme,
                    action: request.action,
                    document: if matches!(request.kind, Kind::PdfImport) {
                        None
                    } else {
                        workspace.active
                    },
                },
                shown: false,
                busy: false,
            },
        );
        if let Err(error) = builder.build() {
            app.state::<Modals>().0.lock().unwrap().remove(&label);
            return Err(error.to_string());
        }
        Ok(label)
    })
    .await
}
#[tauri::command]
pub(crate) fn modal_context(window: WebviewWindow) -> Result<Context, String> {
    window
        .app_handle()
        .state::<Modals>()
        .0
        .lock()
        .unwrap()
        .get(window.label())
        .map(|entry| entry.context.clone())
        .ok_or_else(|| "Dialog is closed".into())
}
#[tauri::command]
pub(crate) async fn modal_ready(window: WebviewWindow) -> Result<(), String> {
    let child = window.clone();
    main(window, move || {
        let app = child.app_handle();
        let owner = {
            let modals = app.state::<Modals>();
            let mut entries = modals.0.lock().unwrap();
            let entry = entries.get_mut(child.label()).ok_or("Dialog is closed")?;
            if entry.shown {
                return Ok(());
            }
            entry.owner.clone()
        };
        let _parent = app.get_webview_window(&owner).ok_or("Parent is closed")?;
        #[cfg(target_os = "macos")]
        child.show().map_err(|e| e.to_string())?;
        #[cfg(not(target_os = "macos"))]
        {
            let _ = _parent;
            child.show().map_err(|e| e.to_string())?;
        }
        if let Some(entry) = app
            .state::<Modals>()
            .0
            .lock()
            .unwrap()
            .get_mut(child.label())
        {
            entry.shown = true;
        }
        child.set_focus().map_err(|e| e.to_string())
    })
    .await
}
#[tauri::command]
pub(crate) fn modal_busy(window: WebviewWindow, busy: bool) -> Result<(), String> {
    let modals = window.app_handle().state::<Modals>();
    let mut entries = modals.0.lock().unwrap();
    entries
        .get_mut(window.label())
        .ok_or("Dialog is closed")?
        .busy = busy;
    Ok(())
}
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Change {
    CreatedDocument,
    Preferences { locale: String, theme: String },
}
#[tauri::command]
pub(crate) fn modal_change(window: WebviewWindow, change: Change) -> Result<(), String> {
    if let Change::Preferences { locale, theme } = &change {
        if !["en", "ja", "zh-CN"].contains(&locale.as_str())
            || !["system", "light", "dark"].contains(&theme.as_str())
        {
            return Err("Invalid preferences".into());
        }
    }
    let modals = window.app_handle().state::<Modals>();
    let entries = modals.0.lock().unwrap();
    let entry = entries.get(window.label()).ok_or("Dialog is closed")?;
    let allowed = matches!(
        (&entry.context.kind, &change),
        (Kind::NewDocument, Change::CreatedDocument) | (Kind::Settings, Change::Preferences { .. })
    );
    if !allowed {
        return Err("Invalid dialog action".into());
    }
    window
        .app_handle()
        .emit_to(&entry.owner, "modal-change", &change)
        .map_err(|error| error.to_string())
}
#[tauri::command]
pub(crate) async fn close_modal_window(
    window: WebviewWindow,
    request_id: Option<String>,
) -> Result<(), String> {
    let caller = window.clone();
    main(window, move || {
        let app = caller.app_handle();
        let label = {
            let modals = app.state::<Modals>();
            let entries = modals.0.lock().unwrap();
            if crate::editor_windows::is_editor(caller.label()) {
                entries
                    .iter()
                    .find(|(_, entry)| {
                        entry.owner == caller.label()
                            && Some(&entry.context.request_id) == request_id.as_ref()
                    })
                    .map(|(label, _)| label.clone())
            } else {
                entries
                    .contains_key(caller.label())
                    .then(|| caller.label().into())
            }
        };
        if let Some(label) = label {
            if let Some(child) = app.get_webview_window(&label) {
                if !can_close(app, &label) {
                    return Err("Dialog operation is in progress".into());
                }
                detach(app, &label);
                child.close().map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    })
    .await
}
pub(crate) fn can_close(app: &AppHandle, label: &str) -> bool {
    app.state::<Modals>()
        .0
        .lock()
        .unwrap()
        .get(label)
        .is_none_or(|entry| !entry.busy)
}
pub(crate) fn detach(app: &AppHandle, label: &str) {
    let entry = app.state::<Modals>().0.lock().unwrap().remove(label);
    if let Some(entry) = entry {
        if matches!(entry.context.kind, Kind::PdfImport) {
            crate::pdf_import::discard(app, &entry.owner);
        }
        if let Some(parent) = app.get_webview_window(&entry.owner) {
            #[cfg(target_os = "macos")]
            crate::canvas::clear_window_preview(&entry.owner);
            #[cfg(not(target_os = "macos"))]
            {
                let _ = parent.set_enabled(true);
            }
            let _ = app.emit_to(&entry.owner, "modal-closed", &entry.context.request_id);
            let _ = parent.set_focus();
        }
    }
}
pub(crate) fn parent_destroyed(app: &AppHandle, owner: &str) {
    crate::pdf_import::discard(app, owner);
    let labels = app
        .state::<Modals>()
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, entry)| entry.owner == owner)
        .map(|(label, _)| label.clone())
        .collect::<Vec<_>>();
    for label in labels {
        detach(app, &label);
        if let Some(child) = app.get_webview_window(&label) {
            let _ = child.destroy();
        }
    }
}
async fn main<T: Send + 'static>(
    window: WebviewWindow,
    action: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    window
        .run_on_main_thread(move || {
            let _ = tx.send(action());
        })
        .map_err(|error| error.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|error| error.to_string()))
        .await
        .map_err(|error| error.to_string())??
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modal_commands_resolve_only_their_registered_editor() {
        let modals = Modals::default();
        for (label, owner) in [("modal-1", "main"), ("modal-2", "editor-2")] {
            modals.0.lock().unwrap().insert(
                label.into(),
                Entry {
                    owner: owner.into(),
                    shown: false,
                    busy: false,
                    context: Context {
                        request_id: label.into(),
                        kind: Kind::Settings,
                        locale: "ja".into(),
                        theme: "dark".into(),
                        action: None,
                        document: None,
                    },
                },
            );
        }
        assert_eq!(modals.owner("modal-1").as_deref(), Some("main"));
        assert_eq!(modals.owner("modal-2").as_deref(), Some("editor-2"));
        assert!(modals.owner("modal-3").is_none());
        modals.0.lock().unwrap().remove("modal-1");
        assert!(modals.owner("modal-1").is_none());
        assert_eq!(modals.owner("modal-2").as_deref(), Some("editor-2"));
    }
    #[test]
    fn modal_requests_reject_unknown_locale_theme_actions_and_ids() {
        let mut request = Request {
            request_id: "request-1".into(),
            kind: Kind::Transform,
            locale: "ja".into(),
            theme: "system".into(),
            action: Some("rotate".into()),
        };
        assert!(request.validate().is_ok());
        request.action = Some("unknown".into());
        assert!(request.validate().is_err());
        request.kind = Kind::Settings;
        request.locale = "xx".into();
        assert!(request.validate().is_err());
        request.locale = "en".into();
        request.theme = "unknown".into();
        assert!(request.validate().is_err());
        request.theme = "dark".into();
        request.request_id = "../editor".into();
        assert!(request.validate().is_err());
    }
}
