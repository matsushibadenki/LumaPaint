//! Host adapter: expensive raster work stays in Rust; format serialization stays independent.
use lumapaint_core::document::Document;
use lumapaint_formats::export::{
    export, ExportOptions, ExportSnapshot, ExportedDocument, FormatId,
};

pub fn svg(document: &Document) -> Result<ExportedDocument, String> {
    build(document, FormatId::Svg)
}
pub fn pdf(document: &Document) -> Result<ExportedDocument, String> {
    build(document, FormatId::Pdf)
}
fn build(document: &Document, format: FormatId) -> Result<ExportedDocument, String> {
    let mut copy = document.clone();
    copy.finish();
    let mut snapshot = ExportSnapshot::capture(&copy);
    if snapshot.state().layer_visible && !snapshot.state().strokes.is_empty() {
        let (w, h) = copy.dimensions();
        if u64::from(w) * u64::from(h) > 16_777_216 {
            return Err("Brush export exceeds 16 megapixels / ブラシの書き出しは1600万画素までです / 笔刷导出上限为1600万像素".into());
        }
        let mut tiles = lumapaint_renderer::project_committed_paint_layer(&copy)?;
        let id = tiles.layers()[0].id.clone();
        tiles.set_layer_appearance(&id, true, 1.)?;
        tiles.set_layer_mask(&id, false, false, 1.)?;
        let mut pixels = vec![0; (w as usize) * (h as usize) * 4];
        for upload in tiles.prepare_full_uploads() {
            let x = upload.coord.x * lumapaint_core::tiles::TILE_SIZE;
            let y = upload.coord.y * lumapaint_core::tiles::TILE_SIZE;
            for row in 0..upload.extent[1] {
                let dst = (((y + row) * w + x) * 4) as usize;
                let src = (row * upload.extent[0] * 4) as usize;
                let len = (upload.extent[0] * 4) as usize;
                pixels[dst..dst + len].copy_from_slice(&upload.pixels[src..src + len]);
            }
        }
        let png = lumapaint_renderer::vector::clipboard_png(w, h, pixels)?;
        snapshot = snapshot.with_paint_png(png).map_err(|e| e.to_string())?;
    }
    let options = ExportOptions { allow_lossy: true };
    let result = export(format, &snapshot, options);
    if matches!(
        result,
        Err(lumapaint_formats::export::ExportError::UnsupportedFeature(
            "pdf.effects_require_raster_fallback"
        ))
    ) {
        snapshot = snapshot
            .with_document_png(lumapaint_renderer::thumbnails::document_png(&copy)?)
            .map_err(|e| e.to_string())?;
        return export(format, &snapshot, options).map_err(|e| e.to_string());
    }
    result.map_err(|e| e.to_string())
}

pub fn report_text(report: &lumapaint_formats::ConversionReport) -> String {
    report.issues.iter().map(|issue| match issue.code {
        "svg.brush_rasterized" => "ブラシは画像として保持されます。ベクターは編集可能です。\nBrushes are preserved as an image; vectors remain editable.\n笔刷保存为图像，矢量仍可编辑。",
        "pdf.effects_rasterized" => "PDFの効果はページ画像として保持されます。\nPDF effects are preserved as a page image.\nPDF效果保存为页面图像。",
        "pdf.text_outlined" => "PDFの文字は輪郭パスに変換されます。\nPDF text is converted to vector outlines.\nPDF文字转换为矢量轮廓。",
        "svg.text_requires_fonts" => "文字は編集可能ですが、表示には同じフォントが必要です。\nText remains editable and requires the same fonts.\n文字仍可编辑，显示需要相同字体。",
        "pdf.selected_page_only" => "最初のページだけを読み込みます。\nOnly the first page is opened.\n仅打开第一页。",
        "ai.pdf_compatible_only" => "AI内のPDF互換部分を読み込みます。AI固有の編集情報は保持されません。\nThe PDF-compatible portion of AI is opened; AI-specific editing data is not retained.\n打开AI的PDF兼容部分，不保留AI专有编辑信息。",
        "pdf.text_conversion" => "PDF内の文字は読み込めません。\nPDF text cannot be imported.\n无法导入PDF文字。",
        "pdf.image_xobject" => "PDF内の画像は読み込めません。\nPDF images cannot be imported.\n无法导入PDF图像。",
        "pdf.image_encoding" | "pdf.image_stencil" | "pdf.image_matte" | "pdf.image_mask_size" => "PDF内の一部の画像形式や画像マスクは読み込めず、省略されます。\nSome PDF image encodings or image masks are unsupported and will be omitted.\n部分PDF图像编码或图像蒙版不受支持，将被省略。",
        "pdf.pattern_conversion" | "pdf.shading_conversion" => "PDF内のパターンやグラデーションは読み込めません。\nPDF patterns or gradients cannot be imported.\n无法导入PDF图案或渐变。",
        "pdf.icc_profile_conversion" | "pdf.cmyk_profile_conversion" => "色は近似RGBへ変換されます。\nColors are converted to approximate RGB.\n颜色转换为近似RGB。",
        "pdf.annotations_omitted" => "PDFの注釈は省略されます。\nPDF annotations are omitted.\n省略PDF批注。",
        "pdf.transparency_group" | "pdf.soft_mask" | "pdf.blend_mode" | "pdf.graphics_effect" => "PDFの透明効果を正確に再現できない部分があります。\nSome PDF transparency effects cannot be reproduced accurately.\n部分PDF透明效果无法准确重现。",
        "pdf.optional_content" | "pdf.unsupported_operator" => "PDFの一部の表示設定や描画命令は対応範囲外です。\nSome PDF display settings or drawing instructions are unsupported.\n部分PDF显示设置或绘制指令不受支持。",
        _ => "外部形式への変換で編集情報が変わります。\nConversion changes editing information.\n格式转换会改变编辑信息。"
    }).collect::<Vec<_>>().join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefixed_svg_filter_uses_pdf_raster_fallback_without_mutating_document() {
        let mut document = Document::default();
        document.import_svg("Filter".into(),r##"<s:svg xmlns:s="http://www.w3.org/2000/svg" width="100" height="60"><s:defs><s:filter id="blur"><s:feGaussianBlur stdDeviation="2"/></s:filter></s:defs><s:rect width="80" height="40" fill="red" filter="url(#blur)"/></s:svg>"##.into()).unwrap();
        let before = serde_json::to_value(document.document_state()).unwrap();
        let exported = pdf(&document).unwrap();
        assert!(exported.bytes.starts_with(b"%PDF-"));
        assert!(exported
            .report
            .issues
            .iter()
            .any(|i| i.code == "pdf.effects_rasterized"));
        assert_eq!(
            serde_json::to_value(document.document_state()).unwrap(),
            before
        );
    }
    #[test]
    fn export_active_paint_keeps_original_and_png_report() {
        use lumapaint_core::document::{Brush, Point};
        let mut document = Document::default();
        document
            .begin(Point { x: 40., y: 40. }, Brush::default())
            .unwrap();
        let before = document.snapshot();
        let result = svg(&document).unwrap();
        let source = std::str::from_utf8(&result.bytes).unwrap();
        assert!(source.contains("data:image/png"));
        assert_eq!(result.report.issues[0].code, "svg.brush_rasterized");
        lumapaint_renderer::validate_svg(source).unwrap();
        assert!(document.has_active_stroke());
        assert_eq!(document.snapshot().revision, before.revision);
    }
}
