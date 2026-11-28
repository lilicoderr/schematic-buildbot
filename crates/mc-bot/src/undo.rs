//! `build-bot undo`: take down exactly what a build placed, and nothing else.
//!
//! The blocks the bot put there are what its plan leaves standing (replayed up to the saved step
//! of an unfinished build). A block is only taken if the server still shows it as the bot placed
//! it; air means it is gone already, anything else is somebody else's and stays. The way is
//! planned in the plan's own coordinates by the same navigator the build uses (top first), and the
//! strict simulator checks every step before anything is sent: only those blocks and scaffold may
//! be broken, nothing standing may be left to pop off, the bot always has ground under it.

use crate::audit::{self, Verdict};
use crate::exec::Demolition;
use crate::nav::{blocks_cell, feet_cell, Cell, Terrain};
use crate::plan::{Header, Plan, Step};
use crate::schematic::split_state;
use crate::scaffold::Site;
use crate::sim::{self, World};
use crate::slice::Recorder;
use crate::sim_rules::{needs_support, second_half};
use crate::world::{eye_to_box, within_reach, BlockPos, CLICK_REACH};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Blocks ahead the navigator may pick a standing place for.
const LOOKAHEAD: usize = 24;

/// What stands after the plan's first `steps` steps (all of them for `None`), scaffold included.
pub fn placed_blocks(plan: &Plan, steps: Option<usize>) -> Result<HashMap<Cell, String>, String> {
    let mut w = World::new(plan.header.origin.1, plan.header.start);
    for (i, st) in plan.steps.iter().take(steps.unwrap_or(usize::MAX)).enumerate() {
        sim::apply(&mut w, st).map_err(|m| format!("step {}: {m}", i + 1))?;
    }
    Ok(w.blocks)
}

#[derive(Debug, Default)]
pub struct Sorted {
    /// Still as the bot placed them: these go (plan coordinates).
    pub ours: HashMap<Cell, String>,
    /// Already air.
    pub gone: usize,
    /// Another block now: left alone (plan cell, what stands there).
    pub changed: Vec<(Cell, String)>,
    /// Cells whose chunk is not loaded.
    pub unknown: usize,
}

/// Compare what the server shows (`state_at`: world position to state id) with what was placed.
pub fn sort(placed: &HashMap<Cell, String>, ground_y: i32, state_at: &dyn Fn(Cell) -> Option<u32>) -> Sorted {
    let mut world = World::new(ground_y, (0, 0, 0));
    world.blocks = placed.clone();
    let mut out = Sorted::default();
    for (&c, _) in placed.iter().filter(|(c, _)| c.1 >= ground_y) {
        let Some(id) = state_at(c) else {
            out.unknown += 1;
            continue;
        };
        let want = audit::expected(&world, c);
        match audit::judge(want.as_deref(), id as i32, None) {
            _ if id == 0 => out.gone += 1,
            Verdict::Ok | Verdict::Soft(_) => {
                out.ours.insert(c, placed[&c].clone());
            }
            Verdict::Wrong(_) => out.changed.push((c, crate::version::active().tables.block_of_state(id).map_or_else(|| format!("state {id}"), String::from))),
        }
    }
    out.changed.sort();
    out
}

/// The cells that come away with another cell (the top of a door, the head of a bed).
fn followers(blocks: &HashMap<Cell, String>) -> HashSet<Cell> {
    blocks
        .iter()
        .filter_map(|(&c, s)| {
            let (name, props) = split_state(s);
            let o = second_half(&name, &props, BlockPos::new(c.0, c.1, c.2))?;
            let first = !(props.get("half").is_some_and(|h| h == "upper") || props.get("part").is_some_and(|p| p == "head"));
            (first && blocks.contains_key(&(o.x, o.y, o.z))).then_some((o.x, o.y, o.z))
        })
        .collect()
}

/// Break order: top first, and a block that cannot stay without another goes before it.
pub fn order(targets: &HashMap<Cell, String>) -> Vec<Cell> {
    let skip = followers(targets);
    let mut cells: Vec<Cell> = targets.keys().copied().filter(|c| !skip.contains(c)).collect();
    cells.sort_by_key(|&(x, y, z)| (-y, z / 5, if (z / 5) % 2 == 0 { x } else { -x }, z));
    let mut holds: HashMap<Cell, Vec<Cell>> = HashMap::new();
    for &c in &cells {
        let (name, props) = split_state(&targets[&c]);
        if let Some(s) = needs_support(&name, &props, BlockPos::new(c.0, c.1, c.2)) {
            holds.entry((s.x, s.y, s.z)).or_default().push(c);
        }
    }
    fn emit(c: Cell, holds: &HashMap<Cell, Vec<Cell>>, done: &mut HashSet<Cell>, out: &mut Vec<Cell>) {
        if !done.insert(c) {
            return;
        }
        for &d in holds.get(&c).into_iter().flatten() {
            emit(d, holds, done, out);
        }
        out.push(c);
    }
    let (mut done, mut out) = (HashSet::new(), Vec::new());
    for c in cells {
        emit(c, &holds, &mut done, &mut out);
    }
    out
}

pub struct Undo {
    pub plan: Plan,
    pub demolition: Demolition,
    /// Blocks no standing place reaches: they stay.
    pub unreachable: Vec<Cell>,
}

/// The cells of `targets` come down in `order`; `foreign` blocks stand in the way and stay.
pub fn plan(header: &Header, targets: &HashMap<Cell, String>, foreign: &HashMap<Cell, String>) -> Result<Undo, String> {
    let ground = header.origin.1;
    let mut terrain = Terrain::new(ground, HashSet::new());
    let mut blocks = foreign.clone();
    blocks.extend(targets.clone());
    for (&c, s) in &blocks {
        let (name, props) = split_state(s);
        // what the strict simulator lets the bot stand on, not the navigator's own idea
        terrain.add_block(c, crate::sim_rules::full_top(&name, &props));
    }
    terrain.sky_top = blocks.keys().map(|c| c.1).max().unwrap_or(ground) + 2;
    let mut site = Site::new(terrain);
    let mut rec = Recorder::new(header.start);
    let list = order(targets);
    let at = |c: Cell| BlockPos::new(c.0, c.1, c.2);
    let mut unreachable = Vec::new();
    for (i, &c) in list.iter().enumerate() {
        let p = at(c);
        let ahead: Vec<BlockPos> = list[i..].iter().take(LOOKAHEAD).map(|&c| at(c)).collect();
        // never stand on the block about to go
        site.terrain.avoid.insert((c.0, c.1 + 1, c.2));
        site.approach(&mut rec, &ahead).map_err(|e| e.to_string())?;
        site.terrain.avoid.clear();
        let here = feet_cell(rec.feet);
        let over = here.0 == c.0 && here.2 == c.2 && here.1 > c.1;
        if !within_reach(rec.feet, p) || blocks_cell(here, p) || over || eye_to_box(rec.feet, p) > CLICK_REACH {
            unreachable.push(c);
            continue;
        }
        rec.steps.push(Step::Break(p));
        site.terrain.remove_block(c);
    }
    let left = site.finish(&mut rec).map_err(|e| e.to_string())?;
    if left > 0 {
        return Err(format!("{left} scaffold block(s) could not be removed"));
    }
    let plan = Plan { header: Header { schematic_hash: 0, stats: Vec::new(), unreachable: Vec::new(), ..header.clone() }, steps: rec.steps };
    let mut demolition = Demolition { blocks, targets: targets.keys().copied().collect() };
    for c in &unreachable {
        demolition.targets.remove(c);
    }
    check(&plan, &demolition)?;
    Ok(Undo { plan, demolition, unreachable })
}

/// Replay `plan` from the world it starts in with the strict simulator.
pub fn check(plan: &Plan, d: &Demolition) -> Result<(), String> {
    let mut w = World::new(plan.header.origin.1, plan.header.start);
    w.blocks = d.blocks.clone();
    w.demolish = d.targets.clone();
    for (i, st) in plan.steps.iter().enumerate() {
        sim::apply(&mut w, st).map_err(|m| format!("the undo plan is illegal at step {}: {st}: {m}", i + 1))?;
    }
    if !w.scaffold.is_empty() {
        return Err(format!("{} scaffold block(s) left standing", w.scaffold.len()));
    }
    let left: BTreeMap<Cell, &String> = d.targets.iter().filter_map(|c| w.blocks.get(c).map(|s| (*c, s))).collect();
    if let Some((c, s)) = left.iter().next() {
        return Err(format!("the undo plan leaves {} block(s) standing, e.g. {s} at {c:?}", left.len()));
    }
    // everything else must stand as before: only targets and their other halves went
    let mine: HashMap<Cell, String> = d.blocks.iter().filter(|(c, _)| d.targets.contains(c)).map(|(c, s)| (*c, s.clone())).collect();
    let halves: HashSet<Cell> = followers(&mine);
    for c in d.blocks.keys().filter(|c| !d.targets.contains(c) && !halves.contains(c)) {
        if !w.blocks.contains_key(c) {
            return Err(format!("the undo plan would remove the block at {c:?}, which is not ours"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "undo_tests.rs"]
mod tests;
