//! Native GUI fixtures for dense rectangles, multiple selection and multilingual text.
use lumapaint_core::{
    document::{vector_svg, Document, TextSettings},
    vector::{VectorObject, VectorPaint, VectorPath, VectorText},
};
use lumapaint_formats::native::NativeDocumentCodec;
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
            data: "M 10 10 H 90 V 90 H 10 Z".into(),
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
        control_points: vec![[10.0, 10.0], [90.0, 90.0]],
    };
    let mut state = document.document_state();
    state.svg_layers[0].vector_objects = (0..count)
        .map(|i| {
            let mut object = prototype.clone();
            object.id = format!("object-{i}");
            object.transform[4] = (i % 32) as f32 * 100.;
            object.transform[5] = (i / 32) as f32 * 100.;
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
    let output = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&output).unwrap();
    for count in [100, 1000] {
        let mut doc = fixture(count);
        let bytes = doc.encode().unwrap();
        let decoded = Document::decode(&bytes).unwrap();
        assert_eq!(
            decoded.svg_layers().next().unwrap().vector_objects.len(),
            count
        );
        std::fs::write(
            std::path::Path::new(&output).join(format!("latency-large-{count}.lumapaint")),
            bytes,
        )
        .unwrap();
    }
    let mut multiple = fixture(1000);
    multiple
        .select_vector_objects(vec!["object-0".into(), "object-1".into()])
        .unwrap();
    std::fs::write(
        std::path::Path::new(&output).join("latency-multiple.lumapaint"),
        multiple.encode().unwrap(),
    )
    .unwrap();
    let effects = fixture(5000);
    let mut state = effects.document_state();
    let layer = &mut state.svg_layers[0];
    for object in &mut layer.vector_objects {
        object.fill.as_mut().unwrap().color = [30, 80, 160, 255];
    }
    layer.source = vector_svg(state.width, state.height, &layer.vector_objects);
    let id = layer.id.clone();
    let mut effects = Document::from_document_state(state).unwrap();
    effects
        .set_layer_effects(
            &id,
            lumapaint_core::layer_effects::LayerEffects {
                enabled: true,
                values: [1., 0., 0., 0., 0., 0., 0., 0., 0., 0.],
                ..Default::default()
            },
        )
        .unwrap();
    let bytes = effects.encode().unwrap();
    assert!(Document::decode(&bytes).unwrap().has_layer_effects(&id));
    std::fs::write(
        std::path::Path::new(&output).join("latency-effects.lumapaint"),
        bytes,
    )
    .unwrap();
    let mut text = Document::default();
    for (i, content) in ["English office", "日本語の文字", "简体中文文本"]
        .into_iter()
        .enumerate()
    {
        text.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: content.into(),
                font_size: 36.0,
                box_width: 500.0,
                ..VectorText::default()
            },
            position: [40.0, 80.0 + i as f32 * 120.0],
            color: [24, 80, 160],
        })
        .unwrap();
    }
    let bytes = text.encode().unwrap();
    assert_eq!(
        Document::decode(&bytes)
            .unwrap()
            .snapshot()
            .text_objects
            .len(),
        3
    );
    std::fs::write(
        std::path::Path::new(&output).join("latency-text.lumapaint"),
        bytes,
    )
    .unwrap();
}
