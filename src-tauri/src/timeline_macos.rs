use super::*;
use crate::timeline::{Data, Layer, Request, Response};
use lumapaint_core::document::animation::Keyframe;
type OutlineCache =
    std::collections::HashMap<String, (usize, Vec<lumapaint_core::vector::VectorObject>)>;

struct Runtime {
    document_id: u64,
    revision: u64,
    frame: u32,
    rendered_frame: Option<u32>,
    started: Option<(Instant, u32)>,
    preview: Option<Document>,
    editing: Option<(String, u32, u64)>,
    gpu_frame: Option<u32>,
    outlines: RefCell<OutlineCache>,
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
        runtime.gpu_frame = None;
        runtime.outlines.borrow_mut().clear();
        if id != runtime.document_id {
            runtime.frame = 0;
            runtime.editing = None;
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
        runtime.preview.as_ref()?;
        // Host tile caches describe the editable document, not animation cels.
        canvas.tile_ready = None;
        canvas.tile_cache = None;
        // Invalidate the GPU tile ownership too: a retained paint-cache flag can
        // otherwise suppress strokes while pointing at another cel's texture.
        if runtime.gpu_frame != Some(runtime.frame) {
            canvas.renderer.clear_tiled_preview();
            runtime.gpu_frame = Some(runtime.frame);
        }
        canvas.renderer.set_tiled_preview_visible(false);
        canvas.renderer.set_selection_overlay_visible(false);
        Some(
            canvas
                .renderer
                .render(canvas.viewport, runtime.preview.as_ref().unwrap()),
        )
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
        let preview = runtime
            .preview
            .get_or_insert_with(|| doc.clone_for_rendering());
        let cache = &runtime.outlines;
        let outline = |source: &str, object: &lumapaint_core::vector::VectorObject| {
            if let Some((_, objects)) = cache.borrow().get(source) {
                return Ok(objects.clone());
            }
            let objects = lumapaint_renderer::text_outlines::outline(source, object)?;
            let bytes = source.len()
                + objects
                    .iter()
                    .map(|o| o.path.data.len() + o.control_points.len() * 8)
                    .sum::<usize>();
            if bytes <= 8 * 1024 * 1024 {
                let mut cache = cache.borrow_mut();
                if cache.len() >= 64
                    || cache.values().map(|(charge, _)| *charge).sum::<usize>() + bytes
                        > 8 * 1024 * 1024
                {
                    cache.clear();
                }
                cache.insert(source.into(), (bytes, objects.clone()));
            }
            Ok::<_, String>(objects)
        };
        preview.evaluate_animation_with_outlines(&doc, runtime.frame, Some(&outline));
        true
    })
}
/// A document pointer-down enters drawing without discarding the displayed cel.
pub(super) fn begin_editing() -> Result<(), String> {
    if !active() {
        return Ok(());
    }
    let frame = RUNTIMES.with(|r| r.borrow()[&current_label()].frame);
    let layer = DOCUMENT.with(|d| d.borrow().snapshot().layer_id);
    let held = DOCUMENT.with(|d| {
        d.borrow()
            .animation()
            .tracks
            .get(&layer)
            .and_then(|t| t.cels.range(..=frame).next_back())
            .is_some()
    });
    if held {
        command(Request::Load { layer, frame })?;
    } else {
        command(Request::Stop)?;
    }
    Ok(())
}
fn record_edit(runtime: &mut Runtime) -> Result<bool, String> {
    let Some((layer, frame, revision)) = runtime.editing.clone() else {
        return Ok(false);
    };
    if runtime.document_id != identity().0 {
        runtime.editing = None;
        return Ok(false);
    }
    let changed = DOCUMENT.with(|d| {
        let mut doc = d.borrow_mut();
        doc.finish();
        if doc.revision() == revision {
            return Ok(false);
        }
        doc.capture_animation_cel(&layer, frame, false)?;
        Ok::<_, String>(true)
    })?;
    runtime.editing = None;
    Ok(changed)
}
fn validate_motion_font(
    doc: &Document,
    layer: &str,
    clip: &lumapaint_core::motion::Clip,
) -> Result<(), String> {
    if !clip.enabled
        || !lumapaint_core::motion::preset(&clip.preset).is_some_and(|preset| {
            preset
                .operations
                .contains(&lumapaint_core::motion::Operation::FontAxes)
        })
    {
        return Ok(());
    }
    let defaults = lumapaint_core::motion::default_axes(&clip.preset);
    let axes = if clip.font_axes.is_empty() {
        &defaults
    } else {
        &clip.font_axes
    };
    let families = if clip.variable_font.is_empty() {
        doc.svg_layers()
            .filter(|svg| svg.id == layer)
            .flat_map(|svg| svg.vector_objects.iter())
            .filter(|o| clip.objects.is_empty() || clip.objects.contains(&o.id))
            .filter_map(|o| o.text.as_ref())
            .map(|text| {
                if text
                    .content
                    .chars()
                    .any(lumapaint_core::motion::kanji::is_cjk)
                    && axes.iter().all(|a| a.tag == "wght")
                {
                    "LumaPaint Noto Sans JP"
                } else {
                    "LumaPaint Roboto Flex"
                }
            })
            .collect::<std::collections::BTreeSet<_>>()
    } else {
        std::collections::BTreeSet::from([clip.variable_font.as_str()])
    };
    let fonts = lumapaint_renderer::vector::font_viewer::variable_catalog();
    for family in families {
        let font = fonts
            .iter()
            .find(|font| font.family == family)
            .ok_or("Variable font unavailable / 可変フォントが見つかりません / 未找到可变字体")?;
        for axis in axes {
            let limits = font.axes.iter().find(|a| a.tag == axis.tag).ok_or(
                "Unsupported font axis / このフォントは指定の軸に対応していません / 字体不支持该轴",
            )?;
            if axis.from < limits.minimum
                || axis.from > limits.maximum
                || axis.to < limits.minimum
                || axis.to > limits.maximum
            {
                return Err("Font axis outside its range / フォント軸の指定範囲を超えています / 字体轴超出范围".into());
            }
        }
    }
    Ok(())
}

pub(crate) fn command(request: Request) -> Result<Response, String> {
    if let Request::SetMotion { document_id, .. }
    | Request::RemoveMotion { document_id, .. }
    | Request::MoveMotion { document_id, .. }
    | Request::StopMotion { document_id } = &request
    {
        if *document_id != identity().0 {
            return Err(
                "The document changed / ドキュメントが切り替わりました / 文档已切换".into(),
            );
        }
    }
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
    let transport_changed = matches!(
        request,
        Request::Play
            | Request::Pause
            | Request::Stop
            | Request::StopMotion { .. }
            | Request::Seek { .. }
    );
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
            editing: None,
            outlines: Default::default(),
            gpu_frame: None,
        });
        if let Request::Seek { frame } = &request {
            if *frame >= DOCUMENT.with(|d| d.borrow().animation().duration) {
                return Err("Frame is outside the timeline".into());
            }
        }
        if matches!(
            request,
            Request::Seek { .. }
                | Request::Play
                | Request::Stop
                | Request::StopMotion { .. }
                | Request::Load { .. }
        ) {
            changed |= record_edit(runtime)?;
        }
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
            Request::Stop | Request::StopMotion { .. } => {
                runtime.started = None;
                runtime.preview = None;
                runtime.gpu_frame = None;
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
                let editing = match &edit {
                    Request::Load { layer, frame }
                    | Request::Capture {
                        layer,
                        frame,
                        blank: true,
                    } => Some((layer.clone(), *frame)),
                    _ => None,
                };
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
                        | Request::SetMotion { .. }
                        | Request::RemoveMotion { .. }
                        | Request::MoveMotion { .. }
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
                        Request::SetMotion { layer, clip, .. } => {
                            clip.validate(18000)?;
                            validate_motion_font(&doc, &layer, &clip)?;
                            doc.set_motion_clip(&layer, *clip)
                        }
                        Request::RemoveMotion { layer, id, .. } => {
                            doc.remove_motion_clip(&layer, &id)
                        }
                        Request::MoveMotion {
                            layer,
                            id,
                            position,
                            ..
                        } => doc.move_motion_clip(&layer, &id, position),
                        _ => unreachable!(),
                    }
                })?;
                runtime.started = None;
                runtime.preview = None;
                runtime.revision = identity().1;
                runtime.gpu_frame = None;
                if let Some((layer, frame)) = editing {
                    runtime.editing = Some((layer, frame, runtime.revision));
                    runtime.frame = frame;
                }
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
                            motions: track.map(|t| t.motions.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                Data {
                    document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                    selected_layer,
                    selected_objects: doc.selected_vector_ids().to_vec(),
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
    if transport_changed {
        if let Some(app) = APP.get() {
            let _ = app.emit_to(current_label(), "timeline-changed", ());
        }
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
    fn variable_font_validation_uses_actual_axes_and_ranges() {
        use lumapaint_core::{
            document::TextSettings,
            motion::{AxisMotion, Clip},
            vector::VectorText,
        };
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "日本語 Motion".into(),
                ..Default::default()
            },
            position: [0., 0.],
            color: [0; 3],
        })
        .unwrap();
        let layer = doc.svg_layers().next().unwrap().id.clone();
        let mut clip = Clip {
            id: "axes".into(),
            preset: "tx-weight-wave".into(),
            ..Default::default()
        };
        assert!(validate_motion_font(&doc, &layer, &clip).is_ok());
        clip.variable_font = "LumaPaint Noto Sans JP".into();
        clip.font_axes = vec![AxisMotion {
            tag: "wdth".into(),
            from: 75.,
            to: 125.,
        }];
        assert!(validate_motion_font(&doc, &layer, &clip).is_err());
        clip.variable_font = "LumaPaint Roboto Flex".into();
        assert!(validate_motion_font(&doc, &layer, &clip).is_ok());
        clip.font_axes[0].to = 10000.;
        assert!(validate_motion_font(&doc, &layer, &clip).is_err());
        clip.variable_font = "unavailable motion test font".into();
        assert!(validate_motion_font(&doc, &layer, &clip).is_err());
        clip.enabled = false;
        assert!(validate_motion_font(&doc, &layer, &clip).is_ok());
    }
    #[test]
    fn stale_motion_commands_and_panel_cleanup_cannot_change_another_document() {
        let _restore = setup();
        let id = identity().0.wrapping_add(1);
        let revision = DOCUMENT.with(|d| d.borrow().revision());
        for request in [
            Request::SetMotion {
                document_id: id,
                layer: "foreign".into(),
                clip: Box::new(lumapaint_core::motion::Clip {
                    id: "clip".into(),
                    ..Default::default()
                }),
            },
            Request::RemoveMotion {
                document_id: id,
                layer: "foreign".into(),
                id: "clip".into(),
            },
            Request::MoveMotion {
                document_id: id,
                layer: "foreign".into(),
                id: "clip".into(),
                position: 0,
            },
            Request::StopMotion { document_id: id },
        ] {
            assert!(command(request).is_err());
        }
        assert_eq!(DOCUMENT.with(|d| d.borrow().revision()), revision);
        assert!(RUNTIMES.with(|r| r.borrow().is_empty()));
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
    fn seeking_and_automatic_editing_preserve_and_record_cel_artwork() {
        let _restore = setup();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.begin(
                lumapaint_core::document::Point { x: 20., y: 20. },
                Brush::default(),
            )
            .unwrap();
            doc.finish();
            doc.capture_animation_cel("layer-1", 0, false).unwrap();
            doc.blank_animation_cel("layer-1", 5).unwrap();
        });
        command(Request::Seek { frame: 2 }).unwrap();
        RUNTIMES.with(|r| {
            assert_eq!(
                r.borrow()[&current_label()]
                    .preview
                    .as_ref()
                    .unwrap()
                    .committed_paint_strokes()
                    .count(),
                1
            );
        });
        begin_editing().unwrap();
        assert!(!active());
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            assert_eq!(doc.committed_paint_strokes().count(), 1);
            doc.begin(
                lumapaint_core::document::Point { x: 40., y: 40. },
                Brush::default(),
            )
            .unwrap();
            doc.finish();
        });
        // Passive UI refresh must not lose the pending editable frame.
        command(Request::Get).unwrap();
        command(Request::Seek { frame: 5 }).unwrap();
        RUNTIMES.with(|r| {
            assert_eq!(
                r.borrow()[&current_label()]
                    .preview
                    .as_ref()
                    .unwrap()
                    .committed_paint_strokes()
                    .count(),
                0
            )
        });
        command(Request::Seek { frame: 2 }).unwrap();
        RUNTIMES.with(|r| {
            assert_eq!(
                r.borrow()[&current_label()]
                    .preview
                    .as_ref()
                    .unwrap()
                    .committed_paint_strokes()
                    .count(),
                2
            )
        });
        DOCUMENT.with(|d| {
            assert_eq!(
                d.borrow().animation().tracks["layer-1"].cels[&2]
                    .strokes
                    .len(),
                2
            )
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
