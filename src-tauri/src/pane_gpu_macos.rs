//! Independently retained inactive-pane surfaces. Canonical documents stay in the session.
use super::*;
struct PaneSurface {
    renderer: Renderer,
    view: Retained<PaintView>,
    charge: u64,
    used: u64,
}
impl Drop for PaneSurface {
    fn drop(&mut self) {
        self.view.stop_frames();
        self.view.removeFromSuperview();
    }
}
thread_local! {static PANES:RefCell<std::collections::HashMap<(String,u64),PaneSurface>>=RefCell::new(std::collections::HashMap::new());static CLOCK:std::cell::Cell<u64>=const{std::cell::Cell::new(0)};}
thread_local! {static TOKENS:RefCell<std::collections::HashMap<(String,u64),(u64,bool)>>=RefCell::new(std::collections::HashMap::new());}
fn accept(previous: Option<(u64, bool)>, generation: u64, visible: bool) -> bool {
    !previous.is_some_and(|(old, show)| generation < old || (generation == old && !show && visible))
}
pub(super) fn close_window(label: &str) {
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
        return Ok(true);
    }
    let source = pane_preview_document(id)?;
    let (w, h, layers) = match &source {
        super::super::ThumbnailDocument::Standard(d) => {
            let (w, h) = d.dimensions();
            (w, h, d.visible_svg_layers().count() as u64 + 1)
        }
        super::super::ThumbnailDocument::Tiled(d) => {
            let (w, h) = d.dimensions();
            (w, h, 1)
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
    let doc_color = match &source {
        super::super::ThumbnailDocument::Standard(d) => d.snapshot().canvas_color,
        _ => CanvasColor::Transparent,
    };
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
    PANES.with(|panes| -> Result<bool, String> {
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
                    panes.insert(key.clone(), PaneSurface { renderer, view, charge, used });
                    if render_metrics_enabled() { eprintln!("LumaPaint pane_gpu create window={label} document={id}"); }
                }
                Err(error) => { view.removeFromSuperview(); return Err(error); }
            }
        }
        let pane = panes.get_mut(&key).unwrap();
        pane.used = used;
        pane.view.setFrame(frame);
        pane.view.setHidden(false);
        let mut overlays = request.overlays;
        overlays.extend(request.overlay);
        update_canvas_mask(&pane.view, overlays);
        let before = pane.renderer.presentation_serial();
        let result = match &source {
            super::super::ThumbnailDocument::Standard(doc) => {
                pane.renderer.clear_tiled_preview();
                pane.renderer.render(viewport, doc)
            }
            super::super::ThumbnailDocument::Tiled(doc) => {
                pane.renderer.install_tiled_preview(doc)?;
                pane.renderer.render(viewport, &Document::default())
            }
        };
        if result.is_err() { pane.view.setHidden(true); }
        result?;
        let presented = pane.renderer.presentation_serial() > before;
        if render_metrics_enabled() {
            eprintln!("LumaPaint pane_gpu present window={label} document={id} presented={presented} serial={}", pane.renderer.presentation_serial());
        }
        Ok(presented)
    })
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
