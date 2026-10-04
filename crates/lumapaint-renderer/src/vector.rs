//! GPU-preferred vector rendering behind a premultiplied RGBA8 boundary shared with wgpu.
//! Existing complex SVGs keep the established resvg behavior; verified simple paths use Skia.
#[cfg(feature = "skia")]
pub mod pathfinder;
#[cfg(test)]
use lumapaint_formats::native::NativeDocumentCodec;
use resvg::{tiny_skia, usvg};
use std::sync::{Arc, OnceLock};

pub(crate) fn system_fonts() -> Arc<usvg::fontdb::Database> {
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
    if text.writing_mode == lumapaint_core::vector::WritingMode::Vertical {
        text.reflow_vertical();
        return text.validate();
    }
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
    let transformed = candidate.runs.iter().any(|run| {
        run.style.scale_x != 1.0 || run.style.scale_y != 1.0 || run.style.rotation != 0.0
    });
    let mut glyph_clusters = Vec::new();
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
            lumapaint_core::vector::TextAlignment::Justify => candidate.indent_left + first_indent,
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
        if transformed {
            use unicode_segmentation::UnicodeSegmentation;
            let mut clusters = Vec::new();
            let mut offset = 0;
            let mut cursor = origin;
            for grapheme in line.graphemes(true) {
                let end = offset + grapheme.encode_utf16().count();
                clusters.push(lumapaint_core::vector::TextGlyphCluster {
                    start: offset,
                    end,
                    x: cursor,
                });
                cursor += measurer.measure(&source, color, grapheme, start + offset)?;
                offset = end;
            }
            glyph_clusters.push(clusters);
        }
    }
    candidate.line_widths = widths;
    candidate.line_origins = origins;
    candidate.style_segment_origins = segment_origins;
    if transformed {
        candidate.glyph_clusters = glyph_clusters;
    }
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
    Ok((advance.abs() * style.scale_x
        + graphemes as f32 * style.tracking * style.font_size / 1000.0)
        .max(0.0))
}

/// Resolve the same face used by SVG before asking AppKit to instantiate it.
pub fn text_font_postscript_name(style: &lumapaint_core::vector::TextStyle) -> Option<String> {
    use usvg::fontdb::{Family, Query, Stretch, Style, Weight};
    let names: Vec<&str> = match style.font_family.as_str() {
        "sans-serif" => vec!["Hiragino Sans", "PingFang SC", "Noto Sans CJK JP", "Arial"],
        "serif" => vec!["Hiragino Mincho ProN", "Songti SC", "Noto Serif CJK JP"],
        "monospace" => vec!["Menlo", "Consolas", "Noto Sans Mono CJK JP"],
        name => vec![name],
    };
    let families: Vec<_> = names.into_iter().map(Family::Name).collect();
    let fonts = system_fonts();
    let id = fonts.query(&Query {
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
    })?;
    Some(fonts.face(id)?.post_script_name.clone())
}

pub(crate) fn font_resolver() -> usvg::FontResolver<'static> {
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

#[cfg(all(feature = "skia", target_os = "macos"))]
#[path = "vector_gpu.rs"]
mod gpu_cache;
#[cfg(feature = "skia")]
pub mod skia_paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgBackend {
    Resvg,
    Skia,
    SkiaGpu,
}

pub struct SvgRaster {
    /// Packed, top-to-bottom RGBA8, premultiplied in sRGB encoding. Exactly width*height*4 bytes.
    pub pixels: Vec<u8>,
    pub backend: SvgBackend,
    /// True only when the uncropped geometry (including strokes) fits in the texture.
    pub fully_contained: bool,
}

/// Fit and center SVG in document pixels. Never resolve external files or network URLs.
/// Document-space content bounds, excluding the empty outer SVG viewport.
pub fn pixel_source_bounds(source: &str) -> Result<[f32; 4], String> {
    if source.len() > 4 * 1024 * 1024 {
        return Err("Invalid image source size".into());
    }
    let options = usvg::Options {
        fontdb: system_fonts(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(source, &options).map_err(|e| e.to_string())?;
    let bounds = tree.root().abs_layer_bounding_box();
    Ok([bounds.x(), bounds.y(), bounds.width(), bounds.height()])
}

pub(crate) struct SvgRegionRaster {
    pub raster: SvgRaster,
    pub origin: (usize, usize),
    pub width: usize,
    pub rectangle: [f32; 4],
}

pub fn rasterize_svg(source: &str, width: u32, height: u32) -> Result<SvgRaster, String> {
    rasterize_svg_region(source, width, height, false, None).map(|region| region.raster)
}

#[cfg(all(test, feature = "skia"))]
thread_local! {
    static COMPATIBILITY_RENDERER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Geometry invariants must compare the same rasterizer on both sides.
/// Otherwise clip removal can switch resvg to Skia and measure AA differences.
#[cfg(all(test, feature = "skia"))]
pub(crate) fn with_compatibility_renderer<T>(render: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            COMPATIBILITY_RENDERER.with(|state| state.set(self.0));
        }
    }
    let _restore = Restore(COMPATIBILITY_RENDERER.with(|state| state.replace(true)));
    render()
}

/// Same SVG parser, transform, resource policy and render backend, with a smaller surface.
pub(crate) fn rasterize_svg_cropped(
    source: &str,
    width: u32,
    height: u32,
) -> Result<SvgRegionRaster, String> {
    rasterize_svg_region(source, width, height, true, None)
}

pub(crate) fn rasterize_svg_workspace(
    source: &str,
    document: [u32; 2],
    size: [u32; 2],
    rect: [f32; 4],
) -> Result<Vec<u8>, String> {
    if rect.iter().any(|value| !value.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return Err("Invalid workspace bounds".into());
    }
    rasterize_svg_region(source, size[0], size[1], false, Some((document, rect)))
        .map(|region| region.raster.pixels)
}

/// Object-local texture, including pixels outside the artboard. Its rectangle
/// remains in document coordinates and is translated only by the GPU.
#[cfg(all(feature = "skia", target_os = "macos"))]
pub(crate) fn rasterize_svg_object_shared(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    document: (u32, u32),
    opacity: f32,
) -> Option<(wgpu::Texture, [f32; 4])> {
    if source.len() > 4 * 1024 * 1024 {
        return None;
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
    let tree = usvg::Tree::from_str(source, &options).ok()?;
    if !supports_skia(tree.root()) || tree.root().children().is_empty() {
        return None;
    }
    let scale =
        (document.0 as f32 / tree.size().width()).min(document.1 as f32 / tree.size().height());
    let x = (document.0 as f32 - tree.size().width() * scale) * 0.5;
    let y = (document.1 as f32 - tree.size().height() * scale) * 0.5;
    let bounds = tree.root().abs_layer_bounding_box();
    let left = (bounds.left() * scale + x - 2.0).floor();
    let top = (bounds.top() * scale + y - 2.0).floor();
    let width = (bounds.right() * scale + x + 2.0).ceil() - left;
    let height = (bounds.bottom() * scale + y + 2.0).ceil() - top;
    if [left, top, width, height].iter().any(|v| !v.is_finite())
        || width <= 0.0
        || height <= 0.0
        || width > 8192.0
        || height > 8192.0
        || width * height * 4.0 > 64.0 * 1024.0 * 1024.0
        || width > device.limits().max_texture_dimension_2d as f32
        || height > device.limits().max_texture_dimension_2d as f32
    {
        return None;
    }
    let normalized = tree.to_string(&usvg::WriteOptions::default());
    let dom = skia_safe::svg::Dom::from_str(&normalized, skia_safe::FontMgr::empty()).ok()?;
    let info = skia_safe::ImageInfo::new(
        (width as i32, height as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let texture = gpu_cache::rasterize_shared(device, queue, &info, |surface| {
        let canvas = surface.canvas();
        canvas.clear(skia_safe::Color::TRANSPARENT);
        canvas.translate((x - left, y - top)).scale((scale, scale));
        if opacity != 1.0 {
            canvas.save_layer_alpha_f(None, opacity);
        }
        dom.render(canvas);
        if opacity != 1.0 {
            canvas.restore();
        }
    })?;
    Some((texture, [left, top, width, height]))
}

pub(crate) fn rasterize_svg_object(
    source: &str,
    document: (u32, u32),
) -> Result<(Vec<u8>, [f32; 4]), String> {
    if source.len() > 4 * 1024 * 1024 {
        return Err("SVG source too large".into());
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
    let tree = usvg::Tree::from_str(source, &options).map_err(|e| e.to_string())?;
    let scale =
        (document.0 as f32 / tree.size().width()).min(document.1 as f32 / tree.size().height());
    let x = (document.0 as f32 - tree.size().width() * scale) * 0.5;
    let y = (document.1 as f32 - tree.size().height() * scale) * 0.5;
    let region = rasterize_tree_region(&tree, [document.0, document.1], [scale, x, y], true, true)?;
    if region.raster.pixels.is_empty() {
        return Err("Empty object cache".into());
    }
    Ok((region.raster.pixels, region.rectangle))
}

fn rasterize_svg_region(
    source: &str,
    width: u32,
    height: u32,
    crop: bool,
    workspace: Option<([u32; 2], [f32; 4])>,
) -> Result<SvgRegionRaster, String> {
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
    let (scale, x, y) = if let Some((document, rect)) = workspace {
        let doc_scale = (document[0] as f32 / size.width()).min(document[1] as f32 / size.height());
        let pixels_per_unit = width as f32 / rect[2];
        (
            doc_scale * pixels_per_unit,
            ((document[0] as f32 - size.width() * doc_scale) * 0.5 - rect[0]) * pixels_per_unit,
            ((document[1] as f32 - size.height() * doc_scale) * 0.5 - rect[1]) * pixels_per_unit,
        )
    } else {
        let scale = (width as f32 / size.width()).min(height as f32 / size.height());
        (
            scale,
            (width as f32 - size.width() * scale) * 0.5,
            (height as f32 - size.height() * scale) * 0.5,
        )
    };

    rasterize_tree_region(&tree, [width, height], [scale, x, y], crop, false)
}

fn rasterize_tree_region(
    tree: &usvg::Tree,
    size: [u32; 2],
    transform: [f32; 3],
    crop: bool,
    object: bool,
) -> Result<SvgRegionRaster, String> {
    let [width, height] = size;
    let [scale, x, y] = transform;
    let bounds = tree.root().abs_layer_bounding_box();
    // One pixel of padding prevents reusing a texture whose antialiasing was clipped.
    let fully_contained = bounds.left() * scale + x >= 1.0
        && bounds.top() * scale + y >= 1.0
        && bounds.right() * scale + x <= width as f32 - 1.0
        && bounds.bottom() * scale + y <= height as f32 - 1.0;

    // Conservative drawable bounds including strokes/effects, rounded to integral pixels.
    // Integer origin and the unchanged document transform retain the full-surface AA phase.
    let coordinate = |value: f32, limit: u32| {
        if object {
            value as i32
        } else {
            value.clamp(0.0, limit as f32) as i32
        }
    };
    let (left, top, right, bottom) = if crop {
        (
            coordinate((bounds.left() * scale + x - 2.0).floor(), width),
            coordinate((bounds.top() * scale + y - 2.0).floor(), height),
            coordinate((bounds.right() * scale + x + 2.0).ceil(), width),
            coordinate((bounds.bottom() * scale + y + 2.0).ceil(), height),
        )
    } else {
        (0, 0, width as i32, height as i32)
    };
    let origin = (left.max(0) as usize, top.max(0) as usize);
    let rectangle = [
        left as f32,
        top as f32,
        (right as i64 - left as i64) as f32,
        (bottom as i64 - top as i64) as f32,
    ];
    if object
        && (rectangle[2] > 8192.0
            || rectangle[3] > 8192.0
            || rectangle[2] * rectangle[3] * 4.0 > 64.0 * 1024.0 * 1024.0)
    {
        return Err("Object cache exceeds raster budget".into());
    }
    if right <= left || bottom <= top || (crop && tree.root().children().is_empty()) {
        return Ok(SvgRegionRaster {
            raster: SvgRaster {
                pixels: Vec::new(),
                backend: SvgBackend::Resvg,
                fully_contained,
            },
            origin,
            rectangle,
            width: 0,
        });
    }
    let width = (right - left) as u32;
    let height = (bottom - top) as u32;
    let x = x - left as f32;
    let y = y - top as f32;

    #[cfg(feature = "skia")]
    if supports_skia(tree.root()) {
        // Normalize with the same parser as the compatibility renderer. No raw resource URLs
        // or unsupported SVG constructs cross into the Skia parser.
        let normalized = tree.to_string(&usvg::WriteOptions::default());
        let dom = skia_safe::svg::Dom::from_str(&normalized, skia_safe::FontMgr::empty())
            .map_err(|error| format!("Skia SVG: {error}"))?;
        // N32 is platform-specific (often BGRA); explicitly convert to the compositor's RGBA.
        let info = skia_safe::ImageInfo::new(
            (width as i32, height as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let draw = |surface: &mut skia_safe::Surface| {
            surface.canvas().clear(skia_safe::Color::TRANSPARENT);
            surface.canvas().translate((x, y)).scale((scale, scale));
            dom.render(surface.canvas());
        };
        #[cfg(target_os = "macos")]
        if let Some(pixels) = gpu_cache::rasterize(&info, draw) {
            return Ok(SvgRegionRaster {
                raster: SvgRaster {
                    pixels,
                    backend: SvgBackend::SkiaGpu,
                    fully_contained,
                },
                origin,
                rectangle,
                width: width as usize,
            });
        }
        let mut surface = skia_safe::surfaces::raster_n32_premul((width as i32, height as i32))
            .ok_or("Skia raster allocation failed")?;
        draw(&mut surface);
        let mut pixels = vec![0; width as usize * height as usize * 4];
        if !surface.read_pixels(&info, &mut pixels, width as usize * 4, (0, 0)) {
            return Err("Skia pixel conversion failed".into());
        }
        return Ok(SvgRegionRaster {
            raster: SvgRaster {
                pixels,
                backend: SvgBackend::Skia,
                fully_contained,
            },
            origin,
            rectangle,
            width: width as usize,
        });
    }

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("SVG raster allocation failed")?;
    resvg::render(
        tree,
        tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, x, y),
        &mut pixmap.as_mut(),
    );
    Ok(SvgRegionRaster {
        raster: SvgRaster {
            pixels: pixmap.take(),
            backend: SvgBackend::Resvg,
            fully_contained,
        },
        origin,
        rectangle,
        width: width as usize,
    })
}

#[cfg(feature = "skia")]
fn supports_skia(group: &usvg::Group) -> bool {
    #[cfg(test)]
    if COMPATIBILITY_RENDERER.with(|state| state.get()) {
        return false;
    }
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
                    .is_none_or(|fill| !matches!(fill.paint(), usvg::Paint::Pattern(_)))
                && path
                    .stroke()
                    .is_none_or(|stroke| !matches!(stroke.paint(), usvg::Paint::Pattern(_)))
        }
        usvg::Node::Text(text) => supports_skia(text.flattened()),
        usvg::Node::Image(_) => false,
    })
}

/// Validate the bounded, Rust-owned document raster before allocating memory.
/// Clipboard transfers have their own smaller limit and must not constrain editing.
pub fn document_rgba_len(width: u32, height: u32) -> Result<usize, String> {
    let limit = lumapaint_core::document::MAX_DOCUMENT_DIMENSION;
    if width == 0 || height == 0 || width > limit || height > limit {
        return Err("Image dimensions must be between 1 and 8192 pixels / 画像の幅・高さは1〜8192ピクセルにしてください / 图像宽高必须在1到8192像素之间".into());
    }
    Ok(width as usize * height as usize * 4)
}

/// Encode a document layer for persistence; full pixels stay on the Rust side.
pub fn document_png(width: u32, height: u32, pixels: Vec<u8>) -> Result<Vec<u8>, String> {
    let expected = document_rgba_len(width, height)?;
    if pixels.len() != expected {
        return Err("Invalid image pixels".into());
    }
    let size = tiny_skia::IntSize::from_wh(width, height).ok_or("Invalid image size")?;
    tiny_skia::Pixmap::from_vec(pixels, size)
        .ok_or("Invalid image pixels")?
        .encode_png()
        .map_err(|e| e.to_string())
}

/// Encode premultiplied document pixels for clipboard transfer.
pub fn clipboard_png(width: u32, height: u32, pixels: Vec<u8>) -> Result<Vec<u8>, String> {
    if u64::from(width) * u64::from(height) > 16_777_216 {
        return Err("Clipboard image is too large (16 megapixels maximum)".into());
    }
    document_png(width, height, pixels)
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
    #[test]
    fn internal_raster_limits_are_independent_from_clipboard_limits() {
        assert_eq!(
            super::document_rgba_len(3508, 4961).unwrap(),
            3508 * 4961 * 4
        );
        assert_eq!(
            super::document_rgba_len(8192, 8192).unwrap(),
            8192 * 8192 * 4
        );
        for (w, h) in [(0, 1), (1, 0), (8193, 1), (1, 8193), (u32::MAX, u32::MAX)] {
            assert!(super::document_rgba_len(w, h).is_err());
        }
        // Reject before allocation; actual clipboard limits are unchanged.
        assert!(super::clipboard_png(3508, 4961, Vec::new())
            .unwrap_err()
            .contains("16 megapixels"));
        assert!(super::document_png(3508, 4961, Vec::new())
            .unwrap_err()
            .contains("Invalid image pixels"));
    }

    #[test]
    fn document_clipping_paths_cover_all_layers_without_changing_artwork() {
        use lumapaint_core::document::{Document, TextSettings};
        use lumapaint_core::vector::{VectorObjectKind, VectorText};
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "seed".into(),
                ..Default::default()
            },
            position: [0., 0.],
            color: [255, 0, 0],
        })
        .unwrap();
        let layer = doc.svg_layers().next().unwrap();
        let id = layer.id.clone();
        let mut object = layer.vector_objects[0].clone();
        object.text = None;
        object.kind = VectorObjectKind::Path;
        object.path.data = "M20 20H100V100H20Z".into();
        object.control_points = vec![[20., 20.], [100., 20.], [100., 100.], [20., 100.]];
        doc.upsert_vector_object(&id, object).unwrap();
        let art = doc.svg_layers().next().unwrap().source.clone();
        doc.saved_path_action("create", None, "Cutout").unwrap();
        let path = doc.snapshot().saved_paths[0].id.clone();
        let before = doc.encode().unwrap();
        doc.saved_path_action("clip", Some(&path), "").unwrap();
        let saved = doc.encode().unwrap();
        let preview = doc.clipping_view();
        let cover = preview.svg_layers().last().unwrap();
        let raster = super::rasterize_svg(&cover.source, 960, 640).unwrap();
        assert_eq!(raster.pixels[(60 * 960 + 60) * 4 + 3], 0);
        assert_eq!(raster.pixels[(140 * 960 + 140) * 4 + 3], 255);
        assert_eq!(doc.svg_layers().next().unwrap().source, art);
        assert_eq!(doc.encode().unwrap(), saved);
        assert!(Document::decode(&saved).unwrap().has_document_clipping());
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.redo();
        assert_eq!(doc.encode().unwrap(), saved);
        doc.saved_path_action("delete", Some(&path), "").unwrap();
        assert!(!doc.has_document_clipping());
        doc.undo();
        assert!(doc.has_document_clipping());
        let before = doc.encode().unwrap();
        assert!(doc.saved_path_action("clip", Some("missing"), "").is_err());
        assert_eq!(doc.encode().unwrap(), before);
    }

    #[test]
    fn vertical_native_positions_are_preserved_in_preview_and_save() {
        use lumapaint_core::document::{Document, TextSettings};
        use lumapaint_core::vector::{VectorText, WritingMode};
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "日本語文".into(),
                writing_mode: WritingMode::Vertical,
                box_width: 120.,
                box_height: Some(200.),
                font_size: 20.,
                soft_breaks: vec![2],
                line_baselines: vec![12., 44.],
                line_origins: vec![3., 7.],
                character_origins: vec![vec![3., 27.], vec![7., 31.]],
                ..Default::default()
            },
            position: [0., 0.],
            color: [0, 0, 0],
        })
        .unwrap();
        let source = &doc.svg_layers().next().unwrap().source;
        assert!(source.contains("x=\"108\" y=\"3\""));
        assert!(source.contains("x=\"76\" y=\"7\""));
        assert!(source.contains("y=\"3 27\""));
        assert!(source.contains("y=\"7 31\""));
        let raster = super::rasterize_svg(source, 960, 640).unwrap();
        assert!(raster.pixels.iter().any(|v| *v != 0));
        let loaded = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(
            loaded.snapshot().text_objects[0].text.character_origins,
            vec![vec![3., 27.], vec![7., 31.]]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vertical_latin_is_rotated_once() {
        fn crop(source: &str) -> (usize, usize, Vec<bool>) {
            let raster = super::rasterize_svg(source, 240, 240).unwrap();
            let mut bounds = [240usize, 240, 0, 0];
            for y in 0..240 {
                for x in 0..240 {
                    if raster.pixels[(y * 240 + x) * 4 + 3] > 127 {
                        bounds[0] = bounds[0].min(x);
                        bounds[1] = bounds[1].min(y);
                        bounds[2] = bounds[2].max(x);
                        bounds[3] = bounds[3].max(y);
                    }
                }
            }
            let mut pixels = Vec::new();
            for y in bounds[1]..=bounds[3] {
                for x in bounds[0]..=bounds[2] {
                    pixels.push(raster.pixels[(y * 240 + x) * 4 + 3] > 127);
                }
            }
            (bounds[2] - bounds[0] + 1, bounds[3] - bounds[1] + 1, pixels)
        }
        for ch in ['F', 'R', 'a', '2'] {
            let vertical = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240"><text x="100" y="40" font-family="Hiragino Sans" font-size="80" writing-mode="tb">{ch}</text></svg>"#
            );
            let expected = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240"><text transform="translate(100 40) rotate(90)" font-family="Hiragino Sans" font-size="80">{ch}</text></svg>"#
            );
            let a = crop(&vertical);
            let b = crop(&expected);
            assert!(
                a.0.abs_diff(b.0) <= 1 && a.1.abs_diff(b.1) <= 1,
                "{ch}: {:?} {:?}",
                (a.0, a.1),
                (b.0, b.1)
            );
            let mismatches = (0..a.1.min(b.1))
                .flat_map(|y| (0..a.0.min(b.0)).map(move |x| (x, y)))
                .filter(|&(x, y)| a.2[y * a.0 + x] != b.2[y * b.0 + x])
                .count();
            assert!(
                (mismatches as f32) / (a.2.len() as f32) < 0.12,
                "Latin glyph is not a single clockwise rotation: {ch}"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn japanese_vertical_glyphs_use_font_alternates() {
        for ch in ['。', '、', 'ぁ', 'っ', 'ゃ', '「', 'ー'] {
            let source = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="180" height="180"><text x="80" y="20" font-family="Hiragino Sans" font-size="80" writing-mode="tb">{ch}</text></svg>"#
            );
            let raster = super::rasterize_svg(&source, 180, 180).unwrap();
            let mut bounds = [180usize, 180, 0, 0];
            for y in 0..180 {
                for x in 0..180 {
                    if raster.pixels[(y * 180 + x) * 4 + 3] > 30 {
                        bounds[0] = bounds[0].min(x);
                        bounds[1] = bounds[1].min(y);
                        bounds[2] = bounds[2].max(x);
                        bounds[3] = bounds[3].max(y);
                    }
                }
            }
            assert!(bounds[0] < bounds[2]);
            match ch {
                '。' | '、' => assert!(
                    bounds[0] > 80 && bounds[3] < 60,
                    "punctuation must sit at the upper right: {ch} {bounds:?}"
                ),
                'ぁ' | 'っ' | 'ゃ' => assert!(
                    bounds[0] + bounds[2] > 160 && bounds[3] < 100,
                    "small kana must use the upright vertical alternate: {ch} {bounds:?}"
                ),
                'ー' => assert!(
                    bounds[3] - bounds[1] > 3 * (bounds[2] - bounds[0]),
                    "prolonged mark must be vertical"
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn writing_direction_renders_vertical_columns_and_round_trips() {
        use lumapaint_core::document::{Document, TextSettings};
        use lumapaint_core::vector::{VectorText, WritingMode};
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "日本語の縦組み文字".into(),
                font_size: 24.,
                box_width: 140.,
                box_height: Some(100.),
                ..Default::default()
            },
            position: [20., 20.],
            color: [20, 40, 120],
        })
        .unwrap();
        let before = doc.encode().unwrap();
        doc.set_text_writing_mode(WritingMode::Vertical, super::reflow_text_with_system_fonts)
            .unwrap();
        let layer = doc.svg_layers().next().unwrap();
        assert!(layer.source.contains("writing-mode=\"tb\""));
        let text = layer.vector_objects[0].text.as_ref().unwrap();
        assert!(text.soft_breaks.len() >= 2);
        let raster = super::rasterize_svg(&layer.source, 960, 640).unwrap();
        let has_ink = |x0: usize, x1: usize| {
            (20..120).any(|y| (x0..x1).any(|x| raster.pixels[(y * 960 + x) * 4 + 3] > 0))
        };
        assert!(has_ink(130, 160));
        assert!(has_ink(90, 130));
        let saved = doc.encode().unwrap();
        assert_eq!(
            Document::decode(&saved).unwrap().snapshot().text_objects[0]
                .text
                .writing_mode,
            WritingMode::Vertical
        );
        doc.undo();
        assert_eq!(doc.encode().unwrap(), before);
        doc.redo();
        assert_eq!(doc.encode().unwrap(), saved);
        doc.set_text_writing_mode(
            WritingMode::Horizontal,
            super::reflow_text_with_system_fonts,
        )
        .unwrap();
        assert!(!doc
            .svg_layers()
            .next()
            .unwrap()
            .source
            .contains("writing-mode=\"tb\""));
    }

    #[test]
    fn outline_view_shows_path_edges_and_preserves_image_layers() {
        use lumapaint_core::document::{Document, TextSettings};
        use lumapaint_core::vector::{VectorObjectKind, VectorText};
        let mut doc = Document::default();
        doc.import_svg("Image".into(), r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect width="10" height="10" fill="red"/></svg>"#.into()).unwrap();
        let image = doc.svg_layers().next().unwrap().source.clone();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Shape".into(),
                ..Default::default()
            },
            position: [0., 0.],
            color: [255, 0, 0],
        })
        .unwrap();
        let layer = doc.svg_layers().find(|l| l.vector_layer).unwrap();
        let id = layer.id.clone();
        let mut shape = layer.vector_objects[0].clone();
        shape.text = None;
        shape.kind = VectorObjectKind::Path;
        shape.path.data = "M20 20H100V100H20Z".into();
        shape.control_points = vec![[20., 20.], [100., 20.], [100., 100.], [20., 100.]];
        doc.upsert_vector_object(&id, shape).unwrap();
        let preview = doc.outline_view([0., 0.], 1.);
        assert_eq!(preview.svg_layers().next().unwrap().source, image);
        let raster = rasterize_svg(
            &preview
                .svg_layers()
                .find(|l| l.vector_layer)
                .unwrap()
                .source,
            960,
            640,
        )
        .unwrap();
        assert_eq!(raster.pixels[(60 * 960 + 60) * 4 + 3], 0);
        assert!(raster.pixels[(20 * 960 + 60) * 4 + 3] > 0);
    }

    #[test]
    fn outline_view_changes_only_rendering_and_removes_text_fill() {
        use lumapaint_core::document::{Document, TextSettings};
        use lumapaint_core::vector::VectorText;
        let mut document = Document::default();
        document
            .set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: "MMMM".into(),
                    font_size: 80.,
                    ..Default::default()
                },
                position: [20., 20.],
                color: [220, 30, 50],
            })
            .unwrap();
        let saved = document.encode().unwrap();
        let before =
            rasterize_svg(&document.svg_layers().next().unwrap().source, 960, 640).unwrap();
        let outline = document.outline_view([0., 0.], 1.);
        let after = rasterize_svg(&outline.svg_layers().next().unwrap().source, 960, 640).unwrap();
        let alpha = |pixels: &[u8]| {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[3] as u64)
                .sum::<u64>()
        };
        assert!(alpha(&after.pixels) > 0);
        assert!(alpha(&after.pixels) < alpha(&before.pixels) / 2);
        assert!(after
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0)
            .all(|p| p[0] == p[1] && p[1] == p[2]));
        assert_eq!(document.encode().unwrap(), saved);
        assert_eq!(document.revision(), outline.revision());
        let restored =
            rasterize_svg(&document.svg_layers().next().unwrap().source, 960, 640).unwrap();
        assert_eq!(restored.pixels, before.pixels);
    }

    use super::*;

    #[test]
    fn fixed_height_text_frame_clips_overflow_in_raster_output() {
        use lumapaint_core::document::{Document, TextSettings};
        use lumapaint_core::vector::VectorText;
        let mut document = Document::default();
        document
            .set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: "MMMM\nMMMM".into(),
                    font_size: 28.0,
                    line_height: 1.5,
                    box_width: 150.0,
                    box_height: Some(38.0),
                    ..Default::default()
                },
                position: [20.0, 20.0],
                color: [0, 0, 0],
            })
            .unwrap();
        let source = &document.svg_layers().next().unwrap().source;
        let raster = rasterize_svg(source, 960, 640).unwrap();
        let alpha_at = |x: usize, y: usize| raster.pixels[(y * 960 + x) * 4 + 3];
        assert!((20..58).any(|y| (20..170).any(|x| alpha_at(x, y) != 0)));
        assert!((60..110).all(|y| (20..170).all(|x| alpha_at(x, y) == 0)));
    }

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
        if cfg!(feature = "skia") {
            assert!(matches!(
                result.backend,
                SvgBackend::Skia | SvgBackend::SkiaGpu
            ));
        } else {
            assert_eq!(result.backend, SvgBackend::Resvg);
        }
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

#[cfg(test)]
mod clipping_tests {
    use super::*;
    use lumapaint_core::{
        document::Document,
        vector::{FillRule, VectorObject, VectorObjectKind, VectorPaint, VectorPath},
    };
    #[test]
    fn object_appearance_renders_and_survives_save_undo() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let shape = |id: &str, color| VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.,
            blend_mode: "normal".into(),
            id: id.into(),
            name: id.into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: "M0 0H20V20H0Z".into(),
                fill_rule: FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint { color }),
            stroke: None,
            stroke_style: Default::default(),
            stroke_width: 0.,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[0., 0.], [20., 20.]],
            text: None,
        };
        doc.upsert_vector_object(&layer, shape("base", [255, 0, 0, 255]))
            .unwrap();
        doc.upsert_vector_object(&layer, shape("top", [0, 0, 255, 255]))
            .unwrap();
        let ids = vec!["top".into()];
        doc.select_vector_objects(ids.clone()).unwrap();
        doc.set_selected_vector_appearance(&ids, Some(0.5), Some("multiply".into()))
            .unwrap();
        let (w, h) = doc.dimensions();
        let render =
            |d: &Document| rasterize_svg(&d.svg_layers().next().unwrap().source, w, h).unwrap();
        let pixel = |r: &SvgRaster| {
            let i = (10 * w as usize + 10) * 4;
            r.pixels[i..i + 4].to_vec()
        };
        let mixed = pixel(&render(&doc));
        assert!((126..=129).contains(&mixed[0]), "{mixed:?}");
        assert_eq!(&mixed[1..], &[0, 0, 255]);
        let saved = doc.encode().unwrap();
        let reopened = Document::decode(&saved).unwrap();
        assert_eq!(mixed, pixel(&render(&reopened)));
        assert!(doc
            .set_selected_vector_appearance(&ids, Some(1.1), None)
            .is_err());
        assert!(doc
            .set_selected_vector_appearance(&ids, None, Some("invalid".into()))
            .is_err());
        assert_eq!(saved, doc.encode().unwrap());
        assert!(doc
            .vector_drag_runs(doc.svg_layers().next().unwrap())
            .is_none());
        doc.undo();
        assert_eq!(pixel(&render(&doc)), vec![0, 0, 255, 255]);
        doc.redo();
        assert_eq!(pixel(&render(&doc)), mixed);
        for mode in lumapaint_core::vector::OBJECT_BLEND_MODES {
            doc.set_selected_vector_appearance(&ids, None, Some((*mode).into()))
                .unwrap();
            render(&doc);
        }
    }

    #[test]
    fn clipping_create_edit_release_undo_and_save_render_correctly() {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        let shape = |id: &str, size: f32, color| VectorObject {
            live_corners: None,
            rectangle_radii: None,
            opacity: 1.0,
            blend_mode: "normal".into(),
            id: id.into(),
            name: id.into(),
            group_path: vec![],
            clipping_group: None,
            bounds_reset: false,
            path: VectorPath {
                data: format!("M0 0H{size}V{size}H0Z"),
                fill_rule: FillRule::NonZero,
            },
            transform: [1., 0., 0., 1., 0., 0.],
            image_frame: None,
            fill_gradient: None,
            stroke_gradient: None,
            fill: Some(VectorPaint { color }),
            stroke: None,
            stroke_style: Default::default(),
            stroke_width: 0.,
            visible: true,
            kind: VectorObjectKind::Rectangle,
            control_points: vec![[0., 0.], [size, size]],
            text: None,
        };
        doc.upsert_vector_object(&layer, shape("art", 80., [255, 0, 0, 255]))
            .unwrap();
        doc.upsert_vector_object(&layer, shape("mask", 40., [0, 0, 255, 255]))
            .unwrap();
        doc.select_vector_objects(vec!["mask".into(), "art".into()])
            .unwrap();
        let original = doc.svg_layers().next().unwrap().source.clone();
        doc.select_vector_objects(vec!["mask".into()]).unwrap();
        assert!(doc.clipping_path("create").is_err());
        assert_eq!(doc.svg_layers().next().unwrap().source, original);
        doc.select_vector_objects(vec!["mask".into(), "art".into()])
            .unwrap();
        doc.clipping_path("create").unwrap();
        let source = doc.svg_layers().next().unwrap().source.clone();
        let (w, h) = doc.dimensions();
        let raster = rasterize_svg(&source, w, h).unwrap();
        let pixel = |x: usize, y: usize| {
            &raster.pixels[(y * w as usize + x) * 4..(y * w as usize + x) * 4 + 4]
        };
        assert_eq!(pixel(20, 20), &[255, 0, 0, 255]);
        assert_eq!(pixel(60, 20)[3], 0);
        assert!(doc
            .vector_drag_runs(doc.svg_layers().next().unwrap())
            .is_none());
        let mut reopened = Document::decode(&doc.encode().unwrap()).unwrap();
        assert_eq!(reopened.svg_layers().next().unwrap().source, source);
        reopened.select_vector_objects(vec!["art".into()]).unwrap();
        reopened.clipping_path("edit").unwrap();
        assert_eq!(reopened.selected_vector_ids(), &["mask"]);
        reopened.move_selected_vectors(20., 0.).unwrap();
        assert_ne!(reopened.svg_layers().next().unwrap().source, source);
        let moved = rasterize_svg(&reopened.svg_layers().next().unwrap().source, w, h).unwrap();
        assert_eq!(moved.pixels[(20 * w as usize + 10) * 4 + 3], 0);
        assert_eq!(moved.pixels[(20 * w as usize + 50) * 4 + 3], 255);
        reopened.undo();
        assert_eq!(reopened.svg_layers().next().unwrap().source, source);
        let mut duplicated = doc.clone();
        duplicated.duplicate_selected_vectors(100., 0.).unwrap();
        let copied = &duplicated.svg_layers().next().unwrap().vector_objects;
        assert_eq!(copied.len(), 4);
        assert_ne!(copied[0].group_path, copied[2].group_path);
        assert_eq!(
            copied[3].clipping_group.as_ref(),
            copied[2].group_path.first()
        );
        assert_eq!(copied[0].transform[4], 0.);
        assert_eq!(copied[2].transform[4], 100.);
        let selected = duplicated.selected_vector_ids().to_vec();
        assert_eq!(selected.len(), 2);
        duplicated.undo();
        assert_eq!(
            duplicated.svg_layers().next().unwrap().vector_objects.len(),
            2
        );
        duplicated.redo();
        assert_eq!(duplicated.selected_vector_ids(), selected);
        let loaded = Document::decode(&duplicated.encode().unwrap()).unwrap();
        assert_eq!(loaded.svg_layers().next().unwrap().vector_objects.len(), 4);
        doc.clipping_path("release").unwrap();
        assert_eq!(doc.svg_layers().next().unwrap().source, original);
        doc.undo();
        assert_eq!(doc.svg_layers().next().unwrap().source, source);
        doc.redo();
        assert_eq!(doc.svg_layers().next().unwrap().source, original);
        doc.undo();
        doc.ungroup_selected_vectors(true).unwrap();
        assert_eq!(doc.svg_layers().next().unwrap().source, original);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportedRasterInfo {
    pub width: u32,
    pub height: u32,
    pub encoded_width: u32,
    pub encoded_height: u32,
    /// EXIF orientation, using the standard values 1 through 8.
    pub orientation: u8,
}

impl ImportedRasterInfo {
    pub fn image_transform(self) -> Option<[f32; 6]> {
        let (w, h) = (self.encoded_width as f32, self.encoded_height as f32);
        match self.orientation {
            1 => None,
            2 => Some([-1., 0., 0., 1., w, 0.]),
            3 => Some([-1., 0., 0., -1., w, h]),
            4 => Some([1., 0., 0., -1., 0., h]),
            5 => Some([0., 1., 1., 0., 0., 0.]),
            6 => Some([0., 1., -1., 0., h, 0.]),
            7 => Some([0., -1., -1., 0., h, w]),
            8 => Some([0., -1., 1., 0., 0., w]),
            _ => None,
        }
    }
}

/// Validate raster contents and read its displayed orientation before placement.
#[cfg(feature = "skia")]
pub fn imported_raster_info(bytes: &[u8], format: &str) -> Result<ImportedRasterInfo, String> {
    let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    let jpeg = bytes.starts_with(&[0xff, 0xd8, 0xff]);
    if !matches!(format, "all" | "png" | "jpeg")
        || !(png || jpeg)
        || (format == "png" && !png)
        || (format == "jpeg" && !jpeg)
    {
        return Err("Choose a matching JPEG or PNG image / 指定形式のJPEGまたはPNGを選択してください / 请选择匹配格式的JPEG或PNG图片".into());
    }
    let mut codec = skia_safe::Codec::from_data(skia_safe::Data::new_copy(bytes))
        .ok_or("Invalid image / 画像を読み取れません / 无法读取图片")?;
    let size = codec.dimensions();
    let origin = codec.origin();
    if size.width <= 0 || size.height <= 0 || size.width > 8192 || size.height > 8192 {
        return Err("Image dimensions must be at most 8192px / 画像は各辺8192pxまでです / 图片每边最多8192像素".into());
    }
    codec
        .get_image(None, None)
        .map_err(|_| "Damaged image / 画像データが破損しています / 图片数据已损坏")?;
    let (encoded_width, encoded_height) = (size.width as u32, size.height as u32);
    let (width, height) = if origin.swaps_width_height() {
        (encoded_height, encoded_width)
    } else {
        (encoded_width, encoded_height)
    };
    Ok(ImportedRasterInfo {
        width,
        height,
        encoded_width,
        encoded_height,
        orientation: origin as u8,
    })
}

/// CPU decoding keeps placement available when the Skia backend is unavailable.
#[cfg(not(feature = "skia"))]
pub fn imported_raster_info(bytes: &[u8], format: &str) -> Result<ImportedRasterInfo, String> {
    use image::ImageDecoder;
    let detected = image::guess_format(bytes).map_err(|e| e.to_string())?;
    if !matches!(detected, image::ImageFormat::Png | image::ImageFormat::Jpeg)
        || !matches!(format, "all" | "png" | "jpeg")
        || (format == "png" && detected != image::ImageFormat::Png)
        || (format == "jpeg" && detected != image::ImageFormat::Jpeg)
    {
        return Err("Choose a matching JPEG or PNG image / 指定形式のJPEGまたはPNGを選択してください / 请选择匹配格式的JPEG或PNG图片".into());
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), detected);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let (encoded_width, encoded_height) = decoder.dimensions();
    let orientation = decoder.orientation().map_err(|e| e.to_string())?.to_exif();
    // Decode fully to reject corrupt images before linking them into the document.
    image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    let (width, height) = if orientation >= 5 {
        (encoded_height, encoded_width)
    } else {
        (encoded_width, encoded_height)
    };
    Ok(ImportedRasterInfo {
        width,
        height,
        encoded_width,
        encoded_height,
        orientation,
    })
}

#[cfg(feature = "skia")]
pub fn imported_raster_size(bytes: &[u8], format: &str) -> Result<(u32, u32), String> {
    let info = imported_raster_info(bytes, format)?;
    Ok((info.width, info.height))
}

#[cfg(all(test, not(feature = "skia")))]
#[test]
fn cpu_raster_import_validates_formats_corruption_and_exif_orientation() {
    let image = image::DynamicImage::new_rgb8(12, 7);
    for (encoding, format, wrong) in [
        (image::ImageFormat::Png, "png", "jpeg"),
        (image::ImageFormat::Jpeg, "jpeg", "png"),
    ] {
        let mut output = std::io::Cursor::new(Vec::new());
        image.write_to(&mut output, encoding).unwrap();
        let bytes = output.into_inner();
        let info = imported_raster_info(&bytes, format).unwrap();
        assert_eq!((info.width, info.height), (12, 7));
        assert_eq!(imported_raster_info(&bytes, "all").unwrap(), info);
        assert!(imported_raster_info(&bytes, wrong).is_err());
        assert!(imported_raster_info(&bytes[..12], format).is_err());
        if encoding == image::ImageFormat::Jpeg {
            for orientation in 1..=8 {
                let mut oriented = bytes[..2].to_vec();
                oriented.extend_from_slice(&[0xff, 0xe1, 0, 34]);
                oriented.extend_from_slice(b"Exif\0\0II");
                oriented.extend_from_slice(&[
                    42,
                    0,
                    8,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0x12,
                    1,
                    3,
                    0,
                    1,
                    0,
                    0,
                    0,
                    orientation,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                ]);
                oriented.extend_from_slice(&bytes[2..]);
                let info = imported_raster_info(&oriented, "jpeg").unwrap();
                assert_eq!(info.orientation, orientation);
                assert_eq!((info.encoded_width, info.encoded_height), (12, 7));
                assert_eq!(
                    (info.width, info.height),
                    if orientation >= 5 { (7, 12) } else { (12, 7) }
                );
            }
        }
    }
    assert!(imported_raster_info(b"not an image", "all").is_err());
}

#[cfg(all(test, feature = "skia"))]
#[test]
#[allow(deprecated)]
fn raster_import_validates_jpeg_png_and_rejects_mismatches() {
    let mut surface = skia_safe::surfaces::raster_n32_premul((12, 7)).unwrap();
    surface.canvas().clear(skia_safe::Color::RED);
    let image = surface.image_snapshot();
    for (encoding, format, wrong) in [
        (skia_safe::EncodedImageFormat::PNG, "png", "jpeg"),
        (skia_safe::EncodedImageFormat::JPEG, "jpeg", "png"),
    ] {
        let data = image.encode_to_data(encoding).unwrap();
        assert_eq!(
            imported_raster_size(data.as_bytes(), format).unwrap(),
            (12, 7)
        );
        assert_eq!(
            imported_raster_size(data.as_bytes(), "all").unwrap(),
            (12, 7)
        );
        assert!(imported_raster_size(data.as_bytes(), wrong).is_err());
        assert!(imported_raster_size(&data.as_bytes()[..12], format).is_err());
    }
    assert!(imported_raster_size(b"not an image", "all").is_err());

    for orientation in 1..=8 {
        let info = ImportedRasterInfo {
            width: if orientation >= 5 { 7 } else { 12 },
            height: if orientation >= 5 { 12 } else { 7 },
            encoded_width: 12,
            encoded_height: 7,
            orientation,
        };
        let matrix = info.image_transform().unwrap_or([1., 0., 0., 1., 0., 0.]);
        let corners = [[0., 0.], [12., 0.], [12., 7.], [0., 7.]].map(|[x, y]| {
            [
                matrix[0] * x + matrix[2] * y + matrix[4],
                matrix[1] * x + matrix[3] * y + matrix[5],
            ]
        });
        let min_x = corners.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let min_y = corners.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let max_y = corners
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(
            [min_x, min_y, max_x, max_y],
            [0., 0., info.width as f32, info.height as f32]
        );
    }
}

#[cfg(test)]
#[test]
fn pixel_layer_moves_outside_and_back_without_accumulating_crops() {
    use lumapaint_core::document::Document;
    let image = r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="100" y="80" width="400" height="300" fill="red"/></svg>"#;
    let mut doc = Document::default();
    doc.import_raster_layer("Image".into(), image.into())
        .unwrap();
    let initial = rasterize_svg(image, 960, 640).unwrap().pixels;
    for (dx, dy) in [
        (800., 0.),
        (-800., 0.),
        (-600., -500.),
        (600., 500.),
        (100., 150.),
        (-100., -150.),
    ] {
        let source = doc
            .translated_pixel_layer_source(dx, dy, false)
            .unwrap()
            .unwrap();
        doc.replace_moved_pixels(source, None).unwrap();
    }
    let source = doc.svg_layers().last().unwrap().source.clone();
    assert_eq!(rasterize_svg(&source, 960, 640).unwrap().pixels, initial);
    assert_eq!(source.matches("<svg ").count(), 1);
    let mut loaded = Document::decode(&doc.encode().unwrap()).unwrap();
    let layer_id = loaded.svg_layers().last().unwrap().id.clone();
    loaded.select_layer(layer_id).unwrap();
    let source = loaded
        .translated_pixel_layer_source(900., 700., false)
        .unwrap()
        .unwrap();
    loaded.replace_moved_pixels(source, None).unwrap();
    let source = loaded
        .translated_pixel_layer_source(-900., -700., false)
        .unwrap()
        .unwrap();
    loaded.replace_moved_pixels(source.clone(), None).unwrap();
    assert_eq!(rasterize_svg(&source, 960, 640).unwrap().pixels, initial);
    // Recover the retained image data from document-sized clipping wrappers written by old moves.
    let old = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><g transform="translate(-800 0)"><svg width="960" height="640"><g transform="translate(800 0)">{image}</g></svg></g></svg>"#
    );
    doc.replace_moved_pixels(old, None).unwrap();
    let source = doc
        .translated_pixel_layer_source(0., 0., false)
        .unwrap()
        .unwrap();
    assert_eq!(rasterize_svg(&source, 960, 640).unwrap().pixels, initial);
}

#[cfg(test)]
mod stroke_appearance_tests {
    use super::*;
    use lumapaint_core::stroke::*;
    fn render(path: &str, style: &StrokeStyle) -> SvgRaster {
        let body = svg_stroke(path, path, "nonzero", 10., style, "#ff0000", 0);
        rasterize_svg(
            &format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100">{body}</svg>"#
            ),
            160,
            100,
        )
        .unwrap()
    }
    fn alpha(image: &SvgRaster, x: usize, y: usize) -> u8 {
        image.pixels[(y * 160 + x) * 4 + 3]
    }
    #[test]
    fn advanced_strokes_match_visible_pixels_and_pick_regions() {
        let cases = [
            (
                "M30 20H100V80H30Z M125 20V80",
                StrokeStyle {
                    alignment: StrokeAlignment::Inside,
                    ..Default::default()
                },
            ),
            (
                "M30 20H100V80H30Z M125 20V80",
                StrokeStyle {
                    contour_alignments: vec![StrokeAlignment::Outside, StrokeAlignment::Inside],
                    ..Default::default()
                },
            ),
            (
                "M30 50H130",
                StrokeStyle {
                    profile: WidthProfile::Custom,
                    width_curve: vec![
                        WidthStop {
                            position: 0.,
                            width: 0.,
                            slope: 0.,
                        },
                        WidthStop {
                            position: 0.5,
                            width: 3.,
                            slope: 0.,
                        },
                        WidthStop {
                            position: 1.,
                            width: 0.,
                            slope: 0.,
                        },
                    ],
                    ..Default::default()
                },
            ),
            (
                "M30 50H130",
                StrokeStyle {
                    start_arrow: Arrowhead::Square,
                    end_arrow: Arrowhead::Diamond,
                    start_arrow_scale: Some(0.5),
                    end_arrow_scale: Some(2.),
                    ..Default::default()
                },
            ),
        ];
        for (path, style) in cases {
            let image = render(path, &style);
            let geometry = selection_geometry(path, 10., &style, [1., 0., 0., 1., 0., 0.], false);
            for y in (3..98).step_by(5) {
                for x in (3..158).step_by(5) {
                    let pixel = alpha(&image, x, y);
                    if pixel == 0 || pixel == 255 {
                        assert_eq!(
                            geometry.contains([x as f32 + 0.5, y as f32 + 0.5], 0.),
                            pixel == 255,
                            "{style:?} at {x},{y}"
                        );
                    }
                }
            }
        }
        for shape in [
            Arrowhead::Diamond,
            Arrowhead::Square,
            Arrowhead::Bar,
            Arrowhead::Stealth,
        ] {
            let style = StrokeStyle {
                end_arrow: shape,
                ..Default::default()
            };
            let image = render("M30 50H130", &style);
            assert!(alpha(&image, 129, 50) > 0);
        }
    }
    #[test]
    fn stroke_alignment_preserves_inside_outside_and_holes() {
        let path = "M30 20H130V80H30Z M50 35H110V65H50Z";
        for (alignment, outside, inside) in [
            (StrokeAlignment::Center, 255, 255),
            (StrokeAlignment::Inside, 0, 255),
            (StrokeAlignment::Outside, 255, 0),
        ] {
            let style = StrokeStyle {
                alignment,
                ..Default::default()
            };
            let body = svg_stroke(path, path, "evenodd", 10., &style, "red", 0);
            let image = rasterize_svg(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100">{body}</svg>"#),160,100).unwrap();
            assert_eq!(alpha(&image, 80, 18), outside, "{alignment:?}");
            assert_eq!(alpha(&image, 80, 22), inside, "{alignment:?}");
            assert_eq!(alpha(&image, 80, 33), inside, "hole {alignment:?}");
            assert_eq!(alpha(&image, 80, 37), outside, "hole {alignment:?}");
            assert_eq!(alpha(&image, 80, 50), 0);
        }
    }
    #[test]
    fn stroke_caps_dashes_and_endpoint_arrows_change_pixels() {
        let path = "M30 50H130";
        let butt = render(path, &StrokeStyle::default());
        assert_eq!(alpha(&butt, 27, 50), 0);
        for cap in [LineCap::Round, LineCap::Square] {
            assert_eq!(
                alpha(
                    &render(
                        path,
                        &StrokeStyle {
                            cap,
                            ..Default::default()
                        }
                    ),
                    27,
                    50
                ),
                255
            );
        }
        let dashed = render(
            path,
            &StrokeStyle {
                dash_array: vec![10., 10.],
                ..Default::default()
            },
        );
        assert_eq!(alpha(&dashed, 35, 50), 255);
        assert_eq!(alpha(&dashed, 45, 50), 0);
        let shifted = render(
            path,
            &StrokeStyle {
                dash_array: vec![10., 10.],
                dash_offset: 10.,
                ..Default::default()
            },
        );
        assert_eq!(alpha(&shifted, 35, 50), 0);
        assert_eq!(alpha(&shifted, 45, 50), 255);
        let arrow = render(
            path,
            &StrokeStyle {
                start_arrow: Arrowhead::Triangle,
                end_arrow: Arrowhead::Triangle,
                ..Default::default()
            },
        );
        assert!(alpha(&arrow, 60, 60) > 0);
        assert!(alpha(&arrow, 100, 60) > 0);
        assert_eq!(alpha(&arrow, 20, 60), 0);
        assert_eq!(alpha(&arrow, 140, 60), 0);
        let open = render(
            path,
            &StrokeStyle {
                alignment: StrokeAlignment::Inside,
                ..Default::default()
            },
        );
        assert_eq!(open.pixels, butt.pixels);
    }
    #[test]
    fn stroke_joins_and_miter_limit_control_corner_extent() {
        let path = "M30 75L80 25L130 75";
        let miter = render(path, &StrokeStyle::default());
        let round = render(
            path,
            &StrokeStyle {
                join: LineJoin::Round,
                ..Default::default()
            },
        );
        let bevel = render(
            path,
            &StrokeStyle {
                join: LineJoin::Bevel,
                ..Default::default()
            },
        );
        assert_eq!(alpha(&miter, 80, 19), 255);
        assert_eq!(alpha(&round, 80, 19), 0);
        assert!(alpha(&round, 80, 20) > 200);
        assert_eq!(alpha(&bevel, 80, 20), 0);
        let limited = render(
            path,
            &StrokeStyle {
                miter_limit: 1.,
                ..Default::default()
            },
        );
        assert_eq!(limited.pixels, bevel.pixels);
        let transformed = svg_stroke(
            path,
            path,
            "nonzero",
            10.,
            &StrokeStyle {
                join: LineJoin::Round,
                ..Default::default()
            },
            "red",
            0,
        );
        let image=rasterize_svg(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><g transform="translate(10 0) scale(0.5)">{transformed}</g></svg>"#),160,100).unwrap();
        assert!(alpha(&image, 50, 11) > 0);
        assert_eq!(alpha(&image, 80, 20), 0);
    }
    #[test]
    fn stroke_variable_closed_dash_preserves_the_closing_join() {
        let path = "M40 30H120V70H40Z";
        let style = StrokeStyle {
            profile: WidthProfile::Bulge,
            cap: LineCap::Square,
            join: LineJoin::Bevel,
            ..Default::default()
        };
        assert_eq!(
            render(path, &style).pixels,
            render(
                path,
                &StrokeStyle {
                    dash_array: vec![500., 10.],
                    ..style
                }
            )
            .pixels
        );
    }
    #[test]
    fn stroke_variable_width_tapers_and_dashes_without_alpha_seams() {
        let path = "M20 50H140";
        for profile in [
            WidthProfile::TaperBoth,
            WidthProfile::TaperStart,
            WidthProfile::TaperEnd,
            WidthProfile::Bulge,
        ] {
            let style = StrokeStyle {
                profile,
                ..Default::default()
            };
            let image = render(path, &style);
            assert!(alpha(&image, 80, 50) > 200, "{profile:?}");
            if matches!(profile, WidthProfile::TaperBoth | WidthProfile::TaperStart) {
                assert_eq!(alpha(&image, 22, 46), 0);
            }
            if matches!(profile, WidthProfile::TaperBoth | WidthProfile::TaperEnd) {
                assert_eq!(alpha(&image, 138, 46), 0);
            }
            let dashed = render(
                path,
                &StrokeStyle {
                    dash_array: vec![10., 10.],
                    ..style
                },
            );
            assert_eq!(alpha(&dashed, 75, 50), 0);
            assert!(alpha(&dashed, 85, 50) > 200);
        }
        let body = svg_stroke(
            path,
            path,
            "nonzero",
            10.,
            &StrokeStyle {
                profile: WidthProfile::Bulge,
                ..Default::default()
            },
            "#ff000080",
            0,
        );
        let image = rasterize_svg(
            &format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100">{body}</svg>"#
            ),
            160,
            100,
        )
        .unwrap();
        assert!((127..=128).contains(&alpha(&image, 80, 50)));
    }
}

#[cfg(test)]
mod imported_svg_edit_tests {
    use super::*;
    use lumapaint_core::svg_backend::{SvgEdit, SvgGeometryBackend};
    use lumapaint_svg::UsvgGeometryBackend;
    fn image(source: &str) -> SvgRaster {
        rasterize_svg(source, 160, 100).unwrap()
    }
    fn same(before: &SvgRaster, after: &SvgRaster) {
        assert_eq!(before.pixels.len(), after.pixels.len());
        let differing = before
            .pixels
            .iter()
            .zip(&after.pixels)
            .filter(|(a, b)| a.abs_diff(**b) > 2)
            .count();
        assert_eq!(differing, 0, "{differing} color channels changed");
    }
    #[test]
    fn materializing_css_shapes_and_use_preserves_pixels_paints_and_clips() {
        for source in [
            r##"<svg width="160" height="100"><style>rect {x:20px;y:20px;width:80px;height:50px;rx:10px;opacity:.6;fill:red;transform:translateX(5px)} path {fill:blue;d:path('M0 0H1V1Z') !important}</style><rect id="shape"/></svg>"##,
            r##"<svg width="160" height="100"><defs><linearGradient id="paint"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><clipPath id="cut"><rect x="10" y="10" width="120" height="60"/></clipPath></defs><ellipse id="shape" cx="70" cy="45" rx="50" ry="30" fill="url(#paint)" clip-path="url(#cut)" opacity=".7"/></svg>"##,
            r##"<svg width="160" height="100"><style>.part {fill:orange;stroke:green;stroke-width:3} g {opacity:.8} path {fill:blue !important}</style><defs><g id="model"><rect class="part" x="5" y="5" width="25" height="30"/><circle class="part" cx="45" cy="20" r="15"/></g></defs><use id="one" href="#model" x="10" y="10" transform="rotate(10)"/><use id="two" href="#model" x="80" y="10"/></svg>"##,
            r##"<svg width="160" height="100"><defs><symbol id="symbol" viewBox="0 0 20 20"><rect x="-5" y="-5" width="30" height="30" fill="red"/><ellipse cx="10" cy="10" rx="8" ry="5" fill="blue"/></symbol></defs><use href="#symbol" x="20" y="10" width="70" height="70"/></svg>"##,
        ] {
            let before = image(source);
            let mut edited = source.to_owned();
            let target = UsvgGeometryBackend
                .objects(source, "l", [160., 100.])
                .remove(0);
            let object = target.clone();
            edited = UsvgGeometryBackend
                .apply(
                    &edited,
                    "l",
                    [160., 100.],
                    &[SvgEdit {
                        object_id: target.id,
                        object: Some(object),
                    }],
                )
                .unwrap();
            let after = image(&edited);
            let changed = before
                .pixels
                .iter()
                .zip(&after.pixels)
                .filter(|(a, b)| a.abs_diff(**b) > 2)
                .count();
            assert_eq!(changed, 0, "Original: {source}\nEdited: {edited}");
            same(&before, &after);
            let targets = UsvgGeometryBackend.objects(&edited, "l", [160., 100.]);
            assert!(!targets.is_empty(), "{edited}");
        }
    }
    #[test]
    fn moving_one_use_does_not_change_the_other_instance_or_definition() {
        let mut source=r##"<svg width="160" height="100"><defs><g id="model"><rect x="5" y="5" width="30" height="30" fill="red"/><circle cx="20" cy="20" r="10" fill="blue"/></g></defs><use href="#model" x="10" y="10"/><use href="#model" x="90" y="10"/></svg>"##.to_owned();
        let before = image(&source);
        let targets = UsvgGeometryBackend.objects(&source, "l", [160., 100.]);
        assert_eq!(targets.len(), 4);
        let changes: Vec<_> = targets
            .into_iter()
            .take(2)
            .map(|t| {
                let mut o = t.clone();
                let indices = lumapaint_core::bezier::anchor_indices(&o);
                lumapaint_core::bezier::translate_controls(&mut o, &indices, [10., 15.], false)
                    .unwrap();
                SvgEdit {
                    object_id: t.id,
                    object: Some(o),
                }
            })
            .collect();
        source = UsvgGeometryBackend
            .apply(&source, "l", [160., 100.], &changes)
            .unwrap();
        let after = image(&source);
        for y in 0..100 {
            for x in 80..160 {
                let i = (y * 160 + x) * 4;
                assert_eq!(before.pixels[i..i + 4], after.pixels[i..i + 4]);
            }
        }
        assert_eq!(before.pixels[(20 * 160 + 20) * 4 + 3], 255);
        assert_eq!(after.pixels[(20 * 160 + 20) * 4 + 3], 0);
        assert!(source.contains(r#"<rect x="5" y="5" width="30" height="30" fill="red"/>"#));
    }
}

#[cfg(test)]
mod independent_export_tests {
    use super::rasterize_svg;
    use lumapaint_core::document::{CanvasColor, Document};
    use lumapaint_formats::export::{export, ExportOptions, ExportSnapshot, FormatId};

    #[test]
    fn embedded_svg_export_preserves_aspect_ratio_and_pixels() {
        // Non-square source in a square document catches accidental stretching.
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><rect x="20" y="10" width="40" height="30" fill="red"/></svg>"#;
        let mut state = Document::default().document_state();
        state.width = 200;
        state.height = 200;
        state.canvas_color = Some(CanvasColor::Transparent);
        let mut document = Document::from_document_state(state).unwrap();
        document
            .import_svg("fixture".into(), source.into())
            .unwrap();
        let result = export(
            FormatId::Svg,
            &ExportSnapshot::capture(&document),
            ExportOptions { allow_lossy: true },
        )
        .unwrap();
        let output = String::from_utf8(result.bytes).unwrap();
        let before = rasterize_svg(source, 200, 200).unwrap();
        let after = rasterize_svg(&output, 200, 200).unwrap();
        assert_eq!(before.pixels, after.pixels);
    }
}

#[cfg(test)]
mod object_cache_tests {
    use super::*;
    #[test]
    fn object_texture_retains_exterior_and_matches_artboard_pixels() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><rect x="-20" y="10" width="60" height="30" fill="red" opacity="0.5"/></svg>"#;
        let (pixels, rect) = rasterize_svg_object(source, (960, 640)).unwrap();
        assert!(rect[0] < 0.0);
        assert!(pixels.len() < 960 * 640 * 4 / 100);
        let original = rasterize_svg(source, 960, 640).unwrap();
        for y in 10..40usize {
            for x in 0..40usize {
                let local = ((y - rect[1] as usize) * rect[2] as usize
                    + (x as i32 - rect[0] as i32) as usize)
                    * 4;
                let full = (y * 960 + x) * 4;
                assert_eq!(&pixels[local..local + 4], &original.pixels[full..full + 4]);
            }
        }
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .any(|(index, p)| rect[0] + ((index % rect[2] as usize) as f32) < 0.0 && p[3] > 0));
    }
}

#[cfg(test)]
mod object_lock_tests {
    use super::*;
    use lumapaint_core::document::{Document, ObjectLockAction};

    #[test]
    fn svg_instance_locks_keep_rendering_and_identity_after_other_nodes_are_deleted() {
        let mut doc = Document::default();
        lumapaint_svg::attach(&mut doc);
        doc.import_svg("instances".into(), r##"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><defs><rect id="model" width="30" height="30" fill="red"/></defs><rect x="10" y="10" width="20" height="20" fill="blue"/><use href="#model" x="50" y="10"/><use href="#model" x="100" y="10"/></svg>"##.into()).unwrap();
        let targets = doc.direct_objects();
        assert_eq!(targets.len(), 3);
        let locked = targets[1].1.id.clone();
        let sibling = targets[2].1.id.clone();
        let original = rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640)
            .unwrap()
            .pixels;
        doc.select_direct_objects(vec![locked.clone()]).unwrap();
        doc.lock_objects(ObjectLockAction::Selection).unwrap();
        let after = rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640)
            .unwrap()
            .pixels;
        assert_eq!(original, after);
        assert!(!doc
            .direct_objects()
            .iter()
            .any(|(_, object)| object.id == locked));
        assert!(doc.select_direct_objects(vec![locked.clone()]).is_err());
        let first = &targets[0].1;
        let anchors: Vec<_> = lumapaint_core::bezier::control_indices(first)
            .into_iter()
            .map(|index| (first.id.clone(), index))
            .collect();
        doc.delete_vector_anchors(&anchors).unwrap();
        assert!(doc.object_is_locked(&locked));
        assert!(!doc
            .direct_objects()
            .iter()
            .any(|(_, object)| object.id == locked));
        assert!(doc
            .direct_objects()
            .iter()
            .any(|(_, object)| object.id == sibling));
        let bytes = doc.encode().unwrap();
        let mut loaded = Document::decode(&bytes).unwrap();
        lumapaint_svg::attach(&mut loaded);
        assert!(loaded.object_is_locked(&locked));
        loaded.lock_objects(ObjectLockAction::UnlockAll).unwrap();
        assert!(loaded
            .direct_objects()
            .iter()
            .any(|(_, object)| object.id == locked));
        assert_eq!(loaded.selected_vector_ids(), [locked]);
        loaded.undo();
        assert!(loaded.has_locked_objects());
    }
}

#[cfg(all(test, feature = "skia"))]
#[test]
fn gradient_skia_cpu_matches_independent_svg_renderer() {
    for body in [
        r#"<linearGradient id="g" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="64" y2="32"><stop stop-color="red"/><stop offset="1" stop-color="blue" stop-opacity=".3"/></linearGradient>"#,
        r#"<radialGradient id="g" cx=".5" cy=".5" r=".5"><stop stop-color="white"/><stop offset="1" stop-color="black"/></radialGradient>"#,
    ] {
        let svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"32\"><defs>{body}</defs><rect width=\"64\" height=\"32\" fill=\"url(#g)\"/></svg>");
        let actual = rasterize_svg(&svg, 64, 32).unwrap();
        assert!(matches!(
            actual.backend,
            SvgBackend::Skia | SvgBackend::SkiaGpu
        ));
        let expected = with_compatibility_renderer(|| rasterize_svg(&svg, 64, 32)).unwrap();
        let difference = actual
            .pixels
            .iter()
            .zip(&expected.pixels)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(difference <= 3, "Gradient difference {difference}");
    }
}

#[cfg(test)]
#[test]
fn gradient_dither_changes_rgb_without_changing_alpha() {
    use lumapaint_core::gradient::*;
    let mut gradient = Gradient {
        geometry: None,
        pixel_style: None,
        kind: GradientKind::Linear,
        angle: 0.,
        aspect: 1.,
        dither: false,
        method: GradientMethod::Classic,
        stops: vec![
            GradientStop {
                position: 0.,
                color: [100, 100, 100, 128],
                midpoint: 0.5,
            },
            GradientStop {
                position: 1.,
                color: [115, 115, 115, 128],
                midpoint: 0.5,
            },
        ],
    };
    let source = |g: &Gradient| {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"96\" height=\"32\"><defs>{}{}</defs><rect width=\"96\" height=\"32\" fill=\"url(#g)\" {}/></svg>",g.svg_definition_in_bounds("g",[0.,0.,96.,32.]),g.svg_dither_filter("dither"),if g.dither{"filter=\"url(#dither)\""}else{""})
    };
    let plain = rasterize_svg(&source(&gradient), 96, 32).unwrap();
    gradient.dither = true;
    let noise = rasterize_svg(&source(&gradient), 96, 32).unwrap();
    assert_eq!(
        plain.pixels.iter().skip(3).step_by(4).collect::<Vec<_>>(),
        noise.pixels.iter().skip(3).step_by(4).collect::<Vec<_>>()
    );
    assert!(plain.pixels != noise.pixels, "Dither did not add noise");
}

#[cfg(test)]
mod image_frame_tests {
    use super::*;
    use lumapaint_core::{document::Document, image_frame::*};
    #[test]
    fn ellipse_clips_cached_graphic_and_frame_fill_stays_behind_image() {
        let mut d = Document::default();
        let image=FrameImage{source_path:Some("/missing/image.png".into()),fingerprint:"cached".into(),name:"Image".into(),width:2,height:2,encoded_width:2,encoded_height:2,orientation_transform:[1.,0.,0.,1.,0.,0.],data_uri:"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAEUlEQVR4nGP4z8DwH4QZYAwAR8oH+WdZbrcAAAAASUVORK5CYII=".into()};
        d.place_frame_image(None, image, "frame").unwrap();
        let (layer, o) = d.image_frame_object("frame").unwrap();
        let layer = layer.id.clone();
        let mut o = o.clone();
        o.path.data = "M100 50 A50 50 0 1 1 0 50 A50 50 0 1 1 100 50Z".into();
        o.kind = lumapaint_core::vector::VectorObjectKind::Ellipse;
        o.control_points = vec![[0., 0.], [100., 100.]];
        o.fill = Some(lumapaint_core::vector::VectorPaint {
            color: [0, 0, 255, 255],
        });
        o.image_frame
            .as_mut()
            .unwrap()
            .fit([0., 0., 100., 100.], FrameFit::Cover)
            .unwrap();
        d.upsert_vector_object(&layer, o).unwrap();
        let raster = rasterize_svg(&d.svg_layers().next().unwrap().source, 960, 640).unwrap();
        assert_eq!(
            &raster.pixels[(50 * 960 + 50) * 4..(50 * 960 + 50) * 4 + 4],
            &[255, 0, 0, 255]
        );
        assert_eq!(raster.pixels[3], 0);
    }
}
