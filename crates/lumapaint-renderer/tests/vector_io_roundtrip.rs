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

#[test]
fn pdf_gradient_roundtrip_preserves_multiple_stops_radial_focal_point_and_transforms() {
    for (name, source) in [
        (
            "alpha-mask",
            r##"<svg width="120" height="90"><defs><mask id="m" style="mask-type:alpha" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="80"><rect width="65" height="60" fill="black" opacity=".4"/></mask></defs><g mask="url(#m)" transform="translate(8 5)"><rect width="80" height="65" fill="red"/></g></svg>"##,
        ),
        (
            "hard-stop",
            r##"<svg width="120" height="90"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset=".5" stop-color="red"/><stop offset=".5" stop-color="blue"/><stop offset="1" stop-color="blue"/></linearGradient></defs><rect width="100" height="80" fill="url(#g)"/></svg>"##,
        ),
        (
            "alpha",
            r##"<svg width="120" height="90"><defs><linearGradient id="g"><stop stop-color="red" stop-opacity=".15"/><stop offset=".4" stop-color="yellow" stop-opacity=".8"/><stop offset="1" stop-color="blue" stop-opacity=".3"/></linearGradient></defs><g opacity=".7" transform="translate(10 15) rotate(8)"><rect width="80" height="50" fill="url(#g)"/></g></svg>"##,
        ),
        (
            "mask",
            r##"<svg width="120" height="90"><defs><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="80"><rect width="100" height="80" fill="white"/><rect x="35" width="20" height="80" fill="#808080"/></mask></defs><g mask="url(#m)" opacity=".65" transform="translate(8 5)"><rect width="80" height="65" fill="red"/><rect x="25" y="15" width="65" height="45" fill="blue"/></g></svg>"##,
        ),
        (
            "linear",
            r##"<svg width="120" height="90"><defs><linearGradient id="g" x1="0" y1="0" x2="90" y2="25" gradientUnits="userSpaceOnUse"><stop stop-color="#ff2000"/><stop offset=".25" stop-color="#20ff00"/><stop offset=".8" stop-color="#0020ff"/><stop offset="1" stop-color="#ffffff"/></linearGradient></defs><path d="M0 0H90V55H0Z" transform="translate(10 12) rotate(8)" fill="url(#g)"/></svg>"##,
        ),
        (
            "radial",
            r##"<svg width="120" height="90"><defs><radialGradient id="g" cx="45" cy="35" r="38" fx="34" fy="25" gradientUnits="userSpaceOnUse" gradientTransform="translate(5 2) scale(1 .8)"><stop stop-color="#ffe000"/><stop offset=".6" stop-color="#e05030"/><stop offset="1" stop-color="#2040a0"/></radialGradient></defs><path d="M10 5H100V75H10Z" fill="url(#g)" stroke="url(#g)" stroke-width="4"/></svg>"##,
        ),
        (
            "group",
            r##"<svg width="120" height="90"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset=".4" stop-color="yellow"/><stop offset="1" stop-color="blue"/></linearGradient></defs><g opacity=".6" transform="translate(12 8) rotate(12)"><rect width="70" height="45" fill="url(#g)"/><rect x="20" y="20" width="60" height="35" fill="url(#g)"/></g></svg>"##,
        ),
    ] {
        let ReadContent::Vector(document) = read_document(
            FormatId::Svg,
            name.into(),
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
        let imported = read_document(
            FormatId::Pdf,
            name.into(),
            &pdf.bytes,
            ReadOptions {
                raster_dpi: 72,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(imported.report.issues.is_empty());
        let ReadContent::Vector(result) = imported.content else {
            panic!()
        };
        let a = rasterize_svg(source, 120, 90).unwrap();
        let b = rasterize_svg(&result.svg_layers().next().unwrap().source, 120, 90).unwrap();
        let error: u64 = a
            .pixels
            .iter()
            .zip(&b.pixels)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        assert!(
            error < 120 * 90 * 2,
            "{name} pixel error {error}: {}",
            result.svg_layers().next().unwrap().source
        );
    }
}

fn text_pdf(content: &str, cid: bool) -> Vec<u8> {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.7");
    let program = doc.add_object(Stream::new(
        dictionary! {},
        include_bytes!("../../lumapaint-formats/tests/fixtures/lp-pdf-test.ttf").to_vec(),
    ));
    let descriptor=doc.add_object(dictionary!{"Type"=>"FontDescriptor","FontName"=>"LPPDFTest","Flags"=>32,"FontBBox"=>vec![0.into(),(-200).into(),800.into(),800.into()],"ItalicAngle"=>0,"Ascent"=>800,"Descent"=>-200,"CapHeight"=>700,"StemV"=>80,"FontFile2"=>program});
    let font = if cid {
        let map = doc.add_object(Stream::new(dictionary! {}, vec![0, 0, 0, 2, 0, 1, 0, 4]));
        let child=doc.add_object(dictionary!{"Type"=>"Font","Subtype"=>"CIDFontType2","BaseFont"=>"LPPDFTest","CIDSystemInfo"=>dictionary!{"Registry"=>Object::string_literal("Adobe"),"Ordering"=>Object::string_literal("Identity"),"Supplement"=>0},"FontDescriptor"=>descriptor,"CIDToGIDMap"=>map,"DW"=>900,"W"=>vec![1.into(),vec![800.into(),600.into()].into(),3.into(),3.into(),1100.into()]});
        doc.add_object(dictionary!{"Type"=>"Font","Subtype"=>"Type0","BaseFont"=>"LPPDFTest","Encoding"=>"Identity-H","DescendantFonts"=>vec![child.into()]})
    } else {
        let mut widths = vec![600.into(); 36];
        widths[0] = 400.into();
        widths[34] = 800.into();
        doc.add_object(dictionary!{"Type"=>"Font","Subtype"=>"TrueType","BaseFont"=>"LPPDFTest","FontDescriptor"=>descriptor,"Encoding"=>"WinAnsiEncoding","FirstChar"=>32,"LastChar"=>67,"Widths"=>widths})
    };
    let pages = doc.new_object_id();
    let stream = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),100.into(),80.into()],"Resources"=>dictionary!{"Font"=>dictionary!{"F"=>font}},"Contents"=>stream});
    doc.objects.insert(
        pages,
        dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1}.into(),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
#[test]
fn pdf_embedded_text_outlines_preserve_spacing_cid_mappings_stroke_and_clip_pixels() {
    for (name, cid, content, expected) in [
        (
            "empty-text-clip",
            false,
            "BT /F 20 Tf 7 Tr ( ) Tj ET 0 0 1 rg 0 0 100 80 re f",
            r##"<svg width="100" height="80"/>"##,
        ),
        (
            "curve",
            false,
            "BT /F 20 Tf 1 .25 .15 1 10 20 Tm (C) Tj ET",
            r##"<svg width="100" height="80"><g transform="matrix(1 0 0 -1 0 80)"><path d="M0 0Q0 700 500 700Q500 0 0 0Z" transform="matrix(.02 .005 .003 .02 10 20)"/></g></svg>"##,
        ),
        (
            "spacing",
            false,
            "BT /F 20 Tf 1 0 0 1 10 50 Tm 2 Tc 4 Tw 80 Tz 3 Ts [(A ) 100 (B)] TJ 24 TL (A) ' ET",
            r##"<svg width="100" height="80"><g transform="matrix(1 0 0 -1 0 80)"><path d="M10 53L18 53L14 67Z M30.8 53H37.2V67H30.8Z M10 29L18 29L14 43Z"/></g></svg>"##,
        ),
        (
            "cid",
            true,
            "BT /F 20 Tf 1 0 0 1 10 40 Tm <000100020003> Tj ET",
            r##"<svg width="100" height="80"><g transform="matrix(1 0 0 -1 0 80)"><path d="M10 40H18V54H10Z M26 40L36 40L31 54Z M38 40H52V54H38Z"/></g></svg>"##,
        ),
        (
            "stroke",
            false,
            "3 w 1 0 0 rg 0 0 1 RG BT /F 20 Tf 2 0 0 2 10 20 Tm 2 Tr (A) Tj ET",
            r##"<svg width="100" height="80"><g transform="matrix(1 0 0 -1 0 80)"><path d="M10 20L30 20L20 48Z" fill="red" stroke="blue" stroke-width="3"/></g></svg>"##,
        ),
        (
            "clip",
            false,
            "BT /F 30 Tf 1 0 0 1 10 25 Tm 7 Tr (AB) Tj ET 0 0 1 rg 0 0 100 80 re f",
            r##"<svg width="100" height="80"><defs><clipPath id="c"><path d="M10 55L25 55L17.5 34Z M28 55H40V34H28Z"/></clipPath></defs><rect width="100" height="80" fill="blue" clip-path="url(#c)"/></svg>"##,
        ),
    ] {
        let pdf = text_pdf(content, cid);
        let result = read_document(
            FormatId::Pdf,
            name.into(),
            &pdf,
            ReadOptions {
                raster_dpi: 72,
                allow_lossy: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.report.issues.len(), 1);
        assert_eq!(result.report.issues[0].code, "pdf.text_outlined");
        let ReadContent::Vector(document) = result.content else {
            panic!()
        };
        let actual = &document.svg_layers().next().unwrap().source;
        let a = rasterize_svg(expected, 100, 80).unwrap();
        let b = rasterize_svg(actual, 100, 80).unwrap();
        let error: u64 = a
            .pixels
            .iter()
            .zip(&b.pixels)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        assert!(
            error < 100 * 80,
            "{name} text pixel error {error}: {actual}"
        );
    }
}

#[test]
fn japanese_cff_cids_preserve_horizontal_and_vertical_positions_in_pdf_and_ai() {
    for (name, bytes, expected) in [
        (
            "japanese-horizontal",
            include_bytes!("../../lumapaint-formats/tests/fixtures/lp-japanese-cid-horizontal.pdf")
                .as_slice(),
            r#"<svg width="100" height="80"><g transform="matrix(1 0 0 -1 0 80)"><path d="M12 40L26 40L19 54Z M32 40H46V54H32Z"/></g></svg>"#,
        ),
        (
            "japanese-vertical",
            include_bytes!("../../lumapaint-formats/tests/fixtures/lp-japanese-cid-vertical.pdf")
                .as_slice(),
            r#"<svg width="100" height="80"><g transform="matrix(1 0 0 -1 0 80)"><path d="M42 47.4L56 47.4L49 61.4Z M42 27.4H56V41.4H42Z"/></g></svg>"#,
        ),
    ] {
        for format in [FormatId::Pdf, FormatId::Illustrator] {
            let result = read_document(
                format,
                name.into(),
                bytes,
                ReadOptions {
                    raster_dpi: 72,
                    allow_lossy: true,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(result
                .report
                .issues
                .iter()
                .any(|issue| issue.code == "pdf.text_outlined"));
            assert!(result
                .report
                .issues
                .iter()
                .all(|issue| matches!(issue.code, "pdf.text_outlined" | "ai.pdf_compatible_only")));
            let ReadContent::Vector(document) = result.content else {
                panic!()
            };
            let actual = &document.svg_layers().next().unwrap().source;
            let golden = rasterize_svg(expected, 100, 80).unwrap();
            let rendered = rasterize_svg(actual, 100, 80).unwrap();
            let error: u64 = golden
                .pixels
                .iter()
                .zip(&rendered.pixels)
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum();
            assert!(
                error < 8000,
                "{name} {format:?}: pixel error {error}: {actual}"
            );
        }
    }
}

#[test]
fn pdf_vertical_cid_metrics_spacing_and_adjustments_preserve_pixels() {
    use lopdf::{dictionary, Document, Object};
    for (metrics, content, expected) in [
        (
            None,
            "BT /F 20 Tf 1 0 0 1 40 70 Tm <00010002> Tj ET",
            r##"<svg width="100" height="80"><g transform="matrix(1 0 0 -1 0 80)"><path d="M32 52.4H40V66.4H32Z M34 32.4L44 32.4L39 46.4Z"/></g></svg>"##,
        ),
        (
            Some(
                dictionary! {"DW2"=>vec![900.into(),(-1100).into()],"W2"=>vec![1.into(),vec![(-1200).into(),300.into(),800.into()].into(),2.into(),2.into(),(-900).into(),400.into(),850.into()]},
            ),
            "BT /F 20 Tf 1 0 0 1 40 70 Tm 50 Tz 2 Tc 3 Ts [<0001> 100 <00020003>] TJ ET",
            r##"<svg width="100" height="80"><g transform="matrix(1 0 0 -1 0 80)"><path d="M37 57H41V71H37Z M36 32L41 32L38.5 46Z M34.5 15H41.5V29H34.5Z"/></g></svg>"##,
        ),
    ] {
        let mut pdf = Document::load_mem(&text_pdf(content, true)).unwrap();
        let root_id = pdf
            .objects
            .iter()
            .find_map(|(id, o)| {
                o.as_dict()
                    .ok()
                    .filter(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0"))
                    .map(|_| *id)
            })
            .unwrap();
        let root = pdf.get_object_mut(root_id).unwrap().as_dict_mut().unwrap();
        root.set("Encoding", "Identity-V");
        let child = root.get(b"DescendantFonts").unwrap().as_array().unwrap()[0]
            .as_reference()
            .unwrap();
        if let Some(metrics) = metrics {
            let dict = pdf.get_object_mut(child).unwrap().as_dict_mut().unwrap();
            for (key, value) in metrics.iter() {
                dict.set(key.clone(), value.clone());
            }
        }
        let mut bytes = Vec::new();
        pdf.save_to(&mut bytes).unwrap();
        let result = read_document(
            FormatId::Pdf,
            "vertical".into(),
            &bytes,
            ReadOptions {
                raster_dpi: 72,
                allow_lossy: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.report.issues.len(), 1);
        let ReadContent::Vector(document) = result.content else {
            panic!()
        };
        let actual = &document.svg_layers().next().unwrap().source;
        let a = rasterize_svg(expected, 100, 80).unwrap();
        let b = rasterize_svg(actual, 100, 80).unwrap();
        let error: u64 = a
            .pixels
            .iter()
            .zip(&b.pixels)
            .map(|(a, b)| u64::from(a.abs_diff(*b)))
            .sum();
        assert!(error < 8000, "vertical pixel error {error}: {actual}");
    }
}
