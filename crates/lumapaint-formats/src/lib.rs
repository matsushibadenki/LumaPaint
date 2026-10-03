//! External-format adapters. The dependency points toward the native core, never toward UI.
pub mod export;
pub mod io;
pub mod native;
pub mod pdf;
pub mod psd;
pub mod svg;
pub mod tile_container;
use lumapaint_core::tiles::TiledRasterState;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum CompatibilityTier {
    /// Native editing and appearance survived a verified round trip.
    A,
    /// Editable conversion with documented differences.
    B,
    /// Appearance fallback; source editability is lost.
    C,
    /// Unsupported or discarded information.
    D,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionIssue {
    /// Stable machine-readable code; UI owns localized presentation.
    pub code: &'static str,
    pub tier: CompatibilityTier,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionReport {
    pub issues: Vec<ConversionIssue>,
}

/// No external container becomes the document's source of truth.
/// Metadata is returned separately until the native tiled model can preserve it.
pub struct ImportedDocument {
    pub raster: TiledRasterState,
    pub report: ConversionReport,
}
pub trait DocumentImporter {
    /// Signature detection only. A successful probe is not validation or support.
    fn probe(&self, bytes: &[u8]) -> bool;
    fn import(&self, bytes: &[u8], options: ImportOptions)
        -> Result<ImportedDocument, ImportError>;
}
#[derive(Clone, Copy, Debug, Default)]
pub struct ImportOptions {
    /// Explicit opt-in to losing layers, resources, or native editing information.
    pub allow_lossy: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    Malformed(&'static str),
    Unsupported(&'static str),
    LimitExceeded(&'static str),
    LossyConversionRequiresConsent(ConversionReport),
}
impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ImportError {}
