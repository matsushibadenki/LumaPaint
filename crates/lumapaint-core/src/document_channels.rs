use super::*;
use crate::layer_mask::LayerEditTarget;

impl Document {
    pub fn editing_channel(&self) -> u32 {
        self.editing_channel
    }
    pub fn select_channel(&mut self, channel: u32) -> Result<(), String> {
        if channel > 8 {
            return Err("Unknown channel".into());
        }
        if self.layer_edit_target() == LayerEditTarget::Mask && ![0, 4].contains(&channel) {
            return Err("Masks have one grayscale channel / マスクはグレースケールの1チャンネルです / 蒙版只有一个灰度通道".into());
        }
        self.finish();
        self.editing_channel = channel;
        Ok(())
    }
    /// Read-only channel scene: inspect the selected target, without other layers
    /// or the canvas display color obscuring its transparency.
    pub fn channel_view(&self) -> Self {
        let mut view = self.clone_for_rendering();
        view.canvas_color = CanvasColor::Transparent;
        view.selected_vector_objects.clear();
        view.layer_groups = Default::default();
        if self.layer_edit_target() == LayerEditTarget::Mask {
            let mut mask = self.layer_effects(self.selected_layer_id()).mask;
            if let Some(mask) = &mut mask {
                mask.enabled = true;
            }
            view.svg_layers.clear();
            view.strokes.clear();
            view.active = None;
            view.paint_source = Some(format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}"><rect width="100%" height="100%" fill="white"/></svg>"#,
                self.width, self.height
            ));
            view.visible = true;
            view.layer_opacity = 1.;
            view.layer_mask_enabled = false;
            view.layer_effects.clear();
            view.layer_effects.insert(
                "layer-1".into(),
                crate::layer_effects::LayerEffects {
                    mask,
                    ..Default::default()
                },
            );
            view.selected_layer = None;
            view.layer_edit_selection = None;
        } else if self.selected_layer_id() == "layer-1" {
            view.svg_layers.clear();
        } else {
            view.visible = false;
            view.strokes.clear();
            view.active = None;
            view.paint_source = None;
            view.svg_layers.retain(|l| l.id == self.selected_layer_id());
        }
        view
    }
    /// A blank, clipped workspace for native channel brush coverage.
    pub fn channel_brush_workspace(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            canvas_color: CanvasColor::Transparent,
            selection: self.selection.clone(),
            ..Default::default()
        }
    }
    pub fn channel_paint_preview(&self, source: String) -> Result<Self, String> {
        let mut source_doc = self.clone_for_rendering();
        if self.selected_layer_id() == "layer-1" {
            source_doc.selected_layer = None;
        }
        let mut view = source_doc.retouch_preview_document(source)?;
        if self.selected_layer_id() == "layer-1" {
            view.selected_layer = Some("clone-stamp-preview-base".into());
            view.layer_edit_selection = None;
        }
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        layer_effects::LayerEffects,
        layer_mask::{LayerMask, MaskKind},
    };
    #[test]
    fn channel_focus_is_transient_and_mask_focus_selects_alpha() {
        let mut doc = Document::default();
        doc.set_layer_effects(
            "layer-1",
            LayerEffects {
                mask: Some(LayerMask::from_selection(MaskKind::Pixel, None, 960, 640).unwrap()),
                ..Default::default()
            },
        )
        .unwrap();
        let state = doc.document_state();
        let revision = doc.revision();
        doc.select_channel(2).unwrap();
        doc.select_layer_target("layer-1".into(), LayerEditTarget::Mask)
            .unwrap();
        assert_eq!(doc.editing_channel(), 4);
        assert_eq!(doc.snapshot().editing_channel, 4);
        assert_eq!(doc.clone_for_rendering().editing_channel(), 4);
        assert!(doc.select_channel(3).is_err());
        assert!(doc.select_channel(9).is_err());
        assert_eq!(doc.revision(), revision);
        assert_eq!(
            serde_json::to_string(&doc.document_state()).unwrap(),
            serde_json::to_string(&state).unwrap()
        );
        let view = doc.channel_view();
        assert_eq!(view.dimensions(), doc.dimensions());
        assert_eq!(view.snapshot().canvas_color, CanvasColor::Transparent);
        assert!(view.paint_source().is_some());
        assert_eq!(doc.layer_edit_target(), LayerEditTarget::Mask);
    }
}
