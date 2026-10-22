//! CPU scene-reference preparation, not application input latency or GPU time.
//! cargo run --release --offline -p lumapaint-renderer --example bench_scene_reference
//! Set LUMAPAINT_RENDER_METRICS=1 for a separate diagnostic run.
use lumapaint_core::document::Document;
use lumapaint_renderer::{performance, thumbnails};
use std::{hint::black_box, time::Instant};

fn fixture(case: &str, trial: usize) -> Document {
    let body = match case {
        "artwork" => {
            let source = include_str!("../../../docs/images/illust.svg");
            // XML declarations belong at document start, not inside the fixture viewport.
            source[source.find("<svg ").unwrap()..].to_owned()
        },
        "text_blur" => "<defs><mask id='m'><rect width='960' height='640' fill='white'/></mask><filter id='b'><feGaussianBlur stdDeviation='1'/></filter></defs><g mask='url(#m)'><rect x='30' y='30' width='300' height='200' fill='red' filter='url(#b)'/><text x='30' y='80' font-size='32'>English 日本語 简体中文</text></g>".into(),
        _ => {
            let count = match case {
                "small" => 32,
                "medium" => 600,
                "large" => 4096,
                "stress" => 8192,
                _ => panic!("Unknown case"),
            };
            (0..count).map(|i| format!("<rect x='{}' y='{}' width='19' height='19' rx='3' fill='rgb({},{},{})'/>", (i%40)*24, (i/40%20)*30, i%255, i*7%255, i*13%255)).collect()
        }
    };
    let source = format!("<svg xmlns='http://www.w3.org/2000/svg' width='960' height='640'>{body}<!-- scene-reference-{case}-{trial} --></svg>");
    let mut state = Document::default().document_state();
    state.width = 960;
    state.height = 640;
    let mut document = Document::from_document_state(state).unwrap();
    document.import_svg(case.into(), source).unwrap();
    document
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let option = |key: &str, default: usize| {
        args.windows(2)
            .find(|p| p[0] == key)
            .map_or(default, |p| p[1].parse().unwrap())
    };
    let samples = option("--samples", 100);
    let runs = option("--runs", 5);
    assert!(samples > 0 && runs > 0);
    let only = args
        .windows(2)
        .find(|p| p[0] == "--case")
        .map(|p| p[1].as_str());
    println!(
        "case,trial,sample,phase,elapsed_us,svg_parses,cache_hits,cache_misses,cache_bypasses,parse_us,admissions,admitted_bytes,insertion_retained_bytes,evictions"
    );
    for case in ["small", "medium", "large", "stress", "artwork", "text_blur"] {
        if only.is_some_and(|name| name != case) {
            continue;
        }
        for trial in 0..runs {
            let doc = fixture(case, trial);
            let original = serde_json::to_vec(&doc.document_state()).unwrap();
            let mut expected = None;
            for sample in 0..=samples {
                performance::take();
                let start = Instant::now();
                let raster = black_box(thumbnails::scene_pixels(&doc, 256).unwrap());
                let elapsed = start.elapsed().as_secs_f64() * 1e6;
                let stats = performance::take();
                assert_eq!((raster.width, raster.height), (256, 171));
                if let Some(pixels) = &expected {
                    assert_eq!(&raster.pixels, pixels);
                } else {
                    assert!(raster.pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
                    if let Some(dir) = std::env::var_os("LUMAPAINT_BENCH_PIXELS") {
                        std::fs::write(
                            std::path::Path::new(&dir).join(format!("{case}-{trial}.rgba")),
                            &raster.pixels,
                        )
                        .unwrap();
                    }
                    expected = Some(raster.pixels);
                }
                let count = |name| stats.counts.get(name).copied().unwrap_or(0);
                println!(
                    "{case},{trial},{sample},{},{elapsed:.3},{},{},{},{},{:.3},{},{},{},{}",
                    if sample == 0 { "cold" } else { "warm" },
                    count("svg_parses"),
                    count("parsed_text_cache_hits"),
                    count("parsed_text_cache_misses"),
                    count("parsed_text_cache_bypasses"),
                    stats
                        .cpu_nanoseconds
                        .get("svg_parse_including_text_shaping")
                        .copied()
                        .unwrap_or(0) as f64
                        / 1000.,
                    count("parsed_tree_cache_admissions"),
                    count("parsed_tree_cache_admitted_bytes"),
                    count("parsed_tree_cache_insertion_retained_bytes"),
                    count("parsed_tree_cache_evictions")
                );
            }
            assert_eq!(serde_json::to_vec(&doc.document_state()).unwrap(), original);
        }
    }
}
