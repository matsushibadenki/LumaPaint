//! Source-preserving SVG path editing. Definitions, paint, groups and effects stay in XML.
use crate::vector::{FillRule, VectorObject, VectorObjectKind, VectorPaint, VectorPath};
use std::ops::Range;

pub struct Target {
    pub object: VectorObject,
    pub value: Range<usize>,
    pub corners_value: Option<Range<usize>>,
}
fn mul(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
fn coordinate(node: roxmltree::Node<'_, '_>, key: &str, default: f32) -> Option<f32> {
    node.attribute(key)
        .map_or(Some(default), |s| {
            let length: svgtypes::Length = s.parse().ok()?;
            let scale = match length.unit {
                svgtypes::LengthUnit::None | svgtypes::LengthUnit::Px => 1.,
                svgtypes::LengthUnit::In => 96.,
                svgtypes::LengthUnit::Cm => 96. / 2.54,
                svgtypes::LengthUnit::Mm => 96. / 25.4,
                svgtypes::LengthUnit::Pt => 96. / 72.,
                svgtypes::LengthUnit::Pc => 16.,
                _ => return None,
            };
            Some((length.number * scale) as f32)
        })
        .filter(|v| v.is_finite())
}
fn viewport(node: roxmltree::Node<'_, '_>, fallback: [f32; 2]) -> Option<[f32; 6]> {
    let x = coordinate(node, "x", 0.)?;
    let y = coordinate(node, "y", 0.)?;
    let Some(vb) = node.attribute("viewBox") else {
        return Some([1., 0., 0., 1., x, y]);
    };
    let vb: svgtypes::ViewBox = vb.parse().ok()?;
    let root = node.parent_element().is_none();
    if !root && (node.attribute("width").is_none() || node.attribute("height").is_none()) {
        return None;
    }
    let dimension = |key: &str, default: f32| {
        if root && node.attribute(key).is_some_and(|v| v.ends_with('%')) {
            Some(default)
        } else {
            coordinate(node, key, default)
        }
    };
    let w = dimension("width", fallback[0])?;
    let h = dimension("height", fallback[1])?;
    if vb.w <= 0. || vb.h <= 0. || w <= 0. || h <= 0. {
        return None;
    }
    let mut sx = w / vb.w as f32;
    let mut sy = h / vb.h as f32;
    let par = node
        .attribute("preserveAspectRatio")
        .unwrap_or("xMidYMid meet");
    let mut ox = 0.;
    let mut oy = 0.;
    if par != "none" {
        let mut words = par.split_whitespace();
        let align = words.next()?;
        if ![
            "xMinYMin", "xMidYMin", "xMaxYMin", "xMinYMid", "xMidYMid", "xMaxYMid", "xMinYMax",
            "xMidYMax", "xMaxYMax",
        ]
        .contains(&align)
        {
            return None;
        }
        let scale = match words.next().unwrap_or("meet") {
            "meet" => sx.min(sy),
            "slice" => sx.max(sy),
            _ => return None,
        };
        sx = scale;
        sy = scale;
        ox = (w - vb.w as f32 * scale)
            * if align.starts_with("xMid") {
                0.5
            } else if align.starts_with("xMax") {
                1.
            } else {
                0.
            };
        oy = (h - vb.h as f32 * scale)
            * if align.ends_with("YMid") {
                0.5
            } else if align.ends_with("YMax") {
                1.
            } else {
                0.
            };
    }
    Some([
        sx,
        0.,
        0.,
        sy,
        x + ox - vb.x as f32 * sx,
        y + oy - vb.y as f32 * sy,
    ])
}

pub fn targets(source: &str, layer_id: &str, size: [f32; 2]) -> Vec<Target> {
    let Ok(xml) = roxmltree::Document::parse(source) else {
        return vec![];
    };
    // CSS can redefine transforms and path data; leave these documents untouched.
    if xml.descendants().any(|n| n.has_tag_name("style")) {
        return vec![];
    }
    let root = xml.root_element();
    let viewbox = root
        .attribute("viewBox")
        .and_then(|s| s.parse::<svgtypes::ViewBox>().ok());
    let natural = |key: &str, axis: usize| -> Option<f32> {
        let fallback = viewbox.map(|v| if axis == 0 { v.w as f32 } else { v.h as f32 });
        match root.attribute(key) {
            Some(value) if value.ends_with('%') => {
                Some(fallback? * value.trim_end_matches('%').parse::<f32>().ok()? / 100.)
            }
            Some(_) => coordinate(root, key, 0.),
            None => fallback,
        }
    };
    let Some((width, height)) = natural("width", 0)
        .zip(natural("height", 1))
        .filter(|(w, h)| *w > 0. && *h > 0.)
    else {
        return vec![];
    };
    // Match rasterize_svg's centered contain fit before resolving SVG-internal transforms.
    let scale = (size[0] / width).min(size[1] / height);
    let fitting = [
        scale,
        0.,
        0.,
        scale,
        (size[0] - width * scale) / 2.,
        (size[1] - height * scale) / 2.,
    ];
    xml.descendants()
        .filter(|n| n.has_tag_name("path"))
        .enumerate()
        .filter_map(|(index, node)| {
            let attr = node.attributes().find(|a| a.name() == "d")?;
            let mut transform = fitting;
            let ancestors: Vec<_> = node.ancestors().filter(|n| n.is_element()).collect();
            for ancestor in ancestors.into_iter().rev() {
                if ["defs", "clipPath", "mask", "pattern", "symbol", "marker"]
                    .contains(&ancestor.tag_name().name())
                {
                    return None;
                }
                let style = ancestor.attribute("style").unwrap_or("");
                if ancestor.attribute("display") == Some("none")
                    || ancestor.attribute("visibility") == Some("hidden")
                    || style.contains("transform")
                    || style.contains("display")
                    || style.contains("visibility")
                    || style.contains("d:")
                {
                    return None;
                }
                if let Some(t) = ancestor.attribute("transform") {
                    let t: svgtypes::Transform = t.parse().ok()?;
                    transform = mul(
                        transform,
                        [
                            t.a as f32, t.b as f32, t.c as f32, t.d as f32, t.e as f32, t.f as f32,
                        ],
                    );
                }
                if ancestor.has_tag_name("svg") {
                    transform = mul(transform, viewport(ancestor, [width, height])?);
                }
            }
            let object = VectorObject {
                live_corners: None,
                id: format!("{layer_id}::svg:{index}"),
                name: node.attribute("id").unwrap_or("SVG path").into(),
                group_path: vec![],
                clipping_group: None,
                bounds_reset: false,
                text: None,
                path: VectorPath {
                    data: attr.value().into(),
                    fill_rule: FillRule::NonZero,
                },
                transform,
                fill: Some(VectorPaint {
                    color: [0, 0, 0, 255],
                }),
                stroke: None,
                stroke_width: 0.,
                stroke_style: Default::default(),
                opacity: 1.,
                blend_mode: "normal".into(),
                visible: true,
                kind: VectorObjectKind::Compound,
                control_points: vec![],
            };
            let mut object = crate::bezier::editable(&object)?;
            if let Some(value) = node
                .attribute("data-lumapaint-corners")
                .filter(|s| !s.is_empty())
            {
                let corners: crate::bezier::LiveCorners = serde_json::from_str(value).ok()?;
                corners.validate().ok()?;
                object.live_corners = Some(corners);
            }
            if crate::bezier::segment_indices(&object).is_empty()
                || crate::bezier::local_point(&object, [0., 0.]).is_err()
            {
                return None;
            }
            Some(Target {
                object,
                value: attr.range_value(),
                corners_value: node
                    .attributes()
                    .find(|a| a.name() == "data-lumapaint-corners")
                    .map(|a| a.range_value()),
            })
        })
        .collect()
}

pub fn replace(
    source: &mut String,
    target: Target,
    object: Option<VectorObject>,
) -> Result<(), String> {
    let metadata = object
        .as_ref()
        .and_then(|o| o.live_corners.as_ref())
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let metadata = metadata
        .replace('&', "&amp;")
        .replace('\"', "&quot;")
        .replace('<', "&lt;");
    let data = object.map_or_else(|| "M0 0".into(), |o| o.path.data);
    let mut edits = vec![(target.value.clone(), data)];
    if let Some(range) = target.corners_value {
        edits.push((range, metadata));
    } else if !metadata.is_empty() {
        let at = target.value.end + 1;
        edits.push((at..at, format!(" data-lumapaint-corners=\"{metadata}\"")));
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (range, value) in edits {
        source.replace_range(range, &value);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_viewport_transform_and_aspect_ratio_align_control_positions() {
        let source = r#"<svg width="400" height="200" viewBox="0 0 100 100"><svg x="10" y="20" width="40" height="20" viewBox="0 0 20 20"><g transform="scale(2)"><path d="M0 0L5 5"/></g></svg></svg>"#;
        let target = targets(source, "layer", [400., 200.]).remove(0);
        assert_eq!(
            crate::bezier::world_point(&target.object, [0., 0.]),
            [140., 40.]
        );
        assert_eq!(
            crate::bezier::world_point(&target.object, [5., 5.]),
            [160., 60.]
        );
    }
    #[test]
    fn css_definitions_hidden_paths_and_noninvertible_transforms_are_not_editable() {
        for source in [
            r#"<svg><style>path{transform:scale(2)}</style><path d="M0 0L10 10"/></svg>"#,
            r#"<svg><defs><path d="M0 0L10 10"/></defs></svg>"#,
            r#"<svg><g display="none"><path d="M0 0L10 10"/></g></svg>"#,
            r#"<svg><path transform="scale(0)" d="M0 0L10 10"/></svg>"#,
        ] {
            assert!(targets(source, "layer", [100., 100.]).is_empty());
        }
    }
}

#[cfg(test)]
mod root_fit_tests {
    use super::*;
    #[test]
    fn root_fit_matches_renderer_centered_contain_scaling() {
        let source =
            r#"<svg width="200" height="100" viewBox="0 0 100 50"><path d="M10 20L40 20"/></svg>"#;
        let object = targets(source, "layer", [960., 640.]).remove(0).object;
        let p = crate::bezier::world_point(&object, [10., 20.]);
        assert!((p[0] - 96.).abs() < 0.001);
        assert!((p[1] - 272.).abs() < 0.001);
        let vb_only = r#"<svg viewBox="0 0 100 50"><path d="M10 20L40 20"/></svg>"#;
        assert_eq!(
            targets(vb_only, "layer", [960., 640.])[0].object.transform,
            object.transform
        );
        assert!(targets(
            r#"<svg><path d="M10 20L40 20"/></svg>"#,
            "layer",
            [960., 640.]
        )
        .is_empty());
    }
}

#[cfg(test)]
mod absolute_unit_tests {
    use super::*;
    #[test]
    fn print_units_and_viewbox_percent_root_resolve_to_the_same_geometry() {
        let path = r#"<path d="M10 20L40 20"/>"#;
        let px = format!(r#"<svg width="96" height="48" viewBox="0 0 100 50">{path}</svg>"#);
        let expected = targets(&px, "layer", [960., 640.])[0].object.transform;
        for dimensions in [
            r#"width="1in" height="0.5in""#,
            r#"width="25.4mm" height="12.7mm""#,
            r#"width="72pt" height="36pt""#,
            r#"width="50%" height="50%""#,
        ] {
            let source = format!(r#"<svg {dimensions} viewBox="0 0 100 50">{path}</svg>"#);
            let matrix = targets(&source, "layer", [960., 640.])[0].object.transform;
            assert!(matrix
                .iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 0.001));
        }
    }
}
