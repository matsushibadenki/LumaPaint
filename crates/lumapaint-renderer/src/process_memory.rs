//! Rate-limited host-process RSS, distinct from logical GPU resource payloads.
use std::time::{Duration, Instant};
#[derive(Debug)]
pub(crate) struct ProcessMemory {
    pub rss_bytes: u64,
    pub peak_rss_bytes: Option<u64>,
}
#[derive(Default)]
pub(crate) struct Sampler {
    last: Option<Instant>,
}
impl Sampler {
    pub fn sample(&mut self) -> Option<ProcessMemory> {
        let now = Instant::now();
        if self
            .last
            .is_some_and(|last| now.duration_since(last) < Duration::from_secs(1))
        {
            return None;
        }
        self.last = Some(now);
        let _timer = crate::performance::time("process_memory_sample_host");
        let memory = read_process_memory();
        if memory.is_none() {
            crate::performance::count("process_memory_samples_unavailable", 1);
        }
        memory
    }
}
#[cfg(target_os = "macos")]
fn read_process_memory() -> Option<ProcessMemory> {
    // MACH_TASK_BASIC_INFO from the installed SDK's mach/task_info.h.
    #[repr(C)]
    #[derive(Default)]
    struct BasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: [i32; 2],
        system_time: [i32; 2],
        policy: i32,
        suspend_count: i32,
    }
    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
    }
    let mut info = BasicInfo::default();
    let expected = (std::mem::size_of::<BasicInfo>() / std::mem::size_of::<i32>()) as u32;
    let mut count = expected;
    // SAFETY: flavor 20 expects this exact repr(C) structure and a word count.
    // The initialized writable buffer covers all advertised words. Query only
    // this process's Mach task port; the OS call cannot retain either pointer.
    let result = unsafe {
        task_info(
            mach_task_self_,
            20,
            (&mut info as *mut BasicInfo).cast(),
            &mut count,
        )
    };
    (result == 0 && count == expected).then_some(ProcessMemory {
        rss_bytes: info.resident_size,
        peak_rss_bytes: Some(info.resident_size_max),
    })
}
#[cfg(any(target_os = "linux", test))]
fn parse_linux_status(status: &str) -> Option<ProcessMemory> {
    let bytes = |name: &str| -> Option<u64> {
        let mut fields = status
            .lines()
            .find_map(|line| line.strip_prefix(name))?
            .split_whitespace();
        let value: u64 = fields.next()?.parse().ok()?;
        if fields.next()? != "kB" || fields.next().is_some() {
            return None;
        }
        value.checked_mul(1024)
    };
    Some(ProcessMemory {
        rss_bytes: bytes("VmRSS:")?,
        peak_rss_bytes: bytes("VmHWM:"),
    })
}
#[cfg(target_os = "linux")]
fn read_process_memory() -> Option<ProcessMemory> {
    parse_linux_status(&std::fs::read_to_string("/proc/self/status").ok()?)
}
#[cfg(target_os = "windows")]
fn read_process_memory() -> Option<ProcessMemory> {
    // https://learn.microsoft.com/windows/win32/api/psapi/ns-psapi-process_memory_counters
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let size = std::mem::size_of::<Counters>() as u32;
    let mut counters = Counters {
        cb: size,
        ..Default::default()
    };
    // SAFETY: query our pseudo-handle (never close it), passing the initialized
    // Windows ABI structure and its complete writable size. No pointer is retained.
    let result = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
    (result != 0).then_some(ProcessMemory {
        rss_bytes: counters.working_set as u64,
        peak_rss_bytes: Some(counters.peak_working_set as u64),
    })
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn read_process_memory() -> Option<ProcessMemory> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_status_requires_rss_and_preserves_unknown_peak() {
        let memory = parse_linux_status("Name: x\nVmRSS:\t123 kB\nVmHWM: 456 kB\n").unwrap();
        assert_eq!(memory.rss_bytes, 123 * 1024);
        assert_eq!(memory.peak_rss_bytes, Some(456 * 1024));
        assert_eq!(
            parse_linux_status("VmRSS: 1 kB").unwrap().peak_rss_bytes,
            None
        );
        for invalid in [
            "VmHWM: 4 kB",
            "VmRSS: -1 kB",
            "VmRSS: 10 MB",
            "VmRSS: 18446744073709551615 kB",
        ] {
            assert!(parse_linux_status(invalid).is_none());
        }
    }
    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    fn current_process_rss_is_available_and_sampling_is_rate_limited() {
        let mut sampler = Sampler::default();
        let memory = sampler.sample().expect("host process RSS available");
        assert!(memory.rss_bytes > 0);
        assert!(memory
            .peak_rss_bytes
            .is_none_or(|peak| peak >= memory.rss_bytes));
        assert!(
            sampler.sample().is_none(),
            "never issue a memory syscall on every frame"
        );
    }
}
