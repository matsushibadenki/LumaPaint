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
        let snapshot = doc.snapshot();
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
                layer: snapshot.layer_id,
                revision: snapshot.revision,
                sender,
                latest: None,
                preview_document,
                completed,
            })
        });
        std::thread::spawn(move || {
            let pending = Arc::new(Mutex::new(None));
            let result = paint_worker(workspace, receiver, |frame| {
                if frame.source.is_some() {
                    let _ = done.send(Ok(frame));
                } else {
                    publish(token, &pending, Ok(frame));
                }
            });
            if let Err(error) = result {
                let _ = done.send(Err(error.clone()));
                publish(token, &pending, Err(error));
            }
        });
        return Ok(true);
    }
    PIXEL_PAINT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(draft) = slot.as_mut() else {
            return Ok(false);
        };
        let snapshot = doc.snapshot();
        if snapshot.layer_id != draft.layer || snapshot.revision != draft.revision {
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
            let source = frame.source.ok_or("Missing committed paint image")?;
            doc.replace_selected_image(source.clone())?;
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
        let source = if finish {
            let (w, h) = workspace.dimensions();
            let png = lumapaint_renderer::vector::document_png(w, h, preview.pixels().to_vec())?;
            Some(clipboard::image_svg(w, h, &png))
        } else {
            None
        };
        output(PaintFrame {
            uploads: preview.take_uploads(),
            pixels: if finish {
                preview.pixels().to_vec()
            } else {
                Vec::new()
            },
            source,
        });
        if finish {
            return Ok(());
        }
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

fn publish(token: u64, pending: &PendingPreview, result: Result<PaintFrame, String>) {
    let Some(app) = APP.get() else { return };
    let schedule = queue_preview(&mut pending.lock().unwrap(), result);
    if schedule {
        let pending = Arc::clone(pending);
        let _ = app.run_on_main_thread(move || {
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
            let snapshot = doc.borrow().snapshot();
            valid = draft.document_id == ACTIVE_DOCUMENT_ID.with(|id| id.get())
                && snapshot.layer_id == draft.layer
                && snapshot.revision == draft.revision;
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
            let opacity = doc
                .svg_layers()
                .find(|layer| layer.id == draft.layer)
                .ok_or("Paint layer missing")?
                .effective_opacity();
            Ok(PreparedPixelTiles {
                id: draft.layer.clone(),
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
