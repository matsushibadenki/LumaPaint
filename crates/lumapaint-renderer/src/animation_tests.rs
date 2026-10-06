use crate::thumbnails::document_pixels;
use lumapaint_core::{
    document::{
        animation::{Interpolation, Keyframe, Property},
        Document,
    },
    vector::VectorObject,
};
use lumapaint_formats::native::NativeDocumentCodec;

fn vector_document() -> (Document, String) {
    let mut state = Document::default().document_state();
    state.width = 64;
    state.height = 64;
    let mut doc = Document::from_document_state(state).unwrap();
    let id = doc.add_vector_layer().unwrap();
    let object:VectorObject=serde_json::from_value(serde_json::json!({
        "id":"square","name":"Square","path":{"data":"M 8 8 H 24 V 24 H 8 Z","fillRule":"nonZero"},
        "transform":[1,0,0,1,0,0],"fill":{"color":[255,0,0,255]},"stroke":null,"strokeWidth":0,"visible":true
    })).unwrap();
    doc.upsert_vector_object(&id, object).unwrap();
    (doc, id)
}
#[test]
fn animation_vector_frames_render_the_expected_positions_and_restore_after_undo() {
    let (mut doc, id) = vector_document();
    let base = document_pixels(&doc).unwrap();
    for (frame, value) in [(0, 0.), (10, 20.)] {
        doc.set_animation_key(
            &id,
            Property::X,
            Keyframe {
                frame,
                value,
                interpolation: Interpolation::Linear,
            },
        )
        .unwrap();
    }
    doc.set_animation_key(
        &id,
        Property::Opacity,
        Keyframe {
            frame: 10,
            value: 50.,
            interpolation: Interpolation::Linear,
        },
    )
    .unwrap();
    let mut preview = doc.clone();
    preview.evaluate_animation(&doc, 10);
    let pixels = document_pixels(&preview).unwrap();
    let at = |x: usize, y: usize| &pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
    assert_eq!(at(10, 10), [255, 255, 255, 255]);
    assert!(at(30, 10)[0] > 245 && at(30, 10)[1] > 110 && at(30, 10)[1] < 145);
    assert_eq!(
        document_pixels(&doc).unwrap(),
        base,
        "Preview must not mutate artwork"
    );
    doc.undo();
    let mut opaque = doc.clone();
    opaque.evaluate_animation(&doc, 10);
    let opaque_pixels = document_pixels(&opaque).unwrap();
    assert_eq!(
        &opaque_pixels[(10 * 64 + 30) * 4..(10 * 64 + 30) * 4 + 4],
        &[255, 0, 0, 255]
    );
    doc.redo();
    let mut again = doc.clone();
    again.evaluate_animation(&doc, 10);
    assert_eq!(document_pixels(&again).unwrap(), pixels);
    let bytes = doc.encode().unwrap();
    let restored = Document::decode(&bytes).unwrap();
    let mut reloaded = restored.clone();
    reloaded.evaluate_animation(&restored, 10);
    assert_eq!(document_pixels(&reloaded).unwrap(), pixels);
}
#[test]
fn animation_pixel_exposures_and_blank_cels_have_identical_saved_and_undo_images() {
    let mut state = Document::default().document_state();
    state.width = 64;
    state.height = 64;
    state.paint_source=Some("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><rect x=\"4\" y=\"4\" width=\"16\" height=\"16\" fill=\"#00ff00\"/></svg>".into());
    let mut doc = Document::from_document_state(state).unwrap();
    let artwork = document_pixels(&doc).unwrap();
    doc.capture_animation_cel("layer-1", 0, false).unwrap();
    doc.blank_animation_cel("layer-1", 5).unwrap();
    let blank = document_pixels(&doc).unwrap();
    assert_ne!(blank, artwork);
    doc.undo();
    assert_eq!(document_pixels(&doc).unwrap(), artwork);
    assert!(!doc.animation().tracks["layer-1"].cels.contains_key(&5));
    doc.redo();
    assert_eq!(document_pixels(&doc).unwrap(), blank);
    let mut preview = doc.clone();
    preview.evaluate_animation(&doc, 4);
    assert_eq!(document_pixels(&preview).unwrap(), artwork);
    preview.evaluate_animation(&doc, 5);
    assert_eq!(document_pixels(&preview).unwrap(), blank);
    assert!(doc.animation().same_picture(0, 4));
    assert!(!doc.animation().same_picture(4, 5));
    let bytes = doc.encode().unwrap();
    let restored = Document::decode(&bytes).unwrap();
    let mut reloaded = restored.clone();
    for frame in [0, 4, 5, 50] {
        preview.evaluate_animation(&doc, frame);
        reloaded.evaluate_animation(&restored, frame);
        assert_eq!(
            document_pixels(&preview).unwrap(),
            document_pixels(&reloaded).unwrap()
        );
    }
}

#[test]
fn animation_imported_svg_and_affine_scale_rotation_keep_original_artwork() {
    let (mut doc, id) = vector_document();
    for (property, value) in [
        (Property::Rotation, 90.),
        (Property::ScaleX, 200.),
        (Property::ScaleY, 50.),
    ] {
        doc.set_animation_key(
            &id,
            property,
            Keyframe {
                frame: 0,
                value,
                interpolation: Interpolation::Linear,
            },
        )
        .unwrap();
    }
    let mut preview = doc.clone();
    preview.evaluate_animation(&doc, 0);
    let matrix = preview.svg_layers().next().unwrap().vector_objects[0].transform;
    for (actual, expected) in matrix.into_iter().zip([0., 2., -0.5, 0., 48., -32.]) {
        assert!((actual - expected).abs() < 0.001);
    }
    let mut state = Document::default().document_state();
    state.width = 64;
    state.height = 64;
    let mut imported = Document::from_document_state(state).unwrap();
    imported.import_svg("Imported".into(),"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><rect x=\"8\" y=\"8\" width=\"16\" height=\"16\" fill=\"red\"/></svg>".into()).unwrap();
    let id = imported.svg_layers().next().unwrap().id.clone();
    imported
        .set_animation_key(
            &id,
            Property::X,
            Keyframe {
                frame: 0,
                value: 20.,
                interpolation: Interpolation::Linear,
            },
        )
        .unwrap();
    let original = imported.svg_layers().next().unwrap().source.clone();
    let mut preview = imported.clone();
    preview.evaluate_animation(&imported, 0);
    let pixels = document_pixels(&preview).unwrap();
    assert_eq!(
        &pixels[(10 * 64 + 30) * 4..(10 * 64 + 30) * 4 + 4],
        &[255, 0, 0, 255]
    );
    assert_eq!(imported.svg_layers().next().unwrap().source, original);
}
