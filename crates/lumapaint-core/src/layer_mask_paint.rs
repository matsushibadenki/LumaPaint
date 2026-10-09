//! Native mask editing; no coverage buffers cross the WebView bridge.
use crate::{
    image_frame::{inverse, multiply},
    layer_mask::{mapped, LayerMask, MaskContent, MaskEdit},
    selection::Selection,
};

fn bounds(selection: &Selection) -> [f32; 4] {
    selection.regions.iter().fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |b, r| {
            [
                b[0].min(r.bounds[0] - r.radius),
                b[1].min(r.bounds[1] - r.radius),
                b[2].max(r.bounds[0] + r.bounds[2] + r.radius),
                b[3].max(r.bounds[1] + r.bounds[3] + r.radius),
            ]
        },
    )
}
impl LayerMask {
    /// Apply a whole brush stroke to grayscale mask coverage, using the same
    /// accumulated dab density as pixel painting. Called with the original mask
    /// for every preview, so opacity does not compound when frames are refreshed.
    pub fn paint_brush_dabs(
        &mut self,
        size: (u32, u32),
        dabs: &[crate::tiles::RasterDab],
        brush: crate::document::Brush,
        clip: Option<&Selection>,
    ) -> Result<(), String> {
        self.validate()?;
        brush.validate()?;
        let Some(MaskContent::Pixel {
            width,
            height,
            runs,
            gray,
        }) = &self.content
        else {
            return Err("Pixel mask required".into());
        };
        let (mut width, mut height) = (*width, *height);
        let density = crate::tiles::dab_density(size.0, size.1, dabs, clip)?;
        if density.is_empty() || brush.no_color || brush.opacity * brush.alpha == 0. {
            return Ok(());
        }
        let mut pixels = vec![0u8; (width * height) as usize];
        for [start, len] in runs {
            pixels[*start as usize..(start + len) as usize].fill(255);
        }
        for [start, len, value] in gray {
            pixels[*start as usize..(start + len) as usize].fill(*value as u8);
        }
        let inverse = inverse(self.transform)?;
        let mut area = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for coord in density.keys() {
            let x = (coord.x * crate::tiles::TILE_SIZE) as f32;
            let y = (coord.y * crate::tiles::TILE_SIZE) as f32;
            let right = (x + crate::tiles::TILE_SIZE as f32).min(size.0 as f32);
            let bottom = (y + crate::tiles::TILE_SIZE as f32).min(size.1 as f32);
            for point in
                [[x, y], [right, y], [x, bottom], [right, bottom]].map(|p| mapped(inverse, p))
            {
                area = [
                    area[0].min(point.x),
                    area[1].min(point.y),
                    area[2].max(point.x),
                    area[3].max(point.y),
                ];
            }
        }
        // Retain painting outside the old mask bounds after an unlinked move.
        let ox = area[0].floor().min(0.);
        let oy = area[1].floor().min(0.);
        let nw = area[2].ceil().max(width as f32) - ox;
        let nh = area[3].ceil().max(height as f32) - oy;
        if !nw.is_finite() || !nh.is_finite() || nw > 8192. || nh > 8192. {
            return Err("Pixel mask size limit exceeded (8192 px)".into());
        }
        let transform = multiply(self.transform, [1., 0., 0., 1., ox, oy]);
        if nw as u32 != width || nh as u32 != height {
            let mut expanded = vec![0; (nw as u32 * nh as u32) as usize];
            for y in 0..height {
                let from = (y * width) as usize;
                let to = ((y + (-oy) as u32) * nw as u32 + (-ox) as u32) as usize;
                expanded[to..to + width as usize]
                    .copy_from_slice(&pixels[from..from + width as usize]);
            }
            pixels = expanded;
            width = nw as u32;
            height = nh as u32;
        }
        area = [area[0] - ox, area[1] - oy, area[2] - ox, area[3] - oy];
        for y in (area[1].floor().max(0.) as u32)..(area[3].ceil().max(0.) as u32).min(height) {
            for x in (area[0].floor().max(0.) as u32)..(area[2].ceil().max(0.) as u32).min(width) {
                let p = mapped(transform, [x as f32 + 0.5, y as f32 + 0.5]);
                if p.x < 0. || p.y < 0. {
                    continue;
                }
                let (wx, wy) = (p.x.floor() as u32, p.y.floor() as u32);
                let coord = crate::tiles::TileCoord {
                    x: wx / crate::tiles::TILE_SIZE,
                    y: wy / crate::tiles::TILE_SIZE,
                };
                let Some(tile) = density.get(&coord) else {
                    continue;
                };
                let depth = tile[((wy % crate::tiles::TILE_SIZE) * crate::tiles::TILE_SIZE
                    + wx % crate::tiles::TILE_SIZE) as usize];
                let amount = (1. - (-depth).exp()) * brush.opacity * brush.alpha;
                let at = (y * width + x) as usize;
                let old = if self.inverted {
                    255 - pixels[at]
                } else {
                    pixels[at]
                };
                let value = crate::brush_blend::composite_gray(
                    old,
                    brush.color[0],
                    amount,
                    brush.blend_mode,
                    [wx, wy],
                );
                pixels[at] = if self.inverted { 255 - value } else { value };
            }
        }
        let mut runs = Vec::new();
        let mut gray = Vec::new();
        for y in 0..height {
            let mut x = 0;
            while x < width {
                let start = y * width + x;
                let value = pixels[start as usize];
                let mut len = 1;
                while x + len < width && pixels[(start + len) as usize] == value {
                    len += 1;
                }
                if value == 255 {
                    runs.push([start, len]);
                } else if value > 0 {
                    gray.push([start, len, u32::from(value)]);
                }
                x += len;
            }
        }
        let mut next = self.clone();
        next.transform = transform;
        next.content = Some(MaskContent::Pixel {
            width,
            height,
            runs,
            gray,
        });
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn bounds_quad(&self) -> Option<[[f32; 2]; 4]> {
        let [x, y, r, b] = match self.content.as_ref()? {
            MaskContent::Pixel { width, height, .. } => [0., 0., *width as f32, *height as f32],
            MaskContent::Vector { selection, edits } => {
                let mut b = bounds(selection);
                for edit in edits.iter().filter(|e| e.reveal) {
                    let inv = inverse(edit.to_geometry).ok()?;
                    let [x, y, r, bottom] = bounds(&edit.selection);
                    for p in [[x, y], [r, y], [r, bottom], [x, bottom]].map(|p| mapped(inv, p)) {
                        b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
                    }
                }
                b
            }
        };
        Some([[x, y], [r, y], [r, b], [x, b]].map(|p| {
            let p = mapped(self.transform, p);
            [p.x, p.y]
        }))
    }
    /// Paint visible/hidden coverage, in document coordinates. Vector edits retain
    /// exact curves, affine placement and selection clipping. Pixel edits update RLE.
    pub fn paint_selection(
        &mut self,
        selection: &Selection,
        clip: Option<&Selection>,
        reveal: bool,
    ) -> Result<(), String> {
        selection.validate()?;
        if let Some(clip) = clip {
            clip.validate()?;
        }
        let mut next = self.clone();
        let value = reveal != self.inverted;
        match next.content.as_mut().ok_or("Missing mask content")? {
            MaskContent::Vector { edits, .. } => edits.push(MaskEdit {
                selection: selection.clone(),
                clip: clip.cloned(),
                to_geometry: self.transform,
                reveal: value,
            }),
            MaskContent::Pixel {
                width,
                height,
                runs,
                gray,
            } => {
                let inv = inverse(self.transform)?;
                let mut area = bounds(selection);
                if let Some(clip) = clip {
                    let c = bounds(clip);
                    area = [
                        area[0].max(c[0]),
                        area[1].max(c[1]),
                        area[2].min(c[2]),
                        area[3].min(c[3]),
                    ];
                }
                let [x, y, r, b] = area;
                if r <= x || b <= y {
                    return Ok(());
                }
                let local = [[x, y], [r, y], [r, b], [x, b]].map(|p| mapped(inv, p));
                let left = local
                    .iter()
                    .map(|p| p.x)
                    .fold(f32::INFINITY, f32::min)
                    .floor();
                let top = local
                    .iter()
                    .map(|p| p.y)
                    .fold(f32::INFINITY, f32::min)
                    .floor();
                let right = local
                    .iter()
                    .map(|p| p.x)
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil();
                let bottom = local
                    .iter()
                    .map(|p| p.y)
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil();
                let ox = left.min(0.);
                let oy = top.min(0.);
                let nw = right.max(*width as f32) - ox;
                let nh = bottom.max(*height as f32) - oy;
                if !nw.is_finite() || !nh.is_finite() || nw > 8192. || nh > 8192. {
                    return Err("Pixel mask size limit exceeded (8192 px)".into());
                }
                let (nw, nh) = (nw as u32, nh as u32);
                let shiftx = (-ox) as u32;
                let shifty = (-oy) as u32;
                let mut rows: Vec<Vec<[u32; 2]>> = vec![Vec::new(); nh as usize];
                for [start, len] in runs.iter() {
                    rows[(start / *width + shifty) as usize].push([start % *width + shiftx, *len]);
                }
                let l = (left - ox).max(0.) as u32;
                let r = (right - ox).min(nw as f32) as u32;
                let t = (top - oy).max(0.) as u32;
                let b = (bottom - oy).min(nh as f32) as u32;
                let mut run_count = runs.len();
                for yy in t..b {
                    let row = &mut rows[yy as usize];
                    run_count -= row.len();
                    let mut bits = vec![false; nw as usize];
                    for [start, len] in row.iter() {
                        bits[*start as usize..(start + len) as usize].fill(true);
                    }
                    for xx in l..r {
                        let p =
                            mapped(self.transform, [xx as f32 + ox + 0.5, yy as f32 + oy + 0.5]);
                        if selection.contains(p) && clip.is_none_or(|c| c.contains(p)) {
                            bits[xx as usize] = value;
                        }
                    }
                    row.clear();
                    let mut start = None;
                    for xx in 0..=nw {
                        if xx < nw && bits[xx as usize] {
                            if start.is_none() {
                                start = Some(xx);
                            }
                        } else if let Some(start) = start.take() {
                            row.push([start, xx - start]);
                        }
                    }
                    run_count += row.len();
                    if run_count > 1_048_576 {
                        return Err("Pixel mask complexity limit exceeded".into());
                    }
                }
                runs.clear();
                for (yy, row) in rows.iter().enumerate() {
                    for [start, len] in row {
                        runs.push([yy as u32 * nw + start, *len]);
                    }
                }
                let mut carried = Vec::<[u32; 3]>::new();
                for [start, len, tone] in gray.iter().copied() {
                    for index in start..start + len {
                        let xx = index % *width + shiftx;
                        let yy = index / *width + shifty;
                        let point =
                            mapped(self.transform, [xx as f32 + ox + 0.5, yy as f32 + oy + 0.5]);
                        if selection.contains(point) && clip.is_none_or(|c| c.contains(point)) {
                            continue;
                        }
                        let offset = yy * nw + xx;
                        if let Some(last) = carried
                            .last_mut()
                            .filter(|r| r[0] + r[1] == offset && r[2] == tone && r[0] / nw == yy)
                        {
                            last[1] += 1;
                        } else {
                            carried.push([offset, 1, tone]);
                        }
                    }
                }
                *gray = carried;
                *width = nw;
                *height = nh;
                next.transform = multiply(self.transform, [1., 0., 0., 1., ox, oy]);
            }
        }
        next.validate()?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::Document,
        layer_effects::LayerEffects,
        layer_mask::{LayerEditTarget, MaskKind},
        selection::SelectionShape,
    };
    #[test]
    fn brush_mask_expands_after_unlinked_translation() {
        let mut mask = LayerMask::from_selection(MaskKind::Pixel, None, 8, 8).unwrap();
        mask.transform_by([1., 0., 0., 1., 12., 12.]).unwrap();
        mask.paint_brush_dabs(
            (32, 32),
            &[crate::tiles::RasterDab {
                x: 4.,
                y: 4.,
                radius: 2.,
                hardness: 1.,
                weight: 4.,
                texture: 0.,
                texture_scale: 1.,
            }],
            crate::document::Brush {
                color: [255; 3],
                alpha: 0.5,
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(mask.coverage([4.5, 4.5]), 128. / 255.);
        assert_eq!(mask.coverage([14.5, 14.5]), 1.);
        assert_eq!(mask.coverage([0.5, 0.5]), 0.);
    }
    #[test]
    fn brush_mask_grayscale_opacity_alpha_clip_and_save() {
        use crate::{brush_blend::BrushBlendMode, document::Brush, tiles::RasterDab};
        let mut mask = LayerMask::from_selection(MaskKind::Pixel, None, 32, 32).unwrap();
        let dab = RasterDab {
            x: 16.,
            y: 16.,
            radius: 8.,
            hardness: 1.,
            weight: 4.,
            texture: 0.,
            texture_scale: 1.,
        };
        let brush = Brush {
            color: [0; 3],
            opacity: 0.5,
            alpha: 0.5,
            ..Default::default()
        };
        let clip = Selection::new(SelectionShape::Rectangle, [16., 0., 16., 32.]);
        mask.paint_brush_dabs((32, 32), &[dab], brush, Some(&clip))
            .unwrap();
        assert_eq!(mask.coverage([16.5, 16.5]), 191. / 255.);
        assert_eq!(mask.coverage([15.5, 16.5]), 1.);
        assert_eq!(mask.coverage([2.5, 2.5]), 1.);
        let loaded: LayerMask =
            serde_json::from_slice(&serde_json::to_vec(&mask).unwrap()).unwrap();
        assert_eq!(loaded, mask);
        loaded.validate().unwrap();
        mask.paint_brush_dabs(
            (32, 32),
            &[dab],
            Brush {
                opacity: 0.5,
                alpha: 1.,
                blend_mode: BrushBlendMode::Clear,
                ..brush
            },
            Some(&clip),
        )
        .unwrap();
        assert_eq!(mask.coverage([16.5, 16.5]), 96. / 255.);
        mask.paint_selection(
            &Selection::new(SelectionShape::Rectangle, [16., 16., 1., 1.]),
            None,
            true,
        )
        .unwrap();
        assert_eq!(mask.coverage([16.5, 16.5]), 1.);
        assert_eq!(mask.coverage([17.5, 16.5]), 96. / 255.);
        mask.inverted = true;
        mask.paint_brush_dabs(
            (32, 32),
            &[dab],
            Brush {
                color: [255; 3],
                opacity: 1.,
                alpha: 1.,
                ..brush
            },
            None,
        )
        .unwrap();
        assert_eq!(mask.coverage([16.5, 16.5]), 1.);
    }
    #[test]
    fn all_layer_mask_combinations_edit_coverage_without_touching_artwork() {
        for vector_layer in [false, true] {
            for kind in [MaskKind::Pixel, MaskKind::Vector] {
                let mut d = Document::default();
                let id = if vector_layer {
                    d.add_vector_layer().unwrap()
                } else {
                    d.add_paint_layer().unwrap()
                };
                let mask = LayerMask::from_selection(kind, None, 32, 32).unwrap();
                d.set_layer_effects(
                    &id,
                    LayerEffects {
                        mask: Some(mask),
                        ..Default::default()
                    },
                )
                .unwrap();
                let artwork = d.svg_layers().find(|l| l.id == id).unwrap().clone();
                let revision = d.revision();
                d.select_layer_target(id.clone(), LayerEditTarget::Mask)
                    .unwrap();
                assert_eq!(
                    d.revision(),
                    revision,
                    "Focus does not dirty artwork or create history"
                );
                let mut effects = d.layer_effects(&id);
                effects
                    .mask
                    .as_mut()
                    .unwrap()
                    .paint_selection(
                        &Selection::new(SelectionShape::Ellipse, [4., 4., 12., 12.]),
                        None,
                        false,
                    )
                    .unwrap();
                d.set_layer_effects(&id, effects.clone()).unwrap();
                assert_eq!(
                    d.svg_layers().find(|l| l.id == id).unwrap().source,
                    artwork.source
                );
                assert_eq!(effects.mask.as_ref().unwrap().coverage([10.5, 10.5]), 0.);
                d.undo();
                assert_eq!(
                    d.layer_effects(&id).mask.unwrap().coverage([10.5, 10.5]),
                    1.
                );
                d.redo();
                assert_eq!(d.layer_effects(&id), effects);
                assert_eq!(d.layer_edit_target(), LayerEditTarget::Mask);
                assert!(d
                    .snapshot()
                    .layers
                    .iter()
                    .find(|l| l.id == id)
                    .unwrap()
                    .effects
                    .as_ref()
                    .unwrap()
                    .mask
                    .as_ref()
                    .unwrap()
                    .content
                    .is_none());
                let bytes = serde_json::to_vec(&d.document_state()).unwrap();
                let loaded =
                    Document::from_document_state(serde_json::from_slice(&bytes).unwrap()).unwrap();
                assert_eq!(loaded.layer_effects(&id), effects);
                d.select_layer_target(id.clone(), LayerEditTarget::None)
                    .unwrap();
                assert_eq!(d.snapshot().layer_edit_target, LayerEditTarget::None);
                d.select_layer(id.clone()).unwrap();
                assert_eq!(d.layer_edit_target(), LayerEditTarget::Content);
                d.select_layer_target(id.clone(), LayerEditTarget::Mask)
                    .unwrap();
                d.set_layer_effects(&id, LayerEffects::default()).unwrap();
                assert_eq!(d.layer_edit_target(), LayerEditTarget::None);
            }
        }
    }
    #[test]
    fn transformed_edits_clip_in_document_space_and_follow_later_mask_moves() {
        for kind in [MaskKind::Pixel, MaskKind::Vector] {
            let mut m = LayerMask::from_selection(kind, None, 16, 16).unwrap();
            m.transform_by([0., 1., -1., 0., 20., 2.]).unwrap();
            let region = Selection::new(SelectionShape::Rectangle, [5., 5., 10., 10.]);
            let clip = Selection::new(SelectionShape::Rectangle, [10., 0., 8., 20.]);
            m.paint_selection(&region, Some(&clip), false).unwrap();
            assert_eq!(m.coverage([12.5, 8.5]), 0.);
            assert_eq!(m.coverage([8.5, 8.5]), 1.);
            m.transform_by([1., 0., 0., 1., 10., 0.]).unwrap();
            assert_eq!(m.coverage([22.5, 8.5]), 0.);
            assert_eq!(m.coverage([18.5, 8.5]), 1.);
            m.inverted = true;
            m.paint_selection(
                &Selection::new(SelectionShape::Rectangle, [15., 5., 5., 5.]),
                None,
                true,
            )
            .unwrap();
            assert_eq!(m.coverage([18.5, 8.5]), 1.);
            m.validate().unwrap();
        }
    }
    #[test]
    fn pixel_expansion_preserves_old_coverage_and_reveals_outside_local_bounds() {
        let mut m = LayerMask::from_selection(MaskKind::Pixel, None, 8, 8).unwrap();
        m.paint_selection(
            &Selection::new(SelectionShape::Rectangle, [-4., -3., 2., 2.]),
            None,
            true,
        )
        .unwrap();
        assert_eq!(m.coverage([-3.5, -2.5]), 1.);
        assert_eq!(m.coverage([-1.5, -1.5]), 0.);
        assert_eq!(m.coverage([7.5, 7.5]), 1.);
        assert_eq!(m.coverage([8.5, 7.5]), 0.);
        m.validate().unwrap();
    }
    #[test]
    fn vector_edits_preserve_affine_circles_and_reject_invalid_changes_atomically() {
        let mut m = LayerMask::from_selection(MaskKind::Vector, None, 32, 32).unwrap();
        m.transform_by([2., 0.3, 0.4, 1., 0., 0.]).unwrap();
        let circle = Selection::new(SelectionShape::Ellipse, [10., 10., 8., 8.]);
        let before = m.clone();
        m.paint_selection(&circle, None, false).unwrap();
        for y in 0..32 {
            for x in 0..32 {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                assert_eq!(
                    m.coverage(p),
                    if circle.contains(crate::document::Point { x: p[0], y: p[1] }) {
                        0.
                    } else {
                        before.coverage(p)
                    }
                );
            }
        }
        let before = m.clone();
        let mut invalid = circle;
        invalid.regions[0].bounds[0] = f32::NAN;
        assert!(m.paint_selection(&invalid, None, true).is_err());
        assert_eq!(m, before);
    }
}
