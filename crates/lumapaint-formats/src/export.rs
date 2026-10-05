//! Export contracts accept immutable native model data and never a renderer or UI state.
use crate::ConversionReport;
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
    Psb,
    Ora,
    Exr,
    Kra,
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
    Tiff,
    Raw,
    Heif,
    Ico,
    Avif,
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

/// Derived from the shared registry so dialogs and exporter support cannot drift apart.
pub fn capabilities() -> Vec<FormatCapabilities> {
    crate::io::FILE_FORMATS
        .iter()
        .map(|entry| FormatCapabilities {
            format: entry.format,
            import: entry.import_adapter,
            export: entry.export_adapter,
            partial: entry.partial,
        })
        .collect()
}

/// Captures current committed plus active content without changing revision, history or saved state.
/// The private state prevents callers bypassing model validation.
#[derive(Clone, Debug)]
pub struct ExportSnapshot {
    state: DocumentState,
    paint_png: Option<Vec<u8>>,
    document_png: Option<Vec<u8>>,
}
impl ExportSnapshot {
    pub fn capture(document: &Document) -> Self {
        let mut copy = document.clone();
        copy.finish();
        Self {
            state: copy.document_state(),
            paint_png: None,
            document_png: None,
        }
    }
    pub fn state(&self) -> &DocumentState {
        &self.state
    }
    /// Optional host-produced paint composite, before layer opacity/masks. Never crosses IPC.
    /// Keep the model snapshot intact, including its editable brush history.
    pub fn with_paint_png(mut self, png: Vec<u8>) -> Result<Self, ExportError> {
        self.validate_png(&png)?;
        self.paint_png = Some(png);
        Ok(self)
    }
    pub fn with_document_png(mut self, png: Vec<u8>) -> Result<Self, ExportError> {
        self.validate_png(&png)?;
        self.document_png = Some(png);
        Ok(self)
    }
    fn validate_png(&self, bytes: &[u8]) -> Result<(), ExportError> {
        let invalid = || ExportError::InvalidDocument("Invalid export PNG".into());
        if bytes.len() > 128 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_limits(png::Limits {
            bytes: 128 * 1024 * 1024,
        });
        let mut reader = decoder.read_info().map_err(|_| invalid())?;
        let info = reader.info();
        if (info.width, info.height) != (self.state.width, self.state.height)
            || info.animation_control.is_some()
        {
            return Err(invalid());
        }
        let size = reader
            .output_buffer_size()
            .filter(|s| *s <= 128 * 1024 * 1024)
            .ok_or_else(invalid)?;
        let mut pixels = vec![0; size];
        reader.next_frame(&mut pixels).map_err(|_| invalid())?;
        reader.finish().map_err(|_| invalid())?;
        Ok(())
    }
    pub(crate) fn document_png(&self) -> Option<&[u8]> {
        self.document_png.as_deref()
    }
    pub(crate) fn paint_png(&self) -> Option<&[u8]> {
        self.paint_png.as_deref()
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
        FormatId::Pdf => crate::pdf::PdfExporter.export(snapshot, options),
        other => Err(ExportError::Unsupported(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raster_attachment_rejects_truncated_or_wrong_size_png() {
        let snapshot = ExportSnapshot::capture(&Document::default());
        assert!(snapshot
            .clone()
            .with_paint_png(b"\x89PNG\r\n\x1a\n".to_vec())
            .is_err());
        let mut bytes = Vec::new();
        {
            let encoder = png::Encoder::new(&mut bytes, 1, 1);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0]).unwrap();
        }
        assert!(snapshot.with_document_png(bytes).is_err());
    }
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
        for format in [FormatId::Illustrator, FormatId::Psd, FormatId::Psb] {
            assert!(
                matches!(export(format, &snapshot, ExportOptions { allow_lossy: true }), Err(ExportError::Unsupported(found)) if found == format)
            );
        }
    }
}
