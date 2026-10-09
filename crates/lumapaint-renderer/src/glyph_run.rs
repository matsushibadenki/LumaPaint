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
        (!result.glyphs.is_empty() && result.clipped_ink_is_disjoint()).then_some(result)
    }
    pub(crate) fn ink_fits_viewport(&self) -> bool {
        self.glyphs.iter().all(|g| {
            let size = g.key.mask_size();
            g.origin[0] + 1. >= 0.
                && g.origin[1] + 1. >= 0.
                && g.origin[0] + size[0] as f32 - 1. <= self.size[0] as f32
                && g.origin[1] + size[1] as f32 - 1. <= self.size[1] as f32
        })
    }
    /// Per-glyph clipping is safe only when affected ink cannot overlap another
    /// glyph. Also used after adding root canvas masks to an existing run.
    pub(crate) fn clipped_ink_is_disjoint(&self) -> bool {
        let (eligible, comparisons) = self.check_clipped_ink();
        crate::performance::count("glyph_clip_overlap_comparisons", comparisons);
        eligible
    }
    fn check_clipped_ink(&self) -> (bool, u64) {
        if !self.glyphs.iter().any(|g| g.key.is_clipped()) {
            return (true, 0);
        }
        // Fractional group clipping occurs after glyph composition. Baking it
        // per glyph is not equivalent at overlapping boundary ink. Use a bounded
        // grid rather than a full pairwise scan; conservatively retain fallback.
        #[derive(Default)]
        struct Cell {
            all: Vec<usize>,
            clipped: Vec<usize>,
        }
        let mut grid: std::collections::HashMap<(i32, i32), Cell> = Default::default();
        let mut comparisons = 0;
        let mut boxes: Vec<[f32; 4]> = Vec::new();
        let mut references = 0;
        for g in &self.glyphs {
            let size = g.key.mask_size();
            let a = [
                g.origin[0] + 1.,
                g.origin[1] + 1.,
                g.origin[0] + size[0] as f32 - 1.,
                g.origin[1] + size[1] as f32 - 1.,
            ];
            for y in (a[1] / 64.).floor() as i32..=(a[3] / 64.).floor() as i32 {
                for x in (a[0] / 64.).floor() as i32..=(a[2] / 64.).floor() as i32 {
                    let cell = grid.entry((x, y)).or_default();
                    // An unclipped glyph only needs to compare with earlier
                    // clipped glyphs; other unclipped pairs cannot cause fallback.
                    let candidates = if g.key.is_clipped() {
                        &cell.all
                    } else {
                        &cell.clipped
                    };
                    for &index in candidates {
                        comparisons += 1;
                        if comparisons > 400_000 {
                            crate::performance::count("glyph_clip_overlap_budget_fallback", 1);
                            return (false, comparisons);
                        }
                        let b = boxes[index];
                        if (g.key.is_clipped() || self.glyphs[index].key.is_clipped())
                            && a[0] < b[2]
                            && b[0] < a[2]
                            && a[1] < b[3]
                            && b[1] < a[3]
                        {
                            crate::performance::count(
                                "glyph_run_ineligible.overlapping_ink_bounds",
                                1,
                            );
                            return (false, comparisons);
                        }
                    }
                    references += 1;
                    if references > 400_000 {
                        return (false, comparisons);
                    }
                    cell.all.push(boxes.len());
                    if g.key.is_clipped() {
                        cell.clipped.push(boxes.len());
                    }
                }
            }
            boxes.push(a);
        }
        (true, comparisons)
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
    if !p.is_visible()
        || p.fill().is_none()
        || p.rendering_mode() != usvg::ShapeRendering::GeometricPrecision
        || p.fill()
            .is_some_and(|f| f.opacity().get() != 1. || !matches!(f.paint(), usvg::Paint::Color(_)))
    {
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
    // Fractional coverage is baked into boundary glyph masks below.
    let bounds = [b.left(), b.top(), b.right(), b.bottom()];
    bounds.iter().all(|v| v.is_finite()).then_some(bounds)
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
        let next = rectangle_clip(c, transform)?;
        // Multiplying two fractional masks differs from intersecting rectangles.
        if clip.iter().any(|v| v.fract() != 0.) && next.iter().any(|v| v.fract() != 0.) {
            return None;
        }
        intersect(clip, next)
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
                        // Match the compatibility renderer's two-stage transform:
                        // layout font units first, then apply the outer SVG transform.
                        let path = path.transform(glyph.outline_transform())?;
                        let ts = transform;
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
                        let mut key = GlyphKey::new(
                            &path,
                            local,
                            [(end[0] - origin[0]) as u32, (end[1] - origin[1]) as u32],
                            fill.rule() == usvg::FillRule::EvenOdd,
                        )?;
                        let fractional = clip.iter().any(|v| v.fract() != 0.);
                        if fractional {
                            key = key.with_clip([
                                clip[0] - origin[0],
                                clip[1] - origin[1],
                                clip[2] - origin[0],
                                clip[3] - origin[1],
                            ])?;
                        }
                        if out.len() >= 16384 {
                            return None;
                        }
                        out.push(GlyphPlacement {
                            key,
                            origin,
                            color: rgba,
                            clip: if fractional {
                                [
                                    clip[0].floor(),
                                    clip[1].floor(),
                                    clip[2].ceil(),
                                    clip[3].ceil(),
                                ]
                            } else {
                                clip
                            },
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unclipped_pairs_are_not_compared_when_one_frame_boundary_is_present() {
        let path =
            tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(1., 1., 4., 4.).unwrap());
        let key = GlyphKey::new(&path, tiny_skia::Transform::identity(), [6, 6], false).unwrap();
        let mut run = GlyphRun {
            glyphs: (0..4096)
                .map(|_| GlyphPlacement {
                    key: key.clone(),
                    origin: [0., 0.],
                    color: [1.; 4],
                    clip: [0., 0., 64., 64.],
                })
                .collect(),
            size: [64, 64],
        };
        assert_eq!(run.check_clipped_ink(), (true, 0));
        let mut boundary = key.clone();
        boundary.set_canvas_clip([1.5, 0., 6., 6.]).unwrap();
        run.glyphs.push(GlyphPlacement {
            key: boundary,
            origin: [20., 0.],
            color: [1.; 4],
            clip: [0., 0., 64., 64.],
        });
        // Previously every unclipped pair in the cell was inspected (8,386,560
        // comparisons), although only 4,096 boundary pairs are relevant.
        assert_eq!(run.check_clipped_ink(), (true, 4096));
        run.glyphs.last_mut().unwrap().origin = [2., 0.];
        assert!(!run.check_clipped_ink().0);
    }
    #[test]
    fn adversarial_empty_ink_candidates_have_a_comparison_budget() {
        let path =
            tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0., 0., 1., 1.).unwrap());
        let key = GlyphKey::new(&path, tiny_skia::Transform::identity(), [2, 2], false)
            .unwrap()
            .with_clip([0.5, 0., 2., 2.])
            .unwrap();
        let run = GlyphRun {
            glyphs: (0..1000)
                .map(|_| GlyphPlacement {
                    key: key.clone(),
                    origin: [0., 0.],
                    color: [1.; 4],
                    clip: [0., 0., 64., 64.],
                })
                .collect(),
            size: [64, 64],
        };
        assert_eq!(run.check_clipped_ink(), (false, 400001));
    }
    #[test]
    fn adding_canvas_clip_rechecks_overlapping_ink() {
        let path =
            tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(1., 1., 4., 4.).unwrap());
        let key = GlyphKey::new(&path, tiny_skia::Transform::identity(), [6, 6], false).unwrap();
        let mut run = GlyphRun {
            glyphs: [0., 2.]
                .into_iter()
                .map(|x| GlyphPlacement {
                    key: key.clone(),
                    origin: [x, 0.],
                    color: [1.; 4],
                    clip: [0., 0., 16., 16.],
                })
                .collect(),
            size: [16, 16],
        };
        assert!(run.clipped_ink_is_disjoint());
        assert!(run.ink_fits_viewport());
        run.glyphs[0]
            .key
            .set_canvas_clip([1.5, 0., 16., 16.])
            .unwrap();
        assert!(!run.clipped_ink_is_disjoint());
        run.glyphs[1].origin = [8., 0.];
        assert!(run.clipped_ink_is_disjoint());
        run.glyphs[1].origin = [8., 12.];
        assert!(!run.ink_fits_viewport());
    }
}
