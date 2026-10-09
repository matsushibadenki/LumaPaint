//! Shared shortcut settings and native canvas routing. Webviews supply command
//! metadata/actions, while Rust owns validated persistence and active bindings.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::Write,
    sync::Mutex,
};
use tauri::{Emitter, Manager};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
    pub id: String,
    pub label: String,
    pub category: String,
    pub default_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Set {
    pub id: String,
    pub name: String,
    pub bindings: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub revision: u64,
    pub active: String,
    pub sets: Vec<Set>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            active: "default".into(),
            sets: vec![Set {
                id: "default".into(),
                name: "LumaPaint".into(),
                bindings: BTreeMap::new(),
            }],
        }
    }
}
#[derive(Default)]
pub struct Store {
    settings: Mutex<Option<Settings>>,
    catalogs: Mutex<HashMap<String, Vec<Command>>>,
}
#[derive(Serialize)]
pub struct Snapshot {
    settings: Settings,
    commands: Vec<Command>,
}
fn key_valid(key: &str) -> bool {
    if key.is_empty() {
        return true;
    }
    let parts = key.split('+').collect::<Vec<_>>();
    let code = *parts.last().unwrap();
    let mut seen = HashSet::new();
    parts[..parts.len()-1].iter().all(|m|["Primary","Control","Alt","Shift"].contains(m)&&seen.insert(*m)) &&
    ((code.len()==4&&code.starts_with("Key")&&code.as_bytes()[3].is_ascii_uppercase()) || (code.len()==6&&code.starts_with("Digit")&&code.as_bytes()[5].is_ascii_digit()) || code.strip_prefix('F').and_then(|s|s.parse::<u8>().ok()).is_some_and(|v|(1..=20).contains(&v)) || ["BracketLeft","BracketRight","Backslash","Semicolon","Quote","Comma","Period","Slash","Minus","Equal","Backquote","IntlYen"].contains(&code)) &&
    // Preserve system application lifecycle keys.
    !["Primary+KeyQ","Primary+KeyH","Primary+Alt+KeyH","Primary+KeyM"].contains(&key)
}
fn validate(settings: &Settings) -> Result<(), String> {
    if settings.version != 1
        || settings.sets.is_empty()
        || settings.sets.len() > 32
        || !settings.sets.iter().any(|s| s.id == settings.active)
    {
        return Err("Invalid shortcut set / ショートカットセットが不正です / 快捷键集无效".into());
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for set in &settings.sets {
        if set.id.is_empty()
            || set.id.len() > 80
            || !ids.insert(&set.id)
            || set.name.trim().is_empty()
            || set.name.chars().count() > 80
            || !names.insert(set.name.trim().to_lowercase())
            || set.bindings.len() > 1024
        {
            return Err("Invalid or duplicate set name / セット名が不正または重複しています / 集名称无效或重复".into());
        }
        for (id, key) in &set.bindings {
            if id.is_empty() || id.len() > 160 || !key_valid(key) {
                return Err(
                    "Invalid or reserved shortcut / 使用できないキーです / 快捷键无效或已保留"
                        .into(),
                );
            }
        }
    }
    Ok(())
}
fn effective<'a>(settings: &'a Settings, command: &'a Command) -> &'a str {
    settings
        .sets
        .iter()
        .find(|s| s.id == settings.active)
        .and_then(|s| s.bindings.get(&command.id))
        .map_or(command.default_key.as_str(), String::as_str)
}
fn no_conflicts(settings: &Settings, commands: &[Command]) -> Result<(), String> {
    let mut keys = HashMap::new();
    for command in commands {
        let key = effective(settings, command);
        if !key.is_empty() {
            if let Some(previous) = keys.insert(key, &command.id) {
                if previous != &command.id {
                    return Err(format!(
                        "Shortcut conflict / キーが競合しています / 快捷键冲突: {previous}, {}",
                        command.id
                    ));
                }
            }
        }
    }
    Ok(())
}
fn load(app: &tauri::AppHandle, slot: &mut Option<Settings>) -> Result<Settings, String> {
    if slot.is_none() {
        let path = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join("shortcuts-v1.json");
        let settings = if path.exists() {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            if bytes.len() > 1_048_576 {
                return Err("Shortcut file too large".into());
            }
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?
        } else {
            Settings::default()
        };
        validate(&settings)?;
        *slot = Some(settings);
    }
    Ok(slot.as_ref().unwrap().clone())
}
#[tauri::command]
pub fn shortcut_catalog(
    window: tauri::WebviewWindow,
    commands: Option<Vec<Command>>,
) -> Result<Snapshot, String> {
    let app = window.app_handle();
    let owner = crate::modal_windows::owner(&window)?;
    let store = app.state::<Store>();
    if let Some(commands) = commands {
        if window.label() != owner || commands.len() > 1024 {
            return Err("Invalid shortcut catalog".into());
        }
        let mut ids = HashSet::new();
        for c in &commands {
            if c.id.len() > 160
                || c.label.len() > 512
                || c.category.len() > 512
                || !key_valid(&c.default_key)
                || !ids.insert(&c.id)
            {
                return Err(format!("Invalid shortcut command: {}", c.id));
            }
        }
        store
            .catalogs
            .lock()
            .unwrap()
            .insert(owner.clone(), commands);
    }
    let settings = load(app, &mut store.settings.lock().unwrap())?;
    let commands = store
        .catalogs
        .lock()
        .unwrap()
        .get(&owner)
        .cloned()
        .unwrap_or_default();
    Ok(Snapshot { settings, commands })
}
#[tauri::command]
pub fn save_shortcuts(
    window: tauri::WebviewWindow,
    mut settings: Settings,
) -> Result<Settings, String> {
    let app = window.app_handle();
    let owner = crate::modal_windows::owner(&window)?;
    let store = app.state::<Store>();
    validate(&settings)?;
    let mut slot = store.settings.lock().unwrap();
    let current = load(app, &mut slot)?;
    if settings.revision != current.revision {
        return Err("Shortcuts changed in another window. Reopen this dialog. / 別のウインドウで設定が更新されました。ダイアログを開き直してください / 其他窗口已更新快捷键，请重新打开对话框".into());
    }
    let catalogs = store.catalogs.lock().unwrap();
    no_conflicts(
        &settings,
        catalogs.get(&owner).map(Vec::as_slice).unwrap_or_default(),
    )?;
    drop(catalogs);
    settings.revision += 1;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(dir.join("shortcuts-v1.json"))
        .map_err(|e| e.to_string())?;
    *slot = Some(settings.clone());
    drop(slot);
    app.emit("shortcuts-changed", &settings)
        .map_err(|e| e.to_string())?;
    Ok(settings)
}
#[cfg(target_os = "macos")]
pub(crate) fn dispatch(
    app: &tauri::AppHandle,
    label: &str,
    event: &objc2_app_kit::NSEvent,
) -> bool {
    use objc2_app_kit::NSEventModifierFlags as F;
    let code = match event.keyCode() {
        0 => "KeyA",
        1 => "KeyS",
        2 => "KeyD",
        3 => "KeyF",
        4 => "KeyH",
        5 => "KeyG",
        6 => "KeyZ",
        7 => "KeyX",
        8 => "KeyC",
        9 => "KeyV",
        11 => "KeyB",
        12 => "KeyQ",
        13 => "KeyW",
        14 => "KeyE",
        15 => "KeyR",
        16 => "KeyY",
        17 => "KeyT",
        18 => "Digit1",
        19 => "Digit2",
        20 => "Digit3",
        21 => "Digit4",
        22 => "Digit6",
        23 => "Digit5",
        24 => "Equal",
        25 => "Digit9",
        26 => "Digit7",
        27 => "Minus",
        28 => "Digit8",
        29 => "Digit0",
        30 => "BracketRight",
        31 => "KeyO",
        32 => "KeyU",
        33 => "BracketLeft",
        34 => "KeyI",
        35 => "KeyP",
        37 => "KeyL",
        38 => "KeyJ",
        39 => "Quote",
        40 => "KeyK",
        41 => "Semicolon",
        42 => "Backslash",
        43 => "Comma",
        44 => "Slash",
        45 => "KeyN",
        46 => "KeyM",
        47 => "Period",
        50 => "Backquote",
        93 => "IntlYen",
        122 => "F1",
        120 => "F2",
        99 => "F3",
        118 => "F4",
        96 => "F5",
        97 => "F6",
        98 => "F7",
        100 => "F8",
        101 => "F9",
        109 => "F10",
        103 => "F11",
        111 => "F12",
        105 => "F13",
        107 => "F14",
        113 => "F15",
        106 => "F16",
        64 => "F17",
        79 => "F18",
        80 => "F19",
        90 => "F20",
        _ => return false,
    };
    let flags = event.modifierFlags();
    let mut key = String::new();
    for (flag, name) in [
        (F::Command, "Primary+"),
        (F::Control, "Control+"),
        (F::Option, "Alt+"),
        (F::Shift, "Shift+"),
    ] {
        if flags.contains(flag) {
            key.push_str(name);
        }
    }
    key.push_str(code);
    let store = app.state::<Store>();
    let slot = store.settings.lock().unwrap();
    let Some(settings) = slot.as_ref() else {
        return false;
    };
    let catalogs = store.catalogs.lock().unwrap();
    let Some(commands) = catalogs.get(label) else {
        return false;
    };
    if let Some(command) = commands.iter().find(|c| effective(settings, c) == key) {
        if !event.isARepeat() {
            let _ = app.emit_to(label, "shortcut-command", &command.id);
        }
        return true;
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicts_reserved_keys_and_cleared_bindings() {
        let mut s = Settings::default();
        let commands = vec![
            Command {
                id: "a".into(),
                label: "A".into(),
                category: "Edit".into(),
                default_key: "Primary+KeyA".into(),
            },
            Command {
                id: "b".into(),
                label: "B".into(),
                category: "Edit".into(),
                default_key: "KeyB".into(),
            },
        ];
        assert!(no_conflicts(&s, &commands).is_ok());
        s.sets[0].bindings.insert("b".into(), "Primary+KeyA".into());
        assert!(no_conflicts(&s, &commands).is_err());
        s.sets[0].bindings.insert("a".into(), String::new());
        assert!(no_conflicts(&s, &commands).is_ok());
        assert!(!key_valid("Primary+KeyQ"));
        assert!(!key_valid("KeyUnknown"));
        assert!(key_valid("Primary+Alt+Shift+KeyK"));
        let bytes = serde_json::to_vec(&s).unwrap();
        assert!(validate(&serde_json::from_slice(&bytes).unwrap()).is_ok());
    }
}
#[tauri::command]
pub fn export_shortcuts(text: String) -> Result<(), String> {
    if text.len() > 1_048_576 {
        return Err("Shortcut list too large".into());
    }
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Text", &["txt"])
        .set_file_name("LumaPaint-shortcuts.txt")
        .save_file()
    {
        std::fs::write(path, text).map_err(|e| e.to_string())?;
    }
    Ok(())
}
