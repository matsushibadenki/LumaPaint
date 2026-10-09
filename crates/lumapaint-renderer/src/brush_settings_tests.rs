use super::*;
use lumapaint_core::{
    brush_blend::BrushBlendMode,
    document::{Brush, CanvasColor},
};
use lumapaint_formats::native::NativeDocumentCodec;
fn document() -> Document {
    let mut state = Document::default().document_state();
    state.width = 64;
    state.height = 64;
    state.canvas_color = Some(CanvasColor::Transparent);
    Document::from_document_state(state).unwrap()
}
fn pixel(doc: &Document) -> [u8; 4] {
    let mut preview = pixel_paint::PixelPaintPreview::new(doc.dimensions()).unwrap();
    preview.update(doc).unwrap();
    preview.pixels()[(32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4]
        .try_into()
        .unwrap()
}
#[test]
fn flow_percentage_controls_click_coverage_and_zero_flow_never_emits_negative_dabs() {
    let mut doc = document();
    doc.begin(
        Point { x: 32., y: 32. },
        Brush {
            size: 24.,
            flow: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(pixel(&doc)[3], 128);
    for flow in [0., 0.01, 0.5, 1.] {
        let mut doc = document();
        doc.begin(
            Point { x: 8., y: 8. },
            Brush {
                size: 24.,
                flow,
                ..Default::default()
            },
        )
        .unwrap();
        for i in 1..100 {
            doc.extend(Point {
                x: 8. + i as f32 * 0.23,
                y: 8.,
            })
            .unwrap();
            assert!(sampled_raster_dabs(doc.active_paint_stroke().unwrap())
                .unwrap()
                .iter()
                .all(|dab| dab.weight >= 0.));
        }
    }
}
#[test]
fn opacity_and_color_alpha_cap_a_stroke_and_survive_history_and_save() {
    let mut doc = document();
    let brush = Brush {
        size: 24.,
        opacity: 0.5,
        alpha: 0.5,
        color: [255, 0, 0],
        ..Default::default()
    };
    doc.begin(Point { x: 32., y: 32. }, brush).unwrap();
    for _ in 0..4 {
        doc.extend(Point { x: 36., y: 32. }).unwrap();
        doc.extend(Point { x: 32., y: 32. }).unwrap();
    }
    assert_eq!(pixel(&doc), [64, 0, 0, 64]);
    doc.finish();
    assert_eq!(pixel(&doc), [64, 0, 0, 64]);
    let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
    assert_eq!(pixel(&loaded), pixel(&doc));
    doc.begin(Point { x: 32., y: 32. }, brush).unwrap();
    doc.finish();
    assert!(pixel(&doc)[3] > 64);
    doc.undo();
    assert_eq!(pixel(&doc), [64, 0, 0, 64]);
    doc.redo();
    assert!(pixel(&doc)[3] > 64);
}
#[test]
fn flow_accumulates_and_multiply_clear_and_preview_agree() {
    let mut doc = document();
    let brush = Brush {
        size: 24.,
        flow: 0.005,
        color: [128; 3],
        ..Default::default()
    };
    doc.begin(Point { x: 32., y: 32. }, brush).unwrap();
    let first = pixel(&doc)[3];
    for _ in 0..10 {
        doc.extend(Point { x: 36., y: 32. }).unwrap();
        doc.extend(Point { x: 32., y: 32. }).unwrap();
    }
    assert!(pixel(&doc)[3] > first);
    let mut doc = document();
    doc.begin(Point { x: 32., y: 32. }, Brush { flow: 1., ..brush })
        .unwrap();
    doc.finish();
    doc.begin(
        Point { x: 32., y: 32. },
        Brush {
            size: 24.,
            color: [128, 64, 255],
            blend_mode: BrushBlendMode::Multiply,
            ..Default::default()
        },
    )
    .unwrap();
    let mut preview = pixel_paint::PixelPaintPreview::new(doc.dimensions()).unwrap();
    preview.update(&doc).unwrap();
    let before = preview.pixels().to_vec();
    preview.update(&doc).unwrap();
    assert_eq!(preview.pixels(), before);
    assert_eq!(pixel(&doc), [64, 32, 128, 255]);
    doc.finish();
    assert_eq!(pixel(&doc), [64, 32, 128, 255]);
    doc.begin_eraser(
        Point { x: 32., y: 32. },
        Brush {
            size: 24.,
            opacity: 0.5,
            ..Default::default()
        },
        1.,
    )
    .unwrap();
    assert_eq!(pixel(&doc)[3], 128);
}
#[test]
fn smoothing_reduces_alternating_jitter_and_flow_zero_is_empty() {
    let mut doc = document();
    doc.begin(
        Point { x: 2., y: 32. },
        Brush {
            smoothing: 1.,
            flow: 0.,
            ..Default::default()
        },
    )
    .unwrap();
    for i in 1..12 {
        doc.extend(Point {
            x: 2. + i as f32 * 4.,
            y: 32. + if i % 2 == 0 { 8. } else { -8. },
        })
        .unwrap();
    }
    assert_eq!(pixel(&doc), [0; 4]);
    let stroke = doc.active_paint_stroke().unwrap();
    let filtered = smoothed_brush_points(stroke);
    let max = filtered
        .iter()
        .map(|(p, _)| (p.y - 32.).abs())
        .fold(0., f32::max);
    assert!(max < 6., "jitter after smoothing: {max}");
}
