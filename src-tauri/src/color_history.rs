//! Session-wide color history. Windows only render projections of this state.
use lumapaint_core::document::{ColorMode, ColorProfile};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::Emitter;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsedColor {
    pub color: [u8; 3],
    pub alpha: f32,
    pub color_mode: ColorMode,
    pub color_profile: ColorProfile,
}
#[derive(Default)]
struct History {
    colors: Vec<UsedColor>,
    capacity: Option<usize>,
}
impl History {
    fn trim(&mut self) {
        self.colors.truncate(self.capacity.unwrap_or(512));
    }
    fn record(&mut self, color: UsedColor) -> Result<(), String> {
        if !color.alpha.is_finite()
            || !(0.0..=1.0).contains(&color.alpha)
            || color.color_profile.mode() != color.color_mode
        {
            return Err(
                "Invalid color history value / 色履歴の値が不正です / 颜色历史参数无效".into(),
            );
        }
        self.colors.retain(|item| item != &color);
        self.colors.insert(0, color);
        self.trim();
        Ok(())
    }
}
static HISTORY: Mutex<History> = Mutex::new(History {
    colors: Vec::new(),
    capacity: None,
});

pub fn record(app: &tauri::AppHandle, color: UsedColor) -> Result<(), String> {
    let mut history = HISTORY.lock().map_err(|e| e.to_string())?;
    history.record(color)?;
    drop(history);
    app.emit("color-history-changed", ())
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn color_history(
    app: tauri::AppHandle,
    color: Option<UsedColor>,
    capacity: Option<usize>,
) -> Result<Vec<UsedColor>, String> {
    if capacity.is_some_and(|value| value > 512) {
        return Err("Invalid history capacity / 色履歴の件数が不正です / 颜色历史容量无效".into());
    }
    if let Some(color) = color {
        record(&app, color)?;
    }
    let mut history = HISTORY.lock().map_err(|e| e.to_string())?;
    let before = history.colors.len();
    if let Some(capacity) = capacity {
        history.capacity = Some(capacity);
        history.trim();
    }
    let changed = history.colors.len() != before;
    let colors = history.colors.clone();
    drop(history);
    if changed {
        let _ = app.emit("color-history-changed", ());
    }
    Ok(colors)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn color(n: u8) -> UsedColor {
        UsedColor {
            color: [n, 0, 0],
            alpha: 0.5,
            color_mode: ColorMode::Rgb,
            color_profile: ColorProfile::DisplayP3,
        }
    }
    #[test]
    fn newest_first_deduplicate_and_evict_oldest() {
        let mut h = History {
            capacity: Some(3),
            ..History::default()
        };
        for n in 0..5 {
            h.record(color(n)).unwrap();
        }
        assert_eq!(h.colors, vec![color(4), color(3), color(2)]);
        h.record(color(3)).unwrap();
        assert_eq!(h.colors, vec![color(3), color(4), color(2)]);
        h.capacity = Some(1);
        h.trim();
        assert_eq!(h.colors, vec![color(3)]);
        h.capacity = Some(4);
        h.trim();
        assert_eq!(h.colors, vec![color(3)]);
    }
    #[test]
    fn alpha_and_profile_are_preserved_and_invalid_input_is_atomic() {
        let mut h = History::default();
        h.record(color(5)).unwrap();
        let mut other = color(5);
        other.alpha = 1.;
        h.record(other.clone()).unwrap();
        other.color_profile = ColorProfile::Srgb;
        h.record(other).unwrap();
        assert_eq!(h.colors.len(), 3);
        let before = h.colors.clone();
        let mut invalid = color(1);
        invalid.alpha = f32::NAN;
        assert!(h.record(invalid).is_err());
        assert_eq!(h.colors, before);
        let mut invalid = color(1);
        invalid.color_mode = ColorMode::Cmyk;
        assert!(h.record(invalid).is_err());
        assert_eq!(h.colors, before);
        let json = serde_json::to_string(&h.colors).unwrap();
        assert_eq!(
            serde_json::from_str::<Vec<UsedColor>>(&json).unwrap(),
            h.colors
        );
        h.capacity = Some(0);
        h.trim();
        h.record(color(7)).unwrap();
        assert!(h.colors.is_empty());
    }
}
