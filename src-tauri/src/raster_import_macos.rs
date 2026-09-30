//! Temporary image placement: document history changes only on confirmation.
use super::*;
use lumapaint_core::document::Point;
#[cfg(test)]
use lumapaint_formats::native::NativeDocumentCodec;

struct Placement {
    name: String,
    image: String,
    size: [f32; 2],
    matrix: [f32; 6],
    revision: u64,
    drag: Option<(Option<BoxDraft>, [f32; 2], [f32; 6])>,
}
thread_local! { static PLACEMENT: RefCell<Option<Placement>> = const { RefCell::new(None) }; }
pub(super) fn corners() -> Option<[[f32; 2]; 4]> {
    PLACEMENT.with(|p| p.borrow().as_ref().map(Placement::corners))
}
pub(super) fn active() -> bool {
    PLACEMENT.with(|p| p.borrow().is_some())
}
fn notify(value: bool) {
    if let Some(app) = APP.get() {
        let _ = app.emit_to("main", "raster-placement", value);
    }
}
pub(super) fn cancel() {
    PLACEMENT.with(|p| *p.borrow_mut() = None);
    notify(false);
}
fn map(m: [f32; 6], p: [f32; 2]) -> [f32; 2] {
    [
        m[0] * p[0] + m[2] * p[1] + m[4],
        m[1] * p[0] + m[3] * p[1] + m[5],
    ]
}
fn multiply(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    let origin = map(a, [b[4], b[5]]);
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        origin[0],
        origin[1],
    ]
}
impl Placement {
    fn corners(&self) -> [[f32; 2]; 4] {
        let [w, h] = self.size;
        [[0., 0.], [w, 0.], [w, h], [0., h]].map(|p| map(self.matrix, p))
    }
    fn source(&self, width: u32, height: u32) -> String {
        let [a, b, c, d, e, f] = self.matrix;
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><g transform=\"matrix({a} {b} {c} {d} {e} {f})\">{}</g></svg>",self.image)
    }
}
pub(super) fn begin(
    name: String,
    bytes: Vec<u8>,
    info: lumapaint_renderer::vector::ImportedRasterInfo,
) -> Result<(), String> {
    use base64::Engine;
    let (width, height, revision) = DOCUMENT.with(|d| {
        let d = d.borrow();
        let (w, h) = d.dimensions();
        (w, h, d.snapshot().revision)
    });
    let scale = (width as f32 / info.width as f32)
        .min(height as f32 / info.height as f32)
        .min(1.);
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let (w, h) = (info.width as f32, info.height as f32);
    let (raw_w, raw_h) = (info.encoded_width, info.encoded_height);
    let transform = info
        .image_transform()
        .map(|m| {
            format!(
                " transform=\"matrix({} {} {} {} {} {})\"",
                m[0], m[1], m[2], m[3], m[4], m[5]
            )
        })
        .unwrap_or_default();
    let placement = Placement {
        name,
        image: format!(
            "<image width=\"{raw_w}\" height=\"{raw_h}\"{transform} href=\"data:{mime};base64,{data}\"/>"
        ),
        size: [w, h],
        matrix: [
            scale,
            0.,
            0.,
            scale,
            (width as f32 - w * scale) / 2.,
            (height as f32 - h * scale) / 2.,
        ],
        revision,
        drag: None,
    };
    // Validate capacity before exposing the interactive placement.
    DOCUMENT.with(|d| {
        d.borrow()
            .clone()
            .import_raster_layer(placement.name.clone(), placement.source(width, height))
    })?;
    PLACEMENT.with(|p| *p.borrow_mut() = Some(placement));
    TOOL.with(|tool| tool.set(CanvasTool::VectorSelect));
    if let Some(app) = APP.get() {
        let _ = app.emit_to("main", "canvas-tool-changed", CanvasTool::VectorSelect);
    }
    notify(true);
    Ok(())
}
pub(super) fn pointer(point: Point, phase: u8, flags: NSEventModifierFlags) {
    PLACEMENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(p) = slot.as_mut() else {
            return;
        };
        let point = [point.x, point.y];
        if phase == 0 {
            let hit = box_hit_corners(p.corners(), point);
            let corners = p.corners();
            let u = [corners[1][0] - corners[0][0], corners[1][1] - corners[0][1]];
            let v = [corners[3][0] - corners[0][0], corners[3][1] - corners[0][1]];
            let q = [point[0] - corners[0][0], point[1] - corners[0][1]];
            let det = u[0] * v[1] - u[1] * v[0];
            let x = (q[0] * v[1] - q[1] * v[0]) / det;
            let y = (u[0] * q[1] - u[1] * q[0]) / det;
            if hit.is_some() || ((0. ..=1.).contains(&x) && (0. ..=1.).contains(&y)) {
                p.drag = Some((hit, point, p.matrix));
            }
        } else if let Some((mut hit, start, original)) = p.drag {
            let m = if let Some(ref mut d) = hit {
                update_box(d, point, flags);
                d.matrix
            } else {
                [1., 0., 0., 1., point[0] - start[0], point[1] - start[1]]
            };
            p.matrix = multiply(m, original);
            if phase == 2 {
                p.drag = None;
            }
        }
    });
}
pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    PLACEMENT.with(|slot| {
        let slot = slot.borrow();
        let p = slot.as_ref()?;
        Some((|| {
            let mut preview = DOCUMENT.with(|d| d.borrow().clone());
            let (w, h) = preview.dimensions();
            preview.import_raster_layer(p.name.clone(), p.source(w, h))?;
            canvas
                .renderer
                .set_frame_overlay(Some(lumapaint_renderer::FrameOverlay {
                    corners: p.corners(),
                    handles: true,
                }));
            canvas.renderer.render(canvas.viewport, &preview)
        })())
    })
}
pub(super) fn finish(commit: bool) -> Result<DocumentSnapshot, String> {
    if commit {
        PLACEMENT.with(|slot| -> Result<(),String> {
            let slot=slot.borrow();
            let p=slot.as_ref().ok_or("No image placement")?;
            DOCUMENT.with(|d| -> Result<(),String> {
                let mut d=d.borrow_mut();
                if d.snapshot().revision!=p.revision {return Err("Document changed; cancel placement / ドキュメントが変更されました。配置をキャンセルしてください / 文档已更改，请取消放置".into());}
                let (w,h)=d.dimensions();
                // Pixel layers already rasterize their source for display and painting.
                // Retain the original compressed image and affine placement instead of
                // expanding it to a full-document PNG at confirmation.
                d.import_raster_layer(p.name.clone(),p.source(w,h))
            })
        })?;
    }
    cancel();
    redraw()?;
    emit_document();
    Ok(DOCUMENT.with(|d| d.borrow().snapshot()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(width: u32, height: u32) -> lumapaint_renderer::vector::ImportedRasterInfo {
        lumapaint_renderer::vector::ImportedRasterInfo {
            width,
            height,
            encoded_width: width,
            encoded_height: height,
            orientation: 1,
        }
    }

    #[test]
    fn confirmation_keeps_compressed_source_when_full_page_png_exceeds_limit() {
        let settings=serde_json::from_value(serde_json::json!({
            "document":{"name":"Large placement","width":1600,"height":1600,"unit":"pixels","resolution":72,"artboards":false,"canvasColor":"transparent","pixelAspectRatio":1},
            "colorMode":"rgb","colorProfile":"srgb","bitDepth":8
        })).unwrap();
        DOCUMENT.with(|d| *d.borrow_mut() = Document::from_preset(settings).unwrap());
        let mut random = 123456789u32;
        let mut pixels = vec![255; 700 * 700 * 4];
        for pixel in pixels.as_chunks_mut::<4>().0 {
            for channel in &mut pixel[..3] {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                *channel = random as u8;
            }
        }
        let png = lumapaint_renderer::vector::document_png(700, 700, pixels).unwrap();
        assert!(png.len() < 3 * 1024 * 1024 - 1024);
        begin("Noise".into(), png, info(700, 700)).unwrap();
        PLACEMENT.with(|p| {
            p.borrow_mut().as_mut().unwrap().matrix = [1600. / 700., 0., 0., 1600. / 700., 0., 0.]
        });
        let preview = PLACEMENT.with(|p| p.borrow().as_ref().unwrap().source(1600, 1600));
        let expected = lumapaint_renderer::vector::rasterize_svg(&preview, 1600, 1600)
            .unwrap()
            .pixels;
        let expanded =
            lumapaint_renderer::vector::document_png(1600, 1600, expected.clone()).unwrap();
        assert!(clipboard::image_svg(1600, 1600, &expanded).len() > 4 * 1024 * 1024);
        finish(true).unwrap();
        let source = DOCUMENT.with(|d| d.borrow().svg_layers().last().unwrap().source.clone());
        assert_eq!(source, preview);
        assert_eq!(
            lumapaint_renderer::vector::rasterize_svg(&source, 1600, 1600)
                .unwrap()
                .pixels,
            expected
        );
        DOCUMENT.with(|d| {
            let mut d = d.borrow_mut();
            let bytes = d.encode().unwrap();
            let loaded = Document::decode(&bytes).unwrap();
            assert_eq!(loaded.svg_layers().last().unwrap().source, source);
            d.undo();
            assert_eq!(d.svg_layers().count(), 0);
            d.redo();
            assert_eq!(d.svg_layers().last().unwrap().source, source);
            // Move the confirmed large image through the actual selection-tool path.
            pixel_move::pointer(
                &mut d,
                Point { x: 100., y: 100. },
                0,
                NSEventModifierFlags::empty(),
            )
            .unwrap();
            pixel_move::pointer(
                &mut d,
                Point { x: 140., y: 125. },
                1,
                NSEventModifierFlags::empty(),
            )
            .unwrap();
            assert_eq!(d.svg_layers().last().unwrap().source, source);
            let preview_source = d
                .translated_pixel_layer_source(40., 25., false)
                .unwrap()
                .unwrap();
            pixel_move::pointer(
                &mut d,
                Point { x: 140., y: 125. },
                2,
                NSEventModifierFlags::empty(),
            )
            .unwrap();
            let moved = d.svg_layers().last().unwrap().source.clone();
            assert_eq!(moved, preview_source);
            assert!(moved.len() < 4 * 1024 * 1024);
            let raster = lumapaint_renderer::vector::rasterize_svg(&moved, 1600, 1600).unwrap();
            for y in 0..1600usize {
                for x in 0..1600usize {
                    let i = (y * 1600 + x) * 4;
                    let reference = if x >= 40 && y >= 25 {
                        &expected
                            [((y - 25) * 1600 + x - 40) * 4..((y - 25) * 1600 + x - 40) * 4 + 4]
                    } else {
                        &[0; 4]
                    };
                    // Re-evaluating the scaled source at a translated origin can
                    // change interpolation rounding by one channel value.
                    for (actual, expected) in raster.pixels[i..i + 4].iter().zip(reference) {
                        assert!(actual.abs_diff(*expected) <= 2);
                    }
                }
            }
            let saved = d.encode().unwrap();
            assert_eq!(
                Document::decode(&saved)
                    .unwrap()
                    .svg_layers()
                    .last()
                    .unwrap()
                    .source,
                moved
            );
            d.undo();
            assert_eq!(d.svg_layers().last().unwrap().source, source);
            d.redo();
            assert_eq!(d.svg_layers().last().unwrap().source, moved);
        });
    }

    #[test]
    fn placement_transforms_without_editing_then_commits_once_or_cancels() {
        DOCUMENT.with(|d| *d.borrow_mut() = Document::default());
        let before = DOCUMENT.with(|d| d.borrow_mut().encode().unwrap());
        let png =
            lumapaint_renderer::vector::document_png(100, 60, vec![255; 100 * 60 * 4]).unwrap();
        begin("Placed".into(), png.clone(), info(100, 60)).unwrap();
        assert_eq!(DOCUMENT.with(|d| d.borrow_mut().encode().unwrap()), before);
        let original = corners().unwrap();
        let center = [
            (original[0][0] + original[2][0]) / 2.,
            (original[0][1] + original[2][1]) / 2.,
        ];
        pointer(
            Point {
                x: center[0],
                y: center[1],
            },
            0,
            NSEventModifierFlags::empty(),
        );
        pointer(
            Point {
                x: center[0] + 30.,
                y: center[1] + 20.,
            },
            2,
            NSEventModifierFlags::empty(),
        );
        assert_eq!(
            corners().unwrap()[0],
            [original[0][0] + 30., original[0][1] + 20.]
        );
        let corner = corners().unwrap()[2];
        pointer(
            Point {
                x: corner[0],
                y: corner[1],
            },
            0,
            NSEventModifierFlags::empty(),
        );
        pointer(
            Point {
                x: corner[0] + 50.,
                y: corner[1] + 30.,
            },
            2,
            NSEventModifierFlags::Shift,
        );
        let scaled = corners().unwrap();
        assert!((scaled[1][0] - scaled[0][0] - 150.).abs() < 0.01);
        // Rotate using the same corner-exterior gesture as vector boxes.
        let q = scaled[0];
        pointer(
            Point {
                x: q[0] - 10.,
                y: q[1] - 10.,
            },
            0,
            NSEventModifierFlags::empty(),
        );
        assert!(PLACEMENT.with(|p| p.borrow().as_ref().unwrap().drag.unwrap().0.unwrap().rotate));
        pointer(
            Point {
                x: q[0] + 40.,
                y: q[1] - 30.,
            },
            2,
            NSEventModifierFlags::empty(),
        );
        assert!((corners().unwrap()[1][1] - corners().unwrap()[0][1]).abs() > 1.);
        finish(true).unwrap();
        assert!(!active());
        DOCUMENT.with(|d| {
            let mut d = d.borrow_mut();
            assert_eq!(d.svg_layers().last().unwrap().name, "Placed");
            assert!(d.svg_layers().last().unwrap().paint_layer);
            assert!(d.svg_layers().last().unwrap().source.contains("matrix("));
            let saved = d.encode().unwrap();
            assert_eq!(
                Document::decode(&saved)
                    .unwrap()
                    .svg_layers()
                    .last()
                    .unwrap()
                    .name,
                "Placed"
            );
            d.undo();
            assert_eq!(d.encode().unwrap(), before);
            d.redo();
            assert_eq!(d.encode().unwrap(), saved);
        });
        let committed = DOCUMENT.with(|d| d.borrow_mut().encode().unwrap());
        begin("Cancelled".into(), png, info(100, 60)).unwrap();
        finish(false).unwrap();
        assert_eq!(
            DOCUMENT.with(|d| d.borrow_mut().encode().unwrap()),
            committed
        );
    }
}
