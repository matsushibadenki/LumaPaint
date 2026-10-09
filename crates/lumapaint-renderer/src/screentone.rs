//! Coordinate-aware adjustment entry point for export, tiles and viewport images.
use lumapaint_core::layer_effects::LayerEffects;
pub fn apply(pixels: &mut [u8], effects: &LayerEffects, width: u32, rect: [f32; 4]) {
    if !effects.active() {
        return;
    }
    if effects.screentone.is_none() && effects.mask.is_none() {
        crate::apply_layer_effects(pixels, effects);
        return;
    }
    let height = pixels.len() / 4 / width.max(1) as usize;
    if width == 0 || height == 0 {
        return;
    }
    let mapping = [
        rect[0],
        rect[1],
        rect[2] / width as f32,
        rect[3] / height as f32,
    ];
    if super::layer_effects_gpu::apply_region(pixels, effects, width, mapping) {
        return;
    }
    apply_cpu(pixels, effects, width, mapping);
}
pub(super) fn apply_cpu(pixels: &mut [u8], effects: &LayerEffects, width: u32, mapping: [f32; 4]) {
    if !effects.active() {
        return;
    }
    let plan = effects.prepare();
    for (i, p) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let a = u32::from(p[3]);
        if a == 0 {
            continue;
        }
        let straight = [0, 1, 2].map(|c| ((u32::from(p[c]) * 255 + a / 2) / a).min(255) as u8);
        let point = [
            mapping[0] + (i as u32 % width) as f32 * mapping[2] + mapping[2] * 0.5,
            mapping[1] + (i as u32 / width) as f32 * mapping[3] + mapping[3] * 0.5,
        ];
        let out = plan.apply_at([straight[0], straight[1], straight[2], p[3]], point);
        for c in 0..3 {
            p[c] = ((u32::from(out[c]) * u32::from(out[3]) + 127) / 255) as u8;
        }
        p[3] = out[3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tone_viewport_mapping_matches_split_regions_and_never_changes_source() {
        let effects = LayerEffects {
            enabled: true,
            screentone: Some(Default::default()),
            ..Default::default()
        };
        let source = [30, 40, 50, 128].repeat(512 * 4);
        let mut full = source.clone();
        apply_cpu(&mut full, &effects, 512, [-9., 13., 0.75, 0.5]);
        for row in 0..4 {
            for col in [0, 256] {
                let mut region = [30, 40, 50, 128].repeat(256);
                apply_cpu(
                    &mut region,
                    &effects,
                    256,
                    [-9. + col as f32 * 0.75, 13. + row as f32 * 0.5, 0.75, 0.5],
                );
                let start = (row * 512 + col) * 4;
                assert_eq!(&full[start..start + 1024], &region);
            }
        }
        let mut disabled = effects;
        disabled.enabled = false;
        let mut unchanged = source.clone();
        apply_cpu(&mut unchanged, &disabled, 512, [0., 0., 1., 1.]);
        assert_eq!(unchanged, source);
    }
    #[test]
    fn screentone_selected_fill_exports_the_same_pattern_as_the_model() {
        use lumapaint_core::{
            document::Document,
            vector::{PathOperation, VectorPath, VectorPathEngine},
        };
        struct Engine;
        impl VectorPathEngine for Engine {
            fn combine(
                &self,
                _: &VectorPath,
                _: &VectorPath,
                _: PathOperation,
            ) -> Result<VectorPath, String> {
                Err("unexpected boolean operation".into())
            }
            fn contains(&self, _: &VectorPath, _: [f32; 2]) -> Result<bool, String> {
                Ok(false)
            }
        }
        let mut state = Document::default().document_state();
        state.width = 32;
        state.height = 16;
        state.resolution = Some(300);
        let mut document = Document::from_document_state(state).unwrap();
        document
            .set_tool_pixel_selection(Some(lumapaint_core::selection::Selection::new(
                lumapaint_core::selection::SelectionShape::Rectangle,
                [0., 0., 16., 16.],
            )))
            .unwrap();
        let tone = lumapaint_core::screentone::Screentone {
            frequency: 30.,
            ..Default::default()
        };
        document
            .add_screentone_layer(tone.clone(), &Engine)
            .unwrap();
        let output = crate::thumbnails::document_pixels(&document).unwrap();
        for y in 0..16 {
            for x in 0..32 {
                let tone_pixel = tone.apply([0, 0, 0, 255], [x as f32 + 0.5, y as f32 + 0.5]);
                let expected = if x < 16 && tone_pixel[3] > 0 {
                    tone_pixel
                } else {
                    [255; 4]
                };
                let i = (y * 32 + x) * 4;
                assert_eq!(&output[i..i + 4], &expected, "at {x},{y}");
            }
        }
    }
}

#[cfg(test)]
mod layer_mask_tests {
    use lumapaint_core::{
        document::Document,
        layer_effects::LayerEffects,
        layer_mask::{LayerMask, MaskKind},
        selection::{Selection, SelectionShape},
    };
    #[test]
    fn four_layer_mask_combinations_render_and_export_with_original_sources_intact() {
        for vector in [false, true] {
            for kind in [MaskKind::Pixel, MaskKind::Vector] {
                let mut document = Document::default();
                let id = if vector {
                    document.add_vector_layer().unwrap()
                } else {
                    document.add_paint_layer().unwrap()
                };
                let mut state = document.document_state();
                state.width = 16;
                state.height = 16;
                state.svg_layers.last_mut().unwrap().source="<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"><rect width=\"16\" height=\"16\" fill=\"black\"/></svg>".into();
                let original = state.svg_layers.last().unwrap().source.clone();
                let mut document = Document::from_document_state(state).unwrap();
                let selection = Selection::new(SelectionShape::Rectangle, [0., 0., 8., 16.]);
                let mask = LayerMask::from_selection(kind, Some(&selection), 16, 16).unwrap();
                document
                    .set_layer_effects(
                        &id,
                        LayerEffects {
                            mask: Some(mask),
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let output = crate::thumbnails::document_pixels(&document).unwrap();
                for y in 0..16 {
                    for x in 0..16 {
                        let i = (y * 16 + x) * 4;
                        let expected = if x < 8 { [0, 0, 0, 255] } else { [255; 4] };
                        assert_eq!(
                            &output[i..i + 4],
                            &expected,
                            "vector={vector}, mask={kind:?} at {x},{y}"
                        );
                    }
                }
                assert_eq!(
                    document.document_state().svg_layers.last().unwrap().source,
                    original
                );
            }
        }
    }
}
