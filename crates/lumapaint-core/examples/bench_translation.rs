//! Core transaction microbenchmark. This is not an application FPS benchmark.
//! cargo run --release -p lumapaint-core --example bench_translation -- --seconds 60 --runs 5
use lumapaint_core::{
    document::{vector_svg, Document},
    vector::{VectorObject, VectorPaint, VectorPath},
};
use std::time::{Duration, Instant};
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
    let args: Vec<_> = std::env::args().collect();
    let arg = |name: &str, default: &str| {
        args.windows(2)
            .find(|p| p[0] == name)
            .map(|p| p[1].as_str())
            .unwrap_or(default)
            .parse::<f64>()
            .unwrap()
    };
    let duration = Duration::from_secs_f64(arg("--seconds", "0.25"));
    let runs = arg("--runs", "1") as usize;
    assert!(!duration.is_zero() && runs > 0);
    println!("objects,run,phase,samples,p50_us,p95_us,p99_us,max_us,over_8_33ms_percent,over_16_67ms_percent");
    for count in [1000, 4096] {
        let mut document = fixture(count);
        for _ in 0..20 {
            document.move_selected_vectors(10.25, 5.5).unwrap();
            document.undo();
        }
        let canonical = document.svg_layers().next().unwrap().source.clone();
        // Host-side work surrounding an edit can exceed the differential move itself.
        // Time construction only; drop outputs outside each measured interval.
        for phase in ["ui_snapshot", "document_clone", "edit_target"] {
            let mut samples = Vec::new();
            for _ in 0..20 {
                let started = Instant::now();
                if phase == "ui_snapshot" {
                    let value = std::hint::black_box(document.snapshot());
                    samples.push(started.elapsed().as_secs_f64() * 1e6);
                    drop(value);
                } else if phase == "edit_target" {
                    std::hint::black_box((
                        document.selected_layer_id(),
                        document.revision(),
                        document.is_dirty(),
                    ));
                    samples.push(started.elapsed().as_secs_f64() * 1e6);
                } else {
                    let value = std::hint::black_box(document.clone());
                    samples.push(started.elapsed().as_secs_f64() * 1e6);
                    drop(value);
                }
            }
            report(count, 0, phase, samples);
        }

        for run in 1..=runs {
            let (mut moved, mut undone, mut redone) = (vec![], vec![], vec![]);
            let end = Instant::now() + duration;
            while Instant::now() < end {
                let t = Instant::now();
                document.move_selected_vectors(10.25, 5.5).unwrap();
                moved.push(t.elapsed().as_secs_f64() * 1e6);
                let t = Instant::now();
                document.undo();
                undone.push(t.elapsed().as_secs_f64() * 1e6);
                let t = Instant::now();
                document.redo();
                redone.push(t.elapsed().as_secs_f64() * 1e6);
                document.undo();
            }
            assert_eq!(document.svg_layers().next().unwrap().source, canonical);
            report(count, run, "move", moved);
            report(count, run, "undo", undone);
            report(count, run, "redo", redone);
            eprintln!(
                "objects={count} run={run} last_translation={:?}",
                document.translation_metrics()
            );
        }
        let mut saves = vec![];
        let mut loads = vec![];
        for _ in 0..10 {
            let t = Instant::now();
            let json = serde_json::to_vec(&document.document_state()).unwrap();
            saves.push(t.elapsed().as_secs_f64() * 1e6);
            let t = Instant::now();
            let state = serde_json::from_slice(&json).unwrap();
            let loaded = Document::from_document_state(state).unwrap();
            loads.push(t.elapsed().as_secs_f64() * 1e6);
            assert_eq!(loaded.svg_layers().next().unwrap().source, canonical);
        }
        report(count, 0, "native_state_save", saves);
        report(count, 0, "native_state_load", loads);
    }
}
