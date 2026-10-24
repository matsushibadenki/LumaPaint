//! Saved-selection composition without whole-page downsampling.
use crate::{
    document::Point,
    selection::{Selection, SelectionMode, SelectionOperation, SelectionShape},
    selection_raster::SelectionRaster,
};

impl Selection {
    pub fn combined(
        &self,
        other: &Self,
        mode: SelectionMode,
        dimensions: (u32, u32),
    ) -> Result<Self, String> {
        self.validate()?;
        other.validate()?;
        if mode == SelectionMode::Replace {
            return Ok(other.clone());
        }
        if dimensions.0 == 0 || dimensions.1 == 0 {
            return Err("Invalid selection dimensions".into());
        }
        // A single hard shape is an exact boolean operand. Retain geometry and
        // shared raster runs, even for page-sized selections above the budget.
        if other.raster.is_none() && other.regions.len() == 1 {
            let region = &other.regions[0];
            if region.paint.is_none()
                && region.operation == SelectionOperation::Replace
                && region.shape != SelectionShape::Stroke
                && self.ensure_capacity().is_ok()
            {
                let mut result = self.clone();
                let mut operand = region.clone();
                operand.operation = match mode {
                    SelectionMode::Add => SelectionOperation::Add,
                    SelectionMode::Subtract => SelectionOperation::Subtract,
                    SelectionMode::Intersect => SelectionOperation::Intersect,
                    SelectionMode::Replace => unreachable!(),
                };
                result.regions.push(operand);
                if result.coverage_bounds().is_none_or(|b| {
                    b[0] >= 0.
                        && b[1] >= 0.
                        && b[2] <= dimensions.0 as f32
                        && b[3] <= dimensions.1 as f32
                }) && result.validate().is_ok()
                {
                    return Ok(result);
                }
            }
        }
        let bounds = match (self.coverage_bounds(), other.coverage_bounds(), mode) {
            (Some(a), Some(b), SelectionMode::Add) => Some([
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]),
            (Some(a), Some(b), SelectionMode::Intersect) => Some([
                a[0].max(b[0]),
                a[1].max(b[1]),
                a[2].min(b[2]),
                a[3].min(b[3]),
            ]),
            (a, _, SelectionMode::Subtract) => a,
            (a, b, SelectionMode::Add) => a.or(b),
            _ => None,
        };
        let b = bounds.unwrap_or([0.; 4]);
        let left = (b[0].floor() - 1.).clamp(0., dimensions.0 as f32);
        let top = (b[1].floor() - 1.).clamp(0., dimensions.1 as f32);
        let right = (b[2].ceil() + 1.).clamp(left, dimensions.0 as f32);
        let bottom = (b[3].ceil() + 1.).clamp(top, dimensions.1 as f32);
        if bounds.is_none() || right <= left || bottom <= top {
            return Self::from_raster(SelectionRaster::from_pixels(
                [1, 1],
                [1., 0., 0., 1., 0., 0.],
                &[0],
            )?);
        }
        let size = [(right - left) as u32, (bottom - top) as u32];
        Self::from_raster(SelectionRaster::from_coverage(
            size,
            [1., 0., 0., 1., left, top],
            |x, y| {
                let point = Point {
                    x: left + x as f32 + 0.5,
                    y: top + y as f32 + 0.5,
                };
                let a = self.coverage(point);
                let b = other.coverage(point);
                let value = match mode {
                    SelectionMode::Add => a.max(b),
                    SelectionMode::Subtract => a * (1. - b),
                    SelectionMode::Intersect => a.min(b),
                    SelectionMode::Replace => unreachable!(),
                };
                (value * 255.).round() as u8
            },
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, PixelSelectionRequest};

    fn mask(pixels: &[u8]) -> Selection {
        Selection::from_raster(
            SelectionRaster::from_pixels([3, 1], [1., 0., 0., 1., 3000., 2000.], pixels).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn selection_combine_large_complex_native_masks_keep_single_pixel_holes_and_gray() {
        let dimensions = (4096, 2048);
        let mut base = Selection::new(SelectionShape::Rectangle, [0., 0., 4096., 2048.]);
        let mut hole = Selection::new(SelectionShape::Rectangle, [3000., 1800., 1., 1.])
            .regions
            .remove(0);
        hole.operation = SelectionOperation::Subtract;
        base.regions.push(hole);
        let mut other = Selection::from_raster(
            SelectionRaster::from_pixels([1, 1], [1., 0., 0., 1., 3001., 1800.], &[127]).unwrap(),
        )
        .unwrap();
        other.invert(dimensions.0, dimensions.1).unwrap();
        for mode in [
            SelectionMode::Add,
            SelectionMode::Subtract,
            SelectionMode::Intersect,
        ] {
            let result = base.combined(&other, mode, dimensions).unwrap();
            let mask = result.raster.as_ref().unwrap();
            assert_eq!(mask.size, [4096, 2048]);
            assert_eq!(mask.transform, [1., 0., 0., 1., 0., 0.]);
            assert!(mask.runs.len() < 8);
            for y in 1799..1802 {
                for x in 2999..3003 {
                    let point = Point {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    };
                    let a = base.coverage(point);
                    let b = other.coverage(point);
                    let expected = match mode {
                        SelectionMode::Add => a.max(b),
                        SelectionMode::Subtract => a * (1. - b),
                        _ => a.min(b),
                    };
                    assert!((result.coverage(point) - expected).abs() <= 0.5 / 255. + 1e-6);
                }
            }
            let loaded: Selection =
                serde_json::from_slice(&serde_json::to_vec(&result).unwrap()).unwrap();
            assert_eq!(loaded, result);
            let moved = result.transformed([1., 0., 0., 1., 1., 0.]).unwrap();
            assert!(std::sync::Arc::ptr_eq(
                &moved.raster.as_ref().unwrap().runs,
                &mask.runs
            ));
        }
    }

    #[test]
    fn selection_combine_keeps_page_sized_geometry_and_one_pixel_holes_exact() {
        let base = Selection::new(SelectionShape::Rectangle, [0., 0., 8192., 8192.]);
        let narrow = Selection::new(SelectionShape::Rectangle, [3000., 2000., 1., 120.]);
        for mode in [
            SelectionMode::Add,
            SelectionMode::Subtract,
            SelectionMode::Intersect,
        ] {
            let result = base.combined(&narrow, mode, (8192, 8192)).unwrap();
            assert!(result.raster.is_none());
            if mode == SelectionMode::Intersect {
                assert_eq!(
                    result.snapshot_summary().regions[0].bounds,
                    [3000., 2000., 1., 120.]
                );
            }
            for x in [2999.5, 3000.5, 3001.5, 7000.5] {
                let selected = match mode {
                    SelectionMode::Add => true,
                    SelectionMode::Subtract => x != 3000.5,
                    _ => x == 3000.5,
                };
                assert_eq!(result.coverage(Point { x, y: 2000.5 }), f32::from(selected));
            }
        }
        let gray = mask(&[1, 127, 254]);
        let result = gray
            .combined(&narrow, SelectionMode::Add, (8192, 8192))
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &gray.raster.as_ref().unwrap().runs,
            &result.raster.as_ref().unwrap().runs
        ));
        let distant = Selection::new(SelectionShape::Rectangle, [4000., 2000., 1., 2.]);
        let union = gray
            .combined(&distant, SelectionMode::Add, (8192, 8192))
            .unwrap();
        assert_eq!(
            union.snapshot_summary().regions[0].bounds,
            [3000., 2000., 1001., 2.]
        );
        let clipped = gray
            .combined(&narrow, SelectionMode::Intersect, (8192, 8192))
            .unwrap();
        assert_eq!(
            clipped.snapshot_summary().regions[0].bounds,
            [3000., 2000., 1., 1.]
        );
    }

    #[test]
    fn selection_combine_gray_masks_match_coverage_and_keep_native_crop() {
        let base = mask(&[1, 127, 254]);
        let other = mask(&[128, 64, 200]);
        for mode in [
            SelectionMode::Add,
            SelectionMode::Subtract,
            SelectionMode::Intersect,
        ] {
            let result = base.combined(&other, mode, (8192, 8192)).unwrap();
            let raster = result.raster.as_ref().unwrap();
            assert_eq!(raster.size, [5, 3]);
            assert_eq!(&raster.transform[..4], &[1., 0., 0., 1.]);
            for i in 0..3 {
                let point = Point {
                    x: 3000.5 + i as f32,
                    y: 2000.5,
                };
                let a = base.coverage(point);
                let b = other.coverage(point);
                let expected = match mode {
                    SelectionMode::Add => a.max(b),
                    SelectionMode::Subtract => a * (1. - b),
                    _ => a.min(b),
                };
                assert!((result.coverage(point) - expected).abs() <= 0.5 / 255. + 1e-6);
            }
            assert_eq!(
                result.coverage(Point {
                    x: 2999.5,
                    y: 2000.5
                }),
                0.
            );
        }
        let mut inverse = other.clone();
        inverse.invert(8192, 8192).unwrap();
        assert_eq!(
            inverse.snapshot_summary().regions[0].bounds,
            [0., 0., 8192., 8192.]
        );
        let intersection = base
            .combined(&inverse, SelectionMode::Intersect, (8192, 8192))
            .unwrap();
        assert_eq!(intersection.raster.as_ref().unwrap().size, [5, 3]);
        assert!(
            (intersection.coverage(Point {
                x: 3001.5,
                y: 2000.5
            }) - 127. / 255.)
                .abs()
                < 1. / 255.
        );
    }

    #[test]
    fn selection_combine_empty_and_large_complex_masks_stay_bounded() {
        let base = mask(&[1, 127, 254]);
        let other = mask(&[128, 64, 200]).translated(100., 0.);
        let empty = base
            .combined(&other, SelectionMode::Intersect, (8192, 8192))
            .unwrap();
        assert_eq!(
            empty.coverage(Point {
                x: 3001.5,
                y: 2000.5
            }),
            0.
        );
        assert_eq!(empty.coverage(Point { x: 10., y: 10. }), 0.);
        assert!(empty.raster.as_ref().unwrap().runs.is_empty());
        let mut inverse = other;
        inverse.invert(8192, 8192).unwrap();
        let result = base
            .combined(&inverse, SelectionMode::Add, (8192, 8192))
            .unwrap();
        let size = result.raster.as_ref().unwrap().size;
        assert_eq!(size, [8192, 8192]);
        assert!(
            u64::from(size[0]) * u64::from(size[1]) <= crate::selection_raster::MAX_SELECTION_AREA
        );
    }

    #[test]
    fn selection_saved_load_composition_roundtrips_and_undoes_once() {
        let mut state = Document::default().document_state();
        state.width = 8192;
        state.height = 8192;
        let mut doc = Document::from_document_state(state).unwrap();
        let saved = mask(&[128, 64, 200]);
        doc.set_tool_pixel_selection(Some(saved.clone())).unwrap();
        doc.pixel_selection_action(PixelSelectionRequest {
            action: "save".into(),
            name: Some("Fine edge".into()),
            ..Default::default()
        })
        .unwrap();
        let base = mask(&[1, 127, 254]);
        doc.set_tool_pixel_selection(Some(base.clone())).unwrap();
        let revision = doc.revision();
        let request = PixelSelectionRequest {
            action: "load".into(),
            name: Some("Fine edge".into()),
            mode: Some(SelectionMode::Subtract),
            ..Default::default()
        };
        let evaluated = doc.loaded_pixel_selection(&request).unwrap();
        assert_eq!(doc.selection(), Some(&base));
        assert_eq!(doc.revision(), revision);
        doc.pixel_selection_action(request.clone()).unwrap();
        assert_eq!(doc.selection(), Some(&evaluated));
        assert_eq!(doc.revision(), revision + 1);
        doc.undo();
        assert_eq!(doc.selection(), Some(&base));
        doc.redo();
        assert_eq!(doc.selection(), Some(&evaluated));
        let loaded = Document::from_document_state(
            serde_json::from_slice(&serde_json::to_vec(&doc.document_state()).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(loaded.selection(), Some(&evaluated));
        assert_eq!(
            loaded
                .saved_pixel_selection(Some("Fine edge"))
                .unwrap()
                .as_ref(),
            &saved
        );
        let before = doc.encode().unwrap();
        assert!(doc
            .pixel_selection_action(PixelSelectionRequest {
                name: Some("Missing".into()),
                ..request
            })
            .is_err());
        assert_eq!(doc.encode().unwrap(), before);
    }
}
