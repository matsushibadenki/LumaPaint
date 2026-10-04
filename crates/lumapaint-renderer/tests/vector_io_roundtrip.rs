use lumapaint_formats::{
    export::{export, ExportOptions, ExportSnapshot, FormatId},
    io::{read_document, ReadContent, ReadOptions},
};
use lumapaint_renderer::vector::rasterize_svg;

#[test]
fn editable_svg_preserves_css_use_gradient_clip_and_mask_pixels() {
    let source = r##"<svg width="120" height="80" xmlns="http://www.w3.org/2000/svg"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><clipPath id="c"><circle cx="50" cy="40" r="30"/></clipPath><mask id="m"><rect width="120" height="80" fill="white"/><rect x="45" width="10" height="80" fill="black"/></mask><rect id="shape" width="90" height="70"/></defs><style>.shape {fill:url(#g)}</style><g mask="url(#m)"><use href="#shape" class="shape" clip-path="url(#c)"/></g></svg>"##;
    let imported = read_document(
        FormatId::Svg,
        "Round trip".into(),
        source.as_bytes(),
        ReadOptions::default(),
    )
    .unwrap();
    let ReadContent::Vector(document) = imported.content else {
        panic!()
    };
    let exported = export(
        FormatId::Svg,
        &ExportSnapshot::capture(&document),
        ExportOptions::default(),
    )
    .unwrap();
    let output = std::str::from_utf8(&exported.bytes).unwrap();
    assert!(!output.contains("data:image"));
    let a = rasterize_svg(source, 120, 80).unwrap();
    let b = rasterize_svg(output, 120, 80).unwrap();
    assert!(a.pixels == b.pixels, "pixel mismatch");
}

#[test]
fn pdf_vector_roundtrip_preserves_fill_stroke_and_transforms() {
    let source = r##"<svg width="120" height="80"><path d="M0 0H50V35H0Z" transform="translate(22 17) rotate(15)" fill="#e03020" stroke="#2040d0" stroke-width="3" stroke-linejoin="round"/></svg>"##;
    let ReadContent::Vector(document) = read_document(
        FormatId::Svg,
        "Round trip".into(),
        source.as_bytes(),
        ReadOptions::default(),
    )
    .unwrap()
    .content
    else {
        panic!()
    };
    let pdf = export(
        FormatId::Pdf,
        &ExportSnapshot::capture(&document),
        ExportOptions::default(),
    )
    .unwrap();
    let ReadContent::Vector(result) = read_document(
        FormatId::Pdf,
        "Round trip".into(),
        &pdf.bytes,
        ReadOptions {
            raster_dpi: 72,
            ..Default::default()
        },
    )
    .unwrap()
    .content
    else {
        panic!()
    };
    let svg = result.svg_layers().next().unwrap().source.clone();
    let a = rasterize_svg(source, 120, 80).unwrap();
    let b = rasterize_svg(&svg, 120, 80).unwrap();
    // Both trees use resvg (no Skia feature here); edge rounding allows <=1 channel.
    let error: u64 = a
        .pixels
        .iter()
        .zip(&b.pixels)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    assert!(error < 120 * 80, "round-trip pixel error {error}");
}

#[test]
fn static_css_variables_calc_scopes_cycles_and_percentages_match_explicit_shapes() {
    let source = r##"<svg width="200" height="100"><style>
    :root {--base:20px;--pos:var(--base);--size:calc(var(--base) * 2);--a:var(--b);--b:var(--a);--ink:var(--a,blue)}
    .shape {x:calc(var(--pos) + 5px);y:12px;width:var(--missing,calc(var(--size) + 6px));height:calc(5px * 5);fill:var(--ink);opacity:calc(1 / 2)}
    .percent {x:100px; width:calc(50% - 10%);height:10px;fill:var(--CASE);}
    </style><g style="--base:8px;--Case:red;--CASE:green"><rect class="shape"/><rect class="percent"/></g></svg>"##;
    let explicit = r##"<svg width="200" height="100"><rect x="25" y="12" width="46" height="25" fill="blue" opacity=".5"/><rect x="100" width="80" height="10" fill="green"/></svg>"##;
    let a = rasterize_svg(source, 200, 100).unwrap();
    let b = rasterize_svg(explicit, 200, 100).unwrap();

    let ReadContent::Vector(document) = read_document(
        FormatId::Svg,
        "CSS variables".into(),
        source.as_bytes(),
        ReadOptions::default(),
    )
    .unwrap()
    .content
    else {
        panic!()
    };
    let output = export(
        FormatId::Svg,
        &ExportSnapshot::capture(&document),
        ExportOptions::default(),
    )
    .unwrap();
    assert!(
        a.pixels == b.pixels,
        "pixel mismatch: {}",
        std::str::from_utf8(&output.bytes).unwrap()
    );
    let c = rasterize_svg(std::str::from_utf8(&output.bytes).unwrap(), 200, 100).unwrap();
    assert!(a.pixels == c.pixels, "export pixel mismatch");
}

#[test]
fn invalid_css_variable_or_calc_resets_winning_value_without_hanging() {
    let source = r##"<svg width="100" height="60"><style>:root{--a:var(--a)}rect{fill:red;fill:var(--a);opacity:calc(1 / 0)}</style><rect width="40" height="30"/></svg>"##;
    let explicit =
        r##"<svg width="100" height="60"><rect width="40" height="30" fill="black"/></svg>"##;
    assert_eq!(
        rasterize_svg(source, 100, 60).unwrap().pixels,
        rasterize_svg(explicit, 100, 60).unwrap().pixels
    );
}

#[test]
fn pdf_images_with_alpha_and_group_opacity_roundtrip_without_flips_or_double_alpha() {
    use base64::Engine;
    let mut bytes = Vec::new();
    {
        let mut e = png::Encoder::new(&mut bytes, 2, 2);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .unwrap()
            .write_image_data(&[
                255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 0, 0,
            ])
            .unwrap();
    }
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let source = format!(
        r##"<svg width="100" height="80"><defs><clipPath id="c"><rect x="10" y="5" width="65" height="55"/></clipPath></defs><g opacity=".6" clip-path="url(#c)"><rect width="30" height="40" fill="red"/><rect x="15" width="30" height="40" fill="blue"/><image width="40" height="40" x="40" y="10" image-rendering="pixelated" href="data:image/png;base64,{data}"/></g></svg>"##
    );
    let ReadContent::Vector(document) = read_document(
        FormatId::Svg,
        "Image".into(),
        source.as_bytes(),
        ReadOptions::default(),
    )
    .unwrap()
    .content
    else {
        panic!()
    };
    let pdf = export(
        FormatId::Pdf,
        &ExportSnapshot::capture(&document),
        ExportOptions::default(),
    )
    .unwrap();
    let ReadContent::Vector(result) = read_document(
        FormatId::Pdf,
        "Image".into(),
        &pdf.bytes,
        ReadOptions {
            raster_dpi: 72,
            ..Default::default()
        },
    )
    .unwrap()
    .content
    else {
        panic!()
    };
    let a = rasterize_svg(&source, 100, 80).unwrap();
    let b = rasterize_svg(&result.svg_layers().next().unwrap().source, 100, 80).unwrap();
    let error: u64 = a
        .pixels
        .iter()
        .zip(&b.pixels)
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    assert!(error < 100 * 80, "image/group pixel error {error}");
}
