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
