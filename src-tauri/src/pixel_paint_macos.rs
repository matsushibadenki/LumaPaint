//! Pixel painting records every input sample on the main thread. Raster work and
//! PNG encoding run on a worker, so AppKit can continue delivering curved strokes.
use super::*;
use lumapaint_core::document::Point;
#[cfg(test)]
use lumapaint_formats::native::NativeDocumentCodec;
use lumapaint_renderer::pixel_paint::PixelPaintPreview;
use lumapaint_renderer::PreparedPixelTiles;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

pub(super) struct PixelPaint {
    token: u64,
    document_id: u64,
    layer: String,
    image_id: String,
    clone_stamp: bool,
    retouch: bool,
    revision: u64,
    sender: mpsc::Sender<Sample>,
    latest: Option<PreparedPixelTiles>,
    preview_document: Document,
    completed: mpsc::Receiver<Result<PaintFrame, String>>,
}

struct Sample {
    point: Point,
    pressure: f32,
    finish: bool,
}
struct PaintFrame {
    uploads: Vec<lumapaint_core::tiles::TileUpload>,
    pixels: Vec<u8>,
    source: Option<String>,
}

pub(super) fn pointer(
    doc: &mut Document,
    point: Point,
    phase: u8,
    pressure: f32,
    eraser: bool,
) -> Result<bool, String> {
    if phase == 0 {
        PIXEL_PAINT.with(|slot| slot.borrow_mut().take());
        let Some(mut workspace) = doc.selected_pixel_paint_workspace()? else {
            return Ok(false);
        };
        let brush = BRUSH.with(|brush| *brush.borrow());
        let started = if eraser {
            workspace.begin_eraser(point, brush, pressure)?
        } else {
            workspace.begin_with_pressure(point, brush, pressure)?
        };
        if !started {
            return Ok(true);
        }
        let layer_id = doc.selected_layer_id().to_owned();
        let revision = doc.revision();
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        let mut preview_document = doc.clone();
        let (w, h) = doc.dimensions();
        preview_document.replace_selected_image(preview_source(w, h, token))?;
        let (sender, receiver) = mpsc::channel();
        let (done, completed) = mpsc::channel();
        PIXEL_PAINT.with(|slot| {
            *slot.borrow_mut() = Some(PixelPaint {
                token,
                document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                image_id: layer_id.clone(),
                clone_stamp: false,
                retouch: false,
                layer: layer_id,
                revision,
                sender,
                latest: None,
                preview_document,
                completed,
            })
        });
        let label = current_label();
        std::thread::spawn(move || {
            let pending = Arc::new(Mutex::new(None));
            let result = paint_worker(workspace, receiver, |frame| {
                if frame.source.is_some() {
                    let _ = done.send(Ok(frame));
                } else {
                    publish(label.clone(), token, &pending, Ok(frame));
                }
            });
            if let Err(error) = result {
                let _ = done.send(Err(error.clone()));
                publish(label.clone(), token, &pending, Err(error));
            }
        });
        return Ok(true);
    }
    PIXEL_PAINT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(draft) = slot.as_mut() else {
            return Ok(false);
        };
        if doc.selected_layer_id() != draft.layer || doc.revision() != draft.revision {
            *slot = None;
            return Err("Paint target changed / 描画先が変更されました / 绘画目标已更改".into());
        }
        draft
            .sender
            .send(Sample {
                point,
                pressure,
                finish: phase == 2,
            })
            .map_err(|_| "Paint worker stopped / 描画処理が停止しました / 绘画处理已停止")?;
        if phase == 2 {
            // Only release waits for the final raster. All drag samples have
            // already been captured, and the edit is atomic before another tool,
            // save, undo or subsequent stroke can run.
            let frame = draft
                .completed
                .recv()
                .map_err(|_| "Paint worker stopped")??;
            let Some(source) = frame.source else {
                *slot = None;
                return Ok(true);
            };
            if draft.retouch {
                doc.replace_retouch_pixels(source.clone(), doc.selection().cloned())?;
            } else if draft.clone_stamp {
                doc.replace_moved_pixels(source.clone(), doc.selection().cloned())?;
            } else {
                doc.replace_selected_image(source.clone())?;
            }
            if draft.layer == "layer-1" {
                *slot = None;
                return Ok(true);
            }
            let layer = doc
                .svg_layers()
                .find(|layer| layer.id == draft.layer)
                .ok_or("Paint layer missing")?;
            let prepared = prepare_frame(
                draft.layer.clone(),
                source,
                doc.dimensions(),
                layer.effective_opacity(),
                frame.pixels,
            );
            PIXEL_PAINT_COMMIT.with(|pending| *pending.borrow_mut() = Some(prepared));
            *slot = None;
        }
        Ok(true)
    })
}

pub(super) fn clone_pointer(
    doc: &mut Document,
    point: Point,
    phase: u8,
    pressure: f32,
    offset: [f32; 2],
    settings: lumapaint_core::clone_stamp::Settings,
) -> Result<bool, String> {
    if phase != 0 {
        return pointer(doc, point, phase, pressure, false);
    }
    PIXEL_PAINT.with(|slot| slot.borrow_mut().take());
    doc.finish();
    let workspace = doc.clone_stamp_workspace()?;
    let sample_doc = doc.clone_stamp_sampling_document(settings.sample)?;
    let brush = BRUSH.with(|brush| *brush.borrow());
    let layer_id = doc.selected_layer_id().to_owned();
    let revision = doc.revision();
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let (w, h) = doc.dimensions();
    let preview_document = doc.clone_stamp_preview_document(preview_source(w, h, token))?;
    let image_id = if layer_id == "layer-1" {
        "clone-stamp-preview-base".into()
    } else {
        layer_id.clone()
    };
    let (sender, receiver) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    PIXEL_PAINT.with(|slot| {
        *slot.borrow_mut() = Some(PixelPaint {
            token,
            document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
            image_id,
            clone_stamp: true,
            retouch: false,
            layer: layer_id,
            revision,
            sender,
            latest: None,
            preview_document,
            completed,
        })
    });
    let label = current_label();
    std::thread::spawn(move || {
        let pending = Arc::new(Mutex::new(None));
        let result = (|| -> Result<(), String> {
            let mut destination = PixelPaintPreview::new((w, h))?;
            destination.update(&workspace)?;
            let source = if settings.sample == lumapaint_core::clone_stamp::Sample::CurrentLayer {
                destination.pixels().to_vec()
            } else {
                lumapaint_renderer::thumbnails::document_pixels(&sample_doc)?
            };
            let mut preview = lumapaint_renderer::clone_stamp::Preview::new(
                (w, h),
                destination.pixels().to_vec(),
                source,
                offset,
                brush,
                settings,
                workspace.selection().cloned(),
            )?;
            let mut uploads = destination.take_uploads();
            preview.sample(point, pressure)?;
            loop {
                uploads = merge_uploads(uploads, preview.take_uploads());
                publish(
                    label.clone(),
                    token,
                    &pending,
                    Ok(PaintFrame {
                        uploads: std::mem::take(&mut uploads),
                        pixels: Vec::new(),
                        source: None,
                    }),
                );
                let Ok(mut sample) = receiver.recv() else {
                    return Ok(());
                };
                preview.sample(sample.point, sample.pressure)?;
                while !sample.finish {
                    match receiver.try_recv() {
                        Ok(next) => {
                            sample = next;
                            preview.sample(sample.point, sample.pressure)?;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                    }
                }
                if sample.finish {
                    let pixels = preview.pixels_after_update();
                    let png = lumapaint_renderer::vector::document_png_ref(w, h, &pixels)?;
                    let _ = done.send(Ok(PaintFrame {
                        uploads: Vec::new(),
                        pixels,
                        source: Some(clipboard::image_svg(w, h, &png)),
                    }));
                    return Ok(());
                }
            }
        })();
        if let Err(error) = result {
            let _ = done.send(Err(error.clone()));
            publish(label, token, &pending, Err(error));
        }
    });
    Ok(true)
}

pub(super) fn retouch_pointer(
    doc: &mut Document,
    point: Point,
    phase: u8,
    pressure: f32,
    kind: lumapaint_core::retouch::Kind,
    settings: lumapaint_core::retouch::Settings,
) -> Result<bool, String> {
    if phase != 0 {
        return pointer(doc, point, phase, pressure, false);
    }
    if settings.strength == 0. {
        return Ok(true);
    }
    PIXEL_PAINT.with(|slot| slot.borrow_mut().take());
    doc.finish();
    let workspace = doc.retouch_workspace()?;
    let sample_doc = if settings.sample_all_layers {
        doc.clone_stamp_sampling_document(lumapaint_core::clone_stamp::Sample::AllLayers)?
    } else {
        workspace.clone()
    };
    let brush = BRUSH.with(|brush| *brush.borrow());
    let layer_id = doc.selected_layer_id().to_owned();
    let revision = doc.revision();
    let alpha_locked = doc.selected_layer_alpha_locked();
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let (w, h) = doc.dimensions();
    let preview_document = doc.retouch_preview_document(preview_source(w, h, token))?;
    let image_id = if layer_id == "layer-1" {
        "clone-stamp-preview-base".into()
    } else {
        layer_id.clone()
    };
    let (sender, receiver) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    PIXEL_PAINT.with(|slot| {
        *slot.borrow_mut() = Some(PixelPaint {
            token,
            document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
            image_id,
            clone_stamp: true,
            retouch: true,
            layer: layer_id,
            revision,
            sender,
            latest: None,
            preview_document,
            completed,
        })
    });
    let label = current_label();
    std::thread::spawn(move || {
        let pending = Arc::new(Mutex::new(None));
        let result = (|| -> Result<(), String> {
            let mut destination = PixelPaintPreview::new((w, h))?;
            destination.update(&workspace)?;
            let source = if !settings.sample_all_layers {
                destination.pixels().to_vec()
            } else {
                lumapaint_renderer::thumbnails::document_pixels(&sample_doc)?
            };
            let original = destination.pixels().to_vec();
            let mut preview = lumapaint_renderer::retouch::Preview::new(
                (w, h),
                destination.pixels().to_vec(),
                source,
                brush,
                settings,
                kind,
                workspace.selection().cloned(),
            )?
            .with_alpha_lock(alpha_locked);
            let mut uploads = destination.take_uploads();
            preview.sample(point, pressure)?;
            loop {
                uploads = merge_uploads(uploads, preview.take_uploads());
                publish(
                    label.clone(),
                    token,
                    &pending,
                    Ok(PaintFrame {
                        uploads: std::mem::take(&mut uploads),
                        pixels: Vec::new(),
                        source: None,
                    }),
                );
                let Ok(mut sample) = receiver.recv() else {
                    return Ok(());
                };
                preview.sample(sample.point, sample.pressure)?;
                while !sample.finish {
                    match receiver.try_recv() {
                        Ok(next) => {
                            sample = next;
                            preview.sample(sample.point, sample.pressure)?;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                    }
                }
                if sample.finish {
                    let pixels = preview.pixels_after_update();
                    if pixels == original {
                        let _ = done.send(Ok(PaintFrame {
                            uploads: Vec::new(),
                            pixels,
                            source: None,
                        }));
                        return Ok(());
                    }
                    let png = lumapaint_renderer::vector::document_png_ref(w, h, &pixels)?;
                    let _ = done.send(Ok(PaintFrame {
                        uploads: Vec::new(),
                        pixels,
                        source: Some(clipboard::image_svg(w, h, &png)),
                    }));
                    return Ok(());
                }
            }
        })();
        if let Err(error) = result {
            let _ = done.send(Err(error.clone()));
            publish(label, token, &pending, Err(error));
        }
    });
    Ok(true)
}

pub(super) fn fill_pointer(
    doc: &mut Document,
    point: Point,
    phase: u8,
    settings: lumapaint_core::paint_bucket::Settings,
) -> Result<bool, String> {
    if phase == 1 {
        return Ok(true);
    }
    if phase == 2 {
        return pointer(doc, point, phase, 1., false);
    }
    if BRUSH.with(|brush| brush.borrow().no_color) {
        return Ok(true);
    }
    PIXEL_PAINT.with(|slot| slot.borrow_mut().take());
    doc.finish();
    let workspace = doc.retouch_workspace()?;
    let sample_doc = if settings.all_layers {
        Some(doc.clone_stamp_sampling_document(lumapaint_core::clone_stamp::Sample::AllLayers)?)
    } else {
        None
    };
    let brush = BRUSH.with(|brush| *brush.borrow());
    let layer_id = doc.selected_layer_id().to_owned();
    let revision = doc.revision();
    let alpha_locked = doc.selected_layer_alpha_locked();
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let (w, h) = doc.dimensions();
    let preview_document = doc.retouch_preview_document(preview_source(w, h, token))?;
    let image_id = if layer_id == "layer-1" {
        "clone-stamp-preview-base".into()
    } else {
        layer_id.clone()
    };
    let (sender, receiver) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    PIXEL_PAINT.with(|slot| {
        *slot.borrow_mut() = Some(PixelPaint {
            token,
            document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
            image_id,
            clone_stamp: true,
            retouch: true,
            layer: layer_id,
            revision,
            sender,
            latest: None,
            preview_document,
            completed,
        })
    });

    let label = current_label();
    std::thread::spawn(move || {
        let pending = Arc::new(Mutex::new(None));
        let result = (|| -> Result<(), String> {
            let mut destination = PixelPaintPreview::new((w, h))?;
            destination.update(&workspace)?;
            let source = if let Some(doc) = sample_doc {
                lumapaint_renderer::thumbnails::document_pixels(&doc)?
            } else {
                destination.pixels().to_vec()
            };
            let fill = if alpha_locked {
                lumapaint_renderer::paint_bucket::fill_alpha_locked
            } else {
                lumapaint_renderer::paint_bucket::fill
            };
            let result = fill(
                (w, h),
                destination.pixels().to_vec(),
                &source,
                point,
                brush.color,
                settings,
                workspace.selection(),
            )?;
            let uploads = merge_uploads(destination.take_uploads(), result.uploads);
            publish(
                label.clone(),
                token,
                &pending,
                Ok(PaintFrame {
                    uploads,
                    pixels: Vec::new(),
                    source: None,
                }),
            );
            let source = if result.changed {
                let png = lumapaint_renderer::vector::document_png_ref(w, h, &result.pixels)?;
                Some(clipboard::image_svg(w, h, &png))
            } else {
                None
            };
            while let Ok(sample) = receiver.recv() {
                if sample.finish {
                    let _ = done.send(Ok(PaintFrame {
                        uploads: Vec::new(),
                        pixels: result.pixels,
                        source,
                    }));
                    return Ok(());
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = done.send(Err(error.clone()));
            publish(label, token, &pending, Err(error));
        }
    });
    Ok(true)
}

fn paint_worker(
    mut workspace: Document,
    receiver: mpsc::Receiver<Sample>,
    mut output: impl FnMut(PaintFrame),
) -> Result<(), String> {
    let mut preview = PixelPaintPreview::new(workspace.dimensions())?;
    let mut finish = false;
    let mut first = true;
    loop {
        // Input is never coalesced: batch all queued points into the stroke,
        // coalescing only the expensive preview renders.
        if !first {
            let Ok(sample) = receiver.recv() else {
                return Ok(());
            };
            workspace.extend_with_pressure(sample.point, sample.pressure)?;
            finish = sample.finish;
        }
        first = false;
        while !finish {
            match receiver.try_recv() {
                Ok(sample) => {
                    workspace.extend_with_pressure(sample.point, sample.pressure)?;
                    finish = sample.finish;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        preview.update(&workspace)?;
        if finish {
            let (w, h) = workspace.dimensions();
            let (pixels, uploads) = preview.into_parts();
            let png = lumapaint_renderer::vector::document_png_ref(w, h, &pixels)?;
            output(PaintFrame {
                uploads,
                pixels,
                source: Some(clipboard::image_svg(w, h, &png)),
            });
            return Ok(());
        }
        output(PaintFrame {
            uploads: preview.take_uploads(),
            pixels: Vec::new(),
            source: None,
        });
    }
}

type PendingPreview = Arc<Mutex<Option<Result<PaintFrame, String>>>>;

// Each tile is a complete replacement. Keep the newest version of every dirty
// tile so skipping intermediate presentations cannot lose parts of the stroke.
fn merge_uploads(
    previous: Vec<lumapaint_core::tiles::TileUpload>,
    current: Vec<lumapaint_core::tiles::TileUpload>,
) -> Vec<lumapaint_core::tiles::TileUpload> {
    let mut tiles = std::collections::BTreeMap::new();
    for upload in previous.into_iter().chain(current) {
        tiles.insert(upload.coord, upload);
    }
    tiles.into_values().collect()
}

fn queue_preview(
    slot: &mut Option<Result<PaintFrame, String>>,
    mut result: Result<PaintFrame, String>,
) -> bool {
    let schedule = slot.is_none();
    if let Some(Ok(previous)) = slot.take() {
        if let Ok(current) = &mut result {
            current.uploads = merge_uploads(previous.uploads, std::mem::take(&mut current.uploads));
        }
    }
    *slot = Some(result);
    schedule
}

fn publish(
    label: String,
    token: u64,
    pending: &PendingPreview,
    result: Result<PaintFrame, String>,
) {
    let Some(app) = APP.get() else { return };
    let schedule = queue_preview(&mut pending.lock().unwrap(), result);
    if schedule {
        let pending = Arc::clone(pending);
        let _ = app.run_on_main_thread(move || {
            let Ok(_session) = SessionGuard::enter(&label) else {
                return;
            };
            let result = pending.lock().unwrap().take();
            if let Some(result) = result {
                receive_frame(token, result);
            }
        });
    }
}

fn receive_frame(token: u64, result: Result<PaintFrame, String>) {
    let handled = PIXEL_PAINT.with(|slot| -> Result<bool, String> {
        let mut slot = slot.borrow_mut();
        let Some(draft) = slot.as_mut().filter(|draft| draft.token == token) else {
            return Ok(false);
        };
        let mut valid = false;
        DOCUMENT.with(|doc| {
            let doc = doc.borrow();
            valid = draft.document_id == ACTIVE_DOCUMENT_ID.with(|id| id.get())
                && doc.selected_layer_id() == draft.layer
                && doc.revision() == draft.revision;
        });
        if !valid {
            *slot = None;
            return Ok(false);
        }
        let frame = match result {
            Ok(frame) => frame,
            Err(error) => {
                *slot = None;
                return Err(error);
            }
        };
        let mut prepared = DOCUMENT.with(|doc| -> Result<PreparedPixelTiles, String> {
            let doc = doc.borrow();
            let (w, h) = doc.dimensions();
            let source = preview_source(w, h, token);
            let opacity = draft
                .preview_document
                .svg_layers()
                .find(|layer| layer.id == draft.image_id)
                .ok_or("Paint layer missing")?
                .effective_opacity();
            Ok(PreparedPixelTiles {
                id: draft.image_id.clone(),
                source,
                size: (w, h),
                opacity,
                uploads: frame.uploads,
            })
        })?;
        if let Some(previous) = draft.latest.take() {
            prepared.uploads = merge_uploads(previous.uploads, prepared.uploads);
        }
        draft.latest = Some(prepared);
        Ok(true)
    });
    match handled {
        Ok(true) => {
            if let Err(error) = redraw() {
                emit_error(error);
            }
        }
        Ok(false) => {}
        Err(error) => {
            PIXEL_PAINT.with(|slot| slot.borrow_mut().take());
            emit_error(error);
            let _ = redraw();
        }
    }
}

fn prepare_frame(
    id: String,
    source: String,
    size: (u32, u32),
    opacity: f32,
    mut pixels: Vec<u8>,
) -> PreparedSvgLayer {
    if opacity != 1.0 {
        for value in &mut pixels {
            *value = (f32::from(*value) * opacity).round() as u8;
        }
    }
    PreparedSvgLayer {
        id,
        source,
        opacity,
        size,
        fully_contained: true,
        pixels,
    }
}

fn preview_source(w: u32, h: u32, token: u64) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\"><!-- pixel paint {token} --></svg>")
}

pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    PIXEL_PAINT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let draft = slot.as_mut()?;
        Some((|| {
            // Keep the last complete frame while the worker prepares the next.
            let Some(prepared) = draft.latest.take() else {
                return Ok(());
            };
            canvas.renderer.install_pixel_tiles(prepared)?;
            canvas
                .renderer
                .render(canvas.viewport, &draft.preview_document)
        })())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a3_pixel_layer_supports_paint_move_save_and_undo() {
        use lumapaint_core::document::{CanvasColor, DocumentSettings, DocumentUnit};
        let (w, h) = (3508, 4961); // A3 at 300 dpi, larger than the clipboard limit.
        let mut doc = Document::default();
        doc.set_document_settings(DocumentSettings {
            name: "A3".into(),
            width: w,
            height: h,
            unit: DocumentUnit::Millimeters,
            resolution: 300,
            artboards: false,
            canvas_color: CanvasColor::White,
            pixel_aspect_ratio: 1.,
        })
        .unwrap();
        let id = doc.add_paint_layer().unwrap();
        doc.select_layer(id.clone()).unwrap();
        let empty_source = doc
            .svg_layers()
            .find(|l| l.id == id)
            .unwrap()
            .source
            .clone();
        let (rw, rh, empty) = clipboard::raw_selected_pixels(&doc).unwrap();
        assert_eq!((rw, rh), (w, h));
        assert!(empty.iter().all(|v| *v == 0));
        drop(empty);
        let point = Point {
            x: (w - 60) as f32,
            y: (h - 80) as f32,
        };
        let mut workspace = doc.selected_pixel_paint_workspace().unwrap().unwrap();
        workspace
            .begin(
                point,
                Brush {
                    size: 20.,
                    hardness: 1.,
                    ..Default::default()
                },
            )
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Sample {
                point,
                pressure: 1.,
                finish: true,
            })
            .unwrap();
        let mut frame = None;
        paint_worker(workspace, receiver, |value| frame = Some(value)).unwrap();
        let frame = frame.unwrap();
        let offset = (((h - 80) * w + w - 60) * 4 + 3) as usize;
        assert!(frame.pixels[offset] > 0);
        doc.replace_selected_image(frame.source.unwrap()).unwrap();
        drop(frame.pixels);
        let painted_source = doc
            .svg_layers()
            .find(|l| l.id == id)
            .unwrap()
            .source
            .clone();
        // Exercise the same path that previously reported "Clipboard image is too large".
        super::super::pixel_move::pointer(&mut doc, point, 0, NSEventModifierFlags::empty())
            .unwrap();
        super::super::pixel_move::pointer(
            &mut doc,
            Point {
                x: point.x - 30.,
                y: point.y,
            },
            2,
            NSEventModifierFlags::empty(),
        )
        .unwrap();
        let pixels = clipboard::raw_selected_pixels(&doc).unwrap().2;
        assert_eq!(pixels[offset], 0);
        assert!(pixels[offset - 30 * 4] > 0);
        drop(pixels);
        let restored = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(restored.dimensions(), (w, h));
        assert_eq!(
            restored.svg_layers().find(|l| l.id == id).unwrap().source,
            doc.svg_layers().find(|l| l.id == id).unwrap().source
        );
        doc.undo();
        assert_eq!(
            doc.svg_layers().find(|l| l.id == id).unwrap().source,
            painted_source
        );
        doc.undo();
        assert_eq!(
            doc.svg_layers().find(|l| l.id == id).unwrap().source,
            empty_source
        );
    }

    #[test]
    fn pending_previews_merge_latest_tiles_into_one_callback() {
        use lumapaint_core::tiles::{TileCoord, TileUpload};
        let frame = |x, value| PaintFrame {
            uploads: vec![TileUpload {
                coord: TileCoord { x, y: 0 },
                origin: [x * 256, 0],
                extent: [256, 256],
                bytes_per_row: 1024,
                pixels: vec![value; 256 * 256 * 4],
            }],
            pixels: Vec::new(),
            source: None,
        };
        let mut pending = None;
        assert!(queue_preview(&mut pending, Ok(frame(0, 128))));
        assert!(!queue_preview(&mut pending, Ok(frame(1, 255))));
        assert!(!queue_preview(&mut pending, Ok(frame(0, 0))));
        let latest = pending.take().unwrap().unwrap();
        assert_eq!(latest.uploads.len(), 2);
        assert_eq!(
            latest.uploads[0].pixels[0], 0,
            "erase must replace earlier tile"
        );
        assert_eq!(
            latest.uploads[1].pixels[0], 255,
            "skipped frame's other tile must survive"
        );
        assert!(queue_preview(&mut pending, Ok(frame(2, 64))));
        assert!(!queue_preview(&mut pending, Err("worker error".into())));
        assert!(pending.unwrap().is_err());
    }

    #[test]
    fn worker_preserves_queued_curved_samples_and_committed_pixels() {
        let mut doc = Document::default();
        let id = doc.add_paint_layer().unwrap();
        doc.select_layer(id.clone()).unwrap();
        let mut workspace = doc.selected_pixel_paint_workspace().unwrap().unwrap();
        workspace
            .begin(
                Point { x: 30., y: 30. },
                Brush {
                    size: 10.,
                    hardness: 1.,
                    ..Default::default()
                },
            )
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        // Queue an L-shaped trajectory before any expensive raster work starts.
        for (point, finish) in [
            (Point { x: 30., y: 100. }, false),
            (Point { x: 30., y: 200. }, false),
            (Point { x: 100., y: 200. }, false),
            (Point { x: 200., y: 200. }, true),
        ] {
            sender
                .send(Sample {
                    point,
                    pressure: 1.,
                    finish,
                })
                .unwrap();
        }
        let before = doc.encode().unwrap();
        let mut final_frame = None;
        paint_worker(workspace, receiver, |frame| final_frame = Some(frame)).unwrap();
        assert_eq!(doc.encode().unwrap(), before);
        let frame = final_frame.unwrap();
        let alpha = |x: usize, y: usize| frame.pixels[(y * 960 + x) * 4 + 3];
        assert!(alpha(30, 100) > 0);
        assert!(alpha(100, 200) > 0);
        assert_eq!(
            alpha(115, 115),
            0,
            "must not replace the curve with a start-to-end chord"
        );
        doc.replace_selected_image(frame.source.unwrap()).unwrap();
        assert_eq!(
            clipboard::raw_selected_pixels(&doc).unwrap().2,
            frame.pixels
        );
        assert_eq!(doc.committed_paint_strokes().count(), 0);
        doc.undo();
        assert!(clipboard::raw_selected_pixels(&doc)
            .unwrap()
            .2
            .iter()
            .all(|v| *v == 0));
        doc.redo();
        assert_eq!(
            clipboard::raw_selected_pixels(&doc).unwrap().2,
            frame.pixels
        );
        let restored = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            restored.svg_layers().find(|l| l.id == id).unwrap().source,
            doc.svg_layers().find(|l| l.id == id).unwrap().source
        );
    }

    #[test]
    fn native_pointer_captures_all_samples_and_commits_eraser_atomically() {
        let mut doc = Document::default();
        let id = doc.add_paint_layer().unwrap();
        doc.select_layer(id).unwrap();
        assert!(doc
            .selected_pixel_paint_workspace()
            .unwrap()
            .unwrap()
            .paint_source()
            .is_none());
        BRUSH.with(|b| {
            *b.borrow_mut() = Brush {
                size: 10.,
                hardness: 1.,
                ..Default::default()
            }
        });
        let start = Point { x: 40., y: 40. };
        pointer(&mut doc, start, 0, 1., false).unwrap();
        assert!(paint_gesture_active());
        pointer(&mut doc, Point { x: 40., y: 100. }, 1, 1., false).unwrap();
        pointer(&mut doc, Point { x: 40., y: 180. }, 1, 1., false).unwrap();
        pointer(&mut doc, Point { x: 180., y: 180. }, 2, 1., false).unwrap();
        assert!(!paint_gesture_active());
        let ready = PIXEL_PAINT_COMMIT.with(|s| s.borrow_mut().take()).unwrap();
        assert_eq!(ready.source, doc.svg_layers().next().unwrap().source);
        assert!(ready.pixels[(100 * 960 + 40) * 4 + 3] > 0);
        assert_eq!(ready.pixels[(110 * 960 + 110) * 4 + 3], 0);
        let painted = ready.pixels;
        pointer(&mut doc, start, 0, 1., true).unwrap();
        pointer(&mut doc, Point { x: 40., y: 100. }, 2, 1., true).unwrap();
        let erased = PIXEL_PAINT_COMMIT.with(|s| s.borrow_mut().take()).unwrap();
        assert_eq!(erased.pixels[(70 * 960 + 40) * 4 + 3], 0);
        doc.undo();
        assert_eq!(clipboard::raw_selected_pixels(&doc).unwrap().2, painted);
    }

    #[test]
    fn retouch_pointer_commits_each_tool_once_and_preserves_noop_history() {
        use lumapaint_core::retouch::{Kind, Settings};
        for kind in [Kind::Blur, Kind::Sharpen, Kind::Smudge] {
            let mut doc = Document::default();
            let id = doc.add_paint_layer().unwrap();
            doc.select_layer(id).unwrap();
            let (w, h) = doc.dimensions();
            let pixels: Vec<u8> = (0..w * h)
                .flat_map(|i| {
                    let value = if i % w < 32 { 64 } else { 192 };
                    [value, value, value, 255]
                })
                .collect();
            let png = lumapaint_renderer::vector::document_png(w, h, pixels.clone()).unwrap();
            doc.replace_selected_image(clipboard::image_svg(w, h, &png))
                .unwrap();
            BRUSH.with(|b| {
                *b.borrow_mut() = Brush {
                    size: 12.,
                    hardness: 1.,
                    ..Brush::default()
                }
            });
            let settings = Settings {
                strength: 1.,
                pressure_strength: false,
                ..Settings::default()
            };
            let source = doc.svg_layers().next().unwrap().source.clone();
            retouch_pointer(&mut doc, Point { x: 30., y: 30. }, 0, 1., kind, settings).unwrap();
            retouch_pointer(&mut doc, Point { x: 34., y: 30. }, 1, 1., kind, settings).unwrap();
            assert_eq!(doc.svg_layers().next().unwrap().source, source);
            retouch_pointer(&mut doc, Point { x: 38., y: 30. }, 2, 1., kind, settings).unwrap();
            let after = clipboard::raw_selected_pixels(&doc).unwrap().2;
            assert_ne!(after, pixels);
            PIXEL_PAINT_COMMIT.with(|s| s.borrow_mut().take());
            doc.undo();
            assert_eq!(clipboard::raw_selected_pixels(&doc).unwrap().2, pixels);
            doc.redo();
            assert_eq!(clipboard::raw_selected_pixels(&doc).unwrap().2, after);
            let revision = doc.revision();
            let no_op = Settings {
                strength: 0.,
                ..settings
            };
            retouch_pointer(&mut doc, Point { x: 32., y: 30. }, 0, 1., kind, no_op).unwrap();
            retouch_pointer(&mut doc, Point { x: 32., y: 30. }, 2, 1., kind, no_op).unwrap();
            assert_eq!(doc.revision(), revision);
        }
    }

    #[test]
    fn retouch_alpha_locked_base_and_image_targets_keep_alpha_and_one_undo() {
        use lumapaint_core::retouch::{Kind, Settings};
        for base in [true, false] {
            for kind in [Kind::Blur, Kind::Sharpen, Kind::Smudge] {
                let mut doc = Document::default();
                if !base {
                    let id = doc.add_paint_layer().unwrap();
                    doc.select_layer(id).unwrap();
                }
                let (w, h) = doc.dimensions();
                let pixels: Vec<u8> = (0..w * h)
                    .flat_map(|i| {
                        let a = [0u8, 64, 128, 255][(i % w % 4) as usize];
                        let value = if i % w < 34 { 64u16 } else { 192 };
                        let v = (value * u16::from(a) / 255) as u8;
                        [v, v, v, a]
                    })
                    .collect();
                let png = lumapaint_renderer::vector::document_png(w, h, pixels).unwrap();
                doc.replace_moved_pixels(clipboard::image_svg(w, h, &png), None)
                    .unwrap();
                let id = doc.snapshot().layer_id;
                doc.set_layer_settings(LayerSettings {
                    id: id.clone(),
                    name: "Alpha locked".into(),
                    opacity: 1.,
                    locked: false,
                    alpha_locked: true,
                    mask_enabled: false,
                    mask_inverted: false,
                    mask_density: 1.,
                })
                .unwrap();
                let before = clipboard::raw_selected_pixels(&doc).unwrap().2;
                BRUSH.with(|b| {
                    *b.borrow_mut() = Brush {
                        size: 12.,
                        hardness: 1.,
                        ..Brush::default()
                    }
                });
                let settings = Settings {
                    strength: 1.,
                    pressure_strength: false,
                    finger_painting: true,
                    ..Settings::default()
                };
                retouch_pointer(&mut doc, Point { x: 30., y: 30. }, 0, 1., kind, settings).unwrap();
                retouch_pointer(&mut doc, Point { x: 38., y: 30. }, 2, 1., kind, settings).unwrap();
                PIXEL_PAINT_COMMIT.with(|slot| slot.borrow_mut().take());
                let after = clipboard::raw_selected_pixels(&doc).unwrap().2;
                assert!(after != before, "base={base}, kind={kind:?}");
                for (a, b) in after
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(before.as_chunks::<4>().0)
                {
                    assert_eq!(a[3], b[3]);
                    if b[3] == 0 {
                        assert_eq!(a, b);
                    }
                }
                assert!(
                    doc.snapshot()
                        .layers
                        .iter()
                        .find(|l| l.id == id)
                        .unwrap()
                        .alpha_locked
                );
                doc.undo();
                assert!(clipboard::raw_selected_pixels(&doc).unwrap().2 == before);
                doc.redo();
                assert!(clipboard::raw_selected_pixels(&doc).unwrap().2 == after);
                let mut decoded = Document::decode(&doc.encode().unwrap()).unwrap();
                decoded.select_layer(id.clone()).unwrap();
                assert!(
                    decoded
                        .snapshot()
                        .layers
                        .iter()
                        .find(|l| l.id == id)
                        .unwrap()
                        .alpha_locked
                );
                assert_eq!(clipboard::raw_selected_pixels(&decoded).unwrap().2, after);
            }
        }
    }

    #[test]
    fn disconnected_worker_does_not_commit_and_stale_results_are_ignored() {
        let mut workspace = Document::default();
        workspace
            .begin(Point { x: 10., y: 10. }, Brush::default())
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        let mut frames = 0;
        paint_worker(workspace, receiver, |_| frames += 1).unwrap();
        assert_eq!(frames, 0);
        receive_frame(u64::MAX, Err("stale".into()));
        assert!(PIXEL_PAINT.with(|slot| slot.borrow().is_none()));
    }
}
