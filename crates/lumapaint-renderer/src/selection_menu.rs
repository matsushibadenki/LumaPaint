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
        return refine_selection(dimensions, doc.selection(), request);
    }
    let sample = crate::selection_tools::EdgeMap::from_automatic_document(
        &doc.selection_sampling_document(request.all_layers.unwrap_or(true)),
    )?;
    let (size, pixels) = sample.sampled_rgba();
    analyze(size, pixels, dimensions, doc.selection(), request)
}
// Crop to coverage plus every filter's support. Small selections on large pages
// retain document-pixel detail; large footprints use bounded overlapping tiles.
fn refine_selection(
    dimensions: (u32, u32),
    selection: Option<&Selection>,
    request: &PixelSelectionRequest,
) -> Result<Selection, String> {
    let source = selection.ok_or("Select an area first / 範囲を選択してください / 请先选择区域")?;
    let amount = bounded(request.amount, 32., 0., 255.)?;
    let padding = if request.action == "refine" {
        let smooth = bounded(request.smooth, 0., 0., 100.)?;
        let shift = bounded(request.shift, 0., -100., 100.)?;
        let feather = bounded(request.feather, 0., 0., 255.)?;
        let contrast = bounded(request.contrast, 0., 0., 100.)?;
        if smooth == 0. && shift == 0. && feather == 0. && contrast == 0. {
            let mut result = source.clone();
            if request.inverse.unwrap_or(false) {
                result.invert(dimensions.0, dimensions.1)?;
            }
            return Ok(result);
        }
        smooth.round() + shift.abs().round() + 3. * (feather * 0.58).ceil()
    } else {
        match request.kind.as_deref().unwrap_or("expand") {
            "feather" => 3. * (amount * 0.58).ceil(),
            "expand" | "contract" | "border" | "smooth" => amount.round(),
            _ => return Err("Unknown selection modification".into()),
        }
    } + 1.;
    let bounds = source.coverage_bounds().unwrap_or([0.; 4]);
    let crop = |padding: f32| {
        let left = (bounds[0].floor() - padding).clamp(0., dimensions.0 as f32);
        let top = (bounds[1].floor() - padding).clamp(0., dimensions.1 as f32);
        [
            left,
            top,
            (bounds[2].ceil() + padding).clamp(left, dimensions.0 as f32),
            (bounds[3].ceil() + padding).clamp(top, dimensions.1 as f32),
        ]
    };
    // Match document-edge behavior; artificial crop edges need transparent
    // samples for every pass, including rounding filter radii on a reduced grid.
    let budget = lumapaint_core::selection_raster::MAX_SELECTION_PIXELS;
    let [left, top, right, bottom] = crop(padding);
    if right <= left || bottom <= top {
        let mut result = Selection::from_raster(SelectionRaster::from_pixels(
            [1, 1],
            [1., 0., 0., 1., 0., 0.],
            &[0],
        )?)?;
        if request.inverse.unwrap_or(false) {
            result.invert(dimensions.0, dimensions.1)?;
        }
        return Ok(result);
    }
    let extent = [(right - left) as u32, (bottom - top) as u32];
    let mut local_request = request.clone();
    local_request.inverse = Some(false);
    let mut result = if u64::from(extent[0]) * u64::from(extent[1]) <= budget {
        analyze_at(
            extent,
            &[],
            (extent[0], extent[1]),
            [left, top],
            selection,
            &local_request,
        )?
    } else {
        refine_tiled(
            extent,
            [left, top],
            source,
            &local_request,
            padding.ceil() as u32,
        )?
    };
    // Inversion covers the entire page, including the area outside the crop,
    // without allocating a full-page mask or reducing the cropped edge detail.
    if request.inverse.unwrap_or(false) {
        result.invert(dimensions.0, dimensions.1)?;
    }
    Ok(result)
}

// Keep each filter's complete support around the output tile. Bands are encoded
// in row order so compressed runs can merge across tile and band boundaries.
fn refine_tiled(
    size: [u32; 2],
    origin: [f32; 2],
    source: &Selection,
    request: &PixelSelectionRequest,
    halo: u32,
) -> Result<Selection, String> {
    use lumapaint_core::selection_raster::MAX_SELECTION_RUNS;
    let mut runs: Vec<[u32; 3]> = Vec::new();
    for y in (0..size[1]).step_by(512) {
        let rows = 512.min(size[1] - y);
        let mut band = vec![0u8; (size[0] * rows) as usize];
        for x in (0..size[0]).step_by(512) {
            let columns = 512.min(size[0] - x);
            let left = x.saturating_sub(halo);
            let top = y.saturating_sub(halo);
            let right = (x + columns + halo).min(size[0]);
            let bottom = (y + rows + halo).min(size[1]);
            let tile_size = [right - left, bottom - top];
            let tile = analyze_at(
                tile_size,
                &[],
                (tile_size[0], tile_size[1]),
                [origin[0] + left as f32, origin[1] + top as f32],
                Some(source),
                request,
            )?;
            let pixels = tile.raster.as_ref().unwrap().pixels();
            for row in 0..rows {
                let start = ((y + row - top) * tile_size[0] + x - left) as usize;
                let target = (row * size[0] + x) as usize;
                band[target..target + columns as usize]
                    .copy_from_slice(&pixels[start..start + columns as usize]);
            }
        }
        let encoded =
            SelectionRaster::from_pixels([size[0], rows], [1., 0., 0., 1., 0., 0.], &band)?;
        for &[start, len, alpha] in encoded.runs.iter() {
            let start = start + y * size[0];
            if let Some(last) = runs.last_mut() {
                if last[0] + last[1] == start && last[2] == alpha {
                    last[1] += len;
                    continue;
                }
            }
            if runs.len() == MAX_SELECTION_RUNS {
                return Err("Selection mask complexity limit exceeded / 選択マスクが複雑すぎます / 选区蒙版过于复杂".into());
            }
            runs.push([start, len, alpha]);
        }
    }
    Selection::from_raster(SelectionRaster {
        size,
        transform: [1., 0., 0., 1., origin[0], origin[1]],
        runs: std::sync::Arc::new(runs),
        interpolation: Default::default(),
    })
}

fn analyze(
    size: [u32; 2],
    pixels: &[u8],
    dimensions: (u32, u32),
    selection: Option<&Selection>,
    request: &PixelSelectionRequest,
) -> Result<Selection, String> {
    analyze_at(size, pixels, dimensions, [0., 0.], selection, request)
}
fn analyze_at(
    size: [u32; 2],
    pixels: &[u8],
    dimensions: (u32, u32),
    origin: [f32; 2],
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
                    x: origin[0] + (i % w) as f32 * sx + sx * 0.5,
                    y: origin[1] + (i / w) as f32 * sy + sy * 0.5,
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
        [sx, 0., 0., sy, origin[0], origin[1]],
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
    fn selection_document(size: u32, selection: Selection) -> Document {
        let mut state = Document::default().document_state();
        state.width = size;
        state.height = size;
        let mut doc = Document::from_document_state(state).unwrap();
        doc.set_tool_pixel_selection(Some(selection)).unwrap();
        doc
    }

    #[test]
    fn selection_refine_tiles_match_full_grid_across_seams() {
        use lumapaint_core::selection::SelectionShape;
        let mut source = Selection::new(SelectionShape::Rectangle, [12., 8., 1000., 530.]);
        source.invert(1030, 550).unwrap();
        let mut seam = Selection::new(SelectionShape::Rectangle, [510., 510., 5., 5.])
            .regions
            .remove(0);
        seam.operation = lumapaint_core::selection::SelectionOperation::Add;
        source.regions.push(seam);
        let requests = ["expand", "contract", "border", "smooth", "feather"].map(|kind| {
            PixelSelectionRequest {
                action: "modify".into(),
                kind: Some(kind.into()),
                amount: Some(3.),
                ..Default::default()
            }
        });
        for request in requests.into_iter().chain([PixelSelectionRequest {
            action: "refine".into(),
            smooth: Some(2.),
            shift: Some(-3.),
            feather: Some(2.),
            contrast: Some(15.),
            ..Default::default()
        }]) {
            let full = analyze_at(
                [1030, 550],
                &[],
                (1030, 550),
                [0., 0.],
                Some(&source),
                &request,
            )
            .unwrap();
            let tiled = refine_tiled([1030, 550], [0., 0.], &source, &request, 20).unwrap();
            let tiled = tiled.raster.as_ref().unwrap().pixels();
            let full = full.raster.as_ref().unwrap().pixels();
            let error = tiled
                .iter()
                .zip(&full)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            let tolerance =
                u8::from(request.action == "refine" || request.kind.as_deref() == Some("feather"));
            assert!(
                error <= tolerance,
                "{} {:?}: max alpha difference {error}",
                request.action,
                request.kind
            );
        }
    }

    #[test]
    fn selection_refine_large_native_grid_keeps_one_pixel_hole() {
        use lumapaint_core::selection::SelectionShape;
        let mut source = Selection::new(SelectionShape::Rectangle, [3000., 1800., 1., 1.]);
        source.invert(4096, 4096).unwrap();
        let doc = selection_document(4096, source);
        let result = evaluate(
            &doc,
            &PixelSelectionRequest {
                action: "refine".into(),
                contrast: Some(20.),
                ..Default::default()
            },
        )
        .unwrap();
        let raster = result.raster.as_ref().unwrap();
        assert_eq!(raster.size, [4096, 4096]);
        assert_eq!(raster.transform, [1., 0., 0., 1., 0., 0.]);
        assert_eq!(
            result.coverage(Point {
                x: 3000.5,
                y: 1800.5
            }),
            0.
        );
        assert_eq!(
            result.coverage(Point {
                x: 2999.5,
                y: 1800.5
            }),
            1.
        );
    }

    #[test]
    fn selection_refine_large_page_keeps_single_pixels_and_inverse_outside_crop() {
        use lumapaint_core::selection::SelectionShape;
        let source = Selection::new(SelectionShape::Rectangle, [3000., 2000., 1., 1.]);
        let doc = selection_document(8192, source.clone());
        let unchanged = evaluate(
            &doc,
            &PixelSelectionRequest {
                action: "refine".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(unchanged, source);
        for action in ["modify", "refine"] {
            for inverse in [false, true] {
                let result = evaluate(
                    &doc,
                    &PixelSelectionRequest {
                        action: action.into(),
                        kind: Some("expand".into()),
                        amount: Some(1.),
                        shift: Some(1.),
                        inverse: Some(inverse),
                        ..Default::default()
                    },
                )
                .unwrap();
                let raster = result.raster.as_ref().unwrap();
                assert_eq!(&raster.transform[..4], &[1., 0., 0., 1.]);
                assert!(raster.size[0] * raster.size[1] < 100);
                for y in 1997..2004 {
                    for x in 2997..3004 {
                        let inside = (2999..3002).contains(&x) && (1999..2002).contains(&y);
                        assert_eq!(
                            result.coverage(Point {
                                x: x as f32 + 0.5,
                                y: y as f32 + 0.5
                            }),
                            f32::from(inside != inverse)
                        );
                    }
                }
                assert_eq!(
                    result.coverage(Point {
                        x: 7000.5,
                        y: 7000.5
                    }),
                    f32::from(inverse)
                );
                assert_eq!(result.coverage(Point { x: -1., y: 0. }), 0.);
            }
        }
    }

    #[test]
    fn selection_refine_cropped_filters_match_full_resolution_at_page_edges_and_holes() {
        use lumapaint_core::selection::{SelectionOperation, SelectionShape};
        let mut source = Selection::new(SelectionShape::Rectangle, [0., 3., 110., 120.]);
        let mut hole = Selection::new(SelectionShape::Rectangle, [70., 60., 1., 1.])
            .regions
            .remove(0);
        hole.operation = SelectionOperation::Subtract;
        source.regions.push(hole);
        let doc = selection_document(256, source);
        for kind in [
            "expand", "contract", "smooth", "feather", "border", "refine",
        ] {
            let request = PixelSelectionRequest {
                action: if kind == "refine" { "refine" } else { "modify" }.into(),
                kind: Some(kind.into()),
                amount: Some(3.),
                smooth: Some(2.),
                feather: Some(3.),
                contrast: Some(17.),
                shift: Some(-2.),
                ..Default::default()
            };
            let cropped = evaluate(&doc, &request).unwrap();
            let full = analyze([256, 256], &[], (256, 256), doc.selection(), &request).unwrap();
            for y in 0..256 {
                for x in 0..256 {
                    let p = Point {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    };
                    assert!(
                        (cropped.coverage(p) - full.coverage(p)).abs() <= 1. / 255.,
                        "{kind} at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn selection_refine_preserves_gray_noop_and_bounds_large_footprints() {
        use lumapaint_core::selection::SelectionShape;
        let source = Selection::from_raster(
            SelectionRaster::from_pixels([3, 1], [1., 0., 0., 1., 3000., 2000.], &[1, 127, 254])
                .unwrap(),
        )
        .unwrap();
        let doc = selection_document(8192, source.clone());
        assert_eq!(
            evaluate(
                &doc,
                &PixelSelectionRequest {
                    action: "refine".into(),
                    ..Default::default()
                }
            )
            .unwrap(),
            source
        );
        let result = evaluate(
            &doc,
            &PixelSelectionRequest {
                action: "modify".into(),
                kind: Some("expand".into()),
                amount: Some(1.),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            result.coverage(Point {
                x: 2999.5,
                y: 2000.5
            }),
            1. / 255.
        );
        assert_eq!(
            result.coverage(Point {
                x: 3000.5,
                y: 2000.5
            }),
            127. / 255.
        );
        let doc = selection_document(
            8192,
            Selection::new(SelectionShape::Rectangle, [0., 0., 8192., 8192.]),
        );
        let result = evaluate(
            &doc,
            &PixelSelectionRequest {
                action: "refine".into(),
                contrast: Some(20.),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.raster.as_ref().unwrap().size, [8192, 8192]);
        assert_eq!(
            result.coverage(Point {
                x: 8000.5,
                y: 8000.5
            }),
            1.
        );
        let doc = selection_document(
            8192,
            Selection::new(SelectionShape::Rectangle, [9000., 2000., 10., 10.]),
        );
        let result = evaluate(
            &doc,
            &PixelSelectionRequest {
                action: "modify".into(),
                amount: Some(1.),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            result.coverage(Point {
                x: 8192.5,
                y: 2000.5
            }),
            0.
        );
    }

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
            for y in 0..48 {
                for x in 0..64 {
                    let point = Point {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    };
                    assert!((direct.coverage(point) - sampled.coverage(point)).abs() <= 1. / 255.);
                }
            }
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
