//! Native GUI fixtures for dense rectangles, multiple selection and multilingual text.
use lumapaint_core::{
    document::{vector_svg, Document},
    vector::{VectorObject, VectorPaint, VectorPath},
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
    use lumapaint_core::document::animation::{Interpolation, Keyframe, Property};
    let output = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&output).unwrap();
    for i in 1..=9 {
        let doc = fixture(100);
        let mut state = doc.document_state();
        state.width = 640;
        state.height = 480;
        for (j, o) in state.svg_layers[0].vector_objects.iter_mut().enumerate() {
            o.transform[4] = (j % 10) as f32 * 50.;
            o.transform[5] = (j / 10) as f32 * 40.;
            o.fill.as_mut().unwrap().color = [(30 + i * 20) as u8, (180 - i * 12) as u8, 180, 255];
        }
        state.svg_layers[0].source = vector_svg(640, 480, &state.svg_layers[0].vector_objects);
        let mut doc = Document::from_document_state(state).unwrap();
        doc.set_animation_settings(24, 48, true).unwrap();
        let layer = doc.svg_layers().next().unwrap().id.clone();
        for (frame, value) in [(0, 0.), (24, 90.), (47, 0.)] {
            doc.set_animation_key(
                &layer,
                Property::X,
                Keyframe {
                    frame,
                    value,
                    interpolation: Interpolation::Linear,
                },
            )
            .unwrap();
        }
        std::fs::write(
            std::path::Path::new(&output).join(format!("Pane-{i}.lumapaint")),
            doc.encode().unwrap(),
        )
        .unwrap();
    }
}
