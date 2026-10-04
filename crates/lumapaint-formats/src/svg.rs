//! Independent editable SVG I/O. Parsing/serialization never uses a renderer or Skia.
use crate::export::{
    DocumentExporter, ExportError, ExportOptions, ExportSnapshot, ExportedDocument, FormatId,
};
use crate::{CompatibilityTier, ConversionIssue, ConversionReport};
use base64::Engine;
use lumapaint_core::document::{vector_svg, CanvasColor, Document};
use std::sync::{Arc, OnceLock};

pub fn import(name: String, source: String) -> Result<Document, String> {
    let tree = tree(&source).map_err(|e| e.to_string())?;
    let mut state = Document::default().document_state();
    state.width = tree.size().width().ceil() as u32;
    state.height = tree.size().height().ceil() as u32;
    state.canvas_color = Some(CanvasColor::Transparent);
    let mut document = Document::from_document_state(state)?;
    document.import_svg(name, source)?;
    Ok(document)
}

pub(crate) fn tree(source: &str) -> Result<usvg::Tree, ExportError> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone();
    usvg::Tree::from_str(
        source,
        &usvg::Options {
            fontdb: fonts,
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
                resolve_string: Box::new(|_, _| None),
            },
            ..Default::default()
        },
    )
    .map_err(|e| ExportError::InvalidDocument(e.to_string()))
}

/// Resolve CSS/use into explicit editable elements. Prefix all generated IDs and references
/// to isolate gradients, clips and masks between layers. Keep editable text/font references.
fn layer_xml(source: &str, index: usize, width: u32, height: u32) -> Result<String, ExportError> {
    let parsed = tree(source)?;
    let size = parsed.size();
    let scale = (width as f32 / size.width()).min(height as f32 / size.height());
    let x = (width as f32 - size.width() * scale) * 0.5;
    let y = (height as f32 - size.height() * scale) * 0.5;
    let normalized = parsed.to_string(&usvg::WriteOptions {
        id_prefix: Some(format!("lp{index}-")),
        preserve_text: true,
        ..Default::default()
    });
    // Use a group instead of a nested SVG viewport, whose implicit overflow clip can
    // crop filter/stroke extents. The document viewport owns the final export clip.
    let xml = roxmltree::Document::parse(&normalized)
        .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
    let root = xml.root_element();
    let body: String = root
        .children()
        .filter(|n| n.is_element())
        .map(|n| &normalized[n.range()])
        .collect();
    Ok(format!(
        "<g transform=\"translate({x} {y}) scale({scale})\">{body}</g>"
    ))
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
        let document =
            Document::from_document_state(state.clone()).map_err(ExportError::InvalidDocument)?;
        let mut report = ConversionReport::default();
        let brush = state.layer_visible && !state.strokes.is_empty();
        if brush {
            report.issues.push(ConversionIssue {
                code: "svg.brush_rasterized",
                tier: CompatibilityTier::C,
            });
            if snapshot.paint_png().is_none() {
                return Err(ExportError::UnsupportedFeature(
                    "svg.paint_composite_required",
                ));
            }
        }
        let has_text = state.svg_layers.iter().filter(|l| l.visible).any(|l| {
            roxmltree::Document::parse(&l.source)
                .is_ok_and(|xml| xml.descendants().any(|n| n.has_tag_name("text")))
        });
        if has_text {
            report.issues.push(ConversionIssue {
                code: "svg.text_requires_fonts",
                tier: CompatibilityTier::B,
            });
        }
        if !report.issues.is_empty() && !options.allow_lossy {
            return Err(ExportError::LossyConversionRequiresConsent(report));
        }
        let mut source = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">", state.width, state.height, state.width, state.height);
        if let Some(mask) = document.clipping_mask_svg() {
            source.push_str(&format!("<defs><mask id=\"lp-document-clip\" maskUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"{}\" height=\"{}\">{}</mask></defs><g mask=\"url(#lp-document-clip)\">", state.width, state.height, layer_xml(&mask, 1000, state.width, state.height)?));
        }
        // Visibility/group masks are resolved from the Rust document, as on screen.

        if document.background_visible() {
            source.push_str(&format!(
                "<g opacity=\"{}\">",
                document.paint_layer_opacity()
            ));
            if state.canvas_color.unwrap_or_default() == CanvasColor::White {
                source.push_str("<rect width=\"100%\" height=\"100%\" fill=\"white\"/>");
            }
            if brush {
                let data =
                    base64::engine::general_purpose::STANDARD.encode(snapshot.paint_png().unwrap());
                source.push_str(&format!(
                    "<image width=\"{}\" height=\"{}\" href=\"data:image/png;base64,{data}\"/>",
                    state.width, state.height
                ));
            } else if let Some(paint) = &state.paint_source {
                source.push_str(&layer_xml(paint, 0, state.width, state.height)?);
            }
            source.push_str("</g>");
        }
        for (index, layer) in state.svg_layers.iter().enumerate() {
            if !layer.visible {
                continue;
            }
            let xml = if layer.vector_layer {
                vector_svg(state.width, state.height, &layer.vector_objects)
            } else {
                layer.source.clone()
            };
            source.push_str(&format!(
                "<g id=\"lp-layer-{}\" opacity=\"{}\">{}</g>",
                index + 1,
                layer.effective_opacity(),
                layer_xml(&xml, index + 1, state.width, state.height)?
            ));
        }
        if state.clipping_path_id.is_some() {
            source.push_str("</g>");
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
    fn isolated_editable_layers_keep_ids_references_and_document_unchanged() {
        let first = r##"<svg width="80" height="60"><defs><linearGradient id="same"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs><style>rect {fill:url(#same)}</style><rect width="80" height="60"/></svg>"##;
        let mut document = import("日本語 简体中文 English".into(), first.into()).unwrap();
        document
            .import_svg("second".into(), first.replace("red", "green"))
            .unwrap();
        let before = crate::native::encode_state(&document.document_state()).unwrap();
        let result = export(
            FormatId::Svg,
            &ExportSnapshot::capture(&document),
            ExportOptions::default(),
        )
        .unwrap();
        let xml = String::from_utf8(result.bytes).unwrap();
        assert!(!xml.contains("data:image/svg"));
        assert!(xml.contains("lp1-same") && xml.contains("lp2-same"));
        assert!(xml.contains("<path") && xml.contains("<linearGradient"));
        assert!(result.report.issues.is_empty());
        assert_eq!(tree(&xml).unwrap().size(), tree(first).unwrap().size());
        assert_eq!(
            crate::native::encode_state(&document.document_state()).unwrap(),
            before
        );
    }
    #[test]
    fn active_brush_is_not_dropped_and_requires_a_composite_and_report() {
        use lumapaint_core::document::{Brush, Point};
        let mut document = Document::default();
        document
            .begin(Point { x: 4., y: 8. }, Brush::default())
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
            Err(ExportError::UnsupportedFeature(
                "svg.paint_composite_required"
            ))
        ));
        assert!(document.has_active_stroke());
        assert_eq!(document.snapshot().revision, before.revision);
    }
}
