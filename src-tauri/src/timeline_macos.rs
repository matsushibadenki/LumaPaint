use super::*;
use crate::timeline::{Data, Layer, Request, Response};
use lumapaint_core::document::animation::Keyframe;

struct Runtime {
    document_id: u64,
    revision: u64,
    frame: u32,
    rendered_frame: Option<u32>,
    started: Option<(Instant, u32)>,
    preview: Option<Document>,
}
thread_local! { static RUNTIMES: RefCell<std::collections::HashMap<String, Runtime>> = RefCell::new(std::collections::HashMap::new()); }
fn identity() -> (u64, u64) {
    (
        ACTIVE_DOCUMENT_ID.with(|id| id.get()),
        DOCUMENT.with(|d| d.borrow().revision()),
    )
}
fn reset_stale(runtime: &mut Runtime) {
    let (id, revision) = identity();
    if id != runtime.document_id || revision != runtime.revision {
        runtime.started = None;
        runtime.preview = None;
        runtime.rendered_frame = None;
        if id != runtime.document_id {
            runtime.frame = 0;
        }
        runtime.document_id = id;
        runtime.revision = revision;
    }
}
pub(super) fn close(label: &str) {
    RUNTIMES.with(|r| {
        r.borrow_mut().remove(label);
    });
}
pub(super) fn active() -> bool {
    RUNTIMES.with(|r| {
        r.borrow()
            .get(&current_label())
            .is_some_and(|r| r.preview.is_some() && (r.document_id, r.revision) == identity())
    })
}
pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    RUNTIMES.with(|r| {
        let mut runtimes = r.borrow_mut();
        let runtime = runtimes.get_mut(&current_label())?;
        reset_stale(runtime);
        let preview = runtime.preview.as_ref()?;
        // Host tile caches describe the editable document, not animation cels.
        canvas.tile_ready = None;
        canvas.tile_cache = None;
        canvas.renderer.set_tiled_preview_visible(false);
        canvas.renderer.set_selection_overlay_visible(false);
        Some(canvas.renderer.render(canvas.viewport, preview))
    })
}
fn evaluate(runtime: &mut Runtime) -> bool {
    DOCUMENT.with(|d| {
        let doc = d.borrow();
        if runtime.preview.is_some()
            && runtime
                .rendered_frame
                .is_some_and(|frame| doc.animation().same_picture(frame, runtime.frame))
        {
            return false;
        }
        runtime.rendered_frame = Some(runtime.frame);
        let preview = runtime.preview.get_or_insert_with(|| doc.clone());
        preview.evaluate_animation(&doc, runtime.frame);
        true
    })
}
pub(crate) fn command(request: Request) -> Result<Response, String> {
    if !DOCUMENT_OPEN.with(|d| d.get()) {
        close(&current_label());
        return Ok(Response {
            frame: 0,
            playing: false,
            preview: false,
            data: None,
        });
    }
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return Err("Animation is available for standard documents / 標準ドキュメントでアニメーションを使用できます / 动画适用于标准文档".into());
    }
    let tick = matches!(request, Request::Tick);
    if !matches!(request, Request::Tick | Request::Get | Request::Pause) {
        ensure_document_open()?;
    }
    let mut changed = false;
    let mut redraw_needed = false;
    let response = RUNTIMES.with(|r| -> Result<Response, String> {
        let mut runtimes = r.borrow_mut();
        let (id, revision) = identity();
        let runtime = runtimes.entry(current_label()).or_insert(Runtime {
            document_id: id,
            revision,
            frame: 0,
            rendered_frame: None,
            started: None,
            preview: None,
        });
        reset_stale(runtime);
        let duration = DOCUMENT.with(|d| {
            let d = d.borrow();
            let a = d.animation();
            a.duration
        });
        runtime.frame = runtime.frame.min(duration - 1);
        match request {
            Request::Get => {}
            Request::Tick => {
                if let Some((start, first)) = runtime.started {
                    let (frame, playing) = DOCUMENT.with(|d| {
                        d.borrow()
                            .animation()
                            .playback_frame(first, start.elapsed())
                    });
                    if !playing {
                        runtime.started = None;
                    }
                    if frame != runtime.frame {
                        runtime.frame = frame;
                        redraw_needed = evaluate(runtime);
                    }
                }
            }
            Request::Play => {
                if runtime.frame == duration - 1 {
                    runtime.frame = 0;
                }
                runtime.started = Some((Instant::now(), runtime.frame));
                evaluate(runtime);
                redraw_needed = true;
            }
            Request::Pause => {
                runtime.started = None;
            }
            Request::Stop => {
                runtime.started = None;
                runtime.preview = None;
                redraw_needed = true;
            }
            Request::Seek { frame } => {
                if frame >= duration {
                    return Err("Frame is outside the timeline".into());
                }
                runtime.started = None;
                runtime.frame = frame;
                evaluate(runtime);
                redraw_needed = true;
            }
            edit => {
                // Validate the edit completely before invalidating preview state.
                let next_frame = match &edit {
                    Request::Duplicate { frame, .. } | Request::MoveKey { frame, .. } => {
                        Some(*frame)
                    }
                    _ => None,
                };
                let show_preview = matches!(
                    edit,
                    Request::Key { .. }
                        | Request::MoveKey { .. }
                        | Request::Remove { .. }
                        | Request::Duplicate { .. }
                );
                DOCUMENT.with(|d| -> Result<(), String> {
                    let mut doc = d.borrow_mut();
                    match edit {
                        Request::Select { layer } => doc.select_layer(layer),
                        Request::Settings {
                            fps,
                            duration,
                            looping,
                        } => doc.set_animation_settings(fps, duration, looping),
                        Request::Capture {
                            layer,
                            frame,
                            blank,
                        } => {
                            if blank {
                                doc.blank_animation_cel(&layer, frame)
                            } else {
                                doc.capture_animation_cel(&layer, frame, false)
                            }
                        }
                        Request::Duplicate { layer, from, frame } => {
                            doc.duplicate_animation_cel(&layer, from, frame)
                        }
                        Request::Load { layer, frame } => doc.load_animation_cel(&layer, frame),
                        Request::Key {
                            layer,
                            frame,
                            property,
                            value,
                            interpolation,
                        } => doc.set_animation_key(
                            &layer,
                            property,
                            Keyframe {
                                frame,
                                value,
                                interpolation,
                            },
                        ),
                        Request::MoveKey {
                            layer,
                            property,
                            from,
                            frame,
                        } => doc.move_animation_key(&layer, property, from, frame),
                        Request::Remove {
                            layer,
                            frame,
                            property,
                        } => doc.remove_animation_key(&layer, frame, property),
                        _ => unreachable!(),
                    }
                })?;
                runtime.started = None;
                runtime.preview = None;
                runtime.revision = identity().1;
                if let Some(frame) = next_frame {
                    runtime.frame = frame;
                }
                runtime.frame = runtime
                    .frame
                    .min(DOCUMENT.with(|d| d.borrow().animation().duration) - 1);
                if show_preview {
                    evaluate(runtime);
                }
                changed = true;
                redraw_needed = true;
            }
        }
        let data = if tick {
            None
        } else {
            Some(DOCUMENT.with(|d| {
                let doc = d.borrow();
                let a = doc.animation();
                let snapshot = doc.snapshot();
                let selected_layer = snapshot.layer_id;
                let layers = snapshot
                    .layers
                    .into_iter()
                    .filter(|l| l.kind == "paint" || l.kind == "vector" || l.kind == "svg")
                    .map(|l| {
                        let track = a.tracks.get(&l.id);
                        Layer {
                            id: l.id,
                            name: l.name,
                            pixel: l.kind == "paint",
                            locked: l.locked,
                            cels: track
                                .map(|t| t.cels.keys().copied().collect())
                                .unwrap_or_default(),
                            channels: track.map(|t| t.channels.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                Data {
                    selected_layer,
                    fps: a.fps,
                    duration: a.duration,
                    looping: a.looping,
                    layers,
                }
            }))
        };
        Ok(Response {
            frame: runtime.frame,
            playing: runtime.started.is_some(),
            preview: runtime.preview.is_some(),
            data,
        })
    })?;
    if changed {
        emit_document();
    }
    if redraw_needed {
        redraw()?;
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Restore {
        document: Option<Document>,
        open: bool,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            close(&current_label());
            DOCUMENT.with(|d| *d.borrow_mut() = self.document.take().unwrap());
            DOCUMENT_OPEN.with(|d| d.set(self.open));
        }
    }
    fn setup() -> Restore {
        let previous = DOCUMENT.with(|d| d.replace(Document::default()));
        let open = DOCUMENT_OPEN.with(|d| d.replace(true));
        close(&current_label());
        Restore {
            document: Some(previous),
            open,
        }
    }
    #[test]
    fn timeline_queries_and_idle_ticks_do_not_commit_live_paint() {
        let _restore = setup();
        DOCUMENT.with(|d| {
            d.borrow_mut()
                .begin(
                    lumapaint_core::document::Point { x: 20., y: 20. },
                    Brush::default(),
                )
                .unwrap()
        });
        let revision = DOCUMENT.with(|d| d.borrow().revision());
        assert!(command(Request::Get).unwrap().data.is_some());
        assert!(command(Request::Tick).unwrap().data.is_none());
        DOCUMENT.with(|d| {
            assert!(d.borrow().has_active_stroke());
            assert_eq!(d.borrow().revision(), revision);
        });
    }
    #[test]
    fn timeline_stops_stale_previews_and_releases_closed_document() {
        let _restore = setup();
        command(Request::Get).unwrap();
        RUNTIMES.with(|r| {
            let mut r = r.borrow_mut();
            let runtime = r.get_mut(&current_label()).unwrap();
            runtime.preview = Some(Document::default());
            runtime.started = Some((Instant::now(), 0));
        });
        assert!(active());
        DOCUMENT.with(|d| {
            d.borrow_mut()
                .set_animation_settings(12, 120, true)
                .unwrap()
        });
        let response = command(Request::Get).unwrap();
        assert!(!response.preview);
        assert!(!response.playing);
        DOCUMENT_OPEN.with(|o| o.set(false));
        assert!(command(Request::Get).unwrap().data.is_none());
        assert!(RUNTIMES.with(|r| r.borrow().get(&current_label()).is_none()));
    }
}
