//! Export contracts accept immutable native model data and never a renderer or UI state.
use crate::{CompatibilityTier, ConversionIssue, ConversionReport};
use lumapaint_core::document::{Document, DocumentState};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatId {
    Native,
    Svg,
    Pdf,
    Illustrator,
    Psd,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatCapabilities {
    pub format: FormatId,
    pub import: bool,
    pub export: bool,
    /// True only for a limited, explicitly documented subset or fallback.
    pub partial: bool,
}

pub const CAPABILITIES: &[FormatCapabilities] = &[
    FormatCapabilities {
        format: FormatId::Native,
        import: true,
        export: true,
        partial: false,
    },
    FormatCapabilities {
        format: FormatId::Svg,
        import: true,
        export: true,
        partial: true,
    },
    FormatCapabilities {
        format: FormatId::Pdf,
        import: false,
        export: false,
        partial: false,
    },
    FormatCapabilities {
        format: FormatId::Illustrator,
        import: false,
        export: false,
        partial: false,
    },
    FormatCapabilities {
        format: FormatId::Psd,
        import: true,
        export: false,
        partial: true,
    },
];

/// Captures current committed plus active content without changing revision, history or saved state.
/// The private state prevents callers bypassing model validation.
#[derive(Clone, Debug)]
pub struct ExportSnapshot {
    state: DocumentState,
}
impl ExportSnapshot {
    pub fn capture(document: &Document) -> Self {
        let mut copy = document.clone();
        copy.finish();
        Self {
            state: copy.document_state(),
        }
    }
    pub fn state(&self) -> &DocumentState {
        &self.state
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExportOptions {
    pub allow_lossy: bool,
}
#[derive(Debug)]
pub enum ExportError {
    Unsupported(FormatId),
    UnsupportedFeature(&'static str),
    InvalidDocument(String),
    LossyConversionRequiresConsent(ConversionReport),
}
impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ExportError {}

pub struct ExportedDocument {
    pub format: FormatId,
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
    pub report: ConversionReport,
}

pub trait DocumentExporter {
    fn format(&self) -> FormatId;
    fn export(
        &self,
        snapshot: &ExportSnapshot,
        options: ExportOptions,
    ) -> Result<ExportedDocument, ExportError>;
}

pub struct NativeExporter;
impl DocumentExporter for NativeExporter {
    fn format(&self) -> FormatId {
        FormatId::Native
    }
    fn export(
        &self,
        snapshot: &ExportSnapshot,
        _: ExportOptions,
    ) -> Result<ExportedDocument, ExportError> {
        Ok(ExportedDocument {
            format: self.format(),
            media_type: "application/vnd.lumapaint+json",
            bytes: crate::native::encode_state(snapshot.state())
                .map_err(ExportError::InvalidDocument)?,
            report: ConversionReport::default(),
        })
    }
}

/// One dispatch boundary for each independent adapter; unsupported formats cannot emit fake files.
pub fn export(
    format: FormatId,
    snapshot: &ExportSnapshot,
    options: ExportOptions,
) -> Result<ExportedDocument, ExportError> {
    match format {
        FormatId::Native => NativeExporter.export(snapshot, options),
        FormatId::Svg => crate::svg::SvgExporter.export(snapshot, options),
        other => Err(ExportError::Unsupported(other)),
    }
}

pub(crate) fn appearance_report() -> ConversionReport {
    ConversionReport {
        issues: vec![ConversionIssue {
            code: "svg.embedded_layers_not_editable",
            tier: CompatibilityTier::C,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_does_not_save_or_mutate_document_and_unavailable_formats_are_explicit() {
        let document = Document::default();
        let before = serde_json::to_value(document.document_state()).unwrap();
        let snapshot = ExportSnapshot::capture(&document);
        let saved = export(FormatId::Native, &snapshot, ExportOptions::default()).unwrap();
        assert_eq!(saved.format, FormatId::Native);
        assert_eq!(
            crate::native::decode(&saved.bytes).unwrap().dimensions(),
            document.dimensions()
        );
        assert_eq!(
            serde_json::to_value(document.document_state()).unwrap(),
            before
        );
        assert_eq!(document.snapshot().revision, 0);
        assert!(!document.snapshot().dirty);
        for format in [FormatId::Pdf, FormatId::Illustrator, FormatId::Psd] {
            assert!(
                matches!(export(format, &snapshot, ExportOptions { allow_lossy: true }), Err(ExportError::Unsupported(found)) if found == format)
            );
        }
    }
}
