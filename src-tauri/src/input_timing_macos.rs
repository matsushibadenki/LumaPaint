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
