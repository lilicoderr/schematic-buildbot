//! Plans the strips of one cut layout (each on its own thread) and the later phase for blocks
//! that no strip can build alone.

use super::{MultiPlan, Part};
use crate::guard::Budget;
use crate::nav::Cell;
use crate::plan::Plan;
use crate::schematic::{Block, Schematic};
use crate::sim::{self, World};
use crate::slice::{self, Setup};
use crate::territory::{Axis, Fence};
use crate::tour::Order;
use crate::world::BlockPos;
use std::collections::HashSet;
use std::time::Duration;

/// Times a strip is planned again after moving blocks it cannot build into the later phase.
const ROUNDS: usize = 5;
const PLAN_TIME: Duration = Duration::from_secs(900);

pub struct Job<'a> {
    pub schem: &'a Schematic,
    pub origin: (i32, i32, i32),
    pub protocol: i32,
}

/// Plan positions the plan leaves out or would build in another state.
fn bad(plan: &Plan) -> HashSet<BlockPos> {
    let fallbacks = plan.header.stats.iter().filter(|(k, _)| k == "fallback_block").filter_map(|(_, v)| {
        let mut n = v.split_whitespace().map(|f| f.parse::<i32>().ok());
        Some(BlockPos::new(n.next()??, n.next()??, n.next()??))
    });
    plan.header.unreachable.iter().map(|u| u.0).chain(fallbacks).collect()
}

impl Job<'_> {
    fn world(&self, b: &Block) -> BlockPos {
        BlockPos::new(self.origin.0 + b.pos.x, self.origin.1 + b.pos.y, self.origin.2 + b.pos.z)
    }

    /// The best plan for `part`: the banded order, or the layered one when that leaves less out.
    fn slice(&self, part: &Schematic, setup: &Setup) -> Result<Plan, String> {
        let run = |kind| {
            let mut budget = Budget::new(PLAN_TIME, usize::MAX);
            slice::slice_setup(part, self.origin, self.protocol, &mut budget, kind, setup)
        };
        let banded = run(Order::BANDED);
        if banded.as_ref().is_ok_and(|p| bad(p).is_empty()) {
            return banded;
        }
        match (banded, run(Order::LAYERED)) {
            (Ok(a), Ok(b)) => Ok(if bad(&b).len() < bad(&a).len() { b } else { a }),
            (Ok(a), Err(_)) => Ok(a),
            (Err(_), b) => b,
        }
    }

    /// One strip: its plan and the blocks it has to leave for the later phase.
    fn strip(&self, mut blocks: Vec<Block>, fence: Fence, start: Cell) -> Result<(Plan, Vec<Block>), String> {
        let mut left = Vec::new();
        for _ in 0..ROUNDS {
            if blocks.is_empty() {
                return Err("a strip has nothing it can build".into());
            }
            let part = Schematic { name: self.schem.name.clone(), blocks: blocks.clone() };
            let plan = self.slice(&part, &Setup { start: Some(start), fence, prebuilt: Vec::new() })?;
            let mut out = bad(&plan);
            if out.is_empty() {
                return Ok((plan, left));
            }
            // a second half goes where its first half goes
            let halves: Vec<BlockPos> = blocks
                .iter()
                .filter(|b| out.contains(&self.world(b)))
                .filter_map(|b| crate::sim_rules::second_half(&b.name, &b.props, self.world(b)))
                .collect();
            out.extend(halves);
            let (gone, kept): (Vec<Block>, Vec<Block>) = blocks.into_iter().partition(|b| out.contains(&self.world(b)));
            left.extend(gone);
            blocks = kept;
        }
        Err("a strip still leaves blocks out after several tries".into())
    }

    /// A start cell on the ground just outside the footprint, in the middle of the strip.
    fn start(&self, fence: Fence, blocks: &[Block]) -> Cell {
        let (min, _) = self.schem.bounds();
        let values = blocks.iter().map(|b| super::along(fence.axis, self.world(b)));
        let mid = (values.clone().min().unwrap_or(0) + values.max().unwrap_or(0)) / 2;
        match fence.axis {
            Axis::X => (mid, self.origin.1, self.origin.2 + min.z - 2),
            Axis::Z => (self.origin.0 + min.x - 2, self.origin.1, mid),
        }
    }

    pub fn plan(&self, fences: &[Fence]) -> Result<MultiPlan, String> {
        let shares: Vec<(Fence, Vec<Block>)> = fences
            .iter()
            .map(|&f| {
                let mine = self.schem.blocks.iter().filter(|b| f.has(self.world(b).x, self.world(b).z));
                (f, mine.cloned().collect())
            })
            .collect();
        let version = crate::version::active();
        let planned: Vec<Result<(Plan, Vec<Block>), String>> = std::thread::scope(|scope| {
            let threads: Vec<_> = shares
                .iter()
                .map(|(f, blocks)| {
                    let start = self.start(*f, blocks);
                    scope.spawn(move || {
                        crate::version::set_active(version);
                        self.strip(blocks.clone(), *f, start)
                    })
                })
                .collect();
            threads.into_iter().map(|t| t.join().unwrap_or_else(|_| Err("planner crashed".into()))).collect()
        });
        let (mut parts, mut left) = (Vec::new(), Vec::new());
        for (bot, result) in planned.into_iter().enumerate() {
            let (plan, rest) = result.map_err(|e| format!("strip {bot}: {e}"))?;
            parts.push(Part { bot, fence: fences[bot], plan });
            left.extend(rest);
        }
        let mut multi = MultiPlan { bots: fences.len(), phases: vec![parts] };
        if !left.is_empty() {
            let last = self.later(&multi, left)?;
            multi.phases.push(vec![last]);
        }
        Ok(multi)
    }

    /// Bot 0 builds what is left, alone, from where its strip plan ended.
    fn later(&self, so_far: &MultiPlan, left: Vec<Block>) -> Result<Part, String> {
        let prebuilt: Vec<(BlockPos, String)> = so_far.standing_before(1);
        let first = &so_far.phases[0][0].plan;
        let mut w = World::new(self.origin.1, first.header.start);
        for st in &first.steps {
            // checkpoints count only the whole plan's own blocks, which this world holds
            sim::apply(&mut w, st).map_err(|e| format!("replaying bot 0: {e}"))?;
        }
        let count = left.len();
        let part = Schematic { name: self.schem.name.clone(), blocks: left };
        let plan = self.slice(&part, &Setup { start: Some(w.feet), fence: Fence::OPEN, prebuilt })?;
        match bad(&plan).len() {
            0 => Ok(Part { bot: 0, fence: Fence::OPEN, plan }),
            n => Err(format!("later phase ({count} blocks): {n} cannot be built exactly")),
        }
    }
}
