//! Filled-area operations; all results return to the portable document model.
use super::skia_paths::SkiaPathEngine;
use lumapaint_core::vector::{
    FillRule, PathfinderOperation as Op, VectorObject, VectorObjectKind, VectorPath,
};
use skia_safe::{Path, PathBuilder, PathFillType, PathOp, PathVerb};

fn boolean(a: &Path, b: &Path, operation: PathOp) -> Result<Path, String> {
    skia_safe::op(a, b, operation).ok_or_else(|| "Pathfinder operation failed".into())
}
fn output(path: Path, source: &VectorObject, strip_stroke: bool) -> Result<VectorObject, String> {
    let mut object = source.clone();
    object.path = VectorPath {
        data: path.to_svg(),
        fill_rule: match path.fill_type() {
            PathFillType::Winding => FillRule::NonZero,
            PathFillType::EvenOdd => FillRule::EvenOdd,
            _ => return Err("Inverse fill is unsupported".into()),
        },
    };
    object.control_points = path.points().iter().map(|p| [p.x, p.y]).collect();
    for gradient in [&mut object.fill_gradient, &mut object.stroke_gradient]
        .into_iter()
        .flatten()
    {
        let bounds =
            lumapaint_core::stroke::path_bounds(&source.path.data).unwrap_or([0., 0., 1., 1.]);
        let g = gradient.geometry_in_bounds(bounds);
        let t = source.transform;
        gradient.geometry = Some([
            t[0] * g[0] + t[2] * g[1],
            t[1] * g[0] + t[3] * g[1],
            t[0] * g[2] + t[2] * g[3],
            t[1] * g[2] + t[3] * g[3],
            t[0] * g[4] + t[2] * g[5] + t[4],
            t[1] * g[4] + t[3] * g[5] + t[5],
        ]);
    }
    object.transform = [1., 0., 0., 1., 0., 0.];
    object.kind = VectorObjectKind::Compound;
    object.text = None;
    object.group_path.clear();
    object.clipping_group = None;
    object.image_frame = None;
    object.live_corners = None;
    object.rectangle_radii = None;
    object.bounds_reset = false;
    if strip_stroke {
        object.stroke = None;
        object.stroke_gradient = None;
    }
    object.validate()?;
    Ok(object)
}
// Groups are logical operands; clipping masks contribute only their clipped contents.
fn normalize_operands(objects: &[VectorObject]) -> Result<Vec<VectorObject>, String> {
    let mut groups: Vec<(String, Path, VectorObject)> = Vec::new();
    for object in objects.iter().filter(|o| o.clipping_group.is_none()) {
        if object
            .image_frame
            .as_ref()
            .is_some_and(|f| f.image.is_some())
        {
            return Err(
                "Raster image contents cannot participate in filled path operations".into(),
            );
        }
        let mut path = if object.text.is_some() {
            let mut source = object.clone();
            source.group_path.clear();
            let svg = lumapaint_core::document::vector_svg(1024, 1024, &[source]);
            let outlines = crate::text_outlines::outline(&svg, object)?;
            let mut result = Path::default();
            for glyph in outlines {
                result = boolean(
                    &result,
                    &SkiaPathEngine::transformed(&glyph)?,
                    PathOp::Union,
                )?;
            }
            result
        } else {
            SkiaPathEngine::transformed(object)?
        };
        for mask in objects.iter().filter(|m| {
            m.clipping_group
                .as_ref()
                .is_some_and(|g| object.group_path.contains(g))
        }) {
            path = boolean(
                &path,
                &SkiaPathEngine::transformed(mask)?,
                PathOp::Intersect,
            )?;
        }
        let key = object.group_path.first().unwrap_or(&object.id).clone();
        if let Some((_, accumulated, style)) = groups.iter_mut().find(|(id, _, _)| id == &key) {
            *accumulated = boolean(accumulated, &path, PathOp::Union)?;
            *style = object.clone();
        } else {
            groups.push((key, path, object.clone()));
        }
    }
    // Reject incomplete clip selections instead of silently changing their meaning.
    for mask in objects.iter().filter(|m| m.clipping_group.is_some()) {
        if !objects.iter().any(|o| {
            o.group_path.contains(mask.clipping_group.as_ref().unwrap())
                && o.clipping_group.is_none()
        }) {
            return Err("Select the clipping group's contents together with its mask".into());
        }
    }
    groups
        .into_iter()
        .map(|(_, path, style)| output(path, &style, false))
        .collect()
}
pub fn compute(objects: &[VectorObject], operation: Op) -> Result<Vec<VectorObject>, String> {
    if !(2..=64).contains(&objects.len()) {
        return Err("Select 2 to 64 shapes".into());
    }
    let normalized = normalize_operands(objects)?;
    let objects = normalized.as_slice();
    if objects.len() < 2 {
        return Err("Select at least two independent shapes or groups".into());
    }
    let paths = objects
        .iter()
        .map(SkiaPathEngine::transformed)
        .collect::<Result<Vec<_>, _>>()?;
    let mut regions: Vec<(Path, usize)> = Vec::new();
    match operation {
        Op::Unite | Op::Intersect | Op::Exclude | Op::MinusFront | Op::MinusBack => {
            let (mut result, style) = if operation == Op::MinusBack {
                (paths.last().unwrap().clone(), paths.len() - 1)
            } else {
                (
                    paths[0].clone(),
                    if operation == Op::MinusFront {
                        0
                    } else {
                        paths.len() - 1
                    },
                )
            };
            let op = match operation {
                Op::Unite => PathOp::Union,
                Op::Intersect => PathOp::Intersect,
                Op::Exclude => PathOp::XOR,
                _ => PathOp::Difference,
            };
            if operation == Op::MinusBack {
                for path in &paths[..paths.len() - 1] {
                    result = boolean(&result, path, op)?;
                }
            } else {
                for path in &paths[1..] {
                    result = boolean(&result, path, op)?;
                }
            }
            regions.push((result, style));
        }
        Op::Trim | Op::Merge | Op::Crop => {
            let count = paths.len() - usize::from(operation == Op::Crop);
            for i in 0..count {
                let mut path = paths[i].clone();
                for front in &paths[i + 1..count] {
                    path = boolean(&path, front, PathOp::Difference)?;
                }
                if operation == Op::Crop {
                    path = boolean(&path, paths.last().unwrap(), PathOp::Intersect)?;
                }
                if !path.is_empty() {
                    regions.push((path, i));
                }
            }
            if operation == Op::Merge {
                let mut merged: Vec<(Path, usize)> = Vec::new();
                for (path, i) in regions {
                    if let Some((existing, _)) = merged.iter_mut().find(|(_, j)| {
                        objects[*j].fill == objects[i].fill
                            && objects[*j].fill_gradient == objects[i].fill_gradient
                            && objects[*j].opacity == objects[i].opacity
                            && objects[*j].blend_mode == objects[i].blend_mode
                    }) {
                        *existing = boolean(existing, &path, PathOp::Union)?;
                    } else {
                        merged.push((path, i));
                    }
                }
                regions = merged;
            }
        }
        Op::Divide | Op::Outline => {
            for (i, path) in paths.iter().enumerate() {
                let mut remaining = path.clone();
                let mut next = Vec::new();
                for (region, style) in regions {
                    let difference = boolean(&region, path, PathOp::Difference)?;
                    let intersection = boolean(&region, path, PathOp::Intersect)?;
                    remaining = boolean(&remaining, &region, PathOp::Difference)?;
                    if !difference.is_empty() {
                        next.push((difference, style));
                    }
                    if !intersection.is_empty() {
                        next.push((intersection, i));
                    }
                }
                if !remaining.is_empty() {
                    next.push((remaining, i));
                }
                if next.len() > 1024 {
                    return Err("Pathfinder subdivision exceeds 1024 regions".into());
                }
                regions = next;
            }
        }
    }
    let mut results = Vec::new();
    let mut edges = std::collections::HashSet::new();
    for (path, style) in regions.into_iter().rev() {
        if path.is_empty() {
            continue;
        }
        if operation != Op::Outline {
            results.push(output(
                path,
                &objects[style],
                matches!(operation, Op::Divide | Op::Trim | Op::Merge | Op::Crop),
            )?);
            continue;
        }
        let mut start = skia_safe::Point::default();
        let mut last = start;
        for item in path.iter() {
            let p = item.points();
            let mut forward = PathBuilder::new();
            let mut reverse = PathBuilder::new();
            match item.verb() {
                PathVerb::Move => {
                    start = p[0];
                    last = start;
                    continue;
                }
                PathVerb::Close => {
                    if last == start {
                        continue;
                    }
                    forward.move_to(last).line_to(start);
                    reverse.move_to(start).line_to(last);
                }
                PathVerb::Line => {
                    forward.move_to(p[0]).line_to(p[1]);
                    reverse.move_to(p[1]).line_to(p[0]);
                }
                PathVerb::Quad => {
                    forward.move_to(p[0]).quad_to(p[1], p[2]);
                    reverse.move_to(p[2]).quad_to(p[1], p[0]);
                }
                PathVerb::Conic => {
                    forward
                        .move_to(p[0])
                        .conic_to(p[1], p[2], item.conic_weight());
                    reverse
                        .move_to(p[2])
                        .conic_to(p[1], p[0], item.conic_weight());
                }
                PathVerb::Cubic => {
                    forward.move_to(p[0]).cubic_to(p[1], p[2], p[3]);
                    reverse.move_to(p[3]).cubic_to(p[2], p[1], p[0]);
                }
            }
            if item.verb() != PathVerb::Close {
                last = *p.last().unwrap();
            }
            let segment = forward.detach();
            let key = std::cmp::min(segment.to_svg(), reverse.detach().to_svg());
            if !edges.insert(key) {
                continue;
            }
            let mut object = output(segment, &objects[style], true)?;
            object.stroke = object.fill.take();
            object.stroke_width = 1.;
            object.stroke_style = Default::default();
            object.kind = VectorObjectKind::Path;
            object.validate()?;
            results.push(object);
            if results.len() > 4096 {
                return Err("Pathfinder outline exceeds 4096 edges".into());
            }
        }
    }
    results.reverse();
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::{document::Document, vector::VectorPaint};
    use lumapaint_formats::native::NativeDocumentCodec;
    fn shape(id: &str, x: f32, color: [u8; 4]) -> VectorObject {
        VectorObject {
            id: id.into(),
            name: id.into(),
            opacity: 1.,
            blend_mode: "normal".into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: "M0 0H20V20H0Z".into(),
                fill_rule: FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., x, 0.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint {
                registration: false,
                color,
            }),
            stroke: None,
            stroke_width: 0.,
            stroke_style: Default::default(),
            live_corners: None,
            rectangle_radii: None,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![],
            text: None,
        }
    }
    fn covers(objects: &[VectorObject], point: (f32, f32)) -> bool {
        objects
            .iter()
            .any(|object| SkiaPathEngine::transformed(object).unwrap().contains(point))
    }
    #[test]
    fn shape_modes_respect_stacking_transforms_and_different_colors() {
        let objects = [
            shape("a", 0., [255, 0, 0, 255]),
            shape("b", 10., [0, 0, 255, 255]),
            shape("c", 15., [0, 255, 0, 255]),
        ];
        for (op, expected) in [
            (Op::Unite, [true, true, true]),
            (Op::MinusFront, [true, false, false]),
            (Op::Intersect, [false, true, false]),
            (Op::Exclude, [true, true, true]),
            (Op::MinusBack, [false, false, true]),
        ] {
            let result = compute(&objects, op).unwrap();
            for (point, inside) in [(5., 10.), (17., 10.), (32., 10.)]
                .into_iter()
                .zip(expected)
            {
                assert_eq!(covers(&result, point), inside, "{op:?} {point:?}");
            }
        }
        assert_eq!(
            compute(&objects, Op::Unite).unwrap()[0].fill,
            objects[2].fill
        );
    }
    #[test]
    fn subdivisions_crop_and_merge_preserve_filled_regions() {
        let objects = [
            shape("a", 0., [255, 0, 0, 255]),
            shape("b", 10., [0, 0, 255, 255]),
        ];
        let divided = compute(&objects, Op::Divide).unwrap();
        assert_eq!(divided.len(), 3);
        for x in [5., 15., 25.] {
            assert_eq!(
                divided
                    .iter()
                    .filter(|object| covers(std::slice::from_ref(object), (x, 10.)))
                    .count(),
                1
            );
        }
        let trimmed = compute(&objects, Op::Trim).unwrap();
        assert_eq!(trimmed.len(), 2);
        assert!(!covers(&trimmed[..1], (15., 10.)));
        let cropped = compute(&objects, Op::Crop).unwrap();
        assert!(covers(&cropped, (15., 10.)));
        assert!(!covers(&cropped, (5., 10.)));
        assert!(!covers(&cropped, (25., 10.)));
        let same = [objects[0].clone(), shape("b", 10., [255, 0, 0, 255])];
        assert_eq!(compute(&same, Op::Merge).unwrap().len(), 1);
    }
    #[test]
    fn outline_returns_unique_open_edges_including_closing_edge() {
        let objects = [
            shape("a", 0., [255, 0, 0, 255]),
            shape("b", 0., [0, 0, 255, 255]),
        ];
        let result = compute(&objects, Op::Outline).unwrap();
        assert_eq!(result.len(), 4);
        for edge in result {
            assert!(edge.fill.is_none());
            assert!(edge.stroke.is_some());
            assert!(!edge.path.data.contains('Z'));
        }
    }
    #[test]
    fn cross_layer_three_shapes_keep_style_stacking_atomic_history_and_native_reload() {
        let mut doc = Document::default();
        let back = doc.add_vector_layer().unwrap();
        let front = doc.add_vector_layer().unwrap();
        let above = doc.add_vector_layer().unwrap();
        doc.upsert_vector_object(&back, shape("a", 0., [255, 0, 0, 255]))
            .unwrap();
        doc.upsert_vector_object(&back, shape("b", 10., [0, 255, 0, 255]))
            .unwrap();
        doc.upsert_vector_object(&front, shape("c", 20., [0, 0, 255, 255]))
            .unwrap();
        doc.upsert_vector_object(&above, shape("untouched", 40., [255, 255, 0, 255]))
            .unwrap();
        doc.select_vector_objects(vec!["a".into(), "b".into(), "c".into()])
            .unwrap();
        let before = doc.encode().unwrap();
        assert!(doc
            .pathfinder_selected_vectors(Op::Unite, |_, _| Err("failed".into()))
            .is_err());
        assert_eq!(doc.encode().unwrap(), before);
        doc.pathfinder_selected_vectors(Op::Unite, compute).unwrap();
        let layers: Vec<_> = doc.svg_layers().collect();
        assert_eq!(layers.last().unwrap().id, above);
        let result = &layers[layers.len() - 2].vector_objects[0];
        assert_eq!(result.fill.unwrap().color, [0, 0, 255, 255]);
        assert!(covers(std::slice::from_ref(result), (5., 10.)));
        let encoded = doc.encode().unwrap();
        let mut restored = Document::decode(&encoded).unwrap();
        assert_eq!(restored.encode().unwrap(), encoded);
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.redo();
        assert_eq!(doc.encode().unwrap(), encoded);
    }
    #[test]
    fn transformed_gradient_is_baked_once_and_merge_keeps_distinct_gradients() {
        let mut a = shape("a", 0., [255, 0, 0, 255]);
        a.transform = [2., 0., 0., 3., 40., 50.];
        let mut gradient:lumapaint_core::gradient::Gradient=serde_json::from_str(r#"{"kind":"linear","angle":0,"aspect":1,"method":"classic","stops":[{"position":0,"color":[0,0,0,255],"midpoint":0.5},{"position":1,"color":[255,255,255,255],"midpoint":0.5}]}"#).unwrap();
        gradient.geometry = Some([20., 0., 0., 20., 0., 0.]);
        a.fill_gradient = Some(gradient.clone());
        let b = shape("b", 10., [255, 0, 0, 255]);
        let result = compute(&[a, b], Op::Trim).unwrap();
        assert!(result.iter().any(|o| o
            .fill_gradient
            .as_ref()
            .is_some_and(|g| g.geometry == Some([40., 0., 0., 60., 40., 50.]))));
        let mut a = shape("a", 0., [255, 0, 0, 255]);
        a.fill_gradient = Some(gradient);
        let b = shape("b", 10., [255, 0, 0, 255]);
        assert_eq!(compute(&[a, b], Op::Merge).unwrap().len(), 2);
    }
    #[test]
    fn document_result_is_atomic_and_undo_redo_restores_selection() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        for object in [
            shape("a", 0., [255, 0, 0, 255]),
            shape("b", 10., [0, 0, 255, 255]),
        ] {
            doc.upsert_vector_object(&layer, object).unwrap();
        }
        doc.select_vector_objects(vec!["a".into(), "b".into()])
            .unwrap();
        let before = doc.snapshot();
        assert!(doc
            .pathfinder_selected_vectors(Op::Divide, |_, _| Err("failed".into()))
            .is_err());
        assert_eq!(doc.snapshot().revision, before.revision);
        doc.pathfinder_selected_vectors(Op::Divide, compute)
            .unwrap();
        assert_eq!(doc.snapshot().selected_vector_objects.len(), 3);
        doc.undo();
        assert_eq!(
            doc.snapshot().selected_vector_objects,
            before.selected_vector_objects
        );
        doc.redo();
        assert_eq!(doc.snapshot().selected_vector_objects.len(), 3);
    }
    #[test]
    fn live_shapes_restore_operands_history_native_and_duplicates() {
        use lumapaint_core::document::CompoundShapeEdit;
        use lumapaint_formats::native::NativeDocumentCodec;
        let edit = |action: &str, operation, operand, translation| CompoundShapeEdit {
            action: action.into(),
            operation,
            operand,
            translation,
        };
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        for o in [
            shape("a", 0., [255, 0, 0, 255]),
            shape("b", 10., [0, 0, 255, 255]),
        ] {
            doc.upsert_vector_object(&layer, o).unwrap();
        }
        doc.select_vector_objects(vec!["a".into(), "b".into()])
            .unwrap();
        doc.make_compound_shape(Op::Unite, compute).unwrap();
        let root = doc.snapshot().selected_vector_objects[0].clone();
        assert_eq!(doc.snapshot().compound_shapes.len(), 1);
        assert!(covers(
            &doc.svg_layers().last().unwrap().vector_objects,
            (25., 10.)
        ));
        let bytes = doc.encode().unwrap();
        let revision = doc.snapshot().revision;
        assert!(doc
            .edit_compound_shape(
                edit("update", None, Some("a".into()), Some([f32::NAN, 0.])),
                compute
            )
            .is_err());
        assert_eq!(doc.snapshot().revision, revision);
        doc.edit_compound_shape(edit("update", Some(Op::Intersect), None, None), compute)
            .unwrap();
        assert!(!covers(
            &doc.svg_layers().last().unwrap().vector_objects,
            (25., 10.)
        ));
        doc.undo();
        assert!(covers(
            &doc.svg_layers().last().unwrap().vector_objects,
            (25., 10.)
        ));
        doc.redo();
        assert_eq!(doc.snapshot().compound_shapes[0].operation, Op::Intersect);
        let mut loaded = Document::decode(&bytes).unwrap();
        loaded.select_vector_objects(vec![root.clone()]).unwrap();
        loaded.duplicate_selected_vectors(40., 0.).unwrap();
        assert_eq!(loaded.snapshot().compound_shapes.len(), 2);
        loaded
            .edit_compound_shape(edit("release", None, None, None), compute)
            .unwrap();
        assert_eq!(loaded.snapshot().compound_shapes.len(), 1);
        assert_eq!(loaded.snapshot().selected_vector_objects.len(), 2);
        let restored = loaded.svg_layers().last().unwrap().vector_objects.clone();
        assert!(covers(&restored, (65., 10.)));
        loaded.undo();
        assert_eq!(loaded.snapshot().compound_shapes.len(), 2);
        loaded.select_vector_objects(vec![root]).unwrap();
        loaded
            .edit_compound_shape(edit("expand", None, None, None), compute)
            .unwrap();
        assert_eq!(loaded.snapshot().compound_shapes.len(), 1);
        loaded.undo();
        assert_eq!(loaded.snapshot().compound_shapes.len(), 2);
        Document::decode(&loaded.encode().unwrap()).unwrap();
    }
    #[test]
    fn groups_clips_and_text_are_logical_filled_operands() {
        use lumapaint_core::document::TextSettings;
        use lumapaint_core::vector::VectorText;
        let mut a = shape("a", 0., [255, 0, 0, 255]);
        a.group_path = vec!["g".into()];
        let mut b = shape("b", 10., [255, 0, 0, 255]);
        b.group_path = vec!["g".into()];
        let c = shape("c", 20., [0, 0, 255, 255]);
        let result = compute(&[a.clone(), b.clone(), c.clone()], Op::Intersect).unwrap();
        assert!(covers(&result, (25., 10.)));
        assert!(!covers(&result, (15., 10.)));
        let mut mask = shape("mask", 5., [255, 0, 0, 255]);
        mask.clipping_group = Some("g".into());
        let result = compute(&[a, b, mask, c], Op::Unite).unwrap();
        assert!(!covers(&result, (2., 10.)));
        assert!(covers(&result, (8., 10.)));
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "O".into(),
                font_size: 40.,
                ..Default::default()
            },
            position: [0., 0.],
            color: [0, 0, 0],
        })
        .unwrap();
        let text = doc
            .svg_layers()
            .flat_map(|l| &l.vector_objects)
            .find(|o| o.text.is_some())
            .unwrap()
            .clone();
        let outline = normalize_operands(std::slice::from_ref(&text)).unwrap();
        assert!(outline[0].text.is_none());
        assert!(!outline[0].path.data.is_empty());
        let mut frame = shape("frame", -10., [0, 0, 0, 255]);
        frame.path.data = "M-10 -100H100V100H-10Z".into();
        frame.control_points = vec![[-10., -100.], [100., 100.]];
        let result = compute(&[text, frame], Op::Intersect).unwrap();
        assert!(boolean(
            &SkiaPathEngine::transformed(&result[0]).unwrap(),
            &SkiaPathEngine::transformed(&outline[0]).unwrap(),
            PathOp::XOR
        )
        .unwrap()
        .is_empty());
    }
    #[test]
    fn nested_live_shapes_can_be_released_and_empty_intersections_remain_editable() {
        use lumapaint_core::document::CompoundShapeEdit;
        use lumapaint_formats::native::NativeDocumentCodec;
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        for o in [
            shape("a", 0., [255, 0, 0, 255]),
            shape("b", 10., [0, 0, 255, 255]),
            shape("c", 100., [0, 255, 0, 255]),
        ] {
            doc.upsert_vector_object(&layer, o).unwrap();
        }
        doc.select_vector_objects(vec!["a".into(), "b".into()])
            .unwrap();
        doc.make_compound_shape(Op::Unite, compute).unwrap();
        let child = doc.snapshot().selected_vector_objects[0].clone();
        doc.select_vector_objects(vec![child.clone(), "c".into()])
            .unwrap();
        doc.make_compound_shape(Op::Intersect, compute).unwrap();
        assert_eq!(doc.snapshot().compound_shapes.len(), 2);
        assert!(!covers(
            &doc.svg_layers().last().unwrap().vector_objects,
            (15., 10.)
        ));
        let parent = doc.snapshot().selected_vector_objects.clone();
        let mut loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        loaded.select_vector_objects(parent).unwrap();
        loaded
            .edit_compound_shape(
                CompoundShapeEdit {
                    action: "release".into(),
                    operation: None,
                    operand: None,
                    translation: None,
                },
                compute,
            )
            .unwrap();
        assert_eq!(loaded.snapshot().compound_shapes.len(), 1);
        loaded.select_vector_objects(vec![child]).unwrap();
        loaded
            .edit_compound_shape(
                CompoundShapeEdit {
                    action: "release".into(),
                    operation: None,
                    operand: None,
                    translation: None,
                },
                compute,
            )
            .unwrap();
        assert!(loaded.snapshot().compound_shapes.is_empty());
        assert_eq!(loaded.svg_layers().last().unwrap().vector_objects.len(), 3);
        Document::decode(&loaded.encode().unwrap()).unwrap();
    }
}
