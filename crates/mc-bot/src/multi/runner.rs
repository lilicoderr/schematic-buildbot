//! Runs a cooperative plan: one connection per bot, one thread per bot, barriers between the
//! phases, a progress file per part, and a shared stop flag so that when one bot fails every
//! other bot stops at its next packet (the build is then resumed by running it again).

use super::{placed_by, MultiPlan};
use crate::exec::Report;
use crate::exec_repair::Offset;
use crate::version::Version;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

#[path = "runner_bot.rs"]
mod runner_bot;

pub struct Config {
    pub server: String,
    /// One name per bot.
    pub names: Vec<String>,
    pub version: &'static Version,
    pub delay: Duration,
    pub walk: bool,
    pub wait_creative: Duration,
    pub state_dir: PathBuf,
    /// World position of the blueprint's (0,0,0) corner; default: beside where bot 0 joins.
    pub origin: Option<(i32, i32, i32)>,
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub elapsed: Duration,
    /// (phase, bot, report) of every part that ran.
    pub reports: Vec<(usize, usize, Report)>,
    /// First failure of each bot that failed.
    pub errors: Vec<(usize, String)>,
}

impl Outcome {
    pub fn clean(&self) -> bool {
        self.errors.is_empty() && self.reports.iter().all(|(.., r)| r.missing.is_empty() && r.differs.is_empty())
    }
}

/// What every bot thread shares.
pub(crate) struct Shared<'a> {
    pub multi: &'a MultiPlan,
    pub cfg: &'a Config,
    pub stop: Arc<AtomicBool>,
    pub joined: Barrier,
    pub offset_known: Barrier,
    pub placed: Barrier,
    pub phase_end: Barrier,
    pub offset: Mutex<Option<Offset>>,
    pub out: Mutex<Outcome>,
}

impl Shared<'_> {
    pub fn fail(&self, bot: usize, msg: String) {
        self.stop.store(true, Ordering::Relaxed);
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        if !out.errors.iter().any(|(b, _)| *b == bot) {
            out.errors.push((bot, msg));
        }
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn standing(&self, phase: usize) -> Vec<(crate::world::BlockPos, String)> {
        self.multi.phases.iter().take(phase).flatten().flat_map(|p| placed_by(&p.plan)).collect()
    }
}

/// Run all phases. Every bot thread passes every barrier, also after a failure, so none hangs.
pub fn run(multi: &MultiPlan, cfg: &Config) -> Result<Outcome, String> {
    if cfg.names.len() != multi.bots {
        return Err(format!("{} bot names for {} bots", cfg.names.len(), multi.bots));
    }
    let n = multi.bots;
    let shared = Shared {
        multi,
        cfg,
        stop: Arc::new(AtomicBool::new(false)),
        joined: Barrier::new(n),
        offset_known: Barrier::new(n),
        placed: Barrier::new(n),
        phase_end: Barrier::new(n),
        offset: Mutex::new(None),
        out: Mutex::new(Outcome::default()),
    };
    std::fs::create_dir_all(&cfg.state_dir).map_err(|e| format!("{}: {e}", cfg.state_dir.display()))?;
    let started = Instant::now();
    std::thread::scope(|scope| {
        for bot in 0..n {
            let shared = &shared;
            scope.spawn(move || runner_bot::bot_main(shared, bot));
        }
    });
    let mut out = shared.out.into_inner().unwrap_or_else(|e| e.into_inner());
    out.elapsed = started.elapsed();
    out.reports.sort_by_key(|r| (r.0, r.1));
    Ok(out)
}
