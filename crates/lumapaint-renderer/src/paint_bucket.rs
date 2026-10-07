//! Region detection stays on the Rust worker; bulk color blending prefers GPU.
use lumapaint_core::{
    document::{Point, Selection},
    paint_bucket::{Pattern, Settings, Source},
    tiles::{TileCoord, TileUpload, TILE_SIZE},
};
use std::collections::{BTreeSet, VecDeque};
pub struct ResultImage {
    pub pixels: Vec<u8>,
    pub uploads: Vec<TileUpload>,
    pub changed: bool,
}
fn straight(p: &[u8]) -> [f32; 4] {
    if p[3] == 0 {
        return [0.; 4];
    }
    [
        p[0] as f32 * 255. / p[3] as f32,
        p[1] as f32 * 255. / p[3] as f32,
        p[2] as f32 * 255. / p[3] as f32,
        p[3] as f32,
    ]
}
fn difference(p: &[u8], seed: [f32; 4]) -> f32 {
    straight(p)
        .into_iter()
        .zip(seed)
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max)
}
/// Separable square morphology. Sliding extrema keep work O(width * height),
/// independent of the requested radius. No image bytes leave the worker.
fn morphology(mask: &[u8], width: usize, radius: usize, grow: bool) -> Vec<u8> {
    if radius == 0 {
        return mask.to_vec();
    }
    let height = mask.len() / width;
    let pass = |input: &[u8], length: usize, lines: usize, vertical: bool| {
        let mut output = vec![0; input.len()];
        for line in 0..lines {
            let index = |position: usize| {
                if vertical {
                    position * width + line
                } else {
                    line * width + position
                }
            };
            let mut queue: VecDeque<usize> = VecDeque::new();
            let mut end = 0;
            for position in 0..length {
                let right = (position + radius + 1).min(length);
                while end < right {
                    while queue.back().is_some_and(|last| {
                        if grow {
                            input[index(*last)] <= input[index(end)]
                        } else {
                            input[index(*last)] >= input[index(end)]
                        }
                    }) {
                        queue.pop_back();
                    }
                    queue.push_back(end);
                    end += 1;
                }
                let left = position.saturating_sub(radius);
                while queue.front().is_some_and(|first| *first < left) {
                    queue.pop_front();
                }
                output[index(position)] = input[index(*queue.front().unwrap())];
            }
        }
        output
    };
    let horizontal = pass(mask, width, height, false);
    pass(&horizontal, height, width, true)
}

pub fn fill(
    size: (u32, u32),
    original: Vec<u8>,
    sample: &[u8],
    point: Point,
    color: [u8; 3],
    settings: Settings,
    selection: Option<&Selection>,
) -> Result<ResultImage, String> {
    fill_impl(
        size, original, sample, point, color, settings, selection, false,
    )
}

/// Fill color while preserving every destination alpha byte.
pub fn fill_alpha_locked(
    size: (u32, u32),
    original: Vec<u8>,
    sample: &[u8],
    point: Point,
    color: [u8; 3],
    settings: Settings,
    selection: Option<&Selection>,
) -> Result<ResultImage, String> {
    fill_impl(
        size, original, sample, point, color, settings, selection, true,
    )
}

#[allow(clippy::too_many_arguments)]
fn fill_impl(
    size: (u32, u32),
    original: Vec<u8>,
    sample: &[u8],
    point: Point,
    color: [u8; 3],
    settings: Settings,
    selection: Option<&Selection>,
    alpha_locked: bool,
) -> Result<ResultImage, String> {
    settings.validate()?;
    let bytes = crate::vector::document_rgba_len(size.0, size.1)?;
    if original.len() != bytes
        || sample.len() != bytes
        || !point.x.is_finite()
        || !point.y.is_finite()
    {
        return Err("Invalid fill image".into());
    }
    let (w, h) = size;
    let mut pixels = original.clone();
    let empty = || ResultImage {
        pixels: original.clone(),
        uploads: Vec::new(),
        changed: false,
    };
    if point.x < 0.
        || point.y < 0.
        || point.x >= w as f32
        || point.y >= h as f32
        || settings.opacity == 0.
    {
        return Ok(empty());
    }
    let start = point.y.floor() as usize * w as usize + point.x.floor() as usize;
    let seed = straight(&sample[start * 4..start * 4 + 4]);
    let allowed = |i: usize| {
        selection.is_none_or(|s| {
            s.contains(Point {
                x: (i % w as usize) as f32 + 0.5,
                y: (i / w as usize) as f32 + 0.5,
            })
        })
    };
    if !allowed(start) {
        return Ok(empty());
    }
    let matches = |i: usize| {
        allowed(i) && difference(&sample[i * 4..i * 4 + 4], seed) <= settings.tolerance as f32
    };
    let mut mask = vec![0u8; bytes / 4];
    let neighbors = |i: usize| {
        let x = i % w as usize;
        let y = i / w as usize;
        [
            if x > 0 { Some(i - 1) } else { None },
            if x + 1 < w as usize {
                Some(i + 1)
            } else {
                None
            },
            if y > 0 { Some(i - w as usize) } else { None },
            if y + 1 < h as usize {
                Some(i + w as usize)
            } else {
                None
            },
        ]
    };
    // Open the matching region to close narrow passages. Re-expand only the
    // reached component, bounded by original matching pixels: line art stays intact.
    let gap = if settings.contiguous {
        settings.close_gap as usize
    } else {
        0
    };
    let candidates = (gap > 0).then(|| {
        let region: Vec<u8> = (0..mask.len())
            .map(|i| if matches(i) { 255 } else { 0 })
            .collect();
        morphology(&region, w as usize, gap, false)
    });
    let seed_at = if let Some(candidates) = &candidates {
        if candidates[start] != 0 {
            Some(start)
        } else {
            let sx = start % w as usize;
            let sy = start / w as usize;
            (sy.saturating_sub(gap)..=(sy + gap).min(h as usize - 1))
                .flat_map(|y| {
                    (sx.saturating_sub(gap)..=(sx + gap).min(w as usize - 1))
                        .map(move |x| y * w as usize + x)
                })
                .filter(|i| candidates[*i] != 0)
                .min_by_key(|i| {
                    (i % w as usize).abs_diff(sx).pow(2) + (i / w as usize).abs_diff(sy).pow(2)
                })
        }
    } else {
        Some(start)
    };
    if settings.contiguous {
        let Some(seed_at) = seed_at else {
            return Ok(empty());
        };
        let mut visited = vec![false; bytes / 4];
        let mut queue = VecDeque::new();
        queue.push_back(seed_at);
        visited[seed_at] = true;
        while let Some(i) = queue.pop_front() {
            if !matches(i) || candidates.as_ref().is_some_and(|c| c[i] == 0) {
                continue;
            }
            mask[i] = 255;
            for n in neighbors(i).into_iter().flatten() {
                if !visited[n] {
                    visited[n] = true;
                    queue.push_back(n);
                }
            }
        }
    } else {
        for (i, value) in mask.iter_mut().enumerate() {
            if matches(i) {
                *value = 255;
            }
        }
    }
    if gap > 0 {
        mask = morphology(&mask, w as usize, gap, true);
        for (i, coverage) in mask.iter_mut().enumerate() {
            if !matches(i) {
                *coverage = 0;
            }
        }
    }
    if settings.anti_alias {
        let mut edges = Vec::new();
        for (i, value) in mask.iter().enumerate() {
            if *value != 0 || !allowed(i) {
                continue;
            }
            if neighbors(i).into_iter().flatten().any(|n| mask[n] == 255) {
                let d = difference(&sample[i * 4..i * 4 + 4], seed) - settings.tolerance as f32;
                if d > 0. && d < 32. {
                    edges.push((i, ((1. - d / 32.) * 255.).round() as u8));
                }
            }
        }
        for (i, a) in edges {
            mask[i] = a;
        }
    }
    if settings.area_offset != 0 {
        mask = morphology(
            &mask,
            w as usize,
            settings.area_offset.unsigned_abs() as usize,
            settings.area_offset > 0,
        );
        for (i, coverage) in mask.iter_mut().enumerate() {
            if !allowed(i) {
                *coverage = 0;
            }
        }
    }
    let mut dirty = BTreeSet::new();
    for (i, a) in mask.iter().enumerate() {
        if *a > 0 && (!alpha_locked || original[i * 4 + 3] > 0) {
            dirty.insert(TileCoord {
                x: (i % w as usize) as u32 / TILE_SIZE,
                y: (i / w as usize) as u32 / TILE_SIZE,
            });
        }
    }
    let mut uploads = Vec::new();
    let mut changed = false;
    for coord in dirty {
        let origin = [coord.x * TILE_SIZE, coord.y * TILE_SIZE];
        let extent = [TILE_SIZE.min(w - origin[0]), TILE_SIZE.min(h - origin[1])];
        let mut values = Vec::with_capacity((extent[0] * extent[1]) as usize);
        for y in origin[1]..origin[1] + extent[1] {
            for x in origin[0]..origin[0] + extent[0] {
                let i = (y * w + x) as usize;
                let mut value = [0.; 12];
                for (c, v) in value[..4].iter_mut().enumerate() {
                    *v = original[i * 4 + c] as f32 / 255.;
                }
                if alpha_locked {
                    let alpha = value[3];
                    if alpha > 0. {
                        for channel in &mut value[..3] {
                            *channel /= alpha;
                        }
                    }
                    value[3] = 1.;
                }
                let s = settings.pattern_size;
                let ink = settings.source == Source::Foreground
                    || match settings.pattern {
                        Pattern::Checker => (x / s + y / s).is_multiple_of(2),
                        Pattern::Stripes => (x + y) % (s * 2) < s,
                        Pattern::Dots => {
                            let dx = (x % s) as f32 - s as f32 * 0.5;
                            let dy = (y % s) as f32 - s as f32 * 0.5;
                            dx.hypot(dy) <= s as f32 * 0.3
                        }
                    };
                if ink {
                    for (c, v) in value[4..7].iter_mut().enumerate() {
                        *v = color[c] as f32 / 255.;
                    }
                    value[7] = 1.;
                }
                value[8] = mask[i] as f32 / 255. * settings.opacity;
                value[9] = crate::clone_stamp::mode_number(settings.mode);
                values.push(value);
            }
        }
        let mut tile = crate::clone_stamp::blend_batch(&values, settings.mode);
        if alpha_locked {
            for (index, pixel) in tile.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = origin[0] + index as u32 % extent[0];
                let y = origin[1] + index as u32 / extent[0];
                let at = ((y * w + x) * 4) as usize;
                let alpha = original[at + 3];
                if alpha == 0 || values[index][8] == 0. || values[index][7] == 0. {
                    pixel.copy_from_slice(&original[at..at + 4]);
                } else {
                    for channel in &mut pixel[..3] {
                        *channel = ((u16::from(*channel) * u16::from(alpha) + 127) / 255) as u8;
                    }
                    pixel[3] = alpha;
                }
            }
        }
        let mut tile_changed = false;
        for (row, y) in (origin[1]..origin[1] + extent[1]).enumerate() {
            let at = ((y * w + origin[0]) * 4) as usize;
            let len = extent[0] as usize * 4;
            let data = &tile[row * len..(row + 1) * len];
            if pixels[at..at + len] != *data {
                tile_changed = true;
            }
            pixels[at..at + len].copy_from_slice(data);
        }
        if tile_changed {
            changed = true;
            uploads.push(TileUpload {
                coord,
                origin,
                extent,
                bytes_per_row: extent[0] * 4,
                pixels: tile,
            });
        }
    }
    Ok(ResultImage {
        pixels,
        uploads,
        changed,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn run(sample: Vec<u8>, settings: Settings, selection: Option<&Selection>) -> ResultImage {
        fill(
            (5, 1),
            vec![0; 20],
            &sample,
            Point { x: 0., y: 0. },
            [255, 0, 0],
            settings,
            selection,
        )
        .unwrap()
    }
    fn colors() -> Vec<u8> {
        [
            [255, 255, 255, 255],
            [255, 255, 255, 255],
            [0, 0, 0, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
        ]
        .concat()
    }
    #[test]
    fn sliding_morphology_matches_brute_force_at_edges_and_large_radii() {
        for (w, h) in [(1usize, 1usize), (1, 7), (7, 1), (8, 5)] {
            let mask: Vec<u8> = (0..w * h).map(|i| ((i * 71 + 19) % 256) as u8).collect();
            for radius in [0, 1, 3, 16, 32] {
                for grow in [true, false] {
                    let expected: Vec<u8> = (0..w * h)
                        .map(|i| {
                            let x = i % w;
                            let y = i / w;
                            let values = (y.saturating_sub(radius)..=(y + radius).min(h - 1))
                                .flat_map(|yy| {
                                    let mask = &mask;
                                    (x.saturating_sub(radius)..=(x + radius).min(w - 1))
                                        .map(move |xx| mask[yy * w + xx])
                                });
                            if grow {
                                values.max().unwrap()
                            } else {
                                values.min().unwrap()
                            }
                        })
                        .collect();
                    assert_eq!(morphology(&mask, w, radius, grow), expected);
                }
            }
        }
    }
    fn broken_outline(transparent: bool) -> Vec<u8> {
        let mut sample = vec![if transparent { 0 } else { 255 }; 15 * 15 * 4];
        for y in 3..=11 {
            for x in 3..=11 {
                if (x == 3 || x == 11 || y == 3 || y == 11) && !(x == 7 && y == 3) {
                    let i = (y * 15 + x) * 4;
                    sample[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }
        sample
    }
    #[test]
    fn close_gap_stops_leak_without_modifying_white_or_transparent_reference() {
        for transparent in [false, true] {
            let sample = broken_outline(transparent);
            let reference = sample.clone();
            let run = |close_gap, contiguous| {
                fill(
                    (15, 15),
                    vec![0; sample.len()],
                    &sample,
                    Point { x: 7., y: 7. },
                    [255, 0, 0],
                    Settings {
                        close_gap,
                        contiguous,
                        anti_alias: false,
                        tolerance: 0,
                        ..Default::default()
                    },
                    None,
                )
                .unwrap()
            };
            assert_eq!(run(0, true).pixels[3], 255);
            let closed = run(1, true);
            assert_eq!(closed.pixels[3], 0);
            assert_eq!(closed.pixels[(7 * 15 + 7) * 4 + 3], 255);
            assert_eq!(closed.pixels[(3 * 15 + 3) * 4 + 3], 0);
            assert_eq!(run(1, false).pixels, run(0, false).pixels);
            assert_eq!(sample, reference);
        }
    }
    #[test]
    fn area_offset_grows_and_shrinks_but_respects_selection_and_alpha_lock() {
        let sample = broken_outline(false);
        let selection = Selection::new(
            lumapaint_core::selection::SelectionShape::Rectangle,
            [3., 3., 9., 9.],
        );
        let run = |area_offset| {
            fill(
                (15, 15),
                vec![0; sample.len()],
                &sample,
                Point { x: 7., y: 7. },
                [255, 0, 0],
                Settings {
                    close_gap: 1,
                    area_offset,
                    anti_alias: false,
                    tolerance: 0,
                    ..Default::default()
                },
                Some(&selection),
            )
            .unwrap()
        };
        let grown = run(2);
        assert_eq!(grown.pixels[(3 * 15 + 3) * 4 + 3], 255);
        assert_eq!(grown.pixels[(2 * 15 + 7) * 4 + 3], 0);
        let shrunk = run(-1);
        assert_eq!(shrunk.pixels[(4 * 15 + 4) * 4 + 3], 0);
        assert_eq!(shrunk.pixels[(7 * 15 + 7) * 4 + 3], 255);
        assert!(!run(-32).changed);
        let mut original = vec![0; sample.len()];
        original[(7 * 15 + 7) * 4..(7 * 15 + 7) * 4 + 4].copy_from_slice(&[32, 32, 32, 64]);
        let result = fill_alpha_locked(
            (15, 15),
            original.clone(),
            &sample,
            Point { x: 7., y: 7. },
            [255, 0, 0],
            Settings {
                close_gap: 1,
                area_offset: 2,
                ..Default::default()
            },
            Some(&selection),
        )
        .unwrap();
        for (pixel, old) in result
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(original.as_chunks::<4>().0)
        {
            assert_eq!(pixel[3], old[3]);
            if old[3] == 0 {
                assert_eq!(pixel, old);
            }
        }
    }
    #[test]
    fn locked_fill_keeps_alpha_and_unselected_pixels_for_every_blend_mode() {
        use lumapaint_core::clone_stamp::Mode;
        let original = [
            [0, 0, 0, 0],
            [32, 32, 32, 64],
            [64, 64, 64, 128],
            [128, 128, 128, 255],
        ]
        .concat();
        let sample = [255; 16];
        let selection = Selection::new(
            lumapaint_core::selection::SelectionShape::Rectangle,
            [0., 0., 3., 1.],
        );
        for mode in [
            Mode::Normal,
            Mode::Multiply,
            Mode::Screen,
            Mode::Overlay,
            Mode::Darken,
            Mode::Lighten,
        ] {
            let result = fill_alpha_locked(
                (4, 1),
                original.clone(),
                &sample,
                Point { x: 1., y: 0. },
                [255, 0, 0],
                Settings {
                    mode,
                    ..Default::default()
                },
                Some(&selection),
            )
            .unwrap();
            assert_eq!(&result.pixels[..4], &original[..4]);
            assert_eq!(&result.pixels[12..], &original[12..]);
            for (a, b) in result
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(original.as_chunks::<4>().0)
            {
                assert_eq!(a[3], b[3]);
                assert!(a[..3].iter().all(|c| *c <= a[3]));
            }
            if mode == Mode::Normal {
                assert_eq!(&result.pixels[4..12], [64, 0, 0, 64, 128, 0, 0, 128]);
            }
        }
        let empty = fill_alpha_locked(
            (4, 1),
            vec![0; 16],
            &sample,
            Point { x: 0., y: 0. },
            [255, 0, 0],
            Settings::default(),
            None,
        )
        .unwrap();
        assert!(!empty.changed && empty.uploads.is_empty());
        let pattern = fill_alpha_locked(
            (4, 1),
            original.clone(),
            &sample,
            Point { x: 1., y: 0. },
            [255, 0, 0],
            Settings {
                source: Source::Pattern,
                pattern: Pattern::Checker,
                pattern_size: 2,
                opacity: 0.5,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(&pattern.pixels[4..8], [48, 16, 16, 64]);
        assert_eq!(&pattern.pixels[8..], &original[8..]);
    }
    #[test]
    fn contiguous_stops_at_boundary_and_global_reaches_disconnected_color() {
        let local = run(
            colors(),
            Settings {
                anti_alias: false,
                ..Default::default()
            },
            None,
        );
        assert_eq!(&local.pixels[..8], [255, 0, 0, 255, 255, 0, 0, 255]);
        assert_eq!(&local.pixels[8..], [0; 12]);
        let global = run(
            colors(),
            Settings {
                contiguous: false,
                ..Default::default()
            },
            None,
        );
        assert_eq!(&global.pixels[12..16], [255, 0, 0, 255]);
        assert_eq!(&global.pixels[8..12], [0; 4]);
    }
    #[test]
    fn tolerance_and_antialias_have_separate_edge_coverage() {
        let sample = [
            [255, 255, 255, 255],
            [235, 235, 235, 255],
            [210, 210, 210, 255],
            [0, 0, 0, 255],
            [255, 255, 255, 255],
        ]
        .concat();
        let hard = run(
            sample.clone(),
            Settings {
                tolerance: 32,
                anti_alias: false,
                ..Default::default()
            },
            None,
        );
        assert_eq!(&hard.pixels[8..12], [0; 4]);
        let soft = run(
            sample,
            Settings {
                tolerance: 32,
                ..Default::default()
            },
            None,
        );
        assert!(soft.pixels[11] > 0 && soft.pixels[11] < 255);
        assert_eq!(&soft.pixels[12..16], [0; 4]);
    }
    #[test]
    fn selection_and_all_layer_reference_do_not_overwrite_source() {
        let selection = Selection::new(
            lumapaint_core::document::SelectionShape::Rectangle,
            [0., 0., 1., 1.],
        );
        let sample = colors();
        let original = sample.clone();
        let result = run(
            sample.clone(),
            Settings {
                opacity: 0.5,
                ..Default::default()
            },
            Some(&selection),
        );
        assert_eq!(&result.pixels[..4], [128, 0, 0, 128]);
        assert_eq!(&result.pixels[4..], [0; 16]);
        assert_eq!(sample, original);
    }
    #[test]
    fn unchanged_zero_opacity_and_outside_click_are_noops() {
        let sample = colors();
        let result = fill(
            (5, 1),
            sample.clone(),
            &sample,
            Point { x: 0., y: 0. },
            [255; 3],
            Settings::default(),
            None,
        )
        .unwrap();
        assert!(!result.changed);
        assert!(result.uploads.is_empty());
        assert!(
            !run(
                sample.clone(),
                Settings {
                    opacity: 0.,
                    ..Default::default()
                },
                None
            )
            .changed
        );
        assert!(
            !fill(
                (5, 1),
                sample.clone(),
                &sample,
                Point { x: -1., y: 0. },
                [255, 0, 0],
                Settings::default(),
                None
            )
            .unwrap()
            .changed
        );
    }
    #[test]
    fn pattern_repeats_in_document_coordinates() {
        let sample = vec![0; 20];
        let result = run(
            sample,
            Settings {
                source: Source::Pattern,
                pattern_size: 2,
                ..Default::default()
            },
            None,
        );
        assert_eq!(&result.pixels[..8], [255, 0, 0, 255, 255, 0, 0, 255]);
        assert_eq!(&result.pixels[8..16], [0; 8]);
        assert_eq!(&result.pixels[16..], [255, 0, 0, 255]);
    }
    #[test]
    fn invalid_inputs_are_rejected() {
        assert!(fill(
            (1, 1),
            vec![0; 4],
            &[0; 3],
            Point { x: 0., y: 0. },
            [0; 3],
            Settings::default(),
            None
        )
        .is_err());
        assert!(fill(
            (1, 1),
            vec![0; 4],
            &[0; 4],
            Point { x: f32::NAN, y: 0. },
            [0; 3],
            Settings::default(),
            None
        )
        .is_err());
        assert!(Settings {
            opacity: f32::NAN,
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
