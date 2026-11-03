//! App-wide editable text appearances; never stores text content, font or geometry.
use lumapaint_core::text_appearance::TextAppearance;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
    sync::Mutex,
};
use tauri::{Emitter, Manager};
const INVALID: &str = "Invalid text preset / 飾り文字プリセットが不正です / 艺术字预设无效";
const MAX_BYTES: u64 = 1024 * 1024;
type Settings = TextAppearance;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    id: u32,
    name: String,
    settings: Settings,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Library {
    version: u32,
    revision: u32,
    next_id: u32,
    presets: Vec<Preset>,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            next_id: 1,
            presets: vec![],
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    revision: u32,
    presets: Vec<Preset>,
}
impl From<&Library> for Snapshot {
    fn from(library: &Library) -> Self {
        Self {
            revision: library.revision,
            presets: library.presets.clone(),
        }
    }
}
#[derive(Default)]
pub struct Store(Mutex<Option<Library>>);
#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Get,
    Add,
    Update,
    Rename,
    Remove,
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name == name.trim()
        && name.chars().count() <= 80
        && !name.chars().any(char::is_control)
}
fn validate(library: &Library) -> Result<(), String> {
    if library.version != 1 || library.presets.len() > 64 || library.next_id == 0 {
        return Err(INVALID.into());
    }
    let mut ids = std::collections::HashSet::new();
    for preset in &library.presets {
        if preset.id == 0
            || preset.id >= library.next_id
            || !ids.insert(preset.id)
            || !valid_name(&preset.name)
        {
            return Err(INVALID.into());
        }
        preset.settings.validate()?;
    }
    Ok(())
}
fn read(path: &Path) -> Result<Library, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Library::default()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(INVALID.into());
    }
    let library = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    validate(&library)?;
    Ok(library)
}
fn write(path: &Path, library: &Library) -> Result<(), String> {
    validate(library)?;
    let bytes = serde_json::to_vec_pretty(library).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(INVALID.into());
    }
    let parent = path.parent().ok_or(INVALID)?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn change(
    library: &mut Library,
    action: Action,
    id: Option<u32>,
    name: Option<String>,
    settings: Option<Settings>,
) -> Result<(), String> {
    if action == Action::Get {
        return Ok(());
    }
    // Capture resolved paints so future built-in palette updates cannot alter saved looks.
    let settings = settings.map(|mut settings| {
        settings.custom = Some(settings.paint().clone());
        settings.enabled = true;
        settings
    });
    let mut next = library.clone();
    match action {
        Action::Add => {
            if next.presets.len() >= 64 {
                return Err(INVALID.into());
            }
            let id = next.next_id;
            next.next_id = id.checked_add(1).ok_or(INVALID)?;
            next.presets.push(Preset {
                id,
                name: name.ok_or(INVALID)?.trim().into(),
                settings: settings.ok_or(INVALID)?,
            });
        }
        Action::Update | Action::Rename | Action::Remove => {
            let index = next
                .presets
                .iter()
                .position(|preset| Some(preset.id) == id)
                .ok_or(INVALID)?;
            match action {
                Action::Update => next.presets[index].settings = settings.ok_or(INVALID)?,
                Action::Rename => next.presets[index].name = name.ok_or(INVALID)?.trim().into(),
                Action::Remove => {
                    next.presets.remove(index);
                }
                _ => unreachable!(),
            }
        }
        Action::Get => unreachable!(),
    }
    validate(&next)?;
    if next != *library {
        next.revision = library.revision.checked_add(1).ok_or(INVALID)?;
        *library = next;
    }
    Ok(())
}
fn transact(
    state: &mut Option<Library>,
    path: &Path,
    action: Action,
    id: Option<u32>,
    name: Option<String>,
    settings: Option<Settings>,
) -> Result<(Snapshot, bool), String> {
    if state.is_none() {
        *state = Some(read(path)?);
    }
    let current = state.as_ref().unwrap();
    let mut next = current.clone();
    change(&mut next, action, id, name, settings)?;
    let changed = next != *current;
    if changed {
        write(path, &next)?;
        *state = Some(next);
    }
    Ok((Snapshot::from(state.as_ref().unwrap()), changed))
}
#[tauri::command]
pub fn text_appearance_preset_library(
    app: tauri::AppHandle,
    store: tauri::State<'_, Store>,
    action: Action,
    id: Option<u32>,
    name: Option<String>,
    settings: Option<Settings>,
) -> Result<Snapshot, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("text-appearance-presets-v1.json");
    let mut state = store.0.lock().map_err(|_| INVALID)?;
    let (snapshot, changed) = transact(&mut state, &path, action, id, name, settings)?;
    if changed {
        // Publish while holding the store lock so mutation notifications remain ordered.
        let _ = app.emit("text-appearance-presets-changed", &snapshot);
    }
    Ok(snapshot)
}

/// Portable files intentionally omit device-local IDs and revisions.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortablePreset {
    name: String,
    settings: Settings,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableLibrary {
    format: String,
    version: u32,
    presets: Vec<PortablePreset>,
}
fn export_bytes(library: &Library) -> Result<Vec<u8>, String> {
    validate(library)?;
    serde_json::to_vec_pretty(&PortableLibrary {
        format: "LumaPaint.TextAppearancePresets".into(),
        version: 1,
        presets: library
            .presets
            .iter()
            .map(|p| PortablePreset {
                name: p.name.clone(),
                settings: p.settings.clone(),
            })
            .collect(),
    })
    .map_err(|e| e.to_string())
}
fn import_bytes(current: &Library, bytes: &[u8]) -> Result<Library, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(INVALID.into());
    }
    let imported: PortableLibrary = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
    if imported.version != 1
        || imported.format != "LumaPaint.TextAppearancePresets"
        || imported.presets.len() > 64
    {
        return Err(INVALID.into());
    }
    let mut next = current.clone();
    for mut preset in imported.presets {
        if !valid_name(&preset.name) {
            return Err(INVALID.into());
        }
        preset.settings.validate()?;
        preset.settings.custom = Some(preset.settings.paint().clone());
        preset.settings.enabled = true;
        if next
            .presets
            .iter()
            .any(|p| p.name == preset.name && p.settings == preset.settings)
        {
            continue;
        }
        change(
            &mut next,
            Action::Add,
            None,
            Some(preset.name),
            Some(preset.settings),
        )?;
    }
    // A whole imported file is one committed library revision.
    if next != *current {
        next.revision = current.revision.checked_add(1).ok_or(INVALID)?;
    }
    Ok(next)
}
#[tauri::command]
pub fn transfer_text_appearance_presets(
    app: tauri::AppHandle,
    store: tauri::State<'_, Store>,
    export: bool,
) -> Result<Snapshot, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("text-appearance-presets-v1.json");
    let dialog = rfd::FileDialog::new().add_filter("LumaPaint Text Presets", &["json"]);
    // Never hold the store lock during a native modal dialog.
    let destination = if export {
        dialog
            .set_file_name("LumaPaint-text-presets.json")
            .save_file()
    } else {
        dialog.pick_file()
    };
    let mut state = store.0.lock().map_err(|_| INVALID)?;
    if state.is_none() {
        *state = Some(read(&path)?);
    }
    let current = state.as_ref().unwrap();
    if let Some(destination) = destination {
        if export {
            let bytes = export_bytes(current)?;
            let mut file = tempfile::NamedTempFile::new_in(destination.parent().ok_or(INVALID)?)
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.as_file().sync_all().map_err(|e| e.to_string())?;
            file.persist(destination).map_err(|e| e.to_string())?;
        } else {
            let mut bytes = Vec::new();
            std::fs::File::open(destination)
                .map_err(|e| e.to_string())?
                .take(MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let next = import_bytes(current, &bytes)?;
            if next != *current {
                write(&path, &next)?;
                *state = Some(next);
                let _ = app.emit(
                    "text-appearance-presets-changed",
                    Snapshot::from(state.as_ref().unwrap()),
                );
            }
        }
    }
    Ok(Snapshot::from(state.as_ref().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_text_presets_merge_roundtrip_and_atomic_rejection() {
        let mut source = Library::default();
        change(
            &mut source,
            Action::Add,
            None,
            Some("夏 / Summer / 夏日".into()),
            Some(settings()),
        )
        .unwrap();
        let bytes = export_bytes(&source).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("nextId"));
        let imported = import_bytes(&Library::default(), &bytes).unwrap();
        assert_eq!(source.presets, imported.presets);
        assert_eq!(import_bytes(&imported, &bytes).unwrap(), imported);
        let mut existing = source.clone();
        existing.presets[0].name = "Existing".into();
        let merged = import_bytes(&existing, &bytes).unwrap();
        assert_eq!(merged.presets.len(), 2);
        assert_eq!(merged.presets[1].id, 2);
        assert_eq!(merged.presets[1].settings, source.presets[0].settings);
        let mut invalid: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        invalid["presets"][0]["settings"]["amount"] = serde_json::json!(99);
        assert!(import_bytes(&existing, &serde_json::to_vec(&invalid).unwrap()).is_err());
        invalid["version"] = serde_json::json!(2);
        assert!(import_bytes(&existing, &serde_json::to_vec(&invalid).unwrap()).is_err());
        assert!(import_bytes(&existing, &vec![0; MAX_BYTES as usize + 1]).is_err());
        let mut full = existing.clone();
        for _ in 1..64 {
            change(
                &mut full,
                Action::Add,
                None,
                Some("Full".into()),
                Some(settings()),
            )
            .unwrap();
        }
        assert!(import_bytes(&full, &bytes).is_err());
        assert_eq!(full.presets.len(), 64);
    }
    fn settings() -> Settings {
        Settings {
            preset: "roseGold".into(),
            enabled: false,
            amount: 1.25,
            custom: None,
        }
    }
    #[test]
    fn text_appearance_presets_lifecycle_limits_validation_and_stable_identity() {
        let mut library = Library::default();
        change(
            &mut library,
            Action::Add,
            None,
            Some(" 線画 / Ink / 线稿 ".into()),
            Some(settings()),
        )
        .unwrap();
        assert_eq!(library.presets[0].name, "線画 / Ink / 线稿");
        let original = library.clone();
        for name in ["", "a\nb", &"x".repeat(81)] {
            assert!(change(
                &mut library,
                Action::Add,
                None,
                Some(name.into()),
                Some(settings())
            )
            .is_err());
            assert_eq!(library, original);
        }
        assert!(change(
            &mut library,
            Action::Update,
            Some(999),
            None,
            Some(settings())
        )
        .is_err());
        let mut invalid = settings();
        invalid.amount = 6.;
        assert!(change(&mut library, Action::Update, Some(1), None, Some(invalid)).is_err());
        assert_eq!(library, original);
        change(
            &mut library,
            Action::Rename,
            Some(1),
            Some("線画 / Ink / 线稿".into()),
            None,
        )
        .unwrap();
        assert_eq!(library, original);
        let mut next = settings();
        next.amount = 1.5;
        change(&mut library, Action::Update, Some(1), None, Some(next)).unwrap();
        assert_eq!(library.revision, 2);
        change(&mut library, Action::Remove, Some(1), None, None).unwrap();
        change(
            &mut library,
            Action::Add,
            None,
            Some("New".into()),
            Some(settings()),
        )
        .unwrap();
        assert_eq!(library.presets[0].id, 2);
        for _ in 1..64 {
            change(
                &mut library,
                Action::Add,
                None,
                Some("Same name is valid".into()),
                Some(settings()),
            )
            .unwrap();
        }
        let full = library.clone();
        assert!(change(
            &mut library,
            Action::Add,
            None,
            Some("Overflow".into()),
            Some(settings())
        )
        .is_err());
        assert_eq!(library, full);
    }
    #[test]
    fn text_appearance_presets_capture_only_paints_and_strength() {
        let value = serde_json::to_value(settings()).unwrap();
        for key in ["content", "fontFamily", "position", "unknown"] {
            let mut invalid = value.clone();
            invalid[key] = serde_json::json!("private text");
            assert!(serde_json::from_value::<Settings>(invalid).is_err());
        }
        let mut library = Library::default();
        change(
            &mut library,
            Action::Add,
            None,
            Some("夏 Summer 夏日".into()),
            Some(settings()),
        )
        .unwrap();
        let saved = &library.presets[0].settings;
        assert!(saved.enabled);
        assert_eq!(saved.amount, 1.25);
        assert_eq!(saved.custom.as_ref(), Some(settings().paint()));
        for bad in [f32::NAN, f32::INFINITY, 2.1] {
            let original = library.clone();
            let mut invalid = settings();
            invalid.amount = bad;
            assert!(change(
                &mut library,
                Action::Add,
                None,
                Some("Bad".into()),
                Some(invalid)
            )
            .is_err());
            assert_eq!(library, original);
        }
        let mut invalid = settings();
        invalid.custom = Some(invalid.paint().clone());
        invalid.custom.as_mut().unwrap().outlines[1].color = "red".into();
        assert!(change(
            &mut library,
            Action::Add,
            None,
            Some("Bad".into()),
            Some(invalid)
        )
        .is_err());
    }
    #[test]
    fn text_appearance_presets_file_round_trip_and_failed_write_leave_committed_state_unchanged() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.json");
        let mut state = None;
        transact(&mut state, &path, Action::Get, None, None, None).unwrap();
        assert!(!path.exists());
        transact(
            &mut state,
            &path,
            Action::Add,
            None,
            Some("仕上げ / Finish / 完稿".into()),
            Some(settings()),
        )
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let old = state.clone();
        assert_eq!(read(&path).unwrap(), state.clone().unwrap());
        // Persisting over a directory fails on all hosts, without publishing the draft.
        assert!(transact(
            &mut state,
            directory.path(),
            Action::Rename,
            Some(1),
            Some("Changed".into()),
            None
        )
        .is_err());
        assert_eq!(state, old);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let mut restarted = None;
        let (snapshot, changed) =
            transact(&mut restarted, &path, Action::Get, None, None, None).unwrap();
        assert!(!changed);
        assert_eq!(snapshot.presets[0].name, "仕上げ / Finish / 完稿");
        assert_eq!(restarted, old);
        assert!(!String::from_utf8(bytes).unwrap().contains("mask"));
    }
    #[test]
    fn text_appearance_presets_corrupt_oversize_and_future_files_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.json");
        for bytes in [
            b"broken".to_vec(),
            br#"{"version":2,"revision":0,"nextId":1,"presets":[]}"#.to_vec(),
            vec![b' '; MAX_BYTES as usize + 1],
        ] {
            std::fs::write(&path, &bytes).unwrap();
            let mut state = None;
            assert!(transact(
                &mut state,
                &path,
                Action::Add,
                None,
                Some("A".into()),
                Some(settings())
            )
            .is_err());
            assert!(state.is_none());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let mut library = Library::default();
        change(
            &mut library,
            Action::Add,
            None,
            Some("A".into()),
            Some(settings()),
        )
        .unwrap();
        library.presets.push(library.presets[0].clone());
        assert!(validate(&library).is_err());
    }
}
