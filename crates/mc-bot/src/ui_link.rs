//! The link between `build-bot ui` and the `build-bot build` process it starts. The UI sets
//! [`ENV`]; the build then prints machine-readable progress lines and listens on its standard
//! input: `stop` (or the end of input, when the UI is gone) asks for a safe stop. Without the
//! variable nothing here does anything.

use crate::plan::{Plan, Step};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Environment variable that turns the link on.
pub const ENV: &str = "BUILDBOT_UI";
/// Every progress line starts with this.
pub const PREFIX: &str = "@bb ";
/// Exit code of a build stopped before its first step (nothing was in progress).
pub const STOPPED_EARLY: i32 = 3;

static STOP: AtomicBool = AtomicBool::new(false);
static BUILDING: AtomicBool = AtomicBool::new(false);

/// `true` once a safe stop was asked for; the executor looks between steps.
pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

fn enabled() -> bool {
    std::env::var_os(ENV).is_some_and(|v| v == "1")
}

/// In a child of the UI: watch standard input for the stop request.
pub fn listen() {
    if !enabled() {
        return;
    }
    std::thread::spawn(|| {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(l) if l.trim() == "stop" => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        STOP.store(true, Ordering::SeqCst);
        // still downloading, planning, connecting or waiting: no step is half done
        if !BUILDING.load(Ordering::SeqCst) {
            std::process::exit(STOPPED_EARLY);
        }
    });
}

/// Progress of one run: `@bb progress <steps> <all steps> <placed> <all placements>`.
pub struct Reporter {
    /// `placed[n]`: placements among the first `n` steps.
    placed: Vec<u32>,
    last: Option<Instant>,
}

impl Reporter {
    /// Call right before the first step; from here a stop waits for a safe point.
    pub fn start(plan: &Plan, from_step: usize) -> Reporter {
        let mut r = Reporter { placed: Vec::new(), last: None };
        if enabled() {
            BUILDING.store(true, Ordering::SeqCst);
            let mut n = 0;
            r.placed.push(0);
            for s in &plan.steps {
                n += u32::from(matches!(s, Step::Place { .. }));
                r.placed.push(n);
            }
            r.step(from_step);
        }
        r
    }

    fn line(&self, done: usize) -> Option<String> {
        let total = self.placed.len().checked_sub(1)?;
        let done = done.min(total);
        Some(format!("{PREFIX}progress {done} {total} {} {}", self.placed[done], self.placed[total]))
    }

    /// `done` steps have run. Printed at most four times a second, and always for the last step.
    pub fn step(&mut self, done: usize) {
        let all = self.placed.len().saturating_sub(1);
        if done < all && self.last.is_some_and(|t| t.elapsed() < Duration::from_millis(250)) {
            return;
        }
        if let Some(line) = self.line(done) {
            self.last = Some(Instant::now());
            let mut out = std::io::stdout().lock();
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_count_placements_up_to_the_step() {
        let r = Reporter { placed: vec![0, 0, 1, 2], last: None };
        assert_eq!(r.line(2).unwrap(), "@bb progress 2 3 1 2");
        assert_eq!(r.line(99).unwrap(), "@bb progress 3 3 2 2");
        assert_eq!(Reporter { placed: Vec::new(), last: None }.line(0), None);
    }
}
