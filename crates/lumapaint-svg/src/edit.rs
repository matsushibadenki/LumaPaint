//! Edit the rendered SVG geometry while retaining the authored XML around it.
use lumapaint_core::vector::{FillRule, VectorObject, VectorObjectKind, VectorPaint, VectorPath};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    ops::Range,
    sync::{Arc, OnceLock},
};

pub struct Target {
    pub object: VectorObject,
    source: usvg::PathSource,
    tree: Arc<usvg::Tree>,
    path: usvg::Path,
    suffix: String,
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
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}
fn parsed_tree(source: &str) -> Option<Arc<usvg::Tree>> {
    type Cache = std::collections::VecDeque<(String, Arc<usvg::Tree>)>;
    thread_local! {static CACHE:std::cell::RefCell<Cache>=const {std::cell::RefCell::new(std::collections::VecDeque::new())};}
    if let Some(tree) = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let index = cache.iter().position(|(text, _)| text == source)?;
        let entry = cache.remove(index)?;
        let tree = entry.1.clone();
        cache.push_front(entry);
        Some(tree)
    }) {
        return Some(tree);
    }
    let options = usvg::Options {
        fontdb: fonts(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = Arc::new(usvg::Tree::from_str(source, &options).ok()?);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.push_front((source.into(), tree.clone()));
        while cache.len() > 4 || cache.iter().map(|(s, _)| s.len()).sum::<usize>() > 8 * 1024 * 1024
        {
            cache.pop_back();
        }
    });
    Some(tree)
}
fn label(source: &usvg::PathSource, xml: &roxmltree::Document<'_>) -> String {
    let node = |range: &Range<usize>| {
        xml.descendants()
            .find(|n| n.is_element() && n.range().start == range.start)
    };
    let Some(element) = node(&source.element) else {
        return "SVG".into();
    };
    let label = element
        .attribute("data-lumapaint-name")
        .or_else(|| element.attribute("id"))
        .unwrap_or(element.tag_name().name());
    if let Some(owner) = source
        .instances
        .first()
        .and_then(node)
        .and_then(|n| n.attribute("id"))
    {
        format!("{owner} / {label}")
    } else {
        label.to_owned()
    }
}
fn drawable(node: roxmltree::Node<'_, '_>) -> bool {
    matches!(
        node.tag_name().name(),
        "path" | "rect" | "ellipse" | "circle" | "line" | "polyline" | "polygon" | "use"
    ) || node.attribute("data-lumapaint-instance").is_some()
}
fn ordinals(xml: &roxmltree::Document<'_>) -> HashMap<usize, usize> {
    xml.descendants()
        .filter(|n| n.is_element() && drawable(*n))
        .filter(|n| {
            !n.ancestors().skip(1).any(|a| {
                a.attribute("data-lumapaint-private").is_some()
                    || a.attribute("data-lumapaint-instance").is_some()
            })
        })
        .enumerate()
        .map(|(i, n)| (n.range().start, i))
        .collect()
}
fn identity(
    source: &usvg::PathSource,
    xml: &roxmltree::Document<'_>,
    ordinals: &HashMap<usize, usize>,
) -> Option<String> {
    let element = |range: &Range<usize>| {
        xml.descendants()
            .find(|n| n.is_element() && n.range().start == range.start)
    };
    let mut ids = Vec::new();
    for range in &source.instances {
        let n = element(range)?;
        ids.push(
            n.attribute("data-lumapaint-edit-id")
                .map(str::to_owned)
                .unwrap_or(format!("svg-use:{}", ordinals.get(&range.start)?)),
        );
    }
    let n = element(&source.element)?;
    ids.push(
        n.attribute("data-lumapaint-edit-id")
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "svg:{}",
                    ordinals.get(&source.element.start).copied().unwrap_or(0)
                )
            }),
    );
    Some(ids.join("/"))
}
fn provenance_key(source: &usvg::PathSource) -> String {
    source
        .instances
        .iter()
        .chain(std::iter::once(&source.element))
        .map(|r| r.start.to_string())
        .collect::<Vec<_>>()
        .join(":")
}
fn path_data(path: &usvg::Path) -> String {
    use tiny_skia_path::PathSegment::*;
    let mut data = String::new();
    for segment in path.data().segments() {
        match segment {
            MoveTo(p) => {
                let _ = write!(data, "M{} {}", p.x, p.y);
            }
            LineTo(p) => {
                let _ = write!(data, "L{} {}", p.x, p.y);
            }
            QuadTo(a, b) => {
                let _ = write!(data, "Q{} {} {} {}", a.x, a.y, b.x, b.y);
            }
            CubicTo(a, b, c) => {
                let _ = write!(data, "C{} {} {} {} {} {}", a.x, a.y, b.x, b.y, c.x, c.y);
            }
            Close => data.push('Z'),
        }
    }
    data
}
fn visit<'a>(
    group: &'a usvg::Group,
    parent: [f32; 6],
    paths: &mut Vec<(&'a usvg::Path, [f32; 6])>,
) {
    if group.opacity().get() == 0. {
        return;
    }
    let t = group.transform();
    let transform = mul(parent, [t.sx, t.ky, t.kx, t.sy, t.tx, t.ty]);
    for node in group.children() {
        match node {
            usvg::Node::Path(p) if p.is_visible() => paths.push((p, transform)),
            usvg::Node::Group(g) => visit(g, transform, paths),
            _ => {}
        }
    }
}
pub fn targets(source: &str, layer_id: &str, size: [f32; 2]) -> Vec<Target> {
    if source.len() > 4 * 1024 * 1024 || size.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return vec![];
    }
    let Ok(xml) = roxmltree::Document::parse(source) else {
        return vec![];
    };
    let root = xml.root_element();
    if root.attribute("viewBox").is_none()
        && (root.attribute("width").is_none() || root.attribute("height").is_none())
    {
        return vec![];
    }
    let Some(tree) = parsed_tree(source) else {
        return vec![];
    };
    let dimensions = tree.size();
    let scale = (size[0] / dimensions.width()).min(size[1] / dimensions.height());
    let fitting = [
        scale,
        0.,
        0.,
        scale,
        (size[0] - dimensions.width() * scale) / 2.,
        (size[1] - dimensions.height() * scale) / 2.,
    ];
    let ordinals = ordinals(&xml);
    let mut paths = Vec::new();
    visit(tree.root(), [1., 0., 0., 1., 0., 0.], &mut paths);
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter_map(|(path, rendered_transform)| {
            let provenance = path.source()?.clone();
            let suffix = identity(&provenance, &xml, &ordinals)?;
            if !seen.insert(suffix.clone()) {
                return None;
            }
            let node = xml
                .descendants()
                .find(|n| n.is_element() && n.range().start == provenance.element.start)?;
            let transform = mul(fitting, rendered_transform);
            let paint = Some(VectorPaint {
                color: [0, 0, 0, 255],
            });
            let object = VectorObject {
                live_corners: None,
                rectangle_radii: None,
                id: format!("{layer_id}::{suffix}"),
                name: label(&provenance, &xml),
                group_path: vec![],
                clipping_group: None,
                bounds_reset: false,
                text: None,
                path: VectorPath {
                    data: path_data(path),
                    fill_rule: if path
                        .fill()
                        .is_some_and(|f| f.rule() == usvg::FillRule::EvenOdd)
                    {
                        FillRule::EvenOdd
                    } else {
                        FillRule::NonZero
                    },
                },
                transform,
                fill: path.fill().and(paint),
                stroke: path.stroke().and(paint),
                stroke_width: path.stroke().map_or(0., |s| s.width().get()),
                stroke_style: Default::default(),
                opacity: 1.,
                blend_mode: "normal".into(),
                visible: true,
                kind: VectorObjectKind::Compound,
                control_points: vec![],
            };
            let mut object = lumapaint_core::bezier::editable(&object)?;
            if let Some(value) = node
                .attribute("data-lumapaint-corners")
                .filter(|s| !s.is_empty())
            {
                let corners: lumapaint_core::bezier::LiveCorners =
                    serde_json::from_str(value).ok()?;
                corners.validate().ok()?;
                object.live_corners = Some(corners);
            }
            if lumapaint_core::bezier::segment_indices(&object).is_empty()
                || lumapaint_core::bezier::local_point(&object, [0., 0.]).is_err()
            {
                return None;
            }
            Some(Target {
                object,
                source: provenance,
                tree: tree.clone(),
                path: path.clone(),
                suffix,
            })
        })
        .collect()
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('\'', "&apos;")
}
fn opening(source: &str, start: usize) -> Option<Range<usize>> {
    let mut quote = None;
    for (offset, c) in source.get(start..)?.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '>' {
            return Some(start..start + offset + 1);
        }
    }
    None
}
/// Add/replace attributes without changing children or unrelated attributes.
fn attributes(
    source: &str,
    node: roxmltree::Node<'_, '_>,
    tag: Option<&str>,
    values: &[(&str, String)],
) -> Result<String, String> {
    let range = opening(source, node.range().start).ok_or("Invalid SVG element")?;
    let mut edits = Vec::new();
    let mut additions = String::new();
    for (key, value) in values {
        if let Some(attr) = node
            .attributes()
            .find(|a| a.name() == *key && a.namespace().is_none())
        {
            edits.push((attr.range_value(), escape(value)));
        } else {
            let _ = write!(additions, " {key}=\"{}\"", escape(value));
        }
    }
    let at = range.end
        - if source.get(range.end - 2..range.end) == Some("/>") {
            2
        } else {
            1
        };
    edits.push((at..at, additions));
    if let Some(tag) = tag {
        let start = range.start + 1;
        let end = source[start..]
            .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .map(|n| start + n)
            .ok_or("Invalid SVG tag")?;
        let qualified = source[start..end]
            .split_once(':')
            .map_or_else(|| tag.to_owned(), |(prefix, _)| format!("{prefix}:{tag}"));
        edits.push((start..end, qualified.clone()));
        if !source[range.clone()].ends_with("/>") {
            let end_start = source[node.range().clone()]
                .rfind("</")
                .map(|n| node.range().start + n + 2)
                .ok_or("Invalid SVG closing tag")?;
            let end = source[end_start..]
                .find('>')
                .map(|n| end_start + n)
                .ok_or("Invalid SVG closing tag")?;
            edits.push((end_start..end, qualified));
        }
    }
    let mut out = source[node.range().clone()].to_owned();
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, value) in edits {
        out.replace_range(
            range.start - node.range().start..range.end - node.range().start,
            &value,
        );
    }
    Ok(out)
}
fn declarations(style: &str) -> Vec<&str> {
    let (mut start, mut depth, mut quote, mut escaped) = (0, 0, None, false);
    let mut parts = Vec::new();
    for (i, c) in style.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth -= 1,
            ';' if depth == 0 => {
                parts.push(&style[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&style[start..]);
    parts
}
fn override_style(style: &str, values: &[(&str, String)]) -> String {
    let mut out = declarations(style)
        .into_iter()
        .filter(|s| {
            s.split_once(':').is_none_or(|(name, _)| {
                !values
                    .iter()
                    .any(|(key, _)| name.trim().eq_ignore_ascii_case(key))
            })
        })
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(";");
    for (key, value) in values {
        if !out.is_empty() {
            out.push(';');
        }
        let _ = write!(out, "{key}:{value} !important");
    }
    out
}
fn metadata(object: Option<&VectorObject>) -> Result<String, String> {
    object
        .and_then(|o| o.live_corners.as_ref())
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())
        .map(|v| v.unwrap_or_default())
}
fn data(object: Option<&VectorObject>) -> String {
    object.map_or_else(|| "M0 0".into(), |o| o.path.data.clone())
}
fn prefix(xml: &roxmltree::Document<'_>, start: usize) -> String {
    for n in 0.. {
        let prefix = format!("lumapaint-private-{start}-{n}-");
        if !xml.descendants().any(|node| {
            node.attribute("id")
                .is_some_and(|id| id.starts_with(&prefix))
        }) {
            return prefix;
        }
    }
    unreachable!()
}
fn options(prefix: String) -> usvg::WriteOptions {
    usvg::WriteOptions {
        id_prefix: Some(prefix),
        source_provenance: true,
        indent: usvg::Indent::None,
        ..Default::default()
    }
}
fn find_instance(group: &usvg::Group, range: &Range<usize>) -> Option<usvg::Node> {
    for node in group.children() {
        if let usvg::Node::Group(g) = node {
            if g.source_range() == Some(range) {
                return Some(node.clone());
            }
            if let Some(node) = find_instance(g, range) {
                return Some(node);
            }
        }
    }
    None
}
fn paint_values(node: roxmltree::Node<'_, '_>) -> Vec<(&'static str, String)> {
    [
        ("fill", "black"),
        ("fill-rule", "nonzero"),
        ("fill-opacity", "1"),
        ("stroke", "none"),
        ("stroke-width", "1"),
        ("stroke-opacity", "1"),
        ("stroke-linecap", "butt"),
        ("stroke-linejoin", "miter"),
        ("stroke-miterlimit", "4"),
        ("stroke-dasharray", "none"),
        ("stroke-dashoffset", "0"),
        ("paint-order", "normal"),
        ("visibility", "visible"),
        ("shape-rendering", "auto"),
    ]
    .into_iter()
    .map(|(key, default)| (key, node.attribute(key).unwrap_or(default).to_owned()))
    .collect()
}
/// Freeze normalized nodes against the surrounding stylesheet after detaching a use.
fn freeze_fragment(
    fragment: &str,
    xml: &roxmltree::Document<'_>,
    changes: &HashMap<String, (&Target, Option<&VectorObject>)>,
) -> Result<String, String> {
    let parsed = roxmltree::Document::parse(fragment).map_err(|e| e.to_string())?;
    let ordinals = ordinals(xml);
    let mut edits = Vec::new();
    for node in parsed
        .descendants()
        .filter(|n| n.is_element() && n != &parsed.root_element())
    {
        let definition = node.ancestors().any(|a| a.has_tag_name("defs"));
        let mut values = if node.has_tag_name("path") {
            paint_values(node)
        } else {
            Vec::new()
        };
        for (key, default) in [
            ("transform", "none"),
            ("opacity", "1"),
            ("display", "inline"),
            ("visibility", "visible"),
            ("clip-path", "none"),
            ("mask", "none"),
            ("filter", "none"),
        ] {
            if !values.iter().any(|(name, _)| *name == key) {
                values.push((key, node.attribute(key).unwrap_or(default).to_owned()));
            }
        }
        let mut attrs = Vec::new();
        if let Some(key) = node
            .attribute("data-lumapaint-source")
            .filter(|_| !definition)
        {
            let starts = key
                .split(':')
                .map(str::parse::<usize>)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "Invalid SVG provenance")?;
            let ranges = starts
                .iter()
                .map(|start| {
                    xml.descendants()
                        .find(|n| n.is_element() && n.range().start == *start)
                        .map(|n| n.range())
                        .ok_or("SVG source missing")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let original = usvg::PathSource {
                element: ranges.last().ok_or("Invalid SVG provenance")?.clone(),
                instances: ranges[..ranges.len() - 1].to_vec(),
            };
            let suffix = identity(&original, xml, &ordinals).ok_or("SVG target missing")?;
            let change = changes.get(key);
            let path = change.map_or_else(
                || node.attribute("d").unwrap_or("").to_owned(),
                |(_, o)| data(*o),
            );
            let meta = if let Some((_, object)) = change {
                metadata(*object)?
            } else {
                xml.descendants()
                    .find(|n| n.is_element() && n.range().start == original.element.start)
                    .and_then(|n| n.attribute("data-lumapaint-corners"))
                    .unwrap_or("")
                    .to_owned()
            };
            values.push(("d", format!("path('{}')", path)));
            attrs.extend([
                ("d", path),
                ("data-lumapaint-edit-id", suffix),
                ("data-lumapaint-name", label(&original, xml)),
                ("data-lumapaint-corners", meta),
            ]);
        } else if node.has_tag_name("path") {
            values.push((
                "d",
                format!("path('{}')", node.attribute("d").unwrap_or("")),
            ));
            if !definition {
                attrs.push(("data-lumapaint-generated", "true".into()));
            }
        }
        for key in [
            "stop-color",
            "stop-opacity",
            "flood-color",
            "flood-opacity",
            "lighting-color",
        ] {
            if let Some(value) = node.attribute(key) {
                values.push((key, value.into()));
            }
        }
        attrs.push((
            "style",
            override_style(node.attribute("style").unwrap_or(""), &values),
        ));
        let full = attributes(fragment, node, None, &attrs)?;
        let end = opening(&full, 0).ok_or("Invalid SVG element")?.end;
        edits.push((
            opening(fragment, node.range().start).ok_or("Invalid SVG element")?,
            full[..end].to_owned(),
        ));
    }
    let mut out = fragment.to_owned();
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (r, value) in edits {
        out.replace_range(r, &value);
    }
    Ok(out)
}

fn replace_many_inner(
    source: &mut String,
    changes: Vec<(Target, Option<VectorObject>)>,
) -> Result<(), String> {
    let xml = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let mut owners: HashMap<usize, Vec<(&Target, Option<&VectorObject>)>> = HashMap::new();
    for (target, object) in &changes {
        let owner = target
            .source
            .instances
            .first()
            .unwrap_or(&target.source.element);
        owners
            .entry(owner.start)
            .or_default()
            .push((target, object.as_ref()));
    }
    let mut edits = Vec::new();
    for (start, changes) in owners {
        let node = xml
            .descendants()
            .find(|n| n.is_element() && n.range().start == start)
            .ok_or("SVG element missing")?;
        let (target, object) = changes[0];
        if !target.source.instances.is_empty() || !node.has_tag_name("path") {
            let resolved = find_instance(target.tree.root(), &node.range())
                .unwrap_or_else(|| usvg::Node::Path(Box::new(target.path.clone())));
            let fragment = target
                .tree
                .node_to_string(&resolved, &options(prefix(&xml, start)));
            let map = changes
                .iter()
                .map(|(t, o)| (provenance_key(&t.source), (*t, *o)))
                .collect();
            let fragment = freeze_fragment(&fragment, &xml, &map)?;
            let parsed = roxmltree::Document::parse(&fragment).map_err(|e| e.to_string())?;
            let content = parsed
                .root_element()
                .children()
                .filter(|n| n.is_element())
                .map(|n| fragment[n.range()].to_owned())
                .collect::<String>();
            let owner_id = identity(
                &usvg::PathSource {
                    element: node.range(),
                    instances: vec![],
                },
                &xml,
                &ordinals(&xml),
            )
            .ok_or("SVG identity missing")?;
            let defaults = [
                ("transform", "none".into()),
                ("opacity", "1".into()),
                ("display", "inline".into()),
                ("visibility", "visible".into()),
                ("clip-path", "none".into()),
                ("mask", "none".into()),
                ("filter", "none".into()),
                ("mix-blend-mode", "normal".into()),
                ("isolation", "auto".into()),
            ];
            let style = override_style(node.attribute("style").unwrap_or(""), &defaults);
            let wrapper = attributes(
                source,
                node,
                Some("g"),
                &[
                    ("data-lumapaint-instance", "true".into()),
                    ("data-lumapaint-edit-id", owner_id),
                    ("style", style.clone()),
                ],
            )?;
            let opening = opening(&wrapper, 0).ok_or("Invalid SVG wrapper")?;
            let header = wrapper[..opening.end]
                .trim_end_matches('>')
                .trim_end_matches('/');
            let qualified = header[1..]
                .split_whitespace()
                .next()
                .ok_or("Invalid SVG wrapper")?;
            let descriptions = node
                .children()
                .filter(|n| n.has_tag_name("title") || n.has_tag_name("desc"))
                .map(|n| source[n.range()].to_owned())
                .collect::<String>();
            edits.push((node.range(),format!(r#"{header}>{descriptions}<g xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" style="{}">{content}</g></{qualified}>"#,escape(&override_style("",&defaults)))));
        } else {
            let path = data(object);
            let values = [("d", format!("path('{}')", path))];
            let attrs = [
                ("d", path),
                ("data-lumapaint-corners", metadata(object)?),
                ("data-lumapaint-edit-id", target.suffix.clone()),
                (
                    "style",
                    override_style(node.attribute("style").unwrap_or(""), &values),
                ),
            ];
            edits.push((node.range(), attributes(source, node, None, &attrs)?));
        }
    }
    let mut updated = source.clone();
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (range, value) in edits {
        updated.replace_range(range, &value);
    }
    roxmltree::Document::parse(&updated).map_err(|e| e.to_string())?;
    *source = updated;
    Ok(())
}
pub fn replace_many(
    source: &mut String,
    changes: Vec<(Target, Option<VectorObject>)>,
) -> Result<(), String> {
    replace_many_inner(source, changes).map_err(|error| {
        format!(
            "SVG edit failed: {error} / SVGの編集に失敗しました: {error} / SVG编辑失败：{error}"
        )
    })
}
#[cfg(test)]
pub fn replace(
    source: &mut String,
    target: Target,
    object: Option<VectorObject>,
) -> Result<(), String> {
    replace_many(source, vec![(target, object)])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_viewport_transform_and_aspect_ratio_align_control_positions() {
        let source = r#"<svg width="400" height="200" viewBox="0 0 100 100"><svg x="10" y="20" width="40" height="20" viewBox="0 0 20 20"><g transform="scale(2)"><path d="M0 0L5 5"/></g></svg></svg>"#;
        let target = targets(source, "layer", [400., 200.]).remove(0);
        assert_eq!(
            lumapaint_core::bezier::world_point(&target.object, [0., 0.]),
            [140., 40.]
        );
        assert_eq!(
            lumapaint_core::bezier::world_point(&target.object, [5., 5.]),
            [160., 60.]
        );
    }
    #[test]
    fn missing_dimensions_definitions_hidden_paths_and_singular_transforms_are_excluded() {
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
        let p = lumapaint_core::bezier::world_point(&object, [10., 20.]);
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

#[cfg(test)]
mod resolved_edit_tests {
    use super::*;
    fn moved(mut object: VectorObject, delta: [f32; 2]) -> VectorObject {
        lumapaint_core::bezier::translate_controls(&mut object, &[0], delta, false).unwrap();
        object
    }
    #[test]
    fn every_basic_shape_is_editable_and_keeps_identity_after_conversion() {
        for element in [
            r#"<rect x="10" y="10" width="30" height="20" rx="5"/>"#,
            r#"<ellipse cx="30" cy="30" rx="15" ry="10"/>"#,
            r#"<circle cx="30" cy="30" r="15"/>"#,
            r#"<line x1="10" y1="10" x2="40" y2="30" stroke="black"/>"#,
            r#"<polyline points="10,10 40,30 60,10"/>"#,
            r#"<polygon points="10,10 40,30 60,10"/>"#,
        ] {
            let mut source = format!(
                r#"<svg width="100" height="100">{element}<path id="other" d="M60 70L80 90"/></svg>"#
            );
            let all = targets(&source, "layer", [100., 100.]);
            assert_eq!(all.len(), 2, "{element}");
            let target = all.into_iter().next().unwrap();
            let id = target.object.id.clone();
            let next = moved(target.object.clone(), [3., 4.]);
            replace(&mut source, target, Some(next)).unwrap();
            let after = targets(&source, "layer", [100., 100.]);
            assert_eq!(after.len(), 2, "{source}");
            assert_eq!(after[0].object.id, id);
            assert!(source.contains(r#"<path id="other" d="M60 70L80 90"/>"#));
            assert!(source.contains("data-lumapaint-instance"));
            let second = moved(after[0].object.clone(), [2., 0.]);
            let target = targets(&source, "layer", [100., 100.]).remove(0);
            replace(&mut source, target, Some(second)).unwrap();
            assert_eq!(targets(&source, "layer", [100., 100.])[0].object.id, id);
        }
    }
    #[test]
    fn css_geometry_transforms_visibility_and_important_d_follow_rendered_state() {
        let mut source=r#"<svg width="100" height="100"><style>rect {x:20px;y:10px;width:30px;height:20px;fill:red} .hidden {display:none} #curve {d:path('M5 5L25 25') !important;transform:translateX(10px) rotate(0deg)}</style><rect/><path id="curve" d="M0 0L1 1"/><path class="hidden" d="M0 0L20 20"/></svg>"#.to_owned();
        let all = targets(&source, "l", [100., 100.]);
        assert_eq!(all.len(), 2);
        assert_eq!(
            lumapaint_core::bezier::world_point(&all[0].object, all[0].object.control_points[0]),
            [20., 10.]
        );
        assert_eq!(
            lumapaint_core::bezier::world_point(&all[1].object, all[1].object.control_points[0]),
            [15., 5.]
        );
        let target = targets(&source, "l", [100., 100.]).remove(1);
        let id = target.object.id.clone();
        let next = moved(target.object.clone(), [5., 3.]);
        replace(&mut source, target, Some(next)).unwrap();
        let edited = targets(&source, "l", [100., 100.])
            .into_iter()
            .find(|t| t.object.id == id)
            .unwrap();
        assert_eq!(
            lumapaint_core::bezier::world_point(&edited.object, edited.object.control_points[0]),
            [20., 8.],
            "{source}"
        );
        assert!(source.contains("#curve {d:path('M5 5L25 25') !important"));
    }
    #[test]
    fn use_instances_edit_independently_and_multiple_leaves_commit_together() {
        let mut source=r##"<svg width="160" height="100"><defs><g id="model"><rect x="5" y="5" width="20" height="20"/><circle cx="35" cy="15" r="10"/></g></defs><use id="one" href="#model" x="10" y="10"/><use id="two" href="#model" x="80" y="10"/><path id="tail" d="M10 70L50 90"/></svg>"##.to_owned();
        let original = source.clone();
        let before = targets(&source, "l", [160., 100.]);
        assert_eq!(before.len(), 5);
        let ids: Vec<_> = before.iter().map(|t| t.object.id.clone()).collect();
        assert_eq!(
            lumapaint_core::bezier::world_point(
                &before[0].object,
                before[0].object.control_points[0]
            ),
            [15., 15.]
        );
        let changes = targets(&source, "l", [160., 100.])
            .into_iter()
            .take(2)
            .map(|t| {
                let o = moved(t.object.clone(), [2., 3.]);
                (t, Some(o))
            })
            .collect();
        replace_many(&mut source, changes).unwrap();
        let after = targets(&source, "l", [160., 100.]);
        assert_eq!(after.len(), 5, "{source}");
        assert_eq!(
            after
                .iter()
                .map(|t| t.object.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert!(source.contains(r##"<use id="two" href="#model" x="80" y="10"/>"##));
        assert!(source.contains(r#"<g id="model"><rect x="5" y="5" width="20" height="20"/><circle cx="35" cy="15" r="10"/></g>"#));
        assert_eq!(
            after[2].object.control_points,
            before[2].object.control_points
        );
        assert_eq!(after[0].object.control_points[0], [7., 8.]);
        assert_ne!(source, original);
    }
    #[test]
    fn nested_use_symbol_viewports_and_prefixed_svg_keep_stable_geometry() {
        let mut source=r##"<s:svg xmlns:s="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100"><s:defs><s:symbol id="symbol" viewBox="0 0 20 20"><s:rect width="20" height="20"/></s:symbol><s:use id="nested" xlink:href="#symbol" width="40" height="40"/></s:defs><s:use xlink:href="#nested" x="10" y="20"/></s:svg>"##.to_owned();
        let target = targets(&source, "l", [100., 100.]).remove(0);
        let id = target.object.id.clone();
        let world =
            lumapaint_core::bezier::world_point(&target.object, target.object.control_points[0]);
        assert_eq!(world, [10., 20.]);
        let next = moved(target.object.clone(), [2., 3.]);
        replace(&mut source, target, Some(next)).unwrap();
        let after = targets(&source, "l", [100., 100.]);
        assert_eq!(after.len(), 1, "{source}");
        assert_eq!(after[0].object.id, id);
        assert_eq!(
            lumapaint_core::bezier::world_point(
                &after[0].object,
                after[0].object.control_points[0]
            ),
            [12., 23.]
        );
        assert!(source.contains("clip-path"));
    }
}
