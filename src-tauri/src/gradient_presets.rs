//! Portable gradient libraries; persistence belongs to Rust, independently of rendering.
use lumapaint_core::gradient::Gradient;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
    pub(crate) name: String,
    pub(crate) gradient: Gradient,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Library {
    version: u32,
    patterns: Vec<Pattern>,
}
fn validate(library: &Library) -> Result<(), String> {
    if library.version != 1 || library.patterns.len() > 128 {
        return Err("Unsupported library version or more than 128 patterns / 未対応の形式、または128件を超えています / 格式不支持或超过128个图案".into());
    }
    for pattern in &library.patterns {
        if pattern.name.trim().is_empty() || pattern.name.chars().count() > 80 {
            return Err("Pattern names must contain 1–80 characters / 名前は1〜80文字にしてください / 名称须为1至80个字符".into());
        }
        pattern.gradient.validate()?;
    }
    Ok(())
}
fn read(path: &std::path::Path) -> Result<Library, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Gradient library exceeds 1 MiB / ファイルは1 MiB以下にしてください / 文件不能超过1 MiB".into());
    }
    let library = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&library)?;
    Ok(library)
}
fn write(path: &std::path::Path, library: &Library) -> Result<(), String> {
    validate(library)?;
    let parent = path.parent().ok_or("Missing parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(library).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub(crate) fn load_patterns(path: &std::path::Path) -> Result<Vec<Pattern>, String> {
    Ok(read(path)?.patterns)
}
// Synchronous Tauri commands execute on the main thread, required by macOS dialogs.
#[tauri::command]
pub fn gradient_library(
    app: tauri::AppHandle,
    action: String,
    pattern: Option<Pattern>,
    index: Option<usize>,
) -> Result<Vec<Pattern>, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("gradients-v1.json");
    let mut library = if path.exists() {
        read(&path)?
    } else {
        Library {
            version: 1,
            patterns: vec![],
        }
    };
    match action.as_str() {
        "get" => {}
        "add" => {
            library.patterns.push(pattern.ok_or("Missing pattern")?);
            write(&path, &library)?;
        }
        "remove" => {
            let i = index.ok_or("Missing index")?;
            if i >= library.patterns.len() {
                return Err("Invalid pattern index".into());
            }
            library.patterns.remove(i);
            write(&path, &library)?;
        }
        "import" | "export" => {
            #[cfg(target_os = "macos")]
            {
                let dialog = rfd::FileDialog::new().add_filter("LumaPaint Gradients", &["json"]);
                if action == "import" {
                    if let Some(source) = dialog.pick_file() {
                        let imported = read(&source)?;
                        library.patterns.extend(imported.patterns);
                        write(&path, &library)?;
                    }
                } else if let Some(destination) =
                    dialog.set_file_name("LumaPaint-gradients.json").save_file()
                {
                    write(&destination, &library)?;
                }
            }
            #[cfg(not(target_os = "macos"))]
            return Err("Gradient file dialogs are not supported on this platform yet / このOSのファイル操作は未対応です / 此系统尚不支持文件操作".into());
        }
        _ => return Err("Unknown gradient library action".into()),
    }
    Ok(library.patterns)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn library() -> Library {
        serde_json::from_str(r#"{"version":1,"patterns":[{"name":"Test","gradient":{"kind":"linear","angle":45,"aspect":1,"method":"classic","stops":[{"position":0,"color":[0,1,2,3],"midpoint":0.3},{"position":1,"color":[255,254,253,252],"midpoint":0.5}]}}]}"#).unwrap()
    }
    #[test]
    fn rejects_corrupt_and_oversized_libraries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.json");
        std::fs::write(&path, b"{}").unwrap();
        assert!(read(&path).is_err());
        std::fs::write(&path, vec![b' '; 1024 * 1024 + 1]).unwrap();
        assert!(read(&path).is_err());
        let mut many = library();
        many.patterns = vec![many.patterns[0].clone(); 129];
        assert!(validate(&many).is_err());
    }
    #[test]
    fn roundtrip_and_reject_invalid_without_replacing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("patterns.json");
        write(&path, &library()).unwrap();
        let mut loaded = read(&path).unwrap();
        assert_eq!(loaded.patterns[0].gradient, library().patterns[0].gradient);
        loaded.patterns[0].gradient.stops[0].position = 2.;
        assert!(write(&path, &loaded).is_err());
        assert!(read(&path).is_ok());
        loaded = library();
        loaded.version = 2;
        assert!(validate(&loaded).is_err());
        loaded = library();
        loaded.patterns[0].name = " ".into();
        assert!(validate(&loaded).is_err());
    }
}
