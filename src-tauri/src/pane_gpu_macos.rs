//! Independently retained inactive-pane surfaces. Canonical documents stay in the session.
use super::*;
struct PaneSurface {
    renderer: Renderer,
    view: Retained<PaintView>,
    charge: u64,
    used: u64,
    viewport: Viewport,
    last_revision: Option<u64>,
}
impl Drop for PaneSurface {
    fn drop(&mut self) {
        self.view.stop_frames();
        self.view.removeFromSuperview();
    }
}
thread_local! {static PANES:RefCell<std::collections::HashMap<(String,u64),PaneSurface>>=RefCell::new(std::collections::HashMap::new());static CLOCK:std::cell::Cell<u64>=const{std::cell::Cell::new(0)};}
thread_local! {static TOKENS:RefCell<std::collections::HashMap<(String,u64),(u64,bool)>>=RefCell::new(std::collections::HashMap::new());}
thread_local! {static TIMERS: RefCell<std::collections::HashMap<String,Retained<objc2_foundation::NSTimer>>>=RefCell::new(std::collections::HashMap::new());}
struct InputPriority {
    at: Instant,
    pending_at: Instant,
    serial: Option<u64>,
}
impl InputPriority {
    fn request(previous: Option<&Self>, serial: Option<u64>, now: Instant) -> Self {
        Self {
            at: now,
            pending_at: previous
                .filter(|p| p.serial == serial)
                .map_or(now, |p| p.pending_at),
            serial,
        }
    }
}
thread_local! {
    static PRIORITY:RefCell<std::collections::HashMap<String,InputPriority>>=RefCell::new(std::collections::HashMap::new());
    static SERVICED:RefCell<std::collections::HashMap<String,Instant>>=RefCell::new(std::collections::HashMap::new());
}
fn priority_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED
        .get_or_init(|| std::env::var_os("LUMAPAINT_PANE_INPUT_PRIORITY").is_none_or(|v| v != "0"))
}
fn continuous_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("LUMAPAINT_PANE_CONTINUOUS_PRIORITY").is_none_or(|v| v != "0")
    })
}
fn source_reuse_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED
        .get_or_init(|| std::env::var_os("LUMAPAINT_PANE_SOURCE_REUSE").is_none_or(|v| v != "0"))
}
fn preview_source(id: u64) -> Result<super::super::ThumbnailDocument, String> {
    let started = Instant::now();
    let source = pane_preview_document(id)?;
    if matches!(&source, super::super::ThumbnailDocument::Standard(_)) {
        lumapaint_core::performance::count("host.pane_render_forks", 1);
    }
    if render_metrics_enabled() {
        eprintln!(
            "lumapaint-pane-source document={id} host_ns={}",
            started.elapsed().as_nanos()
        );
    }
    Ok(source)
}
pub(super) fn note_input() {
    if !priority_enabled() {
        return;
    }
    let label = current_label();
    let serial = CANVAS.with(|c| {
        c.borrow()
            .as_ref()
            .map(|c| c.renderer.presentation_serial())
    });
    PRIORITY.with(|p| {
        let mut priorities = p.borrow_mut();
        let now = Instant::now();
        let next = InputPriority::request(priorities.get(&label), serial, now);
        priorities.insert(label, next);
    });
}
fn input_budget() -> pane_scheduling::BackgroundBudget {
    use pane_scheduling::{budget, BackgroundBudget};
    if !priority_enabled() {
        return BackgroundBudget::Normal;
    }
    let label = current_label();
    let serial = CANVAS.with(|c| {
        c.borrow()
            .as_ref()
            .map(|c| c.renderer.presentation_serial())
    });
    let service = SERVICED
        .with(|s| s.borrow().get(&label).map(Instant::elapsed))
        .unwrap_or(std::time::Duration::from_millis(50));
    PRIORITY.with(|p| {
        p.borrow()
            .get(&label)
            .map_or(BackgroundBudget::Normal, |p| {
                let pending = p.serial.is_some() && p.serial == serial;
                if continuous_enabled() {
                    pane_scheduling::continuous_budget(
                        Some(p.at.elapsed()),
                        pending.then(|| p.pending_at.elapsed()),
                        service,
                    )
                } else {
                    budget(Some(p.at.elapsed()), pending, service)
                }
            })
    })
}
pub(super) fn refresh_clock() {
    let label = current_label();
    let target = PANES.with(|p| {
        p.borrow()
            .iter()
            .find(|((owner, id), pane)| {
                owner == &label && !pane.view.isHidden() && timeline::inactive_playing(*id)
            })
            .map(|(_, p)| p.view.clone())
    });
    let target = target.or_else(|| {
        if timeline::active_playing() {
            CANVAS.with(|c| {
                c.borrow()
                    .as_ref()
                    .filter(|c| !c.view.isHidden())
                    .map(|c| c.view.clone())
            })
        } else {
            None
        }
    });
    TIMERS.with(|t| {
        let mut timers=t.borrow_mut();
        if let Some(view)=target {
            timers.entry(label).or_insert_with(|| {
                // SAFETY: a retained main-thread PaintView implements paneTick:.
                let timer=unsafe{objc2_foundation::NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(1./60.,&view,sel!(paneTick:),None,true)};
                timer.setTolerance(0.002);
                unsafe{NSRunLoop::mainRunLoop().addTimer_forMode(&timer,NSRunLoopCommonModes);}
                timer
            });
        } else if let Some(timer)=timers.remove(&label){timer.invalidate();}
    });
}
fn with_document<T>(
    id: u64,
    f: impl FnOnce(&Document) -> Result<T, String>,
) -> Result<Option<T>, String> {
    with_pane_standard_document(id, f)?.transpose()
}

/// Both borrowed canonical content and the eager diagnostic control use the same
/// preview ownership rules. A deferred animated present must not fall back to the
/// ordinary editing document.
fn render_standard_pane(
    id: u64,
    renderer: &mut Renderer,
    viewport: Viewport,
    doc: &Document,
) -> Result<(), String> {
    if !timeline::render_inactive(id, renderer, viewport, doc, true)? {
        renderer.clear_tiled_preview();
        renderer.render(viewport, doc)?;
    }
    Ok(())
}
pub(super) fn tick(label: &str) {
    let Ok(_session) = SessionGuard::enter_render(label) else {
        return;
    };
    let started = Instant::now();
    if timeline::active_playing()
        && CANVAS.with(|c| c.borrow().as_ref().is_some_and(|c| !c.view.isHidden()))
    {
        if let Err(error) = timeline::tick_active_native() {
            emit_error(error);
        }
    }
    let budget = input_budget();
    if render_metrics_enabled() && budget != pane_scheduling::BackgroundBudget::Normal {
        eprintln!("lumapaint-pane-schedule budget={budget:?}");
    }
    if budget == pane_scheduling::BackgroundBudget::Yield {
        refresh_clock();
        return;
    }
    let limit =
        std::time::Duration::from_millis(if budget == pane_scheduling::BackgroundBudget::Limited {
            2
        } else {
            8
        });
    PANES.with(|p| {
        let mut panes = p.borrow_mut();
        let mut keys = panes
            .iter()
            .filter(|((owner, id), p)| {
                owner == label && !p.view.isHidden() && timeline::inactive_playing(*id)
            })
            .map(|(k, p)| (p.used, k.clone()))
            .collect::<Vec<_>>();
        keys.sort_by_key(|(used, _)| *used);
        for (_, key) in keys {
            let pane = panes.get_mut(&key).unwrap();
            pane.used = CLOCK.with(|c| {
                let n = c.get().wrapping_add(1);
                c.set(n);
                n
            });
            let before = Instant::now();
            match with_document(key.1, |doc| {
                timeline::render_inactive(key.1, &mut pane.renderer, pane.viewport, doc, false)
            }) {
                Ok(Some(true)) => {
                    SERVICED.with(|s| s.borrow_mut().insert(label.to_string(), Instant::now()));
                    if render_metrics_enabled() {
                        eprintln!(
                            "lumapaint-pane-frame document={} host_ns={} batch_elapsed_ns={}",
                            key.1,
                            before.elapsed().as_nanos(),
                            started.elapsed().as_nanos()
                        );
                    }
                }
                Err(error) => {
                    pane.view.setHidden(true);
                    emit_error(error);
                }
                _ => {}
            }
            // Latest wall-clock frame wins; never queue a backlog behind input.
            if started.elapsed() >= limit {
                break;
            }
        }
    });
    refresh_clock();
}
fn accept(previous: Option<(u64, bool)>, generation: u64, visible: bool) -> bool {
    !previous.is_some_and(|(old, show)| generation < old || (generation == old && !show && visible))
}
pub(super) fn close_window(label: &str) {
    #[cfg(feature = "native-input-qa")]
    native_gesture_qa::stop(label);
    PRIORITY.with(|p| p.borrow_mut().remove(label));
    SERVICED.with(|p| p.borrow_mut().remove(label));
    TIMERS.with(|t| {
        if let Some(timer) = t.borrow_mut().remove(label) {
            timer.invalidate();
        }
    });
    TOKENS.with(|t| t.borrow_mut().retain(|(owner, _), _| owner != label));
    PANES.with(|p| p.borrow_mut().retain(|(owner, _), _| owner != label));
}
pub(in crate::canvas) fn sync(
    parent: *mut c_void,
    id: u64,
    generation: u64,
    request: CanvasRequest,
) -> Result<bool, String> {
    let label = current_label();
    let key = (label.clone(), id);
    if generation == 0 || generation > 9_007_199_254_740_991 {
        return Err("Invalid GPU pane generation".into());
    }
    let visible = request.visible && request.width >= 1. && request.height >= 1.;
    let allowed = TOKENS.with(|t| {
        let mut t = t.borrow_mut();
        if !accept(t.get(&key).copied(), generation, visible) {
            return false;
        }
        t.insert(key.clone(), (generation, visible));
        true
    });
    if !allowed {
        return Ok(true);
    }

    if !request.visible || request.width < 1. || request.height < 1. {
        PANES.with(|p| {
            if let Some(p) = p.borrow().get(&key) {
                p.view.setHidden(true);
            }
        });
        refresh_clock();
        return Ok(true);
    }
    // Read scalars from canonical content before allocating a rendering fork.
    let metadata = if source_reuse_enabled() {
        with_pane_standard_document(id, |doc| {
            let (w, h) = doc.dimensions();
            (
                w,
                h,
                doc.visible_svg_layers().count() as u64 + 1,
                doc.canvas_color(),
                doc.revision(),
            )
        })?
    } else {
        None
    };
    // Tiled content keeps the existing path; no revision reuse is promised there.
    let source = if metadata.is_none() {
        Some(preview_source(id)?)
    } else {
        None
    };
    let (w, h, layers, doc_color, revision) = if let Some((w, h, layers, color, revision)) =
        metadata
    {
        (w, h, layers, color, Some(revision))
    } else {
        match source.as_ref().unwrap() {
            super::super::ThumbnailDocument::Standard(d) => {
                let (w, h) = d.dimensions();
                (
                    w,
                    h,
                    d.visible_svg_layers().count() as u64 + 1,
                    {
                        lumapaint_core::performance::count("host.pane_full_metadata_snapshots", 1);
                        d.snapshot().canvas_color
                    },
                    Some(d.revision()),
                )
            }
            super::super::ThumbnailDocument::Tiled(d) => {
                let (w, h) = d.dimensions();
                (w, h, 1, CanvasColor::Transparent, None)
            }
        }
    };
    let charge = u64::from(w) * u64::from(h) * 4 * layers;
    if charge > 128 * 1024 * 1024 {
        return Err(
            "GPU pane budget exceeded / GPU区画の容量上限を超えました / GPU窗格超出容量上限".into(),
        );
    }
    let mtm = MainThreadMarker::new().ok_or("Native canvas requires main thread")?;
    // SAFETY: supplied by Tauri's main-thread with_webview callback and never retained.
    let parent = unsafe { parent.cast::<NSView>().as_ref() }.ok_or("Missing webview")?;
    let Some(frame) = native_frame(
        parent.bounds(),
        parent.safeAreaInsets(),
        parent.isFlipped(),
        &request,
    ) else {
        PANES.with(|p| {
            if let Some(p) = p.borrow().get(&key) {
                p.view.setHidden(true);
            }
        });
        return Ok(true);
    };
    let scale = parent
        .window()
        .ok_or("Missing window")?
        .backingScaleFactor();
    let base = Viewport::new(frame.size.width, frame.size.height, scale, 1., request.dark)?
        .with_document(w, h, CanvasColor::White)?;
    let mut viewport = restore_document_view(base, pane_document_view(id));
    viewport.canvas_color = doc_color;
    DOCUMENT_VIEWS.with(|views| {
        views.borrow_mut().entry(id).or_insert(DocumentView {
            zoom: viewport.screen_zoom(),
            pan_x: viewport.pan_x,
            pan_y: viewport.pan_y,
            width: w as f32,
            height: h as f32,
        });
    });
    let used = CLOCK.with(|v| {
        let n = v.get().wrapping_add(1);
        v.set(n);
        n
    });
    let result = PANES.with(|panes| -> Result<bool, String> {
        let mut panes = panes.borrow_mut();
        if let Some(pane) = panes.get(&key) {
            // SAFETY: the view and its parent are retained on the main thread.
            let same = unsafe { pane.view.superview() }.as_deref()
                .is_some_and(|view| std::ptr::eq(view, parent));
            if !same { panes.remove(&key); }
        }
        // Recreate when document dimensions or layer allocation changes, so the
        // retained budget also accounts for growing documents.
        if panes.get(&key).is_some_and(|pane| pane.charge != charge) {
            panes.remove(&key);
        }
        if !panes.contains_key(&key) {
            loop {
                let count = panes.keys().filter(|(owner, _)| owner == &label).count();
                let bytes = panes.iter().filter(|((owner, _), _)| owner == &label)
                    .map(|(_, pane)| pane.charge).sum::<u64>();
                if count < 8 && bytes + charge <= 256 * 1024 * 1024 { break; }
                let victim = panes.iter()
                    .filter(|((owner, _), pane)| owner == &label && pane.view.isHidden())
                    .min_by_key(|(_, pane)| pane.used).map(|(key, _)| key.clone());
                if let Some(key) = victim { panes.remove(&key); }
                else { return Err("GPU pane resources busy / GPU区画のリソースが不足しています / GPU窗格资源不足".into()); }
            }
            let view = PaintView::new(mtm, frame);
            view.ivars().passive.set(true);
            view.unregisterDraggedTypes();
            view.setClipsToBounds(true);
            parent.addSubview(&view);
            let renderer = (|| {
                let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::METAL,
                    ..wgpu::InstanceDescriptor::new_without_display_handle()
                });
                let handle = AppKitWindowHandle::new(NonNull::from(&*view).cast());
                // SAFETY: the pane owns this retained NSView until after its surface/renderer drop.
                let surface = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(RawDisplayHandle::AppKit(AppKitDisplayHandle::new())),
                    raw_window_handle: RawWindowHandle::AppKit(handle),
                }) }.map_err(|error| error.to_string())?;
                pollster::block_on(Renderer::new(&instance, surface, viewport))
            })();
            match renderer {
                Ok(mut renderer) => {
                    renderer.set_memory_budget(8 * 1024 * 1024);
                    renderer.set_selection_overlay_visible(false);
                    renderer.set_image_frame_guides_visible(false);
                    panes.insert(key.clone(), PaneSurface { renderer, view, charge, used, viewport, last_revision:None });
                    if render_metrics_enabled() { eprintln!("LumaPaint pane_gpu create window={label} document={id}"); }
                }
                Err(error) => { view.removeFromSuperview(); return Err(error); }
            }
        }
        let pane = panes.get_mut(&key).unwrap();
        pane.used = used;
        let same_view=pane.viewport==viewport;
        let was_hidden=pane.view.isHidden();
        pane.viewport = viewport;
        pane.view.setFrame(frame);
        pane.view.setHidden(false);
        let mut overlays = request.overlays;
        overlays.extend(request.overlay);
        update_canvas_mask(&pane.view, overlays);
        if continuous_enabled() && same_view && !was_hidden && revision.is_some() && pane.last_revision==revision && pane.renderer.presentation_serial()>0 {
            if source.is_none() {
                lumapaint_core::performance::count("host.pane_render_forks_avoided",1);
                if render_metrics_enabled(){eprintln!("lumapaint-pane-source-avoided document={id}");}
            }
            if render_metrics_enabled(){eprintln!("lumapaint-pane-sync-skip document={id}");}
            return Ok(true);
        }
        pane.last_revision=None;
        let before = pane.renderer.presentation_serial();
        let result = match source.as_ref() {
            None => {
                // Renderer uploads and caches own their resources; this borrow
                // never leaves the synchronous main-thread render call.
                lumapaint_core::performance::count("host.pane_render_forks_avoided",1);
                lumapaint_core::performance::count("host.pane_borrowed_renders",1);
                if render_metrics_enabled(){eprintln!("lumapaint-pane-source-avoided document={id}");}
                with_document(id,|doc|render_standard_pane(id,&mut pane.renderer,viewport,doc))
                    .and_then(|rendered|rendered.ok_or_else(||"Document unavailable / ドキュメントを取得できません / 无法获取文档".to_string()))
            }
            Some(super::super::ThumbnailDocument::Standard(doc)) => {
                render_standard_pane(id,&mut pane.renderer,viewport,doc)
            }
            Some(super::super::ThumbnailDocument::Tiled(doc)) => {
                super::install_color_managed_tiled_preview(&mut pane.renderer, doc)
                    .and_then(|_| super::tiled_display_document(doc.dimensions()))
                    .and_then(|display| pane.renderer.render(viewport, &display))
            }
        };
        if result.is_err() { pane.view.setHidden(true); }
        result?;
        let presented = pane.renderer.presentation_serial() > before;
        if presented {pane.last_revision=revision;}
        if render_metrics_enabled() {
            eprintln!("LumaPaint pane_gpu present window={label} document={id} presented={presented} serial={}", pane.renderer.presentation_serial());
        }
        Ok(presented)
    });
    refresh_clock();
    result
}

#[cfg(test)]
#[test]
fn gpu_pane_rejects_cancelled_or_stale_generations() {
    assert!(accept(None, 1, true));
    assert!(accept(Some((1, true)), 1, false));
    assert!(!accept(Some((1, false)), 1, true));
    assert!(!accept(Some((2, true)), 1, false));
    assert!(accept(Some((2, false)), 3, true));
}

#[cfg(test)]
#[test]
fn repeated_input_keeps_oldest_unpresented_request() {
    let now = Instant::now();
    let first = InputPriority::request(None, Some(5), now);
    let later = now + Duration::from_millis(100);
    let updated = InputPriority::request(Some(&first), Some(5), later);
    assert_eq!(updated.at, later);
    assert_eq!(updated.pending_at, now);
    let presented = InputPriority::request(Some(&updated), Some(6), later);
    assert_eq!(presented.pending_at, later);
}
