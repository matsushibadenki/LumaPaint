//! Small, independent LUT authoring model. Tables never belong to a WebView.
use serde::{Deserialize, Serialize};
use std::fmt::Write;

const INVALID: &str = "Invalid LUT / LUTの形式・値が不正です / LUT格式或参数无效";
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cube {
    pub title: String,
    pub size: usize,
    pub one_dimensional: bool,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub values: Vec<[f32; 3]>,
}
impl Cube {
    pub fn validate(&self) -> Result<(), String> {
        let limit = if self.one_dimensional { 65536 } else { 65 };
        if !(2..=limit).contains(&self.size)
            || self.title.chars().count() > 120
            || self.title.chars().any(|c| c.is_control() || c == '"')
            || self.values.len()
                != if self.one_dimensional {
                    self.size
                } else {
                    self.size.pow(3)
                }
            || self
                .values
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 65504.)
            || (0..3).any(|c| {
                !self.min[c].is_finite()
                    || !self.max[c].is_finite()
                    || self.max[c] - self.min[c] < 1e-6
                    || !((self.max[c] - self.min[c]).is_finite())
            })
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_BYTES {
            return Err(INVALID.into());
        }
        let mut cube = Self {
            title: "Imported LUT".into(),
            size: 0,
            one_dimensional: false,
            min: [0.; 3],
            max: [1.; 3],
            values: vec![],
        };
        let mut domain_seen = [false; 2];
        for raw in text.trim_start_matches('\u{feff}').lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let words: Vec<_> = line.split_whitespace().collect();
            let key = words[0];
            match key {
                "TITLE" if cube.values.is_empty() => {
                    cube.title = line
                        .strip_prefix("TITLE")
                        .unwrap()
                        .trim()
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .ok_or(INVALID)?
                        .to_owned();
                }
                "LUT_1D_SIZE" | "LUT_3D_SIZE" if cube.size == 0 && words.len() == 2 => {
                    cube.size = words[1].parse().map_err(|_| INVALID)?;
                    cube.one_dimensional = key == "LUT_1D_SIZE";
                    if !(2..=if cube.one_dimensional { 65536 } else { 65 }).contains(&cube.size) {
                        return Err(INVALID.into());
                    }
                }
                "DOMAIN_MIN" | "DOMAIN_MAX" if cube.values.is_empty() => {
                    let index = usize::from(key == "DOMAIN_MAX");
                    if domain_seen[index] {
                        return Err(INVALID.into());
                    }
                    domain_seen[index] = true;
                    let value = triple(&words[1..])?;
                    if index == 0 {
                        cube.min = value;
                    } else {
                        cube.max = value;
                    }
                }
                _ => {
                    if cube.size == 0
                        || cube.values.len()
                            >= if cube.one_dimensional {
                                cube.size
                            } else {
                                cube.size.pow(3)
                            }
                    {
                        return Err(INVALID.into());
                    }
                    cube.values.push(triple(&words)?);
                }
            }
        }
        cube.validate()?;
        Ok(cube)
    }
    /// Red varies fastest. Trilinear interpolation, with input-domain clamping.
    pub fn sample(&self, input: [f32; 3]) -> [f32; 3] {
        let p: [f32; 3] = std::array::from_fn(|c| {
            ((input[c] - self.min[c]) / (self.max[c] - self.min[c])).clamp(0., 1.)
                * (self.size - 1) as f32
        });
        let lo = p.map(|v| v.floor() as usize);
        let hi = lo.map(|v| (v + 1).min(self.size - 1));
        let f: [f32; 3] = std::array::from_fn(|c| p[c] - lo[c] as f32);
        if self.one_dimensional {
            return std::array::from_fn(|c| {
                self.values[lo[c]][c] * (1. - f[c]) + self.values[hi[c]][c] * f[c]
            });
        }
        let mut out = [0.; 3];
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    let bits = [r, g, b];
                    let index: [usize; 3] =
                        std::array::from_fn(|c| if bits[c] == 0 { lo[c] } else { hi[c] });
                    let weight: f32 = (0..3)
                        .map(|c| if bits[c] == 0 { 1. - f[c] } else { f[c] })
                        .product();
                    let v = self.values[index[0] + self.size * (index[1] + self.size * index[2])];
                    for c in 0..3 {
                        out[c] += v[c] * weight;
                    }
                }
            }
        }
        out
    }
}
fn triple(words: &[&str]) -> Result<[f32; 3], String> {
    if words.len() != 3 {
        return Err(INVALID.into());
    }
    let mut v = [0.; 3];
    for i in 0..3 {
        v[i] = words[i].parse::<f32>().map_err(|_| INVALID)?;
        if !v[i].is_finite() {
            return Err(INVALID.into());
        }
    }
    Ok(v)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grade {
    pub name: String,
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub warmth: f32,
    pub strength: f32,
    /// Five evenly spaced knots per curve: master, red, green, blue.
    pub curves: [[f32; 5]; 4],
    pub size: usize,
}
impl Default for Grade {
    fn default() -> Self {
        Self {
            name: "Untitled LUT".into(),
            exposure: 0.,
            contrast: 1.,
            saturation: 1.,
            warmth: 0.,
            strength: 1.,
            curves: [[0., 0.25, 0.5, 0.75, 1.]; 4],
            size: 33,
        }
    }
}
impl Grade {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.chars().count() > 120
            || self.name.chars().any(|c| c.is_control() || c == '"')
            || ![17, 33, 65].contains(&self.size)
            || [
                (self.exposure, -4., 4.),
                (self.contrast, 0., 2.),
                (self.saturation, 0., 2.),
                (self.warmth, -1., 1.),
                (self.strength, 0., 1.),
            ]
            .iter()
            .any(|(v, min, max)| !v.is_finite() || v < min || v > max)
            || self
                .curves
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err(INVALID.into());
        }
        Ok(())
    }
    /// Display-referred RGB grading. This does not perform an ICC or log transform.
    pub fn apply(&self, base: Option<&Cube>, input: [f32; 3]) -> [f32; 3] {
        let mut c = base.map_or(input, |b| b.sample(input));
        for (i, value) in c.iter_mut().enumerate() {
            *value = ((*value * 2_f32.powf(self.exposure) - 0.5) * self.contrast + 0.5)
                + self.warmth * [0.1, 0., -0.1][i];
        }
        let luma = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
        for i in 0..3 {
            c[i] = luma + (c[i] - luma) * self.saturation;
            c[i] = curve(&self.curves[i + 1], curve(&self.curves[0], c[i]));
            c[i] = input[i] * (1. - self.strength) + c[i] * self.strength;
        }
        c
    }
    pub fn bake(&self, base: Option<&Cube>) -> Result<String, String> {
        self.validate()?;
        if let Some(base) = base {
            base.validate()?;
        }
        let mut out=format!("# LumaPaint display-referred RGB LUT\nTITLE \"{}\"\nLUT_3D_SIZE {}\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n",self.name,self.size);
        for b in 0..self.size {
            for g in 0..self.size {
                for r in 0..self.size {
                    let input = [r, g, b].map(|v| v as f32 / (self.size - 1) as f32);
                    let c = self.apply(base, input);
                    writeln!(out, "{:.7} {:.7} {:.7}", c[0], c[1], c[2])
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(out)
    }
}
fn curve(knots: &[f32; 5], x: f32) -> f32 {
    let p = x.clamp(0., 1.) * 4.;
    let i = (p.floor() as usize).min(3);
    let t = p - i as f32;
    knots[i] * (1. - t) + knots[i + 1] * t
}
#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: [f32; 3], b: [f32; 3]) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-5, "{a:?} != {b:?}");
        }
    }
    #[test]
    fn lut_identity_export_order_roundtrip() {
        let grade = Grade {
            size: 17,
            ..Grade::default()
        };
        let cube = Cube::parse(&grade.bake(None).unwrap()).unwrap();
        close(cube.values[1], [1. / 16., 0., 0.]);
        close(cube.sample([0.31, 0.52, 0.87]), [0.31, 0.52, 0.87]);
        close(cube.sample([-1., 0.3, 2.]), [0., 0.3, 1.]);
    }
    #[test]
    fn lut_1d_domain_and_malformed_files() {
        let cube=Cube::parse("TITLE \"Invert\"\nLUT_1D_SIZE 2\nDOMAIN_MIN -1 -1 -1\nDOMAIN_MAX 1 1 1\n1 1 1\n0 0 0\n").unwrap();
        close(cube.sample([-1., 0., 1.]), [1., 0.5, 0.]);
        for text in [
            "LUT_3D_SIZE 999999\n",
            "LUT_1D_SIZE 2\nNaN 0 0\n1 1 1",
            "LUT_1D_SIZE 2\n0 0 0",
            "LUT_1D_SIZE 2\nLUT_3D_SIZE 2",
            "LUT_1D_SIZE 2\nDOMAIN_MAX 0 0 0\n0 0 0\n1 1 1",
        ] {
            assert!(Cube::parse(text).is_err());
        }
    }
    #[test]
    fn lut_grade_strength_curves_and_export_agree() {
        let grade = Grade {
            exposure: 0.5,
            saturation: 0.4,
            curves: [[0., 0.18, 0.6, 0.8, 1.]; 4],
            ..Grade::default()
        };
        let cube = Cube::parse(&grade.bake(None).unwrap()).unwrap();
        let input = [0.25, 0.5, 0.75];
        close(cube.sample(input), grade.apply(None, input));
        close(
            Grade {
                strength: 0.,
                ..grade.clone()
            }
            .apply(Some(&cube), input),
            input,
        );
        assert_ne!(grade.apply(None, input), input);
        assert!(Grade {
            exposure: f32::NAN,
            ..grade
        }
        .validate()
        .is_err());
    }
}
