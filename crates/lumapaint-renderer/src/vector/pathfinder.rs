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
    object.transform = [1., 0., 0., 1., 0., 0.];
    object.kind = VectorObjectKind::Compound;
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
pub fn compute(objects: &[VectorObject], operation: Op) -> Result<Vec<VectorObject>, String> {
    if !(2..=64).contains(&objects.len()) {
        return Err("Select 2 to 64 shapes".into());
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
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint { color }),
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
            .any(|object| Path::from_svg(&object.path.data).unwrap().contains(point))
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
}
