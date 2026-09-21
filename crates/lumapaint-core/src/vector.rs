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
        {
            return Err("Invalid vector object".into());
        }
        Ok(())
    }
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
