//! Cubic easing with monotone time and bounded, optional value overshoot.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyCurve {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}
impl Default for KeyCurve {
    fn default() -> Self {
        Self {
            x1: 1. / 3.,
            y1: 0.,
            x2: 2. / 3.,
            y2: 1.,
        }
    }
}
impl KeyCurve {
    pub fn valid(self) -> bool {
        [self.x1, self.x2]
            .iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
            && [self.y1, self.y2]
                .iter()
                .all(|v| v.is_finite() && (-2. ..=3.).contains(v))
            && self.x1 <= self.x2
    }
    pub fn overshoots(self) -> bool {
        !(0. ..=1.).contains(&self.y1) || !(0. ..=1.).contains(&self.y2)
    }
    pub(super) fn output_valid(self, a: f32, b: f32, property: super::Property) -> bool {
        // The Bezier convex hull keeps the entire continuous interval within
        // the property limits, including between integer preview frames.
        [self.y1, self.y2].iter().all(|y| {
            property.valid(if *y == 0. {
                a
            } else if *y == 1. {
                b
            } else {
                a + (b - a) * y
            })
        })
    }
    pub fn sample(self, time: f32) -> f32 {
        if time <= 0. {
            return 0.;
        }
        if time >= 1. {
            return 1.;
        }
        let bezier = |u: f32, a: f32, b: f32| {
            let v = 1. - u;
            3. * v * v * u * a + 3. * v * u * u * b + u * u * u
        };
        let (mut low, mut high) = (0., 1.);
        for _ in 0..24 {
            let u = (low + high) * 0.5;
            if bezier(u, self.x1, self.x2) < time {
                low = u;
            } else {
                high = u;
            }
        }
        bezier((low + high) * 0.5, self.y1, self.y2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{
        animation::{Interpolation, KeyBatchEdit, Keyframe, Property},
        Document,
    };
    #[test]
    fn overshoot_samples_are_not_clipped_and_time_stays_monotone() {
        for (y, sign) in [(2., 1.), (-1., -1.)] {
            let c = KeyCurve {
                y1: y,
                y2: y,
                ..KeyCurve::default()
            };
            assert!(c.valid() && c.overshoots());
            assert_eq!(c.sample(0.), 0.);
            assert_eq!(c.sample(1.), 1.);
            assert!(if sign > 0. {
                c.sample(0.5) > 1.
            } else {
                c.sample(0.5) < 0.
            });
            for i in 0..=1000 {
                assert!(c.sample(i as f32 / 1000.).is_finite());
            }
            assert!(c.output_valid(20., 40., Property::Opacity));
            assert!(!c.output_valid(0., 100., Property::Opacity));
        }
        for c in [
            KeyCurve {
                y1: -2.01,
                ..KeyCurve::default()
            },
            KeyCurve {
                y2: 3.01,
                ..KeyCurve::default()
            },
            KeyCurve {
                x1: 0.9,
                x2: 0.1,
                ..KeyCurve::default()
            },
        ] {
            assert!(!c.valid());
        }
        assert!(!KeyCurve::default().overshoots());
    }

    #[test]
    fn cubic_curve_matches_linear_and_ease_and_stays_bounded() {
        for i in 0..=100 {
            let t = i as f32 / 100.;
            assert!((KeyCurve::default().sample(t) - t * t * (3. - 2. * t)).abs() < 0.00001);
            let c = KeyCurve {
                x1: 0.1,
                y1: 0.1,
                x2: 0.8,
                y2: 0.8,
            };
            assert!((c.sample(t) - t).abs() < 0.00001);
        }
        for c in [
            KeyCurve {
                x1: 0.,
                y1: 0.,
                x2: 0.,
                y2: 1.,
            },
            KeyCurve {
                x1: 1.,
                y1: 0.,
                x2: 1.,
                y2: 1.,
            },
            KeyCurve {
                x1: 0.1,
                y1: 0.8,
                x2: 0.4,
                y2: 1.,
            },
        ] {
            assert!(c.valid());
            let mut previous = 0.;
            for i in 0..=1000 {
                let value = c.sample(i as f32 / 1000.);
                assert!((0. ..=1.).contains(&value));
                assert!(value >= previous - 0.00001);
                previous = value;
            }
        }
        for c in [
            KeyCurve {
                x1: 0.9,
                x2: 0.1,
                ..KeyCurve::default()
            },
            KeyCurve {
                y1: f32::NAN,
                ..KeyCurve::default()
            },
            KeyCurve {
                y2: 3.1,
                ..KeyCurve::default()
            },
        ] {
            assert!(!c.valid());
        }
    }
    #[test]
    fn tangent_edits_preserve_keys_history_and_saved_evaluation() {
        let mut d = Document::default();
        let id = d.add_vector_layer().unwrap();
        for (frame, value) in [(0, 0.), (20, 100.)] {
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
        let curve = KeyCurve {
            x1: 0.1,
            y1: 0.8,
            x2: 0.4,
            y2: 1.,
        };
        let before = serde_json::to_string(&d.document_state()).unwrap();
        let revision = d.revision();
        d.edit_animation_keys(&id, Property::Opacity, &[0], KeyBatchEdit::Curve { curve })
            .unwrap();
        let value = d.animation().tracks[&id].value(Property::Opacity, 5);
        assert!(value > 60. && value < 100.);
        assert_eq!(d.revision(), revision + 1);
        let saved = serde_json::to_string(&d.document_state()).unwrap();
        d.undo();
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), before);
        d.redo();
        assert_eq!(serde_json::to_string(&d.document_state()).unwrap(), saved);
        let restored =
            Document::from_document_state(serde_json::from_str(&saved).unwrap()).unwrap();
        assert_eq!(
            restored.animation().tracks[&id].value(Property::Opacity, 5),
            value
        );
        let revision = d.revision();
        assert!(d
            .edit_animation_keys(
                &id,
                Property::Opacity,
                &[0],
                KeyBatchEdit::Curve {
                    curve: KeyCurve {
                        x1: 0.9,
                        x2: 0.1,
                        ..curve
                    }
                }
            )
            .is_err());
        assert_eq!(d.revision(), revision);
        d.edit_animation_keys(&id, Property::Opacity, &[0], KeyBatchEdit::Curve { curve })
            .unwrap();
        assert_eq!(d.revision(), revision);
        d.edit_animation_keys(
            &id,
            Property::Opacity,
            &[0],
            KeyBatchEdit::Move {
                frame_delta: 1,
                value_delta: 0.,
            },
        )
        .unwrap();
        assert_eq!(
            d.animation().tracks[&id].channels[&Property::Opacity][0].curve,
            Some(curve)
        );
        d.set_animation_key(
            &id,
            Property::Opacity,
            Keyframe {
                frame: 1,
                value: 1.,
                interpolation: Interpolation::Cubic,
                curve: None,
            },
        )
        .unwrap();
        assert_eq!(
            d.animation().tracks[&id].channels[&Property::Opacity][0].curve,
            Some(curve)
        );
        let old: Keyframe =
            serde_json::from_str(r#"{"frame":0,"value":0,"interpolation":"ease"}"#).unwrap();
        assert!(old.curve.is_none());
    }
}
