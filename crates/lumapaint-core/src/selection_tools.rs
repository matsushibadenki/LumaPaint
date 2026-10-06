use crate::selection::SelectionMode;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub mode: SelectionMode,
    pub diameter: f32,
    pub magnetic_width: f32,
    pub magnetic_contrast: f32,
    pub magnetic_frequency: f32,
    pub all_layers: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: SelectionMode::Replace,
            diameter: 32.,
            magnetic_width: 10.,
            magnetic_contrast: 10.,
            magnetic_frequency: 8.,
            all_layers: true,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if !self.diameter.is_finite()
            || !(1. ..=512.).contains(&self.diameter)
            || !self.magnetic_width.is_finite()
            || !(1. ..=40.).contains(&self.magnetic_width)
            || !self.magnetic_contrast.is_finite()
            || !(0. ..=100.).contains(&self.magnetic_contrast)
            || !self.magnetic_frequency.is_finite()
            || !(1. ..=100.).contains(&self.magnetic_frequency)
        {
            Err("Invalid selection tool settings".into())
        } else {
            Ok(())
        }
    }
}
