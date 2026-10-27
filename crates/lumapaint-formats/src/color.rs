//! Bounded RGB ICC conversion primitives for display buffers.
use moxcms::{ColorProfile, Layout, Transform8BitExecutor, TransformF32Executor, TransformOptions};
use std::sync::Arc;

const MAX_PROFILE_BYTES: usize = 4 * 1024 * 1024;

/// A reusable straight-alpha RGBA8 transform from an embedded RGB ICC profile to sRGB.
/// The caller retains source pixels and profile; this is intended for display buffers only.
pub struct IccToSrgb {
    transform: Arc<Transform8BitExecutor>,
}

impl IccToSrgb {
    pub fn new(profile: &[u8]) -> Result<Self, String> {
        Self::to_profile(profile, &ColorProfile::new_srgb())
    }

    fn to_profile(profile: &[u8], destination: &ColorProfile) -> Result<Self, String> {
        if profile.len() > MAX_PROFILE_BYTES {
            return Err("ICC profile exceeds the supported size".into());
        }
        let source = ColorProfile::new_from_slice(profile).map_err(|e| e.to_string())?;
        let transform = source
            .create_transform_8bit(
                Layout::Rgba,
                destination,
                Layout::Rgba,
                TransformOptions::default(),
            )
            .map_err(|e| e.to_string())?;
        Ok(Self { transform })
    }

    /// Convert premultiplied pixels for a display texture, retaining source alpha.
    pub fn convert_premultiplied(&self, pixels: &mut [u8]) -> Result<(), String> {
        if !pixels.len().is_multiple_of(4) {
            return Err("Invalid premultiplied RGBA buffer".into());
        }
        let mut straight = pixels.to_vec();
        for p in straight.as_chunks_mut::<4>().0 {
            let a = u32::from(p[3]);
            for c in &mut p[..3] {
                *c = (u32::from(*c) * 255 + a / 2)
                    .checked_div(a)
                    .unwrap_or(0)
                    .min(255) as u8;
            }
        }
        let mut transformed = vec![0; pixels.len()];
        self.convert(&straight, &mut transformed)?;
        for p in transformed.as_chunks_mut::<4>().0 {
            let a = u32::from(p[3]);
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * a + 127) / 255) as u8;
            }
        }
        pixels.copy_from_slice(&transformed);
        Ok(())
    }

    pub fn convert(&self, input: &[u8], output: &mut [u8]) -> Result<(), String> {
        if input.len() != output.len() || !input.len().is_multiple_of(4) {
            return Err("RGBA buffers must have equal whole-pixel lengths".into());
        }
        if input.is_empty() {
            return Ok(());
        }
        self.transform
            .transform(input, output)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// RGB ICC conversion into Display P3 encoded RGBA8 for a wide-gamut compositor.
/// This must only be used when the entire destination surface is in Display P3.
pub struct IccToDisplayP3(IccToSrgb);

impl IccToDisplayP3 {
    pub fn new(profile: &[u8]) -> Result<Self, String> {
        IccToSrgb::to_profile(profile, &ColorProfile::new_display_p3()).map(Self)
    }

    pub fn convert_premultiplied(&self, pixels: &mut [u8]) -> Result<(), String> {
        self.0.convert_premultiplied(pixels)
    }

    pub fn convert(&self, input: &[u8], output: &mut [u8]) -> Result<(), String> {
        self.0.convert(input, output)
    }
}

/// Converts premultiplied ICC pixels through Display P3 to extended linear sRGB.
/// Colors outside Display P3 may be clipped by the ICC transform. RGB can be
/// negative or greater than alpha; the compositor must preserve float values.
pub struct IccToExtendedLinearSrgb {
    transform: Arc<TransformF32Executor>,
}

impl IccToExtendedLinearSrgb {
    pub fn new(profile: &[u8]) -> Result<Self, String> {
        if profile.len() > MAX_PROFILE_BYTES {
            return Err("ICC profile exceeds the supported size".into());
        }
        let source = ColorProfile::new_from_slice(profile).map_err(|e| e.to_string())?;
        let transform = source
            .create_transform_f32(
                Layout::Rgba,
                &ColorProfile::new_display_p3(),
                Layout::Rgba,
                TransformOptions::default(),
            )
            .map_err(|e| e.to_string())?;
        Ok(Self { transform })
    }

    pub fn convert_premultiplied(&self, input: &[u8], output: &mut [f32]) -> Result<(), String> {
        if input.len() != output.len() || !input.len().is_multiple_of(4) {
            return Err("RGBA buffers must have equal whole-pixel lengths".into());
        }
        if input.is_empty() {
            return Ok(());
        }
        let mut straight = vec![0.0; input.len()];
        for (src, dst) in input
            .as_chunks::<4>()
            .0
            .iter()
            .zip(straight.as_chunks_mut::<4>().0.iter_mut())
        {
            let alpha = f32::from(src[3]) / 255.0;
            dst[3] = alpha;
            if alpha > 0.0 {
                for c in 0..3 {
                    dst[c] = (f32::from(src[c]) / f32::from(src[3])).min(1.0);
                }
            }
        }
        self.transform
            .transform(&straight, output)
            .map_err(|e| e.to_string())?;
        for (src, pixel) in input
            .as_chunks::<4>()
            .0
            .iter()
            .zip(output.as_chunks_mut::<4>().0.iter_mut())
        {
            let alpha = f32::from(src[3]) / 255.0;
            let mut p3 = [0.0; 3];
            for (c, dst) in pixel[..3].iter().zip(&mut p3) {
                let magnitude = c.abs();
                let linear = if magnitude <= 0.04045 {
                    magnitude / 12.92
                } else {
                    ((magnitude + 0.055) / 1.055).powf(2.4)
                };
                *dst = linear.copysign(*c);
            }
            pixel[0] = (1.224745 * p3[0] - 0.224904 * p3[1]) * alpha;
            pixel[1] = (-0.042058 * p3[0] + 1.042081 * p3[1]) * alpha;
            pixel[2] = (-0.019642 * p3[0] - 0.078655 * p3[1] + 1.098537 * p3[2]) * alpha;
            pixel[3] = alpha;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adobe_rgb_to_srgb_clips_colors_visible_in_display_p3() {
        let data = include_bytes!("../tests/fixtures/lp-photoshop-2026-multi-icc.psd");
        let source_offset = 30 + u32::from_be_bytes(data[26..30].try_into().unwrap()) as usize;
        let resources_len =
            u32::from_be_bytes(data[source_offset..source_offset + 4].try_into().unwrap()) as usize;
        let resources = &data[source_offset + 4..source_offset + 4 + resources_len];
        let mut at = 0;
        let mut profile = None;
        while at < resources.len() {
            assert_eq!(&resources[at..at + 4], b"8BIM");
            let id = u16::from_be_bytes(resources[at + 4..at + 6].try_into().unwrap());
            let name_len = resources[at + 6] as usize;
            at += 6 + ((1 + name_len + 1) & !1);
            let len = u32::from_be_bytes(resources[at..at + 4].try_into().unwrap()) as usize;
            at += 4;
            if id == 1039 {
                profile = Some(&resources[at..at + len]);
            }
            at += len + (len & 1);
        }
        let profile = profile.unwrap();
        let adobe = ColorProfile::new_from_slice(profile).unwrap();
        let srgb = ColorProfile::new_srgb();
        let p3 = ColorProfile::new_display_p3();
        let transform = |from: &ColorProfile, to: &ColorProfile| {
            from.create_transform_8bit(Layout::Rgba, to, Layout::Rgba, TransformOptions::default())
                .unwrap()
        };
        let sample = [0, 255, 0, 255];
        let mut direct = [0; 4];
        let mut intermediate = [0; 4];
        let mut indirect = [0; 4];
        transform(&adobe, &p3)
            .transform(&sample, &mut direct)
            .unwrap();
        let mut prepared = [0; 4];
        let wide = IccToDisplayP3::new(profile).unwrap();
        wide.convert(&sample, &mut prepared).unwrap();
        assert_eq!(prepared, direct);
        let mut premult = [0, 128, 0, 128, 0, 0, 0, 0];
        wide.convert_premultiplied(&mut premult).unwrap();
        assert_eq!(premult[3], 128);
        assert_eq!(&premult[4..], &[0, 0, 0, 0]);
        for c in 0..3 {
            assert!(premult[c].abs_diff(((u32::from(direct[c]) * 128 + 127) / 255) as u8) <= 1);
        }
        let extended = IccToExtendedLinearSrgb::new(profile).unwrap();
        let mut linear = [0.0; 12];
        extended
            .convert_premultiplied(&[0, 255, 0, 255, 0, 128, 0, 128, 0, 0, 0, 0], &mut linear)
            .unwrap();
        eprintln!(
            "Adobe RGB green in extended linear sRGB: {:?}",
            &linear[..4]
        );
        assert!(linear[0] < -0.01 || linear[1] > 1.01 || linear[2] < -0.01);
        let p3_linear = [
            0.82259287 * linear[0] + 0.17753395 * linear[1],
            0.03319951 * linear[0] + 0.96678350 * linear[1],
            0.01708535 * linear[0] + 0.07239572 * linear[1] + 0.91030148 * linear[2],
        ];
        for (actual, expected) in p3_linear.iter().zip(direct) {
            let encoded = if *actual <= 0.0031308 {
                12.92 * actual
            } else {
                1.055 * actual.powf(1.0 / 2.4) - 0.055
            };
            assert!(((encoded * 255.0).round() - f32::from(expected)).abs() <= 2.0);
        }
        assert_eq!(linear[3], 1.0);
        assert!((linear[7] - 128.0 / 255.0).abs() < 0.00001);
        for c in 0..3 {
            assert!((linear[4 + c] - linear[c] * (128.0 / 255.0)).abs() < 0.001);
        }
        assert_eq!(&linear[8..12], &[0.0; 4]);
        assert!(extended
            .convert_premultiplied(&[1, 2, 3], &mut [0.0; 3])
            .is_err());
        transform(&adobe, &srgb)
            .transform(&sample, &mut intermediate)
            .unwrap();
        transform(&srgb, &p3)
            .transform(&intermediate, &mut indirect)
            .unwrap();
        eprintln!("Adobe RGB green: direct Display P3 {direct:?}, via sRGB {intermediate:?} -> {indirect:?}");
        assert!(direct[..3]
            .iter()
            .zip(&indirect[..3])
            .any(|(a, b)| a.abs_diff(*b) >= 8));
        assert_eq!(direct[3], 255);
        assert_eq!(indirect[3], 255);
    }

    #[test]
    fn photoshop_embedded_adobe_rgb_transforms_and_keeps_alpha() {
        let data = include_bytes!("../tests/fixtures/lp-photoshop-2026-multi-icc.psd");
        let size = u32::from_be_bytes(data[26..30].try_into().unwrap()) as usize;
        let start = 30 + size;
        let length = u32::from_be_bytes(data[start..start + 4].try_into().unwrap()) as usize;
        let resources = &data[start + 4..start + 4 + length];
        let mut at = 0;
        let mut icc = None;
        while at < resources.len() {
            assert_eq!(&resources[at..at + 4], b"8BIM");
            let id = u16::from_be_bytes(resources[at + 4..at + 6].try_into().unwrap());
            let n = resources[at + 6] as usize;
            at += 6 + ((1 + n + 1) & !1);
            let n = u32::from_be_bytes(resources[at..at + 4].try_into().unwrap()) as usize;
            at += 4;
            if id == 1039 {
                icc = Some(&resources[at..at + n]);
            }
            at += n + (n & 1);
        }
        let transform = IccToSrgb::new(icc.unwrap()).unwrap();
        let input = [40, 130, 220, 0, 40, 130, 220, 123, 40, 130, 220, 255];
        let mut output = [0; 12];
        transform.convert(&input, &mut output).unwrap();
        assert_eq!([output[3], output[7], output[11]], [0, 123, 255]);
        assert_eq!(&output[..3], &output[4..7]);
        assert_eq!(&output[..3], &output[8..11]);
        assert_ne!(&output[..3], &input[..3]);
        let mut premult = [0, 0, 0, 0, 20, 65, 110, 128, 40, 130, 220, 255];
        transform.convert_premultiplied(&mut premult).unwrap();
        assert_eq!([premult[3], premult[7], premult[11]], [0, 128, 255]);
        assert_eq!(&premult[..4], &[0, 0, 0, 0]);
        assert_eq!(&premult[8..11], &output[8..11]);
        assert!(premult[4..7].iter().all(|c| *c <= 128));
        assert!(transform.convert(&input, &mut output[..11]).is_err());
        assert!(IccToSrgb::new(b"broken").is_err());
        assert!(IccToSrgb::new(&vec![0; MAX_PROFILE_BYTES + 1]).is_err());
    }
}
