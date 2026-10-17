use super::*;
use crate::timeline::{Data, Layer, Request, Response};
use lumapaint_core::document::animation::Keyframe;
type OutlineCache =
    std::collections::HashMap<String, (usize, Vec<lumapaint_core::vector::VectorObject>)>;

type EditingCel = (String, u32, u64, Option<u32>);
struct Runtime {
    document_id: u64,
    revision: u64,
    frame: u32,
    rendered_frame: Option<u32>,
    started: Option<(Instant, u32)>,
    preview: Option<Document>,
    editing: Option<EditingCel>,
    gpu_frame: Option<u32>,
    metadata_sent: bool,
    metadata_selection: Option<(String, Vec<String>)>,
    outlines: RefCell<OutlineCache>,
    onion_key: Option<OnionKey>,
    onion_generation: u64,
    onion_ready: bool,
}
#[derive(PartialEq)]
struct OnionKey {
    side: u32,
    scene_revision: Option<u64>,
    document_id: u64,
    layer: String,
    frame: u32,
    dimensions: (u32, u32),
    settings: lumapaint_core::document::animation::OnionSettings,
    references: Vec<(usize, u32, [u8; 3])>,
}
fn next_onion_generation() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
        runtime.metadata_sent = false;
        runtime.metadata_selection = None;
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
    if label == current_label() {
        CANVAS.with(|c| {
            if let Some(canvas) = c.borrow_mut().as_mut() {
                canvas.renderer.set_onion_visible(false);
                let _ = canvas.renderer.install_onion_skin(None);
            }
        });
    }
}
pub(super) fn configure_onion(canvas: &mut Canvas) {
    let visible = RUNTIMES.with(|r| {
        r.borrow().get(&current_label()).is_some_and(|runtime| {
            runtime.onion_ready
                && runtime.started.is_none()
                && runtime.onion_key.as_ref().is_some_and(|key| {
                    key.document_id == identity().0
                        && key.frame == runtime.frame
                        && DOCUMENT.with(|d| {
                            let doc = d.borrow();
                            (key.scene_revision.is_some() || key.layer == doc.selected_layer_id())
                                && key
                                    .scene_revision
                                    .is_none_or(|revision| revision == doc.revision())
                                && key.settings == doc.animation().onion
                                && key.settings.enabled
                        })
                })
        })
    });
    canvas.renderer.set_onion_visible(visible);
}
/// Called after native viewport changes, with no Canvas borrow held.
pub(super) fn schedule_onion() {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(window) = app.get_webview_window(&current_label()) else {
        return;
    };
    if let Some(job) = onion_job() {
        crate::cel_previews::schedule(window, job);
    }
}
pub(crate) fn onion_job() -> Option<crate::cel_previews::OnionJob> {
    let side = DOCUMENT.with(|doc| {
        CANVAS.with(|c| {
            c.borrow().as_ref().map_or(1024, |c| {
                crate::cel_previews::reference_side(
                    doc.borrow().dimensions(),
                    c.viewport.screen_zoom() * c.viewport.scale,
                )
            })
        })
    });
    onion_job_at_side(side)
}
fn onion_job_at_side(side: u32) -> Option<crate::cel_previews::OnionJob> {
    RUNTIMES.with(|r| {
        let mut runtimes = r.borrow_mut();
        let runtime = runtimes.get_mut(&current_label())?;
        DOCUMENT.with(|d| {
            let doc = d.borrow();
            let settings = &doc.animation().onion;
            let layer = doc.selected_layer_id();
            let scene = settings.scope == lumapaint_core::document::animation::OnionScope::Scene;
            let track = doc.animation().tracks.get(layer);
            let enabled = settings.enabled && runtime.started.is_none();
            let scene_references = if enabled && scene {
                settings.scene_references(doc.animation(), runtime.frame)
            } else {
                vec![]
            };
            let references = if enabled && !scene {
                track
                    .map(|t| settings.references(t, runtime.frame, doc.animation().duration))
                    .unwrap_or_default()
            } else {
                vec![]
            };
            if references.is_empty() && scene_references.is_empty() {
                if runtime.onion_key.take().is_some() {
                    runtime.onion_generation = next_onion_generation();
                    runtime.onion_ready = false;
                    CANVAS.with(|c| {
                        if let Some(canvas) = c.borrow_mut().as_mut() {
                            let _ = canvas.renderer.install_onion_skin(None);
                        }
                    });
                }
                return None;
            }
            let key = OnionKey {
                side,
                scene_revision: scene.then(|| doc.revision()),
                document_id: identity().0,
                layer: if scene { String::new() } else { layer.into() },
                frame: runtime.frame,
                dimensions: doc.dimensions(),
                settings: settings.clone(),
                references: if scene {
                    scene_references
                        .iter()
                        .map(|r| (r.frame as usize, r.opacity.to_bits(), r.color))
                        .collect()
                } else {
                    references
                        .iter()
                        .map(|r| {
                            (
                                std::sync::Arc::as_ptr(&r.cel) as usize,
                                r.opacity.to_bits(),
                                r.color,
                            )
                        })
                        .collect()
                },
            };
            // Held exposures can move the playhead without changing any skin.
            // Keep the texture and only advance its ownership stamp.
            if runtime.onion_key.as_mut().is_some_and(|previous| {
                previous.frame = key.frame;
                *previous == key
            }) {
                return None;
            }
            runtime.onion_generation = next_onion_generation();
            runtime.onion_ready = false;
            runtime.onion_key = Some(key);
            Some(crate::cel_previews::OnionJob {
                generation: runtime.onion_generation,
                document_id: identity().0,
                revision: doc.revision(),
                side,
                label: current_label(),
                dimensions: doc.dimensions(),
                references,
                mode: settings.color,
                scene: scene.then(|| {
                    let started = std::time::Instant::now();
                    let snapshot = doc.clone_for_rendering();
                    if std::env::var_os("LUMAPAINT_ONION_METRICS").is_some() {
                        eprintln!(
                            "lumapaint-onion-host generation={} stage=snapshot elapsed_us={}",
                            runtime.onion_generation,
                            started.elapsed().as_micros()
                        );
                    }
                    snapshot
                }),
                scene_references,
            })
        })
    })
}
pub(crate) fn install_onion_for_window(
    label: &str,
    generation: u64,
    result: Result<Option<lumapaint_renderer::animation_cels::Raster>, String>,
) {
    let Ok(_session) = SessionGuard::enter(label) else {
        return;
    };
    let accepted = RUNTIMES.with(|r| {
        r.borrow().get(label).is_some_and(|r| {
            r.onion_generation == generation
                && r.document_id == identity().0
                && r.onion_key.as_ref().is_some_and(|key| {
                    key.scene_revision
                        .is_none_or(|revision| revision == identity().1)
                })
        })
    });
    if !accepted {
        return;
    }
    let upload_started = std::time::Instant::now();
    let installed = CANVAS.with(|c| {
        let mut canvas = c.borrow_mut();
        let Some(canvas) = canvas.as_mut() else {
            return Ok(());
        };
        canvas.renderer.install_onion_skin(result?)
    });
    if std::env::var_os("LUMAPAINT_ONION_METRICS").is_some() {
        eprintln!("lumapaint-onion-host generation={generation} stage=upload elapsed_us={} accepted=true success={}", upload_started.elapsed().as_micros(), installed.is_ok());
    }
    match installed {
        Ok(()) => {
            RUNTIMES.with(|r| {
                if let Some(runtime) = r.borrow_mut().get_mut(label) {
                    runtime.onion_ready = true;
                }
            });
            let _ = request_redraw();
        }
        Err(error) => emit_error(error),
    }
}
pub(crate) fn thumbnail_sources(
    request: crate::timeline::ThumbnailRequest,
) -> Result<Vec<crate::cel_previews::ThumbnailSource>, String> {
    if request.document_id != identity().0 || request.cels.len() > 64 {
        return Err("Stale or excessive cel preview request".into());
    }
    let editing = RUNTIMES.with(|r| {
        r.borrow()
            .get(&current_label())
            .filter(|r| r.document_id == identity().0)
            .and_then(|r| r.editing.clone())
    });
    DOCUMENT.with(|d| {
        let doc = d.borrow();
        let live = editing
            .as_ref()
            .filter(|(_, _, revision, _)| *revision != doc.revision())
            .and_then(|(layer, frame, _, cel)| {
                let cel = (*cel).or_else(|| {
                    doc.animation()
                        .tracks
                        .get(layer)?
                        .exposure_at(*frame, doc.animation().duration)
                        .map(|(_, e)| e.cel)
                })?;
                Some((layer.clone(), cel, doc.animation_drawing(layer).ok()?))
            });
        request
            .cels
            .into_iter()
            .map(|(layer, frame)| {
                let identity = if request.library {
                    Some(frame)
                } else {
                    doc.animation()
                        .tracks
                        .get(&layer)
                        .and_then(|t| t.exposures.get(&frame))
                        .map(|e| e.cel)
                };
                let live_cel = live
                    .as_ref()
                    .filter(|(id, cel, _)| *id == layer && Some(*cel) == identity)
                    .map(|(_, _, drawing)| drawing.clone());
                let cel = live_cel
                    .or_else(|| {
                        doc.animation()
                            .tracks
                            .get(&layer)
                            .and_then(|t| {
                                if request.library {
                                    t.cel_library.get(&frame).map(|c| &c.drawing)
                                } else {
                                    t.cels.get(&frame)
                                }
                            })
                            .cloned()
                    })
                    .ok_or("Cel is no longer available")?;
                Ok(crate::cel_previews::ThumbnailSource {
                    layer,
                    frame,
                    cel,
                    dimensions: doc.dimensions(),
                })
            })
            .collect()
    })
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
        let previous_frame = runtime.rendered_frame.filter(|_| runtime.preview.is_some());
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
        preview.evaluate_animation_incremental_with_outlines(
            &doc,
            runtime.frame,
            previous_frame,
            Some(&outline),
        );
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
            .and_then(|t| t.exposure_at(frame, 18000))
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
    let Some((layer, frame, revision, library_cel)) = runtime.editing.clone() else {
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
        if let Some(cel) = library_cel {
            doc.record_animation_library_cel(&layer, cel)?;
            return Ok(true);
        }
        let start = doc
            .animation()
            .tracks
            .get(&layer)
            .and_then(|t| t.exposure_at(frame, doc.animation().duration))
            .map_or(frame, |(start, _)| start);
        doc.capture_animation_cel(&layer, start, false)?;
        Ok::<_, String>(true)
    })?;
    runtime.editing = None;
    Ok(changed)
}
pub(super) fn editing_library_drawing(
) -> Option<std::sync::Arc<lumapaint_core::document::animation::Cel>> {
    let (layer, _, _, cel) = RUNTIMES.with(|r| {
        r.borrow()
            .get(&current_label())
            .filter(|r| r.document_id == identity().0)
            .and_then(|r| r.editing.clone())
    })?;
    DOCUMENT.with(|d| {
        d.borrow()
            .animation()
            .tracks
            .get(&layer)?
            .cel_library
            .get(&cel?)
            .map(|c| c.drawing.clone())
    })
}
pub(super) fn restore_library_history(
    before: Option<std::sync::Arc<lumapaint_core::document::animation::Cel>>,
) {
    let Some(before) = before else {
        return;
    };
    let after = editing_library_drawing();
    if after
        .as_ref()
        .is_some_and(|after| std::sync::Arc::ptr_eq(&before, after))
    {
        return;
    }
    RUNTIMES.with(|r| {
        let mut runtimes = r.borrow_mut();
        let Some(runtime) = runtimes.get_mut(&current_label()) else {
            return;
        };
        if let Some((layer, _, revision, Some(cel))) = runtime.editing.as_mut() {
            if DOCUMENT
                .with(|d| {
                    d.borrow_mut()
                        .restore_animation_library_drawing(layer, *cel)
                })
                .is_ok()
            {
                *revision = identity().1;
            } else {
                runtime.editing = None;
            }
        }
    });
}
/// Rebuild the displayed frame before the native Undo/Redo redraw, avoiding a
/// transient view of the ordinary drawing while metadata refresh is pending.
pub(super) fn refresh_history_preview() {
    RUNTIMES.with(|r| {
        let mut runtimes = r.borrow_mut();
        let Some(runtime) = runtimes.get_mut(&current_label()) else {
            return;
        };
        if runtime.document_id == identity().0
            && runtime.preview.is_some()
            && runtime.editing.is_none()
            && runtime.revision != identity().1
        {
            reset_stale(runtime);
            runtime.frame = runtime
                .frame
                .min(DOCUMENT.with(|d| d.borrow().animation().duration) - 1);
            evaluate(runtime);
        }
    });
}
/// Finish before locking or deleting the target layer.
pub(super) fn finish_editing_layer(layer: &str) -> Result<(), String> {
    RUNTIMES.with(|r| {
        let mut runtimes = r.borrow_mut();
        if let Some(runtime) = runtimes.get_mut(&current_label()) {
            if runtime
                .editing
                .as_ref()
                .is_some_and(|(id, _, _, _)| id == layer)
            {
                record_edit(runtime)?;
            }
        }
        Ok(())
    })
}
/// Flush before native save; keep the cel selected for subsequent strokes.
pub(super) fn flush_editing() -> Result<(), String> {
    RUNTIMES.with(|r| {
        let mut runtimes = r.borrow_mut();
        let Some(runtime) = runtimes.get_mut(&current_label()) else {
            return Ok(());
        };
        if runtime.document_id != identity().0 {
            runtime.editing = None;
            return Ok(());
        }
        let editing = runtime.editing.clone();
        record_edit(runtime)?;
        runtime.editing = editing.map(|(layer, frame, _, cel)| (layer, frame, identity().1, cel));
        Ok(())
    })
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
    if let Request::Key {
        document_id: Some(document_id),
        ..
    }
    | Request::SetMotion { document_id, .. }
    | Request::RemoveMotion { document_id, .. }
    | Request::MoveMotion { document_id, .. }
    | Request::ExposureBatch { document_id, .. }
    | Request::ExposureGroup { document_id, .. }
    | Request::EditExposure { document_id, .. }
    | Request::LoadCel { document_id, .. }
    | Request::AssignCel { document_id, .. }
    | Request::RenameCel { document_id, .. }
    | Request::CreateCel { document_id, .. }
    | Request::DeleteCel { document_id, .. }
    | Request::ForkCel { document_id, .. }
    | Request::RemoveExposure { document_id, .. }
    | Request::StopMotion { document_id } = &request
    {
        if *document_id != identity().0 {
            return Err(
                "The document changed / ドキュメントが切り替わりました / 文档已切换".into(),
            );
        }
    }
    if let Request::Onion { document_id, .. } = &request {
        if *document_id != identity().0 {
            return Err(
                "The document changed / ドキュメントが切り替わりました / 文档已切换".into(),
            );
        }
    }
    if !DOCUMENT_OPEN.with(|d| d.get()) {
        close(&current_label());
        return Ok(Response {
            exposure_selection: None,
            exposure_group_selection: None,
            frame: 0,
            playing: false,
            preview: false,
            data: None,
            data_unchanged: false,
        });
    }
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return Err("Animation is available for standard documents / 標準ドキュメントでアニメーションを使用できます / 动画适用于标准文档".into());
    }
    let query = matches!(request, Request::Get);
    let transport_changed = matches!(
        request,
        Request::Play
            | Request::Pause
            | Request::Stop
            | Request::StopMotion { .. }
            | Request::Seek { .. }
    );
    if !matches!(
        request,
        Request::Tick | Request::Get | Request::Refresh | Request::Pause
    ) {
        ensure_document_open()?;
    }
    let mut changed = false;
    let mut redraw_needed = false;
    let response = RUNTIMES.with(|r| -> Result<Response, String> {
        let mut runtimes = r.borrow_mut();
        let (id, revision) = identity();
        let first_open = !runtimes.contains_key(&current_label());
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
            metadata_sent: false,
            metadata_selection: None,
            onion_key: None,
            onion_generation: 0,
            onion_ready: false,
        });
        if let Request::Seek { frame } = &request {
            if *frame >= DOCUMENT.with(|d| d.borrow().animation().duration) {
                return Err("Frame is outside the timeline".into());
            }
        }
        let mut exposure_selection = None;
        let mut exposure_group_selection = None;
        if let Request::ExposureGroup { revision, edits, .. } = &request {
            if *revision != identity().1 { return Err("The document was updated; refresh the selection / 文書が更新されました。選択し直してください / 文档已更新，请重新选择".into()); }
            exposure_group_selection = Some(DOCUMENT.with(|d| d.borrow().animation_exposure_group_selection(edits))?);
        }
        if let Request::ExposureBatch{revision,layer,frames,edit,..}=&request {
            if *revision!=identity().1{return Err("The document was updated; refresh the selection / 文書が更新されました。選択し直してください / 文档已更新，请重新选择".into());}
            exposure_selection = Some((layer.clone(), DOCUMENT.with(|d|d.borrow().animation_exposure_batch_selection(layer,frames,edit))?));
        }
        if let Request::EditExposure {
            layer,
            from,
            start,
            end,
            ..
        } = &request
        {
            DOCUMENT.with(|d| {
                d.borrow()
                    .validate_animation_exposure_edit(layer, *from, *start, *end)
            })?;
            exposure_selection = Some((layer.clone(), vec![*start]));
        }
        if matches!(
            request,
            Request::Seek { .. }
                | Request::Play
                | Request::Stop
                | Request::StopMotion { .. }
                | Request::Load { .. }
                | Request::LoadCel { .. }
                | Request::Select { .. }
                | Request::ExposureBatch { .. }
                | Request::ExposureGroup { .. }
                | Request::EditExposure { .. }
                | Request::AssignCel { .. }
                | Request::RenameCel { .. }
                | Request::CreateCel { .. }
                | Request::DeleteCel { .. }
                | Request::ForkCel { .. }
                | Request::RemoveExposure { .. }
        ) {
            changed |= record_edit(runtime)?;
        }
        let restore_preview = matches!(request, Request::Get | Request::Refresh)
            && runtime.document_id == identity().0
            && runtime.revision != identity().1
            && runtime.preview.is_some()
            && runtime.editing.is_none()
            && !DOCUMENT.with(|d| d.borrow().has_active_stroke());
        reset_stale(runtime);
        let duration = DOCUMENT.with(|d| {
            let d = d.borrow();
            let a = d.animation();
            a.duration
        });
        runtime.frame = runtime.frame.min(duration - 1);
        match request {
            Request::Get => {
                if restore_preview || (first_open
                    && DOCUMENT.with(|d| {
                        let doc = d.borrow();
                        !doc.has_active_stroke()
                            && doc
                                .animation()
                                .tracks
                                .get(doc.selected_layer_id())
                                .is_some_and(|t| !t.cels.is_empty())
                    }))
                {
                    redraw_needed = evaluate(runtime);
                }
            }
            Request::Refresh => { if restore_preview { redraw_needed = evaluate(runtime); } }
            Request::Onion { settings, .. } => {
                settings.validate(duration)?;
                let preview = runtime.preview.is_some();
                let started = runtime.started;
                let editing = runtime.editing.clone();
                record_edit(runtime)?;
                DOCUMENT.with(|d| d.borrow_mut().set_animation_onion(settings))?;
                reset_stale(runtime);
                runtime.started = started;
                if preview {
                    evaluate(runtime);
                }
                if let Some((layer, frame, _, library_cel)) = editing {
                    runtime.editing = Some((layer, frame, identity().1, library_cel));
                }
                changed = true;
                redraw_needed = true;
            }
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
                redraw_needed = evaluate(runtime);
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
                redraw_needed = evaluate(runtime);
            }
            edit => {
                // Validate the edit completely before invalidating preview state.
                let editing = match &edit {
                    Request::Load { layer, frame } => Some((layer.clone(), *frame, None)),
                    Request::Capture { layer, frame, blank } => Some((layer.clone(), *frame,
                        if *blank { None } else { runtime.editing.as_ref().filter(|(id, f, _, _)| id == layer && f == frame).and_then(|(_, _, _, cel)| *cel) })),
                    Request::LoadCel { layer, cel, .. } => Some((layer.clone(), runtime.frame, Some(*cel))),
                    _ => None,
                };
                let next_frame = match &edit {
                    Request::ExposureBatch {frames,edit:lumapaint_core::document::animation::ExposureBatchEdit::Shift{delta,..},..}=>frames.iter().min().map(|f|(i64::from(*f)+i64::from(*delta)).clamp(0,17999) as u32),
                    Request::Key { frame, .. }
                    | Request::Duplicate { frame, .. }
                    | Request::MoveKey { frame, .. }
                    | Request::AssignCel { frame, .. } => Some(*frame),
                    Request::ExposureGroup { .. } => exposure_group_selection.as_ref().and_then(|groups| {
                        let selected = DOCUMENT.with(|d| d.borrow().selected_layer_id().to_string());
                        groups.iter().find(|(layer, frames)| *layer == selected && !frames.is_empty())
                            .or_else(|| groups.iter().find(|(_, frames)| !frames.is_empty())).and_then(|(_, frames)| frames.first().copied())
                    }),
                    Request::EditExposure { start, .. } => Some(*start),
                    _ => None,
                };
                let show_preview = (matches!(edit, Request::Select { .. }) && runtime.preview.is_some()) || matches!(
                    edit,
                    Request::Key { .. }
                        | Request::MoveKey { .. }
                        | Request::Remove { .. }
                        | Request::Duplicate { .. }
                        | Request::SetMotion { .. }
                        | Request::RemoveMotion { .. }
                        | Request::MoveMotion { .. }
                        | Request::ExposureBatch { .. }
                | Request::ExposureGroup { .. }
                | Request::EditExposure { .. }
                        | Request::AssignCel { .. }
                        | Request::RenameCel { .. }
                        | Request::CreateCel { .. }
                        | Request::DeleteCel { .. }
                        | Request::ForkCel { .. }
                        | Request::RemoveExposure { .. }
                );
                DOCUMENT.with(|d| -> Result<(), String> {
                    let mut doc = d.borrow_mut();
                    match edit {
                        Request::Select { layer } => doc.select_layer(layer),
                        Request::ExposureBatch {layer,frames,edit,..} => doc.edit_animation_exposure_batch(&layer,&frames,&edit),
                        Request::ExposureGroup { edits, .. } => doc.edit_animation_exposure_group(&edits),
                        Request::EditExposure {
                            layer,
                            from,
                            start,
                            end,
                            ..
                        } => doc.edit_animation_exposure(&layer, from, start, end),
                        Request::AssignCel {
                            layer, frame, cel, ..
                        } => doc.assign_animation_cel(&layer, frame, cel),
                        Request::RenameCel {
                            layer, cel, name, ..
                        } => doc.rename_animation_cel(&layer, cel, name),
                        Request::CreateCel { layer, .. } => {
                            doc.create_animation_cel(&layer).map(|_| ())
                        }
                        Request::DeleteCel { layer, cel, .. } => {
                            doc.remove_animation_cel(&layer, cel)
                        }
                        Request::ForkCel { layer, frame, .. } => {
                            doc.fork_animation_cel(&layer, frame)
                        }
                        Request::RemoveExposure { layer, frame, .. } => {
                            doc.remove_animation_key(&layer, frame, None)
                        }
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
                            if !blank && runtime.editing.as_ref().is_some_and(|(id, f, _, cel)| *id == layer && *f == frame && cel.is_some()) {
                                doc.record_animation_library_cel(&layer, runtime.editing.as_ref().unwrap().3.unwrap())
                            } else if blank {
                                doc.blank_animation_cel(&layer, frame)
                            } else {
                                let frame = if runtime
                                    .editing
                                    .as_ref()
                                    .is_some_and(|(id, f, _, cel)| *id == layer && *f == frame && cel.is_none())
                                {
                                    doc.animation()
                                        .tracks
                                        .get(&layer)
                                        .and_then(|t| {
                                            t.exposure_at(frame, doc.animation().duration)
                                        })
                                        .map_or(frame, |(start, _)| start)
                                } else {
                                    frame
                                };
                                doc.capture_animation_cel(&layer, frame, false)
                            }
                        }
                        Request::Duplicate { layer, from, frame } => {
                            doc.duplicate_animation_cel(&layer, from, frame)
                        }
                        Request::Load { layer, frame } => doc.load_animation_cel(&layer, frame),
                        Request::LoadCel { layer, cel, .. } => doc.load_animation_library_cel(&layer, cel),
                        Request::Key {
                            layer,
                            frame,
                            property,
                            value,
                            interpolation,
                            ..
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
                if let Some((layer, frame, library_cel)) = editing {
                    // Resolve exposure loads once. Undoing an assignment must
                    // never redirect pending strokes to the replacement cel.
                    let library_cel = library_cel.or_else(|| DOCUMENT.with(|d| {
                        let doc = d.borrow();
                        doc.animation().tracks.get(&layer)?.exposure_at(frame, doc.animation().duration).map(|(_, e)| e.cel)
                    }));
                    runtime.editing = Some((layer, frame, runtime.revision, library_cel));
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
        let selection_unchanged = DOCUMENT.with(|d| {
            let doc = d.borrow();
            runtime
                .metadata_selection
                .as_ref()
                .is_some_and(|(layer, objects)| {
                    layer == doc.selected_layer_id() && objects == doc.selected_vector_ids()
                })
        });
        let data_unchanged = !query && !changed && runtime.metadata_sent && selection_unchanged;
        let data = if data_unchanged {
            None
        } else {
            runtime.metadata_sent = true;
            Some(DOCUMENT.with(|d| {
                let doc = d.borrow();
                let a = doc.animation();
                let snapshot = doc.snapshot();
                let selected_layer = snapshot.layer_id;
                runtime.metadata_selection =
                    Some((selected_layer.clone(), doc.selected_vector_ids().to_vec()));
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
                                .map(|t| t.exposures.keys().copied().collect())
                                .unwrap_or_default(),
                            cel_library: track
                                .map(|t| {
                                    let mut uses = std::collections::HashMap::new();
                                    for exposure in t.exposures.values() {
                                        *uses.entry(exposure.cel).or_insert(0usize) += 1;
                                    }
                                    t.cel_library
                                        .iter()
                                        .map(|(id, c)| crate::timeline::CelInfo {
                                            id: *id,
                                            name: c.name.clone(),
                                            uses: uses.get(id).copied().unwrap_or(0),
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            exposures: track
                                .map(|t| {
                                    t.exposures
                                        .iter()
                                        .map(|(start, e)| crate::timeline::ExposureInfo {
                                            id: e.id.clone(),
                                            start: *start,
                                            end: t.exposure_end(*start, a.duration),
                                            cel: e.cel,
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                            channels: track.map(|t| t.channels.clone()).unwrap_or_default(),
                            motions: track.map(|t| t.motions.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                Data {
                    document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                    selected_layer,
                    revision:doc.revision(),
                    selected_objects: doc.selected_vector_ids().to_vec(),
                    fps: a.fps,
                    duration: a.duration,
                    looping: a.looping,
                    onion: a.onion.clone(),
                    layers,
                }
            }))
        };
        Ok(Response {
            exposure_selection,
            exposure_group_selection,
            frame: runtime.frame,
            playing: runtime.started.is_some(),
            preview: runtime.preview.is_some(),
            data,
            data_unchanged,
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
        // Present after AppKit has processed the panel input and layer changes.
        // Coalesce transport ticks just like native canvas gestures.
        request_redraw()?;
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
    fn library_previews_include_unassigned_drawings_and_validate_requests() {
        let _restore = setup();
        let cel = DOCUMENT.with(|d| d.borrow_mut().create_animation_cel("layer-1").unwrap());
        let sources = thumbnail_sources(crate::timeline::ThumbnailRequest {
            document_id: identity().0,
            cels: vec![("layer-1".into(), cel)],
            library: true,
        })
        .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].frame, cel);
        DOCUMENT.with(|d| {
            assert!(std::sync::Arc::ptr_eq(
                &sources[0].cel,
                &d.borrow().animation().tracks["layer-1"].cel_library[&cel].drawing
            ))
        });
        assert!(thumbnail_sources(crate::timeline::ThumbnailRequest {
            document_id: identity().0,
            cels: vec![("layer-1".into(), u32::MAX)],
            library: true
        })
        .is_err());
        assert!(thumbnail_sources(crate::timeline::ThumbnailRequest {
            document_id: identity().0,
            cels: vec![("layer-1".into(), cel); 65],
            library: true
        })
        .is_err());
    }
    #[test]
    fn batch_timing_guards_stale_revisions_and_commits_one_history_edit() {
        use lumapaint_core::document::animation::ExposureBatchEdit;
        let _restore = setup();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.set_animation_settings(24, 12, true).unwrap();
            for f in [0, 3, 6, 9] {
                doc.capture_animation_cel("layer-1", f, true).unwrap();
            }
        });
        let (id, revision) = identity();
        let result = command(Request::ExposureBatch {
            document_id: id,
            revision,
            layer: "layer-1".into(),
            frames: vec![3, 6],
            edit: ExposureBatchEdit::Shift {
                delta: 2,
                ripple: true,
            },
        })
        .unwrap();
        assert_eq!(
            result.exposure_selection,
            Some(("layer-1".into(), vec![5, 8]))
        );
        assert_eq!(result.frame, 5);
        assert_eq!(result.data.unwrap().duration, 14);
        let updated = identity();
        assert!(command(Request::ExposureBatch {
            document_id: id,
            revision,
            layer: "layer-1".into(),
            frames: vec![5],
            edit: ExposureBatchEdit::Delete { ripple: false }
        })
        .is_err());
        assert_eq!(identity(), updated);
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.undo();
            assert_eq!(doc.animation().duration, 12);
            assert!(doc.animation().tracks["layer-1"].exposures.contains_key(&3));
            doc.redo();
            assert!(doc.animation().tracks["layer-1"].exposures.contains_key(&5));
        });
        close(&current_label());
    }
    #[test]
    fn independent_cel_commands_ranges_and_document_guards() {
        let _restore = setup();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.capture_animation_cel("layer-1", 0, false).unwrap();
            doc.duplicate_animation_cel("layer-1", 0, 10).unwrap();
        });
        let id = identity().0;
        let data = command(Request::Get).unwrap().data.unwrap();
        let layer = &data.layers[0];
        assert_eq!(layer.cel_library.len(), 1);
        assert_eq!(layer.cel_library[0].uses, 2);
        assert_eq!(layer.exposures[0].end, 10);
        command(Request::EditExposure {
            document_id: id,
            layer: "layer-1".into(),
            from: 0,
            start: 2,
            end: 8,
        })
        .unwrap();
        assert_eq!(command(Request::Seek { frame: 8 }).unwrap().frame, 8);
        let before = identity();
        assert!(command(Request::EditExposure {
            document_id: id,
            layer: "layer-1".into(),
            from: 2,
            start: 7,
            end: 11
        })
        .is_err());
        assert_eq!(identity(), before);
        for request in [
            Request::CreateCel {
                document_id: id + 1,
                layer: "layer-1".into(),
            },
            Request::AssignCel {
                document_id: id + 1,
                layer: "layer-1".into(),
                frame: 3,
                cel: 1,
            },
            Request::EditExposure {
                document_id: id + 1,
                layer: "layer-1".into(),
                from: 2,
                start: 2,
                end: 7,
            },
        ] {
            assert!(command(request).is_err());
            assert_eq!(identity(), before);
        }
        command(Request::CreateCel {
            document_id: id,
            layer: "layer-1".into(),
        })
        .unwrap();
        command(Request::RenameCel {
            document_id: id,
            layer: "layer-1".into(),
            cel: 2,
            name: "歩き B".into(),
        })
        .unwrap();
        command(Request::AssignCel {
            document_id: id,
            layer: "layer-1".into(),
            frame: 8,
            cel: 2,
        })
        .unwrap();
        assert!(command(Request::DeleteCel {
            document_id: id,
            layer: "layer-1".into(),
            cel: 2
        })
        .is_err());
        let data = command(Request::RemoveExposure {
            document_id: id,
            layer: "layer-1".into(),
            frame: 8,
        })
        .unwrap()
        .data
        .unwrap();
        let cel = data.layers[0]
            .cel_library
            .iter()
            .find(|c| c.id == 2)
            .unwrap();
        assert_eq!(cel.name, "歩き B");
        assert_eq!(cel.uses, 0);
        command(Request::DeleteCel {
            document_id: id,
            layer: "layer-1".into(),
            cel: 2,
        })
        .unwrap();
        close(&current_label());
    }
    #[test]
    fn scene_onion_jobs_follow_all_tracks_revision_and_selection() {
        use lumapaint_core::document::animation::{OnionScope, OnionSettings};
        let _restore = setup();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.capture_animation_cel("layer-1", 0, false).unwrap();
            doc.begin(
                lumapaint_core::document::Point { x: 30., y: 30. },
                Brush::default(),
            )
            .unwrap();
            doc.finish();
            doc.capture_animation_cel("layer-1", 6, false).unwrap();
            doc.blank_animation_cel("layer-1", 3).unwrap();
        });
        command(Request::Get).unwrap();
        command(Request::Onion {
            document_id: identity().0,
            settings: OnionSettings {
                enabled: true,
                scope: OnionScope::Scene,
                ..Default::default()
            },
        })
        .unwrap();
        command(Request::Seek { frame: 3 }).unwrap();
        let job = onion_job().unwrap();
        assert!(job.references.is_empty());
        assert!(job.scene.is_some());
        assert!(!job.scene_references.is_empty());
        let before = DOCUMENT.with(|d| d.borrow().document_state());
        assert!(crate::cel_previews::prepare_onion(&job).unwrap().is_some());
        assert_eq!(
            serde_json::to_value(DOCUMENT.with(|d| d.borrow().document_state())).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        assert!(onion_job().is_none());
        let small = onion_job_at_side(64).unwrap();
        assert_eq!(small.side, 64);
        assert!(onion_job_at_side(64).is_none());
        let enlarged = onion_job_at_side(128).unwrap();
        assert_eq!(enlarged.side, 128);
        assert_ne!(small.generation, enlarged.generation);
        DOCUMENT.with(|d| {
            d.borrow_mut().add_paint_layer().unwrap();
        });
        let next = onion_job().unwrap();
        assert_ne!(next.generation, job.generation);
        command(Request::Play).unwrap();
        assert!(onion_job().is_none());
    }
    #[test]
    fn onion_jobs_preserve_live_edits_and_hide_during_playback() {
        use lumapaint_core::document::animation::OnionSettings;
        let _restore = setup();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.capture_animation_cel("layer-1", 0, false).unwrap();
            doc.begin(
                lumapaint_core::document::Point { x: 30., y: 30. },
                Brush::default(),
            )
            .unwrap();
            doc.finish();
            doc.capture_animation_cel("layer-1", 6, false).unwrap();
            doc.blank_animation_cel("layer-1", 3).unwrap();
        });
        command(Request::Get).unwrap();
        command(Request::Onion {
            document_id: identity().0,
            settings: OnionSettings {
                enabled: true,
                ..Default::default()
            },
        })
        .unwrap();
        command(Request::Seek { frame: 3 }).unwrap();
        let job = onion_job().unwrap();
        assert_eq!(job.references.len(), 1);
        assert_eq!(job.references[0].frame, 6);
        assert!(
            onion_job().is_none(),
            "Unchanged references must not repeat worker jobs"
        );
        command(Request::Seek { frame: 4 }).unwrap();
        assert!(
            onion_job().is_none(),
            "A held exposure reuses the existing reference texture"
        );
        command(Request::Load {
            layer: "layer-1".into(),
            frame: 3,
        })
        .unwrap();
        DOCUMENT.with(|d| {
            d.borrow_mut()
                .begin(
                    lumapaint_core::document::Point { x: 50., y: 50. },
                    Brush::default(),
                )
                .unwrap()
        });
        let before = identity().1;
        let sources = thumbnail_sources(crate::timeline::ThumbnailRequest {
            library: false,
            document_id: identity().0,
            cels: vec![("layer-1".into(), 6)],
        })
        .unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(identity().1, before);
        assert!(DOCUMENT.with(|d| d.borrow().has_active_stroke()));
        let mut settings = DOCUMENT.with(|d| d.borrow().animation().onion.clone());
        settings.opacity = 50.;
        command(Request::Onion {
            document_id: identity().0,
            settings,
        })
        .unwrap();
        assert_eq!(
            DOCUMENT.with(|d| d.borrow().animation().tracks["layer-1"].cels[&3]
                .strokes
                .len()),
            1
        );
        let current = onion_job().unwrap();
        assert_ne!(current.generation, job.generation);
        command(Request::Play).unwrap();
        assert!(onion_job().is_none());
        RUNTIMES.with(|r| {
            let r = r.borrow();
            let runtime = &r[&current_label()];
            assert!(!runtime.onion_ready);
            assert!(runtime.onion_key.is_none());
            assert_ne!(runtime.onion_generation, current.generation);
        });
        let mut settings = DOCUMENT.with(|d| d.borrow().animation().onion.clone());
        settings.before = 3;
        assert!(
            command(Request::Onion {
                document_id: identity().0,
                settings
            })
            .unwrap()
            .playing,
            "Reference preferences must not stop playback"
        );
        command(Request::Pause).unwrap();
        assert!(onion_job().is_some());
        assert!(thumbnail_sources(crate::timeline::ThumbnailRequest {
            library: false,
            document_id: identity().0.wrapping_add(1),
            cels: vec![]
        })
        .is_err());
        assert!(command(Request::Onion {
            document_id: identity().0.wrapping_add(1),
            settings: OnionSettings::default()
        })
        .is_err());
        close(&current_label());
        assert!(onion_job().is_none());
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
    fn transport_returns_metadata_only_when_needed_and_distinguishes_closed_documents() {
        let _restore = setup();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            let layer = doc.add_vector_layer().unwrap();
            doc.set_animation_key(
                &layer,
                lumapaint_core::document::animation::Property::X,
                Keyframe {
                    frame: 0,
                    value: 12.,
                    interpolation: lumapaint_core::document::animation::Interpolation::Hold,
                },
            )
            .unwrap();
            doc.set_motion_clip(
                &layer,
                lumapaint_core::motion::Clip {
                    id: "transport".into(),
                    alternate_text: "large metadata ".repeat(200),
                    ..Default::default()
                },
            )
            .unwrap();
        });
        let full = command(Request::Get).unwrap();
        assert!(!full.data_unchanged);
        let bytes = serde_json::to_vec(&full).unwrap().len();
        assert!(bytes > 3000);
        for request in [
            Request::Seek { frame: 0 },
            Request::Seek { frame: 1 },
            Request::Pause,
            Request::Refresh,
            Request::Stop,
        ] {
            let response = command(request).unwrap();
            assert!(response.data.is_none());
            assert!(response.data_unchanged);
            assert!(serde_json::to_vec(&response).unwrap().len() < 128);
        }
        DOCUMENT.with(|d| {
            d.borrow_mut()
                .set_animation_settings(12, 120, true)
                .unwrap()
        });
        let changed = command(Request::Seek { frame: 2 }).unwrap();
        assert!(!changed.data_unchanged);
        assert_eq!(changed.data.unwrap().fps, 12);
        assert!(command(Request::Get).unwrap().data.is_some());
        let target = DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            let target = doc
                .snapshot()
                .layers
                .into_iter()
                .find(|layer| layer.id != doc.selected_layer_id())
                .unwrap()
                .id;
            doc.select_layer(target.clone()).unwrap();
            target
        });
        let selection = command(Request::Refresh).unwrap();
        assert_eq!(selection.data.unwrap().selected_layer, target);
        assert!(command(Request::Refresh).unwrap().data_unchanged);
        DOCUMENT_OPEN.with(|open| open.set(false));
        let closed = command(Request::Tick).unwrap();
        assert!(closed.data.is_none());
        assert!(!closed.data_unchanged);
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
        assert!(command(Request::Refresh).unwrap().data.is_none());
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
                d.borrow().animation().tracks["layer-1"].cels[&0]
                    .strokes
                    .len(),
                2
            )
        });
    }
    #[test]
    fn cross_track_edits_return_each_selection_and_reject_the_whole_stale_or_colliding_edit() {
        use lumapaint_core::document::animation::{ExposureBatchEdit, ExposureTrackEdit};
        let _restore = setup();
        let layer = DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.set_animation_settings(24, 12, true).unwrap();
            let layer = doc.add_paint_layer().unwrap();
            for id in ["layer-1", layer.as_str()] {
                for f in [0, 4, 8] {
                    doc.capture_animation_cel(id, f, true).unwrap();
                    doc.edit_animation_exposure(id, f, f, f + 2).unwrap();
                }
            }
            layer
        });
        command(Request::Seek { frame: 0 }).unwrap();
        let selected = command(Request::Select {
            layer: layer.clone(),
        })
        .unwrap();
        assert!(selected.preview);
        let jobs = || {
            vec![
                ExposureTrackEdit {
                    layer: "layer-1".into(),
                    frames: vec![0],
                    edit: ExposureBatchEdit::Shift {
                        delta: 1,
                        ripple: false,
                    },
                },
                ExposureTrackEdit {
                    layer: layer.clone(),
                    frames: vec![0],
                    edit: ExposureBatchEdit::Shift {
                        delta: 1,
                        ripple: false,
                    },
                },
            ]
        };
        let (id, revision) = identity();
        let response = command(Request::ExposureGroup {
            document_id: id,
            revision,
            edits: jobs(),
        })
        .unwrap();
        assert_eq!(
            response.exposure_group_selection,
            Some(vec![("layer-1".into(), vec![1]), (layer.clone(), vec![1])])
        );
        assert!(response.preview);
        let changed = DOCUMENT.with(|d| d.borrow_mut().encode().unwrap());
        assert!(command(Request::ExposureGroup {
            document_id: id,
            revision,
            edits: jobs()
        })
        .is_err());
        assert!(command(Request::ExposureGroup {
            document_id: id + 1,
            revision: identity().1,
            edits: jobs()
        })
        .is_err());
        let mut invalid = jobs();
        invalid[0].frames = vec![1];
        invalid[1].frames = vec![1];
        invalid[1].edit = ExposureBatchEdit::Shift {
            delta: 3,
            ripple: false,
        };
        assert!(command(Request::ExposureGroup {
            document_id: id,
            revision: identity().1,
            edits: invalid
        })
        .is_err());
        assert_eq!(DOCUMENT.with(|d| d.borrow_mut().encode().unwrap()), changed);
        DOCUMENT.with(|d| {
            d.borrow_mut().undo();
        });
        assert!(DOCUMENT.with(|d| d.borrow().animation().tracks["layer-1"]
            .exposures
            .contains_key(&0)));
        assert!(DOCUMENT.with(|d| d.borrow().animation().tracks[&layer]
            .exposures
            .contains_key(&0)));
        refresh_history_preview();
        assert!(active());
        let refreshed = command(Request::Refresh).unwrap();
        assert!(refreshed.preview);
        assert_eq!(refreshed.frame, response.frame);
        DOCUMENT.with(|d| {
            d.borrow_mut().redo();
        });
        let refreshed = command(Request::Refresh).unwrap();
        assert!(refreshed.preview);
        assert_eq!(refreshed.frame, response.frame);
    }
    #[test]
    fn library_drawing_previews_and_save_do_not_create_exposures() {
        let _restore = setup();
        let cel = DOCUMENT.with(|d| d.borrow_mut().create_animation_cel("layer-1").unwrap());
        let id = identity().0;
        command(Request::LoadCel {
            document_id: id,
            layer: "layer-1".into(),
            cel,
        })
        .unwrap();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.begin(
                lumapaint_core::document::Point { x: 20., y: 20. },
                Brush::default(),
            )
            .unwrap();
            doc.finish();
        });
        command(Request::Get).unwrap();
        let request = || crate::timeline::ThumbnailRequest {
            document_id: id,
            cels: vec![("layer-1".into(), cel)],
            library: true,
        };
        let sources = thumbnail_sources(request()).unwrap();
        assert_eq!(sources[0].cel.strokes.len(), 1);
        DOCUMENT.with(|d| {
            assert!(d.borrow().animation().tracks["layer-1"].cel_library[&cel]
                .drawing
                .strokes
                .is_empty())
        });
        flush_editing().unwrap();
        let before = editing_library_drawing();
        DOCUMENT.with(|d| {
            d.borrow_mut().undo();
        });
        restore_library_history(before);
        assert_eq!(
            DOCUMENT.with(|d| d.borrow().committed_paint_strokes().count()),
            0
        );
        let before = editing_library_drawing();
        DOCUMENT.with(|d| {
            d.borrow_mut().redo();
        });
        restore_library_history(before);
        assert_eq!(
            DOCUMENT.with(|d| d.borrow().committed_paint_strokes().count()),
            1
        );
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            let restored = Document::decode(&doc.encode().unwrap()).unwrap();
            let track = &restored.animation().tracks["layer-1"];
            assert!(track.exposures.is_empty());
            assert_eq!(track.cel_library[&cel].drawing.strokes.len(), 1);
            doc.begin(
                lumapaint_core::document::Point { x: 40., y: 40. },
                Brush::default(),
            )
            .unwrap();
            doc.finish();
        });
        command(Request::Seek { frame: 1 }).unwrap();
        DOCUMENT.with(|d| {
            let doc = d.borrow();
            assert!(doc.animation().tracks["layer-1"].exposures.is_empty());
            assert_eq!(
                doc.animation().tracks["layer-1"].cel_library[&cel]
                    .drawing
                    .strokes
                    .len(),
                2
            );
        });
        assert!(command(Request::LoadCel {
            document_id: id + 1,
            layer: "layer-1".into(),
            cel
        })
        .is_err());
    }
    #[test]
    fn unrecorded_shared_cel_thumbnails_follow_drawing_without_mutating_history() {
        let _restore = setup();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.capture_animation_cel("layer-1", 0, true).unwrap();
            let cel = doc.animation().tracks["layer-1"].exposures[&0].cel;
            doc.assign_animation_cel("layer-1", 5, cel).unwrap();
        });
        command(Request::Load {
            layer: "layer-1".into(),
            frame: 2,
        })
        .unwrap();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.begin(
                lumapaint_core::document::Point { x: 20., y: 20. },
                Brush::default(),
            )
            .unwrap();
            doc.finish();
        });
        let revision = identity().1;
        let sources = thumbnail_sources(crate::timeline::ThumbnailRequest {
            document_id: identity().0,
            cels: vec![("layer-1".into(), 0), ("layer-1".into(), 5)],
            library: false,
        })
        .unwrap();
        assert_eq!(sources[0].cel.strokes.len(), 1);
        assert!(std::sync::Arc::ptr_eq(&sources[0].cel, &sources[1].cel));
        assert_eq!(identity().1, revision);
        DOCUMENT.with(|d| {
            d.borrow_mut().undo();
        });
        let sources = thumbnail_sources(crate::timeline::ThumbnailRequest {
            document_id: identity().0,
            cels: vec![("layer-1".into(), 0)],
            library: false,
        })
        .unwrap();
        assert!(sources[0].cel.strokes.is_empty());
    }
    #[test]
    fn undoing_an_assignment_does_not_redirect_pending_drawing() {
        let _restore = setup();
        let (a, b) = DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            let a = doc.create_animation_cel("layer-1").unwrap();
            let b = doc.create_animation_cel("layer-1").unwrap();
            doc.assign_animation_cel("layer-1", 0, a).unwrap();
            doc.assign_animation_cel("layer-1", 0, b).unwrap();
            (a, b)
        });
        command(Request::Load {
            layer: "layer-1".into(),
            frame: 0,
        })
        .unwrap();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            doc.undo(); // Loaded drawing history.
            doc.undo(); // Exposure assignment history.
            assert_eq!(doc.animation().tracks["layer-1"].exposures[&0].cel, a);
            doc.begin(
                lumapaint_core::document::Point { x: 20., y: 20. },
                Brush::default(),
            )
            .unwrap();
            doc.finish();
        });
        command(Request::Seek { frame: 1 }).unwrap();
        DOCUMENT.with(|d| {
            let doc = d.borrow();
            let track = &doc.animation().tracks["layer-1"];
            assert!(track.cel_library[&a].drawing.strokes.is_empty());
            assert_eq!(track.cel_library[&b].drawing.strokes.len(), 1);
            assert_eq!(track.exposures[&0].cel, a);
        });
    }
    #[test]
    fn editing_existing_key_rebuilds_preview_at_its_frame() {
        use lumapaint_core::document::animation::{Interpolation, Property};
        let _restore = setup();
        let layer = DOCUMENT.with(|d| d.borrow_mut().add_vector_layer().unwrap());
        DOCUMENT.with(|d| {
            let object = serde_json::from_value(serde_json::json!({
                "id":"square", "name":"Square", "path":{"data":"M8 8H24V24H8Z","fillRule":"nonZero"},
                "transform":[1,0,0,1,0,0], "fill":{"color":[255,0,0,255]},
                "stroke":null,"strokeWidth":0,"visible":true
            })).unwrap();
            d.borrow_mut().upsert_vector_object(&layer, object).unwrap();
        });
        command(Request::Get).unwrap();
        let revision = identity().1;
        assert!(command(Request::Key {
            document_id: Some(identity().0.wrapping_add(1)),
            layer: layer.clone(),
            frame: 10,
            property: Property::X,
            value: 999.,
            interpolation: Interpolation::Linear,
        })
        .is_err());
        assert_eq!(identity().1, revision);
        for (property, first, second) in [
            (Property::X, 2., 17.),
            (Property::Y, 3., -9.),
            (Property::ScaleX, 100., 125.),
            (Property::ScaleY, 100., 75.),
            (Property::Rotation, 0., 90.),
            (Property::Opacity, 100., 25.),
        ] {
            for value in [first, second] {
                let result = command(Request::Key {
                    document_id: Some(identity().0),
                    layer: layer.clone(),
                    frame: 10,
                    property,
                    value,
                    interpolation: Interpolation::Linear,
                })
                .unwrap();
                assert_eq!(result.frame, 10);
                assert!(result.preview);
                RUNTIMES.with(|r| {
                    let runtimes = r.borrow();
                    let preview = runtimes[&current_label()].preview.as_ref().unwrap();
                    assert_eq!(
                        preview.animation().tracks[&layer].channels[&property][0].value,
                        value
                    );
                    let mut expected = DOCUMENT.with(|d| d.borrow().clone());
                    DOCUMENT.with(|d| expected.evaluate_animation(&d.borrow(), 10));
                    let picture = preview.svg_layers().find(|l| l.id == layer).unwrap();
                    let matrix = picture.vector_objects[0].transform;
                    match property {
                        Property::X => assert!((matrix[4] - value).abs() < 0.001),
                        Property::Y => assert!((matrix[5] - value).abs() < 0.001),
                        Property::ScaleX => assert!((matrix[0] - value / 100.).abs() < 0.001),
                        Property::ScaleY => assert!((matrix[3] - value / 100.).abs() < 0.001),
                        Property::Rotation => {
                            assert!((matrix[0] - value.to_radians().cos() * 1.25).abs() < 0.001);
                            assert!((matrix[1] - value.to_radians().sin() * 1.25).abs() < 0.001);
                        }
                        Property::Opacity => {
                            assert!((picture.opacity - value / 100.).abs() < 0.001)
                        }
                    }
                    assert_eq!(
                        serde_json::to_value(preview.document_state()).unwrap(),
                        serde_json::to_value(expected.document_state()).unwrap()
                    );
                });
            }
        }
    }
    #[test]
    fn timeline_refreshes_current_frame_preview_stops_playback_and_releases_closed_document() {
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
        assert!(response.preview);
        assert!(!response.playing);
        DOCUMENT_OPEN.with(|o| o.set(false));
        assert!(command(Request::Get).unwrap().data.is_none());
        assert!(RUNTIMES.with(|r| r.borrow().get(&current_label()).is_none()));
    }
}
