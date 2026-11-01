//! Shared, bounded color-adjustment presets. Never stores artwork, masks or tone.
use lumapaint_core::layer_effects::LayerEffects;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
    sync::Mutex,
};
use tauri::{Emitter, Manager};
const INVALID: &str = "Invalid effect preset / エフェクトプリセットが不正です / 图层效果预设无效";
const MAX_BYTES: u64 = 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    enabled: bool,
    values: [f32; 10],
    curves: [Vec<[f32; 2]>; 4],
    curve_smooth: [bool; 4],
    mixer: [[f32; 3]; 8],
    grading: [[f32; 3]; 3],
    grading_blend: f32,
    grading_balance: f32,
}
impl Settings {
    fn validate(&self) -> Result<(), String> {
        LayerEffects {
            enabled: self.enabled,
            values: self.values,
            curves: self.curves.clone(),
            curve_smooth: self.curve_smooth,
            mixer: self.mixer,
            grading: self.grading,
            grading_blend: self.grading_blend,
            grading_balance: self.grading_balance,
            ..Default::default()
        }
        .validate()
        .map_err(|_| INVALID.into())
    }
}
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
pub fn effect_preset_library(
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
        .join("effect-presets-v1.json");
    let mut state = store.0.lock().map_err(|_| INVALID)?;
    let (snapshot, changed) = transact(&mut state, &path, action, id, name, settings)?;
    if changed {
        // Publish while holding the store lock so mutation notifications remain ordered.
        let _ = app.emit("effect-presets-changed", &snapshot);
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        serde_json::from_value(serde_json::json!({"enabled":true,"values":[-0.9,12,0,0,0,0,0,0,0,0],
            "curves":[[[0,0],[1,1]],[[0,0],[1,1]],[[0,0],[1,1]],[[0,0],[1,1]]],"curveSmooth":[true,true,true,true],
            "mixer":vec![[0,0,0];8],"grading":vec![[0,0,0];3],"gradingBlend":50,"gradingBalance":-37.5})).unwrap()
    }
    #[test]
    fn effect_presets_lifecycle_limits_validation_and_stable_identity() {
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
        invalid.values[0] = 6.;
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
        next.values[1] = 25.;
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
    fn effect_presets_strict_settings_exclude_local_masks_and_reject_invalid_parameters() {
        let value = serde_json::to_value(settings()).unwrap();
        for key in ["mask", "screentone", "unknown"] {
            let mut invalid = value.clone();
            invalid[key] = serde_json::json!({});
            assert!(serde_json::from_value::<Settings>(invalid).is_err());
        }
        for bad in [f32::NAN, f32::INFINITY, 5.1] {
            let mut s = settings();
            s.values[0] = bad;
            assert!(s.validate().is_err());
        }
        let mut s = settings();
        s.curves[0] = vec![[0., 0.], [0., 1.]];
        assert!(s.validate().is_err());
        s = settings();
        s.grading[0][0] = 361.;
        assert!(s.validate().is_err());
        s = settings();
        s.grading[0][1] = -1.;
        assert!(s.validate().is_err());
        s = settings();
        s.mixer[0][2] = 101.;
        assert!(s.validate().is_err());
        s = settings();
        s.grading_blend = -1.;
        assert!(s.validate().is_err());
        assert!(!value.as_object().unwrap().contains_key("mask"));
        assert!(!value.as_object().unwrap().contains_key("screentone"));
    }
    #[test]
    fn effect_presets_file_round_trip_and_failed_write_leave_committed_state_unchanged() {
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
    fn effect_presets_corrupt_oversize_and_future_files_are_preserved() {
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
