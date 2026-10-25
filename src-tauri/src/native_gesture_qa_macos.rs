//! Opt-in QA build only. Timed events enter AppKit's queue, never the document API.
use super::*;
use std::io::Read;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    tag: String,
    kind: String,
    gestures: u32,
    seconds: f64,
}
struct Run {
    cancel: Arc<AtomicBool>,
    id: u64,
    tag: String,
    kind: String,
}
struct Packet {
    label: String,
    cancel: Arc<AtomicBool>,
    id: u64,
    tag: String,
    kind: String,
    gesture: u32,
    step: u32,
    timestamp: f64,
}
thread_local! {static RUNS:RefCell<std::collections::HashMap<String,Run>>=RefCell::new(std::collections::HashMap::new());}
pub(super) fn stop(label: &str) {
    RUNS.with(|r| {
        if let Some(run) = r.borrow_mut().remove(label) {
            run.cancel.store(true, Ordering::Release);
        }
    });
}
pub(super) fn start() -> Result<(), String> {
    let app = APP.get().ok_or("QA app unavailable")?;
    if app.config().identifier != "app.lumapaint.pane-animation-qa" || current_label() != "main" {
        return Err("Timed input is restricted to the QA app".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open("/tmp/lumapaint-native-gesture-plan.json")
        .map_err(|e| e.to_string())?
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4096 {
        return Err("QA plan exceeds limit".into());
    }
    let plan: Plan = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if plan.tag.len() > 32
        || plan.tag.is_empty()
        || !plan
            .tag
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || !(1..=16).contains(&plan.gestures)
        || !plan.seconds.is_finite()
        || !(0.5..=3.).contains(&plan.seconds)
    {
        return Err("Invalid QA input plan".into());
    }
    let tool = TOOL.with(|t| t.get());
    if !matches!(
        (plan.kind.as_str(), tool),
        ("draw", CanvasTool::Brush) | ("pan", CanvasTool::Hand)
    ) {
        return Err("QA tool does not match the plan".into());
    }
    let owned = std::fs::canonicalize("/tmp/lumapaint-pane-animation-fixtures/Pane-1.lumapaint")
        .map_err(|e| e.to_string())?;
    if PROJECT_PATH.with(|p| {
        p.borrow()
            .as_ref()
            .and_then(|p| p.canonicalize().ok())
            .is_none_or(|p| p != owned)
    }) {
        return Err("QA input requires the owned Pane-1 fixture".into());
    }
    let label = current_label();
    stop(&label);
    let cancel = Arc::new(AtomicBool::new(false));
    let id = ACTIVE_DOCUMENT_ID.with(|i| i.get());
    RUNS.with(|r| {
        r.borrow_mut().insert(
            label.clone(),
            Run {
                cancel: cancel.clone(),
                id,
                tag: plan.tag.clone(),
                kind: plan.kind.clone(),
            },
        )
    });
    let app = app.clone();
    std::thread::spawn(move || {
        let start = Instant::now();
        let epoch: f64 = unsafe {
            let info: Retained<AnyObject> = msg_send![class!(NSProcessInfo), processInfo];
            msg_send![&*info, systemUptime]
        };
        for gesture in 0..plan.gestures {
            let base = Duration::from_secs_f64(f64::from(gesture) * (plan.seconds + 0.15));
            for step in 0..=61u32 {
                let fraction = if step == 0 {
                    0.
                } else if step == 61 {
                    1.
                } else {
                    f64::from(step) / 60.
                };
                let offset = base + Duration::from_secs_f64(fraction * plan.seconds);
                if let Some(wait) = (start + offset).checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                let label = label.clone();
                let cancel = cancel.clone();
                let tag = plan.tag.clone();
                let kind = plan.kind.clone();
                if app
                    .run_on_main_thread(move || {
                        deliver(Packet {
                            label,
                            cancel,
                            id,
                            tag,
                            kind,
                            gesture,
                            step,
                            timestamp: epoch + offset.as_secs_f64(),
                        });
                    })
                    .is_err()
                {
                    return;
                }
            }
        }
        let label2 = label.clone();
        let tag = plan.tag.clone();
        let kind = plan.kind.clone();
        let _ = app.run_on_main_thread(move || {
            if !cancel.load(Ordering::Acquire) {
                eprintln!(
                    "lumapaint-qa-run tag={tag} kind={kind} action=source-finished gestures={}",
                    plan.gestures
                );
            }
            // Keep cancellation identity until queued AppKit events are delivered.
            let _ = label2;
        });
    });
    Ok(())
}
fn guard_failure(valid: bool, active: bool, same_document: bool) -> Option<&'static str> {
    if !valid {
        Some("run_identity")
    } else if !active {
        Some("not_foreground")
    } else if !same_document {
        Some("document_changed")
    } else {
        None
    }
}
#[cfg(test)]
#[test]
fn native_input_guard_distinguishes_identity_foreground_and_document() {
    assert_eq!(guard_failure(true, true, true), None);
    assert_eq!(guard_failure(false, true, true), Some("run_identity"));
    assert_eq!(guard_failure(true, false, true), Some("not_foreground"));
    assert_eq!(guard_failure(true, true, false), Some("document_changed"));
    assert_eq!(guard_failure(false, false, false), Some("run_identity"));
}
fn deliver(packet: Packet) {
    let Packet {
        label,
        cancel,
        id,
        tag,
        kind,
        gesture,
        step,
        timestamp,
    } = packet;
    let label = label.as_str();
    let tag = tag.as_str();
    let kind = kind.as_str();
    let cancel = &cancel;
    if cancel.load(Ordering::Acquire) {
        return;
    }
    let Ok(_session) = SessionGuard::enter_render(label) else {
        cancel.store(true, Ordering::Release);
        eprintln!("lumapaint-qa-run tag={tag} action=interrupted reason=session_entry");
        return;
    };
    let valid = RUNS.with(|r| {
        r.borrow().get(label).is_some_and(|r| {
            r.id == id && r.tag == tag && r.kind == kind && Arc::ptr_eq(&r.cancel, cancel)
        })
    });
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let active = app.isActive();
    let actual_id = ACTIVE_DOCUMENT_ID.with(|v| v.get());
    if let Some(reason) = guard_failure(valid, active, actual_id == id) {
        cancel.store(true, Ordering::Release);
        eprintln!("lumapaint-qa-run tag={tag} action=interrupted reason={reason} run_valid={valid} app_active={active} expected_document={id} actual_document={actual_id}");
        return;
    }
    let view = CANVAS.with(|c| c.borrow().as_ref().map(|c| c.view.clone()));
    let Some(view) = view else {
        cancel.store(true, Ordering::Release);
        eprintln!("lumapaint-qa-run tag={tag} action=interrupted reason=missing_view");
        return;
    };
    let Some(window) = view.window() else {
        cancel.store(true, Ordering::Release);
        eprintln!("lumapaint-qa-run tag={tag} action=interrupted reason=missing_window");
        return;
    };
    let b = view.bounds();
    if view.isHidden()
        || b.size.width < 120.
        || b.size.width > 300.
        || b.size.height < 70.
        || b.size.height > 150.
    {
        cancel.store(true, Ordering::Release);
        eprintln!("lumapaint-qa-run tag={tag} action=geometry-rejected");
        return;
    }
    let f = if step == 0 {
        0.
    } else if step == 61 {
        1.
    } else {
        f64::from(step) / 60.
    };
    let (x0, y0, x1, y1) = if kind == "draw" {
        (
            0.15,
            0.18 + f64::from(gesture % 5) * 0.12,
            0.85,
            0.26 + f64::from(gesture % 5) * 0.12,
        )
    } else if gesture & 1 == 0 {
        (0.25, 0.35, 0.75, 0.65)
    } else {
        (0.75, 0.65, 0.25, 0.35)
    };
    let point = view.convertPoint_toView(
        NSPoint::new(
            (x0 + (x1 - x0) * f) * b.size.width,
            (y0 + (y1 - y0) * f) * b.size.height,
        ),
        None,
    );
    let ty = if step == 0 {
        objc2_app_kit::NSEventType::LeftMouseDown
    } else if step == 61 {
        objc2_app_kit::NSEventType::LeftMouseUp
    } else {
        objc2_app_kit::NSEventType::LeftMouseDragged
    };
    // SAFETY: AppKit's documented mouse-event factory, with its required nil
    // graphics context. All event/window/view objects stay on the main thread.
    let event: Option<Retained<NSEvent>> = unsafe {
        msg_send![class!(NSEvent),
            mouseEventWithType:ty, location:point, modifierFlags:NSEventModifierFlags::empty(),
            timestamp:timestamp, windowNumber:window.windowNumber(), context:std::ptr::null_mut::<AnyObject>(),
            eventNumber:gesture as isize+1, clickCount:1isize, pressure:if step==61{0f32}else{1f32}
        ]
    };
    if let Some(event) = event {
        if step == 0 || step == 61 {
            eprintln!("lumapaint-qa-gesture tag={tag} kind={kind} index={gesture} step={step} timestamp_ns={}",(timestamp*1e9) as u64);
        }
        // Opt-in QA transport timing separates main-dispatch wait from AppKit
        // queue wait. Synthetic source timestamps are not hardware event times.
        if std::env::var_os("LUMAPAINT_QA_DISPATCH_TIMING").is_some() {
            let now: f64 = unsafe {
                let info: Retained<AnyObject> = msg_send![class!(NSProcessInfo), processInfo];
                msg_send![&*info, systemUptime]
            };
            eprintln!("lumapaint-qa-dispatch tag={tag} index={gesture} step={step} source_ns={} dispatch_ns={}",(timestamp*1e9) as u64,(now*1e9) as u64);
        }
        app.postEvent_atStart(&event, false);
    }
}
