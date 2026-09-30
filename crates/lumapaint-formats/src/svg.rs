//! Independent SVG I/O. No Skia, GPU, font database or renderer dependency.
//! Initial exporter preserves isolated SVG layer sources as embedded SVG images.
use crate::export::{
    appearance_report, DocumentExporter, ExportError, ExportOptions, ExportSnapshot,
    ExportedDocument, FormatId,
};
use base64::Engine;
use lumapaint_core::document::{CanvasColor, Document};

/// Source-preserving import into the native model. Runtime editing services are attached by the host.
pub fn import(name: String, source: String) -> Result<Document, String> {
    let mut document = Document::default();
    document.import_svg(name, source)?;
    Ok(document)
}

pub struct SvgExporter;
impl DocumentExporter for SvgExporter {
    fn format(&self) -> FormatId {
        FormatId::Svg
    }
    fn export(
        &self,
        snapshot: &ExportSnapshot,
        options: ExportOptions,
    ) -> Result<ExportedDocument, ExportError> {
        let state = snapshot.state();
        Document::from_document_state(state.clone()).map_err(ExportError::InvalidDocument)?;
        if state.clipping_path_id.is_some() {
            return Err(ExportError::UnsupportedFeature("svg.document_clipping"));
        }
        if state.layer_visible && !state.strokes.is_empty() {
            return Err(ExportError::UnsupportedFeature("svg.brush_strokes"));
        }
        let embedded = (state.layer_visible && state.paint_source.is_some())
            || state.svg_layers.iter().any(|layer| layer.visible);
        let report = if embedded {
            appearance_report()
        } else {
            Default::default()
        };
        if embedded && !options.allow_lossy {
            return Err(ExportError::LossyConversionRequiresConsent(report));
        }
        let mut source = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">", state.width, state.height, state.width, state.height);
        if state.layer_visible && state.canvas_color.unwrap_or_default() == CanvasColor::White {
            source.push_str("<rect width=\"100%\" height=\"100%\" fill=\"white\"/>");
        }
        let mut image = |xml: &str, opacity: f32| {
            let data = base64::engine::general_purpose::STANDARD.encode(xml.as_bytes());
            source.push_str(&format!("<image width=\"{}\" height=\"{}\" preserveAspectRatio=\"xMidYMid meet\" opacity=\"{}\" href=\"data:image/svg+xml;base64,{}\"/>", state.width, state.height, opacity, data));
        };
        if state.layer_visible {
            if let Some(paint) = &state.paint_source {
                let mask = if state.layer_mask_enabled.unwrap_or(false) {
                    let density = state.layer_mask_density.unwrap_or(1.0);
                    if state.layer_mask_inverted.unwrap_or(false) {
                        1.0 - density
                    } else {
                        density
                    }
                } else {
                    1.0
                };
                image(paint, state.layer_opacity.unwrap_or(1.0) * mask);
            }
        }
        for layer in state.svg_layers.iter().filter(|layer| layer.visible) {
            image(&layer.source, layer.effective_opacity());
        }
        source.push_str("</svg>");
        Ok(ExportedDocument {
            format: self.format(),
            media_type: "image/svg+xml",
            bytes: source.into_bytes(),
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::export;
    #[test]
    fn isolated_layers_require_consent_and_keep_sources_and_order() {
        let first = "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect id=\"same\" width=\"8\" height=\"8\"/></svg>";
        let second = first.replace("rect", "ellipse");
        let mut document = import("日本語 简体中文 English".into(), first.into()).unwrap();
        document
            .import_svg("second".into(), second.clone())
            .unwrap();
        let before = crate::native::encode_state(&document.document_state()).unwrap();
        let snapshot = ExportSnapshot::capture(&document);
        assert!(matches!(
            export(FormatId::Svg, &snapshot, ExportOptions::default()),
            Err(ExportError::LossyConversionRequiresConsent(_))
        ));
        let result = export(
            FormatId::Svg,
            &snapshot,
            ExportOptions { allow_lossy: true },
        )
        .unwrap();
        let xml = String::from_utf8(result.bytes).unwrap();
        let one = base64::engine::general_purpose::STANDARD.encode(first);
        let two = base64::engine::general_purpose::STANDARD.encode(second);
        assert!(xml.find(&one).unwrap() < xml.find(&two).unwrap());
        assert_eq!(
            result.report.issues[0].code,
            "svg.embedded_layers_not_editable"
        );
        assert_eq!(
            crate::native::encode_state(&document.document_state()).unwrap(),
            before
        );
    }
    #[test]
    fn active_brush_content_is_captured_but_never_silently_dropped() {
        use lumapaint_core::document::{Brush, Point};
        let mut document = Document::default();
        document
            .begin(Point { x: 4.0, y: 8.0 }, Brush::default())
            .unwrap();
        let before = document.snapshot();
        let snapshot = ExportSnapshot::capture(&document);
        assert_eq!(snapshot.state().strokes.len(), 1);
        assert!(matches!(
            export(
                FormatId::Svg,
                &snapshot,
                ExportOptions { allow_lossy: true }
            ),
            Err(ExportError::UnsupportedFeature("svg.brush_strokes"))
        ));
        assert!(document.has_active_stroke());
        assert_eq!(document.snapshot().revision, before.revision);
        assert_eq!(document.snapshot().dirty, before.dirty);
    }
}
