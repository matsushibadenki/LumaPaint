//! Opt-in, thread-local CPU diagnostics. Durations are inclusive host timings, not GPU time.
//! Worker threads expose separate snapshots; no lock is taken on the rendering hot path.
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Instant;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub counts: BTreeMap<&'static str, u64>,
    pub cpu_nanoseconds: BTreeMap<&'static str, u64>,
}

impl Snapshot {
    pub fn merge(&mut self, other: &Self) {
        for (target, source) in [
            (&mut self.counts, &other.counts),
            (&mut self.cpu_nanoseconds, &other.cpu_nanoseconds),
        ] {
            for (&name, &value) in source {
                let total = target.entry(name).or_default();
                *total = total.saturating_add(value);
            }
        }
    }
}

static PROCESS_TOTAL: OnceLock<Mutex<Snapshot>> = OnceLock::new();

/// Drain and publish one completed batch. Only batch boundaries take a lock.
/// Totals cover published work; pending thread-local work is intentionally excluded.
pub fn take_and_publish() -> Snapshot {
    let snapshot = take();
    if enabled() && (!snapshot.counts.is_empty() || !snapshot.cpu_nanoseconds.is_empty()) {
        PROCESS_TOTAL
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .merge(&snapshot);
    }
    snapshot
}

/// Cumulative published host work; nested/parallel durations are not wall time.
pub fn process_total() -> Snapshot {
    PROCESS_TOTAL.get().map_or_else(Snapshot::default, |total| {
        total
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    })
}

thread_local! {
    static STATS: RefCell<Snapshot> = RefCell::new(Snapshot::default());
    static BATCH_DEPTH: Cell<usize> = const { Cell::new(0) };
    static INPUT: Cell<Option<(u64, Instant)>> = const { Cell::new(None) };
}

/// Synchronous host input/command scope. A queued redraw captures this marker
/// before the scope ends. This does not measure OS dispatch or physical display.
pub struct InputEvent {
    previous: Option<(u64, Instant)>,
    active: bool,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}
pub fn input_event() -> InputEvent {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let active = enabled();
    let previous = if active {
        let marker = (
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            Instant::now(),
        );
        INPUT.with(|input| input.replace(Some(marker)))
    } else {
        None
    };
    InputEvent {
        previous,
        active,
        _thread_bound: std::marker::PhantomData,
    }
}
pub fn input_started() -> Option<(u64, Instant)> {
    if enabled() {
        INPUT.with(Cell::get)
    } else {
        None
    }
}
impl Drop for InputEvent {
    fn drop(&mut self) {
        if self.active {
            INPUT.with(|input| input.set(self.previous));
        }
    }
}

/// Publish completed host work at the outermost synchronous job/session boundary.
/// Nested batches do not take the process lock. Early returns and cancellation
/// still publish the work already performed. Never hold a batch across `.await`:
/// its counters belong to the creating thread.
#[must_use]
pub struct Batch {
    timer: Option<Timer>,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

pub fn batch(name: &'static str) -> Batch {
    let timer = if enabled() {
        BATCH_DEPTH.with(|depth| depth.set(depth.get() + 1));
        count(name, 1);
        Some(time(name))
    } else {
        None
    };
    Batch {
        timer,
        _thread_bound: std::marker::PhantomData,
    }
}

impl Drop for Batch {
    fn drop(&mut self) {
        if self.timer.is_none() {
            return;
        }
        // Finish the inclusive host timer before draining its thread-local data.
        drop(self.timer.take());
        let outermost = BATCH_DEPTH.with(|depth| {
            let next = depth.get() - 1;
            depth.set(next);
            next == 0
        });
        if outermost {
            take_and_publish();
        }
    }
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
    fn nested_input_scopes_restore_the_outer_event_after_unwind() {
        if !enabled() {
            return;
        }
        assert!(input_started().is_none());
        {
            let _outer = input_event();
            let outer = input_started().unwrap();
            let _ = std::panic::catch_unwind(|| {
                let _inner = input_event();
                assert_ne!(input_started().unwrap().0, outer.0);
                panic!("cancel nested input");
            });
            assert_eq!(input_started(), Some(outer));
        }
        assert!(input_started().is_none());
    }

    #[test]
    fn nested_batches_publish_once_even_on_early_return_and_unwind() {
        if !enabled() {
            return;
        }
        take();
        let before = process_total()
            .counts
            .get("test.batch_work")
            .copied()
            .unwrap_or(0);
        let _ = std::panic::catch_unwind(|| {
            let _outer = batch("test.outer_batch");
            {
                let _inner = batch("test.inner_batch");
                count("test.batch_work", 7);
            }
            assert_eq!(
                process_total()
                    .counts
                    .get("test.batch_work")
                    .copied()
                    .unwrap_or(0),
                before
            );
            panic!("test cancelled job");
        });
        assert_eq!(process_total().counts["test.batch_work"], before + 7);
        assert_eq!(take(), Snapshot::default());
        assert_eq!(BATCH_DEPTH.with(Cell::get), 0);
        let early_return = || {
            let _batch = batch("test.return_batch");
            count("test.batch_work", 3);
        };
        early_return();
        assert_eq!(process_total().counts["test.batch_work"], before + 10);
        assert!(process_total()
            .cpu_nanoseconds
            .contains_key("test.outer_batch"));
    }

    #[test]
    fn published_worker_batches_are_counted_once() {
        if !enabled() {
            return;
        }
        let before = process_total()
            .counts
            .get("test.published_worker_batch")
            .copied()
            .unwrap_or(0);
        let worker = std::thread::spawn(|| {
            count("test.published_worker_batch", 17);
            let published = take_and_publish();
            assert_eq!(published.counts["test.published_worker_batch"], 17);
            assert_eq!(take_and_publish(), Snapshot::default());
        });
        worker.join().unwrap();
        assert_eq!(
            process_total().counts["test.published_worker_batch"],
            before + 17
        );
    }

    #[test]
    fn merging_worker_batches_is_saturating_and_preserves_names() {
        let mut total = Snapshot::default();
        total.counts.insert("bytes", u64::MAX - 1);
        let worker = std::thread::spawn(|| {
            let mut snapshot = Snapshot::default();
            snapshot.counts.insert("bytes", 4);
            snapshot.counts.insert("worker", 1);
            snapshot.cpu_nanoseconds.insert("parse", 12);
            snapshot
        })
        .join()
        .unwrap();
        total.merge(&worker);
        assert_eq!(total.counts["bytes"], u64::MAX);
        assert_eq!(total.counts["worker"], 1);
        assert_eq!(total.cpu_nanoseconds["parse"], 12);
        total.merge(&Snapshot::default());
        assert_eq!(total.cpu_nanoseconds["parse"], 12);
    }

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
