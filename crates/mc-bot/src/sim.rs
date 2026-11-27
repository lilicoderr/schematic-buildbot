//! Plan simulator: replays a plan against an in-memory world and checks every step is legal.
//! It uses only the constants documented in docs/PHYSICS.md and shares no planning code with the
//! slicer, so a slicer bug cannot hide itself.

use crate::nav::{blocks_cell, cell_feet, is_standable_surface, Cell};
use crate::plan::{block_hash, Plan, Step};
use crate::schematic::{split_state, Schematic};
use crate::world::{eye_to_box, within_reach, BlockPos, Face, CLICK_REACH};
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
    pub feet: Cell,
}

impl World {
    pub fn new(ground_y: i32, start: Cell) -> Self {
        World { ground_y, blocks: HashMap::new(), scaffold: HashSet::new(), feet: start }
    }

    pub fn solid(&self, c: Cell) -> bool {
        c.1 < self.ground_y || self.blocks.contains_key(&c)
    }

    /// The block top at `c` can carry a player standing above it.
    pub fn carries(&self, c: Cell) -> bool {
        if c.1 < self.ground_y {
            return true;
        }
        self.blocks.get(&c).is_some_and(|s| {
            let (name, props) = split_state(s);
            is_standable_surface(&name, props.get("type").map(String::as_str))
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
        w.feet.1 -= 1;
        if start - w.feet.1 > MAX_DROP {
            return Err("fell more than 3 blocks".into());
        }
    }
    Ok(())
}

/// Support check for one click: the clicked block exists and the face leads into `pos`.
fn click_ok(w: &World, pos: BlockPos, support: BlockPos, face: Face) -> Result<(), String> {
    if !w.solid(cell(support)) {
        return Err(format!("click support {support:?} is air"));
    }
    if support.offset(face) != pos && support != pos {
        return Err(format!("clicking {face:?} of {support:?} does not lead to {pos:?}"));
    }
    Ok(())
}

fn place(w: &mut World, pos: BlockPos, state: &str, clicks: &[crate::plan::ClickStep]) -> Result<(), String> {
    if w.solid(cell(pos)) {
        return Err(format!("{pos:?} is not free"));
    }
    if !within_reach(cell_feet(w.feet), pos) {
        return Err(format!("{pos:?} out of reach from {:?}", w.feet));
    }
    if blocks_cell(w.feet, pos) {
        return Err("placing into the player's own cells".into());
    }
    if clicks.is_empty() {
        return Err("no clicks".into());
    }
    for c in clicks {
        click_ok(w, pos, c.support, c.face)?;
        if eye_to_box(cell_feet(w.feet), c.support) > CLICK_REACH {
            return Err(format!("click support {:?} out of reach", c.support));
        }
    }
    w.blocks.insert(cell(pos), state.to_string());
    Ok(())
}

fn step_once(w: &mut World, st: &Step) -> Result<(), String> {
    match st {
        Step::Move(to) => move_to(w, *to),
        Step::Pillar(c) => {
            if *c != w.feet {
                return Err("PILLAR must be at the feet cell".into());
            }
            if !w.carries((c.0, c.1 - 1, c.2)) || w.solid((c.0, c.1 + 2, c.2)) {
                return Err("pillar needs a floor to click and head room".into());
            }
            w.blocks.insert(*c, SCAFFOLD.into());
            w.scaffold.insert(*c);
            w.feet.1 += 1;
            Ok(())
        }
        Step::Place { pos, state, clicks, .. } => place(w, *pos, state, clicks),
        Step::Support { pos, clicks } => {
            place(w, *pos, SCAFFOLD, clicks)?;
            w.scaffold.insert(cell(*pos));
            Ok(())
        }
        Step::Auto { pos, state } => {
            let name = split_state(state).0;
            let near = Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]).any(|f| {
                w.blocks.get(&cell(pos.offset(*f))).is_some_and(|s| split_state(s).0 == name)
            });
            if !near || w.solid(cell(*pos)) {
                return Err("AUTO needs a free cell next to a placed block of the same kind".into());
            }
            w.blocks.insert(cell(*pos), state.clone());
            Ok(())
        }
        Step::Break(p) => {
            if !w.scaffold.contains(&cell(*p)) {
                return Err(format!("{p:?} is not scaffold; only scaffold may be broken"));
            }
            if eye_to_box(cell_feet(w.feet), *p) > CLICK_REACH {
                return Err("break out of reach".into());
            }
            w.blocks.remove(&cell(*p));
            w.scaffold.remove(&cell(*p));
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

/// Run the whole plan and compare the final world with the schematic placed at the plan origin.
pub fn simulate(plan: &Plan, schem: &Schematic) -> Result<SimReport, SimError> {
    let o = plan.header.origin;
    let mut w = World::new(o.1, plan.header.start);
    if !w.standable(w.feet) {
        return Err(SimError { step: 0, msg: "start cell is not standable".into() });
    }
    let mut rep = SimReport::default();
    for (i, st) in plan.steps.iter().enumerate() {
        step_once(&mut w, st).map_err(|msg| SimError { step: i, msg: format!("{st}: {msg}") })?;
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
