//! CPU vector rendering behind a premultiplied RGBA8 boundary shared with wgpu.
//! Existing complex SVGs keep the established resvg behavior; verified simple paths use Skia.
use resvg::{tiny_skia, usvg};
use std::sync::{Arc, OnceLock};

fn system_fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            configure_generic_font_families(&mut fonts);
            Arc::new(fonts)
        })
        .clone()
}

fn configure_generic_font_families(fonts: &mut usvg::fontdb::Database) {
    use usvg::fontdb::{Family, Query, Stretch, Style, Weight};
    let has_family = |fonts: &usvg::fontdb::Database, family| {
        fonts
            .query(&Query {
                families: &[family],
                weight: Weight::NORMAL,
                stretch: Stretch::Normal,
                style: Style::Normal,
            })
            .is_some()
    };
    let installed_family = |fonts: &usvg::fontdb::Database, needle: &str| {
        fonts
            .faces()
            .flat_map(|face| face.families.iter())
            .find(|(name, _)| name.to_ascii_lowercase().contains(needle))
            .map(|(name, _)| name.clone())
    };
    if !has_family(fonts, Family::SansSerif) {
        if let Some(name) = installed_family(fonts, "sans") {
            fonts.set_sans_serif_family(name);
        }
    }
    if !has_family(fonts, Family::Serif) {
        if let Some(name) = installed_family(fonts, "serif") {
            fonts.set_serif_family(name);
        }
    }
    if !has_family(fonts, Family::Monospace) {
        if let Some(name) = installed_family(fonts, "mono") {
            fonts.set_monospace_family(name);
        }
    }
}

/// May run on a worker at startup so the first text edit avoids a system font scan.
/// The same OnceLock is used by rendering, including when prewarming is still in progress.
pub fn prepare_text_fonts() {
    let _ = system_fonts();
}

/// Portable fallback for hosts without AppKit text layout. It uses the same
/// system font database and CJK fallback policy as SVG rendering. The native
/// macOS editor remains authoritative where available.
pub fn reflow_text_with_system_fonts(
    text: &mut lumapaint_core::vector::VectorText,
    color: [u8; 3],
) -> Result<(), String> {
    let source = text.clone();
    let mut candidate = text.clone();
    let mut measurer = PortableFontMeasurer {
        fonts: system_fonts(),
        resolver: font_resolver(),
        face_cache: std::collections::HashMap::new(),
        glyph_cache: std::collections::HashMap::new(),
    };
    candidate.reflow_soft_breaks_with(|content, start| {
        measurer.measure(&source, color, content, start)
    })?;
    let mut widths = Vec::new();
    let mut origins = Vec::new();
    let mut segment_origins = Vec::new();
    for (index, (start, line, hard_break_before)) in
        candidate.visual_lines().into_iter().enumerate()
    {
        let width = measurer.measure(&source, color, line, start)?;
        let first_indent = if index == 0 || hard_break_before {
            candidate.indent_first
        } else {
            0.0
        };
        let origin = match candidate.alignment {
            lumapaint_core::vector::TextAlignment::Left => candidate.indent_left + first_indent,
            lumapaint_core::vector::TextAlignment::Center => {
                (candidate.box_width + candidate.indent_left - candidate.indent_right
                    + first_indent
                    - width)
                    * 0.5
            }
            lumapaint_core::vector::TextAlignment::Right => {
                candidate.box_width - candidate.indent_right - width
            }
        };
        let starts = candidate.style_segment_starts(start, line, color);
        let mut line_segments = Vec::with_capacity(starts.len());
        let mut cursor = origin;
        for (segment_index, segment_start) in starts.iter().enumerate() {
            line_segments.push(cursor);
            let segment_end = starts
                .get(segment_index + 1)
                .copied()
                .unwrap_or(start + line.encode_utf16().count());
            let start_byte = utf16_byte_index(line, segment_start - start)
                .ok_or("Invalid text style boundary")?;
            let end_byte =
                utf16_byte_index(line, segment_end - start).ok_or("Invalid text style boundary")?;
            cursor +=
                measurer.measure(&source, color, &line[start_byte..end_byte], *segment_start)?;
        }
        widths.push(width);
        origins.push(origin);
        segment_origins.push(line_segments);
    }
    candidate.line_widths = widths;
    candidate.line_origins = origins;
    candidate.style_segment_origins = segment_origins;
    candidate.validate()?;
    *text = candidate;
    Ok(())
}

fn utf16_byte_index(content: &str, offset: usize) -> Option<usize> {
    if offset == 0 {
        return Some(0);
    }
    let mut units = 0;
    for (byte, character) in content.char_indices() {
        units += character.len_utf16();
        if units == offset {
            return Some(byte + character.len_utf8());
        }
        if units > offset {
            return None;
        }
    }
    None
}

struct PortableFontMeasurer {
    fonts: Arc<usvg::fontdb::Database>,
    resolver: usvg::FontResolver<'static>,
    face_cache: std::collections::HashMap<(String, bool, bool), usvg::fontdb::ID>,
    glyph_cache: std::collections::HashMap<(usvg::fontdb::ID, char), bool>,
}

impl PortableFontMeasurer {
    fn measure(
        &mut self,
        text: &lumapaint_core::vector::VectorText,
        color: [u8; 3],
        content: &str,
        start: usize,
    ) -> Result<f32, String> {
        use unicode_segmentation::UnicodeSegmentation;
        let mut width = 0.0;
        let mut offset = start;
        let mut group = String::new();
        let mut group_font = None;
        let mut group_style = None;
        let mut graphemes = 0;
        for grapheme in content.graphemes(true) {
            let style = text.style_at(offset, color);
            let face_key = (style.font_family.clone(), style.bold, style.italic);
            let base = if let Some(id) = self.face_cache.get(&face_key) {
                *id
            } else {
                let id = find_style_font(&self.fonts, &style).ok_or("No usable system font")?;
                self.face_cache.insert(face_key, id);
                id
            };
            let font = if font_supports(&self.fonts, base, grapheme, &mut self.glyph_cache) {
                base
            } else {
                let missing = grapheme
                    .chars()
                    .find(|character| {
                        !font_supports(
                            &self.fonts,
                            base,
                            &character.to_string(),
                            &mut self.glyph_cache,
                        )
                    })
                    .unwrap_or_else(|| grapheme.chars().next().unwrap_or(' '));
                (self.resolver.select_fallback)(missing, &[base], &mut self.fonts).unwrap_or(base)
            };
            if group_font != Some(font) || group_style.as_ref() != Some(&style) {
                if let (Some(id), Some(previous)) = (group_font, group_style.as_ref()) {
                    width += shape_width(&self.fonts, id, &group, previous, graphemes)?;
                }
                group.clear();
                graphemes = 0;
                group_font = Some(font);
                group_style = Some(style);
            }
            group.push_str(grapheme);
            graphemes += 1;
            offset += grapheme.encode_utf16().count();
        }
        if let (Some(id), Some(style)) = (group_font, group_style.as_ref()) {
            width += shape_width(&self.fonts, id, &group, style, graphemes)?;
        }
        Ok(width)
    }
}

fn find_style_font(
    fonts: &usvg::fontdb::Database,
    style: &lumapaint_core::vector::TextStyle,
) -> Option<usvg::fontdb::ID> {
    use usvg::fontdb::{Family, Query, Stretch, Style, Weight};
    let families: Vec<_> = match style.font_family.as_str() {
        "serif" => vec![
            Family::Name("Hiragino Mincho ProN"),
            Family::Name("Songti SC"),
            Family::Serif,
        ],
        "monospace" => vec![Family::Name("Menlo"), Family::Monospace],
        "sans-serif" => vec![
            Family::Name("Hiragino Sans"),
            Family::Name("PingFang SC"),
            Family::SansSerif,
        ],
        name => vec![Family::Name(name), Family::SansSerif],
    };
    fonts.query(&Query {
        families: &families,
        weight: if style.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        },
        stretch: Stretch::Normal,
        style: if style.italic {
            Style::Italic
        } else {
            Style::Normal
        },
    })
}

fn font_supports(
    fonts: &usvg::fontdb::Database,
    id: usvg::fontdb::ID,
    content: &str,
    cache: &mut std::collections::HashMap<(usvg::fontdb::ID, char), bool>,
) -> bool {
    let unknown: Vec<_> = content
        .chars()
        .filter(|character| !character.is_control() && !cache.contains_key(&(id, *character)))
        .collect();
    if !unknown.is_empty() {
        let support = fonts.with_face_data(id, |data, index| {
            ttf_parser::Face::parse(data, index).ok().map(|face| {
                unknown
                    .iter()
                    .map(|character| (*character, face.glyph_index(*character).is_some()))
                    .collect::<Vec<_>>()
            })
        });
        if let Some(Some(support)) = support {
            for (character, supported) in support {
                cache.insert((id, character), supported);
            }
        } else {
            return false;
        }
    }
    content
        .chars()
        .filter(|character| !character.is_control())
        .all(|character| cache.get(&(id, character)) == Some(&true))
}

fn shape_width(
    fonts: &usvg::fontdb::Database,
    id: usvg::fontdb::ID,
    content: &str,
    style: &lumapaint_core::vector::TextStyle,
    graphemes: usize,
) -> Result<f32, String> {
    let advance = fonts
        .with_face_data(id, |data, index| {
            let face = rustybuzz::Face::from_slice(data, index)?;
            let mut buffer = rustybuzz::UnicodeBuffer::new();
            buffer.push_str(content);
            buffer.guess_segment_properties();
            let glyphs = rustybuzz::shape(&face, &[], buffer);
            let units: i64 = glyphs
                .glyph_positions()
                .iter()
                .map(|glyph| i64::from(glyph.x_advance))
                .sum();
            Some(units as f32 * style.font_size / face.units_per_em() as f32)
        })
        .flatten()
        .ok_or("Unable to shape system font")?;
    Ok((advance.abs() + graphemes as f32 * style.tracking * style.font_size / 1000.0).max(0.0))
}

fn font_resolver() -> usvg::FontResolver<'static> {
    let fallback = usvg::FontResolver::default_fallback_selector();
    usvg::FontResolver {
        select_font: usvg::FontResolver::default_font_selector(),
        select_fallback: Box::new(move |character, excluded, database| {
            // Database iteration order can otherwise choose a thin serif for bold CJK text.
            // Prefer matching CJK families and weight, while retaining the standard fallback.
            if let Some(base) = excluded.first().and_then(|id| database.face(*id)) {
                let serif = base.families.iter().any(|(name, _)| {
                    name.contains("Mincho") || name.contains("Songti") || name.contains("Serif")
                });
                let families = if serif {
                    [
                        "Songti SC",
                        "Hiragino Mincho ProN",
                        "Noto Serif CJK SC",
                        "Noto Serif CJK JP",
                    ]
                } else {
                    [
                        "PingFang SC",
                        "Hiragino Sans",
                        "Noto Sans CJK SC",
                        "Noto Sans CJK JP",
                    ]
                };
                for family in families {
                    let id = database.query(&usvg::fontdb::Query {
                        families: &[usvg::fontdb::Family::Name(family)],
                        weight: base.weight,
                        stretch: base.stretch,
                        style: base.style,
                    });
                    if let Some(id) = id.filter(|id| !excluded.contains(id)) {
                        let supported = database
                            .with_face_data(id, |data, index| {
                                ttf_parser::Face::parse(data, index)
                                    .ok()
                                    .and_then(|face| face.glyph_index(character))
                                    .is_some()
                            })
                            .unwrap_or(false);
                        if supported {
                            return Some(id);
                        }
                    }
                }
            }
            fallback(character, excluded, database)
        }),
    }
}

#[cfg(feature = "skia")]
pub mod skia_paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgBackend {
    Resvg,
    Skia,
}

pub struct SvgRaster {
    /// Packed, top-to-bottom RGBA8, premultiplied in sRGB encoding. Exactly width*height*4 bytes.
    pub pixels: Vec<u8>,
    pub backend: SvgBackend,
    /// True only when the uncropped geometry (including strokes) fits in the texture.
    pub fully_contained: bool,
}

/// Fit and center SVG in document pixels. Never resolve external files or network URLs.
pub fn rasterize_svg(source: &str, width: u32, height: u32) -> Result<SvgRaster, String> {
    if source.len() > 4 * 1024 * 1024 || width == 0 || height == 0 || width > 8192 || height > 8192
    {
        return Err("Invalid SVG source size or raster dimensions".into());
    }
    let options = usvg::Options {
        fontdb: system_fonts(),
        font_resolver: font_resolver(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(source, &options).map_err(|error| error.to_string())?;
    let size = tree.size();
    let scale = (width as f32 / size.width()).min(height as f32 / size.height());
    let x = (width as f32 - size.width() * scale) * 0.5;
    let y = (height as f32 - size.height() * scale) * 0.5;

    let bounds = tree.root().abs_layer_bounding_box();
    // One pixel of padding prevents reusing a texture whose antialiasing was clipped.
    let fully_contained = bounds.left() * scale + x >= 1.0
        && bounds.top() * scale + y >= 1.0
        && bounds.right() * scale + x <= width as f32 - 1.0
        && bounds.bottom() * scale + y <= height as f32 - 1.0;

    #[cfg(feature = "skia")]
    if supports_skia(tree.root()) {
        // Normalize with the same parser as the compatibility renderer. No raw resource URLs
        // or unsupported SVG constructs cross into the Skia parser.
        let normalized = tree.to_string(&usvg::WriteOptions::default());
        let dom = skia_safe::svg::Dom::from_str(&normalized, skia_safe::FontMgr::empty())
            .map_err(|error| format!("Skia SVG: {error}"))?;
        let mut surface = skia_safe::surfaces::raster_n32_premul((width as i32, height as i32))
            .ok_or("Skia raster allocation failed")?;
        surface.canvas().clear(skia_safe::Color::TRANSPARENT);
        surface.canvas().translate((x, y)).scale((scale, scale));
        dom.render(surface.canvas());
        // N32 is platform-specific (often BGRA); explicitly convert to the compositor's RGBA.
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut pixels = vec![0; width as usize * height as usize * 4];
        if !surface.read_pixels(&info, &mut pixels, width as usize * 4, (0, 0)) {
            return Err("Skia pixel conversion failed".into());
        }
        return Ok(SvgRaster {
            pixels,
            backend: SvgBackend::Skia,
            fully_contained,
        });
    }

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("SVG raster allocation failed")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, x, y),
        &mut pixmap.as_mut(),
    );
    Ok(SvgRaster {
        pixels: pixmap.take(),
        backend: SvgBackend::Resvg,
        fully_contained,
    })
}

#[cfg(feature = "skia")]
fn supports_skia(group: &usvg::Group) -> bool {
    if group.clip_path().is_some()
        || group.mask().is_some()
        || !group.filters().is_empty()
        || group.blend_mode() != usvg::BlendMode::Normal
        || group.isolate()
        || group.opacity().get() != 1.0
    {
        return false;
    }
    group.children().iter().all(|node| match node {
        usvg::Node::Group(group) => supports_skia(group),
        usvg::Node::Path(path) => {
            path.paint_order() == usvg::PaintOrder::FillAndStroke
                && path.rendering_mode() == usvg::ShapeRendering::GeometricPrecision
                && path
                    .fill()
                    .is_none_or(|fill| matches!(fill.paint(), usvg::Paint::Color(_)))
                && path
                    .stroke()
                    .is_none_or(|stroke| matches!(stroke.paint(), usvg::Paint::Color(_)))
        }
        usvg::Node::Image(_) | usvg::Node::Text(_) => false,
    })
}

/// Encode premultiplied document pixels for clipboard transfer.
pub fn clipboard_png(width: u32, height: u32, pixels: Vec<u8>) -> Result<Vec<u8>, String> {
    if u64::from(width) * u64::from(height) > 16_777_216 {
        return Err("Clipboard image is too large (16 megapixels maximum)".into());
    }
    let size = tiny_skia::IntSize::from_wh(width, height).ok_or("Invalid image size")?;
    tiny_skia::Pixmap::from_vec(pixels, size)
        .ok_or("Invalid image pixels")?
        .encode_png()
        .map_err(|e| e.to_string())
}

pub fn clipboard_png_size(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() < 24 || bytes.len() > 8 * 1024 * 1024 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("Unsupported clipboard image".into());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > 16_777_216
    {
        return Err("Clipboard image is too large".into());
    }
    let image = tiny_skia::Pixmap::decode_png(bytes).map_err(|e| e.to_string())?;
    Ok((image.width(), image.height()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_sans_font_repairs_missing_generic_family() {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        if !fonts.faces().any(|face| {
            face.families
                .iter()
                .any(|(name, _)| name.to_ascii_lowercase().contains("sans"))
        }) {
            return;
        }
        fonts.set_sans_serif_family("LumaPaint missing generic font");
        let generic_sans = |fonts: &usvg::fontdb::Database| {
            fonts.query(&usvg::fontdb::Query {
                families: &[usvg::fontdb::Family::SansSerif],
                weight: usvg::fontdb::Weight::NORMAL,
                stretch: usvg::fontdb::Stretch::Normal,
                style: usvg::fontdb::Style::Normal,
            })
        };
        assert!(generic_sans(&fonts).is_none());
        configure_generic_font_families(&mut fonts);
        assert!(generic_sans(&fonts).is_some());
    }

    #[test]
    fn portable_system_font_reflow_handles_mixed_multilingual_text() {
        use lumapaint_core::vector::{TextStylePatch, VectorText};
        if system_fonts().faces().next().is_none() {
            return;
        }
        let mut text = VectorText {
            content: "Hello 日本語 简体中文 Hello".into(),
            font_family: "sans-serif".into(),
            font_size: 30.0,
            box_width: 120.0,
            ..Default::default()
        };
        text.apply_style(
            6,
            9,
            &TextStylePatch {
                bold: Some(true),
                ..Default::default()
            },
            [0, 0, 0],
        )
        .unwrap();
        let runs = text.runs.clone();
        reflow_text_with_system_fonts(&mut text, [0, 0, 0]).unwrap();
        assert!(!text.soft_breaks.is_empty());
        assert_eq!(text.runs, runs);
        assert_eq!(text.line_widths.len(), text.visual_lines().len());
        assert_eq!(text.line_origins.len(), text.visual_lines().len());
        assert_eq!(text.style_segment_origins.len(), text.visual_lines().len());
        assert!(text.line_widths.iter().any(|width| *width > 0.0));
        assert_eq!(
            text.visual_lines()
                .iter()
                .map(|(_, line, _)| *line)
                .collect::<String>(),
            text.content
        );
        text.validate().unwrap();

        let mut small = VectorText {
            content: "Hello world Hello world".into(),
            font_family: "sans-serif".into(),
            font_size: 12.0,
            box_width: 120.0,
            ..Default::default()
        };
        let mut large = VectorText {
            font_size: 36.0,
            ..small.clone()
        };
        reflow_text_with_system_fonts(&mut small, [0, 0, 0]).unwrap();
        reflow_text_with_system_fonts(&mut large, [0, 0, 0]).unwrap();
        assert!(large.soft_breaks.len() > small.soft_breaks.len());

        let mut aligned = VectorText {
            content: "ABCD".into(),
            font_family: "sans-serif".into(),
            font_size: 30.0,
            box_width: 200.0,
            alignment: lumapaint_core::vector::TextAlignment::Right,
            ..Default::default()
        };
        aligned
            .apply_style(
                2,
                4,
                &TextStylePatch {
                    bold: Some(true),
                    ..Default::default()
                },
                [0, 0, 0],
            )
            .unwrap();
        reflow_text_with_system_fonts(&mut aligned, [0, 0, 0]).unwrap();
        assert!((aligned.line_origins[0] + aligned.line_widths[0] - 200.0).abs() < 0.01);
        assert_eq!(aligned.style_segment_origins[0].len(), 2);
        assert!((aligned.style_segment_origins[0][0] - aligned.line_origins[0]).abs() < 0.01);
        assert!(aligned.style_segment_origins[0][1] > aligned.style_segment_origins[0][0]);
    }

    fn pixel(image: &SvgRaster, width: usize, x: usize, y: usize) -> &[u8] {
        &image.pixels[(y * width + x) * 4..][..4]
    }

    #[test]
    fn drag_texture_reuse_requires_uncropped_geometry() {
        for (x, expected) in [(20, true), (-10, false), (300, false)] {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="80"><rect x="{x}" y="20" width="30" height="30" fill="red"/></svg>"#
            );
            assert_eq!(
                rasterize_svg(&svg, 240, 80).unwrap().fully_contained,
                expected
            );
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn saved_line_width_changes_rendered_advance() {
        let raster = |length: &str| {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="120"><text x="10" y="70" font-family="Arial" font-size="48"><tspan {length}>ABCD</tspan></text></svg>"#
            );
            rasterize_svg(&svg, 400, 120).unwrap()
        };
        let rightmost = |image: &SvgRaster| {
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, pixel)| pixel[3] > 32)
                .map(|(index, _)| index % 400)
                .max()
                .unwrap()
        };
        let natural = raster("");
        let measured = raster("textLength=\"220\" lengthAdjust=\"spacing\"");
        assert!(rightmost(&measured) > rightmost(&natural) + 40);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn saved_style_segment_origins_reach_mixed_style_document_rendering() {
        use lumapaint_core::{
            document::{Document, TextSettings},
            vector::{TextStylePatch, VectorText},
        };
        let mut text = VectorText {
            content: "ABCD".into(),
            font_family: "Arial".into(),
            font_size: 48.0,
            ..Default::default()
        };
        text.apply_style(
            2,
            4,
            &TextStylePatch {
                bold: Some(true),
                ..Default::default()
            },
            [0, 0, 0],
        )
        .unwrap();
        let mut doc = Document::default();
        let mut settings = TextSettings {
            id: None,
            text: text.clone(),
            position: [10.0, 30.0],
            color: [0, 0, 0],
        };
        doc.set_text_object(settings.clone()).unwrap();
        let natural = rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640).unwrap();
        settings.id = Some(doc.snapshot().text_objects[0].id.clone());
        settings.text.line_widths = vec![150.0];
        settings.text.line_origins = vec![0.0];
        settings.text.style_segment_origins = vec![vec![0.0, 100.0]];
        doc.set_text_object(settings.clone()).unwrap();
        let measured = rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640).unwrap();
        settings.text.line_widths = vec![220.0];
        doc.set_text_object(settings).unwrap();
        let wider = rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640).unwrap();
        let rightmost = |image: &SvgRaster| {
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, pixel)| pixel[3] > 32)
                .map(|(index, _)| index % 960)
                .max()
                .unwrap()
        };
        assert!(rightmost(&measured) > rightmost(&natural) + 10);
        assert!(rightmost(&wider) > rightmost(&measured) + 30);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn measured_line_origin_overrides_inherited_text_anchor() {
        let raster = |child_anchor: &str| {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="120"><text text-anchor="middle" font-family="Arial" font-size="48"><tspan x="120" y="70" {child_anchor}>ABCD</tspan></text></svg>"#
            );
            rasterize_svg(&svg, 400, 120).unwrap()
        };
        let leftmost = |image: &SvgRaster| {
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, pixel)| pixel[3] > 32)
                .map(|(index, _)| index % 400)
                .min()
                .unwrap()
        };
        assert!(leftmost(&raster("text-anchor=\"start\"")) > leftmost(&raster("")) + 35);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn per_character_size_and_color_reach_the_raster() {
        use lumapaint_core::{
            document::{Document, TextSettings},
            vector::{TextStylePatch, VectorText},
        };
        let mut text = VectorText {
            content: "ABCD".into(),
            font_family: "Arial".into(),
            font_size: 24.0,
            ..Default::default()
        };
        text.apply_style(
            1,
            2,
            &TextStylePatch {
                font_size: Some(72.0),
                color: Some([255, 0, 0]),
                bold: Some(true),
                ..Default::default()
            },
            [0, 0, 255],
        )
        .unwrap();
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text,
            position: [16.0, 100.0],
            color: [0, 0, 255],
        })
        .unwrap();
        let raster = rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640).unwrap();
        let bounds = |channel: usize| {
            let points: Vec<_> = raster
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, pixel)| {
                    pixel[channel] > 128 && pixel[3] > 128 && pixel[2 - channel] < 10
                })
                .map(|(i, _)| (i % 960, i / 960))
                .collect();
            assert!(!points.is_empty());
            (
                points.iter().map(|p| p.0).min().unwrap(),
                points.iter().map(|p| p.0).max().unwrap(),
                points.iter().map(|p| p.1).max().unwrap()
                    - points.iter().map(|p| p.1).min().unwrap(),
            )
        };
        let red = bounds(0);
        let blue = bounds(2);
        assert!(
            blue.0 < red.0 && red.1 < blue.1,
            "Only the middle character changes color"
        );
        assert!(
            red.2 > blue.2 * 2,
            "Only the selected character changes size"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn text_uses_system_fonts_and_renders_multilingual_lines() {
        for content in ["Hello", "日本語", "简体中文"] {
            let source = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="80"><text x="10" y="48" font-family="sans-serif" font-size="36" fill="blue">{content}</text></svg>"#
            );
            let raster = rasterize_svg(&source, 240, 80).unwrap();
            assert!(
                raster
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] > 0)
                    .count()
                    > 100,
                "Missing rendered text: {content}"
            );
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn saved_soft_breaks_place_glyphs_on_distinct_rendered_lines() {
        use lumapaint_core::document::{Document, TextSettings};
        use lumapaint_core::vector::VectorText;
        let mut text = VectorText {
            content: "ABCD".into(),
            font_family: "Arial".into(),
            font_size: 36.0,
            ..Default::default()
        };
        let mut document = Document::default();
        let settings = |text| TextSettings {
            id: None,
            text,
            position: [10.0, 10.0],
            color: [0, 0, 0],
        };
        document.set_text_object(settings(text.clone())).unwrap();
        let one_line =
            rasterize_svg(&document.svg_layers().next().unwrap().source, 960, 640).unwrap();
        text.soft_breaks = vec![2];
        document
            .set_text_object(TextSettings {
                id: Some(document.snapshot().text_objects[0].id.clone()),
                ..settings(text)
            })
            .unwrap();
        let wrapped =
            rasterize_svg(&document.svg_layers().next().unwrap().source, 960, 640).unwrap();
        let bottom = |image: &SvgRaster| {
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, pixel)| pixel[3] > 0)
                .map(|(index, _)| index / 960)
                .max()
                .unwrap()
        };
        assert!(bottom(&wrapped) > bottom(&one_line) + 30);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn cjk_fallback_renders_requested_weights_and_typography_moves_the_text() {
        let svg = |weight| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="80"><text x="10" y="48" font-family="Hiragino Sans" font-size="36" font-weight="{weight}">汉语</text></svg>"#
            )
        };
        let regular = rasterize_svg(&svg(400), 240, 80).unwrap();
        let bold = rasterize_svg(&svg(700), 240, 80).unwrap();
        // System font collections differ between local macOS installations and the
        // GitHub runner. Some collections expose the same CJK fallback face for both
        // requested weights, so a pixel inequality is not a portable assertion. The
        // document tests verify that font-weight survives SVG generation; here we
        // verify that each requested weight still resolves to drawable CJK glyphs.
        for (weight, raster) in [(400, &regular), (700, &bold)] {
            assert!(
                raster
                    .pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|pixel| pixel[3] > 0)
                    .count()
                    > 100,
                "Missing rendered CJK text for weight {weight}"
            );
        }

        use lumapaint_core::{
            document::{Document, TextSettings},
            vector::{TextAlignment, VectorText},
        };
        let mut doc = Document::default();
        let mut settings = TextSettings {
            id: None,
            text: VectorText {
                content: "Inline".into(),
                font_family: "Arial".into(),
                ..Default::default()
            },
            position: [0.0, 0.0],
            color: [0, 0, 0],
        };
        doc.set_text_object(settings.clone()).unwrap();
        let left = rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640).unwrap();
        settings.id = Some(doc.snapshot().text_objects[0].id.clone());
        settings.text.alignment = TextAlignment::Right;
        settings.text.underline = true;
        doc.set_text_object(settings).unwrap();
        let right = rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640).unwrap();
        let first_x = |image: &SvgRaster| {
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(_, pixel)| pixel[3] > 0)
                .map(|(index, _)| index % 960)
                .min()
                .unwrap()
        };
        assert!(first_x(&right) > first_x(&left) + 100);
    }

    #[test]
    fn simple_paths_preserve_fit_color_alpha_and_fill_rule() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><path d="M0 0H40V20H0Z M10 5H30V15H10Z" fill="red" fill-opacity="0.5" fill-rule="evenodd"/></svg>"#;
        let result = rasterize_svg(source, 80, 80).unwrap();
        assert_eq!(result.pixels.len(), 80 * 80 * 4);
        assert_eq!(pixel(&result, 80, 1, 1), [0, 0, 0, 0]); // centered letterbox
        assert_eq!(pixel(&result, 80, 40, 40), [0, 0, 0, 0]); // even-odd hole
        let red = pixel(&result, 80, 5, 40);
        assert!((127..=128).contains(&red[0]));
        assert_eq!(red[0], red[3]);
        assert_eq!(&red[1..3], [0, 0]); // premultiplied RGBA, not BGRA
        assert_eq!(
            result.backend,
            if cfg!(feature = "skia") {
                SvgBackend::Skia
            } else {
                SvgBackend::Resvg
            }
        );
    }

    #[test]
    fn complex_svg_keeps_compatibility_renderer() {
        let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><defs><linearGradient id="g"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><clipPath id="c"><rect width="10" height="20"/></clipPath></defs><rect width="20" height="20" fill="url(#g)" clip-path="url(#c)"/></svg>"##;
        let result = rasterize_svg(source, 20, 20).unwrap();
        assert_eq!(result.backend, SvgBackend::Resvg);
        assert_eq!(pixel(&result, 20, 15, 10), [0, 0, 0, 0]);
        assert_eq!(pixel(&result, 20, 5, 10)[3], 255);
    }

    #[test]
    fn transformed_dashed_stroke_preserves_position_and_gaps() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><g transform="translate(4 8) scale(2)"><path d="M0 0H16" fill="none" stroke="blue" stroke-width="2" stroke-dasharray="4 4" stroke-linecap="butt"/></g></svg>"#;
        let result = rasterize_svg(source, 40, 40).unwrap();
        assert_eq!(pixel(&result, 40, 8, 8), [0, 0, 255, 255]);
        assert_eq!(pixel(&result, 40, 16, 8), [0, 0, 0, 0]);
        assert_eq!(pixel(&result, 40, 24, 8), [0, 0, 255, 255]);
        assert_eq!(pixel(&result, 40, 8, 14), [0, 0, 0, 0]);
    }

    #[test]
    fn group_opacity_is_applied_after_overlapping_children() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="20"><g opacity="0.5"><rect width="20" height="20" fill="red"/><rect x="10" width="20" height="20" fill="red"/></g></svg>"#;
        let result = rasterize_svg(source, 30, 20).unwrap();
        assert_eq!(result.backend, SvgBackend::Resvg);
        assert_eq!(pixel(&result, 30, 5, 10), pixel(&result, 30, 15, 10));
        assert!((127..=128).contains(&pixel(&result, 30, 15, 10)[3]));
    }

    #[test]
    fn rejects_invalid_input_before_allocation() {
        for (w, h) in [(0, 8), (8, 0), (8193, 1), (1, u32::MAX)] {
            assert!(rasterize_svg("<svg/>", w, h).is_err());
        }
        assert!(rasterize_svg("not SVG", 8, 8).is_err());
        assert!(rasterize_svg(&" ".repeat(4 * 1024 * 1024 + 1), 8, 8).is_err());
    }

    #[test]
    fn external_images_do_not_enter_the_raster() {
        for href in [
            "file:///tmp/lumapaint-do-not-read.png",
            "https://example.invalid/image.png",
        ] {
            let source = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="8" height="8"><image width="8" height="8" xlink:href="{href}"/></svg>"#
            );
            assert!(rasterize_svg(&source, 8, 8)
                .unwrap()
                .pixels
                .iter()
                .all(|b| *b == 0));
        }
    }
}
