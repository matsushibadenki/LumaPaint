//! Per-window, uncommitted crop bounds; document mutations occur only on confirmation.
use super::*;
#[derive(Clone, Copy)]
struct Draft {
    document_id: u64,
    revision: u64,
    bounds: [f32; 4],
    anchor: [f32; 2],
    original: [f32; 4],
    handle: Option<usize>,
    moving: bool,
}
thread_local! { static DRAFTS: std::cell::RefCell<std::collections::HashMap<String,Draft>> = std::cell::RefCell::new(Default::default()); }
pub(super) fn cancel() -> bool {
    DRAFTS.with(|d| d.borrow_mut().remove(&current_label()).is_some())
}
pub(super) fn bounds() -> Option<[f32; 4]> {
    DRAFTS.with(|d| {
        d.borrow()
            .get(&current_label())
            .filter(|d| {
                d.document_id == ACTIVE_DOCUMENT_ID.with(|id| id.get())
                    && d.revision == DOCUMENT.with(|doc| doc.borrow().revision())
            })
            .map(|d| d.bounds)
    })
}
pub(super) fn pointer(
    point: lumapaint_core::document::Point,
    phase: u8,
    viewport: Viewport,
) -> Result<(), String> {
    ensure_document_open()?;
    let p = [
        point.x.clamp(0., viewport.document_width),
        point.y.clamp(0., viewport.document_height),
    ];
    DRAFTS.with(|slot| {
        let mut drafts = slot.borrow_mut();
        let key = current_label();
        if drafts.get(&key).is_some_and(|d| {
            d.document_id != ACTIVE_DOCUMENT_ID.with(|id| id.get())
                || d.revision != DOCUMENT.with(|doc| doc.borrow().revision())
        }) {
            drafts.remove(&key);
        }
        if phase == 0 {
            let original = bounds_for(drafts.get(&key), viewport);
            let corners = [
                [original[0], original[1]],
                [original[0] + original[2], original[1]],
                [original[0] + original[2], original[1] + original[3]],
                [original[0], original[1] + original[3]],
                [original[0] + original[2] * 0.5, original[1]],
                [original[0] + original[2], original[1] + original[3] * 0.5],
                [original[0] + original[2] * 0.5, original[1] + original[3]],
                [original[0], original[1] + original[3] * 0.5],
            ];
            let tolerance = 8. / viewport.screen_zoom();
            let handle = corners
                .iter()
                .position(|c| (c[0] - p[0]).hypot(c[1] - p[1]) <= tolerance);
            let moving = handle.is_none()
                && drafts.contains_key(&key)
                && p[0] > original[0]
                && p[1] > original[1]
                && p[0] < original[0] + original[2]
                && p[1] < original[1] + original[3];
            drafts.insert(
                key,
                Draft {
                    document_id: ACTIVE_DOCUMENT_ID.with(|id| id.get()),
                    revision: DOCUMENT.with(|doc| doc.borrow().revision()),
                    bounds: if moving || handle.is_some() {
                        original
                    } else {
                        [p[0], p[1], 0., 0.]
                    },
                    anchor: p,
                    original,
                    handle,
                    moving,
                },
            );
        } else if let Some(d) = drafts.get_mut(&key) {
            update_bounds(d, p, viewport);
        }
    });
    request_redraw()
}
fn update_bounds(d: &mut Draft, p: [f32; 2], viewport: Viewport) {
    if d.moving {
        d.bounds[2] = d.original[2];
        d.bounds[3] = d.original[3];
        d.bounds[0] =
            (d.original[0] + p[0] - d.anchor[0]).clamp(0., viewport.document_width - d.original[2]);
        d.bounds[1] = (d.original[1] + p[1] - d.anchor[1])
            .clamp(0., viewport.document_height - d.original[3]);
    } else if let Some(h @ 4..=7) = d.handle {
        let o = d.original;
        let (left, top, right, bottom) = match h {
            4 => (o[0], p[1], o[0] + o[2], o[1] + o[3]),
            5 => (o[0], o[1], p[0], o[1] + o[3]),
            6 => (o[0], o[1], o[0] + o[2], p[1]),
            _ => (p[0], o[1], o[0] + o[2], o[1] + o[3]),
        };
        d.bounds = [
            left.min(right),
            top.min(bottom),
            (right - left).abs(),
            (bottom - top).abs(),
        ];
    } else {
        let a = if let Some(h) = d.handle {
            let o = d.original;
            [
                [o[0] + o[2], o[1] + o[3]],
                [o[0], o[1] + o[3]],
                [o[0], o[1]],
                [o[0] + o[2], o[1]],
            ][h]
        } else {
            d.anchor
        };
        d.bounds = [
            a[0].min(p[0]),
            a[1].min(p[1]),
            (p[0] - a[0]).abs(),
            (p[1] - a[1]).abs(),
        ];
    }
}
fn bounds_for(d: Option<&Draft>, v: Viewport) -> [f32; 4] {
    d.map_or([0., 0., v.document_width, v.document_height], |d| d.bounds)
}
pub(super) fn commit() -> Result<(), String> {
    let bounds =
        bounds().ok_or("Drag a crop area / 切り抜き範囲をドラッグしてください / 请拖动裁剪区域")?;
    ensure_document_open()?;
    DOCUMENT.with(|doc| doc.borrow_mut().crop_canvas(bounds))?;
    cancel();
    PAN.with(|pan| pan.set((0., 0.)));
    CANVAS.with(|slot| {
        if let Some(canvas) = slot.borrow_mut().as_mut() {
            canvas.viewport.pan_x = 0.;
            canvas.viewport.pan_y = 0.;
        }
    });
    redraw()?;
    emit_document();
    Ok(())
}
pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    if TOOL.with(|tool| tool.get()) != CanvasTool::Crop {
        return None;
    }
    let b = bounds()?;
    canvas
        .renderer
        .set_frame_overlay(Some(lumapaint_renderer::FrameOverlay {
            corners: [
                [b[0], b[1]],
                [b[0] + b[2], b[1]],
                [b[0] + b[2], b[1] + b[3]],
                [b[0], b[1] + b[3]],
            ],
            handles: true,
            baseline: None,
        }));
    Some(DOCUMENT.with(|doc| canvas.renderer.render(canvas.viewport, &doc.borrow())))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crop_drag_corner_resize_and_move_are_bounded() {
        let viewport = Viewport::new(400., 300., 1., 1., false)
            .unwrap()
            .with_document(150, 120, CanvasColor::White)
            .unwrap();
        let mut d = Draft {
            document_id: 0,
            revision: 0,
            bounds: [20., 30., 100., 80.],
            anchor: [50., 50.],
            original: [20., 30., 100., 80.],
            handle: Some(0),
            moving: false,
        };
        update_bounds(&mut d, [10., 20.], viewport);
        assert_eq!(d.bounds, [10., 20., 110., 90.]);
        d.handle = Some(5);
        update_bounds(&mut d, [140., 40.], viewport);
        assert_eq!(d.bounds, [20., 30., 120., 80.]);
        d.handle = None;
        d.moving = true;
        update_bounds(&mut d, [-100., -100.], viewport);
        assert_eq!(d.bounds[..2], [0., 0.]);
        update_bounds(&mut d, [500., 500.], viewport);
        assert_eq!(d.bounds[..2], [50., 40.]);
        d.moving = false;
        d.anchor = [100., 100.];
        update_bounds(&mut d, [20., 30.], viewport);
        assert_eq!(d.bounds, [20., 30., 80., 70.]);
    }
}
