//! Core preview preparation only; excludes native layout, rasterization and display.
use lumapaint_core::document::{Document, TextSettings};
use lumapaint_core::vector::VectorText;
use std::{hint::black_box, time::Instant};
fn main() {
    for frames in [1, 8] {
        let mut doc = Document::default();
        for i in 0..frames {
            doc.set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: std::iter::repeat_n("日本語 English 简体中文 0123456789", 10)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    box_height: Some(400.),
                    ..Default::default()
                },
                position: [20., 20. + i as f32 * 20.],
                color: [0, 0, 0],
            })
            .unwrap();
        }
        let text = doc.snapshot().text_objects.remove(0);
        let settings = TextSettings {
            id: Some(text.id),
            text: text.text,
            position: text.position,
            color: text.color,
        };
        let mut retained = doc.clone();
        let mut old = Vec::new();
        let mut new = Vec::new();
        for step in 0..22 {
            let mut next = settings.clone();
            next.text.content.push_str(&step.to_string());
            let start = Instant::now();
            let mut preview = doc.clone();
            preview.set_text_object(next.clone()).unwrap();
            let old_us = start.elapsed().as_secs_f64() * 1e6;
            let start = Instant::now();
            retained.update_text_preview(next).unwrap();
            let new_us = start.elapsed().as_secs_f64() * 1e6;
            let expected = preview
                .svg_layers()
                .find(|l| {
                    l.vector_objects
                        .iter()
                        .any(|o| Some(&o.id) == settings.id.as_ref())
                })
                .unwrap();
            let actual = retained.svg_layers().find(|l| l.id == expected.id).unwrap();
            assert_eq!(actual.source, expected.source);
            black_box(preview);
            black_box(&retained);
            if step >= 2 {
                old.push(old_us);
                new.push(new_us);
            }
        }
        old.sort_by(f64::total_cmp);
        new.sort_by(f64::total_cmp);
        println!("frames={frames} samples=20 legacy_median_us={:.3} retained_median_us={:.3} legacy_p95_us={:.3} retained_p95_us={:.3}",old[10],new[10],old[18],new[18]);
    }
}
