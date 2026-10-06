//! Read-only, line-delimited diagnostic interface for UI/preview verification.
use std::io::{BufRead, Write};
fn main() {
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let result = (|| -> Result<serde_json::Value, String> {
            let request: serde_json::Value =
                serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if request["command"] == "font_catalog" {
                return serde_json::to_value(lumapaint_renderer::vector::font_viewer::catalog())
                    .map_err(|e| e.to_string());
            }
            let args = &request["args"];
            let bytes = lumapaint_renderer::vector::font_viewer::preview(
                args["postscript"].as_str().ok_or("Missing face")?,
                args["sample"].as_str().ok_or("Missing sample")?,
                args["size"].as_f64().ok_or("Missing size")? as f32,
                args["width"].as_u64().ok_or("Missing width")? as u32,
                args["height"].as_u64().ok_or("Missing height")? as u32,
            )?;
            Ok(serde_json::json!({"svg":String::from_utf8(bytes).map_err(|e|e.to_string())?}))
        })();
        println!(
            "{}",
            match result {
                Ok(value) => serde_json::json!({"result":value}),
                Err(error) => serde_json::json!({"error":error}),
            }
        );
        std::io::stdout().flush().unwrap();
    }
}
