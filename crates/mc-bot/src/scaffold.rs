//! Temporary scaffold and the `Site` that moves the player around while building.
//!
//! The bot never flies. When no walking route reaches a block it uses [`crate::navplan`]: like a
//! player it pillars up (jump, block under the feet), bridges sideways, and digs its own scaffold
//! again. Every scaffold block is broken before the build ends.

use crate::blocks;
use crate::builder::PlayClient;
use crate::nav::{blocks_cell, cell_feet, feet_cell, Cell, Terrain};
use crate::navplan::{Act, Query};
use crate::plan::ClickStep;
use crate::wayhome::WayHome;
use crate::world::{eye_to_box, within_reach, BlockPos, Face, Vec3, CLICK_REACH};
use std::collections::HashSet;
use std::io;
use std::time::{Duration, Instant};

#[path = "cleanup.rs"]
mod cleanup;
#[path = "way_back.rs"]
mod way_back;

/// Node budget of one scaffold-navigation search.
const SEARCH_NODES: usize = 60_000;
/// Budget of the rare long-distance search for scaffold left far away.
const FAR_NODES: usize = 600_000;

/// What the planner needs from whatever moves the player: the live client, or a recorder that
/// writes plan steps instead (see `slice`).
pub trait Mover {
    fn feet(&self) -> Vec3;
    fn kicked(&self) -> bool;
    fn walk_cells(&mut self, route: &[Cell]) -> io::Result<()>;
    /// Jump, put a scaffold block into `base` (the current feet cell), land on it.
    fn pillar_up(&mut self, base: Cell, item_id: i32) -> io::Result<()>;
    /// Break the scaffold block at `floor` under the feet and fall to height `land_y`.
    fn dig_down(&mut self, floor: BlockPos, land_y: i32) -> io::Result<()>;
    /// Break one scaffold block that is within reach.
    fn break_block(&mut self, pos: BlockPos) -> io::Result<()>;
    /// Place one scaffold block at `pos` by the given click.
    fn place_support(&mut self, pos: BlockPos, click: &ClickStep) -> io::Result<()>;
}

impl Mover for PlayClient {
    fn feet(&self) -> Vec3 {
        self.feet
    }
    fn kicked(&self) -> bool {
        self.kicked.is_some()
    }
    fn walk_cells(&mut self, route: &[Cell]) -> io::Result<()> {
        PlayClient::walk_cells(self, route)
    }
    fn pillar_up(&mut self, base: Cell, item_id: i32) -> io::Result<()> {
        PlayClient::pillar_up(self, base, 1, item_id)
    }
    fn dig_down(&mut self, floor: BlockPos, land_y: i32) -> io::Result<()> {
        PlayClient::break_block(self, floor)?;
        self.fall_to(land_y as f64)
    }
    fn break_block(&mut self, pos: BlockPos) -> io::Result<()> {
        PlayClient::break_block(self, pos)
    }
    fn place_support(&mut self, pos: BlockPos, click: &ClickStep) -> io::Result<()> {
        PlayClient::place_support(self, pos, click.support, click.face, click.cursor)
    }
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

/// What the bot stands on and around while building: the terrain model plus the scaffold blocks
/// still standing.
pub struct Site {
    pub terrain: Terrain,
    /// Never iterated where the order could change the plan.
    scaffold: HashSet<Cell>,
    /// During cleanup: cells whose scaffold was removed twice (never rebuilt, so cleanup
    /// terminates), and those removed once.
    spent: Option<HashSet<Cell>>,
    removed: HashSet<Cell>,
    /// Ways down to the ground found so far.
    way: WayHome,
    pub timing: Timing,
}

fn pos(c: Cell) -> BlockPos {
    BlockPos::new(c.0, c.1, c.2)
}

impl Site {
    pub fn new(terrain: Terrain) -> Self {
        Site { terrain, scaffold: HashSet::new(), spent: None, removed: HashSet::new(), way: WayHome::default(), timing: Timing::default() }
    }

    fn forget(&mut self, c: Cell) {
        self.terrain.remove_block(c);
        self.scaffold.remove(&c);
        if let Some(s) = &mut self.spent {
            if !self.removed.insert(c) {
                s.insert(c);
            }
        }
        self.timing.pillars_removed += 1;
    }

    fn remove(&mut self, bot: &mut impl Mover, c: Cell) -> io::Result<()> {
        bot.break_block(pos(c))?;
        self.forget(c);
        Ok(())
    }

    /// Whether the player at `from` can still get down to ground level with legal moves.
    pub fn grounded(&mut self, from: Cell) -> bool {
        let ground = self.terrain.ground_y;
        if from.1 <= ground {
            return true;
        }
        self.search(from, BlockPos::new(from.0, ground, from.2), &|c| c.1 <= ground, &[]).is_some()
    }

    /// Would the player at `here` still get down after the scaffold block `c` is gone?
    fn safe_without(&mut self, here: Cell, c: Cell) -> bool {
        self.terrain.remove_block(c);
        self.scaffold.remove(&c);
        // the answer must hold after `forget(c)`, which may bar rebuilding there
        let bars = self.removed.contains(&c) && self.spent.as_mut().is_some_and(|s| s.insert(c));
        let ok = self.way_down_from(here);
        if let (true, Some(s)) = (bars, &mut self.spent) {
            s.remove(&c);
        }
        self.terrain.add_block(c, true);
        self.scaffold.insert(c);
        ok
    }

    /// [`Site::grounded`], answered from the remembered ways where possible.
    fn way_down_from(&mut self, here: Cell) -> bool {
        let ground = self.terrain.ground_y;
        if here.1 <= ground {
            return true;
        }
        let t = Instant::now();
        let query = Query { terrain: &self.terrain, scaffold: &self.scaffold, keep_free: &[], spent: self.spent.as_ref(), max_nodes: SEARCH_NODES };
        let way = &mut self.way;
        let ok = way.leads_down(&query, here) || {
            let joins = |c: Cell| c.1 <= ground || way.leads_down(&query, c);
            let found = query.search(here, BlockPos::new(here.0, ground, here.2), &joins);
            if let Some(acts) = &found {
                way.remember(here, acts);
            }
            found.is_some()
        };
        self.timing.search += t.elapsed();
        ok
    }

    /// Perform a searched act sequence. Stops early if the world no longer allows the next act (the
    /// caller searches again).
    fn perform(&mut self, bot: &mut impl Mover, acts: &[Act]) -> io::Result<()> {
        self.timing.routes += 1;
        for act in acts {
            let here = feet_cell(bot.feet());
            let query = Query { terrain: &self.terrain, scaffold: &self.scaffold, keep_free: &[], spent: self.spent.as_ref(), max_nodes: 0 };
            if bot.kicked() || !query.acts(here).contains(act) {
                break;
            }
            let t = Instant::now();
            match *act {
                Act::Walk(to) => {
                    bot.walk_cells(&[here, to])?;
                    self.timing.steps += 1;
                }
                Act::Bridge(to) => {
                    let floor = pos((to.0, to.1 - 1, to.2));
                    let from = pos((here.0, here.1 - 1, here.2));
                    let face = Face::HORIZONTAL.iter().copied().find(|f| from.offset(*f) == floor).expect("adjacent");
                    bot.place_support(floor, &ClickStep { support: from, face, cursor: crate::support::face_cursor(face) })?;
                    self.add_scaffold((floor.x, floor.y, floor.z));
                    bot.walk_cells(&[here, to])?;
                    self.timing.steps += 1;
                }
                Act::StepDown(to) => {
                    let floor = (to.0, to.1 - 1, to.2);
                    let (s, face) = query.side_support(floor).expect("checked by acts");
                    bot.place_support(pos(floor), &ClickStep { support: pos(s), face, cursor: crate::support::face_cursor(face) })?;
                    self.add_scaffold(floor);
                    bot.walk_cells(&[here, to])?;
                    self.timing.steps += 1;
                }
                Act::Pillar(_) => {
                    bot.pillar_up(here, blocks::item_id("cobblestone").unwrap_or(1))?;
                    self.add_scaffold(here);
                }
                Act::Dig(to) => {
                    let floor = (here.0, here.1 - 1, here.2);
                    bot.dig_down(pos(floor), to.1)?;
                    self.forget(floor);
                }
            }
            match act {
                Act::Walk(_) => self.timing.walk += t.elapsed(),
                _ => self.timing.pillar += t.elapsed(),
            }
        }
        Ok(())
    }

    fn add_scaffold(&mut self, c: Cell) {
        self.terrain.add_block(c, true);
        self.scaffold.insert(c);
        self.timing.pillars_built += 1;
    }

    fn search(&mut self, from: Cell, aim: BlockPos, goal: &dyn Fn(Cell) -> bool, keep_free: &[Cell]) -> Option<Vec<Act>> {
        let t = Instant::now();
        let query = Query { terrain: &self.terrain, scaffold: &self.scaffold, keep_free, spent: self.spent.as_ref(), max_nodes: SEARCH_NODES };
        let found = query.search(from, aim, goal);
        self.timing.search += t.elapsed();
        found
    }

    /// `search` with room for a long way (cleanup only, when everything near is done).
    fn search_far(&mut self, from: Cell, aim: BlockPos, goal: &dyn Fn(Cell) -> bool) -> Option<Vec<Act>> {
        let t = Instant::now();
        let query = Query { terrain: &self.terrain, scaffold: &self.scaffold, keep_free: &[], spent: self.spent.as_ref(), max_nodes: FAR_NODES };
        let found = query.search(from, aim, goal);
        self.timing.search += t.elapsed();
        found
    }

    /// Like `search`, but plain cheapest-first towards whatever cell satisfies `goal`.
    fn nearest(&mut self, from: Cell, goal: &dyn Fn(Cell) -> bool) -> Option<Vec<Act>> {
        let t = Instant::now();
        let query = Query { terrain: &self.terrain, scaffold: &self.scaffold, keep_free: &[], spent: self.spent.as_ref(), max_nodes: SEARCH_NODES };
        let found = query.nearest(from, goal);
        self.timing.search += t.elapsed();
        found
    }

    fn can_place_from_here(&self, bot: &impl Mover, target: BlockPos) -> bool {
        let here = feet_cell(bot.feet());
        within_reach(bot.feet(), target) && !blocks_cell(here, target) && !self.terrain.avoid.contains(&here)
    }

    /// Get to a spot from which `upcoming[0]` can be placed: stay, walk, or use scaffold. When
    /// nothing legal reaches the block the bot stays put and the caller reports it unreachable.
    pub fn approach(&mut self, bot: &mut impl Mover, upcoming: &[BlockPos]) -> io::Result<()> {
        let first = upcoming[0];
        let target = (first.x, first.y, first.z);
        if self.scaffold.contains(&target) && !self.clear_one(bot, target)? {
            return Ok(());
        }
        if self.can_place_from_here(bot, first) {
            return Ok(());
        }
        let start = feet_cell(bot.feet());
        let t = Instant::now();
        let walk = self.terrain.route_for_run(start, upcoming, 24, 6_000);
        self.timing.search += t.elapsed();
        if let Some(route) = walk {
            let acts: Vec<Act> = route.iter().skip(1).map(|&c| Act::Walk(c)).collect();
            return self.perform(bot, &acts);
        }
        // Scaffold navigation; execution may stop early, so search again a few times.
        for _ in 0..4 {
            let avoid = self.terrain.avoid.clone();
            let goal = |c: Cell| within_reach(cell_feet(c), first) && !blocks_cell(c, first) && !avoid.contains(&c);
            let Some(acts) = self.search(feet_cell(bot.feet()), first, &goal, &[target]) else { break };
            self.perform(bot, &acts)?;
            if self.can_place_from_here(bot, first) || bot.kicked() {
                break;
            }
        }
        Ok(())
    }

    /// Break one specific scaffold block, moving to a spot that reaches it if needed.
    fn clear_one(&mut self, bot: &mut impl Mover, c: Cell) -> io::Result<bool> {
        for _ in 0..4 {
            if !self.scaffold.contains(&c) {
                return Ok(true);
            }
            let here = feet_cell(bot.feet());
            if c == (here.0, here.1 - 1, here.2) {
                // the fall rule (and its checks) live in the search: use its dig move
                if let Some(land) = self.dig_landing(here) {
                    bot.dig_down(pos(c), land)?;
                    self.forget(c);
                    return Ok(true);
                }
            }
            let over = |g: Cell| g.0 == c.0 && g.2 == c.2 && g.1 > c.1;
            if !over(here) && eye_to_box(bot.feet(), pos(c)) <= CLICK_REACH {
                if self.safe_without(here, c) {
                    self.remove(bot, c)?;
                    return Ok(true);
                }
                // it is our only way down: go down first, then come back for it
                let ground = self.terrain.ground_y;
                let Some(acts) = self.search(here, BlockPos::new(here.0, ground, here.2), &|g| g.1 <= ground, &[]) else {
                    return Ok(false);
                };
                self.perform(bot, &acts)?;
                continue;
            }
            let goal = |g: Cell| eye_to_box(cell_feet(g), pos(c)) <= CLICK_REACH && !over(g);
            let Some(acts) = self.search(here, pos(c), &goal, &[]) else { return Ok(false) };
            self.perform(bot, &acts)?;
        }
        Ok(!self.scaffold.contains(&c))
    }
}
