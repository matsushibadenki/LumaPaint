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
    let coverage = mask.map_or(1., |m| m.coverage(point));
    if coverage == 0. {
        return [0; 4];
    }
    let adjusted = effects.apply_at(input, point);
    let alpha = adjusted[3] as f32 / 255.;
    let paper_color = if paper && adjusted[3] < 255 {
        surface.color_at_footprint(point[0], point[1], pixel_size)
    } else {
        [0; 3]
    };
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
    fn reference_composite(
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
    #[test]
    fn masked_composite_matches_reference_for_hidden_and_partial_coverage() {
        use lumapaint_core::{
            layer_mask::{LayerMask, MaskKind},
            selection::{Selection, SelectionShape},
        };
        let selection = Selection::new(SelectionShape::Rectangle, [2., 2., 8., 8.]);
        let settings = lumapaint_core::layer_effects::LayerEffects::default();
        let effects = settings.prepare();
        for kind in [MaskKind::Pixel, MaskKind::Vector] {
            let mut mask = LayerMask::from_selection(kind, Some(&selection), 16, 16).unwrap();
            for density in [0., 0.5, 1.] {
                mask.density = density;
                for inverted in [false, true] {
                    mask.inverted = inverted;
                    for enabled in [false, true] {
                        mask.enabled = enabled;
                        for point in [[0.5, 0.5], [2., 2.], [5.5, 5.5], [15.5, 15.5]] {
                            for input in [[0; 4], [17, 99, 200, 128], [220, 40, 90, 255]] {
                                for paper in [false, true] {
                                    let surface = PaperSettings {
                                        enabled: true,
                                        ..Default::default()
                                    };
                                    assert_eq!(
                                        composite(
                                            input,
                                            point,
                                            &effects,
                                            Some(&mask),
                                            paper,
                                            surface,
                                            [1., 1.]
                                        ),
                                        reference_composite(
                                            input,
                                            point,
                                            &effects,
                                            Some(&mask),
                                            paper,
                                            surface,
                                            [1., 1.]
                                        )
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

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
