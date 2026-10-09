//! Platform-independent core. No UI, WebView, or OS dependencies belong here.

use serde::Serialize;
pub mod bezier;
pub mod clone_stamp;
pub mod document;
pub mod gradient;
pub mod graph;
#[cfg(feature = "heap-profile")]
pub mod heap_profile;
pub mod layer_effects;
pub mod layer_mask;
mod layer_mask_paint;
pub mod performance;
pub mod scene;
pub mod screentone;
pub mod selection;
pub mod stroke;
pub mod svg_backend;
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

pub mod image_frame;

pub mod paint_bucket;

pub mod selection_tools;

pub mod retouch;
