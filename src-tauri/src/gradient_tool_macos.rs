//! Canvas interactions retain only gradient commands and document-space geometry.
use super::*;
use lumapaint_core::document::Point;
use lumapaint_core::gradient::{Gradient, GradientKind, GradientMethod, GradientStop};
use std::collections::HashMap;
#[derive(Clone)]
struct Axis {
    start: [f32; 2],
    end: [f32; 2],
    dragging: bool,
    handle: u8,
    id: u64,
    revision: u64,
    ids: Vec<String>,
    grab: [f32; 2],
}
thread_local! {
    static OPTIONS: RefCell<HashMap<String,(Gradient,String)>> = RefCell::new(HashMap::new());
    static AXES: RefCell<HashMap<String,Axis>> = RefCell::new(HashMap::new());
}
pub(super) fn options() -> (Gradient, String) {
    OPTIONS
        .with(|o| o.borrow().get(&current_label()).cloned())
        .unwrap_or_else(|| {
            (
                Gradient {
                    geometry: None,
                    pixel_style: None,
                    kind: GradientKind::Linear,
                    angle: 0.,
                    aspect: 1.,
                    dither: false,
                    method: GradientMethod::Classic,
                    stops: vec![
                        GradientStop {
                            position: 0.,
                            color: [0, 0, 0, 255],
                            midpoint: 0.5,
                        },
                        GradientStop {
                            position: 1.,
                            color: [255; 4],
                            midpoint: 0.5,
                        },
                    ],
                },
                "fill".into(),
            )
        })
}
pub(super) fn configure(mut gradient: Gradient, target: &str) {
    gradient.geometry = None;
    OPTIONS.with(|o| {
        o.borrow_mut()
            .insert(current_label(), (gradient, target.into()))
    });
}
pub(super) fn cancel() -> bool {
    AXES.with(|axes| {
        let mut axes = axes.borrow_mut();
        if axes.get(&current_label()).is_some_and(|a| a.dragging) {
            axes.remove(&current_label());
            true
        } else {
            false
        }
    })
}
fn current_axis() -> Option<Axis> {
    let previous = AXES.with(|a| a.borrow().get(&current_label()).cloned());
    DOCUMENT.with(|d| {
        let d = d.borrow();
        let id = ACTIVE_DOCUMENT_ID.with(|i| i.get());
        let ids = d.selected_vector_ids().to_vec();
        if let Some(axis) =
            previous.filter(|a| a.id == id && a.revision == d.revision() && a.ids == ids)
        {
            return Some(axis);
        }
        let (_, target) = options();
        let object = d
            .svg_layers()
            .flat_map(|l| l.vector_objects.iter())
            .find(|o| ids.contains(&o.id))?;
        let gradient = if target == "stroke" {
            object.stroke_gradient.as_ref()
        } else {
            object.fill_gradient.as_ref()
        }?;
        let [ga, gb, _, _, ge, gf] = gradient.geometry?;
        let [a, b, c, e, tx, ty] = object.transform;
        let start = [a * ge + c * gf + tx, b * ge + e * gf + ty];
        let end = [start[0] + a * ga + c * gb, start[1] + b * ga + e * gb];
        let axis = Axis {
            start,
            end,
            dragging: false,
            handle: 2,
            id,
            revision: d.revision(),
            ids,
            grab: start,
        };
        AXES.with(|axes| axes.borrow_mut().insert(current_label(), axis.clone()));
        Some(axis)
    })
}
fn appearance(axis: &Axis) -> Gradient {
    let (mut g, _) = options();
    let dx = axis.end[0] - axis.start[0];
    let dy = axis.end[1] - axis.start[1];
    g.angle = -dy.atan2(dx).to_degrees();
    g.geometry = Some([
        dx,
        dy,
        -dy * g.aspect,
        dx * g.aspect,
        axis.start[0],
        axis.start[1],
    ]);
    g
}
pub(super) fn pointer(
    point: Point,
    phase: u8,
    modifiers: NSEventModifierFlags,
) -> Result<(), String> {
    let p = [point.x, point.y];
    if phase == 0 {
        text_editor::finish(true)?;
        let (id, revision, ids) = DOCUMENT.with(|d| {
            let d = d.borrow();
            (
                ACTIVE_DOCUMENT_ID.with(|i| i.get()),
                d.revision(),
                d.selected_vector_ids().to_vec(),
            )
        });
        if ids.is_empty() {
            prepare_pixel_gradient(&[])?;
        }
        let tolerance = CANVAS.with(|c| {
            c.borrow()
                .as_ref()
                .map_or(6., |c| 6. / c.viewport.screen_zoom())
        });
        let existing = current_axis().filter(|a| !a.ids.is_empty());
        let handle = existing.as_ref().map_or(0, |a| {
            if (p[0] - a.start[0]).hypot(p[1] - a.start[1]) < tolerance {
                1
            } else if (p[0] - a.end[0]).hypot(p[1] - a.end[1]) < tolerance {
                2
            } else if (p[0] - (a.start[0] + a.end[0]) * 0.5)
                .hypot(p[1] - (a.start[1] + a.end[1]) * 0.5)
                < tolerance
            {
                3
            } else {
                0
            }
        });
        let mut axis = if handle > 0 {
            existing.unwrap()
        } else {
            Axis {
                start: p,
                end: p,
                dragging: true,
                handle: 2,
                id,
                revision,
                ids,
                grab: p,
            }
        };
        axis.dragging = true;
        axis.grab = p;
        axis.handle = if handle == 0 { 2 } else { handle };
        AXES.with(|a| a.borrow_mut().insert(current_label(), axis));
    } else {
        let Some(mut axis) = current_axis().filter(|a| a.dragging) else {
            return Ok(());
        };
        let anchor = if axis.handle == 1 {
            axis.end
        } else {
            axis.start
        };
        let mut p = p;
        if modifiers.contains(NSEventModifierFlags::Shift) {
            let dx = p[0] - anchor[0];
            let dy = p[1] - anchor[1];
            let length = dx.hypot(dy);
            let angle =
                (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
            p = [
                anchor[0] + length * angle.cos(),
                anchor[1] + length * angle.sin(),
            ];
        }
        if axis.handle == 3 {
            let dx = p[0] - axis.grab[0];
            let dy = p[1] - axis.grab[1];
            axis.start = [axis.start[0] + dx, axis.start[1] + dy];
            axis.end = [axis.end[0] + dx, axis.end[1] + dy];
            axis.grab = p;
        } else if axis.handle == 1 {
            axis.start = p;
        } else {
            axis.end = p;
        }
        axis.dragging = phase != 2;
        AXES.with(|a| a.borrow_mut().insert(current_label(), axis.clone()));
        if phase == 2 && (axis.end[0] - axis.start[0]).hypot(axis.end[1] - axis.start[1]) > 0.01 {
            let gradient = appearance(&axis);
            let (_, target) = options();
            if !axis.ids.is_empty() {
                DOCUMENT.with(|d| {
                    d.borrow_mut()
                        .set_dragged_vector_gradient(&axis.ids, &target, gradient)
                })?;
                remember_revision();
                emit_document();
            } else {
                let job = prepare_pixel_gradient(&[])?;
                let token = CANVAS
                    .with(|c| c.borrow().as_ref().map(|c| c.token))
                    .ok_or("Missing canvas")?;
                let app = APP.get().ok_or("Missing application")?.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let expected_gradient = gradient.clone();
                    let result = render_pixel_gradient(job, gradient);
                    let _ = app.run_on_main_thread(move || {
                        let Some(_session) = window_sessions::for_canvas(token) else {
                            return;
                        };
                        if current_axis().is_none_or(|a| {
                            a.dragging
                                || a.start != axis.start
                                || a.end != axis.end
                                || appearance(&a) != expected_gradient
                        }) {
                            return;
                        }
                        match result.and_then(commit_pixel_gradient) {
                            Ok(_) => remember_revision(),
                            Err(e) => emit_error(e),
                        }
                    });
                });
            }
        }
    }
    request_redraw()
}
fn remember_revision() {
    let revision = DOCUMENT.with(|d| d.borrow().revision());
    AXES.with(|a| {
        if let Some(a) = a.borrow_mut().get_mut(&current_label()) {
            a.revision = revision;
        }
    });
}
pub(super) fn render(canvas: &mut Canvas) -> Option<Result<(), String>> {
    if TOOL.with(|t| t.get()) != CanvasTool::Gradient {
        return None;
    }
    let axis = current_axis()?;
    canvas
        .renderer
        .set_frame_overlay(Some(lumapaint_renderer::FrameOverlay {
            corners: [axis.start, axis.end, axis.end, axis.start],
            handles: !axis.ids.is_empty() || axis.dragging,
            baseline: None,
        }));
    if axis.dragging
        && !axis.ids.is_empty()
        && (axis.end[0] - axis.start[0]).hypot(axis.end[1] - axis.start[1]) > 0.01
    {
        let mut preview = DOCUMENT.with(|d| d.borrow().clone());
        let (_, target) = options();
        if let Err(e) = preview.set_dragged_vector_gradient(&axis.ids, &target, appearance(&axis)) {
            return Some(Err(e));
        }
        return Some(canvas.renderer.render(canvas.viewport, &preview));
    }
    Some(DOCUMENT.with(|d| canvas.renderer.render(canvas.viewport, &d.borrow())))
}
