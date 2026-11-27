//! Temporary scaffold pillars: the ground-legal way to reach spots above the built surface.
//!
//! The bot never flies. Like a player "pillar jumping", it jumps, places a block under its own
//! feet and lands on it, one block at a time; afterwards it digs the pillar away from the top,
//! dropping one block per step. A pillar is only planned when no walking route reaches a block.

use crate::nav::{blocks_cell, cell_feet, run_from, Cell, Terrain};
use crate::blocks;
use crate::builder::PlayClient;
use crate::nav::feet_cell;
use crate::world::{eye_to_box, within_reach, BlockPos, Face, CLICK_REACH};
use std::collections::{HashMap, VecDeque};
use std::collections::HashSet;
use std::io;
use std::time::{Duration, Instant};

/// Tallest pillar planned in one go.
pub const MAX_HEIGHT: i32 = 4;

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
                let in_pillar = pillar.cells().any(|c| c == (first.x, first.y, first.z));
                if !in_pillar && within_reach(cell_feet(top), first) && !blocks_cell(top, first) && !t.avoid.contains(&top) {
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

/// What the planner needs from whatever moves the player: the live client, or a recorder that
/// writes plan steps instead (see `slice`).
pub trait Mover {
    fn feet(&self) -> crate::world::Vec3;
    fn kicked(&self) -> bool;
    fn walk_cells(&mut self, route: &[Cell]) -> io::Result<()>;
    fn pillar_up(&mut self, base: Cell, height: i32, item_id: i32) -> io::Result<()>;
    fn pillar_down(&mut self, base: Cell, height: i32) -> io::Result<()>;
    /// Break one scaffold block that is within reach.
    fn break_block(&mut self, pos: BlockPos) -> io::Result<()>;
    /// Place one scaffold block at `pos` by the given click.
    fn place_support(&mut self, pos: BlockPos, click: &crate::plan::ClickStep) -> io::Result<()>;
}

impl Mover for PlayClient {
    fn feet(&self) -> crate::world::Vec3 {
        self.feet
    }
    fn kicked(&self) -> bool {
        self.kicked.is_some()
    }
    fn walk_cells(&mut self, route: &[Cell]) -> io::Result<()> {
        PlayClient::walk_cells(self, route)
    }
    fn pillar_up(&mut self, base: Cell, height: i32, item_id: i32) -> io::Result<()> {
        PlayClient::pillar_up(self, base, height, item_id)
    }
    fn pillar_down(&mut self, base: Cell, height: i32) -> io::Result<()> {
        PlayClient::pillar_down(self, base, height)
    }
    fn break_block(&mut self, pos: BlockPos) -> io::Result<()> {
        PlayClient::break_block(self, pos)
    }
    fn place_support(&mut self, pos: BlockPos, click: &crate::plan::ClickStep) -> io::Result<()> {
        PlayClient::place_support(self, pos, click.support, click.face, click.cursor)
    }
}

/// What the bot stands on and around while building: the terrain model plus the scaffold
/// pillars still standing (removed lazily, from the top, when they are in the way or at the end).
pub struct Site {
    pub terrain: Terrain,
    pillars: Vec<Pillar>,
    /// Floor blocks of scaffold bridges still standing (see `bridge.rs`).
    bridges: Vec<Cell>,
    pub timing: Timing,
}

/// Where the time went (cumulative); the caller diffs snapshots per window.
#[derive(Debug, Clone, Copy, Default)]
pub struct Timing {
    pub search: Duration,
    pub walk: Duration,
    pub pillar: Duration,
    pub routes: u32,
    pub steps: u32,
    pub pillars_built: u32,
    pub pillars_removed: u32,
}

impl Site {
    pub fn new(terrain: Terrain) -> Self {
        Site { terrain, pillars: Vec::new(), bridges: Vec::new(), timing: Timing::default() }
    }

    /// Take pillar `i` away. Standing on its top: dig down from the top. Otherwise walk to spots
    /// that reach each block (top first, never from above it) and break it from there.
    /// False if some block cannot be reached.
    fn remove_pillar(&mut self, bot: &mut impl Mover, i: usize) -> io::Result<bool> {
        let p = self.pillars[i].clone();
        let t = Instant::now();
        if feet_cell(bot.feet()) == p.top() {
            bot.pillar_down(p.base, p.height)?;
        } else {
            for c in p.cells().collect::<Vec<_>>().into_iter().rev() {
                let target = BlockPos::new(c.0, c.1, c.2);
                let ok = |g: Cell| !(g.0 == c.0 && g.2 == c.2) && eye_to_box(cell_feet(g), target) <= CLICK_REACH;
                let here = feet_cell(bot.feet());
                if !ok(here) || !self.terrain.can_stand(here) {
                    let goal = |g: Cell| self.terrain.can_stand(g) && ok(g);
                    let Some(route) = self.terrain.route(here, &goal, 40, 60_000) else {
                        if std::env::var("BUILDBOT_DEBUG").is_ok() {
                            eprintln!("cannot reach pillar block {c:?} from {here:?} (pillar {p:?})");
                        }
                        return Ok(false);
                    };
                    self.walk(bot, &route)?;
                }
                bot.break_block(target)?;
                self.terrain.remove_block(c);
            }
        }
        for c in p.cells() {
            self.terrain.remove_block(c);
        }
        self.pillars.remove(i);
        self.timing.pillar += t.elapsed();
        self.timing.pillars_removed += 1;
        Ok(true)
    }

    /// Break, from where the bot stands, the top blocks of every pillar that are within reach
    /// (never a pillar the bot stands over). Cheap, no walking; keeps pillars from being stranded.
    fn sweep_nearby(&mut self, bot: &mut impl Mover, protect: &HashSet<Cell>) -> io::Result<()> {
        let feet = bot.feet();
        let here = feet_cell(feet);
        let mut i = 0;
        while i < self.pillars.len() {
            if self.pillars[i].base.0 == here.0 && self.pillars[i].base.2 == here.2 {
                i += 1;
                continue;
            }
            while self.pillars[i].height > 0 {
                let p = &self.pillars[i];
                let c = (p.base.0, p.base.1 + p.height - 1, p.base.2);
                let target = BlockPos::new(c.0, c.1, c.2);
                if eye_to_box(feet, target) > CLICK_REACH || protect.contains(&c) {
                    break;
                }
                bot.break_block(target)?;
                self.terrain.remove_block(c);
                self.pillars[i].height -= 1;
            }
            if self.pillars[i].height == 0 {
                self.pillars.remove(i);
                self.timing.pillars_removed += 1;
            } else {
                i += 1;
            }
        }
        Ok(())
    }

    /// Remove every remaining pillar. Returns how many could not be reached (left standing).
    pub fn finish(&mut self, bot: &mut impl Mover) -> io::Result<usize> {
        while let Some(i) = self.pillars.len().checked_sub(1) {
            if !self.remove_pillar(bot, i)? {
                if std::env::var("BUILDBOT_DEBUG").is_ok() {
                    eprintln!("cannot remove pillar {:?} from {:?}", self.pillars[i], feet_cell(bot.feet()));
                }
                break;
            }
        }
        while let Some(&c) = self.bridges.last() {
            if !self.remove_bridge(bot, c)? {
                if std::env::var("BUILDBOT_DEBUG").is_ok() {
                    eprintln!("cannot remove bridge floor {c:?} from {:?}", feet_cell(bot.feet()));
                }
                break;
            }
        }
        Ok(self.pillars.len() + self.bridges.len())
    }

    /// Walk a route, counting steps and time. Stepping off a pillar takes it down right away,
    /// and bridge floors are broken behind us (never one the rest of the route still needs).
    fn walk(&mut self, bot: &mut impl Mover, route: &[Cell]) -> io::Result<()> {
        self.timing.routes += 1;
        self.timing.steps += route.len().saturating_sub(1) as u32;
        for (k, pair) in route.windows(2).enumerate() {
            let t = Instant::now();
            bot.walk_cells(pair)?;
            self.timing.walk += t.elapsed();
            if bot.kicked() {
                break;
            }
            if !self.bridges.is_empty() || !self.pillars.is_empty() {
                let ahead: HashSet<Cell> = route[k + 1..].iter().map(|c| (c.0, c.1 - 1, c.2)).collect();
                self.sweep_nearby(bot, &ahead)?;
                self.sweep_bridges(bot, &ahead)?;
            }
        }
        Ok(())
    }

    fn in_scaffold(&self, t: BlockPos) -> Option<usize> {
        self.pillars.iter().position(|p| p.cells().any(|c| c == (t.x, t.y, t.z)))
    }

    fn can_place_from_here(&self, bot: &impl Mover, target: BlockPos) -> bool {
        within_reach(bot.feet(), target)
            && !blocks_cell(feet_cell(bot.feet()), target)
            && self.in_scaffold(target).is_none()
            && !self.terrain.avoid.contains(&feet_cell(bot.feet()))
    }

    /// Get to a spot from which `upcoming[0]` can be placed: stay, walk (over built blocks, jumping
    /// up one, dropping down), or build a scaffold pillar (to reach it, or to climb out of a pit
    /// the walls enclosed). When nothing legal reaches the block the bot stays put and the caller
    /// counts it unreachable: it never flies or hovers.
    pub fn approach(&mut self, bot: &mut impl Mover, upcoming: &[BlockPos]) -> io::Result<()> {
        let first = upcoming[0];
        if self.bridges.contains(&(first.x, first.y, first.z)) && !self.remove_bridge(bot, (first.x, first.y, first.z))? {
            return Ok(());
        }
        while let Some(i) = self.in_scaffold(first) {
            if !self.remove_pillar(bot, i)? {
                return Ok(());
            }
        }
        if self.can_place_from_here(bot, first) {
            return Ok(());
        }
        self.sweep_nearby(bot, &HashSet::new())?;
        self.sweep_bridges(bot, &HashSet::new())?;
        let start = feet_cell(bot.feet());
        let t = Instant::now();
        let route = self.terrain.route_for_run(start, upcoming, 24, 40_000);
        let cut_short = self.terrain.pruned.get();
        self.timing.search += t.elapsed();
        if let Some(route) = route {
            return self.walk(bot, &route);
        }
        let Some(item) = blocks::item_id("cobblestone") else { return Ok(()) };
        // Standing on a pillar too tall to drop from: take it down, then look again.
        if let Some(i) = self.pillars.iter().position(|p| p.top() == start) {
            self.remove_pillar(bot, i)?;
            return self.approach(bot, upcoming);
        }
        let t = Instant::now();
        let planned = plan_pillar(&self.terrain, start, upcoming, 24, 40_000);
        self.timing.search += t.elapsed();
        if let Some((route, pillar)) = planned {
            self.walk(bot, &route)?;
            self.build(bot, pillar, item)?;
        } else if let Some(pillar) = self.plan_escape(start, upcoming) {
            self.build(bot, pillar, item)?;
            let start = feet_cell(bot.feet());
            if let Some(route) = self.terrain.route_for_run(start, upcoming, 24, 40_000) {
                self.walk(bot, &route)?;
            }
        } else {
            // Big sites: the block may simply be a long walk away. Searched last because exhausting a
            // wide area is expensive when nothing is found.
            let t = Instant::now();
            let far = if cut_short { self.terrain.route_for_run(start, upcoming, 160, 400_000) } else { None };
            self.timing.search += t.elapsed();
            if let Some(route) = far {
                return self.walk(bot, &route);
            }
            if let Some(hops) = crate::bridge::plan_bridge(&self.terrain, start, first, 24, 40_000) {
                self.walk_hops(bot, start, &hops)?;
            }
        }
        Ok(())
    }

    /// Walk a mixed route; where a hop needs a floor, bridge first: click the side of the floor
    /// block we stand on and place a scaffold block next to it.
    fn walk_hops(&mut self, bot: &mut impl Mover, start: Cell, hops: &[crate::bridge::Hop]) -> io::Result<()> {
        let mut cur = start;
        for h in hops {
            if h.bridge {
                let floor = BlockPos::new(h.to.0, h.to.1 - 1, h.to.2);
                let from = BlockPos::new(cur.0, cur.1 - 1, cur.2);
                let face = Face::HORIZONTAL.iter().copied().find(|f| from.offset(*f) == floor).unwrap();
                let click = crate::plan::ClickStep { support: from, face, cursor: crate::support::face_cursor(face) };
                if std::env::var("BUILDBOT_TRACE").is_ok() {
                    eprintln!("bridge floor {floor:?} from {cur:?}");
                }
                bot.place_support(floor, &click)?;
                self.terrain.add_block((floor.x, floor.y, floor.z), true);
                if !self.bridges.contains(&(floor.x, floor.y, floor.z)) {
                    self.bridges.push((floor.x, floor.y, floor.z));
                }
            }
            self.walk(bot, &[cur, h.to])?;
            cur = h.to;
            if bot.kicked() {
                break;
            }
        }
        Ok(())
    }

    /// Last resort for cleaning up: bridge to a standing spot that reaches `target` (never above it).
    fn reach_by_bridge(&mut self, bot: &mut impl Mover, target: BlockPos) -> io::Result<bool> {
        let above: Vec<Cell> = (1..=8).map(|d| (target.x, target.y + d, target.z)).collect();
        for c in &above {
            self.terrain.avoid.insert(*c);
        }
        let start = feet_cell(bot.feet());
        let hops = crate::bridge::plan_bridge(&self.terrain, start, target, 40, 80_000);
        for c in &above {
            self.terrain.avoid.remove(c);
        }
        match hops {
            Some(h) => {
                self.walk_hops(bot, start, &h)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Break the bridge floor `c`: from where we stand if possible, else after walking to a spot
    /// that reaches it (never from above it). False if no such spot exists.
    fn remove_bridge(&mut self, bot: &mut impl Mover, c: Cell) -> io::Result<bool> {
        let target = BlockPos::new(c.0, c.1, c.2);
        let reach = |g: Cell| eye_to_box(cell_feet(g), target) <= CLICK_REACH && !(g.0 == c.0 && g.2 == c.2);
        let here = feet_cell(bot.feet());
        if !(reach(here) && self.terrain.can_stand(here)) {
            let goal = |g: Cell| self.terrain.can_stand(g) && reach(g);
            match self.terrain.route(here, &goal, 40, 60_000) {
                Some(route) => self.walk(bot, &route)?,
                None => {
                    if !self.reach_by_bridge(bot, target)? {
                        return Ok(false);
                    }
                }
            }
        }
        bot.break_block(target)?;
        self.terrain.remove_block(c);
        self.bridges.retain(|b| *b != c);
        Ok(true)
    }

    /// Break bridge floors within reach that the bot is not standing on.
    fn sweep_bridges(&mut self, bot: &mut impl Mover, protect: &HashSet<Cell>) -> io::Result<()> {
        let feet = bot.feet();
        let here = feet_cell(feet);
        let mut keep = Vec::new();
        for c in std::mem::take(&mut self.bridges) {
            let target = BlockPos::new(c.0, c.1, c.2);
            if !self.terrain.solid(c) || keep.contains(&c) {
                continue; // already gone (or listed twice): never break a block that is not ours
            }
            let under_us = here.0 == c.0 && here.2 == c.2 && here.1 > c.1;
            if !under_us && !protect.contains(&c) && eye_to_box(feet, target) <= CLICK_REACH {
                bot.break_block(target)?;
                self.terrain.remove_block(c);
            } else {
                keep.push(c);
            }
        }
        self.bridges = keep;
        Ok(())
    }

    fn build(&mut self, bot: &mut impl Mover, pillar: Pillar, item: i32) -> io::Result<()> {
        if bot.kicked() || feet_cell(bot.feet()) != pillar.base {
            return Ok(());
        }
        let t = Instant::now();
        if std::env::var("BUILDBOT_TRACE").is_ok() {
            eprintln!("build {pillar:?} from {:?}", feet_cell(bot.feet()));
        }
        bot.pillar_up(pillar.base, pillar.height, item)?;
        self.timing.pillar += t.elapsed();
        self.timing.pillars_built += 1;
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
            let hits_target = upcoming.first().is_some_and(|f| pillar.cells().any(|c| c == (f.x, f.y, f.z)));
            let found = !hits_target && self.terrain.route_for_run(pillar.top(), upcoming, 24, 40_000).is_some();
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
        assert!(pillar.height >= 1 && pillar.height <= MAX_HEIGHT);
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
