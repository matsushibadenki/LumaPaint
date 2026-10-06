//! Font metadata and bounded, outlined previews; font files remain in Rust.
use super::{font_resolver, system_fonts};
use resvg::usvg;
use serde::Serialize;
use std::sync::OnceLock;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontFace {
    pub postscript: String,
    pub family: String,
    pub style: String,
    pub weight: u16,
    pub italic: bool,
    pub japanese: bool,
    pub latin: bool,
    pub adobe: bool,
}
pub fn catalog() -> &'static [FontFace] {
    static FACES: OnceLock<Vec<FontFace>> = OnceLock::new();
    FACES.get_or_init(|| {
        let fonts = system_fonts();
        let mut seen = std::collections::HashSet::new();
        let mut output = Vec::new();
        for info in fonts.faces() {
            if info.post_script_name.is_empty()
                || info.post_script_name.starts_with('.')
                || !seen.insert(info.post_script_name.clone())
            {
                continue;
            }
            let metadata = fonts
                .with_face_data(info.id, |data, index| {
                    let face = ttf_parser::Face::parse(data, index).ok()?;
                    let name = face
                        .names()
                        .into_iter()
                        .filter(|n| n.name_id == ttf_parser::name_id::SUBFAMILY)
                        .find_map(|n| n.to_string());
                    Some((
                        name.unwrap_or_else(|| {
                            format!(
                                "{}{}",
                                info.weight.0,
                                if info.style != usvg::fontdb::Style::Normal {
                                    " Italic"
                                } else {
                                    ""
                                }
                            )
                        }),
                        ['あ', '漢'].iter().all(|c| face.glyph_index(*c).is_some()),
                        ['A', 'z'].iter().all(|c| face.glyph_index(*c).is_some()),
                    ))
                })
                .flatten();
            let Some((style, japanese, latin)) = metadata else {
                continue;
            };
            let adobe = match &info.source {
                usvg::fontdb::Source::File(p) | usvg::fontdb::Source::SharedFile(p, _) => {
                    p.components().any(|p| p.as_os_str() == "livetype")
                }
                _ => false,
            };
            output.push(FontFace {
                postscript: info.post_script_name.clone(),
                family: info
                    .families
                    .first()
                    .map_or_else(|| info.post_script_name.clone(), |n| n.0.clone()),
                style,
                weight: info.weight.0,
                italic: info.style != usvg::fontdb::Style::Normal,
                japanese,
                latin,
                adobe,
            });
        }
        output.sort_by(|a, b| {
            a.family
                .cmp(&b.family)
                .then(a.weight.cmp(&b.weight))
                .then(a.italic.cmp(&b.italic))
                .then(a.postscript.cmp(&b.postscript))
        });
        output
    })
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
pub fn preview(
    postscript: &str,
    sample: &str,
    size: f32,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, String> {
    if !(8.0..=96.).contains(&size)
        || !(120..=1024).contains(&width)
        || !(32..=512).contains(&height)
        || sample.chars().count() > 128
        || sample.chars().any(|c| c.is_control() && c != '\n')
    {
        return Err("Invalid font preview request".into());
    }
    let face = catalog()
        .iter()
        .find(|f| f.postscript == postscript)
        .ok_or("Font is unavailable")?;
    let mut text = lumapaint_core::vector::VectorText {
        content: sample.into(),
        font_family: postscript.into(),
        font_size: size,
        line_height: 1.2,
        box_width: (width - 16) as f32,
        bold: face.weight >= 600,
        italic: face.italic,
        ..Default::default()
    };
    super::reflow_text_with_system_fonts(&mut text, [225, 225, 225])?;
    let mut source = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><text fill="#e1e1e1" font-family="{}" font-size="{size}" font-weight="{}" font-style="{}">"##,
        escape(postscript),
        face.weight,
        if face.italic { "italic" } else { "normal" }
    );
    for (i, (_, line, _)) in text.visual_lines().into_iter().enumerate() {
        let y = size + (i as f32) * size * 1.2;
        if y - size > height as f32 {
            break;
        }
        source.push_str(&format!(r#"<tspan x="8" y="{y}">{}</tspan>"#, escape(line)));
    }
    source.push_str("</text></svg>");
    let options = usvg::Options {
        fontdb: system_fonts(),
        font_resolver: font_resolver(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(&source, &options).map_err(|e| e.to_string())?;
    let svg = tree.to_string(&usvg::WriteOptions::default());
    if svg.len() > 512 * 1024 {
        return Err("Font preview is too complex".into());
    }
    Ok(svg.into_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn previews_are_outlined_bounded_and_escape_input() {
        let Some(face) = catalog().iter().find(|f| f.latin) else {
            return;
        };
        let svg =
            String::from_utf8(preview(&face.postscript, "AV <&> 日本", 28., 320, 150).unwrap())
                .unwrap();
        assert!(svg.contains("<path"));
        assert!(!svg.contains("<text"));
        assert!(!svg.contains("<script"));
        assert!(preview(&face.postscript, &"A".repeat(129), 28., 320, 150).is_err());
        assert!(preview("missing-font", "A", 28., 320, 150).is_err());
        assert!(preview(&face.postscript, "A", f32::NAN, 320, 150).is_err());
    }
    #[test]
    fn exact_styles_resolve_by_postscript_name_in_default_svg_consumers() {
        let Some(face) = catalog()
            .iter()
            .find(|f| f.latin && (f.italic || f.weight != 400))
        else {
            return;
        };
        let mut db = system_fonts();
        let source = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="50"><text y="30" font-family="{}" font-weight="{}" font-style="{}">AV</text></svg>"#,
            escape(&face.postscript),
            face.weight,
            if face.italic { "italic" } else { "normal" }
        );
        let options = usvg::Options {
            fontdb: db.clone(),
            ..Default::default()
        };
        let tree = usvg::Tree::from_str(&source, &options).unwrap();
        let text = tree
            .root()
            .children()
            .iter()
            .find_map(|node| match node {
                usvg::Node::Text(text) => Some(text),
                _ => None,
            })
            .unwrap();
        let selector = usvg::FontResolver::default_font_selector();
        let id = selector(text.chunks()[0].spans()[0].font(), &mut db).unwrap();
        assert_eq!(db.face(id).unwrap().post_script_name, face.postscript);
        let style = lumapaint_core::vector::VectorText {
            font_family: face.postscript.clone(),
            bold: face.weight >= 600,
            italic: face.italic,
            ..Default::default()
        }
        .base_style([0, 0, 0]);
        assert_eq!(
            super::super::text_font_postscript_name(&style).as_deref(),
            Some(face.postscript.as_str())
        );
    }
    #[test]
    fn catalog_has_unique_postscript_names() {
        let names: std::collections::HashSet<_> = catalog().iter().map(|f| &f.postscript).collect();
        assert_eq!(names.len(), catalog().len());
    }
}
