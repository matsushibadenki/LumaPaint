//! Small, Rust-owned panel previews. Full document rasters never cross IPC.
use crate::{
    sampled_raster_dabs,
    vector::{clipboard_png, rasterize_svg},
};
use lumapaint_core::{
    document::{CanvasColor, Document},
    tiles::TiledRasterDocument,
};
#[cfg(test)]
use lumapaint_formats::native::NativeDocumentCodec;

pub struct Thumbnails {
    pub layers: Vec<(String, Vec<u8>)>,
    pub channels: Vec<Vec<u8>>,
}
fn over(dst: &mut [u8], src: &[u8], opacity: f32) {
    for (d, s) in dst
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.as_chunks::<4>().0.iter())
    {
        let alpha = s[3] as f32 / 255. * opacity;
        for c in 0..4 {
            d[c] = (s[c] as f32 * opacity + d[c] as f32 * (1. - alpha))
                .round()
                .clamp(0., 255.) as u8;
        }
    }
}
pub fn render(document: &Document) -> Result<Thumbnails, String> {
    let mut result = render_sized(document, 64., false)?;
    let view = document.channel_view();
    let selected = render_sized_inner(&view, 64., true, true)?
        .channels
        .remove(0);
    let scale = (64. / document.dimensions().0.max(document.dimensions().1) as f32).min(1.);
    let (w, h) = document.dimensions();
    let channels = channel_pngs(
        (w as f32 * scale).round().max(1.) as u32,
        (h as f32 * scale).round().max(1.) as u32,
        selected,
    )?;
    for (index, channel) in channels.into_iter().enumerate().skip(1) {
        result.channels[index] = channel;
    }
    Ok(result)
}
pub fn page_preview(document: &Document, max_side: u32) -> Result<Vec<u8>, String> {
    Ok(
        render_sized(document, max_side.clamp(64, 2048) as f32, true)?
            .channels
            .remove(0),
    )
}
/// Export-quality host fallback for PDF effects. All pixels remain in Rust.
pub fn document_png(document: &Document) -> Result<Vec<u8>, String> {
    let (w, h) = document.dimensions();
    if u64::from(w) * u64::from(h) > 16_777_216 {
        return Err("Document exceeds raster export limit".into());
    }
    Ok(render_sized(document, w.max(h) as f32, true)?
        .channels
        .remove(0))
}
fn render_sized(document: &Document, max_side: f32, page_only: bool) -> Result<Thumbnails, String> {
    render_sized_inner(document, max_side, page_only, false)
}
pub fn document_pixels(document: &Document) -> Result<Vec<u8>, String> {
    let (w, h) = document.dimensions();
    crate::vector::document_rgba_len(w, h)?;
    Ok(render_sized_inner(document, w.max(h) as f32, true, true)?
        .channels
        .remove(0))
}
fn render_sized_inner(
    document: &Document,
    max_side: f32,
    page_only: bool,
    raw: bool,
) -> Result<Thumbnails, String> {
    let snapshot = document.snapshot();
    let scale = (max_side / snapshot.width.max(snapshot.height) as f32).min(1.);
    let width = (snapshot.width as f32 * scale).round().max(1.) as u32;
    let height = (snapshot.height as f32 * scale).round().max(1.) as u32;
    let mut tiles = TiledRasterDocument::new(width, height)?;
    tiles.add_layer("thumb".into(), "Thumbnail".into())?;
    if let Some(source) = document.paint_source() {
        let mut pixels = rasterize_svg(source, width, height)?.pixels;
        for p in pixels.as_chunks_mut::<4>().0.iter_mut() {
            if p[3] > 0 {
                for c in 0..3 {
                    p[c] = (p[c] as u32 * 255 / p[3] as u32).min(255) as u8;
                }
            }
        }
        tiles.write_rect("thumb", [0, 0, width, height], &pixels)?;
        tiles.discard_history();
    }
    for stroke in document.committed_paint_strokes() {
        let mut selection = stroke.selection.clone();
        if let Some(selection) = &mut selection {
            for region in &mut selection.regions {
                for p in &mut region.points {
                    p.x *= scale;
                    p.y *= scale;
                }
                region.radius *= scale;
                for value in &mut region.bounds {
                    *value *= scale;
                }
            }
        }
        if stroke.clear {
            if let Some(selection) = &selection {
                tiles.clear_selection("thumb", selection)?;
            }
        } else {
            let mut dabs = sampled_raster_dabs(stroke)?;
            for dab in &mut dabs {
                dab.x *= scale;
                dab.y *= scale;
                dab.radius = (dab.radius * scale).max(0.025);
            }
            if stroke.eraser {
                tiles.erase_dabs_clipped("thumb", &dabs, selection.as_ref())?;
            } else {
                tiles.paint_dabs_clipped("thumb", &dabs, stroke.brush.color, selection.as_ref())?;
            }
        }
        tiles.discard_history();
    }
    let mut effects = document.layer_effects("layer-1");
    let mask = effects.mask.take();
    let effects = effects.prepare();
    let mut paint = vec![0; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let input = tiles.layers()[0].tiles.pixel(x, y).unwrap_or([0; 4]);
            let pixel = crate::root_paint::composite(
                input,
                [
                    (x as f32 + 0.5) * snapshot.width as f32 / width as f32,
                    (y as f32 + 0.5) * snapshot.height as f32 / height as f32,
                ],
                &effects,
                mask.as_ref(),
                snapshot.canvas_color == CanvasColor::White,
            );
            let index = ((y * width + x) * 4) as usize;
            paint[index..index + 4].copy_from_slice(&pixel);
        }
    }
    let background = paint.clone();
    let mut composite = vec![0; paint.len()];
    if document.background_visible() {
        over(&mut composite, &background, document.paint_layer_opacity());
    }
    let mut layers = if page_only {
        Vec::new()
    } else {
        vec![("layer-1".into(), clipboard_png(width, height, background)?)]
    };
    for layer in document
        .svg_layers()
        .filter(|layer| snapshot.layers.iter().any(|l| l.id == layer.id))
    {
        let mut pixels = rasterize_svg(&layer.source, width, height)?.pixels;
        crate::screentone::apply(
            &mut pixels,
            &document.layer_effects(&layer.id),
            width,
            [0., 0., snapshot.width as f32, snapshot.height as f32],
        );
        if layer.visible {
            over(&mut composite, &pixels, layer.effective_opacity());
        }
        if !page_only {
            layers.push((layer.id.clone(), clipboard_png(width, height, pixels)?));
        }
    }
    if let Some(source) = document.clipping_mask_svg() {
        let mask = rasterize_svg(&source, width, height)?.pixels;
        for (p, m) in composite
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(mask.as_chunks::<4>().0.iter())
        {
            for c in p {
                *c = (*c as u16 * m[3] as u16 / 255) as u8;
            }
        }
    }
    if document.color_mode() == lumapaint_core::document::ColorMode::Grayscale {
        crate::color_sampler::grayscale_pixels(&mut composite);
    }
    Ok(Thumbnails {
        layers,
        channels: if raw {
            vec![composite]
        } else if page_only {
            vec![clipboard_png(width, height, composite)?]
        } else {
            channel_pngs(width, height, composite)?
        },
    })
}
fn channel_pngs(width: u32, height: u32, composite: Vec<u8>) -> Result<Vec<Vec<u8>>, String> {
    let mut channels = vec![clipboard_png(width, height, composite.clone())?];
    for channel in 1..=8 {
        let mut pixels = composite.clone();
        for p in pixels.as_chunks_mut::<4>().0.iter_mut() {
            let rgb = [p[0], p[1], p[2]].map(|c| {
                if p[3] == 0 {
                    0.
                } else {
                    c as f32 / p[3] as f32
                }
            });
            let k = 1. - rgb[0].max(rgb[1]).max(rgb[2]);
            let value = match channel {
                1..=3 => rgb[channel - 1],
                4 => p[3] as f32 / 255.,
                5..=7 => 1. - (1. - rgb[channel - 5] - k) / (1. - k).max(0.00001),
                _ => 1. - k,
            };
            let gray = (value * 255.).round().clamp(0., 255.) as u8;
            p.copy_from_slice(&[gray, gray, gray, 255]);
        }
        channels.push(clipboard_png(width, height, pixels)?);
    }
    Ok(channels)
}

pub fn render_tiled(document: &TiledRasterDocument) -> Result<Thumbnails, String> {
    use lumapaint_core::tiles::{TileCoord, TILE_SIZE};
    use std::collections::BTreeSet;
    let (dw, dh) = document.dimensions();
    let scale = (64. / dw.max(dh) as f32).min(1.);
    let (width, height) = (
        (dw as f32 * scale).round().max(1.) as u32,
        (dh as f32 * scale).round().max(1.) as u32,
    );
    let sample = |coords: Vec<TileCoord>,
                  tile: &dyn Fn(TileCoord) -> Option<Vec<u8>>,
                  straight: bool| {
        let mut sums = vec![[0u64; 4]; (width * height) as usize];
        for coord in coords {
            if let Some(pixels) = tile(coord) {
                for (i, p) in pixels.as_chunks::<4>().0.iter().enumerate() {
                    let (x, y) = (
                        coord.x * TILE_SIZE + i as u32 % TILE_SIZE,
                        coord.y * TILE_SIZE + i as u32 / TILE_SIZE,
                    );
                    if x >= dw || y >= dh {
                        continue;
                    }
                    let target = &mut sums[((y * height / dh) * width + x * width / dw) as usize];
                    for c in 0..4 {
                        target[c] += if straight && c < 3 {
                            p[c] as u64 * p[3] as u64 / 255
                        } else {
                            p[c] as u64
                        };
                    }
                }
            }
        }
        let mut result = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let count = (((x + 1) * dw).div_ceil(width) - (x * dw).div_ceil(width)) as u64
                    * (((y + 1) * dh).div_ceil(height) - (y * dh).div_ceil(height)) as u64;
                result.extend(
                    sums[(y * width + x) as usize]
                        .map(|v| ((v + count / 2) / count).min(255) as u8),
                );
            }
        }
        result
    };
    let mut layers = Vec::new();
    let mut coords = BTreeSet::new();
    for layer in document.layers() {
        let allocated: Vec<_> = layer.tiles.allocated_coords().collect();
        coords.extend(allocated.iter().copied());
        let pixels = sample(
            allocated,
            &|coord| layer.tiles.tile(coord).map(<[u8]>::to_vec),
            true,
        );
        layers.push((layer.id.clone(), clipboard_png(width, height, pixels)?));
    }
    let composite = sample(
        coords.into_iter().collect(),
        &|coord| document.composite_tile(coord),
        false,
    );
    Ok(Thumbnails {
        layers,
        channels: channel_pngs(width, height, composite)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::document::{Brush, Point};
    fn pixel(png: &[u8], x: u32, y: u32) -> [u8; 4] {
        let image = resvg::tiny_skia::Pixmap::decode_png(png).unwrap();
        let offset = ((y * image.width() + x) * 4) as usize;
        image.data()[offset..offset + 4].try_into().unwrap()
    }
    #[test]
    fn thumbnails_render_pixels_layers_and_channels_and_track_visibility() {
        let mut doc = Document::default();
        doc.begin(
            Point { x: 480., y: 320. },
            Brush {
                size: 200.,
                hardness: 1.,
                color: [255, 0, 0],
                ..Brush::default()
            },
        )
        .unwrap();
        doc.finish();
        let before = doc.encode().unwrap();
        let thumbs = render(&doc).unwrap();
        assert_eq!(pixel(&thumbs.layers[0].1, 32, 21), [255, 0, 0, 255]);
        assert_eq!(pixel(&thumbs.channels[1], 32, 21), [255; 4]);
        assert_eq!(pixel(&thumbs.channels[2], 32, 21), [0, 0, 0, 255]);
        assert_eq!(pixel(&thumbs.channels[4], 32, 21), [255; 4]);
        assert_eq!(doc.encode().unwrap(), before);
        doc.toggle_layer("layer-1").unwrap();
        let hidden = render(&doc).unwrap();
        assert_eq!(hidden.layers[0].1, thumbs.layers[0].1);
        assert_eq!(pixel(&hidden.channels[4], 32, 21), [0, 0, 0, 255]);
        doc.import_svg("Blue".into(),"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960\" height=\"640\"><rect width=\"960\" height=\"640\" fill=\"blue\"/></svg>".into()).unwrap();
        // Channel thumbnails inspect the selected layer target, not all layers.
        let id = doc.svg_layers().last().unwrap().id.clone();
        doc.select_layer(id).unwrap();
        let image = render(&doc).unwrap();
        assert_eq!(pixel(&image.layers[1].1, 32, 21), [0, 0, 255, 255]);
        assert_eq!(pixel(&image.channels[3], 32, 21), [255; 4]);
        assert_eq!(pixel(&image.channels[5], 32, 21), [0, 0, 0, 255]);
    }
    #[test]
    fn selected_mask_alpha_is_visible_above_white_paper_and_updates_after_edit() {
        use lumapaint_core::{
            layer_effects::LayerEffects,
            layer_mask::{LayerEditTarget, LayerMask, MaskKind},
            selection::{Selection, SelectionShape},
        };
        let mut state = Document::default().document_state();
        state.width = 32;
        state.height = 32;
        let mut doc = Document::from_document_state(state).unwrap();
        let mask = LayerMask::from_selection(
            MaskKind::Pixel,
            Some(&Selection::new(
                SelectionShape::Rectangle,
                [0., 0., 16., 32.],
            )),
            32,
            32,
        )
        .unwrap();
        doc.set_layer_effects(
            "layer-1",
            LayerEffects {
                mask: Some(mask),
                ..Default::default()
            },
        )
        .unwrap();
        // The root mask covers the white paper too, in export and GPU tile uploads.
        let pixels = document_pixels(&doc).unwrap();
        assert_eq!(pixels[(8 * 32 + 24) * 4 + 3], 0);
        assert_eq!(pixels[(8 * 32 + 8) * 4 + 3], 255);
        let mut cache = crate::paint_cache::PaintCache::new((32, 32)).unwrap();
        let uploads = cache.prepare(&doc, true).unwrap();
        assert_eq!(uploads[0].pixels, pixels);
        let upper = doc.add_paint_layer().unwrap();
        doc.select_layer(upper).unwrap();
        doc.replace_moved_pixels(r#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="red"/></svg>"#.into(),None).unwrap();
        doc.select_layer_target("layer-1".into(), LayerEditTarget::Mask)
            .unwrap();
        let before = render(&doc).unwrap();
        assert_eq!(pixel(&before.channels[4], 24, 8), [0, 0, 0, 255]);
        assert_eq!(pixel(&before.channels[4], 8, 8), [255; 4]);
        let mut effects = doc.layer_effects("layer-1");
        effects
            .mask
            .as_mut()
            .unwrap()
            .paint_selection(
                &Selection::new(SelectionShape::Rectangle, [4., 4., 8., 8.]),
                None,
                false,
            )
            .unwrap();
        doc.set_layer_effects("layer-1", effects).unwrap();
        assert_eq!(
            pixel(&render(&doc).unwrap().channels[4], 8, 8),
            [0, 0, 0, 255]
        );
        doc.undo();
        assert_eq!(pixel(&render(&doc).unwrap().channels[4], 8, 8), [255; 4]);
    }
    #[test]
    fn tiled_thumbnails_average_pixels_and_apply_masks_to_channels() {
        let mut doc = TiledRasterDocument::new(128, 128).unwrap();
        doc.add_layer("a".into(), "Red".into()).unwrap();
        doc.write_rect("a", [0, 0, 128, 128], &[255, 0, 0, 255].repeat(128 * 128))
            .unwrap();
        doc.set_layer_mask("a", true, false, 0.5).unwrap();
        let thumbs = render_tiled(&doc).unwrap();
        assert_eq!(pixel(&thumbs.layers[0].1, 32, 32), [255, 0, 0, 255]);
        assert_eq!(pixel(&thumbs.channels[4], 32, 32), [128, 128, 128, 255]);
        doc.set_layer_appearance("a", false, 1.).unwrap();
        let hidden = render_tiled(&doc).unwrap();
        assert_eq!(hidden.layers[0].1, thumbs.layers[0].1);
        assert_eq!(pixel(&hidden.channels[4], 32, 32), [0, 0, 0, 255]);
    }

    #[test]
    fn small_brushes_on_large_documents_stay_bounded() {
        let mut doc = Document::default();
        doc.begin(
            Point { x: 20., y: 20. },
            Brush {
                size: 1.,
                ..Brush::default()
            },
        )
        .unwrap();
        doc.finish();
        let thumbs = render(&doc).unwrap();
        for png in thumbs
            .channels
            .iter()
            .chain(thumbs.layers.iter().map(|(_, png)| png))
        {
            let image = resvg::tiny_skia::Pixmap::decode_png(png).unwrap();
            assert!(image.width() <= 64 && image.height() <= 64);
        }
    }
}
