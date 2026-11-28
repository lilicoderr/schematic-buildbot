//! Cooperative building with several bots.
//!
//! The schematic is cut into strips, one per bot ([`partition`]). Inside a phase every bot
//! stands, places and puts scaffold only in its own strip ([`crate::territory::Fence`]), so the
//! phase is legal at any relative speed of the bots, and each bot owns and removes its own
//! scaffold. Blocks no strip can build alone (something they need is across a cut) form a later
//! phase, which starts only after a barrier: every earlier plan has finished. [`joint`] checks
//! the whole thing in one shared world; [`runner`] runs it.

pub mod files;
pub mod joint;
pub mod partition;
pub mod runner;

use crate::plan::{Plan, Step};
use crate::territory::Fence;
use crate::world::BlockPos;

pub const MAX_BOTS: usize = 4;

/// One bot's share of one phase.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub bot: usize,
    pub fence: Fence,
    pub plan: Plan,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MultiPlan {
    pub bots: usize,
    /// Run one after another; the parts of one phase run at the same time.
    pub phases: Vec<Vec<Part>>,
}

/// The schematic blocks a plan leaves standing, in plan order.
pub fn placed_by(plan: &Plan) -> Vec<(BlockPos, String)> {
    plan.steps
        .iter()
        .filter_map(|s| match s {
            Step::Place { pos, state, .. } | Step::Auto { pos, state } => Some((*pos, state.clone())),
            _ => None,
        })
        .collect()
}

/// A plan header's estimated run time in seconds.
pub fn est_seconds(plan: &Plan) -> f64 {
    let stat = plan.header.stats.iter().find(|(k, _)| k == "est_seconds");
    stat.and_then(|(_, v)| v.parse().ok()).unwrap_or(0.0)
}

impl MultiPlan {
    /// Blocks standing when phase `k` starts.
    pub fn standing_before(&self, k: usize) -> Vec<(BlockPos, String)> {
        self.phases.iter().take(k).flatten().flat_map(|p| placed_by(&p.plan)).collect()
    }

    /// Estimated run time: each phase takes as long as its slowest part.
    pub fn est_seconds(&self) -> f64 {
        self.phases.iter().map(|ph| ph.iter().map(|p| est_seconds(&p.plan)).fold(0.0, f64::max)).sum()
    }
}
