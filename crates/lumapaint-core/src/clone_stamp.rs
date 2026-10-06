//! Clone-stamp tool settings are independent of native UI and rendering.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sample {
    #[default]
    CurrentLayer,
    CurrentBelow,
    AllLayers,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub opacity: f32,
    pub flow: f32,
    pub aligned: bool,
    pub sample: Sample,
    pub mode: Mode,
    pub angle: f32,
    pub roundness: f32,
    pub pressure_size: bool,
    pub pressure_opacity: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            opacity: 1.,
            flow: 1.,
            aligned: true,
            sample: Sample::CurrentLayer,
            mode: Mode::Normal,
            angle: 0.,
            roundness: 1.,
            pressure_size: false,
            pressure_opacity: false,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
            || !self.flow.is_finite()
            || !(0. ..=1.).contains(&self.flow)
            || !self.angle.is_finite()
            || !(-360. ..=360.).contains(&self.angle)
            || !self.roundness.is_finite()
            || !(0.01..=1.).contains(&self.roundness)
        {
            Err("Invalid clone stamp settings".into())
        } else {
            Ok(())
        }
    }
}
