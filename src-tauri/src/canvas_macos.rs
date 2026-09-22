//! AppKit ownership is isolated here; no Apple types enter the renderer or document core.
use super::{
    CanvasInfo, CanvasRequest, CanvasTool, DocumentAction, DocumentTabSnapshot,
    DocumentWorkspaceSnapshot,
};
use lumapaint_core::document::{
    Brush, ColorMode, ColorProfile, Document, DocumentSettings, DocumentSnapshot, LayerSettings,
    PaintProjectionState, SelectionMode, SelectionShape, Stroke, TextSettings,
};
use lumapaint_core::tiles::TiledRasterDocument;
use lumapaint_core::vector::{
    FillRule, PathOperation, VectorObject, VectorObjectKind, VectorPaint, VectorPath,
};
use lumapaint_renderer::{
    paint_stroke_into_tiles_at_scale, prepare_svg_layer, project_committed_paint_layer_at_scale,
    stroke_candidate_tile_coords_at_scale, update_projected_paint_appearance, validate_svg, wgpu,
    PreparedSvgLayer, Renderer, ValidatedTileUploads, Viewport,
};
use objc2::{
    define_class, msg_send, rc::Retained, runtime::AnyObject, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{NSCursor, NSEvent, NSEventModifierFlags, NSEventSubtype, NSView};
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
            while let Ok(mut job) = receiver.recv() {
                // At most one request waits while a raster is running. Keep the newest.
                while let Ok(newer) = receiver.try_recv() {
                    job = newer;
                }
                let queued_for = job.queued_at.elapsed();
                let started = Instant::now();
                let result: Result<Vec<PreparedSvgLayer>, String> = job
                    .layers
                    .iter()
                    .map(|layer| prepare_svg_layer(layer, job.size.0, job.size.1))
                    .collect();
                let cpu_time = started.elapsed();
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
                            let uploads =
                                changed.as_ref().map_or(Ok(Vec::new()), |invalidation| {
                                    cache.tiles.prepare_uploads(invalidation)
                                })?;
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
                    _ => None,
                }
            };
            if let Some(cursor) = cursor { self.addCursorRect_cursor(self.bounds(), &cursor); }
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
            else if event.keyCode() == 7 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if !event.isARepeat() {
                    if let Some(app) = APP.get() { let _ = app.emit_to("main", "canvas-swap-colors", ()); }
                }
            }
            else if event.keyCode() == 17 && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) {
                if !event.isARepeat() {
                    if let Some(app) = APP.get() { let _ = app.emit_to("main", "canvas-text-edit", ()); }
                }
            }
            else if event.keyCode() == 53 {
                if cancel_vector_drag() || DOCUMENT.with(|doc| doc.borrow_mut().cancel_selection_gesture()) {
                    if let Err(error) = redraw() { emit_error(error); }
                    emit_document();
                } else { report_edit(DocumentAction::Deselect); }
            }
            else if !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) && [11, 46].contains(&event.keyCode()) {
                let tool = if event.keyCode() == 11 { CanvasTool::Brush }
                    else if event.modifierFlags().contains(NSEventModifierFlags::Shift) { CanvasTool::Ellipse }
                    else { CanvasTool::Rectangle };
                cancel_vector_drag();
                TOOL.with(|value| value.set(tool));
                if let Some(app) = APP.get() { let _ = app.emit_to("main", "canvas-tool-changed", tool); }
            }
            else if !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option) && [9, 32, 35].contains(&event.keyCode()) {
                let tool = match event.keyCode() {
                    9 => CanvasTool::VectorSelect,
                    35 => CanvasTool::VectorPen,
                    _ if event.modifierFlags().contains(NSEventModifierFlags::Shift) => CanvasTool::VectorEllipse,
                    _ => CanvasTool::VectorRectangle,
                };
                cancel_vector_drag();
                TOOL.with(|value| value.set(tool));
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
            if command && [0, 2].contains(&event.keyCode()) {
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
        if phase == 0 {
            if let Err(error) = text_editor::finish(true) {
                emit_error(error);
                return;
            }
            let tool = TOOL.with(|tool| tool.get());
            if tool == CanvasTool::Text
                || (tool == CanvasTool::VectorSelect && event.clickCount() == 2)
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
                        | CanvasTool::VectorPen
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
                        .with(|brush| doc.begin_with_pressure(point, *brush.borrow(), pressure))
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

fn vector_pointer(
    document: &mut Document,
    tool: CanvasTool,
    point: lumapaint_core::document::Point,
    phase: u8,
    modifiers: NSEventModifierFlags,
) -> Result<(), String> {
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
    if tool == CanvasTool::VectorPen && phase == 1 {
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
    if tool == CanvasTool::VectorPen {
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
    if tool != CanvasTool::VectorPen && (width < 1.0 || height < 1.0) {
        return Ok(());
    }

    let (name, path, fill, stroke, stroke_width, kind, control_points) = match tool {
        CanvasTool::VectorPen => {
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
    let layer_id = match document.editable_vector_layer_id() {
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

fn vector_select_pointer(
    document: &mut Document,
    point: lumapaint_core::document::Point,
    phase: u8,
    modifiers: NSEventModifierFlags,
) -> Result<(), String> {
    if phase == 0 {
        cancel_vector_drag();
        VECTOR_CONTROL
            .with(|control| *control.borrow_mut() = document.selected_control_at(point, 6.0));
        if VECTOR_CONTROL.with(|control| control.borrow().is_some()) {
            VECTOR_DRAFT.with(|draft| {
                let mut draft = draft.borrow_mut();
                draft.clear();
                draft.push(point);
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
        if VECTOR_CONTROL.with(|control| control.borrow().is_none()) {
            let start = VECTOR_DRAFT.with(|draft| draft.borrow().first().copied());
            if let Some(start) = start {
                VECTOR_MOVE.with(|offset| offset.set([point.x - start.x, point.y - start.y]));
            }
        }
    } else if phase == 2 {
        VECTOR_MOVE.with(|offset| offset.set([0.0, 0.0]));
        if let Some((id, index)) = VECTOR_CONTROL.with(|control| control.borrow_mut().take()) {
            VECTOR_DRAFT.with(|draft| draft.borrow_mut().clear());
            return document.move_vector_control(&id, index, point);
        }
        let start = VECTOR_DRAFT.with(|draft| std::mem::take(&mut *draft.borrow_mut()));
        if let Some(start) = start.first() {
            document.move_selected_vectors(point.x - start.x, point.y - start.y)?;
        }
    }
    Ok(())
}

fn cancel_vector_drag() -> bool {
    let moving = VECTOR_MOVE.with(|offset| offset.replace([0.0, 0.0]) != [0.0, 0.0]);
    VECTOR_DRAFT.with(|draft| draft.borrow_mut().clear());
    VECTOR_CONTROL.with(|control| control.borrow_mut().take());
    moving
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
    if let Some(app) = APP.get() {
        let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
        let _ = app.emit_to("main", "document-changed", snapshot);
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
            _ => {
                text_editor::finish(true)?;
                return edit(action);
            }
        }
        return Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()));
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
    DOCUMENT.with(|doc| doc.borrow_mut().add_paint_layer())?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|doc| doc.borrow().snapshot()))
}
pub fn add_vector_layer() -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().add_vector_layer())?;
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
pub fn select_vector_objects(ids: Vec<String>) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().select_vector_objects(ids))?;
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
    if text_editor::active() || VECTOR_MOVE.with(|offset| offset.get()) != [0.0, 0.0] {
        return text_editor::render(canvas);
    }
    DOCUMENT.with(|document| {
        let document = document.borrow();
        refresh_tile_preview(canvas, &document);
        if RASTER_SENDER.get().is_some() {
            schedule_raster_job(canvas, &document)?;
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
    if document.has_active_stroke() || document.committed_paint_strokes().next().is_none() {
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
            "LumaPaint tile {} scale={} changed_tiles={} queue_ms={:.2} cpu_ms={:.2} upload_ms={:.2}{}",
            if incremental { "incremental" } else { "projection" },
            key.scale,
            changed_tiles,
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
    tile_job: Option<TileKey>,
    tile_ready: Option<TileKey>,
    tile_failure: Option<TileKey>,
    tile_cache: Option<TileCache>,
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
    static VECTOR_DRAFT: RefCell<Vec<lumapaint_core::document::Point>> = const { RefCell::new(Vec::new()) };
    static VECTOR_CONTROL: RefCell<Option<(String, usize)>> = const { RefCell::new(None) };
    static NEXT_VECTOR_OBJECT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

struct OpenDocument {
    id: u64,
    document: Document,
    path: Option<std::path::PathBuf>,
    fingerprint: Option<crate::project_file::FileFingerprint>,
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
    let document = DOCUMENT.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    let path = PROJECT_PATH.with(|slot| slot.borrow_mut().take());
    let fingerprint = PROJECT_FINGERPRINT.with(|slot| slot.borrow_mut().take());
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    INACTIVE_DOCUMENTS.with(|documents| {
        documents.borrow_mut().push(OpenDocument {
            id,
            document,
            path,
            fingerprint,
        });
    });
}

fn activate_document(entry: OpenDocument) {
    cancel_vector_drag();
    DOCUMENT.with(|slot| *slot.borrow_mut() = entry.document);
    PROJECT_PATH.with(|slot| *slot.borrow_mut() = entry.path);
    PROJECT_FINGERPRINT.with(|slot| *slot.borrow_mut() = entry.fingerprint);
    ACTIVE_DOCUMENT_ID.with(|active| active.set(entry.id));
    DOCUMENT_OPEN.with(|open| open.set(true));
    CHECKPOINT_REVISION.with(|last| last.set(None));
}

fn document_tab(id: u64, document: &Document) -> DocumentTabSnapshot {
    let snapshot = document.snapshot();
    DocumentTabSnapshot {
        id,
        file_name: snapshot.file_name.or(Some(snapshot.name)),
        dirty: snapshot.dirty,
    }
}

pub fn workspace_snapshot() -> DocumentWorkspaceSnapshot {
    let mut documents = INACTIVE_DOCUMENTS.with(|items| {
        items
            .borrow()
            .iter()
            .map(|entry| document_tab(entry.id, &entry.document))
            .collect::<Vec<_>>()
    });
    let open = DOCUMENT_OPEN.with(|value| value.get());
    let active_id = open.then(|| ACTIVE_DOCUMENT_ID.with(|value| value.get()));
    let active = open.then(|| DOCUMENT.with(|doc| doc.borrow().snapshot()));
    if let (Some(id), Some(document)) = (active_id, active.as_ref()) {
        documents.push(DocumentTabSnapshot {
            id,
            file_name: document.file_name.clone().or(Some(document.name.clone())),
            dirty: document.dirty,
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
        cancel_vector_drag();
        text_editor::finish(true)?;
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
    let document = DOCUMENT.with(|document| document.borrow().snapshot());
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
                        tile_job: None,
                        tile_ready: None,
                        tile_failure: None,
                        tile_cache: None,
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
        if tool_changed {
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
            document: DOCUMENT_OPEN
                .with(|open| open.get())
                .then(|| DOCUMENT.with(|doc| doc.borrow().snapshot())),
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
    let active_dirty =
        DOCUMENT_OPEN.with(|open| open.get()) && DOCUMENT.with(|doc| doc.borrow().snapshot().dirty);
    let inactive_dirty = INACTIVE_DOCUMENTS.with(|documents| {
        documents
            .borrow()
            .iter()
            .any(|entry| entry.document.snapshot().dirty)
    });
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
    cancel_vector_drag();
    text_editor::finish(true)?;
    DOCUMENT_OPEN
        .with(|open| open.get())
        .then_some(())
        .ok_or_else(|| "No document is open".into())
}

pub fn new_document() -> Result<DocumentWorkspaceSnapshot, String> {
    text_editor::finish(true)?;
    park_active_document();
    activate_document(OpenDocument {
        id: next_document_id(),
        document: Document::default(),
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
        let dirty = DOCUMENT.with(|doc| doc.borrow().snapshot().dirty);
        if dirty && !confirm_unsaved_changes() {
            return Ok(workspace_snapshot());
        }
        DOCUMENT.with(|doc| *doc.borrow_mut() = Document::default());
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
            if documents[index].document.snapshot().dirty && !confirm_unsaved_changes() {
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
            let (loaded, fingerprint) = crate::project_file::read_with_fingerprint(&path)?;
            for layer in loaded.svg_layers() {
                validate_svg(&layer.source)?;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let mut loaded = loaded;
            loaded.mark_saved(name);
            park_active_document();
            activate_document(OpenDocument {
                id: next_document_id(),
                document: loaded,
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
                let bytes = DOCUMENT.with(|doc| doc.borrow_mut().encode())?;
                crate::project_file::write(&path, &bytes)?;
                let fingerprint = crate::project_file::fingerprint(&path)?;
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                DOCUMENT.with(|doc| doc.borrow_mut().mark_saved(name));
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
        document: recovered,
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
