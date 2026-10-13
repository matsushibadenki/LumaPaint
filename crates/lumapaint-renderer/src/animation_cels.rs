//! Bounded transparent cel references. Tinting/composition remains Rust-owned.
use lumapaint_core::document::animation::OnionColor;

#[derive(Debug)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    /// Premultiplied sRGB RGBA.
    pub pixels: Vec<u8>,
}
pub fn composite(
    target: &mut Raster,
    source: &Raster,
    color: [u8; 3],
    opacity: f32,
    mode: OnionColor,
) -> Result<(), String> {
    let expected = u64::from(source.width) * u64::from(source.height) * 4;
    if source.width == 0
        || source.height == 0
        || source.width.max(source.height) > 1024
        || source.pixels.len() as u64 != expected
        || target.pixels.len() as u64 != expected
        || (target.width, target.height) != (source.width, source.height)
        || !opacity.is_finite()
        || !(0. ..=1.).contains(&opacity)
    {
        return Err("Invalid onion reference raster".into());
    }
    for (dst, src) in target
        .pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(source.pixels.as_chunks::<4>().0.iter())
    {
        let alpha = src[3] as f32 / 255.;
        let coverage = alpha * opacity;
        for channel in 0..3 {
            let original = src[channel] as f32 / 255.;
            let tint = color[channel] as f32 / 255. * alpha;
            let rgb = match mode {
                OnionColor::Original => original,
                OnionColor::Half => (original + tint) * 0.5,
                OnionColor::Tint => tint,
            };
            dst[channel] = ((rgb * opacity + dst[channel] as f32 / 255. * (1. - coverage)) * 255.)
                .round()
                .clamp(0., 255.) as u8;
        }
        dst[3] = ((coverage + dst[3] as f32 / 255. * (1. - coverage)) * 255.)
            .round()
            .clamp(0., 255.) as u8;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn onion_shader_validates_with_linear_premultiplied_blending() {
        let shader = wgpu::naga::front::wgsl::parse_str(include_str!("onion_skin.wgsl")).unwrap();
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&shader)
        .unwrap();
    }
    #[test]
    fn tint_keeps_coverage_and_blank_paper_transparent() {
        let source = Raster {
            width: 2,
            height: 1,
            pixels: vec![0, 0, 0, 255, 0, 0, 0, 0],
        };
        let mut result = Raster {
            width: 2,
            height: 1,
            pixels: vec![0; 8],
        };
        composite(&mut result, &source, [0, 0, 255], 0.5, OnionColor::Tint).unwrap();
        assert_eq!(result.pixels, [0, 0, 128, 128, 0, 0, 0, 0]);
        composite(&mut result, &source, [255, 0, 0], 0.5, OnionColor::Tint).unwrap();
        assert_eq!(result.pixels, [128, 0, 64, 192, 0, 0, 0, 0]);
        assert!(composite(&mut result, &source, [0; 3], f32::NAN, OnionColor::Tint).is_err());
    }
}
