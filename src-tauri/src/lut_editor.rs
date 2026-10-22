//! Native LUT editor store and bounded reference previews; no frame data in IPC.
use image::{DynamicImage, RgbaImage};
use lumapaint_core::lut::{Cube, Grade, MAX_BYTES};
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Emitter, Manager};
const STALE: &str = "LUT changed in another window. Reload. / 別のウインドウでLUTが変更されました。再読込してください / LUT已在其他窗口更改，请重新加载";
#[derive(Clone, Default)]
struct Edit {
    grade: Grade,
    base: Option<Arc<Cube>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Disk {
    grade: Grade,
    base: Option<Cube>,
}
struct Session {
    edit: Edit,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    revision: u64,
    photo: Option<RgbaImage>,
}
#[derive(Default)]
pub struct Store(Mutex<Option<Session>>);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    grade: Grade,
    base_name: Option<String>,
    revision: u64,
    can_undo: bool,
    can_redo: bool,
    has_photo: bool,
    samples: Vec<[f32; 3]>,
}
// This protocol owns only two small encoded reference previews, not document frames.
type PreviewPair = (Vec<u8>, Vec<u8>);
static PREVIEWS: Mutex<Option<PreviewPair>> = Mutex::new(None);
pub fn serve(request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let bytes = PREVIEWS.lock().ok().and_then(|p| {
        p.as_ref().map(|p| {
            if request.uri().path() == "/before" {
                p.0.clone()
            } else {
                p.1.clone()
            }
        })
    });
    let response = tauri::http::Response::builder()
        .status(if bytes.is_some() { 200 } else { 404 })
        .header("Content-Type", "image/png")
        .header("Cache-Control", "no-store")
        .header("Access-Control-Allow-Origin", "*")
        .body(bytes.unwrap_or_default())
        .unwrap();
    responder.respond(response);
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![];
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("File too large / ファイルが大きすぎます / 文件过大".into());
    }
    Ok(bytes)
}
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn persist(path: &Path, edit: &Edit) -> Result<(), String> {
    #[derive(Serialize)]
    struct Borrowed<'a> {
        grade: &'a Grade,
        base: Option<&'a Cube>,
    }
    let bytes = serde_json::to_vec(&Borrowed {
        grade: &edit.grade,
        base: edit.base.as_deref(),
    })
    .map_err(|e| e.to_string())?;
    atomic_write(path, &bytes)
}
fn snapshot(s: &Session) -> Snapshot {
    let mut samples = Vec::with_capacity(216);
    for b in 0..6 {
        for g in 0..6 {
            for r in 0..6 {
                samples.push(
                    s.edit
                        .grade
                        .apply(s.edit.base.as_deref(), [r, g, b].map(|v| v as f32 / 5.)),
                );
            }
        }
    }
    Snapshot {
        grade: s.edit.grade.clone(),
        base_name: s.edit.base.as_ref().map(|b| b.title.clone()),
        revision: s.revision,
        can_undo: !s.undo.is_empty(),
        can_redo: !s.redo.is_empty(),
        has_photo: s.photo.is_some(),
        samples,
    }
}
fn reference() -> RgbaImage {
    RgbaImage::from_fn(512, 288, |x, y| {
        let u = x as f32 / 511.;
        let band = y / 48;
        let rgb = match band {
            0 => [u, u, u],
            1 => [u, 0., 0.],
            2 => [0., u, 0.],
            3 => [0., 0., u],
            4 => [u, 1. - u, 0.5],
            _ => [0.75, 0.48, 0.32].map(|v| v * u * 1.5),
        };
        image::Rgba([
            (rgb[0] * 255.) as u8,
            (rgb[1] * 255.) as u8,
            (rgb[2] * 255.) as u8,
            255,
        ])
    })
}
fn previews(edit: &Edit, photo: Option<&RgbaImage>) -> Result<PreviewPair, String> {
    let fallback;
    let source = if let Some(p) = photo {
        p
    } else {
        fallback = reference();
        &fallback
    };
    let mut result = source.clone();
    for pixel in result.pixels_mut() {
        let c = edit.grade.apply(
            edit.base.as_deref(),
            [pixel[0], pixel[1], pixel[2]].map(|v| v as f32 / 255.),
        );
        for i in 0..3 {
            pixel[i] = (c[i].clamp(0., 1.) * 255.).round() as u8;
        }
    }
    let encode = |image: &RgbaImage| -> Result<Vec<u8>, String> {
        let mut bytes = Cursor::new(vec![]);
        DynamicImage::ImageRgba8(image.clone())
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(bytes.into_inner())
    };
    Ok((encode(source)?, encode(&result)?))
}
fn load_photo(path: &Path) -> Result<RgbaImage, String> {
    let bytes = read_bounded(path, 32 * 1024 * 1024)?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(192 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|e| e.to_string())?;
    Ok(image.thumbnail(640, 400).into_rgba8())
}
fn change(s: &mut Session, edit: Edit, path: &Path) -> Result<(), String> {
    let rendered = previews(&edit, s.photo.as_ref())?;
    persist(path, &edit)?;
    s.undo.push(std::mem::replace(&mut s.edit, edit));
    if s.undo.len() > 32 {
        s.undo.remove(0);
    }
    s.redo.clear();
    s.revision += 1;
    *PREVIEWS.lock().map_err(|e| e.to_string())? = Some(rendered);
    Ok(())
}
#[tauri::command]
pub async fn lut_editor(
    app: tauri::AppHandle,
    action: String,
    revision: Option<u64>,
    grade: Option<Grade>,
) -> Result<Snapshot, String> {
    let mut file: Option<PathBuf> = None;
    if matches!(action.as_str(), "import" | "export" | "photo") {
        let dialog = if action == "photo" {
            rfd::AsyncFileDialog::new()
                .add_filter("Reference image", &["png", "jpg", "jpeg", "webp"])
        } else {
            rfd::AsyncFileDialog::new().add_filter("Cube LUT", &["cube"])
        };
        let chosen = if action == "export" {
            dialog.set_file_name("LumaPaint.cube").save_file().await
        } else {
            dialog.pick_file().await
        };
        if let Some(chosen) = chosen {
            file = Some(chosen.path().to_owned());
        } else {
            return read_snapshot(app).await;
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        execute(&app, &action, revision, grade, file.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}
async fn read_snapshot(app: tauri::AppHandle) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || execute(&app, "get", None, None, None))
        .await
        .map_err(|e| e.to_string())?
}
fn execute(
    app: &tauri::AppHandle,
    action: &str,
    revision: Option<u64>,
    grade: Option<Grade>,
    file: Option<&Path>,
) -> Result<Snapshot, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("lut-editor-v1.json");
    let store = app.state::<Store>();
    let mut guard = store.0.lock().map_err(|e| e.to_string())?;
    if guard.is_none() {
        let edit = if path.exists() {
            let disk: Disk = serde_json::from_slice(&read_bounded(&path, MAX_BYTES)?)
                .map_err(|e| e.to_string())?;
            disk.grade.validate()?;
            if let Some(b) = &disk.base {
                b.validate()?;
            }
            Edit {
                grade: disk.grade,
                base: disk.base.map(Arc::new),
            }
        } else {
            Edit::default()
        };
        *PREVIEWS.lock().map_err(|e| e.to_string())? = Some(previews(&edit, None)?);
        *guard = Some(Session {
            edit,
            undo: vec![],
            redo: vec![],
            revision: 0,
            photo: None,
        });
    }
    let s = guard.as_mut().unwrap();
    if action != "get" && revision != Some(s.revision) {
        return Err(STALE.into());
    }
    match action {
        "get" => {}
        "update" => {
            let grade = grade.ok_or("Missing grade")?;
            grade.validate()?;
            if grade != s.edit.grade {
                change(
                    s,
                    Edit {
                        grade,
                        base: s.edit.base.clone(),
                    },
                    &path,
                )?;
            }
        }
        "reset" => change(s, Edit::default(), &path)?,
        "import" => {
            let bytes = read_bounded(file.ok_or("Missing file")?, MAX_BYTES)?;
            let cube = Cube::parse(std::str::from_utf8(&bytes).map_err(|e| e.to_string())?)?;
            let grade = Grade {
                name: cube.title.clone(),
                ..Grade::default()
            };
            grade.validate()?;
            change(
                s,
                Edit {
                    grade,
                    base: Some(Arc::new(cube)),
                },
                &path,
            )?;
        }
        "export" => {
            let bytes = s.edit.grade.bake(s.edit.base.as_deref())?;
            atomic_write(file.ok_or("Missing file")?, bytes.as_bytes())?;
        }
        "photo" => {
            let photo = load_photo(file.ok_or("Missing file")?)?;
            let rendered = previews(&s.edit, Some(&photo))?;
            s.photo = Some(photo);
            s.revision += 1;
            *PREVIEWS.lock().map_err(|e| e.to_string())? = Some(rendered);
        }
        "chart" => {
            let rendered = previews(&s.edit, None)?;
            s.photo = None;
            s.revision += 1;
            *PREVIEWS.lock().map_err(|e| e.to_string())? = Some(rendered);
        }
        "undo" | "redo" => {
            let next = if action == "undo" {
                s.undo.last()
            } else {
                s.redo.last()
            }
            .cloned();
            if let Some(next) = next {
                let rendered = previews(&next, s.photo.as_ref())?;
                persist(&path, &next)?;
                let previous = std::mem::replace(&mut s.edit, next);
                if action == "undo" {
                    s.undo.pop();
                    s.redo.push(previous);
                } else {
                    s.redo.pop();
                    s.undo.push(previous);
                }
                s.revision += 1;
                *PREVIEWS.lock().map_err(|e| e.to_string())? = Some(rendered);
            }
        }
        _ => return Err("Unknown LUT action".into()),
    }
    let result = snapshot(s);
    drop(guard);
    if action != "get" {
        let _ = app.emit("lut-editor-changed", result.revision);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lut_editor_preview_preserves_alpha_and_changes_color() {
        let source = RgbaImage::from_pixel(2, 2, image::Rgba([80, 100, 120, 70]));
        let edit = Edit {
            grade: Grade {
                exposure: 1.,
                ..Grade::default()
            },
            base: None,
        };
        let (a, b) = previews(&edit, Some(&source)).unwrap();
        let a = image::load_from_memory(&a).unwrap().into_rgba8();
        let b = image::load_from_memory(&b).unwrap().into_rgba8();
        assert_eq!(b.get_pixel(0, 0).0, [160, 200, 240, 70]);
        assert_eq!(a, source);
    }
    #[test]
    fn lut_editor_persistence_and_failed_edit_are_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lut.json");
        let edit = Edit::default();
        persist(&path, &edit).unwrap();
        let disk: Disk = serde_json::from_slice(&read_bounded(&path, MAX_BYTES).unwrap()).unwrap();
        assert_eq!(disk.grade, edit.grade);
        let mut s = Session {
            edit,
            undo: vec![],
            redo: vec![],
            revision: 0,
            photo: None,
        };
        let changed = Edit {
            grade: Grade {
                exposure: 1.,
                ..Grade::default()
            },
            base: None,
        };
        // A directory cannot be atomically replaced with a settings file.
        assert!(change(&mut s, changed.clone(), dir.path()).is_err());
        assert_eq!(s.revision, 0);
        assert_eq!(s.edit.grade.exposure, 0.);
        change(&mut s, changed, &path).unwrap();
        assert_eq!(s.revision, 1);
        assert_eq!(s.undo[0].grade.exposure, 0.);
    }
}
