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
    load_shared_active();
    if fresh {
        start_recovery();
    }
}

// A shared document is owned by exactly one Rust runtime, or parked here.
// Other windows hold its ID and their own viewport, never a document clone.
struct SharedDocument {
    owner: Option<String>,
    parked: Option<OpenDocument>,
    views: HashSet<String>,
    snapshot: DocumentSnapshot,
    tiled: bool,
    recovery: Option<crate::recovery::Recovery>,
    checkpoint: Option<(u64, bool)>,
}
thread_local! {
    static SHARED: RefCell<HashMap<u64, SharedDocument>> = RefCell::new(HashMap::new());
}
pub(super) fn shared_snapshot(id: u64) -> Option<DocumentSnapshot> {
    SHARED.with(|shared| {
        shared
            .borrow()
            .get(&id)
            .map(|record| record.snapshot.clone())
    })
}
pub(super) fn shared_checkpoint() -> bool {
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    SHARED.with(|shared| {
        let mut shared = shared.borrow_mut();
        let Some(record) = shared.get_mut(&id) else {
            return false;
        };
        let Some(recovery) = &record.recovery else {
            return true;
        };
        let snapshot = ACTIVE_TILED_DOCUMENT.with(|slot| {
            slot.borrow().as_ref().map_or_else(
                || DOCUMENT.with(|doc| doc.borrow().snapshot()),
                TiledSession::snapshot,
            )
        });
        let key = (snapshot.revision, snapshot.dirty);
        if record.checkpoint == Some(key) {
            return true;
        }
        ACTIVE_TILED_DOCUMENT.with(|slot| {
            if let Some(document) = slot.borrow().as_ref() {
                recovery.checkpoint_project(
                    crate::project_file::ProjectData::Tiled(document.document.state()),
                    key.0,
                    key.1,
                );
            } else {
                recovery.checkpoint(DOCUMENT.with(|doc| doc.borrow().clone()));
            }
        });
        record.checkpoint = Some(key);
        true
    })
}
pub(super) fn shared_recovery_info() -> Option<Result<crate::recovery::Info, String>> {
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    SHARED.with(|shared| {
        shared.borrow().get(&id).and_then(|record| {
            record
                .recovery
                .as_ref()
                .map(crate::recovery::Recovery::info)
        })
    })
}
pub(super) fn retry_shared_recovery() {
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    SHARED.with(|shared| {
        if let Some(record) = shared.borrow_mut().get_mut(&id) {
            record.checkpoint = None;
        }
    });
}
pub(super) fn shared_tab(id: u64) -> Option<DocumentTabSnapshot> {
    SHARED.with(|shared| {
        shared.borrow().get(&id).map(|entry| DocumentTabSnapshot {
            id,
            file_name: entry
                .snapshot
                .file_name
                .clone()
                .or(Some(entry.snapshot.name.clone())),
            dirty: entry.snapshot.dirty,
            format: if entry.tiled { "tiled" } else { "legacy" },
        })
    })
}
pub(super) fn shared_has_other_views(id: u64) -> bool {
    SHARED.with(|shared| {
        shared
            .borrow()
            .get(&id)
            .is_some_and(|entry| entry.views.len() > 1)
    })
}
fn take_live_document(id: u64) -> OpenDocument {
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
    OpenDocument {
        id,
        content,
        path: PROJECT_PATH.with(|slot| slot.borrow_mut().take()),
        fingerprint: PROJECT_FINGERPRINT.with(|slot| slot.borrow_mut().take()),
    }
}
pub(super) fn park_shared_active(id: u64) -> bool {
    let is_shared = SHARED.with(|shared| shared.borrow().contains_key(&id));
    if !is_shared {
        return false;
    }
    refresh_shared_snapshot();
    let entry = take_live_document(id);
    SHARED.with(|shared| {
        let mut shared = shared.borrow_mut();
        let record = shared.get_mut(&id).unwrap();
        record.owner = None;
        record.parked = Some(entry);
    });
    true
}
pub(super) fn take_shared(id: u64) -> OpenDocument {
    let (parked, owner) = SHARED.with(|shared| {
        let mut shared = shared.borrow_mut();
        let record = shared.get_mut(&id).expect("shared document exists");
        (record.parked.take(), record.owner.take())
    });
    let entry = if let Some(entry) = parked {
        entry
    } else {
        let owner = owner.expect("shared document has an owner");
        if owner == current_label() {
            take_live_document(id)
        } else {
            PARKED.with(|sessions| {
                let mut sessions = sessions.borrow_mut();
                let runtime = sessions
                    .get_mut(&owner)
                    .expect("shared owner runtime exists");
                let content = runtime.active_tiled_document.take().map_or_else(
                    || OpenDocumentContent::Legacy(Box::new(std::mem::take(&mut runtime.document))),
                    OpenDocumentContent::Tiled,
                );
                OpenDocument {
                    id,
                    content,
                    path: runtime.project_path.take(),
                    fingerprint: runtime.project_fingerprint.take(),
                }
            })
        }
    };
    SHARED.with(|shared| shared.borrow_mut().get_mut(&id).unwrap().owner = Some(current_label()));
    entry
}
fn load_shared_active() {
    if !DOCUMENT_OPEN.with(|open| open.get()) {
        return;
    }
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    let needs_load = SHARED.with(|shared| {
        shared
            .borrow()
            .get(&id)
            .is_some_and(|record| record.owner.as_deref() != Some(current_label().as_str()))
    });
    if needs_load {
        let entry = take_shared(id);
        match entry.content {
            OpenDocumentContent::Legacy(document) => {
                DOCUMENT.with(|slot| *slot.borrow_mut() = *document);
                ACTIVE_TILED_DOCUMENT.with(|slot| slot.borrow_mut().take());
            }
            OpenDocumentContent::Tiled(document) => {
                DOCUMENT.with(|slot| *slot.borrow_mut() = Document::default());
                ACTIVE_TILED_DOCUMENT.with(|slot| *slot.borrow_mut() = Some(document));
            }
            OpenDocumentContent::Shared(_) => unreachable!("canonical content cannot be a view"),
        }
        PROJECT_PATH.with(|slot| *slot.borrow_mut() = entry.path);
        PROJECT_FINGERPRINT.with(|slot| *slot.borrow_mut() = entry.fingerprint);
    }
}
pub(super) fn refresh_shared_snapshot() {
    if !DOCUMENT_OPEN.with(|open| open.get()) {
        return;
    }
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    if !SHARED.with(|shared| shared.borrow().contains_key(&id)) {
        return;
    }
    let snapshot = ACTIVE_TILED_DOCUMENT.with(|slot| {
        slot.borrow().as_ref().map_or_else(
            || DOCUMENT.with(|doc| doc.borrow().snapshot()),
            TiledSession::snapshot,
        )
    });
    SHARED.with(|shared| shared.borrow_mut().get_mut(&id).unwrap().snapshot = snapshot);
}
pub(super) fn shared_changed() {
    refresh_shared_snapshot();
    let id = ACTIVE_DOCUMENT_ID.with(|active| active.get());
    let current = current_label();
    let Some((snapshot, labels)) = SHARED.with(|shared| {
        shared
            .borrow()
            .get(&id)
            .map(|record| (record.snapshot.clone(), record.views.clone()))
    }) else {
        return;
    };
    if let Some(app) = APP.get() {
        PARKED.with(|sessions| {
            for label in labels.iter().filter(|label| **label != current) {
                if let Some(runtime) = sessions.borrow_mut().get_mut(label) {
                    let is_active = runtime.document_open && runtime.active_document_id == id;
                    let mut tabs: Vec<_> = runtime
                        .inactive_documents
                        .iter()
                        .map(|entry| document_tab(entry.id, &entry.content))
                        .collect();
                    let active = if is_active {
                        Some(snapshot.clone())
                    } else if runtime.document_open {
                        Some(
                            shared_snapshot(runtime.active_document_id).unwrap_or_else(|| {
                                runtime.active_tiled_document.as_ref().map_or_else(
                                    || runtime.document.snapshot(),
                                    TiledSession::snapshot,
                                )
                            }),
                        )
                    } else {
                        None
                    };
                    if let Some(active) = &active {
                        tabs.push(DocumentTabSnapshot {
                            id: runtime.active_document_id,
                            file_name: active.file_name.clone().or(Some(active.name.clone())),
                            dirty: active.dirty,
                            format: if shared_tab(runtime.active_document_id)
                                .is_some_and(|tab| tab.format == "tiled")
                                || runtime.active_tiled_document.is_some()
                            {
                                "tiled"
                            } else {
                                "legacy"
                            },
                        });
                    }
                    tabs.sort_by_key(|tab| tab.id);
                    let _ = app.emit_to(
                        label,
                        "documents-changed",
                        DocumentWorkspaceSnapshot {
                            active_id: runtime.document_open.then_some(runtime.active_document_id),
                            active,
                            documents: tabs,
                        },
                    );
                    if is_active {
                        runtime.frame_requested = true;
                        let title = format!(
                            "{}{} — LumaPaint",
                            snapshot.name,
                            if snapshot.dirty { " *" } else { "" }
                        );
                        if runtime.window_title != title {
                            if let Some(window) = app.get_webview_window(label) {
                                let _ = window.set_title(&title);
                            }
                            runtime.window_title = title;
                        }
                        let _ = app.emit_to(label, "document-changed", &snapshot);
                        if let Some(canvas) = &runtime.canvas {
                            canvas.view.request_frame();
                        }
                    }
                }
            }
        });
    }
}
pub(super) fn detach_shared_view(id: u64, label: &str, discard: bool) {
    if SHARED.with(|shared| {
        shared
            .borrow()
            .get(&id)
            .is_some_and(|record| record.owner.as_deref() == Some(label))
    }) {
        // Close/transfer first parks the authoritative content if it is live here.
        if label == current_label()
            && DOCUMENT_OPEN.with(|open| open.get())
            && ACTIVE_DOCUMENT_ID.with(|active| active.get()) == id
        {
            park_shared_active(id);
        }
    }
    SHARED.with(|shared| {
        let mut shared = shared.borrow_mut();
        if let Some(record) = shared.get_mut(&id) {
            record.views.remove(label);
        }
        if shared
            .get(&id)
            .is_some_and(|record| record.views.is_empty())
        {
            if discard {
                if let Some(recovery) = shared.get(&id).and_then(|record| record.recovery.as_ref())
                {
                    recovery.clear();
                }
            }
            shared.remove(&id);
        }
    });
}
pub(in crate::canvas) fn open_document_view(
    id: u64,
    target: &str,
) -> Result<DocumentWorkspaceSnapshot, String> {
    if target == current_label()
        || !crate::editor_windows::is_editor(target)
        || CLOSED.with(|c| c.borrow().contains(target))
    {
        return Err("Choose another editor window / 別の編集ウインドウを選択してください / 请选择其他编辑窗口".into());
    }
    if APP
        .get()
        .is_some_and(|app| app.get_webview_window(target).is_none())
    {
        return Err("Editor window not found".into());
    }
    if !workspace_snapshot()
        .documents
        .iter()
        .any(|tab| tab.id == id)
    {
        return Err("Document not found".into());
    }
    text_editor::finish(true)?;
    finish_open_pen()?;
    let source = current_label();
    // Activate the requested source tab before converting it to a shared identity.
    if ACTIVE_DOCUMENT_ID.with(|active| active.get()) != id {
        switch_document(id)?;
    }
    if !SHARED.with(|shared| shared.borrow().contains_key(&id)) {
        let recovery = RECOVERY.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(crate::recovery::Recovery::fork)
                .transpose()
        })?;
        let snapshot = workspace_snapshot().active.unwrap();
        let tiled = ACTIVE_TILED_DOCUMENT.with(|slot| slot.borrow().is_some());
        SHARED.with(|shared| {
            shared.borrow_mut().insert(
                id,
                SharedDocument {
                    owner: Some(source.clone()),
                    parked: None,
                    views: HashSet::from([source.clone()]),
                    snapshot,
                    tiled,
                    recovery,
                    checkpoint: None,
                },
            );
        });
        shared_checkpoint();
        RECOVERY.with(|slot| {
            if let Some(recovery) = slot.borrow().as_ref() {
                recovery.clear();
            }
        });
    }
    {
        let _destination = SessionGuard::enter(target)?;
        if workspace_snapshot()
            .documents
            .iter()
            .any(|tab| tab.id == id)
        {
            return Err("This document already has a view in this window / このウインドウには既に同じ文書のビューがあります / 此窗口中已存在该文档的视图".into());
        }
        text_editor::finish(true)?;
        finish_open_pen()?;
        park_active_document();
        SHARED.with(|shared| {
            shared
                .borrow_mut()
                .get_mut(&id)
                .unwrap()
                .views
                .insert(target.into());
        });
        activate_document(OpenDocument {
            id,
            content: OpenDocumentContent::Shared(id),
            path: None,
            fingerprint: None,
        });
        emit_document();
        redraw()?;
    }
    if let Some(app) = APP.get() {
        if let Some(window) = app.get_webview_window(target) {
            let _ = window.set_focus();
        }
    }
    Ok(workspace_snapshot())
}

pub(super) fn ensure_shared_editable(id: u64) -> Result<(), String> {
    let current = current_label();
    let views = SHARED.with(|shared| shared.borrow().get(&id).map(|record| record.views.clone()));
    if let Some(views) = views {
        let other_editor = PARKED.with(|sessions| {
            sessions.borrow().iter().any(|(label, runtime)| {
                views.contains(label)
                    && label != &current
                    && runtime.document_open
                    && runtime.active_document_id == id
                    && runtime.text.active()
            })
        });
        if other_editor {
            return Err("Finish text editing in the other view / 他のビューの文字編集を確定してください / 请先完成其他视图中的文本编辑".into());
        }
    }
    Ok(())
}

/// Restores the caller even across AppKit nested event loops (dialogs/text drag).
/// Exchange never clones documents or pixel payloads.
pub(in crate::canvas) struct SessionGuard {
    previous: String,
}
impl SessionGuard {
    pub(super) fn enter_render(label: &str) -> Result<Self, String> {
        if SHUTTING_DOWN.with(|flag| flag.get())
            || CLOSED.with(|closed| closed.borrow().contains(label))
        {
            return Err("Editor window is closed".into());
        }
        let previous = current_label();
        activate(label);
        Ok(Self { previous })
    }
    pub(in crate::canvas) fn enter(label: &str) -> Result<Self, String> {
        if SHUTTING_DOWN.with(|flag| flag.get())
            || CLOSED.with(|closed| closed.borrow().contains(label))
        {
            return Err("Editor window is closed".into());
        }
        let previous = current_label();
        if label != previous {
            let target_id = PARKED.with(|sessions| {
                sessions
                    .borrow()
                    .get(label)
                    .filter(|runtime| runtime.document_open)
                    .map(|runtime| runtime.active_document_id)
            });
            if let Some(id) = target_id {
                let source_editing =
                    ACTIVE_DOCUMENT_ID.with(|active| active.get()) == id && text_editor::active();
                if source_editing && shared_has_other_views(id) {
                    return Err("Finish text editing in the other view / 他のビューの文字編集を確定してください / 请先完成其他视图中的文本编辑".into());
                }
            }
        }
        activate(label);
        if let Err(error) = ensure_shared_editable(ACTIVE_DOCUMENT_ID.with(|active| active.get())) {
            activate(&previous);
            return Err(error);
        }
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
    if CLOSED.with(|closed| closed.borrow().contains(&label))
        || SHUTTING_DOWN.with(|flag| flag.get())
    {
        return None;
    }
    let previous = current_label();
    activate(&label);
    Some(SessionGuard { previous })
}

/// Move the authoritative document value; no serialization or pixel cloning.
pub(in crate::canvas) fn transfer_document(
    id: u64,
    target: &str,
) -> Result<DocumentWorkspaceSnapshot, String> {
    let source = current_label();
    if source == target
        || !crate::editor_windows::is_editor(target)
        || CLOSED.with(|c| c.borrow().contains(target))
    {
        return Err("Choose another editor window / 別の編集ウインドウを選択してください / 请选择其他编辑窗口".into());
    }
    if let Some(app) = APP.get() {
        if app.get_webview_window(target).is_none() {
            return Err(
                "Editor window not found / 編集ウインドウが見つかりません / 找不到编辑窗口".into(),
            );
        }
    }
    text_editor::finish(true)?;
    DOCUMENT.with(|doc| {
        let mut doc = doc.borrow_mut();
        finish_pen(&mut doc, false)?;
        doc.finish();
        Ok::<(), String>(())
    })?;
    if !workspace_snapshot()
        .documents
        .iter()
        .any(|tab| tab.id == id)
    {
        return Err("Document not found".into());
    }
    // Complete the destination's transient edits before taking ownership away.
    {
        let _destination = SessionGuard::enter(target)?;
        if workspace_snapshot()
            .documents
            .iter()
            .any(|tab| tab.id == id)
        {
            return Err("This document already has a view in this window / このウインドウには既に同じ文書のビューがあります / 此窗口中已存在该文档的视图".into());
        }
        text_editor::finish(true)?;
        DOCUMENT.with(|doc| {
            let mut doc = doc.borrow_mut();
            finish_pen(&mut doc, false)?;
            doc.finish();
            Ok::<(), String>(())
        })?;
    }
    cancel_vector_drag();
    let was_active = DOCUMENT_OPEN.with(|open| open.get())
        && ACTIVE_DOCUMENT_ID.with(|active| active.get()) == id;
    if was_active {
        park_active_document();
        DOCUMENT_OPEN.with(|open| open.set(false));
        PROJECT_PATH.with(|slot| slot.borrow_mut().take());
        PROJECT_FINGERPRINT.with(|slot| slot.borrow_mut().take());
    }
    let entry = INACTIVE_DOCUMENTS.with(|items| {
        let mut items = items.borrow_mut();
        let index = items.iter().position(|entry| entry.id == id).unwrap();
        items.remove(index)
    });
    if !DOCUMENT_OPEN.with(|open| open.get()) {
        let next = INACTIVE_DOCUMENTS.with(|items| items.borrow_mut().pop());
        if let Some(next) = next {
            activate_document(next);
        } else {
            ACTIVE_DOCUMENT_ID.with(|active| active.set(next_document_id()));
        }
    }
    CHECKPOINT_REVISION.with(|slot| slot.set(None));
    {
        let _destination = match SessionGuard::enter(target) {
            Ok(session) => session,
            Err(error) => {
                if was_active {
                    park_active_document();
                    activate_document(entry);
                } else {
                    INACTIVE_DOCUMENTS.with(|items| items.borrow_mut().push(entry));
                }
                emit_workspace();
                return Err(error);
            }
        };
        park_active_document();
        SHARED.with(|shared| {
            if let Some(record) = shared.borrow_mut().get_mut(&id) {
                record.views.insert(target.into());
            }
        });
        activate_document(entry);
        PAN.with(|pan| pan.set((0., 0.)));
        let zoom = CANVAS.with(|slot| {
            let mut slot = slot.borrow_mut();
            slot.as_mut().map(|canvas| {
                canvas.viewport.pan_x = 0.;
                canvas.viewport.pan_y = 0.;
                canvas.viewport.zoom = 1.;
                canvas.viewport.screen_zoom()
            })
        });
        if let (Some(app), Some(zoom)) = (APP.get(), zoom) {
            let _ = app.emit_to(current_label(), "canvas-zoom-changed", zoom);
        }
        emit_document();
        if let Err(error) = redraw() {
            emit_error(error);
        }
    }
    // The moved tab has already left the source; its canonical content belongs
    // to the destination, so unregister without parking the source's next tab.
    SHARED.with(|shared| {
        if let Some(record) = shared.borrow_mut().get_mut(&id) {
            record.views.remove(&source);
        }
    });
    if DOCUMENT_OPEN.with(|open| open.get()) {
        emit_document();
    } else {
        checkpoint();
        emit_workspace();
    }
    if let Some(app) = APP.get() {
        if let Some(window) = app.get_webview_window(target) {
            let _ = window.set_focus();
        }
    }
    if let Err(error) = redraw() {
        emit_error(error);
    }
    Ok(workspace_snapshot())
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
    if !app
        .webview_windows()
        .keys()
        .filter(|label| crate::editor_windows::is_editor(label))
        .all(|label| confirm_window(label))
    {
        return false;
    }
    let shared: Vec<_> = SHARED.with(|shared| {
        shared
            .borrow()
            .iter()
            .filter(|(_, record)| record.views.len() > 1 && record.snapshot.dirty)
            .filter_map(|(_, record)| record.views.iter().next().cloned())
            .collect()
    });
    shared.into_iter().all(|label| {
        let Ok(_session) = SessionGuard::enter(&label) else {
            return false;
        };
        confirm_unsaved_changes()
    })
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
        let ids: Vec<_> = SHARED.with(|shared| {
            shared
                .borrow()
                .iter()
                .filter(|(_, record)| record.views.contains(label))
                .map(|(id, _)| *id)
                .collect()
        });
        for id in ids {
            detach_shared_view(id, label, discard);
        }
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
    SHARED.with(|shared| {
        for record in shared.borrow().values() {
            if let Some(recovery) = &record.recovery {
                recovery.clear();
            }
        }
    });
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
    fn tiled_recovery_restores_unsaved_tab_and_returns_its_snapshot() {
        let _source = SessionGuard::enter("editor-90122").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut first = crate::recovery::Recovery::start(directory.path().into()).unwrap();
        let mut document = TiledRasterDocument::new(17, 13).unwrap();
        document
            .add_layer("recovered".into(), "Recovered".into())
            .unwrap();
        first.checkpoint_project(
            crate::project_file::ProjectData::Tiled(document.state()),
            document.revision(),
            true,
        );
        first.stop();
        drop(first);
        let recovery = crate::recovery::Recovery::start(directory.path().into()).unwrap();
        let id = recovery.info().unwrap().candidates[0].id.clone();
        RECOVERY.with(|slot| *slot.borrow_mut() = Some(recovery));
        let snapshot = restore_recovery(id).unwrap();
        assert_eq!((snapshot.width, snapshot.height), (17, 13));
        assert_eq!(snapshot.layers[0].name, "Recovered");
        assert!(snapshot.dirty);
        assert!(snapshot.file_name.is_none());
        assert_eq!(workspace_snapshot().documents[0].format, "tiled");
        assert!(PROJECT_PATH.with(|p| p.borrow().is_none()));
        RECOVERY.with(|slot| slot.borrow_mut().take());
        close_window("editor-90122");
    }

    #[test]
    fn opened_psd_is_a_new_unsaved_tiled_tab_and_keeps_previous_document() {
        let _source = SessionGuard::enter("editor-90121").unwrap();
        let original = new_document(preset("Previous")).unwrap().active_id.unwrap();
        open_psd(crate::psd_import::Prepared {
            document: TiledRasterDocument::new(2, 1).unwrap(),
            report: Default::default(),
            name: "Imported.psd".into(),
        })
        .unwrap();
        let workspace = workspace_snapshot();
        assert_eq!(workspace.documents.len(), 2);
        let imported = workspace.active_id.unwrap();
        assert_ne!(imported, original);
        let snapshot = workspace.active.unwrap();
        assert_eq!(snapshot.name, "Imported.psd");
        assert!(snapshot.file_name.is_none());
        assert!(snapshot.dirty);
        assert!(!snapshot.can_undo);
        assert_eq!((snapshot.width, snapshot.height), (2, 1));
        assert!(PROJECT_PATH.with(|p| p.borrow().is_none()));
        switch_document(original).unwrap();
        assert_eq!(
            workspace_snapshot()
                .documents
                .iter()
                .find(|t| t.id == imported)
                .unwrap()
                .file_name
                .as_deref(),
            Some("Imported.psd")
        );
        switch_document(imported).unwrap();
        assert!(workspace_snapshot().active.unwrap().dirty);
        close_window("editor-90121");
    }

    #[test]
    fn shared_tiled_views_keep_layers_and_history_when_the_owner_closes() {
        let _source = SessionGuard::enter("editor-90111").unwrap();
        let id = next_document_id();
        let mut document = TiledRasterDocument::new(1024, 768).unwrap();
        document.add_layer("tiles-a".into(), "A".into()).unwrap();
        activate_document(OpenDocument {
            id,
            content: OpenDocumentContent::Tiled(TiledSession {
                document,
                file_name: Some("Shared tiles".into()),
                saved_revision: Some(0),
                name: "Shared tiles".into(),
            }),
            path: None,
            fingerprint: None,
        });
        open_document_view(id, "editor-90112").unwrap();
        {
            let _view = SessionGuard::enter("editor-90112").unwrap();
            assert_eq!(workspace_snapshot().documents[0].format, "tiled");
            ACTIVE_TILED_DOCUMENT.with(|slot| {
                slot.borrow_mut()
                    .as_mut()
                    .unwrap()
                    .document
                    .add_layer("tiles-b".into(), "B".into())
                    .unwrap()
            });
            ACTIVE_TILED_DOCUMENT.with(|slot| {
                slot.borrow_mut()
                    .as_mut()
                    .unwrap()
                    .document
                    .set_layer_locks("tiles-b", true, false)
                    .unwrap()
            });
            emit_document();
        }
        assert_eq!(
            ACTIVE_TILED_DOCUMENT.with(|slot| slot
                .borrow()
                .as_ref()
                .unwrap()
                .document
                .layers()
                .len()),
            2
        );
        close_window("editor-90111");
        {
            let _view = SessionGuard::enter("editor-90112").unwrap();
            ACTIVE_TILED_DOCUMENT.with(|slot| {
                let mut slot = slot.borrow_mut();
                let session = slot.as_mut().unwrap();
                assert_eq!(session.document.layers().len(), 2);
                assert!(session.document.layers()[1].locked);
                session.document.undo().unwrap();
                assert!(!session.document.layers()[1].locked);
                session.saved_revision = Some(session.document.revision());
            });
            emit_document();
            close_document(id).unwrap();
            assert!(shared_tab(id).is_none());
        }
        close_window("editor-90112");
    }

    #[test]
    fn shared_views_keep_one_document_history_and_independent_viewports() {
        let _source = SessionGuard::enter("editor-90101").unwrap();
        let id = new_document(preset("Shared")).unwrap().active_id.unwrap();
        DOCUMENT.with(|doc| {
            let layer = doc.borrow_mut().add_vector_layer().unwrap();
            doc.borrow_mut().select_layer(layer).unwrap();
            doc.borrow_mut()
                .set_text_object(TextSettings {
                    id: None,
                    text: lumapaint_core::vector::VectorText {
                        content: "Shared text".into(),
                        ..Default::default()
                    },
                    position: [10., 20.],
                    color: [0, 0, 0],
                })
                .unwrap();
        });
        let storage =
            DOCUMENT.with(|doc| doc.borrow().svg_layers().last().unwrap().source.as_ptr());
        PROJECT_PATH.with(|path| {
            *path.borrow_mut() = Some("/private/tmp/shared-document.lumapaint".into())
        });
        PAN.with(|pan| pan.set((11., 12.)));
        open_document_view(id, "editor-90102").unwrap();
        assert_eq!(workspace_snapshot().active_id, Some(id));
        assert_eq!(
            DOCUMENT.with(|doc| doc.borrow().svg_layers().last().unwrap().source.as_ptr()),
            storage
        );
        {
            let _view = SessionGuard::enter("editor-90102").unwrap();
            assert_eq!(workspace_snapshot().active_id, Some(id));
            assert_eq!(
                DOCUMENT.with(|doc| doc.borrow().svg_layers().last().unwrap().source.as_ptr()),
                storage
            );
            assert_eq!(
                PROJECT_PATH
                    .with(|path| path.borrow().clone())
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "/private/tmp/shared-document.lumapaint"
            );
            PAN.with(|pan| pan.set((21., 22.)));
            DOCUMENT.with(|doc| doc.borrow_mut().delete_selected_vector_objects().unwrap());
            emit_document();
            assert!(DOCUMENT.with(|doc| doc
                .borrow()
                .svg_layers()
                .last()
                .unwrap()
                .vector_objects
                .is_empty()));
        }
        assert_eq!(PAN.with(|pan| pan.get()), (11., 12.));
        assert!(DOCUMENT.with(|doc| doc
            .borrow()
            .svg_layers()
            .last()
            .unwrap()
            .vector_objects
            .is_empty()));
        DOCUMENT.with(|doc| doc.borrow_mut().undo());
        emit_document();
        {
            let _view = SessionGuard::enter("editor-90102").unwrap();
            assert_eq!(PAN.with(|pan| pan.get()), (21., 22.));
            assert_eq!(
                DOCUMENT.with(|doc| doc
                    .borrow()
                    .svg_layers()
                    .last()
                    .unwrap()
                    .vector_objects
                    .len()),
                1
            );
            let unrelated = new_document(preset("Other tab"))
                .unwrap()
                .active_id
                .unwrap();
            assert_eq!(workspace_snapshot().documents.len(), 2);
            switch_document(id).unwrap();
            assert_eq!(workspace_snapshot().active_id, Some(id));
            switch_document(unrelated).unwrap();
        }
        close_window("editor-90101");
        {
            let _view = SessionGuard::enter("editor-90102").unwrap();
            switch_document(id).unwrap();
            assert_eq!(
                DOCUMENT.with(|doc| doc
                    .borrow()
                    .svg_layers()
                    .last()
                    .unwrap()
                    .vector_objects
                    .len()),
                1
            );
            assert!(DOCUMENT.with(|doc| doc.borrow().snapshot().can_redo));
            DOCUMENT.with(|doc| doc.borrow_mut().mark_saved("Shared.lumapaint".into()));
            emit_document();
            close_document(id).unwrap();
            assert!(shared_tab(id).is_none());
        }
        close_window("editor-90102");
    }

    #[test]
    fn transfer_preserves_identity_history_and_document_storage() {
        let _source = SessionGuard::enter("editor-90011").unwrap();
        let id = new_document(preset("Transfer")).unwrap().active_id.unwrap();
        DOCUMENT.with(|doc| {
            let layer = doc.borrow_mut().add_vector_layer().unwrap();
            doc.borrow_mut().select_layer(layer).unwrap();
            doc.borrow_mut()
                .set_text_object(TextSettings {
                    id: None,
                    text: lumapaint_core::vector::VectorText {
                        content: "Retained".into(),
                        ..Default::default()
                    },
                    position: [10., 20.],
                    color: [0, 0, 0],
                })
                .unwrap();
        });
        let address =
            DOCUMENT.with(|doc| doc.borrow().svg_layers().last().unwrap().source.as_ptr());
        let selection = DOCUMENT.with(|doc| doc.borrow().snapshot().selected_vector_objects);
        let other = new_document(preset("Remaining"))
            .unwrap()
            .active_id
            .unwrap();
        let source = transfer_document(id, "editor-90012").unwrap();
        assert_eq!(source.active_id, Some(other));
        assert_eq!(source.documents.len(), 1);
        {
            let _destination = SessionGuard::enter("editor-90012").unwrap();
            assert_eq!(workspace_snapshot().active_id, Some(id));
            assert_eq!(
                DOCUMENT.with(|doc| doc.borrow().snapshot().selected_vector_objects),
                selection
            );
            DOCUMENT.with(|doc| {
                assert_eq!(
                    doc.borrow().svg_layers().last().unwrap().source.as_ptr(),
                    address
                );
                assert!(doc.borrow().snapshot().can_undo);
            });
            let source = transfer_document(id, "editor-90011").unwrap();
            assert!(source.documents.is_empty());
        }
        assert_eq!(workspace_snapshot().active_id, Some(id));
        assert_eq!(workspace_snapshot().documents.len(), 2);
        assert!(transfer_document(id, "settings").is_err());
        close_window("editor-90012");
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
