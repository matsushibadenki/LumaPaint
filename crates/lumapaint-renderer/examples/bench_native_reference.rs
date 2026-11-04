//! Native-vector reference preparation. Host microbenchmark, not UI input latency.
use lumapaint_core::{
    document::{vector_svg, Document},
    vector::{VectorObject, VectorPaint, VectorPath},
};
use lumapaint_renderer::{performance, thumbnails};
use std::{hint::black_box, time::Instant};
fn fixture(count: usize) -> Document {
    let mut document = Document::default();
    document.add_vector_layer().unwrap();
    let prototype = VectorObject {
        live_corners: None,
        rectangle_radii: None,
        opacity: 1.0,
        blend_mode: "normal".into(),
        text: None,
        id: "rectangle-1".into(),
        name: "Rectangle".into(),
        group_path: Vec::new(),
        clipping_group: None,
        bounds_reset: false,
        path: VectorPath {
            data: "M 10 10 H 30 V 40 H 10 Z".into(),
            fill_rule: lumapaint_core::vector::FillRule::NonZero,
        },
        transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        image_frame: None,
        fill_gradient: None,
        stroke_gradient: None,
        fill: Some(VectorPaint {
            registration: false,
            color: [0, 0, 0, 255],
        }),
        stroke: None,
        stroke_style: Default::default(),
        stroke_width: 0.0,
        visible: true,
        kind: lumapaint_core::vector::VectorObjectKind::Rectangle,
        control_points: vec![[10.0, 10.0], [30.0, 40.0]],
    };
    let mut state = document.document_state();
    state.svg_layers[0].vector_objects = (0..count)
        .map(|i| {
            let mut object = prototype.clone();
            object.id = format!("object-{i}");
            object.transform[4] = (i % 32) as f32 * 40.;
            object.transform[5] = (i / 32) as f32 * 40.;
            object
        })
        .collect();
    state.svg_layers[0].source = vector_svg(
        state.width,
        state.height,
        &state.svg_layers[0].vector_objects,
    );
    let mut document = Document::from_document_state(state).unwrap();
    document
        .select_vector_objects(vec!["object-0".into()])
        .unwrap();
    document
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let opt = |name: &str, default: usize| {
        args.windows(2)
            .find(|p| p[0] == name)
            .map_or(default, |p| p[1].parse().unwrap())
    };
    let samples = opt("--samples", 100);
    let runs = opt("--runs", 5);
    assert!(samples > 0 && runs > 0);
    println!("objects,trial,sample,phase,elapsed_us,ui_objects,svg_parses,cache_hits");
    for count in [32, 600, 4096, 10000] {
        let doc = fixture(count);
        let original = serde_json::to_vec(&doc.document_state()).unwrap();
        let mut expected = None;
        for trial in 0..runs {
            for sample in 0..=samples {
                performance::take();
                let start = Instant::now();
                let raster = black_box(thumbnails::scene_pixels(&doc, 256).unwrap());
                let elapsed = start.elapsed().as_secs_f64() * 1e6;
                let stats = performance::take();
                if let Some(pixels) = &expected {
                    assert_eq!(&raster.pixels, pixels);
                } else {
                    assert!(raster.pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
                    if let Some(dir) = std::env::var_os("LUMAPAINT_BENCH_PIXELS") {
                        std::fs::write(
                            std::path::Path::new(&dir).join(format!("native-{count}.rgba")),
                            &raster.pixels,
                        )
                        .unwrap();
                    }
                    expected = Some(raster.pixels);
                }
                let n = |name| stats.counts.get(name).copied().unwrap_or(0);
                println!(
                    "{count},{trial},{sample},{},{elapsed:.3},{},{},{}",
                    if sample == 0 { "first" } else { "warm" },
                    n("document_snapshot_default_stroke_styles_elided"),
                    n("svg_parses"),
                    n("parsed_text_cache_hits")
                );
            }
        }
        assert_eq!(serde_json::to_vec(&doc.document_state()).unwrap(), original);
    }
}
