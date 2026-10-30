use super::*;
use lumapaint_core::{
    document::{PageBookState, PageState, TextSettings},
    vector::TextStylePatch,
};
use std::io::Read;

fn text_document(content: &str) -> Document {
    let mut doc = Document::default();
    doc.set_text_object(TextSettings {
        id: None,
        text: VectorText {
            content: content.into(),
            font_size: 12.,
            box_width: 140.,
            box_height: Some(48.),
            ..Default::default()
        },
        position: [20., 30.],
        color: [32, 64, 128],
    })
    .unwrap();
    doc
}
fn exported(doc: &Document) -> ExportedDocument {
    IdmlExporter
        .export(
            &ExportSnapshot::capture(doc),
            ExportOptions { allow_lossy: true },
        )
        .unwrap()
}
fn files(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(zip.by_index(0).unwrap().name(), "mimetype");
    assert_eq!(
        zip.by_index(0).unwrap().compression(),
        zip::CompressionMethod::Stored
    );
    assert_eq!(&bytes[30..38], b"mimetype");
    assert_eq!(u16::from_le_bytes(bytes[28..30].try_into().unwrap()), 0);
    let mut files = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut source = String::new();
        entry.read_to_string(&mut source).unwrap();
        if entry.name() != "mimetype" {
            roxmltree::Document::parse(&source).unwrap();
        }
        assert!(files.insert(entry.name().into(), source).is_none());
    }
    assert_eq!(files["mimetype"], MIME);
    let design = roxmltree::Document::parse(&files["designmap.xml"]).unwrap();
    for reference in design.descendants().filter_map(|n| n.attribute("src")) {
        assert!(files.contains_key(reference), "missing {reference}");
    }
    let container = roxmltree::Document::parse(&files["META-INF/container.xml"]).unwrap();
    assert!(container
        .descendants()
        .any(|n| n.attribute("media-type") == Some("text/xml")));
    let preferences = roxmltree::Document::parse(&files["Resources/Preferences.xml"]).unwrap();
    assert!(preferences
        .descendants()
        .any(|n| n.attribute("PagesPerDocument") == Some("0")));
    let mut saw_layer = false;
    let mut saw_story = false;
    for node in design.root_element().children().filter(|n| n.is_element()) {
        match node.tag_name().name() {
            "Layer" => saw_layer = true,
            "Preferences" | "Styles" | "Fonts" | "Graphic" => assert!(!saw_layer),
            "Story" => saw_story = true,
            "Spread" => assert!(!saw_story),
            _ => {}
        }
    }
    for source in files.values() {
        if let Ok(xml) = roxmltree::Document::parse(source) {
            for story in xml
                .descendants()
                .filter(|n| n.has_tag_name("Story") && n.attribute("Self").is_some())
            {
                assert_eq!(story.attribute("UserText"), Some("true"));
            }
            for node in xml.descendants().filter(|n| {
                matches!(
                    n.tag_name().name(),
                    "Story" | "Spread" | "Page" | "TextFrame" | "Layer"
                )
            }) {
                let Some(uid) = node.attribute("Self") else {
                    continue;
                };
                assert!(u32::from_str_radix(uid.strip_prefix('u').unwrap(), 16).is_ok());
            }
        }
    }
    files
}
fn story_text(xml: &str) -> String {
    let doc = roxmltree::Document::parse(xml).unwrap();
    doc.descendants()
        .filter(|n| n.has_tag_name("Content") || n.has_tag_name("Br"))
        .map(|n| {
            if n.has_tag_name("Br") {
                "\n"
            } else {
                n.text().unwrap_or("")
            }
        })
        .collect()
}
#[test]
fn long_overset_unicode_text_styles_and_package_are_preserved_without_mutating_document() {
    let content = (0..400)
        .map(|i| format!("{i} 日本語 English 简体中文 😀 & <tag>\t"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut doc = text_document(&content);
    let mut settings = doc.snapshot().text_objects[0].clone();
    settings
        .text
        .apply_style(
            2,
            5,
            &TextStylePatch {
                bold: Some(true),
                font_size: Some(18.),
                ..Default::default()
            },
            settings.color,
        )
        .unwrap();
    doc.set_text_object(TextSettings {
        id: Some(settings.id),
        text: settings.text,
        position: settings.position,
        color: settings.color,
    })
    .unwrap();
    let before = crate::native::encode_state(&doc.document_state()).unwrap();
    let snapshot = ExportSnapshot::capture(&doc);
    assert!(matches!(
        IdmlExporter.export(&snapshot, ExportOptions::default()),
        Err(ExportError::LossyConversionRequiresConsent(_))
    ));
    let result = exported(&doc);
    assert_eq!(result.format, FormatId::Idml);
    assert!(result
        .report
        .issues
        .iter()
        .any(|i| i.code == "idml.text_recomposed"));
    let files = files(&result.bytes);
    assert_eq!(story_text(&files["Stories/lp_story_1.xml"]), content);
    let story = roxmltree::Document::parse(&files["Stories/lp_story_1.xml"]).unwrap();
    assert!(story
        .descendants()
        .any(|n| n.attribute("PointSize") == Some("18")));
    let spread = roxmltree::Document::parse(&files["Spreads/lp_spread_0.xml"]).unwrap();
    let frame = spread
        .descendants()
        .find(|n| n.has_tag_name("TextFrame"))
        .unwrap();
    assert_eq!(frame.attribute("ParentStory"), Some("u30000001"));
    assert_eq!(frame.attribute("ItemTransform"), Some("1 0 0 1 20 30"));
    assert_eq!(
        before,
        crate::native::encode_state(&doc.document_state()).unwrap()
    );
}
#[test]
fn linked_frames_share_one_story_without_inserting_a_boundary_paragraph() {
    let mut doc = text_document("AB😀");
    doc.set_text_object(TextSettings {
        id: None,
        text: VectorText {
            content: "中文\nEnd".into(),
            box_width: 160.,
            box_height: Some(60.),
            ..Default::default()
        },
        position: [200., 30.],
        color: [128, 32, 64],
    })
    .unwrap();
    let ids: Vec<_> = doc
        .snapshot()
        .text_objects
        .iter()
        .map(|o| o.id.clone())
        .collect();
    let mut state = doc.document_state();
    state.vector_assets.threads.push(
        serde_json::from_value(serde_json::json!({
            "id":"lp-thread", "name":"本文", "frames":ids
        }))
        .unwrap(),
    );
    let doc = Document::from_document_state(state).unwrap();
    let files = files(&exported(&doc).bytes);
    assert_eq!(
        files.keys().filter(|p| p.starts_with("Stories/")).count(),
        1
    );
    assert_eq!(
        story_text(&files["Stories/lp_story_1.xml"]),
        "AB😀中文\nEnd"
    );
    let spread = roxmltree::Document::parse(&files["Spreads/lp_spread_0.xml"]).unwrap();
    let frames: Vec<_> = spread
        .descendants()
        .filter(|n| n.has_tag_name("TextFrame"))
        .collect();
    assert_eq!(frames.len(), 2);
    assert_eq!(
        frames[0].attribute("NextTextFrame"),
        frames[1].attribute("Self")
    );
    assert_eq!(
        frames[1].attribute("PreviousTextFrame"),
        frames[0].attribute("Self")
    );
    assert_eq!(
        frames[0].attribute("ParentStory"),
        frames[1].attribute("ParentStory")
    );
    assert_eq!(frames[0].attribute("PreviousTextFrame"), Some("n"));
    assert_eq!(frames[1].attribute("NextTextFrame"), Some("n"));
}
#[test]
fn pages_export_in_publication_order_with_independent_resolution_and_unique_ids() {
    let mut first = text_document("First").document_state();
    first.resolution = Some(144);
    let mut second = text_document("Second").document_state();
    second.width = 400;
    second.height = 600;
    second.resolution = Some(72);
    second.pages = Some(PageBookState {
        text_threads: Vec::new(),
        facing: true,
        binding: PageBinding::RightToLeft,
        active: 1,
        next_id: 3,
        pages: vec![
            PageState {
                id: "page-1".into(),
                content: Some(Box::new(first)),
            },
            PageState {
                id: "page-2".into(),
                content: None,
            },
        ],
    });
    let doc = Document::from_document_state(second).unwrap();
    let result = exported(&doc);
    assert!(result
        .report
        .issues
        .iter()
        .any(|i| i.code == "idml.facing_pages_separated"));
    let files = files(&result.bytes);
    assert_eq!(story_text(&files["Stories/lp_story_1.xml"]), "First");
    assert_eq!(story_text(&files["Stories/lp_story_2.xml"]), "Second");
    let a = roxmltree::Document::parse(&files["Spreads/lp_spread_0.xml"]).unwrap();
    let b = roxmltree::Document::parse(&files["Spreads/lp_spread_1.xml"]).unwrap();
    assert_eq!(
        a.descendants()
            .find(|n| n.has_tag_name("TextFrame"))
            .unwrap()
            .attribute("ItemTransform"),
        Some("1 0 0 1 10 15")
    );
    assert_eq!(
        b.descendants()
            .find(|n| n.has_tag_name("Page"))
            .unwrap()
            .attribute("GeometricBounds"),
        Some("0 0 600 400")
    );
    let mut ids = BTreeSet::new();
    for (path, source) in &files {
        if path.ends_with(".xml") {
            let xml = roxmltree::Document::parse(source).unwrap();
            for id in xml.descendants().filter_map(|n| n.attribute("Self")) {
                assert!(ids.insert(id.to_owned()), "duplicate {id}");
            }
        }
    }
}
#[test]
fn vertical_text_frame_rotation_scales_visibility_and_character_settings_are_explicit() {
    let mut doc = text_document("日12文");
    let mut snapshot = doc.snapshot().text_objects[0].clone();
    snapshot.text.writing_mode = WritingMode::Vertical;
    snapshot.text.rotation = 90.;
    snapshot.text.scale_x = 1.5;
    snapshot.text.scale_y = 0.75;
    snapshot.text.tate_chu_yoko = true;
    snapshot.text.tracking = 25.;
    snapshot.text.underline = true;
    doc.set_text_object(TextSettings {
        id: Some(snapshot.id),
        text: snapshot.text,
        position: snapshot.position,
        color: snapshot.color,
    })
    .unwrap();
    let mut state = doc.document_state();
    state.svg_layers[0].visible = false;
    state.svg_layers[0].locked = true;
    let object = &mut state.svg_layers[0].vector_objects[0];
    object.opacity = 0.5;
    let doc = Document::from_document_state(state).unwrap();
    let files = files(&exported(&doc).bytes);
    let story = roxmltree::Document::parse(&files["Stories/lp_story_1.xml"]).unwrap();
    assert!(story
        .descendants()
        .any(|n| n.attribute("StoryOrientation") == Some("Vertical")));
    assert!(story.descendants().any(
        |n| n.attribute("Tatechuyoko") == Some("true") && n.attribute("Tracking") == Some("25")
    ));
    let spread = roxmltree::Document::parse(&files["Spreads/lp_spread_0.xml"]).unwrap();
    assert!(spread
        .descendants()
        .any(|n| n.attribute("Opacity") == Some("50")));
    let design = roxmltree::Document::parse(&files["designmap.xml"]).unwrap();
    assert!(design.descendants().any(|n| n.has_tag_name("Layer")
        && n.attribute("Visible") == Some("false")
        && n.attribute("Locked") == Some("true")));
}
#[test]
fn unparsed_svg_is_rejected_even_with_loss_consent_and_registry_distinguishes_open_from_layer_import(
) {
    let doc = crate::svg::import(
        "Shape".into(),
        "<svg width='20' height='20'><rect width='10' height='10'/></svg>".into(),
    )
    .unwrap();
    assert!(matches!(
        IdmlExporter.export(
            &ExportSnapshot::capture(&doc),
            ExportOptions { allow_lossy: true }
        ),
        Err(ExportError::UnsupportedFeature("idml.native_text_only"))
    ));
    let entry = crate::io::FILE_FORMATS
        .iter()
        .find(|f| f.format == FormatId::Idml)
        .unwrap();
    assert!(entry.export_adapter && entry.open_adapter && !entry.import_adapter);
    assert_eq!(
        crate::io::format_from_extension("IDML"),
        Some(FormatId::Idml)
    );
}

#[test]
fn excessive_physical_page_size_is_rejected_before_archive_output() {
    let mut state = text_document("Large physical page").document_state();
    state.resolution = Some(1);
    let doc = Document::from_document_state(state).unwrap();
    assert!(matches!(
        IdmlExporter.export(
            &ExportSnapshot::capture(&doc),
            ExportOptions { allow_lossy: true }
        ),
        Err(ExportError::UnsupportedFeature("idml.publication_limit"))
    ));
}

fn read_document(bytes: &[u8]) -> Document {
    let loaded = read::read_with_layout(
        bytes,
        crate::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        },
        Some(|t, _| t.reflow_soft_breaks_with(|s, _| Ok(s.chars().count() as f32 * 6.))),
    )
    .unwrap();
    let crate::io::ReadContent::Vector(doc) = loaded.content else {
        panic!()
    };
    *doc
}
#[test]
fn editable_idml_read_preserves_unicode_runs_overset_threads_and_native_save() {
    let mut doc = text_document(
        &(0..400)
            .map(|i| format!("{i} 日本語 English 中文 😀"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    doc.set_text_object(TextSettings {
        id: None,
        text: VectorText {
            box_width: 140.,
            box_height: Some(48.),
            ..Default::default()
        },
        position: [200., 30.],
        color: [0; 3],
    })
    .unwrap();
    let mut state = doc.document_state();
    let ids: Vec<_> = state
        .svg_layers
        .iter()
        .flat_map(|l| &l.vector_objects)
        .map(|o| o.id.clone())
        .collect();
    state.vector_assets.threads.push(
        serde_json::from_value(
            serde_json::json!({"id":"thread-source","name":"Story","frames":ids}),
        )
        .unwrap(),
    );
    let doc = Document::from_document_state(state).unwrap();
    let bytes = exported(&doc).bytes;
    assert!(matches!(
        read::read(&bytes, Default::default()),
        Err(crate::ImportError::LossyConversionRequiresConsent(_))
    ));
    let result = read_document(&bytes);
    let out = exported(&result);
    let f = files(&out.bytes);
    let source = files(&bytes);
    assert_eq!(
        story_text(&f["Stories/lp_story_1.xml"]),
        story_text(&source["Stories/lp_story_1.xml"])
    );
    assert_eq!(result.document_state().vector_assets.threads.len(), 1);
    let saved = crate::native::encode_state(&result.document_state()).unwrap();
    assert!(!saved.is_empty());
    assert!(result
        .snapshot()
        .text_objects
        .iter()
        .all(|t| !t.text.content.is_empty()));
}
fn shape(id: &str) -> VectorObject {
    serde_json::from_value(serde_json::json!({"id":id,"name":"曲線 / Curve / 曲线","path":{"data":"M10 20 C30 0 60 70 80 20 L80 90 Z","fillRule":"nonZero"},"transform":[1,0.2,0.1,1,12,8],"fill":{"color":[200,40,80,255]},"stroke":{"color":[10,20,30,255]},"strokeWidth":2,"visible":true})).unwrap()
}
#[test]
fn text_curves_and_embedded_png_round_trip_as_native_objects_in_order() {
    use base64::Engine;
    let mut state = text_document("Editable mixed 日本語 中文").document_state();
    state.svg_layers[0].vector_objects.push(shape("shape"));
    let mut image = shape("image");
    image.stroke = None;
    image.fill = None;
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 2, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 0, 0, 255, 0, 0, 255, 255])
            .unwrap();
    }
    image.image_frame = Some(lumapaint_core::image_frame::ImageFrame {
        fitting: Default::default(),
        content_transform: Some([40., 0., 0., 70., 10., 20.]),
        image: Some(lumapaint_core::image_frame::FrameImage {
            source_path: None,
            fingerprint: "fixture".into(),
            name: "Image".into(),
            width: 2,
            height: 1,
            encoded_width: 2,
            encoded_height: 1,
            orientation_transform: [1., 0., 0., 1., 0., 0.],
            data_uri: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&png)
            ),
        }),
    });
    state.svg_layers[0].vector_objects.push(image);
    for l in &mut state.svg_layers {
        l.source =
            lumapaint_core::document::vector_svg(state.width, state.height, &l.vector_objects);
    }
    let doc = Document::from_document_state(state).unwrap();
    let bytes = exported(&doc).bytes;
    let result = read_document(&bytes);
    let state = result.document_state();
    let objects = &state.svg_layers[0].vector_objects;
    assert_eq!(objects.len(), 3);
    assert!(objects[0].text.is_some());
    assert!(objects[1].text.is_none());
    assert_eq!(objects[1].fill.unwrap().color, [200, 40, 80, 255]);
    assert_eq!(objects[1].transform, [1., 0.2, 0.1, 1., 12., 8.]);
    assert_eq!(objects[1].stroke_width, 2.);
    let i = objects[2].image_frame.as_ref().unwrap();
    assert_eq!(i.content_transform, Some([40., 0., 0., 70., 10., 20.]));
    assert_eq!(i.image.as_ref().unwrap().width, 2);
    assert_eq!(exported(&result).format, FormatId::Idml);
}
fn rewrite_archive(source: &[u8], change: impl Fn(&str, &str) -> String) -> Vec<u8> {
    let mut source_zip = zip::ZipArchive::new(Cursor::new(source)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source_zip.len() {
        let mut entry = source_zip.by_index(i).unwrap();
        let name = entry.name().to_owned();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        zip.start_file(
            &name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(change(&name, &value).as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
#[test]
fn compressed_adobe_style_package_is_supported_and_invalid_links_or_zip_paths_are_rejected() {
    let bytes = exported(&text_document("Compressed 日本語")).bytes;
    let zipped = rewrite_archive(&bytes, |_, s| s.into());
    assert_eq!(
        read_document(&zipped).snapshot().text_objects[0]
            .text
            .content,
        "Compressed 日本語"
    );
    let broken = rewrite_archive(&bytes, |name, s| {
        if name.starts_with("Spreads/") {
            s.replace("NextTextFrame=\"n\"", "NextTextFrame=\"missing\"")
        } else {
            s.into()
        }
    });
    assert!(read::read(
        &broken,
        crate::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        }
    )
    .is_err());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("../escape.xml", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"x").unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(read::read(&bytes, Default::default()).is_err());
}

#[test]
fn indesign_2026_reexport_retains_editable_text_curve_and_embedded_image() {
    let bytes = include_bytes!("../../tests/fixtures/lp-adobe-idml-mixed.idml");
    let doc = read_document(bytes);
    let state = doc.document_state();
    let objects: Vec<_> = state
        .svg_layers
        .iter()
        .flat_map(|l| &l.vector_objects)
        .collect();
    assert_eq!(objects.len(), 3);
    assert_eq!(
        objects[0].text.as_ref().unwrap().content,
        "Editable 日本語 / English / 简体中文"
    );
    assert_eq!(objects[1].fill.unwrap().color, [220, 80, 110, 255]);
    assert_eq!(objects[1].stroke_width, 4.);
    let image = objects[2]
        .image_frame
        .as_ref()
        .unwrap()
        .image
        .as_ref()
        .unwrap();
    assert_eq!((image.width, image.height), (64, 64));
    let reexport = exported(&doc);
    let restored = read_document(&reexport.bytes);
    assert_eq!(
        restored.snapshot().text_objects[0].text.content,
        objects[0].text.as_ref().unwrap().content
    );
}

#[test]
fn facing_spread_pages_use_page_transforms_without_duplicating_or_dropping_items() {
    let mut first = text_document("Left").document_state();
    first.width = 400;
    first.height = 300;
    first.resolution = Some(72);
    let mut second = text_document("Right").document_state();
    second.width = 400;
    second.height = 300;
    second.resolution = Some(72);
    first.pages = Some(PageBookState {
        text_threads: Vec::new(),
        facing: false,
        binding: PageBinding::LeftToRight,
        active: 0,
        next_id: 3,
        pages: vec![
            PageState {
                id: "page-1".into(),
                content: None,
            },
            PageState {
                id: "page-2".into(),
                content: Some(Box::new(second)),
            },
        ],
    });
    let bytes = exported(&Document::from_document_state(first).unwrap()).bytes;
    let f = files(&bytes);
    let b = &f["Spreads/lp_spread_1.xml"];
    let start = b.find("<Page ").unwrap();
    let end = b.rfind("</Spread>").unwrap();
    let body=b[start..end].replace("Self=\"u50000001\" Name=\"2\" GeometricBounds=\"0 0 300 400\" ItemTransform=\"1 0 0 1 0 0\"","Self=\"u50000001\" Name=\"2\" GeometricBounds=\"0 0 300 400\" ItemTransform=\"1 0 0 1 400 0\"").replace("ItemTransform=\"1 0 0 1 20 30\"","ItemTransform=\"1 0 0 1 420 30\"");
    let bytes = rewrite_archive(&bytes, |name, s| match name {
        "designmap.xml" => s.replace("<idPkg:Spread src=\"Spreads/lp_spread_1.xml\"/>", ""),
        "Spreads/lp_spread_0.xml" => s
            .replace("PageCount=\"1\"", "PageCount=\"2\"")
            .replace("</Spread>", &format!("{body}</Spread>")),
        "Resources/Preferences.xml" => s.replace("FacingPages=\"false\"", "FacingPages=\"true\""),
        _ => s.into(),
    });
    let doc = read_document(&bytes);
    assert_eq!(doc.pages_snapshot().pages.len(), 2);
    assert!(doc.pages_snapshot().facing);
    for (index, name) in ["Left", "Right"].iter().enumerate() {
        let page = doc.page_document(index).unwrap();
        let text = &page.snapshot().text_objects[0];
        assert_eq!(&text.text.content, name);
        assert_eq!(text.position, [20., 30.]);
    }
}
#[test]
fn external_links_and_corrupt_embedded_images_never_silently_drop_content() {
    let bytes = include_bytes!("../../tests/fixtures/lp-adobe-idml-mixed.idml");
    // Use Adobe's embedded image node, replacing its payload with an external URI.
    let external = rewrite_archive(bytes, |name, s| {
        if name.starts_with("Spreads/") && s.contains("<Contents>") {
            let a = s.find("<Contents>").unwrap();
            let b = s[a..].find("</Contents>").unwrap() + a + "</Contents>".len();
            let s = format!("{}{}", &s[..a], &s[b..]);
            let at = s.find("</Image>").unwrap();
            format!("{}<Link Self=\"external-link\" LinkResourceURI=\"file:/private/user-image.png\"/>{}",&s[..at],&s[at..])
        } else {
            s.into()
        }
    });
    assert!(matches!(
        read::read(
            &external,
            crate::io::ReadOptions {
                allow_lossy: true,
                ..Default::default()
            }
        ),
        Err(crate::ImportError::Unsupported("idml.external_image"))
    ));
    let corrupt = rewrite_archive(bytes, |name, s| {
        if name.starts_with("Spreads/") && s.contains("<Contents>") {
            let a = s.find("<Contents>").unwrap() + "<Contents>".len();
            let b = s[a..].find("</Contents>").unwrap() + a;
            format!("{}AAAA{}", &s[..a], &s[b..])
        } else {
            s.into()
        }
    });
    assert!(read::read(
        &corrupt,
        crate::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn jpeg_image_payloads_are_decoded_and_truncation_is_rejected() {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode(
            &[255, 0, 0, 0, 0, 255],
            2,
            1,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    assert_eq!(read::image_info(&bytes).unwrap(), (2, 1, "JPEG"));
    assert!(read::image_info(&bytes[..20]).is_err());
}

fn cross_page_document() -> Document {
    use lumapaint_core::document::{PageTextFrame, PageTextThread};
    let mut first = text_document(
        &(0..120)
            .map(|i| format!("{i} 日本語 English 中文 😀"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .document_state();
    first.resolution = Some(72);
    let mut second = text_document("").document_state();
    second.resolution = Some(72);
    let a = first.svg_layers[0].vector_objects[0].id.clone();
    let b = second.svg_layers[0].vector_objects[0].id.clone();
    first.pages = Some(PageBookState {
        text_threads: vec![PageTextThread {
            id: "book-story".into(),
            name: "本文".into(),
            frames: vec![
                PageTextFrame {
                    page_id: "page-1".into(),
                    object_id: a,
                },
                PageTextFrame {
                    page_id: "page-2".into(),
                    object_id: b,
                },
            ],
        }],
        facing: false,
        binding: PageBinding::LeftToRight,
        active: 0,
        next_id: 3,
        pages: vec![
            PageState {
                id: "page-1".into(),
                content: None,
            },
            PageState {
                id: "page-2".into(),
                content: Some(Box::new(second)),
            },
        ],
    });
    Document::from_document_state(first).unwrap()
}
fn whole_story(doc: &Document) -> String {
    let state = doc.document_state();
    let thread = &state.pages.as_ref().unwrap().text_threads[0];
    thread
        .frames
        .iter()
        .map(|f| {
            let i = state
                .pages
                .as_ref()
                .unwrap()
                .pages
                .iter()
                .position(|p| p.id == f.page_id)
                .unwrap();
            doc.page_document(i)
                .unwrap()
                .svg_layers()
                .flat_map(|l| &l.vector_objects)
                .find(|o| o.id == f.object_id)
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .content
                .clone()
        })
        .collect()
}
fn layout(t: &mut VectorText, _: [u8; 3]) -> Result<(), String> {
    t.reflow_soft_breaks_with(|s, _| Ok(s.chars().count() as f32 * 6.))
}
fn select_page(doc: &mut Document, i: usize) {
    doc.edit_pages(lumapaint_core::document::PageEdit {
        action: "select".into(),
        index: Some(i),
        facing: None,
        binding: None,
    })
    .unwrap();
}
#[test]
fn cross_page_story_reflows_and_survives_native_and_idml_roundtrips() {
    let original = cross_page_document();
    let source = whole_story(&original);
    let bytes = exported(&original).bytes;
    let f = files(&bytes);
    assert_eq!(f.keys().filter(|n| n.starts_with("Stories/")).count(), 1);
    let mut imported = read_document(&bytes);
    assert_eq!(whole_story(&imported), source);
    assert!(
        imported.page_document(1).unwrap().snapshot().text_objects[0]
            .text
            .content
            .len()
            > 100
    );
    assert!(
        !imported.page_document(0).unwrap().snapshot().text_objects[0]
            .text
            .content
            .is_empty()
    );
    let saved = crate::native::encode_state(&imported.document_state()).unwrap();
    let native = crate::native::decode(&saved).unwrap();
    assert_eq!(whole_story(&native), source);
    select_page(&mut imported, 1);
    let again = read_document(&exported(&imported).bytes);
    assert_eq!(whole_story(&again), source);
    let mut invalid = native.document_state();
    invalid.pages.as_mut().unwrap().text_threads[0].frames[1].object_id = "missing".into();
    assert!(Document::from_document_state(invalid).is_err());
}
#[test]
fn editing_cross_page_story_and_undo_from_either_page_preserve_unrelated_edits() {
    let mut doc = read_document(&exported(&cross_page_document()).bytes);
    doc.set_text_layout_backend(Some(layout));
    let original = whole_story(&doc);
    select_page(&mut doc, 1);
    doc.import_svg(
        "Decoration".into(),
        "<svg width='32' height='32'><rect width='32' height='32' fill='blue'/></svg>".into(),
    )
    .unwrap();
    let decoration = doc
        .svg_layers()
        .find(|l| l.name == "Decoration")
        .unwrap()
        .source
        .clone();
    select_page(&mut doc, 0);

    let mut text = doc.snapshot().text_objects[0].clone();
    text.text.content.insert_str(0, "追加 / Added / 新增\n");
    text.text.runs.clear();
    doc.set_text_object(TextSettings {
        id: Some(text.id),
        text: text.text,
        position: text.position,
        color: text.color,
    })
    .unwrap();
    let edited = whole_story(&doc);
    assert!(edited.starts_with("追加"));
    select_page(&mut doc, 1);
    doc.undo();
    assert_eq!(whole_story(&doc), original);
    doc.redo();
    assert_eq!(whole_story(&doc), edited);
    // A non-story edit on one page must not resurrect a globally undone story.
    doc.set_text_object(TextSettings {
        id: None,
        text: VectorText {
            content: "Independent".into(),
            ..Default::default()
        },
        position: [50., 70.],
        color: [0; 3],
    })
    .unwrap();
    select_page(&mut doc, 0);
    doc.undo();
    assert_eq!(whole_story(&doc), original);
    select_page(&mut doc, 1);
    doc.undo();
    assert_eq!(whole_story(&doc), original);
    assert_eq!(doc.snapshot().text_objects.len(), 1);
    assert_eq!(
        doc.svg_layers()
            .find(|l| l.name == "Decoration")
            .unwrap()
            .source,
        decoration
    );

    doc.redo();
    assert_eq!(whole_story(&doc), original);
    doc.redo();
    assert_eq!(whole_story(&doc), edited);
}
#[test]
fn external_image_resolver_is_cached_validated_and_preserves_native_link_path() {
    use base64::Engine;
    let bytes = include_bytes!("../../tests/fixtures/lp-adobe-idml-mixed.idml");
    let mut png = Vec::new();
    let external = rewrite_archive(bytes, |name, s| {
        if name.starts_with("Spreads/") && s.contains("<Contents>") {
            let a = s.find("<Contents>").unwrap();
            let b = s[a..].find("</Contents>").unwrap() + a;
            let s = format!("{}{}", &s[..a], &s[b + "</Contents>".len()..]);
            let at = s.find("</Image>").unwrap();
            format!(
                "{}<Link Self=\"link-test\" LinkResourceURI=\"file:Links/checker.png\"/>{}",
                &s[..at],
                &s[at..]
            )
        } else {
            s.into()
        }
    });
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        if e.name().starts_with("Spreads/") {
            let mut s = String::new();
            e.read_to_string(&mut s).unwrap();
            let r = roxmltree::Document::parse(&s).unwrap();
            if let Some(n) = r.descendants().find(|n| n.has_tag_name("Contents")) {
                let encoded: String = n
                    .text()
                    .unwrap()
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                png = base64::engine::general_purpose::STANDARD
                    .decode(encoded)
                    .unwrap();
            }
        }
    }
    let external = rewrite_archive(&external, |name, s| {
        if !name.starts_with("Spreads/") {
            return s.into();
        }
        let parsed = roxmltree::Document::parse(s).unwrap();
        let Some(node) = parsed
            .descendants()
            .find(|n| n.children().any(|c| c.has_tag_name("Image")))
        else {
            return s.into();
        };
        let mut copy = s[node.range()].to_owned();
        for n in node.descendants() {
            if let Some(id) = n.attribute("Self") {
                copy = copy.replace(&format!("Self=\"{id}\""), &format!("Self=\"{id}-copy\""));
            }
        }
        s.replace("</Spread>", &format!("{copy}</Spread>"))
    });
    let mut calls = 0;
    let imported = read::read_with_links(
        &external,
        crate::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        },
        None,
        &mut |uri| {
            calls += 1;
            assert_eq!(uri, "file:Links/checker.png");
            Ok(read::ResolvedImage {
                bytes: png.clone(),
                source_path: Some("/tmp/Links/checker.png".into()),
            })
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
    let crate::io::ReadContent::Vector(doc) = imported.content else {
        panic!()
    };
    let state = doc.document_state();
    let image = state
        .svg_layers
        .iter()
        .flat_map(|l| &l.vector_objects)
        .find_map(|o| o.image_frame.as_ref()?.image.as_ref())
        .unwrap();
    assert_eq!(image.source_path.as_deref(), Some("/tmp/Links/checker.png"));
    assert!(image.data_uri.starts_with("data:image/png;base64,"));
    assert!(read::read_with_links(
        &external,
        crate::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        },
        None,
        &mut |_| Ok(read::ResolvedImage {
            bytes: b"not an image".to_vec(),
            source_path: None
        })
    )
    .is_err());
}

#[test]
fn indesign_2026_cross_page_story_and_external_image_round_trip_portably() {
    let bytes = include_bytes!("../../tests/fixtures/lp-adobe-idml-cross-page.idml");
    let loaded = read::read_with_links(
        bytes,
        crate::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        },
        Some(layout),
        &mut |uri| {
            assert!(uri.ends_with("/checker.png"));
            Ok(read::ResolvedImage {
                bytes: include_bytes!("../../tests/fixtures/lp-idml-checker.png").to_vec(),
                source_path: None,
            })
        },
    )
    .unwrap();
    assert!(loaded
        .report
        .issues
        .iter()
        .any(|i| i.code == "idml.external_images_resolved"));
    let crate::io::ReadContent::Vector(doc) = loaded.content else {
        panic!()
    };
    let expected = (0..160)
        .map(|i| format!("{i:03} 日本語 English 简体中文 — 跨ページの本文です。"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(whole_story(&doc), expected);
    let portable = exported(&doc);
    let again = read_document(&portable.bytes);
    assert_eq!(whole_story(&again), expected);
    let native =
        crate::native::decode(&crate::native::encode_state(&again.document_state()).unwrap())
            .unwrap();
    assert_eq!(whole_story(&native), expected);
}
#[test]
fn a_text_frame_without_self_is_rejected_without_panicking() {
    let source = exported(&text_document("Malformed identity")).bytes;
    let bytes = rewrite_archive(&source, |name, s| {
        if name.starts_with("Spreads/") {
            s.replace("<TextFrame Self=\"u20000001\"", "<TextFrame")
        } else {
            s.into()
        }
    });
    assert!(read::read(
        &bytes,
        crate::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        }
    )
    .is_err());
}
#[test]
fn cross_page_edits_are_atomic_when_layout_fails_or_another_frame_is_locked() {
    let mut doc = read_document(&exported(&cross_page_document()).bytes);
    let before = crate::native::encode_state(&doc.document_state()).unwrap();
    doc.set_text_layout_backend(Some(|_, _| Err("Layout failed".into())));
    let mut text = doc.snapshot().text_objects[0].clone();
    text.text.content.insert_str(0, "Added ");
    text.text.runs.clear();
    assert!(doc
        .set_text_object(TextSettings {
            id: Some(text.id.clone()),
            text: text.text.clone(),
            position: text.position,
            color: text.color
        })
        .is_err());
    assert_eq!(
        before,
        crate::native::encode_state(&doc.document_state()).unwrap()
    );
    let mut state = doc.document_state();
    let book = state.pages.as_mut().unwrap();
    let id = book.text_threads[0].frames[1].object_id.clone();
    book.pages[1]
        .content
        .as_mut()
        .unwrap()
        .locked_objects
        .insert(id);
    let mut doc = Document::from_document_state(state).unwrap();
    doc.set_text_layout_backend(Some(layout));
    let before = crate::native::encode_state(&doc.document_state()).unwrap();
    assert!(doc
        .set_text_object(TextSettings {
            id: Some(text.id),
            text: text.text,
            position: text.position,
            color: text.color
        })
        .is_err());
    assert_eq!(
        before,
        crate::native::encode_state(&doc.document_state()).unwrap()
    );
}

#[test]
fn resizing_cross_page_frame_reflows_the_next_page_and_can_be_undone_there() {
    let mut doc = read_document(&exported(&cross_page_document()).bytes);
    doc.set_text_layout_backend(Some(layout));
    let original = whole_story(&doc);
    let original_count = doc.snapshot().text_objects[0].text.content.chars().count();
    let mut t = doc.snapshot().text_objects[0].clone();
    t.text.box_height = Some(240.);
    doc.set_text_object(TextSettings {
        id: Some(t.id),
        text: t.text,
        position: t.position,
        color: t.color,
    })
    .unwrap();
    assert_eq!(whole_story(&doc), original);
    assert!(doc.snapshot().text_objects[0].text.content.chars().count() > original_count);
    select_page(&mut doc, 1);
    doc.undo();
    assert_eq!(whole_story(&doc), original);
    assert_eq!(
        doc.page_document(0).unwrap().snapshot().text_objects[0]
            .text
            .box_height,
        Some(48.)
    );
    doc.redo();
    assert_eq!(
        doc.page_document(0).unwrap().snapshot().text_objects[0]
            .text
            .box_height,
        Some(240.)
    );
}
#[test]
fn unsupported_destructive_frame_operations_leave_publication_valid() {
    let mut doc = read_document(&exported(&cross_page_document()).bytes);
    let id = doc.snapshot().text_objects[0].id.clone();
    doc.select_vector_objects(vec![id]).unwrap();
    let before = crate::native::encode_state(&doc.document_state()).unwrap();
    assert!(doc.delete_selected_vector_objects().is_err());
    assert!(doc.set_image_resolution(144).is_err());
    assert!(doc
        .set_text_writing_mode(WritingMode::Vertical, layout)
        .is_err());
    assert!(doc
        .edit_pages(lumapaint_core::document::PageEdit {
            action: "delete".into(),
            index: Some(1),
            facing: None,
            binding: None
        })
        .is_err());
    assert_eq!(
        before,
        crate::native::encode_state(&doc.document_state()).unwrap()
    );
}

#[test]
fn cross_page_history_has_a_bounded_shared_undo_window() {
    let mut doc = read_document(&exported(&cross_page_document()).bytes);
    doc.set_text_layout_backend(Some(layout));
    for i in 0..40 {
        let mut t = doc.snapshot().text_objects[0].clone();
        if i > 0 {
            let end = t.text.content.find('\n').unwrap() + 1;
            t.text.content.replace_range(..end, "");
        }
        t.text.content.insert_str(0, &format!("Edit {i}\n"));
        t.text.runs.clear();
        doc.set_text_object(TextSettings {
            id: Some(t.id),
            text: t.text,
            position: t.position,
            color: t.color,
        })
        .unwrap();
    }
    select_page(&mut doc, 1);
    for _ in 0..40 {
        doc.undo();
    }
    assert!(whole_story(&doc).starts_with("Edit 7\n"));
}

fn vertical_layout(t: &mut VectorText, _: [u8; 3]) -> Result<(), String> {
    if t.writing_mode == WritingMode::Vertical {
        t.reflow_vertical();
        t.validate()
    } else {
        layout(t, [0; 3])
    }
}
fn vertical_publication() -> Document {
    let mut state = cross_page_document().document_state();
    for i in 0..2 {
        let page = if i == 0 {
            &mut state
        } else {
            state.pages.as_mut().unwrap().pages[i]
                .content
                .as_deref_mut()
                .unwrap()
        };
        for layer in &mut page.svg_layers {
            for object in &mut layer.vector_objects {
                if let Some(t) = &mut object.text {
                    t.writing_mode = WritingMode::Vertical;
                    t.box_width = 48.;
                    t.box_height = Some(84.);
                    t.clear_measured_layout();
                }
            }
            layer.source = lumapaint_core::document::vector_svg(
                page.width,
                page.height,
                &layer.vector_objects,
            );
        }
    }
    let mut doc = Document::from_document_state(state).unwrap();
    doc.recompose_imported_publication_text(vertical_layout)
        .unwrap();
    doc.set_text_layout_backend(Some(vertical_layout));
    doc
}
#[test]
fn vertical_cross_page_story_uses_column_width_and_preserves_unicode_styles_and_overset() {
    let mut doc = vertical_publication();
    let original = whole_story(&doc);
    let first = doc.snapshot().text_objects[0].clone();
    assert!(!first.text.content.is_empty());
    assert!(
        first
            .text
            .visual_lines()
            .iter()
            .filter(|(_, line, _)| !line.is_empty())
            .count()
            <= 3
    );
    let count = first.text.content.chars().count();
    let mut t = first.text;
    t.box_width = 120.;
    doc.set_text_object(TextSettings {
        id: Some(first.id),
        text: t,
        position: first.position,
        color: first.color,
    })
    .unwrap();
    assert_eq!(whole_story(&doc), original);
    assert!(doc.snapshot().text_objects[0].text.content.chars().count() > count);
    select_page(&mut doc, 1);
    doc.undo();
    assert_eq!(whole_story(&doc), original);
    assert_eq!(
        doc.page_document(0).unwrap().snapshot().text_objects[0]
            .text
            .box_width,
        48.
    );
    doc.redo();
    let output = exported(&doc);
    let result = read::read_with_layout(
        &output.bytes,
        crate::io::ReadOptions {
            allow_lossy: true,
            ..Default::default()
        },
        Some(vertical_layout),
    )
    .unwrap();
    let crate::io::ReadContent::Vector(doc) = result.content else {
        panic!()
    };
    assert_eq!(whole_story(&doc), original);
    assert_eq!(
        doc.page_document(1).unwrap().snapshot().text_objects[0]
            .text
            .writing_mode,
        WritingMode::Vertical
    );
    let native =
        crate::native::decode(&crate::native::encode_state(&doc.document_state()).unwrap())
            .unwrap();
    assert_eq!(whole_story(&native), original);
}
fn topology(
    doc: &mut Document,
    action: &str,
    frames: Vec<lumapaint_core::document::PageTextFrame>,
    id: Option<String>,
) {
    doc.edit_publication_text(lumapaint_core::document::PublicationTextRequest {
        revision: doc.publication_text_context().revision,
        action: action.into(),
        id,
        name: Some("本文 / Story / 正文".into()),
        frames,
    })
    .unwrap();
}
#[test]
fn create_detach_ui_contract_keeps_all_text_and_restores_links_from_another_page() {
    let mut state = vertical_publication().document_state();
    state.pages.as_mut().unwrap().text_threads.clear();
    let mut doc = Document::from_document_state(state).unwrap();
    doc.set_text_layout_backend(Some(vertical_layout));
    let context = doc.publication_text_context();
    assert_eq!(context.frames.len(), 2);
    assert!(context.frames.iter().all(|f| f.available));
    let targets: Vec<_> = context.frames.iter().map(|f| f.target.clone()).collect();
    let before: Vec<_> = (0..2)
        .map(|i| {
            doc.page_document(i).unwrap().snapshot().text_objects[0]
                .text
                .content
                .clone()
        })
        .collect();
    topology(&mut doc, "create", targets, None);
    let linked = whole_story(&doc);
    assert_eq!(linked, before.concat());
    let id = doc.publication_text_context().threads[0].thread.id.clone();
    topology(&mut doc, "detach", vec![], Some(id.clone()));
    assert!(doc.publication_text_context().threads.is_empty());
    let parts: Vec<_> = (0..2)
        .map(|i| {
            doc.page_document(i).unwrap().snapshot().text_objects[0]
                .text
                .content
                .clone()
        })
        .collect();
    assert_eq!(parts.concat(), linked);
    select_page(&mut doc, 1);
    doc.undo();
    assert_eq!(whole_story(&doc), linked);
    doc.redo();
    assert!(doc.publication_text_context().threads.is_empty());
    let mut t = doc.snapshot().text_objects[0].clone();
    t.text.content.push_str("追加");
    t.text.runs.clear();
    doc.set_text_object(TextSettings {
        id: Some(t.id),
        text: t.text,
        position: t.position,
        color: t.color,
    })
    .unwrap();
    select_page(&mut doc, 0);
    doc.undo();
    doc.undo();
    assert_eq!(whole_story(&doc), linked);
    doc.undo();
    assert!(doc.publication_text_context().threads.is_empty());
    let restored: Vec<_> = (0..2)
        .map(|i| {
            doc.page_document(i).unwrap().snapshot().text_objects[0]
                .text
                .content
                .clone()
        })
        .collect();
    assert_eq!(restored, before);
}
#[test]
fn create_link_rejects_stale_revision_and_mixed_directions_without_mutation() {
    let mut state = vertical_publication().document_state();
    state.pages.as_mut().unwrap().text_threads.clear();
    let mut doc = Document::from_document_state(state).unwrap();
    doc.set_text_layout_backend(Some(vertical_layout));
    let context = doc.publication_text_context();
    let targets: Vec<_> = context.frames.iter().map(|f| f.target.clone()).collect();
    let before = crate::native::encode_state(&doc.document_state()).unwrap();
    let request = |revision| lumapaint_core::document::PublicationTextRequest {
        revision,
        action: "create".into(),
        id: None,
        name: Some("Story".into()),
        frames: targets.clone(),
    };
    assert!(doc
        .edit_publication_text(request(context.revision + 1))
        .is_err());
    assert_eq!(
        before,
        crate::native::encode_state(&doc.document_state()).unwrap()
    );
    let mut state = doc.document_state();
    let page = state.pages.as_mut().unwrap().pages[1]
        .content
        .as_mut()
        .unwrap();
    let o = &mut page.svg_layers[0].vector_objects[0];
    o.text.as_mut().unwrap().writing_mode = WritingMode::Horizontal;
    o.text.as_mut().unwrap().clear_measured_layout();
    page.svg_layers[0].source = lumapaint_core::document::vector_svg(
        page.width,
        page.height,
        &page.svg_layers[0].vector_objects,
    );
    let mut doc = Document::from_document_state(state).unwrap();
    doc.set_text_layout_backend(Some(vertical_layout));
    assert!(doc
        .edit_publication_text(request(doc.publication_text_context().revision))
        .is_err());
}

#[test]
fn relinking_reverses_requested_flow_without_reusing_identity_or_losing_detached_text() {
    let mut doc = vertical_publication();
    let id = doc.publication_text_context().threads[0].thread.id.clone();
    topology(&mut doc, "detach", vec![], Some(id.clone()));
    let context = doc.publication_text_context();
    let mut targets: Vec<_> = context.frames.iter().map(|f| f.target.clone()).collect();
    let expected = [1, 0]
        .map(|i| {
            doc.page_document(i).unwrap().snapshot().text_objects[0]
                .text
                .content
                .clone()
        })
        .concat();
    targets.reverse();
    topology(&mut doc, "create", targets.clone(), None);
    let thread = &doc.publication_text_context().threads[0].thread;
    assert_ne!(thread.id, id);
    assert_eq!(thread.frames, targets);
    assert_eq!(whole_story(&doc), expected);
    doc.undo();
    assert!(doc.publication_text_context().threads.is_empty());
    doc.redo();
    assert_eq!(whole_story(&doc), expected);
    crate::native::decode(&crate::native::encode_state(&doc.document_state()).unwrap()).unwrap();
}

#[test]
fn deleting_a_detached_member_cannot_restore_a_dangling_publication_link() {
    let mut doc = vertical_publication();
    let id = doc.publication_text_context().threads[0].thread.id.clone();
    topology(&mut doc, "detach", vec![], Some(id));
    let frame = doc.snapshot().text_objects[0].id.clone();
    doc.select_vector_objects(vec![frame]).unwrap();
    doc.delete_selected_vector_objects().unwrap();
    doc.undo();
    assert!(doc.publication_text_context().threads.is_empty());
    doc.undo();
    assert!(doc.publication_text_context().threads.is_empty());
    crate::native::decode(&crate::native::encode_state(&doc.document_state()).unwrap()).unwrap();
}

#[test]
fn indesign_2026_vertical_cross_page_round_trip_keeps_complete_story_and_image() {
    let bytes = include_bytes!("../../tests/fixtures/lp-adobe-idml-vertical-cross-page.idml");
    let load = |bytes: &[u8]| {
        let imported = read::read_with_links(
            bytes,
            crate::io::ReadOptions {
                allow_lossy: true,
                ..Default::default()
            },
            Some(vertical_layout),
            &mut |uri| {
                assert!(uri.ends_with("/checker.png"));
                Ok(read::ResolvedImage {
                    bytes: include_bytes!("../../tests/fixtures/lp-idml-checker.png").to_vec(),
                    source_path: None,
                })
            },
        )
        .unwrap();
        let crate::io::ReadContent::Vector(doc) = imported.content else {
            panic!()
        };
        doc
    };
    let doc = load(bytes);
    let expected = (0..160)
        .map(|i| format!("{i:03} 日本語 English 简体中文 — 跨ページの本文です。"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(whole_story(&doc), expected);
    for i in 0..2 {
        let page = doc.page_document(i).unwrap();
        assert_eq!(
            page.snapshot().text_objects[0].text.writing_mode,
            WritingMode::Vertical
        );
        assert!(!page.snapshot().text_objects[0].text.content.is_empty());
    }
    let again = load(&exported(&doc).bytes);
    assert!(again
        .page_document(1)
        .unwrap()
        .document_state()
        .svg_layers
        .iter()
        .flat_map(|layer| &layer.vector_objects)
        .any(|object| object
            .image_frame
            .as_ref()
            .and_then(|frame| frame.image.as_ref())
            .is_some()));
    assert_eq!(whole_story(&again), expected);
    let native =
        crate::native::decode(&crate::native::encode_state(&again.document_state()).unwrap())
            .unwrap();
    assert_eq!(whole_story(&native), expected);
}

#[test]
fn linking_other_pages_invalidates_the_active_panels_revision() {
    let mut doc = vertical_publication();
    let id = doc.publication_text_context().threads[0].thread.id.clone();
    doc.edit_pages(lumapaint_core::document::PageEdit {
        action: "add".into(),
        index: Some(1),
        facing: None,
        binding: None,
    })
    .unwrap();
    select_page(&mut doc, 2);
    doc.set_text_layout_backend(Some(vertical_layout));
    let revision = doc.revision();
    let stale = doc.publication_text_context().revision;
    topology(&mut doc, "detach", vec![], Some(id.clone()));
    assert!(doc.revision() > revision);
    let context = doc.publication_text_context();
    assert!(doc
        .edit_publication_text(lumapaint_core::document::PublicationTextRequest {
            revision: stale,
            action: "detach".into(),
            id: Some(id),
            name: None,
            frames: vec![],
        })
        .is_err());
    let targets = context
        .frames
        .iter()
        .map(|frame| frame.target.clone())
        .collect();
    topology(&mut doc, "create", targets, None);
    assert_ne!(doc.publication_text_context().revision, context.revision);
    doc.undo();
    assert_eq!(doc.pages_snapshot().pages.len(), 3);
    assert!(doc.publication_text_context().threads.is_empty());
    doc.redo();
    assert_eq!(doc.pages_snapshot().pages.len(), 3);
    assert_eq!(doc.publication_text_context().threads.len(), 1);
}

#[test]
fn publication_panel_stamp_tracks_other_pages_and_is_exact_in_javascript() {
    let mut doc = vertical_publication();
    let id = doc.publication_text_context().threads[0].thread.id.clone();
    doc.edit_pages(lumapaint_core::document::PageEdit {
        action: "add".into(),
        index: Some(1),
        facing: None,
        binding: None,
    })
    .unwrap();
    select_page(&mut doc, 2);
    let stale = doc.publication_text_context().revision;
    assert!(stale < (1u64 << 53));
    select_page(&mut doc, 0);
    let mut object = doc.snapshot().text_objects[0].clone();
    object.color = [130, 20, 90];
    doc.set_text_object(TextSettings {
        id: Some(object.id),
        text: object.text,
        position: object.position,
        color: object.color,
    })
    .unwrap();
    select_page(&mut doc, 2);
    assert_ne!(doc.publication_text_context().revision, stale);
    let before = crate::native::encode_state(&doc.document_state()).unwrap();
    assert!(doc
        .edit_publication_text(lumapaint_core::document::PublicationTextRequest {
            revision: stale,
            action: "detach".into(),
            id: Some(id),
            name: None,
            frames: vec![],
        })
        .is_err());
    assert_eq!(
        before,
        crate::native::encode_state(&doc.document_state()).unwrap()
    );
}
