//! Portable non-destructive RGB8 adjustments. No UI or renderer dependencies.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerEffects {
    pub enabled: bool,
    /// Red, orange, yellow, green, aqua, blue, purple, magenta H/S/L offsets.
    #[serde(default)]
    pub mixer: [[f32; 3]; 8],
    /// Shadows, midtones, highlights: hue degrees, saturation %, luminance %.
    #[serde(default)]
    pub grading: [[f32; 3]; 3],
    #[serde(default = "default_grading_blend")]
    pub grading_blend: f32,
    #[serde(default)]
    pub grading_balance: f32,
    /// Exposure (EV), contrast, highlights, shadows, whites, blacks,
    /// temperature, tint, vibrance, saturation. Remaining values use -100..100.
    pub values: [f32; 10],
    /// Composite, red, green, blue curves, normalized 0..1.
    /// Missing interpolation flags preserve legacy piecewise-linear appearance.
    #[serde(default)]
    pub curve_smooth: [bool; 4],
    pub curves: [Vec<[f32; 2]>; 4],
}
impl Default for LayerEffects {
    fn default() -> Self {
        Self {
            enabled: false,
            mixer: [[0.; 3]; 8],
            grading: [[0.; 3]; 3],
            grading_blend: 50.,
            grading_balance: 0.,
            values: [0.; 10],
            curve_smooth: [true; 4],
            curves: std::array::from_fn(|_| vec![[0., 0.], [1., 1.]]),
        }
    }
}
impl LayerEffects {
    pub fn validate(&self) -> Result<(), String> {
        if !self.grading_blend.is_finite()
            || !(0. ..=100.).contains(&self.grading_blend)
            || !self.grading_balance.is_finite()
            || self.grading_balance.abs() > 100.
        {
            return Err("Invalid grading blend or balance".into());
        }
        for (i, value) in self.values.iter().enumerate() {
            let limit = if i == 0 { 5. } else { 100. };
            if !value.is_finite() || value.abs() > limit {
                return Err("Invalid layer adjustment".into());
            }
        }
        if self
            .mixer
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 100.)
            || self.grading.iter().any(|z| {
                z.iter().any(|v| !v.is_finite())
                    || !(0. ..=360.).contains(&z[0])
                    || !(0. ..=100.).contains(&z[1])
                    || z[2].abs() > 100.
            })
        {
            return Err("Invalid color mixer or grading".into());
        }
        for curve in &self.curves {
            if !(2..=16).contains(&curve.len())
                || curve
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
                || curve.windows(2).any(|p| p[0][0] >= p[1][0])
            {
                return Err("Invalid layer curve".into());
            }
        }
        Ok(())
    }
    pub fn apply(&self, pixel: [u8; 4]) -> [u8; 4] {
        if !self.enabled || pixel[3] == 0 {
            return pixel;
        }
        let v = self.values.map(|x| x / 100.);
        let mut rgb = [pixel[0], pixel[1], pixel[2]].map(|c| f32::from(c) / 255.);
        for (i, c) in rgb.iter_mut().enumerate() {
            let gain = match i {
                0 => 1. + v[6] * 0.2 + v[7] * 0.1,
                1 => 1. - v[7] * 0.2,
                _ => 1. - v[6] * 0.2 + v[7] * 0.1,
            };
            if self.values[0] != 0. || gain != 1. {
                let linear = if *c <= 0.04045 {
                    *c / 12.92
                } else {
                    ((*c + 0.055) / 1.055).powf(2.4)
                };
                let linear = linear * self.values[0].exp2() * gain;
                *c = if linear <= 0.0031308 {
                    linear * 12.92
                } else {
                    1.055 * linear.powf(1. / 2.4) - 0.055
                };
            }
            let t = c.clamp(0., 1.);
            *c = (*c - 0.5) * (1. + v[1] * 0.8) + 0.5;
            *c += 0.25 * (v[2] * t * t + v[3] * (1. - t).powi(2))
                + 0.2 * (v[4] * t.powi(4) + v[5] * (1. - t).powi(4));
        }
        let gray = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
        let chroma = rgb.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            - rgb.iter().copied().fold(f32::INFINITY, f32::min);
        let saturation = (1. + v[9] + v[8] * (1. - chroma.clamp(0., 1.))).max(0.);
        for (i, c) in rgb.iter_mut().enumerate() {
            *c = curve_sample(
                &self.curves[i + 1],
                curve_sample(
                    &self.curves[0],
                    (gray + (*c - gray) * saturation).clamp(0., 1.),
                    self.curve_smooth[0],
                ),
                self.curve_smooth[i + 1],
            );
        }
        if self.mixer.iter().flatten().any(|v| *v != 0.) {
            let [h, s, l] = to_hsl(rgb);
            if s > 0. {
                let centers = [0., 30., 60., 120., 180., 240., 270., 300., 360.];
                let index = (0..8).find(|i| h <= centers[i + 1]).unwrap_or(7);
                let t = (h - centers[index]) / (centers[index + 1] - centers[index]);
                let offset: [f32; 3] = std::array::from_fn(|i| {
                    self.mixer[index][i] * (1. - t) + self.mixer[(index + 1) % 8][i] * t
                });
                rgb = from_hsl(
                    (h + offset[0] * 0.3).rem_euclid(360.),
                    (s * (1. + offset[1] / 100.)).clamp(0., 1.),
                    (l + offset[2] / 100. * s).clamp(0., 1.),
                );
            }
        }
        if self.grading.iter().any(|z| z[1] != 0. || z[2] != 0.) {
            let l = (rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722).clamp(0., 1.);
            let weights = grading_weights(l, self.grading_blend, self.grading_balance);
            let original = rgb;
            for (zone, weight) in self.grading.iter().zip(weights) {
                let tint = from_hsl(zone[0].rem_euclid(360.), 1., 0.5);
                for i in 0..3 {
                    rgb[i] += weight * (zone[1] / 100. * (tint[i] - original[i]) + zone[2] / 100.);
                }
            }
        }
        let mut out = pixel;
        for i in 0..3 {
            out[i] = (rgb[i].clamp(0., 1.) * 255.).round() as u8;
        }
        out
    }
}

fn default_grading_blend() -> f32 {
    50.
}
/// Positive balance extends highlights into darker tones; blending softens zone transitions.
fn grading_weights(l: f32, blend: f32, balance: f32) -> [f32; 3] {
    let l = (l + l * (1. - l) * balance / 50.).clamp(0., 1.);
    let base = [(1. - l).powi(2), 2. * l * (1. - l), l.powi(2)];
    if blend == 50. {
        return base;
    }
    let power = ((50. - blend) / 25.).exp2();
    let weights = base.map(|v| v.powf(power));
    let total: f32 = weights.iter().sum();
    weights.map(|v| v / total)
}
fn to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let max = rgb.into_iter().fold(0., f32::max);
    let min = rgb.into_iter().fold(1., f32::min);
    let d = max - min;
    let l = (max + min) / 2.;
    if d == 0. {
        return [0., 0., l];
    }
    let h = if max == rgb[0] {
        ((rgb[1] - rgb[2]) / d).rem_euclid(6.)
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / d + 2.
    } else {
        (rgb[0] - rgb[1]) / d + 4.
    };
    [h * 60., d / (1. - (2. * l - 1.).abs()), l]
}
fn from_hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.).rem_euclid(2.) - 1.).abs());
    let rgb = match (h / 60.) as u32 {
        0 => [c, x, 0.],
        1 => [x, c, 0.],
        2 => [0., c, x],
        3 => [0., x, c],
        4 => [x, 0., c],
        _ => [c, 0., x],
    };
    rgb.map(|v| v + l - c / 2.)
}

/// Shape-preserving cubic Hermite interpolation, equivalent to cubic Bezier
/// segments with x handles at one third of the interval. Extrema do not overshoot.
pub fn curve_sample(curve: &[[f32; 2]], x: f32, smooth: bool) -> f32 {
    if x <= curve[0][0] {
        return curve[0][1];
    }
    if x >= curve[curve.len() - 1][0] {
        return curve[curve.len() - 1][1];
    }
    let i = curve.windows(2).position(|p| x <= p[1][0]).unwrap();
    let [x0, y0] = curve[i];
    let [x1, y1] = curve[i + 1];
    let h = x1 - x0;
    let t = (x - x0) / h;
    if !smooth || curve.len() == 2 {
        return y0 + t * (y1 - y0);
    }
    let m0 = curve_tangent(curve, i);
    let m1 = curve_tangent(curve, i + 1);
    let t2 = t * t;
    let t3 = t2 * t;
    ((2. * t3 - 3. * t2 + 1.) * y0
        + (t3 - 2. * t2 + t) * h * m0
        + (-2. * t3 + 3. * t2) * y1
        + (t3 - t2) * h * m1)
        .clamp(0., 1.)
}
fn curve_tangent(curve: &[[f32; 2]], i: usize) -> f32 {
    let secant = |j: usize| (curve[j + 1][1] - curve[j][1]) / (curve[j + 1][0] - curve[j][0]);
    if i == 0 {
        return secant(0);
    }
    if i == curve.len() - 1 {
        return secant(i - 1);
    }
    let left = secant(i - 1);
    let right = secant(i);
    if left * right <= 0. {
        return 0.;
    }
    let a = curve[i][0] - curve[i - 1][0];
    let b = curve[i + 1][0] - curve[i][0];
    let w1 = 2. * b + a;
    let w2 = b + 2. * a;
    (w1 + w2) / (w1 / left + w2 / right)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cubic_curves_preserve_knots_extrema_and_legacy_linear_values() {
        let curve = [[0., 0.], [0.25, 0.6], [0.7, 0.8], [1., 1.]];
        for [x, y] in curve {
            assert!((curve_sample(&curve, x, true) - y).abs() < 0.00001);
        }
        assert!(
            (curve_sample(&curve, 0.125, true) - curve_sample(&curve, 0.125, false)).abs() > 0.01
        );
        let mut previous = 0.;
        for i in 0..=4096 {
            let y = curve_sample(&curve, i as f32 / 4096., true);
            assert!(y >= previous - 0.00001);
            previous = y;
        }
        let turning = [[0.1, 0.2], [0.4, 0.9], [0.6, 0.1], [0.9, 0.8]];
        for i in 0..=4096 {
            let x = i as f32 / 4096.;
            let y = curve_sample(&turning, x, true);
            assert!((0.09999..=0.90001).contains(&y));
        }
        assert_eq!(curve_sample(&turning, 0., true), 0.2);
        assert_eq!(curve_sample(&turning, 1., true), 0.8);
        let mut saved = serde_json::to_value(LayerEffects::default()).unwrap();
        saved.as_object_mut().unwrap().remove("curveSmooth");
        assert_eq!(
            serde_json::from_value::<LayerEffects>(saved)
                .unwrap()
                .curve_smooth,
            [false; 4]
        );
    }
    #[test]
    fn grading_transitions_balance_and_legacy() {
        assert_eq!(grading_weights(0.5, 50., 0.), [0.25, 0.5, 0.25]);
        assert!(grading_weights(0.5, 0., 0.)[1] > grading_weights(0.5, 100., 0.)[1]);
        assert!(grading_weights(0.5, 50., 100.)[2] > grading_weights(0.5, 50., -100.)[2]);
        for blend in [0., 50., 100.] {
            for balance in [-100., 0., 100.] {
                for i in 0..=255 {
                    let w = grading_weights(i as f32 / 255., blend, balance);
                    assert!((w.iter().sum::<f32>() - 1.).abs() < 0.00001);
                    assert!(w.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)));
                }
            }
        }
        let mut saved = serde_json::to_value(LayerEffects::default()).unwrap();
        saved.as_object_mut().unwrap().remove("gradingBlend");
        saved.as_object_mut().unwrap().remove("gradingBalance");
        assert_eq!(
            serde_json::from_value::<LayerEffects>(saved).unwrap(),
            LayerEffects::default()
        );
        let mut e = LayerEffects {
            enabled: true,
            grading_balance: 100.,
            grading_blend: 0.,
            ..Default::default()
        };
        assert_eq!(e.apply([20, 60, 180, 123]), [20, 60, 180, 123]);
        e.grading_blend = f32::NAN;
        assert!(e.validate().is_err());
    }
    #[test]
    fn mixer_grading_and_legacy_defaults() {
        let mut e = LayerEffects {
            enabled: true,
            ..Default::default()
        };
        e.mixer[0][1] = -100.;
        assert_eq!(e.apply([255, 0, 0, 128]), [128, 128, 128, 128]);
        assert_eq!(e.apply([0, 255, 0, 128]), [0, 255, 0, 128]);
        assert_eq!(e.apply([80, 80, 80, 128]), [80, 80, 80, 128]);
        e.mixer = [[0.; 3]; 8];
        e.grading[0] = [0., 100., 0.];
        assert_eq!(e.apply([0, 0, 0, 128]), [255, 0, 0, 128]);
        assert_eq!(e.apply([255, 255, 255, 128]), [255, 255, 255, 128]);
        assert_eq!(e.apply([1, 2, 3, 0]), [1, 2, 3, 0]);
        let mut saved = serde_json::to_value(LayerEffects::default()).unwrap();
        saved.as_object_mut().unwrap().remove("mixer");
        saved.as_object_mut().unwrap().remove("grading");
        assert_eq!(
            serde_json::from_value::<LayerEffects>(saved).unwrap(),
            LayerEffects::default()
        );
        e.grading[1][0] = f32::NAN;
        assert!(e.validate().is_err());
    }
    #[test]
    fn identity_alpha_curves_and_validation() {
        let mut e = LayerEffects {
            enabled: true,
            ..Default::default()
        };
        for c in 0..=255 {
            assert_eq!(e.apply([c, c, c, 128]), [c, c, c, 128]);
        }
        e.values[0] = 1.;
        assert_eq!(e.apply([64, 64, 64, 128]), [90, 90, 90, 128]);
        e.values[0] = 0.;
        e.curves[1] = vec![[0., 1.], [1., 0.]];
        assert_eq!(e.apply([0, 0, 255, 128]), [255, 0, 255, 128]);
        assert_eq!(e.apply([1, 2, 3, 0]), [1, 2, 3, 0]);
        e.curves[1] = vec![[0., 0.], [0., 1.]];
        assert!(e.validate().is_err());
        e = Default::default();
        e.values[1] = f32::NAN;
        assert!(e.validate().is_err());
    }
}
