//! Synthetic mixed scene for native onion/zoom input measurements (not a user work).
use lumapaint_core::document::{
    animation::{OnionScope, OnionSettings},
    Brush, Document, Point,
};
use lumapaint_formats::native::NativeDocumentCodec;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("output .lumapaint path");
    let artwork = args.next();
    let mut document = Document::default();
    document.set_animation_settings(24, 32, true).unwrap();
    for (index, frame) in [0, 8, 16].into_iter().enumerate() {
        document
            .begin(
                Point {
                    x: 100. + index as f32 * 160.,
                    y: 180.,
                },
                Brush {
                    size: 32.,
                    color: [30, 110, 220],
                    ..Default::default()
                },
            )
            .unwrap();
        document.finish();
        document
            .capture_animation_cel("layer-1", frame, false)
            .unwrap();
        document
            .edit_animation_exposure("layer-1", frame, frame, frame + 4)
            .unwrap();
    }
    document.import_raster_layer("Track B".into(), r##"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="300" y="300" width="120" height="90" fill="#dd503c"/></svg>"##.into()).unwrap();
    let layer = document.selected_layer_id().to_owned();
    for frame in [2, 10, 18] {
        document
            .capture_animation_cel(&layer, frame, false)
            .unwrap();
        document
            .edit_animation_exposure(&layer, frame, frame, frame + 4)
            .unwrap();
    }
    let shapes: String = (0..600)
        .map(|i| {
            format!(
        "<rect x='{}' y='{}' width='19' height='19' rx='3' fill='rgb({},{},{})' opacity='.45'/>",
        (i % 40) * 24, (i / 40) * 30, i % 255, (i * 7) % 255, (i * 13) % 255
    )
        })
        .collect();
    document
        .import_svg_as_layer(
            "600 shapes".into(),
            format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='960' height='640'>{shapes}</svg>"
            ),
        )
        .unwrap();
    document.import_svg_as_layer("Text, mask and blur".into(), r##"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><defs><mask id="m"><rect width="920" height="640" fill="white"/></mask><filter id="b"><feGaussianBlur stdDeviation="1"/></filter></defs><g mask="url(#m)"><circle cx="850" cy="520" r="30" fill="#447799" filter="url(#b)"/><g font-family="sans-serif" font-size="28" fill="#263649"><text x="40" y="530">Onion reference input measurement</text><text x="40" y="580">日本語の参照</text><text x="380" y="580">简体中文参考</text></g></g></svg>"##.into()).unwrap();
    if let Some(path) = artwork {
        document
            .import_svg_as_layer("Artwork".into(), std::fs::read_to_string(path).unwrap())
            .unwrap();
    }
    document
        .set_animation_onion(OnionSettings {
            enabled: true,
            scope: OnionScope::Scene,
            ..Default::default()
        })
        .unwrap();
    std::fs::write(path, document.encode().unwrap()).unwrap();
}
