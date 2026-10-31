//! Normalized tablet pressure and portable brush response, shared by every renderer.
use serde::{Deserialize, Serialize};

pub fn normalize(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0., 1.)
    } else {
        0.
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Response {
    pub enabled: bool,
    pub minimum: f32,
    /// Output at input pressure 0%, 25%, 50%, 75%, 100%.
    pub points: [f32; 5],
}
impl Default for Response {
    fn default() -> Self {
        Self {
            enabled: true,
            minimum: 0.05,
            points: [0., 0.25, 0.5, 0.75, 1.],
        }
    }
}
impl Response {
    pub fn validate(self) -> Result<(), String> {
        if !self.minimum.is_finite()
            || !(0. ..=1.).contains(&self.minimum)
            || self
                .points
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || self.points.windows(2).any(|p| p[0] > p[1])
            || self.points[0] != 0.
            || self.points[4] != 1.
        {
            return Err("Invalid pressure response / 筆圧応答が不正です / 压感响应无效".into());
        }
        Ok(())
    }
    pub fn evaluate(self, pressure: f32) -> f32 {
        if !self.enabled {
            return 1.;
        }
        let p = normalize(pressure) * 4.;
        let index = (p.floor() as usize).min(3);
        (self.points[index] + (self.points[index + 1] - self.points[index]) * (p - index as f32))
            .max(self.minimum)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrushKind {
    #[default]
    Round,
    InkPen,
    Graphite,
    DryBrush,
    Detail,
    Ink,
    Bold,
    Paint,
    Soft,
    Shade,
    Air,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub brush_id: BrushKind,
    pub size: Response,
    pub opacity: Response,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            brush_id: BrushKind::Round,
            size: Response::default(),
            opacity: Response {
                enabled: false,
                minimum: 0.,
                ..Response::default()
            },
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        self.size.validate()?;
        self.opacity.validate()
    }
    pub fn for_brush(brush_id: BrushKind) -> Self {
        let mut settings = Self {
            brush_id,
            ..Self::default()
        };
        match brush_id {
            BrushKind::InkPen | BrushKind::Ink | BrushKind::Detail => {
                settings.size.minimum = 0.02;
                settings.size.points = [0., 0.12, 0.35, 0.65, 1.];
            }
            BrushKind::Graphite | BrushKind::DryBrush => {
                settings.size.minimum = 0.25;
                settings.opacity.enabled = true;
                settings.opacity.minimum = 0.03;
            }
            BrushKind::Paint | BrushKind::Soft | BrushKind::Shade | BrushKind::Air => {
                settings.size.minimum = 0.35;
                settings.size.points = [0., 0.4, 0.7, 0.9, 1.];
                settings.opacity.enabled = true;
            }
            _ => {}
        }
        settings
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_and_legacy_default_are_stable() {
        for p in [-1., 0., 0.01, 0.25, 0.5, 1., 2.] {
            assert_eq!(Settings::default().size.evaluate(p), normalize(p).max(0.05));
            assert_eq!(Settings::default().opacity.evaluate(p), 1.);
        }
        assert_eq!(normalize(f32::NAN), 0.);
        assert_eq!(normalize(f32::INFINITY), 0.);
        let legacy: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy, Settings::default());
    }
    #[test]
    fn independent_curves_minimums_and_interpolation() {
        let s = Settings {
            size: Response {
                minimum: 0.2,
                points: [0., 0.1, 0.3, 0.6, 1.],
                ..Response::default()
            },
            opacity: Response {
                enabled: true,
                minimum: 0.,
                points: [0., 0.4, 0.7, 0.9, 1.],
            },
            ..Settings::default()
        };
        s.validate().unwrap();
        assert_eq!(s.size.evaluate(0.), 0.2);
        assert!((s.size.evaluate(0.625) - 0.45).abs() < 1e-6);
        assert_eq!(s.opacity.evaluate(0.5), 0.7);
        let saved = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&saved).unwrap(), s);
    }
    #[test]
    fn artwork_keeps_response_through_save_undo_and_redo() {
        use crate::document::{Brush, Document, Point};
        let mut document = Document::default();
        let pressure = Settings::for_brush(BrushKind::Graphite);
        let brush = Brush {
            pressure,
            ..Brush::default()
        };
        document
            .begin_with_pressure(Point { x: 10., y: 10. }, brush, 0.2)
            .unwrap();
        document
            .extend_with_pressure(Point { x: 80., y: 40. }, 0.8)
            .unwrap();
        document.finish();
        let loaded = Document::decode(&document.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.visible_strokes().next().unwrap().brush.pressure,
            pressure
        );
        document.undo();
        assert!(document.visible_strokes().next().is_none());
        document.redo();
        assert_eq!(
            document.visible_strokes().next().unwrap().brush.pressure,
            pressure
        );
        let mut legacy = serde_json::to_value(Brush::default()).unwrap();
        legacy.as_object_mut().unwrap().remove("pressure");
        assert_eq!(
            serde_json::from_value::<Brush>(legacy).unwrap().pressure,
            Settings::default()
        );
        let revision = document.revision();
        let invalid = Brush {
            pressure: Settings {
                size: Response {
                    points: [0., 0.8, 0.2, 0.9, 1.],
                    ..Response::default()
                },
                ..pressure
            },
            ..brush
        };
        assert!(document
            .begin_with_pressure(Point { x: 1., y: 1. }, invalid, 0.5)
            .is_err());
        assert_eq!(document.revision(), revision);
    }
    #[test]
    fn invalid_curves_rejected_and_brush_defaults_valid() {
        for points in [
            [0., 0.8, 0.2, 0.9, 1.],
            [0., 0.2, f32::NAN, 0.9, 1.],
            [0.1, 0.2, 0.5, 0.9, 1.],
        ] {
            assert!(Response {
                points,
                ..Response::default()
            }
            .validate()
            .is_err());
        }
        assert!(Response {
            minimum: -0.1,
            ..Response::default()
        }
        .validate()
        .is_err());
        for id in [
            BrushKind::Round,
            BrushKind::InkPen,
            BrushKind::Graphite,
            BrushKind::DryBrush,
            BrushKind::Detail,
            BrushKind::Ink,
            BrushKind::Bold,
            BrushKind::Paint,
            BrushKind::Soft,
            BrushKind::Shade,
            BrushKind::Air,
        ] {
            let s = Settings::for_brush(id);
            s.validate().unwrap();
            for channel in [s.size, s.opacity] {
                let values = (0..=100)
                    .map(|p| channel.evaluate(p as f32 / 100.))
                    .collect::<Vec<_>>();
                assert!(values.windows(2).all(|p| p[0] <= p[1]));
            }
        }
    }
}
