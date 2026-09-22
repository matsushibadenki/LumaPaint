//! Platform-independent core. No UI, WebView, or OS dependencies belong here.

use serde::Serialize;
pub mod document;
pub mod graph;
pub mod selection;
pub mod tile_container;
pub mod tiles;
pub mod vector;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
    pub version: &'static str,
    pub platform: &'static str,
    pub architecture: &'static str,
}

pub fn runtime_info() -> RuntimeInfo {
    RuntimeInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
    }
}
