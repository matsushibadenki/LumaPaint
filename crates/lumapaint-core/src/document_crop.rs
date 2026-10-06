//! Canvas cropping keeps artwork outside the new canvas editable.
use super::*;
impl Document {
    pub fn crop_canvas(&mut self, bounds: [f32; 4]) -> Result<bool, String> {
        if bounds.iter().any(|v| !v.is_finite()) || bounds[2] <= 0. || bounds[3] <= 0. {
            return Err("Invalid crop bounds".into());
        }
        let x = bounds[0].floor();
        let y = bounds[1].floor();
        let width = (bounds[0] + bounds[2]).ceil() - x;
        let height = (bounds[1] + bounds[3]).ceil() - y;
        if x < 0.
            || y < 0.
            || x + width > self.width as f32
            || y + height > self.height as f32
            || width < 1.
            || height < 1.
        {
            return Err("Crop bounds must fit the canvas".into());
        }
        if x == 0. && y == 0. && width == self.width as f32 && height == self.height as f32 {
            return Ok(false);
        }
        let mut next = self.clone();
        next.finish();
        let mut previous = next.vector_history_state();
        previous.crop_dimensions = Some((next.width, next.height));
        previous.strokes = Some(next.strokes.clone());
        let old = (next.width, next.height);
        next.width = width as u32;
        next.height = height as u32;
        let shift = |object: &mut VectorObject| {
            object.transform[4] -= x;
            object.transform[5] -= y;
        };
        for layer in &mut next.svg_layers {
            for object in &mut layer.vector_objects {
                shift(object);
            }
            if layer.vector_layer && !self.noncanonical_vector_sources.contains(&layer.id) {
                layer.source = vector_svg(next.width, next.height, &layer.vector_objects);
            } else {
                layer.source = crop_source(&layer.source, old, (next.width, next.height), x, y)?;
            }
            validate_svg_layer(layer)?;
        }
        if let Some(source) = &mut next.paint_source {
            *source = crop_source(source, old, (next.width, next.height), x, y)?;
        }
        for stroke in &mut next.strokes {
            for point in &mut stroke.points {
                point.x -= x;
                point.y -= y;
            }
            stroke.selection = stroke.selection.as_ref().map(|s| s.translated(-x, -y));
        }
        for shape in &mut next.compound_shapes {
            for object in &mut shape.operands {
                shift(object);
            }
        }
        for path in &mut next.saved_paths {
            for object in &mut path.objects {
                shift(object);
            }
        }
        for guide in &mut next.guides.items {
            if let Some(axis) = &guide.axis {
                guide.position -= if axis == "vertical" { x } else { y };
            }
            for object in &mut guide.objects {
                shift(object);
            }
        }
        next.guides.origin[0] -= x;
        next.guides.origin[1] -= y;
        next.deselect();
        next.record_vector_edit(previous);
        next.revision += 1;
        *self = next;
        Ok(true)
    }
}
fn crop_source(
    source: &str,
    old: (u32, u32),
    size: (u32, u32),
    x: f32,
    y: f32,
) -> Result<String, String> {
    let content = unclipped_pixel_source(source, old.0, old.1)?;
    let xml = roxmltree::Document::parse(&content).map_err(|e| e.to_string())?;
    let content = &content[xml.root_element().range()];
    let result = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}"><g transform="translate({} {})">{content}</g></svg>"#,
        size.0, size.1, -x, -y
    );
    validate_svg_edit_source(&result)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cropping_active_page_keeps_other_pages_and_history() {
        let mut doc = Document::default();
        doc.setup_pages(PageSetup {
            count: 3,
            facing: true,
            binding: PageBinding::LeftToRight,
        })
        .unwrap();
        let original = doc.pages_snapshot();
        doc.crop_canvas([10., 20., 200., 100.]).unwrap();
        let cropped = doc.pages_snapshot();
        assert_eq!(
            (cropped.pages[0].width, cropped.pages[0].height),
            (200, 100)
        );
        for i in 1..3 {
            assert_eq!(
                (cropped.pages[i].width, cropped.pages[i].height),
                (original.pages[i].width, original.pages[i].height)
            );
        }
        doc.undo();
        assert_eq!(doc.dimensions(), (960, 640));
        doc.redo();
        assert_eq!(doc.dimensions(), (200, 100));
    }

    #[test]
    fn crop_preserves_off_canvas_vector_and_imported_svg_content() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let object: VectorObject = serde_json::from_value(serde_json::json!({"id":"crop-shape","name":"Shape","kind":"rectangle","path":{"data":"M0 0H80V80H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,10,20],"fill":{"color":[10,20,30,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
        doc.upsert_vector_object(&layer, object).unwrap();
        doc.import_svg("image".into(),r#"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect width="200" height="100" fill="red"/></svg>"#.into()).unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.crop_canvas([30., 40., 200., 150.]).unwrap();
        assert_eq!(
            doc.svg_layers[0].vector_objects[0].transform[4..],
            [-20., -20.]
        );
        assert!(doc.svg_layers[1].source.contains("translate(-30 -40)"));
        let loaded = Document::from_document_state(doc.document_state()).unwrap();
        assert_eq!(loaded.svg_layers[0].vector_objects.len(), 1);
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }

    #[test]
    fn crop_translates_pixels_and_vectors_atomically_and_undo_restores_dimensions() {
        let mut doc = Document::default();
        doc.begin(Point { x: 60., y: 80. }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 90., y: 100. }).unwrap();
        doc.finish();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc.crop_canvas([20., 30., 200., 150.]).unwrap());
        assert_eq!(doc.dimensions(), (200, 150));
        assert_eq!(doc.strokes[0].points[0], Point { x: 40., y: 50. });
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        assert!(doc.crop_canvas([0., 0., 201., 150.]).is_err());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        assert!(!doc.crop_canvas([0., 0., 200., 150.]).unwrap());
        let loaded = Document::from_document_state(doc.document_state()).unwrap();
        assert_eq!(loaded.dimensions(), (200, 150));
    }
}
