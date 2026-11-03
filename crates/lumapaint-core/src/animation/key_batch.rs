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
    OvershootLinkedCurve {
        curve: KeyCurve,
        control: u8,
    },
    LinkedCurve {
        curve: KeyCurve,
        control: u8,
    },
    LinkedVelocity {
        control: u8,
        value: f64,
        overshoot: bool,
    },
    OvershootVelocity {
        control: u8,
        value: f64,
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
    if i + 1 >= keys.len() || keys[i].interpolation != Interpolation::Cubic || curve.overshoots() {
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
        if other.overshoots() {
            return Err(invalid());
        }
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

// Signed linking in document units. Both control polygons remain inside the
// property's physical limits, including the continuous values between frames.
fn link_overshoot_curve(
    keys: &mut [Keyframe],
    i: usize,
    curve: KeyCurve,
    control: u8,
    property: Property,
) -> Result<(), String> {
    link_constrained_curve(keys, i, curve, control, property, true, false)
}
fn link_constrained_curve(
    keys: &mut [Keyframe],
    i: usize,
    mut curve: KeyCurve,
    control: u8,
    property: Property,
    overshoot: bool,
    preserve_time: bool,
) -> Result<(), String> {
    if i + 1 >= keys.len()
        || keys[i].interpolation != Interpolation::Cubic
        || !curve.valid()
        || !curve.output_valid(keys[i].value, keys[i + 1].value, property)
    {
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
        if !other.valid()
            || (!overshoot && other.overshoots())
            || !other.output_valid(keys[j].value, keys[j + 1].value, property)
        {
            return Err(invalid());
        }
        let dt = f64::from(keys[i + 1].frame - keys[i].frame);
        let dv = f64::from(keys[i + 1].value) - f64::from(keys[i].value);
        let nt = f64::from(keys[j + 1].frame - keys[j].frame);
        let nv = f64::from(keys[j + 1].value) - f64::from(keys[j].value);
        let minimum = 0.0001_f32;
        if !preserve_time && control == 1 {
            curve.x1 = curve.x1.max(minimum);
            curve.x2 = curve.x2.max(curve.x1);
        } else if !preserve_time {
            curve.x2 = 1. - (1. - curve.x2).max(minimum);
            curve.x1 = curve.x1.min(curve.x2);
        }
        let sx = f64::from(if control == 1 {
            curve.x1
        } else {
            1. - curve.x2
        });
        let sy = f64::from(if control == 1 {
            curve.y1
        } else {
            1. - curve.y2
        });
        if sx <= 0. {
            return Err(invalid());
        }
        if preserve_time {
            let (ox, oy) = if control == 1 {
                (1. - other.x2, 1. - other.y2)
            } else {
                (other.x1, other.y1)
            };
            let desired = dv * sy / (dt * sx);
            let existing = nv * f64::from(oy) / (nt * f64::from(ox));
            let (stored_x, stored_y) = if control == 1 {
                (other.x2, other.y2)
            } else {
                (other.x1, other.y1)
            };
            let precision = (nv / nt).abs()
                * f64::from(f32::EPSILON)
                * (f64::from(stored_y.abs()) / f64::from(ox)
                    + f64::from(oy.abs()) * f64::from(stored_x.abs()) / f64::from(ox).powi(2));
            if ox > 0.
                && (desired - existing).abs()
                    <= 1e-6 * desired.abs().max(existing.abs()).max(1e-6) + 2. * precision
            {
                keys[i].curve = Some(curve);
                return Ok(());
            }
        }
        let (lower, upper) = if overshoot {
            overshoot_handle_bounds(keys[j].value, keys[j + 1].value, property)
        } else {
            (0., 1.)
        };
        // The incoming side stores 1-x / 1-y, so reserve its actual f32 width.
        let min_x = f64::from(if control == 1 {
            1. - (1. - minimum)
        } else {
            minimum
        });
        let (ny_low, ny_high) = if control == 1 {
            (1. - upper, 1. - lower)
        } else {
            (lower, upper)
        };
        let requested = if dv == 0. || nv == 0. {
            0.
        } else {
            dv * sy / (dt * sx)
        };
        let r0 = nv * ny_low / (nt * min_x);
        let r1 = nv * ny_high / (nt * min_x);
        // Keep a small inward margin for rounding the time handle and y value.
        let slope = requested.clamp(r0.min(r1) * (1. - 1e-5), r0.max(r1) * (1. - 1e-5));
        let source_y = if dv == 0. { 0. } else { slope * dt * sx / dv };
        if !preserve_time || slope != requested || dv == 0. || nv == 0. {
            if control == 1 {
                curve.y1 = source_y as f32;
            } else {
                curve.y2 = (1. - source_y) as f32;
            }
        }
        let actual_y = f64::from(if control == 1 {
            curve.y1
        } else {
            1. - curve.y2
        });
        let actual_slope = if dv == 0. {
            0.
        } else {
            dv * actual_y / (dt * sx)
        };
        let ratio = if nv == 0. { 0. } else { actual_slope * nt / nv };
        let capacity = if ratio < 0. { -ny_low } else { ny_high };
        let wanted = f64::from(if control == 1 {
            (1. - other.x2).max(minimum)
        } else {
            other.x1.max(minimum)
        });
        let nx = wanted
            .min(if ratio == 0. {
                1.
            } else {
                capacity / ratio.abs()
            })
            .max(min_x) as f32;
        if control == 1 {
            other.x2 = 1. - nx;
            other.x1 = other.x1.min(other.x2);
        } else {
            other.x1 = nx;
            other.x2 = other.x2.max(other.x1);
        }
        let actual_x = f64::from(if control == 1 {
            1. - other.x2
        } else {
            other.x1
        });
        let ny = (ratio * actual_x).clamp(ny_low, ny_high);
        if control == 1 {
            other.y2 = (1. - ny) as f32;
        } else {
            other.y1 = ny as f32;
        }
        // A near-horizontal incoming handle stores 1-y in f32. Refine its
        // time width after rounding y so the requested velocity is preserved.
        if preserve_time && ratio != 0. {
            let encoded_y = f64::from(if control == 1 {
                1. - other.y2
            } else {
                other.y1
            });
            let width = encoded_y / ratio;
            if (min_x..=1.).contains(&width) {
                if control == 1 {
                    other.x2 = 1. - width as f32;
                    other.x1 = other.x1.min(other.x2);
                } else {
                    other.x1 = width as f32;
                    other.x2 = other.x2.max(other.x1);
                }
            }
        }
        if actual_x <= 0.
            || !curve.valid()
            || !other.valid()
            || !curve.output_valid(keys[i].value, keys[i + 1].value, property)
            || !other.output_valid(keys[j].value, keys[j + 1].value, property)
        {
            return Err(invalid());
        }
        keys[j].curve = Some(other);
    }
    keys[i].curve = Some(curve);
    Ok(())
}
fn overshoot_handle_bounds(a: f32, b: f32, property: Property) -> (f64, f64) {
    if a == b {
        return (0., 1.);
    }
    let (min, max) = property.limits();
    let dv = f64::from(b) - f64::from(a);
    let p = (f64::from(min) - f64::from(a)) / dv;
    let q = (f64::from(max) - f64::from(a)) / dv;
    let low = p.min(q).max(-2.);
    let high = p.max(q).min(3.);
    (
        if low > -2. {
            (low + 1e-6 * low.abs().max(1.)).min(0.)
        } else {
            low
        },
        if high < 3. {
            (high - 1e-6 * high.abs().max(1.)).max(1.)
        } else {
            high
        },
    )
}

// Edit one side of an interval in units/second, preserving its time handles.
fn velocity_curve(
    a: &Keyframe,
    b: &Keyframe,
    fps: u32,
    control: u8,
    value: f64,
) -> Result<KeyCurve, String> {
    velocity_curve_with_limits(a, b, fps, control, value, None)
}
fn velocity_curve_with_limits(
    a: &Keyframe,
    b: &Keyframe,
    fps: u32,
    control: u8,
    value: f64,
    property: Option<Property>,
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
    if !c.valid() || (property.is_none() && c.overshoots()) || x <= 0. || rate == 0. {
        return Err(invalid());
    }
    let (low, high) = if let Some(property) = property {
        let (min, max) = property.limits();
        let dv = f64::from(b.value) - f64::from(a.value);
        let lo = (f64::from(min) - f64::from(a.value)) / dv;
        let hi = (f64::from(max) - f64::from(a.value)) / dv;
        (lo.min(hi).max(-2.), lo.max(hi).min(3.))
    } else {
        (0., 1.)
    };
    let speeds = if control == 1 {
        (rate * low / x, rate * high / x)
    } else {
        (rate * (1. - high) / x, rate * (1. - low) / x)
    };
    let current =
        rate * if control == 1 {
            f64::from(c.y1)
        } else {
            1. - f64::from(c.y2)
        } / x;
    if value == current && property.is_none_or(|p| c.output_valid(a.value, b.value, p)) {
        return Ok(c);
    }
    if value < speeds.0.min(speeds.1) || value > speeds.0.max(speeds.1) {
        return Err(invalid());
    }
    let y = if control == 1 {
        value / rate * x
    } else {
        1. - value / rate * x
    };
    if control == 1 {
        c.y1 = y.clamp(low, high) as f32;
    } else {
        c.y2 = y.clamp(low, high) as f32;
    }
    if !c.valid() || property.is_some_and(|p| !c.output_valid(a.value, b.value, p)) {
        return Err(invalid());
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
    if let KeyBatchEdit::LinkedCurve { curve, control }
    | KeyBatchEdit::OvershootLinkedCurve { curve, control } = edit
    {
        if frames.len() != 1 || !curve.valid() || !(1..=2).contains(&control) {
            return Err(invalid());
        }
        let index = keys
            .iter()
            .position(|k| k.frame == frames[0])
            .ok_or_else(invalid)?;
        if matches!(edit, KeyBatchEdit::OvershootLinkedCurve { .. }) {
            link_overshoot_curve(keys, index, curve, control, property)?;
        } else {
            link_curve(keys, index, curve, control)?;
        }
    }
    if let KeyBatchEdit::Velocity { control, value }
    | KeyBatchEdit::OvershootVelocity { control, value }
    | KeyBatchEdit::LinkedVelocity { control, value, .. } = edit
    {
        if frames.len() != 1 {
            return Err(invalid());
        }
        let i = keys
            .iter()
            .position(|k| k.frame == frames[0])
            .ok_or_else(invalid)?;
        let b = keys.get(i + 1).ok_or_else(invalid)?;
        let curve = if matches!(
            edit,
            KeyBatchEdit::OvershootVelocity { .. }
                | KeyBatchEdit::LinkedVelocity {
                    overshoot: true,
                    ..
                }
        ) {
            velocity_curve_with_limits(&keys[i], b, fps, control, value, Some(property))?
        } else {
            velocity_curve(&keys[i], b, fps, control, value)?
        };
        keys[i].interpolation = Interpolation::Cubic;
        keys[i].curve = Some(curve);
        if let KeyBatchEdit::LinkedVelocity { overshoot, .. } = edit {
            link_constrained_curve(keys, i, curve, control, property, overshoot, true)?;
        }
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
            | KeyBatchEdit::OvershootLinkedCurve { .. }
            | KeyBatchEdit::Velocity { .. }
            | KeyBatchEdit::OvershootVelocity { .. }
            | KeyBatchEdit::LinkedVelocity { .. } => (),
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
    fn linked_velocity_refines_near_horizontal_incoming_precision() {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        Arc::make_mut(&mut d.animation).fps = 12;
        Arc::make_mut(&mut d.animation).duration = 1200;
        for (frame, value) in [(0, 6691.2305), (1, 7432.3213), (1000, 4323.6895)] {
            d.set_animation_key(
                &id,
                Property::ScaleX,
                Keyframe {
                    frame,
                    value,
                    interpolation: Interpolation::Cubic,
                    curve: Some(KeyCurve {
                        x1: 0.25,
                        y1: 0.,
                        x2: 0.75,
                        y2: 1.,
                    }),
                },
            )
            .unwrap();
        }
        let edit = KeyBatchEdit::LinkedVelocity {
            control: 1,
            value: -0.25648759293983403,
            overshoot: true,
        };
        d.edit_animation_keys(&id, Property::ScaleX, &[1], edit.clone())
            .unwrap();
        let keys = &d.animation.tracks[&id].channels[&Property::ScaleX];
        assert!((slope(keys, 0, 2) - slope(keys, 1, 1)).abs() * 12. < 0.00001);
        let revision = d.revision();
        d.edit_animation_keys(&id, Property::ScaleX, &[1], edit)
            .unwrap();
        assert_eq!(d.revision(), revision);
    }
    #[test]
    fn linked_velocity_preserves_source_time_and_commits_both_curves_once() {
        for fps in [12, 24, 60] {
            for overshoot in [false, true] {
                for interpolation in [
                    Interpolation::Cubic,
                    Interpolation::Linear,
                    Interpolation::Ease,
                ] {
                    for (i, control) in [(0, 2), (1, 1)] {
                        let mut d = Document::default();
                        let id = d.add_vector_layer().unwrap();
                        Arc::make_mut(&mut d.animation).fps = fps;
                        let base = KeyCurve {
                            x1: 0.25,
                            y1: 0.,
                            x2: 0.75,
                            y2: 1.,
                        };
                        for (j, (frame, value)) in
                            [(5, 20.), (15, 60.), (45, if overshoot { 20. } else { 80. })]
                                .into_iter()
                                .enumerate()
                        {
                            d.set_animation_key(
                                &id,
                                Property::Opacity,
                                Keyframe {
                                    frame,
                                    value,
                                    interpolation: if j == i {
                                        interpolation
                                    } else {
                                        Interpolation::Cubic
                                    },
                                    curve: Some(base),
                                },
                            )
                            .unwrap();
                        }
                        let before = encoded(&d);
                        let revision = d.revision();
                        let source =
                            d.animation.tracks[&id].channels[&Property::Opacity][i].clone();
                        let expected = velocity_curve_with_limits(
                            &source,
                            &d.animation.tracks[&id].channels[&Property::Opacity][i + 1],
                            fps,
                            control,
                            f64::from(fps),
                            overshoot.then_some(Property::Opacity),
                        )
                        .unwrap();
                        let edit = KeyBatchEdit::LinkedVelocity {
                            control,
                            value: f64::from(fps),
                            overshoot,
                        };
                        d.edit_animation_keys(
                            &id,
                            Property::Opacity,
                            &[source.frame],
                            edit.clone(),
                        )
                        .unwrap();
                        let keys = &d.animation.tracks[&id].channels[&Property::Opacity];
                        let c = keys[i].curve.unwrap();
                        assert_eq!((c.x1, c.x2), (expected.x1, expected.x2));
                        assert_eq!(
                            if control == 1 { c.y2 } else { c.y1 },
                            if control == 1 {
                                expected.y2
                            } else {
                                expected.y1
                            }
                        );
                        assert!((slope(keys, 0, 2) - slope(keys, 1, 1)).abs() < 0.0001);
                        assert!(slope(keys, i, control) > 0.);
                        for j in 0..2 {
                            assert!(keys[j].curve.unwrap().output_valid(
                                keys[j].value,
                                keys[j + 1].value,
                                Property::Opacity
                            ));
                        }
                        assert_eq!(d.revision(), revision + 1);
                        let after = encoded(&d);
                        d.undo();
                        assert_eq!(encoded(&d), before);
                        d.redo();
                        assert_eq!(encoded(&d), after);
                        let revision = d.revision();
                        d.edit_animation_keys(&id, Property::Opacity, &[source.frame], edit)
                            .unwrap();
                        assert_eq!(d.revision(), revision);
                    }
                }
            }
        }
    }
    #[test]
    fn linked_velocity_preserves_tiny_source_handles_and_rejects_atomically() {
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
        let c = KeyCurve {
            x1: 0.00001,
            y1: 0.,
            x2: 0.75,
            y2: 1.,
        };
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[15],
            KeyBatchEdit::Curve { curve: c },
        )
        .unwrap();
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[15],
            KeyBatchEdit::LinkedVelocity {
                control: 1,
                value: 240.,
                overshoot: false,
            },
        )
        .unwrap();
        let keys = &d.animation.tracks[&id].channels[&Property::Opacity];
        assert_eq!(keys[1].curve.unwrap().x1, c.x1);
        assert!((slope(keys, 0, 2) - slope(keys, 1, 1)).abs() < 0.001);
        let before = encoded(&d);
        let revision = d.revision();
        for (frames, control, value) in [
            (vec![5, 15], 1, 10.),
            (vec![5], 0, 10.),
            (vec![5], 1, f64::NAN),
            (vec![5], 1, -100.),
            (vec![5], 2, 1e20),
            (vec![30], 1, 10.),
        ] {
            assert!(d
                .edit_animation_keys(
                    &id,
                    Property::Opacity,
                    &frames,
                    KeyBatchEdit::LinkedVelocity {
                        control,
                        value,
                        overshoot: false
                    }
                )
                .is_err());
            assert_eq!(encoded(&d), before);
            assert_eq!(d.revision(), revision);
        }
    }
    #[test]
    fn linked_velocity_limits_flat_extrema_and_preserves_non_custom_neighbors() {
        let base = KeyCurve {
            x1: 0.25,
            y1: 0.,
            x2: 0.75,
            y2: 1.,
        };
        for (end, neighbor) in [
            (20., Interpolation::Cubic),
            (100., Interpolation::Cubic),
            (20., Interpolation::Hold),
            (20., Interpolation::Linear),
            (20., Interpolation::Ease),
        ] {
            let mut d = Document::default();
            let id = d.add_vector_layer().unwrap();
            for (frame, value, interpolation) in [
                (5, 20., Interpolation::Cubic),
                (15, 100., neighbor),
                (45, end, Interpolation::Linear),
            ] {
                d.set_animation_key(
                    &id,
                    Property::Opacity,
                    Keyframe {
                        frame,
                        value,
                        interpolation,
                        curve: Some(base),
                    },
                )
                .unwrap();
            }
            d.edit_animation_keys(
                &id,
                Property::Opacity,
                &[5],
                KeyBatchEdit::LinkedVelocity {
                    control: 2,
                    value: 120.,
                    overshoot: true,
                },
            )
            .unwrap();
            let keys = &d.animation.tracks[&id].channels[&Property::Opacity];
            if neighbor == Interpolation::Cubic {
                assert_eq!(slope(keys, 0, 2), 0.);
                assert_eq!(slope(keys, 1, 1), 0.);
            } else {
                assert!((slope(keys, 0, 2) * 24. - 120.).abs() < 0.001);
                assert_eq!(keys[1].curve, Some(base));
                assert_eq!(keys[1].interpolation, neighbor);
            }
        }
    }
    #[test]
    fn overshoot_link_reserves_f32_space_near_physical_endpoints() {
        let base = KeyCurve {
            x1: 0.25,
            y1: 0.,
            x2: 0.75,
            y2: 1.,
        };
        let mut keys = vec![
            Keyframe {
                frame: 0,
                value: 69.888_63,
                interpolation: Interpolation::Cubic,
                curve: Some(base),
            },
            Keyframe {
                frame: 100,
                value: 0.452_228_22,
                interpolation: Interpolation::Cubic,
                curve: Some(base),
            },
            Keyframe {
                frame: 1000,
                value: 68.778_79,
                interpolation: Interpolation::Linear,
                curve: None,
            },
        ];
        link_overshoot_curve(
            &mut keys,
            1,
            KeyCurve {
                y1: 0.823_857_4,
                ..base
            },
            1,
            Property::Opacity,
        )
        .unwrap();
        assert!(keys[0].curve.unwrap().output_valid(
            keys[0].value,
            keys[1].value,
            Property::Opacity
        ));
        assert!((slope(&keys, 0, 2) - slope(&keys, 1, 1)).abs() < 0.0001);
    }
    #[test]
    fn overshoot_links_signed_slopes_history_and_limits() {
        for property in [Property::X, Property::Opacity, Property::ScaleX] {
            for values in [
                [20., 60., 20.],
                [60., 20., 60.],
                [20., 60., 60.],
                [20., 100., 20.],
            ] {
                for (index, control) in [(0, 2), (1, 1)] {
                    for y in [-0.5, 0.5, 1.5] {
                        let mut d = Document::default();
                        let id = d.add_vector_layer().unwrap();
                        let base = KeyCurve {
                            x1: 0.25,
                            y1: 0.,
                            x2: 0.75,
                            y2: 1.,
                        };
                        for (frame, value) in [5, 15, 45].into_iter().zip(values) {
                            d.set_animation_key(
                                &id,
                                property,
                                Keyframe {
                                    frame,
                                    value,
                                    interpolation: Interpolation::Cubic,
                                    curve: Some(base),
                                },
                            )
                            .unwrap();
                        }
                        let mut curve = base;
                        if control == 1 {
                            curve.y1 = y;
                        } else {
                            curve.y2 = y;
                        }
                        // Invalid physical control points must leave both intervals untouched.
                        let before = encoded(&d);
                        let revision = d.revision();
                        let edit = KeyBatchEdit::OvershootLinkedCurve { curve, control };
                        let result =
                            d.edit_animation_keys(&id, property, &[[5, 15][index]], edit.clone());
                        if !curve.output_valid(values[index], values[index + 1], property) {
                            assert!(result.is_err());
                            assert_eq!(encoded(&d), before);
                            assert_eq!(d.revision(), revision);
                            continue;
                        }
                        result.unwrap();
                        let keys = &d.animation.tracks[&id].channels[&property];
                        let a = slope(keys, 0, 2);
                        let b = slope(keys, 1, 1);
                        assert!(
                            (a - b).abs() <= 0.0001 * a.abs().max(1.),
                            "{property:?} {values:?} {control} {y}: {a} != {b}"
                        );
                        assert_eq!(
                            keys.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>(),
                            vec![(5, values[0]), (15, values[1]), (45, values[2])]
                        );
                        for i in 0..2 {
                            assert!(keys[i].curve.unwrap().output_valid(
                                keys[i].value,
                                keys[i + 1].value,
                                property
                            ));
                        }
                        let after = encoded(&d);
                        if after == before {
                            assert_eq!(d.revision(), revision);
                            continue;
                        }
                        assert_eq!(d.revision(), revision + 1);
                        d.undo();
                        assert_eq!(encoded(&d), before);
                        d.redo();
                        assert_eq!(encoded(&d), after);
                        let saved = serde_json::to_string(&d.animation).unwrap();
                        let restored: Animation = serde_json::from_str(&saved).unwrap();
                        assert_eq!(serde_json::to_string(&restored).unwrap(), saved);
                        let revision = d.revision();
                        d.edit_animation_keys(&id, property, &[[5, 15][index]], edit)
                            .unwrap();
                        assert_eq!(d.revision(), revision);
                    }
                }
            }
        }
    }
    #[test]
    fn overshoot_link_handles_vertical_flat_and_non_custom_neighbors() {
        let base = KeyCurve {
            x1: 0.25,
            y1: 0.,
            x2: 0.75,
            y2: 1.,
        };
        for control in [1, 2] {
            for end in [20., 60.] {
                let mut keys = vec![
                    Keyframe {
                        frame: 5,
                        value: 20.,
                        interpolation: Interpolation::Cubic,
                        curve: Some(base),
                    },
                    Keyframe {
                        frame: 15,
                        value: 60.,
                        interpolation: Interpolation::Cubic,
                        curve: Some(base),
                    },
                    Keyframe {
                        frame: 45,
                        value: end,
                        interpolation: Interpolation::Linear,
                        curve: None,
                    },
                ];
                let i = if control == 1 { 1 } else { 0 };
                let mut curve = base;
                if control == 1 {
                    curve.x1 = 0.;
                    curve.y1 = -0.5;
                } else {
                    curve.x2 = 1.;
                    curve.y2 = 0.5;
                }
                link_overshoot_curve(&mut keys, i, curve, control, Property::Opacity).unwrap();
                assert!((slope(&keys, 0, 2) - slope(&keys, 1, 1)).abs() < 0.01);
                if end == 60. {
                    assert_eq!(slope(&keys, 0, 2), 0.);
                }
                for k in keys.iter().take(2) {
                    assert!(k.curve.unwrap().valid());
                }
                keys[if control == 1 { 0 } else { 1 }].interpolation = Interpolation::Hold;
                link_overshoot_curve(&mut keys, i, base, control, Property::Opacity).unwrap();
                assert_eq!(keys[i].curve, Some(base));
            }
        }
    }
    #[test]
    fn overshoot_velocity_edits_preserve_handles_history_and_output_limits() {
        for fps in [12, 24, 60] {
            for property in [
                Property::X,
                Property::Opacity,
                Property::ScaleX,
                Property::Rotation,
            ] {
                for reverse in [false, true] {
                    let mut d = Document::default();
                    let id = d.add_vector_layer().unwrap();
                    Arc::make_mut(&mut d.animation).fps = fps;
                    let curve = KeyCurve {
                        x1: 0.25,
                        y1: 1.5,
                        x2: 0.75,
                        y2: 1.5,
                    };
                    let values = if reverse { [40., 20.] } else { [20., 40.] };
                    for (frame, value) in [(5, values[0]), (25, values[1])] {
                        d.set_animation_key(
                            &id,
                            property,
                            Keyframe {
                                frame,
                                value,
                                interpolation: Interpolation::Cubic,
                                curve: Some(curve),
                            },
                        )
                        .unwrap();
                    }
                    let rate = if reverse { -(fps as f64) } else { fps as f64 };
                    for control in [1, 2] {
                        let before = serde_json::to_string(&d.document_state()).unwrap();
                        let revision = d.revision();
                        d.edit_animation_keys(
                            &id,
                            property,
                            &[5],
                            KeyBatchEdit::OvershootVelocity {
                                control,
                                value: -rate,
                            },
                        )
                        .unwrap();
                        let c = d.animation.tracks[&id].channels[&property][0]
                            .curve
                            .unwrap();
                        assert_eq!((c.x1, c.x2), (curve.x1, curve.x2));
                        if control == 1 {
                            assert_eq!(c.y1, -0.25);
                            assert_eq!(c.y2, curve.y2);
                        } else {
                            assert_eq!(c.y1, curve.y1);
                            assert_eq!(c.y2, 1.25);
                        }
                        assert!(c.output_valid(values[0], values[1], property));
                        assert_eq!(d.revision(), revision + 1);
                        let saved = serde_json::to_string(&d.document_state()).unwrap();
                        d.undo();
                        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), before);
                        d.redo();
                        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
                        let restored =
                            Document::from_document_state(serde_json::from_str(&saved).unwrap())
                                .unwrap();
                        assert_eq!(
                            restored.animation.tracks[&id].channels[&property][0].curve,
                            Some(c)
                        );
                        let revision = d.revision();
                        d.edit_animation_keys(
                            &id,
                            property,
                            &[5],
                            KeyBatchEdit::OvershootVelocity {
                                control,
                                value: -rate,
                            },
                        )
                        .unwrap();
                        assert_eq!(d.revision(), revision);
                        for (frames, control, value) in [
                            (vec![5], 1, rate * 1e5),
                            (vec![5], 2, f64::NAN),
                            (vec![5], 3, 0.),
                            (vec![25], 1, 0.),
                            (vec![5, 25], 1, 0.),
                        ] {
                            assert!(d
                                .edit_animation_keys(
                                    &id,
                                    property,
                                    &frames,
                                    KeyBatchEdit::OvershootVelocity { control, value }
                                )
                                .is_err());
                            assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
                        }
                        d.undo();
                    }
                }
            }
        }
        assert!(serde_json::from_str::<KeyBatchEdit>(
            r#"{"kind":"overshootVelocity","control":1,"value":-24}"#
        )
        .is_ok());
        assert!(serde_json::from_str::<KeyBatchEdit>(
            r#"{"kind":"overshootVelocity","control":1}"#
        )
        .is_err());
    }

    #[test]
    fn overshoot_curve_history_output_guards_and_saved_evaluation() {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        for property in [
            Property::X,
            Property::Opacity,
            Property::ScaleX,
            Property::Rotation,
        ] {
            for (frame, value) in [(0, 20.), (20, 40.)] {
                d.set_animation_key(
                    &id,
                    property,
                    Keyframe {
                        frame,
                        value,
                        interpolation: Interpolation::Linear,
                        curve: None,
                    },
                )
                .unwrap();
            }
        }
        let before = serde_json::to_string(&d.document_state()).unwrap();
        let curve = KeyCurve {
            y1: 2.,
            y2: 2.,
            ..KeyCurve::default()
        };
        let revision = d.revision();
        d.edit_animation_keys(&id, Property::Opacity, &[0], KeyBatchEdit::Curve { curve })
            .unwrap();
        assert!(d.animation.tracks[&id].value(Property::Opacity, 10) > 40.);
        assert_eq!(d.revision(), revision + 1);
        let saved = serde_json::to_string(&d.document_state()).unwrap();
        d.undo();
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), before);
        d.redo();
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
        let restored =
            Document::from_document_state(serde_json::from_str(&saved).unwrap()).unwrap();
        assert_eq!(
            restored.animation().tracks[&id].value(Property::Opacity, 10),
            d.animation().tracks[&id].value(Property::Opacity, 10)
        );
        let revision = d.revision();
        d.edit_animation_keys(&id, Property::Opacity, &[0], KeyBatchEdit::Curve { curve })
            .unwrap();
        assert_eq!(d.revision(), revision);
        for edit in [
            KeyBatchEdit::ShiftValue { delta: 50. },
            KeyBatchEdit::Move {
                frame_delta: 0,
                value_delta: 50.,
            },
            KeyBatchEdit::Velocity {
                control: 1,
                value: 20.,
            },
            KeyBatchEdit::LinkedCurve { control: 1, curve },
        ] {
            assert!(d
                .edit_animation_keys(
                    &id,
                    Property::Opacity,
                    &[0, 20][..if matches!(
                        edit,
                        KeyBatchEdit::ShiftValue { .. } | KeyBatchEdit::Move { .. }
                    ) {
                        2
                    } else {
                        1
                    }],
                    edit
                )
                .is_err());
            assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
        }
        let mut invalid = d.document_state();
        invalid
            .animation
            .tracks
            .get_mut(&id)
            .unwrap()
            .channels
            .get_mut(&Property::Opacity)
            .unwrap()[0]
            .curve = Some(KeyCurve {
            y1: 3.,
            y2: 3.,
            ..curve
        });
        invalid
            .animation
            .tracks
            .get_mut(&id)
            .unwrap()
            .channels
            .get_mut(&Property::Opacity)
            .unwrap()[1]
            .value = 100.;
        assert!(Document::from_document_state(invalid).is_err());
        for property in [Property::X, Property::ScaleX, Property::Rotation] {
            d.edit_animation_keys(&id, property, &[0], KeyBatchEdit::Curve { curve })
                .unwrap();
            assert!(d.animation.tracks[&id].value(property, 10) > 40.);
        }
        let curve = KeyCurve {
            y1: -1.,
            y2: -1.,
            ..curve
        };
        d.edit_animation_keys(&id, Property::Opacity, &[0], KeyBatchEdit::Curve { curve })
            .unwrap();
        assert!(d.animation.tracks[&id].value(Property::Opacity, 10) < 20.);
    }

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
