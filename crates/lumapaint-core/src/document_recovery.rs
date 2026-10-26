//! Serialization-only recovery forks; live edit histories stay with the source document.
use super::*;

impl Document {
    /// An owned recovery input with the same serialized project content.
    /// Unlike a rendering fork it retains all pages and named selections.
    /// Undo/Redo, journals and picking caches are intentionally omitted. Do not
    /// install this fork as an editable session or use it to replace the source.
    pub fn clone_for_recovery(&self) -> Self {
        // Deliberately exhaustive: new document fields require an explicit storage decision.
        Self {
            quick_mask: self.quick_mask,
            quick_mask_stroke: None,
            saved_pixel_selections: self.saved_pixel_selections.clone(),
            previous_pixel_selection: None,
            vector_assets: self.vector_assets.clone(),
            text_layout_backend: None,
            alignment: self.alignment.clone(),
            paint_bucket_settings: self.paint_bucket_settings.clone(),
            animation: self.animation.clone(),
            saved_vector_selections: self.saved_vector_selections.clone(),
            previous_vector_selection: Vec::new(),
            compound_shapes: self.compound_shapes.clone(),
            scene_journal: Default::default(),
            scene_picking: Default::default(),
            layer_groups: self.layer_groups.clone(),
            guides: self.guides.clone(),
            locked_objects: self.locked_objects.clone(),
            locked_artwork_layers: self.locked_artwork_layers.clone(),
            svg_geometry_backend: None,
            path_editing: self.path_editing.clone(),
            saved_paths: self.saved_paths.clone(),
            clipping_path_id: self.clipping_path_id.clone(),
            selection: self.selection.clone(),
            selection_anchor: None,
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            unit: self.unit,
            resolution: self.resolution,
            artboards: self.artboards,
            pages: self.pages.clone(),
            page_undo: Vec::new(),
            page_redo: Vec::new(),
            canvas_color: self.canvas_color,
            paper: self.paper,
            pixel_aspect_ratio: self.pixel_aspect_ratio,
            layer_effects: self.layer_effects.clone(),
            paint_source: self.paint_source.clone(),
            strokes: self.strokes.clone(),
            redo: Vec::new(),
            active: self.active.clone(),
            visible: self.visible,
            layer_name: self.layer_name.clone(),
            layer_opacity: self.layer_opacity,
            layer_locked: self.layer_locked,
            layer_alpha_locked: self.layer_alpha_locked,
            layer_mask_enabled: self.layer_mask_enabled,
            layer_mask_inverted: self.layer_mask_inverted,
            layer_mask_density: self.layer_mask_density,
            svg_layers: self.svg_layers.clone(),
            selected_layer: self.selected_layer.clone(),
            editing_channel: self.editing_channel,
            layer_edit_selection: self.layer_edit_selection.clone(),
            selected_vector_objects: self.selected_vector_objects.clone(),
            vector_undo: Vec::new(),
            vector_redo: Vec::new(),
            undo_order: Vec::new(),
            redo_order: Vec::new(),
            color_mode: self.color_mode,
            color_profile: self.color_profile,
            bit_depth: self.bit_depth,
            revision: self.revision,
            point_count: self.point_count,
            saved_revision: self.saved_revision,
            text_change_generation: self.text_change_generation,
            translation_metrics: Default::default(),
            noncanonical_vector_sources: self.noncanonical_vector_sources.clone(),
            transform_slots: Default::default(),
            file_name: self.file_name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(doc: &mut Document) {
        doc.begin(Point { x: 30., y: 40. }, Brush::default())
            .unwrap();
        doc.extend(Point { x: 100., y: 140. }).unwrap();
        doc.finish();
    }

    #[test]
    fn recovery_fork_preserves_serialized_content_without_copying_history() {
        let mut doc = Document::default();
        stroke(&mut doc);
        doc.add_vector_layer().unwrap();
        doc.add_vector_layer().unwrap();
        doc.undo();
        assert!(!doc.undo_order.is_empty());
        assert!(!doc.redo_order.is_empty());
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let undo_count = doc.undo_order.len();
        let redo_count = doc.redo_order.len();
        let mut fork = doc.clone_for_recovery();
        assert!(fork.undo_order.is_empty());
        assert!(fork.redo_order.is_empty());
        assert!(fork.vector_undo.is_empty());
        assert!(fork.vector_redo.is_empty());
        assert!(fork.page_undo.is_empty());
        assert!(fork.page_redo.is_empty());
        assert!(fork.redo.is_empty());
        assert_eq!(fork.encode().unwrap(), doc.clone().encode().unwrap());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        assert_eq!(doc.undo_order.len(), undo_count);
        assert_eq!(doc.redo_order.len(), redo_count);
        doc.redo();
        assert_eq!(doc.svg_layers.len(), 2);
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }

    #[test]
    fn recovery_fork_keeps_inactive_pages_named_selections_and_active_strokes() {
        let mut doc = Document::default();
        stroke(&mut doc);
        doc.saved_vector_selections = std::sync::Arc::new(vec![SavedVectorSelection {
            name: "Named objects".into(),
            object_ids: Vec::new(),
        }]);
        doc.edit_pages(PageEdit {
            action: "add".into(),
            index: None,
            facing: None,
            binding: None,
        })
        .unwrap();
        doc.edit_pages(PageEdit {
            action: "select".into(),
            index: Some(1),
            facing: None,
            binding: None,
        })
        .unwrap();
        doc.edit_pages(PageEdit {
            action: "layout".into(),
            index: None,
            facing: Some(true),
            binding: None,
        })
        .unwrap();
        doc.saved_vector_selections = std::sync::Arc::new(vec![SavedVectorSelection {
            name: "Active page selection".into(),
            object_ids: Vec::new(),
        }]);
        doc.saved_pixel_selections = std::sync::Arc::new(vec![SavedPixelSelection {
            name: "Pixel selection".into(),
            selection: std::sync::Arc::new(Selection::new(
                SelectionShape::Rectangle,
                [10., 20., 30., 40.],
            )),
        }]);
        stroke(&mut doc);
        doc.begin(Point { x: 80., y: 90. }, Brush::default())
            .unwrap();
        let mut expected = doc.clone();
        let mut fork = doc.clone_for_recovery();
        assert!(fork.page_undo.is_empty());
        assert_eq!(fork.pages_snapshot().pages.len(), 2);
        assert_eq!(doc.page_document(0).unwrap().strokes.len(), 1);
        assert!(std::ptr::eq(
            fork.page_document(0).unwrap(),
            doc.page_document(0).unwrap()
        ));
        assert!(std::sync::Arc::ptr_eq(
            &fork.saved_vector_selections,
            &doc.saved_vector_selections
        ));
        assert!(std::sync::Arc::ptr_eq(
            &fork.saved_pixel_selections,
            &doc.saved_pixel_selections
        ));
        assert_eq!(fork.encode().unwrap(), expected.encode().unwrap());
        assert!(doc.has_active_stroke());
        assert!(!doc.page_undo.is_empty());
    }

    #[test]
    #[ignore = "manual recovery-copy microbenchmark; excludes application input and disk I/O"]
    fn recovery_fork_history_cost() {
        use std::hint::black_box;
        let mut doc = Document::default();
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><!--{}--><rect width=\"20\" height=\"20\"/></svg>", "x".repeat(65536));
        for index in 0..16 {
            doc.import_svg(format!("Layer {index}"), svg.clone())
                .unwrap();
        }
        for index in 0..64 {
            let mut effects = crate::layer_effects::LayerEffects {
                enabled: true,
                ..Default::default()
            };
            effects.values[1] = index as f32;
            doc.set_layer_effects("layer-1", effects).unwrap();
        }
        assert_eq!(doc.vector_undo.len(), 64);
        fn timed(f: impl FnOnce() -> Document) -> u128 {
            let started = std::time::Instant::now();
            let copy = black_box(f());
            let elapsed = started.elapsed().as_nanos();
            drop(copy);
            elapsed
        }
        let mut full = Vec::new();
        let mut recovery = Vec::new();
        for iteration in 0..20 {
            if iteration % 2 == 0 {
                full.push(timed(|| doc.clone()));
                recovery.push(timed(|| doc.clone_for_recovery()));
            } else {
                recovery.push(timed(|| doc.clone_for_recovery()));
                full.push(timed(|| doc.clone()));
            }
        }
        assert_eq!(
            serde_json::to_value(doc.clone_for_recovery().document_state()).unwrap(),
            serde_json::to_value(doc.document_state()).unwrap()
        );
        println!(
            "samples_json={}",
            serde_json::json!({"full_ns":full,"recovery_ns":recovery})
        );
        full.sort_unstable();
        recovery.sort_unstable();
        println!(
            "recovery_fork_history_entries={} full_clone_median_ns={} recovery_clone_median_ns={}",
            doc.vector_undo.len(),
            full[10],
            recovery[10]
        );
    }
}
