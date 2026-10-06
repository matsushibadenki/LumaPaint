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
        }
    }
}
impl Settings {
    pub fn validate(self) -> Result<(), String> {
        if !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
            || !(2..=512).contains(&self.pattern_size)
        {
            Err("Invalid paint bucket settings".into())
        } else {
            Ok(())
        }
    }
}
