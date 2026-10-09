//! Shared selection tools on pixel layers; image data never crosses the WebView bridge.
use super::*;
use lumapaint_core::document::{Point, Selection};

pub(super) struct PixelDrag {
    start: Point,
    current: Point,
    layer: String,
    selection: Option<Selection>,
    pixels: Option<Vec<u8>>,
    copy: bool,
    retained_source: bool,
    content_bounds: Option<[f32; 4]>,
    dimensions: (u32, u32),
}

pub(super) fn pointer(
    doc: &mut Document,
    point: Point,
    phase: u8,
    flags: NSEventModifierFlags,
) -> Result<(), String> {
    if phase == 0 {
        cancel_vector_drag();
        let snapshot = doc.snapshot();
        let layer = snapshot
            .layers
            .iter()
            .find(|l| l.id == snapshot.layer_id)
            .ok_or("Layer not found")?;
        if layer.locked || !layer.visible {
            return Err("Unlock and show the layer / レイヤーのロックを解除し表示してください / 请解锁并显示图层".into());
        }
        let selection = doc.selection().cloned();
        let copy = flags.contains(NSEventModifierFlags::Option);
        let image = selection.is_none() || copy || flags.contains(NSEventModifierFlags::Command);
        if image && layer.alpha_locked {
            return Err("Unlock layer transparency / 透明ピクセルのロックを解除してください / 请解锁透明像素".into());
        }
        let retained_source = image
            && selection.is_none()
            && doc.translated_pixel_layer_source(0., 0., copy)?.is_some();
        let content_bounds = if retained_source {
            let source = doc
                .svg_layers()
                .find(|l| l.id == snapshot.layer_id)
                .ok_or("Image layer not found")?;
            Some(lumapaint_renderer::vector::pixel_source_bounds(
                &source.source,
            )?)
        } else {
            None
        };
        let pixels = if image && !retained_source {
            Some(clipboard::raw_selected_pixels(doc)?.2)
        } else {
            None
        };
        PIXEL_DRAG.with(|d| {
            *d.borrow_mut() = Some(PixelDrag {
                start: point,
                current: point,
                layer: snapshot.layer_id,
                selection,
                pixels,
                copy,
                retained_source,
                content_bounds,
                dimensions: doc.dimensions(),
            })
        });
    } else {
        PIXEL_DRAG.with(|d| {
            if let Some(d) = d.borrow_mut().as_mut() {
                d.current = point;
            }
        });
        if phase == 2 {
            if let Some(d) = PIXEL_DRAG.with(|d| d.borrow_mut().take()) {
                if doc.snapshot().layer_id != d.layer || doc.dimensions() != d.dimensions {
                    return Err("Selection changed / 選択が変更されました / 选区已更改".into());
                }
                let dx = (d.current.x - d.start.x).round() as i32;
                let dy = (d.current.y - d.start.y).round() as i32;
                if dx == 0 && dy == 0 {
                    return Ok(());
                }
                if d.retained_source {
                    let source = doc
                        .translated_pixel_layer_source(dx as f32, dy as f32, d.copy)?
                        .ok_or("Image layer changed")?;
                    doc.replace_transformed_pixels(
                        source,
                        None,
                        [1., 0., 0., 1., dx as f32, dy as f32],
                        !d.copy,
                    )?;
                } else if let Some(pixels) = d.pixels {
                    let (w, h) = d.dimensions;
                    let moved = move_pixels(&pixels, w, h, d.selection.as_ref(), dx, dy, d.copy);
                    let png = lumapaint_renderer::vector::document_png(w, h, moved)?;
                    let source = clipboard::image_svg(w, h, &png);
                    let whole = d.selection.is_none() && !d.copy;
                    doc.replace_transformed_pixels(
                        source,
                        d.selection.map(|s| s.translated(dx as f32, dy as f32)),
                        [1., 0., 0., 1., dx as f32, dy as f32],
                        whole,
                    )?;
                } else {
                    doc.translate_pixel_selection(d.selection, dx as f32, dy as f32);
                }
            }
        }
    }
    Ok(())
}

pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    PIXEL_DRAG.with(|slot| {
        let slot = slot.borrow();
        let drag = slot.as_ref().filter(|d| d.retained_source)?;
        Some((|| {
            let mut preview = DOCUMENT.with(|d| d.borrow().clone_for_rendering());
            let dx = (drag.current.x - drag.start.x).round();
            let dy = (drag.current.y - drag.start.y).round();
            if dx != 0. || dy != 0. {
                let source = preview
                    .translated_pixel_layer_source(dx, dy, drag.copy)?
                    .ok_or("Image layer changed")?;
                preview.replace_transformed_pixels(
                    source,
                    None,
                    [1., 0., 0., 1., dx, dy],
                    !drag.copy,
                )?;
            }
            canvas.renderer.set_frame_overlay(overlay());
            canvas.renderer.render(canvas.viewport, &preview)
        })())
    })
}

fn move_pixels(
    pixels: &[u8],
    w: u32,
    h: u32,
    selection: Option<&Selection>,
    dx: i32,
    dy: i32,
    copy: bool,
) -> Vec<u8> {
    let selected = |x: u32, y: u32| {
        selection.is_none_or(|s| {
            s.contains(Point {
                x: x as f32 + 0.5,
                y: y as f32 + 0.5,
            })
        })
    };
    let mut result = pixels.to_vec();
    if !copy {
        for y in 0..h {
            for x in 0..w {
                if selected(x, y) {
                    let i = ((y * w + x) * 4) as usize;
                    result[i..i + 4].fill(0);
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            if !selected(x, y) {
                continue;
            }
            let (tx, ty) = (i64::from(x) + i64::from(dx), i64::from(y) + i64::from(dy));
            if tx < 0 || ty < 0 || tx >= i64::from(w) || ty >= i64::from(h) {
                continue;
            }
            let src = ((y * w + x) * 4) as usize;
            let dst = ((ty as u32 * w + tx as u32) * 4) as usize;
            let inv = 255 - u32::from(pixels[src + 3]);
            for c in 0..4 {
                result[dst + c] = (u32::from(pixels[src + c])
                    + (u32::from(result[dst + c]) * inv + 127) / 255)
                    .min(255) as u8;
            }
        }
    }
    result
}

pub(super) fn overlay() -> Option<lumapaint_renderer::FrameOverlay> {
    PIXEL_DRAG.with(|d| {
        d.borrow().as_ref().map(|d| {
            // Use the same pixel-aligned displacement as the image preview and commit.
            let dx = if d.retained_source || d.pixels.is_some() {
                (d.current.x - d.start.x).round()
            } else {
                d.current.x - d.start.x
            };
            let dy = if d.retained_source || d.pixels.is_some() {
                (d.current.y - d.start.y).round()
            } else {
                d.current.y - d.start.y
            };
            let bounds = d
                .selection
                .as_ref()
                .and_then(|s| {
                    s.regions
                        .iter()
                        .map(|r| {
                            let [x, y, w, h] = r.bounds;
                            [x, y, x + w, y + h]
                        })
                        .reduce(|a, b| {
                            [
                                a[0].min(b[0]),
                                a[1].min(b[1]),
                                a[2].max(b[2]),
                                a[3].max(b[3]),
                            ]
                        })
                })
                .map(|[x, y, r, b]| [x, y, r - x, b - y])
                .or(d.content_bounds)
                .unwrap_or([0., 0., d.dimensions.0 as f32, d.dimensions.1 as f32]);
            let [x, y, w, h] = bounds;
            lumapaint_renderer::FrameOverlay {
                corners: [
                    [x + dx, y + dy],
                    [x + w + dx, y + dy],
                    [x + w + dx, y + h + dy],
                    [x + dx, y + h + dy],
                ],
                handles: false,
                baseline: None,
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_guide_tracks_content_instead_of_document_after_repeated_moves() {
        let mut doc = Document::default();
        doc.import_raster_layer("Image".into(),r#"<svg xmlns="http://www.w3.org/2000/svg" width="960" height="640"><g transform="translate(70 90)"><rect width="100" height="150" fill="red"/></g></svg>"#.into()).unwrap();
        for offset in [0., 30.] {
            pointer(
                &mut doc,
                Point { x: 100., y: 100. },
                0,
                NSEventModifierFlags::empty(),
            )
            .unwrap();
            pointer(
                &mut doc,
                Point { x: 130.4, y: 140.2 },
                1,
                NSEventModifierFlags::empty(),
            )
            .unwrap();
            let frame = overlay().unwrap();
            let top = 90. + offset / 30. * 40.;
            assert_eq!(
                frame.corners,
                [
                    [100. + offset, top + 40.],
                    [200. + offset, top + 40.],
                    [200. + offset, top + 190.],
                    [100. + offset, top + 190.]
                ]
            );
            let source = doc
                .translated_pixel_layer_source(30., 40., false)
                .unwrap()
                .unwrap();
            let bounds = lumapaint_renderer::vector::pixel_source_bounds(&source).unwrap();
            assert_eq!(frame.corners[0], [bounds[0], bounds[1]]);
            assert_eq!(
                frame.corners[2],
                [bounds[0] + bounds[2], bounds[1] + bounds[3]]
            );
            pointer(
                &mut doc,
                Point { x: 130.4, y: 140.2 },
                2,
                NSEventModifierFlags::empty(),
            )
            .unwrap();
        }
        assert!(overlay().is_none());
    }

    #[test]
    fn shared_pointer_routes_mask_command_and_option_without_preview_mutation() {
        let mut doc = Document::default();
        doc.begin(
            Point { x: 20., y: 20. },
            Brush {
                size: 8.,
                hardness: 1.,
                ..Default::default()
            },
        )
        .unwrap();
        doc.finish();
        doc.begin_selection_edit(
            Point { x: 10., y: 10. },
            SelectionShape::Rectangle,
            SelectionMode::Replace,
        )
        .unwrap();
        doc.extend_selection(Point { x: 30., y: 30. }, true);
        let initial = doc.selection().cloned();
        let start = Point { x: 20., y: 20. };
        let end = Point { x: 40., y: 20. };
        pointer(&mut doc, start, 0, NSEventModifierFlags::empty()).unwrap();
        pointer(&mut doc, end, 1, NSEventModifierFlags::empty()).unwrap();
        assert_eq!(doc.selection(), initial.as_ref());
        pointer(&mut doc, end, 2, NSEventModifierFlags::empty()).unwrap();
        assert_eq!(doc.selection().unwrap().regions[0].bounds[0], 30.);
        doc.translate_pixel_selection(initial.clone(), 0., 0.);
        for flags in [NSEventModifierFlags::Command, NSEventModifierFlags::Option] {
            pointer(&mut doc, start, 0, flags).unwrap();
            pointer(&mut doc, end, 2, flags).unwrap();
            let image = clipboard::raw_selected_pixels(&doc).unwrap().2;
            let alpha = |x: usize| image[(20 * 960 + x) * 4 + 3];
            assert!(alpha(40) > 0);
            assert_eq!(alpha(20) > 0, flags.contains(NSEventModifierFlags::Option));
            doc.undo();
            assert_eq!(doc.selection(), initial.as_ref());
        }
        cancel_vector_drag();
    }

    #[test]
    fn moves_copies_clips_and_preserves_transparency() {
        let pixels = vec![100, 0, 0, 128, 0, 0, 0, 0, 0, 0, 200, 255];
        let select = Selection::new(SelectionShape::Rectangle, [0., 0., 1., 1.]);
        assert_eq!(
            move_pixels(&pixels, 3, 1, Some(&select), 1, 0, false),
            vec![0, 0, 0, 0, 100, 0, 0, 128, 0, 0, 200, 255]
        );
        assert_eq!(
            move_pixels(&pixels, 3, 1, Some(&select), 1, 0, true),
            vec![100, 0, 0, 128, 100, 0, 0, 128, 0, 0, 200, 255]
        );
        assert_eq!(
            move_pixels(&pixels, 3, 1, None, -1, 0, false),
            vec![0, 0, 0, 0, 0, 0, 200, 255, 0, 0, 0, 0]
        );
    }
}
