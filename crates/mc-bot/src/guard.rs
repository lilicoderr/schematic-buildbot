//! Resource guard for heavy offline jobs (the slicer): below-normal priority, a time limit, a
//! memory limit and a progress line at least every 10 seconds, so a run is never silent and never
//! starves other programs on the same machine.

use std::time::{Duration, Instant};

#[cfg(windows)]
mod sys {
    #[repr(C)]
    pub struct ProcessMemoryCounters {
        pub cb: u32,
        pub page_fault_count: u32,
        pub peak_working_set: usize,
        pub working_set: usize,
        pub rest: [usize; 6],
    }
    extern "system" {
        pub fn GetCurrentProcess() -> isize;
        pub fn SetPriorityClass(process: isize, class: u32) -> i32;
        pub fn K32GetProcessMemoryInfo(process: isize, counters: *mut ProcessMemoryCounters, cb: u32) -> i32;
    }
}

/// Run this process at below-normal priority (no-op where unsupported).
pub fn lower_priority() {
    #[cfg(windows)]
    // SAFETY: plain Win32 calls on the current process handle.
    unsafe {
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x4000;
        sys::SetPriorityClass(sys::GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS);
    }
}

/// Resident memory of this process in bytes, where it can be read.
pub fn rss_bytes() -> Option<usize> {
    #[cfg(windows)]
    // SAFETY: the struct matches PROCESS_MEMORY_COUNTERS and `cb` is its size.
    unsafe {
        let mut c = sys::ProcessMemoryCounters { cb: 0, page_fault_count: 0, peak_working_set: 0, working_set: 0, rest: [0; 6] };
        c.cb = std::mem::size_of::<sys::ProcessMemoryCounters>() as u32;
        let ok = sys::K32GetProcessMemoryInfo(sys::GetCurrentProcess(), &mut c, c.cb);
        return (ok != 0).then_some(c.working_set);
    }
    #[cfg(not(windows))]
    None
}

pub struct Budget {
    started: Instant,
    limit: Duration,
    max_rss: usize,
    last_report: Instant,
}

impl Budget {
    pub fn new(limit: Duration, max_rss: usize) -> Self {
        let now = Instant::now();
        Budget { started: now, limit, max_rss, last_report: now }
    }

    /// Call often. Prints `phase` and `progress` every 10 s; errors with a clear message when the
    /// time or memory limit is passed.
    pub fn check(&mut self, phase: &str, progress: &dyn Fn() -> String) -> Result<(), String> {
        if self.last_report.elapsed() >= Duration::from_secs(10) {
            self.last_report = Instant::now();
            let rss = rss_bytes().map_or(String::new(), |b| format!(", {} MB", b >> 20));
            eprintln!("  [{:>4.0}s] {phase}: {}{rss}", self.started.elapsed().as_secs_f64(), progress());
        }
        if self.started.elapsed() > self.limit {
            return Err(format!("planner gave up: {phase} (time limit {}s passed; {})", self.limit.as_secs(), progress()));
        }
        if let Some(b) = rss_bytes() {
            if b > self.max_rss {
                return Err(format!("planner gave up: {phase} (memory {} MB over the {} MB limit; {})", b >> 20, self.max_rss >> 20, progress()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_stops_on_time() {
        let mut b = Budget::new(Duration::from_millis(0), usize::MAX);
        std::thread::sleep(Duration::from_millis(5));
        let e = b.check("test phase", &|| "3/9".into()).unwrap_err();
        assert!(e.starts_with("planner gave up: test phase"), "{e}");
    }

    #[test]
    fn budget_passes_when_within_limits() {
        let mut b = Budget::new(Duration::from_secs(60), usize::MAX);
        assert!(b.check("p", &|| String::new()).is_ok());
    }

    #[test]
    fn budget_stops_on_memory_when_it_can_be_measured() {
        if rss_bytes().is_some() {
            let mut b = Budget::new(Duration::from_secs(60), 1);
            assert!(b.check("p", &|| String::new()).unwrap_err().contains("memory"));
        }
    }
}
