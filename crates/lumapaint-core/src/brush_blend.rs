//! Brush compositing in straight RGBA. Opacity caps a whole stroke; flow is
//! accumulated by the dab sampler before this kernel is called.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BrushBlendMode {
    #[default]
    Normal,
    Dissolve,
    Behind,
    Clear,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}
fn lum(c: [f32; 3]) -> f32 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}
fn sat(c: [f32; 3]) -> f32 {
    c.into_iter().fold(0., f32::max) - c.into_iter().fold(1., f32::min)
}
fn set_lum(mut c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    c = c.map(|v| v + d);
    let n = c.into_iter().fold(f32::INFINITY, f32::min);
    let x = c.into_iter().fold(f32::NEG_INFINITY, f32::max);
    if n < 0. {
        c = c.map(|v| l + (v - l) * l / (l - n).max(f32::EPSILON));
    }
    if x > 1. {
        c = c.map(|v| l + (v - l) * (1. - l) / (x - l).max(f32::EPSILON));
    }
    c.map(|v| v.clamp(0., 1.))
}
fn set_sat(mut c: [f32; 3], s: f32) -> [f32; 3] {
    let mut order = [0, 1, 2];
    order.sort_by(|a, b| c[*a].total_cmp(&c[*b]));
    let [lo, mid, hi] = order;
    let range = c[hi] - c[lo];
    c[mid] = if range > 0. {
        (c[mid] - c[lo]) * s / range
    } else {
        0.
    };
    c[hi] = if range > 0. { s } else { 0. };
    c[lo] = 0.;
    c
}
fn dodge(b: f32, s: f32) -> f32 {
    if b == 0. {
        0.
    } else if s == 1. {
        1.
    } else {
        (b / (1. - s)).min(1.)
    }
}
fn burn(b: f32, s: f32) -> f32 {
    if b == 1. {
        1.
    } else if s == 0. {
        0.
    } else {
        1. - ((1. - b) / s).min(1.)
    }
}
fn vivid(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        burn(b, 2. * s)
    } else {
        dodge(b, 2. * s - 1.)
    }
}
impl BrushBlendMode {
    pub fn blend(self, back: [f32; 3], source: [f32; 3]) -> [f32; 3] {
        use BrushBlendMode::*;
        match self {
            Hue => return set_lum(set_sat(source, sat(back)), lum(back)),
            Saturation => return set_lum(set_sat(back, sat(source)), lum(back)),
            Color => return set_lum(source, lum(back)),
            Luminosity => return set_lum(back, lum(source)),
            DarkerColor => {
                return if source.iter().sum::<f32>() < back.iter().sum() {
                    source
                } else {
                    back
                }
            }
            LighterColor => {
                return if source.iter().sum::<f32>() > back.iter().sum() {
                    source
                } else {
                    back
                }
            }
            _ => {}
        }
        std::array::from_fn(|i| {
            let (b, s) = (back[i], source[i]);
            match self {
                Darken => b.min(s),
                Multiply => b * s,
                ColorBurn => burn(b, s),
                LinearBurn => b + s - 1.,
                Lighten => b.max(s),
                Screen => b + s - b * s,
                ColorDodge => dodge(b, s),
                LinearDodge => b + s,
                Overlay => {
                    if b <= 0.5 {
                        2. * b * s
                    } else {
                        1. - 2. * (1. - b) * (1. - s)
                    }
                }
                HardLight => {
                    if s <= 0.5 {
                        2. * b * s
                    } else {
                        1. - 2. * (1. - b) * (1. - s)
                    }
                }
                SoftLight => {
                    if s <= 0.5 {
                        b - (1. - 2. * s) * b * (1. - b)
                    } else {
                        let d = if b <= 0.25 {
                            ((16. * b - 12.) * b + 4.) * b
                        } else {
                            b.sqrt()
                        };
                        b + (2. * s - 1.) * (d - b)
                    }
                }
                VividLight => vivid(b, s),
                LinearLight => b + 2. * s - 1.,
                PinLight => {
                    if s <= 0.5 {
                        b.min(2. * s)
                    } else {
                        b.max(2. * s - 1.)
                    }
                }
                HardMix => {
                    if vivid(b, s) < 0.5 {
                        0.
                    } else {
                        1.
                    }
                }
                Difference => (b - s).abs(),
                Exclusion => b + s - 2. * b * s,
                Subtract => b - s,
                Divide => {
                    if s == 0. {
                        1.
                    } else {
                        b / s
                    }
                }
                _ => s,
            }
            .clamp(0., 1.)
        })
    }
}

pub fn composite(
    back: [u8; 4],
    color: [u8; 3],
    mut amount: f32,
    mode: BrushBlendMode,
    alpha_locked: bool,
    position: [u32; 2],
) -> [u8; 4] {
    use BrushBlendMode::*;
    if amount <= 0. || (alpha_locked && (back[3] == 0 || matches!(mode, Behind | Clear))) {
        return back;
    }
    if mode == Dissolve {
        // Stable document-coordinate noise keeps previews, exports and undo identical.
        let mut hash = position[0].wrapping_mul(0x9e3779b9) ^ position[1].wrapping_mul(0x85ebca6b);
        hash ^= hash >> 16;
        hash = hash.wrapping_mul(0x7feb352d);
        hash ^= hash >> 15;
        amount = if (hash as f64 + 0.5) / (u32::MAX as f64 + 1.) < f64::from(amount) {
            1.
        } else {
            0.
        };
    }
    let ba = f32::from(back[3]) / 255.;
    let b = [back[0], back[1], back[2]].map(|v| f32::from(v) / 255.);
    let s = color.map(|v| f32::from(v) / 255.);
    let byte = |v: f32| (v * 255.).round().clamp(0., 255.) as u8;
    if mode == Clear {
        let a = byte(ba * (1. - amount));
        return if a == 0 {
            [0; 4]
        } else {
            [back[0], back[1], back[2], a]
        };
    }
    let blend = mode.blend(b, s);
    if alpha_locked {
        return [
            byte(b[0] * (1. - amount) + blend[0] * amount),
            byte(b[1] * (1. - amount) + blend[1] * amount),
            byte(b[2] * (1. - amount) + blend[2] * amount),
            back[3],
        ];
    }
    let a = amount + ba * (1. - amount);
    if byte(a) == 0 {
        return [0; 4];
    }
    let rgb = std::array::from_fn::<_, 3, _>(|i| {
        byte(if mode == Behind {
            (b[i] * ba + s[i] * amount * (1. - ba)) / a
        } else {
            (b[i] * ba * (1. - amount) + s[i] * amount * (1. - ba) + blend[i] * amount * ba) / a
        })
    });
    [rgb[0], rgb[1], rgb[2], byte(a)]
}

/// Blend a grayscale channel or mask without changing any neighboring components.
pub fn composite_gray(
    old: u8,
    value: u8,
    amount: f32,
    mode: BrushBlendMode,
    position: [u32; 2],
) -> u8 {
    if mode == BrushBlendMode::Clear {
        (f32::from(old) * (1. - amount)).round() as u8
    } else {
        composite(
            [old, old, old, 255],
            [value; 3],
            amount,
            mode,
            false,
            position,
        )[0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Brush;
    #[test]
    fn brush_defaults_load_old_files_and_validate_ranges() {
        let old: Brush =
            serde_json::from_str(r#"{"size":16,"hardness":1,"color":[32,32,32]}"#).unwrap();
        assert_eq!(old, Brush::default());
        for value in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            for brush in [
                Brush {
                    opacity: value,
                    ..old
                },
                Brush { flow: value, ..old },
                Brush {
                    alpha: value,
                    ..old
                },
                Brush {
                    smoothing: value,
                    ..old
                },
            ] {
                assert!(brush.validate().is_err());
            }
        }
        let new = Brush {
            opacity: 0.6,
            flow: 0.2,
            alpha: 0.4,
            smoothing: 0.8,
            blend_mode: BrushBlendMode::ColorBurn,
            ..old
        };
        assert_eq!(
            serde_json::from_str::<Brush>(&serde_json::to_string(&new).unwrap()).unwrap(),
            new
        );
    }
    #[test]
    fn brush_blending_respects_alpha_and_special_modes() {
        use BrushBlendMode::*;
        assert_eq!(
            composite([0; 4], [200, 100, 50], 0.25, Normal, false, [0; 2]),
            [200, 100, 50, 64]
        );
        assert_eq!(
            composite(
                [128, 128, 128, 255],
                [128, 64, 255],
                1.,
                Multiply,
                false,
                [0; 2]
            ),
            [64, 32, 128, 255]
        );
        assert_eq!(
            composite(
                [128, 128, 128, 255],
                [128, 64, 255],
                1.,
                Screen,
                false,
                [0; 2]
            ),
            [192, 160, 255, 255]
        );
        assert_eq!(
            composite([20, 40, 60, 255], [255; 3], 1., Behind, false, [0; 2]),
            [20, 40, 60, 255]
        );
        assert_eq!(
            composite([0; 4], [10, 20, 30], 1., Behind, false, [0; 2]),
            [10, 20, 30, 255]
        );
        assert_eq!(
            composite([20, 40, 60, 128], [255; 3], 0.5, Clear, false, [0; 2]),
            [20, 40, 60, 64]
        );
        assert_eq!(
            composite([20, 40, 60, 128], [255; 3], 1., Clear, true, [0; 2]),
            [20, 40, 60, 128]
        );
        assert_eq!(
            composite([0; 4], [255; 3], 1., Normal, true, [0; 2]),
            [0; 4]
        );
        for mode in [
            Normal,
            Dissolve,
            Behind,
            Clear,
            Darken,
            Multiply,
            ColorBurn,
            LinearBurn,
            DarkerColor,
            Lighten,
            Screen,
            ColorDodge,
            LinearDodge,
            LighterColor,
            Overlay,
            SoftLight,
            HardLight,
            VividLight,
            LinearLight,
            PinLight,
            HardMix,
            Difference,
            Exclusion,
            Subtract,
            Divide,
            Hue,
            Saturation,
            Color,
            Luminosity,
        ] {
            for b in [[0.; 3], [1.; 3], [0.1, 0.4, 0.9]] {
                for s in [[0.; 3], [1.; 3], [0.8, 0.2, 0.6]] {
                    assert!(
                        mode.blend(b, s)
                            .iter()
                            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                        "{mode:?}"
                    );
                }
            }
        }
        let back = [0.1, 0.5, 0.8];
        let source = [0.9, 0.2, 0.4];
        for mode in [Hue, Saturation, Color] {
            assert!((lum(mode.blend(back, source)) - lum(back)).abs() < 0.0001);
        }
        assert!((lum(Luminosity.blend(back, source)) - lum(source)).abs() < 0.0001);
    }
    #[test]
    fn dissolve_is_stable_and_has_expected_pixel_density() {
        let pixels: Vec<_> = (0..4096)
            .map(|x| {
                composite(
                    [0; 4],
                    [100, 80, 20],
                    0.25,
                    BrushBlendMode::Dissolve,
                    false,
                    [x % 64, x / 64],
                )
            })
            .collect();
        let painted = pixels.iter().filter(|p| p[3] == 255).count();
        assert!((900..1150).contains(&painted));
        assert!(pixels.iter().all(|p| p[3] == 0 || p[3] == 255));
        for (x, p) in pixels.iter().enumerate() {
            assert_eq!(
                *p,
                composite(
                    [0; 4],
                    [100, 80, 20],
                    0.25,
                    BrushBlendMode::Dissolve,
                    false,
                    [x as u32 % 64, x as u32 / 64]
                )
            );
        }
    }
}
