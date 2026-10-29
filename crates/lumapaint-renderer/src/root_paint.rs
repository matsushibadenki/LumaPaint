//! Compose the root layer's paper before its mask, consistently across previews
//! and exports. Input is straight RGBA; output is premultiplied RGBA.
use lumapaint_core::{
    document::{CanvasColor, Document},
    layer_effects::PreparedEffects,
    layer_mask::LayerMask,
};
pub(crate) fn masked_paper(document: &Document) -> bool {
    document.canvas_color() == CanvasColor::White
        && document
            .layer_effects("layer-1")
            .mask
            .is_some_and(|m| m.enabled)
}
pub(crate) fn composite(
    input: [u8; 4],
    point: [f32; 2],
    effects: &PreparedEffects<'_>,
    mask: Option<&LayerMask>,
    paper: bool,
    surface: lumapaint_core::natural_media::PaperSettings,
    pixel_size: [f32; 2],
) -> [u8; 4] {
    let adjusted = effects.apply_at(input, point);
    let alpha = adjusted[3] as f32 / 255.;
    let coverage = mask.map_or(1., |m| m.coverage(point));
    let paper_color = surface.color_at_footprint(point[0], point[1], pixel_size);
    let mut out = [0; 4];
    for c in 0..3 {
        out[c] = ((adjusted[c] as f32 * alpha
            + if paper {
                paper_color[c] as f32 * (1. - alpha)
            } else {
                0.
            })
            * coverage)
            .round() as u8;
    }
    out[3] = ((if paper { 255. } else { adjusted[3] as f32 }) * coverage).round() as u8;
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::natural_media::PaperSettings;
    #[test]
    fn natural_media_paper_preserves_alpha_and_mask() {
        let effects = lumapaint_core::layer_effects::LayerEffects::default();
        let effects = effects.prepare();
        let paper = PaperSettings {
            enabled: true,
            ..Default::default()
        };
        let point = [12.5, 9.5];
        let out = composite([0; 4], point, &effects, None, true, paper, [1., 1.]);
        assert_eq!(out[..3], paper.color_at(point[0], point[1]));
        assert_eq!(out[3], 255);
        assert_eq!(
            composite([0; 4], point, &effects, None, false, paper, [1., 1.]),
            [0; 4]
        );
        assert_eq!(
            composite(
                [10, 20, 30, 255],
                point,
                &effects,
                None,
                true,
                paper,
                [1., 1.]
            ),
            [10, 20, 30, 255]
        );
    }
}
