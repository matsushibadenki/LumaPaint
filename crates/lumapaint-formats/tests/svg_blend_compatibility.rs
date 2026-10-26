#[test]
fn svg_plus_lighter_pdf_export_is_explicitly_rejected() {
    let tree = usvg::Tree::from_str(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><g style="mix-blend-mode:plus-lighter"><rect width="20" height="20" fill="red"/></g></svg>"#,
        &usvg::Options::default(),
    )
    .unwrap();
    let error = svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        svg2pdf::ConversionError::UnsupportedBlendMode
    ));
}

#[test]
fn existing_svg_multiply_blend_still_exports_to_pdf() {
    let tree = usvg::Tree::from_str(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><g style="mix-blend-mode:multiply"><rect width="20" height="20" fill="red"/></g></svg>"#,
        &usvg::Options::default(),
    )
    .unwrap();
    let pdf = svg2pdf::to_pdf(
        &tree,
        svg2pdf::ConversionOptions::default(),
        svg2pdf::PageOptions::default(),
    )
    .unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}
