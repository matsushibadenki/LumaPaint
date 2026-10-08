//! Canvas interactions retain only gradient commands and document-space geometry.
use super::*;
use lumapaint_core::document::Point;
use lumapaint_core::gradient::{Gradient, GradientKind, GradientMethod, GradientStop};
use std::{cell::Cell, collections::HashMap};
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
    gradient: Gradient,
    stop: Option<usize>,
    serial: u64,
}
thread_local! {
    static OPTIONS: RefCell<HashMap<String,(Gradient,String)>> = RefCell::new(HashMap::new());
    static AXES: RefCell<HashMap<String,Axis>> = RefCell::new(HashMap::new());
    static SERIAL: Cell<u64> = const { Cell::new(0) };
    static PREVIEWS: RefCell<HashMap<String,PixelPreview>> = RefCell::new(HashMap::new());
}
#[derive(Default)]
struct PixelPreview {
    running: bool,
    requested: Option<Gradient>,
    document: Option<lumapaint_core::document::Document>,
    id: u64,
    revision: u64,
    serial: u64,
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
    PREVIEWS.with(|p| {
        p.borrow_mut().remove(&current_label());
    });
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
            .direct_objects()
            .into_iter()
            .map(|(_, o)| o)
            .find(|o| ids.contains(&o.id))?;
        let gradient = if target == "stroke" {
            object.stroke_gradient.as_ref()
        } else {
            object.fill_gradient.as_ref()
        }?;
        let matrix =
            gradient.geometry_in_bounds(lumapaint_core::stroke::path_bounds(&object.path.data)?);
        let [ga, gb, _, _, ge, gf] = matrix;
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
            gradient: {
                let mut g = gradient.clone();
                g.geometry = Some(compose(object.transform, matrix));
                g
            },
            stop: None,
            serial: 0,
        };
        AXES.with(|axes| axes.borrow_mut().insert(current_label(), axis.clone()));
        Some(axis)
    })
}
fn appearance(axis: &Axis) -> Gradient {
    let mut g = axis.gradient.clone();
    if axis.handle >= 4 {
        return g;
    }
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
        PREVIEWS.with(|p| {
            p.borrow_mut().remove(&current_label());
        });
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
        let mut selected_stop = None;
        let handle = existing.as_ref().map_or(0, |a| {
            for (i, point, midpoint) in controls(a, tolerance / 6.) {
                if (p[0] - point[0]).hypot(p[1] - point[1]) < tolerance {
                    selected_stop = Some(i);
                    return if midpoint { 5 } else { 4 };
                }
            }
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
                gradient: options().0,
                stop: None,
                serial: 0,
            }
        };
        axis.serial = SERIAL.with(|s| {
            let next = s.get().checked_add(1).expect("gradient serial exhausted");
            s.set(next);
            next
        });
        axis.dragging = true;
        axis.stop = selected_stop;
        if let Some(stop) = selected_stop {
            if let Some(window) = APP
                .get()
                .and_then(|app| app.get_webview_window(&current_label()))
            {
                let _ = window.emit("lumapaint-gradient-stop", stop);
            }
        }
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
        if axis.handle >= 4 {
            let i = axis.stop.unwrap();
            let dx = axis.end[0] - axis.start[0];
            let dy = axis.end[1] - axis.start[1];
            let t = ((p[0] - axis.start[0]) * dx + (p[1] - axis.start[1]) * dy)
                / (dx * dx + dy * dy).max(0.0001);
            let lower = i
                .checked_sub(1)
                .map_or(0., |j| axis.gradient.stops[j].position);
            let upper = axis.gradient.stops.get(i + 1).map_or(1., |s| s.position);
            if axis.handle == 4 {
                axis.gradient.stops[i].position = t.clamp(lower, upper);
            } else {
                let start = axis.gradient.stops[i].position;
                axis.gradient.stops[i].midpoint =
                    ((t - start) / (upper - start).max(0.00001)).clamp(0.01, 0.99);
            }
        } else if axis.handle == 3 {
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
                crate::diagnostic_jobs::spawn_blocking(move || {
                    let expected_gradient = gradient.clone();
                    let result = render_pixel_gradient(job, gradient);
                    let _ = app.run_on_main_thread(move || {
                        let Some(_session) = window_sessions::for_canvas(token) else {
                            return;
                        };
                        if current_axis().is_none_or(|a| {
                            a.dragging
                                || a.serial != axis.serial
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
            a.gradient = appearance(a);
            a.revision = revision;
        }
    });
}
fn compose(a: [f32; 6], b: [f32; 6]) -> [f32; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
fn controls(axis: &Axis, inverse_zoom: f32) -> Vec<(usize, [f32; 2], bool)> {
    let dx = axis.end[0] - axis.start[0];
    let dy = axis.end[1] - axis.start[1];
    let length = dx.hypot(dy).max(0.001);
    let normal = [-dy / length * inverse_zoom, dx / length * inverse_zoom];
    let mut out = Vec::new();
    for (i, stop) in axis.gradient.stops.iter().enumerate() {
        out.push((
            i,
            [
                axis.start[0] + dx * stop.position + normal[0] * 14.,
                axis.start[1] + dy * stop.position + normal[1] * 14.,
            ],
            false,
        ));
        if let Some(next) = axis.gradient.stops.get(i + 1) {
            let t = stop.position + (next.position - stop.position) * stop.midpoint;
            out.push((
                i,
                [
                    axis.start[0] + dx * t - normal[0] * 10.,
                    axis.start[1] + dy * t - normal[1] * 10.,
                ],
                true,
            ));
        }
    }
    out
}
fn schedule_preview(axis: &Axis, token: u64) {
    let gradient = appearance(axis);
    let start = PREVIEWS.with(|previews| {
        let mut previews = previews.borrow_mut();
        let state = previews.entry(current_label()).or_default();
        if state.running || state.requested.as_ref() == Some(&gradient) {
            return false;
        }
        state.running = true;
        state.requested = Some(gradient.clone());
        state.serial = axis.serial;
        state.id = axis.id;
        state.revision = axis.revision;
        true
    });
    if !start {
        return;
    }
    let Ok(job) = prepare_pixel_gradient(&[]) else {
        PREVIEWS.with(|p| {
            p.borrow_mut().remove(&current_label());
        });
        return;
    };
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let id = axis.id;
    let revision = axis.revision;
    let serial = axis.serial;
    crate::diagnostic_jobs::spawn_blocking(move || {
        let selection = job.selection.clone();
        let result = render_pixel_gradient(job, gradient);
        let _ = app.run_on_main_thread(move || {
            let Some(_session) = window_sessions::for_canvas(token) else {
                return;
            };
            let valid = current_axis().is_some_and(|a| {
                a.dragging && a.id == id && a.revision == revision && a.serial == serial
            }) && DOCUMENT.with(|d| d.borrow().selection() == selection.as_ref());
            if valid {
                PREVIEWS.with(|p| {
                    if let Some(state) = p.borrow_mut().get_mut(&current_label()) {
                        if state.id == id && state.revision == revision && state.serial == serial {
                            state.running = false;
                            if let Ok(job) = result {
                                state.document = Some(job.document);
                            }
                        }
                    }
                });
                let _ = request_redraw();
            }
        });
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
    let zoom = canvas.viewport.screen_zoom();
    let half = 3. / zoom;
    canvas.renderer.set_guide_drafts(
        controls(&axis, 1. / zoom)
            .into_iter()
            .map(|(_, p, midpoint)| {
                let corners = if midpoint {
                    [
                        [p[0], p[1] - half],
                        [p[0] + half, p[1]],
                        [p[0], p[1] + half],
                        [p[0] - half, p[1]],
                    ]
                } else {
                    [
                        [p[0] - half, p[1] - half],
                        [p[0] + half, p[1] - half],
                        [p[0] + half, p[1] + half],
                        [p[0] - half, p[1] + half],
                    ]
                };
                lumapaint_renderer::FrameOverlay {
                    corners,
                    handles: false,
                    baseline: None,
                }
            })
            .collect(),
    );
    if axis.dragging
        && axis.ids.is_empty()
        && (axis.end[0] - axis.start[0]).hypot(axis.end[1] - axis.start[1]) > 0.01
    {
        schedule_preview(&axis, canvas.token);
        let result = PREVIEWS.with(|previews| {
            let previews = previews.borrow();
            previews
                .get(&current_label())
                .filter(|p| p.id == axis.id && p.revision == axis.revision)
                .and_then(|p| p.document.as_ref())
                .map(|document| canvas.renderer.render(canvas.viewport, document))
        });
        if result.is_some() {
            return result;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transformed_gradient_controls_keep_stop_and_midpoint_distinct_from_axis_handles() {
        let gradient = Gradient {
            geometry: Some([0., 100., -100., 0., 10., 20.]),
            pixel_style: None,
            kind: GradientKind::Linear,
            angle: -90.,
            aspect: 1.,
            dither: false,
            method: GradientMethod::Classic,
            stops: vec![
                GradientStop {
                    position: 0.25,
                    color: [0, 0, 0, 255],
                    midpoint: 0.3,
                },
                GradientStop {
                    position: 0.75,
                    color: [255; 4],
                    midpoint: 0.5,
                },
            ],
        };
        let axis = Axis {
            start: [10., 20.],
            end: [10., 120.],
            dragging: false,
            handle: 4,
            id: 1,
            revision: 2,
            ids: vec!["path".into()],
            grab: [0., 0.],
            gradient: gradient.clone(),
            stop: Some(0),
            serial: 1,
        };
        let controls = controls(&axis, 0.5);
        assert_eq!(controls[0], (0, [3., 45.], false));
        assert_eq!(controls[1], (0, [15., 60.], true));
        assert_eq!(controls[2], (1, [3., 95.], false));
        assert_eq!(appearance(&axis), gradient);
        assert_eq!(
            compose([2., 0., 0., 3., 5., 6.], gradient.geometry.unwrap()),
            [0., 300., -200., 0., 25., 66.]
        );
    }
}
