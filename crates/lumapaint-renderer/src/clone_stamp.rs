//! Immutable stroke source, opacity-capped flow accumulation and local tile previews.
#[path = "clone_stamp_gpu.rs"]
mod gpu;
use lumapaint_core::{
    clone_stamp::{Mode, Settings},
    document::{Brush, Point, Selection},
    tiles::{TileCoord, TileUpload, TILE_SIZE},
};
use std::collections::BTreeSet;
pub struct Preview {
    size: (u32, u32),
    original: Vec<u8>,
    source: Vec<u8>,
    pixels: Vec<u8>,
    coverage: Vec<f32>,
    dirty: BTreeSet<TileCoord>,
    offset: [f32; 2],
    brush: Brush,
    settings: Settings,
    selection: Option<Selection>,
    last: Option<(Point, f32)>,
    distance_since_dab: f32,
}
impl Preview {
    pub fn new(
        size: (u32, u32),
        destination: Vec<u8>,
        source: Vec<u8>,
        offset: [f32; 2],
        brush: Brush,
        settings: Settings,
        selection: Option<Selection>,
    ) -> Result<Self, String> {
        let bytes = crate::vector::document_rgba_len(size.0, size.1)?;
        brush.validate()?;
        settings.validate()?;
        if destination.len() != bytes
            || source.len() != bytes
            || offset.iter().any(|v| !v.is_finite())
        {
            return Err("Invalid clone stamp image".into());
        }
        Ok(Self {
            size,
            pixels: destination.clone(),
            original: destination,
            source,
            coverage: vec![0.; bytes / 4],
            dirty: BTreeSet::new(),
            offset,
            brush,
            settings,
            selection,
            last: None,
            distance_since_dab: 0.,
        })
    }
    pub fn sample(&mut self, point: Point, pressure: f32) -> Result<(), String> {
        if !point.x.is_finite() || !point.y.is_finite() || !pressure.is_finite() {
            return Err("Invalid clone stamp pointer".into());
        }
        let pressure = pressure.clamp(0., 1.);
        if let Some((last, p)) = self.last {
            let distance = (point.x - last.x).hypot(point.y - last.y);
            let spacing = (self.brush.size * 0.15).max(0.5);
            if distance / spacing > 200_000. {
                return Err("Clone stroke exceeds sampling limit".into());
            }
            let mut along = spacing - self.distance_since_dab;
            while along <= distance && distance > 0. {
                let t = along / distance;
                self.dab(
                    Point {
                        x: last.x + (point.x - last.x) * t,
                        y: last.y + (point.y - last.y) * t,
                    },
                    p + (pressure - p) * t,
                );
                along += spacing;
            }
            self.distance_since_dab = (self.distance_since_dab + distance) % spacing;
        } else {
            self.dab(point, pressure);
        }
        self.last = Some((point, pressure));
        Ok(())
    }
    fn dab(&mut self, p: Point, pressure: f32) {
        let radius = (self.brush.size
            * 0.5
            * if self.settings.pressure_size {
                pressure
            } else {
                1.
            })
        .max(0.25);
        let angle = self.settings.angle.to_radians();
        let (sin, cos) = angle.sin_cos();
        let left = (p.x - radius - 1.).floor().max(0.) as u32;
        let top = (p.y - radius - 1.).floor().max(0.) as u32;
        let right = (p.x + radius + 1.).ceil().max(0.).min(self.size.0 as f32) as u32;
        let bottom = (p.y + radius + 1.).ceil().max(0.).min(self.size.1 as f32) as u32;
        for y in top..bottom {
            for x in left..right {
                let position = Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                if self
                    .selection
                    .as_ref()
                    .is_some_and(|s| !s.contains(position))
                {
                    continue;
                }
                let dx = position.x - p.x;
                let dy = position.y - p.y;
                let distance = ((dx * cos + dy * sin) / radius)
                    .hypot((-dx * sin + dy * cos) / (radius * self.settings.roundness));
                let soft = if self.brush.hardness >= 0.999 {
                    ((1. - distance) * radius + 0.5).clamp(0., 1.)
                } else {
                    ((1. - distance) / (1. - self.brush.hardness)).clamp(0., 1.)
                };
                let a = soft
                    * self.settings.flow
                    * if self.settings.pressure_opacity {
                        pressure
                    } else {
                        1.
                    };
                if a <= 0. {
                    continue;
                }
                let index = (y * self.size.0 + x) as usize;
                let old = self.coverage[index];
                self.coverage[index] = 1. - (1. - old) * (1. - a);
                if self.coverage[index] != old {
                    self.dirty.insert(TileCoord {
                        x: x / TILE_SIZE,
                        y: y / TILE_SIZE,
                    });
                }
            }
        }
    }
    fn source_at(&self, x: u32, y: u32) -> [f32; 4] {
        let px = x as f32 + self.offset[0];
        let py = y as f32 + self.offset[1];
        let x0 = px.floor() as i32;
        let y0 = py.floor() as i32;
        let fx = px - px.floor();
        let fy = py - py.floor();
        let mut result = [0.; 4];
        for (dx, dy, w) in [
            (0, 0, (1. - fx) * (1. - fy)),
            (1, 0, fx * (1. - fy)),
            (0, 1, (1. - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let x = x0 + dx;
            let y = y0 + dy;
            if x < 0 || y < 0 || x >= self.size.0 as i32 || y >= self.size.1 as i32 {
                continue;
            }
            let at = (y as usize * self.size.0 as usize + x as usize) * 4;
            for (c, v) in result.iter_mut().enumerate() {
                *v += self.source[at + c] as f32 / 255. * w;
            }
        }
        result
    }
    pub fn take_uploads(&mut self) -> Vec<TileUpload> {
        let mut uploads = Vec::new();
        for coord in std::mem::take(&mut self.dirty) {
            let origin = [coord.x * TILE_SIZE, coord.y * TILE_SIZE];
            let extent = [
                TILE_SIZE.min(self.size.0 - origin[0]),
                TILE_SIZE.min(self.size.1 - origin[1]),
            ];
            let mut values = Vec::with_capacity((extent[0] * extent[1]) as usize);
            let mode = match self.settings.mode {
                Mode::Normal => 0.,
                Mode::Multiply => 1.,
                Mode::Screen => 2.,
                Mode::Overlay => 3.,
                Mode::Darken => 4.,
                Mode::Lighten => 5.,
            };
            for y in origin[1]..origin[1] + extent[1] {
                for x in origin[0]..origin[0] + extent[0] {
                    let index = (y * self.size.0 + x) as usize;
                    let mut value = [0.; 12];
                    for (c, v) in value[..4].iter_mut().enumerate() {
                        *v = self.original[index * 4 + c] as f32 / 255.;
                    }
                    value[4..8].copy_from_slice(&self.source_at(x, y));
                    value[8] = self.coverage[index] * self.settings.opacity;
                    value[9] = mode;
                    values.push(value);
                }
            }
            let pixels = gpu::render(&values).unwrap_or_else(|| {
                values
                    .iter()
                    .flat_map(|v| {
                        blend(
                            v[..4].try_into().unwrap(),
                            v[4..8].try_into().unwrap(),
                            v[8],
                            self.settings.mode,
                        )
                    })
                    .collect()
            });
            for (row, y) in (origin[1]..origin[1] + extent[1]).enumerate() {
                let at = ((y * self.size.0 + origin[0]) * 4) as usize;
                let bytes = extent[0] as usize * 4;
                self.pixels[at..at + bytes]
                    .copy_from_slice(&pixels[row * bytes..(row + 1) * bytes]);
            }
            uploads.push(TileUpload {
                coord,
                origin,
                extent,
                bytes_per_row: extent[0] * 4,
                pixels,
            });
        }
        uploads
    }
    pub fn pixels_after_update(&mut self) -> Vec<u8> {
        self.take_uploads();
        self.pixels.clone()
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}
fn blend(dst: [f32; 4], src: [f32; 4], amount: f32, mode: Mode) -> [u8; 4] {
    let sa = src[3] * amount;
    let da = dst[3];
    let alpha = sa + da * (1. - sa);
    let mut result = [0; 4];
    for c in 0..3 {
        let s = if src[3] > 0. { src[c] / src[3] } else { 0. };
        let d = if da > 0. { dst[c] / da } else { 0. };
        let b = match mode {
            Mode::Normal => s,
            Mode::Multiply => s * d,
            Mode::Screen => 1. - (1. - s) * (1. - d),
            Mode::Overlay => {
                if d <= 0.5 {
                    2. * s * d
                } else {
                    1. - 2. * (1. - s) * (1. - d)
                }
            }
            Mode::Darken => s.min(d),
            Mode::Lighten => s.max(d),
        };
        let color = dst[c] * (1. - sa) + sa * ((1. - da) * s + da * b);
        result[c] = (color * 255.).round().clamp(0., 255.) as u8;
    }
    result[3] = (alpha * 255.).round().clamp(0., 255.) as u8;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn brush() -> Brush {
        Brush {
            size: 4.,
            hardness: 1.,
            ..Default::default()
        }
    }
    fn image() -> Vec<u8> {
        (0..16 * 8)
            .flat_map(|i| {
                if i % 16 < 8 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect()
    }
    fn at(p: &[u8], x: usize, y: usize) -> &[u8] {
        &p[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]
    }
    #[test]
    fn immutable_source_and_stroke_opacity_cap() {
        let pixels = image();
        let mut preview = Preview::new(
            (16, 8),
            pixels.clone(),
            pixels,
            [-8., 0.],
            brush(),
            Settings {
                opacity: 0.5,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        preview.sample(Point { x: 10.5, y: 3.5 }, 1.).unwrap();
        preview.sample(Point { x: 12.5, y: 3.5 }, 1.).unwrap();
        preview.sample(Point { x: 10.5, y: 3.5 }, 1.).unwrap();
        assert_eq!(
            at(&preview.pixels_after_update(), 10, 3),
            [128, 0, 128, 255]
        );
        assert_eq!(at(preview.pixels(), 0, 0), [255, 0, 0, 255]);
    }
    #[test]
    fn transparent_source_does_not_erase_and_invalid_inputs_leave_pixels() {
        let original = image();
        let mut preview = Preview::new(
            (16, 8),
            original.clone(),
            vec![0; 512],
            [0., 0.],
            brush(),
            Settings::default(),
            None,
        )
        .unwrap();
        preview.sample(Point { x: 10.5, y: 3.5 }, 1.).unwrap();
        assert_eq!(preview.pixels_after_update(), original);
        assert!(preview.sample(Point { x: f32::NAN, y: 0. }, 1.).is_err());
        assert_eq!(preview.pixels(), original);
    }
    #[test]
    fn flow_is_independent_of_pointer_event_frequency() {
        let make = || {
            Preview::new(
                (16, 8),
                image(),
                image(),
                [-8., 0.],
                brush(),
                Settings {
                    flow: 0.2,
                    ..Default::default()
                },
                None,
            )
            .unwrap()
        };
        let mut coarse = make();
        let mut fine = make();
        for p in [&mut coarse, &mut fine] {
            p.sample(Point { x: 9., y: 3. }, 1.).unwrap();
        }
        coarse.sample(Point { x: 14., y: 3. }, 1.).unwrap();
        for x in 1..=10 {
            fine.sample(
                Point {
                    x: 9. + x as f32 * 0.5,
                    y: 3.,
                },
                1.,
            )
            .unwrap();
        }
        assert_eq!(coarse.pixels_after_update(), fine.pixels_after_update());
    }
    #[test]
    fn selection_clips_destination_without_clipping_source() {
        let selection = Selection::new(
            lumapaint_core::document::SelectionShape::Rectangle,
            [10., 2., 1., 3.],
        );
        let mut preview = Preview::new(
            (16, 8),
            image(),
            image(),
            [-8., 0.],
            brush(),
            Settings::default(),
            Some(selection),
        )
        .unwrap();
        preview.sample(Point { x: 10.5, y: 3.5 }, 1.).unwrap();
        let pixels = preview.pixels_after_update();
        assert_eq!(at(&pixels, 10, 3), [255, 0, 0, 255]);
        assert_eq!(at(&pixels, 11, 3), [0, 0, 255, 255]);
    }
    #[test]
    fn blending_preserves_premultiplied_alpha_and_modes() {
        assert_eq!(
            blend([0., 0., 0., 0.], [0.5, 0., 0., 0.5], 0.5, Mode::Normal),
            [64, 0, 0, 64]
        );
        assert_eq!(
            blend([1., 1., 1., 1.], [0.5, 0., 0., 1.], 1., Mode::Multiply),
            [128, 0, 0, 255]
        );
        assert_eq!(
            blend([0., 0., 0., 1.], [0.5, 0., 0., 1.], 1., Mode::Screen),
            [128, 0, 0, 255]
        );
    }
}
#[cfg(test)]
mod gpu_tests {
    use super::*;
    #[test]
    fn gpu_stamp_blending_matches_cpu_for_alpha_and_all_modes() {
        let mut values = Vec::new();
        let mut expected = Vec::new();
        for mode in [
            Mode::Normal,
            Mode::Multiply,
            Mode::Screen,
            Mode::Overlay,
            Mode::Darken,
            Mode::Lighten,
        ] {
            let number = match mode {
                Mode::Normal => 0.,
                Mode::Multiply => 1.,
                Mode::Screen => 2.,
                Mode::Overlay => 3.,
                Mode::Darken => 4.,
                Mode::Lighten => 5.,
            };
            for i in 0..1024 {
                let alpha = (i % 251) as f32 / 255.;
                let dst = [alpha * 0.25, alpha * 0.7, alpha * 0.9, alpha];
                let src = [0.4, 0.2, 0.1, 0.5];
                let amount = (i % 101) as f32 / 100.;
                let mut value = [0.; 12];
                value[..4].copy_from_slice(&dst);
                value[4..8].copy_from_slice(&src);
                value[8] = amount;
                value[9] = number;
                values.push(value);
                expected.extend(blend(dst, src, amount, mode));
            }
        }
        if let Some(actual) = gpu::render(&values) {
            for (a, b) in actual.iter().zip(expected) {
                assert!(a.abs_diff(b) <= 1, "GPU {a} vs CPU {b}");
            }
        } else {
            eprintln!("GPU unavailable: CPU fallback verified by portable tests");
        }
    }
}

pub(crate) fn mode_number(mode: Mode) -> f32 {
    match mode {
        Mode::Normal => 0.,
        Mode::Multiply => 1.,
        Mode::Screen => 2.,
        Mode::Overlay => 3.,
        Mode::Darken => 4.,
        Mode::Lighten => 5.,
    }
}
pub(crate) fn blend_batch(values: &[[f32; 12]], mode: Mode) -> Vec<u8> {
    gpu::render(values).unwrap_or_else(|| {
        values
            .iter()
            .flat_map(|v| {
                blend(
                    v[..4].try_into().unwrap(),
                    v[4..8].try_into().unwrap(),
                    v[8],
                    mode,
                )
            })
            .collect()
    })
}
