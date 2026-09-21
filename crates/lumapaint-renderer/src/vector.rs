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
            Arc::new(fonts)
        })
        .clone()
}

/// May run on a worker at startup so the first text edit avoids a system font scan.
/// The same OnceLock is used by rendering, including when prewarming is still in progress.
pub fn prepare_text_fonts() {
    let _ = system_fonts();
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
}

/// Fit and center SVG in document pixels. Never resolve external files or network URLs.
pub fn rasterize_svg(source: &str, width: u32, height: u32) -> Result<SvgRaster, String> {
    if source.len() > 4 * 1024 * 1024 || width == 0 || height == 0 || width > 8192 || height > 8192
    {
        return Err("Invalid SVG source size or raster dimensions".into());
    }
    let options = usvg::Options {
        fontdb: system_fonts(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(image: &SvgRaster, width: usize, x: usize, y: usize) -> &[u8] {
        &image.pixels[(y * width + x) * 4..][..4]
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
