//! What the executor has to keep checking: which plan step owns each cell and which positions
//! still wait for the server's answer. A position is judged ([`crate::audit`]) once the server has
//! acknowledged the actions sent before it was marked, so a late answer is not a wrong one.

use crate::audit::{self, Allowed, Verdict};
use crate::builder::PlayClient;
use crate::exec_repair::Offset;
use crate::extras::Extras;
use crate::plan::{Plan, Step};
use crate::sim::World;
use crate::world::BlockPos;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

/// Judge anyway after this long without an acknowledgement: soon for a server that never sends
/// one, late for one that does (a lag spike must not look like a refused block).
const NO_ACK: Duration = Duration::from_secs(3);
const LATE_ACK: Duration = Duration::from_secs(30);
/// Repairs of one position before it is only reported (something placing cannot fix).
pub const MAX_TRIES: u32 = 3;
/// Changes this far outside the plan's bounding box are other people's business.
const MARGIN: i32 = 2;

pub struct Ledger {
    pub off: Offset,
    /// World position -> index of the latest `PLACE` / `SUPPORT` step for it.
    pub jobs: HashMap<BlockPos, usize>,
    /// Positions this session put a block into: without a report from the server they are missing.
    pub placed: HashSet<BlockPos>,
    pub tries: HashMap<BlockPos, u32>,
    /// What stood beside the plan before the build ([`crate::survey`]): never judged or broken.
    pub keep: HashSet<BlockPos>,
    pending: HashMap<BlockPos, (i32, Instant)>,
    allowed: Allowed,
    lo: (i32, i32, i32),
    hi: (i32, i32, i32),
}

impl Ledger {
    pub fn new(plan: &Plan, off: Offset) -> Self {
        let (mut lo, mut hi) = ((i32::MAX, i32::MAX, i32::MAX), (i32::MIN, i32::MIN, i32::MIN));
        for st in &plan.steps {
            if let Step::Place { pos, .. } | Step::Auto { pos, .. } | Step::Support { pos, .. } = st {
                let p = off.pos(*pos);
                lo = (lo.0.min(p.x), lo.1.min(p.y), lo.2.min(p.z));
                hi = (hi.0.max(p.x), hi.1.max(p.y), hi.2.max(p.z));
            }
        }
        let allowed = audit::allowed(plan);
        Ledger { off, jobs: HashMap::new(), placed: HashSet::new(), tries: HashMap::new(), keep: HashSet::new(), pending: HashMap::new(), allowed, lo, hi }
    }

    /// Remember what step `i` did to the world (also for steps replayed when resuming).
    pub fn own(&mut self, i: usize, st: &Step) -> Option<BlockPos> {
        match st {
            Step::Place { pos, .. } | Step::Support { pos, .. } => {
                self.jobs.insert(self.off.pos(*pos), i);
                Some(self.off.pos(*pos))
            }
            Step::Auto { pos, .. } => Some(self.off.pos(*pos)),
            Step::Pillar((x, y, z)) => {
                let p = self.off.pos(BlockPos::new(*x, *y, *z));
                self.jobs.remove(&p);
                Some(p)
            }
            Step::Break(pos) => {
                let p = self.off.pos(*pos);
                self.jobs.remove(&p);
                self.placed.remove(&p);
                Some(p)
            }
            _ => None,
        }
    }

    /// A step just sent: whatever it touched is checked once the server has answered.
    pub fn sent(&mut self, i: usize, st: &Step, seq: i32) {
        if let Some(p) = self.own(i, st) {
            if !matches!(st, Step::Break(_)) {
                self.placed.insert(p);
            }
            self.pending.insert(p, (seq, Instant::now()));
        }
    }

    /// Queue every position the server reported, asked for or not.
    pub fn collect(&mut self, bot: &mut PlayClient) {
        let now = Instant::now();
        for p in bot.blocks.drain_changed() {
            self.pending.insert(p, (bot.seq, now));
        }
    }

    /// Positions whose answers are complete (removed from the queue).
    pub fn due(&mut self, acked: i32) -> Vec<BlockPos> {
        let limit = if acked == 0 { NO_ACK } else { LATE_ACK };
        let ready: Vec<BlockPos> =
            self.pending.iter().filter(|(_, (seq, since))| acked >= *seq || since.elapsed() > limit).map(|(&p, _)| p).collect();
        for p in &ready {
            self.pending.remove(p);
        }
        ready
    }

    /// Everything that can be judged at all.
    pub fn everything(&self, bot: &PlayClient) -> BTreeSet<(i32, i32, i32)> {
        let reported = bot.blocks.reports().map(|(p, _)| p);
        self.placed.iter().copied().chain(reported).map(|p| (p.y, p.x, p.z)).collect()
    }

    pub fn verdict(&self, bot: &PlayClient, world: &World, extras: &Extras, p: BlockPos) -> Verdict {
        let c = (p.x - self.off.dx, p.y - self.off.dy, p.z - self.off.dz);
        let inside = |v: i32, lo: i32, hi: i32| v >= lo.saturating_sub(MARGIN) && v <= hi.saturating_add(MARGIN);
        let ours = inside(p.x, self.lo.0, self.hi.0) && inside(p.y, self.lo.1, self.hi.1) && inside(p.z, self.lo.2, self.hi.2);
        if c.1 < world.ground_y || !ours || self.keep.contains(&p) || extras.cells.contains(&(p.x, p.y, p.z)) || world.pending.contains_key(&c) {
            return Verdict::Ok;
        }
        let want = audit::expected(world, c);
        match bot.blocks.state(p) {
            Some(id) => audit::judge(want.as_deref(), id, self.allowed.get(&c)),
            None if want.is_some() && self.placed.contains(&p) => Verdict::Wrong("missing".into()),
            None => Verdict::Ok,
        }
    }

    /// Those of `list` that must be repaired, bottom layer first, with the reason.
    pub fn wrong(&self, bot: &PlayClient, world: &World, extras: &Extras, list: impl IntoIterator<Item = BlockPos>) -> Vec<(BlockPos, String)> {
        let mut out: Vec<(BlockPos, String)> = list
            .into_iter()
            .filter(|p| self.tries.get(p).is_none_or(|&n| n < MAX_TRIES))
            .filter_map(|p| match self.verdict(bot, world, extras, p) {
                Verdict::Wrong(why) => Some((p, why)),
                _ => None,
            })
            .collect();
        out.sort_by_key(|(p, _)| (p.y, p.x, p.z));
        out.dedup_by_key(|(p, _)| *p);
        out
    }
}
