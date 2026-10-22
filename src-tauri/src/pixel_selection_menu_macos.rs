//! Native selection commands and generation-checked analysis jobs.
use super::*;
use lumapaint_core::document::PixelSelectionRequest;
use std::cell::Cell;

pub(crate) struct Job {
    document_id: u64,
    revision: u64,
    selection: Option<lumapaint_core::selection::Selection>,
    target: String,
    request: PixelSelectionRequest,
    document: Document,
}
pub(crate) struct ResultData {
    document_id: u64,
    revision: u64,
    selection: Option<lumapaint_core::selection::Selection>,
    target: String,
    mask: Option<lumapaint_core::layer_mask::LayerMask>,
    result: lumapaint_core::selection::Selection,
}
pub(crate) fn immediate(request: PixelSelectionRequest) -> Result<DocumentSnapshot, String> {
    ensure_document_open()?;
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return Err("Selection menu requires a standard document / 通常形式の文書で実行してください / 请在标准文档中执行".into());
    }
    selection_tools::cancel();
    let quick = request.action == "quickMask";
    DOCUMENT.with(|d| d.borrow_mut().pixel_selection_action(request))?;
    if quick && DOCUMENT.with(|d| d.borrow().quick_mask_active()) {
        switch_canvas_tool(CanvasTool::Brush)?;
        if let Some(app) = APP.get() {
            let _ = app.emit_to(current_label(), "canvas-tool-changed", CanvasTool::Brush);
        }
    }
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
pub(crate) fn prepare(mut request: PixelSelectionRequest) -> Result<Job, String> {
    ensure_document_open()?;
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        return Err("Selection menu requires a standard document / 通常形式の文書で実行してください / 请在标准文档中执行".into());
    }
    selection_tools::cancel();
    if matches!(request.action.as_str(), "grow" | "similar") {
        let settings = selection_tools::settings();
        request.amount = Some(settings.tolerance);
        request.all_layers = Some(settings.all_layers);
    }
    DOCUMENT.with(|slot| {
        let mut document = slot.borrow_mut();
        document.finish();
        Ok(Job {
            document_id: ACTIVE_DOCUMENT_ID.with(|v| v.get()),
            revision: document.revision(),
            selection: document.selection().cloned(),
            target: document.selected_layer_id().into(),
            request,
            document: document.clone_for_rendering(),
        })
    })
}
pub(crate) fn evaluate(job: Job) -> Result<ResultData, String> {
    let result = lumapaint_renderer::selection_menu::evaluate(&job.document, &job.request)?;
    let mask = if job.request.output_mask.unwrap_or(false) {
        Some(lumapaint_core::layer_mask::LayerMask::from_selection(
            lumapaint_core::layer_mask::MaskKind::Pixel,
            Some(&result),
            job.document.dimensions().0,
            job.document.dimensions().1,
        )?)
    } else {
        None
    };
    Ok(ResultData {
        document_id: job.document_id,
        revision: job.revision,
        selection: job.selection,
        target: job.target,
        mask,
        result,
    })
}
pub(crate) fn commit(result: ResultData) -> Result<DocumentSnapshot, String> {
    if ACTIVE_DOCUMENT_ID.with(|v| v.get()) != result.document_id {
        return Err("Document changed / 文書が変更されました / 文档已更改".into());
    }
    DOCUMENT.with(|slot| -> Result<(), String> {
        let mut document = slot.borrow_mut();
        if document.has_active_stroke()
            || document.revision() != result.revision
            || document.selection() != result.selection.as_ref()
            || document.selected_layer_id() != result.target
        {
            return Err(
                "Selection target changed / 選択対象が変更されました / 选区目标已更改".into(),
            );
        }
        if let Some(mask) = result.mask {
            let mut effects = document.layer_effects(&result.target);
            effects.mask = Some(mask);
            document.set_layer_effects(&result.target, effects)?;
        } else {
            document.commit_path_selection(Some(result.result))?;
        }
        Ok(())
    })?;
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}
// Preview ownership and generations stay in Rust. Closing the modal invalidates
// in-flight workers; a preview never enters history, recovery, or saved documents.
struct PreviewFrame {
    context: ResultData,
    document: Document,
}
struct PreviewSlot {
    generation: u64,
    frame: Option<PreviewFrame>,
}
thread_local! {
    static PREVIEWS: RefCell<std::collections::HashMap<String, PreviewSlot>> = RefCell::new(std::collections::HashMap::new());
    static PREVIEW_GENERATION: Cell<u64> = const { Cell::new(0) };
}
pub(crate) struct PreviewJob {
    generation: u64,
    job: Job,
}
pub(crate) struct PreviewResult {
    generation: u64,
    result: ResultData,
}
fn matches_document(result: &ResultData, document: &Document) -> bool {
    ACTIVE_DOCUMENT_ID.with(|id| id.get()) == result.document_id
        && !document.has_active_stroke()
        && document.revision() == result.revision
        && document.selection() == result.selection.as_ref()
        && document.selected_layer_id() == result.target
}
pub(crate) fn prepare_preview(mut request: PixelSelectionRequest) -> Result<PreviewJob, String> {
    if request.action != "refine" {
        return Err("Only Select and Mask supports this preview".into());
    }
    // The overlay always shows coverage, regardless of the eventual output type.
    request.output_mask = Some(false);
    let job = prepare(request)?;
    let generation = PREVIEW_GENERATION.with(|id| {
        let next = id.get().wrapping_add(1);
        id.set(next);
        next
    });
    PREVIEWS.with(|previews| {
        let mut previews = previews.borrow_mut();
        let slot = previews.entry(current_label()).or_insert(PreviewSlot {
            generation,
            frame: None,
        });
        slot.generation = generation;
    });
    Ok(PreviewJob { generation, job })
}
pub(crate) fn evaluate_preview(job: PreviewJob) -> Result<PreviewResult, String> {
    Ok(PreviewResult {
        generation: job.generation,
        result: evaluate(job.job)?,
    })
}
pub(crate) fn install_preview(result: PreviewResult) -> Result<bool, String> {
    let installed = PREVIEWS.with(|previews| -> Result<bool, String> {
        let mut previews = previews.borrow_mut();
        let Some(slot) = previews.get_mut(&current_label()) else {
            return Ok(false);
        };
        if slot.generation != result.generation {
            return Ok(false);
        }
        let document = DOCUMENT.with(|doc| -> Result<Option<Document>, String> {
            let doc = doc.borrow();
            if !matches_document(&result.result, &doc) {
                return Ok(None);
            }
            let mut preview = doc.clone_for_rendering();
            preview.set_tool_pixel_selection(Some(result.result.result.clone()))?;
            if !preview.quick_mask_active() {
                preview.pixel_selection_action(PixelSelectionRequest {
                    action: "quickMask".into(),
                    ..Default::default()
                })?;
            }
            Ok(Some(preview))
        })?;
        let Some(document) = document else {
            return Ok(false);
        };
        slot.frame = Some(PreviewFrame {
            context: result.result,
            document,
        });
        Ok(true)
    })?;
    if installed {
        request_redraw()?;
    }
    Ok(installed)
}
pub(crate) fn cancel_preview() -> Result<bool, String> {
    clear_preview(&current_label());
    request_redraw()?;
    Ok(true)
}
pub(crate) fn clear_preview(label: &str) {
    PREVIEWS.with(|previews| previews.borrow_mut().remove(label));
}
pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    PREVIEWS.with(|previews| {
        let previews = previews.borrow();
        let frame = previews.get(&current_label())?.frame.as_ref()?;
        if !DOCUMENT.with(|doc| matches_document(&frame.context, &doc.borrow())) {
            return None;
        }
        Some(canvas.renderer.render(canvas.viewport, &frame.document))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::Point;
    use lumapaint_core::selection::{Selection, SelectionShape};
    #[test]
    fn selection_preview_is_transient_latest_only_and_cancel_rejects_workers() {
        DOCUMENT.with(|d| {
            let mut d = d.borrow_mut();
            *d = Document::default();
            d.set_tool_pixel_selection(Some(Selection::new(
                SelectionShape::Rectangle,
                [100., 100., 30., 30.],
            )))
            .unwrap();
        });
        DOCUMENT_OPEN.with(|v| v.set(true));
        ACTIVE_DOCUMENT_ID.with(|v| v.set(79));
        let original =
            DOCUMENT.with(|d| serde_json::to_value(d.borrow().document_state()).unwrap());
        let before = DOCUMENT.with(|d| d.borrow().snapshot());
        let request = PixelSelectionRequest {
            action: "refine".into(),
            feather: Some(8.),
            ..Default::default()
        };
        let old = evaluate_preview(prepare_preview(request.clone()).unwrap()).unwrap();
        let fresh = evaluate_preview(prepare_preview(request.clone()).unwrap()).unwrap();
        assert!(!install_preview(old).unwrap());
        assert!(install_preview(fresh).unwrap());
        PREVIEWS.with(|previews| {
            let previews = previews.borrow();
            let frame = previews
                .get(&current_label())
                .unwrap()
                .frame
                .as_ref()
                .unwrap();
            assert!(frame.document.quick_mask_active());
            let edge = frame
                .document
                .selection()
                .unwrap()
                .coverage(Point { x: 100.5, y: 115.5 });
            assert!(edge > 0. && edge < 1.);
        });
        assert_eq!(
            DOCUMENT.with(|d| serde_json::to_value(d.borrow().document_state()).unwrap()),
            original
        );
        assert_eq!(
            DOCUMENT.with(|d| d.borrow().snapshot().can_undo),
            before.can_undo
        );
        assert!(!DOCUMENT.with(|d| d.borrow().quick_mask_active()));
        let pending = evaluate_preview(prepare_preview(request.clone()).unwrap()).unwrap();
        clear_preview(&current_label());
        assert!(!install_preview(pending).unwrap());
        // Reopening with the same settings cannot resurrect a previous dialog's job.
        let pending = evaluate_preview(prepare_preview(request.clone()).unwrap()).unwrap();
        clear_preview(&current_label());
        let latest = evaluate_preview(prepare_preview(request.clone()).unwrap()).unwrap();
        assert!(!install_preview(pending).unwrap());
        assert!(install_preview(latest).unwrap());
        clear_preview(&current_label());
        let applied = evaluate(prepare(request).unwrap()).unwrap();
        let expected = applied.result.clone();
        commit(applied).unwrap();
        DOCUMENT.with(|d| {
            let mut d = d.borrow_mut();
            assert_eq!(d.selection(), Some(&expected));
            d.undo();
            assert_eq!(serde_json::to_value(d.document_state()).unwrap(), original);
            d.redo();
            assert_eq!(d.selection(), Some(&expected));
        });
    }
    #[test]
    fn selection_preview_rejects_changed_target_and_non_refine_requests() {
        DOCUMENT.with(|d| {
            let mut d = d.borrow_mut();
            *d = Document::default();
            d.set_tool_pixel_selection(Some(Selection::new(
                SelectionShape::Rectangle,
                [1., 1., 20., 20.],
            )))
            .unwrap();
        });
        DOCUMENT_OPEN.with(|v| v.set(true));
        ACTIVE_DOCUMENT_ID.with(|v| v.set(80));
        assert!(prepare_preview(PixelSelectionRequest {
            action: "inverse".into(),
            ..Default::default()
        })
        .is_err());
        let pending = evaluate_preview(
            prepare_preview(PixelSelectionRequest {
                action: "refine".into(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        DOCUMENT
            .with(|d| d.borrow_mut().set_tool_pixel_selection(None))
            .unwrap();
        assert!(!install_preview(pending).unwrap());
        clear_preview(&current_label());
    }
    #[test]
    fn selection_transform_worker_commits_once_and_rejects_new_paint() {
        DOCUMENT.with(|d| *d.borrow_mut() = Document::default());
        DOCUMENT_OPEN.with(|v| v.set(true));
        ACTIVE_DOCUMENT_ID.with(|v| v.set(78));
        DOCUMENT
            .with(|d| {
                d.borrow_mut().set_path_selection(
                    vec![Point { x: 300.5, y: 200.5 }],
                    0.5,
                    lumapaint_core::selection::SelectionMode::Replace,
                )
            })
            .unwrap();
        let original = DOCUMENT.with(|d| d.borrow().selection().cloned());
        let request = PixelSelectionRequest {
            action: "transform".into(),
            matrix: Some([1., 0., 0., 1., 10., 20.]),
            ..Default::default()
        };
        let result = evaluate(prepare(request.clone()).unwrap()).unwrap();
        commit(result).unwrap();
        DOCUMENT.with(|d| {
            let mut doc = d.borrow_mut();
            assert_eq!(
                doc.selection()
                    .unwrap()
                    .coverage(Point { x: 310.5, y: 220.5 }),
                1.
            );
            doc.undo();
            assert_eq!(doc.selection(), original.as_ref());
            doc.redo();
            assert_eq!(
                doc.selection()
                    .unwrap()
                    .coverage(Point { x: 310.5, y: 220.5 }),
                1.
            );
        });
        let pending = evaluate(prepare(request).unwrap()).unwrap();
        DOCUMENT
            .with(|d| {
                d.borrow_mut()
                    .begin(Point { x: 310.5, y: 220.5 }, Brush::default())
            })
            .unwrap();
        assert!(commit(pending).is_err());
        assert!(DOCUMENT.with(|d| d.borrow().has_active_stroke()));
        DOCUMENT.with(|d| *d.borrow_mut() = Document::default());
    }
    fn result(revision: u64) -> ResultData {
        ResultData {
            document_id: 77,
            revision,
            selection: None,
            target: "layer-1".into(),
            mask: None,
            result: Selection::new(SelectionShape::Rectangle, [5., 5., 20., 20.]),
        }
    }
    #[test]
    fn selection_menu_worker_rejects_stale_document_revision_and_selection() {
        DOCUMENT.with(|d| *d.borrow_mut() = Document::default());
        DOCUMENT_OPEN.with(|v| v.set(true));
        ACTIVE_DOCUMENT_ID.with(|v| v.set(77));
        let revision = DOCUMENT.with(|d| d.borrow().revision());
        assert!(commit(result(revision + 1)).is_err());
        ACTIVE_DOCUMENT_ID.with(|v| v.set(78));
        assert!(commit(result(revision)).is_err());
        ACTIVE_DOCUMENT_ID.with(|v| v.set(77));
        commit(result(revision)).unwrap();
        assert!(DOCUMENT.with(|d| d.borrow().selection().is_some()));
        assert!(commit(result(revision)).is_err());
    }
}
