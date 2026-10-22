//! Bounded grayscale selection masks. Shared between document/history/render forks;
//! UI snapshots expose only bounds, never these pixels.
use crate::{document::Point, image_frame::inverse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MAX_SELECTION_PIXELS: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionInterpolation {
    #[default]
    Nearest,
    Linear,
}
impl SelectionInterpolation {
    fn is_nearest(&self) -> bool {
        *self == Self::Nearest
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectionRaster {
    pub size: [u32; 2],
    pub transform: [f32; 6],
    /// Sorted nonzero runs: linear start, length, alpha (1..255).
    pub runs: Arc<Vec<[u32; 3]>>,
    /// Legacy masks keep their original sampling until an affine resampling edit.
    #[serde(default, skip_serializing_if = "SelectionInterpolation::is_nearest")]
    pub interpolation: SelectionInterpolation,
}
impl SelectionRaster {
    pub fn from_pixels(size: [u32; 2], transform: [f32; 6], pixels: &[u8]) -> Result<Self, String> {
        let count = u64::from(size[0]) * u64::from(size[1]);
        if count > MAX_SELECTION_PIXELS || pixels.len() as u64 != count {
            return Err("Invalid selection pixels".into());
        }
        let mut runs = vec![];
        let mut at = 0;
        while at < pixels.len() {
            if pixels[at] == 0 {
                at += 1;
                continue;
            }
            let start = at;
            let value = pixels[at];
            while at < pixels.len() && pixels[at] == value {
                at += 1;
            }
            runs.push([start as u32, (at - start) as u32, u32::from(value)]);
        }
        let result = Self {
            size,
            transform,
            runs: Arc::new(runs),
            interpolation: SelectionInterpolation::Nearest,
        };
        result.validate()?;
        Ok(result)
    }
    pub fn validate(&self) -> Result<(), String> {
        let count = u64::from(self.size[0]) * u64::from(self.size[1]);
        if self.size.contains(&0)
            || count > MAX_SELECTION_PIXELS
            || self
                .transform
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 32768.)
            || inverse(self.transform).is_err()
            || self.runs.len() > 1024 * 1024
        {
            return Err(
                "Invalid selection mask / 選択マスクが不正か大きすぎます / 选区蒙版无效或过大"
                    .into(),
            );
        }
        let mut end = 0u64;
        for &[start, len, value] in self.runs.iter() {
            if u64::from(start) < end
                || len == 0
                || value == 0
                || value > 255
                || u64::from(start) + u64::from(len) > count
            {
                return Err("Invalid selection run".into());
            }
            end = u64::from(start) + u64::from(len);
        }
        Ok(())
    }
    pub fn pixels(&self) -> Vec<u8> {
        let mut out = vec![0; self.size[0] as usize * self.size[1] as usize];
        for &[s, n, v] in self.runs.iter() {
            out[s as usize..(s + n) as usize].fill(v as u8);
        }
        out
    }
    pub fn coverage(&self, point: Point) -> f32 {
        if !point.x.is_finite() || !point.y.is_finite() {
            return 0.;
        }
        let Ok([a, b, c, d, e, f]) = inverse(self.transform) else {
            return 0.;
        };
        let x = a * point.x + c * point.y + e;
        let y = b * point.x + d * point.y + f;
        if !x.is_finite() || !y.is_finite() {
            return 0.;
        }
        if self.interpolation == SelectionInterpolation::Nearest {
            return self.sample(x.floor(), y.floor());
        }
        // Texels are centered at half-integer coordinates. Outside the stored
        // footprint is transparent, including the interpolation fringe.
        let x = x - 0.5;
        let y = y - 0.5;
        let (l, t) = (x.floor(), y.floor());
        let (fx, fy) = (x - l, y - t);
        let top = self.sample(l, t) * (1. - fx) + self.sample(l + 1., t) * fx;
        let bottom = self.sample(l, t + 1.) * (1. - fx) + self.sample(l + 1., t + 1.) * fx;
        top * (1. - fy) + bottom * fy
    }
    fn sample(&self, x: f32, y: f32) -> f32 {
        if x < 0. || y < 0. || x >= self.size[0] as f32 || y >= self.size[1] as f32 {
            return 0.;
        }
        let index = y as u32 * self.size[0] + x as u32;
        let next = self.runs.partition_point(|r| r[0] <= index);
        if next > 0 {
            let r = self.runs[next - 1];
            if index < r[0] + r[1] {
                return r[2] as f32 / 255.;
            }
        }
        0.
    }
    pub fn transformed(&self, matrix: [f32; 6]) -> Result<Self, String> {
        let mut next = self.clone();
        next.transform = crate::image_frame::multiply(matrix, self.transform);
        next.validate()?;
        Ok(next)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{Selection, SelectionOperation, SelectionRegion, SelectionShape};
    #[test]
    fn selection_raster_linear_sampling_keeps_source_and_legacy_coverage() {
        let mask: SelectionRaster = serde_json::from_str(
            r#"{"size":[2,2],"transform":[1,0,0,1,0,0],"runs":[[1,1,255],[2,1,64],[3,1,128]]}"#,
        )
        .unwrap();
        assert_eq!(mask.coverage(Point { x: 0.99, y: 0.5 }), 0.);
        let mut scaled = mask.transformed([2., 0., 0., 2., 0., 0.]).unwrap();
        assert_eq!(scaled.interpolation, SelectionInterpolation::Nearest);
        scaled.interpolation = SelectionInterpolation::Linear;
        assert!((scaled.coverage(Point { x: 2., y: 1. }) - 0.5).abs() < 1e-6);
        assert!(
            (scaled.coverage(Point { x: 2., y: 2. }) - (255. + 64. + 128.) / (4. * 255.)).abs()
                < 1e-6
        );
        assert_eq!(scaled.coverage(Point { x: -2., y: 1. }), 0.);
        let mut fractional = mask.transformed([1., 0., 0., 1., 0.25, 0.]).unwrap();
        fractional.interpolation = SelectionInterpolation::Linear;
        assert!((fractional.coverage(Point { x: 1., y: 0.5 }) - 0.25).abs() < 1e-6);
        let restored = scaled.transformed([0.5, 0., 0., 0.5, 0., 0.]).unwrap();
        assert!(Arc::ptr_eq(&mask.runs, &restored.runs));
        assert_eq!(restored.pixels(), mask.pixels());
        for y in 0..2 {
            for x in 0..2 {
                let p = Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                assert_eq!(restored.coverage(p), mask.coverage(p));
            }
        }
        let loaded: SelectionRaster =
            serde_json::from_slice(&serde_json::to_vec(&scaled).unwrap()).unwrap();
        assert_eq!(loaded, scaled);
        assert_eq!(loaded.coverage(Point { x: 2., y: 1. }), 0.5);
        let mut solid = SelectionRaster::from_pixels([1, 1], [1., 0., 0., 1., 0., 0.], &[255])
            .unwrap()
            .transformed([2., 0., 0., 2., 0., 0.])
            .unwrap();
        solid.interpolation = SelectionInterpolation::Linear;
        assert_eq!(solid.coverage(Point { x: 0.5, y: 0.5 }), 0.5625);
        assert_eq!(
            solid.coverage(Point {
                x: f32::MAX,
                y: f32::MAX
            }),
            0.
        );
    }
    #[test]
    fn selection_raster_roundtrip_transform_and_zero_are_bounded() {
        let s = Selection::from_raster(
            SelectionRaster::from_pixels([4, 1], [1., 0., 0., 1., 0., 0.], &[0, 128, 255, 0])
                .unwrap(),
        )
        .unwrap();
        assert_eq!(s.coverage(Point { x: 2.5, y: 0.5 }), 1.);
        assert!((s.coverage(Point { x: 1.5, y: 0.5 }) - 0.5).abs() < 0.01);
        let saved = serde_json::to_vec(&s).unwrap();
        let loaded: Selection = serde_json::from_slice(&saved).unwrap();
        assert_eq!(s, loaded);
        let moved = s.translated(10., 20.);
        assert_eq!(moved.coverage(Point { x: 12.5, y: 20.5 }), 1.);
        assert_eq!(moved.coverage(Point { x: 2.5, y: 0.5 }), 0.);
        let mut cut = s.clone();
        cut.regions.push(SelectionRegion {
            paint: None,
            shape: SelectionShape::Rectangle,
            points: vec![],
            radius: 0.,
            bounds: [2., 0., 1., 1.],
            operation: SelectionOperation::Subtract,
        });
        cut.validate().unwrap();
        assert_eq!(cut.coverage(Point { x: 2.5, y: 0.5 }), 0.);
        let empty = Selection::from_raster(
            SelectionRaster::from_pixels([2, 2], [1., 0., 0., 1., 0., 0.], &[0; 4]).unwrap(),
        )
        .unwrap();
        assert!(!empty.contains(Point { x: 0.5, y: 0.5 }));
        empty.validate().unwrap();
        assert!(
            SelectionRaster::from_pixels([u32::MAX, u32::MAX], [1., 0., 0., 1., 0., 0.], &[])
                .is_err()
        );
    }
    #[test]
    fn selection_raster_document_snapshot_omits_pixels_and_masks_keep_gray() {
        use crate::{
            document::Document,
            layer_mask::{LayerMask, MaskKind},
        };
        let selection = Selection::from_raster(
            SelectionRaster::from_pixels(
                [4, 2],
                [1., 0., 0., 1., 0., 0.],
                &[0, 128, 255, 0, 0, 128, 255, 0],
            )
            .unwrap(),
        )
        .unwrap();
        for kind in [MaskKind::Pixel, MaskKind::Vector] {
            let mask = LayerMask::from_selection(kind, Some(&selection), 4, 2).unwrap();
            mask.validate().unwrap();
            assert!((mask.coverage([1.5, 0.5]) - 128. / 255.).abs() < 0.001);
            assert_eq!(mask.coverage([2.5, 1.5]), 1.);
        }
        let mut doc = Document::default();
        doc.set_tool_pixel_selection(Some(selection.clone()))
            .unwrap();
        let summary = doc.snapshot();
        assert!(summary.selection.unwrap().raster.is_none());
        assert!(doc
            .document_state()
            .pixel_selection
            .unwrap()
            .raster
            .is_some());
    }
}
