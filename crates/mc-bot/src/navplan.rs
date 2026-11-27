//! One search for every ground-legal way to get somewhere: walk, jump up one, drop, and the three
//! scaffold moves a player on foot has: pillar up (jump and put a block under the feet), bridge
//! (put a floor block beside the block we stand on and step onto it) and dig down (break the
//! scaffold block we stand on). A* with a bounded node budget; the bot never flies.

use crate::nav::{blocks_cell, cell_feet, Cell, Terrain};
use crate::world::{within_reach, BlockPos};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Walk, jump up one or drop to the cell.
    Walk(Cell),
    /// Place a scaffold floor under the (level, adjacent) cell and step onto it.
    Bridge(Cell),
    /// Put a scaffold block into the current feet cell and land on it; the cell is the new feet.
    Pillar(Cell),
    /// Break the scaffold block under the feet and fall to the cell.
    Dig(Cell),
}

impl Act {
    pub fn to(self) -> Cell {
        match self {
            Act::Walk(c) | Act::Bridge(c) | Act::Pillar(c) | Act::Dig(c) => c,
        }
    }
    fn cost(self, from: Cell) -> u32 {
        match self {
            Act::Walk(c) if c.1 == from.1 => 4,
            Act::Walk(_) => 6,
            Act::Bridge(_) | Act::Pillar(_) => 14,
            Act::Dig(_) => 8,
        }
    }
}

pub struct Query<'a> {
    pub terrain: &'a Terrain,
    /// Scaffold blocks currently standing (only these may be dug).
    pub scaffold: &'a HashSet<Cell>,
    /// Cells no new scaffold may be put into.
    pub keep_free: &'a [Cell],
    pub max_nodes: usize,
}

impl Query<'_> {
    fn free(&self, c: Cell) -> bool {
        !self.terrain.solid(c)
    }

    /// Moves from `c`. `built` says the floor under `c` is scaffold planned earlier on this path.
    pub fn acts(&self, c: Cell) -> Vec<Act> {
        let (x, y, z) = c;
        let t = self.terrain;
        let mut out: Vec<Act> = t.steps_from(c).into_iter().map(Act::Walk).collect();
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let n = (x + dx, y, z + dz);
            let floor = (n.0, y - 1, n.2);
            if y > t.ground_y && self.free(n) && self.free((n.0, y + 1, n.2)) && self.free(floor) && !self.keep_free.contains(&floor) {
                out.push(Act::Bridge(n));
            }
        }
        if y < t.sky_top + 2 && self.free((x, y + 2, z)) && self.free((x, y + 1, z)) && !self.keep_free.contains(&c) {
            out.push(Act::Pillar((x, y + 1, z)));
        }
        if self.scaffold.contains(&(x, y - 1, z)) {
            // fall until something carries us, at most 3 blocks
            for land in (y - 3..y).rev() {
                let below = (x, land - 1, z);
                if t.carries(below) {
                    out.push(Act::Dig((x, land, z)));
                    break;
                }
                if !self.free(below) {
                    break;
                }
            }
        }
        out
    }

    /// Cheapest act sequence from `start` to a cell satisfying `goal`; `aim` guides the search.
    pub fn search(&self, start: Cell, aim: BlockPos, goal: &dyn Fn(Cell) -> bool) -> Option<Vec<Act>> {
        let h = |c: Cell| -> u32 {
            let flat = ((c.0 - aim.x).abs() + (c.2 - aim.z).abs() - 5).max(0);
            let up = (aim.y - c.1 - 4).max(0) + (c.1 - aim.y - 5).max(0);
            // weighted (x1.5): far faster than exact A*, routes stay near the shortest
            (flat * 6 + up * 12) as u32
        };
        let mut best: HashMap<Cell, u32> = HashMap::from([(start, 0)]);
        let mut came: HashMap<Cell, (Cell, Act)> = HashMap::new();
        let mut open = BinaryHeap::from([Reverse((h(start), 0u32, start))]);
        let mut expanded = 0;
        while let Some(Reverse((_, g, cur))) = open.pop() {
            if g > best[&cur] {
                continue;
            }
            if goal(cur) {
                let mut path = Vec::new();
                let mut at = cur;
                while at != start {
                    let (prev, act) = came[&at];
                    path.push(act);
                    at = prev;
                }
                path.reverse();
                return Some(path);
            }
            expanded += 1;
            if expanded > self.max_nodes {
                return None;
            }
            for act in self.acts(cur) {
                let next = act.to();
                let ng = g + act.cost(cur);
                if best.get(&next).is_none_or(|&old| ng < old) {
                    best.insert(next, ng);
                    came.insert(next, (cur, act));
                    open.push(Reverse((ng + h(next), ng, next)));
                }
            }
        }
        None
    }

    /// A route to a cell from which `target` can be placed.
    pub fn to_place(&self, start: Cell, target: BlockPos) -> Option<Vec<Act>> {
        let goal = |c: Cell| within_reach(cell_feet(c), target) && !blocks_cell(c, target) && !self.terrain.avoid.contains(&c);
        self.search(start, target, &goal)
    }
}

#[cfg(test)]
#[path = "navplan_tests.rs"]
mod tests;
