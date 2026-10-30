//! Reproducible text/path/image IDML and optional Adobe re-output import check.
use base64::Engine;
use lumapaint_core::{
    document::{vector_svg, Document, TextSettings},
    vector::VectorText,
};
use lumapaint_formats::{
    export::{export, ExportOptions, ExportSnapshot, FormatId},
    io::{ReadContent, ReadOptions},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Output directory required")?,
    );
    std::fs::create_dir_all(&dir)?;
    let mut state = Document::default().document_state();
    state.width = 612;
    state.height = 792;
    state.resolution = Some(72);
    let mut doc = Document::from_document_state(state)?;
    doc.set_text_object(TextSettings {
        id: None,
        text: VectorText {
            content: "Editable 日本語 / English / 简体中文".into(),
            font_family: "Hiragino Sans GB".into(),
            font_size: 22.,
            box_width: 540.,
            box_height: Some(70.),
            ..Default::default()
        },
        position: [36., 30.],
        color: [30, 60, 100],
    })?;
    let mut state = doc.document_state();
    let shape = serde_json::from_value(
        serde_json::json!({"id":"curve","name":"Bezier","path":{"data":"M40 150 C100 80 180 240 260 150 L260 290 L40 290 Z","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":{"color":[220,80,110,255]},"stroke":{"color":[30,60,100,255]},"strokeWidth":4,"visible":true}),
    )?;
    state.svg_layers[0].vector_objects.push(shape);
    let mut bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut bytes, 64, 64);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = (0..64)
            .flat_map(|y| {
                (0..64).flat_map(move |x| {
                    if (x / 8 + y / 8) % 2 == 0 {
                        [20, 110, 200]
                    } else {
                        [240, 200, 70]
                    }
                })
            })
            .collect();
        enc.write_header()?.write_image_data(&data)?;
    }
    let uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    let image = serde_json::from_value(
        serde_json::json!({"id":"image","name":"Embedded checker","path":{"data":"M320 150 H560 V390 H320 Z","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":null,"stroke":null,"strokeWidth":0,"visible":true,"imageFrame":{"fitting":"stretch","contentTransform":[3.75,0,0,3.75,320,150],"image":{"sourcePath":null,"fingerprint":"checker","name":"Checker.png","width":64,"height":64,"encodedWidth":64,"encodedHeight":64,"orientationTransform":[1,0,0,1,0,0],"dataUri":uri}}}),
    )?;
    state.svg_layers[0].vector_objects.push(image);
    for l in &mut state.svg_layers {
        l.source = vector_svg(state.width, state.height, &l.vector_objects);
    }
    let doc = Document::from_document_state(state)?;
    let snapshot = ExportSnapshot::capture(&doc);
    let encoded = export(
        FormatId::Idml,
        &snapshot,
        ExportOptions { allow_lossy: true },
    )?;
    std::fs::write(dir.join("mixed.idml"), &encoded.bytes)?;
    std::fs::write(
        dir.join("mixed.lumapaint"),
        lumapaint_formats::native::encode_state(snapshot.state())?,
    )?;
    let read_bytes = if let Some(path) = std::env::args_os().nth(2) {
        std::fs::read(path)?
    } else {
        encoded.bytes
    };
    let loaded = lumapaint_formats::idml::read::read(
        &read_bytes,
        ReadOptions {
            allow_lossy: true,
            ..Default::default()
        },
    )?;
    let ReadContent::Vector(document) = loaded.content else {
        panic!()
    };
    let state = document.document_state();
    println!(
        "pages={} text={} objects={} images={} report={}",
        document.pages_snapshot().pages.len(),
        document.snapshot().text_objects.len(),
        state
            .svg_layers
            .iter()
            .map(|l| l.vector_objects.len())
            .sum::<usize>(),
        state
            .svg_layers
            .iter()
            .flat_map(|l| &l.vector_objects)
            .filter(|o| o.image_frame.is_some())
            .count(),
        serde_json::to_string(&loaded.report)?
    );
    std::fs::write(
        dir.join("imported.lumapaint"),
        lumapaint_formats::native::encode_state(&state)?,
    )?;
    Ok(())
}
