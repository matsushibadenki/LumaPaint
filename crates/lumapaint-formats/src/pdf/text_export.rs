//! Font preflight for positioned PDF text. No rasterizer or host state is used.
use std::collections::HashSet;

/// Font subsetting cannot faithfully represent variable/CFF2/color fonts yet.
/// Reject embedding for the page and retain its outlines rather than silently
/// dropping glyphs or emitting a PDF with an invalid font program.
fn subsettable(data: &[u8], index: u32) -> bool {
    let Ok(face) = ttf_parser::Face::parse(data, index) else {
        return false;
    };
    matches!(
        face.permissions(),
        Some(
            ttf_parser::Permissions::Installable
                | ttf_parser::Permissions::Editable
                | ttf_parser::Permissions::PreviewAndPrint
        )
    ) && face.is_subsetting_allowed()
        && face.is_outline_embedding_allowed()
        && !face.is_variable()
        && [b"CFF2", b"COLR", b"CBDT", b"sbix", b"SVG "]
            .iter()
            .all(|tag| {
                face.raw_face()
                    .table(ttf_parser::Tag::from_bytes(tag))
                    .is_none()
            })
}

pub(super) fn can_embed(tree: &usvg::Tree) -> bool {
    fn visit(group: &usvg::Group, fonts: &mut HashSet<usvg::fontdb::ID>, valid: &mut bool) {
        for node in group.children() {
            match node {
                usvg::Node::Group(group) => visit(group, fonts, valid),
                usvg::Node::Text(text) => {
                    for span in text.layouted() {
                        for glyph in &span.positioned_glyphs {
                            *valid &= glyph.id.0 != 0 && glyph.id.0 <= u16::MAX as u32;
                            fonts.insert(glyph.font);
                        }
                    }
                }
                usvg::Node::Image(image) => {
                    if let usvg::ImageKind::SVG(svg) = image.kind() {
                        *valid &= !svg.has_text_nodes() || can_embed(svg);
                    }
                    // Nested SVGs own their font database; do not resolve their
                    // face IDs against the enclosing tree's database.
                    continue;
                }
                _ => {}
            }
            node.subroots(|group| visit(group, fonts, valid));
        }
    }
    let mut fonts = HashSet::new();
    let mut valid = true;
    visit(tree.root(), &mut fonts, &mut valid);
    valid
        && tree.has_text_nodes()
        && fonts.into_iter().all(|id| {
            tree.fontdb()
                .with_face_data(id, subsettable)
                .unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tree(source: &str, font: Vec<u8>) -> usvg::Tree {
        let mut db = usvg::fontdb::Database::new();
        db.load_font_data(font);
        let family = db.faces().next().unwrap().families[0].0.clone();
        db.set_sans_serif_family(family);
        usvg::Tree::from_str(
            source,
            &usvg::Options {
                fontdb: std::sync::Arc::new(db),
                ..Default::default()
            },
        )
        .unwrap()
    }
    fn font() -> Vec<u8> {
        include_bytes!("../../tests/fixtures/lp-pdf-test.ttf").to_vec()
    }
    fn export(tree: &usvg::Tree) -> Vec<u8> {
        svg2pdf::to_pdf(
            tree,
            svg2pdf::ConversionOptions {
                embed_text: can_embed(tree),
                ..Default::default()
            },
            svg2pdf::PageOptions::default(),
        )
        .unwrap()
    }
    const SOURCE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="180" height="100"><text x="10" y="40" font-family="sans-serif" font-size="24" fill="#204080" transform="rotate(5 10 40)">ABあ</text></svg>"##;
    #[test]
    fn text_detection_descends_through_transformed_and_opacity_groups() {
        let nested = SOURCE
            .replace(
                "<text ",
                "<g opacity=\"0.5\"><g transform=\"translate(2 3)\"><text ",
            )
            .replace("</text>", "</text></g></g>");
        let tree = tree(&nested, font());
        assert!(tree.has_text_nodes());
        assert!(can_embed(&tree));
        let pdf = lopdf::Document::load_mem(&export(&tree)).unwrap();
        assert!(pdf
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(|d| d.has(b"ToUnicode")));
    }
    #[test]
    fn shared_glyphs_do_not_overwrite_distinct_unicode_strings() {
        let font = include_bytes!("../../tests/fixtures/lp-pdf-alias-test.ttf").to_vec();
        let source = SOURCE.replace("ABあ", "AＡA");
        let tree = tree(&source, font);
        let bytes = export(&tree);
        let mut pdf = lopdf::Document::load_mem(&bytes).unwrap();
        // lopdf extraction ignores ToUnicode for stream Encodings. In this
        // extraction-only clone, remove Encoding to exercise its ToUnicode
        // decoder. The reader below uses the unmodified bytes/CID Encoding.
        let mut custom = false;
        for object in pdf.objects.values_mut() {
            if let Ok(dict) = object.as_dict_mut() {
                if dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(b"Type0") {
                    custom |= dict.get(b"Encoding").unwrap().as_reference().is_ok();
                    dict.remove(b"Encoding");
                }
            }
        }
        assert!(custom);
        assert_eq!(pdf.extract_text(&[1]).unwrap().trim(), "AＡA");
        crate::pdf::read(
            &bytes,
            crate::io::ReadOptions {
                allow_lossy: true,
                raster_dpi: 72,
                ..Default::default()
            },
        )
        .unwrap();
    }
    #[test]
    fn embedded_truetype_has_unicode_subset_and_text_operators() {
        let tree = tree(SOURCE, font());
        assert!(can_embed(&tree));
        let pdf = lopdf::Document::load_mem(&export(&tree)).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        assert_eq!(pdf.extract_text(&[1]).unwrap().trim(), "ABあ");
        let content = lopdf::content::Content::decode(&pdf.get_page_content(page)).unwrap();
        assert!(content
            .operations
            .iter()
            .any(|op| matches!(op.operator.as_str(), "Tj" | "TJ")));
        assert!(pdf
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(|d| d.has(b"ToUnicode")));
        let subset = pdf
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .find_map(|s| {
                let data = s.get_plain_content().ok()?;
                ttf_parser::Face::parse(&data, 0).ok()?;
                Some(data)
            })
            .unwrap();
        assert!(subsetting_preserves_glyphs(&subset));
    }
    fn subsetting_preserves_glyphs(data: &[u8]) -> bool {
        let face = ttf_parser::Face::parse(data, 0).unwrap();
        // .notdef plus the three drawn glyphs; subset cmap is not required by PDF.
        face.number_of_glyphs() == 4
    }
    fn with_embedding_flags(mut data: Vec<u8>, flags: u16) -> Vec<u8> {
        let count = u16::from_be_bytes(data[4..6].try_into().unwrap()) as usize;
        let table = (0..count)
            .map(|i| 12 + i * 16)
            .find(|i| &data[*i..*i + 4] == b"OS/2")
            .unwrap();
        let offset = u32::from_be_bytes(data[table + 8..table + 12].try_into().unwrap()) as usize;
        data[offset + 8..offset + 10].copy_from_slice(&flags.to_be_bytes());
        data
    }
    #[test]
    fn restricted_no_subset_and_bitmap_only_fonts_keep_outline_fallback() {
        for flags in [2, 0x100, 0x200] {
            let data = with_embedding_flags(font(), flags);
            assert!(!subsettable(&data, 0));
            let tree = tree(SOURCE, data);
            assert!(!can_embed(&tree));
            let pdf = lopdf::Document::load_mem(&export(&tree)).unwrap();
            let page = *pdf.get_pages().values().next().unwrap();
            let content = lopdf::content::Content::decode(&pdf.get_page_content(page)).unwrap();
            assert!(!content
                .operations
                .iter()
                .any(|op| matches!(op.operator.as_str(), "Tj" | "TJ")));
            assert!(!pdf
                .objects
                .values()
                .filter_map(|o| o.as_dict().ok())
                .any(|d| d.has(b"FontFile2")));
        }
    }
    #[test]
    fn cff_japanese_and_vertical_glyphs_keep_unicode() {
        let font = include_bytes!("../../tests/fixtures/lp-japanese-cid.otf").to_vec();
        let source = r#"<svg width="100" height="100"><text x="50" y="20" font-family="sans-serif" font-size="24" writing-mode="tb">あい</text></svg>"#;
        let tree = tree(source, font);
        assert!(can_embed(&tree));
        let bytes = export(&tree);
        let pdf = lopdf::Document::load_mem(&bytes).unwrap();
        assert_eq!(pdf.extract_text(&[1]).unwrap().trim(), "あい");
        assert!(pdf
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(|d| d.has(b"FontFile3")));
        // Our reader can also decode the embedded subset without system fonts.
        crate::pdf::read(
            &bytes,
            crate::io::ReadOptions {
                allow_lossy: true,
                raster_dpi: 72,
                ..Default::default()
            },
        )
        .unwrap();
    }
    #[test]
    fn filled_and_stroked_text_has_one_searchable_copy_in_either_paint_order() {
        for order in ["fill stroke", "stroke fill"] {
            let source = SOURCE.replace(
                "fill=",
                &format!("stroke=\"red\" stroke-width=\"2\" paint-order=\"{order}\" fill="),
            );
            let tree = tree(&source, font());
            let pdf = lopdf::Document::load_mem(&export(&tree)).unwrap();
            assert_eq!(pdf.extract_text(&[1]).unwrap().trim(), "ABあ");
            let page = *pdf.get_pages().values().next().unwrap();
            let content = lopdf::content::Content::decode(&pdf.get_page_content(page)).unwrap();
            assert!(content.operations.iter().any(|op| op.operator == "S"));
        }
    }
    #[test]
    fn long_line_is_split_into_bounded_positioning_arrays_without_losing_text() {
        let content = "A".repeat(9000);
        let source = SOURCE
            .replace("ABあ", &content)
            .replace("font-size=\"24\"", "font-size=\"1\"");
        let tree = tree(&source, font());
        let pdf = lopdf::Document::load_mem(&export(&tree)).unwrap();
        let page = *pdf.get_pages().values().next().unwrap();
        let operations = lopdf::content::Content::decode(&pdf.get_page_content(page))
            .unwrap()
            .operations;
        let arrays: Vec<_> = operations
            .iter()
            .filter(|op| op.operator == "TJ")
            .map(|op| op.operands[0].as_array().unwrap())
            .collect();
        assert_eq!(arrays.len(), 3);
        assert!(arrays.iter().all(|array| array.len() <= 8192));
        assert_eq!(
            pdf.extract_text(&[1])
                .unwrap()
                .split_whitespace()
                .collect::<String>(),
            content
        );
    }
    #[test]
    fn long_native_text_box_keeps_all_visible_lines_in_pdf_and_native_save() {
        use crate::export::{DocumentExporter, ExportOptions, ExportSnapshot};
        use lumapaint_core::{
            document::{Document, TextSettings},
            vector::VectorText,
        };
        let content = (0..400)
            .map(|i| format!("ABC line {i:03}: long text inside a text box."))
            .collect::<Vec<_>>()
            .join("\n");
        let mut state = Document::default().document_state();
        state.width = 640;
        state.height = 8192;
        let mut doc = Document::from_document_state(state).unwrap();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: content.clone(),
                font_size: 10.,
                line_height: 1.2,
                box_width: 600.,
                box_height: Some(8100.),
                ..Default::default()
            },
            position: [20., 20.],
            color: [32, 64, 128],
        })
        .unwrap();
        let saved = crate::native::encode_state(&doc.document_state()).unwrap();
        let restored = crate::native::decode(&saved).unwrap();
        assert_eq!(restored.snapshot().text_objects[0].text.content, content);
        let pdf = super::super::PdfExporter
            .export(
                &ExportSnapshot::capture(&restored),
                ExportOptions { allow_lossy: true },
            )
            .unwrap();
        assert!(pdf
            .report
            .issues
            .iter()
            .any(|i| i.code == "pdf.text_positioned"));
        let parsed = lopdf::Document::load_mem(&pdf.bytes).unwrap();
        let extracted = parsed.extract_text(&[1]).unwrap();
        // lopdf adds a heuristic trailing space after every TJ array. Compare
        // actual lines without that extractor-only suffix, preserving interiors.
        assert_eq!(
            extracted
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n"),
            content
        );
        assert_eq!(extracted.lines().count(), 400);
        crate::pdf::read(
            &pdf.bytes,
            crate::io::ReadOptions {
                allow_lossy: true,
                raster_dpi: 72,
                ..Default::default()
            },
        )
        .unwrap();
    }
    #[test]
    fn pdf_export_dispatch_and_publication_preserve_text_without_mutating_source() {
        use crate::export::{DocumentExporter, ExportOptions, ExportSnapshot};
        let source = r#"<svg width="180" height="100"><text x="10" y="40" font-family="sans-serif" font-size="24">ABC</text></svg>"#;
        let doc = crate::svg::import("Text".into(), source.into()).unwrap();
        let snapshot = ExportSnapshot::capture(&doc);
        let before = crate::native::encode_state(&doc.document_state()).unwrap();
        assert!(matches!(
            super::super::PdfExporter.export(&snapshot, ExportOptions::default()),
            Err(crate::export::ExportError::LossyConversionRequiresConsent(
                _
            ))
        ));
        let mut publication = super::super::PdfPublication::default();
        for _ in 0..2 {
            let exported = super::super::PdfExporter
                .export(&snapshot, ExportOptions { allow_lossy: true })
                .unwrap();
            assert!(exported
                .report
                .issues
                .iter()
                .any(|i| i.code == "pdf.text_positioned"));
            assert!(!exported
                .report
                .issues
                .iter()
                .any(|i| i.code == "pdf.text_outlined"));
            let pdf = lopdf::Document::load_mem(&exported.bytes).unwrap();
            assert_eq!(pdf.extract_text(&[1]).unwrap().trim(), "ABC");
            publication.push(exported).unwrap();
        }
        let pdf = publication
            .finish(ExportOptions { allow_lossy: true })
            .unwrap();
        let pdf = lopdf::Document::load_mem(&pdf.bytes).unwrap();
        for page in [1, 2] {
            assert_eq!(pdf.extract_text(&[page]).unwrap().trim(), "ABC");
        }
        assert_eq!(
            before,
            crate::native::encode_state(&doc.document_state()).unwrap()
        );
    }
    #[test]
    fn missing_glyphs_and_empty_text_cannot_claim_embedded_text() {
        let missing = tree(&SOURCE.replace("ABあ", "未知"), font());
        assert!(!can_embed(&missing));
        let empty = tree("<svg width=\"10\" height=\"10\"/>", font());
        assert!(!can_embed(&empty));
        assert!(!subsettable(b"broken font", 0));
    }
}
