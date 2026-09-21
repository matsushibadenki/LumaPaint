//! Portable path interchange. Engine-owned objects never enter saved documents.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

/// SVG path data (the `d` attribute), in document coordinates, without paint or transforms.
/// This is the engine and editable-document interchange boundary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorPath {
    pub data: String,
    pub fill_rule: FillRule,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorPaint {
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VectorObjectKind {
    #[default]
    Path,
    Rectangle,
    Ellipse,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VectorObject {
    pub id: String,
    pub name: String,
    pub path: VectorPath,
    /// SVG-compatible affine matrix: [a, b, c, d, e, f].
    pub transform: [f32; 6],
    pub fill: Option<VectorPaint>,
    pub stroke: Option<VectorPaint>,
    pub stroke_width: f32,
    pub visible: bool,
    #[serde(default)]
    pub kind: VectorObjectKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub control_points: Vec<[f32; 2]>,
}

impl VectorObject {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 64
            || self.name.trim().is_empty()
            || self.name.chars().count() > 120
            || self.path.data.is_empty()
            || self.path.data.len() > 1024 * 1024
            || self.transform.iter().any(|value| !value.is_finite())
            || !self.stroke_width.is_finite()
            || !(0.0..=4096.0).contains(&self.stroke_width)
            || (self.fill.is_none() && self.stroke.is_none())
            || self.control_points.len() > 65_536
            || self
                .control_points
                .iter()
                .flatten()
                .any(|value| !value.is_finite() || value.abs() >= 100_000.0)
        {
            return Err("Invalid vector object".into());
        }
        Ok(())
    }

    pub fn hit_test(&self, point: [f32; 2], tolerance: f32) -> bool {
        if !self.visible
            || !tolerance.is_finite()
            || tolerance < 0.0
            || self.control_points.is_empty()
        {
            return false;
        }
        let points = self
            .control_points
            .iter()
            .map(|[x, y]| {
                let [a, b, c, d, e, f] = self.transform;
                [a * x + c * y + e, b * x + d * y + f]
            })
            .collect::<Vec<_>>();
        let min_x = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let max_x = points
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let max_y = points
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max);
        match self.kind {
            VectorObjectKind::Rectangle => {
                point[0] >= min_x - tolerance
                    && point[0] <= max_x + tolerance
                    && point[1] >= min_y - tolerance
                    && point[1] <= max_y + tolerance
            }
            VectorObjectKind::Ellipse => {
                let rx = (max_x - min_x) * 0.5 + tolerance;
                let ry = (max_y - min_y) * 0.5 + tolerance;
                let cx = (min_x + max_x) * 0.5;
                let cy = (min_y + max_y) * 0.5;
                rx > 0.0
                    && ry > 0.0
                    && ((point[0] - cx) / rx).powi(2) + ((point[1] - cy) / ry).powi(2) <= 1.0
            }
            VectorObjectKind::Path => points.windows(2).any(|segment| {
                distance_to_segment(point, segment[0], segment[1])
                    <= tolerance.max(self.stroke_width * 0.5)
            }),
        }
    }
}

fn distance_to_segment(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length_squared = delta[0] * delta[0] + delta[1] * delta[1];
    if length_squared <= f32::EPSILON {
        return (point[0] - start[0]).hypot(point[1] - start[1]);
    }
    let t = (((point[0] - start[0]) * delta[0] + (point[1] - start[1]) * delta[1])
        / length_squared)
        .clamp(0.0, 1.0);
    (point[0] - start[0] - t * delta[0]).hypot(point[1] - start[1] - t * delta[1])
}

#[derive(Clone, Copy, Debug)]
pub enum PathOperation {
    Union,
    Difference,
    Intersection,
    Xor,
}

/// Boolean operations act on filled areas. Difference means left minus right.
/// Backends must reject malformed or non-finite geometry, never change either input.
pub trait VectorPathEngine {
    fn combine(
        &self,
        left: &VectorPath,
        right: &VectorPath,
        operation: PathOperation,
    ) -> Result<VectorPath, String>;
    fn contains(&self, path: &VectorPath, point: [f32; 2]) -> Result<bool, String>;
}
