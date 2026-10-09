use super::*;
use lumapaint_core::{document::Point, layer_mask::LayerEditTarget};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
const EDIT_HELP:&str="Use Brush or Eraser on a pixel channel / ピクセルのチャンネルはブラシ・消しゴムで編集してください / 请用画笔或橡皮擦编辑像素通道";
static TOKEN: AtomicU64 = AtomicU64::new(1);
struct Sample {
    point: Point,
    pressure: f32,
    finish: bool,
}
pub(super) struct ChannelGesture {
    token: u64,
    document_id: u64,
    revision: u64,
    layer: String,
    channel: u32,
    sender: mpsc::Sender<Sample>,
    finished: mpsc::Receiver<Result<String, String>>,
    preview: Option<Document>,
}
pub(super) fn select(channel: u32) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    finish_open_pen()?;
    DOCUMENT.with(|d| d.borrow_mut().select_channel(channel))?;
    cancel_vector_drag();
    CANVAS.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            c.renderer.channel = channel;
        }
    });
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
fn color_channel() -> Option<u32> {
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return None;
    }
    DOCUMENT.with(|d| {
        let d = d.borrow();
        (d.editing_channel() != 0 && d.layer_edit_target() == LayerEditTarget::Content)
            .then_some(d.editing_channel())
    })
}
impl ChannelGesture {
    fn valid(&self) -> bool {
        self.document_id == ACTIVE_DOCUMENT_ID.with(|id| id.get())
            && DOCUMENT.with(|d| {
                let d = d.borrow();
                d.revision() == self.revision
                    && d.selected_layer_id() == self.layer
                    && d.editing_channel() == self.channel
                    && d.layer_edit_target() == LayerEditTarget::Content
            })
    }
}
pub(super) fn pointer(point: Point, phase: u8, pressure: f32) -> Result<bool, String> {
    let Some(channel) = color_channel() else {
        return Ok(false);
    };
    let tool = TOOL.with(|t| t.get());
    if matches!(
        tool,
        CanvasTool::Hand
            | CanvasTool::ZoomIn
            | CanvasTool::ZoomOut
            | CanvasTool::Eyedropper
            | CanvasTool::Rectangle
            | CanvasTool::Ellipse
            | CanvasTool::Lasso
            | CanvasTool::PolygonLasso
            | CanvasTool::MagneticLasso
            | CanvasTool::SelectionBrush
    ) {
        return Ok(false);
    }
    if phase == 3 {
        return Ok(true);
    }
    if !matches!(tool, CanvasTool::Brush | CanvasTool::Eraser) || channel > 4 {
        return if phase == 0 {
            Err(EDIT_HELP.into())
        } else {
            Ok(true)
        };
    }
    if phase == 0 {
        cancel_vector_drag();
        let brush = BRUSH.with(|b| *b.borrow());
        if brush.no_color && tool != CanvasTool::Eraser {
            return Ok(true);
        }
        let (workspace, mut stroke, layer, revision) = DOCUMENT.with(|d| -> Result<_, String> {
            let mut d = d.borrow_mut();
            d.finish();
            let source = if channel == 4 {
                d.clone_stamp_workspace()?
            } else {
                d.retouch_workspace()?
            };
            Ok((
                source,
                d.channel_brush_workspace(),
                d.selected_layer_id().to_owned(),
                d.revision(),
            ))
        })?;
        let value = if tool == CanvasTool::Eraser {
            0
        } else {
            (0.2126 * brush.color[0] as f32
                + 0.7152 * brush.color[1] as f32
                + 0.0722 * brush.color[2] as f32)
                .round() as u8
        };
        stroke.begin_with_pressure(
            point,
            Brush {
                color: [255; 3],
                no_color: false,
                ..brush
            },
            pressure,
        )?;
        let token = TOKEN.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        CHANNEL_GESTURE.with(|s| {
            *s.borrow_mut() = Some(ChannelGesture {
                token,
                document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                revision,
                layer,
                channel,
                sender: tx,
                finished,
                preview: None,
            })
        });
        let label = current_label();
        std::thread::spawn(move || {
            let pending = Arc::new(Mutex::new(None));
            let result = worker(workspace, stroke, channel, value, rx, |source| {
                publish(label.clone(), token, &pending, source)
            });
            let _ = done.send(result);
        });
        if CANVAS.with(|c| c.borrow().is_some()) {
            begin_precise_paint_input();
        }
    } else {
        CHANNEL_GESTURE.with(|s|->Result<(),String>{let mut s=s.borrow_mut();let Some(d)=s.as_ref()else{return Ok(());};if !d.valid(){*s=None;return Err("Channel target changed / チャンネルの編集先が変更されました / 通道编辑目标已更改".into());}d.sender.send(Sample{point,pressure,finish:phase==2}).map_err(|_|"Channel worker stopped")?;Ok(())})?;
    }
    if phase == 2 {
        if let Some(d) = CHANNEL_GESTURE.with(|s| s.borrow_mut().take()) {
            let source = d.finished.recv().map_err(|_| "Channel worker stopped")??;
            if !d.valid() {
                return Err("Channel target changed".into());
            }
            DOCUMENT.with(|doc| {
                let mut doc = doc.borrow_mut();
                let selection = doc.selection().cloned();
                if channel == 4 {
                    doc.replace_moved_pixels(source, selection)
                } else {
                    doc.replace_retouch_pixels(source, selection)
                }
            })?;
            emit_document();
        }
    }
    Ok(true)
}
fn worker(
    workspace: Document,
    mut stroke: Document,
    channel: u32,
    value: u8,
    receiver: mpsc::Receiver<Sample>,
    mut output: impl FnMut(String),
) -> Result<String, String> {
    let size = workspace.dimensions();
    let mut raw = lumapaint_renderer::pixel_paint::PixelPaintPreview::new(size)?;
    raw.update(&workspace)?;
    let (pixels, _) = raw.into_parts();
    let mut paint =
        lumapaint_renderer::channel_paint::ChannelPaint::new(size, pixels, channel, value)?;
    let mut first = true;
    let mut finish = false;
    loop {
        if !first {
            let Ok(s) = receiver.recv() else {
                return Err("Channel stroke cancelled".into());
            };
            stroke.extend_with_pressure(s.point, s.pressure)?;
            finish = s.finish;
        }
        first = false;
        while !finish {
            match receiver.try_recv() {
                Ok(s) => {
                    stroke.extend_with_pressure(s.point, s.pressure)?;
                    finish = s.finish;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("Channel stroke cancelled".into())
                }
            }
        }
        paint.update(&stroke)?;
        let png = lumapaint_renderer::vector::document_png_ref(size.0, size.1, paint.pixels())?;
        let source = clipboard::image_svg(size.0, size.1, &png);
        if finish {
            return Ok(source);
        }
        output(source);
    }
}
fn publish(label: String, token: u64, pending: &Arc<Mutex<Option<String>>>, source: String) {
    let Some(app) = APP.get() else {
        return;
    };
    let schedule = pending.lock().unwrap().replace(source).is_none();
    if !schedule {
        return;
    }
    let pending = Arc::clone(pending);
    let _ = app.run_on_main_thread(move || {
        let Ok(_session) = SessionGuard::enter(&label) else {
            return;
        };
        let Some(source) = pending.lock().unwrap().take() else {
            return;
        };
        let result = CHANNEL_GESTURE.with(|slot| -> Result<bool, String> {
            let mut slot = slot.borrow_mut();
            let Some(d) = slot.as_mut().filter(|d| d.token == token && d.valid()) else {
                return Ok(false);
            };
            d.preview = Some(DOCUMENT.with(|doc| doc.borrow().channel_paint_preview(source))?);
            Ok(true)
        });
        match result {
            Ok(true) => {
                let _ = request_redraw();
            }
            Err(e) => emit_error(e),
            _ => {}
        }
    });
}
pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    CHANNEL_GESTURE.with(|s| {
        let s = s.borrow();
        let d = s.as_ref().filter(|d| d.valid())?;
        let preview = d.preview.as_ref()?;
        Some(canvas.renderer.render(canvas.viewport, preview))
    })
}
pub(super) fn edit(action: DocumentAction) -> Option<Result<DocumentSnapshot, String>> {
    let channel = color_channel()?;
    if matches!(
        action,
        DocumentAction::ClearLayer | DocumentAction::DeleteSelectedObjects
    ) {
        if channel > 4 {
            return Some(Err(EDIT_HELP.into()));
        }
        return Some((|| {
            cancel_vector_drag();
            let (source, selection) = DOCUMENT.with(|d| -> Result<_, String> {
                let d = d.borrow();
                Ok((
                    if channel == 4 {
                        d.clone_stamp_workspace()?
                    } else {
                        d.retouch_workspace()?
                    },
                    d.selection().cloned(),
                ))
            })?;
            let (w, h) = source.dimensions();
            let mut raw = lumapaint_renderer::pixel_paint::PixelPaintPreview::new((w, h))?;
            raw.update(&source)?;
            let (mut pixels, _) = raw.into_parts();
            lumapaint_renderer::channel_paint::clear(
                &mut pixels,
                (w, h),
                channel,
                selection.as_ref(),
            )?;
            let png = lumapaint_renderer::vector::document_png_ref(w, h, &pixels)?;
            let svg = clipboard::image_svg(w, h, &png);
            DOCUMENT.with(|d| {
                if channel == 4 {
                    d.borrow_mut().replace_moved_pixels(svg, selection)
                } else {
                    d.borrow_mut().replace_retouch_pixels(svg, selection)
                }
            })?;
            redraw()?;
            emit_document();
            Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
        })());
    }
    if matches!(
        action,
        DocumentAction::Cut
            | DocumentAction::Paste
            | DocumentAction::RegistrationFill
            | DocumentAction::RegistrationStroke
            | DocumentAction::CreateTrimMarks
    ) {
        return Some(Err(EDIT_HELP.into()));
    }
    None
}
pub(super) fn key(event: &NSEvent) -> bool {
    if color_channel().is_none() {
        return false;
    }
    if event.keyCode() == 53 {
        CHANNEL_GESTURE.with(|s| s.borrow_mut().take());
        let _ = redraw();
        return true;
    }
    if [51, 117].contains(&event.keyCode()) {
        if let Some(Err(e)) = edit(DocumentAction::DeleteSelectedObjects) {
            emit_error(e);
        }
        return true;
    }
    if [123, 124, 125, 126].contains(&event.keyCode()) {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pixels() -> Vec<u8> {
        DOCUMENT.with(|d| {
            let source = d.borrow().retouch_workspace().unwrap();
            let mut raster =
                lumapaint_renderer::pixel_paint::PixelPaintPreview::new(source.dimensions())
                    .unwrap();
            raster.update(&source).unwrap();
            raster.into_parts().0
        })
    }
    #[test]
    fn channel_brush_routes_component_edits_and_undo_to_selected_pixel_layer() {
        for root in [true, false] {
            for channel in [1, 2, 3, 4] {
                let mut state = Document::default().document_state();
                state.width = 32;
                state.height = 32;
                let mut d = Document::from_document_state(state).unwrap();
                let id = if root {
                    "layer-1".into()
                } else {
                    d.add_paint_layer().unwrap()
                };
                d.select_layer(id.clone()).unwrap();
                d.replace_moved_pixels(r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#285078"/></svg>"##.into(),None).unwrap();
                d.select_channel(channel).unwrap();
                let revision = d.revision();
                DOCUMENT.with(|doc| *doc.borrow_mut() = d);
                ACTIVE_TILED_DOCUMENT.with(|d| *d.borrow_mut() = None);
                TOOL.with(|t| t.set(CanvasTool::Brush));
                BRUSH.with(|b| {
                    *b.borrow_mut() = Brush {
                        color: [0; 3],
                        size: 8.,
                        hardness: 1.,
                        ..Default::default()
                    }
                });
                let before = pixels();
                assert!(pointer(Point { x: 8., y: 8. }, 0, 1.).unwrap());
                assert!(pointer(Point { x: 16., y: 8. }, 1, 1.).unwrap());
                assert_eq!(pixels(), before);
                assert!(pointer(Point { x: 16., y: 8. }, 2, 1.).unwrap());
                let after = pixels();
                let at = (8 * 32 + 12) * 4;
                if channel == 4 {
                    assert_eq!(&after[at..at + 4], &[0; 4]);
                } else {
                    for c in 0..4 {
                        assert_eq!(
                            after[at + c],
                            if c == channel as usize - 1 {
                                0
                            } else {
                                before[at + c]
                            }
                        );
                    }
                }
                assert_eq!(&after[..4], &before[..4]);
                DOCUMENT.with(|d| {
                    let mut d = d.borrow_mut();
                    assert_eq!(d.revision(), revision + 1);
                    d.undo();
                });
                assert_eq!(pixels(), before);
                DOCUMENT.with(|d| {
                    d.borrow_mut().redo();
                });
                assert_eq!(pixels(), after);
                // Switching target during a stroke discards it without a commit.
                pointer(Point { x: 24., y: 24. }, 0, 1.).unwrap();
                DOCUMENT.with(|d| d.borrow_mut().select_channel(0).unwrap());
                cancel_vector_drag();
                assert_eq!(pixels(), after);
            }
        }
    }
}
