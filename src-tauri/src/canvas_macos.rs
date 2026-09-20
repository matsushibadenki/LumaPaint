//! AppKit ownership is isolated here; no Apple types enter the renderer or document core.
use super::{CanvasInfo, CanvasRequest, DocumentAction};
use lumapaint_core::document::{Brush, ColorMode, ColorProfile, Document, DocumentSnapshot};
use lumapaint_renderer::{validate_svg, wgpu, Renderer, Viewport};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSView};
use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::sync::OnceLock;
use std::{cell::RefCell, ffi::c_void, ptr::NonNull};
use tauri::{Emitter, Manager};

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
pub fn initialize(app: tauri::AppHandle) {
    let _ = APP.set(app);
    start_recovery();
}

define_class!(
    #[unsafe(super(NSView))]
    #[ivars = ()]
    struct PaintView;
    impl PaintView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool { true }
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool { true }
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool { true }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() { window.makeFirstResponder(Some(self)); }
            self.pointer(event, 0);
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) { self.pointer(event, 1); }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) { self.pointer(event, 2); }
        #[unsafe(method(performKeyEquivalent:))]
        fn key_equivalent(&self, event: &NSEvent) -> bool {
            if event.modifierFlags().contains(NSEventModifierFlags::Command) && [1, 31].contains(&event.keyCode()) {
                let action = if event.keyCode() == 31 { super::FileAction::Open }
                    else if event.modifierFlags().contains(NSEventModifierFlags::Shift) { super::FileAction::SaveAs }
                    else { super::FileAction::Save };
                if let Err(error) = file_action(action) { emit_error(error); }
                true
            } else if event.modifierFlags().contains(NSEventModifierFlags::Command) && event.keyCode() == 6 {
                let action = if event.modifierFlags().contains(NSEventModifierFlags::Shift) { DocumentAction::Redo } else { DocumentAction::Undo };
                report_edit(action); true
            } else { unsafe { msg_send![super(self), performKeyEquivalent: event] } }
        }
        #[unsafe(method(undo:))]
        fn undo_action(&self, _sender: Option<&AnyObject>) { report_edit(DocumentAction::Undo); }
        #[unsafe(method(redo:))]
        fn redo_action(&self, _sender: Option<&AnyObject>) { report_edit(DocumentAction::Redo); }
    }
);

impl PaintView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let view = Self::alloc(mtm).set_ivars(());
        // SAFETY: initializing the allocated NSView subclass once on the main thread.
        unsafe { msg_send![super(view), initWithFrame: frame] }
    }

    fn pointer(&self, event: &NSEvent, phase: u8) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let Some(viewport) = CANVAS.with(|slot| slot.borrow().as_ref().map(|c| c.viewport)) else {
            return;
        };
        let point = viewport.document_point(point.x as f32, point.y as f32);
        let result = DOCUMENT
            .with(|doc| {
                let mut doc = doc.borrow_mut();
                let result = if phase == 0 {
                    BRUSH
                        .with(|brush| doc.begin(point, *brush.borrow()))
                        .map(|_| ())
                } else {
                    doc.extend(point)
                };
                if phase == 2 || result.is_err() {
                    doc.finish();
                }
                result
            })
            .and_then(|_| redraw());
        if let Err(error) = result {
            emit_error(error);
        }
        if phase == 2 {
            emit_document();
        }
    }
}

fn emit_error(error: String) {
    if let Some(app) = APP.get() {
        let _ = app.emit_to("main", "canvas-error", error);
    }
}
fn emit_document() {
    checkpoint();
    if let Some(app) = APP.get() {
        let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
        let _ = app.emit_to("main", "document-changed", snapshot);
    }
}
fn report_edit(action: DocumentAction) {
    if let Err(error) = edit(action) {
        emit_error(error);
    }
}

pub fn edit(action: DocumentAction) -> Result<DocumentSnapshot, String> {
    DOCUMENT.with(|doc| {
        let mut doc = doc.borrow_mut();
        match action {
            DocumentAction::Undo => doc.undo(),
            DocumentAction::Redo => doc.redo(),
            DocumentAction::ToggleLayer => doc.toggle_visibility(),
        }
    });
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn toggle_layer(id: String) -> Result<DocumentSnapshot, String> {
    DOCUMENT.with(|doc| doc.borrow_mut().toggle_layer(&id))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_color_mode(mode: ColorMode) -> Result<DocumentSnapshot, String> {
    DOCUMENT.with(|doc| doc.borrow_mut().set_color_mode(mode));
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_bit_depth(depth: u8) -> Result<DocumentSnapshot, String> {
    DOCUMENT.with(|doc| doc.borrow_mut().set_bit_depth(depth))?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_color_profile(profile: ColorProfile) -> Result<DocumentSnapshot, String> {
    DOCUMENT.with(|doc| doc.borrow_mut().set_color_profile(profile))?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn import_svg_layer() -> Result<DocumentSnapshot, String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("SVG", &["svg"])
        .pick_file()
    else {
        return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
    };
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.len() > 4 * 1024 * 1024 {
        return Err("SVG must be no larger than 4 MiB".into());
    }
    let source = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    validate_svg(&source)?;
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    DOCUMENT.with(|doc| doc.borrow_mut().import_svg(name, source))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

fn redraw() -> Result<(), String> {
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            DOCUMENT.with(|doc| canvas.renderer.render(canvas.viewport, &doc.borrow()))
        } else {
            Ok(())
        }
    })
}

struct Canvas {
    // Rust drops fields in declaration order: surface/renderer MUST precede its native view.
    renderer: Renderer,
    view: Retained<PaintView>,
    viewport: Viewport,
}

impl Drop for Canvas {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

thread_local! {
    // This slot is accessed exclusively from Tauri's main-thread callbacks.
    static CANVAS: RefCell<Option<Canvas>> = const { RefCell::new(None) };
    static DOCUMENT: RefCell<Document> = RefCell::new(Document::default());
    static BRUSH: RefCell<Brush> = RefCell::new(Brush::default());
}

pub fn destroy() {
    DOCUMENT.with(|doc| doc.borrow_mut().finish());
    checkpoint();
    CANVAS.with(|slot| {
        slot.borrow_mut().take();
    });
}

pub fn sync(parent: *mut c_void, request: CanvasRequest) -> Result<CanvasInfo, String> {
    if SHUTTING_DOWN.with(|flag| flag.get()) {
        return Ok(CanvasInfo::inactive("hidden"));
    }
    let result = sync_inner(parent, request);
    if result.is_err() {
        destroy();
    }
    result
}

fn sync_inner(parent: *mut c_void, request: CanvasRequest) -> Result<CanvasInfo, String> {
    BRUSH.with(|brush| *brush.borrow_mut() = request.brush);
    let mtm = MainThreadMarker::new().ok_or("Native canvas requires the main thread")?;
    if !request.visible || request.width < 1.0 || request.height < 1.0 {
        destroy();
        return Ok(CanvasInfo::inactive("hidden"));
    }
    // SAFETY: Tauri provides a live WKWebView (an NSView subclass) inside with_webview,
    // on the main thread. This borrowed reference never escapes this callback.
    let parent = unsafe { parent.cast::<NSView>().as_ref() }.ok_or("Missing native webview")?;
    let Some(frame) = native_frame(
        parent.bounds(),
        parent.safeAreaInsets(),
        parent.isFlipped(),
        &request,
    ) else {
        destroy();
        return Ok(CanvasInfo::inactive("hidden"));
    };
    let scale = parent
        .window()
        .ok_or("Canvas window is not attached")?
        .backingScaleFactor();
    let viewport = Viewport::new(
        frame.size.width,
        frame.size.height,
        scale,
        request.zoom,
        request.dark,
    )?;

    CANVAS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let view = PaintView::new(mtm, frame);
            parent.addSubview(&view);
            // The view is attached before creating Metal's layer so its backing scale is known.
            let result = (|| {
                let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::METAL,
                    ..Default::default()
                });
                let handle = AppKitWindowHandle::new(NonNull::from(&*view).cast());
                // SAFETY: view is a retained live NSView on the main thread. Canvas owns it
                // until AFTER Renderer (and its surface) is dropped. No raw pointer is stored elsewhere.
                let surface = unsafe {
                    instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                        raw_display_handle: RawDisplayHandle::AppKit(AppKitDisplayHandle::new()),
                        raw_window_handle: RawWindowHandle::AppKit(handle),
                    })
                }
                .map_err(|e| e.to_string())?;
                pollster::block_on(Renderer::new(&instance, surface, viewport))
            })();
            match result {
                Ok(renderer) => {
                    *slot = Some(Canvas {
                        renderer,
                        view,
                        viewport,
                    })
                }
                Err(error) => {
                    view.removeFromSuperview();
                    return Err(error);
                }
            }
        }
        let canvas = slot.as_mut().ok_or("Canvas initialization failed")?;
        canvas.view.setFrame(frame);
        canvas.viewport = viewport;
        DOCUMENT.with(|doc| canvas.renderer.render(viewport, &doc.borrow()))?;
        Ok(CanvasInfo {
            status: "ready",
            backend: canvas.renderer.backend.clone(),
            adapter_name: canvas.renderer.adapter_name.clone(),
            physical_width: viewport.width,
            physical_height: viewport.height,
            scale_factor: scale,
            document: Some(DOCUMENT.with(|doc| doc.borrow().snapshot())),
        })
    })
}

fn native_frame(
    bounds: NSRect,
    safe: NSEdgeInsets,
    flipped: bool,
    request: &CanvasRequest,
) -> Option<NSRect> {
    // WKWebView's CSS viewport starts inside its safe area, while child NSViews use
    // the full native bounds (including the macOS title bar). Respect both origins.
    let content_width = bounds.size.width - safe.left - safe.right;
    let content_height = bounds.size.height - safe.top - safe.bottom;
    let left = request.x.max(0.0);
    let top = request.y.max(0.0);
    let width = (request.x + request.width).min(content_width) - left;
    let height = (request.y + request.height).min(content_height) - top;
    if width < 1.0 || height < 1.0 {
        return None;
    }
    let y = if flipped {
        safe.top + top
    } else {
        safe.bottom + content_height - top - height
    };
    Some(NSRect::new(
        NSPoint::new(bounds.origin.x + safe.left + left, bounds.origin.y + y),
        NSSize::new(width, height),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_for_title_bar_in_both_coordinate_orientations() {
        let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1200.0, 800.0));
        let safe = NSEdgeInsets {
            top: 32.0,
            left: 0.0,
            bottom: 0.0,
            right: 0.0,
        };
        let request = CanvasRequest {
            x: 79.0,
            y: 228.0,
            width: 1042.0,
            height: 436.0,
            zoom: 1.0,
            dark: false,
            visible: true,
            brush: Brush::default(),
        };
        let flipped = native_frame(bounds, safe, true, &request).unwrap();
        let unflipped = native_frame(bounds, safe, false, &request).unwrap();
        assert_eq!(flipped.origin.y, 260.0);
        assert_eq!(unflipped.origin.y, 104.0);
        assert_eq!(flipped.size.height, 436.0);
    }

    #[test]
    fn clips_to_safe_content_bounds_and_skips_invisible_frames() {
        let bounds = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 480.0));
        let safe = NSEdgeInsets {
            top: 32.0,
            left: 0.0,
            bottom: 0.0,
            right: 0.0,
        };
        let mut request = CanvasRequest {
            x: -10.0,
            y: 400.0,
            width: 700.0,
            height: 100.0,
            zoom: 1.0,
            dark: false,
            visible: true,
            brush: Brush::default(),
        };
        let frame = native_frame(bounds, safe, true, &request).unwrap();
        assert_eq!(frame.origin, NSPoint::new(0.0, 432.0));
        assert_eq!(frame.size, NSSize::new(640.0, 48.0));
        request.y = 500.0;
        assert!(native_frame(bounds, safe, true, &request).is_none());
    }
}

thread_local! {
    static SHUTTING_DOWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PROJECT_PATH: RefCell<Option<std::path::PathBuf>> = const { RefCell::new(None) };
}

pub fn confirm_discard() -> bool {
    if !DOCUMENT.with(|doc| doc.borrow().snapshot().dirty) {
        return true;
    }
    let Some(window) = APP.get().and_then(|app| app.get_webview_window("main")) else {
        return false;
    };
    rfd::MessageDialog::new()
        .set_parent(&window)
        .set_title("LumaPaint")
        .set_description("未保存の変更を破棄しますか？先に保存する場合はキャンセルしてください。\nDiscard unsaved changes? Cancel to save first.\n放弃未保存的更改？如需保存，请先取消。")
        .set_buttons(rfd::MessageButtons::OkCancel)
        .set_level(rfd::MessageLevel::Warning)
        .show() == rfd::MessageDialogResult::Ok
}

pub fn file_action(action: super::FileAction) -> Result<DocumentSnapshot, String> {
    use super::FileAction;
    match action {
        FileAction::Open => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("LumaPaint", &["lumapaint"])
                .pick_file()
            else {
                return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
            };
            // Validate before replacing anything or asking to discard work.
            let loaded = crate::project_file::read(&path)?;
            for layer in loaded.svg_layers() {
                validate_svg(&layer.source)?;
            }
            if confirm_discard() {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                DOCUMENT.with(|doc| doc.borrow_mut().replace_loaded(loaded, name));
                PROJECT_PATH.with(|slot| *slot.borrow_mut() = Some(path));
                if let Err(error) = redraw() {
                    emit_error(error);
                }
            }
        }
        FileAction::Save | FileAction::SaveAs => {
            let existing = PROJECT_PATH.with(|slot| slot.borrow().clone());
            let path = if matches!(action, FileAction::Save) && existing.is_some() {
                existing
            } else {
                let mut dialog = rfd::FileDialog::new().add_filter("LumaPaint", &["lumapaint"]);
                if let Some(existing) = existing.as_ref() {
                    if let Some(parent) = existing.parent() {
                        dialog = dialog.set_directory(parent);
                    }
                    dialog = dialog
                        .set_file_name(existing.file_name().unwrap_or_default().to_string_lossy());
                } else {
                    dialog = dialog.set_file_name("Untitled.lumapaint");
                }
                dialog.save_file()
            };
            if let Some(path) = path {
                let bytes = DOCUMENT.with(|doc| doc.borrow_mut().encode())?;
                crate::project_file::write(&path, &bytes)?;
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                DOCUMENT.with(|doc| doc.borrow_mut().mark_saved(name));
                PROJECT_PATH.with(|slot| *slot.borrow_mut() = Some(path));
            }
        }
    }
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn shutdown() {
    SHUTTING_DOWN.with(|flag| flag.set(true));
    destroy();
    RECOVERY.with(|slot| {
        if let Some(recovery) = slot.borrow_mut().as_mut() {
            recovery.stop();
        }
    });
}

thread_local! {
    static RECOVERY: RefCell<Option<crate::recovery::Recovery>> = const { RefCell::new(None) };
    static RECOVERY_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static CHECKPOINT_REVISION: std::cell::Cell<Option<(u64, bool)>> = const { std::cell::Cell::new(None) };
    static RECOVERY_DISCARDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn start_recovery() {
    let result = APP
        .get()
        .ok_or_else(|| "Application is not initialized".to_string())
        .and_then(|app| app.path().app_local_data_dir().map_err(|e| e.to_string()))
        .and_then(|directory| crate::recovery::Recovery::start(directory.join("recovery")));
    match result {
        Ok(recovery) => {
            RECOVERY.with(|slot| *slot.borrow_mut() = Some(recovery));
            RECOVERY_ERROR.with(|error| *error.borrow_mut() = None);
        }
        Err(error) => RECOVERY_ERROR.with(|slot| *slot.borrow_mut() = Some(error)),
    }
}
fn checkpoint() {
    if RECOVERY_DISCARDED.with(|flag| flag.get()) {
        return;
    }
    let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
    let key = (snapshot.revision, snapshot.dirty);
    if CHECKPOINT_REVISION.with(|last| last.get() == Some(key)) {
        return;
    }
    RECOVERY.with(|slot| {
        if let Some(recovery) = slot.borrow().as_ref() {
            recovery.checkpoint(DOCUMENT.with(|doc| doc.borrow().clone()));
            CHECKPOINT_REVISION.with(|last| last.set(Some(key)));
        }
    });
}
pub fn discard_recovery() {
    RECOVERY_DISCARDED.with(|flag| flag.set(true));
    RECOVERY.with(|slot| {
        if let Some(recovery) = slot.borrow().as_ref() {
            recovery.clear();
        }
    });
}
pub fn recovery_info() -> Result<crate::recovery::Info, String> {
    RECOVERY.with(|slot| match slot.borrow().as_ref() {
        Some(recovery) => recovery.info(),
        None => Ok(crate::recovery::Info {
            status: crate::recovery::Status {
                error: RECOVERY_ERROR.with(|error| error.borrow().clone()),
                ..Default::default()
            },
            candidates: vec![],
        }),
    })
}
pub fn retry_recovery() -> Result<(), String> {
    if RECOVERY.with(|slot| slot.borrow().is_none()) {
        start_recovery();
    }
    CHECKPOINT_REVISION.with(|last| last.set(None));
    checkpoint();
    Ok(())
}
pub fn restore_recovery(id: String) -> Result<DocumentSnapshot, String> {
    let document = RECOVERY.with(|slot| {
        slot.borrow()
            .as_ref()
            .ok_or("Recovery is unavailable")?
            .read_candidate(&id)
    })?;
    for layer in document.svg_layers() {
        validate_svg(&layer.source)?;
    }
    if !confirm_discard() {
        return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
    }
    DOCUMENT.with(|doc| doc.borrow_mut().replace_recovered(document));
    PROJECT_PATH.with(|path| *path.borrow_mut() = None);
    RECOVERY.with(|slot| {
        if let Some(recovery) = slot.borrow_mut().as_mut() {
            recovery.adopt(&id);
        }
    });
    emit_document();
    if let Err(error) = redraw() {
        emit_error(error);
    }
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
