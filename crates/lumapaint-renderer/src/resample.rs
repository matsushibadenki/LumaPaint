//! Resampling of premultiplied document pixels without a JavaScript frame transfer.
//!
//! This is a raster primitive; callers must preserve off-canvas data and commit
//! all document components and resolution in a single history transaction.

use image::{imageops::FilterType, RgbaImage};

/// Resize premultiplied sRGB RGBA8 pixels with an antialiased triangle filter.
/// The input and output use the same representation as the SVG raster backend.
pub fn rgba8_triangle(
    pixels: Vec<u8>,
    old: (u32, u32),
    new: (u32, u32),
) -> Result<Vec<u8>, String> {
    const MAX_PIXELS: u64 = 16_777_216;
    for (width, height) in [old, new] {
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || u64::from(width) * u64::from(height) > MAX_PIXELS
        {
            return Err("Invalid or oversized resampling dimensions".into());
        }
    }
    // The separable filter holds a floating-point vertical intermediate.
    if u64::from(old.0) * u64::from(new.1) > MAX_PIXELS {
        return Err("Resampling intermediate exceeds pixel limit".into());
    }
    let expected = old.0 as usize * old.1 as usize * 4;
    if pixels.len() != expected {
        return Err("Invalid resampling pixel buffer".into());
    }
    if pixels
        .chunks_exact(4)
        .any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3])
    {
        return Err("Resampling requires premultiplied RGBA pixels".into());
    }
    if old == new {
        return Ok(pixels);
    }
    let source = RgbaImage::from_raw(old.0, old.1, pixels).ok_or("Invalid pixel buffer")?;
    Ok(image::imageops::resize(&source, new.0, new.1, FilterType::Triangle).into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resampling_preserves_premultiplied_alpha() {
        let out = rgba8_triangle(vec![255, 0, 0, 255, 0, 0, 0, 0], (2, 1), (4, 1)).unwrap();
        assert_eq!(out.len(), 16);
        assert!(out
            .chunks_exact(4)
            .all(|p| p[0] == p[3] && p[1] == 0 && p[2] == 0));
        assert!(out.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
    }
    #[test]
    fn shrinking_filters_and_checks_input() {
        let pixels = vec![
            255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255,
        ];
        let out = rgba8_triangle(pixels.clone(), (2, 2), (1, 1)).unwrap();
        assert!((120..=135).contains(&out[0]));
        assert_eq!(out[3], 255);
        assert_eq!(
            rgba8_triangle(pixels.clone(), (2, 2), (2, 2)).unwrap(),
            pixels
        );
        assert!(rgba8_triangle(vec![255, 0, 0, 0], (1, 1), (2, 2)).is_err());
        assert!(rgba8_triangle(vec![], (1, 1), (2, 2)).is_err());
        assert!(rgba8_triangle(vec![], (0, 1), (2, 2)).is_err());
    }
    #[test]
    fn directional_motion_masks_rotate_with_rendered_pixels() {
        use lumapaint_core::{document::Document, motion::Clip, vector::VectorObject};
        for preset in [
            "mask-wipe",
            "diagonal-wipe",
            "split-reveal",
            "slice-reveal",
            "pixel-dissolve",
            "ink-reveal",
            "svg-mask-reveal",
        ] {
            let mut state = Document::default().document_state();
            state.width = 64;
            state.height = 48;
            let mut doc = Document::from_document_state(state).unwrap();
            let id = doc.add_vector_layer().unwrap();
            let object: VectorObject=serde_json::from_value(serde_json::json!({"id":"shape","name":"Shape","kind":"rectangle","path":{"data":"M0 0H30V20H0Z","fillRule":"nonZero"},"transform":[1,0,0,1,12,8],"fill":{"color":[100,80,200,255]},"stroke":null,"strokeWidth":1,"visible":true})).unwrap();
            doc.upsert_vector_object(&id, object).unwrap();
            doc.set_motion_clip(
                &id,
                Clip {
                    id: "mask".into(),
                    preset: preset.into(),
                    duration: 10,
                    ..Default::default()
                },
            )
            .unwrap();
            for turns in 1..=3 {
                let mut rotated = doc.clone();
                rotated.rotate_image_quarter_turns(turns).unwrap();
                for frame in [1, 5, 9] {
                    let mut before = doc.clone_for_rendering();
                    before.evaluate_animation(&doc, frame);
                    let mut after = rotated.clone_for_rendering();
                    after.evaluate_animation(&rotated, frame);
                    let a = crate::vector::rasterize_svg(
                        &before.svg_layers().find(|l| l.id == id).unwrap().source,
                        64,
                        48,
                    )
                    .unwrap()
                    .pixels;
                    let (width, height) = rotated.dimensions();
                    let b = crate::vector::rasterize_svg(
                        &after.svg_layers().find(|l| l.id == id).unwrap().source,
                        width,
                        height,
                    )
                    .unwrap()
                    .pixels;
                    for y in 0..48usize {
                        for x in 0..64usize {
                            let i = (y * 64 + x) * 4;
                            let (rx, ry) = match turns {
                                1 => (47 - y, x),
                                2 => (63 - x, 47 - y),
                                _ => (y, 63 - x),
                            };
                            let j = (ry * width as usize + rx) * 4;
                            for c in 0..4 {
                                assert!(a[i+c].abs_diff(b[j+c])<=2,"{preset}, turn {turns}, frame {frame}, ({x},{y}), channel {c}: {} vs {}",a[i+c],b[j+c]);
                            }
                        }
                    }
                }
            }
        }
    }
}
