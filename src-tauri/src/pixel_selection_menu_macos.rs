//! Native selection commands and generation-checked analysis jobs.
use super::*;
use lumapaint_core::document::PixelSelectionRequest;

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
        if document.revision() != result.revision
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
#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::selection::{Selection, SelectionShape};
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
