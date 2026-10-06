//! Ordered region operations keep selection editing independent of raster resolution.
use crate::document::{Point, MAX_DOCUMENT_DIMENSION};
use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_SELECTION_REGIONS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionShape {
    Rectangle,
    Ellipse,
    Polygon,
    Stroke,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionOperation {
    Replace,
    Add,
    Subtract,
    Invert,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionRegion {
    pub shape: SelectionShape,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub radius: f32,
    /// Document pixels: left, top, width, height. Movement may place regions off-page.
    pub bounds: [f32; 4],
    pub operation: SelectionOperation,
}

fn is_zero(value: &f32) -> bool {
    *value == 0.
}
impl SelectionRegion {
    fn contains(&self, point: Point) -> bool {
        let [x, y, w, h] = self.bounds;
        match self.shape {
            SelectionShape::Polygon => {
                let mut inside = false;
                for (a, b) in self
                    .points
                    .iter()
                    .zip(self.points.iter().cycle().skip(1))
                    .take(self.points.len())
                {
                    if segment_distance(point, *a, *b) < 0.0001 {
                        return true;
                    }
                    if (a.y > point.y) != (b.y > point.y)
                        && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x
                    {
                        inside = !inside;
                    }
                }
                inside
            }
            SelectionShape::Stroke => {
                self.points
                    .windows(2)
                    .any(|pair| segment_distance(point, pair[0], pair[1]) <= self.radius)
                    || self
                        .points
                        .first()
                        .is_some_and(|p| (point.x - p.x).hypot(point.y - p.y) <= self.radius)
            }
            SelectionShape::Rectangle => {
                point.x >= x && point.y >= y && point.x < x + w && point.y < y + h
            }
            SelectionShape::Ellipse => {
                ((point.x - x - w * 0.5) / (w * 0.5)).powi(2)
                    + ((point.y - y - h * 0.5) / (h * 0.5)).powi(2)
                    <= 1.0
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Selection {
    pub regions: Vec<SelectionRegion>,
}

// Read the single-shape selection clips already saved by earlier v1 builds.
impl<'de> Deserialize<'de> for Selection {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Composite {
            regions: Vec<SelectionRegion>,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Legacy {
            shape: SelectionShape,
            bounds: [f32; 4],
            inverted: bool,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Composite(Composite),
            Legacy(Legacy),
        }
        let selection = match Saved::deserialize(deserializer)? {
            Saved::Composite(value) => Self {
                regions: value.regions,
            },
            Saved::Legacy(value) => {
                let mut selection = Self::new(value.shape, value.bounds);
                if value.inverted {
                    selection
                        .invert(MAX_DOCUMENT_DIMENSION, MAX_DOCUMENT_DIMENSION)
                        .map_err(serde::de::Error::custom)?;
                }
                selection
            }
        };
        selection.validate().map_err(serde::de::Error::custom)?;
        Ok(selection)
    }
}

impl Selection {
    pub fn new(shape: SelectionShape, bounds: [f32; 4]) -> Self {
        Self {
            regions: vec![SelectionRegion {
                points: Vec::new(),
                radius: 0.,
                shape,
                bounds,
                operation: SelectionOperation::Replace,
            }],
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.regions.is_empty() || self.regions.len() > MAX_SELECTION_REGIONS {
            return Err("Invalid selection region count".into());
        }
        if self.regions.iter().map(|r| r.points.len()).sum::<usize>() > 32768 {
            return Err("Selection path exceeds point limit".into());
        }
        for (index, region) in self.regions.iter().enumerate() {
            if region.points.iter().any(|p| {
                !p.x.is_finite() || !p.y.is_finite() || p.x.abs() > 16384. || p.y.abs() > 16384.
            }) || !region.radius.is_finite()
                || region.radius < 0.
                || region.radius > 512.
                || match region.shape {
                    SelectionShape::Polygon => region.points.len() < 3,
                    SelectionShape::Stroke => region.points.is_empty() || region.radius <= 0.,
                    _ => !region.points.is_empty() || region.radius != 0.,
                }
            {
                return Err("Invalid selection path".into());
            }
            let [x, y, w, h] = region.bounds;
            let limit = MAX_DOCUMENT_DIMENSION as f32;
            if !region.bounds.iter().all(|v| v.is_finite())
                || x.abs() > limit * 2.0
                || y.abs() > limit * 2.0
                || w < 1.0
                || h < 1.0
                || w > limit
                || h > limit
                || (index == 0) != (region.operation == SelectionOperation::Replace)
                || (region.operation == SelectionOperation::Invert
                    && region.shape != SelectionShape::Rectangle)
            {
                return Err("Invalid selection region".into());
            }
        }
        Ok(())
    }
    pub fn contains(&self, point: Point) -> bool {
        let mut inside = false;
        for region in &self.regions {
            let hit = region.contains(point);
            inside = match region.operation {
                SelectionOperation::Replace => hit,
                SelectionOperation::Add => inside || hit,
                SelectionOperation::Subtract => inside && !hit,
                SelectionOperation::Invert => hit && !inside,
            };
        }
        inside
    }
    pub fn ensure_capacity(&self) -> Result<(), String> {
        if self.regions.len() >= MAX_SELECTION_REGIONS {
            Err(
                "Selection operation limit reached (256). Deselect to start a new selection."
                    .into(),
            )
        } else {
            Ok(())
        }
    }
    pub fn invert(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.ensure_capacity()?;
        self.regions.push(SelectionRegion {
            points: Vec::new(),
            radius: 0.,
            shape: SelectionShape::Rectangle,
            bounds: [0.0, 0.0, width as f32, height as f32],
            operation: SelectionOperation::Invert,
        });
        Ok(())
    }
    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        let limit = MAX_DOCUMENT_DIMENSION as f32 * 2.0;
        let mut dx = dx;
        let mut dy = dy;
        for region in &self.regions {
            dx = dx.clamp(-limit - region.bounds[0], limit - region.bounds[0]);
            dy = dy.clamp(-limit - region.bounds[1], limit - region.bounds[1]);
        }
        let mut moved = self.clone();
        for region in &mut moved.regions {
            for p in &mut region.points {
                p.x += dx;
                p.y += dy;
            }
            region.bounds[0] += dx;
            region.bounds[1] += dy;
        }
        moved
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionMode {
    Replace,
    Add,
    Subtract,
}

#[derive(Clone)]
pub(crate) struct SelectionGesture {
    pub vector_layer: Option<String>,
    pub start: Point,
    pub shape: SelectionShape,
    pub mode: SelectionMode,
    pub moving: bool,
    pub base: Option<Selection>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Brush, Document};

    fn point(x: f32, y: f32) -> Point {
        Point { x, y }
    }
    fn drag(
        doc: &mut Document,
        shape: SelectionShape,
        mode: SelectionMode,
        from: (f32, f32),
        to: (f32, f32),
    ) {
        doc.begin_selection_edit(point(from.0, from.1), shape, mode)
            .unwrap();
        doc.extend_selection(point(to.0, to.1), true);
    }
    fn hit(doc: &Document, x: f32, y: f32) -> bool {
        doc.selection().unwrap().contains(point(x, y))
    }

    #[test]
    fn additive_and_subtractive_drags_take_priority_over_moving() {
        let mut doc = Document::default();
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Replace,
            (100.0, 100.0),
            (300.0, 300.0),
        );
        drag(
            &mut doc,
            SelectionShape::Ellipse,
            SelectionMode::Add,
            (220.0, 100.0),
            (420.0, 300.0),
        );
        assert!(hit(&doc, 120.0, 200.0) && hit(&doc, 380.0, 200.0));
        assert!(!hit(&doc, 415.0, 105.0));
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Subtract,
            (200.0, 150.0),
            (250.0, 250.0),
        );
        assert!(!hit(&doc, 225.0, 200.0));
        assert!(hit(&doc, 260.0, 200.0));
        assert!(!doc.snapshot().dirty);
        assert_eq!(doc.snapshot().stroke_count, 0);
    }

    #[test]
    fn move_translates_every_island_and_hole_without_changing_existing_paint() {
        let mut doc = Document::default();
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Replace,
            (100.0, 100.0),
            (300.0, 300.0),
        );
        drag(
            &mut doc,
            SelectionShape::Ellipse,
            SelectionMode::Add,
            (400.0, 100.0),
            (500.0, 200.0),
        );
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Subtract,
            (150.0, 150.0),
            (200.0, 200.0),
        );
        let original = doc.selection().cloned();
        doc.begin(point(110.0, 110.0), Brush::default()).unwrap();
        doc.finish();
        let revision = doc.snapshot().revision;
        doc.begin_selection_edit(
            point(110.0, 110.0),
            SelectionShape::Rectangle,
            SelectionMode::Replace,
        )
        .unwrap();
        doc.extend_selection(point(130.0, 130.0), false);
        doc.extend_selection(point(210.0, 210.0), true);
        assert!(!hit(&doc, 110.0, 110.0));
        assert!(hit(&doc, 210.0, 210.0) && hit(&doc, 550.0, 250.0));
        assert!(!hit(&doc, 275.0, 275.0));
        assert_eq!(doc.visible_strokes().next().unwrap().selection, original);
        assert_eq!(doc.snapshot().revision, revision);
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(loaded.visible_strokes().next().unwrap().selection, original);
    }

    #[test]
    fn cancel_restores_selection_and_zero_size_modifier_drag_is_a_noop() {
        let mut doc = Document::default();
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Replace,
            (100.0, 100.0),
            (300.0, 300.0),
        );
        let original = doc.selection().cloned();
        for mode in [
            SelectionMode::Replace,
            SelectionMode::Add,
            SelectionMode::Subtract,
        ] {
            doc.begin_selection_edit(point(150.0, 150.0), SelectionShape::Ellipse, mode)
                .unwrap();
            doc.extend_selection(point(220.0, 230.0), false);
            assert!(doc.cancel_selection_gesture());
            assert_eq!(doc.selection(), original.as_ref());
            doc.extend_selection(point(250.0, 260.0), true);
            assert_eq!(doc.selection(), original.as_ref());
        }
        drag(
            &mut doc,
            SelectionShape::Ellipse,
            SelectionMode::Add,
            (100.0, 100.0),
            (100.0, 100.0),
        );
        assert_eq!(doc.selection(), original.as_ref());
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Replace,
            (500.0, 400.0),
            (600.0, 500.0),
        );
        assert!(!hit(&doc, 150.0, 150.0));
        assert!(hit(&doc, 550.0, 450.0));
    }

    #[test]
    fn inversion_and_empty_selection_remain_correct_after_subtraction_and_move() {
        let mut doc = Document::default();
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Subtract,
            (100.0, 100.0),
            (300.0, 300.0),
        );
        assert!(hit(&doc, 50.0, 50.0) && !hit(&doc, 150.0, 150.0));
        doc.invert_selection().unwrap();
        assert!(!hit(&doc, 50.0, 50.0) && hit(&doc, 150.0, 150.0));
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Replace,
            (150.0, 150.0),
            (200.0, 200.0),
        );
        assert!(!hit(&doc, 120.0, 120.0) && hit(&doc, 320.0, 320.0));
        doc.select_all();
        drag(
            &mut doc,
            SelectionShape::Rectangle,
            SelectionMode::Subtract,
            (0.0, 0.0),
            (960.0, 640.0),
        );
        assert!(doc.selection().is_some());
        assert!(!hit(&doc, 50.0, 50.0) && !hit(&doc, 950.0, 630.0));
    }

    #[test]
    fn legacy_clips_load_and_excessive_or_invalid_regions_are_rejected() {
        let old: Selection = serde_json::from_str(
            r#"{"shape":"ellipse","bounds":[100,100,200,200],"inverted":true}"#,
        )
        .unwrap();
        assert!(old.contains(point(10.0, 10.0)) && !old.contains(point(200.0, 200.0)));
        let mut oversized = Selection::new(SelectionShape::Rectangle, [0.0, 0.0, 10.0, 10.0]);
        for _ in 1..MAX_SELECTION_REGIONS {
            oversized.regions.push(SelectionRegion {
                points: Vec::new(),
                radius: 0.,
                shape: SelectionShape::Ellipse,
                bounds: [1.0, 1.0, 10.0, 10.0],
                operation: SelectionOperation::Add,
            });
        }
        assert!(oversized.validate().is_ok());
        assert!(oversized.invert(960, 640).is_err());
        oversized.regions.push(oversized.regions[1].clone());
        assert!(
            serde_json::from_slice::<Selection>(&serde_json::to_vec(&oversized).unwrap()).is_err()
        );
        assert!(serde_json::from_str::<Selection>(r#"{"regions":[]}"#).is_err());
    }
}

pub fn segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy).max(1e-12)).clamp(0., 1.);
    (p.x - a.x - t * dx).hypot(p.y - a.y - t * dy)
}
impl SelectionRegion {
    pub fn path_data(&self) -> String {
        if self.shape == SelectionShape::Polygon {
            let mut path = format!("M{} {}", self.points[0].x, self.points[0].y);
            for p in &self.points[1..] {
                path.push_str(&format!("L{} {}", p.x, p.y));
            }
            path.push('Z');
            return path;
        }
        let r = self.radius;
        let mut path = String::new();
        for p in &self.points {
            path.push_str(&format!(
                "M{} {}a{r} {r} 0 1 1 {} 0a{r} {r} 0 1 1 {} 0Z",
                p.x + r,
                p.y,
                -2. * r,
                2. * r
            ));
        }
        for pair in self.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = (b.x - a.x).hypot(b.y - a.y).max(1e-12);
            let (nx, ny) = ((b.y - a.y) * r / length, -(b.x - a.x) * r / length);
            path.push_str(&format!(
                "M{} {}L{} {}L{} {}L{} {}Z",
                a.x + nx,
                a.y + ny,
                b.x + nx,
                b.y + ny,
                b.x - nx,
                b.y - ny,
                a.x - nx,
                a.y - ny
            ));
        }
        path
    }
}
#[cfg(test)]
mod path_tests {
    use super::*;
    use crate::document::Document;
    fn polygon() -> Vec<Point> {
        vec![
            Point { x: 10., y: 10. },
            Point { x: 50., y: 10. },
            Point { x: 50., y: 20. },
            Point { x: 20., y: 20. },
            Point { x: 20., y: 50. },
            Point { x: 10., y: 50. },
        ]
    }
    #[test]
    fn concave_polygon_and_translation_preserve_shape() {
        let mut doc = Document::default();
        doc.set_path_selection(polygon(), 0., SelectionMode::Replace)
            .unwrap();
        let s = doc.selection().unwrap();
        assert!(s.contains(Point { x: 15., y: 40. }));
        assert!(!s.contains(Point { x: 35., y: 35. }));
        assert!(s.contains(Point { x: 10., y: 30. }));
        let moved = s.translated(100., 50.);
        assert!(moved.contains(Point { x: 115., y: 90. }));
        assert!(!moved.contains(Point { x: 15., y: 40. }));
        let encoded = serde_json::to_string(&moved).unwrap();
        let loaded: Selection = serde_json::from_str(&encoded).unwrap();
        assert_eq!(loaded, moved);
    }
    #[test]
    fn brush_capsules_are_continuous_and_add_subtract_combine() {
        let mut doc = Document::default();
        doc.set_path_selection(
            vec![Point { x: 10., y: 10. }, Point { x: 50., y: 10. }],
            4.,
            SelectionMode::Replace,
        )
        .unwrap();
        assert!(doc.selection().unwrap().contains(Point { x: 30., y: 12. }));
        assert!(!doc.selection().unwrap().contains(Point { x: 30., y: 20. }));
        doc.set_path_selection(vec![Point { x: 30., y: 10. }], 2., SelectionMode::Subtract)
            .unwrap();
        assert!(!doc.selection().unwrap().contains(Point { x: 30., y: 10. }));
        assert!(doc.selection().unwrap().contains(Point { x: 15., y: 10. }));
        doc.set_path_selection(polygon(), 0., SelectionMode::Add)
            .unwrap();
        assert!(doc.selection().unwrap().contains(Point { x: 30., y: 10. }));
    }
    #[test]
    fn path_selection_history_is_atomic_and_does_not_copy_layers() {
        let mut doc = Document::default();
        let before = doc.document_state();
        let mut preview = doc.clone();
        preview
            .set_path_selection(polygon(), 0., SelectionMode::Replace)
            .unwrap();
        assert!(doc
            .commit_path_selection(preview.selection().cloned())
            .unwrap());
        let after = doc.document_state();
        doc.undo();
        assert_eq!(
            serde_json::to_value(doc.document_state()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
        doc.redo();
        assert_eq!(
            serde_json::to_value(doc.document_state()).unwrap(),
            serde_json::to_value(after).unwrap()
        );
        assert!(!doc.commit_path_selection(doc.selection().cloned()).unwrap());
        let mut invalid = doc.selection().unwrap().clone();
        invalid.regions[0].points[0].x = f32::NAN;
        let before = doc.selection().cloned();
        assert!(doc.commit_path_selection(Some(invalid)).is_err());
        assert_eq!(doc.selection().cloned(), before);
    }
    #[test]
    fn rectangle_legacy_serialization_remains_compatible() {
        let legacy =
            r#"{"regions":[{"shape":"rectangle","bounds":[0,0,20,20],"operation":"replace"}]}"#;
        let s: Selection = serde_json::from_str(legacy).unwrap();
        let encoded = serde_json::to_string(&s).unwrap();
        assert!(!encoded.contains("points"));
        assert!(!encoded.contains("radius"));
        assert_eq!(serde_json::from_str::<Selection>(&encoded).unwrap(), s);
    }
}
