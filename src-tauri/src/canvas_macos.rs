//! AppKit ownership is isolated here; no Apple types enter the renderer or document core.
use super::{
    CanvasInfo, CanvasRequest, CanvasTool, DocumentAction, DocumentTabSnapshot,
    DocumentWorkspaceSnapshot,
};
use lumapaint_core::document::{
    Brush, CanvasColor, ColorMode, ColorProfile, Document, DocumentSettings, DocumentSnapshot,
    DocumentUnit, LayerSettings, LayerSnapshot, NewDocumentSettings, PaintProjectionState,
    SelectionMode, SelectionShape, Stroke, TextSettings,
};
use lumapaint_core::graph::{ChangeTarget, ProcessingGraph};
use lumapaint_core::tiles::{TileInvalidation, TiledRasterDocument};
use lumapaint_core::vector::{
    FillRule, PathEditAction, PathOperation, VectorObject, VectorObjectKind, VectorPaint,
    VectorPath,
};
use lumapaint_formats::native::NativeDocumentCodec;
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
#[path = "pixel_move_macos.rs"]
mod pixel_move;
#[path = "pixel_paint_macos.rs"]
mod pixel_paint;
#[path = "raster_import_macos.rs"]
mod raster_import;
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
                let before_evicted = frame_cache.stats().evicted_entries;
                let result: Result<Vec<PreparedSvgLayer>, String> = job
                    .layers
                    .iter()
                    .map(|layer| frame_cache.prepare_layer(layer, job.size.0, job.size.1))
                    .collect();
                let cpu_time = started.elapsed();
                if render_metrics_enabled() {
                    let stats = frame_cache.stats();
                    eprintln!(
                        "LumaPaint text-frame cache rasterized={} reused={} entries={} payload_bytes={} payload_budget_bytes={} evicted={}",
                        frame_cache.rasterized_frames - before_rasterized,
                        frame_cache.reused_frames - before_reused,
                        stats.entries,
                        stats.retained_payload_bytes,
                        stats.payload_budget_bytes,
                        stats.evicted_entries - before_evicted
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

fn pan_offset(current: (f32, f32), delta: (f32, f32)) -> (f32, f32) {
    if !delta.0.is_finite() || !delta.1.is_finite() {
        return current;
    }
    (
        (current.0 + delta.0).clamp(-8192., 8192.),
        (current.1 + delta.1).clamp(-8192., 8192.),
    )
}

fn pinch_zoom(current: f32, magnification: f32) -> f32 {
    if !magnification.is_finite() {
        return current;
    }
    (current * (1. + magnification).max(0.01)).clamp(0.25, 4.)
}

define_class!(
    #[unsafe(super(NSView))]
    #[ivars = ()]
    struct PaintView;
impl PaintView {
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let local = NSPoint::new(point.x - self.frame().origin.x, point.y - self.frame().origin.y);
            if CANVAS_OVERLAY.with(|value| value.get()).is_some_and(|r| local.x >= r[0] && local.x <= r[2] && local.y >= r[1] && local.y <= r[3]) {
                return std::ptr::null_mut();
            }
            // SAFETY: forward AppKit hit testing to the NSView superclass.
            unsafe { msg_send![super(self), hitTest: point] }
        }
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
                    CanvasTool::VectorScale | CanvasTool::VectorRotate => Some(NSCursor::crosshairCursor()),
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
            if TOOL.with(|t|t.get()) == CanvasTool::VectorSelect {
                let viewport=CANVAS.with(|slot|slot.borrow().as_ref().map(|c|c.viewport));
                let corners=raster_import::corners().or_else(|| DOCUMENT.with(|d|d.borrow().selected_vector_box()));
                if let (Some(v),Some(c))=(viewport,corners) {
                    let w=v.width as f32/v.scale; let h=v.height as f32/v.scale;
                    let fit=((w-48.)/v.document_width).min((h-48.)/v.document_height).max(0.01)*v.zoom;
                    for i in 0..4 {
                        let a=c[i];let b=c[(i+1)%4];
                        for (point,radius,cursor) in [(a,17.,NSCursor::crosshairCursor()),(a,7.,NSCursor::frameResizeCursorFromPosition_inDirections([NSCursorFrameResizePosition::TopLeft,NSCursorFrameResizePosition::TopRight,NSCursorFrameResizePosition::BottomRight,NSCursorFrameResizePosition::BottomLeft][i],NSCursorFrameResizeDirections::All)), ([(a[0]+b[0])*0.5,(a[1]+b[1])*0.5],7.,NSCursor::frameResizeCursorFromPosition_inDirections(if i%2==0 {NSCursorFrameResizePosition::Top}else{NSCursorFrameResizePosition::Left},NSCursorFrameResizeDirections::All))] {
                            let x=w*0.5+v.pan_x+(point[0]-v.document_width*0.5)*fit;
                            let y=h*0.5+v.pan_y+(point[1]-v.document_height*0.5)*fit;
                            self.addCursorRect_cursor(NSRect::new(NSPoint::new(f64::from(x-radius),f64::from(y-radius)),NSSize::new(f64::from(radius*2.),f64::from(radius*2.))),&cursor);
                        }
                    }
                }
            }
        }
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            if self.isHidden() || !DOCUMENT_OPEN.with(|open|open.get()) || PANNING.with(|p|p.get()) {return;}
            if !event.hasPreciseScrollingDeltas() {return;}
            let dx=event.scrollingDeltaX() as f32;
            let dy=event.scrollingDeltaY() as f32;
            if !dx.is_finite() || !dy.is_finite() || (dx==0. && dy==0.) {return;}
            if paint_gesture_active() {return;}
            if text_editor::active() {if let Err(error)=text_editor::finish(true){emit_error(error);return;}}
            // AppKit already applies the user's natural-scrolling preference and momentum.
            // Delta units are view points, matching the hand tool, independent of zoom/Retina.
            self.pan_by(dx,dy);
        }
        #[unsafe(method(magnifyWithEvent:))]
        fn magnify_with_event(&self, event: &NSEvent) {
            if self.isHidden() || !DOCUMENT_OPEN.with(|open|open.get()) || PANNING.with(|p|p.get()) { return; }
            let delta=event.magnification() as f32;
            if !delta.is_finite() || delta==0. {return;}
            if paint_gesture_active() {return;}
            if text_editor::active() {if let Err(error)=text_editor::finish(true){emit_error(error);return;}}
            let location=self.convertPoint_fromView(event.locationInWindow(),None);
            let viewport=CANVAS.with(|slot|slot.borrow().as_ref().map(|c|c.viewport));
            if let Some(viewport)=viewport {
                let zoom=pinch_zoom(viewport.zoom,delta);
                self.apply_zoom(viewport,location,zoom);
            }
        }
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() { window.makeFirstResponder(Some(self)); }
            if SPACE_DOWN.with(|space| space.get()) || TOOL.with(|tool| tool.get()) == CanvasTool::Hand { self.begin_pan(event); } else {
                if matches!(TOOL.with(|tool| tool.get()), CanvasTool::Brush | CanvasTool::Eraser) {
                    begin_precise_paint_input();
                }
                self.pointer(event, 0);
                if !paint_gesture_active() { end_precise_paint_input(); }
            }
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) { if PANNING.with(|value| value.get()) { self.update_pan(event); } else { self.pointer(event, 1); } }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) { if PANNING.with(|value| value.get()) { self.update_pan(event); PANNING.with(|value| value.set(false)); self.refresh_cursor(); } else { self.pointer(event, 2); } end_precise_paint_input(); }
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) { self.begin_pan(event); }
        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) { self.update_pan(event); }
        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) { self.update_pan(event); PANNING.with(|value| value.set(false)); self.refresh_cursor(); }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if self.isHidden() { return; }
            if raster_import::active() {
                if [36,76,53].contains(&event.keyCode()) {
                    if let Err(error) = raster_import::finish(event.keyCode()!=53) { emit_error(error); }
                }
                return;
            }
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
            // WebView fields own their editing shortcuts while focused (including Cmd+A).
            // AppKit asks sibling views for key equivalents even when they are not responders.
            let focused = self.window().and_then(|window| window.firstResponder()).is_some_and(|responder| {
                Retained::as_ptr(&responder).cast::<c_void>() == (self as *const Self).cast::<c_void>()
            });
            if !focused { return false.into(); }
            if raster_import::active() { return event.modifierFlags().contains(NSEventModifierFlags::Command).into(); }
            let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
            if command && [7,8,9].contains(&event.keyCode()) {
                report_edit(match event.keyCode() { 7 => DocumentAction::Cut, 8 => DocumentAction::Copy, _ => DocumentAction::Paste }); true
            } else if command && [0, 2].contains(&event.keyCode()) {
                report_edit(if event.keyCode() == 0 { DocumentAction::SelectAll } else { DocumentAction::Deselect }); true
            } else if command && event.keyCode() == 34 && event.modifierFlags().contains(NSEventModifierFlags::Shift) {
                report_edit(DocumentAction::InvertSelection); true
            } else if command && event.keyCode() == 45 {
                if let Some(app) = APP.get() { let _ = app.emit_to("main", "new-document-requested", ()); }
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

    fn apply_zoom(&self, viewport: Viewport, location: NSPoint, zoom: f32) {
        if zoom == viewport.zoom || !location.x.is_finite() || !location.y.is_finite() {
            return;
        }
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
        self.refresh_cursor();
        if let Some(app) = APP.get() {
            let _ = app.emit_to("main", "canvas-zoom-changed", zoom);
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
        if raster_import::active() {
            raster_import::pointer(point, phase, event.modifierFlags());
            if let Err(error) = redraw() {
                emit_error(error);
            }
            return;
        }
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
                self.apply_zoom(viewport, location, zoom);
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
                    && DOCUMENT.with(|doc| doc.borrow().selected_layer_is_vector())
                    && event.clickCount() == 2
                    && DOCUMENT.with(|doc| {
                        selected_text_resize_handle(&doc.borrow(), [point.x, point.y]).is_none()
                    }))
            {
                if DOCUMENT.with(|doc| doc.borrow().text_at([point.x, point.y]).is_some())
                    || (point.x >= 0.0
                        && point.y >= 0.0
                        && point.x < viewport.document_width
                        && point.y < viewport.document_height)
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
                if selection_uses_pixel_move(&doc, tool) {
                    return pixel_move::pointer(&mut doc, point, phase, event.modifierFlags());
                }
                if matches!(
                    tool,
                    CanvasTool::VectorSelect
                        | CanvasTool::VectorScale
                        | CanvasTool::VectorRotate
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
                if pixel_paint::pointer(
                    &mut doc,
                    point,
                    phase,
                    pressure,
                    tool == CanvasTool::Eraser,
                )? {
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
        self.pan_by(current.0 - previous.0, current.1 - previous.1);
    }

    fn pan_by(&self, dx: f32, dy: f32) {
        PAN.with(|pan| pan.set(pan_offset(pan.get(), (dx, dy))));
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
        self.refresh_cursor();
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
    fn committed_object(
        &self,
        closed: bool,
        id: &str,
        snap_distance: f32,
    ) -> Result<VectorObject, String> {
        let first = self.nodes[0].0;
        let last = self.nodes.last().unwrap().0;
        if self.nodes.len() >= 3 && (first[0] - last[0]).hypot(first[1] - last[1]) <= snap_distance
        {
            // The final node already supplies the closing curve. Snap its
            // endpoint and incoming handle instead of adding another segment.
            let mut object = self.object(false, id)?;
            let end = object.control_points.len() - 1;
            object.control_points[end] = first;
            for axis in 0..2 {
                object.control_points[end - 1][axis] += first[axis] - last[axis];
            }
            object.path.data = lumapaint_core::bezier::path_data(&object.control_points, true)?;
            object.validate()?;
            Ok(object)
        } else {
            self.object(closed, id)
        }
    }

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
            live_corners: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: id.into(),
            name: "Bezier path".into(),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
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
            stroke_style: Default::default(),
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
            color: document.layer_guide_color(&self.layer),
        });
        guide.stroke_width = 1.0 / zoom;
        // Keep the curve centerline above the actual brush-width stroke.
        let radius = 3.0 / zoom;
        for (anchor, handle) in &self.nodes {
            let incoming = [2.0 * anchor[0] - handle[0], 2.0 * anchor[1] - handle[1]];
            let _ = write!(
                guide.path.data,
                " M {} {} L {} {}",
                incoming[0], incoming[1], handle[0], handle[1]
            );
            for (index, point) in [*anchor, *handle, incoming].into_iter().enumerate() {
                if index != 0 {
                    let r = radius * 0.7;
                    let _ = write!(
                        guide.path.data,
                        " M {} {} a {r} {r} 0 1 0 {} 0 a {r} {r} 0 1 0 {} 0",
                        point[0] - r,
                        point[1],
                        2. * r,
                        -2. * r
                    );
                    continue;
                }
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
        preview.append_vector_guide(&self.layer, guide)?;
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
        // A stricter two-screen-point threshold also resolves near-duplicate
        // endpoints when an open draft is committed by switching tools.
        let snap_distance = CANVAS.with(|slot| {
            pen_close_tolerance(slot.borrow().as_ref().map(|canvas| canvas.viewport)) * 0.25
        });
        document.upsert_vector_object(
            &draft.layer,
            draft.committed_object(closed, &format!("vector-object-{serial}"), snap_distance)?,
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

// Keep the closing target eight logical screen points wide in radius.
// Fit-to-window, zoom, and Retina backing scale all affect document distance.
fn pen_close_tolerance(viewport: Option<Viewport>) -> f32 {
    viewport.map_or(8., |v| {
        let fit = ((v.width as f32 / v.scale - 48.) / v.document_width)
            .min((v.height as f32 / v.scale - 48.) / v.document_height)
            .max(0.01);
        8. / (fit * v.zoom)
    })
}

fn pen_should_close(draft: &PenDraft, position: [f32; 2], tolerance: f32) -> bool {
    draft.nodes.len() >= 2
        && (draft.nodes[0].0[0] - position[0]).hypot(draft.nodes[0].0[1] - position[1]) <= tolerance
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
        let tolerance = CANVAS
            .with(|slot| pen_close_tolerance(slot.borrow().as_ref().map(|canvas| canvas.viewport)));
        let close = PEN_DRAFT.with(|draft| {
            draft
                .borrow()
                .as_ref()
                .is_some_and(|draft| pen_should_close(draft, position, tolerance))
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
    let tolerance = box_tolerance();
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
        document.select_vector_objects(vec![object.id.clone()])?;
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
        if self.original != self.object {
            preview.direct_object_preview(&self.layer, self.object.clone())?;
        }
        let mut guide = self.object.clone();
        guide.clipping_group = None;
        guide.group_path.clear();
        guide.id = format!(
            "guides-{}",
            self.object.id.chars().take(50).collect::<String>()
        );
        guide.kind = VectorObjectKind::Compound;
        guide.fill = None;
        guide.stroke = Some(VectorPaint {
            color: document.layer_guide_color(&self.layer),
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
        for start in lumapaint_core::bezier::segment_indices(&self.object) {
            let segment = &points[start..start + 4];
            let _ = write!(
                guide.path.data,
                " M {} {} C {} {} {} {} {} {}",
                segment[0][0],
                segment[0][1],
                segment[1][0],
                segment[1][1],
                segment[2][0],
                segment[2][1],
                segment[3][0],
                segment[3][1]
            );
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
        for index in lumapaint_core::bezier::control_indices(&self.object) {
            let point = points[index];
            if !index.is_multiple_of(3) {
                let _ = write!(
                    guide.path.data,
                    " M {} {} a {r} {r} 0 1 0 {} 0 a {r} {r} 0 1 0 {} 0",
                    point[0] - r,
                    point[1],
                    2. * r,
                    -2. * r
                );
                continue;
            }
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
        preview.append_vector_guide(&self.layer, guide)?;
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
    if tool == CanvasTool::VectorRotate {
        return rotate_pointer(document, point, phase, modifiers);
    }
    if tool == CanvasTool::VectorScale {
        return scale_pointer(document, point, phase);
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
            live_corners: None,
            text: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: format!("vector-object-{serial}"),
            name: format!("{name} {serial}"),
            group_path: Vec::new(),
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: path,
                fill_rule: FillRule::NonZero,
            },
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill,
            stroke,
            stroke_width,
            stroke_style: Default::default(),
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

// Direct selection reaches imported SVG geometry even when its layer is an image layer.
fn selection_uses_pixel_move(document: &Document, tool: CanvasTool) -> bool {
    !document.selected_layer_is_vector()
        && (tool == CanvasTool::VectorSelect
            || (tool == CanvasTool::VectorDirectSelect && document.direct_objects().is_empty()))
}

fn direct_objects(document: &Document) -> Vec<(String, VectorObject)> {
    document.direct_objects()
}

fn direct_select_ids(document: &mut Document) -> Result<(), String> {
    let ids: Vec<String> =
        DIRECT_POINTS.with(|points| points.borrow().iter().map(|(id, _)| id.clone()).collect());
    let layer = document
        .direct_objects()
        .into_iter()
        .find(|(_, o)| ids.contains(&o.id))
        .map(|(l, _)| l);
    if let Some(layer) = layer {
        document.select_layer(layer)?;
    }
    document.select_direct_objects(ids)
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
        let tolerance = box_tolerance();
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
                .map(|index| {
                    let index = bezier::canonical_anchor(object, index);
                    vec![(object.id.clone(), index)]
                })
            })
            .or_else(|| {
                objects.iter().rev().find_map(|(_, object)| {
                    bezier::segment_hit(object, position, tolerance).map(|(index, _)| {
                        let end = bezier::canonical_anchor(object, index + 3);
                        vec![(object.id.clone(), index), (object.id.clone(), end)]
                    })
                })
            })
            .or_else(|| {
                objects
                    .iter()
                    .rev()
                    .find(|(_, object)| object.hit_test(position, tolerance))
                    .map(|(_, object)| {
                        bezier::anchor_indices(object)
                            .into_iter()
                            .map(|index| (object.id.clone(), index))
                            .collect()
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
                        for index in bezier::anchor_indices(&object) {
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

fn anchor_guides_preview(document: &Document, scale: f32) -> Result<Document, String> {
    let mut preview = document.clone();
    for (layer, object) in direct_objects(document)
        .into_iter()
        .filter(|(_, o)| document.selected_vector_ids().contains(&o.id))
    {
        let draft = AnchorDraft {
            layer,
            original: object.clone(),
            object,
            index: 0,
            start: [0., 0.],
        };
        preview = draft.preview(&preview, scale)?;
    }
    Ok(preview)
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
        marker.clipping_group = None;
        marker.group_path.clear();
        marker.id = format!("direct-selected-{serial}");
        marker.kind = VectorObjectKind::Compound;
        marker.transform = [1., 0., 0., 1., 0., 0.];
        marker.stroke = None;
        marker.fill = Some(VectorPaint {
            color: document.layer_guide_color(&layer),
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
            preview.append_vector_guide(&layer, marker)?;
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
                live_corners: None,
                opacity: 1.0,
                blend_mode: "normal".into(),
                id: "direct-marquee".into(),
                name: "Selection".into(),
                group_path: Vec::new(),
                clipping_group: None,
                bounds_reset: false,
                text: None,
                path: VectorPath {
                    data: format!("M {x} {y} h {w} v {h} h {} Z", -w),
                    fill_rule: FillRule::NonZero,
                },
                transform: [1., 0., 0., 1., 0., 0.],
                fill: None,
                stroke: Some(VectorPaint {
                    color: [48, 144, 255, 255],
                }),
                stroke_style: Default::default(),
                stroke_width: 1. / zoom,
                visible: true,
                kind: VectorObjectKind::Compound,
                control_points: vec![[x, y], [x + w, y + h]],
            },
        )?;
    }
    Ok(preview)
}

#[derive(Clone, Copy)]
struct ScaleDraft {
    center: [f32; 2],
    start: [f32; 2],
    scale: f32,
}
fn scale_pointer(
    document: &mut Document,
    point: lumapaint_core::document::Point,
    phase: u8,
) -> Result<(), String> {
    if phase == 0 {
        cancel_vector_drag();
        if document.selected_vector_ids().is_empty() {
            document.select_vector_at(point, object_selection_tolerance(), false);
        }
        if let Some([x, y, right, bottom]) = document.selected_vector_bounds() {
            SCALE_DRAFT.with(|draft| {
                *draft.borrow_mut() = Some(ScaleDraft {
                    center: [(x + right) * 0.5, (y + bottom) * 0.5],
                    start: [point.x, point.y],
                    scale: 1.,
                })
            });
        }
    } else {
        SCALE_DRAFT.with(|draft| {
            if let Some(draft) = draft.borrow_mut().as_mut() {
                let initial =
                    (draft.start[0] - draft.center[0]).hypot(draft.start[1] - draft.center[1]);
                draft.scale = if initial >= 1. {
                    ((point.x - draft.center[0]).hypot(point.y - draft.center[1]) / initial)
                        .clamp(0.01, 100.)
                } else {
                    ((point.x - draft.start[0]) / 100.).exp().clamp(0.01, 100.)
                };
            }
        });
        if phase == 2 {
            if let Some(draft) = SCALE_DRAFT.with(|d| d.borrow_mut().take()) {
                document.scale_selected_vectors(draft.center, draft.scale)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct RotateDraft {
    center: [f32; 2],
    start: [f32; 2],
    angle: f32,
}
fn rotate_pointer(
    document: &mut Document,
    point: lumapaint_core::document::Point,
    phase: u8,
    modifiers: NSEventModifierFlags,
) -> Result<(), String> {
    if phase == 0 {
        cancel_vector_drag();
        if document.selected_vector_ids().is_empty() {
            document.select_vector_at(point, object_selection_tolerance(), false);
        }
        if let Some([x, y, right, bottom]) = document.selected_vector_bounds() {
            let center = [(x + right) * 0.5, (y + bottom) * 0.5];
            if (point.x - center[0]).hypot(point.y - center[1]) >= 1. {
                ROTATE_DRAFT.with(|d| {
                    *d.borrow_mut() = Some(RotateDraft {
                        center,
                        start: [point.x, point.y],
                        angle: 0.,
                    })
                });
            }
        }
    } else {
        ROTATE_DRAFT.with(|d| {
            if let Some(d) = d.borrow_mut().as_mut() {
                if (point.x - d.center[0]).hypot(point.y - d.center[1]) >= 1. {
                    d.angle = (point.y - d.center[1]).atan2(point.x - d.center[0])
                        - (d.start[1] - d.center[1]).atan2(d.start[0] - d.center[0]);
                    if modifiers.contains(NSEventModifierFlags::Shift) {
                        let step = std::f32::consts::PI / 12.;
                        d.angle = (d.angle / step).round() * step;
                    }
                }
            }
        });
        if phase == 2 {
            if let Some(d) = ROTATE_DRAFT.with(|d| d.borrow_mut().take()) {
                document.rotate_selected_vectors(d.center, d.angle)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct BoxDraft {
    corners: [[f32; 2]; 4],
    handle: [f32; 2],
    start: [f32; 2],
    matrix: [f32; 6],
    rotate: bool,
}
fn box_tolerance() -> f32 {
    CANVAS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|c| {
                let v = c.viewport;
                7. / (((v.width as f32 / v.scale - 48.) / v.document_width)
                    .min((v.height as f32 / v.scale - 48.) / v.document_height)
                    .max(0.01)
                    * v.zoom)
            })
            .unwrap_or(6.)
    })
}
// Ten screen points outside the stroke, independent of canvas zoom.
fn object_selection_tolerance() -> f32 {
    (box_tolerance() * 10. / 7.).min(256.)
}
fn box_hit(document: &Document, p: [f32; 2]) -> Option<BoxDraft> {
    box_hit_corners(document.selected_vector_box()?, p)
}
fn box_hit_corners(corners: [[f32; 2]; 4], p: [f32; 2]) -> Option<BoxDraft> {
    let tolerance = box_tolerance();
    for rotate in [false, true] {
        for h in [
            [0., 0.],
            [0.5, 0.],
            [1., 0.],
            [1., 0.5],
            [1., 1.],
            [0.5, 1.],
            [0., 1.],
            [0., 0.5],
        ] {
            if rotate && (h[0] == 0.5 || h[1] == 0.5) {
                continue;
            }
            let q = [
                corners[0][0]
                    + h[0] * (corners[1][0] - corners[0][0])
                    + h[1] * (corners[3][0] - corners[0][0]),
                corners[0][1]
                    + h[0] * (corners[1][1] - corners[0][1])
                    + h[1] * (corners[3][1] - corners[0][1]),
            ];
            let distance = (p[0] - q[0]).hypot(p[1] - q[1]);
            let center = [
                (corners[0][0] + corners[2][0]) * 0.5,
                (corners[0][1] + corners[2][1]) * 0.5,
            ];
            let outside =
                (p[0] - q[0]) * (q[0] - center[0]) + (p[1] - q[1]) * (q[1] - center[1]) > 0.;
            if (!rotate && distance <= tolerance)
                || (rotate && outside && distance <= tolerance * 2.5)
            {
                return Some(BoxDraft {
                    corners,
                    handle: h,
                    start: p,
                    matrix: [1., 0., 0., 1., 0., 0.],
                    rotate,
                });
            }
        }
    }
    None
}
fn update_box(d: &mut BoxDraft, p: [f32; 2], flags: NSEventModifierFlags) {
    if p == d.start {
        d.matrix = [1., 0., 0., 1., 0., 0.];
        return;
    }
    let c = d.corners;
    if d.rotate {
        let center = [(c[0][0] + c[2][0]) * 0.5, (c[0][1] + c[2][1]) * 0.5];
        let mut angle = (p[1] - center[1]).atan2(p[0] - center[0])
            - (d.start[1] - center[1]).atan2(d.start[0] - center[0]);
        if flags.contains(NSEventModifierFlags::Shift) {
            let step = std::f32::consts::FRAC_PI_4;
            angle = (angle / step).round() * step;
        }
        let (b, a) = angle.sin_cos();
        d.matrix = [
            a,
            b,
            -b,
            a,
            center[0] - a * center[0] + b * center[1],
            center[1] - b * center[0] - a * center[1],
        ];
        return;
    }
    let u = [c[1][0] - c[0][0], c[1][1] - c[0][1]];
    let v = [c[3][0] - c[0][0], c[3][1] - c[0][1]];
    let det = u[0] * v[1] - u[1] * v[0];
    if det.abs() < 0.000001 {
        return;
    }
    let delta = [p[0] - d.start[0], p[1] - d.start[1]];
    let local = [
        (v[1] * delta[0] - v[0] * delta[1]) / det,
        (-u[1] * delta[0] + u[0] * delta[1]) / det,
    ];
    let anchor = if flags.contains(NSEventModifierFlags::Option) {
        [0.5, 0.5]
    } else {
        [1. - d.handle[0], 1. - d.handle[1]]
    };
    let mut scale = [1., 1.];
    for i in 0..2 {
        if d.handle[i] != 0.5 {
            scale[i] = (1. + local[i] / (d.handle[i] - anchor[i])).clamp(0.01, 100.);
        }
    }
    if flags.contains(NSEventModifierFlags::Shift) {
        let value = if (scale[0] - 1.).abs() > (scale[1] - 1.).abs() {
            scale[0]
        } else {
            scale[1]
        };
        scale = [value, value];
    }
    let [sx, sy] = scale;
    let a = (u[0] * sx * v[1] - v[0] * sy * u[1]) / det;
    let b = (u[1] * sx * v[1] - v[1] * sy * u[1]) / det;
    let cc = (-u[0] * sx * v[0] + v[0] * sy * u[0]) / det;
    let dd = (-u[1] * sx * v[0] + v[1] * sy * u[0]) / det;
    let origin = [
        c[0][0] + u[0] * anchor[0] + v[0] * anchor[1],
        c[0][1] + u[1] * anchor[0] + v[1] * anchor[1],
    ];
    d.matrix = [
        a,
        b,
        cc,
        dd,
        origin[0] - a * origin[0] - cc * origin[1],
        origin[1] - b * origin[0] - dd * origin[1],
    ];
}

type VectorMarquee = ([f32; 2], [f32; 2], bool);

fn vector_select_pointer(
    document: &mut Document,
    point: lumapaint_core::document::Point,
    phase: u8,
    modifiers: NSEventModifierFlags,
) -> Result<(), String> {
    if phase != 0 && BOX_DRAFT.with(|d| d.borrow().is_some()) {
        BOX_DRAFT.with(|draft| {
            if let Some(d) = draft.borrow_mut().as_mut() {
                update_box(d, [point.x, point.y], modifiers);
            }
        });
        if phase == 2 {
            if let Some(d) = BOX_DRAFT.with(|draft| draft.borrow_mut().take()) {
                document.affine_selected_vectors(d.matrix)?;
            }
        }
        return Ok(());
    }
    if phase == 0 {
        cancel_vector_drag();
        if TOOL.with(|t| t.get()) == CanvasTool::VectorSelect {
            if let Some(d) = box_hit(document, [point.x, point.y]) {
                BOX_DRAFT.with(|draft| *draft.borrow_mut() = Some(d));
                return Ok(());
            }
        }
        if let Some((settings, handle)) = selected_text_resize_handle(document, [point.x, point.y])
            .filter(|_| !modifiers.contains(NSEventModifierFlags::Option))
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
        let previous = document.selected_vector_ids().to_vec();
        let hit = document.vector_at(point, object_selection_tolerance());
        if hit.is_none() {
            VECTOR_MARQUEE.with(|draft| {
                *draft.borrow_mut() = Some((
                    [point.x, point.y],
                    [point.x, point.y],
                    modifiers.contains(NSEventModifierFlags::Shift),
                ))
            });
            return Ok(());
        }
        if !hit.as_ref().is_some_and(|id| previous.contains(id))
            || modifiers.contains(NSEventModifierFlags::Shift)
        {
            document.select_vector_at(
                point,
                object_selection_tolerance(),
                modifiers.contains(NSEventModifierFlags::Shift),
            );
        }
        VECTOR_DUPLICATE.with(|value| {
            value.set(hit.is_some() && modifiers.contains(NSEventModifierFlags::Option))
        });
        VECTOR_DRAFT.with(|draft| {
            let mut draft = draft.borrow_mut();
            draft.clear();
            if hit.is_some() {
                draft.push(point);
            }
        });
    } else if phase == 1 {
        if VECTOR_MARQUEE.with(|draft| {
            if let Some((_, current, _)) = draft.borrow_mut().as_mut() {
                *current = [point.x, point.y];
                true
            } else {
                false
            }
        }) {
            return Ok(());
        }

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
        if let Some((start, _, additive)) = VECTOR_MARQUEE.with(|draft| draft.borrow_mut().take()) {
            return document.select_vectors_in_rect(
                lumapaint_core::document::Point {
                    x: start[0],
                    y: start[1],
                },
                point,
                additive,
            );
        }

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
        let duplicate = VECTOR_DUPLICATE.with(|value| value.replace(false));
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
                        if duplicate {
                            let mut preview = document.clone();
                            preview.duplicate_selected_vectors(offset[0], offset[1])?;
                            canvas.renderer.render(canvas.viewport, &preview)?;
                        } else {
                            canvas.renderer.render_vector_drag(
                                canvas.viewport,
                                document,
                                offset,
                            )?;
                        }
                    }
                    Ok(())
                })?;
                if if duplicate {
                    document.duplicate_selected_vectors(offset[0], offset[1])?
                } else {
                    document.move_selected_vectors(offset[0], offset[1])?
                } {
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
    if TOOL.with(|t| t.get()) == CanvasTool::VectorSelect {
        return document
            .selected_vector_box()
            .map(|corners| lumapaint_renderer::FrameOverlay {
                corners,
                handles: true,
            });
    }

    if document.selected_vector_ids().len() != 1 {
        return None;
    }
    let id = document.selected_vector_ids().first()?;
    let object = document
        .svg_layers()
        .filter(|layer| layer.visible && !layer.locked && layer.vector_layer)
        .flat_map(|layer| &layer.vector_objects)
        .find(|object| &object.id == id && object.visible)?;
    let text = object.text.as_ref()?;
    if object.bounds_reset {
        return document
            .selected_vector_box()
            .map(|corners| lumapaint_renderer::FrameOverlay {
                corners,
                handles: false,
            });
    }
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

// AppKit normally coalesces mouseDragged events while the main thread uploads
// textures or presents a frame. Retaining points after delivery is too late:
// the OS may already have reduced an entire curved gesture to its last event.
fn begin_precise_paint_input() {
    PAINT_MOUSE_COALESCING.with(|previous| {
        if previous.get().is_none() {
            previous.set(Some(NSEvent::isMouseCoalescingEnabled()));
            NSEvent::setMouseCoalescingEnabled(false);
        }
    });
}

fn end_precise_paint_input() {
    PAINT_MOUSE_COALESCING.with(|previous| {
        if let Some(enabled) = previous.take() {
            NSEvent::setMouseCoalescingEnabled(enabled);
        }
    });
}

// Additional pixel layers keep their in-progress stroke in an isolated workspace.
// Trackpad gestures must not change the pointer-to-document transform mid-stroke.
fn paint_gesture_active() -> bool {
    DOCUMENT.with(|doc| doc.borrow().has_active_stroke())
        || PIXEL_PAINT.with(|draft| draft.borrow().is_some())
}

fn cancel_vector_drag() -> bool {
    end_precise_paint_input();
    let painting = PIXEL_PAINT.with(|d| d.borrow_mut().take().is_some());
    let pixels = PIXEL_DRAG.with(|d| d.borrow_mut().take().is_some());
    let bounding = BOX_DRAFT.with(|d| d.borrow_mut().take().is_some());
    let rotating = ROTATE_DRAFT.with(|d| d.borrow_mut().take().is_some());
    let scaling = SCALE_DRAFT.with(|d| d.borrow_mut().take().is_some());
    let marquee = VECTOR_MARQUEE.with(|draft| draft.borrow_mut().take().is_some());
    VECTOR_DUPLICATE.with(|value| value.set(false));
    let moving = VECTOR_MOVE.with(|offset| offset.replace([0.0, 0.0]) != [0.0, 0.0]);
    VECTOR_DRAFT.with(|draft| draft.borrow_mut().clear());
    VECTOR_CONTROL.with(|control| control.borrow_mut().take());
    let pen = PEN_DRAFT.with(|draft| draft.borrow_mut().take().is_some());
    let anchor = ANCHOR_DRAFT.with(|draft| draft.borrow_mut().take().is_some());
    let direct = DIRECT_GESTURE.with(|draft| draft.borrow_mut().take().is_some());
    let text_frame = TEXT_FRAME_DRAFT.with(|draft| draft.borrow_mut().take().is_some());
    let text_resize = TEXT_RESIZE_DRAFT.with(|draft| draft.borrow_mut().take().is_some());
    painting
        || pixels
        || bounding
        || rotating
        || scaling
        || marquee
        || moving
        || pen
        || anchor
        || direct
        || text_frame
        || text_resize
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
    if raster_import::active() {
        return Err("Confirm or cancel image placement / 画像の配置を確定またはキャンセルしてください / 请确认或取消图片放置".into());
    }
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

pub fn set_vector_stroke_style(
    patch: lumapaint_core::stroke::StrokeStylePatch,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().set_selected_stroke_style(patch))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
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
pub fn arrange_selected_vectors(action: String) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    DOCUMENT.with(|doc| doc.borrow_mut().arrange_selected_vectors(&action))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn select_arrange_layer(id: String) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    DOCUMENT.with(|doc| doc.borrow_mut().select_layer_preserving_objects(id))?;
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
            PathEditAction::Join => {
                let controls = if TOOL.with(|tool| tool.get()) == CanvasTool::VectorDirectSelect {
                    DIRECT_POINTS.with(|points| points.borrow().clone())
                } else {
                    Vec::new()
                };
                document.join_path_endpoints(&controls)?;
                DIRECT_POINTS.with(|points| points.borrow_mut().clear());
                Ok(())
            }
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

pub fn pick_raster_file(format: String) -> Result<Option<std::path::PathBuf>, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    let extensions: &[&str] = match format.as_str() {
        "all" => &["jpg", "jpeg", "png"],
        "jpeg" => &["jpg", "jpeg"],
        "png" => &["png"],
        _ => return Err("Unknown image format".into()),
    };
    Ok(rfd::FileDialog::new()
        .add_filter("JPEG / PNG", extensions)
        .pick_file())
}

pub fn apply_raster_import(
    name: String,
    bytes: Vec<u8>,
    info: lumapaint_renderer::vector::ImportedRasterInfo,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    raster_import::begin(name, bytes, info)?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn finish_raster_import(commit: bool) -> Result<DocumentSnapshot, String> {
    raster_import::finish(commit)
}
pub fn raster_import_snapshot() -> Result<DocumentSnapshot, String> {
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
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
    canvas
        .renderer
        .set_outline_view(OUTLINE_VIEW.with(|state| state.get()));
    // Modal edits are rendered once, with their final state, when the view resumes.
    if canvas.view.isHidden() {
        return Ok(());
    }
    canvas.renderer.set_frame_overlay(None);
    if let Some(result) = raster_import::render(canvas) {
        return result;
    }
    if let Some(prepared) = PIXEL_PAINT_COMMIT.with(|pending| pending.borrow_mut().take()) {
        canvas.renderer.install_prepared_svg(prepared)?;
    }
    if let Some(result) = pixel_paint::render(canvas) {
        return result;
    }
    if let Some(result) = pixel_move::render(canvas) {
        return result;
    }
    if let Some(overlay) = pixel_move::overlay() {
        canvas.renderer.set_frame_overlay(Some(overlay));
        return DOCUMENT.with(|d| canvas.renderer.render(canvas.viewport, &d.borrow()));
    }
    if let Some(d) = BOX_DRAFT.with(|draft| *draft.borrow()) {
        let mut preview = DOCUMENT.with(|doc| doc.borrow().clone());
        preview.affine_selected_vectors(d.matrix)?;
        canvas
            .renderer
            .set_frame_overlay(preview.selected_vector_box().map(|corners| {
                lumapaint_renderer::FrameOverlay {
                    corners,
                    handles: true,
                }
            }));
        return canvas.renderer.render(canvas.viewport, &preview);
    }

    if matches!(
        TOOL.with(|t| t.get()),
        CanvasTool::VectorScale | CanvasTool::VectorRotate
    ) {
        let mut preview = DOCUMENT.with(|doc| doc.borrow().clone());
        if let Some(draft) = ROTATE_DRAFT.with(|d| *d.borrow()) {
            preview.rotate_selected_vectors(draft.center, draft.angle)?;
        }
        if let Some(draft) = SCALE_DRAFT.with(|d| *d.borrow()) {
            preview.scale_selected_vectors(draft.center, draft.scale)?;
        }
        if let Some(corners) = preview.selected_vector_box() {
            canvas
                .renderer
                .set_frame_overlay(Some(lumapaint_renderer::FrameOverlay {
                    corners,
                    handles: false,
                }));
        }
        return canvas.renderer.render(canvas.viewport, &preview);
    }
    if let Some((start, end, _)) = VECTOR_MARQUEE.with(|draft| *draft.borrow()) {
        if start != end {
            canvas
                .renderer
                .set_frame_overlay(Some(draft_frame_overlay(start, end)));
        }
        return DOCUMENT.with(|doc| canvas.renderer.render(canvas.viewport, &doc.borrow()));
    }
    if VECTOR_DUPLICATE.with(|value| value.get()) {
        let offset = VECTOR_MOVE.with(|value| value.get());
        if offset != [0., 0.] {
            let mut preview = DOCUMENT.with(|doc| doc.borrow().clone());
            preview.duplicate_selected_vectors(offset[0], offset[1])?;
            canvas
                .renderer
                .set_frame_overlay(selected_text_frame_overlay(&preview));
            return canvas.renderer.render(canvas.viewport, &preview);
        }
    }
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
    let guide_scale = ((canvas.viewport.width as f32 / canvas.viewport.scale - 48.)
        / canvas.viewport.document_width)
        .min(
            (canvas.viewport.height as f32 / canvas.viewport.scale - 48.)
                / canvas.viewport.document_height,
        )
        .max(0.01)
        * canvas.viewport.zoom;
    if matches!(
        TOOL.with(|tool| tool.get()),
        CanvasTool::VectorAnchorAdd
            | CanvasTool::VectorAnchorDelete
            | CanvasTool::VectorAnchorConvert
    ) && ANCHOR_DRAFT.with(|draft| draft.borrow().is_none())
        && ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_none())
    {
        let preview = DOCUMENT.with(|doc| anchor_guides_preview(&doc.borrow(), guide_scale))?;
        return canvas.renderer.render(canvas.viewport, &preview);
    }
    if TOOL.with(|tool| tool.get()) == CanvasTool::VectorDirectSelect
        && ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_none())
    {
        let preview = DOCUMENT.with(|document| direct_preview(&document.borrow(), guide_scale))?;
        return canvas.renderer.render(canvas.viewport, &preview);
    }
    if let Some(draft) = ANCHOR_DRAFT.with(|draft| draft.borrow().clone()) {
        let preview = DOCUMENT.with(|document| draft.preview(&document.borrow(), guide_scale))?;
        return canvas.renderer.render(canvas.viewport, &preview);
    }
    if let Some(draft) = PEN_DRAFT.with(|draft| draft.borrow().clone()) {
        let preview = DOCUMENT.with(|document| draft.preview(&document.borrow(), guide_scale))?;
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
        if document.paint_source().is_some()
            || document.visible_strokes().any(|stroke| stroke.eraser)
        {
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
    static CANVAS_OVERLAY: std::cell::Cell<Option<[f64; 4]>> = const { std::cell::Cell::new(None) };
    static BRUSH_CURSOR: RefCell<Option<Retained<NSCursor>>> = const { RefCell::new(None) };
    static BRUSH_CURSOR_KEY: std::cell::Cell<(u32, bool)> = const { std::cell::Cell::new((0, false)) };
    // This slot is accessed exclusively from Tauri's main-thread callbacks.
    static CANVAS: RefCell<Option<Canvas>> = const { RefCell::new(None) };
    static DOCUMENT: RefCell<Document> = RefCell::new({ let mut document = Document::default(); lumapaint_svg::attach(&mut document); let _ = document.select_layer("layer-1".into()); document });
    static ACTIVE_TILED_DOCUMENT: RefCell<Option<TiledSession>> = const { RefCell::new(None) };
    static BRUSH: RefCell<Brush> = RefCell::new(Brush::default());
    static TOOL: std::cell::Cell<CanvasTool> = const { std::cell::Cell::new(CanvasTool::Brush) };
    static DOCUMENT_OPEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ACTIVE_DOCUMENT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    static NEXT_DOCUMENT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(2) };
    static INACTIVE_DOCUMENTS: RefCell<Vec<OpenDocument>> = const { RefCell::new(Vec::new()) };
    static PAN: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
    static LAST_PAN_POINT: std::cell::Cell<(f32, f32)> = const { std::cell::Cell::new((0.0, 0.0)) };
    static PANNING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static SPACE_DOWN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PAINT_MOUSE_COALESCING: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    static PIXEL_PAINT_COMMIT: RefCell<Option<PreparedSvgLayer>> = const { RefCell::new(None) };
    static PIXEL_PAINT: RefCell<Option<pixel_paint::PixelPaint>> = const { RefCell::new(None) };
    static PIXEL_DRAG: RefCell<Option<pixel_move::PixelDrag>> = const { RefCell::new(None) };
    static BOX_DRAFT: RefCell<Option<BoxDraft>> = const { RefCell::new(None) };
    static ROTATE_DRAFT: RefCell<Option<RotateDraft>> = const { RefCell::new(None) };
    static SCALE_DRAFT: RefCell<Option<ScaleDraft>> = const { RefCell::new(None) };
    static VECTOR_MARQUEE: RefCell<Option<VectorMarquee>> = const { RefCell::new(None) };
    static VECTOR_DUPLICATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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
                guide_color: [48, 144, 255, 255],
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
    raster_import::cancel();
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

fn sync_viewport_document(
    viewport: &mut Viewport,
    width: u32,
    height: u32,
    canvas_color: CanvasColor,
) {
    viewport.document_width = width as f32;
    viewport.document_height = height as f32;
    viewport.canvas_color = canvas_color;
}

fn activate_document(entry: OpenDocument) {
    cancel_vector_drag();
    let (width, height, canvas_color) = match &entry.content {
        OpenDocumentContent::Legacy(document) => {
            let snapshot = document.snapshot();
            (snapshot.width, snapshot.height, snapshot.canvas_color)
        }
        OpenDocumentContent::Tiled(document) => {
            let (width, height) = document.document.dimensions();
            (width, height, CanvasColor::White)
        }
    };
    match entry.content {
        OpenDocumentContent::Legacy(mut document) => {
            lumapaint_svg::attach(&mut document);
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
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            sync_viewport_document(&mut canvas.viewport, width, height, canvas_color);
        }
    });
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
            view.setClipsToBounds(true);
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
        update_canvas_mask(&canvas.view, request.overlay);
        canvas.viewport = viewport;
        canvas.renderer.channel = request.channel;
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

// Punch a hole only in the native view's composition, leaving the GPU viewport fixed.
// The WebView below receives pointer events in this same rectangle.
fn update_canvas_mask(view: &PaintView, overlay: Option<[f64; 4]>) {
    CANVAS_OVERLAY.with(|value| value.set(overlay));
    // SAFETY: all Core Animation objects are retained and accessed on the main thread.
    unsafe {
        let layer: Option<Retained<AnyObject>> = msg_send![view, layer];
        let Some(layer) = layer else { return };
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
        if let Some(r) = overlay {
            let size = view.bounds().size;
            let left = r[0].clamp(0.0, size.width);
            let top = r[1].clamp(0.0, size.height);
            let right = r[2].clamp(left, size.width);
            let bottom = r[3].clamp(top, size.height);
            let mask: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let _: () = msg_send![&*mask, setFrame: view.bounds()];
            let white = NSColor::whiteColor();
            let color = white.CGColor();
            for (x, y, width, height) in [
                (0.0, 0.0, size.width, top),
                (0.0, bottom, size.width, size.height - bottom),
                (0.0, top, left, bottom - top),
                (right, top, size.width - right, bottom - top),
            ] {
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                let part: Retained<AnyObject> = msg_send![class!(CALayer), layer];
                let _: () = msg_send![&*part, setFrame: NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))];
                let _: () = msg_send![&*part, setBackgroundColor: &*color];
                let _: () = msg_send![&*mask, addSublayer: &*part];
            }
            let _: () = msg_send![&*layer, setMask: &*mask];
        } else {
            let _: () = msg_send![&*layer, setMask: std::ptr::null::<AnyObject>()];
        }
        let _: () = msg_send![class!(CATransaction), commit];
    }
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
    fn activating_a_different_document_size_updates_the_viewport_aspect_ratio() {
        let mut viewport = Viewport::new(1200., 800., 2., 1., true).unwrap();
        sync_viewport_document(&mut viewport, 1600, 900, CanvasColor::White);
        assert_eq!(
            (viewport.document_width, viewport.document_height),
            (1600., 900.)
        );

        sync_viewport_document(&mut viewport, 900, 1600, CanvasColor::Transparent);
        assert_eq!(
            (viewport.document_width, viewport.document_height),
            (900., 1600.)
        );
        assert_eq!(viewport.canvas_color, CanvasColor::Transparent);
    }

    #[test]
    fn opening_png_and_jpeg_creates_matching_unsaved_documents() {
        let (width, height) = (37u32, 23u32);
        let pixels = [24, 120, 210, 255]
            .into_iter()
            .cycle()
            .take((width * height * 4) as usize)
            .collect();
        let png = lumapaint_renderer::vector::document_png(width, height, pixels).unwrap();
        let info = lumapaint_renderer::vector::imported_raster_info(&png, "png").unwrap();
        for (bytes, name, mime, rasterizes) in [
            (png, "sample.png", "image/png", true),
            // JPEG decoding/header validation is covered in lumapaint-renderer;
            // this verifies the opened document's JPEG MIME and persistence path.
            (
                vec![0xff, 0xd8, 0xff, 0xd9],
                "sample.jpeg",
                "image/jpeg",
                false,
            ),
        ] {
            let mut document = opened_raster_document(name.into(), bytes, info).unwrap();
            let snapshot = document.snapshot();
            assert_eq!((snapshot.width, snapshot.height), (37, 23));
            assert_eq!(snapshot.name, name);
            assert!(snapshot.dirty);
            assert!(snapshot.file_name.is_none());
            let layer = document.svg_layers().last().unwrap();
            assert_eq!(layer.name, name);
            assert!(layer.paint_layer && !layer.vector_layer);
            assert!(layer.source.contains(mime));
            assert!(layer.source.contains("width=\"37\" height=\"23\""));
            if rasterizes {
                let raster =
                    lumapaint_renderer::vector::rasterize_svg(&layer.source, 37, 23).unwrap();
                assert_eq!(raster.pixels.len(), 37 * 23 * 4);
                assert!(raster.pixels[3] > 0);
                assert!(raster.pixels[raster.pixels.len() - 1] > 0);
            }
            let saved = document.encode().unwrap();
            let reopened = Document::decode(&saved).unwrap().snapshot();
            assert_eq!((reopened.width, reopened.height), (37, 23));
        }
    }

    #[test]
    fn pen_closure_uses_screen_distance_at_all_zoom_and_backing_scales() {
        let draft = PenDraft {
            layer: "path".into(),
            brush: Brush::default(),
            nodes: vec![([100., 100.], [100., 100.]), ([300., 200.], [300., 200.])],
        };
        for backing in [1., 2.] {
            let mut v = Viewport::new(848., 648., backing, 1., false).unwrap();
            v.document_width = 1600.;
            v.document_height = 1200.;
            for zoom in [0.25, 1., 4., 16.] {
                v.zoom = zoom;
                let tolerance = pen_close_tolerance(Some(v));
                assert!((tolerance - 16. / zoom).abs() < 0.001);
                assert!(pen_should_close(
                    &draft,
                    [100. + tolerance * 0.9, 100.],
                    tolerance
                ));
                assert!(!pen_should_close(
                    &draft,
                    [100. + tolerance * 1.1, 100.],
                    tolerance
                ));
            }
            v.zoom = 0.5;
            assert!(pen_should_close(
                &draft,
                [110., 100.],
                pen_close_tolerance(Some(v))
            ));
            v.zoom = 4.;
            assert!(!pen_should_close(
                &draft,
                [110., 100.],
                pen_close_tolerance(Some(v))
            ));
        }
        let single = PenDraft {
            nodes: vec![draft.nodes[0]],
            ..draft
        };
        assert!(!pen_should_close(&single, [100., 100.], 8.));
    }

    #[test]
    fn pen_guides_keep_centerline_and_screen_width_without_changing_artwork() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let draft = PenDraft {
            layer,
            brush: Brush {
                size: 24.,
                ..Brush::default()
            },
            nodes: vec![([20., 20.], [50., 0.]), ([200., 150.], [220., 180.])],
        };
        let original = doc.encode().unwrap();
        for scale in [0.125, 0.5, 1., 4., 16.] {
            let preview = draft.preview(&doc, scale).unwrap();
            let objects: Vec<_> = preview
                .svg_layers()
                .flat_map(|l| &l.vector_objects)
                .collect();
            let artwork = objects.iter().find(|o| o.id == "pen-draft").unwrap();
            let guide = objects.iter().find(|o| o.id == "pen-guides").unwrap();
            assert!(guide.path.data.starts_with(&artwork.path.data));
            assert!(guide.path.data.contains(" a ") && guide.path.data.contains(" h "));
            assert_eq!(guide.stroke_width * scale, 1.);
            assert_eq!(artwork.stroke_width, 24.);
            assert_eq!(doc.encode().unwrap(), original);
        }
    }

    #[test]
    fn pen_commit_snaps_existing_near_endpoints_without_a_tiny_closing_segment() {
        let draft = PenDraft {
            layer: "path".into(),
            brush: Brush::default(),
            nodes: vec![
                ([10., 10.], [20., 0.]),
                ([100., 100.], [120., 80.]),
                ([11., 10.5], [16., 12.5]),
            ],
        };
        for closed in [false, true] {
            let result = draft.committed_object(closed, "snap", 2.).unwrap();
            assert!(result.path.data.ends_with('Z'));
            assert_eq!(result.control_points.len(), 7);
            assert_eq!(result.control_points[0], result.control_points[6]);
            assert_eq!(result.control_points[1], [20., 0.]);
            assert_eq!(result.control_points[5], [5., 8.]);
        }
        let separate = draft.committed_object(false, "open", 0.5).unwrap();
        assert!(!separate.path.data.ends_with('Z'));
        assert_eq!(separate.control_points[6], [11., 10.5]);
        assert_eq!(draft.nodes[2].0, [11., 10.5]);
    }

    #[test]
    fn pen_near_start_commits_closed_path_without_extra_anchor() {
        PEN_DRAFT.with(|slot| slot.borrow_mut().take());
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        doc.select_layer(layer).unwrap();
        for (x, y) in [(100., 100.), (200., 100.), (200., 200.), (104., 102.)] {
            bezier_pointer(&mut doc, lumapaint_core::document::Point { x, y }, 0).unwrap();
            bezier_pointer(&mut doc, lumapaint_core::document::Point { x, y }, 2).unwrap();
        }
        assert!(PEN_DRAFT.with(|slot| slot.borrow().is_none()));
        let object = &doc.svg_layers().next().unwrap().vector_objects[0];
        assert!(object.path.data.ends_with('Z'));
        assert_eq!(object.control_points.len(), 10);
        assert_eq!(object.control_points.first(), object.control_points.last());
        doc.undo();
        assert!(doc.svg_layers().next().unwrap().vector_objects.is_empty());
    }

    #[test]
    fn saved_path_direct_selection_is_scoped_and_guides_do_not_change_clip_geometry() {
        DIRECT_POINTS.with(|p| p.borrow_mut().clear());
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let draft = PenDraft {
            layer: layer.clone(),
            brush: Brush::default(),
            nodes: vec![([20., 20.], [40., 0.]), ([100., 80.], [120., 60.])],
        };
        doc.upsert_vector_object(&layer, draft.object(false, "artwork").unwrap())
            .unwrap();
        doc.saved_path_action("new", None, "Path").unwrap();
        assert!(direct_objects(&doc).is_empty());
        let target = doc.selected_vector_target().unwrap().unwrap();
        doc.upsert_vector_object(&target, draft.object(false, "path-only").unwrap())
            .unwrap();
        assert_eq!(direct_objects(&doc).len(), 1);
        DIRECT_POINTS.with(|p| *p.borrow_mut() = vec![("path-only".into(), 0)]);
        direct_select_ids(&mut doc).unwrap();
        assert!(doc.has_active_saved_path());
        assert_eq!(doc.selected_vector_ids(), &["path-only"]);
        let mut preview = anchor_guides_preview(&doc, 1.).unwrap();
        assert_eq!(preview.snapshot().saved_paths[0].components, 1);
        let reloaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(reloaded.snapshot().saved_paths[0].components, 1);
        doc.saved_path_action("deactivate", None, "").unwrap();
        assert_eq!(direct_objects(&doc)[0].1.id, "artwork");
        // A native editing guide belongs only to a preview clone.
        assert_ne!(preview.encode().unwrap(), doc.encode().unwrap());
        DIRECT_POINTS.with(|p| p.borrow_mut().clear());
    }

    #[test]
    fn direct_guides_show_curve_anchors_handles_without_modifying_document() {
        DIRECT_POINTS.with(|p| p.borrow_mut().clear());
        DIRECT_GESTURE.with(|d| d.borrow_mut().take());
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let draft = PenDraft {
            layer: layer.clone(),
            brush: Brush::default(),
            nodes: vec![([20., 20.], [40., 0.]), ([100., 80.], [120., 60.])],
        };
        let object = draft.object(false, "guide-test").unwrap();
        doc.upsert_vector_object(&layer, object).unwrap();
        doc.select_vector_objects(vec!["guide-test".into()])
            .unwrap();
        let before = doc.encode().unwrap();
        let revision = doc.revision();
        for scale in [0.25, 1., 4.] {
            let preview = direct_preview(&doc, scale).unwrap();
            let guide = preview
                .svg_layers()
                .flat_map(|l| &l.vector_objects)
                .find(|o| o.id == "guides-guide-test")
                .unwrap();
            assert!(guide.path.data.contains(" C "));
            assert!(guide.path.data.contains(" a "));
            assert!(guide.path.data.contains(" h "));
            assert_eq!(guide.stroke_width, 1. / scale);
        }
        let idle = anchor_guides_preview(&doc, 2.).unwrap();
        let guide = idle
            .svg_layers()
            .flat_map(|l| &l.vector_objects)
            .find(|o| o.id == "guides-guide-test")
            .unwrap();
        assert!(guide.path.data.contains(" C ") && guide.path.data.contains(" a "));
        assert_eq!(guide.stroke_width, 0.5);
        assert_eq!(doc.encode().unwrap(), before);
        assert_eq!(doc.revision(), revision);
    }

    #[test]
    fn trackpad_pan_uses_both_axes_in_view_points_and_limits_offsets() {
        assert_eq!(pan_offset((10., 20.), (12.5, -8.)), (22.5, 12.));
        assert_eq!(pan_offset((10., 20.), (-12.5, 8.)), (-2.5, 28.));
        assert_eq!(pan_offset((8190., -8190.), (10., -10.)), (8192., -8192.));
        assert_eq!(pan_offset((10., 20.), (f32::NAN, 1.)), (10., 20.));
        let first = pan_offset((0., 0.), (8., 4.));
        assert_eq!(pan_offset(first, (4., 2.)), (12., 6.));
    }

    #[test]
    fn pinch_zoom_in_out_limits_and_invalid_events() {
        assert!((pinch_zoom(1., 0.2) - 1.2).abs() < 0.0001);
        assert!((pinch_zoom(1., -0.2) - 0.8).abs() < 0.0001);
        assert_eq!(pinch_zoom(4., 1.), 4.);
        assert_eq!(pinch_zoom(0.25, -0.9), 0.25);
        assert_eq!(pinch_zoom(1., f32::NAN), 1.);
        assert_eq!(pinch_zoom(1., 0.), 1.);
    }

    #[test]
    fn bounding_handles_transform_preview_commit_undo_and_cancel() {
        TOOL.with(|t| t.set(CanvasTool::VectorSelect));
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: lumapaint_core::vector::VectorText {
                content: "Box".into(),
                box_width: 100.,
                box_height: Some(50.),
                ..Default::default()
            },
            position: [100., 100.],
            color: [0, 0, 0],
        })
        .unwrap();
        let original = doc.selected_vector_box().unwrap();
        let start = original[2];
        let end = [start[0] + 100., start[1] + 50.];
        let point = |p: [f32; 2]| lumapaint_core::document::Point { x: p[0], y: p[1] };
        let flags = NSEventModifierFlags::empty();
        let revision = doc.revision();
        vector_select_pointer(&mut doc, point(start), 0, flags).unwrap();
        assert!(BOX_DRAFT.with(|d| d.borrow().is_some()));
        vector_select_pointer(&mut doc, point(end), 1, flags).unwrap();
        assert_eq!(doc.revision(), revision);
        vector_select_pointer(&mut doc, point(end), 2, flags).unwrap();
        assert_eq!(doc.revision(), revision + 1);
        let changed = doc.selected_vector_box().unwrap();
        assert!((changed[0][0] - original[0][0]).abs() < 0.001);
        assert!((changed[2][0] - end[0]).abs() < 0.001);
        doc.undo();
        assert_eq!(doc.selected_vector_box().unwrap(), original);
        doc.redo();
        assert_eq!(doc.selected_vector_box().unwrap(), changed);
        vector_select_pointer(&mut doc, point(changed[2]), 0, flags).unwrap();
        cancel_vector_drag();
        assert!(BOX_DRAFT.with(|d| d.borrow().is_none()));
        TOOL.with(|t| t.set(CanvasTool::Brush));
    }
    #[test]
    fn bounding_math_keeps_rotated_anchor_and_supports_center_and_rotation() {
        let corners = [[10., 20.], [10., 120.], [-40., 120.], [-40., 20.]];
        let mut d = BoxDraft {
            corners,
            handle: [1., 1.],
            start: corners[2],
            matrix: [1., 0., 0., 1., 0., 0.],
            rotate: false,
        };
        update_box(&mut d, [-90., 220.], NSEventModifierFlags::empty());
        let apply = |m: [f32; 6], p: [f32; 2]| {
            [
                m[0] * p[0] + m[2] * p[1] + m[4],
                m[1] * p[0] + m[3] * p[1] + m[5],
            ]
        };
        assert_eq!(apply(d.matrix, corners[0]), corners[0]);
        assert_eq!(apply(d.matrix, corners[2]), [-90., 220.]);
        update_box(
            &mut d,
            [-90., 220.],
            NSEventModifierFlags::Option | NSEventModifierFlags::Shift,
        );
        assert_eq!(apply(d.matrix, [-15., 70.]), [-15., 70.]);
        d.rotate = true;
        d.start = [35., 70.];
        update_box(&mut d, [-15., 120.], NSEventModifierFlags::Shift);
        let p = apply(d.matrix, [35., 70.]);
        assert!((p[0] + 15.).abs() < 0.001 && (p[1] - 120.).abs() < 0.001);
    }

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
    fn imported_shapes_use_direct_selection_instead_of_image_layer_movement() {
        use lumapaint_core::document::Point;
        DIRECT_POINTS.with(|points| points.borrow_mut().clear());
        DIRECT_GESTURE.with(|draft| draft.borrow_mut().take());
        let project = serde_json::json!({
            "format": "LumaPaint", "version": 1, "width": 640, "height": 400,
            "layerVisible": true, "strokes": [],
            "svgLayers": [{ "id": "svg-layer-1", "name": "SVG", "visible": true,
                "source": include_str!("../../tests/fixtures/direct-svg-shapes.svg") }]
        });
        let mut document = Document::decode(&serde_json::to_vec(&project).unwrap()).unwrap();
        lumapaint_svg::attach(&mut document);
        assert!(!selection_uses_pixel_move(
            &document,
            CanvasTool::VectorDirectSelect
        ));
        assert!(selection_uses_pixel_move(
            &document,
            CanvasTool::VectorSelect
        ));
        assert!(selection_uses_pixel_move(
            &Document::default(),
            CanvasTool::VectorDirectSelect
        ));
        let before = document.direct_objects();
        let start = Point { x: 69., y: 220. };
        direct_pointer(&mut document, start, 0, NSEventModifierFlags::empty()).unwrap();
        assert_eq!(DIRECT_POINTS.with(|p| p.borrow().len()), 1);
        direct_pointer(
            &mut document,
            Point { x: 79., y: 230. },
            2,
            NSEventModifierFlags::empty(),
        )
        .unwrap();
        let after = document.direct_objects();
        assert_eq!(before.len(), after.len());
        assert_ne!(before[3].1.control_points, after[3].1.control_points);
        assert_eq!(before[5].1.control_points, after[5].1.control_points);
        assert_eq!(before[0].1.control_points, after[0].1.control_points);
        document.undo();
        assert_eq!(
            document.direct_objects()[3].1.control_points,
            before[3].1.control_points
        );
        DIRECT_POINTS.with(|points| points.borrow_mut().clear());
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
        assert!(
            document.selected_layer_is_vector(),
            "shared tools must route a newly created path to vector editing"
        );
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
    fn option_drag_copies_text_once_and_cancel_keeps_original() {
        use lumapaint_core::{document::Point, vector::VectorText};
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Copy".into(),
                ..Default::default()
            },
            position: [100., 100.],
            color: [0, 0, 0],
        })
        .unwrap();
        let flags = NSEventModifierFlags::Option;
        vector_select_pointer(&mut doc, Point { x: 110., y: 110. }, 0, flags).unwrap();
        vector_select_pointer(&mut doc, Point { x: 150., y: 130. }, 1, flags).unwrap();
        assert_eq!(doc.snapshot().text_objects.len(), 1);
        assert!(VECTOR_DUPLICATE.with(|v| v.get()));
        vector_select_pointer(&mut doc, Point { x: 150., y: 130. }, 2, flags).unwrap();
        let objects = doc.snapshot().text_objects;
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0].position, [100., 100.]);
        assert_eq!(objects[1].position, [140., 120.]);
        doc.undo();
        assert_eq!(doc.snapshot().text_objects.len(), 1);
        doc.redo();
        assert_eq!(doc.snapshot().text_objects.len(), 2);
        doc.undo();
        vector_select_pointer(&mut doc, Point { x: 110., y: 110. }, 0, flags).unwrap();
        vector_select_pointer(&mut doc, Point { x: 180., y: 180. }, 1, flags).unwrap();
        cancel_vector_drag();
        vector_select_pointer(&mut doc, Point { x: 180., y: 180. }, 2, flags).unwrap();
        assert_eq!(doc.snapshot().text_objects.len(), 1);
        assert!(!VECTOR_DUPLICATE.with(|v| v.get()));
    }

    #[test]
    fn vector_marquee_selects_multiple_adds_and_cancels() {
        use lumapaint_core::{document::Point, vector::VectorText};
        let mut doc = Document::default();
        for x in [100., 500.] {
            doc.set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: "Box".into(),
                    box_width: 100.,
                    ..Default::default()
                },
                position: [x, 100.],
                color: [0, 0, 0],
            })
            .unwrap();
        }
        let original = doc.selected_vector_ids().to_vec();
        let revision = doc.revision();
        let flags = NSEventModifierFlags::empty();
        let start = Point { x: 10., y: 10. };
        let end = Point { x: 900., y: 600. };
        vector_select_pointer(&mut doc, start, 0, flags).unwrap();
        vector_select_pointer(&mut doc, end, 1, flags).unwrap();
        assert!(VECTOR_MARQUEE.with(|d| d.borrow().is_some()));
        assert_eq!(doc.selected_vector_ids(), original);
        vector_select_pointer(&mut doc, end, 2, flags).unwrap();
        assert_eq!(doc.selected_vector_ids().len(), 2);
        assert_eq!(doc.revision(), revision);
        let all = doc.selected_vector_ids().to_vec();
        vector_select_pointer(&mut doc, start, 0, flags).unwrap();
        vector_select_pointer(&mut doc, Point { x: 20., y: 20. }, 1, flags).unwrap();
        assert!(cancel_vector_drag());
        vector_select_pointer(&mut doc, Point { x: 20., y: 20. }, 2, flags).unwrap();
        assert_eq!(doc.selected_vector_ids(), all);
        // Reverse-direction marquee and Shift addition use the same core geometry.
        doc.select_vector_objects(original.clone()).unwrap();
        vector_select_pointer(&mut doc, end, 0, NSEventModifierFlags::Shift).unwrap();
        vector_select_pointer(&mut doc, start, 2, NSEventModifierFlags::Shift).unwrap();
        assert_eq!(doc.selected_vector_ids().len(), 2);
        vector_select_pointer(&mut doc, start, 0, flags).unwrap();
        vector_select_pointer(&mut doc, start, 2, flags).unwrap();
        assert!(doc.selected_vector_ids().is_empty());
    }

    #[test]
    fn scale_drag_previews_then_commits_once_and_cancels() {
        use lumapaint_core::{document::Point, vector::VectorText};
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Scale".into(),
                box_width: 100.,
                ..Default::default()
            },
            position: [100., 100.],
            color: [0, 0, 0],
        })
        .unwrap();
        let [x, y, right, bottom] = doc.selected_vector_bounds().unwrap();
        let center = [(x + right) * 0.5, (y + bottom) * 0.5];
        let start = Point {
            x: right,
            y: center[1],
        };
        let end = Point {
            x: center[0] + 2. * (right - center[0]),
            y: center[1],
        };
        let revision = doc.revision();
        scale_pointer(&mut doc, start, 0).unwrap();
        scale_pointer(&mut doc, end, 1).unwrap();
        assert_eq!(doc.revision(), revision);
        assert_eq!(SCALE_DRAFT.with(|d| d.borrow().unwrap().scale), 2.);
        scale_pointer(&mut doc, end, 2).unwrap();
        let scaled = doc.selected_vector_bounds().unwrap();
        assert!(((scaled[2] - scaled[0]) / (right - x) - 2.).abs() < 0.001);
        assert_eq!(doc.revision(), revision + 1);
        doc.undo();
        assert_eq!(doc.selected_vector_bounds().unwrap(), [x, y, right, bottom]);
        doc.redo();
        assert_eq!(doc.selected_vector_bounds().unwrap(), scaled);
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.svg_layers().next().unwrap().vector_objects[0].transform,
            doc.svg_layers().next().unwrap().vector_objects[0].transform
        );
        scale_pointer(&mut doc, start, 0).unwrap();
        scale_pointer(&mut doc, end, 1).unwrap();
        cancel_vector_drag();
        scale_pointer(&mut doc, end, 2).unwrap();
        assert_eq!(doc.selected_vector_bounds().unwrap(), scaled);
    }

    #[test]
    fn rotate_drag_preserves_center_and_undo_cancel() {
        use lumapaint_core::{document::Point, vector::VectorText};
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Rotate".into(),
                box_width: 100.,
                ..Default::default()
            },
            position: [100., 100.],
            color: [0, 0, 0],
        })
        .unwrap();
        let [x, y, r, b] = doc.selected_vector_bounds().unwrap();
        let c = [(x + r) * 0.5, (y + b) * 0.5];
        let original = doc.svg_layers().next().unwrap().vector_objects[0].transform;
        let revision = doc.revision();
        let start = Point {
            x: c[0] + 100.,
            y: c[1],
        };
        let end = Point {
            x: c[0],
            y: c[1] + 100.,
        };
        rotate_pointer(&mut doc, start, 0, NSEventModifierFlags::Shift).unwrap();
        rotate_pointer(&mut doc, end, 1, NSEventModifierFlags::Shift).unwrap();
        assert_eq!(doc.revision(), revision);
        rotate_pointer(&mut doc, end, 2, NSEventModifierFlags::Shift).unwrap();
        let [nx, ny, nr, nb] = doc.selected_vector_bounds().unwrap();
        assert!(((nr - nx) - (b - y)).abs() < 0.001);
        assert!(((nb - ny) - (r - x)).abs() < 0.001);
        assert!(((nx + nr) * 0.5 - c[0]).abs() < 0.001);
        let rotated = doc.svg_layers().next().unwrap().vector_objects[0].transform;
        doc.undo();
        assert_eq!(
            doc.svg_layers().next().unwrap().vector_objects[0].transform,
            original
        );
        doc.redo();
        assert_eq!(
            doc.svg_layers().next().unwrap().vector_objects[0].transform,
            rotated
        );
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.svg_layers().next().unwrap().vector_objects[0].transform,
            rotated
        );
        rotate_pointer(&mut doc, start, 0, NSEventModifierFlags::empty()).unwrap();
        rotate_pointer(&mut doc, end, 1, NSEventModifierFlags::empty()).unwrap();
        cancel_vector_drag();
        rotate_pointer(&mut doc, end, 2, NSEventModifierFlags::empty()).unwrap();
        assert_eq!(
            doc.svg_layers().next().unwrap().vector_objects[0].transform,
            rotated
        );
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
            overlay: None,
            channel: 0,
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
            overlay: None,
            channel: 0,
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
    if raster_import::active() {
        return Err("Confirm or cancel image placement / 画像の配置を確定またはキャンセルしてください / 请确认或取消图片放置".into());
    }
    finish_open_pen()?;
    cancel_vector_drag();
    text_editor::finish(true)?;
    (DOCUMENT_OPEN.with(|open| open.get())
        && ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_none()))
    .then_some(())
    .ok_or_else(|| "This document is read-only in the current workspace".into())
}

pub fn new_document(
    settings: lumapaint_core::document::NewDocumentSettings,
) -> Result<DocumentWorkspaceSnapshot, String> {
    let document = Document::from_preset(settings)?;
    text_editor::finish(true)?;
    park_active_document();
    activate_document(OpenDocument {
        id: next_document_id(),
        content: OpenDocumentContent::Legacy(Box::new(document)),
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
                .add_filter(
                    "LumaPaint / JPEG / PNG",
                    &["lumapaint", "jpg", "jpeg", "png"],
                )
                .pick_file()
            else {
                return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
            };
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(extension.as_str(), "jpg" | "jpeg" | "png") {
                let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
                if metadata.len() > 3 * 1024 * 1024 - 1024 {
                    return Err("Image exceeds the current 3 MiB import limit / 現在の読み込み上限は約3MiBです / 当前导入上限约为3MiB".into());
                }
                let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
                let format = if extension == "png" { "png" } else { "jpeg" };
                let info = lumapaint_renderer::vector::imported_raster_info(&bytes, format)?;
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let document = opened_raster_document(name, bytes, info)?;
                park_active_document();
                activate_document(OpenDocument {
                    id: next_document_id(),
                    content: OpenDocumentContent::Legacy(Box::new(document)),
                    // Opening an image creates a new unsaved LumaPaint document.
                    path: None,
                    fingerprint: None,
                });
                if let Err(error) = redraw() {
                    emit_error(error);
                }
                emit_document();
                return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
            }
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

fn opened_raster_document(
    name: String,
    bytes: Vec<u8>,
    info: lumapaint_renderer::vector::ImportedRasterInfo,
) -> Result<Document, String> {
    use base64::Engine;
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let (width, height) = (info.width, info.height);
    let (raw_width, raw_height) = (info.encoded_width, info.encoded_height);
    let transform = info
        .image_transform()
        .map(|m| {
            format!(
                " transform=\"matrix({} {} {} {} {} {})\"",
                m[0], m[1], m[2], m[3], m[4], m[5]
            )
        })
        .unwrap_or_default();
    let source = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><image width=\"{raw_width}\" height=\"{raw_height}\"{transform} href=\"data:{mime};base64,{data}\"/></svg>"
    );
    let mut document = Document::from_preset(NewDocumentSettings {
        document: DocumentSettings {
            name: name.clone(),
            width,
            height,
            unit: DocumentUnit::Pixels,
            resolution: 72,
            artboards: false,
            canvas_color: CanvasColor::Transparent,
            pixel_aspect_ratio: 1.0,
        },
        color_mode: ColorMode::Rgb,
        color_profile: ColorProfile::Srgb,
        bit_depth: 8,
    })?;
    document.import_raster_layer(name, source)?;
    Ok(document)
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

pub fn clipping_path(action: &str) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().clipping_path(action))?;
    if action == "edit" {
        DIRECT_POINTS.with(|points| points.borrow_mut().clear());
        TOOL.with(|tool| tool.set(CanvasTool::VectorDirectSelect));
    }
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn compound_path(release: bool) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        doc.borrow_mut().compound_path(release, |objects| {
            let engine = lumapaint_renderer::vector::skia_paths::SkiaPathEngine;
            if release {
                engine.split_compound(&objects[0])
            } else {
                engine.compound_objects(objects).map(|path| vec![path])
            }
        })
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn transform_objects(action: &str, values: [f32; 4]) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    cancel_vector_drag();
    DOCUMENT.with(|d| d.borrow_mut().transform_selected_vectors(action, values))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

pub fn set_vector_appearance(
    ids: &[String],
    opacity: Option<f32>,
    blend_mode: Option<String>,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|d| {
        d.borrow_mut()
            .set_selected_vector_appearance(ids, opacity, blend_mode)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

pub fn set_vector_paint(
    ids: &[String],
    target: &str,
    color: Option<[u8; 3]>,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|d| d.borrow_mut().set_selected_vector_paint(ids, target, color))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

pub fn outline_text() -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    DOCUMENT.with(|d| {
        d.borrow_mut()
            .outline_selected_text(lumapaint_renderer::text_outlines::outline)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

thread_local! { static OUTLINE_VIEW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
pub fn outline_view(value: Option<bool>) -> Result<bool, String> {
    if let Some(value) = value {
        text_editor::finish(true)?;
        OUTLINE_VIEW.with(|state| state.set(value));
        CANVAS.with(|slot| {
            if let Some(canvas) = slot.borrow_mut().as_mut() {
                canvas.renderer.set_outline_view(value);
            }
        });
        redraw()?;
    }
    Ok(OUTLINE_VIEW.with(|state| state.get()))
}

pub fn text_writing_mode(
    mode: lumapaint_core::vector::WritingMode,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    DOCUMENT.with(|doc| {
        doc.borrow_mut()
            .set_text_writing_mode(mode, text_editor::reflow_text)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn saved_path_action(
    action: String,
    id: Option<String>,
    name: String,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    DOCUMENT.with(|doc| {
        let mut document = doc.borrow_mut();
        finish_pen(&mut document, false)?;
        document.saved_path_action(&action, id.as_deref(), &name)
    })?;
    DIRECT_POINTS.with(|points| points.borrow_mut().clear());
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn thumbnail_document() -> Result<super::ThumbnailDocument, String> {
    ensure_document_open()?;
    if let Some(document) = ACTIVE_TILED_DOCUMENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|session| session.document.clone())
    }) {
        return Ok(super::ThumbnailDocument::Tiled(Box::new(document)));
    }
    DOCUMENT.with(|doc| {
        Ok(super::ThumbnailDocument::Standard(Box::new(
            doc.borrow().clone(),
        )))
    })
}

pub fn direct_control_info() -> Result<Vec<crate::canvas::DirectControlInfo>, String> {
    ensure_document_open()?;
    let points = DIRECT_POINTS.with(|p| p.borrow().clone());
    let info = DOCUMENT.with(|doc| {
        let doc = doc.borrow();
        let objects = doc.direct_objects();
        points
            .into_iter()
            .filter_map(|(id, index)| {
                if !doc.selected_vector_ids().contains(&id) {
                    return None;
                }
                let object = &objects.iter().find(|(_, o)| o.id == id)?.1;
                let p =
                    lumapaint_core::bezier::world_point(object, *object.control_points.get(index)?);
                Some(crate::canvas::DirectControlInfo {
                    id,
                    index,
                    x: p[0],
                    y: p[1],
                    anchor: index.is_multiple_of(3),
                    radius: object.live_corners.as_ref().map(|c| c.radius),
                })
            })
            .collect::<Vec<_>>()
    });
    DIRECT_POINTS
        .with(|p| *p.borrow_mut() = info.iter().map(|p| (p.id.clone(), p.index)).collect());
    Ok(info)
}
pub fn edit_direct_controls(
    mode: String,
    values: Vec<f32>,
    preview: bool,
    expected: Vec<(String, usize)>,
    revision: u64,
) -> Result<crate::canvas::DirectEditResult, String> {
    ensure_document_open()?;
    let points = DIRECT_POINTS.with(|p| p.borrow().clone());
    if points != expected
        || points.is_empty()
        || DOCUMENT.with(|d| d.borrow().snapshot().revision) != revision
    {
        return Err("Selection changed; reopen the control editor / 選択が変わりました。節点編集を開き直してください / 选择已更改，请重新打开节点编辑器".into());
    }
    let mut draft = DOCUMENT.with(|doc| doc.borrow().clone());
    match (mode.as_str(), values.as_slice()) {
        ("position", [x, y]) => draft.set_direct_coordinates(&points, [*x, *y])?,
        ("move", [x, y]) => draft.move_vector_controls(&points, [*x, *y], false)?,
        ("corner", [radius]) => draft.set_direct_corner_radius(&points, *radius)?,
        _ => return Err("Invalid direct edit".into()),
    }
    let svg =
        draft.direct_preview_svg(&points.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
    let snapshot = draft.snapshot();
    if !preview {
        DOCUMENT.with(|doc| *doc.borrow_mut() = draft);
        if mode == "corner" {
            DIRECT_POINTS.with(|p| p.borrow_mut().clear());
        }
        redraw()?;
        emit_document();
    }
    Ok(crate::canvas::DirectEditResult {
        snapshot,
        preview: svg,
    })
}

#[cfg(test)]
mod direct_command_tests {
    use super::*;
    fn fixture() -> (String, u64) {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let object = VectorObject {
            live_corners: None,
            id: "direct-command".into(),
            name: "Test".into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            text: None,
            path: VectorPath {
                data: "M0 0H100V100H0Z M20 20V80H80V20Z".into(),
                fill_rule: FillRule::EvenOdd,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            fill: Some(VectorPaint {
                color: [10, 20, 30, 255],
            }),
            stroke: None,
            stroke_width: 0.,
            stroke_style: Default::default(),
            opacity: 1.,
            blend_mode: "normal".into(),
            visible: true,
            kind: VectorObjectKind::Compound,
            control_points: vec![[0., 0.], [100., 100.]],
        };
        doc.upsert_vector_object(&layer, object).unwrap();
        doc.select_direct_objects(vec!["direct-command".into()])
            .unwrap();
        let revision = doc.snapshot().revision;
        DOCUMENT.with(|d| *d.borrow_mut() = doc);
        DOCUMENT_OPEN.with(|v| v.set(true));
        ACTIVE_TILED_DOCUMENT.with(|d| *d.borrow_mut() = None);
        DIRECT_POINTS.with(|p| *p.borrow_mut() = vec![("direct-command".into(), 3)]);
        ("direct-command".into(), revision)
    }
    #[test]
    fn numeric_preview_leaves_authoritative_document_and_history_untouched() {
        let (id, revision) = fixture();
        let before = DOCUMENT.with(|d| d.borrow_mut().encode().unwrap());
        let info = direct_control_info().unwrap();
        assert_eq!((info[0].x, info[0].y), (100., 0.));
        let result =
            edit_direct_controls("corner".into(), vec![20.], true, vec![(id, 3)], revision)
                .unwrap();
        assert!(result.preview.contains(" C "));
        assert_eq!(DOCUMENT.with(|d| d.borrow_mut().encode().unwrap()), before);
        assert_eq!(DIRECT_POINTS.with(|p| p.borrow().len()), 1);
    }
    #[test]
    fn numeric_commit_is_one_edit_and_rejects_stale_revision() {
        let (id, revision) = fixture();
        let expected = vec![(id, 3)];
        let before = DOCUMENT.with(|d| d.borrow_mut().encode().unwrap());
        edit_direct_controls(
            "position".into(),
            vec![120., 10.],
            false,
            expected.clone(),
            revision,
        )
        .unwrap();
        assert_eq!(direct_control_info().unwrap()[0].x, 120.);
        let after = DOCUMENT.with(|d| d.borrow_mut().encode().unwrap());
        assert!(edit_direct_controls(
            "position".into(),
            vec![130., 20.],
            false,
            expected,
            revision
        )
        .is_err());
        assert_eq!(DOCUMENT.with(|d| d.borrow_mut().encode().unwrap()), after);
        DOCUMENT.with(|d| d.borrow_mut().undo());
        assert_eq!(DOCUMENT.with(|d| d.borrow_mut().encode().unwrap()), before);
    }
    #[test]
    fn activating_reloaded_document_attaches_svg_service_without_changing_saved_data() {
        let mut document = Document::default();
        document
            .import_svg(
                "SVG".into(),
                "<svg width=\"100\" height=\"100\"><rect width=\"20\" height=\"20\"/></svg>".into(),
            )
            .unwrap();
        let encoded = document.encode().unwrap();
        let loaded = Document::decode(&encoded).unwrap();
        assert!(!loaded.has_svg_geometry_backend());
        activate_document(OpenDocument {
            id: 1,
            content: OpenDocumentContent::Legacy(Box::new(loaded)),
            path: None,
            fingerprint: None,
        });
        DOCUMENT.with(|slot| {
            let mut document = slot.borrow_mut();
            assert!(document.has_svg_geometry_backend());
            assert_eq!(document.direct_objects().len(), 1);
            assert_eq!(document.encode().unwrap(), encoded);
        });
    }

    #[test]
    fn imported_svg_selection_preview_guides_preserve_source_and_clip_definitions() {
        DIRECT_POINTS.with(|p| p.borrow_mut().clear());
        let mut doc = Document::default();
        lumapaint_svg::attach(&mut doc);
        doc.import_svg("Imported".into(),r##"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><defs><clipPath id="clip"><path d="M0 0H100V100H0Z"/></clipPath></defs><g clip-path="url(#clip)"><path fill="#3070ff" d="M20 20H80V80H20Z"/></g></svg>"##.into()).unwrap();
        let original = doc.svg_layers().next().unwrap().source.clone();
        let p = lumapaint_core::document::Point { x: 20., y: 20. };
        let flags = NSEventModifierFlags::empty();
        direct_pointer(&mut doc, p, 0, flags).unwrap();
        direct_pointer(&mut doc, p, 2, flags).unwrap();
        assert_eq!(DIRECT_POINTS.with(|p| p.borrow().len()), 1);
        let preview = direct_preview(&doc, 1.).unwrap();
        assert_eq!(doc.svg_layers().next().unwrap().source, original);
        assert_eq!(preview.svg_layers().next().unwrap().source, original);
        assert!(preview.svg_layers().count() > 1);
    }
}
