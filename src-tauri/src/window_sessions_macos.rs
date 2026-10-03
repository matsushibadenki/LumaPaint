//! Main-thread Rust ownership of independent editor sessions. Native views and
//! WebViews only identify a session; documents and GPU surfaces stay here.
use super::*;
use std::collections::{HashMap, HashSet};

struct Runtime {
    window_title: String,
    outline_view: bool,
    frame_requested: bool,
    canvas_overlay: Vec<[f64; 4]>,
    brush_cursor: Option<Retained<NSCursor>>,
    brush_cursor_key: (u32, bool),
    canvas: Option<Canvas>,
    document: Document,
    active_tiled_document: Option<TiledSession>,
    brush: Brush,
    tool: CanvasTool,
    document_open: bool,
    active_document_id: u64,
    inactive_documents: Vec<OpenDocument>,
    pan: (f32, f32),
    last_pan_point: (f32, f32),
    panning: bool,
    space_down: bool,
    paint_mouse_coalescing: Option<bool>,
    pixel_paint_commit: Option<PreparedSvgLayer>,
    pixel_paint: Option<pixel_paint::PixelPaint>,
    pixel_drag: Option<pixel_move::PixelDrag>,
    box_draft: Option<BoxDraft>,
    rotate_draft: Option<RotateDraft>,
    scale_draft: Option<ScaleDraft>,
    vector_marquee: Option<VectorMarquee>,
    vector_duplicate: bool,
    vector_move: [f32; 2],
    direct_points: Vec<(String, usize)>,
    direct_gesture: Option<DirectGesture>,
    anchor_draft: Option<AnchorDraft>,
    pen_draft: Option<PenDraft>,
    guide_draft: Option<(String, f32)>,
    guide_drag: Option<(String, [f32; 2])>,
    guide_object_draft: Vec<lumapaint_renderer::FrameOverlay>,
    vector_draft: Vec<lumapaint_core::document::Point>,
    text_frame_draft: Option<([f32; 2], [f32; 2])>,
    text_resize_draft: Option<TextResizeDraft>,
    vector_control: Option<(String, usize)>,
    project_path: Option<std::path::PathBuf>,
    project_fingerprint: Option<crate::project_file::FileFingerprint>,
    recovery: Option<crate::recovery::Recovery>,
    recovery_error: Option<String>,
    checkpoint_revision: Option<(u64, bool)>,
    recovery_discarded: bool,
    text: text_editor::WindowContext,
    placement: raster_import::WindowContext,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            window_title: String::new(),
            outline_view: false,
            frame_requested: false,
            canvas_overlay: Vec::new(),
            brush_cursor: None,
            brush_cursor_key: (0, false),
            canvas: None,
            document: {
                let mut document = Document::default();
                lumapaint_svg::attach(&mut document);
                let _ = document.select_layer("layer-1".into());
                document
            },
            active_tiled_document: None,
            brush: Brush::default(),
            tool: CanvasTool::Brush,
            document_open: false,
            active_document_id: 1,
            inactive_documents: Vec::new(),
            pan: (0.0, 0.0),
            last_pan_point: (0.0, 0.0),
            panning: false,
            space_down: false,
            paint_mouse_coalescing: None,
            pixel_paint_commit: None,
            pixel_paint: None,
            pixel_drag: None,
            box_draft: None,
            rotate_draft: None,
            scale_draft: None,
            vector_marquee: None,
            vector_duplicate: false,
            vector_move: [0.0, 0.0],
            direct_points: Vec::new(),
            direct_gesture: None,
            anchor_draft: None,
            pen_draft: None,
            guide_draft: None,
            guide_drag: None,
            guide_object_draft: Vec::new(),
            vector_draft: Vec::new(),
            text_frame_draft: None,
            text_resize_draft: None,
            vector_control: None,
            project_path: None,
            project_fingerprint: None,
            recovery: None,
            recovery_error: None,
            checkpoint_revision: None,
            recovery_discarded: false,
            text: Default::default(),
            placement: Default::default(),
        }
    }
}
impl Runtime {
    fn exchange(&mut self) {
        WINDOW_TITLE.with(|slot| std::mem::swap(&mut self.window_title, &mut *slot.borrow_mut()));
        OUTLINE_VIEW.with(|slot| self.outline_view = slot.replace(self.outline_view));
        FRAME_REQUESTED.with(|slot| self.frame_requested = slot.replace(self.frame_requested));
        CANVAS_OVERLAY.with(|slot| {
            self.canvas_overlay = slot.replace(std::mem::take(&mut self.canvas_overlay))
        });
        BRUSH_CURSOR.with(|slot| std::mem::swap(&mut self.brush_cursor, &mut *slot.borrow_mut()));
        BRUSH_CURSOR_KEY.with(|slot| self.brush_cursor_key = slot.replace(self.brush_cursor_key));
        CANVAS.with(|slot| std::mem::swap(&mut self.canvas, &mut *slot.borrow_mut()));
        DOCUMENT.with(|slot| std::mem::swap(&mut self.document, &mut *slot.borrow_mut()));
        ACTIVE_TILED_DOCUMENT
            .with(|slot| std::mem::swap(&mut self.active_tiled_document, &mut *slot.borrow_mut()));
        BRUSH.with(|slot| std::mem::swap(&mut self.brush, &mut *slot.borrow_mut()));
        TOOL.with(|slot| self.tool = slot.replace(self.tool));
        DOCUMENT_OPEN.with(|slot| self.document_open = slot.replace(self.document_open));
        ACTIVE_DOCUMENT_ID
            .with(|slot| self.active_document_id = slot.replace(self.active_document_id));
        INACTIVE_DOCUMENTS
            .with(|slot| std::mem::swap(&mut self.inactive_documents, &mut *slot.borrow_mut()));
        PAN.with(|slot| self.pan = slot.replace(self.pan));
        LAST_PAN_POINT.with(|slot| self.last_pan_point = slot.replace(self.last_pan_point));
        PANNING.with(|slot| self.panning = slot.replace(self.panning));
        SPACE_DOWN.with(|slot| self.space_down = slot.replace(self.space_down));
        PAINT_MOUSE_COALESCING
            .with(|slot| self.paint_mouse_coalescing = slot.replace(self.paint_mouse_coalescing));
        PIXEL_PAINT_COMMIT
            .with(|slot| std::mem::swap(&mut self.pixel_paint_commit, &mut *slot.borrow_mut()));
        PIXEL_PAINT.with(|slot| std::mem::swap(&mut self.pixel_paint, &mut *slot.borrow_mut()));
        PIXEL_DRAG.with(|slot| std::mem::swap(&mut self.pixel_drag, &mut *slot.borrow_mut()));
        BOX_DRAFT.with(|slot| std::mem::swap(&mut self.box_draft, &mut *slot.borrow_mut()));
        ROTATE_DRAFT.with(|slot| std::mem::swap(&mut self.rotate_draft, &mut *slot.borrow_mut()));
        SCALE_DRAFT.with(|slot| std::mem::swap(&mut self.scale_draft, &mut *slot.borrow_mut()));
        VECTOR_MARQUEE
            .with(|slot| std::mem::swap(&mut self.vector_marquee, &mut *slot.borrow_mut()));
        VECTOR_DUPLICATE.with(|slot| self.vector_duplicate = slot.replace(self.vector_duplicate));
        VECTOR_MOVE.with(|slot| self.vector_move = slot.replace(self.vector_move));
        DIRECT_POINTS.with(|slot| std::mem::swap(&mut self.direct_points, &mut *slot.borrow_mut()));
        DIRECT_GESTURE
            .with(|slot| std::mem::swap(&mut self.direct_gesture, &mut *slot.borrow_mut()));
        ANCHOR_DRAFT.with(|slot| std::mem::swap(&mut self.anchor_draft, &mut *slot.borrow_mut()));
        GUIDE_DRAFT.with(|slot| std::mem::swap(&mut self.guide_draft, &mut *slot.borrow_mut()));
        GUIDE_DRAG.with(|slot| std::mem::swap(&mut self.guide_drag, &mut *slot.borrow_mut()));
        GUIDE_OBJECT_DRAFT
            .with(|slot| std::mem::swap(&mut self.guide_object_draft, &mut *slot.borrow_mut()));
        PEN_DRAFT.with(|slot| std::mem::swap(&mut self.pen_draft, &mut *slot.borrow_mut()));
        VECTOR_DRAFT.with(|slot| std::mem::swap(&mut self.vector_draft, &mut *slot.borrow_mut()));
        TEXT_FRAME_DRAFT
            .with(|slot| std::mem::swap(&mut self.text_frame_draft, &mut *slot.borrow_mut()));
        TEXT_RESIZE_DRAFT
            .with(|slot| std::mem::swap(&mut self.text_resize_draft, &mut *slot.borrow_mut()));
        VECTOR_CONTROL
            .with(|slot| std::mem::swap(&mut self.vector_control, &mut *slot.borrow_mut()));
        PROJECT_PATH.with(|slot| std::mem::swap(&mut self.project_path, &mut *slot.borrow_mut()));
        PROJECT_FINGERPRINT
            .with(|slot| std::mem::swap(&mut self.project_fingerprint, &mut *slot.borrow_mut()));
        RECOVERY.with(|slot| std::mem::swap(&mut self.recovery, &mut *slot.borrow_mut()));
        RECOVERY_ERROR
            .with(|slot| std::mem::swap(&mut self.recovery_error, &mut *slot.borrow_mut()));
        CHECKPOINT_REVISION
            .with(|slot| self.checkpoint_revision = slot.replace(self.checkpoint_revision));
        RECOVERY_DISCARDED
            .with(|slot| self.recovery_discarded = slot.replace(self.recovery_discarded));
        self.text.exchange();
        self.placement.exchange();
    }
}
thread_local! {
    static CURRENT: RefCell<String> = RefCell::new("main".into());
    static PARKED: RefCell<HashMap<String, Runtime>> = RefCell::new(HashMap::new());
    static CLOSED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}
pub(in crate::canvas) fn current_label() -> String {
    CURRENT.with(|slot| slot.borrow().clone())
}

fn activate(label: &str) {
    let previous = current_label();
    if previous == label {
        return;
    }
    let (mut runtime, fresh) = PARKED.with(|sessions| match sessions.borrow_mut().remove(label) {
        Some(runtime) => (runtime, false),
        None => (
            Runtime {
                active_document_id: next_document_id(),
                ..Default::default()
            },
            true,
        ),
    });
    runtime.exchange();
    CURRENT.with(|slot| *slot.borrow_mut() = label.into());
    if !CLOSED.with(|closed| closed.borrow().contains(&previous)) {
        PARKED.with(|sessions| {
            sessions.borrow_mut().insert(previous, runtime);
        });
    }
    if fresh {
        start_recovery();
    }
}

/// Restores the caller even across AppKit nested event loops (dialogs/text drag).
/// Exchange never clones documents or pixel payloads.
pub(in crate::canvas) struct SessionGuard {
    previous: String,
}
impl SessionGuard {
    pub(in crate::canvas) fn enter(label: &str) -> Result<Self, String> {
        if SHUTTING_DOWN.with(|flag| flag.get())
            || CLOSED.with(|closed| closed.borrow().contains(label))
        {
            return Err("Editor window is closed".into());
        }
        let previous = current_label();
        activate(label);
        Ok(Self { previous })
    }
}
impl Drop for SessionGuard {
    fn drop(&mut self) {
        if !SHUTTING_DOWN.with(|flag| flag.get())
            && !CLOSED.with(|closed| closed.borrow().contains(&self.previous))
        {
            activate(&self.previous);
        }
    }
}

pub(in crate::canvas) fn for_canvas(token: u64) -> Option<SessionGuard> {
    let active = CANVAS.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|canvas| canvas.token == token)
    });
    let label = if active {
        Some(current_label())
    } else {
        PARKED.with(|sessions| {
            sessions.borrow().iter().find_map(|(label, runtime)| {
                runtime
                    .canvas
                    .as_ref()
                    .filter(|canvas| canvas.token == token)
                    .map(|_| label.clone())
            })
        })
    }?;
    SessionGuard::enter(&label).ok()
}

pub(in crate::canvas) fn confirm_window(label: &str) -> bool {
    let Ok(_session) = SessionGuard::enter(label) else {
        return true;
    };
    confirm_discard()
}
pub(in crate::canvas) fn confirm_all() -> bool {
    let Some(app) = APP.get() else {
        return confirm_discard();
    };
    app.webview_windows()
        .keys()
        .filter(|label| crate::editor_windows::is_editor(label))
        .all(|label| confirm_window(label))
}
pub(in crate::canvas) fn close_window(label: &str) {
    close(label, true);
}
fn close(label: &str, discard: bool) {
    if CLOSED.with(|closed| closed.borrow().contains(label)) {
        return;
    }
    {
        let Ok(_session) = SessionGuard::enter(label) else {
            return;
        };
        if discard {
            discard_recovery();
        }
        destroy();
        RECOVERY.with(|slot| {
            if let Some(mut recovery) = slot.borrow_mut().take() {
                recovery.stop();
            }
        });
    }
    CLOSED.with(|closed| {
        closed.borrow_mut().insert(label.into());
    });
    PARKED.with(|sessions| {
        sessions.borrow_mut().remove(label);
    });
    // The last closed window may still occupy the live slots. Release its large
    // document/preview payloads even if macOS keeps the application running.
    if current_label() == label {
        Runtime::default().exchange();
    }
}
pub(in crate::canvas) fn shutdown_all() {
    let labels = APP
        .get()
        .map(|app| app.webview_windows().into_keys().collect::<Vec<_>>())
        .unwrap_or_else(|| vec![current_label()]);
    for label in labels {
        if crate::editor_windows::is_editor(&label) {
            close(&label, false);
        }
    }
    SHUTTING_DOWN.with(|flag| flag.set(true));
}

pub(in crate::canvas) fn discard_all() {
    if let Some(app) = APP.get() {
        for label in app
            .webview_windows()
            .keys()
            .filter(|label| crate::editor_windows::is_editor(label))
        {
            if let Ok(_session) = SessionGuard::enter(label) {
                discard_recovery();
            }
        }
    }
}

/// All windows share one profile lock; each writer has its own recovery copy.
pub(super) fn fork_recovery() -> Option<Result<crate::recovery::Recovery, String>> {
    PARKED.with(|sessions| {
        sessions.borrow().values().find_map(|runtime| {
            runtime
                .recovery
                .as_ref()
                .map(crate::recovery::Recovery::fork)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(name: &str) -> NewDocumentSettings {
        NewDocumentSettings {
            pages: None,
            document: DocumentSettings {
                name: name.into(),
                width: 320,
                height: 240,
                unit: DocumentUnit::Pixels,
                resolution: 72,
                artboards: true,
                canvas_color: CanvasColor::White,
                pixel_aspect_ratio: 1.0,
            },
            color_mode: ColorMode::Rgb,
            color_profile: ColorProfile::Srgb,
            bit_depth: 8,
        }
    }

    #[test]
    fn windows_keep_independent_documents_tabs_history_and_view_state() {
        let original = current_label();
        let a = SessionGuard::enter("editor-90001").unwrap();
        assert!(workspace_snapshot().active.is_none());
        let first = new_document(preset("A")).unwrap().active_id.unwrap();
        DOCUMENT.with(|doc| {
            let layer = doc.borrow_mut().add_vector_layer().unwrap();
            doc.borrow_mut().select_layer(layer).unwrap();
            doc.borrow_mut()
                .set_text_object(TextSettings {
                    id: None,
                    text: lumapaint_core::vector::VectorText {
                        content: "Window A".into(),
                        ..Default::default()
                    },
                    position: [10., 20.],
                    color: [0, 0, 0],
                })
                .unwrap();
        });
        PAN.with(|pan| pan.set((55., 66.)));
        TOOL.with(|tool| tool.set(CanvasTool::TextVertical));
        OUTLINE_VIEW.with(|view| view.set(true));
        let second = new_document(preset("A second tab"))
            .unwrap()
            .active_id
            .unwrap();
        PAN.with(|pan| pan.set((55., 66.)));
        let b = SessionGuard::enter("editor-90002").unwrap();
        assert!(workspace_snapshot().documents.is_empty());
        assert_eq!(PAN.with(|pan| pan.get()), (0., 0.));
        assert!(!OUTLINE_VIEW.with(|view| view.get()));
        let third = new_document(preset("B")).unwrap().active_id.unwrap();
        assert_ne!(third, first);
        assert!(switch_document(first).is_err());
        PAN.with(|pan| pan.set((-10., -20.)));
        drop(b);
        assert_eq!(current_label(), "editor-90001");
        assert_eq!(workspace_snapshot().active_id, Some(second));
        assert_eq!(workspace_snapshot().documents.len(), 2);
        assert_eq!(PAN.with(|pan| pan.get()), (55., 66.));
        assert!(matches!(
            TOOL.with(|tool| tool.get()),
            CanvasTool::TextVertical
        ));
        assert!(OUTLINE_VIEW.with(|view| view.get()));
        switch_document(first).unwrap();
        let snapshot = DOCUMENT.with(|doc| doc.borrow().snapshot());
        assert!(snapshot.can_undo);
        assert_eq!(snapshot.text_objects[0].text.content, "Window A");
        close_window("editor-90002");
        assert_eq!(workspace_snapshot().active_id, Some(first));
        assert!(SessionGuard::enter("editor-90002").is_err());
        drop(a);
        assert_eq!(current_label(), original);
    }

    #[test]
    fn nested_callbacks_restore_outer_edits_without_cloning_document_storage() {
        let outer = SessionGuard::enter("editor-90003").unwrap();
        DOCUMENT.with(|doc| {
            let layer = doc.borrow_mut().add_vector_layer().unwrap();
            doc.borrow_mut().select_layer(layer).unwrap();
            doc.borrow_mut()
                .set_text_object(TextSettings {
                    id: None,
                    text: lumapaint_core::vector::VectorText {
                        content: "Outer".into(),
                        ..Default::default()
                    },
                    position: [10., 20.],
                    color: [0, 0, 0],
                })
                .unwrap()
        });
        let address =
            DOCUMENT.with(|doc| doc.borrow().svg_layers().next().unwrap().source.as_ptr());
        let inner = SessionGuard::enter("editor-90004").unwrap();
        assert!(DOCUMENT.with(|doc| doc.borrow().snapshot().text_objects.is_empty()));
        drop(inner);
        assert_eq!(current_label(), "editor-90003");
        assert_eq!(
            DOCUMENT.with(|doc| doc.borrow().svg_layers().next().unwrap().source.as_ptr()),
            address
        );
        assert!(for_canvas(u64::MAX).is_none());
        drop(outer);
    }
}
