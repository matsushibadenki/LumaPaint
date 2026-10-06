//! Host adapter: expensive raster work stays in Rust; format serialization stays independent.
use lumapaint_core::document::Document;
use lumapaint_formats::export::{
    export, ExportOptions, ExportSnapshot, ExportedDocument, FormatId,
};

pub fn svg(document: &Document) -> Result<ExportedDocument, String> {
    build(document, FormatId::Svg)
}
pub fn pdf(document: &Document) -> Result<ExportedDocument, String> {
    let count = document.pages_snapshot().pages.len();
    if count == 1 {
        return build(document, FormatId::Pdf);
    }
    let mut publication = lumapaint_formats::pdf::PdfPublication::default();
    for index in 0..count {
        let page = document
            .detached_page(index)
            .ok_or("Publication page is missing")?;
        publication
            .push(build(&page, FormatId::Pdf)?)
            .map_err(export_error)?;
    }
    publication
        .finish(ExportOptions { allow_lossy: true })
        .map_err(export_error)
}
fn build(document: &Document, format: FormatId) -> Result<ExportedDocument, String> {
    let mut copy = document.clone();
    copy.finish();
    let mut snapshot = ExportSnapshot::capture(&copy);
    if snapshot.state().layer_effects.values().any(|e| e.enabled) {
        snapshot = snapshot
            .with_document_png(lumapaint_renderer::thumbnails::document_png(&copy)?)
            .map_err(export_error)?;
        return export(format, &snapshot, ExportOptions { allow_lossy: true })
            .map_err(export_error);
    }
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
            copy_paint_tile(&mut pixels, w, &upload);
        }
        let png = lumapaint_renderer::vector::clipboard_png(w, h, pixels)?;
        snapshot = snapshot.with_paint_png(png).map_err(export_error)?;
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
            .map_err(export_error)?;
        return export(format, &snapshot, options).map_err(export_error);
    }
    result.map_err(export_error)
}

fn export_error(error: lumapaint_formats::export::ExportError) -> String {
    match error {
        lumapaint_formats::export::ExportError::UnsupportedFeature("pdf.print_bleed_requires_vector_content") =>
            "トンボ付きPDFでは、用紙外のレイヤー効果・フィルターの書き出しはまだ対応していません。\nPDF export with printer marks does not yet support layer effects or filters outside the page.\n带印刷标记的PDF导出暂不支持页面外的图层效果或滤镜。".into(),
        other => other.to_string(),
    }
}
fn copy_paint_tile(pixels: &mut [u8], width: u32, upload: &lumapaint_core::tiles::TileUpload) {
    let [x, y] = upload.origin;
    for row in 0..upload.extent[1] {
        let dst = (((y + row) * width + x) * 4) as usize;
        let src = (row * upload.bytes_per_row) as usize;
        let len = (upload.extent[0] * 4) as usize;
        pixels[dst..dst + len].copy_from_slice(&upload.pixels[src..src + len]);
    }
}

pub fn report_text(report: &lumapaint_formats::ConversionReport) -> String {
    report.issues.iter().map(|issue| match issue.code {
        "svg.brush_rasterized" => "ブラシは画像として保持されます。ベクターは編集可能です。\nBrushes are preserved as an image; vectors remain editable.\n笔刷保存为图像，矢量仍可编辑。",
        "pdf.effects_rasterized" => "PDFの効果はページ画像として保持されます。\nPDF effects are preserved as a page image.\nPDF效果保存为页面图像。",
        "pdf.registration_preview_only" => "PDFのレジストレーションカラーはプレビュー色へ変換され、全版属性は保持されません。\nPDF registration colors are converted to preview colors; all-plate attributes are not retained.\nPDF套版色转换为预览色，不保留全色版属性。",
        "pdf.text_outlined" => "PDFの文字は輪郭パスに変換されます。\nPDF text is converted to vector outlines.\nPDF文字转换为矢量轮廓。",
        "svg.text_requires_fonts" => "文字は編集可能ですが、表示には同じフォントが必要です。\nText remains editable and requires the same fonts.\n文字仍可编辑，显示需要相同字体。",
        "pdf.selected_page_only" => "選んだページだけを読み込みます。\nOnly the selected page is opened.\n仅打开所选页面。",
        "ai.pdf_compatible_only" => "AI内のPDF互換部分を読み込みます。AI固有の編集情報は保持されません。\nThe PDF-compatible portion of AI is opened; AI-specific editing data is not retained.\n打开AI的PDF兼容部分，不保留AI专有编辑信息。",
        "pdf.font_substitution" => "PDFのフォントをシステムフォントへ置き換えます。字形が変わる場合があります。\nPDF fonts are substituted with system fonts; glyph shapes may change.\nPDF字体替换为系统字体，字形可能变化。",
        "pdf.font_program" | "pdf.font_encoding" | "pdf.font_widths" | "pdf.font_state" | "pdf.glyph_missing" | "pdf.glyph_outline" | "pdf.text_position" => "PDF内の一部のフォント・文字符号・文字配置は対応範囲外で、文字が省略されます。\nSome PDF fonts, encodings, or text placements are unsupported; affected text is omitted.\n部分PDF字体、编码或文字位置不受支持，相关文字将被省略。",
        "pdf.invisible_text_omitted" => "PDFの不可視文字は省略されます。\nInvisible PDF text is omitted.\n省略PDF的不可见文字。",
        "pdf.text_conversion" => "PDF内の文字は読み込めません。\nPDF text cannot be imported.\n无法导入PDF文字。",
        "pdf.image_xobject" => "PDF内の画像は読み込めません。\nPDF images cannot be imported.\n无法导入PDF图像。",
        "pdf.image_encoding" | "pdf.image_stencil" | "pdf.image_matte" | "pdf.image_mask_size" => "PDF内の一部の画像形式や画像マスクは読み込めず、省略されます。\nSome PDF image encodings or image masks are unsupported and will be omitted.\n部分PDF图像编码或图像蒙版不受支持，将被省略。",
        "pdf.gradient_function" | "pdf.gradient_extent" | "pdf.gradient_geometry" | "pdf.gradient_transform" => "PDF内の一部のグラデーション関数・描画範囲・形状は対応範囲外です。\nSome PDF gradient functions, extents, or geometries are unsupported.\n部分PDF渐变函数、绘制范围或几何形状不受支持。",
        "pdf.pattern_conversion" | "pdf.shading_conversion" => "PDF内のパターンやグラデーションは読み込めません。\nPDF patterns or gradients cannot be imported.\n无法导入PDF图案或渐变。",
        "pdf.icc_profile_conversion" | "pdf.cmyk_profile_conversion" => "色は近似RGBへ変換されます。\nColors are converted to approximate RGB.\n颜色转换为近似RGB。",
        "pdf.annotations_omitted" => "PDFの注釈は省略されます。\nPDF annotations are omitted.\n省略PDF批注。",
        "pdf.transparency_group" | "pdf.soft_mask" | "pdf.blend_mode" | "pdf.graphics_effect" => "PDFの透明効果を正確に再現できない部分があります。\nSome PDF transparency effects cannot be reproduced accurately.\n部分PDF透明效果无法准确重现。",
        "pdf.optional_content" | "pdf.unsupported_operator" => "PDFの一部の表示設定や描画命令は対応範囲外です。\nSome PDF display settings or drawing instructions are unsupported.\n部分PDF显示设置或绘制指令不受支持。",
        "svg.layer_effects_rasterized" => "レイヤーエフェクトの見た目を保持するため画像へ統合します。編集可能な設定はネイティブ形式に保存してください。\nLayer effects are flattened to preserve appearance. Save editable settings in the native format.\n图层效果将合并为图像以保留外观。请使用原生格式保存可编辑设置。",
        _ => "外部形式への変換で編集情報が変わります。\nConversion changes editing information.\n格式转换会改变编辑信息。"
    }).collect::<Vec<_>>().join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cropped_edge_tile_keeps_row_pitch_and_canvas_offset() {
        use lumapaint_core::tiles::{TileCoord, TileUpload};
        let mut tile = vec![99; 256 * 256 * 4];
        tile[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        tile[1024..1032].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let upload = TileUpload {
            coord: TileCoord { x: 1, y: 0 },
            origin: [256, 0],
            extent: [2, 2],
            bytes_per_row: 1024,
            pixels: tile,
        };
        let mut canvas = vec![0; 258 * 2 * 4];
        copy_paint_tile(&mut canvas, 258, &upload);
        assert_eq!(&canvas[1024..1032], &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(&canvas[2056..2064], &[9, 10, 11, 12, 13, 14, 15, 16]);
        assert!(canvas[..1024].iter().all(|v| *v == 0));
        assert!(canvas[1032..2056].iter().all(|v| *v == 0));
    }
    #[test]
    fn pdf_exports_inactive_pages_and_active_paint_without_changing_publication() {
        use lumapaint_core::document::{Brush, PageBinding, PageEdit, Point};
        let mut doc = Document::default();
        doc.import_svg("日本語".into(), "<svg width=\"100\" height=\"60\"><rect width=\"80\" height=\"40\" fill=\"red\"/></svg>".into()).unwrap();
        doc.edit_pages(PageEdit {
            action: "duplicate".into(),
            index: Some(0),
            facing: None,
            binding: None,
        })
        .unwrap();
        doc.edit_pages(PageEdit {
            action: "layout".into(),
            index: None,
            facing: Some(true),
            binding: Some(PageBinding::RightToLeft),
        })
        .unwrap();
        doc.edit_pages(PageEdit {
            action: "select".into(),
            index: Some(1),
            facing: None,
            binding: None,
        })
        .unwrap();
        doc.begin(Point { x: 40., y: 40. }, Brush::default())
            .unwrap();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let snapshot = doc.snapshot();
        let result = pdf(&doc).unwrap();
        assert_eq!(
            lumapaint_formats::pdf::inspect(&result.bytes)
                .unwrap()
                .len(),
            2
        );
        assert!(result
            .report
            .issues
            .iter()
            .any(|i| i.code == "svg.brush_rasterized"));
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        assert!(doc.has_active_stroke());
        assert_eq!(doc.snapshot().revision, snapshot.revision);
        assert_eq!(doc.pages_snapshot().active, 1);
    }
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
