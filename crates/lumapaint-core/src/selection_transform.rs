//! Selection-only affine edits. Sampling is bounded by the selected footprint,
//! independent of the document canvas size; source masks survive repeated edits.
use crate::{
    document::Point,
    selection::{Selection, SelectionOperation, SelectionShape},
};
use std::sync::Arc;

impl Selection {
    pub fn transformed(&self, matrix: [f32; 6]) -> Result<Self, String> {
        self.validate()?;
        if matrix == [1., 0., 0., 1., 0., 0.] {
            return Ok(self.clone());
        }
        let mut result = self.clone();
        let [a, b, c, d, e, f] = matrix;
        if [a, b, c, d, e, f]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 32768.)
            || (a * d - b * c).abs() < 1e-6
        {
            return Err(
                "Invalid selection transform / 選択範囲の変形値が不正です / 选区变换无效".into(),
            );
        }
        if result
            .regions
            .iter()
            .any(|r| r.shape == SelectionShape::Stroke || r.operation == SelectionOperation::Invert)
        {
            result = result.rasterized_for_transform()?;
        }
        if let Some(mask) = &result.raster {
            let mut transformed = mask.transformed(matrix)?;
            if matrix[..4] != [1., 0., 0., 1.] || e.fract() != 0. || f.fract() != 0. {
                transformed.interpolation = crate::selection_raster::SelectionInterpolation::Linear;
            }
            result.raster = Some(Arc::new(transformed));
        }
        for region in &mut result.regions {
            let [x, y, w, h] = region.bounds;
            let points=match region.shape {
            SelectionShape::Rectangle=>vec![Point{x,y},Point{x:x+w,y},Point{x:x+w,y:y+h},Point{x,y:y+h}],
            SelectionShape::Ellipse=>(0..128).map(|i|{let t=i as f32*std::f32::consts::TAU/128.;Point{x:x+w*(0.5+0.5*t.cos()),y:y+h*(0.5+0.5*t.sin())}}).collect(),
            SelectionShape::Polygon=>region.points.clone(),
            SelectionShape::Stroke=>return Err("Convert brush selections before transforming / ブラシ選択は輪郭に変換してから変形してください / 请先将画笔选区转换为轮廓".into()),
        };
            region.points = points
                .into_iter()
                .map(|p| Point {
                    x: a * p.x + c * p.y + e,
                    y: b * p.x + d * p.y + f,
                })
                .collect();
            let minx = region
                .points
                .iter()
                .map(|p| p.x)
                .fold(f32::INFINITY, f32::min);
            let maxx = region
                .points
                .iter()
                .map(|p| p.x)
                .fold(f32::NEG_INFINITY, f32::max);
            let miny = region
                .points
                .iter()
                .map(|p| p.y)
                .fold(f32::INFINITY, f32::min);
            let maxy = region
                .points
                .iter()
                .map(|p| p.y)
                .fold(f32::NEG_INFINITY, f32::max);
            region.bounds = [minx, miny, (maxx - minx).max(1.), (maxy - miny).max(1.)];
            region.shape = SelectionShape::Polygon;
            region.radius = 0.;
            if region.operation == SelectionOperation::Invert {
                return Err("Rasterize inverse selections before transforming / 反転選択は輪郭に変換してから変形してください / 请先转换反选区域".into());
            }
        }
        result.validate()?;
        Ok(result)
    }

    /// Conservative nonzero support in document coordinates, including soft fringes.
    /// Ordered boolean operations can shrink or replace the support.
    pub fn coverage_bounds(&self) -> Option<[f32; 4]> {
        use crate::selection_raster::SelectionInterpolation;
        // Bounds follow boolean operations; subtraction cannot enlarge support.
        // Do not clip to the page: a selection can be moved off-page and back.
        let mut bounds = self
            .raster
            .as_ref()
            .filter(|m| !m.runs.is_empty())
            .map(|mask| {
                let pad = if mask.interpolation == SelectionInterpolation::Linear {
                    0.5
                } else {
                    0.
                };
                let [a, b, c, d, e, f] = mask.transform;
                let (w, h) = (mask.size[0] as f32, mask.size[1] as f32);
                point_bounds(
                    [
                        (-pad, -pad),
                        (w + pad, -pad),
                        (w + pad, h + pad),
                        (-pad, h + pad),
                    ]
                    .into_iter()
                    .map(|(x, y)| Point {
                        x: a * x + c * y + e,
                        y: b * x + d * y + f,
                    }),
                )
            });
        for region in &self.regions {
            let region_bounds = if region.points.is_empty() {
                let [x, y, w, h] = region.bounds;
                [x, y, x + w, y + h]
            } else {
                let mut b = point_bounds(region.points.iter().copied());
                let radius = if region.shape == SelectionShape::Stroke {
                    region.radius + if region.paint.is_some() { 1. } else { 0. }
                } else {
                    0.
                };
                b[0] -= radius;
                b[1] -= radius;
                b[2] += radius;
                b[3] += radius;
                b
            };
            bounds = if region.paint.is_some_and(|p| p[0] == 0.) {
                bounds
            } else {
                match region.operation {
                    SelectionOperation::Replace | SelectionOperation::Invert => Some(region_bounds),
                    SelectionOperation::Add => {
                        Some(bounds.map_or(region_bounds, |b| union(b, region_bounds)))
                    }
                    SelectionOperation::Subtract => bounds,
                    SelectionOperation::Intersect => bounds.and_then(|b| {
                        let v = [
                            b[0].max(region_bounds[0]),
                            b[1].max(region_bounds[1]),
                            b[2].min(region_bounds[2]),
                            b[3].min(region_bounds[3]),
                        ];
                        (v[2] > v[0] && v[3] > v[1]).then_some(v)
                    }),
                }
            };
        }
        bounds
    }

    fn rasterized_for_transform(&self) -> Result<Self, String> {
        use crate::selection_raster::{SelectionRaster, MAX_SELECTION_PIXELS};
        let bounds = self.coverage_bounds();
        let Some([left, top, right, bottom]) = bounds else {
            return Self::from_raster(SelectionRaster::from_pixels(
                [1, 1],
                [1., 0., 0., 1., 0., 0.],
                &[0],
            )?);
        };
        // Padding in document pixels retains the native-resolution bilinear fringe.
        let (left, top) = (left.floor() - 1., top.floor() - 1.);
        let (width, height) = (
            (right.ceil() + 1. - left) as f64,
            (bottom.ceil() + 1. - top) as f64,
        );
        let scale = (MAX_SELECTION_PIXELS as f64 / (width * height))
            .sqrt()
            .min(1.);
        let mut size = [
            (width * scale).floor().max(1.) as u32,
            (height * scale).floor().max(1.) as u32,
        ];
        size[0] = size[0].min((MAX_SELECTION_PIXELS / u64::from(size[1])) as u32);
        let sx = width as f32 / size[0] as f32;
        let sy = height as f32 / size[1] as f32;
        let pixels = (0..size[0] * size[1])
            .map(|i| {
                (255.
                    * self.coverage(Point {
                        x: left + (i % size[0]) as f32 * sx + 0.5 * sx,
                        y: top + (i / size[0]) as f32 * sy + 0.5 * sy,
                    }))
                .round() as u8
            })
            .collect::<Vec<_>>();
        Self::from_raster(SelectionRaster::from_pixels(
            size,
            [sx, 0., 0., sy, left, top],
            &pixels,
        )?)
    }
}

fn point_bounds(points: impl Iterator<Item = Point>) -> [f32; 4] {
    points.fold(
        [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ],
        |b, p| [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)],
    )
}
fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Document, PixelSelectionRequest},
        selection::{SelectionMode, SelectionRegion},
        selection_raster::MAX_SELECTION_PIXELS,
    };
    fn stroke(x: f32, y: f32, radius: f32) -> Selection {
        Selection {
            raster: None,
            regions: vec![SelectionRegion {
                paint: None,
                shape: SelectionShape::Stroke,
                points: vec![Point { x, y }],
                radius,
                bounds: [x - radius, y - radius, 2. * radius, 2. * radius],
                operation: SelectionOperation::Replace,
            }],
        }
    }
    #[test]
    fn selection_coverage_bounds_preserve_previous_coverage_when_painting() {
        let mut selection = Selection::new(SelectionShape::Rectangle, [10., 10., 2., 2.]);
        let mut painted = stroke(100.5, 100.5, 2.).regions.remove(0);
        painted.paint = Some([0.5, 0.2]);
        painted.operation = SelectionOperation::Add;
        selection.regions.push(painted);
        let mut erased = stroke(500.5, 500.5, 2.).regions.remove(0);
        erased.operation = SelectionOperation::Add;
        erased.paint = Some([0., 0.2]);
        selection.regions.push(erased);
        assert_eq!(selection.coverage_bounds(), Some([10., 10., 103.5, 103.5]));
        let moved = selection.transformed([1., 0., 0., 1., 1., 0.]).unwrap();
        assert_eq!(moved.coverage(Point { x: 11.5, y: 10.5 }), 1.);
        assert!((moved.coverage(Point { x: 101.5, y: 100.5 }) - 0.5).abs() < 1. / 255.);
    }

    #[test]
    fn selection_transform_preserves_one_pixel_stroke_on_large_page_and_history() {
        let mut state = Document::default().document_state();
        state.width = 8192;
        state.height = 8192;
        let mut doc = Document::from_document_state(state).unwrap();
        doc.set_path_selection(
            vec![Point {
                x: 3000.5,
                y: 2000.5,
            }],
            0.5,
            SelectionMode::Replace,
        )
        .unwrap();
        let before = doc.selection().unwrap().clone();
        let request = PixelSelectionRequest {
            action: "transform".into(),
            matrix: Some([1., 0., 0., 1., 10., 20.]),
            ..Default::default()
        };
        doc.pixel_selection_action(request).unwrap();
        let after = doc.selection().unwrap().clone();
        let mask = after.raster.as_ref().unwrap();
        assert!(mask.size[0] * mask.size[1] <= 16);
        assert_eq!(&mask.transform[..4], &[1., 0., 0., 1.]);
        assert_eq!(
            after.coverage(Point {
                x: 3010.5,
                y: 2020.5
            }),
            1.
        );
        assert_eq!(
            after.coverage(Point {
                x: 3000.5,
                y: 2000.5
            }),
            0.
        );
        assert_eq!(
            after.coverage(Point {
                x: 3011.5,
                y: 2020.5
            }),
            0.
        );
        doc.undo();
        assert_eq!(doc.selection(), Some(&before));
        doc.redo();
        assert_eq!(doc.selection(), Some(&after));
        let loaded = Document::from_document_state(
            serde_json::from_slice(&serde_json::to_vec(&doc.document_state()).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(loaded.selection(), Some(&after));
        let snapshot = doc.encode().unwrap();
        assert!(doc
            .pixel_selection_action(PixelSelectionRequest {
                action: "transform".into(),
                matrix: Some([0.; 6]),
                ..Default::default()
            })
            .is_err());
        assert_eq!(doc.encode().unwrap(), snapshot);
    }
    #[test]
    fn selection_transform_preserves_off_page_gray_and_boolean_composition() {
        let mut selection = stroke(-4.5, 8.5, 2.5);
        selection.regions.push(SelectionRegion {
            paint: Some([0.3, 0.2]),
            shape: SelectionShape::Stroke,
            points: vec![Point { x: 5.5, y: 8.5 }],
            radius: 3.,
            bounds: [2.5, 5.5, 6., 6.],
            operation: SelectionOperation::Add,
        });
        let mut cut = Selection::new(SelectionShape::Rectangle, [-6., 7., 1., 3.])
            .regions
            .remove(0);
        cut.operation = SelectionOperation::Subtract;
        selection.regions.push(cut);
        let transformed = selection.transformed([1., 0., 0., 1., 20., 10.]).unwrap();
        for y in 0..18 {
            for x in -10..12 {
                let p = Point {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                let actual = transformed.coverage(Point {
                    x: p.x + 20.,
                    y: p.y + 10.,
                });
                assert!(
                    (actual - selection.coverage(p)).abs() <= 0.5 / 255. + 1e-6,
                    "at {x},{y}"
                );
            }
        }
        assert!(
            transformed.coverage(Point { x: 28.5, y: 18.5 }) > 0.,
            "gray fringe must survive cropped bounds"
        );
        let back = transformed
            .transformed([1., 0., 0., 1., -20., -10.])
            .unwrap();
        assert!(Arc::ptr_eq(
            &back.raster.as_ref().unwrap().runs,
            &transformed.raster.as_ref().unwrap().runs
        ));
        let unchanged = selection.transformed([1., 0., 0., 1., 0., 0.]).unwrap();
        assert_eq!(unchanged, selection);
    }
    #[test]
    fn selection_transform_inverse_is_bounded_and_intersection_stays_local() {
        let mut selection = stroke(50.5, 50.5, 10.);
        selection.invert(8192, 8192).unwrap();
        let raster = selection.transformed([1., 0., 0., 1., 1., 0.]).unwrap();
        let mask = raster.raster.as_ref().unwrap();
        assert!(u64::from(mask.size[0]) * u64::from(mask.size[1]) <= MAX_SELECTION_PIXELS);
        assert!(mask.size[0] >= 2047);
        assert_eq!(raster.coverage(Point { x: 1001., y: 1000. }), 1.);
        assert_eq!(raster.coverage(Point { x: 51.5, y: 50.5 }), 0.);
        assert_eq!(raster.coverage(Point { x: -10., y: 1000. }), 0.);
        let mut clip = Selection::new(SelectionShape::Rectangle, [100., 100., 10., 10.])
            .regions
            .remove(0);
        clip.operation = SelectionOperation::Intersect;
        selection.regions.push(clip);
        let cropped = selection.transformed([1., 0., 0., 1., 1., 0.]).unwrap();
        assert_eq!(cropped.raster.as_ref().unwrap().size, [12, 12]);
        assert_eq!(cropped.coverage(Point { x: 105.5, y: 105.5 }), 1.);
        assert_eq!(cropped.coverage(Point { x: 115.5, y: 105.5 }), 0.);
    }
}
