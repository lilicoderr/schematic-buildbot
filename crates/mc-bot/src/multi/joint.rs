//! The joint simulator: every plan of a cooperative build replayed in one shared world with
//! one player per bot, under several interleavings of the bots' steps.
//!
//! The argument for "any speed is safe" is the static rule checked first: the parts of a phase
//! have disjoint strips and no step reads or changes a column outside its own strip (feet,
//! head, jumps and drops are all in the columns of the cells moved between). The replays then
//! check it again with the server rules of [`crate::sim`] plus the multi-player ones.

use super::{MultiPlan, Part};
use crate::nav::Cell;
use crate::plan::{block_hash, Step};
use crate::schematic::{Block, Schematic};
use crate::sim::{self, World};
use crate::sim_rules::{self as rules, in_body};
use crate::world::BlockPos;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Default, PartialEq)]
pub struct JointReport {
    pub placed: usize,
    pub scaffold: usize,
    pub interleavings: usize,
    /// Neighbour-decided properties that will differ (the same in every interleaving).
    pub neighbour_mismatches: usize,
}

/// Which bot moves next.
#[derive(Debug, Clone, Copy)]
enum Schedule {
    /// By estimated step times, like the real run.
    Timed,
    /// One bot after another, in this direction.
    InTurn { reverse: bool },
    /// Random runs of steps.
    Random(u64),
}

const SCHEDULES: [Schedule; 6] = [
    Schedule::Timed,
    Schedule::InTurn { reverse: false },
    Schedule::InTurn { reverse: true },
    Schedule::Random(1),
    Schedule::Random(0x9e37_79b9),
    Schedule::Random(0xdead_beef),
];

/// Every column a step uses must be in the bot's strip (clicks on the ground are free).
fn in_strip(part: &Part, ground: i32, st: &Step) -> Result<(), String> {
    let own = |x: i32, y: i32, z: i32| {
        if y < ground || part.fence.has(x, z) {
            Ok(())
        } else {
            Err(format!("bot {} uses ({x}, {y}, {z}) outside its strip", part.bot))
        }
    };
    match st {
        Step::Move(c) | Step::Pillar(c) => own(c.0, c.1, c.2),
        Step::Break(p) => own(p.x, p.y, p.z),
        Step::Auto { pos, .. } => own(pos.x, pos.y, pos.z),
        Step::Support { pos, clicks } | Step::Place { pos, clicks, .. } => {
            own(pos.x, pos.y, pos.z)?;
            if let Step::Place { state, .. } = st {
                let (name, props) = crate::schematic::split_state(state);
                if let Some(o) = rules::second_half(&name, &props, *pos) {
                    own(o.x, o.y, o.z)?;
                }
            }
            clicks.iter().try_for_each(|c| own(c.support.x, c.support.y, c.support.z))
        }
        Step::Checkpoint { .. } => Ok(()),
    }
}

fn static_rules(multi: &MultiPlan, ground: i32) -> Result<(), String> {
    for (k, phase) in multi.phases.iter().enumerate() {
        for (i, a) in phase.iter().enumerate() {
            if a.bot >= multi.bots || phase[..i].iter().any(|b| b.bot == a.bot || !b.fence.disjoint(&a.fence)) {
                return Err(format!("phase {}: the bots' strips overlap or a bot is listed twice", k + 1));
            }
            if !a.fence.has(a.plan.header.start.0, a.plan.header.start.2) || a.plan.header.origin.1 != ground {
                return Err(format!("phase {}: bot {} does not start in its strip on the same ground", k + 1, a.bot));
            }
            for (n, st) in a.plan.steps.iter().enumerate() {
                in_strip(a, ground, st).map_err(|e| format!("phase {} step {}: {e}", k + 1, n + 1))?;
            }
        }
    }
    Ok(())
}

/// The shared world during one replay.
struct Shared {
    world: World,
    /// Which bot put each scaffold block there.
    owner: HashMap<Cell, usize>,
    /// Placements in the order they happened.
    order: Vec<Block>,
    placed: usize,
    scaffold: usize,
}

/// One bot inside a phase.
struct Agent<'a> {
    part: &'a Part,
    feet: Cell,
    next: usize,
    /// What this plan's checkpoints must count: the blocks standing at its start plus its own.
    count: u32,
    hash: u64,
    /// Estimated time of the next step's start, in hundredths of a second.
    clock: u64,
}

fn duration(feet: Cell, st: &Step) -> u64 {
    match st {
        Step::Move(c) => u64::from(crate::shorten::move_cost(feet, *c)),
        Step::Pillar(_) | Step::Break(_) | Step::Support { .. } => 35,
        Step::Place { .. } => 5,
        _ => 0,
    }
}

impl Shared {
    /// One step of agent `i`, with the other agents standing in the world.
    fn step(&mut self, agents: &mut [Agent], i: usize) -> Result<(), String> {
        let (bot, st) = (agents[i].part.bot, &agents[i].part.plan.steps[agents[i].next]);
        let others: Vec<(usize, Cell)> = agents.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, a)| (a.part.bot, a.feet)).collect();
        let clear = |cell: Cell| match others.iter().find(|(_, f)| in_body(*f, cell)) {
            Some((b, _)) => Err(format!("{cell:?} is inside bot {b}")),
            None => Ok(()),
        };
        match st {
            Step::Move(c) | Step::Pillar(c) => {
                clear(*c)?;
                clear((c.0, c.1 + 1, c.2))?;
            }
            Step::Place { pos, state, .. } => {
                clear((pos.x, pos.y, pos.z))?;
                let (name, props) = crate::schematic::split_state(state);
                if let Some(o) = rules::second_half(&name, &props, *pos) {
                    clear((o.x, o.y, o.z))?;
                }
            }
            Step::Support { pos, .. } => clear((pos.x, pos.y, pos.z))?,
            Step::Break(p) if self.owner.get(&(p.x, p.y, p.z)) != Some(&bot) => {
                return Err(format!("breaks scaffold at {p:?} it does not own"));
            }
            _ => {}
        }
        if let Step::Checkpoint { count, hash, .. } = st {
            if (*count, *hash) != (agents[i].count, agents[i].hash) {
                return Err("checkpoint differs from what the bot has built".into());
            }
        } else {
            self.world.feet = agents[i].feet;
            sim::merge_check(&self.world, st)?;
            sim::apply(&mut self.world, st)?;
            agents[i].clock += duration(agents[i].feet, st);
            agents[i].feet = self.world.feet;
        }
        match st {
            Step::Pillar(c) => {
                self.owner.insert(*c, bot);
                self.scaffold += 1;
            }
            Step::Support { pos, .. } => {
                self.owner.insert((pos.x, pos.y, pos.z), bot);
                self.scaffold += 1;
            }
            Step::Break(p) => {
                self.owner.remove(&(p.x, p.y, p.z));
                let dropped = others.iter().find(|(_, f)| !self.world.carries((f.0, f.1 - 1, f.2)));
                if let Some((b, _)) = dropped {
                    return Err(format!("breaking {p:?} takes away what bot {b} stands on"));
                }
            }
            Step::Place { pos, state, .. } | Step::Auto { pos, state } => {
                self.order.push(Block::from_state(*pos, state));
                self.placed += usize::from(matches!(st, Step::Place { .. }));
                agents[i].count += 1;
                agents[i].hash = agents[i].hash.wrapping_add(block_hash(*pos, state));
            }
            _ => {}
        }
        agents[i].next += 1;
        Ok(())
    }
}

/// Replay all phases under one schedule; returns the world for the end checks.
fn replay(multi: &MultiPlan, ground: i32, schedule: Schedule) -> Result<Shared, String> {
    let world = World::new(ground, (0, ground, 0));
    let mut shared = Shared { world, owner: HashMap::new(), order: Vec::new(), placed: 0, scaffold: 0 };
    let mut rng = match schedule {
        Schedule::Random(seed) => seed,
        _ => 0,
    };
    for (k, phase) in multi.phases.iter().enumerate() {
        let standing = shared.order.clone();
        let base = standing.iter().fold(0u64, |h, b| h.wrapping_add(block_hash(b.pos, &b.state_string())));
        let mut agents: Vec<Agent> = phase
            .iter()
            .map(|part| Agent { part, feet: part.plan.header.start, next: 0, count: standing.len() as u32, hash: base, clock: 0 })
            .collect();
        for a in &agents {
            if !shared.world.standable(a.feet) {
                return Err(format!("phase {}: bot {} cannot stand at its start {:?}", k + 1, a.part.bot, a.feet));
            }
        }
        let mut run = (0usize, 0u64);
        loop {
            let live: Vec<usize> = (0..agents.len()).filter(|&i| agents[i].next < agents[i].part.plan.steps.len()).collect();
            let Some(&first) = live.first() else { break };
            let i = match schedule {
                Schedule::Timed => *live.iter().min_by_key(|&&i| agents[i].clock).unwrap_or(&first),
                Schedule::InTurn { reverse: false } => first,
                Schedule::InTurn { reverse: true } => *live.last().unwrap_or(&first),
                Schedule::Random(_) => {
                    if run.1 == 0 || !live.contains(&run.0) {
                        rng = rng.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                        run = (live[(rng >> 33) as usize % live.len()], 1 + (rng >> 20) % 400);
                    }
                    run.1 -= 1;
                    run.0
                }
            };
            let n = agents[i].next;
            shared.step(&mut agents, i).map_err(|e| {
                format!("phase {} bot {} step {} ({}), {schedule:?}: {e}", k + 1, agents[i].part.bot, n + 1, agents[i].part.plan.steps[n])
            })?;
        }
    }
    Ok(shared)
}

fn finished(shared: &Shared, schem: &Schematic, origin: (i32, i32, i32)) -> Result<(), String> {
    let w = &shared.world;
    if !w.scaffold.is_empty() || !w.pending.is_empty() {
        return Err(format!("{} scaffold block(s) and {} second half(s) are left over", w.scaffold.len(), w.pending.len()));
    }
    if w.mismatches > 0 {
        return Err(format!("{} placement(s) would get another state than requested", w.mismatches));
    }
    let mut wanted = HashSet::new();
    for b in &schem.blocks {
        let c = (origin.0 + b.pos.x, origin.1 + b.pos.y, origin.2 + b.pos.z);
        if w.blocks.get(&c) != Some(&b.state_string()) {
            return Err(format!("final world differs at {c:?}: {:?} vs {}", w.blocks.get(&c), b.state_string()));
        }
        wanted.insert(c);
    }
    match w.blocks.len() == wanted.len() {
        true => Ok(()),
        false => Err("the final world has blocks the schematic does not".into()),
    }
}

/// Check a cooperative plan: strips disjoint, every step legal in the shared world under every
/// schedule tried, all scaffold gone and the result exactly the schematic.
pub fn check(multi: &MultiPlan, schem: &Schematic, origin: (i32, i32, i32)) -> Result<JointReport, String> {
    static_rules(multi, origin.1)?;
    let mut report = JointReport::default();
    let mut first: Option<Vec<(BlockPos, String)>> = None;
    for schedule in SCHEDULES {
        let shared = replay(multi, origin.1, schedule)?;
        finished(&shared, schem, origin).map_err(|e| format!("{schedule:?}: {e}"))?;
        let mut wrong: Vec<(BlockPos, String)> = crate::predict::mismatches(&shared.order, origin.1).into_iter().map(|m| (m.pos, m.prop)).collect();
        wrong.sort_by_key(|(p, prop)| (p.x, p.y, p.z, prop.clone()));
        if first.as_ref().is_some_and(|f| *f != wrong) {
            return Err(format!("{schedule:?}: the final block states depend on how fast the bots are"));
        }
        report = JointReport { placed: shared.placed, scaffold: shared.scaffold, interleavings: report.interleavings + 1, neighbour_mismatches: wrong.len() };
        first = Some(wrong);
    }
    Ok(report)
}
