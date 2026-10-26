//! JSON wire-size accounting, not an allocator/RSS measurement.
use super::*;
use std::{
    cell::RefCell,
    hash::{DefaultHasher, Hasher},
    io::Write,
    rc::Weak,
    sync::Mutex,
};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    unique_summary_wire_bytes: usize,
    logical_window_wire_bytes: usize,
    unique_summaries: usize,
    evictions: u64,
    windows: Vec<Window>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    label: String,
    document_id: Option<u64>,
    attempted_document_id: Option<u64>,
    sequence: u64,
    summary_wire_bytes: usize,
    stale_version: bool,
    delivery_failed: bool,
    ready: bool,
}
static REPORT: Mutex<Option<Report>> = Mutex::new(None);
pub(crate) fn report() -> Report {
    REPORT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}
#[derive(Default)]
struct Monitor {
    sizes: HashMap<usize, (Weak<DocumentSnapshot>, usize, u64)>,
    latest: HashMap<u64, (usize, u64)>,
    evictions: u64,
}
thread_local! { static MONITOR: RefCell<Monitor> = RefCell::new(Monitor::default()); }
struct Counter(usize, DefaultHasher);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let remaining = (MAX_SUMMARY_WIRE_BYTES + 1).saturating_sub(self.0);
        self.1.write(&bytes[..bytes.len().min(remaining)]);
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > MAX_SUMMARY_WIRE_BYTES {
            self.0 = MAX_SUMMARY_WIRE_BYTES + 1;
            return Err(std::io::Error::other(
                "Summary exceeds cache wire-size limit",
            ));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn summary_bytes(summary: &Rc<DocumentSnapshot>) -> Result<usize, String> {
    MONITOR.with(|slot| {
        let mut monitor = slot.borrow_mut();
        monitor
            .sizes
            .retain(|_, (weak, _, _)| weak.strong_count() > 0);
        let key = Rc::as_ptr(summary) as usize;
        if let Some((weak, size, _)) = monitor.sizes.get(&key) {
            if weak.ptr_eq(&Rc::downgrade(summary)) {
                return Ok(*size);
            }
        }
        let mut counter = Counter(0, DefaultHasher::new());
        if let Err(error) = serde_json::to_writer(&mut counter, &wire::DocumentView(summary, false))
        {
            if counter.0 <= MAX_SUMMARY_WIRE_BYTES {
                return Err(error.to_string());
            }
        }
        monitor
            .sizes
            .insert(key, (Rc::downgrade(summary), counter.0, counter.1.finish()));
        Ok(counter.0)
    })
}
pub(super) fn observe(id: Option<u64>, summary: Option<&Rc<DocumentSnapshot>>) {
    if let (Some(id), Some(summary)) = (id, summary) {
        if summary_bytes(summary).is_ok() {
            MONITOR.with(|slot| {
                let mut monitor = slot.borrow_mut();
                if let Some((_, bytes, digest)) = monitor.sizes.get(&(Rc::as_ptr(summary) as usize))
                {
                    let version = (*bytes, *digest);
                    monitor.latest.insert(id, version);
                }
            });
        }
    }
}
fn is_stale(cache: &Cache, monitor: &Monitor) -> bool {
    if cache.previous.is_some() && cache.active_id != cache.attempted_id {
        return true;
    }
    cache
        .active_id
        .and_then(|id| monitor.latest.get(&id))
        .zip(
            cache
                .previous
                .as_ref()
                .and_then(|previous| monitor.sizes.get(&(Rc::as_ptr(previous) as usize))),
        )
        .is_some_and(|(latest, (_, bytes, digest))| *latest != (*bytes, *digest))
}
pub(super) fn enforce_and_report(caches: &mut HashMap<String, Cache>) -> Report {
    MONITOR.with(|slot| {
        let mut monitor = slot.borrow_mut();
        let mut versions: HashMap<usize, (usize, bool)> = HashMap::new();
        for cache in caches.values() {
            if let Some(summary) = &cache.previous {
                let entry = versions
                    .entry(Rc::as_ptr(summary) as usize)
                    .or_insert((cache.retained_bytes, true));
                entry.1 &= is_stale(cache, &monitor);
            }
        }
        let mut total: usize = versions.values().map(|v| v.0).sum();
        // Evict whole versions, stale first, then larger versions. Every affected
        // subscriber keeps its sequence but must receive a full packet next time.
        let mut candidates: Vec<_> = versions.into_iter().collect();
        candidates.sort_by_key(|(key, (bytes, stale))| (!*stale, std::cmp::Reverse(*bytes), *key));
        for (key, (bytes, _)) in candidates {
            if total <= MAX_CACHE_WIRE_BYTES {
                break;
            }
            for cache in caches.values_mut() {
                if cache
                    .previous
                    .as_ref()
                    .is_some_and(|s| Rc::as_ptr(s) as usize == key)
                {
                    cache.previous = None;
                    cache.retained_bytes = 0;
                    cache.ready = false;
                }
            }
            total = total.saturating_sub(bytes);
            monitor.evictions = monitor.evictions.saturating_add(1);
        }
        let mut report = Report {
            evictions: monitor.evictions,
            ..Report::default()
        };
        let mut unique = std::collections::HashSet::new();
        for (label, cache) in caches.iter() {
            if let Some(summary) = &cache.previous {
                if unique.insert(Rc::as_ptr(summary) as usize) {
                    report.unique_summary_wire_bytes += cache.retained_bytes;
                    report.unique_summaries += 1;
                }
            }
            report.logical_window_wire_bytes += cache.retained_bytes;
            report.windows.push(Window {
                label: label.clone(),
                document_id: cache.active_id,
                attempted_document_id: cache.attempted_id,
                sequence: cache.sequence,
                summary_wire_bytes: cache.retained_bytes,
                stale_version: is_stale(cache, &monitor),
                delivery_failed: cache.delivery_failed,
                ready: cache.ready,
            });
        }
        report.windows.sort_by(|a, b| a.label.cmp(&b.label));
        monitor
            .sizes
            .retain(|_, (weak, _, _)| weak.strong_count() > 0);
        monitor
            .latest
            .retain(|id, _| caches.values().any(|cache| cache.active_id == Some(*id)));
        *REPORT.lock().unwrap_or_else(|e| e.into_inner()) = Some(report.clone());
        report
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_notifications::tests::fixture;
    fn source(bytes: usize) -> Rc<DocumentSnapshot> {
        let mut summary = fixture().active.unwrap();
        summary.name = "x".repeat(bytes);
        Rc::new(summary)
    }
    fn deliver(cache: &mut Cache, id: u64, source: &Rc<DocumentSnapshot>) {
        cache
            .publish_shared(Some(id), Some(Rc::clone(source)), &[], false, |_| Ok(()))
            .unwrap();
    }
    #[test]
    fn few_objects_with_large_metadata_exceed_byte_limit_and_release_cache() {
        let small = source(0);
        let mut cache = Cache::default();
        deliver(&mut cache, 1, &small);
        let big = source(MAX_SUMMARY_WIRE_BYTES + 1);
        assert_eq!(summary_bytes(&big).unwrap(), MAX_SUMMARY_WIRE_BYTES + 1);
        assert!(cache
            .publish_shared(Some(1), Some(Rc::clone(&big)), &[], false, |_| Err(
                "delivery failed".into()
            ))
            .is_err());
        assert_eq!(cache.sequence, 1);
        assert!(Rc::ptr_eq(cache.previous.as_ref().unwrap(), &small));
        let weak = Rc::downgrade(&big);
        cache
            .publish_shared(Some(1), Some(big), &[], false, |json| {
                let packet: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert!(packet["baseSequence"].is_null());
                Ok(())
            })
            .unwrap();
        assert!(!cache.ready);
        assert!(cache.previous.is_none());
        assert!(weak.upgrade().is_none());
        cache
            .publish_shared(Some(1), Some(small), &[], false, |json| {
                assert!(
                    serde_json::from_str::<serde_json::Value>(&json).unwrap()["baseSequence"]
                        .is_null()
                );
                Ok(())
            })
            .unwrap();
    }
    #[test]
    fn reports_unique_and_logical_bytes_and_failed_stale_destination() {
        let old = source(65536);
        let new = source(131072);
        let old_bytes = summary_bytes(&old).unwrap();
        let new_bytes = summary_bytes(&new).unwrap();
        let mut caches = HashMap::new();
        deliver(caches.entry("a".into()).or_default(), 300001, &old);
        deliver(caches.entry("b".into()).or_default(), 300001, &old);
        observe(Some(300001), Some(&old));
        let report = enforce_and_report(&mut caches);
        assert_eq!(report.unique_summary_wire_bytes, old_bytes);
        assert_eq!(report.logical_window_wire_bytes, old_bytes * 2);
        let equivalent = Rc::new((*old).clone());
        observe(Some(300001), Some(&equivalent));
        assert!(!enforce_and_report(&mut caches)
            .windows
            .iter()
            .any(|w| w.stale_version));
        observe(Some(300001), Some(&new));
        deliver(caches.get_mut("b").unwrap(), 300001, &new);
        assert!(caches
            .get_mut("a")
            .unwrap()
            .publish_shared(Some(300001), Some(Rc::clone(&new)), &[], false, |_| Err(
                "failed".into()
            ))
            .is_err());
        let report = enforce_and_report(&mut caches);
        assert_eq!(report.unique_summary_wire_bytes, old_bytes + new_bytes);
        assert!(report.windows[0].stale_version);
        assert!(report.windows[0].delivery_failed);
        assert!(!report.windows[1].stale_version);
        println!(
            "cache_measurement={}",
            serde_json::to_string(&report).unwrap()
        );
        deliver(caches.get_mut("a").unwrap(), 300001, &new);
        let report = enforce_and_report(&mut caches);
        assert_eq!(report.unique_summaries, 1);
        assert!(!report
            .windows
            .iter()
            .any(|w| w.stale_version || w.delivery_failed));
        assert!(caches
            .get_mut("a")
            .unwrap()
            .publish_shared(Some(300009), Some(Rc::clone(&new)), &[], false, |_| Err(
                "switch failed".into()
            ))
            .is_err());
        let report = enforce_and_report(&mut caches);
        assert!(report.windows[0].stale_version);
        assert_eq!(report.windows[0].document_id, Some(300001));
        assert_eq!(report.windows[0].attempted_document_id, Some(300009));
    }
    #[test]
    fn global_budget_evicts_stale_version_without_advancing_sequence() {
        let old = source(1800000);
        let new = source(1800001);
        let mut caches = HashMap::new();
        deliver(caches.entry("stale".into()).or_default(), 300002, &old);
        observe(Some(300002), Some(&new));
        for id in 300003..300008 {
            let summary = source(1800000);
            deliver(caches.entry(id.to_string()).or_default(), id, &summary);
            observe(Some(id), Some(&summary));
        }
        let report = enforce_and_report(&mut caches);
        assert!(report.unique_summary_wire_bytes <= MAX_CACHE_WIRE_BYTES);
        assert!(!caches["stale"].ready);
        assert_eq!(caches["stale"].sequence, 1);
        assert!(caches["stale"].previous.is_none());
        caches
            .get_mut("stale")
            .unwrap()
            .publish_shared(Some(300002), Some(new), &[], false, |json| {
                let packet: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert_eq!(packet["sequence"], 2);
                assert!(packet["baseSequence"].is_null());
                Ok(())
            })
            .unwrap();
    }
}
