//! Reproducible native-text IDML specimen. Uses installed fonts, no user artwork.
use lumapaint_core::{
    document::{
        Document, PageBinding, PageBookState, PageState, TextSettings, VectorSelectionRequest,
    },
    vector::{VectorText, WritingMode},
};
use lumapaint_formats::export::{export, ExportOptions, ExportSnapshot, FormatId};
fn add(
    doc: &mut Document,
    content: String,
    font: &str,
    size: f32,
    position: [f32; 2],
    dimensions: [f32; 2],
    mode: WritingMode,
) -> Result<String, String> {
    doc.set_text_object(TextSettings {
        id: None,
        text: VectorText {
            content,
            font_family: font.into(),
            font_size: size,
            line_height: 1.5,
            box_width: dimensions[0],
            box_height: Some(dimensions[1]),
            writing_mode: mode,
            ..Default::default()
        },
        position,
        color: [32, 64, 128],
    })?;
    Ok(doc.snapshot().text_objects.last().unwrap().id.clone())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Usage: idml_text_exchange OUTPUT_DIRECTORY")?,
    );
    std::fs::create_dir_all(&directory)?;
    let mut state = Document::default().document_state();
    state.width = 612;
    state.height = 792;
    state.resolution = Some(72);
    let mut doc = Document::from_document_state(state.clone())?;
    add(
        &mut doc,
        "LumaPaint IDML text exchange".into(),
        "Helvetica",
        24.,
        [40., 30.],
        [532., 50.],
        WritingMode::Horizontal,
    )?;
    let content = (0..400)
        .map(|i| format!("{i:03} 日本語 English 简体中文 — 長文の本文です。"))
        .collect::<Vec<_>>()
        .join("\n");
    let first = add(
        &mut doc,
        content,
        "Hiragino Sans GB",
        14.,
        [40., 100.],
        [250., 560.],
        WritingMode::Horizontal,
    )?;
    let second = add(
        &mut doc,
        String::new(),
        "Hiragino Sans GB",
        14.,
        [320., 100.],
        [250., 560.],
        WritingMode::Horizontal,
    )?;
    doc.set_text_layout_backend(Some(|t, _| {
        t.reflow_soft_breaks_with(|s, _| Ok(s.chars().count() as f32 * 7.))
    }));
    doc.select_vector_objects(vec![first, second])?;
    doc.vector_asset_action(VectorSelectionRequest {
        action: "assetCreate".into(),
        criterion: Some("linkedText".into()),
        name: Some("本文 / Story / 正文".into()),
        new_name: None,
        bulk: None,
    })?;
    let mut vertical = Document::from_document_state(state)?;
    add(
        &mut vertical,
        "日本語の縦組み\n简体中文\nEnglish".into(),
        "Hiragino Sans GB",
        24.,
        [60., 60.],
        [440., 640.],
        WritingMode::Vertical,
    )?;
    let mut state = doc.document_state();
    state.pages = Some(PageBookState {
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
                content: Some(Box::new(vertical.document_state())),
            },
        ],
    });
    let document = Document::from_document_state(state)?;
    let snapshot = ExportSnapshot::capture(&document);
    let result = export(
        FormatId::Idml,
        &snapshot,
        ExportOptions { allow_lossy: true },
    )?;
    std::fs::write(directory.join("text-exchange.idml"), result.bytes)?;
    std::fs::write(
        directory.join("text-exchange.lumapaint"),
        lumapaint_formats::native::encode_state(snapshot.state())?,
    )?;
    println!("{}", serde_json::to_string(&result.report)?);
    Ok(())
}
