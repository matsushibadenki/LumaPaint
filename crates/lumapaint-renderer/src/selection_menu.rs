//! Selection analysis uses a bounded native sampling image on the host worker.
//! Pixel masks stay in Rust; only command parameters and summaries cross IPC.
use lumapaint_core::{
    document::{Document, PixelSelectionRequest, Point},
    selection::Selection,
    selection_raster::SelectionRaster,
};
use std::collections::{HashMap, HashSet, VecDeque};

fn bounded(value: Option<f32>, default: f32, min: f32, max: f32) -> Result<f32, String> {
    let v = value.unwrap_or(default);
    if !v.is_finite() || v < min || v > max {
        Err("Invalid selection parameter / 選択パラメータが不正です / 选区参数无效".into())
    } else {
        Ok(v)
    }
}
pub fn evaluate(doc: &Document, request: &PixelSelectionRequest) -> Result<Selection, String> {
    if request.action == "transform" {
        return doc
            .selection()
            .ok_or("Select an area first / 範囲を選択してください / 请先选择区域")?
            .transformed(
                request
                    .matrix
                    .ok_or("Transform missing / 変形値がありません / 缺少变换参数")?,
            );
    }
    let dimensions = doc.dimensions();
    // Refinement depends only on coverage. Avoid rasterizing artwork or allocating
    // RGBA samples on every slider update; use the same grid for preview and apply.
    if matches!(request.action.as_str(), "refine" | "modify") {
        let scale = (1024. / dimensions.0.max(dimensions.1) as f32).min(1.);
        let size = [
            (dimensions.0 as f32 * scale).round().max(1.) as u32,
            (dimensions.1 as f32 * scale).round().max(1.) as u32,
        ];
        return analyze(size, &[], dimensions, doc.selection(), request);
    }
    let sample = crate::selection_tools::EdgeMap::from_automatic_document(
        &doc.selection_sampling_document(request.all_layers.unwrap_or(true)),
    )?;
    let (size, pixels) = sample.sampled_rgba();
    analyze(size, pixels, dimensions, doc.selection(), request)
}
fn analyze(
    size: [u32; 2],
    pixels: &[u8],
    dimensions: (u32, u32),
    selection: Option<&Selection>,
    request: &PixelSelectionRequest,
) -> Result<Selection, String> {
    let (w, h) = (size[0] as usize, size[1] as usize);
    let coverage_only = matches!(request.action.as_str(), "refine" | "modify");
    if w == 0 || h == 0 || w * h > 4 * 1024 * 1024 || (!coverage_only && pixels.len() != w * h * 4)
    {
        return Err("Invalid sampling image".into());
    }
    let sx = dimensions.0 as f32 / size[0] as f32;
    let sy = dimensions.1 as f32 / size[1] as f32;
    let amount = bounded(request.amount, 32., 0., 255.)?;
    let mut mask = (0..w * h)
        .map(|i| {
            selection.map_or(0., |s| {
                s.coverage(Point {
                    x: (i % w) as f32 * sx + sx * 0.5,
                    y: (i / w) as f32 * sy + sy * 0.5,
                })
            })
        })
        .collect::<Vec<_>>();
    let require_selection = || {
        if selection.is_none() {
            Err("Select an area first / 範囲を選択してください / 请先选择区域".to_string())
        } else {
            Ok(())
        }
    };
    match request.action.as_str() {
        "colorRange" => {
            let target = request.color.unwrap_or([0, 0, 0]);
            let kind = request.kind.as_deref().unwrap_or("sampled");
            if ![
                "sampled",
                "reds",
                "greens",
                "blues",
                "highlights",
                "midtones",
                "shadows",
            ]
            .contains(&kind)
            {
                return Err("Unknown color range".into());
            }
            for (i, p) in pixels.as_chunks::<4>().0.iter().enumerate() {
                let color = p[..3].iter().map(|v| f32::from(*v)).collect::<Vec<_>>();
                let luma = (color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722) / 255.;
                let value = match kind {
                    "highlights" => {
                        ((luma - 0.5) / (amount.max(1.) / 255.).max(0.01)).clamp(0., 1.)
                    }
                    "shadows" => ((0.5 - luma) / (amount.max(1.) / 255.).max(0.01)).clamp(0., 1.),
                    "midtones" => {
                        (1. - (luma - 0.5).abs() * 2.).powf((255. / amount.max(1.)).min(16.))
                    }
                    "reds" | "greens" | "blues" => {
                        let c = match kind {
                            "reds" => 0,
                            "greens" => 1,
                            _ => 2,
                        };
                        ((color[c] - color[(c + 1) % 3].max(color[(c + 2) % 3])) / amount.max(1.))
                            .clamp(0., 1.)
                    }
                    _ => {
                        let distance = (0..3)
                            .map(|c| (color[c] - f32::from(target[c])).abs())
                            .fold(0., f32::max);
                        if amount == 0. {
                            f32::from(distance == 0.)
                        } else {
                            (1. - distance / amount).clamp(0., 1.)
                        }
                    }
                };
                mask[i] = value * f32::from(p[3]) / 255.;
            }
        }
        "focusArea" => {
            let luma = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| {
                    (f32::from(p[0]) * 0.2126 + f32::from(p[1]) * 0.7152 + f32::from(p[2]) * 0.0722)
                        / 255.
                })
                .collect::<Vec<_>>();
            let mean = box_blur(&luma, w, h, 2);
            let squares = luma.iter().map(|v| v * v).collect::<Vec<_>>();
            let mean_square = box_blur(&squares, w, h, 2);
            let scores = mean
                .iter()
                .zip(mean_square)
                .map(|(&a, b)| (b - a * a).max(0.).sqrt())
                .collect::<Vec<_>>();
            let peak = scores.iter().copied().fold(0., f32::max).max(1e-5);
            let threshold = bounded(request.amount, 35., 0., 100.)? / 100.;
            for (i, score) in scores.iter().enumerate() {
                mask[i] = ((*score / peak - threshold) / 0.15 + 0.5).clamp(0., 1.)
                    * f32::from(pixels[i * 4 + 3])
                    / 255.;
            }
        }
        "grow" | "similar" => {
            require_selection()?;
            let tolerance = amount.round() as i32;
            let cell = tolerance + 1;
            let mut colors: HashMap<[i32; 3], Vec<[u8; 3]>> = HashMap::new();
            let mut unique = HashSet::new();
            for (i, p) in pixels.as_chunks::<4>().0.iter().enumerate() {
                if mask[i] >= 0.5 && p[3] > 0 {
                    let color = [p[0], p[1], p[2]];
                    if unique.insert(color) {
                        let key = color.map(|v| i32::from(v) / cell);
                        colors.entry(key).or_default().push(color);
                    }
                }
            }
            if colors.is_empty() {
                return Err("Selection has no visible sampled colors / 選択範囲に参照できる色がありません / 选区没有可采样的颜色".into());
            }
            let accepts = |i: usize| {
                let p = &pixels[i * 4..i * 4 + 4];
                if p[3] == 0 {
                    return false;
                }
                let key = [p[0], p[1], p[2]].map(|v| i32::from(v) / cell);
                if colors.contains_key(&key) {
                    return true;
                }
                for r in -1..=1 {
                    for g in -1..=1 {
                        for b in -1..=1 {
                            if colors
                                .get(&[key[0] + r, key[1] + g, key[2] + b])
                                .is_some_and(|values| {
                                    values.iter().any(|c| {
                                        (0..3).all(|axis| {
                                            (i32::from(c[axis]) - i32::from(p[axis])).abs()
                                                <= tolerance
                                        })
                                    })
                                })
                            {
                                return true;
                            }
                        }
                    }
                }
                false
            };
            if request.action == "similar" {
                for (i, value) in mask.iter_mut().enumerate() {
                    if accepts(i) {
                        *value = 1.;
                    }
                }
            } else {
                let mut queue = VecDeque::new();
                let mut seen = vec![false; w * h];
                for (i, value) in mask.iter().enumerate() {
                    if *value >= 0.5 {
                        seen[i] = true;
                        queue.push_back(i);
                    }
                }
                while let Some(i) = queue.pop_front() {
                    let x = i % w;
                    let y = i / w;
                    for next in [
                        x.checked_sub(1).map(|_| i - 1),
                        (x + 1 < w).then_some(i + 1),
                        y.checked_sub(1).map(|_| i - w),
                        (y + 1 < h).then_some(i + w),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if !seen[next] {
                            seen[next] = true;
                            if accepts(next) {
                                mask[next] = 1.;
                                queue.push_back(next);
                            }
                        }
                    }
                }
            }
        }
        "modify" => {
            require_selection()?;
            let radius = (amount / sx.max(sy)).round().clamp(0., 255.) as usize;
            mask = match request.kind.as_deref().unwrap_or("expand") {
                "expand" => morph(&mask, w, h, radius, true),
                "contract" => morph(&mask, w, h, radius, false),
                "border" => {
                    let outer = morph(&mask, w, h, radius.div_ceil(2), true);
                    let inner = morph(&mask, w, h, radius / 2, false);
                    outer
                        .into_iter()
                        .zip(inner)
                        .map(|(a, b)| (a - b).max(0.))
                        .collect()
                }
                "smooth" => box_blur(&mask, w, h, radius)
                    .into_iter()
                    .map(|v| f32::from(v >= 0.5))
                    .collect(),
                "feather" => feather(&mask, w, h, amount / sx.max(sy)),
                _ => return Err("Unknown selection modification".into()),
            };
        }
        "refine" => {
            require_selection()?;
            let smooth = bounded(request.smooth, 0., 0., 100.)?;
            let radius = (smooth / sx.max(sy)).round() as usize;
            if radius > 0 {
                mask = box_blur(&mask, w, h, radius);
            }
            let shift = bounded(request.shift, 0., -100., 100.)?;
            if shift != 0. {
                mask = morph(
                    &mask,
                    w,
                    h,
                    (shift.abs() / sx.max(sy)).round() as usize,
                    shift > 0.,
                );
            }
            let radius = bounded(request.feather, 0., 0., 255.)? / sx.max(sy);
            if radius > 0. {
                mask = feather(&mask, w, h, radius);
            }
            let contrast = bounded(request.contrast, 0., 0., 100.)? / 100.;
            if contrast > 0. {
                let scale = 1. / (1. - contrast).max(0.01);
                for value in &mut mask {
                    *value = ((*value - 0.5) * scale + 0.5).clamp(0., 1.);
                }
            }
        }
        _ => return Err("Unknown selection analysis".into()),
    }
    if request.action == "focusArea" {
        let radius = bounded(request.feather, 0., 0., 255.)? / sx.max(sy);
        if radius > 0. {
            mask = feather(&mask, w, h, radius);
        }
    }
    if request.inverse.unwrap_or(false) {
        for value in &mut mask {
            *value = 1. - *value;
        }
    }
    let bytes = mask
        .into_iter()
        .map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
        .collect::<Vec<_>>();
    Selection::from_raster(SelectionRaster::from_pixels(
        size,
        [sx, 0., 0., sy, 0., 0.],
        &bytes,
    )?)
}
fn box_blur(input: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return input.to_vec();
    }
    let radius = radius.min(255);
    let mut horizontal = vec![0.; input.len()];
    let mut out = horizontal.clone();
    for y in 0..h {
        let mut prefix = vec![0.; w + 1];
        for x in 0..w {
            prefix[x + 1] = prefix[x] + input[y * w + x];
        }
        for x in 0..w {
            let lo = x.saturating_sub(radius);
            let hi = (x + radius + 1).min(w);
            horizontal[y * w + x] = (prefix[hi] - prefix[lo]) / (hi - lo) as f32;
        }
    }
    for x in 0..w {
        let mut prefix = vec![0.; h + 1];
        for y in 0..h {
            prefix[y + 1] = prefix[y] + horizontal[y * w + x];
        }
        for y in 0..h {
            let lo = y.saturating_sub(radius);
            let hi = (y + radius + 1).min(h);
            out[y * w + x] = (prefix[hi] - prefix[lo]) / (hi - lo) as f32;
        }
    }
    out
}
fn feather(input: &[f32], w: usize, h: usize, radius: f32) -> Vec<f32> {
    let r = (radius * 0.58).ceil() as usize;
    if r == 0 {
        return input.to_vec();
    }
    let mut out = input.to_vec();
    for _ in 0..3 {
        out = box_blur(&out, w, h, r);
    }
    out
}
fn morph(input: &[f32], w: usize, h: usize, radius: usize, max: bool) -> Vec<f32> {
    if radius == 0 {
        return input.to_vec();
    }
    let radius = radius.min(255);
    let mut out = input.to_vec();
    let mut temporary = out.clone();
    for axis in 0..2 {
        let (lines, length) = if axis == 0 { (h, w) } else { (w, h) };
        for line in 0..lines {
            let at = |p: usize| {
                if axis == 0 {
                    line * w + p
                } else {
                    p * w + line
                }
            };
            let mut queue: VecDeque<usize> = VecDeque::new();
            let mut next = 0;
            for p in 0..length {
                let hi = (p + radius + 1).min(length);
                while next < hi {
                    while queue.back().is_some_and(|&last| {
                        if max {
                            out[at(last)] <= out[at(next)]
                        } else {
                            out[at(last)] >= out[at(next)]
                        }
                    }) {
                        queue.pop_back();
                    }
                    queue.push_back(next);
                    next += 1;
                }
                let lo = p.saturating_sub(radius);
                while queue.front().is_some_and(|&first| first < lo) {
                    queue.pop_front();
                }
                temporary[at(p)] = out[at(*queue.front().unwrap())];
            }
        }
        std::mem::swap(&mut out, &mut temporary);
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_formats::native::NativeDocumentCodec;
    #[test]
    fn selection_refine_coverage_only_matches_sampled_path() {
        let mut state = Document::default().document_state();
        state.width = 64;
        state.height = 48;
        let mut doc = Document::from_document_state(state).unwrap();
        doc.set_tool_pixel_selection(Some(Selection::new(
            lumapaint_core::selection::SelectionShape::Rectangle,
            [10., 10., 30., 20.],
        )))
        .unwrap();
        for inverse in [false, true] {
            let request = PixelSelectionRequest {
                action: "refine".into(),
                smooth: Some(2.),
                feather: Some(3.),
                shift: Some(-2.),
                contrast: Some(25.),
                inverse: Some(inverse),
                ..Default::default()
            };
            let direct = evaluate(&doc, &request).unwrap();
            let sampled = analyze(
                [64, 48],
                &vec![127; 64 * 48 * 4],
                (64, 48),
                doc.selection(),
                &request,
            )
            .unwrap();
            assert_eq!(direct, sampled);
        }
        let invalid = PixelSelectionRequest {
            action: "refine".into(),
            feather: Some(f32::NAN),
            ..Default::default()
        };
        assert!(evaluate(&doc, &invalid).is_err());
    }
    #[test]
    fn transform_evaluates_selection_without_rendering_or_mutating_artwork() {
        let mut doc = Document::default();
        doc.begin(Point { x: 100., y: 100. }, Default::default())
            .unwrap();
        doc.finish();
        doc.set_path_selection(
            vec![Point { x: 300.5, y: 200.5 }],
            0.5,
            lumapaint_core::selection::SelectionMode::Replace,
        )
        .unwrap();
        let before = doc.encode().unwrap();
        let transformed = evaluate(
            &doc,
            &PixelSelectionRequest {
                action: "transform".into(),
                matrix: Some([2., 0., 0., 2., 0.25, 0.5]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(transformed.raster.as_ref().unwrap().size[0] <= 4);
        assert!(
            transformed.coverage(Point {
                x: 601.25,
                y: 401.5
            }) > 0.99
        );
        assert_eq!(doc.encode().unwrap(), before);
        assert!(evaluate(
            &doc,
            &PixelSelectionRequest {
                action: "transform".into(),
                matrix: Some([f32::NAN; 6]),
                ..Default::default()
            }
        )
        .is_err());
        assert_eq!(doc.encode().unwrap(), before);
    }
    fn request(action: &str) -> PixelSelectionRequest {
        PixelSelectionRequest {
            action: action.into(),
            ..Default::default()
        }
    }
    #[test]
    fn color_range_has_soft_coverage_and_excludes_transparency() {
        let pixels = [255, 0, 0, 255, 240, 0, 0, 255, 0, 0, 255, 255, 255, 0, 0, 0];
        let s = analyze(
            [4, 1],
            &pixels,
            (4, 1),
            None,
            &PixelSelectionRequest {
                color: Some([255, 0, 0]),
                amount: Some(30.),
                ..request("colorRange")
            },
        )
        .unwrap();
        assert_eq!(s.coverage(Point { x: 0.5, y: 0.5 }), 1.);
        assert!((s.coverage(Point { x: 1.5, y: 0.5 }) - 0.5).abs() < 0.01);
        assert_eq!(s.coverage(Point { x: 2.5, y: 0.5 }), 0.);
        assert_eq!(s.coverage(Point { x: 3.5, y: 0.5 }), 0.);
    }
    #[test]
    fn grow_is_contiguous_similar_is_global() {
        let pixels = [
            255, 0, 0, 255, 250, 0, 0, 255, 0, 0, 255, 255, 250, 0, 0, 255,
        ];
        let base = Selection::new(
            lumapaint_core::selection::SelectionShape::Rectangle,
            [0., 0., 1., 1.],
        );
        for (action, last) in [("grow", false), ("similar", true)] {
            let s = analyze(
                [4, 1],
                &pixels,
                (4, 1),
                Some(&base),
                &PixelSelectionRequest {
                    amount: Some(10.),
                    ..request(action)
                },
            )
            .unwrap();
            assert!(s.contains(Point { x: 1.5, y: 0.5 }));
            assert!(!s.contains(Point { x: 2.5, y: 0.5 }));
            assert_eq!(s.contains(Point { x: 3.5, y: 0.5 }), last);
        }
    }
    #[test]
    fn morphology_feather_and_invalid_parameters() {
        let mut input = vec![0.; 49];
        input[24] = 1.;
        let expanded = morph(&input, 7, 7, 1, true);
        assert_eq!(expanded.iter().filter(|v| **v > 0.).count(), 9);
        assert_eq!(morph(&expanded, 7, 7, 1, false), input);
        let soft = feather(&expanded, 7, 7, 2.);
        assert!(soft.iter().any(|v| *v > 0. && *v < 1.));
        let p = vec![255; 7 * 7 * 4];
        assert!(analyze([7, 7], &p, (7, 7), None, &request("refine")).is_err());
        assert!(analyze(
            [7, 7],
            &p,
            (7, 7),
            None,
            &PixelSelectionRequest {
                amount: Some(f32::NAN),
                ..request("colorRange")
            }
        )
        .is_err());
    }
    #[test]
    fn focus_area_prefers_local_detail() {
        let mut p = vec![128; 32 * 16 * 4];
        for y in 0..16 {
            for x in 0..32 {
                let i = (y * 32 + x) * 4;
                p[i + 3] = 255;
                if x > 16 {
                    p[i..i + 3].fill(if (x + y) % 2 == 0 { 0 } else { 255 });
                }
            }
        }
        let s = analyze([32, 16], &p, (32, 16), None, &request("focusArea")).unwrap();
        assert!(s.coverage(Point { x: 24.5, y: 8.5 }) > 0.9);
        assert_eq!(s.coverage(Point { x: 4.5, y: 8.5 }), 0.);
    }
}
