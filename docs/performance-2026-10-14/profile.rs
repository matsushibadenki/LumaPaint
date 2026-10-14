//! Core transaction microbenchmark. This is not an application FPS benchmark.
//! cargo run --release -p lumapaint-core --example bench_translation -- --seconds 60 --runs 5
use lumapaint_core::{
    document::{vector_svg, Document},
    vector::{VectorObject, VectorPaint, VectorPath},
};
use std::time::Instant;
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
fn report(count: usize, run: usize, name: &str, mut samples: Vec<f64>) {
    samples.sort_by(f64::total_cmp);
    let p = |percent: usize| samples[((samples.len() - 1) * percent).div_ceil(100)];
    let over = |limit: f64| {
        samples.iter().filter(|&&t| t > limit).count() as f64 * 100. / samples.len() as f64
    };
    println!(
        "{count},{run},{name},{},{:.3},{:.3},{:.3},{:.3},{:.6},{:.6}",
        samples.len(),
        p(50),
        p(95),
        p(99),
        samples.last().unwrap(),
        over(8333.33),
        over(16666.67)
    );
}
fn main() {
 let args: Vec<String> = std::env::args().collect();
 let count: usize = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(4096);
 let phase = args.get(2).map(String::as_str).unwrap_or("restore");
 let doc = fixture(count);
 let state = doc.document_state();
 let json = serde_json::to_vec(&state).unwrap();
 let mut samples = vec![];
 for _ in 0..std::env::var("LUMA_SAMPLES").ok().and_then(|v| v.parse().ok()).unwrap_or(200) {
  let input = if phase == "restore" { Some(state.clone()) } else { None };
  let t = Instant::now();
  match phase {
   "restore" => { let value = Document::from_document_state(input.unwrap()).unwrap(); samples.push(t.elapsed().as_secs_f64()*1e6); std::hint::black_box(value); },
   "snapshot" => { let value = doc.snapshot(); samples.push(t.elapsed().as_secs_f64()*1e6); std::hint::black_box(value); },
   "decode" => { let value: lumapaint_core::document::DocumentState = serde_json::from_slice(&json).unwrap(); samples.push(t.elapsed().as_secs_f64()*1e6); std::hint::black_box(value); },
   "validate" => { for o in &state.svg_layers[0].vector_objects { o.validate().unwrap(); } samples.push(t.elapsed().as_secs_f64()*1e6); },
   "svg" => { let value=vector_svg(state.width,state.height,&state.svg_layers[0].vector_objects); samples.push(t.elapsed().as_secs_f64()*1e6); std::hint::black_box(value); },
   _ => panic!("unknown phase"),
  }
 }
 report(count, 0, phase, samples);
}
