//! Settings for local pixel retouch brushes.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Blur,
    Sharpen,
    Smudge,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub strength: f32,
    pub sample_all_layers: bool,
    pub protect_detail: bool,
    pub finger_painting: bool,
    pub pressure_size: bool,
    pub pressure_strength: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            strength: 0.5,
            sample_all_layers: false,
            protect_detail: true,
            finger_painting: false,
            pressure_size: false,
            pressure_strength: true,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if !self.strength.is_finite() || !(0.0..=1.0).contains(&self.strength) {
            Err("Invalid retouch strength".into())
        } else {
            Ok(())
        }
    }
}
