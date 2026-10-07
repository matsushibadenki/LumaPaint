use crate::clone_stamp::Mode;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    #[default]
    Foreground,
    Pattern,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pattern {
    #[default]
    Checker,
    Stripes,
    Dots,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub source: Source,
    pub pattern: Pattern,
    pub pattern_size: u32,
    pub mode: Mode,
    pub opacity: f32,
    pub tolerance: u8,
    pub anti_alias: bool,
    pub contiguous: bool,
    pub all_layers: bool,
    /// Radius in document pixels used to seal narrow passages before flooding.
    #[serde(default)]
    pub close_gap: u8,
    /// Signed region offset in document pixels. Positive values grow the fill.
    #[serde(default)]
    pub area_offset: i8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            source: Source::Foreground,
            pattern: Pattern::Checker,
            pattern_size: 8,
            mode: Mode::Normal,
            opacity: 1.,
            tolerance: 32,
            anti_alias: true,
            contiguous: true,
            all_layers: false,
            close_gap: 0,
            area_offset: 0,
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
            || !(2..=512).contains(&self.pattern_size)
            || self.close_gap > 16
            || !(-32..=32).contains(&self.area_offset)
        {
            Err("Invalid paint bucket settings".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_settings_default_to_unchanged_fill_and_reject_invalid_offsets() {
        let mut value = serde_json::to_value(Settings::default()).unwrap();
        value.as_object_mut().unwrap().remove("closeGap");
        value.as_object_mut().unwrap().remove("areaOffset");
        assert_eq!(
            serde_json::from_value::<Settings>(value).unwrap(),
            Settings::default()
        );
        for settings in [
            Settings {
                close_gap: 17,
                ..Default::default()
            },
            Settings {
                area_offset: 33,
                ..Default::default()
            },
            Settings {
                area_offset: -33,
                ..Default::default()
            },
        ] {
            assert!(settings.validate().is_err());
        }
        let settings = Settings {
            close_gap: 16,
            area_offset: -32,
            ..Default::default()
        };
        settings.validate().unwrap();
        assert_eq!(
            serde_json::from_str::<Settings>(&serde_json::to_string(&settings).unwrap()).unwrap(),
            settings
        );
    }
}
