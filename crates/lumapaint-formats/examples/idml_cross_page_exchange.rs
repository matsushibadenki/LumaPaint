//! Self-authored two-page story and an external PNG for Adobe interoperability checks.
use base64::Engine;
use lumapaint_core::{
    document::{
        vector_svg, Document, PageBinding, PageBookState, PageState, PageTextFrame, PageTextThread,
        TextSettings,
    },
    vector::VectorText,
};
use lumapaint_formats::{
    export::{export, ExportOptions, ExportSnapshot, FormatId},
    io::{ReadContent, ReadOptions},
};
use std::io::{Cursor, Read, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Output directory required")?,
    );
    std::fs::create_dir_all(directory.join("Links"))?;
    let vertical = std::env::var_os("LUMAPAINT_IDML_VERTICAL").is_some();
    let mut states = Vec::new();
    let mut refs = Vec::new();
    for page in 0..2 {
        let mut state = Document::default().document_state();
        state.width = 612;
        state.height = 792;
        state.resolution = Some(72);
        let mut doc = Document::from_document_state(state)?;
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: if page == 0 {
                    (0..160)
                        .map(|i| format!("{i:03} 日本語 English 简体中文 — 跨ページの本文です。"))
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    String::new()
                },
                font_family: "Hiragino Sans GB".into(),
                font_size: 14.,
                line_height: 1.5,
                writing_mode: if vertical {
                    lumapaint_core::vector::WritingMode::Vertical
                } else {
                    Default::default()
                },
                box_width: if vertical { 120. } else { 500. },
                box_height: Some(550.),
                ..Default::default()
            },
            position: [if vertical { 440. } else { 40. }, 70.],
            color: [30, 60, 100],
        })?;
        refs.push(PageTextFrame {
            page_id: format!("page-{}", page + 1),
            object_id: doc.snapshot().text_objects[0].id.clone(),
        });
        states.push(doc.document_state());
    }
    let mut png = Vec::new();
    {
        let mut e = png::Encoder::new(&mut png, 16, 16);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let pixels: Vec<u8> = (0..16)
            .flat_map(|y| {
                (0..16).flat_map(move |x| {
                    if (x / 4 + y / 4) % 2 == 0 {
                        [20, 110, 200]
                    } else {
                        [240, 200, 70]
                    }
                })
            })
            .collect();
        e.write_header()?.write_image_data(&pixels)?;
    }
    let path = directory.join("Links/checker.png");
    std::fs::write(&path, &png)?;
    let image = serde_json::from_value(
        serde_json::json!({"id":"linked-checker","name":"External PNG","path":{"data":"M500 680H564V744H500Z","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":null,"stroke":null,"strokeWidth":0,"visible":true,"imageFrame":{"fitting":"stretch","contentTransform":[4,0,0,4,500,680],"image":{"sourcePath":null,"fingerprint":"checker","name":"checker.png","width":16,"height":16,"encodedWidth":16,"encodedHeight":16,"orientationTransform":[1,0,0,1,0,0],"dataUri":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(&png))}}}),
    )?;
    states[1].svg_layers[0].vector_objects.push(image);
    for l in &mut states[1].svg_layers {
        l.source = vector_svg(612, 792, &l.vector_objects);
    }
    let second = states.pop().unwrap();
    let mut first = states.pop().unwrap();
    first.pages = Some(PageBookState {
        text_threads: vec![PageTextThread {
            id: "cross-page".into(),
            name: "本文".into(),
            frames: refs,
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
    let doc = Document::from_document_state(first)?;
    let snapshot = ExportSnapshot::capture(&doc);
    std::fs::write(
        directory.join("cross-page.lumapaint"),
        lumapaint_formats::native::encode_state(snapshot.state())?,
    )?;
    let encoded = export(
        FormatId::Idml,
        &snapshot,
        ExportOptions { allow_lossy: true },
    )?;
    let mut source = zip::ZipArchive::new(Cursor::new(&encoded.bytes))?;
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut entry = source.by_index(i)?;
        let name = entry.name().to_owned();
        let mut s = String::new();
        entry.read_to_string(&mut s)?;
        if name.starts_with("Spreads/") && s.contains("<Contents>") {
            let a = s.find("<Contents>").unwrap();
            let b = s[a..].find("</Contents>").unwrap() + a + "</Contents>".len();
            s.replace_range(a..b, "");
            let at = s.find("</Image>").unwrap();
            s.insert_str(at,&format!("<Link Self=\"u78000000\" LinkResourceURI=\"file:{}\" LinkResourceFormat=\"$ID/PNG\" StoredState=\"Normal\" LinkClassID=\"35906\" LinkClientID=\"257\" ImportPolicy=\"NoAutoImport\" ExportPolicy=\"NoAutoExport\"/>",path.canonicalize()?.display()));
        }
        output.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )?;
        output.write_all(s.as_bytes())?;
    }
    let bytes = output.finish()?.into_inner();
    std::fs::write(directory.join("cross-page-linked.idml"), &bytes)?;
    let input = if let Some(path) = std::env::args_os().nth(2) {
        std::fs::read(path)?
    } else {
        bytes
    };
    let loaded = lumapaint_formats::idml::read::read_with_links(
        &input,
        ReadOptions {
            allow_lossy: true,
            ..Default::default()
        },
        Some(|t, _| {
            if t.writing_mode == lumapaint_core::vector::WritingMode::Vertical {
                t.reflow_vertical();
                t.validate()
            } else {
                t.reflow_soft_breaks_with(|s, _| Ok(s.chars().count() as f32 * 7.))
            }
        }),
        &mut |uri| {
            Ok(lumapaint_formats::idml::read::ResolvedImage {
                bytes: std::fs::read(uri.strip_prefix("file:").unwrap_or(uri)).map_err(|_| {
                    lumapaint_formats::ImportError::Unsupported("idml.external_image")
                })?,
                source_path: Some(path.canonicalize().unwrap().to_string_lossy().into_owned()),
            })
        },
    )?;
    let ReadContent::Vector(doc) = loaded.content else {
        panic!()
    };
    let state = doc.document_state();
    println!(
        "pages={} cross_page_stories={} page_characters={:?}",
        doc.pages_snapshot().pages.len(),
        state.pages.as_ref().unwrap().text_threads.len(),
        (0..2)
            .map(|i| doc.page_document(i).unwrap().snapshot().text_objects[0]
                .text
                .content
                .chars()
                .count())
            .collect::<Vec<_>>()
    );
    std::fs::write(
        directory.join("imported.lumapaint"),
        lumapaint_formats::native::encode_state(&state)?,
    )?;
    Ok(())
}
