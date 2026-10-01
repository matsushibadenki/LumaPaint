//! Renderer-independent gradient appearance, shared by vector and pixel tools.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GradientKind {
    Linear,
    Radial,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GradientMethod {
    Classic,
    Linear,
    Perceptual,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GradientStop {
    pub position: f32,
    pub color: [u8; 4],
    pub midpoint: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Gradient {
    pub kind: GradientKind,
    pub angle: f32,
    pub aspect: f32,
    #[serde(default)]
    pub dither: bool,
    pub method: GradientMethod,
    pub stops: Vec<GradientStop>,
}
impl Gradient {
    pub fn validate(&self) -> Result<(), String> {
        if !self.angle.is_finite()
            || self.angle.abs() > 36000.
            || !self.aspect.is_finite()
            || !(0.01..=10.).contains(&self.aspect)
            || !(2..=64).contains(&self.stops.len())
            || self.stops.iter().any(|s| {
                !s.position.is_finite()
                    || !(0. ..=1.).contains(&s.position)
                    || !s.midpoint.is_finite()
                    || !(0.01..=0.99).contains(&s.midpoint)
            })
            || self.stops.windows(2).any(|s| s[0].position > s[1].position)
        {
            return Err("Invalid gradient / グラデーションの値が不正です / 渐变参数无效".into());
        }
        Ok(())
    }
    pub fn sample(&self, position: f32) -> [u8; 4] {
        let first = &self.stops[0];
        if position < first.position {
            return first.color;
        }
        let Some(pair) = self.stops.windows(2).find(|s| position < s[1].position) else {
            return self.stops.last().unwrap().color;
        };
        let t = ((position - pair[0].position)
            / (pair[1].position - pair[0].position).max(f32::EPSILON))
        .clamp(0., 1.);
        let m = pair[0].midpoint;
        let t = if t < m {
            0.5 * t / m
        } else {
            0.5 + 0.5 * (t - m) / (1. - m)
        };
        let mut color = [0; 4];
        let perceptual = (self.method == GradientMethod::Perceptual)
            .then(|| perceptual_mix(pair[0].color, pair[1].color, t as f64));
        for (i, c) in color.iter_mut().enumerate() {
            if i < 3 {
                if let Some(rgb) = perceptual {
                    *c = rgb[i];
                    continue;
                }
            }
            let a = pair[0].color[i] as f32 / 255.;
            let b = pair[1].color[i] as f32 / 255.;
            let v = if i < 3 && self.method == GradientMethod::Linear {
                let decode = |v: f32| {
                    if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                };
                let v = decode(a) * (1. - t) + decode(b) * t;
                if v <= 0.0031308 {
                    v * 12.92
                } else {
                    1.055 * v.powf(1. / 2.4) - 0.055
                }
            } else {
                a * (1. - t) + b * t
            };
            *c = (v * 255.).round().clamp(0., 255.) as u8;
        }
        color
    }
    pub fn svg_dither_filter(&self, id: &str) -> String {
        if !self.dither {
            return String::new();
        }
        format!(
            r#"<filter id="{id}" x="0" y="0" width="1" height="1" color-interpolation-filters="sRGB"><feTurbulence type="fractalNoise" baseFrequency=".73" numOctaves="1" seed="7" result="noise"/><feColorMatrix in="noise" type="matrix" values="8 0 0 0 -3.5 0 8 0 0 -3.5 0 0 8 0 -3.5 0 0 0 0 .5" result="grain"/><feComposite in="SourceGraphic" in2="grain" operator="arithmetic" k1="0" k2="1" k3=".0156862745" k4="-.00784313725" result="colored"/><feComposite in="colored" in2="SourceGraphic" operator="atop"/></filter>"#
        )
    }
    pub fn svg_definition(&self, id: &str) -> String {
        self.svg_definition_in_bounds(id, [0., 0., 1., 1.])
    }
    pub fn svg_definition_in_bounds(&self, id: &str, bounds: [f32; 4]) -> String {
        use std::fmt::Write;
        let r = self.angle.to_radians();
        let (sin, cos) = r.sin_cos();
        let [x, y, right, bottom] = bounds;
        let w = (right - x).max(0.001);
        let h = (bottom - y).max(0.001);
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let length = (w * cos.abs() + h * sin.abs()) * 0.5;
        let mut svg=match self.kind {
            GradientKind::Linear=>format!("<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">",cx-cos*length,cy+sin*length,cx+cos*length,cy-sin*length),
            GradientKind::Radial=>format!("<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"0\" cy=\"0\" r=\"{}\" gradientTransform=\"translate({cx} {cy}) rotate({}) scale(1 {})\">",w*0.5,-self.angle,self.aspect),
        };
        // Bake midpoint and interpolation into standard SVG stops for independent exporters.
        for (index, stop) in self.stops.iter().enumerate() {
            let next = self.stops.get(index + 1);
            let count = if next.is_some() { 32 } else { 1 };
            for j in 0..count {
                let p = next.map_or(stop.position, |n| {
                    let middle = stop.position + (n.position - stop.position) * stop.midpoint;
                    if j < 16 {
                        stop.position + (middle - stop.position) * j as f32 / 16.
                    } else {
                        middle + (n.position - middle) * (j - 16) as f32 / 16.
                    }
                });
                let c = if j == 0 { stop.color } else { self.sample(p) };
                let _ = write!(
                    svg,
                    "<stop offset=\"{p}\" stop-color=\"#{:02x}{:02x}{:02x}\" stop-opacity=\"{}\"/>",
                    c[0],
                    c[1],
                    c[2],
                    c[3] as f32 / 255.
                );
            }
        }
        svg.push_str(match self.kind {
            GradientKind::Linear => "</linearGradient>",
            GradientKind::Radial => "</radialGradient>",
        });
        svg
    }
}

// Oklab matrices: Björn Ottosson's public-domain reference implementation.
fn perceptual_mix(a: [u8; 4], b: [u8; 4], t: f64) -> [u8; 3] {
    let multiply =
        |v: [f64; 3], m: [[f64; 3]; 3]| m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2]);
    let lab = |c: [u8; 4]| {
        let rgb = [c[0], c[1], c[2]].map(|v| {
            let v = v as f64 / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        let lms = multiply(
            rgb,
            [
                [0.4122214708, 0.5363325363, 0.0514459929],
                [0.2119034982, 0.6806995451, 0.1073969566],
                [0.0883024619, 0.2817188376, 0.6299787005],
            ],
        )
        .map(f64::cbrt);
        multiply(
            lms,
            [
                [0.2104542553, 0.7936177850, -0.0040720468],
                [1.9779984951, -2.4285922050, 0.4505937099],
                [0.0259040371, 0.7827717662, -0.8086757660],
            ],
        )
    };
    let a = lab(a);
    let b = lab(b);
    let mixed = std::array::from_fn(|i| a[i] * (1. - t) + b[i] * t);
    let lms = multiply(
        mixed,
        [
            [1., 0.3963377774, 0.2158037573],
            [1., -0.1055613458, -0.0638541728],
            [1., -0.0894841775, -1.2914855480],
        ],
    )
    .map(|v| v * v * v);
    multiply(
        lms,
        [
            [4.0767416621, -3.3077115913, 0.2309699292],
            [-1.2684380046, 2.6097574011, -0.3413193965],
            [-0.0041960863, -0.7034186147, 1.7076147010],
        ],
    )
    .map(|v| {
        let v = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1. / 2.4) - 0.055
        };
        (v * 255.).round().clamp(0., 255.) as u8
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn gradient() -> Gradient {
        Gradient {
            kind: GradientKind::Linear,
            angle: 0.,
            aspect: 1.,
            dither: false,
            method: GradientMethod::Classic,
            stops: vec![
                GradientStop {
                    position: 0.,
                    color: [0, 0, 0, 0],
                    midpoint: 0.25,
                },
                GradientStop {
                    position: 1.,
                    color: [255; 4],
                    midpoint: 0.5,
                },
            ],
        }
    }
    #[test]
    fn midpoint_alpha_and_linear_light() {
        let mut g = gradient();
        g.validate().unwrap();
        assert_eq!(g.sample(0.25), [128; 4]);
        g.method = GradientMethod::Linear;
        assert_eq!(g.sample(0.25), [188, 188, 188, 128]);
        assert!(g.svg_definition("g").contains("linearGradient"));
        g.method = GradientMethod::Perceptual;
        assert_eq!(g.sample(0.25), [99, 99, 99, 128]);
        assert_eq!(
            perceptual_mix([255, 0, 0, 255], [0, 0, 255, 255], 0.5),
            [140, 83, 162]
        );
    }
    #[test]
    fn reject_invalid() {
        let mut g = gradient();
        g.stops[1].position = -1.;
        assert!(g.validate().is_err());
        g = gradient();
        g.aspect = f32::NAN;
        assert!(g.validate().is_err());
    }
}
