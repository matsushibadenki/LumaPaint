//! Finder drops: AppKit receives drops over the GPU view; decoding stays on workers.
use super::*;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSDragOperation, NSDraggingInfo, NSPasteboardTypeFileURL};
use objc2_foundation::NSURL;
use std::path::{Path, PathBuf};

pub(super) fn supported(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "lumapaint" | "svg" | "png" | "jpg" | "jpeg" | "webp" | "psd" | "psb" | "pdf" | "ai"
    )
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}
pub(super) fn paths(sender: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let Some(items) = sender.draggingPasteboard().pasteboardItems() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            // NSURL decodes escaped spaces, Unicode and percent signs without guessing paths.
            let value = unsafe { item.stringForType(NSPasteboardTypeFileURL) }?;
            let url = NSURL::URLWithString(&value)?;
            if !url.isFileURL() {
                return None;
            }
            let path = url.path()?.to_string();
            supported(Path::new(&path)).then_some(path)
        })
        .collect()
}
pub(super) fn drag_operation(sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
    if paths(sender).is_empty() {
        NSDragOperation::None
    } else {
        NSDragOperation::Copy
    }
}
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeDrop {
    paths: Vec<String>,
    target_id: Option<u64>,
}
pub(super) fn perform(view: &PaintView, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
    let paths = paths(sender);
    if paths.is_empty() {
        return false;
    }
    let Ok(_session) = SessionGuard::enter(&view.ivars().label) else {
        return false;
    };
    let local = view.convertPoint_fromView(sender.draggingLocation(), None);
    let on_document = CANVAS.with(|slot| {
        slot.borrow().as_ref().is_some_and(|canvas| {
            let point = canvas
                .viewport
                .document_point(local.x as f32, local.y as f32);
            point.x >= 0.
                && point.y >= 0.
                && point.x < canvas.viewport.document_width
                && point.y < canvas.viewport.document_height
        })
    });
    let target_id = (on_document && DOCUMENT_OPEN.with(|open| open.get()))
        .then(|| ACTIVE_DOCUMENT_ID.with(|id| id.get()));
    APP.get().is_some_and(|app| {
        app.emit_to(
            &view.ivars().label,
            "native-file-drop",
            NativeDrop { paths, target_id },
        )
        .is_ok()
    })
}

pub(crate) enum Prepared {
    Open(OpenDocument),
    Layer {
        name: String,
        source: String,
        raster: bool,
    },
    Pdf(PathBuf),
    TiledLayer {
        name: String,
        tiles: lumapaint_core::tiles::SparseTiles,
    },
}
/// No session/thread-local document state may be accessed while preparing a file.
pub(crate) fn prepare(path: PathBuf, as_layer: bool) -> Result<Prepared, String> {
    if !supported(&path) {
        return Err("Unsupported file / 未対応のファイルです / 不支持的文件".into());
    }
    if !path.is_file() {
        return Err(
            "Drop a file, not a folder / ファイルをドロップしてください / 请拖放文件".into(),
        );
    }
    let ext = extension(&path);
    if matches!(ext.as_str(), "pdf" | "ai") {
        return Ok(Prepared::Pdf(path));
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let layer_name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let (content, project_path, fingerprint) = match ext.as_str() {
        "lumapaint" => {
            let (project, fingerprint) = crate::project_file::read_any_with_fingerprint(&path)?;
            let content = match project {
                crate::project_file::ProjectData::Legacy(mut document) => {
                    for layer in document.svg_layers() {
                        validate_svg(&layer.source)?;
                    }
                    document.mark_saved(name.clone());
                    OpenDocumentContent::Legacy(document)
                }
                crate::project_file::ProjectData::Tiled(state) => {
                    OpenDocumentContent::Tiled(Box::new(TiledSession {
                        selected_layer: None,
                        document: TiledRasterDocument::from_state(state)?,
                        name: name.clone(),
                        file_name: Some(name.clone()),
                        saved_revision: Some(0),
                    }))
                }
            };
            (content, Some(path), Some(fingerprint))
        }
        "psd" | "psb" => {
            let prepared = crate::psd_import::decode(&path)?;
            return if as_layer {
                let source = tiled_svg(&prepared.document)?;
                Ok(Prepared::Layer {
                    name: layer_name,
                    source,
                    raster: true,
                })
            } else {
                Ok(Prepared::Open(OpenDocument {
                    id: 0,
                    content: OpenDocumentContent::Tiled(Box::new(TiledSession {
                        selected_layer: None,
                        document: prepared.document,
                        name,
                        file_name: None,
                        saved_revision: None,
                    })),
                    path: None,
                    fingerprint: None,
                }))
            };
        }
        "svg" => {
            let bytes = read_bounded(&path, 4 * 1024 * 1024)?;
            let source = String::from_utf8(bytes.clone()).map_err(|e| e.to_string())?;
            validate_svg(&source)?;
            if as_layer {
                return Ok(Prepared::Layer {
                    name: layer_name,
                    source,
                    raster: false,
                });
            }
            let decoded = lumapaint_formats::io::read_document(
                lumapaint_formats::export::FormatId::Svg,
                name,
                &bytes,
                Default::default(),
            )
            .map_err(|e| e.to_string())?;
            let lumapaint_formats::io::ReadContent::Vector(mut document) = decoded.content else {
                return Err("Expected SVG document".into());
            };
            lumapaint_svg::attach(&mut document);
            (OpenDocumentContent::Legacy(document), None, None)
        }
        _ => {
            let mut bytes = read_bounded(&path, 3 * 1024 * 1024 - 1024)?;
            if ext == "webp" {
                let mut reader = image::ImageReader::with_format(
                    std::io::Cursor::new(bytes),
                    image::ImageFormat::WebP,
                );
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(8192);
                limits.max_image_height = Some(8192);
                limits.max_alloc = Some(64 * 1024 * 1024);
                reader.limits(limits);
                let image = reader.decode().map_err(|e| e.to_string())?;
                let mut png = std::io::Cursor::new(Vec::new());
                image
                    .write_to(&mut png, image::ImageFormat::Png)
                    .map_err(|e| e.to_string())?;
                bytes = png.into_inner();
                if bytes.len() > 3 * 1024 * 1024 - 1024 {
                    return Err("Converted image exceeds the import limit / 変換画像が読み込み上限を超えています / 转换图像超出导入限制".into());
                }
            }
            let info = lumapaint_renderer::vector::imported_raster_info(&bytes, "all")?;
            // The existing image loader handles orientation/resolution and validates dimensions.
            let document = opened_raster_document(name, bytes, info)?;
            (OpenDocumentContent::Legacy(Box::new(document)), None, None)
        }
    };
    if as_layer {
        let (source, raster) = match &content {
            OpenDocumentContent::Legacy(document) if ext != "lumapaint" => (
                document
                    .svg_layers()
                    .next()
                    .ok_or("Missing image layer")?
                    .source
                    .clone(),
                true,
            ),
            OpenDocumentContent::Legacy(document) => (
                String::from_utf8(crate::vector_export::svg(document)?.bytes)
                    .map_err(|e| e.to_string())?,
                false,
            ),
            OpenDocumentContent::Tiled(session) => (tiled_svg(&session.document)?, true),
            OpenDocumentContent::Shared(_) => {
                return Err("Unexpected shared import document".into())
            }
        };
        validate_svg(&source)?;
        Ok(Prepared::Layer {
            name: layer_name,
            source,
            raster,
        })
    } else {
        Ok(Prepared::Open(OpenDocument {
            id: 0,
            content,
            path: project_path,
            fingerprint,
        }))
    }
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("File exceeds the import limit / ファイルが読み込み容量の上限を超えています / 文件超出导入大小限制".into());
    }
    Ok(bytes)
}
fn tiled_svg(document: &TiledRasterDocument) -> Result<String, String> {
    use base64::Engine;
    use lumapaint_core::tiles::{TileCoord, TILE_SIZE};
    let (w, h) = document.dimensions();
    if u64::from(w) * u64::from(h) > 16_777_216 {
        return Err("Layer import exceeds 16 megapixels / レイヤー取り込みの上限は1600万画素です / 图层导入上限为1600万像素".into());
    }
    let mut pixels = vec![0; (w as usize) * (h as usize) * 4];
    for ty in 0..h.div_ceil(TILE_SIZE) {
        for tx in 0..w.div_ceil(TILE_SIZE) {
            let Some(tile) = document.composite_tile(TileCoord { x: tx, y: ty }) else {
                continue;
            };
            for y in 0..TILE_SIZE.min(h - ty * TILE_SIZE) {
                let length = TILE_SIZE.min(w - tx * TILE_SIZE) as usize * 4;
                let dst = (((ty * TILE_SIZE + y) * w + tx * TILE_SIZE) * 4) as usize;
                let src = (y * TILE_SIZE * 4) as usize;
                pixels[dst..dst + length].copy_from_slice(&tile[src..src + length]);
            }
        }
    }
    let png = lumapaint_renderer::vector::clipboard_png(w, h, pixels)?;
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    Ok(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\"><image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{data}\"/></svg>"))
}
pub(crate) fn check_target(target_id: Option<u64>) -> Result<(), String> {
    if let Some(id) = target_id {
        if !DOCUMENT_OPEN.with(|open| open.get())
            || ACTIVE_DOCUMENT_ID.with(|active| active.get()) != id
        {
            return Err("Drop target document changed / ドロップ先のドキュメントが変わりました / 拖放目标文档已更改".into());
        }
        window_sessions::ensure_shared_editable(id)?;
        if ACTIVE_TILED_DOCUMENT.with(|slot| slot.borrow().is_some()) {
            ensure_tiled_open()?;
        } else {
            ensure_document_open()?;
        }
    } else if raster_import::active() {
        return Err("Confirm or cancel image placement / 画像の配置を確定またはキャンセルしてください / 请确认或取消图像放置".into());
    }
    Ok(())
}
pub(crate) fn target_context(target_id: Option<u64>) -> Result<Option<([u32; 2], bool)>, String> {
    check_target(target_id)?;
    if target_id.is_none() {
        return Ok(None);
    }
    let tiled = ACTIVE_TILED_DOCUMENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|session| session.document.dimensions())
    });
    let (w, h) = tiled.unwrap_or_else(|| DOCUMENT.with(|doc| doc.borrow().dimensions()));
    Ok(Some(([w, h], tiled.is_some())))
}
pub(crate) fn fit_layer(
    prepared: Prepared,
    context: Option<([u32; 2], bool)>,
) -> Result<Prepared, String> {
    let Some(([w, h], tiled)) = context else {
        return Ok(prepared);
    };
    let Prepared::Layer {
        name,
        source,
        raster,
    } = prepared
    else {
        return Ok(prepared);
    };
    let source = &source[source.find("<svg").ok_or("Missing SVG root")?..];
    let source = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\">{source}</svg>"
    );
    validate_svg(&source)?;
    if tiled {
        if u64::from(w) * u64::from(h) > 16_777_216 {
            return Err("Layer import exceeds 16 megapixels / レイヤー取り込みの上限は1600万画素です / 图层导入上限为1600万像素".into());
        }
        let pixels = lumapaint_renderer::vector::rasterize_svg(&source, w, h)?.pixels;
        let mut tiles = lumapaint_core::tiles::SparseTiles::new(w, h)?;
        tiles.write_rect(0, 0, w, h, &pixels)?;
        Ok(Prepared::TiledLayer { name, tiles })
    } else {
        Ok(Prepared::Layer {
            name,
            source,
            raster,
        })
    }
}

pub(crate) fn apply(prepared: Prepared, target_id: Option<u64>) -> Result<(), String> {
    check_target(target_id)?;
    text_editor::finish(true)?;
    match prepared {
        Prepared::Open(mut document) => {
            document.id = next_document_id();
            park_active_document();
            activate_document(document);
        }
        Prepared::Layer {
            name,
            source,
            raster,
        } => {
            DOCUMENT.with(|doc| {
                if raster {
                    doc.borrow_mut().import_raster_layer(name, source)
                } else {
                    doc.borrow_mut().import_svg_as_layer(name, source)
                }
            })?;
        }
        Prepared::TiledLayer { name, tiles } => {
            ACTIVE_TILED_DOCUMENT.with(|slot| -> Result<(), String> {
                let mut slot = slot.borrow_mut();
                let session = slot.as_mut().ok_or("Drop target document changed")?;
                let mut serial = 1;
                while session
                    .document
                    .layers()
                    .iter()
                    .any(|layer| layer.id == format!("imported-{serial}"))
                {
                    serial += 1;
                }
                let id = format!("imported-{serial}");
                session.document.import_layer(id.clone(), name, tiles)?;
                session.selected_layer = Some(id);
                Ok(())
            })?;
        }
        Prepared::Pdf(path) => {
            crate::pdf_import::prepare(
                APP.get().ok_or("Application unavailable")?.clone(),
                current_label(),
                path,
                target_id,
            );
            return Ok(());
        }
    }
    redraw()?;
    emit_document();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"2\" height=\"2\"><rect width=\"2\" height=\"2\" fill=\"#ff0000\"/></svg>";
    fn reset_target() {
        DOCUMENT.with(|doc| *doc.borrow_mut() = Document::default());
        ACTIVE_TILED_DOCUMENT.with(|slot| *slot.borrow_mut() = None);
        DOCUMENT_OPEN.with(|open| open.set(true));
        ACTIVE_DOCUMENT_ID.with(|id| id.set(7001));
        PROJECT_PATH.with(|path| *path.borrow_mut() = Some("target.lumapaint".into()));
        INACTIVE_DOCUMENTS.with(|docs| docs.borrow_mut().clear());
    }
    #[test]
    fn svg_opens_a_new_document_or_becomes_a_separate_undoable_layer() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("日本語 image.SVG");
        std::fs::write(&path, SVG).unwrap();
        reset_target();
        apply(prepare(path.clone(), false).unwrap(), None).unwrap();
        assert_ne!(ACTIVE_DOCUMENT_ID.with(|id| id.get()), 7001);
        assert_eq!(INACTIVE_DOCUMENTS.with(|docs| docs.borrow().len()), 1);
        assert_eq!(DOCUMENT.with(|doc| doc.borrow().dimensions()), (2, 2));
        assert!(PROJECT_PATH.with(|path| path.borrow().is_none()));

        reset_target();
        DOCUMENT.with(|doc| doc.borrow_mut().add_vector_layer().unwrap());
        let before = DOCUMENT.with(|doc| doc.borrow().svg_layers().count());
        let context = target_context(Some(7001)).unwrap();
        for _ in 0..2 {
            apply(
                fit_layer(prepare(path.clone(), true).unwrap(), context).unwrap(),
                Some(7001),
            )
            .unwrap();
        }
        DOCUMENT.with(|doc| {
            let mut doc = doc.borrow_mut();
            assert_eq!(doc.svg_layers().count(), before + 2);
            assert_eq!(doc.svg_layers().last().unwrap().name, "日本語 image");
            let source = doc.svg_layers().last().unwrap().source.clone();
            doc.undo();
            assert_eq!(doc.svg_layers().count(), before + 1);
            doc.redo();
            assert_eq!(doc.svg_layers().last().unwrap().source, source);
        });
        assert_eq!(
            PROJECT_PATH.with(|path| path.borrow().clone()),
            Some("target.lumapaint".into())
        );
    }
    #[test]
    fn native_project_retains_its_save_path_when_opened_and_imports_without_switching() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native.lumapaint");
        let mut source = Document::default();
        source
            .import_svg_as_layer("Red".into(), SVG.into())
            .unwrap();
        std::fs::write(&path, source.encode().unwrap()).unwrap();
        reset_target();
        apply(prepare(path.clone(), false).unwrap(), None).unwrap();
        assert_eq!(
            PROJECT_PATH.with(|p| p.borrow().clone()),
            Some(path.clone())
        );
        assert!(PROJECT_FINGERPRINT.with(|p| p.borrow().is_some()));
        assert!(!DOCUMENT.with(|doc| doc.borrow().snapshot().dirty));
        reset_target();
        apply(
            fit_layer(
                prepare(path, true).unwrap(),
                target_context(Some(7001)).unwrap(),
            )
            .unwrap(),
            Some(7001),
        )
        .unwrap();
        assert_eq!(ACTIVE_DOCUMENT_ID.with(|id| id.get()), 7001);
        DOCUMENT.with(|doc| {
            let mut doc = doc.borrow_mut();
            assert_eq!(doc.svg_layers().count(), 1);
            doc.undo();
            assert_eq!(doc.svg_layers().count(), 0);
        });
    }
    #[test]
    fn png_preserves_alpha_and_native_size_inside_a_larger_page() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("alpha.png");
        let png =
            lumapaint_renderer::vector::clipboard_png(2, 1, vec![255, 0, 0, 128, 0, 255, 0, 255])
                .unwrap();
        std::fs::write(&path, png).unwrap();
        let prepared = fit_layer(prepare(path, true).unwrap(), Some(([4, 4], false))).unwrap();
        let Prepared::Layer { source, raster, .. } = prepared else {
            panic!("Expected layer");
        };
        assert!(raster);
        let pixels = lumapaint_renderer::vector::rasterize_svg(&source, 4, 4)
            .unwrap()
            .pixels;
        assert_eq!(pixels[3], 128);
        assert_eq!(&pixels[4..8], &[0, 255, 0, 255]);
        assert_eq!(pixels[11], 0);
        assert_eq!(pixels[19], 0);
    }
    #[test]
    fn invalid_input_and_changed_target_cannot_modify_document() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("broken.png");
        std::fs::write(&path, b"invalid image").unwrap();
        reset_target();
        let before = DOCUMENT.with(|doc| doc.borrow_mut().encode().unwrap());
        assert!(prepare(path, true).is_err());
        assert!(apply(
            Prepared::Layer {
                name: "Wrong target".into(),
                source: SVG.into(),
                raster: false
            },
            Some(7002)
        )
        .is_err());
        assert_eq!(
            before,
            DOCUMENT.with(|doc| doc.borrow_mut().encode().unwrap())
        );
        assert!(supported(Path::new("IMAGE.JPEG")));
        assert!(!supported(Path::new("script.txt")));
        assert!(prepare(directory.path().join("folder.svg"), false).is_err());
    }
    #[test]
    fn tiled_target_import_and_undo_keep_pixels_and_existing_layers() {
        reset_target();
        let mut document = TiledRasterDocument::new(4, 4).unwrap();
        document.add_layer("base".into(), "Base".into()).unwrap();
        ACTIVE_TILED_DOCUMENT.with(|slot| {
            *slot.borrow_mut() = Some(TiledSession {
                selected_layer: Some("base".into()),
                document,
                name: "Tiled".into(),
                file_name: None,
                saved_revision: None,
            })
        });
        let context = target_context(Some(7001)).unwrap();
        let prepared = Prepared::Layer {
            name: "Red".into(),
            source: SVG.into(),
            raster: false,
        };
        apply(fit_layer(prepared, context).unwrap(), Some(7001)).unwrap();
        ACTIVE_TILED_DOCUMENT.with(|slot| {
            let mut slot = slot.borrow_mut();
            let session = slot.as_mut().unwrap();
            assert_eq!(session.document.layers().len(), 2);
            assert_eq!(
                session.document.layers()[1].tiles.pixel(0, 0),
                Some([255, 0, 0, 255])
            );
            assert_eq!(
                session.document.layers()[1].tiles.pixel(3, 3),
                Some([0, 0, 0, 0])
            );
            session.document.undo().unwrap();
            assert_eq!(session.document.layers().len(), 1);
            assert_eq!(session.selected_layer_id(), "base");
            session.document.redo().unwrap();
            assert_eq!(
                session.document.layers()[1].tiles.pixel(0, 0),
                Some([255, 0, 0, 255])
            );
        });
    }
    #[test]
    fn tiled_source_flattens_edge_tiles_with_correct_stride() {
        let mut document = TiledRasterDocument::new(257, 1).unwrap();
        document
            .add_layer("source".into(), "Source".into())
            .unwrap();
        document
            .write_rect("source", [256, 0, 1, 1], &[0, 0, 255, 255])
            .unwrap();
        let source = tiled_svg(&document).unwrap();
        let pixels = lumapaint_renderer::vector::rasterize_svg(&source, 257, 1)
            .unwrap()
            .pixels;
        assert_eq!(&pixels[1024..1028], &[0, 0, 255, 255]);
        assert_eq!(pixels[1023], 0);
    }
}
