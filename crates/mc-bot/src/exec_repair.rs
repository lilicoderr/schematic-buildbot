//! Checkpoint repair for the plan executor: find placements the server never confirmed and
//! re-place them with a short local plan (the online navigator), then walk back to where the plan
//! expects the player.

use crate::builder::PlayClient;
use crate::exec::Sent;
use crate::extras::Extras;
use crate::nav::{feet_cell, Cell, Terrain};
use crate::scaffold::Site;
use crate::sim::World;
use crate::world::BlockPos;
use std::collections::HashSet;
use std::io;

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

/// Schematic blocks (with a placement step) the server has not confirmed, in world coordinates.
pub fn missing(bot: &PlayClient, world: &World, sent: &Sent, off: Offset) -> Vec<BlockPos> {
    let mut out: Vec<BlockPos> = world
        .blocks
        .keys()
        .filter(|c| !world.scaffold.contains(c))
        .map(|c| off.pos(BlockPos::new(c.0, c.1, c.2)))
        .filter(|p| sent.contains_key(p) && !bot.blocks.is_confirmed(*p))
        .collect();
    // a refused second click leaves a single slab: confirmed, but not done
    for (c, s) in &world.blocks {
        let p = off.pos(BlockPos::new(c.0, c.1, c.2));
        if sent.contains_key(&p) && bot.blocks.is_confirmed(p) && crate::verify::half_done_double_slab(s, bot.blocks.state(p)) {
            out.push(p);
        }
    }
    out.sort_by_key(|p| (p.y, p.x, p.z));
    out
}

/// Re-place every unconfirmed block; returns how many were attempted.
pub fn repair(bot: &mut PlayClient, world: &World, sent: &Sent, off: Offset, extras: &mut Extras, say: &mut dyn FnMut(&str)) -> io::Result<usize> {
    let todo = missing(bot, world, sent, off);
    if todo.is_empty() {
        return Ok(0);
    }
    let mut site = site_of(world, off, extras);
    let plan_feet = off.cell(world.feet);
    for p in &todo {
        let Some((item, pl)) = sent.get(p) else { continue };
        say(&format!("repair: placing {p:?} again"));
        bot.blocks.forget(*p);
        site.approach(bot, std::slice::from_ref(p))?;
        if !bot.place(*p, *item, pl)? {
            say(&format!("repair: {p:?} still out of reach"));
        }
        if bot.kicked.is_some() {
            extras.set(site.standing());
            return Ok(todo.len());
        }
    }
    site.finish(bot)?;
    extras.set(site.standing());
    bot.idle(std::time::Duration::from_millis(300))?;
    // back to where the plan expects us
    if feet_cell(bot.feet) != plan_feet && !return_to(bot, world, off, extras, say)? {
        return Err(io::Error::other("repair: cannot get back to the planned position on the ground"));
    }
    Ok(todo.len())
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
