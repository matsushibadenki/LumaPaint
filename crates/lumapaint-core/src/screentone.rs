//! Editable procedural manga tones. Coordinates are always document pixels.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u32)]
pub enum ToneKind {
    Dot,
    Gradient,
    Line,
    Pattern,
    Cg,
    Sand,
    Crosshatch,
    Effect,
    Background,
    White,
    Transfer,
    Color,
    Copy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Screentone {
    pub kind: ToneKind,
    pub variant: u32,
    pub frequency: f32,
    pub dpi: f32,
    pub density: f32,
    pub size: f32,
    pub angle: f32,
    pub offset: [f32; 2],
    pub gradient_end: f32,
    pub extent: f32,
    pub seed: u32,
    pub color: [u8; 3],
    pub paper: bool,
    pub luminance: bool,
    pub inverted: bool,
}
impl Default for Screentone {
    fn default() -> Self {
        Self {
            kind: ToneKind::Dot,
            variant: 0,
            frequency: 60.,
            dpi: 300.,
            density: 30.,
            size: 1.,
            angle: 45.,
            offset: [0.; 2],
            gradient_end: 0.,
            extent: 600.,
            seed: 1,
            color: [0; 3],
            paper: false,
            luminance: false,
            inverted: false,
        }
    }
}
impl Screentone {
    pub fn validate(&self) -> Result<(), String> {
        if self.variant > 3
            || self.seed > 65535
            || ![
                (self.frequency, 1., 150.),
                (self.dpi, 1., 1200.),
                (self.density, 0., 100.),
                (self.size, 0.1, 4.),
                (self.angle, -360., 360.),
                (self.offset[0], -100000., 100000.),
                (self.offset[1], -100000., 100000.),
                (self.gradient_end, 0., 100.),
                (self.extent, 1., 100000.),
            ]
            .iter()
            .all(|(v, a, b)| v.is_finite() && (*a..=*b).contains(v))
        {
            return Err("Invalid screentone settings".into());
        }
        Ok(())
    }
    /// Straight-alpha output; source alpha acts as the editable tone mask.
    pub fn apply(&self, pixel: [u8; 4], point: [f32; 2]) -> [u8; 4] {
        if pixel[3] == 0 {
            return pixel;
        }
        let [x, y] = [point[0] - self.offset[0], point[1] - self.offset[1]];
        let (s, c) = self.angle.to_radians().sin_cos();
        let period = self.dpi / self.frequency;
        let u = (x * c + y * s) / period;
        let v = (-x * s + y * c) / period;
        let a = (u - floor(u) - 0.5).abs();
        let b = (v - floor(v) - 0.5).abs();
        let n = self.variant;
        let mut d = self.density / 100.;
        if self.kind == ToneKind::Gradient {
            let t = match n {
                1 => (x * x + y * y).sqrt() / self.extent,
                2 => (x * c + y * s).abs() / self.extent,
                3 => ((x * c + y * s) / self.extent * std::f32::consts::PI).sin() * 0.5 + 0.5,
                _ => (x * c + y * s) / self.extent,
            }
            .clamp(0., 1.);
            d = d * (1. - t) + self.gradient_end / 100. * t;
        }
        if self.luminance || self.kind == ToneKind::Copy {
            d *= 1.
                - (pixel[0] as f32 * 0.2126 + pixel[1] as f32 * 0.7152 + pixel[2] as f32 * 0.0722)
                    / 255.;
        }
        if self.inverted {
            d = 1. - d;
        }
        let dot = match n {
            1 => 4. * a.max(b).powi(2),
            2 => 2. * (a + b).powi(2),
            3 => (a * a * 2. + b * b * 6.).min(1.),
            _ => std::f32::consts::PI * (a * a + b * b),
        };
        let line = 2. * b;
        let noise = hash(floor(u) as i32 as u32, floor(v) as i32 as u32, self.seed);
        let score = match self.kind {
            ToneKind::Dot | ToneKind::Color | ToneKind::Copy | ToneKind::White => dot,
            ToneKind::Gradient => std::f32::consts::PI * (a * a + b * b),
            ToneKind::Line => match n {
                1 => (2. * (v + 0.2 * (u * 1.5).sin()).fract_euclid() - 1.).abs(),
                2 => line.max(if u.fract_euclid() < 0.65 { 0. } else { 1. }),
                3 => (line - 0.45).abs() * 2.,
                _ => line,
            },
            ToneKind::Pattern => match n {
                1 => 2. * (a + b - 0.35).abs(),
                2 => {
                    let theta = (v.fract_euclid() - 0.5).atan2(u.fract_euclid() - 0.5);
                    ((a * a + b * b).sqrt() / (0.3 + 0.12 * (theta * 5.).cos())).min(1.)
                }
                3 => ((u.floor() + v.floor()) as i32 & 1) as f32,
                _ => 2. * a.min(b),
            },
            ToneKind::Cg => match n {
                1 => ((u * 0.8).sin() * (v * 0.8).cos() + 1.) * 0.5,
                2 => ((u * u + v * v).sqrt() * 1.4).sin() * 0.5 + 0.5,
                3 => ((u * 0.4).sin() + (v * 0.6).sin() + 2.) * 0.25,
                _ => ((u + v * 0.5).sin() + 1.) * 0.5,
            },
            ToneKind::Sand => {
                let scale = (n + 1) as f32;
                hash(
                    floor(u * scale) as i32 as u32,
                    floor(v * scale) as i32 as u32,
                    self.seed,
                )
            }
            ToneKind::Crosshatch => match n {
                1 => 2. * a.min(b).min((a - b).abs()),
                2 => {
                    let jitter = noise * 0.3;
                    (2. * b + jitter).min(2. * a + 0.3 - jitter)
                }
                3 => (2. * b)
                    .max(if (u.floor() as i32 & 1) == 0 { 0. } else { 1. })
                    .min((2. * a).max(if (v.floor() as i32 & 1) == 1 { 0. } else { 1. })),
                _ => 2. * a.min(b),
            },
            ToneKind::Effect => match n {
                1 => (u * 0.8).sin().abs(),
                2 => ((u * u + v * v).sqrt()).sin().abs(),
                3 => {
                    let theta = v.atan2(u);
                    ((theta * 12.).sin().abs() + (u * u + v * v).sqrt() * 0.01).min(1.)
                }
                _ => (v.atan2(u) * 24.).sin().abs(),
            },
            ToneKind::Background => match n {
                1 => (2. * b).min(
                    (2. * (u + if (v.floor() as i32 & 1) == 0 { 0. } else { 0.5 }).fract_euclid()
                        - 1.)
                        .abs()
                        .max(0.2),
                ),
                2 => ((u * 0.2).sin() + (v * 0.3 + (u * 0.1).sin()).sin() + 2.) * 0.25,
                3 => ((v * 2. + (u * 0.3).sin()).sin()).abs(),
                _ => (2. * b).max((u * 0.3).sin().abs()),
            },
            ToneKind::Transfer => match n {
                1 => (dot + noise * 0.4).min(1.),
                2 => (line + noise * 0.5).min(1.),
                3 => (2. * a.min(b) + noise * 0.35).min(1.),
                _ => (dot * 0.7 + noise * 0.6).min(1.),
            },
        };
        let ink = d >= 1. || (d > 0. && score < d * self.size * self.size);
        let color = if self.kind == ToneKind::White {
            [255; 3]
        } else {
            self.color
        };
        if ink {
            [color[0], color[1], color[2], pixel[3]]
        } else if self.paper {
            [255, 255, 255, pixel[3]]
        } else {
            [0; 4]
        }
    }
}
fn floor(x: f32) -> f32 {
    x.floor()
}
trait EuclidFract {
    fn fract_euclid(self) -> Self;
}
impl EuclidFract for f32 {
    fn fract_euclid(self) -> Self {
        self - self.floor()
    }
}
fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(1597334677) ^ y.wrapping_mul(3812015801) ^ seed.wrapping_mul(2798796415);
    h = (h ^ (h >> 16)).wrapping_mul(2246822519);
    h ^= h >> 13;
    (h & 65535) as f32 / 65536.
}

#[cfg(test)]
mod tests {
    use super::*;
    pub const KINDS: [ToneKind; 13] = [
        ToneKind::Dot,
        ToneKind::Gradient,
        ToneKind::Line,
        ToneKind::Pattern,
        ToneKind::Cg,
        ToneKind::Sand,
        ToneKind::Crosshatch,
        ToneKind::Effect,
        ToneKind::Background,
        ToneKind::White,
        ToneKind::Transfer,
        ToneKind::Color,
        ToneKind::Copy,
    ];
    #[test]
    fn all_tone_designs_generate_distinct_masks_and_preserve_alpha() {
        for kind in KINDS {
            let mut patterns = std::collections::HashSet::new();
            for variant in 0..4 {
                let t = Screentone {
                    kind,
                    variant,
                    density: 43.,
                    angle: 17.,
                    extent: 90.,
                    offset: [11., -5.],
                    ..Default::default()
                };
                t.validate().unwrap();
                let mask: Vec<u8> = (0..128 * 128)
                    .map(|i| {
                        let out = t.apply(
                            [0, 0, 0, 137],
                            [(i % 128) as f32 + 0.5, (i / 128) as f32 + 0.5],
                        );
                        assert!(out[3] == 0 || out[3] == 137);
                        out[3]
                    })
                    .collect();
                assert!(
                    mask.contains(&0) && mask.contains(&137),
                    "{kind:?}/{variant}"
                );
                assert!(patterns.insert(mask), "duplicate {kind:?}/{variant}");
            }
        }
    }
    #[test]
    fn tone_density_endpoints_noise_and_copy_luminance() {
        let mut tone = Screentone::default();
        for kind in KINDS {
            tone.kind = kind;
            tone.gradient_end = 0.;
            tone.density = 0.;
            assert_eq!(tone.apply([0, 0, 0, 255], [50.5, 21.5]), [0; 4]);
            tone.gradient_end = 100.;
            tone.density = 100.;
            assert_eq!(tone.apply([0, 0, 0, 123], [50.5, 21.5])[3], 123);
        }
        tone.kind = ToneKind::Copy;
        assert_eq!(tone.apply([255, 255, 255, 255], [0.5, 0.5]), [0; 4]);
        tone.kind = ToneKind::Sand;
        let before = tone.apply([50, 80, 20, 128], [9.5, 7.5]);
        assert_eq!(tone.apply([50, 80, 20, 128], [9.5, 7.5]), before);
        assert_eq!(tone.apply([0, 0, 0, 0], [9.5, 7.5]), [0; 4]);
    }
    #[test]
    fn invalid_tone_settings_are_rejected_and_legacy_effects_load() {
        let mut tone = Screentone::default();
        for bad in [f32::NAN, f32::INFINITY, 0., 151.] {
            tone.frequency = bad;
            assert!(tone.validate().is_err());
        }
        tone = Screentone::default();
        tone.variant = 4;
        assert!(tone.validate().is_err());
        let e = crate::layer_effects::LayerEffects::default();
        let mut json = serde_json::to_value(&e).unwrap();
        json.as_object_mut().unwrap().remove("screentone");
        assert_eq!(
            serde_json::from_value::<crate::layer_effects::LayerEffects>(json).unwrap(),
            e
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::{
        document::Document,
        tiles::{TileCoord, TiledRasterDocument, TILE_SIZE},
        vector::{PathOperation, VectorPath, VectorPathEngine},
    };
    struct RectangleOnly;
    impl VectorPathEngine for RectangleOnly {
        fn combine(
            &self,
            _: &VectorPath,
            _: &VectorPath,
            _: PathOperation,
        ) -> Result<VectorPath, String> {
            Err("unexpected boolean operation".into())
        }
        fn contains(&self, _: &VectorPath, _: [f32; 2]) -> Result<bool, String> {
            Ok(false)
        }
    }
    #[test]
    fn screentone_layer_creation_selection_save_and_single_undo() {
        let mut d = Document::default();
        d.set_tool_pixel_selection(Some(crate::selection::Selection::new(
            crate::selection::SelectionShape::Rectangle,
            [10., 20., 80., 60.],
        )))
        .unwrap();
        let before = d.document_state();
        let selection = d.selection().cloned();
        let id = d
            .add_screentone_layer(Screentone::default(), &RectangleOnly)
            .unwrap();
        let tone = d.layer_effects(&id).screentone.unwrap();
        assert_eq!(tone.dpi, d.snapshot().resolution as f32);
        let layer = d.svg_layers().find(|l| l.id == id).unwrap();
        assert_eq!(layer.vector_objects[0].path.data, "M10 20h80v60h-80Z");
        assert!(layer.vector_objects[0].fill_gradient.is_none());
        assert!(d.selection().is_none());
        let state = d.document_state();
        let restored = Document::from_document_state(
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.layer_effects(&id).screentone, Some(tone));
        d.undo();
        assert!(d.svg_layers().all(|l| l.id != id));
        assert_eq!(d.selection(), selection.as_ref());
        assert_eq!(
            serde_json::to_value(d.document_state()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        d.redo();
        assert!(d.layer_effects(&id).screentone.is_some());
        let revision = d.revision();
        let invalid = Screentone {
            frequency: 0.,
            ..Default::default()
        };
        assert!(d.add_screentone_layer(invalid, &RectangleOnly).is_err());
        assert_eq!(d.revision(), revision);
    }
    #[test]
    fn screentone_raster_tile_boundaries_history_and_native_roundtrip() {
        let mut d = TiledRasterDocument::new(512, 2).unwrap();
        d.add_layer("tone".into(), "Tone".into()).unwrap();
        d.write_rect("tone", [0, 0, 512, 2], &[0, 0, 0, 255].repeat(1024))
            .unwrap();
        d.discard_history();
        let original = d.state();
        let tone = Screentone {
            angle: 13.,
            frequency: 37.,
            offset: [7., 3.],
            ..Default::default()
        };
        let effects = crate::layer_effects::LayerEffects {
            enabled: true,
            screentone: Some(tone.clone()),
            ..Default::default()
        };
        d.set_layer_effects("tone", effects).unwrap();
        for tx in 0..2 {
            let pixels = d.composite_tile(TileCoord { x: tx, y: 0 }).unwrap();
            for y in 0..2 {
                for x in 0..TILE_SIZE {
                    let point = [(tx * TILE_SIZE + x) as f32 + 0.5, y as f32 + 0.5];
                    let expected = tone.apply([0, 0, 0, 255], point);
                    let i = ((y * TILE_SIZE + x) * 4) as usize;
                    assert_eq!(&pixels[i..i + 4], &expected);
                }
            }
        }
        let saved = d.state();
        assert_eq!(saved.layers[0].tiles, original.layers[0].tiles);
        assert_eq!(
            TiledRasterDocument::from_state(
                serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap()
            )
            .unwrap()
            .state(),
            saved
        );
        d.undo().unwrap();
        assert_eq!(d.state(), original);
        d.redo().unwrap();
        assert_eq!(d.state(), saved);
    }
}
