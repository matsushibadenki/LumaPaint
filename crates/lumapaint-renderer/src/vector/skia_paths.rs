//! Skia PathOps adapter; callers exchange only portable SVG path data and fill rules.
use lumapaint_core::vector::{FillRule, PathOperation, VectorObject, VectorPath, VectorPathEngine};
use skia_safe::{Matrix, Path, PathFillType, PathOp};

pub struct SkiaPathEngine;

fn parse(path: &VectorPath) -> Result<Path, String> {
    if path.data.len() > 1024 * 1024 {
        return Err("Vector path exceeds 1 MiB".into());
    }
    let parsed = Path::from_svg(&path.data).ok_or("Invalid SVG path data")?;
    if !parsed.is_finite() || parsed.count_points() > 65536 {
        return Err("Invalid or excessive vector geometry".into());
    }
    Ok(parsed.with_fill_type(match path.fill_rule {
        FillRule::NonZero => PathFillType::Winding,
        FillRule::EvenOdd => PathFillType::EvenOdd,
    }))
}

impl VectorPathEngine for SkiaPathEngine {
    fn combine(
        &self,
        left: &VectorPath,
        right: &VectorPath,
        operation: PathOperation,
    ) -> Result<VectorPath, String> {
        let result = skia_safe::op(
            &parse(left)?,
            &parse(right)?,
            match operation {
                PathOperation::Union => PathOp::Union,
                PathOperation::Difference => PathOp::Difference,
                PathOperation::Intersection => PathOp::Intersect,
                PathOperation::Xor => PathOp::XOR,
            },
        )
        .ok_or("Skia path operation failed")?;
        let fill_rule = match result.fill_type() {
            PathFillType::Winding => FillRule::NonZero,
            PathFillType::EvenOdd => FillRule::EvenOdd,
            _ => return Err("Inverse fill is not supported by the portable path model".into()),
        };
        let output = VectorPath {
            data: result.to_svg(),
            fill_rule,
        };
        // Enforce the same contract for outputs so they can safely feed another operation.
        parse(&output)?;
        Ok(output)
    }

    fn contains(&self, path: &VectorPath, point: [f32; 2]) -> Result<bool, String> {
        if !point.iter().all(|value| value.is_finite()) {
            return Err("Non-finite vector query point".into());
        }
        Ok(parse(path)?.contains((point[0], point[1])))
    }
}

impl SkiaPathEngine {
    /// Combine filled objects in document coordinates, including their SVG transforms.
    pub fn combine_objects(
        &self,
        back: &VectorObject,
        front: &VectorObject,
        operation: PathOperation,
    ) -> Result<(VectorPath, Vec<[f32; 2]>), String> {
        let transformed = |object: &VectorObject| -> Result<Path, String> {
            let [a, b, c, d, e, f] = object.transform;
            let matrix = Matrix::new_all(a, c, e, b, d, f, 0.0, 0.0, 1.0);
            let path = parse(&object.path)?.with_transform(&matrix);
            if !path.is_finite() || path.count_points() > 65536 {
                return Err("Invalid transformed vector geometry".into());
            }
            Ok(path)
        };
        let result = skia_safe::op(
            &transformed(back)?,
            &transformed(front)?,
            match operation {
                PathOperation::Union => PathOp::Union,
                PathOperation::Difference => PathOp::Difference,
                PathOperation::Intersection => PathOp::Intersect,
                PathOperation::Xor => PathOp::XOR,
            },
        )
        .ok_or("Skia path operation failed")?;
        let fill_rule = match result.fill_type() {
            PathFillType::Winding => FillRule::NonZero,
            PathFillType::EvenOdd => FillRule::EvenOdd,
            _ => return Err("Inverse fill is not supported".into()),
        };
        let points = result
            .points()
            .iter()
            .map(|point| [point.x, point.y])
            .collect();
        let path = VectorPath {
            data: result.to_svg(),
            fill_rule,
        };
        if !path.data.is_empty() {
            parse(&path)?;
        }
        Ok((path, points))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::Document;
    use lumapaint_core::vector::{VectorObjectKind, VectorPaint};
    fn path(data: &str) -> VectorPath {
        VectorPath {
            data: data.into(),
            fill_rule: FillRule::NonZero,
        }
    }

    #[test]
    fn boolean_operations_preserve_coverage_and_inputs() {
        let left = path("M0 0H20V20H0Z");
        let right = path("M10 0H30V20H10Z");
        let original = (left.clone(), right.clone());
        for (operation, expected) in [
            (PathOperation::Union, [true, true, true]),
            (PathOperation::Difference, [true, false, false]),
            (PathOperation::Intersection, [false, true, false]),
            (PathOperation::Xor, [true, false, true]),
        ] {
            let result = SkiaPathEngine.combine(&left, &right, operation).unwrap();
            for (x, inside) in [5., 15., 25.].into_iter().zip(expected) {
                assert_eq!(SkiaPathEngine.contains(&result, [x, 10.]).unwrap(), inside);
            }
        }
        assert_eq!((left, right), original);
    }

    #[test]
    fn supports_holes_curves_and_empty_results() {
        let mut hole = path("M0 0H20V20H0Z M5 5H15V15H5Z");
        hole.fill_rule = FillRule::EvenOdd;
        assert!(!SkiaPathEngine.contains(&hole, [10., 10.]).unwrap());
        assert!(SkiaPathEngine.contains(&hole, [2., 10.]).unwrap());
        let circle = path("M10 0A10 10 0 1 1 10 20A10 10 0 1 1 10 0Z");
        let combined = SkiaPathEngine
            .combine(&circle, &hole, PathOperation::Union)
            .unwrap();
        assert!(SkiaPathEngine.contains(&combined, [10., 10.]).unwrap());
        let empty = SkiaPathEngine
            .combine(&circle, &circle, PathOperation::Difference)
            .unwrap();
        assert!(!SkiaPathEngine.contains(&empty, [10., 10.]).unwrap());
    }

    #[test]
    fn rejects_invalid_geometry_and_queries() {
        assert!(SkiaPathEngine
            .contains(&path("not a path"), [0., 0.])
            .is_err());
        assert!(SkiaPathEngine
            .contains(&path("M0 0L1e99 0"), [0., 0.])
            .is_err());
        assert!(SkiaPathEngine
            .contains(&path("M0 0"), [f32::NAN, 0.])
            .is_err());
        assert!(SkiaPathEngine
            .contains(&path(&" ".repeat(1024 * 1024 + 1)), [0., 0.])
            .is_err());
    }

    #[test]
    fn selected_transformed_shapes_combine_atomically_and_undo() {
        let mut document = Document::default();
        let layer_id = document.add_vector_layer().unwrap();
        let shape = |id: &str, x: f32| VectorObject {
            id: id.into(),
            name: id.into(),
            path: path("M0 0H20V20H0Z"),
            transform: [1.0, 0.0, 0.0, 1.0, x, 0.0],
            fill: Some(VectorPaint {
                color: [20, 90, 180, 255],
            }),
            stroke: None,
            stroke_width: 0.0,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[0.0, 0.0], [20.0, 20.0]],
            text: None,
        };
        document
            .upsert_vector_object(&layer_id, shape("back", 5.0))
            .unwrap();
        document
            .upsert_vector_object(&layer_id, shape("front", 15.0))
            .unwrap();
        document
            .select_vector_objects(vec!["front".into(), "back".into()])
            .unwrap();
        let original = document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .clone();
        document
            .combine_selected_vectors(PathOperation::Difference, |back, front, op| {
                SkiaPathEngine.combine_objects(back, front, op)
            })
            .unwrap();
        let combined = document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap();
        assert_eq!(combined.vector_objects.len(), 1);
        assert_eq!(combined.vector_objects[0].id, "back");
        assert!(SkiaPathEngine
            .contains(&combined.vector_objects[0].path, [10.0, 10.0])
            .unwrap());
        assert!(!SkiaPathEngine
            .contains(&combined.vector_objects[0].path, [20.0, 10.0])
            .unwrap());
        assert_eq!(document.snapshot().selected_vector_objects, ["back"]);
        document.undo();
        let restored = document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap();
        assert_eq!(restored.vector_objects, original.vector_objects);
        assert_eq!(restored.source, original.source);
        assert_eq!(
            document.snapshot().selected_vector_objects,
            ["front", "back"]
        );
        document.redo();
        assert_eq!(
            document
                .svg_layers()
                .find(|layer| layer.id == layer_id)
                .unwrap()
                .vector_objects
                .len(),
            1
        );
        let reopened = Document::decode(&document.encode().unwrap()).unwrap();
        let reopened_object = &reopened
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .vector_objects[0];
        assert_eq!(reopened_object.kind, VectorObjectKind::Compound);
        assert!(SkiaPathEngine
            .contains(&reopened_object.path, [10.0, 10.0])
            .unwrap());

        document.undo();
        let before = document.snapshot().revision;
        let mut incompatible = shape("front", 15.0);
        incompatible.fill = Some(VectorPaint {
            color: [200, 20, 20, 255],
        });
        document
            .upsert_vector_object(&layer_id, incompatible)
            .unwrap();
        let incompatible_revision = document.snapshot().revision;
        assert!(document
            .combine_selected_vectors(PathOperation::Union, |back, front, op| {
                SkiaPathEngine.combine_objects(back, front, op)
            })
            .is_err());
        assert_eq!(document.snapshot().revision, incompatible_revision);
        assert!(incompatible_revision > before);

        document
            .upsert_vector_object(&layer_id, shape("front", 100.0))
            .unwrap();
        document
            .combine_selected_vectors(PathOperation::Intersection, |back, front, op| {
                SkiaPathEngine.combine_objects(back, front, op)
            })
            .unwrap();
        assert!(document
            .svg_layers()
            .find(|layer| layer.id == layer_id)
            .unwrap()
            .vector_objects
            .is_empty());
        assert!(document.snapshot().selected_vector_objects.is_empty());
    }
}
