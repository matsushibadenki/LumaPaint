use super::*;
use lumapaint_core::clone_stamp::Settings;
use lumapaint_core::document::Point;
#[derive(Default)]
struct State {
    settings: Settings,
    source: Option<(u64, Point)>,
    offset: Option<[f32; 2]>,
    marker: Option<Point>,
}
thread_local! { static STATES: RefCell<std::collections::BTreeMap<String,State>> = const { RefCell::new(std::collections::BTreeMap::new()) }; }
pub(super) fn settings() -> Settings {
    STATES.with(|states| {
        states
            .borrow_mut()
            .entry(current_label())
            .or_default()
            .settings
    })
}
pub(super) fn configure(settings: Settings) -> Result<(), String> {
    settings.validate()?;
    STATES.with(|states| {
        let mut states = states.borrow_mut();
        let state = states.entry(current_label()).or_default();
        if state.settings.aligned != settings.aligned {
            state.offset = None;
        }
        state.settings = settings;
    });
    Ok(())
}
pub(super) fn pointer(
    doc: &mut Document,
    point: Point,
    phase: u8,
    pressure: f32,
    options: NSEventModifierFlags,
) -> Result<(), String> {
    if options.contains(NSEventModifierFlags::Option) {
        if phase == 0 {
            doc.clone_stamp_workspace()?;
            STATES.with(|states| {
                let mut states = states.borrow_mut();
                let state = states.entry(current_label()).or_default();
                state.source = Some((ACTIVE_DOCUMENT_ID.with(|id| id.get()), point));
                state.offset = None;
                state.marker = Some(point);
            });
        }
        return Ok(());
    }
    let (settings,offset)=STATES.with(|states| -> Result<_,String> {let mut states=states.borrow_mut();let state=states.entry(current_label()).or_default();let (id,source)=state.source.ok_or("Option-click a source first / Optionクリックでコピー元を指定してください / 请先按Option单击取样点")?;if id!=ACTIVE_DOCUMENT_ID.with(|v|v.get()){return Err("Set a source in this document / このドキュメントでコピー元を指定してください / 请在此文档设置取样点".into());}let offset=if state.settings.aligned { *state.offset.get_or_insert([source.x-point.x,source.y-point.y]) } else {if phase==0 {state.offset=Some([source.x-point.x,source.y-point.y]);}state.offset.unwrap_or([0.,0.])};state.marker=Some(Point{x:point.x+offset[0],y:point.y+offset[1]});Ok((state.settings,offset))})?;
    pixel_paint::clone_pointer(doc, point, phase, pressure, offset, settings).map(|_| ())
}

pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    if TOOL.with(|tool| tool.get()) != CanvasTool::CloneStamp {
        return None;
    }
    let marker = STATES.with(|states| {
        states.borrow().get(&current_label()).and_then(|state| {
            state
                .source
                .filter(|(id, _)| *id == ACTIVE_DOCUMENT_ID.with(|id| id.get()))
                .and(state.marker)
        })
    });
    if let Some(p) = marker {
        let r = 6. / canvas.viewport.zoom;
        canvas
            .renderer
            .set_frame_overlay(Some(lumapaint_renderer::FrameOverlay {
                corners: [
                    [p.x, p.y - r],
                    [p.x, p.y + r],
                    [p.x, p.y + r],
                    [p.x, p.y - r],
                ],
                handles: false,
                baseline: Some([[p.x - r, p.y], [p.x + r, p.y]]),
            }));
    }
    if let Some(result) = pixel_paint::render(canvas) {
        return Some(result);
    }
    Some(DOCUMENT.with(|doc| canvas.renderer.render(canvas.viewport, &doc.borrow())))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_required_and_base_stamp_commits_once_with_exact_undo() {
        STATES.with(|s| s.borrow_mut().clear());
        ACTIVE_DOCUMENT_ID.with(|id| id.set(772));
        BRUSH.with(|b| {
            *b.borrow_mut() = lumapaint_core::document::Brush {
                size: 4.,
                hardness: 1.,
                ..Default::default()
            }
        });
        let mut doc = Document::default();
        doc.crop_canvas([0., 0., 16., 8.]).unwrap();
        doc.replace_moved_pixels(r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8"><rect width="8" height="8" fill="red"/><rect x="8" width="8" height="8" fill="blue"/></svg>"#.into(),None).unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let target = Point { x: 10.5, y: 3.5 };
        assert!(pointer(&mut doc, target, 0, 1., NSEventModifierFlags::empty()).is_err());
        pointer(
            &mut doc,
            Point { x: 2.5, y: 3.5 },
            0,
            1.,
            NSEventModifierFlags::Option,
        )
        .unwrap();
        pointer(&mut doc, target, 0, 1., NSEventModifierFlags::empty()).unwrap();
        pointer(&mut doc, target, 2, 1., NSEventModifierFlags::empty()).unwrap();
        let mut preview = lumapaint_renderer::pixel_paint::PixelPaintPreview::new((16, 8)).unwrap();
        preview
            .update(&doc.clone_stamp_workspace().unwrap())
            .unwrap();
        assert_eq!(
            &preview.pixels()[(3 * 16 + 10) * 4..(3 * 16 + 11) * 4],
            [255, 0, 0, 255]
        );
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        pointer(
            &mut doc,
            Point { x: 12.5, y: 3.5 },
            0,
            1.,
            NSEventModifierFlags::empty(),
        )
        .unwrap();
        pointer(
            &mut doc,
            Point { x: 12.5, y: 3.5 },
            2,
            1.,
            NSEventModifierFlags::empty(),
        )
        .unwrap();
        STATES.with(|states| assert_eq!(states.borrow()[&current_label()].offset, Some([-8., 0.])));
        configure(Settings {
            aligned: false,
            ..Default::default()
        })
        .unwrap();
        pointer(
            &mut doc,
            Point { x: 12.5, y: 3.5 },
            0,
            1.,
            NSEventModifierFlags::empty(),
        )
        .unwrap();
        pointer(
            &mut doc,
            Point { x: 12.5, y: 3.5 },
            2,
            1.,
            NSEventModifierFlags::empty(),
        )
        .unwrap();
        STATES
            .with(|states| assert_eq!(states.borrow()[&current_label()].offset, Some([-10., 0.])));
        ACTIVE_DOCUMENT_ID.with(|id| id.set(773));
        assert!(pointer(&mut doc, target, 0, 1., NSEventModifierFlags::empty()).is_err());
    }
    #[test]
    fn stamp_on_image_layer_preserves_underlying_layer_and_undo() {
        STATES.with(|s| s.borrow_mut().clear());
        ACTIVE_DOCUMENT_ID.with(|id| id.set(774));
        BRUSH.with(|b| {
            *b.borrow_mut() = lumapaint_core::document::Brush {
                size: 4.,
                hardness: 1.,
                ..Default::default()
            }
        });
        let mut doc = Document::default();
        doc.crop_canvas([0., 0., 16., 8.]).unwrap();
        let layer = doc.add_paint_layer().unwrap();
        doc.select_layer(layer).unwrap();
        doc.replace_selected_image(r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8"><rect width="8" height="8" fill="red"/><rect x="8" width="8" height="8" fill="blue"/></svg>"#.into()).unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        pointer(
            &mut doc,
            Point { x: 2.5, y: 3.5 },
            0,
            1.,
            NSEventModifierFlags::Option,
        )
        .unwrap();
        let target = Point { x: 10.5, y: 3.5 };
        pointer(&mut doc, target, 0, 1., NSEventModifierFlags::empty()).unwrap();
        pointer(&mut doc, target, 2, 1., NSEventModifierFlags::empty()).unwrap();
        let mut preview = lumapaint_renderer::pixel_paint::PixelPaintPreview::new((16, 8)).unwrap();
        preview
            .update(&doc.clone_stamp_workspace().unwrap())
            .unwrap();
        assert_eq!(
            &preview.pixels()[(3 * 16 + 10) * 4..(3 * 16 + 11) * 4],
            [255, 0, 0, 255]
        );
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        PIXEL_PAINT_COMMIT.with(|pending| pending.borrow_mut().take());
    }
}
