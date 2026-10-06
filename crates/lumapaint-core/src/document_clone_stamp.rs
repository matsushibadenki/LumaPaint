use super::*;
impl Document {
    pub fn clone_stamp_preview_document(&self, source: String) -> Result<Self, String> {
        self.pixel_preview_document(source, false)
    }
    pub fn retouch_preview_document(&self, source: String) -> Result<Self, String> {
        self.pixel_preview_document(source, true)
    }
    fn pixel_preview_document(&self, source: String, preserve_alpha: bool) -> Result<Self, String> {
        let mut result = self.clone();
        if self.selected_layer.is_some() {
            result.replace_image_source(source, preserve_alpha)?;
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
        let mut result = self.clone();
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
