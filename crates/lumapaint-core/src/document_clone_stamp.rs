use super::*;
impl Document {
    /// References are scoped to this page. Deleted IDs are omitted from UI and
    /// serialization. Structural-edit history recovers refs when a layer returns.
    pub fn paint_bucket_settings(&self) -> crate::paint_bucket::Settings {
        let mut settings = self.paint_bucket_settings.clone();
        settings
            .reference_layers
            .retain(|id| id == "layer-1" || self.svg_layers.iter().any(|layer| &layer.id == id));
        settings
    }
    /// Metadata-only history: no layer/source clones or SVG regeneration.
    pub fn set_paint_bucket_settings(
        &mut self,
        settings: crate::paint_bucket::Settings,
    ) -> Result<bool, String> {
        settings.validate()?;
        if settings
            .reference_layers
            .iter()
            .any(|id| id != "layer-1" && !self.svg_layers.iter().any(|layer| &layer.id == id))
        {
            return Err("Choose valid reference layers / 有効な参照レイヤーを選択してください / 请选择有效的参考图层".into());
        }
        if self.paint_bucket_settings == settings {
            return Ok(false);
        }
        self.finish();
        let previous = std::mem::replace(&mut self.paint_bucket_settings, settings);
        self.vector_undo
            .push(VectorHistoryEntry::PaintBucket(previous));
        self.undo_order.push(HistoryKind::Vector);
        self.vector_redo.clear();
        self.redo_order.clear();
        self.revision += 1;
        Ok(true)
    }
    pub fn clone_stamp_preview_document(&self, source: String) -> Result<Self, String> {
        self.pixel_preview_document(source, false)
    }
    pub fn retouch_preview_document(&self, source: String) -> Result<Self, String> {
        self.pixel_preview_document(source, true)
    }
    fn pixel_preview_document(&self, source: String, preserve_alpha: bool) -> Result<Self, String> {
        let mut result = self.clone_for_rendering();
        if self.selected_layer.is_some() {
            result.replace_image_source_inner(source, preserve_alpha, false)?;
        } else {
            result.paint_source = None;
            result.strokes.clear();
            result.point_count = 0;
            result.svg_layers.insert(
                0,
                SvgLayer {
                    id: "clone-stamp-preview-base".into(),
                    name: self.layer_name.clone(),
                    visible: self.visible,
                    opacity: self.paint_layer_opacity(),
                    locked: false,
                    alpha_locked: false,
                    mask_enabled: false,
                    mask_inverted: false,
                    mask_density: 1.,
                    source,
                    paint_layer: true,
                    vector_layer: false,
                    vector_objects: Vec::new(),
                },
            );
            let effects = self.layer_effects("layer-1");
            result
                .layer_effects
                .insert("clone-stamp-preview-base".into(), effects);
        }
        Ok(result)
    }
    pub fn clone_stamp_workspace(&self) -> Result<Self, String> {
        self.pixel_filter_workspace(false)
    }
    pub fn retouch_workspace(&self) -> Result<Self, String> {
        self.pixel_filter_workspace(true)
    }
    fn pixel_filter_workspace(&self, preserve_alpha: bool) -> Result<Self, String> {
        if let Some(workspace) = self.pixel_paint_workspace(preserve_alpha)? {
            return Ok(workspace);
        }
        if self.layer_locked || (self.layer_alpha_locked && !preserve_alpha) || !self.visible {
            return Err("Unlock and show the pixel layer / ピクセルレイヤーを表示しロックを解除してください / 请显示并解锁像素图层".into());
        }
        Ok(Self {
            width: self.width,
            height: self.height,
            paint_source: self.paint_source.clone(),
            strokes: self.strokes.clone(),
            point_count: self.point_count,
            selection: self.selection.clone(),
            canvas_color: CanvasColor::Transparent,
            ..Self::default()
        })
    }
    /// Construct a read-only sampling scene; source flags and artwork stay intact.
    pub fn paint_bucket_sampling_document(
        &self,
        settings: &crate::paint_bucket::Settings,
    ) -> Result<Self, String> {
        settings.validate()?;
        let references = &settings.reference_layers;
        if settings.use_reference_layers
            && (references.is_empty()
                || references.iter().any(|id| {
                    id != "layer-1" && !self.svg_layers.iter().any(|layer| &layer.id == id)
                }))
        {
            return Err("Choose valid reference layers / 有効な参照レイヤーを選択してください / 请选择有效的参考图层".into());
        }
        let mut result =
            self.clone_stamp_sampling_document(crate::clone_stamp::Sample::AllLayers)?;
        if settings.use_reference_layers {
            result.visible &= references.iter().any(|id| id == "layer-1");
        }
        let mut visible = result.visible
            && (!settings.use_reference_layers || references.iter().any(|id| id == "layer-1"));
        for layer in &mut result.svg_layers {
            if settings.use_reference_layers {
                layer.visible &= references.contains(&layer.id);
            }
            if layer.visible
                && settings.exclude_text
                && layer.vector_layer
                && !self.noncanonical_vector_sources.contains(&layer.id)
                && layer
                    .vector_objects
                    .iter()
                    .any(|object| object.kind == VectorObjectKind::Text)
            {
                layer
                    .vector_objects
                    .retain(|object| object.kind != VectorObjectKind::Text);
                layer.source = vector_svg(result.width, result.height, &layer.vector_objects);
            }
            visible |= layer.visible
                && (!settings.exclude_text
                    || !layer.vector_layer
                    || !layer.vector_objects.is_empty()
                    || self.noncanonical_vector_sources.contains(&layer.id));
        }
        if settings.use_reference_layers && !visible {
            return Err(
                "Show a reference layer / 参照レイヤーを表示してください / 请显示参考图层".into(),
            );
        }
        Ok(result)
    }
    pub fn clone_stamp_sampling_document(
        &self,
        sample: crate::clone_stamp::Sample,
    ) -> Result<Self, String> {
        use crate::clone_stamp::Sample;
        if sample == Sample::CurrentLayer {
            let mut result = self.clone_stamp_workspace()?;
            result.selection = None;
            return Ok(result);
        }
        let mut result = self.clone_for_rendering();
        result.finish();
        result.selection = None;
        if sample == Sample::CurrentBelow {
            let count = if let Some(id) = &self.selected_layer {
                self.svg_layers
                    .iter()
                    .position(|l| &l.id == id)
                    .map_or(0, |i| i + 1)
            } else {
                0
            };
            result.svg_layers.truncate(count);
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rendering_forks_omit_history_and_inactive_pages_without_changing_artwork() {
        let mut doc = Document::default();
        let id = doc.add_paint_layer().unwrap();
        for gap in 1..=16 {
            doc.set_paint_bucket_settings(crate::paint_bucket::Settings {
                close_gap: gap,
                ..Default::default()
            })
            .unwrap();
        }
        doc.edit_pages(PageEdit {
            action: "duplicate".into(),
            index: Some(0),
            facing: None,
            binding: None,
        })
        .unwrap();
        doc.select_layer(id).unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let undo_count = doc.undo_order.len();
        assert!(undo_count > 0);
        assert!(!doc.page_undo.is_empty());
        let assert_no_history = |fork: &Document| {
            assert!(fork.undo_order.is_empty());
            assert!(fork.redo_order.is_empty());
            assert!(fork.vector_undo.is_empty());
            assert!(fork.vector_redo.is_empty());
            assert!(fork.page_undo.is_empty());
            assert!(fork.page_redo.is_empty());
            assert!(fork.redo.is_empty());
            assert!(fork.pages.is_none());
        };
        let fork = doc.clone_for_rendering();
        assert_no_history(&fork);
        assert_eq!(fork.scene_journal.cursor(), 0);
        let mut fork_snapshot = serde_json::to_value(fork.snapshot()).unwrap();
        let mut source_snapshot = serde_json::to_value(doc.snapshot()).unwrap();
        for key in ["pages", "canUndo", "canRedo"] {
            fork_snapshot.as_object_mut().unwrap().remove(key);
            source_snapshot.as_object_mut().unwrap().remove(key);
        }
        assert_eq!(fork_snapshot, source_snapshot);
        let sampled = doc
            .clone_stamp_sampling_document(crate::clone_stamp::Sample::AllLayers)
            .unwrap();
        assert_no_history(&sampled);
        for all_layers in [false, true] {
            let selection_sample = doc.selection_sampling_document(all_layers);
            assert_no_history(&selection_sample);
            assert!(selection_sample.selection.is_none());
        }
        assert_no_history(&doc.clipping_view());
        let preview = doc.clone_stamp_preview_document("<svg/>".into()).unwrap();
        assert_no_history(&preview);
        assert_eq!(doc.undo_order.len(), undo_count);
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }
    #[test]
    #[ignore = "manual rendering fork timing; not an application latency benchmark"]
    fn rendering_fork_timing_with_history_and_pages() {
        let mut doc = Document::default();
        let layer = doc.add_paint_layer().unwrap();
        doc.select_layer(layer).unwrap();
        let padding = "x".repeat(128 * 1024);
        for generation in 0..32 {
            doc.replace_selected_image(format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><!--{padding}{generation}--><rect width=\"64\" height=\"64\" fill=\"red\"/></svg>"
            )).unwrap();
        }
        for _ in 0..3 {
            doc.edit_pages(PageEdit {
                action: "duplicate".into(),
                index: Some(0),
                facing: None,
                binding: None,
            })
            .unwrap();
        }
        let measure = |full: bool| {
            let mut times = Vec::new();
            for iteration in 0..22 {
                let start = std::time::Instant::now();
                let fork = if full {
                    doc.clone()
                } else {
                    doc.clone_for_rendering()
                };
                std::hint::black_box(&fork);
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                if iteration >= 2 {
                    times.push(elapsed);
                }
                drop(fork);
            }
            times.sort_by(f64::total_cmp);
            (times[10], times[18])
        };
        let full = measure(true);
        let render = measure(false);
        println!("Rendering fork fixture: 960x640, 128 KiB SVG, 32 image edits, 4 pages; 2 warmups + 20 samples; allocation/copy only, destruction excluded");
        println!("Full clone median {:.3} ms / p95 {:.3} ms; rendering fork median {:.3} ms / p95 {:.3} ms", full.0, full.1, render.0, render.1);
    }
    #[test]
    fn sampling_fork_includes_live_stroke_but_does_not_finish_original_gesture() {
        let mut doc = Document::default();
        doc.begin(Point { x: 12., y: 20. }, Brush::default())
            .unwrap();
        let revision = doc.revision();
        let sampled = doc
            .clone_stamp_sampling_document(crate::clone_stamp::Sample::AllLayers)
            .unwrap();
        assert!(!sampled.has_active_stroke());
        assert_eq!(sampled.strokes.len(), 1);
        assert!(doc.has_active_stroke());
        assert!(doc.strokes.is_empty());
        assert_eq!(doc.revision(), revision);
        doc.finish();
        doc.undo();
        assert!(doc.strokes.is_empty());
    }
    #[test]
    fn bucket_settings_round_trip_without_artwork_or_scene_changes_and_undo_as_metadata() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let sources: Vec<_> = doc
            .svg_layers
            .iter()
            .map(|layer| layer.source.clone())
            .collect();
        let cursor = doc.scene_journal().cursor();
        let settings = crate::paint_bucket::Settings {
            use_reference_layers: true,
            reference_layers: vec![layer.clone()],
            exclude_text: true,
            close_gap: 3,
            area_offset: -2,
            ..Default::default()
        };
        assert!(doc.set_paint_bucket_settings(settings.clone()).unwrap());
        assert!(matches!(
            doc.vector_undo.last(),
            Some(VectorHistoryEntry::PaintBucket(_))
        ));
        let revision = doc.revision();
        assert!(!doc.set_paint_bucket_settings(settings.clone()).unwrap());
        assert_eq!(doc.revision(), revision);
        assert_eq!(doc.scene_journal().cursor(), cursor);
        assert_eq!(
            doc.svg_layers
                .iter()
                .map(|layer| layer.source.clone())
                .collect::<Vec<_>>(),
            sources
        );
        let state = doc.document_state();
        let after = serde_json::to_value(&state).unwrap();
        let loaded = Document::from_document_state(state).unwrap();
        assert_eq!(loaded.paint_bucket_settings(), settings);
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        assert_eq!(doc.scene_journal().cursor(), cursor);
        let invalid = crate::paint_bucket::Settings {
            reference_layers: vec!["missing".into()],
            ..settings
        };
        assert!(doc.set_paint_bucket_settings(invalid).is_err());
        assert_eq!(doc.revision(), revision + 2);
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
    }
    #[test]
    fn bucket_settings_prune_deleted_references_but_layer_undo_restores_them() {
        let mut doc = Document::default();
        let layer = doc.add_paint_layer().unwrap();
        let settings = crate::paint_bucket::Settings {
            use_reference_layers: true,
            reference_layers: vec![layer.clone()],
            ..Default::default()
        };
        doc.set_paint_bucket_settings(settings.clone()).unwrap();
        doc.delete_layer(&layer).unwrap();
        assert!(doc.paint_bucket_settings().reference_layers.is_empty());
        assert!(doc.paint_bucket_settings().use_reference_layers);
        assert!(doc
            .paint_bucket_sampling_document(&doc.paint_bucket_settings())
            .is_err());
        let loaded = Document::from_document_state(doc.document_state()).unwrap();
        assert!(loaded.paint_bucket_settings().reference_layers.is_empty());
        doc.undo();
        assert_eq!(doc.paint_bucket_settings(), settings);
        doc.redo();
        assert!(doc.paint_bucket_settings().reference_layers.is_empty());
    }
    #[test]
    fn bucket_settings_do_not_bind_a_new_layer_reusing_a_deleted_id() {
        let mut doc = Document::default();
        let layer = doc.add_paint_layer().unwrap();
        doc.set_paint_bucket_settings(crate::paint_bucket::Settings {
            use_reference_layers: true,
            reference_layers: vec![layer.clone()],
            ..Default::default()
        })
        .unwrap();
        doc.delete_layer(&layer).unwrap();
        let replacement = doc.add_paint_layer().unwrap();
        assert_eq!(replacement, layer);
        assert!(doc.paint_bucket_settings().reference_layers.is_empty());
        doc.undo();
        doc.undo();
        assert_eq!(doc.paint_bucket_settings().reference_layers, vec![layer]);
    }
    #[test]
    fn bucket_settings_follow_page_duplication_switching_and_reload() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let settings = crate::paint_bucket::Settings {
            use_reference_layers: true,
            reference_layers: vec![layer],
            close_gap: 4,
            ..Default::default()
        };
        doc.set_paint_bucket_settings(settings.clone()).unwrap();
        let edit = |doc: &mut Document, action: &str, index| {
            doc.edit_pages(PageEdit {
                action: action.into(),
                index,
                facing: None,
                binding: None,
            })
            .unwrap()
        };
        edit(&mut doc, "duplicate", Some(0));
        edit(&mut doc, "select", Some(1));
        assert_eq!(doc.paint_bucket_settings(), settings);
        let changed = crate::paint_bucket::Settings {
            close_gap: 7,
            ..settings.clone()
        };
        doc.set_paint_bucket_settings(changed.clone()).unwrap();
        edit(&mut doc, "select", Some(0));
        assert_eq!(doc.paint_bucket_settings(), settings);
        edit(&mut doc, "add", None);
        edit(&mut doc, "select", Some(1));
        assert_eq!(
            doc.paint_bucket_settings(),
            crate::paint_bucket::Settings::default()
        );
        let mut loaded = Document::from_document_state(doc.document_state()).unwrap();
        edit(&mut loaded, "select", Some(0));
        assert_eq!(loaded.paint_bucket_settings(), settings);
        edit(&mut loaded, "select", Some(2));
        assert_eq!(loaded.paint_bucket_settings(), changed);
    }
    #[test]
    fn bucket_settings_load_legacy_defaults_and_reject_corrupt_metadata() {
        let mut legacy = serde_json::to_value(Document::default().document_state()).unwrap();
        assert!(legacy.get("paintBucketSettings").is_none());
        assert_eq!(
            Document::from_document_state(serde_json::from_value(legacy.clone()).unwrap())
                .unwrap()
                .paint_bucket_settings(),
            crate::paint_bucket::Settings::default()
        );
        let mut settings = crate::paint_bucket::Settings::default();
        for invalid in [
            crate::paint_bucket::Settings {
                close_gap: 17,
                ..settings.clone()
            },
            crate::paint_bucket::Settings {
                reference_layers: vec!["missing".into()],
                ..settings.clone()
            },
        ] {
            legacy["paintBucketSettings"] = serde_json::to_value(invalid).unwrap();
            assert!(
                Document::from_document_state(serde_json::from_value(legacy.clone()).unwrap())
                    .is_err()
            );
        }
        settings.reference_layers = vec!["layer-1".into(), "layer-1".into()];
        legacy["paintBucketSettings"] = serde_json::to_value(settings).unwrap();
        assert!(Document::from_document_state(serde_json::from_value(legacy).unwrap()).is_err());
    }
    #[test]
    fn reference_sampling_is_read_only_and_rejects_stale_or_hidden_sources() {
        let mut doc = Document::default();
        let first = doc.add_paint_layer().unwrap();
        let second = doc.add_vector_layer().unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let settings = crate::paint_bucket::Settings {
            use_reference_layers: true,
            reference_layers: vec![second.clone()],
            ..Default::default()
        };
        let sampled = doc.paint_bucket_sampling_document(&settings).unwrap();
        assert!(!sampled.visible);
        assert!(
            !sampled
                .svg_layers
                .iter()
                .find(|l| l.id == first)
                .unwrap()
                .visible
        );
        assert!(
            sampled
                .svg_layers
                .iter()
                .find(|l| l.id == second)
                .unwrap()
                .visible
        );
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        for ids in [
            vec![],
            vec!["missing".into()],
            vec![second.clone(), second.clone()],
        ] {
            assert!(doc
                .paint_bucket_sampling_document(&crate::paint_bucket::Settings {
                    reference_layers: ids,
                    ..settings.clone()
                })
                .is_err());
        }
        doc.svg_layers
            .iter_mut()
            .find(|l| l.id == second)
            .unwrap()
            .visible = false;
        assert!(doc.paint_bucket_sampling_document(&settings).is_err());
        assert!(
            doc.paint_bucket_sampling_document(&crate::paint_bucket::Settings {
                reference_layers: vec!["layer-1".into()],
                ..settings
            })
            .unwrap()
            .visible
        );
    }
    #[test]
    fn reference_sampling_excludes_editable_text_without_changing_document() {
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Line art".into(),
                ..Default::default()
            },
            position: [10., 20.],
            color: [0, 0, 0],
        })
        .unwrap();
        let layer = doc.svg_layers[0].id.clone();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let sampled = doc
            .paint_bucket_sampling_document(&crate::paint_bucket::Settings {
                all_layers: true,
                exclude_text: true,
                ..Default::default()
            })
            .unwrap();
        assert!(sampled.svg_layers[0].vector_objects.is_empty());
        assert!(!sampled.svg_layers[0].source.contains("<text"));
        assert!(doc
            .paint_bucket_sampling_document(&crate::paint_bucket::Settings {
                use_reference_layers: true,
                reference_layers: vec![layer],
                exclude_text: true,
                ..Default::default()
            })
            .is_err());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }
    #[test]
    fn retouch_accepts_alpha_lock_but_preserves_layer_guards_and_history() {
        for base in [true, false] {
            let mut doc = Document::default();
            if !base {
                let id = doc.add_paint_layer().unwrap();
                doc.select_layer(id).unwrap();
            }
            let id = doc.snapshot().layer_id;
            doc.set_layer_settings(LayerSettings {
                id: id.clone(),
                name: "Protected".into(),
                opacity: 1.,
                locked: false,
                alpha_locked: true,
                mask_enabled: false,
                mask_inverted: false,
                mask_density: 1.,
            })
            .unwrap();
            assert!(doc.clone_stamp_workspace().is_err());
            assert!(doc.retouch_workspace().is_ok());
            let before = serde_json::to_value(doc.document_state()).unwrap();
            assert!(doc.retouch_preview_document("<svg/>".into()).is_ok());
            assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
            doc.replace_retouch_pixels("<svg/>".into(), None).unwrap();
            assert!(
                doc.snapshot()
                    .layers
                    .iter()
                    .find(|l| l.id == id)
                    .unwrap()
                    .alpha_locked
            );
            let after = serde_json::to_value(doc.document_state()).unwrap();
            doc.undo();
            assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
            doc.redo();
            assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
            doc.set_layer_settings(LayerSettings {
                id,
                name: "Protected".into(),
                opacity: 1.,
                locked: true,
                alpha_locked: true,
                mask_enabled: false,
                mask_inverted: false,
                mask_density: 1.,
            })
            .unwrap();
            let locked = serde_json::to_value(doc.document_state()).unwrap();
            assert!(doc.retouch_workspace().is_err());
            assert!(doc.replace_retouch_pixels("<svg/>".into(), None).is_err());
            assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), locked);
        }
    }
    #[test]
    fn cloning_pixels_is_one_atomic_history_entry() {
        let mut doc = Document::default();
        doc.begin(Point { x: 12., y: 20. }, Brush::default())
            .unwrap();
        doc.finish();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let workspace = doc.clone_stamp_workspace().unwrap();
        assert_eq!(workspace.strokes.len(), 1);
        assert_eq!(workspace.canvas_color, CanvasColor::Transparent);
        doc.replace_moved_pixels(r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect width="20" height="20" fill="red"/></svg>"#.into(),None).unwrap();
        let after = serde_json::to_value(doc.document_state()).unwrap();
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), after);
        doc.layer_locked = true;
        assert!(doc.clone_stamp_workspace().is_err());
        let locked = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc.replace_moved_pixels("invalid".into(), None).is_err());
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), locked);
    }
    #[test]
    fn sampling_ranges_and_base_preview_do_not_mutate_document() {
        use crate::clone_stamp::Sample;
        let mut doc = Document::default();
        doc.add_vector_layer().unwrap();
        doc.add_vector_layer().unwrap();
        doc.selected_layer = None;
        let before = serde_json::to_value(doc.document_state()).unwrap();
        assert_eq!(
            doc.clone_stamp_sampling_document(Sample::CurrentBelow)
                .unwrap()
                .svg_layers
                .len(),
            0
        );
        assert_eq!(
            doc.clone_stamp_sampling_document(Sample::AllLayers)
                .unwrap()
                .svg_layers
                .len(),
            2
        );
        let preview = doc.clone_stamp_preview_document("<svg/>".into()).unwrap();
        assert_eq!(preview.svg_layers[0].id, "clone-stamp-preview-base");
        assert_eq!(preview.svg_layers.len(), 3);
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
    }
}
