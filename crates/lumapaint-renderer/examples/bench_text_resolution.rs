use lumapaint_core::document::{
    CanvasColor, Document, DocumentSettings, DocumentUnit, TextSettings,
};
use lumapaint_core::vector::VectorText;
use lumapaint_renderer::frame_cache::FrameRasterCache;
use lumapaint_renderer::prepare_svg_layer;
use lumapaint_renderer::vector::rasterize_svg;
use std::time::Instant;

fn main() {
    for lines in [5, 45] {
        let mut document = Document::default();
        document
            .set_document_settings(DocumentSettings {
                name: "Text quality benchmark".into(),
                width: 2048,
                height: 2048,
                unit: DocumentUnit::Pixels,
                resolution: 72,
                artboards: false,
                canvas_color: CanvasColor::White,
                pixel_aspect_ratio: 1.0,
            })
            .unwrap();
        let line = "Text frame rendering: English 日本語 简体中文 0123456789";
        let text = VectorText {
            content: std::iter::repeat_n(line, lines)
                .collect::<Vec<_>>()
                .join("\n"),
            font_size: 28.0,
            line_height: 1.5,
            box_width: 1700.0,
            box_height: Some(2000.0),
            ..Default::default()
        };
        document
            .set_text_object(TextSettings {
                id: None,
                text,
                position: [60.0, 30.0],
                color: [0, 0, 0],
            })
            .unwrap();
        let source = &document.svg_layers().next().unwrap().source;
        for size in [2048, 1024, 512] {
            // Unique comments force cold parsing without changing visible content.
            let backend = rasterize_svg(source, size, size).unwrap().backend;
            let mut cold = 0.0;
            for revision in 0..3 {
                let changed = format!("{source}<!-- benchmark {size}/{revision} -->");
                let start = Instant::now();
                rasterize_svg(&changed, size, size).unwrap();
                cold += start.elapsed().as_secs_f64() * 1000.0;
            }
            rasterize_svg(source, size, size).unwrap();
            if lumapaint_renderer::performance::enabled() {
                lumapaint_renderer::performance::take();
            }
            let start = Instant::now();
            let mut samples = Vec::with_capacity(20);
            for _ in 0..20 {
                let sample = Instant::now();
                rasterize_svg(source, size, size).unwrap();
                samples.push(sample.elapsed().as_secs_f64() * 1000.0);
            }
            println!(
                "{lines} lines, {size}, {backend:?}: cold {:.1} ms, warm {:.1} ms",
                cold / 3.0,
                start.elapsed().as_secs_f64() * 1000.0 / 20.0
            );
            samples.sort_by(f64::total_cmp);
            println!(
                "warm samples=20 median {:.2} ms p95 {:.2} ms p99 {:.2} ms",
                (samples[9] + samples[10]) / 2.0,
                samples[18],
                samples[19]
            );
            if lumapaint_renderer::performance::enabled() {
                println!(
                    "warm-only CPU profile: {:?}",
                    lumapaint_renderer::performance::take()
                );
            }
        }
    }
    compare_frame_cache();
    if lumapaint_renderer::performance::enabled() {
        println!(
            "CPU profile (this benchmark thread): {:?}",
            lumapaint_renderer::performance::take()
        );
    }
}

fn compare_frame_cache() {
    let mut document = Document::default();
    document
        .set_document_settings(DocumentSettings {
            name: "Frame cache benchmark".into(),
            width: 2048,
            height: 2048,
            unit: DocumentUnit::Pixels,
            resolution: 72,
            artboards: false,
            canvas_color: CanvasColor::White,
            pixel_aspect_ratio: 1.0,
        })
        .unwrap();
    let line = "Text frame rendering: English 日本語 简体中文 0123456789";
    for (index, y) in [40.0, 1050.0].into_iter().enumerate() {
        if index == 1 {
            let id = document.svg_layers().next().unwrap().id.clone();
            document.select_layer(id).unwrap();
        }
        document
            .set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: std::iter::repeat_n(line, 20).collect::<Vec<_>>().join("\n"),
                    font_size: 28.0,
                    line_height: 1.5,
                    box_width: 1700.0,
                    box_height: Some(900.0),
                    ..Default::default()
                },
                position: [40.0, y],
                color: [0, 0, 0],
            })
            .unwrap();
    }
    let mut cache = FrameRasterCache::default();
    cache
        .prepare_layer(document.svg_layers().next().unwrap(), 2048, 2048)
        .unwrap();
    let mut full_ms = 0.0;
    let mut cached_ms = 0.0;
    let mut display_cache = FrameRasterCache::default();
    display_cache
        .prepare_display_layer(document.svg_layers().next().unwrap(), 2048, 2048)
        .unwrap();
    let mut display_ms = 0.0;
    let mut display_bytes = 0;
    for revision in 0..3 {
        let first = document.snapshot().text_objects[0].clone();
        let mut settings = TextSettings {
            id: Some(first.id),
            text: first.text,
            position: first.position,
            color: first.color,
        };
        settings.text.content.push_str(&format!(" {revision}"));
        document.set_text_object(settings).unwrap();
        let layer = document.svg_layers().next().unwrap();
        let started = Instant::now();
        let full = prepare_svg_layer(layer, 2048, 2048).unwrap();
        full_ms += started.elapsed().as_secs_f64() * 1000.0;
        let started = Instant::now();
        let cached = cache.prepare_layer(layer, 2048, 2048).unwrap();
        cached_ms += started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(cached.pixels, full.pixels);
        if lumapaint_renderer::performance::enabled() {
            lumapaint_renderer::performance::take();
        }
        let started = Instant::now();
        let prepared = display_cache
            .prepare_display_layer(layer, 2048, 2048)
            .unwrap();
        display_ms += started.elapsed().as_secs_f64() * 1000.0;
        display_bytes = prepared.pixel_bytes();
        if lumapaint_renderer::performance::enabled() {
            let stats = lumapaint_renderer::performance::take();
            if matches!(
                &prepared,
                lumapaint_renderer::frame_cache::PreparedDisplayLayer::Text(_)
            ) {
                assert_eq!(
                    stats
                        .counts
                        .get("full_canvas_allocations")
                        .copied()
                        .unwrap_or(0),
                    0
                );
                assert_eq!(
                    stats
                        .cpu_nanoseconds
                        .get("text_frame_cpu_composite")
                        .copied()
                        .unwrap_or(0),
                    0
                );
            }
            println!("retained preparation-only profile revision {revision}: {stats:?}");
        }
    }
    println!(
        "two frames, edit one: full {:.1} ms, cached {:.1} ms; rerasterized {} frames",
        full_ms / 3.0,
        cached_ms / 3.0,
        cache.rasterized_frames
    );
    println!("retained frame preparation {:.1} ms; payload {} bytes vs {} full-canvas bytes (GPU reuse measured separately)", display_ms / 3.0, display_bytes, 2048 * 2048 * 4);
}
