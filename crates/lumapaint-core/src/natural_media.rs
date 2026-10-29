//! Deterministic document-coordinate paper relief and pigment deposition.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaperKind {
    #[default]
    Smooth,
    ColdPress,
    Rough,
    Canvas,
    Washi,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct PaperSettings {
    #[serde(
        default = "identity_transform",
        skip_serializing_if = "is_identity_transform"
    )]
    pub texture_transform: [f32; 6],
    pub enabled: bool,
    pub kind: PaperKind,
    pub scale: f32,
    pub depth: f32,
    pub color: [u8; 3],
}
fn identity_transform() -> [f32; 6] {
    [1., 0., 0., 1., 0., 0.]
}
fn is_identity_transform(t: &[f32; 6]) -> bool {
    *t == identity_transform()
}
impl Default for PaperSettings {
    fn default() -> Self {
        Self {
            texture_transform: identity_transform(),
            enabled: false,
            kind: PaperKind::ColdPress,
            scale: 1.,
            depth: 0.6,
            color: [249, 246, 235],
        }
    }
}
impl PaperSettings {
    pub fn validate(self) -> Result<(), String> {
        if self
            .texture_transform
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || (self.texture_transform[0] * self.texture_transform[3]
                - self.texture_transform[1] * self.texture_transform[2])
                .abs()
                < 1e-10
            || !self.scale.is_finite()
            || !(0.25..=8.).contains(&self.scale)
            || !self.depth.is_finite()
            || !(0. ..=1.).contains(&self.depth)
        {
            return Err("Invalid paper settings / 用紙設定が不正です / 纸张设置无效".into());
        }
        Ok(())
    }
    pub fn packed(self) -> [f32; 4] {
        [
            if self.enabled {
                self.kind as u32 as f32 + 1.
            } else {
                0.
            },
            self.scale,
            self.depth,
            0.,
        ]
    }
    pub fn texture_point(self, x: f32, y: f32) -> (f32, f32) {
        let t = self.texture_transform;
        (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5])
    }
    pub fn transform_texture(&mut self, matrix: [f32; 6]) -> Result<(), String> {
        self.texture_transform = crate::image_frame::multiply(
            self.texture_transform,
            crate::image_frame::inverse(matrix)?,
        );
        self.validate()
    }
    pub fn color_at(self, x: f32, y: f32) -> [u8; 3] {
        if !self.enabled {
            return [255; 3];
        }
        let (x, y) = self.texture_point(x, y);
        let shade = 1. - self.depth * 0.14 * (1. - relief(x, y, self.packed()));
        self.color.map(|v| (v as f32 * shade).round() as u8)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Medium {
    #[default]
    Graphite,
    Charcoal,
    Pastel,
    Watercolor,
    Gouache,
    Oil,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct MaterialSettings {
    #[serde(
        default = "identity_transform",
        skip_serializing_if = "is_identity_transform"
    )]
    pub grain_transform: [f32; 6],
    pub enabled: bool,
    pub medium: Medium,
    pub loading: f32,
    pub wetness: f32,
    pub granulation: f32,
    pub edge: f32,
    pub paper: PaperSettings,
}
impl Default for MaterialSettings {
    fn default() -> Self {
        Self {
            grain_transform: identity_transform(),
            enabled: false,
            medium: Medium::Graphite,
            loading: 0.65,
            wetness: 0.35,
            granulation: 0.6,
            edge: 0.3,
            paper: PaperSettings::default(),
        }
    }
}
impl MaterialSettings {
    pub fn grain_point(self, x: f32, y: f32) -> (f32, f32) {
        let t = self.grain_transform;
        let snap = |v: f32| (v * 256. + 0.5).floor() / 256.;
        (
            snap(t[0] * x + t[2] * y + t[4]),
            snap(t[1] * x + t[3] * y + t[5]),
        )
    }
    pub fn transform_grain(&mut self, matrix: [f32; 6]) -> Result<(), String> {
        self.grain_transform = crate::image_frame::multiply(
            self.grain_transform,
            crate::image_frame::inverse(matrix)?,
        );
        self.validate()
    }
    pub fn validate(self) -> Result<(), String> {
        self.paper.validate()?;
        let t = self.grain_transform;
        if t.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || (t[0] * t[3] - t[1] * t[2]).abs() < 1e-10
        {
            return Err("Invalid grain transform".into());
        }
        if [self.loading, self.wetness, self.granulation, self.edge]
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err("Invalid material settings / 画材設定が不正です / 绘画材料设置无效".into());
        }
        Ok(())
    }
    pub fn packed(self) -> ([f32; 4], [f32; 4], [f32; 4]) {
        (
            [
                if self.enabled {
                    self.medium as u32 as f32 + 1.
                } else {
                    0.
                },
                self.loading,
                self.wetness,
                self.granulation,
            ],
            [self.edge, 0., 0., 0.],
            self.paper.packed(),
        )
    }
}
fn noise(x: f32, y: f32) -> f32 {
    let mut h = (x.max(0.).floor() as u32).wrapping_mul(1973)
        ^ (y.max(0.).floor() as u32).wrapping_mul(9277)
        ^ 89173;
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    (h & 255) as f32 / 255.
}
fn smooth_noise(x: f32, y: f32) -> f32 {
    let ix = x.floor();
    let iy = y.floor();
    let fx = x - ix;
    let fy = y - iy;
    let tx = fx * fx * (3. - 2. * fx);
    let ty = fy * fy * (3. - 2. * fy);
    let a = noise(ix, iy);
    let b = noise(ix + 1., iy);
    let c = noise(ix, iy + 1.);
    let d = noise(ix + 1., iy + 1.);
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * ty
}
pub fn relief(x: f32, y: f32, p: [f32; 4]) -> f32 {
    if p[0] < 0.5 || p[0] < 1.5 {
        return 0.5;
    }
    let x = x / p[1];
    let y = y / p[1];
    let fine = noise(x, y);
    let coarse = smooth_noise((x + 11.) / 4., (y + 7.) / 4.);
    if p[0] < 2.5 {
        0.35 * fine + 0.65 * coarse
    } else if p[0] < 3.5 {
        0.2 * fine + 0.8 * smooth_noise(x / 8., y / 8.)
    } else if p[0] < 4.5 {
        let a = (x / 3.).floor() as u32 % 2;
        let b = (y / 3.).floor() as u32 % 2;
        0.2 * fine + if a == b { 0.7 } else { 0.1 }
    } else {
        0.35 * fine + 0.65 * smooth_noise(x / 9., y / 1.5)
    }
}
/// Same arithmetic as natural_media.wgsl. Radial distance is normalized to tip radius.
pub fn deposition(x: f32, y: f32, r: f32, m: [f32; 4], a: [f32; 4], p: [f32; 4]) -> f32 {
    deposition_mapped(x, y, x, y, r, m, a, p)
}
pub fn deposition_mapped(
    x: f32,
    y: f32,
    paper_x: f32,
    paper_y: f32,
    r: f32,
    m: [f32; 4],
    a: [f32; 4],
    p: [f32; 4],
) -> f32 {
    if m[0] < 0.5 {
        return 1.;
    }
    if m[1] <= 0. {
        return 0.;
    }
    let h = relief(paper_x, paper_y, p);
    let relief = 0.5 + (h - 0.5) * p[2];
    let grain = 1. - m[3] + m[3] * noise(x, y);
    let load = m[1];
    let wet = m[2];
    let dry = ((relief - (1. - load) * 0.75) * 3.).clamp(0., 1.);
    if m[0] < 1.5 {
        (0.02 + dry * 0.35) * grain
    } else if m[0] < 2.5 {
        dry * 0.22 * grain
    } else if m[0] < 3.5 {
        (0.1 + dry * 0.65) * grain
    } else if m[0] < 4.5 {
        let rim = ((r - 0.65) / 0.35).clamp(0., 1.);
        (0.03 + load * 0.12)
            * (1. - wet * 0.65)
            * (1. + a[0] * rim * 3.)
            * (1. - m[3] * 0.75 * relief)
            * grain
    } else if m[0] < 5.5 {
        (0.35 + load * 0.65) * (0.5 + dry * 0.5) * (1. - wet * 0.3) * grain
    } else {
        let bristle = 0.35 + 0.65 * noise(x / 2., y / 12.);
        (0.4 + load * 0.6) * (0.4 + dry * 0.6) * bristle * (1. - wet * 0.2) * grain
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural_media_relief_and_deposition_are_stable_and_bounded() {
        for kind in [
            PaperKind::Smooth,
            PaperKind::ColdPress,
            PaperKind::Rough,
            PaperKind::Canvas,
            PaperKind::Washi,
        ] {
            let p = PaperSettings {
                enabled: true,
                kind,
                ..Default::default()
            };
            for x in 0..100 {
                let h = relief(x as f32, 11., p.packed());
                assert!((0. ..=1.).contains(&h));
                assert_eq!(h, relief(x as f32, 11., p.packed()));
                for medium in [
                    Medium::Graphite,
                    Medium::Charcoal,
                    Medium::Pastel,
                    Medium::Watercolor,
                    Medium::Gouache,
                    Medium::Oil,
                ] {
                    let s = MaterialSettings {
                        enabled: true,
                        medium,
                        paper: p,
                        ..Default::default()
                    };
                    let (m, a, p) = s.packed();
                    let d = deposition(x as f32, 11., 0.9, m, a, p);
                    assert!(d.is_finite() && (0. ..=4.).contains(&d));
                }
            }
        }
    }
    #[test]
    fn natural_media_loading_and_compatibility() {
        let old: MaterialSettings = serde_json::from_str("{}").unwrap();
        assert!(!old.enabled);
        assert_eq!(deposition(2., 2., 0., [0.; 4], [0.; 4], [0.; 4]), 1.);
        let p = PaperSettings {
            enabled: true,
            ..Default::default()
        };
        let sum = |load| {
            let (m, a, p) = MaterialSettings {
                enabled: true,
                loading: load,
                paper: p,
                ..Default::default()
            }
            .packed();
            (0..100)
                .map(|x| deposition(x as f32, 10., 0., m, a, p))
                .sum::<f32>()
        };
        assert_eq!(sum(0.), 0.);
        assert!(sum(1.) > sum(0.));
        assert!(PaperSettings {
            scale: f32::NAN,
            ..p
        }
        .validate()
        .is_err());
    }
}
#[cfg(test)]
mod document_tests {
    use super::*;
    use crate::document::{Brush, Document, Point};
    #[test]
    fn natural_media_document_history_captures_paper_and_preserves_strokes() {
        let mut d = Document::default();
        let p = PaperSettings {
            enabled: true,
            kind: PaperKind::Canvas,
            ..Default::default()
        };
        d.set_paper(p).unwrap();
        let b = Brush {
            material: MaterialSettings {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        d.begin(Point { x: 20., y: 20. }, b).unwrap();
        d.finish();
        let state = d.document_state();
        assert_eq!(state.strokes[0].brush.material.paper, p);
        let json = serde_json::to_string(&state).unwrap();
        let loaded = Document::from_document_state(serde_json::from_str(&json).unwrap()).unwrap();
        assert_eq!(loaded.paper(), p);
        d.undo();
        d.undo();
        assert!(!d.paper().enabled);
        d.redo();
        assert_eq!(d.paper(), p);
        let revision = d.revision();
        assert!(d
            .set_paper(PaperSettings {
                scale: f32::INFINITY,
                ..p
            })
            .is_err());
        assert_eq!(d.revision(), revision);
        assert_eq!(d.paper(), p);
    }
}
