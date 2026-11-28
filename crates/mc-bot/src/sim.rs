//! Plan simulator: replays a plan against an in-memory world and checks every step is legal.
//! Its rules ([`crate::sim_rules`], from docs/RE/*.md) share no code with the planner, so a
//! slicer bug cannot hide itself.

use crate::nav::Cell;
use crate::plan::{block_hash, ClickStep, Plan, Step};
use crate::schematic::{split_state, Schematic};
use crate::sim_rules::{self as rules, eye_dist, in_body, MAX_CLICK, MAX_TARGET};
use crate::world::{BlockPos, Face};
use std::collections::{HashMap, HashSet};

pub const MAX_DROP: i32 = 3;
/// Block put by `PILLAR`; the only thing `BREAK` may remove.
pub const SCAFFOLD: &str = "cobblestone";

#[derive(Debug, PartialEq)]
pub struct SimError {
    /// 0-based index of the offending step (steps.len() for end-of-plan checks).
    pub step: usize,
    pub msg: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct SimReport {
    pub placed: usize,
    pub moves: usize,
    pub pillars: usize,
    pub supports: usize,
    pub breaks: usize,
    pub checkpoints: usize,
}

pub struct World {
    pub ground_y: i32,
    pub blocks: HashMap<Cell, String>,
    pub scaffold: HashSet<Cell>,
    /// Second halves (door top, bed head) the server has created and no `AUTO` has named yet.
    pub pending: HashMap<Cell, String>,
    /// Placements whose clicks would not give the requested state.
    pub mismatches: usize,
    pub feet: Cell,
    /// Blocks an undo may break besides scaffold: the ones the bot itself placed earlier.
    pub demolish: HashSet<Cell>,
}

impl World {
    pub fn new(ground_y: i32, start: Cell) -> Self {
        World { ground_y, blocks: HashMap::new(), scaffold: HashSet::new(), pending: HashMap::new(), mismatches: 0, feet: start, demolish: HashSet::new() }
    }

    pub fn solid(&self, c: Cell) -> bool {
        c.1 < self.ground_y || self.blocks.contains_key(&c) || self.pending.contains_key(&c)
    }

    /// The block top at `c` can carry a player standing above it.
    pub fn carries(&self, c: Cell) -> bool {
        if c.1 < self.ground_y {
            return true;
        }
        self.blocks.get(&c).is_some_and(|s| {
            let (name, props) = split_state(s);
            rules::full_top(&name, &props)
        })
    }

    pub fn standable(&self, c: Cell) -> bool {
        self.carries((c.0, c.1 - 1, c.2)) && !self.solid(c) && !self.solid((c.0, c.1 + 1, c.2))
    }

    /// Schematic blocks currently standing: (count, order-independent hash).
    pub fn built(&self) -> (u32, u64) {
        let mut n = 0;
        let mut h = 0u64;
        for (c, s) in &self.blocks {
            if !self.scaffold.contains(c) {
                n += 1;
                h = h.wrapping_add(block_hash(BlockPos::new(c.0, c.1, c.2), s));
            }
        }
        (n, h)
    }
}

fn cell(p: BlockPos) -> Cell {
    (p.x, p.y, p.z)
}

fn move_to(w: &mut World, to: Cell) -> Result<(), String> {
    let from = w.feet;
    if (to.0 - from.0).abs() + (to.2 - from.2).abs() != 1 {
        return Err(format!("{to:?} is not one horizontal step from {from:?}"));
    }
    let dy = to.1 - from.1;
    if dy == 1 {
        if w.solid((from.0, from.1 + 2, from.2)) {
            return Err("no head room above the start cell to jump".into());
        }
    } else if (-MAX_DROP..0).contains(&dy) {
        if (to.1..=from.1 + 1).any(|y| w.solid((to.0, y, to.2))) {
            return Err("drop column is not free".into());
        }
    } else if dy != 0 {
        return Err(format!("height change {dy} is neither a jump up 1 nor a drop of at most {MAX_DROP}"));
    }
    if !w.standable(to) {
        return Err(format!("{to:?} is not standable (support below, free feet and head)"));
    }
    w.feet = to;
    Ok(())
}

fn fall_after_break(w: &mut World) -> Result<(), String> {
    let start = w.feet.1;
    while !w.carries((w.feet.0, w.feet.1 - 1, w.feet.2)) {
        if w.solid((w.feet.0, w.feet.1 - 1, w.feet.2)) {
            return Err("would come to rest on a block without a full top".into());
        }
        w.feet.1 -= 1;
        if start - w.feet.1 > MAX_DROP {
            return Err("fell more than 3 blocks".into());
        }
    }
    Ok(())
}

fn usable_at(w: &World, c: Cell) -> bool {
    w.blocks.get(&c).or_else(|| w.pending.get(&c)).is_some_and(|s| rules::consumes_click(&split_state(s).0))
}

/// One click without sneaking: real block, harmless to click, in reach, cursor on the block.
/// `merging` is the second click of a double slab, which lands on the block just placed.
fn click_ok(w: &World, pos: BlockPos, c: &ClickStep, merging: bool) -> Result<(), String> {
    let support = c.support;
    if !(merging && support == pos) {
        if !w.solid(cell(support)) {
            return Err(format!("click support {support:?} is air"));
        }
        if usable_at(w, cell(support)) {
            return Err(format!("click support {support:?} is a usable block (a plain click would open it)"));
        }
        if merging || support.offset(c.face) != pos {
            return Err(format!("clicking {:?} of {support:?} does not lead to {pos:?}", c.face));
        }
    }
    let (x, y, z) = c.cursor;
    if ![x, y, z].iter().all(|v| (0.0..=1.0).contains(v)) {
        return Err(format!("cursor {:?} is not on the block", c.cursor));
    }
    if eye_dist(w.feet, support) > MAX_CLICK {
        return Err(format!("click support {support:?} out of reach"));
    }
    Ok(())
}

/// A cell that can receive a block from where the player stands.
fn receivable(w: &World, pos: BlockPos) -> Result<(), String> {
    if w.solid(cell(pos)) {
        return Err(format!("{pos:?} is not free"));
    }
    if in_body(w.feet, cell(pos)) {
        return Err("placing into the player's own cells".into());
    }
    Ok(())
}

fn place(w: &mut World, pos: BlockPos, state: &str, yaw: Option<f32>, clicks: &[ClickStep]) -> Result<(), String> {
    let (name, props) = split_state(state);
    let is = |k: &str, v: &str| props.get(k).is_some_and(|x| x == v);
    let other = rules::second_half(&name, &props, pos);
    if other.is_some() && (is("half", "upper") || is("part", "head")) {
        return Err("a second half appears by itself and cannot be placed".into());
    }
    receivable(w, pos)?;
    if eye_dist(w.feet, pos) > MAX_TARGET {
        return Err(format!("{pos:?} out of reach from {:?}", w.feet));
    }
    let wanted = rules::clicks_needed(&name, &props);
    if clicks.len() != wanted {
        return Err(format!("{} click(s) given, {wanted} needed", clicks.len()));
    }
    let first = &clicks[0];
    click_ok(w, pos, first, false)?;
    // later clicks add to the block the first one created (double slab, candles, snow layers ...)
    let lower = first.face == Face::Up || (first.face != Face::Down && first.cursor.1 <= 0.5);
    let onto = if name.ends_with("_slab") && !lower { Face::Down } else { Face::Up };
    for next in &clicks[1..] {
        click_ok(w, pos, next, true)?;
        if next.support != pos || next.face != onto {
            return Err("a repeat click does not merge into the placed block".into());
        }
    }
    if let Some(s) = rules::needs_support(&name, &props, pos) {
        if !w.solid(cell(s)) {
            return Err(format!("{state} cannot stay without a block at {s:?}"));
        }
    }
    if let Some(o) = other {
        receivable(w, o).map_err(|e| format!("second half: {e}"))?;
    }
    // no yaw in the plan: the player looks at the middle of the target
    let yaw = yaw.unwrap_or_else(|| {
        let d = ((pos.x - w.feet.0) as f64, (pos.z - w.feet.2) as f64);
        (-d.0.atan2(d.1)).to_degrees() as f32
    });
    if !yaw.is_finite() {
        return Err("yaw is not a number".into());
    }
    if !name.ends_with("_slab") || wanted == 1 {
        let (support, face) = (first.support, first.face);
        w.mismatches += usize::from(rules::derived_mismatch(&name, &props, pos, support, face, first.cursor.1, yaw));
    }
    w.blocks.insert(cell(pos), state.to_string());
    if let Some(o) = other {
        w.pending.insert(cell(o), state.to_string());
    }
    Ok(())
}

/// Apply one step to the world model (legality checked); the executor uses it too.
pub fn apply(w: &mut World, st: &Step) -> Result<(), String> {
    match st {
        Step::Move(to) => move_to(w, *to),
        Step::Pillar(c) => {
            if *c != w.feet {
                return Err("PILLAR must be at the feet cell".into());
            }
            if !w.carries((c.0, c.1 - 1, c.2)) || w.solid(*c) || w.solid((c.0, c.1 + 2, c.2)) {
                return Err("pillar needs a floor to click and head room".into());
            }
            if usable_at(w, (c.0, c.1 - 1, c.2)) {
                return Err("pillar would click a usable block".into());
            }
            w.blocks.insert(*c, SCAFFOLD.into());
            w.scaffold.insert(*c);
            w.feet.1 += 1;
            Ok(())
        }
        Step::Place { pos, state, yaw, clicks } => place(w, *pos, state, *yaw, clicks),
        Step::Support { pos, clicks } => {
            place(w, *pos, SCAFFOLD, None, clicks)?;
            w.scaffold.insert(cell(*pos));
            Ok(())
        }
        Step::Auto { pos, state } => {
            let (name, props) = split_state(state);
            let from_lower = w.pending.get(&cell(*pos)).is_some_and(|lower| {
                let (n, p) = split_state(lower);
                n == name && props.get("facing").zip(p.get("facing")).is_none_or(|(a, b)| a == b)
            });
            let wrong_half = props.get("half").is_some_and(|h| h != "upper") || props.get("part").is_some_and(|p| p != "head");
            if !from_lower || wrong_half {
                return Err("AUTO is only the second half a previous PLACE created in this cell".into());
            }
            w.pending.remove(&cell(*pos));
            w.blocks.insert(cell(*pos), state.clone());
            Ok(())
        }
        Step::Break(p) => {
            if !w.scaffold.contains(&cell(*p)) && !w.demolish.contains(&cell(*p)) {
                return Err(format!("{p:?} is not scaffold; only scaffold may be broken"));
            }
            if eye_dist(w.feet, *p) > MAX_CLICK {
                return Err("break out of reach".into());
            }
            // whatever needs this block is one of its six neighbours
            for f in Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]) {
                let n = p.offset(*f);
                let Some(s) = w.blocks.get(&cell(n)) else { continue };
                let (name, props) = split_state(s);
                if rules::needs_support(&name, &props, n) == Some(*p) {
                    return Err(format!("{s} at {n:?} would fall or pop off"));
                }
            }
            let gone = w.blocks.remove(&cell(*p));
            w.scaffold.remove(&cell(*p));
            // the other half of a door or bed goes with it
            if let Some(s) = gone.filter(|_| w.demolish.remove(&cell(*p))) {
                let (name, props) = split_state(&s);
                if let Some(o) = rules::second_half(&name, &props, *p).filter(|o| w.demolish.remove(&cell(*o))) {
                    w.blocks.remove(&cell(o));
                }
            }
            fall_after_break(w)
        }
        Step::Checkpoint { count, hash, .. } => {
            let (n, h) = w.built();
            if n != *count || h != *hash {
                return Err(format!("checkpoint mismatch: world has {n} blocks / {h:016x}, plan says {count} / {hash:016x}"));
            }
            Ok(())
        }
    }
}

/// A first click that would turn the clicked slab into a double slab leaves the target empty.
fn merge_check(w: &World, st: &Step) -> Result<(), String> {
    let Step::Place { pos, state, clicks, .. } = st else { return Ok(()) };
    let Some(c) = clicks.first().filter(|c| c.support != *pos) else { return Ok(()) };
    let name = split_state(state).0;
    match w.blocks.get(&cell(c.support)) {
        Some(s) if rules::slab_merges(&name, s, c.face, c.cursor.1) => {
            Err(format!("the click merges into the slab at {:?} instead of placing at {pos:?}", c.support))
        }
        _ => Ok(()),
    }
}

/// Run the whole plan and compare the final world with the schematic placed at the plan origin.
pub fn simulate(plan: &Plan, schem: &Schematic) -> Result<SimReport, SimError> {
    let o = plan.header.origin;
    let mut w = World::new(o.1, plan.header.start);
    if !w.standable(w.feet) {
        return Err(SimError { step: 0, msg: "start cell is not standable".into() });
    }
    let mut rep = SimReport::default();
    for (i, st) in plan.steps.iter().enumerate() {
        // not part of `apply`: the executor must still replay plans written before this rule
        merge_check(&w, st).and_then(|()| apply(&mut w, st)).map_err(|msg| SimError { step: i, msg: format!("{st}: {msg}") })?;
        match st {
            Step::Move(_) => rep.moves += 1,
            Step::Pillar(_) => rep.pillars += 1,
            Step::Place { .. } => rep.placed += 1,
            Step::Support { .. } => rep.supports += 1,
            Step::Break(_) => rep.breaks += 1,
            Step::Checkpoint { .. } => rep.checkpoints += 1,
            Step::Auto { .. } => {}
        }
    }
    let end = plan.steps.len();
    let err = |msg: String| SimError { step: end, msg };
    if !w.scaffold.is_empty() {
        return Err(err(format!("{} scaffold block(s) left standing", w.scaffold.len())));
    }
    let skipped: HashSet<Cell> = plan.header.unreachable.iter().map(|(p, _)| cell(*p)).collect();
    if !w.pending.is_empty() {
        return Err(err(format!("{} second half(s) the server created are not in the plan", w.pending.len())));
    }
    let stat = plan.header.stats.iter().find(|(k, _)| k == "default_state_fallbacks");
    let declared: usize = stat.and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    if w.mismatches > declared {
        return Err(err(format!("{} placement(s) would get another state than requested, plan declares {declared}", w.mismatches)));
    }
    for b in &schem.blocks {
        let c = (o.0 + b.pos.x, o.1 + b.pos.y, o.2 + b.pos.z);
        match w.blocks.get(&c) {
            Some(s) if *s == b.state_string() => {}
            None if skipped.contains(&c) => {}
            other => return Err(err(format!("final world differs at {c:?}: {other:?} vs {}", b.state_string()))),
        }
    }
    if w.blocks.len() + skipped.len() != schem.blocks.len() {
        return Err(err("final world has blocks the schematic does not".into()));
    }
    Ok(rep)
}

#[cfg(test)]
#[path = "sim_tests.rs"]
mod sim_tests;
