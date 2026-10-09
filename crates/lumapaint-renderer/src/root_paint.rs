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
) -> [u8; 4] {
    let adjusted = effects.apply_at(input, point);
    let alpha = adjusted[3] as f32 / 255.;
    let coverage = mask.map_or(1., |m| m.coverage(point));
    let mut out = [0; 4];
    for c in 0..3 {
        out[c] = ((adjusted[c] as f32 * alpha + if paper { 255. * (1. - alpha) } else { 0. })
            * coverage)
            .round() as u8;
    }
    out[3] = ((if paper { 255. } else { adjusted[3] as f32 }) * coverage).round() as u8;
    out
}
