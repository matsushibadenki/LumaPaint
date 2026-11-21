use base64::Engine;
use edfx::{Document, Locale, Paragraph};
#[cfg(target_os = "macos")]
use lumapaint_core::tiles::EdfxTransform;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[cfg(target_os = "macos")]
use tauri::Emitter;
use tauri::{AppHandle, Manager, WebviewWindow};

type Session = Arc<Mutex<Editing>>;

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(crate) struct Placed {
    pub document_id: u64,
    pub layer_id: String,
    pub data: Arc<str>,
    pub transform: EdfxTransform,
}

#[derive(Default)]
struct Editing {
    document: Option<Document>,
    #[cfg(target_os = "macos")]
    placed: Option<Placed>,
}

fn session(window: &WebviewWindow) -> Result<Session, String> {
    if !crate::modal_windows::is_edfx(window) {
        return Err("EDFX editor is closed".into());
    }
    Ok(window
        .state::<Sessions>()
        .editing
        .lock()
        .unwrap()
        .entry(window.label().into())
        .or_default()
        .clone())
}

#[derive(Default)]
pub(crate) struct Sessions {
    editing: Mutex<HashMap<String, Session>>,
    pending: Mutex<HashMap<String, PathBuf>>,
}

#[cfg(target_os = "macos")]
pub(crate) fn queue_open(app: &AppHandle, owner: &str, path: PathBuf) -> Result<(), String> {
    app.state::<Sessions>()
        .pending
        .lock()
        .unwrap()
        .insert(owner.into(), path);
    app.emit_to(owner, "edfx-open-request", ())
        .map_err(|error| error.to_string())
}

pub(crate) fn discard(app: &AppHandle, label: &str) {
    let sessions = app.state::<Sessions>();
    sessions.editing.lock().unwrap().remove(label);
    sessions.pending.lock().unwrap().remove(label);
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Edit {
    Open,
    OpenPending,
    LoadPlaced,
    New { locale: Locale, face: u32 },
    Reflow { paragraphs: Vec<Paragraph> },
    Save,
    ExportSvg,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page {
    paragraphs: Vec<Paragraph>,
    preview: Option<String>,
    placed: bool,
}

fn page(document: &Document, placed: bool) -> Page {
    const WIDTH: u32 = 300;
    const HEIGHT: u32 = 420;
    let preview = (|| -> Option<String> {
        let mut svg = Vec::new();
        document.write_svg(&mut svg).ok()?;
        let source = std::str::from_utf8(&svg).ok()?;
        let raster = lumapaint_renderer::vector::rasterize_svg(source, WIDTH, HEIGHT).ok()?;
        let png = lumapaint_renderer::vector::document_png(WIDTH, HEIGHT, raster.pixels).ok()?;
        Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        ))
    })();
    Page {
        paragraphs: document.paragraphs().to_vec(),
        preview,
        placed,
    }
}

#[tauri::command]
pub(crate) async fn edfx_edit(window: WebviewWindow, edit: Edit) -> Result<Option<Page>, String> {
    let session = session(&window)?;
    if matches!(edit, Edit::LoadPlaced) {
        #[cfg(target_os = "macos")]
        {
            let placed =
                crate::canvas::on_main(window, crate::canvas::platform::selected_edfx).await?;
            return crate::diagnostic_jobs::spawn_blocking(move || {
                let doc = Document::from_json(placed.data.as_bytes())
                    .map_err(|error| error.to_string())?;
                let response = page(&doc, true);
                *session.lock().map_err(|e| e.to_string())? = Editing {
                    document: Some(doc),
                    placed: Some(placed),
                };
                Ok(Some(response))
            })
            .await
            .map_err(|e| e.to_string())?;
        }
        #[cfg(not(target_os = "macos"))]
        return Err("EDFX canvas placement is unavailable on this platform".into());
    }
    let picked = match &edit {
        Edit::Open => {
            rfd::AsyncFileDialog::new()
                .add_filter("EDFX", &["edfx"])
                .pick_file()
                .await
        }
        Edit::New { .. } => {
            rfd::AsyncFileDialog::new()
                .add_filter("OpenType", &["otf", "ttf", "ttc", "otc"])
                .pick_file()
                .await
        }
        Edit::Save => {
            rfd::AsyncFileDialog::new()
                .add_filter("EDFX", &["edfx"])
                .set_file_name("document.edfx")
                .save_file()
                .await
        }
        Edit::ExportSvg => {
            rfd::AsyncFileDialog::new()
                .add_filter("SVG", &["svg"])
                .set_file_name("document.svg")
                .save_file()
                .await
        }
        Edit::OpenPending | Edit::LoadPlaced | Edit::Reflow { .. } => None,
    };
    let path = if matches!(edit, Edit::OpenPending) {
        let owner = crate::modal_windows::owner(&window)?;
        window
            .state::<Sessions>()
            .pending
            .lock()
            .unwrap()
            .remove(&owner)
    } else {
        picked.map(|file| file.path().to_owned())
    };
    if path.is_none() && !matches!(edit, Edit::Reflow { .. }) {
        return Ok(None);
    }
    crate::diagnostic_jobs::spawn_blocking(move || {
        let path = path.as_deref();
        let mut state = session.lock().map_err(|e| e.to_string())?;
        match edit {
            Edit::Open | Edit::OpenPending => {
                let doc = Document::load(path.unwrap()).map_err(|e| e.to_string())?;
                *state = Editing {
                    document: Some(doc),
                    #[cfg(target_os = "macos")]
                    placed: None,
                };
            }
            Edit::New { locale, face } => {
                let mut bytes = Vec::new();
                std::fs::File::open(path.unwrap())
                    .and_then(|file| file.take(edfx::MAX_FONT as u64 + 1).read_to_end(&mut bytes))
                    .map_err(|e| e.to_string())?;
                let doc = Document::new(
                    vec![Paragraph {
                        locale,
                        text: String::new(),
                    }],
                    &bytes,
                    face,
                )
                .map_err(|e| e.to_string())?;
                *state = Editing {
                    document: Some(doc),
                    #[cfg(target_os = "macos")]
                    placed: None,
                };
            }
            Edit::LoadPlaced => unreachable!(),
            Edit::Reflow { paragraphs } => state
                .document
                .as_mut()
                .ok_or("No EDFX document")?
                .replace_paragraphs(paragraphs)
                .map_err(|e| e.to_string())?,
            Edit::Save => state
                .document
                .as_ref()
                .ok_or("No EDFX document")?
                .save(path.unwrap())
                .map_err(|e| e.to_string())?,
            Edit::ExportSvg => state
                .document
                .as_ref()
                .ok_or("No EDFX document")?
                .export_svg(path.unwrap())
                .map_err(|e| e.to_string())?,
        }
        #[cfg(target_os = "macos")]
        let placed = state.placed.is_some();
        #[cfg(not(target_os = "macos"))]
        let placed = false;
        Ok(state
            .document
            .as_ref()
            .map(|document| page(document, placed)))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(target_os = "macos")]
pub(crate) struct TileTarget {
    pub dimensions: (u32, u32),
    pub ppi: [f64; 2],
}

#[cfg(target_os = "macos")]
fn render_page_tiles(
    paths: &str,
    target: TileTarget,
    transform: EdfxTransform,
) -> Result<lumapaint_core::tiles::SparseTiles, String> {
    use lumapaint_core::tiles::{SparseTiles, TILE_SIZE};
    use resvg::{tiny_skia, usvg};

    let (width, height) = target.dimensions;
    let [x, y, scale] = transform.0;
    let [sx, sy] = target.ppi.map(|value| value / 72_000. * scale);
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><g transform="translate({x} {y}) scale({sx} {sy})">{paths}</g></svg>"#
    );
    let tree =
        usvg::Tree::from_str(&svg, &usvg::Options::default()).map_err(|error| error.to_string())?;
    let mut tiles = SparseTiles::new(width, height)?;
    if tree.root().children().is_empty() {
        return Ok(tiles);
    }
    let bounds = tree.root().abs_bounding_box();
    let x0 = (bounds.left().floor() as u32).min(width) / TILE_SIZE;
    let y0 = (bounds.top().floor() as u32).min(height) / TILE_SIZE;
    let x1 = (bounds.right().ceil() as u32)
        .min(width)
        .div_ceil(TILE_SIZE);
    let y1 = (bounds.bottom().ceil() as u32)
        .min(height)
        .div_ceil(TILE_SIZE);
    if u64::from(x1.saturating_sub(x0)) * u64::from(y1.saturating_sub(y0)) > 512 {
        return Err("EDFX page covers too many tiles / EDFXの描画範囲が広すぎます / EDFX 页面覆盖的图块过多".into());
    }
    for ty in y0..y1 {
        for tx in x0..x1 {
            let x = tx * TILE_SIZE;
            let y = ty * TILE_SIZE;
            let w = TILE_SIZE.min(width - x);
            let h = TILE_SIZE.min(height - y);
            let mut pixmap = tiny_skia::Pixmap::new(w, h).ok_or("Cannot allocate EDFX tile")?;
            resvg::render(
                &tree,
                tiny_skia::Transform::from_translate(-(x as f32), -(y as f32)),
                &mut pixmap.as_mut(),
            );
            // EDFX 0.1 glyphs are black, so RGB is zero in both alpha encodings.
            tiles.write_rect(x, y, w, h, pixmap.data())?;
        }
    }
    Ok(tiles)
}

#[tauri::command]
pub(crate) async fn edfx_canvas_apply(window: WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let session = session(&window)?;
        let (document_id, tile_target) =
            crate::canvas::on_main(window.clone(), crate::canvas::platform::edfx_canvas_target)
                .await?;
        let worker = session.clone();
        let (placed, paths, tiles, data) = crate::diagnostic_jobs::spawn_blocking(move || {
            let editing = worker.lock().map_err(|e| e.to_string())?;
            let doc = editing.document.as_ref().ok_or("No EDFX document")?;
            if editing
                .placed
                .as_ref()
                .is_some_and(|layer| layer.document_id != document_id)
            {
                return Err("The target document has changed".to_string());
            }
            let paths = glyph_paths(doc)?;
            let transform = editing
                .placed
                .as_ref()
                .map_or_else(EdfxTransform::default, |layer| layer.transform);
            let tiles = tile_target
                .map(|target| render_page_tiles(&paths, target, transform))
                .transpose()?;
            Ok((
                editing.placed.clone(),
                paths,
                tiles,
                Arc::<str>::from(doc.to_json().map_err(|e| e.to_string())?),
            ))
        })
        .await
        .map_err(|e| e.to_string())??;
        let placed = crate::canvas::on_main(window, move || {
            crate::canvas::platform::apply_edfx(document_id, placed, paths, tiles, data)
        })
        .await?;
        session.lock().map_err(|e| e.to_string())?.placed = Some(placed);
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("EDFX canvas placement is unavailable on this platform".into())
    }
}

#[cfg(target_os = "macos")]
fn glyph_paths(doc: &Document) -> Result<String, String> {
    let mut svg = Vec::new();
    doc.write_svg(&mut svg).map_err(|error| error.to_string())?;
    let svg = String::from_utf8(svg).map_err(|error| error.to_string())?;
    let (_, paths) = svg.split_once(">\n").ok_or("Invalid EDFX SVG")?;
    let (paths, _) = paths.rsplit_once("</svg>").ok_or("Invalid EDFX SVG")?;
    Ok(paths.to_owned())
}

#[tauri::command]
pub(crate) async fn edfx_transform_mode(
    window: WebviewWindow,
    mode: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::canvas::on_main(window, move || {
            crate::canvas::platform::edfx_transform_mode(mode)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, mode);
        Err("EDFX canvas transform is unavailable on this platform".into())
    }
}

#[cfg(target_os = "macos")]
pub(crate) async fn transform_placement(
    window: WebviewWindow,
    placed: Placed,
    target: Option<TileTarget>,
    transform: EdfxTransform,
) -> Result<(), String> {
    let tiles = async {
        transform.validate()?;
        let Some(target) = target else {
            return Ok(None);
        };
        let data = placed.data.clone();
        crate::diagnostic_jobs::spawn_blocking(move || {
            let doc = Document::from_json(data.as_bytes()).map_err(|error| error.to_string())?;
            render_page_tiles(&glyph_paths(&doc)?, target, transform).map(Some)
        })
        .await
        .map_err(|error| error.to_string())?
    }
    .await;
    crate::canvas::on_main(window, move || {
        crate::canvas::platform::finish_edfx_transform(placed, tiles, transform)
    })
    .await
}
