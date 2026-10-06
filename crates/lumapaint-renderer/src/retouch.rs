//! Local premultiplied-RGBA filters. Every dab reads an immutable patch and
//! writes only its brush footprint; GPU failures use the identical CPU kernel.
#[path = "retouch_gpu.rs"]
mod gpu;
use lumapaint_core::{
    document::{Brush, Point, Selection},
    retouch::{Kind, Settings},
    tiles::{TileCoord, TileUpload, TILE_SIZE},
};
use std::collections::BTreeSet;
pub struct Preview {
    size: (u32, u32),
    pixels: Vec<u8>,
    source: Vec<u8>,
    brush: Brush,
    settings: Settings,
    kind: Kind,
    selection: Option<Selection>,
    dirty: BTreeSet<TileCoord>,
    preserve_alpha: bool,
    last: Option<Point>,
}
impl Preview {
    pub fn new(
        size: (u32, u32),
        pixels: Vec<u8>,
        source: Vec<u8>,
        brush: Brush,
        settings: Settings,
        kind: Kind,
        selection: Option<Selection>,
    ) -> Result<Self, String> {
        let len = crate::vector::document_rgba_len(size.0, size.1)?;
        brush.validate()?;
        settings.validate()?;
        if pixels.len() != len || source.len() != len {
            return Err("Invalid retouch image".into());
        }
        Ok(Self {
            size,
            pixels,
            source,
            brush,
            settings,
            kind,
            selection,
            dirty: BTreeSet::new(),
            preserve_alpha: false,
            last: None,
        })
    }
    pub fn with_alpha_lock(mut self, locked: bool) -> Self {
        self.preserve_alpha = locked;
        self
    }
    pub fn sample(&mut self, p: Point, pressure: f32) -> Result<(), String> {
        if !p.x.is_finite() || !p.y.is_finite() || !pressure.is_finite() {
            return Err("Invalid retouch pointer".into());
        }
        let pressure = pressure.clamp(0., 1.);
        if self.settings.strength == 0. || pressure == 0. {
            self.last = Some(p);
            return Ok(());
        }
        if let Some(last) = self.last {
            let distance = (p.x - last.x).hypot(p.y - last.y);
            if distance < 0.001 {
                return Ok(());
            }
            let spacing = (self.brush.size * 0.1).max(1.);
            let steps = (distance / spacing).ceil().min(32768.) as u32;
            let mut prev = last;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let q = Point {
                    x: last.x + (p.x - last.x) * t,
                    y: last.y + (p.y - last.y) * t,
                };
                self.dab(q, prev, pressure, false);
                prev = q;
            }
        } else {
            self.dab(p, p, pressure, true);
        }
        self.last = Some(p);
        Ok(())
    }
    fn dab(&mut self, p: Point, previous: Point, pressure: f32, first: bool) {
        if self.kind == Kind::Smudge && first && !self.settings.finger_painting {
            return;
        }
        let radius = (self.brush.size
            * 0.5
            * if self.settings.pressure_size {
                pressure
            } else {
                1.
            })
        .max(0.25);
        let left = (p.x - radius - 1.).floor().max(0.).min(self.size.0 as f32) as u32;
        let top = (p.y - radius - 1.).floor().max(0.).min(self.size.1 as f32) as u32;
        let right = (p.x + radius + 1.).ceil().max(0.).min(self.size.0 as f32) as u32;
        let bottom = (p.y + radius + 1.).ceil().max(0.).min(self.size.1 as f32) as u32;
        if left >= right || top >= bottom {
            return;
        }
        let dx = previous.x - p.x;
        let dy = previous.y - p.y;
        let halo = dx.abs().max(dy.abs()).ceil() as u32 + 2;
        let origin = [left.saturating_sub(halo), top.saturating_sub(halo)];
        let extent = [
            right.saturating_add(halo).min(self.size.0) - origin[0],
            bottom.saturating_add(halo).min(self.size.1) - origin[1],
        ];
        let mut source = Vec::with_capacity((extent[0] * extent[1]) as usize);
        for y in origin[1]..origin[1] + extent[1] {
            for x in origin[0]..origin[0] + extent[0] {
                let at = ((y * self.size.0 + x) * 4) as usize;
                source.push(std::array::from_fn(|c| self.source[at + c] as f32 / 255.));
            }
        }
        let mut inputs = Vec::new();
        let mut indices = Vec::new();
        for y in top..bottom {
            for x in left..right {
                let q = Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                if self.selection.as_ref().is_some_and(|s| !s.contains(q)) {
                    continue;
                }
                let distance = (q.x - p.x).hypot(q.y - p.y) / radius;
                let coverage = if self.brush.hardness >= 0.999 {
                    ((1. - distance) * radius + 0.5).clamp(0., 1.)
                } else {
                    ((1. - distance) / (1. - self.brush.hardness)).clamp(0., 1.)
                };
                let amount = coverage
                    * self.settings.strength
                    * if self.settings.pressure_strength {
                        pressure
                    } else {
                        1.
                    };
                if amount <= 0. {
                    continue;
                }
                let at = ((y * self.size.0 + x) * 4) as usize;
                if self.preserve_alpha && self.pixels[at + 3] == 0 {
                    continue;
                }
                let mut v = [0.; 8];
                for (c, value) in v[..4].iter_mut().enumerate() {
                    *value = self.pixels[at + c] as f32 / 255.;
                }
                v[4] = (x - origin[0]) as f32;
                v[5] = (y - origin[1]) as f32;
                v[6] = amount;
                inputs.push(v);
                indices.push(at);
            }
        }
        let mut params = [0.; 12];
        params[0] = extent[0] as f32;
        params[1] = extent[1] as f32;
        params[2] = match self.kind {
            Kind::Blur => 0.,
            Kind::Sharpen => 1.,
            Kind::Smudge => 2.,
        };
        params[3] = f32::from(self.settings.protect_detail);
        params[4] = dx;
        params[5] = dy;
        params[6] = f32::from(first && self.settings.finger_painting && !self.brush.no_color);
        params[7] = f32::from(self.preserve_alpha);
        for c in 0..3 {
            params[8 + c] = self.brush.color[c] as f32 / 255.;
        }
        params[11] = 1.;
        let output = gpu::render(&inputs, &source, params).unwrap_or_else(|| {
            inputs
                .iter()
                .flat_map(|v| kernel(*v, &source, params))
                .collect()
        });
        for (at, rgba) in indices.into_iter().zip(output.as_chunks::<4>().0.iter()) {
            if self.pixels[at..at + 4] != *rgba {
                self.pixels[at..at + 4].copy_from_slice(rgba);
                self.source[at..at + 4].copy_from_slice(rgba);
                let index = at as u32 / 4;
                self.dirty.insert(TileCoord {
                    x: index % self.size.0 / TILE_SIZE,
                    y: index / self.size.0 / TILE_SIZE,
                });
            }
        }
    }
    pub fn take_uploads(&mut self) -> Vec<TileUpload> {
        std::mem::take(&mut self.dirty)
            .into_iter()
            .map(|coord| {
                let origin = [coord.x * TILE_SIZE, coord.y * TILE_SIZE];
                let extent = [
                    TILE_SIZE.min(self.size.0 - origin[0]),
                    TILE_SIZE.min(self.size.1 - origin[1]),
                ];
                let mut rgba = Vec::new();
                for y in origin[1]..origin[1] + extent[1] {
                    let at = ((y * self.size.0 + origin[0]) * 4) as usize;
                    rgba.extend_from_slice(&self.pixels[at..at + extent[0] as usize * 4]);
                }
                TileUpload {
                    coord,
                    origin,
                    extent,
                    bytes_per_row: extent[0] * 4,
                    pixels: rgba,
                }
            })
            .collect()
    }
    pub fn pixels_after_update(&self) -> Vec<u8> {
        self.pixels.clone()
    }
}
fn at(source: &[[f32; 4]], params: [f32; 12], x: i32, y: i32) -> [f32; 4] {
    let w = params[0] as i32;
    let h = params[1] as i32;
    source[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize]
}
fn kernel(v: [f32; 8], source: &[[f32; 4]], params: [f32; 12]) -> [u8; 4] {
    let x = v[4] as i32;
    let y = v[5] as i32;
    let center = at(source, params, x, y);
    let mut desired = [0.; 4];
    if params[2] < 1.5 {
        let weights = [1., 2., 1.];
        for oy in -1..=1 {
            for ox in -1..=1 {
                let s = at(source, params, x + ox, y + oy);
                let weight = weights[(ox + 1) as usize] * weights[(oy + 1) as usize] / 16.;
                for c in 0..4 {
                    desired[c] += s[c] * weight;
                }
            }
        }
        if params[2] > 0.5 {
            let a = center[3];
            for c in 0..3 {
                let delta = (center[c] - desired[c]) * 1.5;
                let delta = if params[3] > 0.5 {
                    delta.clamp(-0.15 * a, 0.15 * a)
                } else {
                    delta
                };
                desired[c] = (center[c] + delta).clamp(0., a);
            }
            desired[3] = a;
        }
    } else if params[6] > 0.5 {
        let a = params[11];
        for c in 0..3 {
            desired[c] = params[8 + c] * a;
        }
        desired[3] = a;
    } else {
        let px = v[4] + params[4];
        let py = v[5] + params[5];
        let fx = px - px.floor();
        let fy = py - py.floor();
        for (ox, oy, weight) in [
            (0, 0, (1. - fx) * (1. - fy)),
            (1, 0, fx * (1. - fy)),
            (0, 1, (1. - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let s = at(
                source,
                params,
                px.floor() as i32 + ox,
                py.floor() as i32 + oy,
            );
            for c in 0..4 {
                desired[c] += s[c] * weight;
            }
        }
    }
    if params[7] > 0.5 {
        for c in 0..3 {
            desired[c] = if desired[3] > 0. {
                (desired[c] / desired[3]).clamp(0., 1.) * v[3]
            } else {
                v[c]
            };
        }
        desired[3] = v[3];
    }
    std::array::from_fn(|c| {
        ((v[c] + (desired[c] - v[c]) * v[6]).clamp(0., 1.) * 255.).round() as u8
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn brush() -> Brush {
        Brush {
            size: 8.,
            hardness: 1.,
            color: [255, 0, 0],
            ..Brush::default()
        }
    }
    fn edge() -> Vec<u8> {
        (0..16 * 8)
            .flat_map(|i| {
                let v = if i % 16 < 8 { 64 } else { 192 };
                [v, v, v, 255]
            })
            .collect()
    }
    fn preview(kind: Kind, settings: Settings) -> Preview {
        let pixels = edge();
        Preview::new(
            (16, 8),
            pixels.clone(),
            pixels,
            brush(),
            settings,
            kind,
            None,
        )
        .unwrap()
    }
    #[test]
    fn blur_reduces_edge_contrast_and_sharpen_increases_it_without_changing_alpha() {
        let p = Point { x: 8., y: 4. };
        let settings = Settings {
            strength: 1.,
            pressure_strength: false,
            ..Settings::default()
        };
        let mut blur = preview(Kind::Blur, settings);
        blur.sample(p, 1.).unwrap();
        assert!(blur.pixels[(3 * 16 + 7) * 4] > 64);
        assert!(blur.pixels[(3 * 16 + 8) * 4] < 192);
        assert!(blur.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        assert!(!blur.take_uploads().is_empty());
        let mut sharp = preview(Kind::Sharpen, settings);
        sharp.sample(p, 1.).unwrap();
        assert!(sharp.pixels[(3 * 16 + 7) * 4] < 64);
        assert!(sharp.pixels[(3 * 16 + 8) * 4] > 192);
        assert!(sharp.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }
    #[test]
    fn smudge_moves_color_and_finger_painting_seeds_foreground() {
        let settings = Settings {
            strength: 1.,
            pressure_strength: false,
            ..Settings::default()
        };
        let mut smear = preview(Kind::Smudge, settings);
        let original = smear.pixels.clone();
        smear.sample(Point { x: 6., y: 4. }, 1.).unwrap();
        assert_eq!(smear.pixels, original);
        smear.sample(Point { x: 11., y: 4. }, 1.).unwrap();
        assert!(smear.pixels[(3 * 16 + 9) * 4] < 192);
        let mut finger = preview(
            Kind::Smudge,
            Settings {
                finger_painting: true,
                ..settings
            },
        );
        finger.sample(Point { x: 8., y: 4. }, 1.).unwrap();
        assert_eq!(
            &finger.pixels[(3 * 16 + 8) * 4..(3 * 16 + 8) * 4 + 4],
            &[255, 0, 0, 255]
        );
    }
    #[test]
    fn selection_pressure_zero_and_invalid_input_preserve_pixels() {
        use lumapaint_core::document::SelectionShape;
        let mut p = preview(Kind::Blur, Settings::default());
        p.selection = Some(Selection::new(SelectionShape::Rectangle, [0., 0., 8., 8.]));
        let before = p.pixels.clone();
        p.sample(Point { x: 8., y: 4. }, 0.).unwrap();
        assert_eq!(p.pixels, before);
        assert!(p.sample(Point { x: f32::NAN, y: 0. }, 1.).is_err());
        assert_eq!(p.pixels, before);
        p.last = None;
        p.sample(Point { x: 8., y: 4. }, 1.).unwrap();
        for y in 0..8 {
            assert_eq!(
                &p.pixels[(y * 16 + 8) * 4..(y * 16 + 16) * 4],
                &before[(y * 16 + 8) * 4..(y * 16 + 16) * 4]
            );
        }
    }
    #[test]
    fn transparent_color_does_not_leak_and_zero_strength_is_noop() {
        let pixels = vec![0; 16 * 8 * 4];
        let mut p = Preview::new(
            (16, 8),
            pixels.clone(),
            pixels.clone(),
            brush(),
            Settings::default(),
            Kind::Blur,
            None,
        )
        .unwrap();
        p.sample(Point { x: 8., y: 4. }, 1.).unwrap();
        assert_eq!(p.pixels, pixels);
        assert!(p.take_uploads().is_empty());
        let mut zero = preview(
            Kind::Sharpen,
            Settings {
                strength: 0.,
                ..Settings::default()
            },
        );
        let before = zero.pixels.clone();
        zero.sample(Point { x: 8., y: 4. }, 1.).unwrap();
        assert_eq!(zero.pixels, before);
    }
    #[test]
    fn alpha_lock_preserves_transparency_with_composite_samples_for_all_tools() {
        for kind in [Kind::Blur, Kind::Sharpen, Kind::Smudge] {
            for sample_all in [false, true] {
                let pixels: Vec<u8> = (0..128)
                    .flat_map(|i| {
                        let a = [0u8, 64, 128, 255][i % 4];
                        let v = if i % 16 < 10 {
                            a / 4
                        } else {
                            (u16::from(a) * 3 / 4) as u8
                        };
                        [v, v, v, a]
                    })
                    .collect();
                let source = if sample_all { edge() } else { pixels.clone() };
                let mut p = Preview::new(
                    (16, 8),
                    pixels.clone(),
                    source,
                    brush(),
                    Settings {
                        strength: 1.,
                        pressure_strength: false,
                        finger_painting: true,
                        sample_all_layers: sample_all,
                        ..Settings::default()
                    },
                    kind,
                    None,
                )
                .unwrap()
                .with_alpha_lock(true);
                p.sample(Point { x: 8., y: 4. }, 1.).unwrap();
                p.sample(Point { x: 11., y: 4. }, 1.).unwrap();
                assert!(p.pixels != pixels, "kind={kind:?}, all={sample_all}");
                for (after, before) in p
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(pixels.as_chunks::<4>().0)
                {
                    assert_eq!(after[3], before[3]);
                    assert!(after[..3].iter().all(|c| *c <= after[3]));
                    if before[3] == 0 {
                        assert_eq!(after, before);
                    }
                }
            }
        }
    }
    #[test]
    #[ignore = "requires hardware GPU"]
    fn gpu_retouch_kernel_matches_cpu_for_filters_alpha_and_finger_painting() {
        let source: Vec<[f32; 4]> = (0..64)
            .map(|i| {
                let a = if i % 3 == 0 { 0.4 } else { 1. };
                [i as f32 / 64. * a, 0.3 * a, 0.6 * a, a]
            })
            .collect();
        let inputs: Vec<[f32; 8]> = (0..64)
            .map(|i| {
                let s = source[i];
                [
                    s[0],
                    s[1],
                    s[2],
                    s[3],
                    (i % 8) as f32,
                    (i / 8) as f32,
                    0.7,
                    0.,
                ]
            })
            .collect();
        for kind in 0..3 {
            for (finger, alpha_lock) in [(0., 0.), (1., 0.), (0., 1.), (1., 1.)] {
                let params = [
                    8.,
                    8.,
                    kind as f32,
                    1.,
                    -1.2,
                    0.4,
                    finger,
                    alpha_lock,
                    0.7,
                    0.2,
                    0.1,
                    1.,
                ];
                let actual = gpu::force_render(&inputs, &source, params).expect("GPU required");
                let expected: Vec<u8> = inputs
                    .iter()
                    .flat_map(|v| kernel(*v, &source, params))
                    .collect();
                if alpha_lock > 0.5 {
                    for (pixel, input) in actual.as_chunks::<4>().0.iter().zip(&inputs) {
                        assert_eq!(pixel[3], (input[3] * 255.).round() as u8);
                    }
                }
                for (a, b) in actual.iter().zip(expected) {
                    assert!(a.abs_diff(b) <= 1, "{a} vs {b}");
                }
            }
        }
    }
}
