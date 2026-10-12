//! Ordered centerline strokes from the attributed, separately licensed KanjiVG dataset.
use super::Clip;
use crate::{
    document::vector_svg,
    vector::{TextRun, VectorObject, WritingMode},
};
use std::{
    collections::BTreeMap,
    fmt::Write,
    sync::{Arc, Mutex, OnceLock},
};
use unicode_segmentation::UnicodeSegmentation;
#[derive(Debug)]
pub struct Stroke {
    pub path: String,
    pub length: f32,
}
#[derive(Debug)]
pub struct Glyph {
    pub strokes: Vec<Stroke>,
}
fn data() -> &'static BTreeMap<String, Vec<String>> {
    static DATA: OnceLock<BTreeMap<String, Vec<String>>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/kanjivg/strokes.json"))
            .expect("validated bundled stroke data")
    })
}
pub fn contains(character: char) -> bool {
    data().contains_key(&character.to_string())
}
type GlyphCache = Mutex<Vec<(char, Arc<Glyph>)>>;
pub fn glyph(character: char) -> Option<Arc<Glyph>> {
    static CACHE: OnceLock<GlyphCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(mut entries) = cache.lock() {
        if let Some(i) = entries.iter().position(|(c, _)| *c == character) {
            let entry = entries.remove(i);
            let value = entry.1.clone();
            entries.push(entry);
            return Some(value);
        }
    }
    let paths = data().get(&character.to_string())?;
    let glyph = Arc::new(Glyph {
        strokes: paths
            .iter()
            .map(|path| Stroke {
                path: path.clone(),
                length: crate::bezier::cubic_contours(path)
                    .ok()
                    .map(|parts| {
                        parts
                            .iter()
                            .map(|(p, _)| {
                                crate::bezier::flattened(p)
                                    .windows(2)
                                    .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
                                    .sum::<f32>()
                            })
                            .sum()
                    })
                    .unwrap_or(0.)
                    .max(0.001),
            })
            .collect(),
    });
    if let Ok(mut entries) = cache.lock() {
        if entries.len() >= 256 {
            entries.remove(0);
        }
        entries.push((character, glyph.clone()));
    }
    Some(glyph)
}
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,0x3400..=0x9fff|0x20000..=0x323af)
}
pub fn progress(glyph: &Glyph, p: f32, gap: f32) -> Vec<f32> {
    let total = glyph
        .strokes
        .iter()
        .map(|s| (s.length * 3.).max(150.) + gap)
        .sum::<f32>()
        - gap;
    let time = p.clamp(0., 1.) * total.max(1.);
    let mut start = 0.;
    glyph
        .strokes
        .iter()
        .map(|s| {
            let duration = (s.length * 3.).max(150.);
            let v = ((time - start) / duration).clamp(0., 1.);
            start += duration + gap;
            v
        })
        .collect()
}
pub fn source(object: &VectorObject, clip: &Clip, p: f32, dimensions: [u32; 2]) -> String {
    let Some(text) = &object.text else {
        return vector_svg(dimensions[0], dimensions[1], std::slice::from_ref(object));
    };
    let mut hidden = object.clone();
    let target = hidden.text.as_mut().unwrap();
    target.runs.clear();
    let color = object.fill.map_or([32, 32, 32], |paint| {
        [paint.color[0], paint.color[1], paint.color[2]]
    });
    let mut at = 0;
    let mut total = 0.;
    for g in text.content.graphemes(true) {
        let end = at + g.encode_utf16().count();
        let mut style = text.style_at(at, color);
        if let Some(character) = g.chars().next().filter(|_| g.chars().count() == 1) {
            if let Some(glyph) = glyph(character) {
                style.no_color = true;
                total += glyph
                    .strokes
                    .iter()
                    .map(|s| (s.length * 3.).max(150.) + clip.stroke_gap_ms)
                    .sum::<f32>();
            }
        }
        target.runs.push(TextRun {
            start: at,
            end,
            style,
        });
        at = end;
    }
    total = (total - clip.stroke_gap_ms).max(1.);
    let time = p.clamp(0., 1.) * total;
    let mut start = 0.;
    let mut output = vector_svg(dimensions[0], dimensions[1], std::slice::from_ref(&hidden));
    let mut strokes = String::new();
    let [a, b, c, d, e, f] = object.transform;
    let _=write!(strokes,"<metadata>KanjiVG © Ulrich Apel and contributors; CC BY-SA 3.0; https://kanjivg.tagaini.net/; normalized by LumaPaint</metadata><g transform=\"matrix({a} {b} {c} {d} {e} {f})\"><g transform=\"rotate({}) scale({} {})\">",text.rotation,text.scale_x,text.scale_y);
    for (line_index, (line_start, line, _)) in text.visual_lines().into_iter().enumerate() {
        let mut utf16 = line_start;
        let mut scalar = 0;
        let mut pen = text.line_origins.get(line_index).copied().unwrap_or(
            text.indent_left
                + if line_index == 0 {
                    text.indent_first
                } else {
                    0.
                },
        );
        let cross = text.line_baselines.get(line_index).copied().unwrap_or(
            if text.writing_mode == WritingMode::Horizontal {
                text.font_size * (1. + line_index as f32 * text.line_height) + text.space_before
            } else {
                text.box_width
                    - text.font_size * 0.5
                    - line_index as f32 * text.font_size * text.line_height
            },
        );
        for g in line.graphemes(true) {
            let style = text.style_at(utf16, color);
            let origin = text
                .character_origins
                .get(line_index)
                .and_then(|row| row.get(scalar))
                .copied()
                .unwrap_or(pen);
            let glyph = g
                .chars()
                .next()
                .filter(|_| g.chars().count() == 1)
                .and_then(glyph);
            if let Some(glyph) = glyph {
                let (x, y) = if text.writing_mode == WritingMode::Horizontal {
                    (origin, cross - style.font_size - style.baseline_shift)
                } else {
                    (cross - style.font_size * 0.5, origin)
                };
                let scale = style.font_size / 109.;
                let [r, g, b] = style.color;
                let _=write!(strokes,"<g transform=\"translate({x} {y}) rotate({}) scale({} {})\" fill=\"none\" stroke=\"rgb({r},{g},{b})\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",style.rotation,scale*style.scale_x,scale*style.scale_y,clip.stroke_width);
                for stroke in &glyph.strokes {
                    let duration = (stroke.length * 3.).max(150.);
                    let local = ((time - start) / duration).clamp(0., 1.);
                    let local = clip.curve.map_or(local, |curve| {
                        curve
                            .sample(local, duration * local / 1000., clip.seed)
                            .clamp(0., 1.)
                    });
                    if local > 0. && !style.no_color {
                        let _ = write!(
                            strokes,
                            "<path d=\"{}\" stroke-dasharray=\"{} {}\" stroke-dashoffset=\"{}\"/>",
                            stroke.path,
                            stroke.length,
                            stroke.length,
                            (1. - local) * stroke.length
                        );
                    }
                    start += duration + clip.stroke_gap_ms;
                }
                strokes.push_str("</g>");
            }
            pen = origin
                + style.font_size * if g.is_ascii() { 0.55 } else { 1. }
                + style.tracking * style.font_size / 1000.;
            utf16 += g.encode_utf16().count();
            scalar += g.chars().count();
        }
    }
    strokes.push_str("</g></g>");
    if let Some(end) = output.rfind("</svg>") {
        output.insert_str(end, &strokes);
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_bundled_stroke_has_valid_centerline_geometry() {
        assert_eq!(data().len(), 6626);
        for (character, strokes) in data() {
            assert_eq!(character.chars().count(), 1);
            assert!(!strokes.is_empty());
            for path in strokes {
                let contours = crate::bezier::cubic_contours(path)
                    .unwrap_or_else(|error| panic!("{character}: {error}"));
                assert!(!contours.is_empty(), "{character}");
                assert!(
                    contours
                        .iter()
                        .any(|(curves, _)| crate::bezier::flattened(curves)
                            .windows(2)
                            .any(|w| w[0] != w[1])),
                    "{character}"
                );
            }
        }
    }
    #[test]
    fn canonical_order_and_duration_follow_centerlines() {
        let day = glyph('日').unwrap();
        let eternity = glyph('永').unwrap();
        assert_eq!(day.strokes.len(), 4);
        assert_eq!(eternity.strokes.len(), 5);
        assert!(day.strokes[0].path.starts_with('M'));
        let start = progress(&eternity, 0., 100.);
        assert!(start.iter().all(|p| *p == 0.));
        let middle = progress(&eternity, 0.4, 100.);
        assert!(middle[0] > 0.);
        assert!(middle.last().unwrap() < &1.);
        assert!(middle.windows(2).all(|p| p[0] >= p[1]));
        assert!(progress(&eternity, 1., 100.).iter().all(|p| *p == 1.));
        assert!(!contains('🦀'));
    }
}
