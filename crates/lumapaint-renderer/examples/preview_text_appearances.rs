use lumapaint_core::{
    document::{Document, TextSettings},
    text_appearance::{presets, TextAppearance},
    vector::VectorText,
};
use lumapaint_renderer::vector::{document_png, rasterize_svg, reflow_text_with_system_fonts};
fn main() -> Result<(), String> {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/lumapaint-headline-preview".into());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut doc = Document::default();
    let mut text = VectorText {
        content: "見出し Title".into(),
        font_size: 80.,
        bold: true,
        point_text: true,
        box_width: 700.,
        ..Default::default()
    };
    reflow_text_with_system_fonts(&mut text, [28, 40, 60])?;
    let mut settings = TextSettings {
        id: None,
        text,
        position: [70., 80.],
        color: [28, 40, 60],
    };
    doc.set_text_object(settings.clone())?;
    settings.id = Some(doc.snapshot().text_objects[0].id.clone());
    std::fs::write(
        format!("{dir}/Headline-QA.lumapaint"),
        lumapaint_formats::native::encode_state(&doc.document_state())?,
    )
    .map_err(|e| e.to_string())?;
    for preset in presets() {
        let preset = &preset.id;
        settings.text.appearance = Some(TextAppearance {
            preset: preset.clone(),
            enabled: true,
            amount: 1.,
            custom: None,
        });
        doc.set_text_object(settings.clone())?;
        let source = &doc
            .svg_layers()
            .find(|l| l.source.contains("data-lp-text-appearance"))
            .unwrap()
            .source;
        let raster = rasterize_svg(source, 960, 640)?;
        std::fs::write(
            format!("{dir}/{preset}.png"),
            document_png(960, 640, raster.pixels)?,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
