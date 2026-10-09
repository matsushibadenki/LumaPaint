//! Layer-independent masks. Raster coverage and vector geometry stay in the core.
use crate::{
    document::Point,
    selection::{Selection, SelectionShape},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MaskKind {
    Pixel,
    Vector,
}
/// Transient editing focus, shared by all views of a document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LayerEditTarget {
    #[default]
    Content,
    Mask,
    None,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaskEdit {
    pub selection: Selection,
    pub clip: Option<Selection>,
    /// Maps mask-local coordinates to the geometry's original document space.
    pub to_geometry: [f32; 6],
    pub reveal: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum MaskContent {
    Pixel {
        width: u32,
        height: u32,
        runs: Vec<[u32; 2]>,
    },
    Vector {
        selection: Selection,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        edits: Vec<MaskEdit>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerMask {
    #[serde(default = "default_linked")]
    pub linked: bool,
    #[serde(default = "identity_transform")]
    pub transform: [f32; 6],
    pub kind: MaskKind,
    pub enabled: bool,
    pub inverted: bool,
    pub density: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MaskContent>,
}
pub fn default_linked() -> bool {
    true
}
pub fn identity_transform() -> [f32; 6] {
    [1., 0., 0., 1., 0., 0.]
}
impl LayerMask {
    pub fn from_selection(
        kind: MaskKind,
        selection: Option<&Selection>,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let full = Selection::new(
            SelectionShape::Rectangle,
            [0., 0., width as f32, height as f32],
        );
        let selection = selection.unwrap_or(&full);
        selection.validate()?;
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return Err("Invalid mask dimensions".into());
        }
        let content = match kind {
            MaskKind::Vector => MaskContent::Vector {
                selection: selection.clone(),
                edits: Vec::new(),
            },
            MaskKind::Pixel => {
                let mut runs = Vec::new();
                let rectangle = selection.regions.len() == 1
                    && selection.regions[0].shape == SelectionShape::Rectangle;
                let mut left = width;
                let mut top = height;
                let mut right = 0;
                let mut bottom = 0;
                for region in &selection.regions {
                    let [x, y, w, h] = region.bounds;
                    left = left.min((x - 0.5).ceil().clamp(0., width as f32) as u32);
                    top = top.min((y - 0.5).ceil().clamp(0., height as f32) as u32);
                    right = right.max(
                        (x + w + if rectangle { -0.5 } else { 0.5 })
                            .ceil()
                            .clamp(0., width as f32) as u32,
                    );
                    bottom = bottom.max(
                        (y + h + if rectangle { -0.5 } else { 0.5 })
                            .ceil()
                            .clamp(0., height as f32) as u32,
                    );
                }
                // Rectangular/all-visible masks need one run per row, no pixel scan.
                for y in top..bottom {
                    if rectangle {
                        if right > left {
                            runs.push([y * width + left, right - left]);
                        }
                        continue;
                    }
                    let mut start = None;
                    for x in left..=right {
                        let hit = x < right
                            && selection.contains(Point {
                                x: x as f32 + 0.5,
                                y: y as f32 + 0.5,
                            });
                        if hit && start.is_none() {
                            start = Some(x);
                        }
                        if !hit {
                            if let Some(start) = start.take() {
                                runs.push([y * width + start, x - start]);
                            }
                        }
                    }
                    if runs.len() > 1_048_576 {
                        return Err("Pixel mask complexity limit exceeded".into());
                    }
                }
                MaskContent::Pixel {
                    width,
                    height,
                    runs,
                }
            }
        };
        Ok(Self {
            linked: true,
            transform: identity_transform(),
            kind,
            enabled: true,
            inverted: false,
            density: 1.,
            content: Some(content),
        })
    }
    pub fn transform_by(&mut self, matrix: [f32; 6]) -> Result<(), String> {
        let next = crate::image_frame::multiply(matrix, self.transform);
        validate_transform(next)?;
        self.transform = next;
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        validate_transform(self.transform)?;
        if !self.density.is_finite() || !(0. ..=1.).contains(&self.density) {
            return Err("Invalid mask density".into());
        }
        match (&self.kind, &self.content) {
            (MaskKind::Vector, Some(MaskContent::Vector { selection, edits })) => {
                selection.validate()?;
                if edits.len() > 1024 {
                    return Err("Vector mask complexity limit exceeded".into());
                }
                let mut points = 0;
                for edit in edits {
                    validate_transform(edit.to_geometry)?;
                    edit.selection.validate()?;
                    if let Some(clip) = &edit.clip {
                        clip.validate()?;
                    }
                    points += edit
                        .selection
                        .regions
                        .iter()
                        .map(|r| r.points.len() + 8)
                        .sum::<usize>();
                    points += edit
                        .clip
                        .iter()
                        .flat_map(|s| &s.regions)
                        .map(|r| r.points.len() + 8)
                        .sum::<usize>();
                }
                if points > 262_144 {
                    return Err("Vector mask complexity limit exceeded".into());
                }
                Ok(())
            }
            (
                MaskKind::Pixel,
                Some(MaskContent::Pixel {
                    width,
                    height,
                    runs,
                }),
            ) => {
                if *width == 0
                    || *height == 0
                    || *width > 8192
                    || *height > 8192
                    || runs.len() > 1_048_576
                {
                    return Err("Invalid pixel mask".into());
                }
                let mut end = 0;
                for [start, len] in runs {
                    if *len == 0
                        || *start < end
                        || *start >= width * height
                        || *len > width - start % width
                    {
                        return Err("Invalid mask run".into());
                    }
                    end = start + len;
                }
                Ok(())
            }
            _ => Err("Missing or mismatched mask content".into()),
        }
    }
    pub fn coverage(&self, point: [f32; 2]) -> f32 {
        if !self.enabled {
            return 1.;
        }
        let Ok([a, b, c, d, e, f]) = crate::image_frame::inverse(self.transform) else {
            return 0.;
        };
        let point = [
            a * point[0] + c * point[1] + e,
            b * point[0] + d * point[1] + f,
        ];
        let hit = match &self.content {
            Some(MaskContent::Vector { selection, edits }) => {
                let mut hit = selection.contains(Point {
                    x: point[0],
                    y: point[1],
                });
                for edit in edits {
                    let p = mapped(edit.to_geometry, point);
                    if edit.selection.contains(p)
                        && edit.clip.as_ref().is_none_or(|clip| clip.contains(p))
                    {
                        hit = edit.reveal;
                    }
                }
                hit
            }
            Some(MaskContent::Pixel {
                width,
                height,
                runs,
            }) => {
                let x = point[0].floor();
                let y = point[1].floor();
                if x < 0. || y < 0. || x >= *width as f32 || y >= *height as f32 {
                    false
                } else {
                    let index = y as u32 * width + x as u32;
                    let at = runs.partition_point(|r| r[0] <= index);
                    at > 0 && index < runs[at - 1][0] + runs[at - 1][1]
                }
            }
            None => true,
        };
        let value = f32::from(hit);
        1. - self.density + self.density * if self.inverted { 1. - value } else { value }
    }
    pub fn hash_state(&self, hash: &mut impl std::hash::Hasher) {
        hash.write_u8(u8::from(self.linked));
        for v in self.transform {
            hash.write_u32(v.to_bits());
        }
        hash.write_u8(self.kind as u8);
        hash.write_u8(u8::from(self.enabled));
        hash.write_u8(u8::from(self.inverted));
        hash.write_u32(self.density.to_bits());
        match &self.content {
            Some(MaskContent::Pixel {
                width,
                height,
                runs,
            }) => {
                hash.write_u32(*width);
                hash.write_u32(*height);
                hash.write_usize(runs.len());
                for run in runs {
                    hash.write_u32(run[0]);
                    hash.write_u32(run[1]);
                }
            }
            Some(MaskContent::Vector { selection, edits }) => {
                // Include exact geometry and placement in renderer cache keys.
                if let Ok(bytes) = serde_json::to_vec(edits) {
                    hash.write(&bytes);
                }
                hash.write_usize(selection.regions.len());
                for r in &selection.regions {
                    hash.write_u8(r.shape as u8);
                    hash.write_u8(r.operation as u8);
                    hash.write_u32(r.radius.to_bits());
                    for v in r.bounds {
                        hash.write_u32(v.to_bits());
                    }
                    hash.write_usize(r.points.len());
                    for p in &r.points {
                        hash.write_u32(p.x.to_bits());
                        hash.write_u32(p.y.to_bits());
                    }
                }
            }
            None => {}
        }
    }
    pub fn summary(&self) -> Self {
        Self {
            linked: self.linked,
            transform: self.transform,
            kind: self.kind,
            enabled: self.enabled,
            inverted: self.inverted,
            density: self.density,
            content: None,
        }
    }
}

pub(crate) fn mapped(m: [f32; 6], p: [f32; 2]) -> Point {
    Point {
        x: m[0] * p[0] + m[2] * p[1] + m[4],
        y: m[1] * p[0] + m[3] * p[1] + m[5],
    }
}

pub fn validate_transform(matrix: [f32; 6]) -> Result<(), String> {
    if matrix.iter().any(|v| !v.is_finite() || v.abs() > 1e10)
        || crate::image_frame::inverse(matrix)?
            .iter()
            .any(|v| !v.is_finite())
    {
        return Err("Invalid mask transform".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::Document, layer_effects::LayerEffects, selection::SelectionOperation};
    #[test]
    fn four_layer_mask_combinations_save_and_undo_without_sending_payload_to_ui() {
        for vector in [false, true] {
            for kind in [MaskKind::Pixel, MaskKind::Vector] {
                let mut doc = Document::default();
                let id = if vector {
                    doc.add_vector_layer().unwrap()
                } else {
                    doc.add_paint_layer().unwrap()
                };
                let selection = Selection::new(SelectionShape::Ellipse, [4., 4., 8., 8.]);
                let mask = LayerMask::from_selection(kind, Some(&selection), 16, 16).unwrap();
                let mut effects = LayerEffects {
                    mask: Some(mask.clone()),
                    ..Default::default()
                };
                doc.set_layer_effects(&id, effects.clone()).unwrap();
                assert!(doc.has_layer_effects(&id));
                let snap = doc.snapshot();
                let summary = snap
                    .layers
                    .iter()
                    .find(|l| l.id == id)
                    .unwrap()
                    .effects
                    .as_ref()
                    .unwrap();
                assert!(summary.mask.as_ref().unwrap().content.is_none());
                let mut summary = summary.clone();
                summary.mask.as_mut().unwrap().inverted = true;
                doc.set_layer_effects(&id, summary).unwrap();
                assert_eq!(
                    doc.layer_effects(&id).mask.as_ref().unwrap().content,
                    mask.content
                );
                doc.undo();
                assert_eq!(doc.layer_effects(&id), effects);
                doc.redo();
                effects.mask.as_mut().unwrap().inverted = true;
                assert_eq!(doc.layer_effects(&id), effects);
                let json = serde_json::to_vec(&doc.document_state()).unwrap();
                let loaded =
                    Document::from_document_state(serde_json::from_slice(&json).unwrap()).unwrap();
                assert_eq!(loaded.layer_effects(&id), effects);
                assert_eq!(
                    loaded
                        .layer_effects(&id)
                        .prepare()
                        .apply_at([90, 60, 30, 128], [8., 8.])[3],
                    0
                );
                assert_eq!(
                    loaded
                        .layer_effects(&id)
                        .prepare()
                        .apply_at([90, 60, 30, 128], [0., 0.])[3],
                    128
                );
            }
        }
    }
    #[test]
    fn raster_and_vector_match_compound_selection_at_pixel_centers() {
        let mut s = Selection::new(SelectionShape::Rectangle, [2., 2., 12., 12.]);
        let mut hole = Selection::new(SelectionShape::Ellipse, [5., 5., 6., 6.])
            .regions
            .remove(0);
        hole.operation = SelectionOperation::Subtract;
        s.regions.push(hole);
        let pixel = LayerMask::from_selection(MaskKind::Pixel, Some(&s), 16, 16).unwrap();
        let vector = LayerMask::from_selection(MaskKind::Vector, Some(&s), 16, 16).unwrap();
        for y in -1..17 {
            for x in -1..17 {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                assert_eq!(pixel.coverage(p), vector.coverage(p));
            }
        }
        let mut m = pixel;
        m.density = 0.5;
        assert_eq!(m.coverage([0., 0.]), 0.5);
        m.enabled = false;
        assert_eq!(m.coverage([0., 0.]), 1.);
    }
    #[test]
    fn rejects_malformed_runs_and_geometry() {
        let mut mask = LayerMask::from_selection(MaskKind::Pixel, None, 8, 8).unwrap();
        mask.content = Some(MaskContent::Pixel {
            width: 8,
            height: 8,
            runs: vec![[7, 2]],
        });
        assert!(mask.validate().is_err());
        mask.content = None;
        assert!(mask.validate().is_err());
        assert!(LayerMask::from_selection(MaskKind::Vector, None, 0, 8).is_err());
    }
}
