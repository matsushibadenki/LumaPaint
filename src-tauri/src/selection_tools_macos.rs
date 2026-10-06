use super::*;
use lumapaint_core::{document::Point, selection::Selection, selection_tools::Settings};
struct Draft {
    document_id: u64,
    revision: u64,
    tool: CanvasTool,
    points: Vec<Point>,
    hover: Option<Point>,
    base: Option<Selection>,
    mode: SelectionMode,
    radius: f32,
    preview: Document,
    edges: Option<lumapaint_renderer::selection_tools::EdgeMap>,
}
thread_local! {static DRAFTS:RefCell<std::collections::BTreeMap<String,Draft>>=const{RefCell::new(std::collections::BTreeMap::new())};static SETTINGS:RefCell<std::collections::BTreeMap<String,Settings>>=const{RefCell::new(std::collections::BTreeMap::new())};}
pub(super) fn is_tool(tool: CanvasTool) -> bool {
    matches!(
        tool,
        CanvasTool::Lasso
            | CanvasTool::PolygonLasso
            | CanvasTool::MagneticLasso
            | CanvasTool::SelectionBrush
    )
}
pub(super) fn settings() -> Settings {
    SETTINGS.with(|s| *s.borrow_mut().entry(current_label()).or_default())
}
pub(super) fn configure(settings: Settings) -> Result<(), String> {
    settings.validate()?;
    SETTINGS.with(|s| s.borrow_mut().insert(current_label(), settings));
    if let Some(app) = APP.get() {
        let _ = app.emit("selection-tool-settings-changed", ());
    }
    Ok(())
}
pub(super) fn active() -> bool {
    DRAFTS.with(|d| d.borrow().contains_key(&current_label()))
}
pub(super) fn cancel() -> bool {
    DRAFTS.with(|d| d.borrow_mut().remove(&current_label()).is_some())
}
fn update_preview(draft: &mut Draft) -> Result<(), String> {
    draft.preview.set_tool_pixel_selection(draft.base.clone())?;
    let mut points = draft.points.clone();
    if draft.tool == CanvasTool::PolygonLasso {
        if let Some(hover) = draft.hover {
            points.push(hover);
        }
    }
    draft
        .preview
        .set_path_selection(points, draft.radius, draft.mode)
}
fn commit_draft(doc: &mut Document, draft: Draft) -> Result<(), String> {
    if draft.document_id != ACTIVE_DOCUMENT_ID.with(|id| id.get())
        || draft.revision != doc.revision()
    {
        return Err("Selection target changed / 選択対象が変更されました / 选择目标已更改".into());
    }
    if draft.points.len() < if draft.radius > 0. { 1 } else { 3 } {
        return Ok(());
    }
    let mut preview = draft.preview;
    preview.set_tool_pixel_selection(draft.base)?;
    preview.set_path_selection(draft.points, draft.radius, draft.mode)?;
    doc.commit_path_selection(preview.selection().cloned())
        .map(|_| ())
}
pub(super) fn confirm() -> Result<(), String> {
    if let Some(draft) = DRAFTS.with(|d| d.borrow_mut().remove(&current_label())) {
        DOCUMENT.with(|doc| commit_draft(&mut doc.borrow_mut(), draft))?;
        emit_document();
        redraw()?;
    }
    Ok(())
}
pub(super) fn remove_last() -> Result<(), String> {
    DRAFTS.with(|d| {
        let mut d = d.borrow_mut();
        if let Some(draft) = d.get_mut(&current_label()) {
            draft.points.pop();
            if draft.points.is_empty() {
                d.remove(&current_label());
            } else {
                update_preview(draft)?;
            }
        }
        Ok(())
    })
}
pub(super) fn pointer(
    doc: &mut Document,
    tool: CanvasTool,
    point: Point,
    phase: u8,
    flags: NSEventModifierFlags,
    double_click: bool,
) -> Result<(), String> {
    let settings = settings();
    let zoom = CANVAS
        .with(|c| c.borrow().as_ref().map_or(1., |c| c.viewport.screen_zoom()))
        .max(0.001);
    DRAFTS.with(|d| -> Result<(), String> {
        let mut drafts = d.borrow_mut();
        let label = current_label();
        if phase == 0
            && (!matches!(tool, CanvasTool::PolygonLasso | CanvasTool::MagneticLasso)
                || !drafts.contains_key(&label))
        {
            if point.x < 0.
                || point.y < 0.
                || point.x > doc.dimensions().0 as f32
                || point.y > doc.dimensions().1 as f32
            {
                return Ok(());
            }
            let mode = if flags.contains(NSEventModifierFlags::Shift | NSEventModifierFlags::Option)
            {
                SelectionMode::Intersect
            } else if flags.contains(NSEventModifierFlags::Option) {
                SelectionMode::Subtract
            } else if flags.contains(NSEventModifierFlags::Shift)
                || (tool == CanvasTool::SelectionBrush && settings.mode == SelectionMode::Replace)
            {
                SelectionMode::Add
            } else {
                settings.mode
            };
            let edges = if tool == CanvasTool::MagneticLasso {
                Some(lumapaint_renderer::selection_tools::EdgeMap::from_document(
                    &doc.selection_sampling_document(settings.all_layers),
                )?)
            } else {
                None
            };
            let point = edges
                .as_ref()
                .map_or(point, |edges| edges.snap(point, settings, zoom));
            drafts.insert(
                label.clone(),
                Draft {
                    document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                    revision: doc.revision(),
                    tool,
                    points: vec![point],
                    hover: None,
                    base: doc.selection().cloned(),
                    mode,
                    radius: if tool == CanvasTool::SelectionBrush {
                        settings.diameter / 2.
                    } else {
                        0.
                    },
                    preview: doc.clone(),
                    edges,
                },
            );
        } else if let Some(draft) = drafts.get_mut(&label) {
            if draft.document_id != ACTIVE_DOCUMENT_ID.with(|id| id.get())
                || draft.revision != doc.revision()
                || draft.tool != tool
            {
                drafts.remove(&label);
                return Ok(());
            }
            let point = draft
                .edges
                .as_ref()
                .map_or(point, |edges| edges.snap(point, settings, zoom));
            if phase == 0 && matches!(tool, CanvasTool::PolygonLasso | CanvasTool::MagneticLasso) {
                let close = draft.points.len() >= 3
                    && ((point.x - draft.points[0].x).hypot(point.y - draft.points[0].y) * zoom
                        <= 8.
                        || double_click);
                if close {
                    let draft = drafts.remove(&label).unwrap();
                    return commit_draft(doc, draft);
                }
                if draft.points.len() >= 16000 {
                    return Err(
                        "Selection point limit reached / 選択点の上限です / 已达到选择点上限"
                            .into(),
                    );
                }
                draft.points.push(point);
            } else if matches!(
                tool,
                CanvasTool::Lasso | CanvasTool::SelectionBrush | CanvasTool::MagneticLasso
            ) && (phase == 1
                || phase == 3 && tool == CanvasTool::MagneticLasso
                || phase == 2 && tool != CanvasTool::MagneticLasso)
            {
                let spacing = if tool == CanvasTool::MagneticLasso {
                    (50. / settings.magnetic_frequency).max(0.5)
                } else {
                    0.5
                };
                if draft
                    .points
                    .last()
                    .is_some_and(|p| (p.x - point.x).hypot(p.y - point.y) * zoom >= spacing)
                {
                    if draft.points.len() >= 16000 {
                        return Err("Selection point limit reached".into());
                    }
                    draft.points.push(point);
                }
            } else if phase == 3 {
                draft.hover = Some(point);
            }
        }
        if phase == 2 && matches!(tool, CanvasTool::Lasso | CanvasTool::SelectionBrush) {
            if let Some(draft) = drafts.remove(&label) {
                return commit_draft(doc, draft);
            }
        }
        if let Some(draft) = drafts.get_mut(&label) {
            update_preview(draft)?;
        }
        Ok(())
    })
}
pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    DRAFTS.with(|d| {
        let d = d.borrow();
        let draft = d.get(&current_label())?;
        if draft.document_id != ACTIVE_DOCUMENT_ID.with(|id| id.get())
            || draft.revision != DOCUMENT.with(|doc| doc.borrow().revision())
            || !is_tool(TOOL.with(|t| t.get()))
        {
            return None;
        }
        let mut points = draft.points.clone();
        if draft.tool == CanvasTool::PolygonLasso {
            if let Some(p) = draft.hover {
                points.push(p);
            }
        }
        if draft.radius == 0. {
            canvas.renderer.set_guide_drafts(
                points
                    .windows(2)
                    .map(|p| lumapaint_renderer::FrameOverlay {
                        corners: [
                            [p[0].x, p[0].y],
                            [p[1].x, p[1].y],
                            [p[1].x, p[1].y],
                            [p[0].x, p[0].y],
                        ],
                        handles: false,
                        baseline: None,
                    })
                    .collect(),
            );
        }
        Some(canvas.renderer.render(canvas.viewport, &draft.preview))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polygon_clicks_close_without_changing_document_until_commit() {
        cancel();
        ACTIVE_DOCUMENT_ID.with(|id| id.set(902));
        let mut doc = Document::default();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        for point in [
            Point { x: 10., y: 10. },
            Point { x: 50., y: 10. },
            Point { x: 10., y: 50. },
        ] {
            pointer(
                &mut doc,
                CanvasTool::PolygonLasso,
                point,
                0,
                NSEventModifierFlags::empty(),
                false,
            )
            .unwrap();
            pointer(
                &mut doc,
                CanvasTool::PolygonLasso,
                point,
                2,
                NSEventModifierFlags::empty(),
                false,
            )
            .unwrap();
        }
        assert!(active());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        pointer(
            &mut doc,
            CanvasTool::PolygonLasso,
            Point { x: 10., y: 10. },
            0,
            NSEventModifierFlags::empty(),
            false,
        )
        .unwrap();
        assert!(!active());
        assert!(doc.selection().unwrap().contains(Point { x: 15., y: 15. }));
        assert!(!doc.selection().unwrap().contains(Point { x: 40., y: 40. }));
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }
    #[test]
    fn freehand_cancel_and_brush_subtraction_preserve_source_pixels() {
        cancel();
        ACTIVE_DOCUMENT_ID.with(|id| id.set(903));
        let mut doc = Document::default();
        pointer(
            &mut doc,
            CanvasTool::Lasso,
            Point { x: 10., y: 10. },
            0,
            NSEventModifierFlags::empty(),
            false,
        )
        .unwrap();
        pointer(
            &mut doc,
            CanvasTool::Lasso,
            Point { x: 50., y: 10. },
            1,
            NSEventModifierFlags::empty(),
            false,
        )
        .unwrap();
        assert!(cancel());
        assert!(doc.selection().is_none());
        configure(Settings {
            diameter: 20.,
            ..Default::default()
        })
        .unwrap();
        pointer(
            &mut doc,
            CanvasTool::SelectionBrush,
            Point { x: 20., y: 20. },
            0,
            NSEventModifierFlags::empty(),
            false,
        )
        .unwrap();
        pointer(
            &mut doc,
            CanvasTool::SelectionBrush,
            Point { x: 40., y: 20. },
            2,
            NSEventModifierFlags::empty(),
            false,
        )
        .unwrap();
        assert!(doc.selection().unwrap().contains(Point { x: 30., y: 20. }));
        configure(Settings {
            diameter: 8.,
            ..Default::default()
        })
        .unwrap();
        pointer(
            &mut doc,
            CanvasTool::SelectionBrush,
            Point { x: 30., y: 20. },
            0,
            NSEventModifierFlags::Option,
            false,
        )
        .unwrap();
        pointer(
            &mut doc,
            CanvasTool::SelectionBrush,
            Point { x: 30., y: 20. },
            2,
            NSEventModifierFlags::Option,
            false,
        )
        .unwrap();
        assert!(!doc.selection().unwrap().contains(Point { x: 30., y: 20. }));
        assert_eq!(doc.snapshot().stroke_count, 0);
    }
}
