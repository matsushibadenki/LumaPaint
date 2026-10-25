//! Versioned, bounded workspace metadata. Never contains document pixels/history.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};
use tauri::Manager;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SavedDocument {
    pub id: u64,
    pub path: Option<PathBuf>,
    pub view: Option<serde_json::Value>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Session {
    pub version: u32,
    pub layout: serde_json::Value,
    pub documents: Vec<SavedDocument>,
    pub active_id: Option<u64>,
}
impl Session {
    fn validate(&self) -> Result<(), String> {
        let bad = || "Invalid workspace layout / 配置設定が不正です / 窗格配置无效".to_string();
        if self.version != 1 || self.documents.len() > 256 {
            return Err(bad());
        }
        let l = &self.layout;
        let rows = l["rows"]
            .as_u64()
            .filter(|v| (1..=3).contains(v))
            .ok_or_else(bad)? as usize;
        let cols = l["columns"]
            .as_u64()
            .filter(|v| (1..=3).contains(v))
            .ok_or_else(bad)? as usize;
        let panes = l["panes"]
            .as_array()
            .filter(|p| p.len() == rows * cols)
            .ok_or_else(bad)?;
        if l["focused"]
            .as_u64()
            .is_none_or(|v| v >= panes.len() as u64)
        {
            return Err(bad());
        }
        for (key, count) in [("rowSizes", rows), ("columnSizes", cols)] {
            let values = l[key]
                .as_array()
                .filter(|v| v.len() == count)
                .ok_or_else(bad)?;
            let sum = values.iter().try_fold(0., |sum, v| {
                v.as_f64()
                    .filter(|v| v.is_finite() && *v > 0. && *v <= 1.)
                    .map(|v| sum + v)
                    .ok_or_else(bad)
            })?;
            if (sum - 1.).abs() > 0.001 {
                return Err(bad());
            }
        }
        let ids = self.documents.iter().map(|d| d.id).collect::<HashSet<_>>();
        if ids.len() != self.documents.len() {
            return Err(bad());
        }
        let mut seen = HashSet::new();
        for pane in panes {
            let tabs = pane["tabs"].as_array().ok_or_else(bad)?;
            for id in tabs {
                let id = id.as_u64().filter(|id| ids.contains(id)).ok_or_else(bad)?;
                if !seen.insert(id) {
                    return Err(bad());
                }
            }
            if !pane["selected"].is_null() && !tabs.iter().any(|v| v == &pane["selected"]) {
                return Err(bad());
            }
        }
        if self.active_id.is_some_and(|id| !ids.contains(&id)) {
            return Err(bad());
        }
        for d in &self.documents {
            if d.path
                .as_ref()
                .is_some_and(|p| !p.is_absolute() || p.as_os_str().len() > 4096)
            {
                return Err(bad());
            }
            if let Some(v) = &d.view {
                for key in ["zoom", "panX", "panY", "width", "height"] {
                    if v[key].as_f64().is_none_or(|n| !n.is_finite()) {
                        return Err(bad());
                    }
                }
                let z = v["zoom"].as_f64().unwrap();
                if !(0.0001..=640.).contains(&z)
                    || ["panX", "panY"]
                        .iter()
                        .any(|k| v[*k].as_f64().unwrap().abs() > 8_388_608.)
                    || ["width", "height"]
                        .iter()
                        .any(|k| !(1. ..=8192.).contains(&v[*k].as_f64().unwrap()))
                {
                    return Err(bad());
                }
            }
        }
        Ok(())
    }
}
fn path(window: &tauri::WebviewWindow) -> Result<PathBuf, String> {
    if !crate::editor_windows::is_editor(window.label()) {
        return Err("Unknown editor window".into());
    }
    Ok(window
        .app_handle()
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("workspaces")
        .join(format!("{}-v1.json", window.label())))
}
fn pending() -> &'static Mutex<HashMap<PathBuf, (u64, Session)>> {
    static P: OnceLock<Mutex<HashMap<PathBuf, (u64, Session)>>> = OnceLock::new();
    P.get_or_init(Default::default)
}
static GENERATION: AtomicU64 = AtomicU64::new(1);
static WRITE: Mutex<()> = Mutex::new(());
fn queue_pending(target: PathBuf, generation: u64, state: Session) -> Result<bool, String> {
    let mut queued = pending().lock().map_err(|e| e.to_string())?;
    if queued
        .get(&target)
        .is_some_and(|(newer, _)| *newer > generation)
    {
        return Ok(false);
    }
    queued.insert(target, (generation, state));
    Ok(true)
}

fn write_pending(target: &std::path::Path, gen: u64) -> Result<(), String> {
    let _write = WRITE.lock().map_err(|e| e.to_string())?;
    let data = {
        let p = pending().lock().map_err(|e| e.to_string())?;
        let Some((g, s)) = p.get(target) else {
            return Ok(());
        };
        if *g != gen {
            return Ok(());
        }
        serde_json::to_vec(s).map_err(|e| e.to_string())?
    };
    if data.len() > 131072 {
        return Err("Workspace metadata exceeds limit".into());
    }
    let parent = target.parent().ok_or("Invalid workspace path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&data).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(target).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(target_os = "macos")]
pub(crate) fn flush_current(label: &str) {
    let latest = pending().lock().ok().and_then(|p| {
        p.iter()
            .find(|(path, _)| {
                path.file_name()
                    .is_some_and(|n| n == format!("{label}-v1.json").as_str())
            })
            .map(|(path, (_, s))| (path.clone(), s.layout.clone()))
    });
    if let Some((target, layout)) = latest {
        let result = (|| {
            let state = crate::canvas::platform::capture_workspace_session(layout)?;
            state.validate()?;
            let gen = GENERATION.fetch_add(1, Ordering::Relaxed);
            pending()
                .lock()
                .map_err(|e| e.to_string())?
                .insert(target.clone(), (gen, state));
            write_pending(&target, gen)
        })();
        if let Err(error) = result {
            eprintln!("Workspace save: {error}");
        }
    }
}
#[tauri::command]
pub async fn save_document_workspace_view(
    window: tauri::WebviewWindow,
    layout: serde_json::Value,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let target = path(&window)?;
        let gen = GENERATION.fetch_add(1, Ordering::Relaxed);
        let state = crate::canvas::on_main(window, move || {
            crate::canvas::platform::capture_workspace_session(layout)
        })
        .await?;

        state.validate()?;
        if !queue_pending(target.clone(), gen, state)? {
            return Ok(());
        }
        crate::diagnostic_jobs::spawn_blocking(move || write_pending(&target, gen))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, layout);
        Err("Workspace restore is unavailable on this platform".into())
    }
}
#[derive(Serialize)]
pub struct Restored {
    pub workspace: Option<crate::canvas::DocumentWorkspaceSnapshot>,
    pub layout: Option<serde_json::Value>,
    pub warnings: usize,
}
#[tauri::command]
pub async fn restore_document_workspace(window: tauri::WebviewWindow) -> Result<Restored, String> {
    let target = path(&window)?;
    #[cfg(target_os = "macos")]
    {
        let loaded = crate::diagnostic_jobs::spawn_blocking(move || -> Result<_, String> {
            if !target.exists() {
                return Ok(None);
            }
            let size = std::fs::metadata(&target).map_err(|e| e.to_string())?.len();
            if size > 131072 {
                return Err("Workspace metadata exceeds limit".into());
            }
            let state: Session =
                serde_json::from_slice(&std::fs::read(target).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            state.validate()?;
            let mut projects = vec![];
            let mut warnings = 0;
            let mut total = 0u64;
            for d in &state.documents {
                if let Some(path) = &d.path {
                    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                    total = total.saturating_add(len);
                    if total > 128 * 1024 * 1024 {
                        warnings += 1;
                        continue;
                    }
                    match crate::project_file::read_any_with_fingerprint(path) {
                        Ok(project) => projects.push((d.clone(), project)),
                        Err(_) => warnings += 1,
                    }
                }
            }
            Ok(Some((state, projects, warnings)))
        })
        .await
        .map_err(|e| e.to_string())??;
        let Some((state, projects, warnings)) = loaded else {
            return Ok(Restored {
                layout: None,
                warnings: 0,
                workspace: None,
            });
        };
        crate::canvas::on_main(window, move || {
            Ok(Restored {
                layout: crate::canvas::platform::restore_workspace_session(state, projects)?,
                warnings,
                workspace: Some(crate::canvas::platform::workspace_snapshot()),
            })
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = target;
        Ok(Restored {
            layout: None,
            warnings: 0,
            workspace: None,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_metadata_rejects_corruption_and_duplicates() {
        let mut s = Session {
            version: 1,
            layout: serde_json::json!({"rows":1,"columns":1,"focused":0,"rowSizes":[1],"columnSizes":[1],"panes":[{"tabs":[2],"selected":2}]}),
            documents: vec![SavedDocument {
                id: 2,
                path: Some(PathBuf::from("/tmp/a.lumapaint")),
                view: None,
            }],
            active_id: Some(2),
        };
        s.validate().unwrap();
        let bytes = serde_json::to_vec(&s).unwrap();
        serde_json::from_slice::<Session>(&bytes)
            .unwrap()
            .validate()
            .unwrap();
        s.layout["rowSizes"] = serde_json::json!([-1]);
        assert!(s.validate().is_err());
        s.layout["rowSizes"] = serde_json::json!([1]);
        s.layout["panes"][0]["tabs"] = serde_json::json!([2, 2]);
        assert!(s.validate().is_err());
    }
    #[test]
    fn atomic_workspace_write_ignores_superseded_generations() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("main-v1.json");
        let state = Session {
            version: 1,
            layout: serde_json::json!({"rows":1,"columns":1,"focused":0,"rowSizes":[1],"columnSizes":[1],"panes":[{"tabs":[],"selected":null}]}),
            documents: vec![],
            active_id: None,
        };
        state.validate().unwrap();
        pending()
            .lock()
            .unwrap()
            .insert(target.clone(), (200, state.clone()));
        assert!(!queue_pending(target.clone(), 199, state.clone()).unwrap());
        write_pending(&target, 199).unwrap();
        assert!(!target.exists());
        write_pending(&target, 200).unwrap();
        let restored: Session = serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.layout, state.layout);
        pending().lock().unwrap().remove(&target);
    }
}
