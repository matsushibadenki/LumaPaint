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
    LinkedCurve {
        curve: KeyCurve,
        control: u8,
    },
    Velocity {
        control: u8,
        value: f64,
    },
    Delete,
}
fn invalid() -> String {
    "Invalid keys, collision or output limit / キーの選択・重複・出力範囲を確認してください / 请检查关键帧选择、重叠和输出范围".into()
}
// Link existing cubic intervals in document units, not normalized percentages.
// With bounded values, a local extremum must have a horizontal tangent.
fn link_curve(
    keys: &mut [Keyframe],
    i: usize,
    mut curve: KeyCurve,
    control: u8,
) -> Result<(), String> {
    if i + 1 >= keys.len() || keys[i].interpolation != Interpolation::Cubic {
        return Err(invalid());
    }
    let neighbor = if control == 1 {
        i.checked_sub(1)
    } else if i + 2 < keys.len() {
        Some(i + 1)
    } else {
        None
    };
    if let Some(j) = neighbor.filter(|&j| keys[j].interpolation == Interpolation::Cubic) {
        let mut other = keys[j].curve.unwrap_or_default();
        let dt = f64::from(keys[i + 1].frame - keys[i].frame);
        let dv = f64::from(keys[i + 1].value) - f64::from(keys[i].value);
        let nt = f64::from(keys[j + 1].frame - keys[j].frame);
        let nv = f64::from(keys[j + 1].value) - f64::from(keys[j].value);
        let (x, mut y) = if control == 1 {
            (curve.x1.max(0.0001), curve.y1)
        } else {
            ((1. - curve.x2).max(0.0001), 1. - curve.y2)
        };
        if dv * nv <= 0. {
            y = 0.;
        }
        if control == 1 {
            curve.x1 = x;
            curve.x2 = curve.x2.max(x);
            curve.y1 = y;
        } else {
            curve.x2 = 1. - x;
            curve.x1 = curve.x1.min(curve.x2);
            curve.y2 = 1. - y;
        }
        let (x, y) = if control == 1 {
            (curve.x1, curve.y1)
        } else {
            (1. - curve.x2, 1. - curve.y2)
        };
        let slope = dv * f64::from(y) / (dt * f64::from(x));
        let ratio = if nv == 0. {
            0.
        } else {
            (slope * nt / nv).abs()
        };
        let wanted = if control == 1 {
            1. - other.x2
        } else {
            other.x1
        };
        let x = f64::from(wanted.max(0.0001)).min(if ratio > 0. { 1. / ratio } else { 1. }) as f32;
        let y = (ratio * f64::from(x)).min(1.) as f32;
        if control == 1 {
            other.x2 = 1. - x;
            other.x1 = other.x1.min(other.x2);
            other.y2 = 1. - y;
        } else {
            other.x1 = x;
            other.x2 = other.x2.max(x);
            other.y1 = y;
        }
        let dx = if control == 1 {
            1. - other.x2
        } else {
            other.x1
        };
        if dx <= 0. || !other.valid() || !curve.valid() {
            return Err(invalid());
        }
        keys[j].curve = Some(other);
    }
    keys[i].curve = Some(curve);
    Ok(())
}

// Edit one side of an interval in units/second, preserving its time handles.
fn velocity_curve(
    a: &Keyframe,
    b: &Keyframe,
    fps: u32,
    control: u8,
    value: f64,
) -> Result<KeyCurve, String> {
    if !(1..=2).contains(&control)
        || !value.is_finite()
        || fps == 0
        || b.frame <= a.frame
        || a.interpolation == Interpolation::Hold
    {
        return Err(invalid());
    }
    let mut c = match a.interpolation {
        Interpolation::Linear => KeyCurve {
            x1: 1. / 3.,
            y1: 1. / 3.,
            x2: 2. / 3.,
            y2: 2. / 3.,
        },
        Interpolation::Ease => KeyCurve::default(),
        _ => a.curve.unwrap_or_default(),
    };
    let x = if control == 1 {
        f64::from(c.x1)
    } else {
        1. - f64::from(c.x2)
    };
    let rate =
        (f64::from(b.value) - f64::from(a.value)) * f64::from(fps) / f64::from(b.frame - a.frame);
    if !c.valid() || x <= 0. || rate == 0. {
        return Err(invalid());
    }
    let limit = rate / x;
    if value < limit.min(0.) || value > limit.max(0.) {
        return Err(invalid());
    }
    let y = (value / rate * x).clamp(0., 1.);
    if control == 1 {
        c.y1 = y as f32;
    } else {
        c.y2 = (1. - y) as f32;
    }
    Ok(c)
}

fn apply_keys(
    keys: &mut Vec<Keyframe>,
    property: Property,
    duration: u32,
    fps: u32,
    frames: &[u32],
    edit: KeyBatchEdit,
) -> Result<bool, String> {
    let selected: BTreeSet<_> = frames.iter().copied().collect();
    if frames.is_empty() || frames.len() > 4096 || selected.len() != frames.len() {
        return Err(invalid());
    }
    if selected.iter().any(|f| !keys.iter().any(|k| k.frame == *f)) {
        return Err(invalid());
    }
    let before = keys.clone();
    if let KeyBatchEdit::LinkedCurve { curve, control } = edit {
        if frames.len() != 1 || !curve.valid() || !(1..=2).contains(&control) {
            return Err(invalid());
        }
        let index = keys
            .iter()
            .position(|k| k.frame == frames[0])
            .ok_or_else(invalid)?;
        link_curve(keys, index, curve, control)?;
    }
    if let KeyBatchEdit::Velocity { control, value } = edit {
        if frames.len() != 1 {
            return Err(invalid());
        }
        let i = keys
            .iter()
            .position(|k| k.frame == frames[0])
            .ok_or_else(invalid)?;
        let b = keys.get(i + 1).ok_or_else(invalid)?;
        let curve = velocity_curve(&keys[i], b, fps, control, value)?;
        keys[i].interpolation = Interpolation::Cubic;
        keys[i].curve = Some(curve);
    }
    for key in keys.iter_mut().filter(|k| selected.contains(&k.frame)) {
        match edit {
            KeyBatchEdit::Move {
                frame_delta,
                value_delta,
            } => {
                let frame = i64::from(key.frame) + i64::from(frame_delta);
                let value = key.value + value_delta;
                if frame < 0
                    || frame >= i64::from(duration)
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
                if frame < 0 || frame >= i64::from(duration) {
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
            KeyBatchEdit::Delete
            | KeyBatchEdit::LinkedCurve { .. }
            | KeyBatchEdit::Velocity { .. } => (),
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
        return Ok(false);
    }
    Ok(true)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyKeys {
    pub property: Property,
    pub frames: Vec<u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayerPropertyKeys {
    pub layer: String,
    pub selections: Vec<PropertyKeys>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum KeyGroupEdit {
    ShiftValues {
        position: f32,
        rotation: f32,
        scale: f32,
        opacity: f32,
    },
    ShiftTime {
        delta: i32,
    },
    Interpolation {
        interpolation: Interpolation,
    },
    Delete,
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
        let mut next = self.animation.as_ref().clone();
        let duration = next.duration;
        let fps = next.fps;
        let keys = next
            .tracks
            .get_mut(id)
            .and_then(|t| t.channels.get_mut(&property))
            .ok_or_else(invalid)?;
        if apply_keys(keys, property, duration, fps, frames, edit)? {
            self.commit_animation(next)
        } else {
            Ok(())
        }
    }
    pub fn edit_animation_key_group(
        &mut self,
        id: &str,
        selections: &[PropertyKeys],
        edit: KeyGroupEdit,
    ) -> Result<(), String> {
        self.edit_animation_key_tracks(
            &[LayerPropertyKeys {
                layer: id.to_string(),
                selections: selections.to_vec(),
            }],
            edit,
        )
    }
    pub fn edit_animation_key_tracks(
        &mut self,
        tracks: &[LayerPropertyKeys],
        edit: KeyGroupEdit,
    ) -> Result<(), String> {
        if let KeyGroupEdit::ShiftValues {
            position,
            rotation,
            scale,
            opacity,
        } = edit
        {
            if ![position, rotation, scale, opacity]
                .iter()
                .all(|v| v.is_finite())
            {
                return Err(invalid());
            }
        }
        if tracks.is_empty()
            || tracks.len() > 4096
            || tracks
                .iter()
                .flat_map(|t| &t.selections)
                .map(|s| s.frames.len())
                .sum::<usize>()
                > 4096
        {
            return Err(invalid());
        }
        let mut layers = BTreeSet::new();
        for track in tracks {
            self.animation_layer(&track.layer, false)?;
            if !layers.insert(&track.layer)
                || track.selections.is_empty()
                || track.selections.len() > 6
            {
                return Err(invalid());
            }
        }
        let mut next = self.animation.as_ref().clone();
        let duration = next.duration;
        let fps = next.fps;
        let mut changed = false;
        for track in tracks {
            let mut seen = BTreeSet::new();
            for selection in &track.selections {
                if !seen.insert(selection.property) {
                    return Err(invalid());
                }
                let keys = next
                    .tracks
                    .get_mut(&track.layer)
                    .and_then(|t| t.channels.get_mut(&selection.property))
                    .ok_or_else(invalid)?;
                let edit = match edit {
                    KeyGroupEdit::ShiftValues {
                        position,
                        rotation,
                        scale,
                        opacity,
                    } => KeyBatchEdit::ShiftValue {
                        delta: match selection.property {
                            Property::X | Property::Y => position,
                            Property::Rotation => rotation,
                            Property::ScaleX | Property::ScaleY => scale,
                            Property::Opacity => opacity,
                        },
                    },
                    KeyGroupEdit::ShiftTime { delta } => KeyBatchEdit::ShiftTime { delta },
                    KeyGroupEdit::Interpolation { interpolation } => {
                        KeyBatchEdit::Interpolation { interpolation }
                    }
                    KeyGroupEdit::Delete => KeyBatchEdit::Delete,
                };
                changed |= apply_keys(
                    keys,
                    selection.property,
                    duration,
                    fps,
                    &selection.frames,
                    edit,
                )?;
            }
        }
        if changed {
            self.commit_animation(next)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn velocity_edits_preserve_endpoints_time_handles_and_history() {
        for (values, fps, control, speed) in [
            ([10., 90.], 24, 1, 48.),
            ([90., 10.], 60, 2, -120.),
            ([10., 90.], 12, 2, 0.),
        ] {
            let mut d = Document::default();
            let id = d.add_vector_layer().unwrap();
            Arc::make_mut(&mut d.animation).fps = fps;
            for (frame, value) in [(5, values[0]), (25, values[1])] {
                d.set_animation_key(
                    &id,
                    Property::Opacity,
                    Keyframe {
                        frame,
                        value,
                        interpolation: Interpolation::Linear,
                        curve: None,
                    },
                )
                .unwrap();
            }
            let before = serde_json::to_string(&d.document_state()).unwrap();
            let revision = d.revision();
            d.edit_animation_keys(
                &id,
                Property::Opacity,
                &[5],
                KeyBatchEdit::Velocity {
                    control,
                    value: speed,
                },
            )
            .unwrap();
            let keys = &d.animation.tracks[&id].channels[&Property::Opacity];
            let c = keys[0].curve.unwrap();
            assert_eq!(
                keys.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>(),
                vec![(5, values[0]), (25, values[1])]
            );
            assert_eq!((c.x1, c.x2), (1. / 3., 2. / 3.));
            let rate = f64::from(values[1] - values[0]) * f64::from(fps) / 20.;
            let actual = if control == 1 {
                rate * f64::from(c.y1) / f64::from(c.x1)
            } else {
                rate * (1. - f64::from(c.y2)) / (1. - f64::from(c.x2))
            };
            assert!((actual - speed).abs() < 0.0001);
            assert_eq!(d.revision(), revision + 1);
            let saved = serde_json::to_string(&d.document_state()).unwrap();
            d.undo();
            assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), before);
            d.redo();
            assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
            let restored =
                Document::from_document_state(serde_json::from_str(&saved).unwrap()).unwrap();
            assert_eq!(
                restored.animation.tracks[&id].channels[&Property::Opacity][0].curve,
                Some(c)
            );
            let revision = d.revision();
            d.edit_animation_keys(
                &id,
                Property::Opacity,
                &[5],
                KeyBatchEdit::Velocity {
                    control,
                    value: speed,
                },
            )
            .unwrap();
            assert_eq!(d.revision(), revision);
            for (frames, control, value) in [
                (vec![5, 25], 1, 0.),
                (vec![25], 1, 0.),
                (vec![5], 0, 0.),
                (vec![5], 3, 0.),
                (vec![5], 1, f64::NAN),
                (vec![5], 2, f64::INFINITY),
                (vec![5], 1, rate * 100.),
                (vec![5], 2, -rate),
            ] {
                assert!(d
                    .edit_animation_keys(
                        &id,
                        Property::Opacity,
                        &frames,
                        KeyBatchEdit::Velocity { control, value }
                    )
                    .is_err());
                assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
            }
        }
    }
    #[test]
    fn velocity_preserves_other_handle_and_rejects_hold_flat_vertical() {
        let b = Keyframe {
            frame: 20,
            value: 100.,
            interpolation: Interpolation::Linear,
            curve: None,
        };
        let a = Keyframe {
            frame: 0,
            value: 0.,
            interpolation: Interpolation::Cubic,
            curve: Some(KeyCurve {
                x1: 0.2,
                y1: 0.4,
                x2: 0.7,
                y2: 0.8,
            }),
        };
        let c = velocity_curve(&a, &b, 24, 1, 60.).unwrap();
        assert_eq!((c.x1, c.x2, c.y2), (0.2, 0.7, 0.8));
        assert!((c.y1 - 0.1).abs() < 0.00001);
        let ease = Keyframe {
            interpolation: Interpolation::Ease,
            ..a.clone()
        };
        assert_eq!(velocity_curve(&ease, &b, 24, 1, 60.).unwrap().y2, 1.);
        assert!(velocity_curve(
            &Keyframe {
                interpolation: Interpolation::Hold,
                ..a.clone()
            },
            &b,
            24,
            1,
            0.
        )
        .is_err());
        assert!(velocity_curve(
            &a,
            &Keyframe {
                value: 0.,
                ..b.clone()
            },
            24,
            1,
            0.
        )
        .is_err());
        assert!(velocity_curve(
            &Keyframe {
                curve: Some(KeyCurve {
                    x1: 0.,
                    ..KeyCurve::default()
                }),
                ..a.clone()
            },
            &b,
            24,
            1,
            0.
        )
        .is_err());
        assert!(velocity_curve(
            &Keyframe {
                curve: Some(KeyCurve {
                    x2: 1.,
                    ..KeyCurve::default()
                }),
                ..a
            },
            &b,
            24,
            2,
            0.
        )
        .is_err());
        assert!(serde_json::from_str::<KeyBatchEdit>(
            r#"{"kind":"velocity","control":1,"value":60}"#
        )
        .is_ok());
        assert!(
            serde_json::from_str::<KeyBatchEdit>(r#"{"kind":"velocity","control":1}"#).is_err()
        );
    }
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
    fn slope(keys: &[Keyframe], i: usize, control: u8) -> f64 {
        let c = keys[i].curve.unwrap();
        let (x, y) = if control == 1 {
            (c.x1, c.y1)
        } else {
            (1. - c.x2, 1. - c.y2)
        };
        (f64::from(keys[i + 1].value) - f64::from(keys[i].value)) * f64::from(y)
            / (f64::from(keys[i + 1].frame - keys[i].frame) * f64::from(x))
    }
    #[test]
    fn linked_tangents_match_document_slopes_and_commit_once() {
        for (frame, control) in [(5, 2), (15, 1)] {
            let (mut d, id) = fixture();
            d.edit_animation_keys(
                &id,
                Property::Opacity,
                &[5, 15],
                KeyBatchEdit::Interpolation {
                    interpolation: Interpolation::Cubic,
                },
            )
            .unwrap();
            let before = encoded(&d);
            let undo = d.undo_order.len();
            let curve = KeyCurve {
                x1: 0.25,
                y1: 0.4,
                x2: 0.75,
                y2: 0.6,
            };
            d.edit_animation_keys(
                &id,
                Property::Opacity,
                &[frame],
                KeyBatchEdit::LinkedCurve { curve, control },
            )
            .unwrap();
            let keys = &d.animation.tracks[&id].channels[&Property::Opacity];
            assert!((slope(keys, 0, 2) - slope(keys, 1, 1)).abs() < 0.00001);
            assert_eq!(
                keys.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>(),
                vec![(5, 10.), (15, 30.), (30, 80.)]
            );
            assert_eq!(d.undo_order.len(), undo + 1);
            let after = encoded(&d);
            d.undo();
            assert_eq!(encoded(&d), before);
            d.redo();
            assert_eq!(encoded(&d), after);
            let saved = serde_json::to_string(&d.animation).unwrap();
            let restored: Animation = serde_json::from_str(&saved).unwrap();
            assert_eq!(
                restored.tracks[&id].channels[&Property::Opacity][0].curve,
                keys_curve(&d, &id, 0)
            );
            let revision = d.revision();
            d.edit_animation_keys(
                &id,
                Property::Opacity,
                &[frame],
                KeyBatchEdit::LinkedCurve { curve, control },
            )
            .unwrap();
            assert_eq!(d.revision(), revision);
        }
    }
    fn keys_curve(d: &Document, id: &str, i: usize) -> Option<KeyCurve> {
        d.animation.tracks[id].channels[&Property::Opacity][i].curve
    }
    #[test]
    fn linked_extrema_plateaus_and_invalid_requests_are_bounded_and_atomic() {
        for end in [10., 30., 80.] {
            let (mut d, id) = fixture();
            let mut keys = d.animation.tracks[&id].channels[&Property::Opacity].clone();
            keys[2].value = end;
            for key in &mut keys {
                key.interpolation = Interpolation::Cubic;
            }
            link_curve(
                &mut keys,
                0,
                KeyCurve {
                    x1: 0.,
                    y1: 1.,
                    x2: 1.,
                    y2: 0.,
                },
                2,
            )
            .unwrap();
            assert!(keys.iter().all(|k| k.curve.is_none_or(KeyCurve::valid)));
            assert!(
                (slope(&keys, 0, 2) - slope(&keys, 1, 1)).abs()
                    < 0.001 * slope(&keys, 0, 2).abs().max(1.)
            );
            if end <= 30. {
                assert_eq!(slope(&keys, 0, 2), 0.);
                assert_eq!(slope(&keys, 1, 1), 0.);
            }
            let before = encoded(&d);
            let revision = d.revision();
            for (frames, control, curve) in [
                (vec![5, 15], 1, KeyCurve::default()),
                (vec![30], 2, KeyCurve::default()),
                (vec![5], 0, KeyCurve::default()),
                (
                    vec![5],
                    1,
                    KeyCurve {
                        x1: f32::NAN,
                        ..KeyCurve::default()
                    },
                ),
            ] {
                assert!(d
                    .edit_animation_keys(
                        &id,
                        Property::Opacity,
                        &frames,
                        KeyBatchEdit::LinkedCurve { curve, control }
                    )
                    .is_err());
                assert_eq!(encoded(&d), before);
                assert_eq!(d.revision(), revision);
            }
        }
        let (mut d, id) = fixture();
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[5],
            KeyBatchEdit::Interpolation {
                interpolation: Interpolation::Cubic,
            },
        )
        .unwrap();
        let before = d.animation.tracks[&id].channels[&Property::Opacity][1].clone();
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[5],
            KeyBatchEdit::LinkedCurve {
                curve: KeyCurve::default(),
                control: 2,
            },
        )
        .unwrap();
        let other = &d.animation.tracks[&id].channels[&Property::Opacity][1];
        assert_eq!(before.curve, other.curve);
        assert_eq!(before.interpolation, other.interpolation);
        assert!(serde_json::from_str::<KeyBatchEdit>(
            r#"{"kind":"linkedCurve","curve":{"x1":0.2,"y1":0.4,"x2":0.8,"y2":0.6},"control":2}"#
        )
        .is_ok());
    }
    fn group_fixture() -> (Document, String, Vec<PropertyKeys>) {
        let (mut d, id) = fixture();
        for (frame, value) in [(15, 100.), (30, 200.)] {
            d.set_animation_key(
                &id,
                Property::X,
                Keyframe {
                    frame,
                    value,
                    interpolation: Interpolation::Cubic,
                    curve: Some(KeyCurve::default()),
                },
            )
            .unwrap();
        }
        (
            d,
            id,
            vec![
                PropertyKeys {
                    property: Property::Opacity,
                    frames: vec![5],
                },
                PropertyKeys {
                    property: Property::X,
                    frames: vec![15],
                },
            ],
        )
    }
    #[test]
    fn property_group_edits_preserve_values_curves_and_one_undo() {
        let (mut d, id, mut selection) = group_fixture();
        let before = encoded(&d);
        let undo = d.undo_order.len();
        d.edit_animation_key_group(&id, &selection, KeyGroupEdit::ShiftTime { delta: 3 })
            .unwrap();
        assert_eq!(d.undo_order.len(), undo + 1);
        let after = encoded(&d);
        assert_eq!(
            d.animation.tracks[&id].channels[&Property::Opacity][0].frame,
            8
        );
        let x = &d.animation.tracks[&id].channels[&Property::X][0];
        assert_eq!((x.frame, x.value), (18, 100.));
        assert_eq!(x.curve, Some(KeyCurve::default()));
        d.undo();
        assert_eq!(encoded(&d), before);
        d.redo();
        assert_eq!(encoded(&d), after);
        selection[0].frames = vec![8];
        selection[1].frames = vec![18];
        let revision = d.revision();
        d.edit_animation_key_group(&id, &selection, KeyGroupEdit::ShiftTime { delta: 0 })
            .unwrap();
        assert_eq!(d.revision(), revision);
        d.edit_animation_key_group(
            &id,
            &selection,
            KeyGroupEdit::Interpolation {
                interpolation: Interpolation::Hold,
            },
        )
        .unwrap();
        assert_eq!(
            d.animation.tracks[&id].channels[&Property::X][0].curve,
            Some(KeyCurve::default())
        );
        let held = encoded(&d);
        let undo = d.undo_order.len();
        d.edit_animation_key_group(&id, &selection, KeyGroupEdit::Delete)
            .unwrap();
        assert_eq!(d.undo_order.len(), undo + 1);
        assert_eq!(d.animation.tracks[&id].channels[&Property::X].len(), 1);
        d.undo();
        assert_eq!(encoded(&d), held);
        let saved = serde_json::to_string(&d.animation).unwrap();
        let restored: Animation = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.tracks[&id].channels[&Property::X][0].frame, 18);
        assert!(serde_json::from_str::<KeyGroupEdit>(r#"{"kind":"shiftTime","delta":2}"#).is_ok());
        assert!(
            serde_json::from_str::<KeyGroupEdit>(r#"{"kind":"shiftValue","delta":2}"#).is_err()
        );
    }
    #[test]
    fn property_group_rejections_do_not_partially_edit_any_channel() {
        let (mut d, id, selection) = group_fixture();
        let before = encoded(&d);
        let revision = d.revision();
        let undo = d.undo_order.len();
        for delta in [15, -6, 18000] {
            assert!(d
                .edit_animation_key_group(&id, &selection, KeyGroupEdit::ShiftTime { delta })
                .is_err());
            assert_eq!(encoded(&d), before);
        }
        for invalid_selection in [
            vec![],
            vec![selection[0].clone(), selection[0].clone()],
            vec![
                selection[0].clone(),
                PropertyKeys {
                    property: Property::X,
                    frames: vec![99],
                },
            ],
            vec![PropertyKeys {
                property: Property::X,
                frames: vec![],
            }],
            vec![PropertyKeys {
                property: Property::X,
                frames: vec![15, 15],
            }],
            vec![PropertyKeys {
                property: Property::X,
                frames: vec![15; 4097],
            }],
        ] {
            assert!(d
                .edit_animation_key_group(&id, &invalid_selection, KeyGroupEdit::Delete)
                .is_err());
            assert_eq!(encoded(&d), before);
        }
        assert_eq!(d.revision(), revision);
        assert_eq!(d.undo_order.len(), undo);
        d.locked_artwork_layers.insert(id.clone());
        let before = encoded(&d);
        assert!(d
            .edit_animation_key_group(&id, &selection, KeyGroupEdit::Delete)
            .is_err());
        assert_eq!(encoded(&d), before);
    }
    #[test]
    fn cross_layer_keys_commit_once_and_reject_partial_changes() {
        let (mut d, id, selection) = group_fixture();
        let other = d.add_vector_layer().unwrap();
        for frame in [0, 20] {
            d.set_animation_key(
                &other,
                Property::X,
                Keyframe {
                    frame,
                    value: frame as f32,
                    interpolation: Interpolation::Cubic,
                    curve: Some(KeyCurve::default()),
                },
            )
            .unwrap();
        }
        let tracks = vec![
            LayerPropertyKeys {
                layer: id.clone(),
                selections: selection,
            },
            LayerPropertyKeys {
                layer: other.clone(),
                selections: vec![PropertyKeys {
                    property: Property::X,
                    frames: vec![0],
                }],
            },
        ];
        let before = encoded(&d);
        let revision = d.revision();
        let undo = d.undo_order.len();
        assert!(d
            .edit_animation_key_tracks(&tracks, KeyGroupEdit::ShiftTime { delta: 20 })
            .is_err());
        assert_eq!(encoded(&d), before);
        assert_eq!(d.revision(), revision);
        assert!(d
            .edit_animation_key_tracks(
                &[tracks[0].clone(), tracks[0].clone()],
                KeyGroupEdit::Delete
            )
            .is_err());
        assert_eq!(encoded(&d), before);
        d.locked_artwork_layers.insert(other.clone());
        let locked = encoded(&d);
        assert!(d
            .edit_animation_key_tracks(&tracks, KeyGroupEdit::Delete)
            .is_err());
        assert_eq!(encoded(&d), locked);
        d.locked_artwork_layers.remove(&other);
        d.edit_animation_key_tracks(&tracks, KeyGroupEdit::ShiftTime { delta: 3 })
            .unwrap();
        assert_eq!(d.undo_order.len(), undo + 1);
        assert_eq!(
            d.animation.tracks[&id].channels[&Property::Opacity][0].frame,
            8
        );
        assert_eq!(
            d.animation.tracks[&other].channels[&Property::X][0].frame,
            3
        );
        assert_eq!(
            d.animation.tracks[&other].channels[&Property::X][0].curve,
            Some(KeyCurve::default())
        );
        let after = encoded(&d);
        d.undo();
        assert_eq!(encoded(&d), before);
        d.redo();
        assert_eq!(encoded(&d), after);
        let moved = tracks
            .iter()
            .map(|t| LayerPropertyKeys {
                layer: t.layer.clone(),
                selections: t
                    .selections
                    .iter()
                    .map(|s| PropertyKeys {
                        property: s.property,
                        frames: s.frames.iter().map(|f| f + 3).collect(),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let rev = d.revision();
        d.edit_animation_key_tracks(&moved, KeyGroupEdit::ShiftTime { delta: 0 })
            .unwrap();
        assert_eq!(d.revision(), rev);
        d.edit_animation_key_tracks(
            &moved,
            KeyGroupEdit::Interpolation {
                interpolation: Interpolation::Hold,
            },
        )
        .unwrap();
        let held = encoded(&d);
        d.edit_animation_key_tracks(&moved, KeyGroupEdit::Delete)
            .unwrap();
        d.undo();
        assert_eq!(encoded(&d), held);
        let saved = serde_json::to_string(&d.animation).unwrap();
        let restored: Animation = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.tracks[&other].channels[&Property::X][0].frame, 3);
    }
    #[test]
    fn unit_offsets_edit_all_layers_once_and_preserve_timing_and_curves() {
        let mut d = Document::default();
        let mut tracks = Vec::new();
        for _ in 0..2 {
            let layer = d.add_vector_layer().unwrap();
            let mut selections = Vec::new();
            for (property, value) in [
                (Property::X, 1.),
                (Property::Y, 20.),
                (Property::Rotation, 90.),
                (Property::ScaleX, 100.),
                (Property::ScaleY, 50.),
                (Property::Opacity, 40.),
            ] {
                d.set_animation_key(
                    &layer,
                    property,
                    Keyframe {
                        frame: 5,
                        value,
                        interpolation: Interpolation::Cubic,
                        curve: Some(KeyCurve::default()),
                    },
                )
                .unwrap();
                selections.push(PropertyKeys {
                    property,
                    frames: vec![5],
                });
            }
            tracks.push(LayerPropertyKeys { layer, selections });
        }
        let before = encoded(&d);
        let undo = d.undo_order.len();
        d.edit_animation_key_tracks(
            &tracks,
            KeyGroupEdit::ShiftValues {
                position: 5.,
                rotation: -15.,
                scale: 10.,
                opacity: -5.,
            },
        )
        .unwrap();
        assert_eq!(d.undo_order.len(), undo + 1);
        for track in &tracks {
            for (p, v) in [
                (Property::X, 6.),
                (Property::Y, 25.),
                (Property::Rotation, 75.),
                (Property::ScaleX, 110.),
                (Property::ScaleY, 60.),
                (Property::Opacity, 35.),
            ] {
                let k = &d.animation.tracks[&track.layer].channels[&p][0];
                assert_eq!((k.frame, k.value), (5, v));
                assert_eq!(k.interpolation, Interpolation::Cubic);
                assert_eq!(k.curve, Some(KeyCurve::default()));
            }
        }
        let after = encoded(&d);
        d.undo();
        assert_eq!(encoded(&d), before);
        d.redo();
        assert_eq!(encoded(&d), after);
        let rev = d.revision();
        d.edit_animation_key_tracks(
            &tracks,
            KeyGroupEdit::ShiftValues {
                position: 0.,
                rotation: 0.,
                scale: 0.,
                opacity: 0.,
            },
        )
        .unwrap();
        assert_eq!(d.revision(), rev);
        let saved = serde_json::to_string(&d.animation).unwrap();
        let restored: Animation = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            restored.tracks[&tracks[1].layer].channels[&Property::Opacity][0].value,
            35.
        );
        for edit in [
            KeyGroupEdit::ShiftValues {
                position: 1.,
                rotation: 0.,
                scale: 0.,
                opacity: 66.,
            },
            KeyGroupEdit::ShiftValues {
                position: 1.,
                rotation: 0.,
                scale: -60.,
                opacity: 0.,
            },
            KeyGroupEdit::ShiftValues {
                position: 1.,
                rotation: 36000.,
                scale: 0.,
                opacity: 0.,
            },
            KeyGroupEdit::ShiftValues {
                position: f32::INFINITY,
                rotation: 0.,
                scale: 0.,
                opacity: 0.,
            },
            KeyGroupEdit::ShiftValues {
                position: 0.,
                rotation: f32::NAN,
                scale: 0.,
                opacity: 0.,
            },
        ] {
            assert!(d.edit_animation_key_tracks(&tracks, edit).is_err());
            assert_eq!(encoded(&d), after);
            assert_eq!(d.revision(), rev);
        }
        assert!(serde_json::from_str::<KeyGroupEdit>(
            r#"{"kind":"shiftValues","position":1,"rotation":2,"scale":3,"opacity":4}"#
        )
        .is_ok());
        assert!(
            serde_json::from_str::<KeyGroupEdit>(r#"{"kind":"shiftValues","position":1}"#).is_err()
        );
    }
}
