//! Filter presets edit existing canonical effect/animation state, never pixels over IPC.
use super::*;
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterCategory {
    Pixel,
    Vector,
    Animation,
}
#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterPreset {
    Grayscale,
    Invert,
    Contrast,
    Linear,
    Hold,
    Smooth,
}
impl Document {
    /// Color filters target the entire selected layer, independent of selections.
    /// Presets replace their owned adjustment fields; masks and tone survive.
    pub fn apply_filter(
        &mut self,
        id: &str,
        category: FilterCategory,
        preset: FilterPreset,
    ) -> Result<(), String> {
        if self.has_active_stroke() {
            return Err(
                "Finish the current stroke / 描画を確定してください / 请先完成当前笔画".into(),
            );
        }
        if self.editing_channel() != 0
            || self.layer_edit_target() != crate::layer_mask::LayerEditTarget::Content
        {
            return Err(
                "Select layer content / レイヤー本体を選択してください / 请选择图层内容".into(),
            );
        }
        let pixel = if id == "layer-1" {
            true
        } else {
            self.svg_layers
                .iter()
                .find(|l| l.id == id)
                .ok_or("Layer not found / レイヤーがありません / 图层不存在")?
                .paint_layer
        };
        let locked = self.locked_artwork_layers.contains(id)
            || if id == "layer-1" {
                self.layer_locked
            } else {
                self.svg_layers.iter().find(|l| l.id == id).unwrap().locked
            };
        if locked {
            return Err("Layer is locked / レイヤーがロックされています / 图层已锁定".into());
        }
        match category {
            FilterCategory::Animation => {
                let interpolation = match preset {
                    FilterPreset::Linear => animation::Interpolation::Linear,
                    FilterPreset::Hold => animation::Interpolation::Hold,
                    FilterPreset::Smooth => animation::Interpolation::Ease,
                    _ => {
                        return Err(
                            "Incompatible filter / 対応しないフィルターです / 不兼容的滤镜".into(),
                        )
                    }
                };
                self.filter_animation_keys(id, interpolation)
            }
            FilterCategory::Pixel | FilterCategory::Vector => {
                if pixel != matches!(category, FilterCategory::Pixel) {
                    return Err("Select a compatible layer / 対応するレイヤーを選択してください / 请选择兼容图层".into());
                }
                let mut effects = self.layer_effects(id);
                match preset {
                    FilterPreset::Grayscale => {
                        // Keep tone/alpha, but remove later color reintroduction.
                        effects.values[8] = 0.;
                        effects.values[9] = -100.;
                        for channel in 1..4 {
                            effects.curves[channel] = vec![[0., 0.], [1., 1.]];
                            effects.curve_smooth[channel] = true;
                        }
                        for zone in &mut effects.grading {
                            zone[1] = 0.;
                        }
                    }
                    FilterPreset::Invert => {
                        effects.curves[0] = vec![[0., 1.], [1., 0.]];
                        effects.curve_smooth[0] = false;
                    }
                    FilterPreset::Contrast => effects.values[1] = 25.,
                    _ => {
                        return Err(
                            "Incompatible filter / 対応しないフィルターです / 不兼容的滤镜".into(),
                        )
                    }
                }
                effects.enabled = true;
                self.set_layer_effects(id, effects)
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layer_filters_preserve_artwork_and_are_atomic_idempotent() {
        let mut d = Document::default();
        let before = d.document_state();
        d.apply_filter("layer-1", FilterCategory::Pixel, FilterPreset::Invert)
            .unwrap();
        let revision = d.revision();
        assert_eq!(
            d.layer_effects("layer-1").curves[0],
            vec![[0., 1.], [1., 0.]]
        );
        d.apply_filter("layer-1", FilterCategory::Pixel, FilterPreset::Invert)
            .unwrap();
        assert_eq!(d.revision(), revision);
        d.undo();
        assert_eq!(
            serde_json::to_value(d.document_state()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        d.redo();
        assert!(d.layer_effects("layer-1").enabled);
        let revision = d.revision();
        assert!(d
            .apply_filter("layer-1", FilterCategory::Vector, FilterPreset::Grayscale)
            .is_err());
        assert_eq!(d.revision(), revision);
        let id = d.add_vector_layer().unwrap();
        d.apply_filter(&id, FilterCategory::Vector, FilterPreset::Grayscale)
            .unwrap();
        assert_eq!(d.layer_effects(&id).values[9], -100.);
    }
    #[test]
    fn color_filters_preserve_alpha_and_unrelated_adjustments() {
        let mut d = Document::default();
        d.apply_filter("layer-1", FilterCategory::Pixel, FilterPreset::Invert)
            .unwrap();
        assert_eq!(
            d.layer_effects("layer-1").apply([20, 100, 200, 77]),
            [235, 155, 55, 77]
        );
        let mut e = crate::layer_effects::LayerEffects {
            enabled: true,
            ..Default::default()
        };
        e.values[0] = 1.;
        e.values[8] = 50.;
        e.grading[0] = [120., 50., 0.];
        e.curves[1] = vec![[0., 0.], [1., 0.5]];
        e.grading_balance = 20.;
        d.set_layer_effects("layer-1", e).unwrap();
        d.apply_filter("layer-1", FilterCategory::Pixel, FilterPreset::Grayscale)
            .unwrap();
        let e = d.layer_effects("layer-1");
        let p = e.apply([20, 100, 200, 77]);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        assert_eq!(p[3], 77);
        assert_eq!(e.values[0], 1.);
        assert_eq!(e.grading_balance, 20.);
        let rev = d.revision();
        assert!(d
            .apply_filter("layer-1", FilterCategory::Pixel, FilterPreset::Hold)
            .is_err());
        assert_eq!(d.revision(), rev);
        d.layer_locked = true;
        assert!(d
            .apply_filter("layer-1", FilterCategory::Pixel, FilterPreset::Contrast)
            .is_err());
        assert_eq!(d.revision(), rev);
    }
    #[test]
    fn animation_filter_preserves_values_and_has_one_undo() {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        for (frame, value) in [(0, 0.), (12, 120.)] {
            d.set_animation_key(
                &id,
                animation::Property::X,
                animation::Keyframe {
                    frame,
                    value,
                    interpolation: animation::Interpolation::Linear,
                },
            )
            .unwrap();
        }
        let original = serde_json::to_value(d.animation()).unwrap();
        d.apply_filter(&id, FilterCategory::Animation, FilterPreset::Hold)
            .unwrap();
        assert_eq!(
            d.animation().tracks[&id].value(animation::Property::X, 6),
            0.
        );
        let rev = d.revision();
        d.apply_filter(&id, FilterCategory::Animation, FilterPreset::Hold)
            .unwrap();
        assert_eq!(d.revision(), rev);
        d.undo();
        assert_eq!(serde_json::to_value(d.animation()).unwrap(), original);
        assert_eq!(
            d.animation().tracks[&id].value(animation::Property::X, 6),
            60.
        );
        assert!(d
            .apply_filter("layer-1", FilterCategory::Animation, FilterPreset::Hold)
            .is_err());
    }
}
