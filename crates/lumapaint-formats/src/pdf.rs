//! PDF conversion uses only independent parsers/writers; never a renderer or GPU.
use crate::export::{
    DocumentExporter, ExportError, ExportOptions, ExportSnapshot, ExportedDocument, FormatId,
};
use crate::{
    io::{ReadDocument, ReadOptions},
    CompatibilityTier, ConversionIssue, ConversionReport, ImportError,
};
use base64::Engine;
mod read;
pub fn read(bytes: &[u8], options: ReadOptions) -> Result<ReadDocument, ImportError> {
    read::read(bytes, options)
}

pub struct PdfExporter;
impl DocumentExporter for PdfExporter {
    fn format(&self) -> FormatId {
        FormatId::Pdf
    }
    fn export(
        &self,
        snapshot: &ExportSnapshot,
        options: ExportOptions,
    ) -> Result<ExportedDocument, ExportError> {
        let svg = crate::svg::SvgExporter.export(snapshot, ExportOptions { allow_lossy: true })?;
        let xml = std::str::from_utf8(&svg.bytes)
            .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
        let doc = roxmltree::Document::parse(xml)
            .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
        let effects = doc.descendants().any(|n| {
            n.has_tag_name("filter") || n.attribute("spreadMethod").is_some_and(|s| s != "pad")
        });
        let mut report = svg.report;
        report
            .issues
            .retain(|i| i.code != "svg.text_requires_fonts");
        let outlined = doc.descendants().any(|n| n.has_tag_name("text"));
        let tree = if effects {
            let png = snapshot
                .document_png()
                .ok_or(ExportError::UnsupportedFeature(
                    "pdf.effects_require_raster_fallback",
                ))?;
            report.issues.push(ConversionIssue {
                code: "pdf.effects_rasterized",
                tier: CompatibilityTier::C,
            });
            let state = snapshot.state();
            let data = base64::engine::general_purpose::STANDARD.encode(png);
            crate::svg::tree(&format!("<svg width=\"{}\" height=\"{}\" xmlns=\"http://www.w3.org/2000/svg\"><image width=\"{}\" height=\"{}\" href=\"data:image/png;base64,{data}\"/></svg>",state.width,state.height,state.width,state.height))?
        } else {
            if outlined {
                report.issues.push(ConversionIssue {
                    code: "pdf.text_outlined",
                    tier: CompatibilityTier::B,
                });
            }
            // No svg2pdf text/filter renderer features: convert font-shaped glyphs to paths
            // using the parser, then serialize a second tree made only of vector elements.
            let parsed = crate::svg::tree(xml)?;
            crate::svg::tree(&parsed.to_string(&usvg::WriteOptions::default()))?
        };
        if !report.issues.is_empty() && !options.allow_lossy {
            return Err(ExportError::LossyConversionRequiresConsent(report));
        }
        let bytes = svg2pdf::to_pdf(
            &tree,
            svg2pdf::ConversionOptions {
                embed_text: false,
                ..Default::default()
            },
            svg2pdf::PageOptions {
                dpi: snapshot.state().resolution.unwrap_or(72) as f32,
            },
        )
        .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
        Ok(ExportedDocument {
            format: FormatId::Pdf,
            media_type: "application/pdf",
            bytes,
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pdf_import_resolution_preserves_physical_page_size_on_export() {
        let document = crate::svg::import(
            "PDF size".into(),
            "<svg width=\"120\" height=\"80\"><rect width=\"100\" height=\"60\"/></svg>".into(),
        )
        .unwrap();
        let pdf = PdfExporter
            .export(
                &ExportSnapshot::capture(&document),
                ExportOptions::default(),
            )
            .unwrap();
        let crate::io::ReadContent::Vector(document) = read(
            &pdf.bytes,
            ReadOptions {
                raster_dpi: 144,
                ..Default::default()
            },
        )
        .unwrap()
        .content
        else {
            panic!()
        };
        assert_eq!(document.dimensions(), (240, 160));
        assert_eq!(document.document_state().resolution, Some(144));
        let pdf = PdfExporter
            .export(
                &ExportSnapshot::capture(&document),
                ExportOptions::default(),
            )
            .unwrap();
        let parsed = lopdf::Document::load_mem(&pdf.bytes).unwrap();
        let page = *parsed.get_pages().values().next().unwrap();
        let bounds = parsed
            .get_dictionary(page)
            .unwrap()
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(bounds[2].as_float().unwrap(), 120.);
        assert_eq!(bounds[3].as_float().unwrap(), 80.);
    }
    #[test]
    fn independent_vector_pdf_writer_and_bounded_parser_round_trip_paths() {
        let document=crate::svg::import("PDF test".into(),r#"<svg width="120" height="80"><path d="M10 10H100V60H10Z" fill="red" stroke="blue" stroke-width="3"/></svg>"#.into()).unwrap();
        let snapshot = ExportSnapshot::capture(&document);
        let result = PdfExporter
            .export(&snapshot, ExportOptions::default())
            .unwrap();
        assert!(result.bytes.starts_with(b"%PDF-"));
        assert!(result.report.issues.is_empty());
        let converted = read(
            &result.bytes,
            ReadOptions {
                raster_dpi: 72,
                ..Default::default()
            },
        )
        .unwrap();
        let crate::io::ReadContent::Vector(decoded) = converted.content else {
            panic!()
        };
        assert_eq!(decoded.dimensions(), (120, 80));
        assert!(decoded
            .svg_layers()
            .next()
            .unwrap()
            .source
            .contains("<path"));
        assert!(converted.report.issues.is_empty());
    }
    #[test]
    fn malformed_encrypted_or_missing_pages_never_become_empty_artwork() {
        for bytes in [b"not a pdf".as_slice(), b"%PDF-1.7\n".as_slice()] {
            assert!(read(bytes, ReadOptions::default()).is_err());
        }
        let document = lumapaint_core::document::Document::default();
        let result = PdfExporter
            .export(
                &ExportSnapshot::capture(&document),
                ExportOptions::default(),
            )
            .unwrap();
        assert!(read(
            &result.bytes,
            ReadOptions {
                page_index: 1,
                ..Default::default()
            }
        )
        .is_err());
        assert!(crate::io::read_document(
            FormatId::Illustrator,
            "AI".into(),
            b"%!PS-Adobe",
            ReadOptions::default()
        )
        .is_err());
    }
}
