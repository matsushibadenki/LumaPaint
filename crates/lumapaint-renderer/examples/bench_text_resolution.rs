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
            rasterize_svg(source, size, size).unwrap();
            let start = Instant::now();
            for _ in 0..3 {
                rasterize_svg(source, size, size).unwrap();
            }
            println!(
                "{lines} lines, {size}: {:.1} ms",
                start.elapsed().as_secs_f64() * 1000.0 / 3.0
            );
        }
    }
    compare_frame_cache();
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
    }
    println!(
        "two frames, edit one: full {:.1} ms, cached {:.1} ms; rerasterized {} frames",
        full_ms / 3.0,
        cached_ms / 3.0,
        cache.rasterized_frames
    );
}
