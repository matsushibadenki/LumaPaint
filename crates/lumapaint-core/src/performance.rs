//! Opt-in, thread-local CPU diagnostics. Durations are inclusive host timings, not GPU time.
//! Worker threads expose separate snapshots; no lock is taken on the rendering hot path.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Instant;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub counts: BTreeMap<&'static str, u64>,
    pub cpu_nanoseconds: BTreeMap<&'static str, u64>,
}

thread_local! {
    static STATS: RefCell<Snapshot> = RefCell::new(Snapshot::default());
}

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("LUMAPAINT_RENDER_METRICS").is_some()
            || std::env::var_os("LUMAPAINT_RENDER_DEBUG").is_some()
    })
}

pub fn count(name: &'static str, value: u64) {
    if enabled() {
        STATS.with(|stats| {
            let mut stats = stats.borrow_mut();
            let counter = stats.counts.entry(name).or_default();
            *counter = counter.saturating_add(value);
        });
    }
}

/// Drain this thread only. Background work must be measured on the worker itself.
pub fn take() -> Snapshot {
    STATS.with(|stats| std::mem::take(&mut *stats.borrow_mut()))
}

#[must_use]
pub struct Timer {
    name: &'static str,
    start: Option<Instant>,
}

pub fn time(name: &'static str) -> Timer {
    Timer {
        name,
        start: enabled().then(Instant::now),
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let elapsed = start.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            STATS.with(|stats| {
                let mut stats = stats.borrow_mut();
                let total = stats.cpu_nanoseconds.entry(self.name).or_default();
                *total = total.saturating_add(elapsed);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_are_drained_and_workers_do_not_share_counters() {
        take();
        STATS.with(|stats| stats.borrow_mut().counts.insert("parent", 7));
        let worker = std::thread::spawn(|| {
            assert_eq!(take(), Snapshot::default());
            STATS.with(|stats| stats.borrow_mut().counts.insert("worker", 3));
            take()
        })
        .join()
        .unwrap();
        assert_eq!(worker.counts.get("worker"), Some(&3));
        let parent = take();
        assert_eq!(parent.counts.get("parent"), Some(&7));
        assert!(!parent.counts.contains_key("worker"));
        assert_eq!(take(), Snapshot::default());
    }
}
