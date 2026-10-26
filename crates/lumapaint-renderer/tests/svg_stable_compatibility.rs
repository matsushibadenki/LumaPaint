use lumapaint_renderer::vector::{rasterize_svg, SvgBackend};

#[test]
fn svg_plus_lighter_adds_the_backdrop_in_the_compatibility_renderer() {
    let raster = rasterize_svg(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#800000"/><g style="mix-blend-mode:plus-lighter"><rect width="8" height="8" fill="#800000"/></g></svg>"##,
        8,
        8,
    )
    .unwrap();
    assert_eq!(raster.backend, SvgBackend::Resvg);
    assert!(raster
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 0, 0, 255]));
}

#[test]
fn svg_lighter_filter_adds_both_inputs() {
    let raster = rasterize_svg(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><defs><filter id="sum" x="0" y="0" width="100%" height="100%" color-interpolation-filters="sRGB"><feFlood flood-color="#800000" result="a"/><feFlood flood-color="#800000" result="b"/><feComposite in="a" in2="b" operator="lighter"/></filter></defs><rect width="8" height="8" filter="url(#sum)"/></svg>"##,
        8,
        8,
    )
    .unwrap();
    assert_eq!(raster.backend, SvgBackend::Resvg);
    assert!(raster
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == [255, 0, 0, 255]));
}

#[test]
fn root_css_transform_keeps_percentage_reference_and_default_origin() {
    let tree = resvg::usvg::Tree::from_str(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" style="transform:translate(10%,0) rotate(90deg)"><rect x="10" y="20" width="10" height="10" fill="red"/></svg>"#,
        &resvg::usvg::Options::default(),
    )
    .unwrap();
    let mut pixels = resvg::tiny_skia::Pixmap::new(100, 100).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    assert_eq!(pixels.pixel(85, 15).unwrap().red(), 255);
    assert_eq!(pixels.pixel(15, 25).unwrap().alpha(), 0);
}
