//! Application-owned display preference. Document geometry always remains in pixels.
use std::sync::Mutex;
use tauri::{Emitter, Manager};

pub struct MeasurementUnits(pub Mutex<String>);
impl MeasurementUnits {
    pub fn load(app: &tauri::AppHandle) -> Self {
        let value = app
            .path()
            .app_config_dir()
            .ok()
            .and_then(|p| std::fs::read_to_string(p.join("measurement-unit.txt")).ok())
            .filter(|v| valid(v))
            .unwrap_or_else(|| "pixels".into());
        Self(Mutex::new(value))
    }
}
fn valid(unit: &str) -> bool {
    matches!(
        unit,
        "pixels" | "millimeters" | "centimeters" | "inches" | "points"
    )
}
#[tauri::command]
pub fn measurement_unit(state: tauri::State<'_, MeasurementUnits>) -> Result<String, String> {
    state.0.lock().map(|v| v.clone()).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn set_measurement_unit(
    app: tauri::AppHandle,
    state: tauri::State<'_, MeasurementUnits>,
    unit: String,
) -> Result<(), String> {
    if !valid(&unit) {
        return Err("Invalid measurement unit".into());
    }
    let mut current = state.0.lock().map_err(|e| e.to_string())?;
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("measurement-unit.txt"), &unit).map_err(|e| e.to_string())?;
    *current = unit.clone();
    app.emit("measurement-unit-changed", unit)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_supported_display_units_are_accepted() {
        for unit in ["pixels", "millimeters", "centimeters", "inches", "points"] {
            assert!(valid(unit));
        }
        for unit in ["", "mm", "percent", "../pixels"] {
            assert!(!valid(unit));
        }
    }
}
