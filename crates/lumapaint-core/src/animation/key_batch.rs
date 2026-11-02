//! Atomic edits of selected keys in one property. The document owns all values.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum KeyBatchEdit {
    Move {
        #[serde(rename = "frameDelta")]
        frame_delta: i32,
        #[serde(rename = "valueDelta")]
        value_delta: f32,
    },
    ShiftTime {
        delta: i32,
    },
    ShiftValue {
        delta: f32,
    },
    Interpolation {
        interpolation: Interpolation,
    },
    Curve {
        curve: KeyCurve,
    },
    Delete,
}
fn invalid() -> String {
    "Invalid keys, collision or output limit / キーの選択・重複・出力範囲を確認してください / 请检查关键帧选择、重叠和输出范围".into()
}
impl Document {
    pub fn edit_animation_keys(
        &mut self,
        id: &str,
        property: Property,
        frames: &[u32],
        edit: KeyBatchEdit,
    ) -> Result<(), String> {
        self.animation_layer(id, false)?;
        let selected: BTreeSet<_> = frames.iter().copied().collect();
        if frames.is_empty() || frames.len() > 4096 || selected.len() != frames.len() {
            return Err(invalid());
        }
        let mut next = self.animation.as_ref().clone();
        let keys = next
            .tracks
            .get_mut(id)
            .and_then(|t| t.channels.get_mut(&property))
            .ok_or_else(invalid)?;
        if selected.iter().any(|f| !keys.iter().any(|k| k.frame == *f)) {
            return Err(invalid());
        }
        let before = keys.clone();
        for key in keys.iter_mut().filter(|k| selected.contains(&k.frame)) {
            match edit {
                KeyBatchEdit::Move {
                    frame_delta,
                    value_delta,
                } => {
                    let frame = i64::from(key.frame) + i64::from(frame_delta);
                    let value = key.value + value_delta;
                    if frame < 0
                        || frame >= i64::from(next.duration)
                        || !value_delta.is_finite()
                        || !property.valid(value)
                    {
                        return Err(invalid());
                    }
                    key.frame = frame as u32;
                    key.value = value;
                }
                KeyBatchEdit::ShiftTime { delta } => {
                    let frame = i64::from(key.frame) + i64::from(delta);
                    if frame < 0 || frame >= i64::from(next.duration) {
                        return Err(invalid());
                    }
                    key.frame = frame as u32;
                }
                KeyBatchEdit::ShiftValue { delta } => {
                    let value = key.value + delta;
                    if !delta.is_finite() || !property.valid(value) {
                        return Err(invalid());
                    }
                    key.value = value;
                }
                KeyBatchEdit::Interpolation { interpolation } => key.interpolation = interpolation,
                KeyBatchEdit::Curve { curve } => {
                    if !curve.valid() {
                        return Err(invalid());
                    }
                    key.interpolation = Interpolation::Cubic;
                    key.curve = Some(curve);
                }
                KeyBatchEdit::Delete => (),
            }
        }
        if matches!(edit, KeyBatchEdit::Delete) {
            keys.retain(|k| !selected.contains(&k.frame));
        }
        keys.sort_by_key(|k| k.frame);
        if keys.windows(2).any(|w| w[0].frame == w[1].frame) {
            return Err(invalid());
        }
        if keys.len() == before.len()
            && keys.iter().zip(&before).all(|(a, b)| {
                a.frame == b.frame
                    && a.value == b.value
                    && a.interpolation == b.interpolation
                    && a.curve == b.curve
            })
        {
            return Ok(());
        }
        self.commit_animation(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Document, String) {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        for (frame, value) in [(5, 10.), (15, 30.), (30, 80.)] {
            d.set_animation_key(
                &id,
                Property::Opacity,
                Keyframe {
                    curve: None,
                    frame,
                    value,
                    interpolation: Interpolation::Ease,
                },
            )
            .unwrap();
        }
        (d, id)
    }
    fn encoded(d: &Document) -> String {
        serde_json::to_string(&d.document_state()).unwrap()
    }
    #[test]
    fn key_batch_preserves_spacing_values_and_one_history_entry() {
        let (mut d, id) = fixture();
        let before = encoded(&d);
        let undo = d.undo_order.len();
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[5, 15],
            KeyBatchEdit::ShiftTime { delta: 10 },
        )
        .unwrap();
        let keys = &d.animation.tracks[&id].channels[&Property::Opacity];
        assert_eq!(
            keys.iter().map(|k| k.frame).collect::<Vec<_>>(),
            vec![15, 25, 30]
        );
        assert_eq!(
            keys.iter().map(|k| k.value).collect::<Vec<_>>(),
            vec![10., 30., 80.]
        );
        assert_eq!(d.undo_order.len(), undo + 1);
        let after = encoded(&d);
        d.undo();
        assert_eq!(encoded(&d), before);
        d.redo();
        assert_eq!(encoded(&d), after);
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[15, 25],
            KeyBatchEdit::ShiftValue { delta: 20. },
        )
        .unwrap();
        assert_eq!(
            d.animation.tracks[&id].channels[&Property::Opacity][1].value,
            50.
        );
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[15, 25],
            KeyBatchEdit::Interpolation {
                interpolation: Interpolation::Hold,
            },
        )
        .unwrap();
        assert!(d.animation.tracks[&id].channels[&Property::Opacity][..2]
            .iter()
            .all(|k| k.interpolation == Interpolation::Hold));
        let before = encoded(&d);
        d.edit_animation_keys(&id, Property::Opacity, &[15, 25, 30], KeyBatchEdit::Delete)
            .unwrap();
        assert!(d.animation.tracks[&id].channels[&Property::Opacity].is_empty());
        d.undo();
        assert_eq!(encoded(&d), before);
    }
    #[test]
    fn key_batch_rejections_and_noops_are_atomic() {
        let (mut d, id) = fixture();
        let before = encoded(&d);
        let revision = d.revision();
        let undo = d.undo_order.len();
        for (frames, edit) in [
            (vec![], KeyBatchEdit::Delete),
            (vec![5, 5], KeyBatchEdit::Delete),
            (vec![5, 6], KeyBatchEdit::Delete),
            (vec![5, 15], KeyBatchEdit::ShiftTime { delta: 15 }),
            (vec![5, 15], KeyBatchEdit::ShiftTime { delta: -6 }),
            (vec![5, 15], KeyBatchEdit::ShiftTime { delta: 18000 }),
            (vec![5, 30], KeyBatchEdit::ShiftValue { delta: 21. }),
            (vec![5, 15], KeyBatchEdit::ShiftValue { delta: f32::NAN }),
        ] {
            assert!(d
                .edit_animation_keys(&id, Property::Opacity, &frames, edit)
                .is_err());
            assert_eq!(encoded(&d), before);
            assert_eq!(d.revision(), revision);
            assert_eq!(d.undo_order.len(), undo);
        }
        for edit in [
            KeyBatchEdit::ShiftTime { delta: 0 },
            KeyBatchEdit::ShiftValue { delta: 0. },
            KeyBatchEdit::Interpolation {
                interpolation: Interpolation::Ease,
            },
        ] {
            d.edit_animation_keys(&id, Property::Opacity, &[5, 15], edit)
                .unwrap();
            assert_eq!(d.revision(), revision);
            assert_eq!(d.undo_order.len(), undo);
        }
    }
    #[test]
    fn key_batch_keeps_other_channels_and_rejects_locked_layers() {
        let (mut d, id) = fixture();
        d.set_animation_key(
            &id,
            Property::Y,
            Keyframe {
                curve: None,
                frame: 5,
                value: 42.,
                interpolation: Interpolation::Linear,
            },
        )
        .unwrap();
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[5, 15],
            KeyBatchEdit::ShiftTime { delta: 10 },
        )
        .unwrap();
        let state = d.document_state();
        let serialized = serde_json::to_string(&state).unwrap();
        let restored = serde_json::from_str::<serde_json::Value>(&serialized).unwrap();
        assert_eq!(
            restored["animation"]["tracks"][&id]["channels"]["y"][0]["value"],
            42.
        );
        assert_eq!(
            restored["animation"]["tracks"][&id]["channels"]["opacity"][1]["frame"],
            25
        );
        d.locked_artwork_layers.insert(id.clone());
        let before = encoded(&d);
        let revision = d.revision();
        assert!(d
            .edit_animation_keys(&id, Property::Opacity, &[15, 25], KeyBatchEdit::Delete)
            .is_err());
        assert_eq!(encoded(&d), before);
        assert_eq!(d.revision(), revision);
    }
    #[test]
    fn simultaneous_key_move_is_one_atomic_history_entry() {
        let (mut d, id) = fixture();
        let before = encoded(&d);
        let undo = d.undo_order.len();
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[5, 15],
            KeyBatchEdit::Move {
                frame_delta: 10,
                value_delta: 15.,
            },
        )
        .unwrap();
        let keys = &d.animation.tracks[&id].channels[&Property::Opacity];
        assert_eq!(
            keys.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>(),
            vec![(15, 25.), (25, 45.), (30, 80.)]
        );
        assert!(keys.iter().all(|k| k.interpolation == Interpolation::Ease));
        assert_eq!(d.undo_order.len(), undo + 1);
        let after = encoded(&d);
        d.undo();
        assert_eq!(encoded(&d), before);
        d.redo();
        assert_eq!(encoded(&d), after);
    }
    #[test]
    fn simultaneous_key_move_rejects_both_axes_without_partial_edits() {
        let (mut d, id) = fixture();
        let before = encoded(&d);
        let revision = d.revision();
        let undo = d.undo_order.len();
        for (frame_delta, value_delta) in [
            (15, 5.),
            (-6, 5.),
            (18000, 5.),
            (1, 91.),
            (1, -11.),
            (1, f32::NAN),
            (1, f32::INFINITY),
        ] {
            assert!(d
                .edit_animation_keys(
                    &id,
                    Property::Opacity,
                    &[5, 15],
                    KeyBatchEdit::Move {
                        frame_delta,
                        value_delta
                    }
                )
                .is_err());
            assert_eq!(encoded(&d), before);
            assert_eq!(d.revision(), revision);
            assert_eq!(d.undo_order.len(), undo);
        }
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[5, 15],
            KeyBatchEdit::Move {
                frame_delta: 0,
                value_delta: 0.,
            },
        )
        .unwrap();
        assert_eq!(d.revision(), revision);
        assert_eq!(d.undo_order.len(), undo);
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[5, 15],
            KeyBatchEdit::Move {
                frame_delta: 0,
                value_delta: 5.,
            },
        )
        .unwrap();
        assert_eq!(
            d.animation.tracks[&id].channels[&Property::Opacity][0].value,
            15.
        );
        assert!(serde_json::from_str::<KeyBatchEdit>(
            r#"{"kind":"move","frameDelta":1,"valueDelta":2}"#
        )
        .is_ok());
        assert!(serde_json::from_str::<KeyBatchEdit>(r#"{"kind":"move","frameDelta":1}"#).is_err());
    }
}
