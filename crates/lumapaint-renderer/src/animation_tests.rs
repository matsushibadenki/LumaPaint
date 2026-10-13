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

#[test]
fn rendering_forks_match_full_clones_for_sampling_and_animation() {
    let (mut doc, id) = vector_document();
    doc.set_animation_key(
        &id,
        Property::X,
        Keyframe {
            frame: 8,
            value: 12.,
            interpolation: Interpolation::Linear,
        },
    )
    .unwrap();
    let mut effects = doc.layer_effects(&id);
    effects.enabled = true;
    effects.values[0] = 0.5;
    doc.set_layer_effects(&id, effects).unwrap();
    let mut expected = doc.clone();
    expected.finish();
    assert_eq!(
        document_pixels(&doc.clone_for_rendering()).unwrap(),
        document_pixels(&expected).unwrap()
    );
    let sample = doc
        .clone_stamp_sampling_document(lumapaint_core::clone_stamp::Sample::AllLayers)
        .unwrap();
    assert_eq!(
        document_pixels(&sample).unwrap(),
        document_pixels(&expected).unwrap()
    );
    let mut preview = doc.clone_for_rendering();
    for frame in [0, 4, 8, 20] {
        preview.evaluate_animation(&doc, frame);
        expected.evaluate_animation(&doc, frame);
        assert_eq!(
            document_pixels(&preview).unwrap(),
            document_pixels(&expected).unwrap()
        );
    }
}

#[test]
fn history_free_pixel_preview_matches_committed_image_without_mutating_source() {
    let (mut doc, _) = vector_document();
    let id = doc.add_paint_layer().unwrap();
    doc.select_layer(id.clone()).unwrap();
    let source = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><rect x=\"20\" y=\"20\" width=\"24\" height=\"24\" fill=\"#0000ff\" opacity=\"0.5\"/></svg>";
    let original = document_pixels(&doc).unwrap();
    let before = serde_json::to_value(doc.document_state()).unwrap();
    let preview = doc.clone_stamp_preview_document(source.into()).unwrap();
    let retouch = doc.retouch_preview_document(source.into()).unwrap();
    let mut committed = doc.clone();
    committed.replace_moved_pixels(source.into(), None).unwrap();
    let expected = document_pixels(&committed).unwrap();
    assert_eq!(document_pixels(&preview).unwrap(), expected);
    assert_eq!(document_pixels(&retouch).unwrap(), expected);
    assert_ne!(expected, original);
    assert_eq!(doc.snapshot().layer_id, id);
    assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
}

#[test]
fn selecting_partial_text_layers_does_not_change_artwork_or_scene_generation() {
    use lumapaint_core::document::TextSettings;
    use lumapaint_core::vector::VectorText;
    let mut state = Document::default().document_state();
    state.width = 256;
    state.height = 256;
    let mut doc = Document::from_document_state(state).unwrap();
    doc.set_text_object(TextSettings {
        id: None,
        text: VectorText {
            content: "日本語 English 中文".into(),
            box_width: 220.,
            ..Default::default()
        },
        position: [8., 8.],
        color: [0, 0, 0],
    })
    .unwrap();
    let first = doc.snapshot().text_objects[0].id.clone();
    doc.set_text_object(TextSettings {
        id: None,
        text: VectorText {
            content: "Second frame".into(),
            box_width: 220.,
            ..Default::default()
        },
        position: [8., 100.],
        color: [80, 40, 180],
    })
    .unwrap();
    let second = doc.snapshot().text_objects[1].id.clone();
    let before = document_pixels(&doc).unwrap();
    let revision = doc.revision();
    let cursor = doc.scene_journal().cursor();
    let sources: Vec<_> = doc.svg_layers().map(|layer| layer.source.clone()).collect();
    for selection in [
        vec![first.clone()],
        vec![second.clone()],
        vec![first, second],
        vec![],
    ] {
        doc.select_vector_objects(selection).unwrap();
        assert_eq!(document_pixels(&doc).unwrap(), before);
        assert_eq!(doc.revision(), revision);
        assert_eq!(doc.scene_journal().cursor(), cursor);
        assert_eq!(
            doc.svg_layers()
                .map(|layer| layer.source.clone())
                .collect::<Vec<_>>(),
            sources
        );
    }
}

#[test]
fn incremental_timeline_matches_full_frames_in_both_directions_and_after_edits() {
    let (mut doc, moving) = vector_document();
    let held = doc.add_vector_layer().unwrap();
    let mut object = doc
        .svg_layers()
        .find(|l| l.id == moving)
        .unwrap()
        .vector_objects[0]
        .clone();
    object.id = "held-square".into();
    object.transform[5] = 24.;
    doc.upsert_vector_object(&held, object).unwrap();
    for (layer, property, keys) in [
        (
            &moving,
            Property::X,
            vec![
                (0, 0., Interpolation::Linear),
                (30, 24., Interpolation::Ease),
            ],
        ),
        (
            &held,
            Property::Y,
            vec![
                (0, 0., Interpolation::Hold),
                (20, -8., Interpolation::Linear),
            ],
        ),
    ] {
        for (frame, value, interpolation) in keys {
            doc.set_animation_key(
                layer,
                property,
                Keyframe {
                    frame,
                    value,
                    interpolation,
                },
            )
            .unwrap();
        }
    }
    doc.capture_animation_cel("layer-1", 0, false).unwrap();
    doc.blank_animation_cel("layer-1", 5).unwrap();
    doc.set_motion_clip(
        &held,
        lumapaint_core::motion::Clip {
            id: "late".into(),
            start: 40,
            duration: 10,
            ..Default::default()
        },
    )
    .unwrap();
    let original = serde_json::to_value(doc.document_state()).unwrap();
    let mut preview = doc.clone_for_rendering();
    let mut previous = None;
    for frame in [0, 2, 4, 5, 19, 20, 39, 40, 45, 60, 3, 119, 0] {
        let held_source = preview
            .svg_layers()
            .find(|l| l.id == held)
            .unwrap()
            .source
            .as_ptr();
        let stats =
            preview.evaluate_animation_incremental_with_outlines(&doc, frame, previous, None);
        if previous == Some(0) && frame == 2 {
            assert_eq!(stats.evaluated_tracks, 1);
            assert_eq!(stats.reused_tracks, 2);
            assert_eq!(
                held_source,
                preview
                    .svg_layers()
                    .find(|l| l.id == held)
                    .unwrap()
                    .source
                    .as_ptr()
            );
        }
        let mut full = doc.clone_for_rendering();
        full.evaluate_animation(&doc, frame);
        assert!(
            document_pixels(&preview).unwrap() == document_pixels(&full).unwrap(),
            "frame {frame}"
        );
        previous = Some(frame);
    }
    assert_eq!(
        original,
        serde_json::to_value(doc.document_state()).unwrap()
    );
    doc.set_animation_key(
        &moving,
        Property::X,
        Keyframe {
            frame: 0,
            value: 12.,
            interpolation: Interpolation::Hold,
        },
    )
    .unwrap();
    let stats = preview.evaluate_animation_incremental_with_outlines(&doc, 2, None, None);
    assert_eq!(stats.evaluated_tracks, 3);
    let mut full = doc.clone_for_rendering();
    full.evaluate_animation(&doc, 2);
    assert!(document_pixels(&preview).unwrap() == document_pixels(&full).unwrap());
}
