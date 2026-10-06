use super::*;
use lumapaint_core::{document::Point, paint_bucket::Settings};
thread_local! {static SETTINGS:RefCell<std::collections::BTreeMap<String,Settings>>=const{RefCell::new(std::collections::BTreeMap::new())};}
pub(super) fn settings() -> Settings {
    SETTINGS.with(|s| *s.borrow_mut().entry(current_label()).or_default())
}
pub(super) fn configure(settings: Settings) -> Result<(), String> {
    settings.validate()?;
    SETTINGS.with(|s| {
        s.borrow_mut().insert(current_label(), settings);
    });
    if let Some(app) = APP.get() {
        let _ = app.emit("paint-bucket-settings-changed", ());
    }
    Ok(())
}
pub(super) fn pointer(doc: &mut Document, point: Point, phase: u8) -> Result<(), String> {
    pixel_paint::fill_pointer(doc, point, phase, settings()).map(|_| ())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bucket_base_commit_is_atomic_undoable_and_noop_has_no_history() {
        ACTIVE_DOCUMENT_ID.with(|id| id.set(884));
        BRUSH.with(|b| {
            *b.borrow_mut() = lumapaint_core::document::Brush {
                color: [255, 0, 0],
                ..Default::default()
            }
        });
        let mut doc = Document::default();
        doc.crop_canvas([0., 0., 5., 1.]).unwrap();
        doc.replace_moved_pixels(r#"<svg xmlns="http://www.w3.org/2000/svg" width="5" height="1"><rect width="2" height="1" fill="white"/><rect x="2" width="1" height="1" fill="black"/><rect x="3" width="2" height="1" fill="white"/></svg>"#.into(),None).unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let point = Point { x: 0., y: 0. };
        pointer(&mut doc, point, 0).unwrap();
        pointer(&mut doc, point, 2).unwrap();
        let after = serde_json::to_value(doc.document_state()).unwrap();
        let mut pixels = lumapaint_renderer::pixel_paint::PixelPaintPreview::new((5, 1)).unwrap();
        pixels
            .update(&doc.clone_stamp_workspace().unwrap())
            .unwrap();
        assert_eq!(&pixels.pixels()[..4], [255, 0, 0, 255]);
        assert_eq!(&pixels.pixels()[12..16], [255, 255, 255, 255]);
        pointer(&mut doc, point, 0).unwrap();
        pointer(&mut doc, point, 2).unwrap();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn all_layers_samples_boundaries_but_only_paints_selected_empty_image_layer() {
        ACTIVE_DOCUMENT_ID.with(|id| id.set(885));
        BRUSH.with(|b| {
            *b.borrow_mut() = lumapaint_core::document::Brush {
                color: [255, 0, 0],
                ..Default::default()
            }
        });
        configure(Settings {
            all_layers: true,
            ..Default::default()
        })
        .unwrap();
        let mut doc = Document::default();
        doc.crop_canvas([0., 0., 5., 1.]).unwrap();
        doc.replace_moved_pixels(r#"<svg xmlns="http://www.w3.org/2000/svg" width="5" height="1"><rect x="2" width="1" height="1" fill="black"/></svg>"#.into(),None).unwrap();
        let source = doc.paint_source().unwrap().to_string();
        let layer = doc.add_paint_layer().unwrap();
        doc.select_layer(layer).unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let point = Point { x: 0., y: 0. };
        pointer(&mut doc, point, 0).unwrap();
        pointer(&mut doc, point, 2).unwrap();
        let mut pixels = lumapaint_renderer::pixel_paint::PixelPaintPreview::new((5, 1)).unwrap();
        pixels
            .update(&doc.clone_stamp_workspace().unwrap())
            .unwrap();
        assert_eq!(&pixels.pixels()[..4], [255, 0, 0, 255]);
        assert_eq!(&pixels.pixels()[12..16], [0; 4]);
        assert_eq!(doc.paint_source().unwrap(), source);
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        PIXEL_PAINT_COMMIT.with(|p| p.borrow_mut().take());
    }
}
