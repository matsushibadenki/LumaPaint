//! Skia PathOps adapter; callers exchange only portable SVG path data and fill rules.
use lumapaint_core::vector::{FillRule, PathOperation, VectorPath, VectorPathEngine};
use skia_safe::{Path, PathFillType, PathOp};

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

#[cfg(test)]
mod tests {
    use super::*;
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
}
