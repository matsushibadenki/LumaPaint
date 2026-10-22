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
    #[serde(default = "default_tolerance")]
    pub tolerance: f32,
    #[serde(default = "default_contiguous")]
    pub contiguous: bool,
}
fn default_tolerance() -> f32 {
    24.
}
fn default_contiguous() -> bool {
    true
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
            tolerance: default_tolerance(),
            contiguous: true,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if !self.tolerance.is_finite()
            || !(0. ..=255.).contains(&self.tolerance)
            || !self.diameter.is_finite()
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_settings_receive_automatic_selection_defaults_and_validate_tolerance() {
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "mode":"replace","diameter":32,"magneticWidth":10,
            "magneticContrast":10,"magneticFrequency":8,"allLayers":true
        }))
        .unwrap();
        assert_eq!(settings.tolerance, 24.);
        assert!(settings.contiguous);
        for tolerance in [f32::NAN, -1., 256.] {
            assert!(Settings {
                tolerance,
                ..settings
            }
            .validate()
            .is_err());
        }
    }
}
