//! Atomic in-place vector-to-pixel conversion. Raster production belongs to renderer.
use super::*;
impl Document {
    pub fn rasterize_vector_layer(&mut self, id: &str, source: String) -> Result<(), String> {
        if self.path_editing.is_some() {
            return Err(
                "Finish path editing / パスの編集を終了してください / 请先结束路径编辑".into(),
            );
        }
        let index = self
            .svg_layers
            .iter()
            .position(|l| l.id == id)
            .ok_or("Layer not found / レイヤーが見つかりません / 找不到图层")?;
        let current = &self.svg_layers[index];
        if !current.vector_layer || current.locked {
            return Err("Select an unlocked vector layer / ロックされていないベクターレイヤーを選択してください / 请选择未锁定的矢量图层".into());
        }
        let mut layer = current.clone();
        let removed: std::collections::HashSet<_> =
            layer.vector_objects.iter().map(|o| o.id.clone()).collect();
        layer.source = source;
        layer.vector_layer = false;
        layer.paint_layer = true;
        layer.vector_objects.clear();
        layer.motion_scene = None;
        validate_svg_layer(&layer)?;
        let total = self
            .svg_layers
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, l)| l.source.len())
            .sum::<usize>()
            + layer.source.len();
        if total > MAX_SVG_TOTAL_BYTES {
            return Err("Project contains too much image data / 画像データが上限を超えます / 图像数据超出限制".into());
        }
        self.finish();
        let previous = self.vector_history_state();
        self.svg_layers[index] = layer;
        self.locked_objects.retain(|id| !removed.contains(id));
        self.selected_vector_objects.clear();
        self.selected_layer = Some(id.into());
        self.layer_edit_selection = None;
        self.layer_groups.selected = vec![id.into()];
        self.record_vector_edit(previous);
        self.revision += 1;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rasterize_vector_layer_preserves_identity_appearance_and_undo() {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        d.svg_layers[0].opacity = 0.4;
        d.svg_layers[0].visible = false;
        d.svg_layers[0].mask_enabled = true;
        d.svg_layers[0].mask_density = 0.6;
        let original = d.svg_layers().find(|l| l.id == id).unwrap().source.clone();
        let pixel="<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><image width=\"1\" height=\"1\" href=\"data:image/png;base64,iVBORw0KGgo=\"/></svg>";
        let before = d.snapshot();
        d.rasterize_vector_layer(&id, pixel.into()).unwrap();
        let layer = d.svg_layers().find(|l| l.id == id).unwrap();
        assert!(layer.paint_layer && !layer.vector_layer);
        assert_eq!(layer.id, id);
        assert_eq!(
            layer.name,
            before.layers.iter().find(|l| l.id == id).unwrap().name
        );
        assert_eq!(layer.opacity, 0.4);
        assert!(!layer.visible);
        assert!(layer.mask_enabled);
        assert_eq!(layer.mask_density, 0.6);
        assert!(layer.vector_objects.is_empty());
        let loaded = Document::from_document_state(d.document_state()).unwrap();
        assert!(
            loaded
                .svg_layers()
                .find(|l| l.id == id)
                .unwrap()
                .paint_layer
        );
        d.undo();
        let layer = d.svg_layers().find(|l| l.id == id).unwrap();
        assert!(layer.vector_layer);
        assert_eq!(layer.source, original);
        d.redo();
        assert!(d.svg_layers().find(|l| l.id == id).unwrap().paint_layer);
    }
    #[test]
    fn rasterize_vector_layer_rejects_invalid_input_atomically() {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        let revision = d.revision();
        assert!(d.rasterize_vector_layer(&id, "invalid".into()).is_err());
        assert_eq!(d.revision(), revision);
        assert!(d
            .rasterize_vector_layer("layer-1", "<svg/>".into())
            .is_err());
        assert_eq!(d.revision(), revision);
    }
}
