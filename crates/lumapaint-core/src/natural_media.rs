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
/// Number of samples along each pixel axis for a transformed texture.
/// The bounded grid keeps work predictable for extreme reductions.
pub fn texture_samples(t: [f32; 6], pixel_size: f32) -> u32 {
    texture_samples_for_footprint(texture_footprint(t, pixel_size))
}
pub fn texture_samples_for_footprint(footprint: f32) -> u32 {
    let limit = if footprint > 32. { 12. } else { 8. };
    (footprint - 0.0001).ceil().clamp(1., limit) as u32
}
pub fn texture_footprint(t: [f32; 6], pixel_size: f32) -> f32 {
    t[0].hypot(t[1]).max(t[2].hypot(t[3])) * pixel_size
}
/// Deterministic stratification avoids repeatedly sampling the same noise cells
/// when an output pixel spans more source pixels than the bounded sample grid.
pub fn texture_sample_offset(i: u32, j: u32, n: u32, jitter: bool) -> (f32, f32) {
    let (jx, jy) = if jitter {
        (
            ((i * 3 + j * 5) % 7) as f32 - 3.,
            ((i * 5 + j * 3) % 7) as f32 - 3.,
        )
    } else {
        (0., 0.)
    };
    (
        (i as f32 + 0.5 + jx / 8.) / n as f32 - 0.5,
        (j as f32 + 0.5 + jy / 8.) / n as f32 - 0.5,
    )
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
        self.color_at_footprint(x, y, [1., 1.])
    }
    pub fn color_at_footprint(self, x: f32, y: f32, pixel_size: [f32; 2]) -> [u8; 3] {
        if !self.enabled {
            return [255; 3];
        }
        if self.depth == 0. || self.kind == PaperKind::Smooth {
            let shade = 1. - self.depth * 0.07;
            return self.color.map(|v| (v as f32 * shade).round() as u8);
        }
        let size = pixel_size[0].max(pixel_size[1]);
        let n = texture_samples(self.texture_transform, size);
        let jitter = texture_footprint(self.texture_transform, size) > 8.;
        let mut sum = 0.;
        for j in 0..n {
            for i in 0..n {
                let (ox, oy) = texture_sample_offset(i, j, n, jitter);
                let (px, py) = self.texture_point(x + ox * pixel_size[0], y + oy * pixel_size[1]);
                sum += relief(px, py, self.packed());
            }
        }
        let shade = 1. - self.depth * 0.14 * (1. - sum / (n * n) as f32);
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
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the CPU reference parameters aligned with natural_media.wgsl"
)]
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
    fn large_reductions_average_more_source_detail_and_avoid_grid_aliasing() {
        let paper = PaperSettings {
            enabled: true,
            kind: PaperKind::Washi,
            depth: 1.,
            ..Default::default()
        };
        for factor in [8_u32, 16, 32, 64, 128] {
            let transform = [factor as f32, 0., 0., factor as f32, 0., 0.];
            let mut new_error = 0.;
            let mut old_error = 0.;
            let mut previous_error = 0.;
            for y in 0..8 {
                for x in 0..8 {
                    let origin_x = (x * factor) as f32;
                    let origin_y = (y * factor) as f32;
                    let mut exact = 0.;
                    for j in 0..factor {
                        for i in 0..factor {
                            exact += relief(
                                origin_x + i as f32 + 0.5,
                                origin_y + j as f32 + 0.5,
                                paper.packed(),
                            );
                        }
                    }
                    exact /= (factor * factor) as f32;
                    let sample = |n: u32, jitter: bool| {
                        let mut value = 0.;
                        for j in 0..n {
                            for i in 0..n {
                                let (dx, dy) = texture_sample_offset(i, j, n, jitter);
                                value += relief(
                                    origin_x + (0.5 + dx) * factor as f32,
                                    origin_y + (0.5 + dy) * factor as f32,
                                    paper.packed(),
                                );
                            }
                        }
                        value / (n * n) as f32
                    };
                    let n = texture_samples(transform, 1.);
                    new_error += (sample(n, factor > 8) - exact).powi(2);
                    old_error += (sample(4, false) - exact).powi(2);
                    previous_error += (sample(8, factor > 8) - exact).powi(2);
                }
            }
            assert!(
                new_error < old_error,
                "factor {factor}: {new_error} >= {old_error}"
            );
            if factor > 32 {
                assert!(
                    new_error < previous_error,
                    "factor {factor}: {new_error} >= {previous_error}"
                );
            }
        }
    }
    #[test]
    fn extreme_reductions_improve_pencil_and_dry_brush_average() {
        for factor in [64_u32, 128] {
            for texture in [1., 2.] {
                let mut new_error = 0.;
                let mut old_error = 0.;
                for y in 0..8 {
                    for x in 0..8 {
                        let origin = [(x * factor) as f32, (y * factor) as f32];
                        let mut exact = 0.;
                        for j in 0..factor {
                            for i in 0..factor {
                                exact += crate::tiles::brush_texture(
                                    origin[0] + i as f32 + 0.5,
                                    origin[1] + j as f32 + 0.5,
                                    texture,
                                );
                            }
                        }
                        exact /= (factor * factor) as f32;
                        let sample = |n| {
                            let mut sum = 0.;
                            for j in 0..n {
                                for i in 0..n {
                                    let (dx, dy) = texture_sample_offset(i, j, n, true);
                                    sum += crate::tiles::brush_texture(
                                        origin[0] + (0.5 + dx) * factor as f32,
                                        origin[1] + (0.5 + dy) * factor as f32,
                                        texture,
                                    );
                                }
                            }
                            sum / (n * n) as f32
                        };
                        new_error += (sample(12) - exact).powi(2);
                        old_error += (sample(8) - exact).powi(2);
                    }
                }
                assert!(
                    new_error < old_error,
                    "factor {factor}, texture {texture}: {new_error} >= {old_error}"
                );
            }
        }
    }
    #[test]
    fn reduced_paper_averages_the_original_pixel_footprint() {
        for kind in [
            PaperKind::ColdPress,
            PaperKind::Rough,
            PaperKind::Canvas,
            PaperKind::Washi,
        ] {
            let original = PaperSettings {
                enabled: true,
                kind,
                ..Default::default()
            };
            let mut reduced = original;
            reduced
                .transform_texture([0.5, 0., 0., 0.5, 0., 0.])
                .unwrap();
            for (x, y) in [(10.5, 13.5), (35.5, 70.5), (80.5, 55.5)] {
                let mut sum = 0.;
                for dy in [-0.25, 0.25] {
                    for dx in [-0.25, 0.25] {
                        sum += relief((x + dx) * 2., (y + dy) * 2., original.packed());
                    }
                }
                let shade = 1. - original.depth * 0.14 * (1. - sum / 4.);
                assert_eq!(
                    reduced.color_at(x, y),
                    original.color.map(|v| (v as f32 * shade).round() as u8)
                );
            }
        }
        assert_eq!(texture_samples([1., 0., 0., 1., 0., 0.], 1.), 1);
        assert_eq!(texture_samples([8., 0., 0., 8., 0., 0.], 1.), 8);
    }
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
