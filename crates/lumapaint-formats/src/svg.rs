//! Independent editable SVG I/O. Parsing/serialization never uses a renderer or Skia.
use crate::export::{
    DocumentExporter, ExportError, ExportOptions, ExportSnapshot, ExportedDocument, FormatId,
};
use crate::{CompatibilityTier, ConversionIssue, ConversionReport};
use base64::Engine;
use lumapaint_core::document::{vector_svg, CanvasColor, Document};
use std::sync::{Arc, OnceLock};

fn mode_svg(source: String, mode: lumapaint_core::document::ColorMode) -> String {
    if mode != lumapaint_core::document::ColorMode::Grayscale {
        return source;
    }
    let end = source.find('>').unwrap() + 1;
    let close = source.rfind("</svg>").unwrap();
    format!("{}<defs><filter id=\"lp-gray-mode\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\" color-interpolation-filters=\"linearRGB\"><feColorMatrix type=\"matrix\" values=\".2126 .7152 .0722 0 0 .2126 .7152 .0722 0 0 .2126 .7152 .0722 0 0 0 0 0 1 0\"/></filter></defs><g filter=\"url(#lp-gray-mode)\">{}</g></svg>", &source[..end], &source[end..close])
}

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

/// Preserve unpainted geometry/text that the appearance parser intentionally omits.
fn unpainted_body(source: &str, index: usize) -> Result<String, ExportError> {
    let xml = roxmltree::Document::parse(source)
        .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
    let ids: Vec<_> = xml
        .descendants()
        .filter_map(|n| n.attribute("id"))
        .collect();
    let mut edits = Vec::new();
    for node in xml.descendants().filter(|n| n.is_element()) {
        for attribute in node.attributes() {
            let mut value = attribute.value().to_string();
            if attribute.name() == "id" {
                value = format!("lp-none-{index}-{value}");
            } else if ["clip-path", "mask", "fill", "stroke", "href"].contains(&attribute.name()) {
                for id in &ids {
                    value = value.replace(
                        &format!("url(#{id})"),
                        &format!("url(#lp-none-{index}-{id})"),
                    );
                }
            }
            if value != attribute.value() {
                edits.push((attribute.range_value(), value));
            }
        }
    }
    let mut preserved = source.to_string();
    for (range, value) in edits.into_iter().rev() {
        preserved.replace_range(range, &value);
    }
    Ok(preserved[preserved.find('>').unwrap() + 1..preserved.rfind("</svg>").unwrap()].into())
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
        if state.layer_effects.iter().any(|(id, e)| {
            e.enabled
                && if id == "layer-1" {
                    document.background_visible()
                } else {
                    document.visible_svg_layers().any(|l| &l.id == id)
                }
        }) {
            report.issues.push(ConversionIssue {
                code: "svg.layer_effects_rasterized",
                tier: CompatibilityTier::C,
            });
            if !options.allow_lossy {
                return Err(ExportError::LossyConversionRequiresConsent(report));
            }
            let png = snapshot
                .document_png()
                .ok_or(ExportError::UnsupportedFeature(
                    "svg.layer_effects_require_raster_fallback",
                ))?;
            let data = base64::engine::general_purpose::STANDARD.encode(png);
            return Ok(ExportedDocument { format: self.format(), media_type:"image/svg+xml", bytes: mode_svg(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><image width=\"100%\" height=\"100%\" href=\"data:image/png;base64,{data}\"/></svg>", state.width,state.height), state.color_mode).into_bytes(), report });
        }
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
                source.push_str(&format!(
                    "<rect width=\"{}\" height=\"{}\" fill=\"white\"/>",
                    state.width, state.height
                ));
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
            // usvg discards unpainted paths. Keep their editable geometry in SVG output.
            let no_paint: Vec<_> = layer
                .vector_objects
                .iter()
                .filter(|o| {
                    let empty_text = o.text.as_ref().is_some_and(|t| {
                        t.runs.iter().all(|r| r.style.no_color)
                            && (t.no_color
                                || t.runs.iter().map(|r| r.end - r.start).sum::<usize>()
                                    == t.content.encode_utf16().count())
                    });
                    layer.vector_layer
                        && o.visible
                        && (o.fill.is_none() || empty_text)
                        && o.stroke.is_none()
                        && o.fill_gradient.is_none()
                        && o.stroke_gradient.is_none()
                        && (o.text.is_none() || empty_text)
                        && o.image_frame.is_none()
                        && o.group_path.is_empty()
                        && o.clipping_group.is_none()
                })
                .cloned()
                .collect();
            let unpainted =
                unpainted_body(&vector_svg(state.width, state.height, &no_paint), index + 1)?;
            let layer_body = format!(
                "{}{}",
                layer_xml(&xml, index + 1, state.width, state.height)?,
                unpainted
            );
            source.push_str(&format!(
                "<g id=\"lp-layer-{}\" opacity=\"{}\">{}</g>",
                index + 1,
                layer.effective_opacity(),
                layer_body
            ));
        }
        if state.clipping_path_id.is_some() {
            source.push_str("</g>");
        }
        source.push_str("</svg>");
        Ok(ExportedDocument {
            format: self.format(),
            media_type: "image/svg+xml",
            bytes: mode_svg(source, state.color_mode).into_bytes(),
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::export;
    #[test]
    fn svg_keeps_unpainted_editable_geometry() {
        let mut d = Document::default();
        let layer = d.add_vector_layer().unwrap();
        let object = serde_json::from_value(serde_json::json!({"id":"empty-paint","name":"None", "path":{"data":"M10 10H30V40H10Z","fillRule":"nonZero"}, "transform":[1,0,0,1,0,0], "fill":null,"stroke":null,"strokeWidth":0,"visible":true})).unwrap();
        d.upsert_vector_object(&layer, object).unwrap();
        let exported = SvgExporter
            .export(&ExportSnapshot::capture(&d), ExportOptions::default())
            .unwrap();
        let xml = std::str::from_utf8(&exported.bytes).unwrap();
        let parsed = roxmltree::Document::parse(xml).unwrap();
        let path = parsed
            .descendants()
            .find(|n| n.has_tag_name("path"))
            .unwrap();
        assert_eq!(path.attribute("fill"), Some("none"));
        assert_eq!(path.attribute("stroke"), Some("none"));
        let data = path.attribute("d").unwrap();
        assert_eq!(
            lumapaint_core::bezier::cubic_contours(data).unwrap(),
            lumapaint_core::bezier::cubic_contours("M10 10H30V40H10Z").unwrap()
        );
        let imported = import("None".into(), xml.into()).unwrap();
        assert!(imported.svg_layers().next().unwrap().source.contains(data));
    }
    #[test]
    fn ordinary_effects_require_explicit_raster_fallback_and_consent() {
        let mut d = Document::default();
        d.set_layer_effects(
            "layer-1",
            lumapaint_core::layer_effects::LayerEffects {
                enabled: true,
                ..Default::default()
            },
        )
        .unwrap();
        let snapshot = ExportSnapshot::capture(&d);
        assert!(matches!(
            export(FormatId::Svg, &snapshot, ExportOptions::default()),
            Err(ExportError::LossyConversionRequiresConsent(_))
        ));
        assert!(matches!(
            export(
                FormatId::Svg,
                &snapshot,
                ExportOptions { allow_lossy: true }
            ),
            Err(ExportError::UnsupportedFeature(
                "svg.layer_effects_require_raster_fallback"
            ))
        ));
        let loaded =
            crate::native::decode(&crate::native::encode_state(&d.document_state()).unwrap())
                .unwrap();
        assert!(loaded.has_layer_effects("layer-1"));
    }
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

#[cfg(test)]
mod color_mode_tests {
    #[test]
    fn grayscale_export_applies_linear_luminance_after_compositing() {
        let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\" fill=\"red\"/></svg>";
        let gray = super::mode_svg(
            source.into(),
            lumapaint_core::document::ColorMode::Grayscale,
        );
        assert!(gray.contains("color-interpolation-filters=\"linearRGB\""));
        assert!(gray.contains(".2126 .7152 .0722"));
        assert!(super::tree(&gray).is_ok());
        assert_eq!(
            super::mode_svg(source.into(), lumapaint_core::document::ColorMode::Lab),
            source
        );
    }
}
