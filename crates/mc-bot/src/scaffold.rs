//! Temporary scaffold pillars: the ground-legal way to reach spots above the built surface.
//!
//! The bot never flies. Like a player "pillar jumping", it jumps, places a block under its own
//! feet and lands on it, one block at a time; afterwards it digs the pillar away from the top,
//! dropping one block per step. A pillar is only planned when no walking route reaches a block.

use crate::nav::{blocks_cell, cell_feet, run_from, Cell, Terrain};
use crate::blocks;
use crate::builder::PlayClient;
use crate::nav::feet_cell;
use crate::world::{within_reach, BlockPos};
use std::collections::{HashMap, VecDeque};
use std::io;

/// Tallest pillar planned in one go.
pub const MAX_HEIGHT: i32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pillar {
    /// Cell the bot stands in before the first jump; scaffold blocks fill `base.1 .. base.1 + height`.
    pub base: Cell,
    pub height: i32,
}

impl Pillar {
    /// The scaffold cells, lowest first.
    pub fn cells(&self) -> impl Iterator<Item = Cell> + '_ {
        (0..self.height).map(|k| (self.base.0, self.base.1 + k, self.base.2))
    }

    /// Feet cell of the bot on top of the finished pillar.
    pub fn top(&self) -> Cell {
        (self.base.0, self.base.1 + self.height, self.base.2)
    }
}

/// Cheapest pillar (walk there + build) from which `upcoming[0]` is in reach, with the walking
/// route to its base. Returns `None` when even `MAX_HEIGHT` is not enough.
pub fn plan_pillar(
    t: &Terrain,
    start: Cell,
    upcoming: &[BlockPos],
    radius: i32,
    max_nodes: usize,
) -> Option<(Vec<Cell>, Pillar)> {
    let first = *upcoming.first()?;
    let mut came: HashMap<Cell, (Cell, i32)> = HashMap::from([(start, (start, 0))]);
    let mut queue = VecDeque::from([start]);
    let mut best: Option<(i64, Cell, i32)> = None;
    while let Some(cur) = queue.pop_front() {
        let depth = came[&cur].1;
        if best.is_some_and(|(cost, ..)| depth as i64 > cost) {
            break;
        }
        if t.can_stand(cur) {
            for h in 1..=MAX_HEIGHT {
                let pillar = Pillar { base: cur, height: h };
                if !column_free(t, &pillar) {
                    break;
                }
                let top = pillar.top();
                if within_reach(cell_feet(top), first) && !blocks_cell(top, first) {
                    let cost = depth as i64 + 3 * h as i64 - run_from(top, upcoming) as i64 / 4;
                    if best.is_none_or(|(b, ..)| cost < b) {
                        best = Some((cost, cur, h));
                    }
                    break;
                }
            }
        }
        if came.len() > max_nodes {
            break;
        }
        for next in t.steps_from(cur) {
            let far = (next.0 - start.0).abs().max((next.2 - start.2).abs()) > radius;
            if !far && !came.contains_key(&next) {
                came.insert(next, (cur, depth + 1));
                queue.push_back(next);
            }
        }
    }
    let (_, base, height) = best?;
    let mut route = vec![base];
    let mut at = base;
    while at != start {
        at = came[&at].0;
        route.push(at);
    }
    route.reverse();
    Some((route, Pillar { base, height }))
}

/// Every cell the bot and the scaffold will occupy is free.
fn column_free(t: &Terrain, p: &Pillar) -> bool {
    let (x, y, z) = p.base;
    (y..=y + p.height + 1).all(|yy| !t.solid((x, yy, z)))
}

/// What the bot stands on and around while building: the terrain model plus the scaffold
/// pillars still standing (removed lazily, from the top, when they are in the way or at the end).
pub struct Site {
    pub terrain: Terrain,
    pillars: Vec<Pillar>,
}

impl Site {
    pub fn new(terrain: Terrain) -> Self {
        Site { terrain, pillars: Vec::new() }
    }

    /// Walk to the top of pillar `i`, dig it away, drop to its base. False if the top is unreachable.
    fn remove_pillar(&mut self, bot: &mut PlayClient, i: usize) -> io::Result<bool> {
        let p = self.pillars[i].clone();
        let top = p.top();
        if feet_cell(bot.feet) != top {
            let route = self.terrain.route(feet_cell(bot.feet), &|c| c == top, 40, 60_000);
            if let Some(route) = route {
                bot.walk_cells(&route)?;
            }
        }
        if feet_cell(bot.feet) != top || bot.kicked.is_some() {
            return Ok(false);
        }
        bot.pillar_down(p.base, p.height)?;
        for c in p.cells() {
            self.terrain.remove_block(c);
        }
        self.pillars.remove(i);
        Ok(true)
    }

    /// Remove every remaining pillar. Returns how many could not be reached (left standing).
    pub fn finish(&mut self, bot: &mut PlayClient) -> io::Result<usize> {
        while let Some(i) = self.pillars.len().checked_sub(1) {
            if !self.remove_pillar(bot, i)? {
                break;
            }
        }
        Ok(self.pillars.len())
    }

    fn in_scaffold(&self, t: BlockPos) -> Option<usize> {
        self.pillars.iter().position(|p| p.cells().any(|c| c == (t.x, t.y, t.z)))
    }

    fn can_place_from_here(&self, bot: &PlayClient, target: BlockPos) -> bool {
        within_reach(bot.feet, target) && !blocks_cell(feet_cell(bot.feet), target) && self.in_scaffold(target).is_none()
    }

    /// Get to a spot from which `upcoming[0]` can be placed: stay, walk (over built blocks, jumping
    /// up one, dropping down), or build a scaffold pillar (to reach it, or to climb out of a pit
    /// the walls enclosed). When nothing legal reaches the block the bot stays put and the caller
    /// counts it unreachable: it never flies or hovers.
    pub fn approach(&mut self, bot: &mut PlayClient, upcoming: &[BlockPos]) -> io::Result<()> {
        let first = upcoming[0];
        while let Some(i) = self.in_scaffold(first) {
            if !self.remove_pillar(bot, i)? {
                return Ok(());
            }
        }
        if self.can_place_from_here(bot, first) {
            return Ok(());
        }
        let start = feet_cell(bot.feet);
        if let Some(route) = self.terrain.route_for_run(start, upcoming, 24, 40_000) {
            return bot.walk_cells(&route);
        }
        let Some(item) = blocks::item_id("cobblestone") else { return Ok(()) };
        // Standing on a pillar too tall to drop from: take it down, then look again.
        if let Some(i) = self.pillars.iter().position(|p| p.top() == start) {
            self.remove_pillar(bot, i)?;
            return self.approach(bot, upcoming);
        }
        if let Some((route, pillar)) = plan_pillar(&self.terrain, start, upcoming, 24, 40_000) {
            bot.walk_cells(&route)?;
            self.build(bot, pillar, item)?;
        } else if let Some(pillar) = self.plan_escape(start, upcoming) {
            self.build(bot, pillar, item)?;
            let start = feet_cell(bot.feet);
            if let Some(route) = self.terrain.route_for_run(start, upcoming, 24, 40_000) {
                bot.walk_cells(&route)?;
            }
        }
        Ok(())
    }

    fn build(&mut self, bot: &mut PlayClient, pillar: Pillar, item: i32) -> io::Result<()> {
        if bot.kicked.is_some() || feet_cell(bot.feet) != pillar.base {
            return Ok(());
        }
        bot.pillar_up(pillar.base, pillar.height, item)?;
        for c in pillar.cells() {
            self.terrain.add_block(c, true);
        }
        self.pillars.push(pillar);
        Ok(())
    }

    /// A pillar on the spot after which a walking route to the target exists (the bot is walled in).
    fn plan_escape(&mut self, start: Cell, upcoming: &[BlockPos]) -> Option<Pillar> {
        for height in 1..=MAX_HEIGHT {
            let pillar = Pillar { base: start, height };
            if !column_free(&self.terrain, &pillar) {
                break;
            }
            for c in pillar.cells() {
                self.terrain.add_block(c, true);
            }
            let found = self.terrain.route_for_run(pillar.top(), upcoming, 24, 40_000).is_some();
            for c in pillar.cells() {
                self.terrain.remove_block(c);
            }
            if found {
                return Some(pillar);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn pillar_reaches_a_block_no_walking_route_reaches() {
        let t = Terrain::new(0, HashSet::new());
        let target = BlockPos::new(5, 7, 0);
        assert!(t.route_for_run((0, 0, 0), &[target], 24, 20_000).is_none(), "out of reach on foot");
        let (route, pillar) = plan_pillar(&t, (0, 0, 0), &[target], 24, 20_000).expect("pillar");
        assert!(pillar.height >= 2 && pillar.height <= MAX_HEIGHT);
        assert!(t.can_stand(pillar.base), "base is standable ground");
        assert_eq!(*route.last().unwrap(), pillar.base);
        assert!(within_reach(cell_feet(pillar.top()), target));
        assert!(!blocks_cell(pillar.top(), target));
    }

    #[test]
    fn every_pillar_block_rests_on_the_one_below() {
        let t = Terrain::new(0, HashSet::new());
        let (_, p) = plan_pillar(&t, (0, 0, 0), &[BlockPos::new(2, 6, 2)], 24, 20_000).unwrap();
        // The first block sits on the base's floor, each next on the previous scaffold block.
        assert!(t.can_stand(p.base));
        let cells: Vec<Cell> = p.cells().collect();
        assert_eq!(cells[0], p.base);
        for w in cells.windows(2) {
            assert_eq!((w[1].0, w[1].1 - 1, w[1].2), w[0]);
        }
    }

    #[test]
    fn too_high_is_unreachable_not_flown_to() {
        let t = Terrain::new(0, HashSet::new());
        assert!(plan_pillar(&t, (0, 0, 0), &[BlockPos::new(0, 40, 3)], 24, 20_000).is_none());
    }
}
