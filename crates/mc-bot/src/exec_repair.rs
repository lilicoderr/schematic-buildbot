//! Repair for the plan executor: whenever the server shows another state than the plan wants,
//! break and place again with a short local plan (the online navigator, [`crate::refit`]), then
//! walk back to where the plan expects the player.

use crate::builder::PlayClient;
use crate::extras::Extras;
use crate::ledger::Ledger;
use crate::nav::{feet_cell, Cell, Terrain};
use crate::plan::Plan;
use crate::scaffold::Site;
use crate::sim::World;
use crate::world::BlockPos;
use std::collections::HashSet;
use std::io;
use std::time::Duration;

/// Translation from plan coordinates to world coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Offset {
    pub dx: i32,
    pub dy: i32,
    pub dz: i32,
}

impl Offset {
    /// The plan's start cell is where the player actually stands.
    pub fn between(plan_start: Cell, actual: Cell) -> Self {
        Offset { dx: actual.0 - plan_start.0, dy: actual.1 - plan_start.1, dz: actual.2 - plan_start.2 }
    }
    pub fn cell(&self, c: Cell) -> Cell {
        (c.0 + self.dx, c.1 + self.dy, c.2 + self.dz)
    }
    pub fn pos(&self, p: BlockPos) -> BlockPos {
        BlockPos::new(p.x + self.dx, p.y + self.dy, p.z + self.dz)
    }
}

/// Long enough to sit out a server lag spike; returns as soon as the acknowledgement is there.
const SYNC: Duration = Duration::from_secs(20);

/// Wait for the server's answers to everything sent so far.
pub fn settle(bot: &mut PlayClient) -> io::Result<()> {
    bot.sync(SYNC)?;
    bot.idle(Duration::from_millis(100))
}

/// What a repair needs to know about the run.
pub struct Run<'a> {
    pub plan: &'a Plan,
    pub world: &'a World,
    pub ledger: &'a mut Ledger,
    pub extras: &'a mut Extras,
}

/// Repair whatever among `list` the server shows wrong, and what the repair itself disturbs
/// (each position at most [`crate::ledger::MAX_TRIES`] times). Returns the blocks worked on.
pub fn heal(bot: &mut PlayClient, run: &mut Run, mut list: Vec<BlockPos>, say: &mut dyn FnMut(&str)) -> io::Result<usize> {
    let mut count = 0;
    while bot.kicked.is_none() {
        let wrong = run.ledger.wrong(bot, run.world, run.extras, list);
        if wrong.is_empty() {
            break;
        }
        let off = run.ledger.off;
        for (p, why) in &wrong {
            say(&format!("repair: {p:?} (plan {},{},{}): {why}", p.x - off.dx, p.y - off.dy, p.z - off.dz));
            *run.ledger.tries.entry(*p).or_default() += 1;
        }
        list = wrong.into_iter().map(|w| w.0).collect();
        count += list.len();
        crate::refit::fix(bot, run, &list, say)?;
        settle(bot)?;
        run.ledger.collect(bot);
        list.extend(run.ledger.due(bot.acked));
    }
    Ok(count)
}

/// Get on foot to the feet cell the plan expects, searching the world as it stands now: walking
/// where possible, otherwise with temporary scaffold (kept in `extras` until it can be broken).
/// False means the bot is not there; walking on with plan coordinates would cross thin air.
pub fn return_to(bot: &mut PlayClient, world: &World, off: Offset, extras: &mut Extras, say: &mut dyn FnMut(&str)) -> io::Result<bool> {
    let goal = off.cell(world.feet);
    if feet_cell(bot.feet) == goal {
        return Ok(true);
    }
    let mut site = site_of(world, off, extras);
    let there = match site.go_to(bot, goal) {
        Ok(true) => site.drop_in_reach(bot).map(|()| true),
        other => other,
    };
    // recorded even after an error, so that nothing built here is forgotten
    extras.set(site.standing());
    if matches!(there, Ok(false)) {
        say(&format!("no ground route back to the planned position {goal:?} from {:?}", feet_cell(bot.feet)));
    }
    there
}

/// The planned world (its scaffold counts as fixed) plus the extra blocks, which may be dug.
pub fn site_of(world: &World, off: Offset, extras: &Extras) -> Site {
    let mut terrain = terrain_of(world, off);
    terrain.sky_top = world.blocks.keys().map(|c| c.1 + off.dy).max().unwrap_or(0) + 2;
    let mut site = Site::new(terrain);
    for &c in &extras.cells {
        site.adopt(c);
    }
    site
}

fn terrain_of(world: &World, off: Offset) -> Terrain {
    let mut terrain = Terrain::new(world.ground_y + off.dy, HashSet::new());
    terrain.fence = world.fence.shifted(off.dx, off.dz);
    for (c, s) in &world.blocks {
        let (name, props) = crate::schematic::split_state(s);
        let standable = crate::nav::is_standable_surface(&name, props.get("type").map(String::as_str));
        terrain.add_block(off.cell(*c), standable);
    }
    terrain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_translates_cells_and_positions() {
        let off = Offset::between((2, 64, 2), (10, 70, -4));
        assert_eq!(off, Offset { dx: 8, dy: 6, dz: -6 });
        assert_eq!(off.cell((2, 64, 2)), (10, 70, -4));
        assert_eq!(off.pos(BlockPos::new(3, 65, 4)), BlockPos::new(11, 71, -2));
    }
}
