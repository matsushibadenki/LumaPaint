//! Clipboard transfer is staged before cut mutates the document.
use super::*;
use base64::Engine;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSData, NSString};
use serde::{Deserialize, Serialize};
const TYPE: &str = "com.lumapaint.selection.v1";
const LIMIT: usize = 8 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Content {
    Vectors { objects: Vec<VectorObject> },
    Pixels { source: String },
}

fn image_svg(width: u32, height: u32, png: &[u8]) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><image width=\"{width}\" height=\"{height}\" href=\"data:image/png;base64,{data}\"/></svg>")
}

fn read() -> Result<Content, String> {
    let board = NSPasteboard::generalPasteboard();
    if let Some(value) = board.stringForType(&NSString::from_str(TYPE)) {
        if value.length() > LIMIT {
            return Err("Clipboard content is too large".into());
        }
        let json = value.to_string();
        return serde_json::from_str(&json).map_err(|e| e.to_string());
    }
    if let Some(data) = board.dataForType(&NSString::from_str("public.png")) {
        if data.length() > LIMIT {
            return Err("Clipboard image is too large".into());
        }
        let bytes = data.to_vec();
        let (w, h) = lumapaint_renderer::vector::clipboard_png_size(&bytes)?;
        return Ok(Content::Pixels {
            source: image_svg(w, h, &bytes),
        });
    }
    Err("No supported content in clipboard / 貼り付け可能なデータがありません / 剪贴板中没有可粘贴的数据".into())
}

fn write(content: &Content, png: Option<&[u8]>) -> Result<(), String> {
    let json = serde_json::to_string(content).map_err(|e| e.to_string())?;
    if json.len() > LIMIT {
        return Err("Clipboard content is too large".into());
    }
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    if !board.setString_forType(&NSString::from_str(&json), &NSString::from_str(TYPE)) {
        return Err("Cannot write clipboard".into());
    }
    if let Some(png) = png {
        board.setData_forType(
            Some(&NSData::with_bytes(png)),
            &NSString::from_str("public.png"),
        );
    }
    Ok(())
}

fn selected_pixels(document: &Document) -> Result<(u32, u32, Vec<u8>), String> {
    let snapshot = document.snapshot();
    let (width, height) = document.dimensions();
    if u64::from(width) * u64::from(height) > 16_777_216 {
        return Err("Clipboard image is too large (16 megapixels maximum) / コピーできる画像は1600万画素までです / 剪贴板图像最多1600万像素".into());
    }
    let layer = snapshot
        .layers
        .iter()
        .find(|layer| layer.id == snapshot.layer_id)
        .ok_or("Layer not found")?;
    if !layer.visible {
        return Err(
            "Show the selected layer first / 選択レイヤーを表示してください / 请显示所选图层"
                .into(),
        );
    }
    let mut pixels = if layer.id == "layer-1" {
        let projected = lumapaint_renderer::project_committed_paint_layer(document)?;
        let tiles = &projected.layers()[0].tiles;
        let mut pixels = vec![0; width as usize * height as usize * 4];
        for y in 0..height {
            for x in 0..width {
                let mut p = tiles.pixel(x, y).unwrap_or([0; 4]);
                let alpha = p[3];
                for channel in &mut p[..3] {
                    *channel = (u16::from(*channel) * u16::from(alpha) / 255) as u8;
                }
                let offset = ((y * width + x) * 4) as usize;
                pixels[offset..offset + 4].copy_from_slice(&p);
            }
        }
        pixels
    } else {
        let source = &document
            .svg_layers()
            .find(|layer| layer.id == snapshot.layer_id)
            .ok_or("Layer not found")?
            .source;
        lumapaint_renderer::vector::rasterize_svg(source, width, height)?.pixels
    };
    // Layer appearance remains on the source layer; copied pixels retain it once.
    let opacity = layer.opacity
        * if !layer.mask_enabled {
            1.
        } else if layer.mask_inverted {
            1. - layer.mask_density
        } else {
            layer.mask_density
        };
    for pixel in pixels.as_chunks_mut::<4>().0.iter_mut() {
        for channel in pixel {
            *channel = (f32::from(*channel) * opacity).round() as u8;
        }
    }
    Ok((width, height, pixels))
}

pub fn action(action: DocumentAction) -> Result<(), String> {
    ensure_document_open()?;
    if matches!(action, DocumentAction::Paste) {
        let content = read()?;
        DOCUMENT.with(|document| { let mut document=document.borrow_mut(); match content {
            Content::Vectors { objects } => document.paste_content(None,objects),
            Content::Pixels { source } => {
                validate_svg(&source)?;
                let (width,height)=document.dimensions();
                document.paste_content(Some(format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\">{source}</svg>")),vec![])
            }
        } })?;
    } else {
        let cut = matches!(action, DocumentAction::Cut);
        let mut candidate = DOCUMENT.with(|document| document.borrow().clone());
        if !candidate.selected_vector_ids().is_empty() && candidate.selection().is_none() {
            let objects: Vec<_> = candidate
                .svg_layers()
                .filter(|layer| layer.visible)
                .flat_map(|layer| layer.vector_objects.iter())
                .filter(|object| {
                    object.visible && candidate.selected_vector_ids().contains(&object.id)
                })
                .cloned()
                .collect();
            if objects.is_empty() {
                return Err(
                    "Select objects to copy / オブジェクトを選択してください / 请选择对象".into(),
                );
            }
            if cut {
                if objects.len() != candidate.selected_vector_ids().len() {
                    return Err("Show all selected objects before cutting / 選択オブジェクトをすべて表示してください / 请先显示所有选中的对象".into());
                }
                candidate.delete_selected_vector_objects()?;
            }
            write(&Content::Vectors { objects }, None)?;
        } else {
            let snapshot = candidate.snapshot();
            let selection = candidate.selection().cloned().unwrap_or_else(|| {
                lumapaint_core::document::Selection::new(
                    SelectionShape::Rectangle,
                    [0., 0., snapshot.width as f32, snapshot.height as f32],
                )
            });
            let (width, height, pixels) = selected_pixels(&candidate)?;
            let mut copied = pixels.clone();
            for (index, pixel) in copied.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                if !selection.contains(lumapaint_core::document::Point {
                    x: (index as u32 % width) as f32 + 0.5,
                    y: (index as u32 / width) as f32 + 0.5,
                }) {
                    pixel.fill(0);
                }
            }
            if !copied.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0) {
                return Err(
                    "Selected area is empty / 選択範囲に内容がありません / 所选区域为空".into(),
                );
            }
            let png = lumapaint_renderer::vector::clipboard_png(width, height, copied)?;
            if cut {
                if snapshot.layer_id == "layer-1" {
                    if candidate.selection().is_none() {
                        candidate.select_all();
                    }
                    candidate.cut_paint_selection()?;
                    if snapshot.selection.is_none() {
                        candidate.deselect();
                    }
                } else {
                    let layer = candidate
                        .svg_layers()
                        .find(|layer| layer.id == snapshot.layer_id)
                        .unwrap();
                    if layer.vector_layer {
                        return Err("Use vector selection to cut vector objects / ベクターの切り取りにはオブジェクトを選択してください / 请使用矢量选择来剪切对象".into());
                    }
                    // Rebuild raw source pixels so opacity/mask are not applied twice.
                    let mut remaining =
                        lumapaint_renderer::vector::rasterize_svg(&layer.source, width, height)?
                            .pixels;
                    for (index, pixel) in remaining.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        if selection.contains(lumapaint_core::document::Point {
                            x: (index as u32 % width) as f32 + 0.5,
                            y: (index as u32 / width) as f32 + 0.5,
                        }) {
                            pixel.fill(0);
                        }
                    }
                    let image =
                        lumapaint_renderer::vector::clipboard_png(width, height, remaining)?;
                    candidate.replace_selected_image(image_svg(width, height, &image))?;
                }
            }
            write(
                &Content::Pixels {
                    source: image_svg(width, height, &png),
                },
                Some(&png),
            )?;
        }
        if cut {
            DOCUMENT.with(|document| *document.borrow_mut() = candidate);
        }
    }
    redraw()?;
    emit_document();
    Ok(())
}
