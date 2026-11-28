//! What the page shows about the running (or last) build, fed with the output lines of the
//! build process.

use super::jsonw;
use mc_bot::ui_link::PREFIX;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const LOG_LINES: usize = 1000;
const LINE_CHARS: usize = 500;
/// The blocks/s figure looks at this much of the recent past.
const WINDOW: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Idle,
    /// Reading, planning, connecting: nothing has been built in this run yet.
    Starting,
    Building,
    /// A safe stop was asked for.
    Stopping,
    Done,
    Stopped,
    Failed,
}

impl Phase {
    pub fn active(self) -> bool {
        matches!(self, Phase::Starting | Phase::Building | Phase::Stopping)
    }

    fn name(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Starting => "starting",
            Phase::Building => "building",
            Phase::Stopping => "stopping",
            Phase::Done => "done",
            Phase::Stopped => "stopped",
            Phase::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Counts {
    pub done: u64,
    pub steps: u64,
    pub placed: u64,
    pub places: u64,
}

pub struct Job {
    pub phase: Phase,
    pub counts: Counts,
    pub repairs: u64,
    log: VecDeque<String>,
    /// Number of the first line still in `log`.
    dropped: u64,
    /// When the first step count arrived, and the counts then (a resumed build starts higher).
    first: Option<(Instant, Counts)>,
    /// Recent (time, placements).
    recent: VecDeque<(Instant, u64)>,
}

impl Default for Job {
    fn default() -> Self {
        Job { phase: Phase::Idle, counts: Counts::default(), repairs: 0, log: VecDeque::new(), dropped: 0, first: None, recent: VecDeque::new() }
    }
}

impl Job {
    pub fn start(&mut self) {
        // log numbers keep counting up, so a page that is polling does not miss lines
        let dropped = self.dropped + self.log.len() as u64;
        *self = Job { phase: Phase::Starting, dropped, ..Job::default() };
    }

    /// One line of the build process (standard output or error).
    pub fn line(&mut self, line: &str, now: Instant) {
        if let Some(rest) = line.strip_prefix(PREFIX) {
            let n: Vec<u64> = rest.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
            if let (true, &[done, steps, placed, places]) = (rest.starts_with("progress "), n.as_slice()) {
                self.progress(Counts { done, steps, placed, places }, now);
            }
            return;
        }
        if line.trim_start().starts_with("repair: ") {
            self.repairs += 1;
        }
        self.say(line);
    }

    pub fn say(&mut self, line: &str) {
        self.log.push_back(line.chars().filter(|c| !c.is_control()).take(LINE_CHARS).collect());
        if self.log.len() > LOG_LINES {
            self.log.pop_front();
            self.dropped += 1;
        }
    }

    fn progress(&mut self, counts: Counts, now: Instant) {
        if self.phase == Phase::Starting {
            self.phase = Phase::Building;
        }
        self.counts = counts;
        self.first.get_or_insert((now, counts));
        self.recent.push_back((now, counts.placed));
        // keep one sample older than the window, to measure from
        while self.recent.len() > 2 && now.duration_since(self.recent[1].0) > WINDOW {
            self.recent.pop_front();
        }
    }

    /// Placements per second over the recent window; falls towards zero while nothing happens.
    pub fn rate(&self, now: Instant) -> f64 {
        let Some(&(t, placed)) = self.recent.front() else { return 0.0 };
        let secs = now.duration_since(t).as_secs_f64();
        if secs < 1.0 { 0.0 } else { self.counts.placed.saturating_sub(placed) as f64 / secs }
    }

    /// Seconds left, from the average pace of steps in this run (walking steps take time too).
    pub fn eta(&self, now: Instant) -> Option<u64> {
        let (t, first) = self.first?;
        let (secs, ran) = (now.duration_since(t).as_secs_f64(), self.counts.done.saturating_sub(first.done));
        (secs >= 3.0 && ran > 0).then(|| ((self.counts.steps.saturating_sub(self.counts.done)) as f64 * secs / ran as f64).ceil() as u64)
    }

    /// The status answer, with the log lines from number `since` on.
    pub fn json(&self, since: u64, now: Instant) -> String {
        let c = self.counts;
        let building = matches!(self.phase, Phase::Building | Phase::Stopping);
        let skip = since.saturating_sub(self.dropped).min(self.log.len() as u64) as usize;
        let percent = if c.steps == 0 { 0.0 } else { 100.0 * c.done as f64 / c.steps as f64 };
        jsonw::object(&[
            ("phase", jsonw::text(self.phase.name())),
            // never "100%" before the last step
            ("percent", format!("{:.1}", (percent * 10.0).floor() / 10.0)),
            ("step", c.done.to_string()),
            ("steps", c.steps.to_string()),
            ("placed", c.placed.to_string()),
            ("places", c.places.to_string()),
            ("blocks_per_s", format!("{:.1}", if building { self.rate(now) } else { 0.0 })),
            ("eta_s", self.eta(now).filter(|_| building).map_or("null".into(), |s| s.to_string())),
            ("repairs", self.repairs.to_string()),
            ("log", jsonw::array(self.log.iter().skip(skip).map(|l| jsonw::text(l)))),
            ("next", (self.dropped + self.log.len() as u64).to_string()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines_drive_phase_rate_and_eta() {
        let (mut j, t) = (Job::default(), Instant::now());
        j.start();
        j.line("planning 10 blocks...", t);
        assert_eq!(j.phase, Phase::Starting);
        j.line("@bb progress 100 1100 40 440", t);
        j.line("  repair: BlockPos { x: 1 }: missing", t);
        j.line("@bb progress 600 1100 140 440", t + Duration::from_secs(10));
        assert_eq!((j.phase, j.repairs, j.counts.placed), (Phase::Building, 1, 140));
        assert_eq!(j.rate(t + Duration::from_secs(10)), 10.0);
        assert_eq!(j.eta(t + Duration::from_secs(10)), Some(10));
        assert!(j.rate(t + Duration::from_secs(100)) < 2.0, "a stall shows");
        let json = j.json(1, t);
        assert!(json.contains("\"percent\":54.5") && json.contains("\"next\":2") && !json.contains("planning"), "{json}");
    }

    #[test]
    fn the_log_is_bounded_and_numbered_across_runs() {
        let mut j = Job::default();
        for i in 0..LOG_LINES + 5 {
            j.say(&format!("l{i}\u{1b}"));
        }
        assert!(j.json(0, Instant::now()).contains("\"l5\""));
        j.start();
        j.say("again");
        assert!(j.json(LOG_LINES as u64 + 5, Instant::now()).contains("[\"again\"]"));
    }
}
