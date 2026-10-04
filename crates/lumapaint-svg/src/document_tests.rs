use crate::attach;
use lumapaint_core::{bezier, document::Document};
use lumapaint_formats::native::NativeDocumentCodec;
fn source(doc: &Document) -> &str {
    &doc.svg_layers().next().unwrap().source
}
fn document(width: u32, height: u32) -> Document {
    let bytes=serde_json::to_vec(&serde_json::json!({"format":"LumaPaint", "version":1,"width":width,"height":height,"layerVisible":true,"strokes":[]})).unwrap();
    let mut doc = Document::decode(&bytes).unwrap();
    attach(&mut doc);
    doc
}
fn reload(bytes: &[u8]) -> Document {
    let mut doc = Document::decode(bytes).unwrap();
    attach(&mut doc);
    doc
}
fn imported() -> Document {
    let mut doc = document(200, 200);
    doc.import_svg("SVG".into(),r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 100 100"><defs><linearGradient id="g"><stop offset="0" stop-color="red"/></linearGradient><clipPath id="c"><path d="M0 0H100V100H0Z"/></clipPath></defs><g transform="translate(5 10)" clip-path="url(#c)"><path id="art" fill="url(#g)" d="M0 0H40V40H0Z M10 10V30H30V10Z"/><path id="other" d="M50 50L80 80"/></g></svg>"##.into()).unwrap();
    doc
}
#[test]
fn shape_and_css_use_edits_are_one_transaction_and_survive_reload() {
    let mut doc = document(160, 100);
    doc.import_svg("Instances".into(), r##"<svg width="160" height="100"><style>rect {x:5px;y:5px;width:20px;height:20px} #hidden{display:none}</style><defs><g id="model"><rect/><ellipse cx="40" cy="15" rx="10" ry="8"/></g></defs><use id="one" href="#model" x="10"/><use id="two" href="#model" x="80"/><path id="hidden" d="M0 0L50 50"/></svg>"##.into()).unwrap();
    let before = doc.direct_objects();
    assert_eq!(before.len(), 4);
    let ids: Vec<_> = before.iter().map(|(_, o)| o.id.clone()).collect();
    doc.select_direct_objects(ids[..2].to_vec()).unwrap();
    let original = doc.encode().unwrap();
    let controls: Vec<_> = ids[..2].iter().map(|id| (id.clone(), 0)).collect();
    doc.move_vector_controls(&controls, [2., 3.], false)
        .unwrap();
    let changed = doc.encode().unwrap();
    let mut saved = reload(&changed);
    let after = saved.direct_objects();
    assert_eq!(
        after.iter().map(|(_, o)| o.id.clone()).collect::<Vec<_>>(),
        ids
    );
    for i in 0..2 {
        let p = bezier::world_point(&before[i].1, before[i].1.control_points[0]);
        let q = bezier::world_point(&after[i].1, after[i].1.control_points[0]);
        assert_eq!(q, [p[0] + 2., p[1] + 3.]);
    }
    assert_eq!(before[2].1.control_points, after[2].1.control_points);
    assert_eq!(before[3].1.transform, after[3].1.transform);
    saved
        .set_direct_coordinates(&[(ids[0].clone(), 0)], [30., 40.])
        .unwrap();
    let edited = saved.direct_objects().remove(0).1;
    assert_eq!(
        bezier::world_point(&edited, edited.control_points[0]),
        [30., 40.]
    );
    doc.undo();
    assert_eq!(doc.encode().unwrap(), original);
    doc.redo();
    assert_eq!(doc.encode().unwrap(), changed);
    let unchanged = doc.encode().unwrap();
    assert!(doc
        .move_vector_controls(
            &[(ids[0].clone(), 0), ("missing".into(), 0)],
            [1., 1.],
            false
        )
        .is_err());
    assert_eq!(doc.encode().unwrap(), unchanged);
}
#[test]
fn imported_svg_moves_path_without_losing_xml_paints_clips_or_other_paths() {
    let mut doc = imported();
    let targets = doc.direct_objects();
    assert_eq!(targets.len(), 2);
    let id = targets[0].1.id.clone();
    doc.select_direct_objects(vec![id.clone()]).unwrap();
    let encoded = doc.encode().unwrap();
    let original = source(&doc).to_owned();
    doc.move_vector_controls(&[(id.clone(), 0)], [10., 20.], false)
        .unwrap();
    let changed = source(&doc).to_owned();
    assert!(changed.contains("fill=\"url(#g)\""));
    assert!(changed.contains("clip-path=\"url(#c)\""));
    assert!(changed.contains("id=\"other\" d=\"M50 50L80 80\""));
    assert!(changed.contains("<path d=\"M0 0H100V100H0Z\"/>"));
    assert_eq!(doc.direct_objects()[0].1.control_points[0], [5., 10.]);
    let saved = reload(&doc.encode().unwrap());
    assert_eq!(source(&saved), changed);
    doc.undo();
    assert_eq!(source(&doc), original);
    assert_eq!(doc.encode().unwrap(), encoded);
    doc.redo();
    assert_ne!(source(&doc), original);
}
#[test]
fn imported_live_corners_save_original_path_and_can_restore_after_reload() {
    let mut doc = imported();
    let id = doc.direct_objects()[0].1.id.clone();
    doc.set_direct_corner_radius(&[(id, 3)], 12.).unwrap();
    assert!(source(&doc).contains("data-lumapaint-corners="));
    let mut saved = reload(&doc.encode().unwrap());
    let target = saved.direct_objects().remove(0).1;
    assert_eq!(target.live_corners.as_ref().unwrap().radius, 12.);
    saved
        .set_direct_corner_radius(&[(target.id, 3)], 0.)
        .unwrap();
    assert_eq!(
        saved.direct_objects()[0].1.control_points,
        imported().direct_objects()[0].1.control_points
    );
}
#[test]
fn failed_mixed_edit_is_atomic_and_coordinates_respect_transforms() {
    let mut doc = imported();
    let id = doc.direct_objects()[0].1.id.clone();
    let original = doc.encode().unwrap();
    assert!(doc
        .move_vector_controls(&[(id.clone(), 0), ("missing".into(), 0)], [10., 10.], false)
        .is_err());
    assert_eq!(doc.encode().unwrap(), original);
    doc.set_direct_coordinates(&[(id.clone(), 0)], [50., 70.])
        .unwrap();
    let o = doc.direct_objects()[0].1.clone();
    assert_eq!(bezier::world_point(&o, o.control_points[0]), [50., 70.]);
    let original = doc.encode().unwrap();
    assert!(doc
        .set_direct_coordinates(&[(id, 0)], [f32::INFINITY, 0.])
        .is_err());
    assert_eq!(doc.encode().unwrap(), original);
    let mut project: serde_json::Value = serde_json::from_slice(&doc.encode().unwrap()).unwrap();
    project["svgLayers"][0]["locked"] = serde_json::Value::Bool(true);
    doc = reload(&serde_json::to_vec(&project).unwrap());
    assert!(doc
        .move_vector_controls(&[(o.id, 0)], [1., 1.], false)
        .is_err());
}
#[test]
fn svg_deletion_splits_closed_path_keeps_defs_and_undo_restores_source() {
    let mut doc = imported();
    let id = doc.direct_objects()[0].1.id.clone();
    let before = source(&doc).to_owned();
    doc.delete_vector_anchors(&[(id, 3)]).unwrap();
    let object = doc.direct_objects()[0].1.clone();
    let ranges = bezier::contour_ranges(&object);
    assert!(!ranges[0].2);
    assert!(ranges[1].2);
    assert!(source(&doc).contains("<defs>"));
    doc.undo();
    assert_eq!(source(&doc), before);
}

#[test]
fn hide_css_shapes_and_use_instances_preserves_style_identity_and_survives_reload() {
    use lumapaint_core::document::ObjectVisibilityAction as Action;
    let mut doc = document(160, 100);
    doc.import_svg("Visibility".into(), r##"<svg width="160" height="100"><style>rect {fill:red!important} #already-hidden{display:none}</style><defs><rect id="model" width="20" height="20"/></defs><rect id="plain" width="20" height="20" style="fill:blue;stroke:black"/><use id="one" href="#model" x="30"/><use id="two" href="#model" x="70"/><rect id="already-hidden" x="100" width="20" height="20"/></svg>"##.into()).unwrap();
    let original = doc.direct_objects();
    assert_eq!(original.len(), 3);
    let ids: Vec<_> = original.iter().map(|(_, o)| o.id.clone()).collect();
    doc.select_direct_objects(ids[..2].to_vec()).unwrap();
    let before = doc.encode().unwrap();
    doc.hide_objects(Action::Selection).unwrap();
    assert!(doc.has_hidden_objects());
    assert!(doc.selected_vector_ids().is_empty());
    assert_eq!(doc.direct_objects().len(), 1);
    assert_eq!(
        crate::edit::targets(source(&doc), "render", [160., 100.]).len(),
        1,
        "{}",
        source(&doc)
    );
    assert_eq!(doc.direct_objects()[0].1.id, ids[2]);
    assert!(doc.select_direct_objects(vec![ids[0].clone()]).is_err());
    let mut saved = reload(&doc.encode().unwrap());
    assert_eq!(saved.direct_objects().len(), 1);
    saved.hide_objects(Action::ShowAll).unwrap();
    let shown = saved.direct_objects();
    assert_eq!(
        shown.iter().map(|(_, o)| o.id.clone()).collect::<Vec<_>>(),
        ids
    );
    for (a, b) in original.iter().zip(&shown) {
        assert_eq!(a.1.transform, b.1.transform);
        assert_eq!(a.1.path.data, b.1.path.data);
    }
    assert!(source(&saved).contains("style=\"fill:blue;stroke:black\""));
    assert!(!source(&saved).contains("data-lumapaint-hidden-style"));
    assert!(source(&saved).contains("#already-hidden{display:none}"));
    assert!(source(&saved).contains("<use id=\"one\""));
    doc.undo();
    assert_eq!(doc.encode().unwrap(), before);
    doc.redo();
    assert_eq!(doc.direct_objects().len(), 1);
}

#[test]
fn imported_gradient_edit_is_independent_atomic_and_survives_history_and_reload() {
    let mut doc = document(160, 100);
    doc.import_svg("Gradients".into(), r##"<svg width="160" height="100"><style>rect {fill:url(#g)}</style><defs><linearGradient id="g"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><rect id="shape" width="25" height="30"/></defs><use id="one" href="#shape" x="10"/><use id="two" href="#shape" x="80"/></svg>"##.into()).unwrap();
    let before = doc.direct_objects();
    assert_eq!(before.len(), 2);
    let id = before[0].1.id.clone();
    let second = before[1].1.fill_gradient.clone();
    doc.select_direct_objects(vec![id.clone()]).unwrap();
    let original = doc.encode().unwrap();
    let mut gradient = before[0].1.fill_gradient.clone().unwrap();
    gradient.stops[0].color = [0, 255, 0, 128];
    gradient.stops[0].midpoint = 0.3;
    gradient.method = lumapaint_core::gradient::GradientMethod::Perceptual;
    doc.set_selected_vector_gradient(std::slice::from_ref(&id), "fill", gradient.clone())
        .unwrap();
    let edited = doc.encode().unwrap();
    let after = doc.direct_objects();
    assert_eq!(after[0].1.id, id);
    let snapshot = doc.snapshot();
    assert_eq!(
        snapshot
            .layers
            .iter()
            .flat_map(|l| &l.objects)
            .find(|o| o.id == id)
            .unwrap()
            .fill_gradient,
        after[0].1.fill_gradient
    );
    assert_eq!(
        after[0].1.fill_gradient.as_ref().unwrap().stops,
        gradient.stops
    );
    assert_eq!(after[1].1.fill_gradient, second);
    assert_eq!(
        reload(&edited).direct_objects()[0].1.fill_gradient,
        after[0].1.fill_gradient
    );
    doc.undo();
    assert_eq!(doc.encode().unwrap(), original);
    doc.redo();
    assert_eq!(doc.encode().unwrap(), edited);
    let unchanged = doc.encode().unwrap();
    assert!(doc
        .set_selected_vector_gradient(&["missing".into()], "fill", gradient)
        .is_err());
    assert_eq!(doc.encode().unwrap(), unchanged);
}

#[test]
fn repeated_svg_stop_edits_replace_private_definitions_without_growing_source() {
    let mut doc = document(100, 100);
    doc.import_svg("Gradient".into(), r##"<s:svg xmlns:s="http://www.w3.org/2000/svg" width="100" height="100"><s:defs><s:linearGradient id="g"><s:stop offset="0" stop-color="red"/><s:stop offset="1" stop-color="blue"/></s:linearGradient></s:defs><s:path id="p" d="M0 0H100V100H0Z" fill="url(#g)"/></s:svg>"##.into()).unwrap();
    let object = doc.direct_objects()[0].1.clone();
    let id = object.id;
    let mut gradient = object.fill_gradient.unwrap();
    doc.select_direct_objects(vec![id.clone()]).unwrap();
    let mut first = 0;
    for i in 0..20 {
        gradient.stops[0].midpoint = 0.3 + i as f32 * 0.01;
        doc.set_selected_vector_gradient(std::slice::from_ref(&id), "fill", gradient.clone())
            .unwrap();
        if i == 0 {
            first = source(&doc).len();
        }
        assert!(source(&doc).len() < first + 1024);
        assert!(doc.direct_objects()[0].1.fill_gradient.is_some());
    }
}
