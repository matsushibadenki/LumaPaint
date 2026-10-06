//! Automatic spacing derived from the same font database used by the renderer.
use lumapaint_core::vector::{KerningMode, TextStyle, VectorText, WritingMode};
use resvg::usvg;
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

const BANDS: usize = 32;
#[derive(Clone)]
struct GlyphShape {
    advance: f32,
    edges: Vec<([f32; 2], [f32; 2])>,
}
struct Outline {
    edges: Vec<([f32; 2], [f32; 2])>,
    at: [f32; 2],
    start: [f32; 2],
    offset: [f32; 2],
    scale: f32,
    vertical: bool,
}
impl Outline {
    fn mapped(&self, x: f32, y: f32) -> [f32; 2] {
        let x = (x + self.offset[0]) * self.scale;
        let y = (y + self.offset[1]) * self.scale;
        if self.vertical {
            [-y, x]
        } else {
            [x, y]
        }
    }
    fn edge(&mut self, point: [f32; 2]) {
        self.edges.push((self.at, point));
        self.at = point;
    }
}
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.at = self.mapped(x, y);
        self.start = self.at;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.edge(self.mapped(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let a = self.at;
        let b = self.mapped(x1, y1);
        let c = self.mapped(x, y);
        for i in 1..=12 {
            let t = i as f32 / 12.;
            let u = 1. - t;
            self.edge([
                u * u * a[0] + 2. * u * t * b[0] + t * t * c[0],
                u * u * a[1] + 2. * u * t * b[1] + t * t * c[1],
            ]);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.at;
        let b = self.mapped(x1, y1);
        let c = self.mapped(x2, y2);
        let d = self.mapped(x, y);
        for i in 1..=16 {
            let t = i as f32 / 16.;
            let u = 1. - t;
            self.edge([
                u * u * u * a[0] + 3. * u * u * t * b[0] + 3. * u * t * t * c[0] + t * t * t * d[0],
                u * u * u * a[1] + 3. * u * u * t * b[1] + 3. * u * t * t * c[1] + t * t * t * d[1],
            ]);
        }
    }
    fn close(&mut self) {
        self.edge(self.start);
    }
}
fn shape(data: &[u8], index: u32, value: &str, vertical: bool, kern: bool) -> Option<GlyphShape> {
    let rb = rustybuzz::Face::from_slice(data, index)?;
    let face = ttf_parser::Face::parse(data, index).ok()?;
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(value);
    buffer.guess_segment_properties();
    if vertical {
        buffer.set_direction(rustybuzz::Direction::TopToBottom);
    }
    let tag = ttf_parser::Tag::from_bytes;
    let features = [
        rustybuzz::Feature::new(tag(b"kern"), u32::from(kern), ..),
        rustybuzz::Feature::new(tag(b"vkrn"), u32::from(kern), ..),
        rustybuzz::Feature::new(tag(b"liga"), 0, ..),
    ];
    let result = rustybuzz::shape(&rb, &features, buffer);
    let mut outline = Outline {
        edges: Vec::new(),
        at: [0.; 2],
        start: [0.; 2],
        offset: [0.; 2],
        scale: 1. / face.units_per_em() as f32,
        vertical,
    };
    let mut pen = [0.; 2];
    for (info, pos) in result.glyph_infos().iter().zip(result.glyph_positions()) {
        outline.offset = [pen[0] + pos.x_offset as f32, pen[1] + pos.y_offset as f32];
        face.outline_glyph(ttf_parser::GlyphId(info.glyph_id as u16), &mut outline);
        pen[0] += pos.x_advance as f32;
        pen[1] += pos.y_advance as f32;
    }
    Some(GlyphShape {
        advance: if vertical { -pen[1] } else { pen[0] } * outline.scale,
        edges: outline.edges,
    })
}
fn japanese(value: &str) -> bool {
    value.chars().next().is_some_and(|c| matches!(c as u32, 0x3000..=0x9fff | 0xf900..=0xfaff | 0xff01..=0xff60 | 0xffe0..=0xffe6 | 0x20000..=0x323af))
}
fn extent(shape: &GlyphShape, y: f32) -> Option<[f32; 2]> {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for &(a, b) in &shape.edges {
        if (a[1] <= y && y < b[1]) || (b[1] <= y && y < a[1]) {
            let x = a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]);
            min = min.min(x);
            max = max.max(x);
        }
    }
    min.is_finite().then_some([min, max])
}
fn optical(left: &GlyphShape, right: &GlyphShape) -> f32 {
    let low = left
        .edges
        .iter()
        .chain(&right.edges)
        .flat_map(|(a, b)| [a[1], b[1]])
        .fold(f32::INFINITY, f32::min);
    let high = left
        .edges
        .iter()
        .chain(&right.edges)
        .flat_map(|(a, b)| [a[1], b[1]])
        .fold(f32::NEG_INFINITY, f32::max);
    let mut clearance = f32::INFINITY;
    for i in 0..BANDS {
        let y = low + (high - low) * (i as f32 + 0.5) / BANDS as f32;
        if let (Some(a), Some(b)) = (extent(left, y), extent(right, y)) {
            clearance = clearance.min(left.advance + b[0] - a[1]);
        }
    }
    if clearance.is_finite() {
        (0.08 - clearance).clamp(-0.25, 0.25)
    } else {
        0.
    }
}

/// Pair adjustments in document pixels, before tracking. Offsets are UTF-16.
/// `native_delta` cancels the font's existing pair kerning for an AppKit override.
#[derive(Clone, Debug)]
pub struct Adjustment {
    pub start: usize,
    pub length: usize,
    pub amount: f32,
    pub native_delta: f32,
}
#[derive(Clone, Debug)]
pub struct Advance {
    pub start: usize,
    pub length: usize,
    pub advance: f32,
}
pub fn adjustments(text: &VectorText, color: [u8; 3]) -> Vec<Adjustment> {
    spacing(text, color)
        .into_iter()
        .filter_map(|(advance, kern)| {
            kern.map(|(amount, native_delta)| Adjustment {
                start: advance.start,
                length: advance.length,
                amount,
                native_delta,
            })
        })
        .collect()
}
pub fn advances(text: &VectorText, color: [u8; 3]) -> Vec<Advance> {
    spacing(text, color)
        .into_iter()
        .map(|(advance, _)| advance)
        .collect()
}
fn spacing(text: &VectorText, color: [u8; 3]) -> Vec<(Advance, Option<(f32, f32)>)> {
    let mut fonts = super::system_fonts();
    let vertical = text.writing_mode == WritingMode::Vertical;
    let mut cache: HashMap<(usvg::fontdb::ID, String), GlyphShape> = HashMap::new();
    let mut pairs: HashMap<(usvg::fontdb::ID, String), f32> = HashMap::new();
    let mut support = HashMap::new();
    let resolver = super::font_resolver();
    let mut parts = Vec::new();
    let mut at = 0;
    for g in text.content.graphemes(true) {
        let style = text.style_at(at, color);
        let mut id = super::find_style_font(&fonts, &style);
        if let Some(base) = id {
            if !super::font_supports(&fonts, base, g, &mut support) {
                if let Some(c) = g
                    .chars()
                    .find(|c| !super::font_supports(&fonts, base, &c.to_string(), &mut support))
                {
                    id = (resolver.select_fallback)(c, &[base], &mut fonts).or(id);
                }
            }
        }
        parts.push((at, g, style, id));
        at += g.encode_utf16().count();
    }
    let mut output = Vec::new();
    for (i, (at, g, style, id)) in parts.iter().enumerate() {
        let Some(id) = *id else {
            continue;
        };
        let left = cache
            .entry((id, (*g).into()))
            .or_insert_with(|| {
                fonts
                    .with_face_data(id, |data, index| shape(data, index, g, vertical, false))
                    .flatten()
                    .unwrap_or(GlyphShape {
                        advance: 1.,
                        edges: Vec::new(),
                    })
            })
            .clone();
        let next = parts
            .get(i + 1)
            .filter(|(_, g, _, _)| !g.chars().any(char::is_whitespace));
        let mut metric = 0.;
        if let Some((_, other, other_style, Some(other_id))) = next {
            if *other_id == id
                && compatible(style, other_style)
                && !g.chars().any(char::is_whitespace)
            {
                let pair = format!("{g}{other}");
                metric = *pairs.entry((id, pair.clone())).or_insert_with(|| {
                    fonts
                        .with_face_data(id, |data, index| {
                            Some(
                                shape(data, index, &pair, vertical, true)?.advance
                                    - shape(data, index, &pair, vertical, false)?.advance,
                            )
                        })
                        .flatten()
                        .unwrap_or(0.)
                });
            }
        }
        let custom = if g.chars().any(char::is_whitespace) {
            None
        } else if style.kerning == KerningMode::JapaneseMonospaced && japanese(g) {
            Some(1. - left.advance)
        } else if style.kerning == KerningMode::Optical {
            next.and_then(|(_, other, other_style, other_id)| {
                if !compatible(style, other_style) {
                    return None;
                }
                let other_id = (*other_id)?;
                let right = cache.entry((other_id, (*other).into())).or_insert_with(|| {
                    fonts
                        .with_face_data(other_id, |data, index| {
                            shape(data, index, other, vertical, false)
                        })
                        .flatten()
                        .unwrap_or(GlyphShape {
                            advance: 1.,
                            edges: Vec::new(),
                        })
                });
                Some(optical(&left, right))
            })
        } else {
            None
        };
        let desired = custom.unwrap_or(metric);
        let factor = style.font_size
            * if vertical {
                style.scale_y
            } else {
                style.scale_x
            };
        let advance = if g.contains('\n') || g.contains('\r') {
            0.
        } else {
            ((left.advance + desired) * factor + style.tracking * style.font_size / 1000.).max(0.1)
        };
        output.push((
            Advance {
                start: *at,
                length: g.encode_utf16().count(),
                advance,
            },
            custom.map(|desired| (desired * factor, (desired - metric) * factor)),
        ));
    }
    output
}
fn compatible(a: &TextStyle, b: &TextStyle) -> bool {
    a.font_size == b.font_size
        && a.scale_x == b.scale_x
        && a.scale_y == b.scale_y
        && a.baseline_shift == b.baseline_shift
        && a.rotation == b.rotation
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle(left: f32, right: f32) -> GlyphShape {
        GlyphShape {
            advance: 1.,
            edges: vec![
                ([left, 0.], [right, 0.]),
                ([right, 0.], [right, 1.]),
                ([right, 1.], [left, 1.]),
                ([left, 1.], [left, 0.]),
            ],
        }
    }
    #[test]
    fn optical_uses_contour_clearance_and_limits_compression() {
        assert!((optical(&rectangle(0.1, 0.9), &rectangle(0.1, 0.9)) + 0.12).abs() < 0.0001);
        assert_eq!(optical(&rectangle(0.3, 0.7), &rectangle(0.3, 0.7)), -0.25);
    }
    #[test]
    fn modes_use_actual_fonts_and_preserve_utf16_and_whitespace() {
        let mut text = VectorText {
            content: "AV AV\nA".into(),
            font_family: "Arial".into(),
            font_size: 100.,
            kerning: KerningMode::Optical,
            ..Default::default()
        };
        if super::super::find_style_font(&super::super::system_fonts(), &text.base_style([0, 0, 0]))
            .is_none()
        {
            return;
        }
        let values = adjustments(&text, [0, 0, 0]);
        assert_eq!(
            values.iter().map(|a| a.start).collect::<Vec<_>>(),
            vec![0, 3]
        );
        assert!(values
            .iter()
            .all(|a| a.amount.is_finite() && a.amount.abs() <= 25.));
        text.kerning = KerningMode::Metrics;
        assert!(adjustments(&text, [0, 0, 0]).is_empty());
        text.content = "A𠮷日本A".into();
        text.kerning = KerningMode::JapaneseMonospaced;
        let widths = advances(&text, [0, 0, 0]);
        assert_eq!(
            widths.iter().map(|a| a.start).collect::<Vec<_>>(),
            vec![0, 1, 3, 4, 5]
        );
        for a in &widths[1..4] {
            assert!((a.advance - 100.).abs() < 0.001);
        }
        text.tracking = 100.;
        let tracked = advances(&text, [0, 0, 0]);
        for (a, b) in tracked.iter().zip(widths) {
            assert!((a.advance - b.advance - 10.).abs() < 0.001);
        }
        text.writing_mode = WritingMode::Vertical;
        for a in &advances(&text, [0, 0, 0])[1..4] {
            assert!((a.advance - 110.).abs() < 0.001);
        }
    }
    #[test]
    fn custom_kerning_layout_exports_positions_for_both_directions() {
        use lumapaint_core::document::Document;
        use lumapaint_formats::native::NativeDocumentCodec;
        let mut text = VectorText {
            content: "AV日本AV".into(),
            font_family: "sans-serif".into(),
            font_size: 40.,
            kerning: KerningMode::Optical,
            box_width: 1000.,
            ..Default::default()
        };
        if super::super::system_fonts().faces().next().is_none() {
            return;
        }
        super::super::reflow_text_with_system_fonts(&mut text, [0, 0, 0]).unwrap();
        assert!(!text.glyph_clusters[0].is_empty());
        let mut doc = Document::default();
        doc.set_text_object(lumapaint_core::document::TextSettings {
            id: None,
            text: text.clone(),
            position: [10., 10.],
            color: [0, 0, 0],
        })
        .unwrap();
        let source = doc.svg_layers().next().unwrap().source.clone();
        assert!(source.contains("font-kerning=\"none\""));
        let expected = doc.snapshot().text_objects[0].text.clone();
        let saved = doc.encode().unwrap();
        let restored = Document::decode(&saved).unwrap();
        assert_eq!(restored.snapshot().text_objects[0].text, expected);
        doc.undo();
        assert!(doc.snapshot().text_objects.is_empty());
        doc.redo();
        assert_eq!(doc.svg_layers().next().unwrap().source, source);
        text.writing_mode = WritingMode::Vertical;
        super::super::reflow_text_with_system_fonts(&mut text, [0, 0, 0]).unwrap();
        assert_eq!(text.character_origins[0].len(), 6);
        assert!(text.character_origins[0].windows(2).all(|p| p[1] > p[0]));
    }
    #[test]
    fn monospaced_is_limited_to_full_width_characters() {
        assert!(japanese("あ"));
        assert!(japanese("「"));
        assert!(japanese("𠮷"));
        assert!(!japanese("A"));
        assert!(!japanese("ｱ"));
    }
}
