//! Independent CPU polygon adapter for the portable filled-path contract.
//! Cubics are adaptively flattened at 0.01 document units; this is an explicit approximation.
use i_overlay::{
    core::{fill_rule::FillRule as Rule, overlay_rule::OverlayRule},
    float::single::SingleFloatOverlay,
};
use lumapaint_core::vector::{FillRule, PathOperation, VectorPath, VectorPathEngine};
use std::fmt::Write;
pub struct PortablePathEngine;
fn contours(path: &VectorPath) -> Result<Vec<Vec<[f64; 2]>>, String> {
    if path.data.len() > 1024 * 1024 {
        return Err("Path exceeds 1 MiB".into());
    }
    if path.data.trim().is_empty() {
        return Ok(vec![]);
    }
    let source = lumapaint_core::bezier::cubic_contours(&path.data)?;
    let mut result = Vec::new();
    let mut count = 0;
    fn flatten(c: [[f32; 2]; 4], depth: u8, out: &mut Vec<[f64; 2]>) -> Result<(), String> {
        let a = c[0];
        let b = c[3];
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]);
        let flat = if len < 1e-6 {
            c[1..3]
                .iter()
                .all(|p| (p[0] - a[0]).hypot(p[1] - a[1]) < 0.01)
        } else {
            c[1..3].iter().all(|p| {
                let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (len * len)).clamp(0., 1.);
                (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1]) < 0.01
            })
        };
        if flat {
            out.push(b.map(f64::from));
        } else {
            if depth >= 20 {
                return Err("Path subdivision limit".into());
            }
            let mid = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
            let p = mid(c[0], c[1]);
            let q = mid(c[1], c[2]);
            let r = mid(c[2], c[3]);
            let s = mid(p, q);
            let t = mid(q, r);
            let u = mid(s, t);
            flatten([c[0], p, s, u], depth + 1, out)?;
            flatten([u, t, r, c[3]], depth + 1, out)?;
        }
        if out.len() > 65536 {
            return Err("Path exceeds point limit".into());
        }
        Ok(())
    }
    for (points, _) in source {
        if points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() >= 100_000.)
        {
            return Err("Invalid vector coordinates".into());
        }
        let mut p = vec![points[0].map(f64::from)];
        for c in points.windows(4).step_by(3) {
            flatten(c.try_into().unwrap(), 0, &mut p)?;
        }
        if p.last() == p.first() {
            p.pop();
        }
        count += p.len();
        if count > 65536 {
            return Err("Path exceeds point limit".into());
        }
        if p.len() >= 3 {
            result.push(p);
        }
    }
    Ok(result)
}
fn rule(path: &VectorPath) -> Rule {
    if path.fill_rule == FillRule::EvenOdd {
        Rule::EvenOdd
    } else {
        Rule::NonZero
    }
}
impl VectorPathEngine for PortablePathEngine {
    fn combine(
        &self,
        left: &VectorPath,
        right: &VectorPath,
        operation: PathOperation,
    ) -> Result<VectorPath, String> {
        // Resolve each operand's winding independently before combining mixed fill rules.
        let empty: Vec<Vec<[f64; 2]>> = vec![];
        let a = contours(left)?.overlay_as::<i64>(&empty, OverlayRule::Subject, rule(left));
        let b = contours(right)?.overlay_as::<i64>(&empty, OverlayRule::Subject, rule(right));
        let shapes = a.overlay_as::<i64>(
            &b,
            match operation {
                PathOperation::Union => OverlayRule::Union,
                PathOperation::Difference => OverlayRule::Difference,
                PathOperation::Intersection => OverlayRule::Intersect,
                PathOperation::Xor => OverlayRule::Xor,
            },
            Rule::NonZero,
        );
        let mut data = String::new();
        let mut count = 0;
        for contour in shapes.into_iter().flatten() {
            if let Some(first) = contour.first() {
                let _ = write!(data, "M{} {}", first[0], first[1]);
                for p in &contour[1..] {
                    let _ = write!(data, "L{} {}", p[0], p[1]);
                }
                data.push('Z');
            }
            count += contour.len();
            if count > 65536 || data.len() > 1024 * 1024 {
                return Err("Boolean output exceeds portable limits".into());
            }
        }
        Ok(VectorPath {
            data,
            fill_rule: FillRule::NonZero,
        })
    }
    fn contains(&self, path: &VectorPath, point: [f32; 2]) -> Result<bool, String> {
        if point.iter().any(|v| !v.is_finite()) {
            return Err("Invalid query point".into());
        }
        let p = point.map(f64::from);
        let mut winding = 0;
        for contour in contours(path)? {
            for (a, b) in contour
                .iter()
                .zip(contour.iter().cycle().skip(1))
                .take(contour.len())
            {
                let side = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
                if a[1] <= p[1] && b[1] > p[1] && side > 0. {
                    winding += 1;
                }
                if a[1] > p[1] && b[1] <= p[1] && side < 0. {
                    winding -= 1;
                }
            }
        }
        Ok(if path.fill_rule == FillRule::EvenOdd {
            winding % 2 != 0
        } else {
            winding != 0
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn contract(engine: &dyn VectorPathEngine) {
        let path = |d: &str| VectorPath {
            data: d.into(),
            fill_rule: FillRule::EvenOdd,
        };
        let a = path("M0 0H20V20H0Z M5 5H15V15H5Z");
        let b = path("M10 -10H30V10H10Z");
        assert!(!engine.contains(&a, [10., 10.]).unwrap());
        assert!(engine.contains(&a, [2., 2.]).unwrap());
        for op in [
            PathOperation::Union,
            PathOperation::Intersection,
            PathOperation::Difference,
            PathOperation::Xor,
        ] {
            let result = engine.combine(&a, &b, op).unwrap();
            for p in [
                [2., 2.],
                [12., 2.],
                [22., 2.],
                [12., 8.],
                [12., 18.],
                [32., 32.],
            ] {
                let x = engine.contains(&a, p).unwrap();
                let y = engine.contains(&b, p).unwrap();
                let expected = match op {
                    PathOperation::Union => x || y,
                    PathOperation::Intersection => x && y,
                    PathOperation::Difference => x && !y,
                    PathOperation::Xor => x != y,
                };
                assert_eq!(engine.contains(&result, p).unwrap(), expected);
            }
        }
        let empty = engine.combine(&a, &a, PathOperation::Difference).unwrap();
        assert!(!engine.contains(&empty, [2., 2.]).unwrap());
        assert!(engine
            .combine(&path("not-a-path"), &b, PathOperation::Union)
            .is_err());
        assert!(engine.contains(&a, [f32::NAN, 0.]).is_err());
        let c = path("M0 0C0 20 20 20 20 0Z");
        assert!(engine.contains(&c, [10., 5.]).unwrap());
        assert!(!engine.contains(&c, [10., 18.]).unwrap());
    }
    #[test]
    fn portable_engine_passes_filled_area_contract() {
        contract(&PortablePathEngine);
    }
    #[cfg(feature = "skia")]
    #[test]
    fn skia_and_portable_engines_share_the_same_contract_and_curve_membership() {
        use crate::vector::skia_paths::SkiaPathEngine;
        contract(&SkiaPathEngine);
        let a = VectorPath {
            data: "M0 0C0 30 30 30 30 0Z".into(),
            fill_rule: FillRule::NonZero,
        };
        let b = VectorPath {
            data: "M8 -2H20V24H8Z".into(),
            fill_rule: FillRule::EvenOdd,
        };
        for op in [
            PathOperation::Union,
            PathOperation::Difference,
            PathOperation::Intersection,
            PathOperation::Xor,
        ] {
            let p = PortablePathEngine.combine(&a, &b, op).unwrap();
            let s = SkiaPathEngine.combine(&a, &b, op).unwrap();
            for y in [-1., 2., 6., 10., 18., 26.] {
                for x in [-1., 2., 6., 12., 24., 32.] {
                    assert_eq!(
                        PortablePathEngine.contains(&p, [x, y]).unwrap(),
                        SkiaPathEngine.contains(&s, [x, y]).unwrap()
                    );
                }
            }
        }
    }
}
