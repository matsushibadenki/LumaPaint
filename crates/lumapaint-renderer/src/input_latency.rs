//! Opt-in handler-to-present-call timings, never physical input-to-photon latency.
use std::time::Instant;
const WINDOW: usize = 512;
#[derive(Clone, Copy)]
struct Pending {
    first: (u64, Instant),
    latest: (u64, Instant),
    marks: u64,
}
pub(crate) struct Tracker {
    pending: Option<Pending>,
    values: [u64; WINDOW],
    completed: u64,
}
impl Default for Tracker {
    fn default() -> Self {
        Self {
            pending: None,
            values: [0; WINDOW],
            completed: 0,
        }
    }
}
/// Captures the marker by value so deferred frames retain their input identity.
pub(crate) struct Stage {
    name: &'static str,
    marker: (u64, Instant),
    started: Instant,
}
impl Drop for Stage {
    fn drop(&mut self) {
        let now = Instant::now();
        let ns = |start| {
            now.saturating_duration_since(start)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64
        };
        eprintln!(
            "lumapaint-input-stage event={} stage={} host_ns={} handler_elapsed_host_ns={}",
            self.marker.0,
            self.name,
            ns(self.started),
            ns(self.marker.1)
        );
    }
}
impl Tracker {
    pub fn stage(&self, name: &'static str) -> Option<Stage> {
        Some(Stage {
            name,
            marker: self.pending?.latest,
            started: Instant::now(),
        })
    }

    pub fn mark(&mut self, marker: (u64, Instant)) {
        if let Some(pending) = self.pending.as_mut() {
            if pending.latest.0 == marker.0 {
                return;
            }
            pending.latest = marker;
            pending.marks += 1;
        } else {
            self.pending = Some(Pending {
                first: marker,
                latest: marker,
                marks: 1,
            });
        }
    }
    fn complete(&mut self, now: Instant) -> Option<(Pending, u64, u64)> {
        let pending = self.pending.take()?;
        let ns = |start| {
            now.saturating_duration_since(start)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64
        };
        let first = ns(pending.first.1);
        let latest = ns(pending.latest.1);
        self.values[self.completed as usize % WINDOW] = latest;
        self.completed += 1;
        Some((pending, first, latest))
    }
    fn distribution(&self) -> (usize, u64, u64, u64, usize, usize) {
        let count = self.completed.min(WINDOW as u64) as usize;
        let mut values = self.values;
        let values = &mut values[..count];
        values.sort_unstable();
        let percentile = |p: usize| {
            if count == 0 {
                0
            } else {
                values[(count * p).div_ceil(100) - 1]
            }
        };
        (
            count,
            percentile(50),
            percentile(95),
            percentile(99),
            values.iter().filter(|&&n| n > 16_666_667).count(),
            values.iter().filter(|&&n| n > 8_333_333).count(),
        )
    }
    pub fn presented(&mut self, gpu: Option<(u64, u64)>) {
        let Some((pending, first, latest)) = self.complete(Instant::now()) else {
            return;
        };
        eprintln!("lumapaint-input-present first_event={} latest_event={} coalesced_events={} first_handler_to_present_host_ns={first} latest_handler_to_present_host_ns={latest} gpu_stream_frame={gpu:?}", pending.first.0, pending.latest.0, pending.marks);
        if self.completed.is_multiple_of(60) {
            let (count, p50, p95, p99, over60, over120) = self.distribution();
            eprintln!("lumapaint-input-present-summary completed={} window={count} latest_host_p50_ns={p50} latest_host_p95_ns={p95} latest_host_p99_ns={p99} over_16_67ms={over60} over_8_33ms={over120}", self.completed);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deferred_stages_capture_latest_marker_without_consuming_pending_input() {
        let mut tracker = Tracker::default();
        assert!(tracker.stage("surface_acquire").is_none());
        let start = Instant::now();
        tracker.mark((1, start));
        tracker.mark((2, start));
        let stage = tracker.stage("surface_acquire").unwrap();
        tracker.mark((3, start));
        assert_eq!(stage.marker.0, 2);
        assert_eq!(tracker.pending.unwrap().latest.0, 3);
        drop(stage);
        tracker.complete(start);
        assert!(tracker.stage("surface_acquire").is_none());
    }
    #[test]
    fn coalescing_ignores_duplicate_marks_clears_on_present_and_bounds_history() {
        let mut t = Tracker::default();
        let start = Instant::now();
        t.mark((1, start));
        t.mark((1, start));
        t.mark((2, start + std::time::Duration::from_millis(10)));
        let (p, first, latest) = t
            .complete(start + std::time::Duration::from_millis(20))
            .unwrap();
        assert_eq!(p.marks, 2);
        assert_eq!(first, 20_000_000);
        assert_eq!(latest, 10_000_000);
        assert!(t.complete(start).is_none());
        for id in 3..=WINDOW as u64 + 3 {
            t.mark((id, start));
            t.complete(start + std::time::Duration::from_millis(30));
        }
        assert_eq!(
            t.distribution(),
            (WINDOW, 30_000_000, 30_000_000, 30_000_000, WINDOW, WINDOW)
        );
        assert!(t.pending.is_none());
    }
}
