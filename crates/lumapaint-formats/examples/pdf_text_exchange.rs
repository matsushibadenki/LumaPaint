//! Reproducible text-exchange specimen for Adobe/manual QA; no user documents.
use lumapaint_formats::export::{export, ExportOptions, ExportSnapshot, FormatId};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Usage: pdf_text_exchange OUTPUT_DIRECTORY")?,
    );
    std::fs::create_dir_all(&directory)?;
    let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="480">
<rect width="640" height="480" fill="white"/>
<g font-family="Helvetica" font-size="28" fill="#204080">
<text x="32" y="55">LumaPaint PDF text exchange</text>
<text x="32" y="110" letter-spacing="2">ABC 123 — editable text</text>
<text x="32" y="160" stroke="#c04020" stroke-width="1" paint-order="stroke fill">Fill and stroke</text>
<text x="32" y="210" transform="rotate(-5 32 210)">Rotated text</text>
</g>
<g font-family="Hiragino Sans GB" font-size="28" fill="#204080">
<text x="32" y="270">日本語の文字と配置</text>
<text x="32" y="320">简体中文文本与位置</text>
<text x="555" y="90" writing-mode="tb">日本語の縦組み</text>
</g>
</svg>"##;
    let document = lumapaint_formats::svg::import("PDF text exchange".into(), source.into())?;
    let snapshot = ExportSnapshot::capture(&document);
    let output = export(
        FormatId::Pdf,
        &snapshot,
        ExportOptions { allow_lossy: true },
    )?;
    if !output
        .report
        .issues
        .iter()
        .any(|i| i.code == "pdf.text_positioned")
    {
        return Err("Specimen requires embeddable Helvetica and Hiragino Sans GB fonts".into());
    }
    let pdf = lopdf::Document::load_mem(&output.bytes)?;
    let extracted = pdf.extract_text(&[1])?;
    for expected in [
        "LumaPaint PDF text exchange",
        "日本語の文字と配置",
        "简体中文文本与位置",
        "日本語の縦組み",
    ] {
        if !extracted.contains(expected) {
            return Err(format!("Missing extracted text: {expected}").into());
        }
    }
    std::fs::write(directory.join("text-exchange.svg"), source)?;
    std::fs::write(directory.join("text-exchange.pdf"), output.bytes)?;
    std::fs::write(
        directory.join("text-exchange.lumapaint"),
        lumapaint_formats::native::encode_state(snapshot.state())?,
    )?;
    std::fs::write(directory.join("text-exchange.txt"), extracted)?;
    println!("{}", serde_json::to_string(&output.report)?);
    Ok(())
}
