//! Shared, renderer-independent color and gradient sample library.
use lumapaint_core::gradient::Gradient;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use tauri::{Emitter, Manager};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Paint {
    Color { color: [u8; 3] },
    Gradient { gradient: Gradient },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Swatch {
    id: u32,
    name: String,
    paint: Paint,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    name: String,
    paint: Paint,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Library {
    version: u32,
    next_id: u32,
    #[serde(default)]
    basic_samples_added: bool,
    swatches: Vec<Swatch>,
}
// Run once, so deleting a built-in sample never causes it to reappear.
fn add_basic_samples(library: &mut Library, basics: Vec<Draft>) -> Result<(), String> {
    if library.basic_samples_added {
        return Ok(());
    }
    for draft in basics {
        if library.swatches.len() < 512 && !library.swatches.iter().any(|s| s.name == draft.name) {
            add(library, draft)?;
        }
    }
    library.basic_samples_added = true;
    Ok(())
}
const INVALID: &str =
    "Invalid swatch library / スウォッチの値・形式が不正です / 色板参数或格式无效";
fn validate(library: &Library) -> Result<(), String> {
    if library.version != 1 || library.swatches.len() > 512 {
        return Err(INVALID.into());
    }
    let mut ids = std::collections::HashSet::new();
    for s in &library.swatches {
        if s.id >= library.next_id
            || !ids.insert(s.id)
            || s.name.trim().is_empty()
            || s.name.chars().count() > 80
        {
            return Err(INVALID.into());
        }
        if let Paint::Gradient { gradient } = &s.paint {
            gradient.validate()?;
        }
    }
    Ok(())
}
fn read(path: &std::path::Path) -> Result<Library, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(INVALID.into());
    }
    let library = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&library)?;
    Ok(library)
}
#[cfg(any(target_os = "macos", test))]
fn import_library(path: &std::path::Path) -> Result<Library, String> {
    match read(path) {
        Ok(library) => Ok(library),
        Err(original) => {
            let patterns = crate::gradient_presets::load_patterns(path).map_err(|_| original)?;
            let mut library = Library {
                version: 1,
                next_id: 1,
                basic_samples_added: false,
                swatches: vec![],
            };
            for p in patterns {
                add(
                    &mut library,
                    Draft {
                        name: p.name,
                        paint: Paint::Gradient {
                            gradient: p.gradient,
                        },
                    },
                )?;
            }
            Ok(library)
        }
    }
}
fn write(path: &std::path::Path, library: &Library) -> Result<(), String> {
    validate(library)?;
    let parent = path.parent().ok_or(INVALID)?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(library).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn add(library: &mut Library, draft: Draft) -> Result<(), String> {
    let id = library.next_id;
    library.next_id = id.checked_add(1).ok_or(INVALID)?;
    library.swatches.push(Swatch {
        id,
        name: draft.name,
        paint: draft.paint,
    });
    validate(library)
}
fn add_many(library: &mut Library, drafts: Vec<Draft>) -> Result<(), String> {
    if drafts.is_empty() || drafts.len() > 64 {
        return Err(INVALID.into());
    }
    let mut next = library.clone();
    for draft in drafts {
        add(&mut next, draft)?;
    }
    *library = next;
    Ok(())
}
fn edit(library: &mut Library, id: u32, draft: Option<Draft>) -> Result<(), String> {
    let index = library
        .swatches
        .iter()
        .position(|s| s.id == id)
        .ok_or(INVALID)?;
    if let Some(draft) = draft {
        library.swatches[index] = Swatch {
            id,
            name: draft.name,
            paint: draft.paint,
        };
    } else {
        library.swatches.remove(index);
    }
    validate(library)
}
// Sync commands serialize library mutations and keep Cocoa dialogs on the main thread.
#[tauri::command]
pub fn swatch_library(
    app: tauri::AppHandle,
    action: String,
    id: Option<u32>,
    draft: Option<Draft>,
    seeds: Option<Vec<Draft>>,
    batch: Option<Vec<Draft>>,
) -> Result<Vec<Swatch>, String> {
    let directory = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let path = directory.join("swatches-v1.json");
    let mut library = if path.exists() {
        read(&path)?
    } else {
        let mut library = Library {
            version: 1,
            next_id: 1,
            basic_samples_added: false,
            swatches: vec![],
        };
        for seed in seeds.clone().unwrap_or_default() {
            add(&mut library, seed)?;
        }
        // Preserve previously registered gradient patterns when first opening the unified panel.
        let old = directory.join("gradients-v1.json");
        if old.exists() {
            for p in crate::gradient_presets::load_patterns(&old)? {
                add(
                    &mut library,
                    Draft {
                        name: p.name,
                        paint: Paint::Gradient {
                            gradient: p.gradient,
                        },
                    },
                )?;
            }
        }
        write(&path, &library)?;
        library
    };
    if !library.basic_samples_added {
        if let Some(seeds) = seeds {
            let basics: Vec<_> = seeds
                .into_iter()
                .filter(|s| matches!(s.paint, Paint::Gradient { .. }))
                .take(5)
                .collect();
            if basics.len() == 5 {
                add_basic_samples(&mut library, basics)?;
                write(&path, &library)?;
            }
        }
    }
    match action.as_str() {
        "get" => {}
        "add" => {
            add(&mut library, draft.ok_or(INVALID)?)?;
            write(&path, &library)?;
        }
        "addMany" => {
            add_many(&mut library, batch.ok_or(INVALID)?)?;
            write(&path, &library)?;
        }
        "update" => {
            edit(
                &mut library,
                id.ok_or(INVALID)?,
                Some(draft.ok_or(INVALID)?),
            )?;
            write(&path, &library)?;
        }
        "remove" => {
            edit(&mut library, id.ok_or(INVALID)?, None)?;
            write(&path, &library)?;
        }
        "import" | "export" => {
            #[cfg(target_os = "macos")]
            {
                let dialog = rfd::FileDialog::new().add_filter("LumaPaint Swatches", &["json"]);
                if action == "import" {
                    if let Some(source) = dialog.pick_file() {
                        let imported = import_library(&source)?;
                        for s in imported.swatches {
                            add(
                                &mut library,
                                Draft {
                                    name: s.name,
                                    paint: s.paint,
                                },
                            )?;
                        }
                        write(&path, &library)?;
                    }
                } else if let Some(destination) =
                    dialog.set_file_name("LumaPaint-swatches.json").save_file()
                {
                    write(&destination, &library)?;
                }
            }
            #[cfg(not(target_os="macos"))]
            return Err("File dialogs are not supported on this platform / このOSのファイル操作は未対応です / 此系统尚不支持文件操作".into());
        }
        _ => return Err(INVALID.into()),
    }
    if action != "get" {
        let _ = app.emit("swatches-changed", ());
    }
    Ok(library.swatches)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn draft(name: &str) -> Draft {
        Draft {
            name: name.into(),
            paint: Paint::Color {
                color: [12, 34, 56],
            },
        }
    }
    #[test]
    fn builtin_migration_preserves_user_samples_and_deleted_defaults() {
        let mut library: Library =
            serde_json::from_str(r#"{"version":1,"nextId":1,"swatches":[]}"#).unwrap();
        add(&mut library, draft("Custom")).unwrap();
        add_basic_samples(&mut library, vec![draft("Basic")]).unwrap();
        assert_eq!(library.swatches[0].name, "Custom");
        edit(&mut library, 2, None).unwrap();
        let serialized = serde_json::to_string(&library).unwrap();
        let mut restored: Library = serde_json::from_str(&serialized).unwrap();
        add_basic_samples(&mut restored, vec![draft("Basic")]).unwrap();
        assert_eq!(restored.swatches.len(), 1);
        assert_eq!(restored.swatches[0].id, 1);
    }
    #[test]
    fn stable_identity_edit_delete_and_roundtrip() {
        let mut l = Library {
            version: 1,
            next_id: 1,
            basic_samples_added: false,
            swatches: vec![],
        };
        add(&mut l, draft("A")).unwrap();
        add(&mut l, draft("B")).unwrap();
        edit(&mut l, 1, None).unwrap();
        edit(&mut l, 2, Some(draft("Renamed"))).unwrap();
        assert_eq!(l.swatches[0].name, "Renamed");
        assert!(edit(&mut l, 1, None).is_err());
        add(&mut l, draft("C")).unwrap();
        assert_eq!(l.swatches[1].id, 3);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("swatches.json");
        write(&path, &l).unwrap();
        let saved = read(&path).unwrap();
        assert_eq!(saved.swatches[0].paint, l.swatches[0].paint);
        l.swatches[0].name = " ".into();
        assert!(write(&path, &l).is_err());
        assert_eq!(read(&path).unwrap().swatches[0].name, "Renamed");
    }
    #[test]
    fn palette_batch_is_atomic() {
        let mut l = Library {
            version: 1,
            next_id: 1,
            basic_samples_added: false,
            swatches: vec![],
        };
        add_many(
            &mut l,
            (0..5).map(|i| draft(&format!("Tone {i}"))).collect(),
        )
        .unwrap();
        assert_eq!(l.swatches.len(), 5);
        let before = l.next_id;
        assert!(add_many(&mut l, vec![draft("Valid"), draft(" ")]).is_err());
        assert_eq!(l.swatches.len(), 5);
        assert_eq!(l.next_id, before);
        for _ in 0..505 {
            add(&mut l, draft("Existing")).unwrap();
        }
        assert!(add_many(&mut l, (0..5).map(|_| draft("Overflow")).collect()).is_err());
        assert_eq!(l.swatches.len(), 510);
    }
    #[test]
    fn imports_legacy_gradients_and_retains_full_appearance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.json");
        std::fs::write(&path,r#"{"version":1,"patterns":[{"name":"Radial","gradient":{"kind":"radial","angle":12,"aspect":0.7,"method":"perceptual","dither":true,"stops":[{"position":0,"color":[1,2,3,4],"midpoint":0.2},{"position":1,"color":[5,6,7,8],"midpoint":0.5}]}}]}"#).unwrap();
        let imported = import_library(&path).unwrap();
        let Paint::Gradient { gradient } = &imported.swatches[0].paint else {
            panic!("gradient expected")
        };
        assert!(gradient.dither);
        assert_eq!(gradient.stops[0].color[3], 4);
        assert_eq!(gradient.stops[0].midpoint, 0.2);
        let saved = dir.path().join("new.json");
        write(&saved, &imported).unwrap();
        assert_eq!(
            read(&saved).unwrap().swatches[0].paint,
            imported.swatches[0].paint
        );
        std::fs::write(&path, r#"{"version":2,"patterns":[]}"#).unwrap();
        assert!(import_library(&path).is_err());
    }
    #[test]
    fn rejects_duplicate_ids_invalid_version_and_excess() {
        let mut l = Library {
            version: 1,
            next_id: 1,
            basic_samples_added: false,
            swatches: vec![],
        };
        add(&mut l, draft("A")).unwrap();
        l.swatches.push(l.swatches[0].clone());
        assert!(validate(&l).is_err());
        l.swatches.pop();
        l.version = 2;
        assert!(validate(&l).is_err());
        l.version = 1;
        for _ in 0..511 {
            add(&mut l, draft("C")).unwrap();
        }
        assert!(add(&mut l, draft("Too many")).is_err());
    }
}
