//! AppKit ownership is isolated here; no Apple types enter the renderer or document core.
#[path = "crop_tool_macos.rs"]
mod crop_tool;
#[path = "gradient_tool_macos.rs"]
mod gradient_tool;
#[path = "window_sessions_macos.rs"]
pub(super) mod window_sessions;
pub(super) use window_sessions::{current_label, SessionGuard};

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
    class, define_class, msg_send, rc::Retained, runtime::AnyObject, sel, AnyThread, DefinedClass,
    MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSColor, NSCursor, NSCursorFrameResizeDirections, NSCursorFrameResizePosition, NSEvent,
    NSEventModifierFlags, NSEventSubtype, NSImage, NSView,
};
use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::TrySendError,
    OnceLock,
};
use std::time::{Duration, Instant};
use std::{cell::RefCell, ffi::c_void, ptr::NonNull};
use tauri::{Emitter, Manager};

#[path = "clipboard_macos.rs"]
mod clipboard;
#[path = "clone_stamp_tool_macos.rs"]
mod clone_stamp_tool;
#[path = "paint_bucket_tool_macos.rs"]
mod paint_bucket_tool;
#[path = "pixel_move_macos.rs"]
mod pixel_move;
#[path = "pixel_paint_macos.rs"]
mod pixel_paint;
#[path = "raster_import_macos.rs"]
mod raster_import;
#[path = "selection_tools_macos.rs"]
mod selection_tools;
#[path = "text_editor_macos.rs"]
mod text_editor;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static RASTER_SENDER: OnceLock<crate::render_queue::LatestSender<RasterJob>> = OnceLock::new();
static TILE_SENDER: OnceLock<crate::render_queue::LatestSender<TileJob>> = OnceLock::new();
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
    let (sender, receiver) = crate::render_queue::channel::<RasterJob>();
    if std::thread::Builder::new()
        .name("svg-raster".into())
        .spawn(move || {
            let mut frame_cache = FrameRasterCache::default();
            while let Ok(job) = receiver.recv() {
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
                    if let Some(_session) = window_sessions::for_canvas(job.key.canvas_token) {
                        finish_raster_job(job.key, result, queued_for, cpu_time);
                    }
                });
            }
        })
        .is_ok()
    {
        let _ = RASTER_SENDER.set(sender);
    }
    let (sender, receiver) = crate::render_queue::channel::<TileJob>();
    if std::thread::Builder::new()
        .name("tile-project".into())
        .spawn(move || {
            while let Ok(job) = receiver.recv() {
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
                    let Some(_session) = window_sessions::for_canvas(job.key.source.canvas_token)
                    else {
                        return;
                    };
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
        (current.0 + delta.0).clamp(
            -lumapaint_renderer::MAX_CANVAS_PAN,
            lumapaint_renderer::MAX_CANVAS_PAN,
        ),
        (current.1 + delta.1).clamp(
            -lumapaint_renderer::MAX_CANVAS_PAN,
            lumapaint_renderer::MAX_CANVAS_PAN,
        ),
    )
}

fn pinch_zoom(current: f32, magnification: f32) -> f32 {
    if !magnification.is_finite() {
        return current;
    }
    (current * (1. + magnification).max(0.01)).clamp(
        lumapaint_renderer::MIN_SCREEN_ZOOM,
        lumapaint_renderer::MAX_SCREEN_ZOOM,
    )
}

// UI echoes describe a displayed scale; only a newer explicit command can
// replace the native scale accumulated by trackpad events.
fn synchronized_zoom(
    current: f32,
    applied: Option<u64>,
    requested: f64,
    revision: Option<u64>,
) -> f64 {
    if revision.is_none() || revision > applied {
        requested
    } else {
        current as f64
    }
}

struct FrameState {
    label: String,
    display_link: RefCell<Option<Retained<AnyObject>>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[ivars = FrameState]
    struct PaintView;
impl PaintView {
        #[unsafe(method(frameTick:))]
        fn frame_tick(&self, _link: &AnyObject) {
            let Ok(_session) = SessionGuard::enter_render(&self.ivars().label) else { return; };
            if FRAME_REQUESTED.with(|requested| requested.replace(false)) {
                if let Err(error) = redraw() { emit_error(error); }
            }
            if !FRAME_REQUESTED.with(|requested| requested.get()) {
                if let Some(link) = self.ivars().display_link.borrow().as_ref() {
                    unsafe { let _: () = msg_send![&**link, setPaused: true]; }
                }
            }
        }
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool { true }
        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            let Ok(_session) = SessionGuard::enter_render(&self.ivars().label) else { return; };
            if self.ivars().display_link.borrow().is_some() { return; }
            if FRAME_REQUESTED.with(|requested| requested.replace(false)) {
                text_editor::prepare_frame();
                if let Err(error) = redraw() { emit_error(error); }
            }
        }

        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return std::ptr::null_mut(); };
            let local = NSPoint::new(point.x - self.frame().origin.x, point.y - self.frame().origin.y);
            if CANVAS_OVERLAY.with(|value| value.borrow().iter().any(|r| local.x >= r[0] && local.x <= r[2] && local.y >= r[1] && local.y <= r[3])) {
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
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            let cursor = if PANNING.with(|value| value.get()) {
                Some(NSCursor::closedHandCursor())
            } else {
                match TOOL.with(|tool| tool.get()) {
                    CanvasTool::ZoomIn => Some(NSCursor::zoomInCursor()),
                    CanvasTool::ZoomOut => Some(NSCursor::zoomOutCursor()),
                    CanvasTool::Eyedropper => Some(tool_icon_cursor(CanvasTool::Eyedropper)),
                    CanvasTool::Gradient | CanvasTool::Crop => Some(NSCursor::crosshairCursor()),
                    CanvasTool::Hand => Some(NSCursor::openHandCursor()),
                    CanvasTool::Text | CanvasTool::TextFrame => Some(NSCursor::IBeamCursor()),
                    CanvasTool::TextVertical | CanvasTool::TextFrameVertical => Some(NSCursor::IBeamCursorForVerticalLayout()),
                    CanvasTool::VectorSelect => Some(NSCursor::arrowCursor()),
                    CanvasTool::VectorScale | CanvasTool::VectorRotate => Some(tool_icon_cursor(TOOL.with(|tool|tool.get()))),
                    CanvasTool::VectorDirectSelect | CanvasTool::VectorPen | CanvasTool::VectorPencil | CanvasTool::VectorAnchorAdd | CanvasTool::VectorAnchorDelete | CanvasTool::VectorAnchorConvert | CanvasTool::VectorRectangle | CanvasTool::VectorEllipse | CanvasTool::ImageFrameRectangle | CanvasTool::ImageFrameEllipse | CanvasTool::Rectangle | CanvasTool::Ellipse => Some(tool_icon_cursor(TOOL.with(|tool|tool.get()))),
                    CanvasTool::PaintBucket | CanvasTool::Lasso | CanvasTool::PolygonLasso | CanvasTool::MagneticLasso => Some(NSCursor::crosshairCursor()),
                    CanvasTool::Brush | CanvasTool::Eraser | CanvasTool::CloneStamp | CanvasTool::Blur | CanvasTool::Sharpen | CanvasTool::Smudge | CanvasTool::SelectionBrush => BRUSH_CURSOR.with(|cursor| cursor.borrow().clone()).or_else(|| Some(NSCursor::crosshairCursor())),
                }
            };
            if let Some(cursor) = cursor { self.addCursorRect_cursor(self.bounds(), &cursor); }
            if matches!(TOOL.with(|tool| tool.get()), CanvasTool::VectorSelect | CanvasTool::TextFrame | CanvasTool::TextFrameVertical) {
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
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
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
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            if self.isHidden() || !DOCUMENT_OPEN.with(|open|open.get()) || PANNING.with(|p|p.get()) { return; }
            let delta=event.magnification() as f32;
            if !delta.is_finite() || delta==0. {return;}
            if paint_gesture_active() {return;}
            if text_editor::active() {if let Err(error)=text_editor::finish(true){emit_error(error);return;}}
            let location=self.convertPoint_fromView(event.locationInWindow(),None);
            let viewport=CANVAS.with(|slot|slot.borrow().as_ref().map(|c|c.viewport));
            if let Some(viewport)=viewport {
                let zoom=pinch_zoom(viewport.screen_zoom(),delta)/viewport.fit_zoom();
                self.apply_zoom(viewport,location,zoom);
            }
        }
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self,event:&NSEvent){if modal_input_blocked(&self.ivars().label){return;}let Ok(_session)=SessionGuard::enter(&self.ivars().label)else{return;};if selection_tools::active(){self.pointer(event,3);}}
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            if let Some(window) = self.window() { window.makeFirstResponder(Some(self));window.setAcceptsMouseMovedEvents(true); }
            if SPACE_DOWN.with(|space| space.get()) || TOOL.with(|tool| tool.get()) == CanvasTool::Hand { self.begin_pan(event); } else {
                if matches!(TOOL.with(|tool| tool.get()), CanvasTool::Brush | CanvasTool::Eraser | CanvasTool::CloneStamp | CanvasTool::Blur | CanvasTool::Sharpen | CanvasTool::Smudge) {
                    begin_precise_paint_input();
                }
                self.pointer(event, 0);
                if !paint_gesture_active() { end_precise_paint_input(); }
            }
        }
        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; if PANNING.with(|value| value.get()) { self.update_pan(event); } else { self.pointer(event, 1); } }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; if PANNING.with(|value| value.get()) { self.update_pan(event); PANNING.with(|value| value.set(false)); self.refresh_cursor(); } else { self.pointer(event, 2); } end_precise_paint_input(); }
        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; self.begin_pan(event); }
        #[unsafe(method(otherMouseDragged:))]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; self.update_pan(event); }
        #[unsafe(method(otherMouseUp:))]
        fn other_mouse_up(&self, event: &NSEvent) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; self.update_pan(event); PANNING.with(|value| value.set(false)); self.refresh_cursor(); }
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            if self.isHidden() { return; }
            if raster_import::active() {
                if [36,76,53].contains(&event.keyCode()) {
                    if let Err(error) = raster_import::finish(event.keyCode()!=53) { emit_error(error); }
                }
                return;
            }
            if event.keyCode() == 49 { SPACE_DOWN.with(|space| space.set(true)); }
            else if event.keyCode() == 34 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if let Err(error) = switch_canvas_tool(CanvasTool::Eyedropper) { emit_error(error); return; }
                self.refresh_cursor();
                if let Some(app) = APP.get() { let _ = app.emit_to(current_label(), "canvas-tool-changed", CanvasTool::Eyedropper); }
            }
            else if event.keyCode() == 5 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                let tool=if event.modifierFlags().contains(NSEventModifierFlags::Shift){CanvasTool::PaintBucket}else{CanvasTool::Gradient};if let Err(error)=switch_canvas_tool(tool){emit_error(error);return;}
                self.refresh_cursor();
                if let Some(app)=APP.get(){let _=app.emit_to(current_label(),"canvas-tool-changed",tool);}
            }
            else if event.keyCode()==3 && !event.modifierFlags().intersects(NSEventModifierFlags::Command|NSEventModifierFlags::Control|NSEventModifierFlags::Option){let tool=if event.modifierFlags().contains(NSEventModifierFlags::Shift){CanvasTool::ImageFrameEllipse}else{CanvasTool::ImageFrameRectangle};if let Err(e)=switch_canvas_tool(tool){emit_error(e);return;}
            if let Some(app)=APP.get(){let _=app.emit_to(current_label(),"canvas-tool-changed",tool);}}
            else if event.keyCode() == 14 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if let Err(error) = switch_canvas_tool(CanvasTool::Eraser) { emit_error(error); return; }
                if let Some(app) = APP.get() { let _ = app.emit_to(current_label(), "canvas-tool-changed", CanvasTool::Eraser); }
            }
            else if event.keyCode() == 7 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if !event.isARepeat() {
                    if let Some(app) = APP.get() { let _ = app.emit_to(current_label(), "canvas-swap-colors", ()); }
                }
            }
            else if event.keyCode() == 17 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if !event.isARepeat() {
                    if let Err(error) = finish_open_pen() { emit_error(error); return; }
                    if let Some(app) = APP.get() { let _ = app.emit_to(current_label(), "canvas-text-edit", ()); }
                }
            }
            else if [36, 76].contains(&event.keyCode()) && TOOL.with(|tool| tool.get()) == CanvasTool::VectorPen {
                if let Err(error) = DOCUMENT.with(|doc| finish_pen(&mut doc.borrow_mut(), false)).and_then(|_| redraw()) { emit_error(error); }
                emit_document();
            }
            else if [123,124,125,126].contains(&event.keyCode())
                && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option)
                && DOCUMENT.with(|d| !d.borrow().guides().selected.is_empty()) {
                let amount=DOCUMENT.with(|d| d.borrow().guides().nudge[usize::from(event.modifierFlags().contains(NSEventModifierFlags::Shift))]);
                let delta=match event.keyCode() {123=>[-amount,0.],124=>[amount,0.],125=>[0.,amount],_=>[0.,-amount]};
                if let Err(error)=edit_guides(lumapaint_core::document::GuideEdit {action:"moveSelected".into(),id:None,axis:None,position:None,delta:Some(delta)}) {emit_error(error);}
            }
            else if [123,124,125,126].contains(&event.keyCode()) && TOOL.with(|tool| tool.get()) == CanvasTool::VectorDirectSelect {
                let amount = if event.modifierFlags().contains(NSEventModifierFlags::Shift) { 10. } else { 1. };
                let delta = match event.keyCode() { 123 => [-amount,0.], 124 => [amount,0.], 125 => [0.,amount], _ => [0.,-amount] };
                let points = DIRECT_POINTS.with(|points| points.borrow().clone());
                if let Err(error) = DOCUMENT.with(|doc| doc.borrow_mut().move_vector_controls(&points,delta,false)).and_then(|_| redraw()) { emit_error(error); }
                emit_document();
            }
            else if [36,76].contains(&event.keyCode()) && selection_tools::active() {if let Err(e)=selection_tools::confirm(){emit_error(e);}}
            else if [51,117].contains(&event.keyCode()) && selection_tools::active() {if let Err(e)=selection_tools::remove_last().and_then(|_|redraw()){emit_error(e);}}
            else if event.keyCode()==32 && !event.modifierFlags().intersects(NSEventModifierFlags::Command|NSEventModifierFlags::Control|NSEventModifierFlags::Option) {let current=TOOL.with(|t|t.get());let tool=if event.modifierFlags().contains(NSEventModifierFlags::Shift){match current{CanvasTool::Blur=>CanvasTool::Sharpen,CanvasTool::Sharpen=>CanvasTool::Smudge,_=>CanvasTool::Blur}}else{CanvasTool::Blur};if let Err(e)=switch_canvas_tool(tool){emit_error(e);}
                if let Some(app)=APP.get(){let _=app.emit_to(current_label(),"canvas-tool-changed",tool);}}
            else if event.keyCode()==37 && !event.modifierFlags().intersects(NSEventModifierFlags::Command|NSEventModifierFlags::Control|NSEventModifierFlags::Option) {let current=TOOL.with(|t|t.get());let tool=if event.modifierFlags().contains(NSEventModifierFlags::Shift){match current{CanvasTool::Lasso=>CanvasTool::PolygonLasso,CanvasTool::PolygonLasso=>CanvasTool::MagneticLasso,CanvasTool::MagneticLasso=>CanvasTool::SelectionBrush,_=>CanvasTool::Lasso}}else{CanvasTool::Lasso};if let Err(e)=switch_canvas_tool(tool){emit_error(e);}
                if let Some(app)=APP.get(){let _=app.emit_to(current_label(),"canvas-tool-changed",tool);}}
            else if [36,76].contains(&event.keyCode()) && TOOL.with(|t|t.get())==CanvasTool::Crop { if let Err(e)=crop_tool::commit(){emit_error(e);} }
            else if event.keyCode() == 8 && !event.modifierFlags().intersects(NSEventModifierFlags::Command|NSEventModifierFlags::Control|NSEventModifierFlags::Option) { if let Err(e)=switch_canvas_tool(CanvasTool::Crop){emit_error(e);return;}
                if let Some(app)=APP.get(){let _=app.emit_to(current_label(),"canvas-tool-changed",CanvasTool::Crop);} }
            else if event.keyCode() == 53 {
                if selection_tools::cancel() || crop_tool::cancel() || cancel_vector_drag() || DOCUMENT.with(|doc| doc.borrow_mut().cancel_selection_gesture()) {
                    if let Err(error) = redraw() { emit_error(error); }
                    emit_document();
                } else { report_edit(DocumentAction::Deselect); }
            }
            else if [51, 117].contains(&event.keyCode())
                && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option)
            {
                report_edit(DocumentAction::DeleteSelectedObjects);
            }
            else if !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) && [1, 11, 46].contains(&event.keyCode()) {
                let tool = if event.keyCode() == 1 { CanvasTool::CloneStamp } else if event.keyCode() == 11 { CanvasTool::Brush }
                    else if event.modifierFlags().contains(NSEventModifierFlags::Shift) { CanvasTool::Ellipse }
                    else { CanvasTool::Rectangle };
                if let Err(error) = switch_canvas_tool(tool) { emit_error(error); return; }
                if let Some(app) = APP.get() { let _ = app.emit_to(current_label(), "canvas-tool-changed", tool); }
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
                if let Some(app) = APP.get() { let _ = app.emit_to(current_label(), "canvas-tool-changed", tool); }
            }
            else { unsafe { msg_send![super(self), keyDown: event] } }
        }
        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; };
            if event.keyCode() == 49 { SPACE_DOWN.with(|space| space.set(false)); PANNING.with(|value| value.set(false)); self.refresh_cursor(); }
            else { unsafe { msg_send![super(self), keyUp: event] } }
        }
        #[unsafe(method(performKeyEquivalent:))]
        fn key_equivalent(&self, event: &NSEvent) -> bool {
            if modal_input_blocked(&self.ivars().label) { return false.into(); }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return false.into(); };
            if self.isHidden() || text_editor::active() { return false.into(); }
            // WebView fields own their editing shortcuts while focused (including Cmd+A).
            // AppKit asks sibling views for key equivalents even when they are not responders.
            let focused = self.window().and_then(|window| window.firstResponder()).is_some_and(|responder| {
                Retained::as_ptr(&responder).cast::<c_void>() == (self as *const Self).cast::<c_void>()
            });
            if !focused { return false.into(); }
            if raster_import::active() { return event.modifierFlags().contains(NSEventModifierFlags::Command).into(); }
            let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
            if command && event.keyCode() == 19 {
                report_edit(if event.modifierFlags().contains(NSEventModifierFlags::Option) { DocumentAction::UnlockAllObjects } else { DocumentAction::LockSelection }); true
            } else if command && event.keyCode() == 20 {
                report_edit(if event.modifierFlags().contains(NSEventModifierFlags::Option) { DocumentAction::ShowAllObjects } else { DocumentAction::HideSelection }); true
            } else if command && [7,8,9].contains(&event.keyCode()) {
                report_edit(match event.keyCode() { 7 => DocumentAction::Cut, 8 => DocumentAction::Copy, _ => DocumentAction::Paste }); true
            } else if command && event.keyCode() == 2 {
                if let Some(app)=APP.get(){let _=app.emit_to(current_label(),"place-image-requested",());} true
            } else if command && (event.keyCode()==22 || event.modifierFlags().contains(NSEventModifierFlags::Option) && [0,30,33].contains(&event.keyCode())) {
                let action=if event.keyCode()==22 {"reselect"} else if event.keyCode()==0 {"artboard"} else if event.keyCode()==30 {"above"} else {"below"};
                if let Err(e)=vector_selection_action(lumapaint_core::document::VectorSelectionRequest{action:action.into(),criterion:None,name:None,new_name:None}){emit_error(e);}true
            } else if command && event.keyCode() == 0 {
                report_edit(if !event.modifierFlags().contains(NSEventModifierFlags::Shift) { DocumentAction::SelectAll } else { DocumentAction::Deselect }); true
            } else if command && event.keyCode() == 34 && event.modifierFlags().contains(NSEventModifierFlags::Shift) {
                report_edit(DocumentAction::InvertSelection); true
            } else if command && event.keyCode() == 45 && event.modifierFlags().contains(NSEventModifierFlags::Shift) {
                if let Some(app) = APP.get() { if let Err(error) = crate::editor_windows::create(app) { emit_error(error); } }
                true
            } else if command && event.keyCode() == 45 {
                if let Some(app) = APP.get() { let _ = app.emit_to(current_label(), "new-document-requested", ()); }
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
        fn undo_action(&self, _sender: Option<&AnyObject>) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; report_edit(DocumentAction::Undo); }
        #[unsafe(method(redo:))]
        fn redo_action(&self, _sender: Option<&AnyObject>) {
            if modal_input_blocked(&self.ivars().label) { return; }
            let Ok(_session) = SessionGuard::enter(&self.ivars().label) else { return; }; report_edit(DocumentAction::Redo); }
    }
);

impl PaintView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let view = Self::alloc(mtm).set_ivars(FrameState {
            label: current_label(),
            display_link: RefCell::new(None),
        });
        // SAFETY: initializing the allocated NSView subclass once on the main thread.
        unsafe { msg_send![super(view), initWithFrame: frame] }
    }

    fn request_frame(&self) {
        let mut slot = self.ivars().display_link.borrow_mut();
        if slot.is_none() {
            // macOS 14+: follows the view's display, including 60/120Hz and
            // display changes. Older systems use AppKit's layer update cycle.
            let available: bool = unsafe {
                msg_send![self, respondsToSelector: sel!(displayLinkWithTarget:selector:)]
            };
            if available {
                unsafe {
                    let link: Retained<AnyObject> =
                        msg_send![self, displayLinkWithTarget: self, selector: sel!(frameTick:)];
                    let run_loop = NSRunLoop::mainRunLoop();
                    let _: () =
                        msg_send![&*link, addToRunLoop: &*run_loop, forMode: NSRunLoopCommonModes];
                    *slot = Some(link);
                }
            }
        }
        if let Some(link) = slot.as_ref() {
            unsafe {
                let _: () = msg_send![&**link, setPaused: false];
            }
        } else {
            self.setNeedsDisplay(true);
        }
    }

    fn stop_frames(&self) {
        if let Some(link) = self.ivars().display_link.borrow_mut().take() {
            unsafe {
                let _: () = msg_send![&*link, invalidate];
            }
        }
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
        if let Err(error) = request_redraw() {
            emit_error(error);
        }
        self.refresh_cursor();
        if let Some(app) = APP.get() {
            let _ = app.emit_to(
                current_label(),
                "canvas-zoom-changed",
                viewport.screen_zoom(),
            );
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
        if let Some((axis, _)) = GUIDE_DRAFT
            .with(|g| g.borrow().clone())
            .filter(|_| GUIDE_DRAG.with(|g| g.borrow().is_none()))
        {
            let position = if axis == "horizontal" {
                point.y
            } else {
                point.x
            };
            if let Err(e) = ruler_guide(axis, position, if phase == 2 { 2 } else { 1 }) {
                emit_error(e);
            }
            return;
        }
        if matches!(
            TOOL.with(|t| t.get()),
            CanvasTool::VectorSelect | CanvasTool::VectorDirectSelect
        ) && !text_editor::active()
        {
            if phase == 0 {
                let scale = ruler_viewport(viewport).zoom;
                if let Some(id) =
                    DOCUMENT.with(|d| d.borrow().guide_hit([point.x, point.y], 6. / scale))
                {
                    DIRECT_POINTS.with(|p| p.borrow_mut().clear());
                    DOCUMENT.with(|d| {
                        let mut d = d.borrow_mut();
                        if event.modifierFlags().contains(NSEventModifierFlags::Shift) {
                            d.select_guide_additive(id.clone());
                        } else if !d.guides().selected.contains(&id) {
                            d.select_guide(Some(id.clone()));
                        }
                    });
                    GUIDE_DRAG.with(|g| *g.borrow_mut() = Some((id, [point.x, point.y])));
                    emit_document();
                    let _ = redraw();
                    return;
                }
                if !event.modifierFlags().contains(NSEventModifierFlags::Shift) {
                    DOCUMENT.with(|d| d.borrow_mut().select_guide(None));
                }
            }
            if let Some((id, start)) = GUIDE_DRAG.with(|g| g.borrow().clone()) {
                if phase == 2 {
                    GUIDE_DRAG.with(|g| g.borrow_mut().take());
                    if (point.x - start[0]).hypot(point.y - start[1]) < 0.001 {
                        GUIDE_DRAFT.with(|g| g.borrow_mut().take());
                        GUIDE_OBJECT_DRAFT.with(|g| g.borrow_mut().clear());
                        let _ = redraw();
                        return;
                    }
                    let axis = DOCUMENT.with(|d| {
                        d.borrow()
                            .guides()
                            .items
                            .iter()
                            .find(|g| g.id == id)
                            .and_then(|g| g.axis.clone())
                    });
                    let position = axis.map(|a| if a == "horizontal" { point.y } else { point.x });
                    let edit = lumapaint_core::document::GuideEdit {
                        action: if event.modifierFlags().contains(NSEventModifierFlags::Option) {
                            "duplicate"
                        } else {
                            "moveSelected"
                        }
                        .into(),
                        id: Some(id),
                        axis: None,
                        position,
                        delta: Some([point.x - start[0], point.y - start[1]]),
                    };
                    if let Err(e) = edit_guides(edit) {
                        emit_error(e);
                    }
                } else if phase == 1 {
                    GUIDE_DRAFT.with(|g| g.borrow_mut().take());
                    let [dx, dy] = [point.x - start[0], point.y - start[1]];
                    let edges = DOCUMENT.with(|d| {
                        let d = d.borrow();
                        let a = viewport.document_point(0., 0.);
                        let b = viewport.document_point(
                            viewport.width as f32 / viewport.scale,
                            viewport.height as f32 / viewport.scale,
                        );
                        d.guides()
                            .items
                            .iter()
                            .filter(|g| d.guides().selected.contains(&g.id))
                            .flat_map(|g| match g.axis.as_deref() {
                                Some("horizontal") => vec![[[a.x, g.position], [b.x, g.position]]],
                                Some("vertical") => vec![[[g.position, a.y], [g.position, b.y]]],
                                _ => g
                                    .objects
                                    .iter()
                                    .flat_map(|o| {
                                        lumapaint_core::stroke::path_edges(
                                            &o.path.data,
                                            o.transform,
                                        )
                                    })
                                    .collect(),
                            })
                            .collect::<Vec<_>>()
                    });
                    GUIDE_OBJECT_DRAFT.with(|g| {
                        *g.borrow_mut() = edges
                            .into_iter()
                            .map(|[a, b]| {
                                let p = [a[0] + dx, a[1] + dy];
                                let q = [b[0] + dx, b[1] + dy];
                                lumapaint_renderer::FrameOverlay {
                                    corners: [p, q, q, p],
                                    handles: false,
                                    baseline: None,
                                }
                            })
                            .collect()
                    });
                    let _ = redraw();
                }
                return;
            }
        }
        if phase == 0 && !raster_import::active() {
            let page = DOCUMENT.with(|doc| {
                doc.borrow().facing_neighbor().and_then(|(index, x, n)| {
                    let (w, h) = n.dimensions();
                    (point.x >= x && point.x < x + w as f32 && point.y >= 0. && point.y < h as f32)
                        .then_some(index)
                })
            });
            if let Some(index) = page {
                if let Err(error) = edit_pages(lumapaint_core::document::PageEdit {
                    action: "select".into(),
                    index: Some(index),
                    facing: None,
                    binding: None,
                }) {
                    emit_error(error);
                }
                return;
            }
        }
        let tool = TOOL.with(|tool| tool.get());
        if tool == CanvasTool::Crop {
            if let Err(e) = crop_tool::pointer(point, phase, viewport) {
                emit_error(e);
            }
            return;
        }
        if tool == CanvasTool::Gradient {
            if let Err(e) = gradient_tool::pointer(point, phase, event.modifierFlags()) {
                gradient_tool::cancel();
                emit_error(e);
            }
            return;
        }
        if tool == CanvasTool::Eyedropper {
            if phase == 0 {
                if let Err(error) = sample_canvas_color(point) {
                    emit_error(error);
                }
            }
            return;
        }
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
                let zoom = (viewport.screen_zoom() * factor).clamp(
                    lumapaint_renderer::MIN_SCREEN_ZOOM,
                    lumapaint_renderer::MAX_SCREEN_ZOOM,
                ) / viewport.fit_zoom();
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
            if matches!(tool, CanvasTool::TextFrame | CanvasTool::TextFrameVertical)
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
            if matches!(tool, CanvasTool::Text | CanvasTool::TextVertical)
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
                    if let Err(error) = text_editor::begin_at(
                        [point.x, point.y],
                        matches!(tool, CanvasTool::Text | CanvasTool::TextVertical),
                    ) {
                        emit_error(error);
                    }
                }
                return;
            }
        }
        if matches!(
            TOOL.with(|tool| tool.get()),
            CanvasTool::Text | CanvasTool::TextVertical
        ) {
            return;
        }
        if matches!(
            TOOL.with(|tool| tool.get()),
            CanvasTool::TextFrame | CanvasTool::TextFrameVertical
        ) {
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
            .and_then(|_| {
                if phase == 1 {
                    request_redraw()
                } else {
                    redraw()
                }
            });
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
                        | CanvasTool::ImageFrameRectangle
                        | CanvasTool::ImageFrameEllipse
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
                if selection_tools::is_tool(tool) {
                    return selection_tools::pointer(
                        &mut doc,
                        tool,
                        point,
                        phase,
                        event.modifierFlags(),
                        event.clickCount() > 1,
                    );
                }
                if tool == CanvasTool::PaintBucket {
                    return paint_bucket_tool::pointer(&mut doc, point, phase);
                }
                if matches!(
                    tool,
                    CanvasTool::Blur | CanvasTool::Sharpen | CanvasTool::Smudge
                ) {
                    let mut settings = retouch_settings()?;
                    if tool == CanvasTool::Smudge
                        && event.modifierFlags().contains(NSEventModifierFlags::Option)
                    {
                        settings.finger_painting = true;
                    }
                    let kind = match tool {
                        CanvasTool::Blur => lumapaint_core::retouch::Kind::Blur,
                        CanvasTool::Sharpen => lumapaint_core::retouch::Kind::Sharpen,
                        _ => lumapaint_core::retouch::Kind::Smudge,
                    };
                    return pixel_paint::retouch_pointer(
                        &mut doc, point, phase, pressure, kind, settings,
                    )
                    .map(|_| ());
                }
                if tool == CanvasTool::CloneStamp {
                    return clone_stamp_tool::pointer(
                        &mut doc,
                        point,
                        phase,
                        pressure,
                        event.modifierFlags(),
                    );
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
            .and_then(|_| {
                if phase == 1 {
                    request_redraw()
                } else {
                    redraw()
                }
            });
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
        if let Err(error) = request_redraw() {
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
    linear: [f32; 4],
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
            rectangle_radii: None,
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
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: None,
            stroke: (!self.brush.no_color).then_some(VectorPaint {
                registration: false,
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
            registration: false,
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

fn sample_canvas_color(point: lumapaint_core::document::Point) -> Result<(), String> {
    text_editor::finish(true)?;
    let color = ACTIVE_TILED_DOCUMENT
        .with(|tiled| {
            tiled.borrow().as_ref().map(|session| {
                lumapaint_renderer::color_sampler::sample_tiled(&session.document, point)
            })
        })
        .unwrap_or_else(|| {
            CANVAS.with(|slot| {
                let mut slot = slot.borrow_mut();
                let canvas = slot.as_mut().ok_or("Canvas unavailable")?;
                DOCUMENT.with(|document| canvas.renderer.sample_color(&document.borrow(), point))
            })
        })?;
    if let Some(color) = color {
        BRUSH.with(|brush| brush.borrow_mut().color = color);
        // Editable path selections receive the sampled fill; raster sampling never paints.
        let changed = if ACTIVE_TILED_DOCUMENT.with(|tiled| tiled.borrow().is_some()) {
            false
        } else {
            DOCUMENT.with(|document| {
                let mut document = document.borrow_mut();
                let ids = document.selected_vector_ids().to_vec();
                let editable = !ids.is_empty()
                    && document
                        .svg_layers()
                        .flat_map(|layer| {
                            layer
                                .vector_objects
                                .iter()
                                .map(move |object| (layer, object))
                        })
                        .filter(|(_, object)| ids.contains(&object.id))
                        .all(|(layer, object)| {
                            !layer.locked
                                && layer.visible
                                && object.visible
                                && object.text.is_none()
                        });
                if editable {
                    document.set_selected_vector_paint(&ids, "fill", Some(color))?;
                }
                Ok::<_, String>(editable)
            })?
        };
        if let Some(app) = APP.get() {
            let _ = app.emit_to(current_label(), "canvas-sampled-color", color);
        }
        if changed {
            redraw()?;
            emit_document();
        }
    }
    Ok(())
}

fn switch_canvas_tool(tool: CanvasTool) -> Result<(), String> {
    if TOOL.with(|current| current.get()) != tool {
        finish_open_pen()?;
        crop_tool::cancel();
        selection_tools::cancel();
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
            registration: false,
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

fn vector_sample_spacing(viewport: Option<Viewport>) -> f32 {
    viewport.map_or(1.0, |viewport| 1.0 / viewport.screen_zoom())
}
fn current_vector_sample_spacing() -> f32 {
    CANVAS.with(|slot| vector_sample_spacing(slot.borrow().as_ref().map(|canvas| canvas.viewport)))
}

fn vector_pointer(
    document: &mut Document,
    tool: CanvasTool,
    point: lumapaint_core::document::Point,
    phase: u8,
    modifiers: NSEventModifierFlags,
) -> Result<(), String> {
    let point = if (phase > 0
        || matches!(
            tool,
            CanvasTool::VectorRectangle
                | CanvasTool::VectorEllipse
                | CanvasTool::VectorPen
                | CanvasTool::VectorPencil
        ))
        && !modifiers.contains(NSEventModifierFlags::Control)
        && !matches!(
            tool,
            CanvasTool::VectorRotate | CanvasTool::VectorScale | CanvasTool::VectorSelect
        ) {
        let tolerance = CANVAS.with(|c| {
            c.borrow()
                .as_ref()
                .map_or(6., |c| 6. / c.viewport.screen_zoom())
        });
        let p = document.snap_to_guides([point.x, point.y], tolerance);
        lumapaint_core::document::Point { x: p[0], y: p[1] }
    } else {
        point
    };
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
    let spacing = current_vector_sample_spacing();
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
                .is_none_or(|last| (point.x - last.x).hypot(point.y - last.y) >= spacing)
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
            .is_none_or(|last| (point.x - last.x).hypot(point.y - last.y) >= spacing)
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
    if tool != CanvasTool::VectorPencil && (width < spacing || height < spacing) {
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
                    registration: false,
                    color: [color[0], color[1], color[2], 255],
                }),
                size,
                VectorObjectKind::Path,
                points.iter().map(|point| [point.x, point.y]).collect(),
            )
        }
        CanvasTool::VectorRectangle | CanvasTool::ImageFrameRectangle => {
            let x = start.x.min(end.x);
            let y = start.y.min(end.y);
            let data = format!("M {x} {y} H {} V {} H {x} Z", x + width, y + height);
            let color = BRUSH.with(|brush| brush.borrow().color);
            (
                "Rectangle",
                data,
                Some(VectorPaint {
                    registration: false,
                    color: [color[0], color[1], color[2], 255],
                }),
                None,
                0.0,
                VectorObjectKind::Rectangle,
                vec![[x, y], [x + width, y + height]],
            )
        }
        CanvasTool::VectorEllipse | CanvasTool::ImageFrameEllipse => {
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
                    registration: false,
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
    let serial = NEXT_VECTOR_OBJECT_ID.with(|next| {
        let value = next.get();
        next.set(value + 1);
        value
    });
    let object = VectorObject {
        image_frame: matches!(
            tool,
            CanvasTool::ImageFrameRectangle | CanvasTool::ImageFrameEllipse
        )
        .then(Default::default),
        fill_gradient: None,
        stroke_gradient: None,
        live_corners: None,
        rectangle_radii: None,
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
        fill: if BRUSH.with(|brush| brush.borrow().no_color)
            || matches!(
                tool,
                CanvasTool::ImageFrameRectangle | CanvasTool::ImageFrameEllipse
            ) {
            None
        } else {
            fill
        },
        stroke: if BRUSH.with(|brush| brush.borrow().no_color) {
            None
        } else {
            stroke
        },
        stroke_width,
        stroke_style: Default::default(),
        visible: true,
        kind,
        control_points,
    };
    if matches!(
        tool,
        CanvasTool::ImageFrameRectangle | CanvasTool::ImageFrameEllipse
    ) {
        document.insert_image_frame(object)?;
    } else {
        let layer = match document
            .selected_vector_target()?
            .or_else(|| document.editable_vector_layer_id())
        {
            Some(id) => id,
            None => document.add_vector_layer()?,
        };
        document.upsert_vector_object(&layer, object)?;
    }
    Ok(())
}

#[derive(Clone)]
struct DirectGesture {
    corner: Option<(String, lumapaint_core::bezier::CornerHandle)>,
    corner_all: bool,
    start: [f32; 2],
    current: [f32; 2],
    marquee: bool,
    baseline: Vec<(String, usize)>,
    break_smooth: bool,
}

// Direct selection reaches imported SVG geometry even when its layer is an image layer.
fn selection_uses_pixel_move(document: &Document, tool: CanvasTool) -> bool {
    !document.selected_layer_is_vector()
        && document.guides().selected.is_empty()
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
        if let Some((id, handle)) = objects
            .iter()
            .rev()
            .filter(|(_, o)| document.selected_vector_ids().contains(&o.id))
            .find_map(|(_, o)| {
                bezier::corner_handles(o, tolerance * 2.)
                    .into_iter()
                    .find(|h| {
                        (h.point[0] - position[0]).hypot(h.point[1] - position[1]) <= tolerance
                    })
                    .map(|h| (o.id.clone(), h))
            })
        {
            DIRECT_GESTURE.with(|draft| {
                *draft.borrow_mut() = Some(DirectGesture {
                    corner: Some((id, handle)),
                    corner_all: flags.contains(NSEventModifierFlags::Shift),
                    start: position,
                    current: position,
                    marquee: false,
                    baseline: vec![],
                    break_smooth: false,
                })
            });
            return Ok(());
        }
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
                corner: None,
                corner_all: false,
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
                if flags.contains(NSEventModifierFlags::Shift)
                    && !draft.marquee
                    && draft.corner.is_none()
                {
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
                if draft.corner.is_some() {
                    apply_corner_gesture(document, &draft)?;
                } else if draft.marquee {
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

fn apply_corner_gesture(document: &mut Document, draft: &DirectGesture) -> Result<(), String> {
    let Some((id, handle)) = &draft.corner else {
        return Ok(());
    };
    if draft.start == draft.current {
        return Ok(());
    }
    let delta = [
        draft.current[0] - draft.start[0],
        draft.current[1] - draft.start[1],
    ];
    let radius = (handle.radius
        + (delta[0] * handle.direction[0] + delta[1] * handle.direction[1]) / handle.factor)
        .clamp(0., 4096.);
    let values = if draft.corner_all {
        let object = document
            .direct_objects()
            .into_iter()
            .find(|(_, o)| &o.id == id)
            .ok_or("Corner object missing")?
            .1;
        lumapaint_core::bezier::corner_handles(&object, 0.)
            .into_iter()
            .map(|h| (h.anchor, radius))
            .collect()
    } else {
        vec![(handle.anchor, radius)]
    };
    document.set_live_corner_values(id, &values)
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
        if draft.corner.is_some() {
            apply_corner_gesture(&mut preview, draft)?;
        } else {
            preview.move_vector_controls(
                &points,
                [
                    draft.current[0] - draft.start[0],
                    draft.current[1] - draft.start[1],
                ],
                draft.break_smooth,
            )?;
        }
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
        marker.fill_gradient = None;
        marker.stroke_gradient = None;
        marker.live_corners = None;
        marker.fill = Some(VectorPaint {
            registration: false,
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
        marker.path.fill_rule = lumapaint_core::vector::FillRule::EvenOdd;
        for handle in lumapaint_core::bezier::corner_handles(&object, box_tolerance() * 2.) {
            let [x, y] = handle.point;
            for r in [4.5 / zoom, 2.5 / zoom] {
                let _ = write!(
                    marker.path.data,
                    " M {} {y} a {r} {r} 0 1 0 {} 0 a {r} {r} 0 1 0 {} 0 Z",
                    x - r,
                    2. * r,
                    -2. * r
                );
            }
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
                rectangle_radii: None,
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
                image_frame: None,
                fill_gradient: None,
                stroke_gradient: None,
                fill: None,
                stroke: Some(VectorPaint {
                    registration: false,
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
                draft.scale = if initial >= current_vector_sample_spacing() {
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
            if (point.x - center[0]).hypot(point.y - center[1]) >= current_vector_sample_spacing() {
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
                if (point.x - d.center[0]).hypot(point.y - d.center[1])
                    >= current_vector_sample_spacing()
                {
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
    scale_content: bool,
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
                    scale_content: false,
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

fn snapped_artwork_offset(
    document: &Document,
    offset: [f32; 2],
    modifiers: NSEventModifierFlags,
) -> [f32; 2] {
    GUIDE_OBJECT_DRAFT.with(|g| g.borrow_mut().clear());
    if modifiers.contains(NSEventModifierFlags::Control) {
        return offset;
    }
    let Some(bounds) = document.selected_vector_bounds() else {
        return offset;
    };
    let tolerance = current_vector_sample_spacing() * 6.;
    let baseline: Vec<_> = selected_text_frame_overlay(document)
        .and_then(|f| f.baseline)
        .into_iter()
        .flatten()
        .collect();
    let (offset, markers) = document.snap_guide_translation(bounds, offset, &baseline, tolerance);
    let radius = current_vector_sample_spacing() * 3.;
    GUIDE_OBJECT_DRAFT.with(|g| {
        *g.borrow_mut() = markers
            .into_iter()
            .map(|[x, y]| lumapaint_renderer::FrameOverlay {
                corners: [
                    [x - radius, y - radius],
                    [x + radius, y - radius],
                    [x + radius, y + radius],
                    [x - radius, y + radius],
                ],
                handles: false,
                baseline: None,
            })
            .collect()
    });
    offset
}

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
                document.resize_selected_image_frames(d.matrix, d.rotate || d.scale_content)?;
            }
        }
        return Ok(());
    }
    if phase == 0 {
        cancel_vector_drag();
        if let Some((settings, handle)) = selected_text_resize_handle(document, [point.x, point.y])
            .filter(|_| !modifiers.contains(NSEventModifierFlags::Command))
        {
            TEXT_RESIZE_DRAFT.with(|draft| {
                *draft.borrow_mut() = Some(TextResizeDraft {
                    linear: text_object_linear(document, settings.id.as_deref()),
                    original: settings,
                    handle,
                    start: [point.x, point.y],
                    current: [point.x, point.y],
                });
            });
            return Ok(());
        }
        if TOOL.with(|t| t.get()) == CanvasTool::VectorSelect
            || (modifiers.contains(NSEventModifierFlags::Command)
                && selected_text_resize_handle(document, [point.x, point.y]).is_some())
        {
            if let Some(mut d) = box_hit(document, [point.x, point.y]) {
                d.scale_content = modifiers.contains(NSEventModifierFlags::Command);
                BOX_DRAFT.with(|draft| *draft.borrow_mut() = Some(d));
                return Ok(());
            }
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
                VECTOR_MOVE.with(|offset| {
                    offset.set(snapped_artwork_offset(
                        document,
                        [point.x - start.x, point.y - start.y],
                        modifiers,
                    ))
                });
            }
        }
    } else if phase == 2 {
        if let Some((start, _, additive)) = VECTOR_MARQUEE.with(|draft| draft.borrow_mut().take()) {
            return document.select_layout_in_rect(
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
                if text_frame_needs_reflow(&draft.original, &settings) {
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
            let offset =
                snapped_artwork_offset(document, [point.x - start.x, point.y - start.y], modifiers);
            GUIDE_OBJECT_DRAFT.with(|g| g.borrow_mut().clear());
            if offset != [0.0, 0.0] {
                // Present the release position with the existing drag textures before
                // committing. The worker can then prepare the new SVG without a blank frame.
                CANVAS.with(|slot| -> Result<(), String> {
                    let mut slot = slot.borrow_mut();
                    if let Some(canvas) = slot.as_mut() {
                        let overlay = selected_text_frame_overlay(document).map(|mut overlay| {
                            for corner in overlay
                                .corners
                                .iter_mut()
                                .chain(overlay.baseline.iter_mut().flatten())
                            {
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
    let object = document
        .svg_layers()
        .filter(|layer| layer.visible && !layer.locked && layer.vector_layer)
        .flat_map(|layer| &layer.vector_objects)
        .find(|object| object.id == selected[0] && object.visible)?;
    let text = object.text.as_ref()?;
    if text.point_text {
        return None;
    }
    let height = text_frame_height(text);
    let settings = TextSettings {
        id: Some(object.id.clone()),
        text: text.clone(),
        position: [object.transform[4], object.transform[5]],
        color: object.fill.map_or([0, 0, 0], |fill| {
            [fill.color[0], fill.color[1], fill.color[2]]
        }),
    };
    let linear = [
        object.transform[0],
        object.transform[1],
        object.transform[2],
        object.transform[3],
    ];
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
            let target = text_frame_world_point(&settings, linear, [x, y]);
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
        let a = text_frame_world_point(&settings, linear, from);
        let b = text_frame_world_point(&settings, linear, to);
        let vx = b[0] - a[0];
        let vy = b[1] - a[1];
        let t = (((point[0] - a[0]) * vx + (point[1] - a[1]) * vy) / (vx * vx + vy * vy))
            .clamp(0.0, 1.0);
        let distance = (point[0] - a[0] - t * vx).hypot(point[1] - a[1] - t * vy);
        if distance <= tolerance && nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, handle));
        }
    }
    if nearest.is_none() && text.box_height.is_none() {
        if let Some(hit) = document
            .selected_vector_box()
            .and_then(|corners| box_hit_corners(corners, point))
            .filter(|hit| !hit.rotate)
        {
            nearest = Some((
                0.0,
                (
                    (hit.handle[0] * 2.0 - 1.0) as i8,
                    (hit.handle[1] * 2.0 - 1.0) as i8,
                ),
            ));
        }
    }
    nearest.map(|(_, handle)| (settings, handle))
}

fn text_object_linear(document: &Document, id: Option<&str>) -> [f32; 4] {
    document
        .svg_layers()
        .flat_map(|layer| &layer.vector_objects)
        .find(|object| Some(object.id.as_str()) == id)
        .map_or([1., 0., 0., 1.], |object| {
            [
                object.transform[0],
                object.transform[1],
                object.transform[2],
                object.transform[3],
            ]
        })
}

fn text_frame_world_point(settings: &TextSettings, linear: [f32; 4], point: [f32; 2]) -> [f32; 2] {
    let local = text_frame_point(settings, point);
    let [x, y] = settings.position;
    let [a, b, c, d] = linear;
    [
        x + a * (local[0] - x) + c * (local[1] - y),
        y + b * (local[0] - x) + d * (local[1] - y),
    ]
}

fn text_frame_height(text: &lumapaint_core::vector::VectorText) -> f32 {
    text.box_height.unwrap_or_else(|| {
        text.layout_bounds
            .map_or(
                text.visual_lines().len() as f32 * text.font_size * text.line_height,
                |bounds| bounds[3],
            )
            .max(16.0)
    })
}

fn text_frame_needs_reflow(original: &TextSettings, resized: &TextSettings) -> bool {
    original.text.box_width != resized.text.box_width
        || (original.text.writing_mode == lumapaint_core::vector::WritingMode::Vertical
            && original.text.box_height != resized.text.box_height)
}

fn resized_text_settings(draft: &TextResizeDraft) -> TextSettings {
    let mut settings = draft.original.clone();
    let text = &settings.text;
    let angle = text.rotation.to_radians();
    let world_dx = draft.current[0] - draft.start[0];
    let world_dy = draft.current[1] - draft.start[1];
    let [a, b, c, d] = draft.linear;
    let determinant = a * d - b * c;
    if determinant.abs() < 1e-8 {
        return settings;
    }
    let dx = (d * world_dx - c * world_dy) / determinant;
    let dy = (-b * world_dx + a * world_dy) / determinant;
    let local_dx = (dx * angle.cos() + dy * angle.sin()) / text.scale_x;
    let local_dy = (-dx * angle.sin() + dy * angle.cos()) / text.scale_y;
    let old_width = text.box_width;
    let old_height = text_frame_height(text);
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
    settings.position = text_frame_world_point(&draft.original, draft.linear, [left, top]);
    settings.text.box_width = right - left;
    settings.text.box_height = Some(bottom - top);
    if text_frame_needs_reflow(&draft.original, &settings) {
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
    let (box_width, box_height) = if TOOL.with(|tool| tool.get()) == CanvasTool::TextFrameVertical
        && (start[0] - end[0]).abs() < 3.0
        && (start[1] - end[1]).abs() < 3.0
    {
        (box_height, box_width)
    } else {
        (box_width, box_height)
    };
    let settings = TextSettings {
        id: None,
        text: lumapaint_core::vector::VectorText {
            content: String::new(),
            box_width,
            box_height: Some(box_height),
            writing_mode: if TOOL.with(|tool| tool.get()) == CanvasTool::TextFrameVertical {
                lumapaint_core::vector::WritingMode::Vertical
            } else {
                lumapaint_core::vector::WritingMode::Horizontal
            },
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
        baseline: None,
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
        baseline: None,
    }
}

fn selected_text_frame_overlay(document: &Document) -> Option<lumapaint_renderer::FrameOverlay> {
    let fallback = if TOOL.with(|t| t.get()) == CanvasTool::VectorSelect {
        document
            .selected_vector_box()
            .map(|corners| lumapaint_renderer::FrameOverlay {
                corners,
                handles: true,
                baseline: None,
            })
    } else {
        None
    };
    if document.selected_vector_ids().len() != 1 {
        return fallback;
    }
    let id = document.selected_vector_ids().first()?;
    let object = document
        .svg_layers()
        .filter(|layer| layer.visible && !layer.locked && layer.vector_layer)
        .flat_map(|layer| &layer.vector_objects)
        .find(|object| &object.id == id && object.visible)?;
    let Some(text) = object.text.as_ref() else {
        return fallback;
    };
    if text.point_text {
        let corners = document.selected_vector_box()?;
        let bounds = text
            .layout_bounds
            .unwrap_or([0., 0., text.box_width, text.font_size]);
        let baseline = text
            .line_baselines
            .first()
            .copied()
            .unwrap_or(text.font_size);
        let points = if text.writing_mode == lumapaint_core::vector::WritingMode::Vertical {
            [
                [text.box_width - baseline, bounds[1]],
                [text.box_width - baseline, bounds[1] + bounds[3]],
            ]
        } else {
            [[bounds[0], baseline], [bounds[0] + bounds[2], baseline]]
        };
        let angle = text.rotation.to_radians();
        return Some(lumapaint_renderer::FrameOverlay {
            corners,
            handles: true,
            baseline: Some(points.map(|[x, y]| {
                let (x, y) = (x * text.scale_x, y * text.scale_y);
                lumapaint_core::bezier::world_point(
                    object,
                    [
                        x * angle.cos() - y * angle.sin(),
                        x * angle.sin() + y * angle.cos(),
                    ],
                )
            })),
        });
    }
    if fallback.is_some() {
        return fallback;
    }

    if object.bounds_reset {
        return document
            .selected_vector_box()
            .map(|corners| lumapaint_renderer::FrameOverlay {
                corners,
                handles: false,
                baseline: None,
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
        baseline: None,
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
    let guides = GUIDE_DRAFT.with(|g| g.borrow_mut().take().is_some())
        | GUIDE_DRAG.with(|g| g.borrow_mut().take().is_some());
    GUIDE_OBJECT_DRAFT.with(|g| g.borrow_mut().clear());
    end_precise_paint_input();
    let gradient = gradient_tool::cancel();
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
        || guides
        || gradient
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
        let _ = app.emit_to(current_label(), "canvas-error", error);
    }
}
fn emit_document() {
    checkpoint();
    if ACTIVE_TILED_DOCUMENT.with(|document| document.borrow().is_none()) {
        if let Some(app) = APP.get() {
            let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
            let _ = app.emit_to(current_label(), "document-changed", snapshot);
        }
    }
    emit_workspace();
    window_sessions::shared_changed();
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
    if matches!(
        action,
        DocumentAction::LockSelection
            | DocumentAction::LockArtworkAbove
            | DocumentAction::LockOtherLayers
            | DocumentAction::UnlockAllObjects
            | DocumentAction::HideSelection
            | DocumentAction::HideArtworkAbove
            | DocumentAction::HideOtherLayers
            | DocumentAction::ShowAllObjects
    ) {
        finish_open_pen()?;
        cancel_vector_drag();
        if matches!(
            action,
            DocumentAction::LockSelection
                | DocumentAction::UnlockAllObjects
                | DocumentAction::HideSelection
                | DocumentAction::HideArtworkAbove
                | DocumentAction::HideOtherLayers
                | DocumentAction::ShowAllObjects
        ) {
            DIRECT_POINTS.with(|points| points.borrow_mut().clear());
        }
    }
    if matches!(
        action,
        DocumentAction::SelectAll | DocumentAction::Deselect | DocumentAction::InvertSelection
    ) {
        DIRECT_POINTS.with(|p| p.borrow_mut().clear());
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
            DocumentAction::CreateTrimMarks => doc.create_trim_marks()?,
            DocumentAction::RegistrationFill | DocumentAction::RegistrationStroke => {
                let ids = doc.selected_vector_ids().to_vec();
                doc.set_selected_vector_paint(
                    &ids,
                    if matches!(action, DocumentAction::RegistrationFill) {
                        "registrationFill"
                    } else {
                        "registrationStroke"
                    },
                    Some([0, 0, 0]),
                )?;
            }

            DocumentAction::Undo => doc.undo(),
            DocumentAction::Redo => doc.redo(),
            DocumentAction::ToggleLayer => {
                let snapshot = doc.layer_groups_snapshot();
                if let Some(group) = snapshot
                    .groups
                    .iter()
                    .find(|g| snapshot.selected.contains(&g.id))
                {
                    doc.edit_layer_groups(lumapaint_core::document::LayerGroupEdit {
                        action: "visibility".into(),
                        id: Some(group.id.clone()),
                        target: None,
                        name: None,
                        ids: vec![],
                        additive: false,
                        mask: None,
                    })?;
                } else {
                    doc.toggle_visibility();
                }
            }
            DocumentAction::SelectAll => {
                if vector_selection_context() {
                    doc.vector_selection_action(
                        lumapaint_core::document::VectorSelectionRequest {
                            action: "all".into(),
                            criterion: None,
                            name: None,
                            new_name: None,
                        },
                    )?;
                } else {
                    doc.select_all();
                }
            }
            DocumentAction::Deselect => {
                if !doc.selected_vector_ids().is_empty() {
                    doc.vector_selection_action(
                        lumapaint_core::document::VectorSelectionRequest {
                            action: "deselect".into(),
                            criterion: None,
                            name: None,
                            new_name: None,
                        },
                    )?;
                }
                doc.deselect();
            }
            DocumentAction::InvertSelection => {
                if vector_selection_context() {
                    doc.vector_selection_action(
                        lumapaint_core::document::VectorSelectionRequest {
                            action: "invert".into(),
                            criterion: None,
                            name: None,
                            new_name: None,
                        },
                    )?;
                } else {
                    doc.invert_selection()?;
                }
            }
            DocumentAction::ClearLayer => doc.clear_selected_layer()?,
            DocumentAction::LockSelection => {
                doc.lock_objects(lumapaint_core::document::ObjectLockAction::Selection)?
            }
            DocumentAction::LockArtworkAbove => {
                doc.lock_objects(lumapaint_core::document::ObjectLockAction::ArtworkAbove)?
            }
            DocumentAction::LockOtherLayers => {
                doc.lock_objects(lumapaint_core::document::ObjectLockAction::OtherLayers)?
            }
            DocumentAction::UnlockAllObjects => {
                doc.lock_objects(lumapaint_core::document::ObjectLockAction::UnlockAll)?
            }
            DocumentAction::HideSelection => {
                doc.hide_objects(lumapaint_core::document::ObjectVisibilityAction::Selection)?
            }
            DocumentAction::HideArtworkAbove => {
                doc.hide_objects(lumapaint_core::document::ObjectVisibilityAction::ArtworkAbove)?
            }
            DocumentAction::HideOtherLayers => {
                doc.hide_objects(lumapaint_core::document::ObjectVisibilityAction::OtherLayers)?
            }
            DocumentAction::ShowAllObjects => {
                doc.hide_objects(lumapaint_core::document::ObjectVisibilityAction::ShowAll)?
            }
            DocumentAction::Copy | DocumentAction::Cut | DocumentAction::Paste => unreachable!(),
            DocumentAction::DeleteSelectedObjects => {
                if !doc.guides().selected.is_empty() {
                    doc.delete_selected_vector_objects()?;
                } else if TOOL.with(|tool| tool.get()) == CanvasTool::VectorDirectSelect {
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
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        ensure_tiled_open()?;
        ACTIVE_TILED_DOCUMENT.with(|d| -> Result<(), String> {
            let mut slot = d.borrow_mut();
            let session = slot.as_mut().ok_or("Missing tiled document")?;
            let layer = session
                .document
                .layers()
                .iter()
                .find(|l| l.id == id)
                .ok_or("Unknown raster layer")?;
            let (visible, opacity) = (!layer.visible, layer.opacity);
            session
                .document
                .set_layer_appearance(&id, visible, opacity)?;
            Ok(())
        })?;
        redraw()?;
        emit_document();
        return tiled_snapshot();
    }
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().toggle_layer(&id))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

fn ensure_tiled_open() -> Result<(), String> {
    if DOCUMENT_OPEN.with(|open| open.get())
        && ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some())
        && !raster_import::active()
    {
        Ok(())
    } else {
        Err(
            "No editable tiled document / 編集可能なタイル文書がありません / 没有可编辑的瓦片文档"
                .into(),
        )
    }
}
fn tiled_snapshot() -> Result<DocumentSnapshot, String> {
    ACTIVE_TILED_DOCUMENT.with(|d| {
        d.borrow()
            .as_ref()
            .map(TiledSession::snapshot)
            .ok_or_else(|| "Missing tiled document".into())
    })
}
pub fn set_layer_effects(
    id: String,
    effects: lumapaint_core::layer_effects::LayerEffects,
) -> Result<DocumentSnapshot, String> {
    if !ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        ensure_document_open()?;
        DOCUMENT.with(|d| d.borrow_mut().set_layer_effects(&id, effects))?;
        redraw()?;
        emit_document();
        return Ok(DOCUMENT.with(|d| d.borrow().snapshot()));
    }
    ensure_tiled_open()?;
    ACTIVE_TILED_DOCUMENT.with(|d| -> Result<(), String> {
        d.borrow_mut()
            .as_mut()
            .ok_or("Missing tiled document")?
            .document
            .set_layer_effects(&id, effects)?;
        Ok(())
    })?;
    redraw()?;
    emit_document();
    tiled_snapshot()
}

pub fn set_raster_blend_mode(
    id: String,
    mode: lumapaint_core::tiles::RasterBlendMode,
) -> Result<DocumentSnapshot, String> {
    ensure_tiled_open()?;
    ACTIVE_TILED_DOCUMENT.with(|d| -> Result<(), String> {
        d.borrow_mut()
            .as_mut()
            .ok_or("No tiled document")?
            .document
            .set_layer_blend_mode(&id, mode)?;
        Ok(())
    })?;
    redraw()?;
    emit_document();
    tiled_snapshot()
}

pub fn set_layer_settings(settings: LayerSettings) -> Result<DocumentSnapshot, String> {
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        ensure_tiled_open()?;
        ACTIVE_TILED_DOCUMENT.with(|d| -> Result<(), String> {
            d.borrow_mut()
                .as_mut()
                .ok_or("Missing tiled document")?
                .document
                .set_layer_settings(settings)?;
            Ok(())
        })?;
        redraw()?;
        emit_document();
        return tiled_snapshot();
    }

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
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        ensure_tiled_open()?;
        ACTIVE_TILED_DOCUMENT.with(|d| -> Result<(), String> {
            let mut d = d.borrow_mut();
            let session = d.as_mut().ok_or("Missing tiled document")?;
            if !session.document.layers().iter().any(|l| l.id == id) {
                return Err("Unknown raster layer".into());
            }
            session.selected_layer = Some(id);
            Ok(())
        })?;
        emit_document();
        return tiled_snapshot();
    }

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
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return select_layer(id);
    }
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
    use lumapaint_core::vector::PathfinderOperation as Op;
    pathfinder_vectors(match operation {
        PathOperation::Union => Op::Unite,
        PathOperation::Difference => Op::MinusFront,
        PathOperation::Intersection => Op::Intersect,
        PathOperation::Xor => Op::Exclude,
    })
}
pub fn pathfinder_vectors(
    operation: lumapaint_core::vector::PathfinderOperation,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        doc.borrow_mut()
            .pathfinder_selected_vectors(operation, lumapaint_renderer::vector::pathfinder::compute)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn make_compound_shape(
    operation: lumapaint_core::vector::PathfinderOperation,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        doc.borrow_mut()
            .make_compound_shape(operation, lumapaint_renderer::vector::pathfinder::compute)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn edit_compound_shape(
    edit: lumapaint_core::document::CompoundShapeEdit,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| {
        doc.borrow_mut()
            .edit_compound_shape(edit, lumapaint_renderer::vector::pathfinder::compute)
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
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        ensure_tiled_open()?;
        ACTIVE_TILED_DOCUMENT.with(|d| -> Result<(), String> {
            d.borrow_mut()
                .as_mut()
                .ok_or("Missing tiled document")?
                .document
                .reorder_layers(&ids)?;
            Ok(())
        })?;
        redraw()?;
        emit_document();
        return tiled_snapshot();
    }
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
        .add_filter(
            "SVG / PDF / AI / Images",
            &["svg", "pdf", "ai", "png", "jpg", "jpeg", "webp"],
        )
        .pick_file()
    else {
        return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
    };
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf") || e.eq_ignore_ascii_case("ai"))
    {
        if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
            return Err("PDF layer import is not available for tiled documents".into());
        }
        crate::pdf_import::prepare(
            APP.get().ok_or("Application unavailable")?.clone(),
            current_label(),
            path,
            Some(ACTIVE_DOCUMENT_ID.with(|id| id.get())),
        );
        return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
    }
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

// AppKit coalesces setNeedsDisplay requests into its layer update cycle.
// Model/input events still run in full; only the latest preview is rendered.
thread_local! {
    static FRAME_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn request_redraw() -> Result<(), String> {
    let view = CANVAS.with(|slot| slot.borrow().as_ref().map(|canvas| canvas.view.clone()));
    if let Some(view) = view {
        FRAME_REQUESTED.with(|requested| requested.set(true));
        view.request_frame();
    }
    Ok(())
}

fn redraw() -> Result<(), String> {
    FRAME_REQUESTED.with(|requested| requested.set(false));
    text_editor::prepare_frame();
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            render_canvas(canvas)
        } else {
            Ok(())
        }
    })
}

fn ruler_viewport(viewport: Viewport) -> super::RulerViewport {
    let width = viewport.width as f32 / viewport.scale;
    let height = viewport.height as f32 / viewport.scale;
    let zoom = viewport.screen_zoom();
    super::RulerViewport {
        ruler_origin: DOCUMENT.with(|d| d.borrow().guides().origin),
        width,
        height,
        origin_x: width * 0.5 + viewport.pan_x - viewport.document_width * 0.5 * zoom,
        origin_y: height * 0.5 + viewport.pan_y - viewport.document_height * 0.5 * zoom,
        zoom,
    }
}
thread_local! {
    static RULER_VIEWPORTS: RefCell<std::collections::HashMap<String, super::RulerViewport>> = RefCell::new(std::collections::HashMap::new());
}
fn emit_ruler_viewport(viewport: Viewport) {
    let value = ruler_viewport(viewport);
    let label = current_label();
    let changed = RULER_VIEWPORTS
        .with(|values| values.borrow_mut().insert(label.clone(), value) != Some(value));
    if changed {
        if let Some(app) = APP.get() {
            let _ = app.emit_to(label, "canvas-ruler-changed", value);
        }
    }
}

fn render_canvas(canvas: &mut Canvas) -> Result<(), String> {
    let started = Instant::now();
    let draft = GUIDE_DRAFT
        .with(|g| g.borrow().clone())
        .map(|(axis, position)| {
            let v = canvas.viewport;
            let a = v.document_point(0., 0.);
            let b = v.document_point(v.width as f32 / v.scale, v.height as f32 / v.scale);
            let (p, q) = if axis == "horizontal" {
                ([a.x, position], [b.x, position])
            } else {
                ([position, a.y], [position, b.y])
            };
            lumapaint_renderer::FrameOverlay {
                corners: [p, q, q, p],
                handles: false,
                baseline: None,
            }
        });
    canvas.renderer.set_guide_drafts(
        draft
            .into_iter()
            .chain(GUIDE_OBJECT_DRAFT.with(|g| g.borrow().clone()))
            .collect(),
    );
    let mode = if text_editor::active() {
        "inline-text"
    } else if VECTOR_MOVE.with(|offset| offset.get()) != [0.0, 0.0] {
        "vector-drag"
    } else {
        "committed"
    };
    update_page_preview(canvas)?;
    let result = render_canvas_inner(canvas);
    emit_ruler_viewport(canvas.viewport);
    if render_metrics_enabled() && !canvas.view.isHidden() {
        eprintln!(
            "LumaPaint render mode={mode} main_ms={:.2}",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    result
}

fn render_canvas_inner(canvas: &mut Canvas) -> Result<(), String> {
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_none()) {
        let (width, height) = DOCUMENT.with(|d| d.borrow().dimensions());
        if canvas.viewport.document_width != width as f32
            || canvas.viewport.document_height != height as f32
        {
            let zoom = canvas.viewport.screen_zoom();
            canvas.viewport.document_width = width as f32;
            canvas.viewport.document_height = height as f32;
            canvas.viewport = canvas.viewport.with_screen_zoom(zoom);
        }
    }
    canvas
        .renderer
        .set_outline_view(OUTLINE_VIEW.with(|state| state.get()));
    // Modal edits are rendered once, with their final state, when the view resumes.
    if canvas.view.isHidden() {
        return Ok(());
    }
    canvas.renderer.set_image_frame_guides_visible(true);
    canvas.renderer.set_selection_overlay_visible(true);
    canvas.renderer.set_frame_overlay(None);
    if let Some(result) = TOOL_WINDOW_PREVIEWS.with(|previews| {
        let previews = previews.borrow();
        let preview = previews.get(&current_label())?;
        if preview.document_id != ACTIVE_DOCUMENT_ID.with(|id| id.get())
            || preview.revision != DOCUMENT.with(|doc| doc.borrow().revision())
        {
            return None;
        }
        let mut viewport = preview.zoom.map_or(canvas.viewport, |zoom| {
            canvas.viewport.with_screen_zoom(zoom)
        });
        let screen_zoom = viewport.screen_zoom();
        let (width, height) = preview.document.dimensions();
        viewport.document_width = width as f32;
        viewport.document_height = height as f32;
        viewport = viewport.with_screen_zoom(screen_zoom);
        Some(canvas.renderer.render(viewport, &preview.document))
    }) {
        return result;
    }

    if let Some(result) = selection_tools::render(canvas) {
        return result;
    }
    if let Some(result) = crop_tool::render(canvas) {
        return result;
    }
    if let Some(result) = gradient_tool::render(canvas) {
        return result;
    }
    if let Some(result) = raster_import::render(canvas) {
        return result;
    }
    if let Some(prepared) = PIXEL_PAINT_COMMIT.with(|pending| pending.borrow_mut().take()) {
        canvas.renderer.install_prepared_svg(prepared)?;
    }
    if let Some(result) = clone_stamp_tool::render(canvas) {
        return result;
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
        preview.resize_selected_image_frames(d.matrix, d.rotate || d.scale_content)?;
        canvas
            .renderer
            .set_frame_overlay(preview.selected_vector_box().map(|corners| {
                lumapaint_renderer::FrameOverlay {
                    corners,
                    handles: true,
                    baseline: None,
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
                    baseline: None,
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
        if text_frame_needs_reflow(&draft.original, &settings) {
            text_editor::reflow(&mut settings)?;
        }
        let mut preview = DOCUMENT.with(|document| document.borrow().clone());
        preview.set_text_object(settings.clone())?;
        canvas
            .renderer
            .set_frame_overlay(text_frame_overlay(&settings, true).map(|mut overlay| {
                let [a, b, c, d] = draft.linear;
                let [x, y] = settings.position;
                for point in overlay
                    .corners
                    .iter_mut()
                    .chain(overlay.baseline.iter_mut().flatten())
                {
                    let dx = point[0] - x;
                    let dy = point[1] - y;
                    *point = [x + a * dx + c * dy, y + b * dx + d * dy];
                }
                overlay
            }));
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
        CanvasTool::VectorSelect
            | CanvasTool::Text
            | CanvasTool::TextVertical
            | CanvasTool::TextFrame
            | CanvasTool::TextFrameVertical
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
        match sender.try_send(canvas.token, job) {
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
        && canvas.tile_cache.as_ref().is_some_and(|cache| {
            cache.state.stroke_count == state.stroke_count && cache.state.same_effects(state)
        })
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
        match sender.try_send(canvas.token, job) {
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
        match sender.try_send(canvas.token, job) {
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
        .try_send(
            canvas.token,
            TileJob {
                key,
                work: TileWork::Full(Box::new(document.clone())),
                state,
                dimensions: document.dimensions(),
                queued_at: Instant::now(),
            },
        )
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
    match sender.try_send(
        canvas.token,
        RasterJob {
            key,
            layers,
            size: document.dimensions(),
            queued_at: Instant::now(),
        },
    ) {
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
                // Shared Metal textures may have completed on the UI thread while
                // this compatibility job was running. Do not upload duplicate pixels.
                let missing: std::collections::HashSet<_> = DOCUMENT.with(|document| {
                    canvas
                        .renderer
                        .missing_svg_layers(&document.borrow())
                        .into_iter()
                        .map(|layer| layer.id)
                        .collect()
                });
                for layer in layers {
                    if !missing.contains(&layer.id) {
                        continue;
                    }
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
    zoom_revision: Option<u64>,
    token: u64,
    raster_job: Option<RasterKey>,
    raster_failure: Option<RasterKey>,
    pending_vector_commit: Option<RasterKey>,
    tile_job: Option<TileKey>,
    tile_ready: Option<TileKey>,
    tile_failure: Option<TileKey>,
    tile_cache: Option<TileCache>,
    active_tiled_key: Option<(u64, u64)>,
    page_preview_key: Option<String>,
}

impl Drop for Canvas {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

thread_local! {
    static CANVAS_OVERLAY: RefCell<Vec<[f64;4]>> = const { RefCell::new(Vec::new()) };
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
    Shared(u64),
    Legacy(Box<Document>),
    Tiled(TiledSession),
}

struct TiledSession {
    selected_layer: Option<String>,
    document: TiledRasterDocument,
    file_name: Option<String>,
    saved_revision: Option<u64>,
    name: String,
}

impl TiledSession {
    fn dirty(&self) -> bool {
        Some(self.document.revision()) != self.saved_revision
    }

    fn snapshot(&self) -> DocumentSnapshot {
        let mut snapshot = Document::default().snapshot();
        let (width, height) = self.document.dimensions();
        snapshot.name = self.file_name.clone().unwrap_or_else(|| self.name.clone());
        snapshot.file_name = self.file_name.clone();
        snapshot.width = width;
        snapshot.height = height;
        snapshot.raster_resolution = self.document.resolution();
        snapshot.layer_id = self
            .selected_layer
            .clone()
            .or_else(|| {
                self.document
                    .layers()
                    .iter()
                    .rev()
                    .find(|l| l.visible)
                    .map(|l| l.id.clone())
            })
            .or_else(|| self.document.layers().last().map(|l| l.id.clone()))
            .unwrap_or_default();
        snapshot.layer_visible = self.document.layers().iter().any(|layer| layer.visible);
        snapshot.layers = self
            .document
            .layers()
            .iter()
            .map(|layer| LayerSnapshot {
                effects: Some(layer.effects.clone()),
                raster_blend_mode: Some(layer.blend_mode),
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
            Self::Shared(id) => {
                window_sessions::shared_tab(*id).is_some_and(|tab| tab.dirty)
                    && !window_sessions::shared_has_other_views(*id)
            }
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
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    if window_sessions::park_shared_active(id) {
        INACTIVE_DOCUMENTS.with(|documents| {
            documents.borrow_mut().push(OpenDocument {
                id,
                content: OpenDocumentContent::Shared(id),
                path: None,
                fingerprint: None,
            })
        });
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
    let entry = if matches!(entry.content, OpenDocumentContent::Shared(_)) {
        window_sessions::take_shared(entry.id)
    } else {
        entry
    };
    cancel_vector_drag();
    let (width, height, canvas_color) = match &entry.content {
        OpenDocumentContent::Shared(_) => unreachable!("shared content was resolved"),
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
        OpenDocumentContent::Shared(_) => unreachable!("shared content was resolved"),
        OpenDocumentContent::Legacy(mut document) => {
            lumapaint_svg::attach(&mut document);
            document.initialize_layer_selection();
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
        OpenDocumentContent::Shared(shared_id) => {
            window_sessions::shared_tab(*shared_id).expect("shared tab exists")
        }
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
            file_name: document
                .file_name
                .clone()
                .or_else(|| Some(document.name.clone())),
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
        let snapshot = workspace_snapshot();
        let label = current_label();
        if let Some(window) = app.get_webview_window(&label) {
            let title = snapshot.active.as_ref().map_or_else(
                || {
                    if label == "main" {
                        "LumaPaint".into()
                    } else {
                        format!(
                            "LumaPaint — {}",
                            label
                                .strip_prefix("editor-")
                                .and_then(|id| id.parse::<u64>().ok())
                                .unwrap_or(0)
                                + 1
                        )
                    }
                },
                |document| {
                    format!(
                        "{}{} — LumaPaint",
                        document.name,
                        if document.dirty { " *" } else { "" }
                    )
                },
            );
            WINDOW_TITLE.with(|last| {
                if *last.borrow() != title {
                    let _ = window.set_title(&title);
                    *last.borrow_mut() = title;
                }
            });
        }
        let _ = app.emit_to(label, "documents-changed", snapshot);
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
    let size = if TOOL.with(|tool| tool.get()) == CanvasTool::SelectionBrush {
        selection_tools::settings().diameter
    } else {
        BRUSH.with(|brush| brush.borrow().size)
    };
    let diameter = (size * fit).max(1.);
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
    if tool_changed {
        crop_tool::cancel();
    }
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
    let mut base = Viewport::new(
        frame.size.width,
        frame.size.height,
        scale,
        1.0,
        request.dark,
    )?
    .with_document(document.width, document.height, document.canvas_color)?;
    base.pasteboard_color = request.pasteboard_color;
    let requested_zoom = if request.absolute_zoom {
        if request.zoom == 0.0 {
            spread_fit(base).0 as f64
        } else {
            request.zoom
        }
    } else {
        request.zoom * base.fit_zoom() as f64
    };
    let (zoom, explicit, previous_zoom) = CANVAS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map_or((requested_zoom, true, requested_zoom), |canvas| {
                (
                    synchronized_zoom(
                        canvas.viewport.screen_zoom(),
                        canvas.zoom_revision,
                        requested_zoom,
                        request.zoom_revision,
                    ),
                    request.zoom_revision.is_none() || request.zoom_revision > canvas.zoom_revision,
                    canvas.viewport.screen_zoom() as f64,
                )
            })
    });
    let (pan_x, pan_y) = if explicit && request.absolute_zoom && request.zoom == 0.0 {
        let (_, x, y) = spread_fit(base);
        (x, y)
    } else if explicit {
        let ratio = (zoom / previous_zoom) as f32;
        (
            (pan_x * ratio).clamp(
                -lumapaint_renderer::MAX_CANVAS_PAN,
                lumapaint_renderer::MAX_CANVAS_PAN,
            ),
            (pan_y * ratio).clamp(
                -lumapaint_renderer::MAX_CANVAS_PAN,
                lumapaint_renderer::MAX_CANVAS_PAN,
            ),
        )
    } else {
        (pan_x, pan_y)
    };
    PAN.with(|pan| pan.set((pan_x, pan_y)));
    let viewport = base.with_screen_zoom(zoom as f32).with_pan(pan_x, pan_y)?;

    let result = CANVAS.with(|slot| {
        let mut slot = slot.borrow_mut();
        // A replaced webview must not reuse a surface attached to the previous parent.
        if slot.as_ref().is_some_and(|canvas| {
            // SAFETY: both native views are live and accessed on the main thread.
            unsafe { canvas.view.superview() }
                .as_deref()
                .is_none_or(|view| !std::ptr::eq(view, parent))
        }) {
            if let Some(canvas) = slot.take() {
                canvas.view.stop_frames();
            }
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
                        zoom_revision: request.zoom_revision,
                        token: NEXT_CANVAS_TOKEN.fetch_add(1, Ordering::Relaxed),
                        raster_job: None,
                        raster_failure: None,
                        pending_vector_commit: None,
                        tile_job: None,
                        tile_ready: None,
                        tile_failure: None,
                        tile_cache: None,
                        active_tiled_key: None,
                        page_preview_key: None,
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
        let mut overlays = request.overlays.clone();
        overlays.extend(request.overlay);
        update_canvas_mask(&canvas.view, overlays);
        canvas.viewport = viewport;
        if request.zoom_revision > canvas.zoom_revision {
            canvas.zoom_revision = request.zoom_revision;
        }
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
            zoom: Some(viewport.screen_zoom()),
            ruler_viewport: Some(ruler_viewport(viewport)),
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

// Subtract the union of floating panels; overlapping panels never expose the canvas.
fn visible_canvas_regions(bounds: [f64; 4], holes: &[[f64; 4]]) -> Vec<[f64; 4]> {
    let mut regions = vec![bounds];
    for h in holes {
        regions = regions
            .into_iter()
            .flat_map(|r| {
                let l = r[0].max(h[0]);
                let t = r[1].max(h[1]);
                let right = r[2].min(h[2]);
                let b = r[3].min(h[3]);
                if l >= right || t >= b {
                    return vec![r];
                }
                [
                    [r[0], r[1], r[2], t],
                    [r[0], b, r[2], r[3]],
                    [r[0], t, l, b],
                    [right, t, r[2], b],
                ]
                .into_iter()
                .filter(|v| v[0] < v[2] && v[1] < v[3])
                .collect()
            })
            .collect();
    }
    regions
}

// Punch a hole only in the native view's composition, leaving the GPU viewport fixed.
// The WebView below receives pointer events in this same rectangle.
fn update_canvas_mask(view: &PaintView, overlays: Vec<[f64; 4]>) {
    CANVAS_OVERLAY.with(|value| *value.borrow_mut() = overlays.clone());
    // SAFETY: all Core Animation objects are retained and accessed on the main thread.
    unsafe {
        let layer: Option<Retained<AnyObject>> = msg_send![view, layer];
        let Some(layer) = layer else { return };
        let _: () = msg_send![class!(CATransaction), begin];
        let _: () = msg_send![class!(CATransaction), setDisableActions: true];
        if !overlays.is_empty() {
            let size = view.bounds().size;
            let mask: Retained<AnyObject> = msg_send![class!(CALayer), layer];
            let _: () = msg_send![&*mask, setFrame: view.bounds()];
            let white = NSColor::whiteColor();
            let color = white.CGColor();
            for [x, y, right, bottom] in
                visible_canvas_regions([0., 0., size.width, size.height], &overlays)
            {
                let part: Retained<AnyObject> = msg_send![class!(CALayer), layer];
                let _: () = msg_send![&*part, setFrame: NSRect::new(NSPoint::new(x, y), NSSize::new(right-x, bottom-y))];
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
    fn tiled_layer_selection_blend_unlock_history_and_snapshot_stay_in_rust() {
        use lumapaint_core::tiles::RasterBlendMode;
        use lumapaint_formats::DocumentImporter;
        let mut document = TiledRasterDocument::new(1, 1).unwrap();
        document.add_layer("base".into(), "Base".into()).unwrap();
        document.add_layer("top".into(), "日本語".into()).unwrap();
        document
            .write_rect("base", [0, 0, 1, 1], &[255, 0, 0, 255])
            .unwrap();
        document
            .write_rect("top", [0, 0, 1, 1], &[0, 0, 255, 128])
            .unwrap();
        document.discard_history();
        let revision = document.revision();
        let previous = ACTIVE_TILED_DOCUMENT.with(|d| {
            d.replace(Some(TiledSession {
                selected_layer: None,
                document,
                name: "Test".into(),
                file_name: None,
                saved_revision: Some(revision),
            }))
        });
        let was_open = DOCUMENT_OPEN.with(|o| o.replace(true));
        struct Restore(Option<TiledSession>, bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                ACTIVE_TILED_DOCUMENT.with(|d| *d.borrow_mut() = self.0.take());
                DOCUMENT_OPEN.with(|o| o.set(self.1));
            }
        }
        let _restore = Restore(previous, was_open);
        assert!(ensure_document_open().is_err());
        assert!(ensure_tiled_open().is_ok());
        let selected = select_arrange_layer("base".into()).unwrap();
        assert_eq!(selected.layer_id, "base");
        assert!(!selected.dirty);
        assert!(select_layer("missing".into()).is_err());
        assert_eq!(tiled_snapshot().unwrap().layer_id, "base");
        select_layer("top".into()).unwrap();
        let changed = set_raster_blend_mode("top".into(), RasterBlendMode::Multiply).unwrap();
        assert!(changed.dirty && changed.can_undo);
        assert_eq!(
            changed.layers[1].raster_blend_mode,
            Some(RasterBlendMode::Multiply)
        );
        let saved_state =
            ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().as_ref().unwrap().document.state());
        let native = lumapaint_formats::tile_container::encode(&saved_state).unwrap();
        assert_eq!(
            lumapaint_formats::tile_container::decode(&native).unwrap(),
            saved_state
        );
        for format in [
            lumapaint_formats::export::FormatId::Psd,
            lumapaint_formats::export::FormatId::Psb,
        ] {
            let saved =
                lumapaint_formats::export::export_raster(format, &saved_state, Default::default())
                    .unwrap();
            let loaded = lumapaint_formats::psd::PsdImporter
                .import(&saved.bytes, Default::default())
                .unwrap();
            assert_eq!(
                loaded.raster.layers[1].blend_mode,
                RasterBlendMode::Multiply
            );
            assert_eq!(loaded.raster.layers[1].tiles, saved_state.layers[1].tiles);
        }
        let undone = edit(DocumentAction::Undo).unwrap();
        assert_eq!(
            undone.layers[1].raster_blend_mode,
            Some(RasterBlendMode::Normal)
        );
        assert!(undone.can_redo);
        let redone = edit(DocumentAction::Redo).unwrap();
        assert_eq!(
            redone.layers[1].raster_blend_mode,
            Some(RasterBlendMode::Multiply)
        );
        let settings = LayerSettings {
            id: "top".into(),
            name: "日本語".into(),
            opacity: 1.,
            locked: true,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
        };
        set_layer_settings(settings.clone()).unwrap();
        assert!(set_raster_blend_mode("top".into(), RasterBlendMode::Screen).is_err());
        set_layer_settings(LayerSettings {
            locked: false,
            ..settings
        })
        .unwrap();
        assert!(set_raster_blend_mode("top".into(), RasterBlendMode::Screen).is_ok());
        let hidden = toggle_layer("top".into()).unwrap();
        assert!(!hidden.layers[1].visible);
        assert_eq!(hidden.layer_id, "top");
        assert!(edit(DocumentAction::Undo).unwrap().layers[1].visible);
        assert!(!edit(DocumentAction::Redo).unwrap().layers[1].visible);
        let changed = set_layer_settings(LayerSettings {
            id: "top".into(),
            name: "背景 / Background / 背景".into(),
            opacity: 0.4,
            locked: true,
            alpha_locked: false,
            mask_enabled: false,
            mask_inverted: false,
            mask_density: 1.,
        })
        .unwrap();
        assert_eq!(changed.layers[1].opacity, 0.4);
        assert!(changed.layers[1].locked);
        assert_eq!(changed.layers[1].name, "背景 / Background / 背景");
        assert_eq!(edit(DocumentAction::Undo).unwrap().layers[1].opacity, 1.);
        assert_eq!(edit(DocumentAction::Redo).unwrap().layers[1].opacity, 0.4);
        // Visibility remains editable while locked, without changing pixel storage.
        assert!(toggle_layer("top".into()).unwrap().layers[1].visible);
        assert!(!toggle_layer("top".into()).unwrap().layers[1].visible);
        let state = ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().as_ref().unwrap().document.state());
        assert!(toggle_layer("unknown".into()).is_err());
        assert_eq!(
            state,
            ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().as_ref().unwrap().document.state())
        );
        let native = lumapaint_formats::tile_container::encode(&state).unwrap();
        assert_eq!(
            state,
            lumapaint_formats::tile_container::decode(&native).unwrap()
        );
        for format in [
            lumapaint_formats::export::FormatId::Psd,
            lumapaint_formats::export::FormatId::Psb,
        ] {
            let exported = lumapaint_formats::export::export_raster(
                format,
                &state,
                lumapaint_formats::export::ExportOptions { allow_lossy: true },
            )
            .unwrap();
            assert!(exported
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.protectionExpanded"));
            let loaded = lumapaint_formats::psd::PsdImporter
                .import(&exported.bytes, Default::default())
                .unwrap()
                .raster;
            assert!(!loaded.layers[1].visible);
            assert_eq!(loaded.layers[1].opacity, 0.4);
            assert_eq!(loaded.layers[1].name, state.layers[1].name);
            assert_eq!(loaded.layers[1].tiles, state.layers[1].tiles);
        }
        let moved = reorder_layers(vec!["base".into(), "top".into()]).unwrap();
        assert_eq!(moved.layers[0].id, "top");
        assert_eq!(moved.layer_id, "top");
        assert_eq!(edit(DocumentAction::Undo).unwrap().layers[0].id, "base");
        assert_eq!(edit(DocumentAction::Redo).unwrap().layers[0].id, "top");
        let reordered =
            ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().as_ref().unwrap().document.state());
        assert!(reorder_layers(vec!["base".into(), "base".into()]).is_err());
        assert_eq!(
            reordered,
            ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().as_ref().unwrap().document.state())
        );
        let saved = lumapaint_formats::tile_container::encode(&reordered).unwrap();
        assert_eq!(
            lumapaint_formats::tile_container::decode(&saved).unwrap(),
            reordered
        );
        for format in [
            lumapaint_formats::export::FormatId::Psd,
            lumapaint_formats::export::FormatId::Psb,
        ] {
            let saved = lumapaint_formats::export::export_raster(
                format,
                &reordered,
                lumapaint_formats::export::ExportOptions { allow_lossy: true },
            )
            .unwrap();
            let loaded = lumapaint_formats::psd::PsdImporter
                .import(&saved.bytes, Default::default())
                .unwrap()
                .raster;
            assert_eq!(loaded.layers.len(), 2);
            assert_eq!(
                loaded.layers.iter().map(|l| &l.name).collect::<Vec<_>>(),
                reordered.layers.iter().map(|l| &l.name).collect::<Vec<_>>()
            );
            assert_eq!(loaded.layers[0].tiles, reordered.layers[0].tiles);
        }
    }
    #[test]
    fn tiled_mask_panel_settings_disable_without_erasing_and_round_trip() {
        use lumapaint_formats::DocumentImporter;
        let mut document = TiledRasterDocument::new(2, 1).unwrap();
        document
            .add_layer("masked".into(), "マスク / Mask / 蒙版".into())
            .unwrap();
        document
            .write_rect("masked", [0, 0, 2, 1], &[0, 0, 255, 255, 0, 0, 255, 255])
            .unwrap();
        document
            .write_mask_rect("masked", [0, 0, 2, 1], &[0, 255])
            .unwrap();
        document.set_layer_mask("masked", true, false, 1.).unwrap();
        document.discard_history();
        let original = document.state();
        let revision = document.revision();
        let previous = ACTIVE_TILED_DOCUMENT.with(|d| {
            d.replace(Some(TiledSession {
                selected_layer: Some("masked".into()),
                document,
                file_name: None,
                name: "Mask test".into(),
                saved_revision: Some(revision),
            }))
        });
        let was_open = DOCUMENT_OPEN.with(|o| o.replace(true));
        struct Restore(Option<TiledSession>, bool);
        impl Drop for Restore {
            fn drop(&mut self) {
                ACTIVE_TILED_DOCUMENT.with(|d| *d.borrow_mut() = self.0.take());
                DOCUMENT_OPEN.with(|o| o.set(self.1));
            }
        }
        let _restore = Restore(previous, was_open);
        let settings = LayerSettings {
            id: "masked".into(),
            name: "マスク / Mask / 蒙版".into(),
            opacity: 1.,
            locked: false,
            alpha_locked: false,
            mask_enabled: true,
            mask_inverted: true,
            mask_density: 0.5,
        };
        let changed = set_layer_settings(settings.clone()).unwrap();
        assert!(changed.layers[0].mask_inverted);
        assert_eq!(changed.layers[0].mask_density, 0.5);
        let coord = lumapaint_core::tiles::TileCoord { x: 0, y: 0 };
        let pixels = ACTIVE_TILED_DOCUMENT.with(|d| {
            d.borrow()
                .as_ref()
                .unwrap()
                .document
                .composite_tile(coord)
                .unwrap()
        });
        assert_eq!(&pixels[..8], &[0, 0, 255, 255, 0, 0, 128, 128]);
        assert!(
            set_layer_settings(LayerSettings {
                mask_enabled: false,
                ..settings.clone()
            })
            .unwrap()
            .layers[0]
                .mask_inverted
        );
        let disabled =
            ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().as_ref().unwrap().document.state());
        assert!(!disabled.layers[0].mask_enabled);
        assert_eq!(disabled.layers[0].mask_tiles, original.layers[0].mask_tiles);
        assert_eq!(disabled.layers[0].tiles, original.layers[0].tiles);
        assert_eq!(
            edit(DocumentAction::Undo).unwrap().layers[0].mask_density,
            0.5
        );
        let restored = edit(DocumentAction::Undo).unwrap();
        assert_eq!(restored.layers[0].mask_density, 1.);
        assert!(!restored.layers[0].mask_inverted);
        assert!(!restored.can_undo);
        edit(DocumentAction::Redo).unwrap();
        edit(DocumentAction::Redo).unwrap();
        set_layer_settings(settings).unwrap();
        let state = ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().as_ref().unwrap().document.state());
        assert_eq!(state.layers[0].mask_tiles, original.layers[0].mask_tiles);
        let saved = lumapaint_formats::tile_container::encode(&state).unwrap();
        assert_eq!(
            lumapaint_formats::tile_container::decode(&saved).unwrap(),
            state
        );
        for format in [
            lumapaint_formats::export::FormatId::Psd,
            lumapaint_formats::export::FormatId::Psb,
        ] {
            let saved = lumapaint_formats::export::export_raster(
                format,
                &state,
                lumapaint_formats::export::ExportOptions { allow_lossy: true },
            )
            .unwrap();
            assert!(saved
                .report
                .issues
                .iter()
                .any(|i| i.code == "psd.nativeMaskDensityBaked"));
            let loaded = lumapaint_formats::psd::PsdImporter
                .import(&saved.bytes, Default::default())
                .unwrap()
                .raster;
            let loaded = TiledRasterDocument::from_state(loaded).unwrap();
            let actual = loaded.composite_tile(coord).unwrap();
            for (a, b) in pixels[..8].iter().zip(&actual[..8]) {
                assert!(a.abs_diff(*b) <= 1);
            }
            assert_eq!(loaded.state().layers[0].tiles, state.layers[0].tiles);
        }
    }

    #[test]
    fn corner_gesture_changes_one_corner_and_shift_changes_all_with_single_undo() {
        let mut document = Document::default();
        let layer = document.add_vector_layer().unwrap();
        let object = PenDraft {
            layer: layer.clone(),
            brush: Brush::default(),
            nodes: vec![
                ([20., 20.], [20., 20.]),
                ([200., 20.], [200., 20.]),
                ([100., 180.], [100., 180.]),
            ],
        }
        .object(true, "corner-test")
        .unwrap();
        document
            .upsert_vector_object(&layer, object.clone())
            .unwrap();
        document
            .select_vector_objects(vec![object.id.clone()])
            .unwrap();
        let before = document.encode().unwrap();
        let handle = lumapaint_core::bezier::corner_handles(&object, 8.)[0].clone();
        let draft = DirectGesture {
            corner: Some((object.id.clone(), handle.clone())),
            corner_all: false,
            start: handle.point,
            current: [
                handle.point[0] + handle.direction[0] * handle.factor * 10.,
                handle.point[1] + handle.direction[1] * handle.factor * 10.,
            ],
            marquee: false,
            baseline: vec![],
            break_smooth: false,
        };
        let mut preview = document.clone();
        apply_corner_gesture(&mut preview, &draft).unwrap();
        assert_eq!(document.encode().unwrap(), before);
        apply_corner_gesture(&mut document, &draft).unwrap();
        let object = document
            .direct_objects()
            .into_iter()
            .find(|(_, o)| o.id == "corner-test")
            .unwrap()
            .1;
        let radii = &object.live_corners.as_ref().unwrap().radii;
        assert_eq!(radii.iter().filter(|r| **r > 0.).count(), 1);
        document.undo();
        assert_eq!(document.encode().unwrap(), before);
        apply_corner_gesture(
            &mut document,
            &DirectGesture {
                corner_all: true,
                ..draft
            },
        )
        .unwrap();
        let object = document
            .direct_objects()
            .into_iter()
            .find(|(_, o)| o.id == "corner-test")
            .unwrap()
            .1;
        assert!(object
            .live_corners
            .as_ref()
            .unwrap()
            .radii
            .iter()
            .all(|r| (*r - 10.).abs() < 0.001));
        document.undo();
        assert_eq!(document.encode().unwrap(), before);
    }

    #[test]
    fn guide_selection_routes_shift_marquee_without_moving_pixel_layer() {
        let mut d = Document::default();
        assert!(selection_uses_pixel_move(&d, CanvasTool::VectorSelect));
        d.edit_guides(lumapaint_core::document::GuideEdit {
            action: "add".into(),
            id: None,
            axis: Some("vertical".into()),
            position: Some(40.),
            delta: None,
        })
        .unwrap();
        d.edit_guides(lumapaint_core::document::GuideEdit {
            action: "lock".into(),
            id: None,
            axis: None,
            position: None,
            delta: None,
        })
        .unwrap();
        d.select_guide(Some(d.guides().items[0].id.clone()));
        assert!(!selection_uses_pixel_move(&d, CanvasTool::VectorSelect));
        d.select_guide(None);
        assert!(selection_uses_pixel_move(&d, CanvasTool::VectorSelect));
    }

    #[test]
    fn facing_preview_renders_neighbor_content_and_clips_to_its_page() {
        use lumapaint_core::document::{PageBinding, PageEdit, Point};
        let mut doc = Document::default();
        let edit = |action: &str, index| PageEdit {
            action: action.into(),
            index,
            facing: None,
            binding: None,
        };
        doc.edit_pages(edit("add", None)).unwrap();
        doc.edit_pages(edit("select", Some(1))).unwrap();
        doc.import_svg("Neighbor".into(), r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect x="-10000" y="0" width="20000" height="100" fill="#ff0000"/></svg>"##.into()).unwrap();
        doc.edit_pages(edit("add", Some(1))).unwrap();
        doc.edit_pages(edit("select", Some(2))).unwrap();
        doc.edit_pages(PageEdit {
            action: "layout".into(),
            index: None,
            facing: Some(true),
            binding: Some(PageBinding::LeftToRight),
        })
        .unwrap();
        let before = doc.document_state();
        let preview = facing_preview_document(&doc).unwrap().unwrap();
        let (_, x, neighbor) = doc.facing_neighbor().unwrap();
        let mut sampler = lumapaint_renderer::color_sampler::ColorSampler::default();
        assert_eq!(
            sampler
                .sample(&preview, Point { x: x + 20., y: 20. })
                .unwrap(),
            Some([255, 0, 0])
        );
        assert_eq!(
            sampler
                .sample(&preview, Point { x: x - 20., y: 20. })
                .unwrap(),
            None
        );
        assert_eq!(
            sampler.sample(&preview, Point { x: 20., y: 20. }).unwrap(),
            Some([255, 255, 255])
        );
        assert_eq!(neighbor.pages_snapshot().pages.len(), 1);
        assert_eq!(
            serde_json::to_value(before).unwrap(),
            serde_json::to_value(doc.document_state()).unwrap()
        );
    }

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
        assert_eq!(
            pan_offset((8_388_606., -8_388_606.), (10., -10.)),
            (8_388_608., -8_388_608.)
        );
        assert_eq!(pan_offset((10., 20.), (f32::NAN, 1.)), (10., 20.));
        let first = pan_offset((0., 0.), (8., 4.));
        assert_eq!(pan_offset(first, (4., 2.)), (12., 6.));
    }

    #[test]
    fn pinch_zoom_in_out_limits_and_invalid_events() {
        assert!((pinch_zoom(1., 0.2) - 1.2).abs() < 0.0001);
        assert!((pinch_zoom(1., -0.2) - 0.8).abs() < 0.0001);
        assert_eq!(pinch_zoom(640., 1.), 640.);
        assert_eq!(pinch_zoom(0.0313, -0.9), 0.0313);
        assert_eq!(pinch_zoom(1., f32::NAN), 1.);
        assert_eq!(pinch_zoom(1., 0.), 1.);
        let viewport = Viewport::new(800., 500., 1., 1., false)
            .unwrap()
            .with_screen_zoom(640.);
        assert!((vector_sample_spacing(Some(viewport)) - 1. / 640.).abs() < 0.000001);
        assert_eq!(vector_sample_spacing(None), 1.);
    }

    #[test]
    fn delayed_ui_sync_preserves_pinch_scale_until_a_new_explicit_command() {
        assert_eq!(
            synchronized_zoom(1.8, Some(3), 1.0, Some(3)),
            1.8_f32 as f64
        );
        assert_eq!(
            synchronized_zoom(1.8, Some(3), 0.5, Some(2)),
            1.8_f32 as f64
        );
        assert_eq!(synchronized_zoom(1.8, Some(3), 2.0, Some(4)), 2.0);
        assert_eq!(synchronized_zoom(1.8, None, 1.0, Some(0)), 1.0);
        assert_eq!(synchronized_zoom(1.8, Some(3), 0.5, None), 0.5);
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
        let flags = NSEventModifierFlags::Command;
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
    fn text_bounding_drag_resizes_frame_without_scaling_glyphs() {
        TOOL.with(|tool| tool.set(CanvasTool::VectorSelect));
        let mut document = Document::default();
        document
            .set_text_object(TextSettings {
                id: None,
                text: lumapaint_core::vector::VectorText {
                    content: "文字 English 简体中文".into(),
                    box_width: 100.,
                    box_height: Some(50.),
                    ..Default::default()
                },
                position: [100., 100.],
                color: [0, 0, 0],
            })
            .unwrap();
        let original = document.snapshot().text_objects[0].text.clone();
        let start = lumapaint_core::document::Point { x: 200., y: 150. };
        vector_select_pointer(&mut document, start, 0, NSEventModifierFlags::empty()).unwrap();
        assert!(BOX_DRAFT.with(|draft| draft.borrow().is_none()));
        assert!(TEXT_RESIZE_DRAFT.with(|draft| draft.borrow().is_some()));
        vector_select_pointer(
            &mut document,
            lumapaint_core::document::Point { x: 300., y: 200. },
            1,
            NSEventModifierFlags::empty(),
        )
        .unwrap();
        let draft = TEXT_RESIZE_DRAFT.with(|draft| draft.borrow().clone().unwrap());
        let resized = resized_text_settings(&draft);
        assert_eq!(resized.text.box_width, 200.);
        assert_eq!(resized.text.box_height, Some(100.));
        assert_eq!(resized.text.font_size, original.font_size);
        assert_eq!(resized.text.scale_x, original.scale_x);
        assert_eq!(resized.text.scale_y, original.scale_y);
        assert_eq!(resized.text.runs, original.runs);
        assert_eq!(resized.text.content, original.content);
        assert!(cancel_vector_drag());
        assert_eq!(document.snapshot().text_objects[0].text, original);
        // A previously transformed object still resizes in its local frame units.
        let transformed = resized_text_settings(&TextResizeDraft {
            linear: [0., 2., -3., 0.],
            original: draft.original.clone(),
            start: [0., 0.],
            current: [-150., 200.],
            handle: (1, 1),
        });
        assert_eq!(transformed.text.box_width, 200.);
        assert_eq!(transformed.text.box_height, Some(100.));
        assert_eq!(transformed.text.font_size, original.font_size);
        TOOL.with(|tool| tool.set(CanvasTool::Brush));
    }

    #[test]
    fn bounding_math_keeps_rotated_anchor_and_supports_center_and_rotation() {
        let corners = [[10., 20.], [10., 120.], [-40., 120.], [-40., 20.]];
        let mut d = BoxDraft {
            corners,
            handle: [1., 1.],
            start: corners[2],
            matrix: [1., 0., 0., 1., 0., 0.],
            scale_content: false,
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
            linear: [1., 0., 0., 1.],
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
    fn point_text_selection_uses_measured_bounds_and_transformed_baseline() {
        for vertical in [false, true] {
            let mut document = Document::default();
            let layer = document.add_vector_layer().unwrap();
            document.select_layer(layer).unwrap();
            document
                .set_text_object(TextSettings {
                    id: None,
                    text: lumapaint_core::vector::VectorText {
                        point_text: true,
                        content: "あいうえお".into(),
                        writing_mode: if vertical {
                            lumapaint_core::vector::WritingMode::Vertical
                        } else {
                            lumapaint_core::vector::WritingMode::Horizontal
                        },
                        box_width: 60.,
                        font_size: 20.,
                        line_baselines: vec![20.],
                        layout_bounds: Some([0., 0., 100., 100.]),
                        ..Default::default()
                    },
                    position: [50., 40.],
                    color: [0, 0, 0],
                })
                .unwrap();
            let overlay = selected_text_frame_overlay(&document).unwrap();
            assert!(overlay.handles);
            assert_eq!(
                overlay.baseline,
                Some(if vertical {
                    [[90., 40.], [90., 140.]]
                } else {
                    [[50., 60.], [150., 60.]]
                })
            );
            let snapshot = document.snapshot();
            let settings = TextSettings {
                id: Some(snapshot.text_objects[0].id.clone()),
                text: snapshot.text_objects[0].text.clone(),
                position: [50., 40.],
                color: [0, 0, 0],
            };
            assert!(text_frame_overlay(&settings, false).is_none());
            assert!(selected_text_resize_handle(&document, [50., 40.]).is_none());
        }
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
            pasteboard_color: None,
            overlays: Vec::new(),
            overlay: None,
            channel: 0,
            x: 79.0,
            y: 228.0,
            width: 1042.0,
            height: 436.0,
            zoom: 1.0,
            absolute_zoom: false,
            zoom_revision: None,
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
            pasteboard_color: None,
            overlays: Vec::new(),
            overlay: None,
            channel: 0,
            x: -10.0,
            y: 400.0,
            width: 700.0,
            height: 100.0,
            zoom: 1.0,
            absolute_zoom: false,
            zoom_revision: None,
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
        && !window_sessions::shared_has_other_views(ACTIVE_DOCUMENT_ID.with(|active| active.get()))
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
    let Some(window) = APP
        .get()
        .and_then(|app| app.get_webview_window(&current_label()))
    else {
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

pub fn open_document_view(id: u64, target: &str) -> Result<DocumentWorkspaceSnapshot, String> {
    window_sessions::open_document_view(id, target)
}

pub fn move_document_to_window(id: u64, target: &str) -> Result<DocumentWorkspaceSnapshot, String> {
    window_sessions::transfer_document(id, target)
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
    window_sessions::ensure_shared_editable(id)?;
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
        if dirty && !window_sessions::shared_has_other_views(id) && !confirm_unsaved_changes() {
            return Ok(workspace_snapshot());
        }
        window_sessions::detach_shared_view(id, &current_label(), true);
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
            window_sessions::detach_shared_view(id, &current_label(), true);
            Ok(())
        })?;
        emit_workspace();
    }
    Ok(workspace_snapshot())
}

pub fn open_psd(prepared: crate::psd_import::Prepared) -> Result<(), String> {
    text_editor::finish(true)?;
    park_active_document();
    activate_document(OpenDocument {
        id: next_document_id(),
        content: OpenDocumentContent::Tiled(TiledSession {
            selected_layer: None,
            document: prepared.document,
            name: prepared.name,
            file_name: None,
            saved_revision: None,
        }),
        path: None,
        fingerprint: None,
    });
    if let Err(error) = redraw() {
        emit_error(error);
    }
    emit_document();
    Ok(())
}

fn export_tiled_psd() -> Result<DocumentSnapshot, String> {
    let snapshot = ACTIVE_TILED_DOCUMENT
        .with(|d| d.borrow().as_ref().map(TiledSession::snapshot))
        .ok_or("No tiled document")?;
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PSD", &["psd"])
        .add_filter("PSB", &["psb"])
        .set_file_name("Untitled.psd")
        .save_file()
    else {
        return Ok(snapshot);
    };
    let format = match path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("psd") => lumapaint_formats::export::FormatId::Psd,
        Some("psb") => lumapaint_formats::export::FormatId::Psb,
        _ => {
            return Err(
                "Choose .psd or .psb / .psdまたは.psbを指定してください / 请选择.psd或.psb".into(),
            )
        }
    };
    let state = ACTIVE_TILED_DOCUMENT.with(|d| -> Result<_, String> {
        let borrowed = d.borrow();
        let session = borrowed.as_ref().ok_or("No tiled document")?;
        let bytes: usize = session.document.layers().iter().map(|layer|
            (layer.tiles.allocated_tile_count() * 4 + layer.mask.allocated_tile_count()) * (lumapaint_core::tiles::TILE_SIZE as usize).pow(2)).sum();
        if bytes > 64 * 1024 * 1024 { return Err("PSD/PSB export source exceeds 64 MiB / PSD・PSB書き出し元のタイルが64MiBを超えています / PSD/PSB导出源瓦片超过64MiB".into()); }
        Ok(session.document.state())
    })?;
    let label = current_label();
    let app = APP.get().ok_or("Application unavailable")?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = lumapaint_formats::export::export_raster(
            format,
            &state,
            lumapaint_formats::export::ExportOptions { allow_lossy: true },
        )
        .map_err(|e| e.to_string());
        let _ = app.run_on_main_thread(move || {
            let Ok(_session) = SessionGuard::enter(&label) else {
                return;
            };
            match result {
                Err(error) => emit_error(error),
                Ok(result) => {
                    if !result.report.issues.is_empty()
                        && rfd::MessageDialog::new()
                            .set_title("互換性 / Compatibility / 兼容性")
                            .set_description(crate::psd_import::report_text(&result.report))
                            .set_buttons(rfd::MessageButtons::OkCancel)
                            .show()
                            != rfd::MessageDialogResult::Ok
                    {
                        return;
                    }
                    if let Err(error) = crate::project_file::write(&path, &result.bytes) {
                        emit_error(error);
                    }
                }
            }
        });
    });
    Ok(snapshot)
}

pub fn file_action(action: super::FileAction) -> Result<DocumentSnapshot, String> {
    text_editor::finish(true)?;
    use super::FileAction;
    match action {
        FileAction::Export => {
            if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
                ensure_tiled_open()?;
                return export_tiled_psd();
            }
            ensure_document_open()?;
            let Some(path) = rfd::FileDialog::new()
                .add_filter("SVG", &["svg"])
                .add_filter("PDF", &["pdf"])
                .set_file_name("Untitled.svg")
                .save_file()
            else {
                return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
            };
            let document = DOCUMENT.with(|doc| doc.borrow().clone());
            let label = current_label();
            let app = APP.get().ok_or("Application unavailable")?.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let result = if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                {
                    crate::vector_export::pdf(&document)
                } else {
                    crate::vector_export::svg(&document)
                };
                let _ = app.run_on_main_thread(move || {
                    let Ok(_session) = SessionGuard::enter(&label) else {
                        return;
                    };
                    match result {
                        Err(error) => emit_error(error),
                        Ok(result) => {
                            if !result.report.issues.is_empty()
                                && rfd::MessageDialog::new()
                                    .set_title("互換性 / Compatibility / 兼容性")
                                    .set_description(crate::vector_export::report_text(
                                        &result.report,
                                    ))
                                    .set_buttons(rfd::MessageButtons::OkCancel)
                                    .show()
                                    != rfd::MessageDialogResult::Ok
                            {
                                return;
                            }
                            if let Err(error) = crate::project_file::write(&path, &result.bytes) {
                                emit_error(error);
                            }
                        }
                    }
                });
            });
        }
        FileAction::Open => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter(
                    "LumaPaint / SVG / PDF / AI / PSD / PSB / JPEG / PNG",
                    &[
                        "lumapaint",
                        "svg",
                        "pdf",
                        "ai",
                        "psd",
                        "psb",
                        "jpg",
                        "jpeg",
                        "png",
                    ],
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
            if extension == "psd" || extension == "psb" {
                crate::psd_import::open(
                    APP.get().ok_or("Application unavailable")?.clone(),
                    current_label(),
                    path,
                )?;
                return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
            }
            if matches!(extension.as_str(), "pdf" | "ai") {
                crate::pdf_import::prepare(
                    APP.get().ok_or("Application unavailable")?.clone(),
                    current_label(),
                    path,
                    None,
                );
                return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
            }
            if extension == "svg" {
                let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                use std::io::Read;
                let mut bytes = Vec::new();
                file.take(4 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                let decoded = lumapaint_formats::io::read_document(
                    lumapaint_formats::export::FormatId::Svg,
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    &bytes,
                    lumapaint_formats::io::ReadOptions::default(),
                )
                .map_err(|e| e.to_string())?;
                let lumapaint_formats::io::ReadContent::Vector(mut document) = decoded.content
                else {
                    return Err("Expected SVG document".into());
                };
                lumapaint_svg::attach(&mut document);
                for layer in document.svg_layers() {
                    validate_svg(&layer.source)?;
                }
                park_active_document();
                activate_document(OpenDocument {
                    id: next_document_id(),
                    content: OpenDocumentContent::Legacy(document),
                    path: None,
                    fingerprint: None,
                });
                redraw()?;
                emit_document();
                return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
            }
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
                        selected_layer: None,
                        document: TiledRasterDocument::from_state(state)?,
                        file_name: Some(name.clone()),
                        saved_revision: Some(0),
                        name: name.clone(),
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
                            document.saved_revision = Some(document.document.revision());
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
        guide_layout: None,
        pages: None,
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

thread_local! {
    static RECOVERY: RefCell<Option<crate::recovery::Recovery>> = const { RefCell::new(None) };
    static RECOVERY_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static CHECKPOINT_REVISION: std::cell::Cell<Option<(u64, bool)>> = const { std::cell::Cell::new(None) };
    static RECOVERY_DISCARDED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn start_recovery() {
    let result = window_sessions::fork_recovery().unwrap_or_else(|| {
        APP.get()
            .ok_or_else(|| "Application is not initialized".to_string())
            .and_then(|app| app.path().app_local_data_dir().map_err(|e| e.to_string()))
            .and_then(|directory| crate::recovery::Recovery::start(directory.join("recovery")))
    });
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
    if window_sessions::shared_checkpoint() {
        return;
    }
    if ACTIVE_TILED_DOCUMENT.with(|slot| slot.borrow().is_some()) {
        ACTIVE_TILED_DOCUMENT.with(|slot| {
            let slot = slot.borrow();
            let document = slot.as_ref().unwrap();
            let key = (document.document.revision(), document.dirty());
            if CHECKPOINT_REVISION.with(|last| last.get() == Some(key)) {
                return;
            }
            RECOVERY.with(|recovery| {
                if let Some(recovery) = recovery.borrow().as_ref() {
                    recovery.checkpoint_project(
                        crate::project_file::ProjectData::Tiled(document.document.state()),
                        key.0,
                        key.1,
                    );
                    CHECKPOINT_REVISION.with(|last| last.set(Some(key)));
                }
            });
        });
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
    if let Some(info) = window_sessions::shared_recovery_info() {
        return info;
    }
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
    window_sessions::retry_shared_recovery();
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
    let project = RECOVERY.with(|slot| {
        slot.borrow()
            .as_ref()
            .ok_or("Recovery is unavailable")?
            .read_candidate_project(&id)
    })?;
    let content = match project {
        crate::project_file::ProjectData::Legacy(document) => {
            for layer in document.svg_layers() {
                validate_svg(&layer.source)?;
            }
            let mut recovered = Document::default();
            recovered.replace_recovered(*document);
            OpenDocumentContent::Legacy(Box::new(recovered))
        }
        crate::project_file::ProjectData::Tiled(state) => {
            OpenDocumentContent::Tiled(TiledSession {
                selected_layer: None,
                document: TiledRasterDocument::from_state(state)?,
                name: "Recovered tiled document / 復旧したタイル文書 / 恢复的瓦片文档".into(),
                file_name: None,
                saved_revision: None,
            })
        }
    };
    park_active_document();
    activate_document(OpenDocument {
        id: next_document_id(),
        content,
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
    Ok(ACTIVE_TILED_DOCUMENT.with(|slot| {
        slot.borrow().as_ref().map_or_else(
            || DOCUMENT.with(|doc| doc.borrow().snapshot()),
            TiledSession::snapshot,
        )
    }))
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

pub fn edit_transform_panel(
    edit: lumapaint_core::document::TransformPanelEdit,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    cancel_vector_drag();
    DOCUMENT.with(|d| d.borrow_mut().edit_transform_panel(edit))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
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
            rectangle_radii: None,
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
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                registration: false,
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
    fn vector_selection_commands_and_context_shortcuts_keep_pixel_selection_separate() {
        let (id, revision) = fixture();
        TOOL.with(|t| t.set(CanvasTool::VectorSelect));
        let snapshot = edit(DocumentAction::SelectAll).unwrap();
        assert_eq!(
            snapshot.selected_vector_objects.as_slice(),
            std::slice::from_ref(&id)
        );
        assert!(snapshot.selection.is_none());
        assert_eq!(snapshot.revision, revision);
        assert!(DIRECT_POINTS.with(|p| p.borrow().is_empty()));
        let deselected = edit(DocumentAction::Deselect).unwrap();
        assert!(deselected.selected_vector_objects.is_empty());
        assert!(deselected.can_reselect_vectors);
        let reselected =
            vector_selection_action(lumapaint_core::document::VectorSelectionRequest {
                action: "reselect".into(),
                criterion: None,
                name: None,
                new_name: None,
            })
            .unwrap();
        assert_eq!(
            reselected.selected_vector_objects.as_slice(),
            std::slice::from_ref(&id)
        );
        let saved = vector_selection_action(lumapaint_core::document::VectorSelectionRequest {
            action: "save".into(),
            criterion: None,
            name: Some("制作対象".into()),
            new_name: None,
        })
        .unwrap();
        assert_eq!(saved.saved_vector_selections[0].name, "制作対象");
        let encoded = DOCUMENT.with(|d| d.borrow_mut().encode().unwrap());
        let mut restored = Document::decode(&encoded).unwrap();
        restored
            .vector_selection_action(lumapaint_core::document::VectorSelectionRequest {
                action: "load".into(),
                criterion: None,
                name: Some("制作対象".into()),
                new_name: None,
            })
            .unwrap();
        assert_eq!(restored.selected_vector_ids(), [id]);
        edit(DocumentAction::Deselect).unwrap();
        TOOL.with(|t| t.set(CanvasTool::Brush));
        let pixel = edit(DocumentAction::SelectAll).unwrap();
        assert!(pixel.selection.is_some());
        assert!(pixel.selected_vector_objects.is_empty());
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

thread_local! { static WINDOW_TITLE: RefCell<String> = const { RefCell::new(String::new()) }; }

pub(super) fn prepare_modal() -> Result<DocumentWorkspaceSnapshot, String> {
    finish_open_pen()?;
    cancel_vector_drag();
    text_editor::finish(true)?;
    DOCUMENT.with(|doc| doc.borrow_mut().finish());
    SPACE_DOWN.with(|value| value.set(false));
    PANNING.with(|value| value.set(false));
    checkpoint();
    request_redraw()?;
    Ok(workspace_snapshot())
}
fn modal_input_blocked(label: &str) -> bool {
    APP.get()
        .is_some_and(|app| crate::modal_windows::blocked(app, label))
}

pub fn create_gradient_fill_layer(
    gradient: lumapaint_core::gradient::Gradient,
) -> Result<DocumentSnapshot, String> {
    let _ = prepare_pixel_gradient(&[])?;
    DOCUMENT.with(|document| {
        document.borrow_mut().add_gradient_fill_layer(
            gradient,
            &lumapaint_renderer::vector::skia_paths::SkiaPathEngine,
        )
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
pub fn apply_gradient(
    ids: &[String],
    target: &str,
    gradient: lumapaint_core::gradient::Gradient,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|d| {
        d.borrow_mut()
            .set_selected_vector_gradient(ids, target, gradient)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
pub struct PixelGradientJob {
    document: Document,
    id: u64,
    revision: u64,
    layer: String,
    selection: Option<lumapaint_core::document::Selection>,
}
pub fn prepare_pixel_gradient(ids: &[String]) -> Result<PixelGradientJob, String> {
    ensure_document_open()?;
    let document = DOCUMENT.with(|d| {
        d.borrow_mut().finish();
        d.borrow().clone()
    });
    if !ids.is_empty()
        || !document.selected_vector_ids().is_empty()
        || document.selected_layer_is_vector()
    {
        return Err(
            "Select a pixel layer / ピクセルレイヤーを選択してください / 请选择像素图层".into(),
        );
    }
    let snapshot = document.snapshot();
    let layer = snapshot
        .layers
        .iter()
        .find(|l| l.id == snapshot.layer_id)
        .ok_or("Layer not found")?;
    if layer.locked || layer.alpha_locked || !layer.visible || layer.kind != "paint" {
        return Err("Unlock and show the pixel layer / ピクセルレイヤーを表示しロックを解除してください / 请显示并解锁像素图层".into());
    }
    Ok(PixelGradientJob {
        id: ACTIVE_DOCUMENT_ID.with(|i| i.get()),
        revision: document.revision(),
        layer: snapshot.layer_id,
        selection: document.selection().cloned(),
        document,
    })
}
pub fn render_pixel_gradient(
    mut job: PixelGradientJob,
    gradient: lumapaint_core::gradient::Gradient,
) -> Result<PixelGradientJob, String> {
    gradient.validate()?;
    let (w, h, mut pixels) = clipboard::raw_selected_pixels(&job.document)?;
    let bounds =
        job.selection
            .as_ref()
            .filter(|s| {
                !s.regions.is_empty()
                    && !s.regions.iter().any(|r| {
                        r.operation == lumapaint_core::selection::SelectionOperation::Invert
                    })
            })
            .map(|s| {
                s.regions.iter().fold(
                    [
                        f32::INFINITY,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        f32::NEG_INFINITY,
                    ],
                    |mut b, r| {
                        b[0] = b[0].min(r.bounds[0]);
                        b[1] = b[1].min(r.bounds[1]);
                        b[2] = b[2].max(r.bounds[0] + r.bounds[2]);
                        b[3] = b[3].max(r.bounds[1] + r.bounds[3]);
                        b
                    },
                )
            })
            .unwrap_or([0., 0., w as f32, h as f32]);
    let definition = gradient.svg_definition_in_bounds("pixel-gradient", bounds)
        + &gradient.svg_dither_filter("pixel-dither");
    let filter = if gradient.dither {
        "filter=\"url(#pixel-dither)\""
    } else {
        ""
    };
    let svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\"><defs>{definition}</defs><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#pixel-gradient)\" {filter}/></svg>",bounds[0],bounds[1],bounds[2]-bounds[0],bounds[3]-bounds[1]);
    let gradient_pixels = if gradient.pixel_style.is_some() {
        let mut gradient = gradient.clone();
        if gradient.geometry.is_none() {
            let r = -gradient.angle.to_radians();
            let length = (bounds[2] - bounds[0]).max(1.);
            let (sin, cos) = r.sin_cos();
            gradient.geometry = Some([
                cos * length,
                sin * length,
                -sin * length * gradient.aspect,
                cos * length * gradient.aspect,
                (bounds[0] + bounds[2]) * 0.5,
                (bounds[1] + bounds[3]) * 0.5,
            ]);
        }
        lumapaint_renderer::gradient_raster::rasterize(&gradient, w, h)?
    } else {
        lumapaint_renderer::vector::rasterize_svg(&svg, w, h)?.pixels
    };
    for (i, (dest, source)) in pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(gradient_pixels.as_chunks::<4>().0)
        .enumerate()
    {
        if job.selection.as_ref().is_some_and(|s| {
            !s.contains(lumapaint_core::document::Point {
                x: (i as u32 % w) as f32 + 0.5,
                y: (i as u32 / w) as f32 + 0.5,
            })
        }) {
            continue;
        }
        for c in 0..4 {
            dest[c] =
                (source[c] as u16 + dest[c] as u16 * (255 - source[3] as u16) / 255).min(255) as u8;
        }
    }
    let png = lumapaint_renderer::vector::document_png(w, h, pixels)?;
    job.document
        .replace_moved_pixels(clipboard::image_svg(w, h, &png), job.selection.clone())?;
    Ok(job)
}
pub fn commit_pixel_gradient(job: PixelGradientJob) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    if ACTIVE_DOCUMENT_ID.with(|i| i.get()) != job.id
        || DOCUMENT.with(|d| {
            let d = d.borrow();
            d.revision() != job.revision
                || d.snapshot().layer_id != job.layer
                || d.selection() != job.selection.as_ref()
        })
    {
        return Err("Document or selection changed / ドキュメントまたは選択が変更されました / 文档或选区已更改".into());
    }
    DOCUMENT.with(|d| *d.borrow_mut() = job.document);
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

#[cfg(test)]
mod pixel_gradient_tests {
    use super::*;
    #[test]
    fn pixel_gradient_preserves_outside_selection_and_has_single_undo() {
        let mut document = Document::default();
        let mut settings:DocumentSettings=serde_json::from_value(serde_json::json!({"name":"Gradient test","width":32,"height":16,"unit":"pixels","resolution":72,"artboards":false,"canvasColor":"transparent","pixelAspectRatio":1})).unwrap();
        settings.canvas_color = lumapaint_core::document::CanvasColor::Transparent;
        document.set_document_settings(settings).unwrap();
        document.select_layer("layer-1".into()).unwrap();
        document
            .begin_selection_edit(
                lumapaint_core::document::Point { x: 8., y: 4. },
                SelectionShape::Rectangle,
                lumapaint_core::document::SelectionMode::Replace,
            )
            .unwrap();
        document.extend_selection(lumapaint_core::document::Point { x: 24., y: 12. }, true);
        let selection = document.selection().cloned();
        let revision = document.revision();
        let gradient = lumapaint_core::gradient::Gradient {
            geometry: None,
            pixel_style: None,
            kind: lumapaint_core::gradient::GradientKind::Linear,
            angle: 0.,
            aspect: 1.,
            dither: false,
            method: lumapaint_core::gradient::GradientMethod::Classic,
            stops: vec![
                lumapaint_core::gradient::GradientStop {
                    position: 0.,
                    color: [255, 0, 0, 255],
                    midpoint: 0.5,
                },
                lumapaint_core::gradient::GradientStop {
                    position: 1.,
                    color: [0, 0, 255, 255],
                    midpoint: 0.5,
                },
            ],
        };
        for style in [
            None,
            Some(lumapaint_core::gradient::PixelGradientStyle::Angular),
            Some(lumapaint_core::gradient::PixelGradientStyle::Reflected),
            Some(lumapaint_core::gradient::PixelGradientStyle::Diamond),
        ] {
            let mut gradient = gradient.clone();
            gradient.pixel_style = style;
            gradient.geometry = Some([16., 0., 0., 8., 8., 4.]);
            let mut result = render_pixel_gradient(
                PixelGradientJob {
                    document: document.clone(),
                    id: 1,
                    revision,
                    layer: "layer-1".into(),
                    selection: selection.clone(),
                },
                gradient,
            )
            .unwrap();
            let raster = lumapaint_renderer::vector::rasterize_svg(
                result.document.paint_source().unwrap(),
                32,
                16,
            )
            .unwrap();
            assert_eq!(raster.pixels[3], 0);
            let left = (8 * 32 + 9) * 4;
            let right = (8 * 32 + 22) * 4;
            assert_eq!(raster.pixels[left + 3], 255);
            if style.is_none() {
                assert!(raster.pixels[left] > raster.pixels[left + 2]);
                assert!(raster.pixels[right + 2] > raster.pixels[right]);
            }
            assert_eq!(result.document.selection(), selection.as_ref());
            result.document.undo();
            assert!(result.document.paint_source().is_none());
            result.document.redo();
            assert!(result.document.paint_source().is_some());
        }
    }
}

pub fn configure_gradient_tool(
    gradient: lumapaint_core::gradient::Gradient,
    target: &str,
) -> Result<(), String> {
    ensure_document_open()?;
    gradient_tool::configure(gradient, target);
    Ok(())
}

pub fn tool_options() -> Result<super::ToolOptionsSnapshot, String> {
    ensure_document_open()?;
    let (gradient, gradient_target) = gradient_tool::options();
    Ok(super::ToolOptionsSnapshot {
        crop_bounds: crop_tool::bounds(),
        gradient,
        gradient_target,
        brush: BRUSH.with(|brush| *brush.borrow()),
        zoom: CANVAS.with(|slot| {
            slot.borrow()
                .as_ref()
                .map_or(1., |canvas| canvas.viewport.screen_zoom())
        }),
    })
}
pub fn set_tool_brush(brush: Brush) -> Result<(), String> {
    ensure_document_open()?;
    brush.validate()?;
    BRUSH.with(|slot| *slot.borrow_mut() = brush);
    if let Some(app) = APP.get() {
        let _ = app.emit_to(current_label(), "canvas-brush-changed", brush);
    }
    request_redraw()
}
pub fn set_tool_zoom(zoom: f32) -> Result<(), String> {
    ensure_document_open()?;
    CANVAS.with(|slot| -> Result<(), String> {
        let mut slot = slot.borrow_mut();
        let canvas = slot.as_mut().ok_or("Canvas is unavailable")?;
        canvas.viewport = canvas.viewport.with_screen_zoom(zoom);
        canvas.view.refresh_cursor();
        Ok(())
    })?;
    if let Some(app) = APP.get() {
        let _ = app.emit_to(current_label(), "canvas-zoom-changed", zoom);
    }
    request_redraw()
}
fn apply_numeric_tool(
    document: &mut Document,
    tool: CanvasTool,
    bounds: [f32; 4],
) -> Result<(), String> {
    if tool == CanvasTool::Crop {
        document.crop_canvas(bounds)?;
        return Ok(());
    }
    let mut next = document.clone();
    let start = lumapaint_core::document::Point {
        x: bounds[0],
        y: bounds[1],
    };
    let end = lumapaint_core::document::Point {
        x: bounds[0] + bounds[2],
        y: bounds[1] + bounds[3],
    };
    if matches!(tool, CanvasTool::Rectangle | CanvasTool::Ellipse) {
        let snapshot = next.snapshot();
        if start.x < 0.
            || start.y < 0.
            || end.x > snapshot.width as f32
            || end.y > snapshot.height as f32
        {
            return Err("Selection must fit the document / 選択範囲はドキュメント内に指定してください / 选区必须位于文档内".into());
        }
        next.deselect();
        next.begin_selection_edit(
            start,
            if tool == CanvasTool::Rectangle {
                SelectionShape::Rectangle
            } else {
                SelectionShape::Ellipse
            },
            SelectionMode::Replace,
        )?;
        next.extend_selection(end, true);
    } else {
        let serial = NEXT_VECTOR_OBJECT_ID.with(|next| next.get());
        vector_pointer(&mut next, tool, start, 0, NSEventModifierFlags::empty())?;
        vector_pointer(&mut next, tool, end, 2, NSEventModifierFlags::empty())?;
        if NEXT_VECTOR_OBJECT_ID.with(|next| next.get()) == serial {
            return Err("Shape is smaller than the current drawing precision / 図形が小さすぎます / 图形过小".into());
        }
        next.select_vector_objects(vec![format!("vector-object-{serial}")])?;
    }
    *document = next;
    Ok(())
}
pub fn numeric_tool(tool: CanvasTool, bounds: [f32; 4]) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| apply_numeric_tool(&mut doc.borrow_mut(), tool, bounds))?;
    if tool == CanvasTool::Crop {
        crop_tool::cancel();
    }
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
#[cfg(test)]
mod numeric_tool_tests {
    use super::*;
    #[test]
    fn numeric_shapes_share_native_geometry_and_failed_selection_preserves_document() {
        let mut doc = Document::default();
        apply_numeric_tool(&mut doc, CanvasTool::VectorRectangle, [12., 24., 100., 80.]).unwrap();
        assert_eq!(
            doc.selected_vector_bounds().unwrap(),
            [12., 24., 112., 104.]
        );
        assert!(doc.snapshot().can_undo);
        let before = serde_json::to_value(doc.document_state()).unwrap();
        assert!(apply_numeric_tool(&mut doc, CanvasTool::Rectangle, [-2., 0., 10., 10.]).is_err());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        apply_numeric_tool(&mut doc, CanvasTool::Ellipse, [20., 30., 60., 40.]).unwrap();
        assert!(doc.selection().is_some());
    }
}
pub fn sample_tool_point(x: f32, y: f32) -> Result<(), String> {
    ensure_document_open()?;
    sample_canvas_color(lumapaint_core::document::Point { x, y })
}

thread_local! {
    // Static tool glyphs are cached once on the AppKit thread, independent of document render caches.
    static TOOL_ICON_CURSORS: RefCell<Vec<(CanvasTool,Retained<NSCursor>)>> = const { RefCell::new(Vec::new()) };
}
#[allow(deprecated)]
fn tool_icon_cursor(tool: CanvasTool) -> Retained<NSCursor> {
    if let Some(cursor) = TOOL_ICON_CURSORS.with(|cache| {
        cache
            .borrow()
            .iter()
            .find(|(kind, _)| *kind == tool)
            .map(|(_, cursor)| cursor.clone())
    }) {
        return cursor;
    }
    let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(28., 28.));
    image.lockFocus();
    let mut lines: Vec<Vec<[f64; 2]>> = vec![];
    let mut oval = None;
    let hotspot = if tool == CanvasTool::VectorDirectSelect {
        [3., 25.]
    } else {
        [8., 20.]
    };
    match tool {
        CanvasTool::VectorDirectSelect => lines.push(vec![
            [3., 25.],
            [3., 9.],
            [7., 14.],
            [11., 7.],
            [14., 9.],
            [10., 16.],
            [17., 16.],
            [3., 25.],
        ]),
        CanvasTool::Eyedropper => {
            lines.push(vec![
                [8., 20.],
                [13., 20.],
                [24., 9.],
                [19., 4.],
                [8., 15.],
                [8., 20.],
            ]);
            lines.push(vec![[17., 11.], [22., 6.]]);
        }
        CanvasTool::VectorScale => {
            lines.push(vec![[8., 20.], [23., 5.]]);
            lines.push(vec![[8., 13.], [8., 20.], [15., 20.]]);
            lines.push(vec![[16., 5.], [23., 5.], [23., 12.]]);
        }
        CanvasTool::VectorRotate => {
            oval = Some([8., 5., 16., 16.]);
            lines.push(vec![[16., 23.], [23., 20.], [21., 14.]]);
        }
        CanvasTool::Rectangle | CanvasTool::VectorRectangle | CanvasTool::ImageFrameRectangle => {
            lines.push(vec![
                [15., 13.],
                [25., 13.],
                [25., 3.],
                [15., 3.],
                [15., 13.],
            ]);
        }
        CanvasTool::Ellipse | CanvasTool::VectorEllipse | CanvasTool::ImageFrameEllipse => {
            oval = Some([14., 3., 12., 10.]);
        }
        _ => {
            lines.push(vec![
                [8., 20.],
                [12., 12.],
                [21., 3.],
                [26., 8.],
                [17., 17.],
                [8., 20.],
            ]);
            lines.push(vec![[12., 12.], [17., 17.]]);
        }
    }
    if tool != CanvasTool::VectorDirectSelect {
        lines.push(vec![[3., 20.], [13., 20.]]);
        lines.push(vec![[8., 15.], [8., 25.]]);
    }
    if matches!(
        tool,
        CanvasTool::VectorAnchorAdd | CanvasTool::VectorAnchorDelete
    ) {
        lines.push(vec![[19., 23.], [27., 23.]]);
        if tool == CanvasTool::VectorAnchorAdd {
            lines.push(vec![[23., 19.], [23., 27.]]);
        }
    }
    if tool == CanvasTool::VectorAnchorConvert {
        lines.push(vec![[17., 22.], [22., 27.], [27., 22.]]);
    }
    // SAFETY: main-thread AppKit context is current; geometry and line widths are bounded constants.
    unsafe {
        let path: Retained<AnyObject> = msg_send![class!(NSBezierPath), bezierPath];
        for points in lines {
            let _: () = msg_send![&*path,moveToPoint:NSPoint::new(points[0][0],points[0][1])];
            for point in points.iter().skip(1) {
                let _: () = msg_send![&*path,lineToPoint:NSPoint::new(point[0],point[1])];
            }
        }
        if let Some([x, y, w, h]) = oval {
            let _: () = msg_send![&*path,appendBezierPathWithOvalInRect:NSRect::new(NSPoint::new(x,y),NSSize::new(w,h))];
        }
        NSColor::whiteColor().setStroke();
        let _: () = msg_send![&*path,setLineWidth:3.0f64];
        let _: () = msg_send![&*path, stroke];
        NSColor::blackColor().setStroke();
        let _: () = msg_send![&*path,setLineWidth:1.0f64];
        let _: () = msg_send![&*path, stroke];
    }
    image.unlockFocus();
    let cursor = NSCursor::initWithImage_hotSpot(
        NSCursor::alloc(),
        &image,
        NSPoint::new(hotspot[0], 28. - hotspot[1]),
    );
    TOOL_ICON_CURSORS.with(|cache| cache.borrow_mut().push((tool, cursor.clone())));
    cursor
}

struct ToolWindowPreview {
    document_id: u64,
    revision: u64,
    document: Document,
    zoom: Option<f32>,
}
thread_local! {
    // The Rust host owns previews by editor session, never by a WebView's document state.
    static TOOL_WINDOW_PREVIEWS:RefCell<std::collections::HashMap<String,ToolWindowPreview>>=RefCell::new(std::collections::HashMap::new());
}
fn build_tool_preview(
    document: &Document,
    request: super::ToolPreviewRequest,
) -> Result<(Document, Option<f32>), String> {
    use super::ToolPreviewRequest as Request;
    let mut preview = document.clone();
    let mut zoom = None;
    match request {
        Request::Numeric { tool, bounds } => apply_numeric_tool(&mut preview, tool, bounds)?,
        Request::Text { mut settings } => {
            text_editor::reflow(&mut settings)?;
            preview.set_text_object(*settings)?;
        }
        Request::Transform { action, values } => {
            preview.transform_selected_vectors(&action, values)?;
        }
        Request::Zoom { zoom: value } => zoom = Some(value),
        Request::Sample { x, y } => {
            preview.begin_selection(
                lumapaint_core::document::Point {
                    x: x - 3.,
                    y: y - 3.,
                },
                SelectionShape::Ellipse,
            );
            preview.extend_selection(
                lumapaint_core::document::Point {
                    x: x + 3.,
                    y: y + 3.,
                },
                true,
            );
        }
        Request::Brush { tool, brush } => {
            let snapshot = preview.snapshot();
            let start = lumapaint_core::document::Point {
                x: snapshot.width as f32 * 0.4,
                y: snapshot.height as f32 * 0.5,
            };
            let end = lumapaint_core::document::Point {
                x: snapshot.width as f32 * 0.6,
                y: start.y,
            };
            if matches!(tool, CanvasTool::VectorPen | CanvasTool::VectorPencil) {
                let layer = preview.add_vector_layer()?;
                preview.select_layer(layer)?;
                let original = BRUSH.with(|slot| slot.replace(brush));
                let result = vector_pointer(
                    &mut preview,
                    CanvasTool::VectorPencil,
                    start,
                    0,
                    NSEventModifierFlags::empty(),
                )
                .and_then(|_| {
                    vector_pointer(
                        &mut preview,
                        CanvasTool::VectorPencil,
                        end,
                        2,
                        NSEventModifierFlags::empty(),
                    )
                });
                BRUSH.with(|slot| slot.replace(original));
                result?;
            } else {
                // A temporary paint layer avoids altering or erasing existing content.
                let layer = preview.add_paint_layer()?;
                preview.select_layer(layer)?;
                preview.begin(start, brush)?;
                preview.extend(end)?;
                preview.finish();
            }
        }
    }
    Ok((preview, zoom))
}
pub fn tool_preview(request: Option<super::ToolPreviewRequest>) -> Result<(), String> {
    let label = current_label();
    TOOL_WINDOW_PREVIEWS.with(|previews| previews.borrow_mut().remove(&label));
    ensure_document_open()?;
    if let Some(request) = request {
        let result = request
            .validate()
            .and_then(|_| DOCUMENT.with(|doc| build_tool_preview(&doc.borrow(), request)));
        match result {
            Ok((document, zoom)) => {
                let revision = DOCUMENT.with(|doc| doc.borrow().revision());
                TOOL_WINDOW_PREVIEWS.with(|previews| {
                    previews.borrow_mut().insert(
                        label,
                        ToolWindowPreview {
                            document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                            revision,
                            document,
                            zoom,
                        },
                    )
                });
            }
            Err(error) => {
                request_redraw()?;
                return Err(error);
            }
        }
    }
    request_redraw()
}
pub fn clear_window_preview(label: &str) {
    TOOL_WINDOW_PREVIEWS.with(|previews| previews.borrow_mut().remove(label));
    if let Ok(_session) = SessionGuard::enter(label) {
        let _ = request_redraw();
    }
}
#[cfg(test)]
mod tool_window_preview_tests {
    use super::*;
    #[test]
    fn preview_is_read_only_and_always_starts_from_the_committed_model() {
        let doc = Document::default();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let (a, _) = build_tool_preview(
            &doc,
            super::super::ToolPreviewRequest::Numeric {
                tool: CanvasTool::VectorRectangle,
                bounds: [10., 20., 100., 80.],
            },
        )
        .unwrap();
        let (b, _) = build_tool_preview(
            &doc,
            super::super::ToolPreviewRequest::Numeric {
                tool: CanvasTool::VectorRectangle,
                bounds: [10., 20., 200., 80.],
            },
        )
        .unwrap();
        assert_eq!(a.selected_vector_bounds().unwrap(), [10., 20., 110., 100.]);
        assert_eq!(b.selected_vector_bounds().unwrap(), [10., 20., 210., 100.]);
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        assert!(!doc.snapshot().can_undo);
    }
}

#[cfg(test)]
mod panel_occlusion_tests {
    use super::visible_canvas_regions;
    #[test]
    fn overlapping_floating_panels_subtract_the_union_and_keep_other_canvas_visible() {
        let holes = [[10., 10., 60., 60.], [40., 40., 90., 90.]];
        let regions = visible_canvas_regions([0., 0., 100., 100.], &holes);
        let area: f64 = regions.iter().map(|r| (r[2] - r[0]) * (r[3] - r[1])).sum();
        assert_eq!(area, 5400.);
        for x in 0..100 {
            for y in 0..100 {
                let x = x as f64 + 0.5;
                let y = y as f64 + 0.5;
                let contains = |r: &[f64; 4]| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
                assert_eq!(regions.iter().any(contains), !holes.iter().any(contains));
            }
        }
        assert_eq!(
            visible_canvas_regions([0., 0., 100., 100.], &[]),
            vec![[0., 0., 100., 100.]]
        );
    }
}

// Linked graphics commands use the active Rust document; WebViews never own image data.
pub fn frame_place_context(explicit: Option<String>) -> Result<(Option<String>, u64, u64), String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    DOCUMENT.with(|d| {
        let d = d.borrow();
        let target = if explicit.is_some() {
            explicit
        } else {
            let ids = d.selected_vector_ids();
            if ids.len() == 1 && d.image_frame_object(&ids[0]).is_ok() {
                Some(ids[0].clone())
            } else {
                None
            }
        };
        if let Some(id) = &target {
            let (l, o) = d.image_frame_object(id)?;
            if l.locked || !l.visible || !o.visible || d.object_is_locked(id) {
                return Err("Frame is locked or hidden".into());
            }
        }
        Ok((target, ACTIVE_DOCUMENT_ID.with(|i| i.get()), d.revision()))
    })
}
pub fn frame_image_context(
    id: &str,
) -> Result<(lumapaint_core::image_frame::FrameImage, u64, u64), String> {
    ensure_document_open()?;
    DOCUMENT.with(|d| {
        let d = d.borrow();
        let image = d
            .image_frame_object(id)?
            .1
            .image_frame
            .as_ref()
            .unwrap()
            .image
            .clone()
            .ok_or("Empty frame")?;
        Ok((image, ACTIVE_DOCUMENT_ID.with(|i| i.get()), d.revision()))
    })
}
fn validate_frame_context(id: u64, revision: u64) -> Result<(), String> {
    ensure_document_open()?;
    if ACTIVE_DOCUMENT_ID.with(|i| i.get()) != id
        || DOCUMENT.with(|d| d.borrow().revision()) != revision
    {
        Err("Document changed / ドキュメントが変更されました。もう一度配置してください / 文档已更改，请重新置入".into())
    } else {
        Ok(())
    }
}
pub fn place_frame_image(
    target: Option<String>,
    image: lumapaint_core::image_frame::FrameImage,
    id: u64,
    revision: u64,
) -> Result<DocumentSnapshot, String> {
    validate_frame_context(id, revision)?;
    let serial = NEXT_VECTOR_OBJECT_ID.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    });
    DOCUMENT.with(|d| {
        let mut d = d.borrow_mut();
        let id = d.place_frame_image(target.as_deref(), image, &format!("image-frame-{serial}"))?;
        d.select_vector_objects(vec![id])
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
pub fn update_frame_image(
    target: String,
    image: lumapaint_core::image_frame::FrameImage,
    id: u64,
    revision: u64,
) -> Result<DocumentSnapshot, String> {
    validate_frame_context(id, revision)?;
    DOCUMENT.with(|d| {
        let mut d = d.borrow_mut();
        let mut frame = d
            .image_frame_object(&target)?
            .1
            .image_frame
            .clone()
            .unwrap();
        if let (Some(old), Some(matrix)) = (&frame.image, frame.content_transform) {
            frame.content_transform = Some(lumapaint_core::image_frame::multiply(
                matrix,
                [
                    old.width as f32 / image.width as f32,
                    0.,
                    0.,
                    old.height as f32 / image.height as f32,
                    0.,
                    0.,
                ],
            ));
        }
        frame.image = Some(image);
        d.edit_image_frame(&target, frame)
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
pub fn frame_links(
) -> Result<Vec<(String, Option<lumapaint_core::image_frame::FrameImage>)>, String> {
    ensure_document_open()?;
    Ok(DOCUMENT.with(|d| {
        d.borrow()
            .svg_layers()
            .flat_map(|l| l.vector_objects.iter())
            .filter_map(|o| {
                o.image_frame
                    .as_ref()
                    .map(|f| (o.id.clone(), f.image.clone()))
            })
            .collect()
    }))
}
pub fn edit_image_frame(
    id: &str,
    action: &str,
    values: Vec<f32>,
) -> Result<DocumentSnapshot, String> {
    if action == "go" {
        return go_to_image_frame(id);
    }
    use lumapaint_core::image_frame::FrameFit;
    ensure_document_open()?;
    DOCUMENT.with(|d| {
        let mut d = d.borrow_mut();
        match action {
            "contain" => d.fit_image_frame(id, FrameFit::Contain),
            "cover" => d.fit_image_frame(id, FrameFit::Cover),
            "stretch" => d.fit_image_frame(id, FrameFit::Stretch),
            "offset" if values.len() == 2 => d.move_frame_content(id, [values[0], values[1]]),
            "embed" => {
                let mut frame = d.image_frame_object(id)?.1.image_frame.clone().unwrap();
                if let Some(i) = &mut frame.image {
                    i.source_path = None;
                }
                d.edit_image_frame(id, frame)
            }
            _ => Err("Unknown image frame action".into()),
        }
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

pub fn manage_frame_images(
    updates: Vec<(String, Option<lumapaint_core::image_frame::FrameImage>)>,
    document_id: u64,
    revision: u64,
) -> Result<DocumentSnapshot, String> {
    validate_frame_context(document_id, revision)?;
    DOCUMENT.with(|d| d.borrow_mut().manage_frame_images(updates))?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

fn go_to_image_frame(id: &str) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    let bounds = DOCUMENT.with(|d| {
        let mut d = d.borrow_mut();
        let layer = d.image_frame_object(id)?.0.id.clone();
        d.select_layer(layer)?;
        d.select_vector_objects(vec![id.into()])?;
        d.selected_vector_bounds()
            .ok_or_else(|| String::from("Frame has no bounds"))
    })?;
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            let v = &mut canvas.viewport;
            let zoom = v.screen_zoom();
            v.pan_x = ((v.document_width - (bounds[0] + bounds[2])) * 0.5 * zoom).clamp(
                -lumapaint_renderer::MAX_CANVAS_PAN,
                lumapaint_renderer::MAX_CANVAS_PAN,
            );
            v.pan_y = ((v.document_height - (bounds[1] + bounds[3])) * 0.5 * zoom).clamp(
                -lumapaint_renderer::MAX_CANVAS_PAN,
                lumapaint_renderer::MAX_CANVAS_PAN,
            );
            PAN.with(|pan| pan.set((v.pan_x, v.pan_y)));
        }
    });
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

#[cfg(test)]
mod ruler_tests {
    use super::*;
    #[test]
    fn ruler_coordinates_match_native_document_mapping_at_retina_and_high_zoom() {
        for backing in [1., 2.] {
            for zoom in [0.0313, 1., 640.] {
                let mut v = Viewport::new(1000., 700., backing, 1., false).unwrap();
                v.document_width = 800.;
                v.document_height = 600.;
                v = v.with_screen_zoom(zoom).with_pan(-200., 120.).unwrap();
                let ruler = ruler_viewport(v);
                assert_eq!(ruler.width, 1000.);
                assert_eq!(ruler.height, 700.);
                let point = v.document_point(400., 300.);
                assert!((ruler.origin_x + point.x * ruler.zoom - 400.).abs() < 0.1);
                assert!((ruler.origin_y + point.y * ruler.zoom - 300.).abs() < 0.1);
            }
        }
    }
}

fn spread_fit(viewport: Viewport) -> (f32, f32, f32) {
    DOCUMENT.with(|doc| {
        let doc = doc.borrow();
        let (w, h) = doc.dimensions();
        let bounds = doc
            .facing_neighbor()
            .map_or([0., 0., w as f32, h as f32], |(_, x, n)| {
                let (nw, nh) = n.dimensions();
                [
                    x.min(0.),
                    0.,
                    (x + nw as f32).max(w as f32),
                    nh.max(h) as f32,
                ]
            });
        let zoom = ((viewport.width as f32 / viewport.scale - 48.) / (bounds[2] - bounds[0]))
            .min((viewport.height as f32 / viewport.scale - 48.) / bounds[3])
            .clamp(
                lumapaint_renderer::MIN_SCREEN_ZOOM,
                lumapaint_renderer::MAX_SCREEN_ZOOM,
            );
        (
            zoom,
            (w as f32 * 0.5 - (bounds[0] + bounds[2]) * 0.5) * zoom,
            (h as f32 - bounds[3]) * 0.5 * zoom,
        )
    })
}
fn fit_pages() -> Result<(), String> {
    let fit = CANVAS.with(|slot| slot.borrow().as_ref().map(|c| spread_fit(c.viewport)));
    if let Some((zoom, x, y)) = fit {
        CANVAS.with(|slot| {
            if let Some(c) = slot.borrow_mut().as_mut() {
                c.viewport = c.viewport.with_screen_zoom(zoom);
                c.viewport.pan_x = x;
                c.viewport.pan_y = y;
            }
        });
        PAN.with(|p| p.set((x, y)));
        if let Some(app) = APP.get() {
            let _ = app.emit_to(current_label(), "canvas-zoom-changed", zoom);
        }
    }
    redraw()
}
pub fn edit_pages(edit: lumapaint_core::document::PageEdit) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    cancel_vector_drag();
    DOCUMENT.with(|doc| doc.borrow_mut().edit_pages(edit))?;
    let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            canvas.viewport.document_width = snapshot.width as f32;
            canvas.viewport.document_height = snapshot.height as f32;
            canvas.viewport.canvas_color = snapshot.canvas_color;
            canvas.page_preview_key = None;
        }
    });
    fit_pages()?;
    emit_document();
    emit_workspace();
    Ok(snapshot)
}
fn update_page_preview(canvas: &mut Canvas) -> Result<(), String> {
    if ACTIVE_TILED_DOCUMENT.with(|doc| doc.borrow().is_some()) {
        if canvas.page_preview_key.take().is_some() {
            canvas.renderer.set_page_preview(None);
        }
        return Ok(());
    }
    DOCUMENT.with(|slot| {
        let doc = slot.borrow();
        canvas.viewport.document_width = doc.dimensions().0 as f32;
        canvas.viewport.document_height = doc.dimensions().1 as f32;
        canvas.viewport.canvas_color = doc.canvas_color();
        let snapshot = doc.pages_snapshot();
        let neighbor = doc.facing_neighbor();
        let key = neighbor.map(|(index, x, n)| {
            format!(
                "{}:{}:{index}:{x}:{}:{:?}:{:?}",
                ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                snapshot.pages[snapshot.active].id,
                n.revision(),
                n.dimensions(),
                doc.dimensions()
            )
        });
        if canvas.page_preview_key == key {
            return Ok(());
        }
        let preview = facing_preview_document(&doc)?;
        canvas.renderer.set_page_preview(preview);
        canvas.page_preview_key = key;
        Ok(())
    })
}

fn facing_preview_document(doc: &Document) -> Result<Option<Document>, String> {
    let neighbor = doc.facing_neighbor();
    let preview = if let Some((_, x, n)) = neighbor {
        let (nw, nh) = n.dimensions();
        let (w, h) = doc.dimensions();
        // Vector pages keep their original source; paint pages use a native cached composite.
        let inner = if n.committed_paint_strokes().next().is_none() {
            use lumapaint_formats::export::{export, ExportOptions, ExportSnapshot, FormatId};
            export(
                FormatId::Svg,
                &ExportSnapshot::capture(n),
                ExportOptions { allow_lossy: true },
            )
            .ok()
            .and_then(|v| String::from_utf8(v.bytes).ok())
        } else {
            None
        };
        let inner = if let Some(inner) = inner {
            inner
        } else {
            use base64::Engine;
            let bytes = lumapaint_renderer::thumbnails::page_preview(n, 2048)?;
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{nw}\" height=\"{nh}\"><image width=\"{nw}\" height=\"{nh}\" href=\"data:image/png;base64,{}\"/></svg>",base64::engine::general_purpose::STANDARD.encode(bytes))
        };
        let source=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><defs><clipPath id=\"lpNeighborClip\"><rect x=\"{x}\" width=\"{nw}\" height=\"{nh}\"/></clipPath></defs><g clip-path=\"url(#lpNeighborClip)\"><rect x=\"{x}\" width=\"{nw}\" height=\"{nh}\" fill=\"white\" stroke=\"#777\"/><g transform=\"translate({x} 0)\">{inner}</g></g></svg>");
        let mut preview = Document::default();
        let mut state = preview.document_state();
        state.width = w;
        state.height = h;
        preview = Document::from_document_state(state)?;
        preview.import_svg("Facing page preview".into(), source)?;
        Some(preview)
    } else {
        None
    };
    Ok(preview)
}

pub fn page_thumbnail_documents(indices: Vec<usize>) -> Result<Vec<(String, Document)>, String> {
    if !DOCUMENT_OPEN.with(|open| open.get())
        || ACTIVE_TILED_DOCUMENT.with(|doc| doc.borrow().is_some())
    {
        return Err("No editable document".into());
    }
    DOCUMENT.with(|doc| {
        let doc = doc.borrow();
        let snapshot = doc.pages_snapshot();
        indices
            .into_iter()
            .map(|index| {
                let page = snapshot.pages.get(index).ok_or("Page not found")?;
                let mut copy = doc.page_document(index).ok_or("Page not found")?.clone();
                copy.finish();
                Ok((page.id.clone(), copy))
            })
            .collect()
    })
}

thread_local! {static GUIDE_DRAFT:RefCell<Option<(String,f32)>>=const {RefCell::new(None)}; static GUIDE_DRAG:RefCell<Option<(String,[f32;2])>>=const {RefCell::new(None)};}
pub fn edit_guides(
    mut edit: lumapaint_core::document::GuideEdit,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    if edit.action == "saveLayout" {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("LumaPaint Guide Layout", &["json"])
            .set_file_name("LumaPaint-guides.json")
            .save_file()
        {
            let source = DOCUMENT.with(|d| d.borrow().encode_guide_layout())?;
            std::fs::write(path, source).map_err(|e| e.to_string())?;
        }
        return Ok(DOCUMENT.with(|d| d.borrow().snapshot()));
    }
    if edit.action == "loadLayout" {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("LumaPaint Guide Layout", &["json"])
            .pick_file()
        else {
            return Ok(DOCUMENT.with(|d| d.borrow().snapshot()));
        };
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 8 * 1024 * 1024 {
            return Err("Guide layout is too large".into());
        }
        edit.id = Some(std::fs::read_to_string(path).map_err(|e| e.to_string())?);
        edit.action = "importLayout".into();
    }
    GUIDE_OBJECT_DRAFT.with(|g| g.borrow_mut().clear());
    GUIDE_DRAFT.with(|g| g.borrow_mut().take());
    GUIDE_DRAG.with(|g| g.borrow_mut().take());
    DOCUMENT.with(|d| d.borrow_mut().edit_guides(edit))?;
    let snapshot = DOCUMENT.with(|d| d.borrow().snapshot());
    emit_document();
    emit_workspace();
    redraw()?;
    Ok(snapshot)
}
pub fn ruler_guide(axis: String, position: f32, phase: u8) -> Result<(), String> {
    if phase == 0 {
        ensure_document_open()?;
    }
    if !DOCUMENT_OPEN.with(|d| d.get()) || ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return Err("No editable document".into());
    }
    if !matches!(axis.as_str(), "horizontal" | "vertical")
        || !position.is_finite()
        || position.abs() > 1_000_000.
        || phase > 3
    {
        return Err("Invalid guide gesture".into());
    }
    if phase == 1 && GUIDE_DRAFT.with(|g| g.borrow().is_none()) {
        return Ok(());
    }
    if phase == 2 {
        if GUIDE_DRAFT.with(|g| g.borrow().is_none()) {
            return Ok(());
        }
        GUIDE_DRAFT.with(|g| g.borrow_mut().take());
        DOCUMENT.with(|d| {
            d.borrow_mut()
                .edit_guides(lumapaint_core::document::GuideEdit {
                    action: "add".into(),
                    id: None,
                    axis: Some(axis),
                    position: Some(position),
                    delta: None,
                })
        })?;
        emit_document();
        emit_workspace();
    } else if phase == 3 {
        GUIDE_DRAFT.with(|g| g.borrow_mut().take());
    } else {
        GUIDE_DRAFT.with(|g| *g.borrow_mut() = Some((axis, position)));
    }
    redraw()
}

thread_local! {static GUIDE_OBJECT_DRAFT:RefCell<Vec<lumapaint_renderer::FrameOverlay>>=const {RefCell::new(Vec::new())};}

pub fn ruler_origin(point: [f32; 2], phase: u8) -> Result<(), String> {
    if phase > 3 || point.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {
        return Err("Invalid ruler origin gesture".into());
    }
    if phase == 0 {
        ensure_document_open()?;
    }
    if phase == 2 {
        if GUIDE_DRAFT.with(|g| g.borrow().is_none()) {
            return Ok(());
        }
        edit_guides(lumapaint_core::document::GuideEdit {
            action: "origin".into(),
            id: None,
            axis: None,
            position: None,
            delta: Some(point),
        })?;
    } else if phase == 3 {
        GUIDE_DRAFT.with(|g| g.borrow_mut().take());
        GUIDE_OBJECT_DRAFT.with(|g| g.borrow_mut().clear());
    } else if phase == 0 || GUIDE_DRAFT.with(|g| g.borrow().is_some()) {
        GUIDE_DRAFT.with(|g| *g.borrow_mut() = Some(("origin".into(), point[0])));
        let line = CANVAS.with(|c| {
            c.borrow().as_ref().map(|c| {
                let v = c.viewport;
                let a = v.document_point(0., 0.);
                let b = v.document_point(v.width as f32 / v.scale, v.height as f32 / v.scale);
                let p = [a.x, point[1]];
                let q = [b.x, point[1]];
                lumapaint_renderer::FrameOverlay {
                    corners: [p, q, q, p],
                    handles: false,
                    baseline: None,
                }
            })
        });
        GUIDE_OBJECT_DRAFT.with(|g| *g.borrow_mut() = line.into_iter().collect());
    }
    redraw()
}

pub fn edit_layer_groups(
    edit: lumapaint_core::document::LayerGroupEdit,
) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    text_editor::finish(true)?;
    cancel_vector_drag();
    DOCUMENT.with(|doc| doc.borrow_mut().edit_layer_groups(edit))?;
    redraw()?;
    emit_document();
    emit_workspace();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}

pub fn apply_pdf_import(
    mut decoded: lumapaint_formats::io::ReadDocument,
    target: Option<u64>,
    name: String,
) -> Result<bool, String> {
    if let Some(target) = target {
        ensure_document_open()?;
        if ACTIVE_DOCUMENT_ID.with(|id| id.get()) != target
            || ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some())
        {
            return Err("読み込み先のドキュメントを選択してください。\nSelect the original destination document.\n请选择原始目标文档。".into());
        }
    }
    decoded
        .report
        .issues
        .retain(|i| i.code != "pdf.selected_page_only");
    if !decoded.report.issues.is_empty()
        && rfd::MessageDialog::new()
            .set_title("互換性 / Compatibility / 兼容性")
            .set_description(crate::vector_export::report_text(&decoded.report))
            .set_buttons(rfd::MessageButtons::OkCancel)
            .show()
            != rfd::MessageDialogResult::Ok
    {
        return Ok(false);
    }
    let lumapaint_formats::io::ReadContent::Vector(mut document) = decoded.content else {
        return Err("Expected PDF vector document".into());
    };
    lumapaint_svg::attach(&mut document);
    for index in 0..document.pages_snapshot().pages.len() {
        let page = document.page_document(index).ok_or("Missing PDF page")?;
        for layer in page.svg_layers() {
            validate_svg(&layer.source)?;
        }
    }
    if target.is_some() {
        let destination = DOCUMENT.with(|doc| doc.borrow().snapshot());
        let source = crate::pdf_import::layer_source(&document, &destination)?;
        validate_svg(&source)?;
        DOCUMENT.with(|doc| doc.borrow_mut().import_raster_layer(name, source))?;
    } else {
        park_active_document();
        activate_document(OpenDocument {
            id: next_document_id(),
            content: OpenDocumentContent::Legacy(document),
            path: None,
            fingerprint: None,
        });
    }
    if let Err(error) = redraw() {
        emit_error(error);
    }
    emit_document();
    Ok(true)
}

pub fn crop_action(confirm: bool) -> Result<(), String> {
    if confirm {
        crop_tool::commit()
    } else {
        crop_tool::cancel();
        redraw()
    }
}

pub(super) fn clone_stamp_settings() -> Result<lumapaint_core::clone_stamp::Settings, String> {
    Ok(clone_stamp_tool::settings())
}
pub(super) fn set_clone_stamp_settings(
    settings: lumapaint_core::clone_stamp::Settings,
) -> Result<(), String> {
    clone_stamp_tool::configure(settings)?;
    if let Some(app) = APP.get() {
        let _ = app.emit("clone-stamp-settings-changed", ());
    }
    Ok(())
}

pub(super) fn paint_bucket_settings() -> Result<lumapaint_core::paint_bucket::Settings, String> {
    Ok(paint_bucket_tool::settings())
}
pub(super) fn set_paint_bucket_settings(
    settings: lumapaint_core::paint_bucket::Settings,
) -> Result<(), String> {
    paint_bucket_tool::configure(settings)
}

pub(super) fn selection_tool_settings() -> Result<lumapaint_core::selection_tools::Settings, String>
{
    Ok(selection_tools::settings())
}
pub(super) fn set_selection_tool_settings(
    settings: lumapaint_core::selection_tools::Settings,
) -> Result<(), String> {
    selection_tools::configure(settings)?;
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow().as_ref() {
            if update_brush_cursor(canvas.viewport) {
                canvas.view.refresh_cursor();
            }
        }
    });
    request_redraw()
}
pub(super) fn selection_path_action(confirm: bool) -> Result<(), String> {
    if confirm {
        selection_tools::confirm()
    } else {
        selection_tools::cancel();
        redraw()
    }
}

thread_local! { static RETOUCH_SETTINGS: RefCell<std::collections::BTreeMap<String,lumapaint_core::retouch::Settings>> = const { RefCell::new(std::collections::BTreeMap::new()) }; }
pub(super) fn retouch_settings() -> Result<lumapaint_core::retouch::Settings, String> {
    Ok(RETOUCH_SETTINGS.with(|v| {
        v.borrow()
            .get(&current_label())
            .copied()
            .unwrap_or_default()
    }))
}
pub(super) fn set_retouch_settings(
    settings: lumapaint_core::retouch::Settings,
) -> Result<(), String> {
    settings.validate()?;
    RETOUCH_SETTINGS.with(|v| v.borrow_mut().insert(current_label(), settings));
    if let Some(app) = APP.get() {
        let _ = app.emit("retouch-settings-changed", ());
    }
    Ok(())
}

pub fn vector_selection_action(
    request: lumapaint_core::document::VectorSelectionRequest,
) -> Result<DocumentSnapshot, String> {
    if raster_import::active() {
        return Err("Confirm or cancel image placement / 画像の配置を確定またはキャンセルしてください / 请确认或取消图片放置".into());
    }
    ensure_document_open()?;
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return Err("Vector selection requires an editable document".into());
    }
    text_editor::finish(true)?;
    finish_open_pen()?;
    let selecting = !matches!(
        request.action.as_str(),
        "save" | "rename" | "delete" | "update"
    );
    DOCUMENT.with(|d| d.borrow_mut().vector_selection_action(request))?;
    if selecting {
        DIRECT_POINTS.with(|p| p.borrow_mut().clear());
        switch_canvas_tool(CanvasTool::VectorSelect)?;
        if let Some(app) = APP.get() {
            let _ = app.emit_to(
                current_label(),
                "canvas-tool-changed",
                CanvasTool::VectorSelect,
            );
        }
    }
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
fn vector_selection_context() -> bool {
    TOOL.with(|t| {
        matches!(
            t.get(),
            CanvasTool::VectorSelect
                | CanvasTool::VectorDirectSelect
                | CanvasTool::VectorPen
                | CanvasTool::VectorPencil
                | CanvasTool::VectorRectangle
                | CanvasTool::VectorEllipse
                | CanvasTool::VectorScale
                | CanvasTool::VectorRotate
                | CanvasTool::VectorAnchorAdd
                | CanvasTool::VectorAnchorDelete
                | CanvasTool::VectorAnchorConvert
                | CanvasTool::Text
                | CanvasTool::TextVertical
                | CanvasTool::TextFrame
                | CanvasTool::TextFrameVertical
                | CanvasTool::ImageFrameRectangle
                | CanvasTool::ImageFrameEllipse
        )
    })
}
