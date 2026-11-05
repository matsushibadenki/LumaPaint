//! CPU fallback regressions: a viewport must be a crop of the complete scene.
use super::*;

fn compare_crop(source: &str, dimensions: [u32; 2], zoom: f32, pan: [f32; 2]) {
    let size = [192, 160];
    let rect = [pan[0], pan[1], size[0] as f32 / zoom, size[1] as f32 / zoom];
    let left = (pan[0] * zoom).ceil().max(0.) as usize + 64;
    let top = (pan[1] * zoom).ceil().max(0.) as usize + 64;
    let w = ((dimensions[0] as f32 - pan[0]) * zoom)
        .ceil()
        .max(size[0] as f32) as usize
        + left
        + 64;
    let h = ((dimensions[1] as f32 - pan[1]) * zoom)
        .ceil()
        .max(size[1] as f32) as usize
        + top
        + 64;
    let full = rasterize_svg_workspace(
        source,
        dimensions,
        [w as u32, h as u32],
        [
            rect[0] - left as f32 / zoom,
            rect[1] - top as f32 / zoom,
            w as f32 / zoom,
            h as f32 / zoom,
        ],
    )
    .unwrap();
    let actual = rasterize_svg_workspace(source, dimensions, size, rect).unwrap();
    let mut total = 0u64;
    let mut max = 0;
    for y in 0..size[1] as usize {
        for x in 0..size[0] as usize * 4 {
            let delta =
                actual[y * size[0] as usize * 4 + x].abs_diff(full[((y + top) * w + left) * 4 + x]);
            total += u64::from(delta);
            max = max.max(delta);
        }
    }
    let mean = total as f64 / actual.len() as f64;
    eprintln!("cpu crop zoom={zoom} pan={pan:?} mean={mean:.6} max={max}");
    assert!(
        mean < 0.02 && max <= 8,
        "viewport differs from full composite"
    );
}

#[test]
fn nested_effect_viewport_preserves_coordinates_and_alpha() {
    let source = r##"<svg width="320" height="240"><defs>
<clipPath id="clip"><path d="M0 0H320V240H0Z M90 60V130H150V60Z" clip-rule="evenodd"/></clipPath>
<linearGradient id="g"><stop stop-color="#ff4000" stop-opacity=".2"/><stop offset="1" stop-color="#2080ff" stop-opacity=".9"/></linearGradient>
<mask id="m" mask-type="alpha"><g opacity=".8"><rect width="320" height="240" fill="url(#g)"/></g></mask>
<filter id="f" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur in="SourceAlpha" stdDeviation="2" result="blur"/><feOffset dx="4" dy="3" result="offset"/><feMerge><feMergeNode in="offset"/><feMergeNode in="SourceGraphic"/></feMerge></filter>
</defs><g mask="url(#m)"><g filter="url(#f)" opacity=".7"><g clip-path="url(#clip)" opacity=".8" transform="rotate(7 160 120)"><rect width="320" height="240" fill="url(#g)"/><g opacity=".6"><circle cx="180" cy="130" r="65" fill="lime"/><path d="M40 30L280 190L80 200Z" fill="red"/></g></g></g></g></svg>"##;
    for zoom in [1., 2., 4.] {
        for pan in [[40.25, 30.5], [88.75, 64.25], [-12., -8.]] {
            compare_crop(source, [320, 240], zoom, pan);
        }
    }
}

#[test]
#[ignore = "requires the licensed AdobeStock derived fixture"]
fn complex_cpu_composite_crop_comparison() {
    use lumapaint_formats::native::NativeDocumentCodec;
    let doc = lumapaint_core::document::Document::decode(
        &std::fs::read(std::env::var("LUMAPAINT_COMPLEX_DOCUMENT").unwrap()).unwrap(),
    )
    .unwrap();
    let source = &doc.svg_layers().next().unwrap().source;
    for zoom in [0.7, 2., 4.] {
        for pan in [[420., 280.], [440.25, 300.5], [380.75, 260.25]] {
            compare_crop(source, [1040, 715], zoom, pan);
        }
    }
}
