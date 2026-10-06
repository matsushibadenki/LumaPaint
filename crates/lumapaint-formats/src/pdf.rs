//! PDF conversion uses only independent parsers/writers; never a renderer or GPU.
use crate::export::{
    DocumentExporter, ExportError, ExportOptions, ExportSnapshot, ExportedDocument, FormatId,
};
use crate::{
    io::{ReadDocument, ReadOptions},
    CompatibilityTier, ConversionIssue, ConversionReport, ImportError,
};
use base64::Engine;
mod print_layout;
mod read;
mod registration;
pub use read::{inspect, read_all, read_selected, PageInfo};
pub fn read(bytes: &[u8], options: ReadOptions) -> Result<ReadDocument, ImportError> {
    read::read(bytes, options)
}

pub struct PdfExporter;
/// Assemble independently exported pages without rendering, changing DPI, or
/// discarding their own fonts, images, transparency and resource dictionaries.
pub struct PdfPublication {
    document: lopdf::Document,
    pages: Vec<lopdf::ObjectId>,
    report: ConversionReport,
    input_bytes: usize,
}
impl Default for PdfPublication {
    fn default() -> Self {
        Self {
            document: lopdf::Document::with_version("1.7"),
            pages: Vec::new(),
            report: ConversionReport::default(),
            input_bytes: 0,
        }
    }
}
impl PdfPublication {
    pub fn push(&mut self, exported: ExportedDocument) -> Result<(), ExportError> {
        let invalid = || ExportError::InvalidDocument("Invalid PDF publication page".into());
        if exported.format != FormatId::Pdf || self.pages.len() >= 512 {
            return Err(invalid());
        }
        let total = self
            .input_bytes
            .checked_add(exported.bytes.len())
            .ok_or_else(invalid)?;
        if total > 128 * 1024 * 1024 {
            return Err(invalid());
        }
        let mut page = lopdf::Document::load_mem(&exported.bytes).map_err(|_| invalid())?;
        if page.get_pages().len() != 1 || page.is_encrypted() || page.objects.len() > 100_000 {
            return Err(invalid());
        }
        let id = *page.get_pages().values().next().ok_or_else(invalid)?;
        // Flatten inherited attributes before replacing the page's Parent.
        // Referenced resources remain in the source object graph and are renumbered.
        let mut dictionary = page.get_dictionary(id).map_err(|_| invalid())?.clone();
        let mut parent = dictionary.get(b"Parent").ok().cloned();
        let mut ancestors = std::collections::BTreeSet::new();
        while let Some(object) = parent {
            let parent_id = object.as_reference().map_err(|_| invalid())?;
            if !ancestors.insert(parent_id) || ancestors.len() > 64 {
                return Err(invalid());
            }
            let node = page.get_dictionary(parent_id).map_err(|_| invalid())?;
            for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
                if dictionary.get(key).is_err() {
                    if let Ok(value) = node.get(key) {
                        dictionary.set(key, value.clone());
                    }
                }
            }
            parent = node.get(b"Parent").ok().cloned();
        }
        page.objects
            .insert(id, lopdf::Object::Dictionary(dictionary));
        page.renumber_objects_with(self.document.max_id + 1);
        let id = *page.get_pages().values().next().ok_or_else(invalid)?;
        if self.document.objects.len() + page.objects.len() > 100_000 {
            return Err(invalid());
        }
        self.document.max_id = self.document.max_id.max(page.max_id);
        self.document.objects.extend(page.objects);
        self.pages.push(id);
        self.input_bytes = total;
        for issue in exported.report.issues {
            if !self.report.issues.contains(&issue) {
                self.report.issues.push(issue);
            }
        }
        Ok(())
    }
    pub fn finish(mut self, options: ExportOptions) -> Result<ExportedDocument, ExportError> {
        use lopdf::{dictionary, Object};
        if self.pages.is_empty() {
            return Err(ExportError::InvalidDocument("Empty PDF publication".into()));
        }
        if !options.allow_lossy && !self.report.issues.is_empty() {
            return Err(ExportError::LossyConversionRequiresConsent(self.report));
        }
        let root = self.document.new_object_id();
        for id in &self.pages {
            self.document
                .get_dictionary_mut(*id)
                .map_err(|e| ExportError::InvalidDocument(e.to_string()))?
                .set("Parent", root);
        }
        self.document.objects.insert(
            root,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Count" => self.pages.len() as i64,
                "Kids" => self.pages.iter().copied().map(Object::Reference).collect::<Vec<_>>()
            }),
        );
        let catalog = self
            .document
            .add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
        self.document.trailer.set("Root", catalog);
        self.document.prune_objects();
        let mut bytes = Vec::new();
        self.document
            .save_to(&mut bytes)
            .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
        if bytes.len() > 128 * 1024 * 1024 {
            return Err(ExportError::InvalidDocument(
                "PDF publication exceeds 128 MiB".into(),
            ));
        }
        Ok(ExportedDocument {
            format: FormatId::Pdf,
            media_type: "application/pdf",
            bytes,
            report: self.report,
        })
    }
}
impl DocumentExporter for PdfExporter {
    fn format(&self) -> FormatId {
        FormatId::Pdf
    }
    fn export(
        &self,
        snapshot: &ExportSnapshot,
        options: ExportOptions,
    ) -> Result<ExportedDocument, ExportError> {
        let layout = print_layout::PrintLayout::capture(snapshot)?;
        if layout.is_some() && snapshot.state().layer_effects.values().any(|e| e.enabled) {
            return Err(ExportError::UnsupportedFeature(
                "pdf.print_bleed_requires_vector_content",
            ));
        }
        let svg = crate::svg::SvgExporter.export(snapshot, ExportOptions { allow_lossy: true })?;
        let xml = std::str::from_utf8(&svg.bytes)
            .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
        let print_xml = layout.as_ref().map(|l| l.svg(xml, snapshot)).transpose()?;
        let xml = print_xml.as_deref().unwrap_or(xml);
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
            if layout.is_some() {
                return Err(ExportError::UnsupportedFeature(
                    "pdf.print_bleed_requires_vector_content",
                ));
            }
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
            bytes: registration::apply(bytes, snapshot, layout.as_ref())?,
            report,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_cid_cff_preserves_horizontal_and_vertical_outlines() {
        for bytes in [
            include_bytes!("../tests/fixtures/lp-japanese-cid-horizontal.pdf").as_slice(),
            include_bytes!("../tests/fixtures/lp-japanese-cid-vertical.pdf").as_slice(),
        ] {
            let options = ReadOptions {
                allow_lossy: true,
                raster_dpi: 72,
                ..Default::default()
            };
            let original = read(bytes, options).unwrap();
            let crate::io::ReadContent::Vector(original) = original.content else {
                panic!()
            };
            let mut pdf = lopdf::Document::load_mem(bytes).unwrap();
            let (id, program) = pdf
                .objects
                .iter()
                .find_map(|(id, o)| {
                    o.as_stream()
                        .ok()
                        .filter(|s| {
                            s.dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok())
                                == Some(b"OpenType")
                        })
                        .map(|s| (*id, s.get_plain_content().unwrap()))
                })
                .unwrap();
            let face = ttf_parser::Face::parse(&program, 0).unwrap();
            let cff = face
                .raw_face()
                .table(ttf_parser::Tag::from_bytes(b"CFF "))
                .unwrap()
                .to_vec();
            *pdf.get_object_mut(id).unwrap() = lopdf::Object::Stream(lopdf::Stream::new(
                lopdf::dictionary! {"Subtype"=>"CIDFontType0C"},
                cff,
            ));
            let mut encoded = vec![];
            pdf.save_to(&mut encoded).unwrap();
            let converted = read(&encoded, options).unwrap();
            assert!(!converted
                .report
                .issues
                .iter()
                .any(|i| i.code == "pdf.font_program" || i.code == "pdf.text_conversion"));
            let crate::io::ReadContent::Vector(converted) = converted.content else {
                panic!()
            };
            assert_eq!(
                original.svg_layers().next().unwrap().source,
                converted.svg_layers().next().unwrap().source
            );
        }
    }
    #[test]
    fn embedded_mixed_length_japanese_cmaps_match_identity_outlines() {
        for bytes in [
            include_bytes!("../tests/fixtures/lp-japanese-cid-horizontal.pdf").as_slice(),
            include_bytes!("../tests/fixtures/lp-japanese-cid-vertical.pdf").as_slice(),
        ] {
            let options = ReadOptions {
                allow_lossy: true,
                raster_dpi: 72,
                ..Default::default()
            };
            let original = read(bytes, options).unwrap();
            let crate::io::ReadContent::Vector(original) = original.content else {
                panic!()
            };
            let mut pdf = lopdf::Document::load_mem(bytes).unwrap();
            let font = pdf
                .objects
                .iter()
                .find_map(|(id, o)| {
                    o.as_dict()
                        .ok()
                        .filter(|d| {
                            d.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Type0")
                        })
                        .map(|_| *id)
                })
                .unwrap();
            let vertical = pdf
                .get_dictionary(font)
                .unwrap()
                .get(b"Encoding")
                .unwrap()
                .as_name()
                .unwrap()
                == b"Identity-V";
            let cmap=pdf.add_object(lopdf::Stream::new(lopdf::dictionary! {"WMode"=>i64::from(vertical)},b"/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> def 2 begincodespacerange <00> <7f> <8140> <9ffc> endcodespacerange 2 begincidchar <41> 42 <8140> 7 endcidchar".to_vec()));
            pdf.get_object_mut(font)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Encoding", cmap);
            let contents = pdf.get_page_content(*pdf.get_pages().values().next().unwrap());
            let changed = String::from_utf8(contents)
                .unwrap()
                .replace("<002A0007>", "<418140>");
            let page = *pdf.get_pages().values().next().unwrap();
            let stream = pdf.add_object(lopdf::Stream::new(
                lopdf::dictionary! {},
                changed.into_bytes(),
            ));
            pdf.get_object_mut(page)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Contents", stream);
            let mut encoded = vec![];
            pdf.save_to(&mut encoded).unwrap();
            let converted = read(&encoded, options).unwrap();
            let crate::io::ReadContent::Vector(converted) = converted.content else {
                panic!()
            };
            assert_eq!(
                original.svg_layers().next().unwrap().source,
                converted.svg_layers().next().unwrap().source
            );
        }
    }
    #[test]
    fn publication_preserves_page_order_dimensions_and_individual_dpi() {
        let mut publication = PdfPublication::default();
        for (width, height, dpi, color) in [
            (120, 80, 72, "red"),
            (240, 160, 144, "blue"),
            (60, 100, 72, "green"),
        ] {
            let mut doc = crate::svg::import("日本語ページ".into(), format!("<svg width=\"{width}\" height=\"{height}\"><rect width=\"{width}\" height=\"{height}\" fill=\"{color}\"/></svg>")).unwrap();
            let mut state = doc.document_state();
            state.resolution = Some(dpi);
            doc = lumapaint_core::document::Document::from_document_state(state).unwrap();
            publication
                .push(
                    PdfExporter
                        .export(&ExportSnapshot::capture(&doc), ExportOptions::default())
                        .unwrap(),
                )
                .unwrap();
        }
        let result = publication.finish(ExportOptions::default()).unwrap();
        let pages = inspect(&result.bytes).unwrap();
        assert_eq!(
            pages
                .iter()
                .map(|p| (p.width_points, p.height_points))
                .collect::<Vec<_>>(),
            [(120., 80.), (120., 80.), (60., 100.)]
        );
        // All-page decoding does not report discarded pages or require consent for that.
        let imported = read_all(
            &result.bytes,
            ReadOptions {
                raster_dpi: 72,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!imported
            .report
            .issues
            .iter()
            .any(|i| i.code == "pdf.selected_page_only"));
        let crate::io::ReadContent::Vector(document) = imported.content else {
            panic!()
        };
        assert_eq!(document.pages_snapshot().pages.len(), 3);
        let saved = crate::native::encode_state(&document.document_state()).unwrap();
        let restored = crate::native::decode(&saved).unwrap();
        let mut again = PdfPublication::default();
        for (index, (size, color)) in [
            ((120, 80), "#ff0000"),
            ((120, 80), "#0000ff"),
            ((60, 100), "#008000"),
        ]
        .into_iter()
        .enumerate()
        {
            let page = restored.detached_page(index).unwrap();
            assert_eq!(page.dimensions(), size);
            assert_eq!(page.document_state().resolution, Some(72));
            assert!(page.svg_layers().next().unwrap().source.contains(color));
            again
                .push(
                    PdfExporter
                        .export(&ExportSnapshot::capture(&page), ExportOptions::default())
                        .unwrap(),
                )
                .unwrap();
        }
        let again = again.finish(ExportOptions::default()).unwrap();
        assert_eq!(
            inspect(&again.bytes)
                .unwrap()
                .iter()
                .map(|p| (p.width_points, p.height_points))
                .collect::<Vec<_>>(),
            [(120., 80.), (120., 80.), (60., 100.)]
        );
        let subset = read_selected(
            &result.bytes,
            ReadOptions {
                allow_lossy: true,
                raster_dpi: 72,
                ..Default::default()
            },
            &[2, 0, 2],
        )
        .unwrap();
        let crate::io::ReadContent::Vector(subset) = subset.content else {
            panic!()
        };
        assert_eq!(subset.dimensions(), (120, 80));
        assert_eq!(subset.detached_page(1).unwrap().dimensions(), (60, 100));
        for index in 0..3 {
            let result = read(
                &result.bytes,
                ReadOptions {
                    page_index: index,
                    allow_lossy: true,
                    raster_dpi: 72,
                    ..Default::default()
                },
            )
            .unwrap();
            let crate::io::ReadContent::Vector(doc) = result.content else {
                panic!()
            };
            assert_eq!(doc.svg_layers().count(), 1);
        }
    }
    #[test]
    fn all_page_import_checks_later_pages_and_page_count_before_installation() {
        use lopdf::{dictionary, Object};
        fn fixture(count: usize, annotated: bool) -> Vec<u8> {
            let mut doc = lopdf::Document::with_version("1.7");
            let root = doc.new_object_id();
            let mut kids = Vec::new();
            for index in 0..count {
                let mut page = dictionary! { "Type" => "Page", "Parent" => root,
                "MediaBox" => vec![0.into(),0.into(),72.into(),144.into()] };
                if annotated && index + 1 == count {
                    page.set("Annots", Object::Array(vec![]));
                }
                kids.push(doc.add_object(page).into());
            }
            doc.objects.insert(
                root,
                Object::Dictionary(
                    dictionary! { "Type" => "Pages", "Count" => count as i64, "Kids" => kids },
                ),
            );
            let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => root });
            doc.trailer.set("Root", catalog);
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            bytes
        }
        let bytes = fixture(2, true);
        assert!(matches!(
            read_all(&bytes, ReadOptions::default()),
            Err(ImportError::LossyConversionRequiresConsent(_))
        ));
        let decoded = read_all(
            &bytes,
            ReadOptions {
                allow_lossy: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(decoded.report.issues.len(), 1);
        assert_eq!(decoded.report.issues[0].code, "pdf.annotations_omitted");
        assert!(matches!(
            read_all(&fixture(513, false), ReadOptions::default()),
            Err(ImportError::LimitExceeded("pdf.publication_pages"))
        ));
        let selected =
            read_selected(&fixture(3, false), ReadOptions::default(), &[2, 0, 2]).unwrap();
        let crate::io::ReadContent::Vector(selected) = selected.content else {
            panic!()
        };
        assert_eq!(selected.document_state().pages.unwrap().pages.len(), 2);
        assert!(read_selected(&bytes, ReadOptions::default(), &[]).is_err());
        assert!(read_selected(&bytes, ReadOptions::default(), &[2]).is_err());
        // Unselected annotations must not prevent an otherwise lossless import.
        assert!(read_selected(&bytes, ReadOptions::default(), &[0]).is_ok());
        let decoded = read_all(&fixture(1, false), ReadOptions::default()).unwrap();
        let crate::io::ReadContent::Vector(doc) = decoded.content else {
            panic!()
        };
        assert!(doc.document_state().pages.is_none());
    }
    #[test]
    fn publication_rejects_empty_wrong_format_and_multi_page_inputs_atomically() {
        assert!(PdfPublication::default()
            .finish(ExportOptions::default())
            .is_err());
        let mut writer = PdfPublication::default();
        assert!(writer
            .push(ExportedDocument {
                format: FormatId::Pdf,
                media_type: "application/pdf",
                bytes: two_pages(),
                report: ConversionReport::default()
            })
            .is_err());
        assert!(writer.pages.is_empty() && writer.document.objects.is_empty());
        assert!(writer
            .push(ExportedDocument {
                format: FormatId::Svg,
                media_type: "image/svg+xml",
                bytes: vec![],
                report: ConversionReport::default()
            })
            .is_err());
        assert!(writer.pages.is_empty());
    }
    fn two_pages() -> Vec<u8> {
        let document = crate::svg::import(
            "Pages".into(),
            "<svg width=\"120\" height=\"80\"><rect width=\"120\" height=\"80\"/></svg>".into(),
        )
        .unwrap();
        let exported = PdfExporter
            .export(
                &ExportSnapshot::capture(&document),
                ExportOptions::default(),
            )
            .unwrap();
        let mut doc = lopdf::Document::load_mem(&exported.bytes).unwrap();
        let first = *doc.get_pages().values().next().unwrap();
        let parent = doc
            .get_dictionary(first)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        let mut second = doc.get_dictionary(first).unwrap().clone();
        second.set("MediaBox", vec![0.into(), 0.into(), 60.into(), 40.into()]);
        let second = doc.add_object(second);
        let page = doc.get_dictionary_mut(first).unwrap();
        page.set("CropBox", vec![10.into(), 20.into(), 110.into(), 70.into()]);
        page.set("UserUnit", 2);
        page.set("Rotate", 90);
        let pages = doc.get_dictionary_mut(parent).unwrap();
        pages.set("Count", 2);
        pages.set("Kids", vec![first.into(), second.into()]);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }
    #[test]
    fn page_inspection_and_nonfirst_page_selection_preserve_page_geometry() {
        let bytes = two_pages();
        let pages = inspect(&bytes).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(
            (pages[0].width_points, pages[0].height_points),
            (100., 200.)
        );
        assert_eq!((pages[1].width_points, pages[1].height_points), (60., 40.));
        let crate::io::ReadContent::Vector(document) = read(
            &bytes,
            ReadOptions {
                page_index: 1,
                raster_dpi: 72,
                allow_lossy: true,
                ..Default::default()
            },
        )
        .unwrap()
        .content
        else {
            panic!()
        };
        assert_eq!(document.dimensions(), (60, 40));
        assert!(read(
            &bytes,
            ReadOptions {
                page_index: 2,
                allow_lossy: true,
                ..Default::default()
            }
        )
        .is_err());
        assert!(inspect(b"not PDF").is_err());
    }
    #[test]
    fn inspection_rejects_invalid_page_geometry_without_decoding_artwork() {
        let bytes = two_pages();
        let mut doc = lopdf::Document::load_mem(&bytes).unwrap();
        let first = *doc.get_pages().values().next().unwrap();
        doc.get_dictionary_mut(first)
            .unwrap()
            .set("CropBox", vec![10.into(), 0.into(), 0.into(), 10.into()]);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert!(inspect(&bytes).is_err());
    }
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
