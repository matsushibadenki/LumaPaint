//! Panel thumbnail preparation. Host microbenchmark, not UI input latency.
//! cargo run --release --offline -p lumapaint-renderer --example bench_panel_thumbnails -- 30
//! Pass a positive warm sample count. Set BENCH_OUTPUT to an existing directory
//! to save the full PNG sets outside the timed region.
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
    let samples: usize = std::env::args().nth(1).unwrap().parse().unwrap();
    let output = std::env::var_os("BENCH_OUTPUT");
    println!("case,sample,us,ui_objects");
    for case in ["32", "600", "4096", "10000", "artwork", "text"] {
        let mut doc = if let Ok(count) = case.parse() {
            fixture(count)
        } else {
            let mut doc = Document::default();
            let source = if case == "artwork" {
                include_str!("../../../docs/images/illust.svg")
            } else {
                "<svg xmlns='http://www.w3.org/2000/svg' width='960' height='640'><text x='30' y='80' font-size='32'>English 日本語 简体中文</text></svg>"
            };
            doc.import_svg(case.into(), source.into()).unwrap();
            doc
        };
        let mut state = doc.document_state();
        state.width = 960;
        state.height = 640;
        doc = Document::from_document_state(state).unwrap();
        use lumapaint_core::{
            layer_effects::LayerEffects,
            layer_mask::{LayerMask, MaskKind},
            selection::{Selection, SelectionShape},
        };
        let id = doc.artwork_svg_layers().next().unwrap().id.clone();
        for (id, kind) in [
            ("layer-1", MaskKind::Pixel),
            (id.as_str(), MaskKind::Vector),
        ] {
            let mut mask = LayerMask::from_selection(
                kind,
                Some(&Selection::new(
                    SelectionShape::Rectangle,
                    [0., 0., 480., 640.],
                )),
                960,
                640,
            )
            .unwrap();
            mask.enabled = false;
            mask.density = 0.5;
            doc.set_layer_effects(
                id,
                LayerEffects {
                    mask: Some(mask),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let original = serde_json::to_vec(&doc.document_state()).unwrap();
        let mut expected = None;
        for i in 0..=samples {
            performance::take();
            let start = Instant::now();
            let result = black_box(thumbnails::render(&doc).unwrap());
            let us = start.elapsed().as_secs_f64() * 1e6;
            let stats = performance::take();
            assert_eq!(result.masks.len(), 2);
            let value = (result.layers, result.masks, result.channels);
            if let Some(expected) = &expected {
                assert_eq!(&value, expected);
            } else {
                if let Some(dir) = &output {
                    std::fs::write(
                        std::path::Path::new(dir).join(case),
                        serde_json::to_vec(&value).unwrap(),
                    )
                    .unwrap();
                }
                expected = Some(value);
            }
            println!(
                "{case},{i},{us:.3},{}",
                stats
                    .counts
                    .get("document_snapshot_default_stroke_styles_elided")
                    .copied()
                    .unwrap_or(0)
            );
        }
        assert_eq!(serde_json::to_vec(&doc.document_state()).unwrap(), original);
    }
}
