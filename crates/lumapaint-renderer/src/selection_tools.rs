use lumapaint_core::{
    document::{Document, Point},
    selection_tools::Settings,
};
pub struct EdgeMap {
    size: (u32, u32),
    scale: f32,
    pixels: Vec<u8>,
}
impl EdgeMap {
    pub(crate) fn sampled_rgba(&self) -> ([u32; 2], &[u8]) {
        ([self.size.0, self.size.1], &self.pixels)
    }
    pub fn from_document(doc: &Document) -> Result<Self, String> {
        let png = crate::thumbnails::page_preview(doc, 1024)?;
        let image = image::load_from_memory(&png)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        let size = image.dimensions();
        Ok(Self {
            size,
            scale: size.0 as f32 / doc.dimensions().0 as f32,
            pixels: image.into_raw(),
        })
    }
    fn channel(&self, x: i32, y: i32, c: usize) -> f32 {
        let x = x.clamp(0, self.size.0 as i32 - 1);
        let y = y.clamp(0, self.size.1 as i32 - 1);
        let i = ((y as u32 * self.size.0 + x as u32) * 4) as usize;
        self.pixels[i + c] as f32
    }
    fn edge(&self, x: i32, y: i32) -> f32 {
        (0..4)
            .map(|c| {
                (self.channel(x + 1, y, c) - self.channel(x - 1, y, c))
                    .hypot(self.channel(x, y + 1, c) - self.channel(x, y - 1, c))
            })
            .fold(0., f32::max)
    }
    pub fn snap(&self, point: Point, settings: Settings, screen_zoom: f32) -> Point {
        let radius = (settings.magnetic_width / screen_zoom.max(0.001) * self.scale).clamp(1., 40.);
        let cx = (point.x * self.scale).round() as i32;
        let cy = (point.y * self.scale).round() as i32;
        let mut best = 0.;
        let mut hit = point;
        for dy in -(radius.ceil() as i32)..=radius.ceil() as i32 {
            for dx in -(radius.ceil() as i32)..=radius.ceil() as i32 {
                let distance = (dx as f32).hypot(dy as f32);
                if distance > radius {
                    continue;
                }
                let x = cx + dx;
                let y = cy + dy;
                if x < 1 || y < 1 || x + 1 >= self.size.0 as i32 || y + 1 >= self.size.1 as i32 {
                    continue;
                }
                let edge = self.edge(x, y);
                if edge < settings.magnetic_contrast * 2.55 {
                    continue;
                }
                let score = edge * (1. - distance / (radius + 1.));
                if score > best {
                    best = score;
                    hit = Point {
                        x: x as f32 / self.scale,
                        y: y as f32 / self.scale,
                    };
                }
            }
        }
        hit
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn magnetic_snap_follows_color_or_alpha_edges_and_rejects_flat_regions() {
        let mut pixels = vec![0; 32 * 16 * 4];
        for y in 0..16 {
            for x in 16..32 {
                pixels[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4]
                    .copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        let map = EdgeMap {
            size: (32, 16),
            scale: 1.,
            pixels,
        };
        let point = map.snap(
            Point { x: 13., y: 8. },
            Settings {
                magnetic_width: 6.,
                ..Default::default()
            },
            1.,
        );
        assert!((15. ..=16.).contains(&point.x));
        let flat = EdgeMap {
            size: (32, 16),
            scale: 1.,
            pixels: vec![255; 32 * 16 * 4],
        };
        assert_eq!(
            flat.snap(Point { x: 13., y: 8. }, Default::default(), 1.),
            Point { x: 13., y: 8. }
        );
    }
    #[test]
    fn polygon_selection_clips_paint_bucket_and_survives_native_reload() {
        use lumapaint_core::{document::Brush, selection::SelectionMode};
        use lumapaint_formats::native::NativeDocumentCodec;
        let mut doc = Document::default();
        doc.set_path_selection(
            vec![
                Point { x: 0., y: 0. },
                Point { x: 5., y: 0. },
                Point { x: 0., y: 5. },
            ],
            0.,
            SelectionMode::Replace,
        )
        .unwrap();
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.selection(), doc.selection());
        let result = crate::paint_bucket::fill(
            (8, 8),
            vec![0; 256],
            &[0; 256],
            Point { x: 1., y: 1. },
            Brush::default().color,
            Default::default(),
            loaded.selection(),
        )
        .unwrap();
        assert!(result.pixels[9 * 4 + 3] > 0);
        assert_eq!(result.pixels[(6 * 8 + 6) * 4 + 3], 0);
    }
}

/// Color/alpha based selection on a bounded sampling image. Pixels never cross IPC.
pub enum AutoSelectionKind {
    Wand,
    Quick,
    Object,
}
impl EdgeMap {
    pub fn from_automatic_document(doc: &Document) -> Result<Self, String> {
        let mut image = crate::thumbnails::scene_pixels(doc, 1024)?;
        for p in image.pixels.as_chunks_mut::<4>().0.iter_mut() {
            if p[3] > 0 {
                let alpha = u32::from(p[3]);
                for c in &mut p[..3] {
                    *c = (u32::from(*c) * 255 / alpha).min(255) as u8;
                }
            }
        }
        Ok(Self {
            size: (image.width, image.height),
            scale: image.width as f32 / doc.dimensions().0 as f32,
            pixels: image.pixels,
        })
    }
    pub fn automatic_path(
        &self,
        kind: AutoSelectionKind,
        points: &[Point],
        settings: Settings,
        dimensions: (u32, u32),
    ) -> Result<Vec<Point>, String> {
        use std::collections::{BTreeMap, VecDeque};
        settings.validate()?;
        if points.is_empty()
            || dimensions.0 == 0
            || dimensions.1 == 0
            || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite())
        {
            return Err("Invalid selection coordinates".into());
        }
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        let mut mask = vec![false; w * h];
        let pixel = |i: usize| -> [u8; 4] { self.pixels[i * 4..i * 4 + 4].try_into().unwrap() };
        // Compare premultiplied colors: invisible RGB must not split transparency.
        let distance = |a: [u8; 4], b: [u8; 4]| -> f32 {
            let mut d = a[3].abs_diff(b[3]) as f32;
            for (ac, bc) in a[..3].iter().zip(&b[..3]) {
                d = d
                    .max((*ac as f32 * a[3] as f32 / 255. - *bc as f32 * b[3] as f32 / 255.).abs());
            }
            d
        };
        let sample = |p: Point| -> (usize, usize) {
            (
                (p.x * w as f32 / dimensions.0 as f32)
                    .floor()
                    .clamp(0., (w - 1) as f32) as usize,
                (p.y * h as f32 / dimensions.1 as f32)
                    .floor()
                    .clamp(0., (h - 1) as f32) as usize,
            )
        };
        let mut seeds = Vec::new();
        let mut background = [0; 4];
        if matches!(kind, AutoSelectionKind::Object) {
            let (ax, ay) = sample(points[0]);
            let (bx, by) = sample(*points.last().unwrap());
            let click = ax == bx && ay == by;
            let (x0, x1, y0, y1) = if click {
                (0, w - 1, 0, h - 1)
            } else {
                (ax.min(bx), ax.max(bx), ay.min(by), ay.max(by))
            };
            // Most frequent perimeter color is the local backdrop, not a semantic AI label.
            let mut colors = BTreeMap::<[u8; 4], usize>::new();
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if x == x0 || x == x1 || y == y0 || y == y1 {
                        let mut color = pixel(y * w + x);
                        if color[3] == 0 {
                            color = [0; 4];
                        }
                        *colors.entry(color).or_default() += 1;
                    }
                }
            }
            if let Some((color, _)) = colors.into_iter().max_by_key(|(_, n)| *n) {
                background = color;
            }
            let (seed_x0, seed_x1, seed_y0, seed_y1) = if click {
                (ax, ax, ay, ay)
            } else {
                (x0, x1, y0, y1)
            };
            for y in seed_y0..=seed_y1 {
                for x in seed_x0..=seed_x1 {
                    let i = y * w + x;
                    if pixel(i)[3] > 0 && distance(pixel(i), background) > settings.tolerance {
                        seeds.push(i);
                    }
                }
            }
        } else {
            for &p in points
                .iter()
                .take(if matches!(kind, AutoSelectionKind::Wand) {
                    1
                } else {
                    points.len()
                })
            {
                let (x, y) = sample(p);
                if matches!(kind, AutoSelectionKind::Quick) {
                    let radius = (settings.diameter * self.scale * 0.5).max(0.5);
                    let step = (radius / 4.).max(1.) as usize;
                    let r = radius.ceil() as usize;
                    for sy in (y.saturating_sub(r)..=(y + r).min(h - 1)).step_by(step) {
                        for sx in (x.saturating_sub(r)..=(x + r).min(w - 1)).step_by(step) {
                            if (sx as f32 - x as f32).hypot(sy as f32 - y as f32) <= radius
                                && distance(pixel(sy * w + sx), pixel(y * w + x))
                                    <= settings.tolerance
                            {
                                seeds.push(sy * w + sx);
                            }
                        }
                    }
                }
                seeds.push(y * w + x);
            }
        }
        let object = matches!(kind, AutoSelectionKind::Object);
        let global = matches!(kind, AutoSelectionKind::Wand) && !settings.contiguous;
        let mut queue = VecDeque::new();
        for seed in seeds {
            if mask[seed] {
                continue;
            }
            let target = pixel(seed);
            let accepts = |i: usize| {
                if object {
                    pixel(i)[3] > 0 && distance(pixel(i), background) > settings.tolerance
                } else {
                    distance(pixel(i), target) <= settings.tolerance
                }
            };
            if global {
                for (i, selected) in mask.iter_mut().enumerate() {
                    *selected |= accepts(i);
                }
                break;
            }
            mask[seed] = true;
            queue.push_back(seed);
            while let Some(i) = queue.pop_front() {
                let (x, y) = (i % w, i / w);
                for next in [
                    x.checked_sub(1).map(|_| i - 1),
                    (x + 1 < w).then_some(i + 1),
                    y.checked_sub(1).map(|_| i - w),
                    (y + 1 < h).then_some(i + w),
                ]
                .into_iter()
                .flatten()
                {
                    if !mask[next] && accepts(next) {
                        mask[next] = true;
                        queue.push_back(next);
                    }
                }
            }
        }
        // Merge identical horizontal spans vertically; bridge rectangles with doubled edges.
        // The existing even/odd polygon model retains holes and disconnected islands.
        let mut rectangles: Vec<[usize; 4]> = Vec::new();
        let mut previous: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for y in 0..h {
            let mut current = BTreeMap::new();
            let mut x = 0;
            while x < w {
                if !mask[y * w + x] {
                    x += 1;
                    continue;
                }
                let left = x;
                while x < w && mask[y * w + x] {
                    x += 1;
                }
                let index = if let Some(&index) = previous.get(&(left, x)) {
                    rectangles[index][3] = y + 1;
                    index
                } else {
                    rectangles.push([left, y, x, y + 1]);
                    rectangles.len() - 1
                };
                current.insert((left, x), index);
            }
            previous = current;
        }
        if rectangles.len() * 6 + 1 > 16000 {
            return Err("Selection is too detailed; reduce tolerance noise / 選択が複雑すぎます。許容値を調整してください / 选区过于复杂，请调整容差".into());
        }
        let sx = dimensions.0 as f32 / w as f32;
        let sy = dimensions.1 as f32 / h as f32;
        let mut path = Vec::new();
        for [l, t, r, b] in rectangles {
            let a = Point {
                x: l as f32 * sx,
                y: t as f32 * sy,
            };
            let root = *path.first().unwrap_or(&a);
            path.extend([
                a,
                Point {
                    x: r as f32 * sx,
                    y: t as f32 * sy,
                },
                Point {
                    x: r as f32 * sx,
                    y: b as f32 * sy,
                },
                Point {
                    x: l as f32 * sx,
                    y: b as f32 * sy,
                },
                a,
                root,
            ]);
        }
        if path.is_empty() {
            path.extend([
                Point { x: -2., y: -2. },
                Point { x: -1., y: -2. },
                Point { x: -1., y: -1. },
            ]);
        }
        Ok(path)
    }
}

#[cfg(test)]
mod automatic_tests {
    use super::*;
    use lumapaint_core::selection::SelectionMode;
    fn selected(
        map: &EdgeMap,
        kind: AutoSelectionKind,
        points: &[Point],
        settings: Settings,
    ) -> Document {
        let mut doc = Document::default();
        let path = map
            .automatic_path(kind, points, settings, map.size)
            .unwrap();
        doc.set_path_selection(path, 0., SelectionMode::Replace)
            .unwrap();
        doc
    }
    fn fixture() -> EdgeMap {
        let mut pixels = vec![255; 12 * 10 * 4];
        for y in 2..8 {
            for x in 2..6 {
                pixels[(y * 12 + x) * 4..(y * 12 + x) * 4 + 4].copy_from_slice(&[200, 0, 0, 255]);
            }
        }
        for y in 3..5 {
            for x in 3..5 {
                pixels[(y * 12 + x) * 4..(y * 12 + x) * 4 + 4].copy_from_slice(&[255; 4]);
            }
        }
        pixels[(4 * 12 + 9) * 4..(4 * 12 + 9) * 4 + 4].copy_from_slice(&[200, 0, 0, 255]);
        EdgeMap {
            size: (12, 10),
            scale: 1.,
            pixels,
        }
    }
    #[test]
    fn wand_contiguity_holes_and_global_islands_survive_polygon_conversion() {
        let map = fixture();
        let p = Point { x: 2.5, y: 2.5 };
        let doc = selected(&map, AutoSelectionKind::Wand, &[p], Settings::default());
        let region = doc.selection().unwrap();
        assert!(region.contains(p));
        assert!(!region.contains(Point { x: 3.5, y: 3.5 }));
        assert!(!region.contains(Point { x: 9.5, y: 4.5 }));
        let global = selected(
            &map,
            AutoSelectionKind::Wand,
            &[p],
            Settings {
                contiguous: false,
                ..Default::default()
            },
        );
        assert!(global
            .selection()
            .unwrap()
            .contains(Point { x: 9.5, y: 4.5 }));
        for y in 0..10 {
            for x in 0..12 {
                assert_eq!(
                    global.selection().unwrap().contains(Point {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5
                    }),
                    map.pixels[(y * 12 + x) * 4] == 200
                );
            }
        }
    }
    #[test]
    fn object_box_expands_to_connected_foreground_and_quick_brush_combines_regions() {
        let map = fixture();
        let object = selected(
            &map,
            AutoSelectionKind::Object,
            &[Point { x: 1., y: 1. }, Point { x: 6., y: 6. }],
            Settings::default(),
        );
        assert!(object
            .selection()
            .unwrap()
            .contains(Point { x: 2.5, y: 7.5 }));
        assert!(!object
            .selection()
            .unwrap()
            .contains(Point { x: 9.5, y: 4.5 }));
        let quick = selected(
            &map,
            AutoSelectionKind::Quick,
            &[Point { x: 2.5, y: 2.5 }, Point { x: 9.5, y: 4.5 }],
            Settings {
                diameter: 1.,
                ..Default::default()
            },
        );
        assert!(quick
            .selection()
            .unwrap()
            .contains(Point { x: 9.5, y: 4.5 }));
    }
    #[test]
    fn disconnected_islands_do_not_select_the_doubled_bridge() {
        let mut map = EdgeMap {
            size: (8, 8),
            scale: 1.,
            pixels: vec![255; 8 * 8 * 4],
        };
        for (x, y) in [(1, 1), (5, 5)] {
            map.pixels[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].copy_from_slice(&[0, 0, 0, 255]);
        }
        let doc = selected(
            &map,
            AutoSelectionKind::Wand,
            &[Point { x: 1.5, y: 1.5 }],
            Settings {
                contiguous: false,
                ..Default::default()
            },
        );
        let edges = polygon_edges(&doc.selection().unwrap().regions[0].points);
        assert_eq!(edges.len(), 8);
        assert!(edges
            .iter()
            .all(|[a, b]| (a.x == b.x || a.y == b.y) && a != b));
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(
                    doc.selection().unwrap().contains(Point {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5
                    }),
                    [(1, 1), (5, 5)].contains(&(x, y))
                );
            }
        }
    }
    #[test]
    fn transparency_ignores_hidden_rgb_and_tolerance_changes_coverage() {
        let map = EdgeMap {
            size: (3, 1),
            scale: 1.,
            pixels: vec![255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255],
        };
        let doc = selected(
            &map,
            AutoSelectionKind::Wand,
            &[Point { x: 0., y: 0. }],
            Settings {
                tolerance: 0.,
                ..Default::default()
            },
        );
        assert!(doc.selection().unwrap().contains(Point { x: 1.5, y: 0.5 }));
        assert!(!doc.selection().unwrap().contains(Point { x: 2.5, y: 0.5 }));
    }
}

/// Even/odd contours cancel repeated edges. Remove bridges before GPU distance
/// evaluation so disconnected selections do not acquire connecting outlines.
pub(crate) fn polygon_edges(points: &[Point]) -> Vec<[Point; 2]> {
    use std::collections::BTreeMap;
    let key = |p: Point| {
        [
            if p.x == 0. { 0 } else { p.x.to_bits() },
            if p.y == 0. { 0 } else { p.y.to_bits() },
        ]
    };
    let mut edges = BTreeMap::new();
    for (&a, &b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        let (ka, kb) = (key(a), key(b));
        if ka == kb {
            continue;
        }
        let entry = edges
            .entry((ka.min(kb), ka.max(kb)))
            .or_insert((false, [a, b]));
        entry.0 = !entry.0;
    }
    edges
        .into_values()
        .filter_map(|(odd, edge)| odd.then_some(edge))
        .collect()
}
