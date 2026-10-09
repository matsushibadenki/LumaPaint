//! Thumbnail focus routes native editing without passing mask data through JS.
use super::*;
use lumapaint_core::{
    document::Point,
    layer_effects::LayerEffects,
    layer_mask::LayerEditTarget,
    selection::{Selection, SelectionShape},
};

const TARGET_ERROR: &str = "Select a content or mask thumbnail / 本体またはマスクのサムネイルを選択してください / 请选择内容或蒙版缩略图";
const TOOL_ERROR: &str = "Use Brush, Eraser, Rectangle, Ellipse or Move for masks / マスクにはブラシ・消しゴム・図形・移動ツールを使用してください / 请使用画笔、橡皮擦、形状或移动工具编辑蒙版";

pub(super) struct MaskGesture {
    document_id: u64,
    revision: u64,
    layer: String,
    effects: LayerEffects,
    clip: Option<Selection>,
    points: Vec<Point>,
    brush: Brush,
    size: (u32, u32),
    tool: CanvasTool,
    radius: f32,
    reveal: bool,
    root_source: Option<String>,
}
pub(super) fn target() -> LayerEditTarget {
    ACTIVE_TILED_DOCUMENT
        .with(|d| {
            d.borrow()
                .as_ref()
                .map(|s| s.document.layer_edit_target(s.selected_layer_id()))
        })
        .unwrap_or_else(|| DOCUMENT.with(|d| d.borrow().layer_edit_target()))
}
fn snapshot() -> DocumentSnapshot {
    ACTIVE_TILED_DOCUMENT
        .with(|d| d.borrow().as_ref().map(TiledSession::snapshot))
        .unwrap_or_else(|| DOCUMENT.with(|d| d.borrow().snapshot()))
}
fn effects(id: &str) -> LayerEffects {
    ACTIVE_TILED_DOCUMENT
        .with(|d| {
            d.borrow().as_ref().and_then(|s| {
                s.document
                    .layers()
                    .iter()
                    .find(|l| l.id == id)
                    .map(|l| l.effects.clone())
            })
        })
        .unwrap_or_else(|| DOCUMENT.with(|d| d.borrow().layer_effects(id)))
}
fn editable(s: &DocumentSnapshot) -> Result<(), String> {
    let layer = s
        .layers
        .iter()
        .find(|l| l.id == s.layer_id)
        .ok_or(TARGET_ERROR)?;
    if layer.locked || !layer.visible {
        return Err("Unlock and show the layer / レイヤーのロックを解除し表示してください / 请解锁并显示图层".into());
    }
    Ok(())
}
fn moving(tool: CanvasTool) -> bool {
    matches!(
        tool,
        CanvasTool::VectorSelect
            | CanvasTool::VectorDirectSelect
            | CanvasTool::VectorScale
            | CanvasTool::VectorRotate
    )
}
fn root_source(id: &str, effects: &LayerEffects) -> Result<Option<String>, String> {
    if id != "layer-1"
        || !effects.mask.as_ref().is_some_and(|m| m.linked)
        || ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some())
    {
        return Ok(None);
    }
    DOCUMENT.with(|d| {
        let mut d = d.borrow_mut();
        d.finish();
        if d.committed_paint_strokes().next().is_none() {
            return Ok(None);
        }
        let (w, h, pixels) = clipboard::raw_selected_pixels(&d)?;
        let png = lumapaint_renderer::vector::document_png(w, h, pixels)?;
        Ok(Some(clipboard::image_svg(w, h, &png)))
    })
}
impl MaskGesture {
    fn valid(&self) -> bool {
        if self.document_id != ACTIVE_DOCUMENT_ID.with(|id| id.get()) {
            return false;
        }
        ACTIVE_TILED_DOCUMENT
            .with(|d| {
                d.borrow().as_ref().map(|s| {
                    s.selected_layer_id() == self.layer
                        && s.document.revision() == self.revision
                        && s.document.layer_edit_target(&self.layer) == LayerEditTarget::Mask
                })
            })
            .unwrap_or_else(|| {
                DOCUMENT.with(|d| {
                    let d = d.borrow();
                    d.selected_layer_id() == self.layer
                        && d.revision() == self.revision
                        && d.layer_edit_target() == LayerEditTarget::Mask
                })
            })
    }
    fn matrix(&self) -> [f32; 6] {
        let a = self.points[0];
        let b = *self.points.last().unwrap();
        let q = self
            .effects
            .mask
            .as_ref()
            .and_then(|m| m.bounds_quad())
            .unwrap_or([[0., 0.]; 4]);
        let cx = (q[0][0] + q[2][0]) * 0.5;
        let cy = (q[0][1] + q[2][1]) * 0.5;
        let [aa, bb, cc, dd] = match self.tool {
            CanvasTool::VectorScale => {
                let scale = ((b.x - cx).hypot(b.y - cy) / (a.x - cx).hypot(a.y - cy).max(1.))
                    .clamp(0.01, 100.);
                [scale, 0., 0., scale]
            }
            CanvasTool::VectorRotate => {
                let angle = (b.y - cy).atan2(b.x - cx) - (a.y - cy).atan2(a.x - cx);
                let (sn, cs) = angle.sin_cos();
                [cs, sn, -sn, cs]
            }
            _ => return [1., 0., 0., 1., b.x - a.x, b.y - a.y],
        };
        [
            aa,
            bb,
            cc,
            dd,
            cx - aa * cx - cc * cy,
            cy - bb * cx - dd * cy,
        ]
    }
    fn painted(&self) -> Result<LayerEffects, String> {
        let mut e = self.effects.clone();
        if matches!(self.tool, CanvasTool::Brush | CanvasTool::Eraser)
            && e.mask
                .as_ref()
                .is_some_and(|m| m.kind == lumapaint_core::layer_mask::MaskKind::Pixel)
        {
            let mut brush = self.brush;
            let value = if self.tool == CanvasTool::Eraser {
                0
            } else {
                (0.2126 * brush.color[0] as f32
                    + 0.7152 * brush.color[1] as f32
                    + 0.0722 * brush.color[2] as f32)
                    .round() as u8
            };
            brush.color = [value; 3];
            brush.no_color = false;
            if self.tool == CanvasTool::Eraser {
                brush.blend_mode = Default::default();
            }
            let stroke = lumapaint_core::document::Stroke {
                brush,
                points: self.points.clone(),
                pressures: Vec::new(),
                selection: self.clip.clone(),
                eraser: self.tool == CanvasTool::Eraser,
                clear: false,
            };
            e.mask.as_mut().unwrap().paint_brush_dabs(
                self.size,
                &lumapaint_renderer::sampled_raster_dabs(&stroke)?,
                brush,
                self.clip.as_ref(),
            )?;
            return Ok(e);
        }
        let a = self.points[0];
        let b = *self.points.last().unwrap();
        let mut selection = if matches!(
            self.tool,
            CanvasTool::VectorRectangle | CanvasTool::VectorEllipse
        ) {
            Selection::new(
                if self.tool == CanvasTool::VectorEllipse {
                    SelectionShape::Ellipse
                } else {
                    SelectionShape::Rectangle
                },
                [
                    a.x.min(b.x),
                    a.y.min(b.y),
                    (a.x - b.x).abs().max(1.),
                    (a.y - b.y).abs().max(1.),
                ],
            )
        } else {
            let minx = self
                .points
                .iter()
                .map(|p| p.x)
                .fold(f32::INFINITY, f32::min);
            let miny = self
                .points
                .iter()
                .map(|p| p.y)
                .fold(f32::INFINITY, f32::min);
            let maxx = self
                .points
                .iter()
                .map(|p| p.x)
                .fold(f32::NEG_INFINITY, f32::max);
            let maxy = self
                .points
                .iter()
                .map(|p| p.y)
                .fold(f32::NEG_INFINITY, f32::max);
            Selection::new(
                SelectionShape::Stroke,
                [minx, miny, (maxx - minx).max(1.), (maxy - miny).max(1.)],
            )
        };
        if selection.regions[0].shape == SelectionShape::Stroke {
            selection.regions[0].points = self.points.clone();
            selection.regions[0].radius = self.radius;
        }
        e.mask
            .as_mut()
            .ok_or("Layer mask not found")?
            .paint_selection(&selection, self.clip.as_ref(), self.reveal)?;
        Ok(e)
    }
}
fn set_effects(id: &str, effects: LayerEffects) -> Result<(), String> {
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        ACTIVE_TILED_DOCUMENT.with(|d| {
            d.borrow_mut()
                .as_mut()
                .ok_or("Missing tiled document")?
                .document
                .set_layer_effects(id, effects)
                .map(|_| ())
        })
    } else {
        DOCUMENT.with(|d| d.borrow_mut().set_layer_effects(id, effects))
    }
}
fn transform(id: &str, matrix: [f32; 6], root: Option<String>) -> Result<(), String> {
    if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
        let mut e = effects(id);
        let mask = e.mask.as_mut().ok_or("Layer mask not found")?;
        if mask.linked {
            return Err("Unlink the mask to move it on tiled documents / タイル形式ではリンクを解除してマスクを移動してください / 平铺文档请先取消链接再移动蒙版".into());
        }
        mask.transform_by(matrix)?;
        set_effects(id, e)
    } else {
        DOCUMENT.with(|d| d.borrow_mut().transform_layer_mask(id, matrix, root))
    }
}
/// True means this event has been consumed, including explicit no-target focus.
pub(super) fn pointer(point: Point, phase: u8) -> Result<bool, String> {
    let tool = TOOL.with(|t| t.get());
    if matches!(
        tool,
        CanvasTool::Hand
            | CanvasTool::ZoomIn
            | CanvasTool::ZoomOut
            | CanvasTool::Eyedropper
            | CanvasTool::Crop
    ) {
        return Ok(false);
    }
    match target() {
        LayerEditTarget::Content => return Ok(false),
        LayerEditTarget::None => return Ok(true),
        LayerEditTarget::Mask => {}
    }
    // Marquees/lassos still select document coverage; subsequent strokes honor it.
    if matches!(
        tool,
        CanvasTool::Rectangle
            | CanvasTool::Ellipse
            | CanvasTool::Lasso
            | CanvasTool::PolygonLasso
            | CanvasTool::MagneticLasso
            | CanvasTool::SelectionBrush
    ) {
        return Ok(false);
    }
    if phase == 3 {
        return Ok(true);
    }
    if !moving(tool)
        && !matches!(
            tool,
            CanvasTool::Brush
                | CanvasTool::Eraser
                | CanvasTool::VectorPencil
                | CanvasTool::VectorRectangle
                | CanvasTool::VectorEllipse
        )
    {
        return if phase == 0 {
            Err(TOOL_ERROR.into())
        } else {
            Ok(true)
        };
    }
    if phase == 0 {
        cancel_vector_drag();
        DOCUMENT.with(|d| d.borrow_mut().finish());
        let s = snapshot();
        editable(&s)?;
        let e = effects(&s.layer_id);
        let brush = BRUSH.with(|b| *b.borrow());
        if !moving(tool) && brush.no_color && tool != CanvasTool::Eraser {
            return Ok(true);
        }
        let root = if moving(tool) {
            root_source(&s.layer_id, &e)?
        } else {
            None
        };
        let luma = 0.2126 * f32::from(brush.color[0])
            + 0.7152 * f32::from(brush.color[1])
            + 0.0722 * f32::from(brush.color[2]);
        if matches!(
            tool,
            CanvasTool::Brush | CanvasTool::Eraser | CanvasTool::VectorPencil
        ) && CANVAS.with(|c| c.borrow().is_some())
        {
            begin_precise_paint_input();
        }
        MASK_GESTURE.with(|d| {
            *d.borrow_mut() = Some(MaskGesture {
                document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                revision: s.revision,
                layer: s.layer_id,
                effects: e,
                clip: Some(s.selection.unwrap_or_else(|| {
                    Selection::new(
                        SelectionShape::Rectangle,
                        [0., 0., s.width as f32, s.height as f32],
                    )
                })),
                points: vec![point],
                brush,
                size: (s.width, s.height),
                tool,
                radius: (brush.size * 0.5).clamp(0.5, 512.),
                reveal: tool != CanvasTool::Eraser && luma >= 128.,
                root_source: root,
            })
        });
    } else {
        MASK_GESTURE.with(|slot| -> Result<(),String> {
            let mut slot=slot.borrow_mut();let Some(d)=slot.as_mut() else{return Ok(());};
            if !d.valid(){*slot=None;return Err("Mask edit target changed / マスクの編集先が変更されました / 蒙版编辑目标已更改".into());}
            if moving(d.tool) || matches!(d.tool,CanvasTool::VectorRectangle|CanvasTool::VectorEllipse) {d.points.truncate(1);d.points.push(point);}
            else if d.points.last().is_none_or(|p|(p.x-point.x).hypot(p.y-point.y)>=0.25) {
                if d.points.len()>=32768{*slot=None;return Err("Mask stroke is too long / マスクの線が長すぎます / 蒙版笔画过长".into());}
                d.points.push(point);
            }
            Ok(())
        })?;
    }
    if phase == 2 {
        if let Some(d) = MASK_GESTURE.with(|s| s.borrow_mut().take()) {
            if moving(d.tool) {
                transform(&d.layer, d.matrix(), d.root_source)?;
            } else {
                set_effects(&d.layer, d.painted()?)?;
            }
        }
        emit_document();
    }
    Ok(true)
}
pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    if target() != LayerEditTarget::Mask {
        return None;
    }
    let moving_tool = moving(TOOL.with(|t| t.get()));
    let overlay = |e: &LayerEffects| {
        e.mask
            .as_ref()
            .and_then(|m| m.bounds_quad())
            .filter(|_| moving_tool)
            .map(|corners| lumapaint_renderer::FrameOverlay {
                corners,
                handles: false,
                baseline: None,
            })
    };
    MASK_GESTURE.with(|slot| {
        let slot = slot.borrow();
        let draft = slot.as_ref().filter(|d| d.valid());
        if ACTIVE_TILED_DOCUMENT.with(|d| d.borrow().is_some()) {
            // The regular tiled renderer installs committed coverage. During a
            // gesture install a Rust-owned preview, then invalidate its cache key.
            let d = draft?;
            return Some((|| {
                let mut preview =
                    ACTIVE_TILED_DOCUMENT.with(|s| s.borrow().as_ref().unwrap().document.clone());
                let mut e = if moving(d.tool) {
                    d.effects.clone()
                } else {
                    d.painted()?
                };
                if moving(d.tool) {
                    let mask = e.mask.as_mut().unwrap();
                    if mask.linked {
                        return Ok(());
                    }
                    mask.transform_by(d.matrix())?;
                }
                preview.set_layer_effects(&d.layer, e.clone())?;
                canvas.renderer.install_tiled_preview(&preview)?;
                canvas.active_tiled_key = None;
                canvas.renderer.set_frame_overlay(overlay(&e));
                DOCUMENT.with(|doc| canvas.renderer.render(canvas.viewport, &doc.borrow()))
            })());
        }
        Some((|| {
            let mut preview = DOCUMENT.with(|d| d.borrow().clone_for_rendering());
            if let Some(d) = draft {
                if moving(d.tool) {
                    preview.transform_layer_mask(&d.layer, d.matrix(), d.root_source.clone())?;
                } else {
                    preview.set_layer_effects(&d.layer, d.painted()?)?;
                }
            }
            canvas
                .renderer
                .set_frame_overlay(overlay(&preview.layer_effects(preview.selected_layer_id())));
            canvas.renderer.render(canvas.viewport, &preview)
        })())
    })
}
pub(super) fn key(event: &NSEvent) -> bool {
    let target = target();
    if target == LayerEditTarget::Content {
        return false;
    }
    let key = event.keyCode();
    if key == 53 {
        MASK_GESTURE.with(|s| s.borrow_mut().take());
        let _ = redraw();
        return true;
    }
    if ![123, 124, 125, 126, 51, 117].contains(&key)
        || event.modifierFlags().intersects(
            NSEventModifierFlags::Command
                | NSEventModifierFlags::Control
                | NSEventModifierFlags::Option,
        )
    {
        return false;
    }
    if target == LayerEditTarget::None {
        return true;
    }
    let result = (|| {
        if [51, 117].contains(&key) {
            return clear();
        }
        cancel_vector_drag();
        let s = snapshot();
        editable(&s)?;
        let e = effects(&s.layer_id);
        let root = root_source(&s.layer_id, &e)?;
        let n = if event.modifierFlags().contains(NSEventModifierFlags::Shift) {
            10.
        } else {
            1.
        };
        let (dx, dy) = match key {
            123 => (-n, 0.),
            124 => (n, 0.),
            125 => (0., n),
            _ => (0., -n),
        };
        transform(&s.layer_id, [1., 0., 0., 1., dx, dy], root)?;
        redraw()?;
        emit_document();
        Ok(())
    })();
    if let Err(e) = result {
        emit_error(e);
    }
    true
}
fn clear() -> Result<(), String> {
    cancel_vector_drag();
    let s = snapshot();
    editable(&s)?;
    let mut e = effects(&s.layer_id);
    let selection = s.selection.unwrap_or_else(|| {
        Selection::new(
            SelectionShape::Rectangle,
            [0., 0., s.width as f32, s.height as f32],
        )
    });
    e.mask
        .as_mut()
        .ok_or("Layer mask not found")?
        .paint_selection(&selection, None, false)?;
    set_effects(&s.layer_id, e)?;
    redraw()?;
    emit_document();
    Ok(())
}
pub(super) fn edit(action: DocumentAction) -> Option<Result<DocumentSnapshot, String>> {
    let target = target();
    if target == LayerEditTarget::Content {
        return None;
    }
    match action {
        DocumentAction::ClearLayer | DocumentAction::DeleteSelectedObjects => {
            Some(if target == LayerEditTarget::None {
                Err(TARGET_ERROR.into())
            } else {
                clear().map(|_| snapshot())
            })
        }
        DocumentAction::Copy
        | DocumentAction::Cut
        | DocumentAction::Paste
        | DocumentAction::RegistrationFill
        | DocumentAction::RegistrationStroke
        | DocumentAction::CreateTrimMarks => Some(Err(if target == LayerEditTarget::None {
            TARGET_ERROR
        } else {
            TOOL_ERROR
        }
        .into())),
        _ => None,
    }
}

pub(super) fn transform_action(
    action: &str,
    values: [f32; 4],
) -> Option<Result<DocumentSnapshot, String>> {
    match target() {
        LayerEditTarget::Content => return None,
        LayerEditTarget::None => return Some(Err(TARGET_ERROR.into())),
        LayerEditTarget::Mask => {}
    }
    Some((|| {
        cancel_vector_drag();
        let s = snapshot();
        editable(&s)?;
        let e = effects(&s.layer_id);
        if values.iter().any(|v| !v.is_finite() || v.abs() > 100_000.) {
            return Err("Invalid transform values".into());
        }
        let q = e
            .mask
            .as_ref()
            .and_then(|m| m.bounds_quad())
            .ok_or("Layer mask not found")?;
        let cx = (q[0][0] + q[2][0]) * 0.5;
        let cy = (q[0][1] + q[2][1]) * 0.5;
        let [a, b, c, d] = match action {
            "move" => [1., 0., 0., 1.],
            "scale" if (0.01..=100.).contains(&values[0]) && (0.01..=100.).contains(&values[1]) => {
                [values[0], 0., 0., values[1]]
            }
            "rotate" => {
                let (sn, cs) = values[0].to_radians().sin_cos();
                [cs, sn, -sn, cs]
            }
            "reflect" => {
                let (sn, cs) = (2. * values[0].to_radians()).sin_cos();
                [cs, sn, sn, -cs]
            }
            "shear" if values[0].abs() < 89. => [1., 0., values[0].to_radians().tan(), 1.],
            _ => return Err(TOOL_ERROR.into()),
        };
        let matrix = if action == "move" {
            [a, b, c, d, values[0], values[1]]
        } else {
            [a, b, c, d, cx - a * cx - c * cy, cy - b * cx - d * cy]
        };
        transform(&s.layer_id, matrix, root_source(&s.layer_id, &e)?)?;
        redraw()?;
        emit_document();
        Ok(snapshot())
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::layer_mask::{LayerMask, MaskKind};
    #[test]
    fn thumbnail_target_routes_mask_gestures_and_blocks_deselected_content() {
        for vector in [false, true] {
            for kind in [MaskKind::Pixel, MaskKind::Vector] {
                ACTIVE_TILED_DOCUMENT.with(|s| *s.borrow_mut() = None);
                let mut d = Document::default();
                let id = if vector {
                    d.add_vector_layer().unwrap()
                } else {
                    d.add_paint_layer().unwrap()
                };
                let mask = LayerMask::from_selection(kind, None, 64, 64).unwrap();
                d.set_layer_effects(
                    &id,
                    LayerEffects {
                        mask: Some(mask),
                        ..Default::default()
                    },
                )
                .unwrap();
                d.select_layer_target(id.clone(), LayerEditTarget::Mask)
                    .unwrap();
                let artwork = d.svg_layers().find(|l| l.id == id).unwrap().clone();
                DOCUMENT.with(|doc| *doc.borrow_mut() = d);
                TOOL.with(|t| t.set(CanvasTool::Brush));
                BRUSH.with(|b| {
                    b.borrow_mut().color = [0, 0, 0];
                    b.borrow_mut().size = 8.;
                });
                assert!(pointer(Point { x: 8., y: 8. }, 0).unwrap());
                assert!(pointer(Point { x: 24., y: 8. }, 1).unwrap());
                // Preview is transient; the original mask is unchanged until release.
                assert_eq!(effects(&id).mask.unwrap().coverage([16.5, 8.5]), 1.);
                assert!(pointer(Point { x: 24., y: 8. }, 2).unwrap());
                DOCUMENT.with(|doc| {
                    let mut d = doc.borrow_mut();
                    assert_eq!(d.layer_effects(&id).mask.unwrap().coverage([16.5, 8.5]), 0.);
                    assert_eq!(
                        d.svg_layers().find(|l| l.id == id).unwrap().source,
                        artwork.source
                    );
                    d.undo();
                    assert_eq!(d.layer_effects(&id).mask.unwrap().coverage([16.5, 8.5]), 1.);
                    d.redo();
                    assert_eq!(d.layer_effects(&id).mask.unwrap().coverage([16.5, 8.5]), 0.);
                    d.select_layer_target(id.clone(), LayerEditTarget::None)
                        .unwrap();
                });
                let before = snapshot().revision;
                assert!(pointer(Point { x: 32., y: 32. }, 0).unwrap());
                assert!(pointer(Point { x: 32., y: 32. }, 2).unwrap());
                assert_eq!(snapshot().revision, before);
                DOCUMENT.with(|doc| {
                    doc.borrow_mut()
                        .select_layer_target(id.clone(), LayerEditTarget::Content)
                        .unwrap()
                });
                assert!(!pointer(Point { x: 32., y: 32. }, 0).unwrap());
                DOCUMENT.with(|doc| {
                    doc.borrow_mut()
                        .select_layer_target(id.clone(), LayerEditTarget::Mask)
                        .unwrap()
                });
                TOOL.with(|t| t.set(CanvasTool::Text));
                assert!(pointer(Point { x: 32., y: 32. }, 0).is_err());
                assert_eq!(snapshot().revision, before);
            }
        }
        MASK_GESTURE.with(|s| s.borrow_mut().take());
    }
}
