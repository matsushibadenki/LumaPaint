//! Diagnostic-build Rust heap accounting. Includes all Rust threads from startup.
//! Requested layout bytes exclude allocator metadata, C/C++ allocations and VRAM.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub live_bytes: u64,
    pub peak_bytes: u64,
    pub live_allocations: u64,
    pub allocation_calls: u64,
    pub reallocation_calls: u64,
    pub failed_calls: u64,
    pub allocated_bytes: u64,
    pub released_bytes: u64,
}

struct Allocator {
    live: AtomicU64,
    peak: AtomicU64,
    objects: AtomicU64,
    calls: AtomicU64,
    reallocations: AtomicU64,
    failures: AtomicU64,
    allocated: AtomicU64,
    released: AtomicU64,
}

impl Allocator {
    const fn new() -> Self {
        Self {
            live: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            objects: AtomicU64::new(0),
            calls: AtomicU64::new(0),
            reallocations: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            allocated: AtomicU64::new(0),
            released: AtomicU64::new(0),
        }
    }

    fn grow(&self, bytes: usize) {
        let bytes = bytes as u64;
        let live = self.live.fetch_add(bytes, Relaxed) + bytes;
        self.peak.fetch_max(live, Relaxed);
        self.allocated.fetch_add(bytes, Relaxed);
    }

    fn shrink(&self, bytes: usize) {
        self.live.fetch_sub(bytes as u64, Relaxed);
        self.released.fetch_add(bytes as u64, Relaxed);
    }

    fn allocation_result(&self, pointer: *mut u8, size: usize) {
        self.calls.fetch_add(1, Relaxed);
        if pointer.is_null() {
            self.failures.fetch_add(1, Relaxed);
        } else {
            self.grow(size);
            self.objects.fetch_add(1, Relaxed);
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            live_bytes: self.live.load(Relaxed),
            peak_bytes: self.peak.load(Relaxed),
            live_allocations: self.objects.load(Relaxed),
            allocation_calls: self.calls.load(Relaxed),
            reallocation_calls: self.reallocations.load(Relaxed),
            failed_calls: self.failures.load(Relaxed),
            allocated_bytes: self.allocated.load(Relaxed),
            released_bytes: self.released.load(Relaxed),
        }
    }
}

// SAFETY: every allocation delegates unchanged to System. Accounting uses only
// atomics: no allocation, locks, environment access, or logging inside allocator calls.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: caller supplies a valid nonzero allocation layout.
        let pointer = unsafe { System.alloc(layout) };
        self.allocation_result(pointer, layout.size());
        pointer
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: caller supplies a valid nonzero allocation layout.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        self.allocation_result(pointer, layout.size());
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        self.shrink(layout.size());
        self.objects.fetch_sub(1, Relaxed);
        // SAFETY: pointer and layout are the original System allocation pair.
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        self.reallocations.fetch_add(1, Relaxed);
        // SAFETY: caller guarantees original allocation/layout and valid new size.
        let result = unsafe { System.realloc(pointer, layout, size) };
        if result.is_null() {
            self.failures.fetch_add(1, Relaxed);
        } else if size >= layout.size() {
            self.grow(size - layout.size());
        } else {
            self.shrink(layout.size() - size);
        }
        result
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator::new();

/// Independently sampled fields; concurrent allocations can span the sample.
pub fn snapshot() -> Snapshot {
    ALLOCATOR.snapshot()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts_growth_shrink_zeroing_and_failures_balance() {
        let allocator = Allocator::new();
        let layout = Layout::from_size_align(64, 16).unwrap();
        // SAFETY: layouts match each successful allocation; each is freed once.
        unsafe {
            let pointer = allocator.alloc_zeroed(layout);
            assert!(!pointer.is_null());
            assert!(std::slice::from_raw_parts(pointer, 64)
                .iter()
                .all(|&b| b == 0));
            let pointer = allocator.realloc(pointer, layout, 128);
            assert!(!pointer.is_null());
            let pointer = allocator.realloc(pointer, Layout::from_size_align(128, 16).unwrap(), 32);
            assert!(!pointer.is_null());
            allocator.dealloc(pointer, Layout::from_size_align(32, 16).unwrap());
        }
        allocator.allocation_result(std::ptr::null_mut(), 500);
        let sample = allocator.snapshot();
        assert_eq!(sample.live_bytes, 0);
        assert_eq!(sample.live_allocations, 0);
        assert_eq!(sample.peak_bytes, 128);
        assert_eq!(sample.allocated_bytes, 128);
        assert_eq!(sample.released_bytes, 128);
        assert_eq!(sample.failed_calls, 1);
        assert_eq!(sample.reallocation_calls, 2);
    }
    #[test]
    fn worker_allocations_are_included_and_process_snapshot_allocates_nothing() {
        let allocator = std::sync::Arc::new(Allocator::new());
        let worker = allocator.clone();
        std::thread::spawn(move || {
            let layout = Layout::from_size_align(1024, 8).unwrap();
            // SAFETY: valid layout; same pointer/layout freed exactly once.
            unsafe {
                let pointer = worker.alloc(layout);
                assert!(!pointer.is_null());
                worker.dealloc(pointer, layout);
            }
        })
        .join()
        .unwrap();
        assert_eq!(allocator.snapshot().allocated_bytes, 1024);
        assert_eq!(allocator.snapshot().live_bytes, 0);
        let before = snapshot();
        let after = snapshot();
        assert!(after.allocated_bytes >= before.allocated_bytes);
    }
}
