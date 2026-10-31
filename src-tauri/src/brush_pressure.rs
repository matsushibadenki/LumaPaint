//! Rust-owned brush response presets and bounded scratch drawing (never artwork).
use lumapaint_core::{
    document::{Brush, Point, Stroke},
    pressure::{BrushKind, Settings},
    tiles::TiledRasterDocument,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tauri::Manager;
const INVALID: &str = "Invalid pressure settings / 筆圧設定が不正です / 压感设置无效";
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Library {
    version: u8,
    presets: BTreeMap<BrushKind, Settings>,
}
static PRESETS: Mutex<()> = Mutex::new(());
#[tauri::command]
pub fn brush_pressure_preset(
    app: tauri::AppHandle,
    id: BrushKind,
    action: String,
    settings: Option<Settings>,
) -> Result<Settings, String> {
    let _lock = PRESETS.lock().map_err(|e| e.to_string())?;
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    let path = dir.join("brush-pressure-v1.json");
    let mut library = match std::fs::read(&path) {
        Ok(bytes) => {
            if bytes.len() > 65536 {
                return Err(INVALID.into());
            }
            let value: Library = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
            if value.version != 1 || value.presets.len() > 11 {
                return Err(INVALID.into());
            }
            for (key, s) in &value.presets {
                s.validate()?;
                if *key != s.brush_id {
                    return Err(INVALID.into());
                }
            }
            value
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Library {
            version: 1,
            ..Library::default()
        },
        Err(e) => return Err(e.to_string()),
    };
    match action.as_str() {
        "get" => Ok(library
            .presets
            .get(&id)
            .copied()
            .unwrap_or_else(|| Settings::for_brush(id))),
        "save" | "reset" => {
            let next = if action == "reset" {
                Settings::for_brush(id)
            } else {
                settings.ok_or(INVALID)?
            };
            next.validate()?;
            if next.brush_id != id {
                return Err(INVALID.into());
            }
            if action == "reset" {
                library.presets.remove(&id);
            } else {
                library.presets.insert(id, next);
            }
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let tmp = dir.join("brush-pressure-v1.tmp");
            std::fs::write(
                &tmp,
                serde_json::to_vec(&library).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, path).map_err(|e| e.to_string())?;
            Ok(next)
        }
        _ => Err(INVALID.into()),
    }
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    x: f32,
    y: f32,
    pressure: f32,
}
impl Sample {
    fn valid(self) -> bool {
        [self.x, self.y, self.pressure]
            .iter()
            .all(|v| v.is_finite())
            && (0. ..=256.).contains(&self.x)
            && (0. ..=128.).contains(&self.y)
            && (0. ..=1.).contains(&self.pressure)
    }
}
#[derive(Clone, Default)]
struct Pad {
    owner: String,
    strokes: Vec<Vec<Sample>>,
    active: bool,
}
impl Pad {
    fn request(&mut self, owner: &str, action: &str, samples: Vec<Sample>) -> Result<(), String> {
        if owner.len() > 64
            || owner.is_empty()
            || !owner
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(INVALID.into());
        }
        if action == "open" {
            if !samples.is_empty() {
                return Err(INVALID.into());
            }
            if self.active {
                self.edit("cancel", vec![])?;
            }
            self.owner = owner.into();
            if self.strokes.is_empty() {
                self.edit("demo", vec![])?;
            }
            Ok(())
        } else {
            if self.owner != owner {
                return Err(
                    "Scratch view changed / 試し描きの表示が変わりました / 试画视图已更改".into(),
                );
            }
            self.edit(action, samples)
        }
    }
    fn edit(&mut self, action: &str, samples: Vec<Sample>) -> Result<(), String> {
        if samples.len() > 32 || samples.iter().any(|s| !s.valid()) {
            return Err(INVALID.into());
        }
        match action {
            "begin" => {
                if samples.is_empty() {
                    return Err(INVALID.into());
                }
                if self.active {
                    self.strokes.pop();
                }
                if self.strokes.len() >= 8 {
                    self.strokes.remove(0);
                }
                self.strokes.push(samples);
                self.active = true;
            }
            "append" | "end" => {
                if !self.active {
                    return Err(INVALID.into());
                }
                let stroke = self.strokes.last_mut().ok_or(INVALID)?;
                if stroke.len() + samples.len() > 256 {
                    return Err(
                        "Scratch stroke is full / 試し描きの点数上限です / 试画点数已满".into(),
                    );
                }
                stroke.extend(samples);
                if action == "end" {
                    self.active = false;
                }
            }
            "cancel" => {
                if self.active {
                    self.strokes.pop();
                }
                self.active = false;
            }
            "clear" => {
                self.strokes.clear();
                self.active = false;
            }
            "demo" => {
                self.strokes = vec![(0..=48)
                    .map(|i| {
                        let t = i as f32 / 48.;
                        Sample {
                            x: 18. + 220. * t,
                            y: 64. + 20. * (t * std::f32::consts::TAU).sin(),
                            pressure: t,
                        }
                    })
                    .collect()];
                self.active = false;
            }
            "refresh" => {}
            _ => return Err(INVALID.into()),
        }
        Ok(())
    }
    fn png(&self, brush: Brush) -> Result<Vec<u8>, String> {
        let mut tiles = TiledRasterDocument::new(256, 128)?;
        tiles.add_layer("scratch".into(), "Scratch".into())?;
        for samples in self.strokes.iter().filter(|_| !brush.no_color) {
            let stroke = Stroke {
                brush,
                points: samples.iter().map(|s| Point { x: s.x, y: s.y }).collect(),
                pressures: samples.iter().map(|s| s.pressure).collect(),
                eraser: false,
                clear: false,
                selection: None,
            };
            lumapaint_renderer::paint_stroke_into_tiles(&mut tiles, "scratch", &stroke)?;
            tiles.discard_history();
        }
        lumapaint_renderer::thumbnails::tiled_page_preview(&tiles, 256)
    }
}
static PADS: Mutex<BTreeMap<String, Pad>> = Mutex::new(BTreeMap::new());
pub(crate) fn forget_window(label: &str) {
    if let Ok(mut images) = IMAGES.lock() {
        images.retain(|(owner, _, _)| owner != label);
    }
    if let Ok(mut pads) = PADS.lock() {
        pads.remove(label);
    }
}
type PreviewResource = (String, u64, Arc<Vec<u8>>);
static IMAGES: Mutex<VecDeque<PreviewResource>> = Mutex::new(VecDeque::new());
static NEXT_IMAGE: AtomicU64 = AtomicU64::new(1);
fn image_url(label: String, png: Vec<u8>) -> Result<String, String> {
    let id = NEXT_IMAGE.fetch_add(1, Ordering::Relaxed);
    let mut images = IMAGES.lock().map_err(|e| e.to_string())?;
    while images
        .iter()
        .filter(|(owner, _, _)| *owner == label)
        .count()
        >= 4
    {
        if let Some(index) = images.iter().position(|(owner, _, _)| *owner == label) {
            images.remove(index);
        }
    }
    while images.len() >= 64 {
        images.pop_front();
    }
    images.push_back((label, id, Arc::new(png)));
    Ok(format!("pressure-preview://localhost/{id}"))
}
pub fn serve(request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let id = request
        .uri()
        .path()
        .trim_start_matches('/')
        .parse::<u64>()
        .ok();
    let png = IMAGES.lock().ok().and_then(|images| {
        images
            .iter()
            .find(|(_, token, _)| Some(*token) == id)
            .map(|(_, _, bytes)| bytes.clone())
    });
    let response = tauri::http::Response::builder()
        .status(if png.is_some() { 200 } else { 404 })
        .header("Content-Type", "image/png")
        .header("Cache-Control", "no-store")
        .body(png.map_or_else(Vec::new, |bytes| (*bytes).clone()))
        .unwrap();
    responder.respond(response);
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    url: String,
    size_curve: Vec<[f32; 2]>,
    opacity_curve: Vec<[f32; 2]>,
    pressure: f32,
    size: f32,
    opacity: f32,
}
#[tauri::command]
pub async fn brush_pressure_preview(
    window: tauri::WebviewWindow,
    action: String,
    brush: Brush,
    samples: Vec<Sample>,
    owner: String,
) -> Result<Preview, String> {
    if !crate::editor_windows::is_editor(window.label()) {
        return Err(INVALID.into());
    }
    brush.validate()?;
    if owner.len() > 64
        || owner.is_empty()
        || !owner
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        return Err(INVALID.into());
    }
    let label = window.label().to_string();
    let pad = {
        let mut pads = PADS.lock().map_err(|e| e.to_string())?;
        if !pads.contains_key(&label) && pads.len() >= 16 {
            return Err(INVALID.into());
        }
        let pad = pads.entry(label.clone()).or_default();
        pad.request(&owner, &action, samples)?;
        pad.clone()
    };
    crate::diagnostic_jobs::spawn_blocking(move || {
        let pressure = pad
            .strokes
            .last()
            .and_then(|s| s.last())
            .map_or(1., |s| s.pressure);
        let curve = |response: lumapaint_core::pressure::Response| {
            (0..=32)
                .map(|i| {
                    let p = i as f32 / 32.;
                    [p, response.evaluate(p)]
                })
                .collect()
        };
        let url = image_url(label, pad.png(brush)?)?;
        Ok(Preview {
            url,
            size_curve: curve(brush.pressure.size),
            opacity_curve: curve(brush.pressure.opacity),
            pressure,
            size: brush.size * brush.pressure.size.evaluate(pressure),
            opacity: if brush.no_color {
                0.
            } else {
                brush.opacity * brush.alpha * brush.pressure.opacity.evaluate(pressure)
            },
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scratch_is_bounded_atomic_and_cancelable() {
        let mut pad = Pad::default();
        pad.edit("demo", vec![]).unwrap();
        let before = pad.strokes.len();
        assert!(pad
            .edit(
                "begin",
                vec![Sample {
                    x: f32::NAN,
                    y: 2.,
                    pressure: 0.5
                }]
            )
            .is_err());
        assert_eq!(pad.strokes.len(), before);
        pad.edit(
            "begin",
            vec![Sample {
                x: 30.,
                y: 40.,
                pressure: 0.5,
            }],
        )
        .unwrap();
        pad.edit("cancel", vec![]).unwrap();
        assert_eq!(pad.strokes.len(), before);
        for _ in 0..20 {
            pad.edit(
                "begin",
                vec![Sample {
                    x: 30.,
                    y: 40.,
                    pressure: 0.5,
                }],
            )
            .unwrap();
            pad.edit("end", vec![]).unwrap();
        }
        assert_eq!(pad.strokes.len(), 8);
        assert!(pad.edit("append", vec![]).is_err());
        pad.edit("clear", vec![]).unwrap();
        assert!(pad.strokes.is_empty());
    }
    #[test]
    fn stale_view_cannot_cancel_a_new_gesture() {
        let mut pad = Pad::default();
        pad.request("old", "open", vec![]).unwrap();
        pad.request("new", "open", vec![]).unwrap();
        pad.request(
            "new",
            "begin",
            vec![Sample {
                x: 20.,
                y: 20.,
                pressure: 0.5,
            }],
        )
        .unwrap();
        let count = pad.strokes.len();
        assert!(pad.request("old", "cancel", vec![]).is_err());
        assert!(pad.active);
        assert_eq!(pad.strokes.len(), count);
        pad.request("new", "end", vec![]).unwrap();
        assert!(!pad.active);
    }
    #[test]
    fn scratch_uses_the_real_renderer_without_artwork_state() {
        let mut pad = Pad::default();
        pad.edit("demo", vec![]).unwrap();
        let mut brush = Brush::default();
        let a = pad.png(brush).unwrap();
        brush.pressure.size.enabled = false;
        let b = pad.png(brush).unwrap();
        assert_ne!(a, b);
        brush.pressure.opacity.enabled = true;
        let c = pad.png(brush).unwrap();
        assert_ne!(b, c);
        assert_eq!(pad.strokes[0].len(), 49);
    }
}
