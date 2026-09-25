//! AppKit ownership is isolated here; no Apple types enter the renderer or document core.
use super::{
    CanvasInfo, CanvasRequest, CanvasTool, DocumentAction, DocumentTabSnapshot,
    DocumentWorkspaceSnapshot,
};
use lumapaint_core::document::{
    Brush, ColorMode, ColorProfile, Document, DocumentSettings, DocumentSnapshot, LayerSettings,
    LayerSnapshot, PaintProjectionState, SelectionMode, SelectionShape, Stroke, TextSettings,
};
use lumapaint_core::graph::{ChangeTarget, ProcessingGraph};
use lumapaint_core::tiles::{TileInvalidation, TiledRasterDocument};
use lumapaint_core::vector::{
    FillRule, PathEditAction, PathOperation, VectorObject, VectorObjectKind, VectorPaint,
    VectorPath,
};
use lumapaint_renderer::frame_cache::FrameRasterCache;
use lumapaint_renderer::{
    paint_stroke_into_tiles_at_scale, project_committed_paint_layer_at_scale,
    stroke_candidate_tile_coords_at_scale, update_projected_paint_appearance, validate_svg, wgpu,
    PreparedSvgLayer, Renderer, ValidatedTileUploads, Viewport,
};
use objc2::{
    class, define_class, msg_send, rc::Retained, runtime::AnyObject, AnyThread, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::{
    NSColor, NSCursor, NSCursorFrameResizeDirections, NSCursorFrameResizePosition, NSEvent,
    NSEventModifierFlags, NSEventSubtype, NSImage, NSView,
};
use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::{sync_channel, SyncSender, TrySendError},
    OnceLock,
};
use std::time::{Duration, Instant};
use std::{cell::RefCell, ffi::c_void, ptr::NonNull};
use tauri::{Emitter, Manager};

#[path = "clipboard_macos.rs"]
mod clipboard;
#[path = "text_editor_macos.rs"]
mod text_editor;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static RASTER_SENDER: OnceLock<SyncSender<RasterJob>> = OnceLock::new();
static TILE_SENDER: OnceLock<SyncSender<TileJob>> = OnceLock::new();
static NEXT_CANVAS_TOKEN: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, PartialEq, Eq)]
struct RasterKey {
    document_id: u64,
    revision: u64,
    canvas_token: u64,
}

impl RasterKey {
    fn matches(self, document_id: u64, revision: u64, canvas_token: u64) -> bool {
        self.document_id == document_id
            && self.revision == revision
            && self.canvas_token == canvas_token
    }
}

struct RasterJob {
    key: RasterKey,
    layers: Vec<lumapaint_core::document::SvgLayer>,
    size: (u32, u32),
    queued_at: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct TileKey {
    source: RasterKey,
    scale: u32,
}

impl TileKey {
    fn matches(self, document_id: u64, revision: u64, canvas_token: u64, scale: u32) -> bool {
        self.source.matches(document_id, revision, canvas_token) && self.scale == scale
    }
}

struct TileJob {
    key: TileKey,
    work: TileWork,
    state: PaintProjectionState,
    dimensions: (u32, u32),
    queued_at: Instant,
}

enum TileWork {
    Full(Box<Document>),
    Append {
        cache: Box<TileCache>,
        stroke: Stroke,
    },
    Appearance {
        cache: Box<TileCache>,
    },
    Reconcile {
        cache: Box<TileCache>,
        document: Box<Document>,
        stroke_hint: Option<Stroke>,
    },
}

struct TileResult {
    tiles: TiledRasterDocument,
    uploads: ValidatedTileUploads,
    incremental: bool,
}

struct TileCache {
    key: TileKey,
    state: PaintProjectionState,
    dimensions: (u32, u32),
    tiles: TiledRasterDocument,
}

fn render_metrics_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("LUMAPAINT_RENDER_METRICS").is_some())
}

pub fn initialize(app: tauri::AppHandle) {
    let _ = APP.set(app.clone());
    let tile_app = app.clone();
    start_recovery();
    // Optional prewarming: rendering still initializes fonts if the worker cannot start.
    let _ = std::thread::Builder::new()
        .name("text-fonts".into())
        .spawn(lumapaint_renderer::vector::prepare_text_fonts);
    let (sender, receiver) = sync_channel::<RasterJob>(1);
    if std::thread::Builder::new()
        .name("svg-raster".into())
        .spawn(move || {
            let mut frame_cache = FrameRasterCache::default();
            while let Ok(mut job) = receiver.recv() {
                // At most one request waits while a raster is running. Keep the newest.
                while let Ok(newer) = receiver.try_recv() {
                    job = newer;
                }
                let queued_for = job.queued_at.elapsed();
                let started = Instant::now();
                let before_rasterized = frame_cache.rasterized_frames;
                let before_reused = frame_cache.reused_frames;
                let result: Result<Vec<PreparedSvgLayer>, String> = job
                    .layers
                    .iter()
                    .map(|layer| frame_cache.prepare_layer(layer, job.size.0, job.size.1))
                    .collect();
                let cpu_time = started.elapsed();
                if render_metrics_enabled() {
                    eprintln!(
                        "LumaPaint text-frame cache rasterized={} reused={}",
                        frame_cache.rasterized_frames - before_rasterized,
                        frame_cache.reused_frames - before_reused
                    );
                }
                let _ = app.run_on_main_thread(move || {
                    finish_raster_job(job.key, result, queued_for, cpu_time)
                });
            }
        })
        .is_ok()
    {
        let _ = RASTER_SENDER.set(sender);
    }
    let (sender, receiver) = sync_channel::<TileJob>(1);
    if std::thread::Builder::new()
        .name("tile-project".into())
        .spawn(move || {
            while let Ok(mut job) = receiver.recv() {
                while let Ok(newer) = receiver.try_recv() {
                    job = newer;
                }
                let queued_for = job.queued_at.elapsed();
                let started = Instant::now();
                let result = match job.work {
                    TileWork::Full(document) => {
                        project_committed_paint_layer_at_scale(&document, job.key.scale).map(
                            |tiles| {
                                let uploads = tiles.prepare_full_uploads();
                                (tiles, uploads, false)
                            },
                        )
                    }
                    TileWork::Append { mut cache, stroke } => {
                        let layer_id = cache.tiles.layers()[0].id.clone();
                        paint_stroke_into_tiles_at_scale(
                            &mut cache.tiles,
                            &layer_id,
                            &stroke,
                            job.key.scale,
                        )
                        .and_then(|changed| {
                            let uploads = if let Some(invalidation) = changed {
                                let graph = ProcessingGraph::project_raster(&cache.tiles)?;
                                let output = graph.affected_output_tiles(
                                    &invalidation,
                                    ChangeTarget::Source,
                                    cache.tiles.dimensions(),
                                )?;
                                cache.tiles.prepare_uploads(&TileInvalidation {
                                    layer_id: invalidation.layer_id,
                                    coords: output,
                                })?
                            } else {
                                Vec::new()
                            };
                            Ok((cache.tiles, uploads, true))
                        })
                    }
                    TileWork::Appearance { mut cache } => {
                        update_projected_paint_appearance(&mut cache.tiles, job.state)
                            .map(|uploads| (cache.tiles, uploads, true))
                    }
                    TileWork::Reconcile {
                        cache,
                        document,
                        stroke_hint,
                    } => project_committed_paint_layer_at_scale(&document, job.key.scale).and_then(
                        |tiles| {
                            let uploads = stroke_hint
                                .as_ref()
                                .and_then(|stroke| {
                                    stroke_candidate_tile_coords_at_scale(
                                        stroke,
                                        job.dimensions,
                                        job.key.scale,
                                    )
                                    .ok()
                                })
                                .and_then(|coords| {
                                    let layer_id = tiles.layers().first()?.id.clone();
                                    let graph = ProcessingGraph::project_raster(&tiles).ok()?;
                                    graph
                                        .affected_output_tiles(
                                            &TileInvalidation { layer_id, coords },
                                            ChangeTarget::Source,
                                            tiles.dimensions(),
                                        )
                                        .ok()
                                })
                                .map_or_else(
                                    || tiles.prepare_changed_uploads(&cache.tiles),
                                    |coords| {
                                        tiles.prepare_changed_uploads_in_coords(
                                            &cache.tiles,
                                            &coords,
                                        )
                                    },
                                )?;
                            Ok((tiles, uploads, true))
                        },
                    ),
                }
                .and_then(|(tiles, uploads, incremental)| {
                    Ok(TileResult {
                        uploads: ValidatedTileUploads::new(tiles.dimensions(), uploads)?,
                        tiles,
                        incremental,
                    })
                });
                let cpu_time = started.elapsed();
                let _ = tile_app.run_on_main_thread(move || {
                    finish_tile_job(
                        job.key,
                        job.state,
                        job.dimensions,
                        result,
                        queued_for,
                        cpu_time,
                    )
                });
            }
        })
        .is_ok()
    {
        let _ = TILE_SENDER.set(sender);
    }
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
        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            let cursor = if PANNING.with(|value| value.get()) {
                Some(NSCursor::closedHandCursor())
            } else {
                match TOOL.with(|tool| tool.get()) {
                    CanvasTool::ZoomIn => Some(NSCursor::zoomInCursor()),
                    CanvasTool::ZoomOut => Some(NSCursor::zoomOutCursor()),
                    CanvasTool::Hand => Some(NSCursor::openHandCursor()),
                    CanvasTool::Text | CanvasTool::TextFrame => Some(NSCursor::IBeamCursor()),
                    CanvasTool::VectorSelect => Some(NSCursor::arrowCursor()),
                    CanvasTool::VectorDirectSelect => Some(NSCursor::crosshairCursor()),
                    CanvasTool::Brush | CanvasTool::Eraser => BRUSH_CURSOR.with(|cursor| cursor.borrow().clone()).or_else(|| Some(NSCursor::crosshairCursor())),
                    _ => Some(NSCursor::crosshairCursor()),
                }
            };
            if let Some(cursor) = cursor { self.addCursorRect_cursor(self.bounds(), &cursor); }
            if matches!(TOOL.with(|tool| tool.get()), CanvasTool::VectorSelect | CanvasTool::TextFrame) {
                let viewport = CANVAS.with(|slot| slot.borrow().as_ref().map(|canvas| canvas.viewport));
                let selected = DOCUMENT.with(|document| {
                    let document = document.borrow();
                    let ids = document.selected_vector_ids();
                    if ids.len() != 1 { return None; }
                    let object = document.snapshot().text_objects.into_iter()
                        .find(|object| object.id == ids[0] && object.editable)?;
                    object.text.box_height?;
                    Some(TextSettings { id: Some(object.id), text: object.text,
                        position: object.position, color: object.color })
                });
                if let (Some(viewport), Some(settings)) = (viewport, selected) {
                    let width = viewport.width as f32 / viewport.scale;
                    let height = viewport.height as f32 / viewport.scale;
                    let fit = ((width - 48.0) / viewport.document_width)
                        .min((height - 48.0) / viewport.document_height).max(0.01) * viewport.zoom;
                    for xi in 0..3 {
                        for yi in 0..3 {
                            if xi == 1 && yi == 1 { continue; }
                            let local = [settings.text.box_width * xi as f32 * 0.5,
                                settings.text.box_height.unwrap_or(0.0) * yi as f32 * 0.5];
                            let [x, y] = text_frame_point(&settings, local);
                            let screen_x = width * 0.5 + viewport.pan_x + (x - viewport.document_width * 0.5) * fit;
                            let screen_y = height * 0.5 + viewport.pan_y + (y - viewport.document_height * 0.5) * fit;
                            let position = match (xi, yi) {
                                (0, 0) => NSCursorFrameResizePosition::TopLeft,
                                (1, 0) => NSCursorFrameResizePosition::Top,
                                (2, 0) => NSCursorFrameResizePosition::TopRight,
                                (0, 1) => NSCursorFrameResizePosition::Left,
                                (2, 1) => NSCursorFrameResizePosition::Right,
                                (0, 2) => NSCursorFrameResizePosition::BottomLeft,
                                (1, 2) => NSCursorFrameResizePosition::Bottom,
                                _ => NSCursorFrameResizePosition::BottomRight,
                            };
                            let cursor = NSCursor::frameResizeCursorFromPosition_inDirections(
                                position, NSCursorFrameResizeDirections::All);
                            self.addCursorRect_cursor(NSRect::new(
                                NSPoint::new(f64::from(screen_x - 8.0), f64::from(screen_y - 8.0)),
                                NSSize::new(16.0, 16.0)), &cursor);
                        }
                    }
                }
            }
        }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() { window.makeFirstResponder(Some(self)); }
            if SPACE_DOWN.with(|space| space.get()) || TOOL.with(|tool| tool.get()) == CanvasTool::Hand { self.begin_pan(event); } else { self.pointer(event, 0); }
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) { if PANNING.with(|value| value.get()) { self.update_pan(event); } else { self.pointer(event, 1); } }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) { if PANNING.with(|value| value.get()) { self.update_pan(event); PANNING.with(|value| value.set(false)); self.refresh_cursor(); } else { self.pointer(event, 2); } }
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) { self.begin_pan(event); }
        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) { self.update_pan(event); }
        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) { self.update_pan(event); PANNING.with(|value| value.set(false)); self.refresh_cursor(); }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if self.isHidden() { return; }
            if event.keyCode() == 49 { SPACE_DOWN.with(|space| space.set(true)); }
            else if event.keyCode() == 14 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if let Err(error) = switch_canvas_tool(CanvasTool::Eraser) { emit_error(error); return; }
                if let Some(app) = APP.get() { let _ = app.emit_to("main", "canvas-tool-changed", CanvasTool::Eraser); }
            }
            else if event.keyCode() == 7 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if !event.isARepeat() {
                    if let Some(app) = APP.get() { let _ = app.emit_to("main", "canvas-swap-colors", ()); }
                }
            }
            else if event.keyCode() == 17 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if !event.isARepeat() {
                    if let Err(error) = finish_open_pen() { emit_error(error); return; }
                    if let Some(app) = APP.get() { let _ = app.emit_to("main", "canvas-text-edit", ()); }
                }
            }
            else if [36, 76].contains(&event.keyCode()) && TOOL.with(|tool| tool.get()) == CanvasTool::VectorPen {
                if let Err(error) = DOCUMENT.with(|doc| finish_pen(&mut doc.borrow_mut(), false)).and_then(|_| redraw()) { emit_error(error); }
                emit_document();
            }
            else if [123,124,125,126].contains(&event.keyCode()) && TOOL.with(|tool| tool.get()) == CanvasTool::VectorDirectSelect {
                let amount = if event.modifierFlags().contains(NSEventModifierFlags::Shift) { 10. } else { 1. };
                let delta = match event.keyCode() { 123 => [-amount,0.], 124 => [amount,0.], 125 => [0.,amount], _ => [0.,-amount] };
                let points = DIRECT_POINTS.with(|points| points.borrow().clone());
                if let Err(error) = DOCUMENT.with(|doc| doc.borrow_mut().move_vector_controls(&points,delta,false)).and_then(|_| redraw()) { emit_error(error); }
                emit_document();
            }
            else if event.keyCode() == 53 {
                if cancel_vector_drag() || DOCUMENT.with(|doc| doc.borrow_mut().cancel_selection_gesture()) {
                    if let Err(error) = redraw() { emit_error(error); }
                    emit_document();
                } else { report_edit(DocumentAction::Deselect); }
            }
            else if [51, 117].contains(&event.keyCode())
                && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option)
            {
                report_edit(DocumentAction::DeleteSelectedObjects);
            }
            else if !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) && [11, 46].contains(&event.keyCode()) {
                let tool = if event.keyCode() == 11 { CanvasTool::Brush }
                    else if event.modifierFlags().contains(NSEventModifierFlags::Shift) { CanvasTool::Ellipse }
                    else { CanvasTool::Rectangle };
                if let Err(error) = switch_canvas_tool(tool) { emit_error(error); return; }
                if let Some(app) = APP.get() { let _ = app.emit_to("main", "canvas-tool-changed", tool); }
            }
            else if !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) && [0, 9, 32, 35, 45].contains(&event.keyCode()) {
                let tool = match event.keyCode() {
                    0 => CanvasTool::VectorDirectSelect,
                    9 => CanvasTool::VectorSelect,
                    35 => CanvasTool::VectorPen,
                    45 => CanvasTool::VectorPencil,
                    _ if event.modifierFlags().contains(NSEventModifierFlags::Shift) => CanvasTool::VectorEllipse,
                    _ => CanvasTool::VectorRectangle,
                };
                if let Err(error) = switch_canvas_tool(tool) { emit_error(error); return; }
                if let Some(app) = APP.get() { let _ = app.emit_to("main", "canvas-tool-changed", tool); }
            }
            else { unsafe { msg_send![super(self), keyDown: event] } }
        }
        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            if event.keyCode() == 49 { SPACE_DOWN.with(|space| space.set(false)); PANNING.with(|value| value.set(false)); self.refresh_cursor(); }
            else { unsafe { msg_send![super(self), keyUp: event] } }
        }
        #[unsafe(method(performKeyEquivalent:))]
        fn key_equivalent(&self, event: &NSEvent) -> bool {
            if self.isHidden() || text_editor::active() { return false.into(); }
            let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
            if command && [7,8,9].contains(&event.keyCode()) {
                report_edit(match event.keyCode() { 7 => DocumentAction::Cut, 8 => DocumentAction::Copy, _ => DocumentAction::Paste }); true
            } else if command && [0, 2].contains(&event.keyCode()) {
                report_edit(if event.keyCode() == 0 { DocumentAction::SelectAll } else { DocumentAction::Deselect }); true
            } else if command && event.keyCode() == 34 && event.modifierFlags().contains(NSEventModifierFlags::Shift) {
                report_edit(DocumentAction::InvertSelection); true
            } else if command && event.keyCode() == 45 {
                if let Err(error) = new_document() { emit_error(error); }
                true
            } else if command && event.keyCode() == 13 {
                if DOCUMENT_OPEN.with(|open| open.get()) {
                    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
                    if let Err(error) = close_document(id) { emit_error(error); }
                }
                true
            } else if command && [1, 31].contains(&event.keyCode()) {
                let action = if event.keyCode() == 31 { super::FileAction::Open }
                    else if event.modifierFlags().contains(NSEventModifierFlags::Shift) { super::FileAction::SaveAs }
                    else { super::FileAction::Save };
                if let Err(error) = file_action(action) { emit_error(error); }
                true
            } else if command && event.keyCode() == 6 {
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

    fn refresh_cursor(&self) {
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(self);
        }
    }

    fn pointer(&self, event: &NSEvent, phase: u8) {
        if self.isHidden() || !DOCUMENT_OPEN.with(|open| open.get()) {
            return;
        }
        let location = self.convertPoint_fromView(event.locationInWindow(), None);
        let Some(viewport) = CANVAS.with(|slot| slot.borrow().as_ref().map(|c| c.viewport)) else {
            return;
        };
        let point = viewport.document_point(location.x as f32, location.y as f32);
        let tool = TOOL.with(|tool| tool.get());
        if tool == CanvasTool::VectorPen
            && phase == 0
            && (point.x < 0.0
                || point.y < 0.0
                || point.x >= viewport.document_width
                || point.y >= viewport.document_height)
        {
            if let Err(error) = finish_open_pen() {
                emit_error(error);
            }
            return;
        }
        if matches!(tool, CanvasTool::ZoomIn | CanvasTool::ZoomOut) {
            if phase == 0
                && point.x >= 0.0
                && point.y >= 0.0
                && point.x < viewport.document_width
                && point.y < viewport.document_height
            {
                let factor = if tool == CanvasTool::ZoomIn {
                    1.25
                } else {
                    0.8
                };
                let zoom = (viewport.zoom * factor).clamp(0.25, 4.0);
                if zoom != viewport.zoom {
                    let viewport = viewport.zoom_around(location.x as f32, location.y as f32, zoom);
                    PAN.with(|pan| pan.set((viewport.pan_x, viewport.pan_y)));
                    CANVAS.with(|slot| {
                        if let Some(canvas) = slot.borrow_mut().as_mut() {
                            canvas.viewport = viewport;
                        }
                    });
                    if let Err(error) = redraw() {
                        emit_error(error);
                    }
                    if let Some(app) = APP.get() {
                        let _ = app.emit_to("main", "canvas-zoom-changed", zoom);
                    }
                }
            }
            return;
        }
        if ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_some()) {
            return;
        }
        if phase == 0 {
            if let Err(error) = text_editor::finish(true) {
                emit_error(error);
                return;
            }
            let tool = TOOL.with(|tool| tool.get());
            if tool == CanvasTool::TextFrame
                && point.x >= 0.0
                && point.y >= 0.0
                && point.x < viewport.document_width
                && point.y < viewport.document_height
                && DOCUMENT.with(|doc| {
                    selected_text_resize_handle(&doc.borrow(), [point.x, point.y]).is_none()
                })
                && DOCUMENT.with(|doc| doc.borrow().text_at([point.x, point.y]).is_some())
            {
                if let Err(error) = text_editor::begin_at([point.x, point.y], false) {
                    emit_error(error);
                }
                return;
            }
            if tool == CanvasTool::Text
                || (tool == CanvasTool::VectorSelect
                    && event.clickCount() == 2
                    && DOCUMENT.with(|doc| {
                        selected_text_resize_handle(&doc.borrow(), [point.x, point.y]).is_none()
                    }))
            {
                if point.x >= 0.0
                    && point.y >= 0.0
                    && point.x < viewport.document_width
                    && point.y < viewport.document_height
                {
                    if let Err(error) =
                        text_editor::begin_at([point.x, point.y], tool == CanvasTool::Text)
                    {
                        emit_error(error);
                    }
                }
                return;
            }
        }
        if TOOL.with(|tool| tool.get()) == CanvasTool::Text {
            return;
        }
        if TOOL.with(|tool| tool.get()) == CanvasTool::TextFrame {
            let resizing = TEXT_RESIZE_DRAFT.with(|draft| draft.borrow().is_some())
                || (phase == 0
                    && DOCUMENT.with(|doc| {
                        selected_text_resize_handle(&doc.borrow(), [point.x, point.y]).is_some()
                    }));
            let result = if resizing {
                DOCUMENT.with(|doc| {
                    vector_select_pointer(
                        &mut doc.borrow_mut(),
                        point,
                        phase,
                        event.modifierFlags(),
                    )
                })
            } else {
                text_frame_pointer(point, phase)
            }
            .and_then(|_| redraw());
            if let Err(error) = result {
                TEXT_FRAME_DRAFT.with(|draft| draft.borrow_mut().take());
                emit_error(error);
            }
            if phase == 2 {
                emit_document();
            }
            if phase == 0 || phase == 2 {
                self.refresh_cursor();
            }
            return;
        }

        let pressure = if event.subtype() == NSEventSubtype::TabletPoint {
            event.pressure().clamp(0.0, 1.0)
        } else {
            1.0
        };
        let result = DOCUMENT
            .with(|doc| {
                let mut doc = doc.borrow_mut();
                let tool = TOOL.with(|value| value.get());
                if matches!(
                    tool,
                    CanvasTool::VectorSelect
                        | CanvasTool::VectorDirectSelect
                        | CanvasTool::VectorPen
                        | CanvasTool::VectorPencil
                        | CanvasTool::VectorAnchorAdd
                        | CanvasTool::VectorAnchorDelete
                        | CanvasTool::VectorAnchorConvert
                        | CanvasTool::VectorRectangle
                        | CanvasTool::VectorEllipse
                ) {
                    return vector_pointer(&mut doc, tool, point, phase, event.modifierFlags());
                }
                if matches!(tool, CanvasTool::Rectangle | CanvasTool::Ellipse) {
                    if phase == 0 {
                        doc.begin_selection_edit(
                            point,
                            if tool == CanvasTool::Rectangle {
                                SelectionShape::Rectangle
                            } else {
                                SelectionShape::Ellipse
                            },
                            selection_mode(event.modifierFlags()),
                        )?;
                    } else {
                        doc.extend_selection(point, phase == 2);
                    }
                    return Ok(());
                }
                let result = if phase == 0 {
                    BRUSH
                        .with(|brush| {
                            if tool == CanvasTool::Eraser {
                                doc.begin_eraser(point, *brush.borrow(), pressure)
                            } else {
                                doc.begin_with_pressure(point, *brush.borrow(), pressure)
                            }
                        })
                        .map(|_| ())
                } else {
                    doc.extend_with_pressure(point, pressure)
                };
                if phase == 2 || result.is_err() {
                    doc.finish();
                }
                result
            })
            .and_then(|_| redraw());
        if let Err(error) = result {
            cancel_vector_drag();
            let _ = redraw();
            emit_error(error);
        }
        if phase == 2 {
            emit_document();
        }
        if phase == 0 || phase == 2 {
            self.refresh_cursor();
        }
    }

    fn begin_pan(&self, event: &NSEvent) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        LAST_PAN_POINT.with(|last| last.set((point.x as f32, point.y as f32)));
        PANNING.with(|value| value.set(true));
        self.refresh_cursor();
    }

    fn update_pan(&self, event: &NSEvent) {
        if !PANNING.with(|value| value.get()) {
            return;
        }
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let current = (point.x as f32, point.y as f32);
        let previous = LAST_PAN_POINT.with(|last| last.replace(current));
        PAN.with(|pan| {
            let (x, y) = pan.get();
            pan.set((
                (x + current.0 - previous.0).clamp(-8192.0, 8192.0),
                (y + current.1 - previous.1).clamp(-8192.0, 8192.0),
            ));
        });
        CANVAS.with(|slot| {
            if let Some(canvas) = slot.borrow_mut().as_mut() {
                let (x, y) = PAN.with(|pan| pan.get());
                canvas.viewport.pan_x = x;
                canvas.viewport.pan_y = y;
            }
        });
        if let Err(error) = redraw() {
            emit_error(error);
        }
    }
}

#[derive(Clone)]
struct PenDraft {
    layer: String,
    // Anchor and symmetric outgoing handle; incoming is its reflection.
    nodes: Vec<([f32; 2], [f32; 2])>,
    brush: Brush,
}

#[derive(Clone)]
struct TextResizeDraft {
    original: TextSettings,
    handle: (i8, i8),
    start: [f32; 2],
    current: [f32; 2],
}

impl PenDraft {
    fn object(&self, closed: bool, id: &str) -> Result<VectorObject, String> {
        let mut controls = vec![self.nodes[0].0];
        let count = self.nodes.len();
        for index in 1..count + usize::from(closed) {
            let (_, outgoing) = self.nodes[(index - 1) % count];
            let (anchor, handle) = self.nodes[index % count];
            controls.extend([
                outgoing,
                [2.0 * anchor[0] - handle[0], 2.0 * anchor[1] - handle[1]],
                anchor,
            ]);
        }
        Ok(VectorObject {
            id: id.into(),
            name: "Bezier path".into(),
            group_path: Vec::new(),
            text: None,
            path: VectorPath {
                data: lumapaint_core::bezier::path_data(&controls, closed)?,
                fill_rule: FillRule::NonZero,
            },
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill: None,
            stroke: Some(VectorPaint {
                color: [
                    self.brush.color[0],
                    self.brush.color[1],
                    self.brush.color[2],
                    255,
                ],
            }),
            stroke_width: self.brush.size,
            visible: true,
            kind: VectorObjectKind::Bezier,
            control_points: controls,
        })
    }

    fn preview(&self, document: &Document, zoom: f32) -> Result<Document, String> {
        use std::fmt::Write;
        let mut preview = document.clone();
        preview.upsert_vector_object(&self.layer, self.object(false, "pen-draft")?)?;
        let mut guide = self.object(false, "pen-guides")?;
        guide.kind = VectorObjectKind::Compound;
        guide.stroke = Some(VectorPaint {
            color: [60, 160, 255, 255],
        });
        guide.stroke_width = 1.0 / zoom;
        guide.path.data.clear();
        let radius = 3.0 / zoom;
        for (anchor, handle) in &self.nodes {
            let incoming = [2.0 * anchor[0] - handle[0], 2.0 * anchor[1] - handle[1]];
            let _ = write!(
                guide.path.data,
                " M {} {} L {} {}",
                incoming[0], incoming[1], handle[0], handle[1]
            );
            for point in [*anchor, *handle, incoming] {
                let _ = write!(
                    guide.path.data,
                    " M {} {} h {} v {} h {} Z",
                    point[0] - radius,
                    point[1] - radius,
                    radius * 2.0,
                    radius * 2.0,
                    -radius * 2.0
                );
            }
        }
        preview.upsert_vector_object(&self.layer, guide)?;
        Ok(preview)
    }
}

fn finish_pen(document: &mut Document, closed: bool) -> Result<(), String> {
    let draft = PEN_DRAFT.with(|draft| draft.borrow().clone());
    if let Some(draft) = draft.filter(|draft| draft.nodes.len() >= 2) {
        // Recheck the target: a panel edit may have locked or hidden it meanwhile.
        if document.selected_vector_target()?.as_deref() != Some(draft.layer.as_str()) {
            return Err("Select the original editable vector layer / 元の編集可能なベクターレイヤーを選択してください / 请选择原来的可编辑矢量图层".into());
        }
        let serial = NEXT_VECTOR_OBJECT_ID.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });
        document.upsert_vector_object(
            &draft.layer,
            draft.object(closed, &format!("vector-object-{serial}"))?,
        )?;
    }
    PEN_DRAFT.with(|draft| draft.borrow_mut().take());
    Ok(())
}

// Commit once before leaving pen input. Escape remains the explicit cancellation path.
pub fn finish_open_pen() -> Result<(), String> {
    if !PEN_DRAFT.with(|draft| draft.borrow().is_some()) {
        return Ok(());
    }
    DOCUMENT.with(|document| finish_pen(&mut document.borrow_mut(), false))?;
    redraw()?;
    emit_document();
    Ok(())
}

fn switch_canvas_tool(tool: CanvasTool) -> Result<(), String> {
    if TOOL.with(|current| current.get()) != tool {
        finish_open_pen()?;
        cancel_vector_drag();
        TOOL.with(|current| current.set(tool));
    }
    Ok(())
}

fn bezier_pointer(
    document: &mut Document,
    point: lumapaint_core::document::Point,
    phase: u8,
) -> Result<(), String> {
    let position = [point.x, point.y];
    if phase == 0 {
        let layer = document
            .selected_vector_target()?
            .ok_or("Select a vector layer / ベクターレイヤーを選択してください / 请选择矢量图层")?;
        let zoom = CANVAS.with(|slot| {
            slot.borrow()
                .as_ref()
                .map_or(1.0, |canvas| canvas.viewport.zoom)
        });
        let close = PEN_DRAFT.with(|draft| {
            draft.borrow().as_ref().is_some_and(|draft| {
                draft.nodes.len() >= 2
                    && (draft.nodes[0].0[0] - point.x).hypot(draft.nodes[0].0[1] - point.y)
                        <= 6.0 / zoom
            })
        });
        if close {
            return finish_pen(document, true);
        }
        PEN_DRAFT.with(|draft| -> Result<(), String> {
            let mut draft = draft.borrow_mut();
            let draft = draft.get_or_insert_with(|| PenDraft { layer: layer.clone(), nodes: vec![], brush: BRUSH.with(|brush| *brush.borrow()) });
            if draft.layer != layer { return Err("Finish or cancel the current path first / 現在のパスを確定または取り消してください / 请先完成或取消当前路径".into()); }
            if draft.nodes.len() >= 4096 { return Err("Too many pen anchors".into()); }
            draft.nodes.push((position, position));
            Ok(())
        })?;
    } else {
        PEN_DRAFT.with(|draft| {
            if let Some(draft) = draft.borrow_mut().as_mut() {
                if let Some(node) = draft.nodes.last_mut() {
                    node.1 = position;
                }
            }
        });
    }
    Ok(())
}

#[derive(Clone)]
struct AnchorDraft {
    layer: String,
    original: VectorObject,
    object: VectorObject,
    index: usize,
    start: [f32; 2],
}

fn anchor_pointer(
    document: &mut Document,
    tool: CanvasTool,
    point: lumapaint_core::document::Point,
    phase: u8,
) -> Result<(), String> {
    use lumapaint_core::bezier;
    let position = [point.x, point.y];
    let tolerance = CANVAS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or(6.0, |canvas| 6.0 / canvas.viewport.zoom)
    });
    if phase == 0 {
        ANCHOR_DRAFT.with(|draft| draft.borrow_mut().take());
        let Some(layer_id) = document.selected_vector_target()? else {
            return Ok(());
        };
        let candidate = document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .and_then(|layer| {
                layer
                    .vector_objects
                    .iter()
                    .rev()
                    .filter(|object| object.visible)
                    .filter_map(bezier::editable)
                    .find_map(|object| {
                        if tool == CanvasTool::VectorAnchorAdd {
                            if bezier::control_hit(&object, position, tolerance, false).is_some() {
                                return None;
                            }
                            bezier::segment_hit(&object, position, tolerance)
                                .map(|(index, t)| (object, index, t))
                        } else {
                            bezier::control_hit(
                                &object,
                                position,
                                tolerance,
                                tool == CanvasTool::VectorAnchorConvert,
                            )
                            .map(|index| (object, index, 0.))
                        }
                    })
            });
        let Some((mut object, index, t)) = candidate else {
            return Ok(());
        };
        if tool == CanvasTool::VectorAnchorAdd {
            bezier::insert(&mut object, index, t)?;
        } else if tool == CanvasTool::VectorAnchorDelete {
            bezier::remove(&mut object, index)?;
        } else {
            let original = object.clone();
            bezier::convert(&mut object, index, None)?;
            ANCHOR_DRAFT.with(|draft| {
                *draft.borrow_mut() = Some(AnchorDraft {
                    layer: layer_id,
                    original,
                    object,
                    index,
                    start: position,
                })
            });
            return Ok(());
        }
        document.upsert_vector_object(&layer_id, object)?;
    } else if tool == CanvasTool::VectorAnchorConvert {
        ANCHOR_DRAFT.with(|draft| -> Result<(), String> {
            let mut draft = draft.borrow_mut();
            if let Some(draft) = draft.as_mut() {
                draft.object = draft.original.clone();
                let handle = if (position[0] - draft.start[0]).hypot(position[1] - draft.start[1])
                    > tolerance * 0.5
                {
                    Some(bezier::local_point(&draft.object, position)?)
                } else {
                    None
                };
                bezier::convert(&mut draft.object, draft.index, handle)?;
            }
            Ok(())
        })?;
        if phase == 2 {
            if let Some(draft) = ANCHOR_DRAFT.with(|draft| draft.borrow_mut().take()) {
                if document.selected_vector_target()?.as_deref() != Some(draft.layer.as_str()) {
                    return Err("Select the original vector layer / 元のベクターレイヤーを選択してください / 请选择原来的矢量图层".into());
                }
                if draft.object != draft.original {
                    document.upsert_vector_object(&draft.layer, draft.object)?;
                }
            }
        }
    }
    Ok(())
}

impl AnchorDraft {
    fn preview(&self, document: &Document, zoom: f32) -> Result<Document, String> {
        use std::fmt::Write;
        let mut preview = document.clone();
        preview.upsert_vector_object(&self.layer, self.object.clone())?;
        let mut guide = self.object.clone();
        guide.id = format!(
            "guides-{}",
            self.object.id.chars().take(50).collect::<String>()
        );
        guide.kind = VectorObjectKind::Compound;
        guide.fill = None;
        guide.stroke = Some(VectorPaint {
            color: [60, 160, 255, 255],
        });
        guide.stroke_width = 1. / zoom;
        guide.transform = [1., 0., 0., 1., 0., 0.];
        guide.path.data.clear();
        let points: Vec<_> = self
            .object
            .control_points
            .iter()
            .map(|&p| lumapaint_core::bezier::world_point(&self.object, p))
            .collect();
        for segment in points.windows(4).step_by(3) {
            let _ = write!(
                guide.path.data,
                " M {} {} L {} {} M {} {} L {} {}",
                segment[0][0],
                segment[0][1],
                segment[1][0],
                segment[1][1],
                segment[2][0],
                segment[2][1],
                segment[3][0],
                segment[3][1]
            );
        }
        let r = 3. / zoom;
        for point in points {
            let _ = write!(
                guide.path.data,
                " M {} {} h {} v {} h {} Z",
                point[0] - r,
                point[1] - r,
                r * 2.,
                r * 2.,
                -r * 2.
            );
        }
        preview.upsert_vector_object(&self.layer, guide)?;
        Ok(preview)
    }
}

fn vector_pointer(
    document: &mut Document,
    tool: CanvasTool,
    point: lumapaint_core::document::Point,
    phase: u8,
    modifiers: NSEventModifierFlags,
) -> Result<(), String> {
    if matches!(
        tool,
        CanvasTool::VectorAnchorAdd
            | CanvasTool::VectorAnchorDelete
            | CanvasTool::VectorAnchorConvert
    ) {
        return anchor_pointer(document, tool, point, phase);
    }
    if tool == CanvasTool::VectorPen {
        return bezier_pointer(document, point, phase);
    }
    if tool == CanvasTool::VectorDirectSelect {
        return direct_pointer(document, point, phase, modifiers);
    }
    if tool == CanvasTool::VectorSelect {
        return vector_select_pointer(document, point, phase, modifiers);
    }
    if phase == 0 {
        VECTOR_DRAFT.with(|draft| {
            let mut draft = draft.borrow_mut();
            draft.clear();
            draft.push(point);
        });
        return Ok(());
    }
    if tool == CanvasTool::VectorPencil && phase == 1 {
        VECTOR_DRAFT.with(|draft| {
            let mut draft = draft.borrow_mut();
            if draft
                .last()
                .is_none_or(|last| (point.x - last.x).hypot(point.y - last.y) >= 1.0)
            {
                draft.push(point);
            }
        });
        return Ok(());
    }
    if phase != 2 {
        return Ok(());
    }

    let mut points = VECTOR_DRAFT.with(|draft| std::mem::take(&mut *draft.borrow_mut()));
    if tool == CanvasTool::VectorPencil {
        if points
            .last()
            .is_none_or(|last| (point.x - last.x).hypot(point.y - last.y) >= 1.0)
        {
            points.push(point);
        }
    } else {
        points.push(point);
    }
    if points.len() < 2 {
        return Ok(());
    }
    let start = points[0];
    let end = *points.last().unwrap_or(&start);
    let width = (end.x - start.x).abs();
    let height = (end.y - start.y).abs();
    if tool != CanvasTool::VectorPencil && (width < 1.0 || height < 1.0) {
        return Ok(());
    }

    let (name, path, fill, stroke, stroke_width, kind, control_points) = match tool {
        CanvasTool::VectorPencil => {
            let mut data = format!("M {} {}", points[0].x, points[0].y);
            for point in points.iter().skip(1) {
                use std::fmt::Write;
                let _ = write!(data, " L {} {}", point.x, point.y);
            }
            let color = BRUSH.with(|brush| brush.borrow().color);
            let size = BRUSH.with(|brush| brush.borrow().size);
            (
                "Path",
                data,
                None,
                Some(VectorPaint {
                    color: [color[0], color[1], color[2], 255],
                }),
                size,
                VectorObjectKind::Path,
                points.iter().map(|point| [point.x, point.y]).collect(),
            )
        }
        CanvasTool::VectorRectangle => {
            let x = start.x.min(end.x);
            let y = start.y.min(end.y);
            let data = format!("M {x} {y} H {} V {} H {x} Z", x + width, y + height);
            let color = BRUSH.with(|brush| brush.borrow().color);
            (
                "Rectangle",
                data,
                Some(VectorPaint {
                    color: [color[0], color[1], color[2], 255],
                }),
                None,
                0.0,
                VectorObjectKind::Rectangle,
                vec![[x, y], [x + width, y + height]],
            )
        }
        CanvasTool::VectorEllipse => {
            let cx = (start.x + end.x) * 0.5;
            let cy = (start.y + end.y) * 0.5;
            let rx = width * 0.5;
            let ry = height * 0.5;
            let data = format!(
                "M {} {cy} A {rx} {ry} 0 1 0 {} {cy} A {rx} {ry} 0 1 0 {} {cy} Z",
                cx - rx,
                cx + rx,
                cx - rx
            );
            let color = BRUSH.with(|brush| brush.borrow().color);
            (
                "Ellipse",
                data,
                Some(VectorPaint {
                    color: [color[0], color[1], color[2], 255],
                }),
                None,
                0.0,
                VectorObjectKind::Ellipse,
                vec![[cx - rx, cy - ry], [cx + rx, cy + ry]],
            )
        }
        _ => return Ok(()),
    };
    let layer_id = match document
        .selected_vector_target()?
        .or_else(|| document.editable_vector_layer_id())
    {
        Some(id) => id,
        None => document.add_vector_layer()?,
    };
    let serial = NEXT_VECTOR_OBJECT_ID.with(|next| {
        let value = next.get();
        next.set(value + 1);
        value
    });
    document.upsert_vector_object(
        &layer_id,
        VectorObject {
            text: None,
            id: format!("vector-object-{serial}"),
            name: format!("{name} {serial}"),
            group_path: Vec::new(),
            path: VectorPath {
                data: path,
                fill_rule: FillRule::NonZero,
            },
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill,
            stroke,
            stroke_width,
            visible: true,
            kind,
            control_points,
        },
    )
}

#[derive(Clone)]
struct DirectGesture {
    start: [f32; 2],
    current: [f32; 2],
    marquee: bool,
    baseline: Vec<(String, usize)>,
    break_smooth: bool,
}

fn direct_objects(document: &Document) -> Vec<(String, VectorObject)> {
    document
        .svg_layers()
        .filter(|layer| layer.vector_layer && layer.visible && !layer.locked)
        .flat_map(|layer| {
            layer
                .vector_objects
                .iter()
                .filter(|object| object.visible)
                .filter_map(|object| {
                    lumapaint_core::bezier::editable(object)
                        .map(|object| (layer.id.clone(), object))
                })
        })
        .collect()
}

fn direct_select_ids(document: &mut Document) -> Result<(), String> {
    let ids: Vec<String> =
        DIRECT_POINTS.with(|points| points.borrow().iter().map(|(id, _)| id.clone()).collect());
    let layer = document
        .svg_layers()
        .find(|layer| {
            layer
                .vector_objects
                .iter()
                .any(|object| ids.contains(&object.id))
        })
        .map(|layer| layer.id.clone());
    if let Some(layer) = layer {
        document.select_layer(layer)?;
    }
    document.select_vector_objects(ids)
}

fn direct_pointer(
    document: &mut Document,
    point: lumapaint_core::document::Point,
    phase: u8,
    flags: NSEventModifierFlags,
) -> Result<(), String> {
    use lumapaint_core::bezier;
    let position = [point.x, point.y];
    if phase == 0 {
        let objects = direct_objects(document);
        DIRECT_POINTS.with(|points| {
            points.borrow_mut().retain(|(id, index)| {
                document.selected_vector_ids().contains(id)
                    && objects
                        .iter()
                        .any(|(_, object)| &object.id == id && *index < object.control_points.len())
            })
        });
        let tolerance = CANVAS.with(|slot| {
            slot.borrow()
                .as_ref()
                .map_or(6., |canvas| 6. / canvas.viewport.zoom)
        });
        let hit = objects
            .iter()
            .rev()
            .find_map(|(_, object)| {
                bezier::control_hit(
                    object,
                    position,
                    tolerance,
                    document.selected_vector_ids().contains(&object.id),
                )
                .map(|mut index| {
                    if index == object.control_points.len() - 1 && object.path.data.ends_with('Z') {
                        index = 0;
                    }
                    vec![(object.id.clone(), index)]
                })
            })
            .or_else(|| {
                objects.iter().rev().find_map(|(_, object)| {
                    bezier::segment_hit(object, position, tolerance).map(|(index, _)| {
                        let end = if index + 3 == object.control_points.len() - 1
                            && object.path.data.ends_with('Z')
                        {
                            0
                        } else {
                            index + 3
                        };
                        vec![(object.id.clone(), index), (object.id.clone(), end)]
                    })
                })
            });
        let shift = flags.contains(NSEventModifierFlags::Shift);
        let marquee = hit.is_none();
        let baseline = DIRECT_POINTS.with(|points| {
            if shift {
                points.borrow().clone()
            } else {
                vec![]
            }
        });
        DIRECT_POINTS.with(|points| {
            let mut points = points.borrow_mut();
            if let Some(hit) = hit {
                if !shift && !hit.iter().all(|point| points.contains(point)) {
                    points.clear();
                }
                for point in hit {
                    if shift && points.contains(&point) {
                        points.retain(|p| p != &point);
                    } else if !points.contains(&point) {
                        points.push(point);
                    }
                }
            } else if !shift {
                points.clear();
            }
        });
        direct_select_ids(document)?;
        DIRECT_GESTURE.with(|draft| {
            *draft.borrow_mut() = Some(DirectGesture {
                start: position,
                current: position,
                marquee,
                baseline,
                break_smooth: false,
            })
        });
    } else {
        DIRECT_GESTURE.with(|draft| {
            if let Some(draft) = draft.borrow_mut().as_mut() {
                draft.current = position;
                if flags.contains(NSEventModifierFlags::Shift) && !draft.marquee {
                    let dx = position[0] - draft.start[0];
                    let dy = position[1] - draft.start[1];
                    if dx.abs() > dy.abs() {
                        draft.current[1] = draft.start[1];
                    } else {
                        draft.current[0] = draft.start[0];
                    }
                }
                draft.break_smooth = flags.contains(NSEventModifierFlags::Option);
            }
        });
        if phase == 2 {
            if let Some(draft) = DIRECT_GESTURE.with(|draft| draft.borrow_mut().take()) {
                if draft.marquee {
                    let mut points = draft.baseline;
                    for (_, object) in direct_objects(document) {
                        for index in (0..object.control_points.len()).step_by(3) {
                            if index == object.control_points.len() - 1
                                && object.path.data.ends_with('Z')
                            {
                                continue;
                            }
                            let p = bezier::world_point(&object, object.control_points[index]);
                            if p[0] >= draft.start[0].min(position[0])
                                && p[0] <= draft.start[0].max(position[0])
                                && p[1] >= draft.start[1].min(position[1])
                                && p[1] <= draft.start[1].max(position[1])
                            {
                                let selected = (object.id.clone(), index);
                                if !points.contains(&selected) {
                                    points.push(selected);
                                }
                            }
                        }
                    }
                    DIRECT_POINTS.with(|selected| *selected.borrow_mut() = points);
                    direct_select_ids(document)?;
                } else {
                    let points = DIRECT_POINTS.with(|points| points.borrow().clone());
                    document.move_vector_controls(
                        &points,
                        [
                            draft.current[0] - draft.start[0],
                            draft.current[1] - draft.start[1],
                        ],
                        draft.break_smooth,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn direct_preview(document: &Document, zoom: f32) -> Result<Document, String> {
    use std::fmt::Write;
    let mut preview = document.clone();
    let points = DIRECT_POINTS.with(|points| points.borrow().clone());
    let gesture = DIRECT_GESTURE.with(|draft| draft.borrow().clone());
    if let Some(draft) = gesture.as_ref().filter(|draft| !draft.marquee) {
        preview.move_vector_controls(
            &points,
            [
                draft.current[0] - draft.start[0],
                draft.current[1] - draft.start[1],
            ],
            draft.break_smooth,
        )?;
    }
    let objects = direct_objects(&preview);
    for (serial, (layer, object)) in objects
        .into_iter()
        .filter(|(_, object)| document.selected_vector_ids().contains(&object.id))
        .enumerate()
    {
        let draft = AnchorDraft {
            layer: layer.clone(),
            original: object.clone(),
            object: object.clone(),
            index: 0,
            start: [0., 0.],
        };
        // Reuse the same anchor/handle guides as the anchor-point tool.
        preview = draft.preview(&preview, zoom)?;
        let mut marker = object.clone();
        marker.id = format!("direct-selected-{serial}");
        marker.kind = VectorObjectKind::Compound;
        marker.transform = [1., 0., 0., 1., 0., 0.];
        marker.stroke = None;
        marker.fill = Some(VectorPaint {
            color: [60, 160, 255, 255],
        });
        marker.path.data.clear();
        let r = 3. / zoom;
        for (_, index) in points
            .iter()
            .filter(|(id, index)| id == &object.id && *index < object.control_points.len())
        {
            let p = lumapaint_core::bezier::world_point(&object, object.control_points[*index]);
            let _ = write!(
                marker.path.data,
                " M {} {} h {} v {} h {} Z",
                p[0] - r,
                p[1] - r,
                r * 2.,
                r * 2.,
                -r * 2.
            );
        }
        if !marker.path.data.is_empty() {
            preview.upsert_vector_object(&layer, marker)?;
        }
    }
    if let Some(draft) = gesture.filter(|draft| draft.marquee && draft.start != draft.current) {
        // An overlay-only SVG layer avoids changing or selecting document objects.
        let x = draft.start[0].min(draft.current[0]);
        let y = draft.start[1].min(draft.current[1]);
        let w = (draft.current[0] - draft.start[0]).abs().max(0.1);
        let h = (draft.current[1] - draft.start[1]).abs().max(0.1);
        let layer = preview.add_vector_layer()?;
        preview.upsert_vector_object(
            &layer,
            VectorObject {
                id: "direct-marquee".into(),
                name: "Selection".into(),
                group_path: Vec::new(),
                text: None,
                path: VectorPath {
                    data: format!("M {x} {y} h {w} v {h} h {} Z", -w),
                    fill_rule: FillRule::NonZero,
                },
                transform: [1., 0., 0., 1., 0., 0.],
                fill: None,
                stroke: Some(VectorPaint {
                    color: [60, 160, 255, 255],
                }),
                stroke_width: 1. / zoom,
                visible: true,
                kind: VectorObjectKind::Compound,
                control_points: vec![[x, y], [x + w, y + h]],
            },
        )?;
    }
    Ok(preview)
}

fn vector_select_pointer(
    document: &mut Document,
    point: lumapaint_core::document::Point,
    phase: u8,
    modifiers: NSEventModifierFlags,
) -> Result<(), String> {
    if phase == 0 {
        cancel_vector_drag();
        if let Some((settings, handle)) = selected_text_resize_handle(document, [point.x, point.y])
        {
            TEXT_RESIZE_DRAFT.with(|draft| {
                *draft.borrow_mut() = Some(TextResizeDraft {
                    original: settings,
                    handle,
                    start: [point.x, point.y],
                    current: [point.x, point.y],
                });
            });
            return Ok(());
        }
        let hit =
            document.select_vector_at(point, 6.0, modifiers.contains(NSEventModifierFlags::Shift));
        VECTOR_DRAFT.with(|draft| {
            let mut draft = draft.borrow_mut();
            draft.clear();
            if hit.is_some() {
                draft.push(point);
            }
        });
    } else if phase == 1 {
        if TEXT_RESIZE_DRAFT.with(|draft| draft.borrow().is_some()) {
            TEXT_RESIZE_DRAFT.with(|draft| {
                if let Some(draft) = draft.borrow_mut().as_mut() {
                    draft.current = [point.x, point.y];
                }
            });
            return Ok(());
        }
        if VECTOR_CONTROL.with(|control| control.borrow().is_none()) {
            let start = VECTOR_DRAFT.with(|draft| draft.borrow().first().copied());
            if let Some(start) = start {
                VECTOR_MOVE.with(|offset| offset.set([point.x - start.x, point.y - start.y]));
            }
        }
    } else if phase == 2 {
        if let Some(mut draft) = TEXT_RESIZE_DRAFT.with(|draft| draft.borrow_mut().take()) {
            draft.current = [point.x, point.y];
            let mut settings = resized_text_settings(&draft);
            if settings.position != draft.original.position
                || settings.text.box_width != draft.original.text.box_width
                || settings.text.box_height != draft.original.text.box_height
            {
                if settings.text.box_width != draft.original.text.box_width {
                    text_editor::reflow(&mut settings)?;
                }
                document.set_text_object(settings)?;
                hold_vector_commit_frame(document);
            }
            return Ok(());
        }
        VECTOR_MOVE.with(|offset| offset.set([0.0, 0.0]));
        if let Some((id, index)) = VECTOR_CONTROL.with(|control| control.borrow_mut().take()) {
            VECTOR_DRAFT.with(|draft| draft.borrow_mut().clear());
            return document.move_vector_control(&id, index, point);
        }
        let start = VECTOR_DRAFT.with(|draft| std::mem::take(&mut *draft.borrow_mut()));
        if let Some(start) = start.first() {
            let offset = [point.x - start.x, point.y - start.y];
            if offset != [0.0, 0.0] {
                // Present the release position with the existing drag textures before
                // committing. The worker can then prepare the new SVG without a blank frame.
                CANVAS.with(|slot| -> Result<(), String> {
                    let mut slot = slot.borrow_mut();
                    if let Some(canvas) = slot.as_mut() {
                        let overlay = selected_text_frame_overlay(document).map(|mut overlay| {
                            for corner in &mut overlay.corners {
                                corner[0] += offset[0];
                                corner[1] += offset[1];
                            }
                            overlay
                        });
                        canvas.renderer.set_frame_overlay(overlay);
                        canvas
                            .renderer
                            .render_vector_drag(canvas.viewport, document, offset)?;
                    }
                    Ok(())
                })?;
                if document.move_selected_vectors(offset[0], offset[1])? {
                    hold_vector_commit_frame(document);
                }
            }
        }
    }
    Ok(())
}

fn hold_vector_commit_frame(document: &Document) {
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            canvas.pending_vector_commit = Some(RasterKey {
                document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                revision: document.revision(),
                canvas_token: canvas.token,
            });
        }
    });
}

fn waiting_for_vector_commit(
    pending: &mut Option<RasterKey>,
    current: RasterKey,
    missing_svg: bool,
) -> bool {
    if *pending == Some(current) && missing_svg {
        return true;
    }
    *pending = None;
    false
}

fn text_frame_point(settings: &TextSettings, local: [f32; 2]) -> [f32; 2] {
    let angle = settings.text.rotation.to_radians();
    let x = local[0] * settings.text.scale_x;
    let y = local[1] * settings.text.scale_y;
    [
        settings.position[0] + x * angle.cos() - y * angle.sin(),
        settings.position[1] + x * angle.sin() + y * angle.cos(),
    ]
}

fn selected_text_resize_handle(
    document: &Document,
    point: [f32; 2],
) -> Option<(TextSettings, (i8, i8))> {
    let selected = document.selected_vector_ids();
    if selected.len() != 1 {
        return None;
    }
    let snapshot = document.snapshot();
    let object = snapshot
        .text_objects
        .into_iter()
        .find(|object| object.id == selected[0] && object.editable)?;
    let height = object.text.box_height?;
    let settings = TextSettings {
        id: Some(object.id),
        text: object.text,
        position: object.position,
        color: object.color,
    };
    let tolerance = CANVAS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|canvas| {
                let viewport = canvas.viewport;
                let width = viewport.width as f32 / viewport.scale;
                let height = viewport.height as f32 / viewport.scale;
                let fit = ((width - 48.0) / viewport.document_width)
                    .min((height - 48.0) / viewport.document_height)
                    .max(0.01)
                    * viewport.zoom;
                8.0 / fit
            })
            .unwrap_or(6.0)
    });
    let mut nearest = None;
    for (xi, x) in [
        (0, 0.0),
        (1, settings.text.box_width * 0.5),
        (2, settings.text.box_width),
    ] {
        for (yi, y) in [(0, 0.0), (1, height * 0.5), (2, height)] {
            if xi == 1 && yi == 1 {
                continue;
            }
            let target = text_frame_point(&settings, [x, y]);
            let distance = (point[0] - target[0]).hypot(point[1] - target[1]);
            if distance <= tolerance && nearest.is_none_or(|(best, _)| distance < best) {
                nearest = Some((distance, (xi as i8 - 1, yi as i8 - 1)));
            }
        }
    }
    for (from, to, handle) in [
        ([0.0, 0.0], [settings.text.box_width, 0.0], (0, -1)),
        ([0.0, height], [settings.text.box_width, height], (0, 1)),
        ([0.0, 0.0], [0.0, height], (-1, 0)),
        (
            [settings.text.box_width, 0.0],
            [settings.text.box_width, height],
            (1, 0),
        ),
    ] {
        let a = text_frame_point(&settings, from);
        let b = text_frame_point(&settings, to);
        let vx = b[0] - a[0];
        let vy = b[1] - a[1];
        let t = (((point[0] - a[0]) * vx + (point[1] - a[1]) * vy) / (vx * vx + vy * vy))
            .clamp(0.0, 1.0);
        let distance = (point[0] - a[0] - t * vx).hypot(point[1] - a[1] - t * vy);
        if distance <= tolerance && nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, handle));
        }
    }
    nearest.map(|(_, handle)| (settings, handle))
}

fn resized_text_settings(draft: &TextResizeDraft) -> TextSettings {
    let mut settings = draft.original.clone();
    let text = &settings.text;
    let angle = text.rotation.to_radians();
    let dx = draft.current[0] - draft.start[0];
    let dy = draft.current[1] - draft.start[1];
    let local_dx = (dx * angle.cos() + dy * angle.sin()) / text.scale_x;
    let local_dy = (-dx * angle.sin() + dy * angle.cos()) / text.scale_y;
    let old_width = text.box_width;
    let old_height = text.box_height.unwrap_or(16.0);
    let min_width = 16.0_f32
        .max(text.indent_left + text.indent_right + 0.001)
        .min(old_width);
    let left = if draft.handle.0 < 0 {
        local_dx.clamp(old_width - 8192.0, old_width - min_width)
    } else {
        0.0
    };
    let top = if draft.handle.1 < 0 {
        local_dy.clamp(old_height - 8192.0, old_height - 16.0)
    } else {
        0.0
    };
    let right = if draft.handle.0 > 0 {
        (old_width + local_dx).clamp(left + min_width, left + 8192.0)
    } else {
        old_width
    };
    let bottom = if draft.handle.1 > 0 {
        (old_height + local_dy).clamp(top + 16.0, top + 8192.0)
    } else {
        old_height
    };
    settings.position = text_frame_point(&draft.original, [left, top]);
    settings.text.box_width = right - left;
    settings.text.box_height = Some(bottom - top);
    if settings.text.box_width != draft.original.text.box_width {
        settings.text.clear_measured_layout();
    }
    settings
}

fn text_frame_geometry(start: [f32; 2], end: [f32; 2]) -> ([f32; 2], f32, f32) {
    let width = (end[0] - start[0]).abs();
    let height = (end[1] - start[1]).abs();
    if width < 3.0 && height < 3.0 {
        return (start, 240.0, 120.0);
    }
    (
        [start[0].min(end[0]), start[1].min(end[1])],
        width.max(16.0),
        height.max(16.0),
    )
}

fn text_frame_pointer(point: lumapaint_core::document::Point, phase: u8) -> Result<(), String> {
    if phase == 0 {
        DOCUMENT.with(|document| {
            let document = document.borrow();
            let (width, height) = document.dimensions();
            if point.x < 0.0 || point.y < 0.0 || point.x >= width as f32 || point.y >= height as f32 {
                return Err("Start the text frame inside the document / ドキュメント内から文字枠を作成してください / 请在文档内创建文本框".into());
            }
            document.selected_vector_target()?;
            Ok::<(), String>(())
        })?;
        TEXT_FRAME_DRAFT
            .with(|draft| *draft.borrow_mut() = Some(([point.x, point.y], [point.x, point.y])));
        return Ok(());
    }
    let bounds = DOCUMENT.with(|document| {
        let document = document.borrow();
        let (width, height) = document.dimensions();
        (width as f32, height as f32)
    });
    let end = [point.x.clamp(0.0, bounds.0), point.y.clamp(0.0, bounds.1)];
    if phase == 1 {
        TEXT_FRAME_DRAFT.with(|draft| {
            if let Some(frame) = draft.borrow_mut().as_mut() {
                frame.1 = end;
            }
        });
        return Ok(());
    }
    let Some((start, _)) = TEXT_FRAME_DRAFT.with(|draft| draft.borrow_mut().take()) else {
        return Ok(());
    };
    let (position, box_width, box_height) = text_frame_geometry(start, end);
    let settings = TextSettings {
        id: None,
        text: lumapaint_core::vector::VectorText {
            content: String::new(),
            box_width,
            box_height: Some(box_height),
            ..Default::default()
        },
        position,
        color: BRUSH.with(|brush| brush.borrow().color),
    };
    DOCUMENT.with(|document| document.borrow_mut().set_text_object(settings))?;
    let snapshot = DOCUMENT.with(|document| document.borrow().snapshot());
    let object = snapshot
        .text_objects
        .iter()
        .find(|object| snapshot.selected_vector_objects.contains(&object.id))
        .ok_or("Text frame was not created")?;
    text_editor::begin(TextSettings {
        id: Some(object.id.clone()),
        text: object.text.clone(),
        position: object.position,
        color: object.color,
    })
}

fn text_frame_overlay(
    settings: &TextSettings,
    handles: bool,
) -> Option<lumapaint_renderer::FrameOverlay> {
    let height = settings.text.box_height?;
    let width = settings.text.box_width;
    Some(lumapaint_renderer::FrameOverlay {
        corners: [
            text_frame_point(settings, [0.0, 0.0]),
            text_frame_point(settings, [width, 0.0]),
            text_frame_point(settings, [width, height]),
            text_frame_point(settings, [0.0, height]),
        ],
        handles,
    })
}

fn draft_frame_overlay(start: [f32; 2], end: [f32; 2]) -> lumapaint_renderer::FrameOverlay {
    let ([x, y], width, height) = text_frame_geometry(start, end);
    lumapaint_renderer::FrameOverlay {
        corners: [
            [x, y],
            [x + width, y],
            [x + width, y + height],
            [x, y + height],
        ],
        handles: false,
    }
}

fn selected_text_frame_overlay(document: &Document) -> Option<lumapaint_renderer::FrameOverlay> {
    let id = document.selected_vector_ids().first()?;
    let object = document
        .svg_layers()
        .filter(|layer| layer.visible && !layer.locked && layer.vector_layer)
        .flat_map(|layer| &layer.vector_objects)
        .find(|object| &object.id == id && object.visible)?;
    let text = object.text.as_ref()?;
    let height = text.box_height?;
    let angle = text.rotation.to_radians();
    let corners = [
        [0.0, 0.0],
        [text.box_width, 0.0],
        [text.box_width, height],
        [0.0, height],
    ]
    .map(|[x, y]| {
        let (x, y) = (x * text.scale_x, y * text.scale_y);
        lumapaint_core::bezier::world_point(
            object,
            [
                x * angle.cos() - y * angle.sin(),
                x * angle.sin() + y * angle.cos(),
            ],
        )
    });
    Some(lumapaint_renderer::FrameOverlay {
        corners,
        handles: true,
    })
}

fn cancel_vector_drag() -> bool {
    let moving = VECTOR_MOVE.with(|offset| offset.replace([0.0, 0.0]) != [0.0, 0.0]);
    VECTOR_DRAFT.with(|draft| draft.borrow_mut().clear());
    VECTOR_CONTROL.with(|control| control.borrow_mut().take());
    let pen = PEN_DRAFT.with(|draft| draft.borrow_mut().take().is_some());
    let anchor = ANCHOR_DRAFT.with(|draft| draft.borrow_mut().take().is_some());
    let direct = DIRECT_GESTURE.with(|draft| draft.borrow_mut().take().is_some());
    let text_frame = TEXT_FRAME_DRAFT.with(|draft| draft.borrow_mut().take().is_some());
    let text_resize = TEXT_RESIZE_DRAFT.with(|draft| draft.borrow_mut().take().is_some());
    moving || pen || anchor || direct || text_frame || text_resize
}

fn selection_mode(flags: NSEventModifierFlags) -> SelectionMode {
    if flags.contains(NSEventModifierFlags::Option) {
        SelectionMode::Subtract
    } else if flags.contains(NSEventModifierFlags::Shift) {
        SelectionMode::Add
    } else {
        SelectionMode::Replace
    }
}

fn emit_error(error: String) {
    if let Some(app) = APP.get() {
        let _ = app.emit_to("main", "canvas-error", error);
    }
}
fn emit_document() {
    checkpoint();
    if ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_none()) {
        if let Some(app) = APP.get() {
            let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
            let _ = app.emit_to("main", "document-changed", snapshot);
        }
    }
    emit_workspace();
}
fn report_edit(action: DocumentAction) {
    if let Err(error) = edit(action) {
        emit_error(error);
    }
}

pub fn edit(action: DocumentAction) -> Result<DocumentSnapshot, String> {
    if text_editor::active() {
        match action {
            DocumentAction::Undo => text_editor::history(false),
            DocumentAction::Redo => text_editor::history(true),
            DocumentAction::SelectAll => text_editor::select_all(),
            DocumentAction::Copy | DocumentAction::Cut | DocumentAction::Paste => {
                text_editor::clipboard(action)
            }
            _ => {
                text_editor::finish(true)?;
                return edit(action);
            }
        }
        return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
    }
    if matches!(
        action,
        DocumentAction::Copy | DocumentAction::Cut | DocumentAction::Paste
    ) {
        clipboard::action(action)?;
        return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
    }
    if ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_some()) {
        ACTIVE_TILED_DOCUMENT.with(|document| -> Result<(), String> {
            let mut document = document.borrow_mut();
            let session = document.as_mut().ok_or("Missing tiled document")?;
            match action {
                DocumentAction::Undo => {
                    session.document.undo()?;
                }
                DocumentAction::Redo => {
                    session.document.redo()?;
                }
                _ => return Err("This tiled document operation is not available yet".into()),
            }
            Ok(())
        })?;
        redraw()?;
        emit_document();
        return ACTIVE_TILED_DOCUMENT.with(|document| {
            document
                .borrow()
                .as_ref()
                .map(TiledSession::snapshot)
                .ok_or_else(|| "Missing tiled document".into())
        });
    }
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        let mut doc = doc.borrow_mut();
        match action {
            DocumentAction::Undo => doc.undo(),
            DocumentAction::Redo => doc.redo(),
            DocumentAction::ToggleLayer => doc.toggle_visibility(),
            DocumentAction::SelectAll => doc.select_all(),
            DocumentAction::Deselect => doc.deselect(),
            DocumentAction::InvertSelection => doc.invert_selection()?,
            DocumentAction::ClearLayer => doc.clear_selected_layer()?,
            DocumentAction::Copy | DocumentAction::Cut | DocumentAction::Paste => unreachable!(),
            DocumentAction::DeleteSelectedObjects => {
                if TOOL.with(|tool| tool.get()) == CanvasTool::VectorDirectSelect {
                    let points = DIRECT_POINTS.with(|points| points.borrow().clone());
                    doc.delete_vector_anchors(&points)?;
                    DIRECT_POINTS.with(|points| points.borrow_mut().clear());
                } else {
                    doc.delete_selected_vector_objects()?;
                }
            }
        }
        Ok::<(), String>(())
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn toggle_layer(id: String) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().toggle_layer(&id))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_layer_settings(settings: LayerSettings) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().set_layer_settings(settings))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn delete_layer(id: String) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().delete_layer(&id))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn add_paint_layer() -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        let mut doc = doc.borrow_mut();
        let id = doc.add_paint_layer()?;
        doc.select_layer(id)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn select_layer(id: String) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    // Commit against the original destination before changing the selected layer.
    // A failed commit leaves both the editor and selection intact.
    text_editor::finish(true)?;
    DOCUMENT.with(|doc| doc.borrow_mut().select_layer(id))?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn add_vector_layer() -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        let mut doc = doc.borrow_mut();
        let id = doc.add_vector_layer()?;
        doc.select_layer(id)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn set_text_object(mut settings: TextSettings) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::reflow(&mut settings)?;
    DOCUMENT.with(|doc| doc.borrow_mut().set_text_object(settings))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn upsert_vector_object(
    layer_id: String,
    object: VectorObject,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().upsert_vector_object(&layer_id, object))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn set_vector_stroke_width(width: f32, color: [u8; 3]) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|document| {
        document
            .borrow_mut()
            .set_selected_stroke_width(width, color)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|document| document.borrow().snapshot()))
}

pub fn select_vector_objects(ids: Vec<String>) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().select_vector_objects(ids))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_vector_object_visibility(
    layer_id: String,
    object_id: String,
    visible: bool,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        doc.borrow_mut()
            .set_vector_object_visibility(&layer_id, &object_id, visible)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn reorder_vector_objects(
    layer_id: String,
    ids: Vec<String>,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().reorder_vector_objects(&layer_id, &ids))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn combine_selected_vectors(operation: PathOperation) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        doc.borrow_mut()
            .combine_selected_vectors(operation, |back, front, op| {
                lumapaint_renderer::vector::skia_paths::SkiaPathEngine
                    .combine_objects(back, front, op)
            })
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn group_selected_vectors() -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().group_selected_vectors())?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn ungroup_selected_vectors(all: bool) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().ungroup_selected_vectors(all))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn edit_selected_paths(action: PathEditAction) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        let mut document = doc.borrow_mut();
        match action {
            PathEditAction::Join => document.join_selected_paths(),
            PathEditAction::Average => {
                let controls = DIRECT_POINTS.with(|points| points.borrow().clone());
                document.average_vector_controls(&controls)
            }
            PathEditAction::Outline => document.replace_selected_path_geometry(
                |object| {
                    lumapaint_renderer::vector::skia_paths::SkiaPathEngine.outline_object(object)
                },
                true,
            ),
            PathEditAction::Offset => document.replace_selected_path_geometry(
                |object| {
                    lumapaint_renderer::vector::skia_paths::SkiaPathEngine
                        .offset_object(object, 10.0)
                },
                false,
            ),
            PathEditAction::DivideBelow => document.combine_selected_vectors(
                PathOperation::Difference,
                |back, front, operation| {
                    lumapaint_renderer::vector::skia_paths::SkiaPathEngine
                        .combine_objects(back, front, operation)
                },
            ),
            PathEditAction::SplitGrid => document.split_selected_paths(|object| {
                lumapaint_renderer::vector::skia_paths::SkiaPathEngine
                    .split_object_grid(object, 2, 2)
            }),
            _ => document.edit_selected_paths(action),
        }
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn reorder_layers(ids: Vec<String>) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().reorder_layers(&ids))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_color_mode(mode: ColorMode) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().set_color_mode(mode));
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_bit_depth(depth: u8) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().set_bit_depth(depth))?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_color_profile(profile: ColorProfile) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().set_color_profile(profile))?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn set_document_settings(settings: DocumentSettings) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().set_document_settings(settings))?;
    let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            canvas.viewport.document_width = snapshot.width as f32;
            canvas.viewport.document_height = snapshot.height as f32;
            canvas.viewport.canvas_color = snapshot.canvas_color;
        }
    });
    reset_pan()?;
    emit_document();
    Ok(snapshot)
}

pub fn import_svg_layer() -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Images", &["svg", "png", "jpg", "jpeg", "webp"])
        .pick_file()
    else {
        return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
    };
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if metadata.len() > 4 * 1024 * 1024 {
        return Err("SVG must be no larger than 4 MiB".into());
    }
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let source = if extension == "svg" {
        String::from_utf8(bytes).map_err(|error| error.to_string())?
    } else {
        use base64::Engine;
        let mime = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            _ => return Err("Unsupported image format".into()),
        };
        let (width, height) = DOCUMENT.with(|doc| {
            let snapshot = doc.borrow().snapshot();
            (snapshot.width, snapshot.height)
        });
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}\" height=\"{height}\"><image width=\"{width}\" height=\"{height}\" preserveAspectRatio=\"xMidYMid meet\" xlink:href=\"data:{mime};base64,{data}\"/></svg>")
    };
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
            render_canvas(canvas)
        } else {
            Ok(())
        }
    })
}

fn render_canvas(canvas: &mut Canvas) -> Result<(), String> {
    let started = Instant::now();
    let mode = if text_editor::active() {
        "inline-text"
    } else if VECTOR_MOVE.with(|offset| offset.get()) != [0.0, 0.0] {
        "vector-drag"
    } else {
        "committed"
    };
    let result = render_canvas_inner(canvas);
    if render_metrics_enabled() && !canvas.view.isHidden() {
        eprintln!(
            "LumaPaint render mode={mode} main_ms={:.2}",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    result
}

fn render_canvas_inner(canvas: &mut Canvas) -> Result<(), String> {
    // Modal edits are rendered once, with their final state, when the view resumes.
    if canvas.view.isHidden() {
        return Ok(());
    }
    canvas.renderer.set_frame_overlay(None);
    if text_editor::active() || VECTOR_MOVE.with(|offset| offset.get()) != [0.0, 0.0] {
        return text_editor::render(canvas);
    }
    if let Some(draft) = TEXT_RESIZE_DRAFT.with(|draft| draft.borrow().clone()) {
        let mut settings = resized_text_settings(&draft);
        if settings.text.box_width != draft.original.text.box_width {
            text_editor::reflow(&mut settings)?;
        }
        let mut preview = DOCUMENT.with(|document| document.borrow().clone());
        preview.set_text_object(settings.clone())?;
        canvas
            .renderer
            .set_frame_overlay(text_frame_overlay(&settings, true));
        return canvas.renderer.render(canvas.viewport, &preview);
    }
    if TOOL.with(|tool| tool.get()) == CanvasTool::VectorDirectSelect
        && ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_none())
    {
        let preview =
            DOCUMENT.with(|document| direct_preview(&document.borrow(), canvas.viewport.zoom))?;
        return canvas.renderer.render(canvas.viewport, &preview);
    }
    if let Some(draft) = ANCHOR_DRAFT.with(|draft| draft.borrow().clone()) {
        let preview =
            DOCUMENT.with(|document| draft.preview(&document.borrow(), canvas.viewport.zoom))?;
        return canvas.renderer.render(canvas.viewport, &preview);
    }
    if let Some(draft) = PEN_DRAFT.with(|draft| draft.borrow().clone()) {
        let preview =
            DOCUMENT.with(|document| draft.preview(&document.borrow(), canvas.viewport.zoom))?;
        return canvas.renderer.render(canvas.viewport, &preview);
    }
    if let Some((start, end)) = TEXT_FRAME_DRAFT.with(|draft| *draft.borrow()) {
        canvas
            .renderer
            .set_frame_overlay(Some(draft_frame_overlay(start, end)));
    }
    if matches!(
        TOOL.with(|tool| tool.get()),
        CanvasTool::VectorSelect | CanvasTool::TextFrame
    ) && TEXT_FRAME_DRAFT.with(|draft| draft.borrow().is_none())
    {
        let overlay = DOCUMENT.with(|document| selected_text_frame_overlay(&document.borrow()));
        canvas.renderer.set_frame_overlay(overlay);
    }
    let tiled = ACTIVE_TILED_DOCUMENT.with(|document| {
        document.borrow().as_ref().map(|document| {
            let (width, height) = document.document.dimensions();
            (document.document.revision(), width, height)
        })
    });
    if let Some((revision, width, height)) = tiled {
        let key = (ACTIVE_DOCUMENT_ID.with(|id| id.get()), revision);
        if canvas.active_tiled_key != Some(key) {
            ACTIVE_TILED_DOCUMENT.with(|document| -> Result<(), String> {
                let document = document.borrow();
                let document = &document.as_ref().ok_or("Missing tiled document")?.document;
                canvas.renderer.install_tiled_preview(document)
            })?;
            canvas.active_tiled_key = Some(key);
            canvas.tile_cache = None;
            canvas.tile_ready = None;
        }
        canvas.viewport.document_width = width as f32;
        canvas.viewport.document_height = height as f32;
        return DOCUMENT
            .with(|document| canvas.renderer.render(canvas.viewport, &document.borrow()));
    }
    if canvas.active_tiled_key.take().is_some() {
        canvas.renderer.clear_tiled_preview();
    }
    DOCUMENT.with(|document| {
        let document = document.borrow();
        if document.visible_strokes().any(|stroke| stroke.eraser) {
            if RASTER_SENDER.get().is_some() {
                schedule_raster_job(canvas, &document)?;
                return canvas.renderer.render_deferred(canvas.viewport, &document);
            }
            return canvas.renderer.render(canvas.viewport, &document);
        }
        refresh_tile_preview(canvas, &document);
        if RASTER_SENDER.get().is_some() {
            schedule_raster_job(canvas, &document)?;
            let current = RasterKey {
                document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                revision: document.revision(),
                canvas_token: canvas.token,
            };
            if waiting_for_vector_commit(
                &mut canvas.pending_vector_commit,
                current,
                !canvas.renderer.svg_layers_ready(&document),
            ) {
                // Leave the complete release-position frame on the surface. Do not
                // present a frame with the changed layer omitted by render_deferred.
                return Ok(());
            }
            canvas.renderer.render_deferred(canvas.viewport, &document)
        } else {
            canvas.renderer.render(canvas.viewport, &document)
        }
    })
}

fn refresh_tile_preview(canvas: &mut Canvas, document: &Document) {
    let Some(scale) = canvas.viewport.compatible_tile_preview_scale() else {
        canvas.renderer.set_tiled_preview_visible(false);
        return;
    };
    if document.committed_paint_strokes().next().is_none() {
        canvas.renderer.set_tiled_preview_visible(false);
        return;
    }
    let key = TileKey {
        source: RasterKey {
            document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
            revision: document.revision(),
            canvas_token: canvas.token,
        },
        scale,
    };
    if canvas.tile_ready == Some(key) {
        canvas.renderer.set_tiled_preview_visible(true);
        return;
    }
    // An active stroke does not change the committed revision. Reuse its matching
    // tile preview, but never install a stale one while the pointer is down.
    if document.has_active_stroke() {
        canvas.renderer.set_tiled_preview_visible(false);
        return;
    }
    let state = document.paint_projection_state();
    if canvas.tile_cache.as_ref().is_some_and(|cache| {
        canvas.tile_ready == Some(cache.key)
            && cache.key.source.document_id == key.source.document_id
            && cache.key.source.canvas_token == key.source.canvas_token
            && cache.key.scale == key.scale
            && cache.key.source.revision.checked_add(1) == Some(key.source.revision)
            && cache.dimensions == document.dimensions()
            && cache.state.same_composite(state)
    }) {
        let cache = canvas.tile_cache.as_mut().expect("Reuse cache was checked");
        if let Some(layer_id) = cache.tiles.layers().first().map(|layer| layer.id.clone()) {
            let (locked, alpha_locked) = state.locks();
            if cache
                .tiles
                .set_layer_locks(&layer_id, locked, alpha_locked)
                .is_ok()
            {
                cache.tiles.discard_history();
                cache.key = key;
                cache.state = state;
                canvas.tile_ready = Some(key);
                canvas.renderer.set_tiled_preview_visible(true);
                return;
            }
        }
    }
    canvas.renderer.set_tiled_preview_visible(false);
    if canvas.tile_job == Some(key) || canvas.tile_failure == Some(key) {
        return;
    }
    let Some(sender) = TILE_SENDER.get() else {
        canvas.renderer.clear_tiled_preview();
        canvas.tile_cache = None;
        canvas.tile_ready = None;
        return;
    };
    if can_append_tile_preview(canvas.tile_cache.as_ref(), document, key, state) {
        let cache = canvas.tile_cache.take().expect("Append cache was checked");
        let stroke = document.committed_paint_strokes().last().unwrap().clone();
        let job = TileJob {
            key,
            work: TileWork::Append {
                cache: Box::new(cache),
                stroke,
            },
            state,
            dimensions: document.dimensions(),
            queued_at: Instant::now(),
        };
        match sender.try_send(job) {
            Ok(()) => canvas.tile_job = Some(key),
            Err(TrySendError::Full(job)) => {
                if let TileWork::Append { cache, .. } = job.work {
                    canvas.tile_cache = Some(*cache);
                }
            }
            Err(TrySendError::Disconnected(_)) => canvas.renderer.clear_tiled_preview(),
        }
        return;
    }
    if can_reconcile_tile_preview(canvas.tile_cache.as_ref(), document, key)
        && canvas
            .tile_cache
            .as_ref()
            .is_some_and(|cache| cache.state.stroke_count == state.stroke_count)
    {
        let cache = canvas
            .tile_cache
            .take()
            .expect("Appearance cache was checked");
        let job = TileJob {
            key,
            work: TileWork::Appearance {
                cache: Box::new(cache),
            },
            state,
            dimensions: document.dimensions(),
            queued_at: Instant::now(),
        };
        match sender.try_send(job) {
            Ok(()) => canvas.tile_job = Some(key),
            Err(TrySendError::Full(job)) => {
                if let TileWork::Appearance { cache } = job.work {
                    canvas.tile_cache = Some(*cache);
                }
            }
            Err(TrySendError::Disconnected(_)) => canvas.renderer.clear_tiled_preview(),
        }
        return;
    }
    if can_reconcile_tile_preview(canvas.tile_cache.as_ref(), document, key) {
        let stroke_hint = canvas.tile_cache.as_ref().and_then(|cache| {
            if !cache.state.same_appearance(state) {
                return None;
            }
            if cache.state.stroke_count.checked_add(1) == Some(state.stroke_count) {
                document.committed_paint_strokes().last().cloned()
            } else if state.stroke_count.checked_add(1) == Some(cache.state.stroke_count) {
                document.last_undone_paint_stroke().cloned()
            } else {
                None
            }
        });
        let cache = canvas
            .tile_cache
            .take()
            .expect("Reconcile cache was checked");
        let job = TileJob {
            key,
            work: TileWork::Reconcile {
                cache: Box::new(cache),
                document: Box::new(document.clone()),
                stroke_hint,
            },
            state,
            dimensions: document.dimensions(),
            queued_at: Instant::now(),
        };
        match sender.try_send(job) {
            Ok(()) => canvas.tile_job = Some(key),
            Err(TrySendError::Full(job)) => {
                if let TileWork::Reconcile { cache, .. } = job.work {
                    canvas.tile_cache = Some(*cache);
                }
            }
            Err(TrySendError::Disconnected(_)) => canvas.renderer.clear_tiled_preview(),
        }
        return;
    }
    canvas.renderer.clear_tiled_preview();
    canvas.tile_cache = None;
    canvas.tile_ready = None;
    if sender
        .try_send(TileJob {
            key,
            work: TileWork::Full(Box::new(document.clone())),
            state,
            dimensions: document.dimensions(),
            queued_at: Instant::now(),
        })
        .is_ok()
    {
        canvas.tile_job = Some(key);
    }
}

fn can_append_tile_preview(
    cache: Option<&TileCache>,
    document: &Document,
    key: TileKey,
    state: PaintProjectionState,
) -> bool {
    let Some(cache) = cache else {
        return false;
    };
    if cache.key.source.document_id != key.source.document_id
        || cache.key.source.canvas_token != key.source.canvas_token
        || cache.key.scale != key.scale
        || cache.key.source.revision.checked_add(1) != Some(key.source.revision)
        || cache.dimensions != document.dimensions()
        || !cache.state.can_append_one(state)
    {
        return false;
    }
    document.committed_paint_strokes().last().is_some()
}

fn can_reconcile_tile_preview(
    cache: Option<&TileCache>,
    document: &Document,
    key: TileKey,
) -> bool {
    let Some(cache) = cache else {
        return false;
    };
    cache.key.source.document_id == key.source.document_id
        && cache.key.source.canvas_token == key.source.canvas_token
        && cache.key.scale == key.scale
        && cache.key.source.revision.checked_add(1) == Some(key.source.revision)
        && cache.dimensions == document.dimensions()
}

fn schedule_raster_job(canvas: &mut Canvas, document: &Document) -> Result<(), String> {
    let layers = canvas.renderer.missing_svg_layers(document);
    if layers.is_empty() {
        canvas.raster_job = None;
        canvas.raster_failure = None;
        return Ok(());
    }
    let key = RasterKey {
        document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
        revision: document.revision(),
        canvas_token: canvas.token,
    };
    if canvas.raster_job == Some(key) || canvas.raster_failure == Some(key) {
        return Ok(());
    }
    let Some(sender) = RASTER_SENDER.get() else {
        return Ok(());
    };
    match sender.try_send(RasterJob {
        key,
        layers,
        size: document.dimensions(),
        queued_at: Instant::now(),
    }) {
        Ok(()) => canvas.raster_job = Some(key),
        Err(TrySendError::Full(_)) => {} // Completion of the running job retries the latest state.
        Err(TrySendError::Disconnected(_)) => return Err("SVG raster worker stopped".into()),
    }
    Ok(())
}

fn finish_raster_job(
    key: RasterKey,
    result: Result<Vec<PreparedSvgLayer>, String>,
    queued_for: Duration,
    cpu_time: Duration,
) {
    let document_id = ACTIVE_DOCUMENT_ID.with(|id| id.get());
    let revision = DOCUMENT.with(|document| document.borrow().revision());
    let mut failure = None;
    let mut upload_bytes = 0usize;
    let upload_started = Instant::now();
    CANVAS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(canvas) = slot.as_mut() else {
            return false;
        };
        if canvas.raster_job == Some(key) {
            canvas.raster_job = None;
        }
        if !key.matches(document_id, revision, canvas.token) {
            return false;
        }
        match result {
            Ok(layers) => {
                for layer in layers {
                    upload_bytes += layer.pixels.len();
                    if let Err(error) = canvas.renderer.install_prepared_svg(layer) {
                        failure = Some(error);
                        break;
                    }
                }
            }
            Err(error) => failure = Some(error),
        }
        if failure.is_some() {
            canvas.raster_failure = Some(key);
        }
        true
    });
    if render_metrics_enabled() && upload_bytes > 0 {
        eprintln!(
            "LumaPaint raster queue_ms={:.2} cpu_ms={:.2} upload_ms={:.2} bytes={upload_bytes}",
            queued_for.as_secs_f64() * 1000.0,
            cpu_time.as_secs_f64() * 1000.0,
            upload_started.elapsed().as_secs_f64() * 1000.0
        );
    }
    if let Some(error) = failure {
        emit_error(error);
    } else {
        if let Err(error) = redraw() {
            emit_error(error);
        }
    }
}

fn finish_tile_job(
    key: TileKey,
    state: PaintProjectionState,
    source_dimensions: (u32, u32),
    result: Result<TileResult, String>,
    queued_for: Duration,
    cpu_time: Duration,
) {
    let document_id = ACTIVE_DOCUMENT_ID.with(|id| id.get());
    let (revision, dimensions, active, current_state) = DOCUMENT.with(|document| {
        let document = document.borrow();
        (
            document.revision(),
            document.dimensions(),
            document.has_active_stroke(),
            document.paint_projection_state(),
        )
    });
    let mut failure = None;
    let mut incremental = false;
    let mut changed_tiles = 0;
    let mut write_calls = 0;
    let upload_started = Instant::now();
    CANVAS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(canvas) = slot.as_mut() else {
            return;
        };
        if canvas.tile_job == Some(key) {
            canvas.tile_job = None;
        }
        if active
            || dimensions != source_dimensions
            || state != current_state
            || canvas.viewport.compatible_tile_preview_scale() != Some(key.scale)
            || !key.matches(document_id, revision, canvas.token, key.scale)
        {
            return;
        }
        match result {
            Ok(prepared) => {
                incremental = prepared.incremental;
                changed_tiles = prepared.uploads.len();
                write_calls = prepared.uploads.write_count();
                let upload = if prepared.incremental {
                    canvas
                        .renderer
                        .update_tiled_preview_batch(&prepared.uploads)
                } else {
                    canvas.renderer.install_tiled_preview_batch_at_scale(
                        dimensions,
                        key.scale,
                        &prepared.uploads,
                    )
                };
                if let Err(error) = upload {
                    failure = Some(error);
                } else {
                    canvas.renderer.set_tiled_preview_visible(true);
                    canvas.tile_ready = Some(key);
                    canvas.tile_cache = Some(TileCache {
                        key,
                        state,
                        dimensions,
                        tiles: prepared.tiles,
                    });
                }
            }
            Err(error) => failure = Some(error),
        }
        if failure.is_some() {
            canvas.tile_failure = Some(key);
            canvas.tile_cache = None;
            canvas.tile_ready = None;
            canvas.renderer.clear_tiled_preview();
        }
    });
    if render_metrics_enabled() {
        eprintln!(
            "LumaPaint tile {} scale={} changed_tiles={} write_calls={} queue_ms={:.2} cpu_ms={:.2} upload_ms={:.2}{}",
            if incremental { "incremental" } else { "projection" },
            key.scale,
            changed_tiles,
            write_calls,
            queued_for.as_secs_f64() * 1000.0,
            cpu_time.as_secs_f64() * 1000.0,
            upload_started.elapsed().as_secs_f64() * 1000.0,
            failure.as_ref().map_or("", |_| " fallback=v1")
        );
    }
    if let Err(error) = redraw() {
        emit_error(error);
    }
}

struct Canvas {
    // Rust drops fields in declaration order: surface/renderer MUST precede its native view.
    renderer: Renderer,
    view: Retained<PaintView>,
    viewport: Viewport,
    token: u64,
    raster_job: Option<RasterKey>,
    raster_failure: Option<RasterKey>,
    pending_vector_commit: Option<RasterKey>,
    tile_job: Option<TileKey>,
    tile_ready: Option<TileKey>,
    tile_failure: Option<TileKey>,
    tile_cache: Option<TileCache>,
    active_tiled_key: Option<(u64, u64)>,
}

impl Drop for Canvas {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

thread_local! {
    static BRUSH_CURSOR: RefCell<Option<Retained<NSCursor>>> = const { RefCell::new(None) };
    static BRUSH_CURSOR_KEY: std::cell::Cell<(u32, bool)> = const { std::cell::Cell::new((0, false)) };
    // This slot is accessed exclusively from Tauri's main-thread callbacks.
    static CANVAS: RefCell<Option<Canvas>> = const { RefCell::new(None) };
    static DOCUMENT: RefCell<Document> = RefCell::new({ let mut document = Document::default(); let _ = document.select_layer("layer-1".into()); document });
    static ACTIVE_TILED_DOCUMENT: RefCell<Option<TiledSession>> = const { RefCell::new(None) };
    static BRUSH: RefCell<Brush> = RefCell::new(Brush::default());
    static TOOL: std::cell::Cell<CanvasTool> = const { std::cell::Cell::new(CanvasTool::Brush) };
    static DOCUMENT_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    static ACTIVE_DOCUMENT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static NEXT_DOCUMENT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(2) };
    static INACTIVE_DOCUMENTS: RefCell<Vec<OpenDocument>> = const { RefCell::new(Vec::new()) };
    static PAN: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
    static LAST_PAN_POINT: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
    static PANNING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static SPACE_DOWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static VECTOR_MOVE: std::cell::Cell<[f32; 2]> = const { std::cell::Cell::new([0.0, 0.0]) };
    static DIRECT_POINTS: RefCell<Vec<(String,usize)>> = const { RefCell::new(Vec::new()) };
    static DIRECT_GESTURE: RefCell<Option<DirectGesture>> = const { RefCell::new(None) };
    static ANCHOR_DRAFT: RefCell<Option<AnchorDraft>> = const { RefCell::new(None) };
    static PEN_DRAFT: RefCell<Option<PenDraft>> = const { RefCell::new(None) };
    static VECTOR_DRAFT: RefCell<Vec<lumapaint_core::document::Point>> = const { RefCell::new(Vec::new()) };
    static TEXT_FRAME_DRAFT: RefCell<Option<([f32; 2], [f32; 2])>> = const { RefCell::new(None) };
    static TEXT_RESIZE_DRAFT: RefCell<Option<TextResizeDraft>> = const { RefCell::new(None) };
    static VECTOR_CONTROL: RefCell<Option<(String, usize)>> = const { RefCell::new(None) };
    static NEXT_VECTOR_OBJECT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

struct OpenDocument {
    id: u64,
    content: OpenDocumentContent,
    path: Option<std::path::PathBuf>,
    fingerprint: Option<crate::project_file::FileFingerprint>,
}

enum OpenDocumentContent {
    Legacy(Box<Document>),
    Tiled(TiledSession),
}

struct TiledSession {
    document: TiledRasterDocument,
    file_name: Option<String>,
    saved_revision: u64,
}

impl TiledSession {
    fn dirty(&self) -> bool {
        self.document.revision() != self.saved_revision
    }

    fn snapshot(&self) -> DocumentSnapshot {
        let mut snapshot = Document::default().snapshot();
        let (width, height) = self.document.dimensions();
        snapshot.name = self
            .file_name
            .clone()
            .unwrap_or_else(|| "Untitled tiled document".into());
        snapshot.file_name = self.file_name.clone();
        snapshot.width = width;
        snapshot.height = height;
        snapshot.layer_id = "tile-preview".into();
        snapshot.layer_visible = self.document.layers().iter().any(|layer| layer.visible);
        snapshot.layers = self
            .document
            .layers()
            .iter()
            .map(|layer| LayerSnapshot {
                objects: Vec::new(),
                id: layer.id.clone(),
                name: layer.name.clone(),
                kind: "paint",
                visible: layer.visible,
                opacity: layer.opacity,
                locked: layer.locked,
                alpha_locked: layer.alpha_locked,
                mask_enabled: layer.mask_enabled,
                mask_inverted: layer.mask_inverted,
                mask_density: layer.mask_density,
                deletable: false,
                stroke_count: 0,
            })
            .collect();
        snapshot.revision = self.document.revision();
        snapshot.dirty = self.dirty();
        snapshot.can_undo = self.document.can_undo();
        snapshot.can_redo = self.document.can_redo();
        snapshot
    }
}

impl OpenDocumentContent {
    fn dirty(&self) -> bool {
        match self {
            Self::Legacy(document) => document.snapshot().dirty,
            Self::Tiled(document) => document.dirty(),
        }
    }
}

fn next_document_id() -> u64 {
    NEXT_DOCUMENT_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

fn park_active_document() {
    if !DOCUMENT_OPEN.with(|open| open.get()) {
        return;
    }
    let content = ACTIVE_TILED_DOCUMENT
        .with(|slot| slot.borrow_mut().take())
        .map_or_else(
            || {
                OpenDocumentContent::Legacy(Box::new(
                    DOCUMENT.with(|slot| std::mem::take(&mut *slot.borrow_mut())),
                ))
            },
            OpenDocumentContent::Tiled,
        );
    let path = PROJECT_PATH.with(|slot| slot.borrow_mut().take());
    let fingerprint = PROJECT_FINGERPRINT.with(|slot| slot.borrow_mut().take());
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    INACTIVE_DOCUMENTS.with(|documents| {
        documents.borrow_mut().push(OpenDocument {
            id,
            content,
            path,
            fingerprint,
        });
    });
}

fn activate_document(entry: OpenDocument) {
    cancel_vector_drag();
    match entry.content {
        OpenDocumentContent::Legacy(mut document) => {
            let id = document.snapshot().layer_id;
            let _ = document.select_layer(id);
            ACTIVE_TILED_DOCUMENT.with(|slot| slot.borrow_mut().take());
            DOCUMENT.with(|slot| *slot.borrow_mut() = *document);
        }
        OpenDocumentContent::Tiled(document) => {
            DOCUMENT.with(|slot| *slot.borrow_mut() = Document::default());
            ACTIVE_TILED_DOCUMENT.with(|slot| *slot.borrow_mut() = Some(document));
        }
    }
    PROJECT_PATH.with(|slot| *slot.borrow_mut() = entry.path);
    PROJECT_FINGERPRINT.with(|slot| *slot.borrow_mut() = entry.fingerprint);
    ACTIVE_DOCUMENT_ID.with(|active| active.set(entry.id));
    DOCUMENT_OPEN.with(|open| open.set(true));
    CHECKPOINT_REVISION.with(|last| last.set(None));
}

fn document_tab(id: u64, content: &OpenDocumentContent) -> DocumentTabSnapshot {
    match content {
        OpenDocumentContent::Legacy(document) => {
            let snapshot = document.snapshot();
            DocumentTabSnapshot {
                id,
                file_name: snapshot.file_name.or(Some(snapshot.name)),
                dirty: snapshot.dirty,
                format: "legacy",
            }
        }
        OpenDocumentContent::Tiled(document) => DocumentTabSnapshot {
            id,
            file_name: document.file_name.clone(),
            dirty: document.dirty(),
            format: "tiled",
        },
    }
}

pub fn workspace_snapshot() -> DocumentWorkspaceSnapshot {
    let mut documents = INACTIVE_DOCUMENTS.with(|items| {
        items
            .borrow()
            .iter()
            .map(|entry| document_tab(entry.id, &entry.content))
            .collect::<Vec<_>>()
    });
    let open = DOCUMENT_OPEN.with(|value| value.get());
    let active_id = open.then(|| ACTIVE_DOCUMENT_ID.with(|value| value.get()));
    let active_is_tiled = ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_some());
    let active = open.then(|| {
        ACTIVE_TILED_DOCUMENT.with(|document| {
            document.borrow().as_ref().map_or_else(
                || DOCUMENT.with(|doc| doc.borrow().snapshot()),
                TiledSession::snapshot,
            )
        })
    });
    if let (Some(id), Some(document)) = (active_id, active.as_ref()) {
        documents.push(DocumentTabSnapshot {
            id,
            file_name: document.file_name.clone().or(Some(document.name.clone())),
            dirty: document.dirty,
            format: if active_is_tiled { "tiled" } else { "legacy" },
        });
    }
    documents.sort_by_key(|document| document.id);
    DocumentWorkspaceSnapshot {
        active_id,
        active,
        documents,
    }
}

fn emit_workspace() {
    if let Some(app) = APP.get() {
        let _ = app.emit_to("main", "documents-changed", workspace_snapshot());
    }
}

pub fn destroy() {
    cancel_vector_drag();
    let _ = text_editor::finish(false);
    DOCUMENT.with(|doc| doc.borrow_mut().finish());
    checkpoint();
    CANVAS.with(|slot| {
        slot.borrow_mut().take();
    });
}

fn suspend() {
    cancel_vector_drag();
    DOCUMENT.with(|doc| doc.borrow_mut().finish());
    checkpoint();
    SPACE_DOWN.with(|value| value.set(false));
    PANNING.with(|value| value.set(false));
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow().as_ref() {
            // Keep the Metal surface, pipelines and layer textures across modal dialogs.
            if let Some(window) = canvas.view.window() {
                let focused = window.firstResponder().is_some_and(|responder| {
                    Retained::as_ptr(&responder).cast::<c_void>()
                        == Retained::as_ptr(&canvas.view).cast::<c_void>()
                });
                if focused {
                    // SAFETY: the retained view and its parent are accessed on the main thread.
                    let parent = unsafe { canvas.view.superview() };
                    window.makeFirstResponder(parent.as_deref().map(|view| &**view));
                }
            }
            canvas.view.setHidden(true);
        }
    });
}

pub fn reset_pan() -> Result<(), String> {
    PAN.with(|pan| pan.set((0.0, 0.0)));
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            canvas.viewport.pan_x = 0.0;
            canvas.viewport.pan_y = 0.0;
        }
    });
    redraw()
}

// AppKit composites this cursor independently of the document GPU surface.
// Cache by screen diameter so pointer movement never rasterizes the document.
#[allow(deprecated)]
fn update_brush_cursor(viewport: Viewport) -> bool {
    let width = viewport.width as f32 / viewport.scale;
    let height = viewport.height as f32 / viewport.scale;
    let fit = ((width - 48.0) / viewport.document_width)
        .min((height - 48.0) / viewport.document_height)
        .max(0.01)
        * viewport.zoom;
    let diameter = BRUSH.with(|brush| brush.borrow().size * fit).max(1.0);
    let eraser = TOOL.with(|tool| tool.get() == CanvasTool::Eraser);
    let key = (diameter.to_bits(), eraser);
    if BRUSH_CURSOR_KEY.with(|cached| cached.get() == key) {
        return false;
    }
    let side = f64::from(diameter) + 8.0;
    let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(side, side));
    image.lockFocus();
    // SAFETY: AppKit drawing is on the main thread with the image's context current.
    unsafe {
        let ring: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPathWithOvalInRect: NSRect::new(NSPoint::new(4.0, 4.0), NSSize::new(diameter.into(), diameter.into()))];
        NSColor::whiteColor().setStroke();
        let _: () = msg_send![&*ring, setLineWidth: 3.0f64];
        let _: () = msg_send![&*ring, stroke];
        NSColor::blackColor().setStroke();
        let _: () = msg_send![&*ring, setLineWidth: 1.0f64];
        let _: () = msg_send![&*ring, stroke];
        let marker: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPath];
        let center = side * 0.5;
        let _: () = msg_send![&*marker, moveToPoint: NSPoint::new(center - 2.5, center)];
        let _: () = msg_send![&*marker, lineToPoint: NSPoint::new(center + 2.5, center)];
        if !eraser {
            let _: () = msg_send![&*marker, moveToPoint: NSPoint::new(center, center - 2.5)];
            let _: () = msg_send![&*marker, lineToPoint: NSPoint::new(center, center + 2.5)];
        }
        NSColor::whiteColor().setStroke();
        let _: () = msg_send![&*marker, setLineWidth: 3.0f64];
        let _: () = msg_send![&*marker, stroke];
        NSColor::blackColor().setStroke();
        let _: () = msg_send![&*marker, setLineWidth: 1.0f64];
        let _: () = msg_send![&*marker, stroke];
    }
    image.unlockFocus();
    let cursor = NSCursor::initWithImage_hotSpot(
        NSCursor::alloc(),
        &image,
        NSPoint::new(side * 0.5, side * 0.5),
    );
    BRUSH_CURSOR.with(|cached| *cached.borrow_mut() = Some(cursor));
    BRUSH_CURSOR_KEY.with(|cached| cached.set(key));
    true
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
    let tool_changed = TOOL.with(|tool| tool.get()) != request.tool;
    if tool_changed || !request.visible {
        // Report editing errors without treating them as GPU failures or losing the draft.
        if let Err(error) = finish_open_pen() {
            emit_error(error);
        } else {
            cancel_vector_drag();
        }
        if let Err(error) = text_editor::finish(true) {
            // An edit validation error is not a renderer failure. Keep the editor
            // (and its uncommitted text) alive, and report it through the UI alert.
            emit_error(error);
        }
    }
    BRUSH.with(|brush| *brush.borrow_mut() = request.brush);
    TOOL.with(|tool| tool.set(request.tool));
    let mtm = MainThreadMarker::new().ok_or("Native canvas requires the main thread")?;
    if !request.visible || request.width < 1.0 || request.height < 1.0 {
        suspend();
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
        suspend();
        return Ok(CanvasInfo::inactive("hidden"));
    };
    let scale = parent
        .window()
        .ok_or("Canvas window is not attached")?
        .backingScaleFactor();
    let (pan_x, pan_y) = PAN.with(|pan| pan.get());
    let document = ACTIVE_TILED_DOCUMENT.with(|tiled| {
        tiled.borrow().as_ref().map_or_else(
            || DOCUMENT.with(|document| document.borrow().snapshot()),
            TiledSession::snapshot,
        )
    });
    let viewport = Viewport::new(
        frame.size.width,
        frame.size.height,
        scale,
        request.zoom,
        request.dark,
    )?
    .with_pan(pan_x, pan_y)?
    .with_document(document.width, document.height, document.canvas_color)?;

    let result = CANVAS.with(|slot| {
        let mut slot = slot.borrow_mut();
        // A replaced webview must not reuse a surface attached to the previous parent.
        if slot.as_ref().is_some_and(|canvas| {
            // SAFETY: both native views are live and accessed on the main thread.
            unsafe { canvas.view.superview() }
                .as_deref()
                .is_none_or(|view| !std::ptr::eq(view, parent))
        }) {
            slot.take();
        }
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
                        token: NEXT_CANVAS_TOKEN.fetch_add(1, Ordering::Relaxed),
                        raster_job: None,
                        raster_failure: None,
                        pending_vector_commit: None,
                        tile_job: None,
                        tile_ready: None,
                        tile_failure: None,
                        tile_cache: None,
                        active_tiled_key: None,
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
        canvas.view.setHidden(false);
        if update_brush_cursor(viewport) || tool_changed {
            canvas.view.refresh_cursor();
        }
        render_canvas(canvas)?;
        Ok(CanvasInfo {
            status: "ready",
            backend: canvas.renderer.backend.clone(),
            adapter_name: canvas.renderer.adapter_name.clone(),
            physical_width: viewport.width,
            physical_height: viewport.height,
            scale_factor: scale,
            document: DOCUMENT_OPEN.with(|open| open.get()).then(|| {
                ACTIVE_TILED_DOCUMENT.with(|tiled| {
                    tiled.borrow().as_ref().map_or_else(
                        || DOCUMENT.with(|doc| doc.borrow().snapshot()),
                        TiledSession::snapshot,
                    )
                })
            }),
        })
    });
    text_editor::layout()?;
    result
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
    fn vector_release_holds_last_frame_until_ready_and_rejects_stale_documents() {
        let key = RasterKey {
            document_id: 7,
            revision: 12,
            canvas_token: 3,
        };
        let mut pending = Some(key);
        assert!(waiting_for_vector_commit(&mut pending, key, true));
        assert!(pending == Some(key));
        assert!(!waiting_for_vector_commit(&mut pending, key, false));
        assert!(pending.is_none());
        for changed in [
            RasterKey {
                document_id: 8,
                ..key
            },
            RasterKey {
                revision: 13,
                ..key
            },
            RasterKey {
                canvas_token: 4,
                ..key
            },
        ] {
            let mut pending = Some(key);
            assert!(!waiting_for_vector_commit(&mut pending, changed, true));
            assert!(pending.is_none());
        }
        assert!(!waiting_for_vector_commit(&mut None, key, true));
    }

    #[test]
    fn text_frame_handles_resize_edges_and_corners_without_moving_opposite_edges() {
        let original = TextSettings {
            id: Some("text-1".into()),
            text: lumapaint_core::vector::VectorText {
                box_width: 200.0,
                box_height: Some(100.0),
                ..Default::default()
            },
            position: [50.0, 40.0],
            color: [0, 0, 0],
        };
        let draft = TextResizeDraft {
            original: original.clone(),
            handle: (-1, -1),
            start: [50.0, 40.0],
            current: [70.0, 55.0],
        };
        let resized = resized_text_settings(&draft);
        assert_eq!(resized.position, [70.0, 55.0]);
        assert_eq!(resized.text.box_width, 180.0);
        assert_eq!(resized.text.box_height, Some(85.0));
        assert_eq!(text_frame_point(&resized, [180.0, 85.0]), [250.0, 140.0]);
        let right = resized_text_settings(&TextResizeDraft {
            handle: (1, 0),
            current: [90.0, 40.0],
            ..draft.clone()
        });
        assert_eq!(right.position, original.position);
        assert_eq!(right.text.box_width, 240.0);
        assert_eq!(right.text.box_height, Some(100.0));
        let unchanged = resized_text_settings(&TextResizeDraft {
            current: draft.start,
            ..draft
        });
        assert_eq!(unchanged.position, original.position);
        assert_eq!(unchanged.text.box_width, original.text.box_width);
    }

    #[test]
    fn selected_text_frame_overlay_preserves_text_raster_source() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        document.select_layer(layer).unwrap();
        document
            .set_text_object(TextSettings {
                id: None,
                text: lumapaint_core::vector::VectorText {
                    content: "Text".into(),
                    box_width: 200.0,
                    box_height: Some(100.0),
                    ..Default::default()
                },
                position: [50.0, 40.0],
                color: [0, 0, 0],
            })
            .unwrap();
        let snapshot = document.snapshot();
        let revision = document.revision();
        let settings = TextSettings {
            id: Some(snapshot.text_objects[0].id.clone()),
            text: snapshot.text_objects[0].text.clone(),
            position: snapshot.text_objects[0].position,
            color: snapshot.text_objects[0].color,
        };
        let source = document.svg_layers().next().unwrap().source.clone();
        let overlay = text_frame_overlay(&settings, true).unwrap();
        assert_eq!(
            selected_text_resize_handle(&document, [100.0, 40.0]).map(|(_, handle)| handle),
            Some((0, -1))
        );
        assert_eq!(
            selected_text_resize_handle(&document, [50.0, 40.0]).map(|(_, handle)| handle),
            Some((-1, -1))
        );
        assert_eq!(document.revision(), revision);
        assert_eq!(document.snapshot().text_objects.len(), 1);
        assert!(overlay.handles);
        assert_eq!(
            overlay.corners,
            [[50.0, 40.0], [250.0, 40.0], [250.0, 140.0], [50.0, 140.0]]
        );
        assert_eq!(document.svg_layers().next().unwrap().source, source);
        assert_eq!(
            selected_text_frame_overlay(&document).unwrap().corners,
            overlay.corners
        );
        document.select_vector_objects(Vec::new()).unwrap();
        assert!(selected_text_frame_overlay(&document).is_none());
        assert_eq!(document.svg_layers().next().unwrap().source, source);
        assert_eq!(document.revision(), revision);
    }

    #[test]
    fn text_frame_drag_handles_reverse_direction_and_keeps_preview_out_of_history() {
        assert_eq!(
            text_frame_geometry([220., 180.], [40., 60.]),
            ([40., 60.], 180., 120.)
        );
        assert_eq!(
            text_frame_geometry([40., 60.], [40., 60.]),
            ([40., 60.], 240., 120.)
        );
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        document.select_layer(layer).unwrap();
        let revision = document.revision();
        let overlay = draft_frame_overlay([220., 180.], [40., 60.]);
        assert_eq!(document.revision(), revision);
        assert!(document
            .svg_layers()
            .all(|layer| layer.vector_objects.is_empty()));
        assert_eq!(
            overlay.corners,
            [[40., 60.], [220., 60.], [220., 180.], [40., 180.]]
        );
        assert!(!overlay.handles);
    }

    #[test]
    fn pen_preview_commit_control_edit_and_undo_preserve_cubics() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        let draft = PenDraft {
            layer: layer.clone(),
            brush: Brush::default(),
            nodes: vec![([10., 10.], [10., 60.]), ([110., 10.], [110., -40.])],
        };
        let preview = draft.preview(&document, 1.0).unwrap();
        assert!(document
            .svg_layers()
            .next()
            .unwrap()
            .vector_objects
            .is_empty());
        assert_eq!(preview.svg_layers().next().unwrap().vector_objects.len(), 2);
        let object = draft.object(false, "curve").unwrap();
        assert_eq!(object.path.data, "M 10 10 C 10 60 110 60 110 10");
        document
            .upsert_vector_object(&layer, object.clone())
            .unwrap();
        document
            .move_vector_control(
                "curve",
                0,
                lumapaint_core::document::Point { x: 20., y: 10. },
            )
            .unwrap();
        let edited = &document.svg_layers().next().unwrap().vector_objects[0];
        assert_eq!(edited.control_points[1], [20., 60.]);
        assert!(edited.path.data.contains(" C "));
        document.undo();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            object
        );
        document.undo();
        assert!(document
            .svg_layers()
            .next()
            .unwrap()
            .vector_objects
            .is_empty());
        document.redo();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            object
        );
        let json = serde_json::to_string(&object).unwrap();
        assert_eq!(serde_json::from_str::<VectorObject>(&json).unwrap(), object);
        let closed = draft.object(true, "closed").unwrap();
        assert!(closed.path.data.ends_with(" Z"));
        assert_eq!(closed.control_points.first(), closed.control_points.last());
    }

    #[test]
    fn anchor_tools_commit_once_preview_cancel_and_undo() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        document.select_layer(layer.clone()).unwrap();
        let pen = PenDraft {
            layer: layer.clone(),
            brush: Brush::default(),
            nodes: vec![([10., 10.], [10., 60.]), ([110., 10.], [110., -40.])],
        };
        let original = pen.object(false, "curve").unwrap();
        document
            .upsert_vector_object(&layer, original.clone())
            .unwrap();
        let point = |x, y| lumapaint_core::document::Point { x, y };
        anchor_pointer(
            &mut document,
            CanvasTool::VectorAnchorAdd,
            point(60., 47.5),
            0,
        )
        .unwrap();
        let inserted = document.svg_layers().next().unwrap().vector_objects[0].clone();
        assert_eq!(inserted.control_points.len(), 7);
        document.undo();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            original
        );
        document.redo();
        anchor_pointer(
            &mut document,
            CanvasTool::VectorAnchorConvert,
            point(60., 47.5),
            0,
        )
        .unwrap();
        anchor_pointer(
            &mut document,
            CanvasTool::VectorAnchorConvert,
            point(80., 70.),
            1,
        )
        .unwrap();
        let preview = ANCHOR_DRAFT
            .with(|draft| draft.borrow().as_ref().unwrap().preview(&document, 1.))
            .unwrap();
        assert_eq!(preview.svg_layers().next().unwrap().vector_objects.len(), 2);
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            inserted
        );
        assert!(cancel_vector_drag());
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            inserted
        );
        anchor_pointer(
            &mut document,
            CanvasTool::VectorAnchorConvert,
            point(60., 47.5),
            0,
        )
        .unwrap();
        anchor_pointer(
            &mut document,
            CanvasTool::VectorAnchorConvert,
            point(80., 70.),
            2,
        )
        .unwrap();
        assert_ne!(
            document.svg_layers().next().unwrap().vector_objects[0],
            inserted
        );
        document.undo();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            inserted
        );
        anchor_pointer(
            &mut document,
            CanvasTool::VectorAnchorDelete,
            point(60., 47.5),
            0,
        )
        .unwrap();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0]
                .control_points
                .len(),
            4
        );
        document.undo();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            inserted
        );
        document.select_layer("layer-1".into()).unwrap();
        assert!(anchor_pointer(
            &mut document,
            CanvasTool::VectorAnchorDelete,
            point(60., 47.5),
            0
        )
        .is_err());
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            inserted
        );
    }

    #[test]
    fn open_pen_survives_tool_switch_and_outside_commit_once() {
        for next in [
            CanvasTool::VectorSelect,
            CanvasTool::VectorPencil,
            CanvasTool::Brush,
            CanvasTool::Eraser,
            CanvasTool::Text,
            CanvasTool::Hand,
        ] {
            let mut document = Document::default();
            let layer = document.add_vector_layer().unwrap();
            document.select_layer(layer.clone()).unwrap();
            DOCUMENT.with(|slot| *slot.borrow_mut() = document);
            TOOL.with(|tool| tool.set(CanvasTool::VectorPen));
            PEN_DRAFT.with(|draft| {
                *draft.borrow_mut() = Some(PenDraft {
                    layer,
                    brush: Brush::default(),
                    nodes: vec![([10., 10.], [10., 60.]), ([110., 10.], [110., -40.])],
                })
            });
            switch_canvas_tool(next).unwrap();
            // Outside-click IPC and a later sync must not create another path.
            finish_open_pen().unwrap();
            switch_canvas_tool(next).unwrap();
            assert!(PEN_DRAFT.with(|draft| draft.borrow().is_none()));
            DOCUMENT.with(|document| {
                let mut document = document.borrow_mut();
                let objects = &document.svg_layers().next().unwrap().vector_objects;
                assert_eq!(objects.len(), 1);
                assert_eq!(objects[0].path.data, "M 10 10 C 10 60 110 60 110 10");
                document.undo();
                assert!(document
                    .svg_layers()
                    .next()
                    .unwrap()
                    .vector_objects
                    .is_empty());
                document.redo();
                assert_eq!(
                    document.svg_layers().next().unwrap().vector_objects.len(),
                    1
                );
            });
        }
    }

    #[test]
    fn failed_pen_commit_preserves_draft_for_retry() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        PEN_DRAFT.with(|draft| {
            *draft.borrow_mut() = Some(PenDraft {
                layer: layer.clone(),
                brush: Brush::default(),
                nodes: vec![([10., 10.], [10., 10.]), ([100., 100.], [100., 100.])],
            })
        });
        document.select_layer("layer-1".into()).unwrap();
        assert!(finish_pen(&mut document, false).is_err());
        assert!(PEN_DRAFT.with(|draft| draft.borrow().is_some()));
        document.select_layer(layer).unwrap();
        finish_pen(&mut document, false).unwrap();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects.len(),
            1
        );
        assert!(PEN_DRAFT.with(|draft| draft.borrow().is_none()));
    }

    #[test]
    fn edited_shape_can_be_reselected_by_its_fill_and_moved() {
        use lumapaint_core::document::Point;
        for (tool, anchor) in [
            (CanvasTool::VectorRectangle, [40.0, 40.0]),
            (CanvasTool::VectorEllipse, [240.0, 140.0]),
        ] {
            cancel_vector_drag();
            DIRECT_POINTS.with(|points| points.borrow_mut().clear());
            let mut document = Document::default();
            document.add_vector_layer().unwrap();
            let flags = NSEventModifierFlags::empty();
            let point = |x, y| Point { x, y };
            vector_pointer(&mut document, tool, point(40.0, 40.0), 0, flags).unwrap();
            vector_pointer(&mut document, tool, point(240.0, 240.0), 2, flags).unwrap();
            let id = document.svg_layers().next().unwrap().vector_objects[0]
                .id
                .clone();
            direct_pointer(&mut document, point(anchor[0], anchor[1]), 0, flags).unwrap();
            direct_pointer(
                &mut document,
                point(anchor[0] + 20.0, anchor[1] + 10.0),
                1,
                flags,
            )
            .unwrap();
            direct_pointer(
                &mut document,
                point(anchor[0] + 20.0, anchor[1] + 10.0),
                2,
                flags,
            )
            .unwrap();
            let edited = document.svg_layers().next().unwrap().vector_objects[0].clone();
            assert_eq!(edited.kind, VectorObjectKind::Bezier);
            vector_select_pointer(&mut document, point(400.0, 400.0), 0, flags).unwrap();
            vector_select_pointer(&mut document, point(400.0, 400.0), 2, flags).unwrap();
            assert!(document.selected_vector_ids().is_empty());
            vector_select_pointer(&mut document, point(140.0, 140.0), 0, flags).unwrap();
            assert_eq!(document.selected_vector_ids(), &[id]);
            vector_select_pointer(&mut document, point(160.0, 170.0), 2, flags).unwrap();
            let moved = document.svg_layers().next().unwrap().vector_objects[0].clone();
            assert_eq!(&moved.transform[4..], &[20.0, 30.0]);
            document.undo();
            assert_eq!(
                document.svg_layers().next().unwrap().vector_objects[0],
                edited
            );
            document.redo();
            assert_eq!(
                document.svg_layers().next().unwrap().vector_objects[0],
                moved
            );
        }
    }

    #[test]
    fn direct_selection_moves_multiple_anchors_with_preview_and_one_undo() {
        DIRECT_POINTS.with(|points| points.borrow_mut().clear());
        DIRECT_GESTURE.with(|draft| draft.borrow_mut().take());
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        let pen = PenDraft {
            layer: layer.clone(),
            brush: Brush::default(),
            nodes: vec![
                ([20., 20.], [20., 30.]),
                ([100., 20.], [100., 30.]),
                ([150., 80.], [150., 90.]),
            ],
        };
        let original = pen.object(false, "direct-test").unwrap();
        document
            .upsert_vector_object(&layer, original.clone())
            .unwrap();
        let p = |x, y| lumapaint_core::document::Point { x, y };
        let flags = NSEventModifierFlags::empty();
        direct_pointer(&mut document, p(20., 20.), 0, flags).unwrap();
        direct_pointer(&mut document, p(20., 20.), 2, flags).unwrap();
        direct_pointer(&mut document, p(100., 20.), 0, NSEventModifierFlags::Shift).unwrap();
        direct_pointer(&mut document, p(100., 20.), 2, flags).unwrap();
        assert_eq!(DIRECT_POINTS.with(|points| points.borrow().len()), 2);
        direct_pointer(&mut document, p(20., 20.), 0, flags).unwrap();
        direct_pointer(&mut document, p(30., 35.), 1, flags).unwrap();
        let preview = direct_preview(&document, 1.).unwrap();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            original
        );
        assert_eq!(
            preview.svg_layers().next().unwrap().vector_objects[0].control_points[0],
            [30., 35.]
        );
        direct_pointer(&mut document, p(30., 35.), 2, flags).unwrap();
        let edited = document.svg_layers().next().unwrap().vector_objects[0].clone();
        assert_eq!(edited.control_points[0], [30., 35.]);
        assert_eq!(edited.control_points[3], [110., 35.]);
        assert_eq!(edited.control_points[6], original.control_points[6]);
        document.undo();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            original
        );
        document.redo();
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            edited
        );
        // Empty-space marquee selects only anchors inside its bounds.
        direct_pointer(&mut document, p(5., 5.), 0, flags).unwrap();
        direct_pointer(&mut document, p(120., 50.), 1, flags).unwrap();
        direct_preview(&document, 1.).unwrap();
        direct_pointer(&mut document, p(120., 50.), 2, flags).unwrap();
        assert_eq!(DIRECT_POINTS.with(|points| points.borrow().len()), 2);
        direct_pointer(&mut document, p(30., 35.), 0, flags).unwrap();
        direct_pointer(&mut document, p(60., 50.), 1, flags).unwrap();
        assert!(cancel_vector_drag());
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects[0],
            edited
        );
        DIRECT_POINTS.with(|points| points.borrow_mut().clear());
    }

    #[test]
    fn pen_click_drag_close_and_cancel() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        document.select_layer(layer).unwrap();
        let point = |x, y| lumapaint_core::document::Point { x, y };
        bezier_pointer(&mut document, point(10., 10.), 0).unwrap();
        bezier_pointer(&mut document, point(10., 50.), 1).unwrap();
        bezier_pointer(&mut document, point(10., 50.), 2).unwrap();
        bezier_pointer(&mut document, point(100., 10.), 0).unwrap();
        bezier_pointer(&mut document, point(100., 10.), 2).unwrap();
        assert!(document
            .svg_layers()
            .next()
            .unwrap()
            .vector_objects
            .is_empty());
        bezier_pointer(&mut document, point(10., 10.), 0).unwrap();
        assert!(PEN_DRAFT.with(|draft| draft.borrow().is_none()));
        assert!(document.svg_layers().next().unwrap().vector_objects[0]
            .path
            .data
            .ends_with(" Z"));
        bezier_pointer(&mut document, point(50., 50.), 0).unwrap();
        assert!(cancel_vector_drag());
        assert_eq!(
            document.svg_layers().next().unwrap().vector_objects.len(),
            1
        );
    }

    #[test]
    fn tile_result_rejects_an_old_document_canvas_or_scale() {
        let key = TileKey {
            source: RasterKey {
                document_id: 2,
                revision: 7,
                canvas_token: 11,
            },
            scale: 2,
        };
        assert!(key.matches(2, 7, 11, 2));
        assert!(!key.matches(3, 7, 11, 2));
        assert!(!key.matches(2, 8, 11, 2));
        assert!(!key.matches(2, 7, 12, 2));
        assert!(!key.matches(2, 7, 11, 1));
    }

    #[test]
    fn raster_result_requires_the_same_document_revision_and_canvas() {
        let key = RasterKey {
            document_id: 7,
            revision: 12,
            canvas_token: 3,
        };
        assert!(key.matches(7, 12, 3));
        assert!(!key.matches(8, 12, 3));
        assert!(!key.matches(7, 13, 3));
        assert!(!key.matches(7, 12, 4));
    }

    #[test]
    fn text_drag_updates_before_mouse_up_and_commits_once() {
        use lumapaint_core::{document::Point, vector::VectorText};
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Follow".into(),
                ..Default::default()
            },
            position: [100.0, 100.0],
            color: [0, 0, 0],
        })
        .unwrap();
        let revision = doc.snapshot().revision;
        let flags = NSEventModifierFlags::empty();
        vector_select_pointer(&mut doc, Point { x: 110.0, y: 110.0 }, 0, flags).unwrap();
        vector_select_pointer(&mut doc, Point { x: 140.0, y: 125.0 }, 1, flags).unwrap();
        assert_eq!(VECTOR_MOVE.with(|offset| offset.get()), [30.0, 15.0]);
        assert_eq!(doc.snapshot().text_objects[0].position, [100.0, 100.0]);
        vector_select_pointer(&mut doc, Point { x: 160.0, y: 95.0 }, 1, flags).unwrap();
        assert_eq!(VECTOR_MOVE.with(|offset| offset.get()), [50.0, -15.0]);
        assert_eq!(doc.snapshot().revision, revision);
        vector_select_pointer(&mut doc, Point { x: 160.0, y: 95.0 }, 2, flags).unwrap();
        assert_eq!(VECTOR_MOVE.with(|offset| offset.get()), [0.0, 0.0]);
        assert_eq!(doc.snapshot().text_objects[0].position, [150.0, 85.0]);
        assert_eq!(doc.snapshot().revision, revision + 1);
        doc.undo();
        assert_eq!(doc.snapshot().text_objects[0].position, [100.0, 100.0]);
        vector_select_pointer(&mut doc, Point { x: 110.0, y: 110.0 }, 0, flags).unwrap();
        vector_select_pointer(&mut doc, Point { x: 190.0, y: 180.0 }, 1, flags).unwrap();
        assert!(cancel_vector_drag());
        vector_select_pointer(&mut doc, Point { x: 190.0, y: 180.0 }, 2, flags).unwrap();
        assert_eq!(doc.snapshot().text_objects[0].position, [100.0, 100.0]);
    }

    #[test]
    fn selection_modifiers_route_native_drags() {
        assert_eq!(
            selection_mode(NSEventModifierFlags::empty()),
            SelectionMode::Replace
        );
        assert_eq!(
            selection_mode(NSEventModifierFlags::Shift),
            SelectionMode::Add
        );
        assert_eq!(
            selection_mode(NSEventModifierFlags::Option),
            SelectionMode::Subtract
        );
        assert_eq!(
            selection_mode(NSEventModifierFlags::Shift | NSEventModifierFlags::Option),
            SelectionMode::Subtract
        );
    }

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
            tool: CanvasTool::Brush,
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
            tool: CanvasTool::Brush,
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
    static PROJECT_FINGERPRINT: RefCell<Option<crate::project_file::FileFingerprint>> = const { RefCell::new(None) };
}

pub fn confirm_discard() -> bool {
    if let Err(error) = text_editor::finish(true) {
        emit_error(error);
        return false;
    }
    let active_dirty = DOCUMENT_OPEN.with(|open| open.get())
        && ACTIVE_TILED_DOCUMENT.with(|document| {
            document.borrow().as_ref().map_or_else(
                || DOCUMENT.with(|doc| doc.borrow().snapshot().dirty),
                TiledSession::dirty,
            )
        });
    let inactive_dirty = INACTIVE_DOCUMENTS
        .with(|documents| documents.borrow().iter().any(|entry| entry.content.dirty()));
    if !active_dirty && !inactive_dirty {
        return true;
    }
    confirm_unsaved_changes()
}

fn confirm_unsaved_changes() -> bool {
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

fn ensure_document_open() -> Result<(), String> {
    finish_open_pen()?;
    cancel_vector_drag();
    text_editor::finish(true)?;
    (DOCUMENT_OPEN.with(|open| open.get())
        && ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_none()))
    .then_some(())
    .ok_or_else(|| "This document is read-only in the current workspace".into())
}

pub fn new_document() -> Result<DocumentWorkspaceSnapshot, String> {
    text_editor::finish(true)?;
    park_active_document();
    activate_document(OpenDocument {
        id: next_document_id(),
        content: OpenDocumentContent::Legacy(Box::default()),
        path: None,
        fingerprint: None,
    });
    redraw()?;
    emit_document();
    Ok(workspace_snapshot())
}

pub fn switch_document(id: u64) -> Result<DocumentWorkspaceSnapshot, String> {
    text_editor::finish(true)?;
    if DOCUMENT_OPEN.with(|open| open.get()) && ACTIVE_DOCUMENT_ID.with(|active| active.get()) == id
    {
        return Ok(workspace_snapshot());
    }
    let entry = INACTIVE_DOCUMENTS.with(|documents| {
        let mut documents = documents.borrow_mut();
        documents
            .iter()
            .position(|entry| entry.id == id)
            .map(|index| documents.remove(index))
    });
    let Some(entry) = entry else {
        return Err("Document not found".into());
    };
    park_active_document();
    activate_document(entry);
    redraw()?;
    emit_document();
    Ok(workspace_snapshot())
}

pub fn close_document(id: u64) -> Result<DocumentWorkspaceSnapshot, String> {
    text_editor::finish(true)?;
    let is_active = DOCUMENT_OPEN.with(|open| open.get())
        && ACTIVE_DOCUMENT_ID.with(|active| active.get()) == id;
    if is_active {
        let dirty = ACTIVE_TILED_DOCUMENT.with(|document| {
            document.borrow().as_ref().map_or_else(
                || DOCUMENT.with(|doc| doc.borrow().snapshot().dirty),
                TiledSession::dirty,
            )
        });
        if dirty && !confirm_unsaved_changes() {
            return Ok(workspace_snapshot());
        }
        DOCUMENT.with(|doc| *doc.borrow_mut() = Document::default());
        ACTIVE_TILED_DOCUMENT.with(|document| document.borrow_mut().take());
        PROJECT_PATH.with(|path| *path.borrow_mut() = None);
        PROJECT_FINGERPRINT.with(|fingerprint| *fingerprint.borrow_mut() = None);
        let next = INACTIVE_DOCUMENTS.with(|documents| documents.borrow_mut().pop());
        if let Some(next) = next {
            activate_document(next);
            redraw()?;
            emit_document();
        } else {
            DOCUMENT_OPEN.with(|open| open.set(false));
            CHECKPOINT_REVISION.with(|last| last.set(None));
            redraw()?;
            emit_workspace();
        }
    } else {
        INACTIVE_DOCUMENTS.with(|documents| -> Result<(), String> {
            let mut documents = documents.borrow_mut();
            let Some(index) = documents.iter().position(|entry| entry.id == id) else {
                return Err("Document not found".into());
            };
            if documents[index].content.dirty() && !confirm_unsaved_changes() {
                return Ok(());
            }
            documents.remove(index);
            Ok(())
        })?;
        emit_workspace();
    }
    Ok(workspace_snapshot())
}

pub fn file_action(action: super::FileAction) -> Result<DocumentSnapshot, String> {
    text_editor::finish(true)?;
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
            let (project, fingerprint) = crate::project_file::read_any_with_fingerprint(&path)?;
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let content = match project {
                crate::project_file::ProjectData::Legacy(mut document) => {
                    for layer in document.svg_layers() {
                        validate_svg(&layer.source)?;
                    }
                    document.mark_saved(name);
                    OpenDocumentContent::Legacy(document)
                }
                crate::project_file::ProjectData::Tiled(state) => {
                    OpenDocumentContent::Tiled(TiledSession {
                        document: TiledRasterDocument::from_state(state)?,
                        file_name: Some(name),
                        saved_revision: 0,
                    })
                }
            };
            park_active_document();
            activate_document(OpenDocument {
                id: next_document_id(),
                content,
                path: Some(path),
                fingerprint: Some(fingerprint),
            });
            if let Err(error) = redraw() {
                emit_error(error);
            }
        }
        FileAction::Save | FileAction::SaveAs => {
            if !DOCUMENT_OPEN.with(|open| open.get()) {
                return Err("No document is open".into());
            }
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
                if matches!(action, FileAction::Save) {
                    let expected = PROJECT_FINGERPRINT.with(|slot| *slot.borrow());
                    if let Some(expected) = expected {
                        let current = crate::project_file::fingerprint(&path).map_err(|_| {
                            "The project file was moved or deleted outside LumaPaint. Use Save As to keep your changes.".to_string()
                        })?;
                        if current != expected {
                            return Err("The project file changed outside LumaPaint. Use Save As to avoid overwriting those changes.".into());
                        }
                    }
                }
                let tiled = ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_some());
                if tiled {
                    ACTIVE_TILED_DOCUMENT.with(|document| {
                        let document = document.borrow();
                        let document = document.as_ref().ok_or("Missing tiled document")?;
                        crate::project_file::write_tiled(&path, &document.document.state())
                    })?;
                } else {
                    let bytes = DOCUMENT.with(|doc| doc.borrow_mut().encode())?;
                    crate::project_file::write(&path, &bytes)?;
                }
                let fingerprint = crate::project_file::fingerprint(&path)?;
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                if tiled {
                    ACTIVE_TILED_DOCUMENT.with(|document| {
                        if let Some(document) = document.borrow_mut().as_mut() {
                            document.file_name = Some(name);
                            document.saved_revision = document.document.revision();
                        }
                    });
                } else {
                    DOCUMENT.with(|doc| doc.borrow_mut().mark_saved(name));
                }
                PROJECT_PATH.with(|slot| *slot.borrow_mut() = Some(path));
                PROJECT_FINGERPRINT.with(|slot| *slot.borrow_mut() = Some(fingerprint));
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
pub fn delete_recovery(id: String) -> Result<crate::recovery::Info, String> {
    RECOVERY.with(|slot| {
        let recovery = slot.borrow();
        let recovery = recovery.as_ref().ok_or("Recovery is unavailable")?;
        recovery.delete_candidate(&id)?;
        recovery.info()
    })
}
pub fn delete_all_recoveries() -> Result<crate::recovery::Info, String> {
    RECOVERY.with(|slot| {
        let recovery = slot.borrow();
        let recovery = recovery.as_ref().ok_or("Recovery is unavailable")?;
        recovery.delete_all_candidates()?;
        recovery.info()
    })
}
pub fn restore_recovery(id: String) -> Result<DocumentSnapshot, String> {
    text_editor::finish(true)?;
    let document = RECOVERY.with(|slot| {
        slot.borrow()
            .as_ref()
            .ok_or("Recovery is unavailable")?
            .read_candidate(&id)
    })?;
    for layer in document.svg_layers() {
        validate_svg(&layer.source)?;
    }
    let mut recovered = Document::default();
    recovered.replace_recovered(document);
    park_active_document();
    activate_document(OpenDocument {
        id: next_document_id(),
        content: OpenDocumentContent::Legacy(Box::new(recovered)),
        path: None,
        fingerprint: None,
    });
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

pub fn text_fonts() -> Result<Vec<String>, String> {
    text_editor::font_families()
}
pub fn begin_text_edit(settings: TextSettings) -> Result<(), String> {
    text_editor::begin(settings)
}
pub fn set_text_edit_color(id: Option<String>, color: [u8; 3]) -> Result<(), String> {
    text_editor::set_color(id, color)
}
pub fn update_text_edit(
    mut settings: TextSettings,
    patch: Option<lumapaint_core::vector::TextStylePatch>,
) -> Result<DocumentSnapshot, String> {
    if text_editor::active() {
        text_editor::update(settings, patch)?;
    } else {
        if let Some(patch) = patch {
            settings.text.apply_style(
                0,
                settings.text.content.encode_utf16().count(),
                &patch,
                settings.color,
            )?;
        }
        set_text_object(settings)?;
    }
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn finish_text_edit(commit: bool) -> Result<DocumentSnapshot, String> {
    text_editor::finish(commit)?;
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
