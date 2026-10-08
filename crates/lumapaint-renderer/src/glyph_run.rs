//! Atlas requests derived from the existing usvg shaping/layout engine.
//! Unsupported appearance is rejected as a whole, never partly rendered.
use crate::glyph_atlas::GlyphKey;
use resvg::{tiny_skia, usvg};

pub struct GlyphPlacement {
    pub key: GlyphKey,
    pub origin: [f32; 2],
    pub color: [f32; 4],
    pub clip: [f32; 4],
}
pub struct GlyphRun {
    pub glyphs: Vec<GlyphPlacement>,
    pub size: [u32; 2],
}
struct Outline(tiny_skia::PathBuilder);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x: f32, y: f32, a: f32, b: f32) {
        self.0.quad_to(x, y, a, b);
    }
    fn curve_to(&mut self, x: f32, y: f32, a: f32, b: f32, c: f32, d: f32) {
        self.0.cubic_to(x, y, a, b, c, d);
    }
    fn close(&mut self) {
        self.0.close();
    }
}
impl GlyphRun {
    /// Physical pixel coordinates. Integer translation preserves atlas identity;
    /// fractional translation/scale produce new coverage requests.
    pub fn from_tree(
        tree: &usvg::Tree,
        size: [u32; 2],
        transform: tiny_skia::Transform,
    ) -> Option<Self> {
        if size.contains(&0) || size.iter().any(|&s| s > 8192) || !transform.is_valid() {
            return None;
        }
        let mut result = Self {
            glyphs: Vec::new(),
            size,
        };
        collect(
            tree.root(),
            transform,
            [0., 0., size[0] as f32, size[1] as f32],
            tree,
            &mut result.glyphs,
        )?;
        (!result.glyphs.is_empty()).then_some(result)
    }
}
fn intersect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}
fn rectangle_clip(clip: &usvg::ClipPath, transform: tiny_skia::Transform) -> Option<[f32; 4]> {
    if clip.clip_path().is_some() {
        return None;
    }
    let root = clip.root();
    if root.children().len() != 1 {
        return None;
    }
    let usvg::Node::Path(p) = &root.children()[0] else {
        return None;
    };
    if !p.is_visible() || p.fill().is_none() {
        return None;
    }
    let ts = transform
        .pre_concat(clip.transform())
        .pre_concat(root.transform());
    let path = p.data().clone().transform(ts)?;
    let mut points = Vec::new();
    let mut closed = false;
    for s in path.segments() {
        match s {
            tiny_skia::PathSegment::MoveTo(p) | tiny_skia::PathSegment::LineTo(p) => points.push(p),
            tiny_skia::PathSegment::Close => closed = true,
            _ => return None,
        }
    }
    if points.len() == 5 && points[0] == points[4] {
        points.pop();
    }
    if points.len() != 4
        || !closed
        || points
            .iter()
            .enumerate()
            .any(|(i, p)| points[..i].contains(p))
    {
        return None;
    }
    let b = path.bounds();
    for i in 0..4 {
        let p = points[i];
        let q = points[(i + 1) % 4];
        if !((p.x == b.left() || p.x == b.right())
            && (p.y == b.top() || p.y == b.bottom())
            && (p.x == q.x || p.y == q.y))
        {
            return None;
        }
    }
    // Fractional clip coverage needs a mask; retain compatibility for now.
    let bounds = [b.left(), b.top(), b.right(), b.bottom()];
    bounds
        .iter()
        .all(|v| v.is_finite() && v.fract() == 0.)
        .then_some(bounds)
}
fn collect(
    group: &usvg::Group,
    parent: tiny_skia::Transform,
    clip: [f32; 4],
    tree: &usvg::Tree,
    out: &mut Vec<GlyphPlacement>,
) -> Option<()> {
    if group.opacity().get() != 1.
        || group.mask().is_some()
        || !group.filters().is_empty()
        || group.blend_mode() != usvg::BlendMode::Normal
    {
        return None;
    }
    let transform = parent.pre_concat(group.transform());
    let clip = if let Some(c) = group.clip_path() {
        intersect(clip, rectangle_clip(c, transform)?)
    } else {
        clip
    };
    if clip[0] >= clip[2] || clip[1] >= clip[3] {
        return Some(());
    }
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, transform, clip, tree, out)?,
            usvg::Node::Text(t) => {
                if t.rendering_mode() == usvg::TextRendering::OptimizeSpeed {
                    return None;
                }
                for span in t.layouted() {
                    if !span.visible {
                        continue;
                    }
                    if span.stroke.is_some()
                        || span.underline.is_some()
                        || span.overline.is_some()
                        || span.line_through.is_some()
                    {
                        return None;
                    }
                    let Some(fill) = &span.fill else {
                        continue;
                    };
                    let usvg::Paint::Color(color) = fill.paint() else {
                        return None;
                    };
                    let rgba = [
                        f32::from(color.red) / 255.,
                        f32::from(color.green) / 255.,
                        f32::from(color.blue) / 255.,
                        fill.opacity().get(),
                    ];
                    for glyph in &span.positioned_glyphs {
                        let id = ttf_parser::GlyphId(u16::try_from(glyph.id.0).ok()?);
                        let path =
                            tree.fontdb().with_face_data(glyph.font, |bytes, index| {
                                let face = ttf_parser::Face::parse(bytes, index).ok()?;
                                if face.is_variable()
                                    || face.is_color_glyph(id)
                                    || face.glyph_svg_image(id).is_some()
                                    || face.glyph_raster_image(id, u16::MAX).is_some()
                                {
                                    return None;
                                }
                                let mut builder = Outline(tiny_skia::PathBuilder::new());
                                face.outline_glyph(id, &mut builder);
                                // Spaces have no outline and need no atlas slot.
                                Some(builder.0.finish())
                            })??;
                        let Some(path) = path else {
                            continue;
                        };
                        let ts = transform.pre_concat(glyph.outline_transform());
                        let bounds = path.clone().transform(ts)?.bounds();
                        let origin = [bounds.left().floor() - 1., bounds.top().floor() - 1.];
                        let end = [bounds.right().ceil() + 1., bounds.bottom().ceil() + 1.];
                        if !origin
                            .iter()
                            .chain(&end)
                            .all(|v| v.is_finite() && v.abs() < 16_777_216.)
                        {
                            return None;
                        }
                        if end[0] <= clip[0]
                            || end[1] <= clip[1]
                            || origin[0] >= clip[2]
                            || origin[1] >= clip[3]
                        {
                            continue;
                        }
                        let local = tiny_skia::Transform::from_translate(-origin[0], -origin[1])
                            .pre_concat(ts);
                        let key = GlyphKey::new(
                            &path,
                            local,
                            [(end[0] - origin[0]) as u32, (end[1] - origin[1]) as u32],
                            fill.rule() == usvg::FillRule::EvenOdd,
                        )?;
                        if out.len() >= 16384 {
                            return None;
                        }
                        out.push(GlyphPlacement {
                            key,
                            origin,
                            color: rgba,
                            clip,
                        });
                    }
                }
            }
            // Non-text content and decoration/image glyphs keep the established route.
            _ => return None,
        }
    }
    Some(())
}
