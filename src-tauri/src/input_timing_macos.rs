//! Opt-in synchronous input stages, correlated with renderer presentation logs.
//! Host durations exclude OS event queueing and physical display latency.
use std::time::Instant;

pub(super) struct Stage {
    name: &'static str,
    input: Option<(u64, Instant)>,
    started: Option<Instant>,
}

impl Stage {
    pub(super) fn new(name: &'static str) -> Self {
        let input = lumapaint_core::performance::input_started();
        Self {
            name,
            input,
            started: input.map(|_| Instant::now()),
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        if let (Some((event, handler_started)), Some(started)) = (self.input, self.started) {
            let now = Instant::now();
            eprintln!(
                "lumapaint-input-stage event={event} stage={} host_ns={} handler_elapsed_host_ns={}",
                self.name,
                now.saturating_duration_since(started).as_nanos(),
                now.saturating_duration_since(handler_started).as_nanos(),
            );
        }
    }
}

// Production priority must not depend on opt-in performance instrumentation.
thread_local! { static INTERACTION_DEPTH: std::cell::Cell<u32> = const {std::cell::Cell::new(0)}; }
pub(super) struct Interaction;
impl Interaction {
    pub fn new() -> Self {
        INTERACTION_DEPTH.with(|d| d.set(d.get() + 1));
        Self
    }
    pub fn active() -> bool {
        INTERACTION_DEPTH.with(|d| d.get() > 0)
    }
}
impl Drop for Interaction {
    fn drop(&mut self) {
        INTERACTION_DEPTH.with(|d| d.set(d.get() - 1));
    }
}
pub(super) fn event_age(event: &objc2_app_kit::NSEvent) {
    let Some((id, _)) = lumapaint_core::performance::input_started() else {
        return;
    };
    // NSEvent timestamps and systemUptime both use seconds since system startup.
    let uptime: f64 = unsafe {
        let info: objc2::rc::Retained<objc2::runtime::AnyObject> =
            objc2::msg_send![objc2::class!(NSProcessInfo), processInfo];
        objc2::msg_send![&*info, systemUptime]
    };
    let age = uptime - event.timestamp();
    if age.is_finite() && (0. ..=60.).contains(&age) {
        eprintln!(
            "lumapaint-input-queue event={id} age_ns={} event_timestamp_ns={}",
            (age * 1e9) as u64,
            (event.timestamp() * 1e9) as u64
        );
    }
}
#[cfg(test)]
#[test]
fn nested_interactions_restore_priority_scope() {
    assert!(!Interaction::active());
    {
        let _outer = Interaction::new();
        {
            let _inner = Interaction::new();
            assert!(Interaction::active());
        }
        assert!(Interaction::active());
    }
    assert!(!Interaction::active());
}
