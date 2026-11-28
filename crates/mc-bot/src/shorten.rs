//! Post-pass over a finished plan: every run of walk steps is replaced by the quickest walk
//! between its two ends in the world as it stands at that moment. Also the time model of the
//! executor's movement.

use crate::nav::{Cell, Terrain};
use crate::plan::{Plan, Step};
use crate::sim::{self, World};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// Time of one move in hundredths of a second, as the executor performs it sprinting: level steps
/// at 0.28 blocks per tick, a jump in 6 ticks, a drop in 4 ticks plus the fall.
pub fn move_cost(from: Cell, to: Cell) -> u32 {
    match to.1.cmp(&from.1) {
        std::cmp::Ordering::Equal => 18,
        std::cmp::Ordering::Greater => 30,
        std::cmp::Ordering::Less => 25 + 10 * (from.1 - to.1) as u32,
    }
}

/// Estimated seconds spent on all `MOVE` steps of `plan`.
pub fn move_seconds(plan: &Plan) -> f64 {
    let mut feet = plan.header.start;
    let mut total = 0u64;
    for st in &plan.steps {
        match st {
            Step::Move(c) => {
                total += u64::from(move_cost(feet, *c));
                feet = *c;
            }
            Step::Pillar(c) => feet = (c.0, c.1 + 1, c.2),
            Step::Break(p) if p.x == feet.0 && p.z == feet.2 && p.y == feet.1 - 1 => feet.1 -= 1,
            _ => {}
        }
    }
    total as f64 / 100.0
}

/// Cells a search may expand beyond this are not worth it.
const MAX_NODES: usize = 20_000;

/// Cheapest walk from `from` to `to` that costs less than `below` (`from` excluded).
fn quickest(t: &Terrain, from: Cell, to: Cell, below: u32) -> Option<Vec<Cell>> {
    let rest = |c: Cell| 18 * ((c.0 - to.0).unsigned_abs() + (c.2 - to.2).unsigned_abs());
    let mut best: HashMap<Cell, (u32, Cell)> = HashMap::from([(from, (0, from))]);
    let mut open = BinaryHeap::from([Reverse((rest(from), 0u32, from))]);
    while let Some(Reverse((_, g, cur))) = open.pop() {
        if cur == to {
            let mut path = Vec::new();
            let mut at = cur;
            while at != from {
                path.push(at);
                at = best[&at].1;
            }
            path.reverse();
            return Some(path);
        }
        if best.len() > MAX_NODES {
            break;
        }
        for next in t.steps_from(cur) {
            let ng = g + move_cost(cur, next);
            if ng + rest(next) < below && best.get(&next).is_none_or(|b| ng < b.0) {
                best.insert(next, (ng, cur));
                open.push(Reverse((ng + rest(next), ng, next)));
            }
        }
    }
    None
}

/// Bring the walking model's view of cell `c` up to date with the checker's world.
fn track(t: &mut Terrain, w: &World, c: Cell) {
    t.remove_block(c);
    if w.solid(c) {
        t.add_block(c, w.carries(c));
    }
}

/// Shorten every walk of `plan`. A replacement is only taken when the plan checker accepts each
/// of its steps; the caller simulates the whole plan afterwards anyway.
pub fn shorten(plan: &mut Plan) {
    let ground = plan.header.origin.1;
    let mut w = World::new(ground, plan.header.start);
    let mut t = Terrain::new(ground, Default::default());
    let old = std::mem::take(&mut plan.steps);
    let mut out: Vec<Step> = Vec::with_capacity(old.len());
    let mut i = 0;
    while i < old.len() {
        let cells: Vec<Cell> = old[i..].iter().map_while(|s| if let Step::Move(c) = s { Some(*c) } else { None }).collect();
        if let Some(&goal) = cells.last() {
            i += cells.len();
            let from = w.feet;
            let cost = std::iter::once(from).chain(cells.iter().copied()).zip(cells.iter().copied()).map(|(a, b)| move_cost(a, b)).sum();
            let found = (cells.len() > 1).then(|| quickest(&t, from, goal, cost)).flatten();
            let legal = found.filter(|path| {
                w.feet = from;
                path.iter().all(|&c| sim::apply(&mut w, &Step::Move(c)).is_ok())
            });
            w.feet = goal;
            out.extend(legal.unwrap_or(cells).into_iter().map(Step::Move));
            continue;
        }
        let st = &old[i];
        // errors are left for the simulation that follows to report
        let _ = sim::apply(&mut w, st);
        match st {
            Step::Pillar(c) => track(&mut t, &w, *c),
            Step::Place { pos, .. } | Step::Support { pos, .. } | Step::Auto { pos, .. } | Step::Break(pos) => {
                // the cell above too: a second half appears there
                track(&mut t, &w, (pos.x, pos.y, pos.z));
                track(&mut t, &w, (pos.x, pos.y + 1, pos.z));
            }
            _ => {}
        }
        out.push(st.clone());
        i += 1;
    }
    plan.steps = out;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Header;

    #[test]
    fn a_detour_becomes_the_straight_walk() {
        let steps = [(0, 0, 1), (1, 0, 1), (2, 0, 1), (2, 0, 0)].map(Step::Move).to_vec();
        let mut plan = Plan { header: Header { start: (0, 0, 0), ..Default::default() }, steps };
        let before = move_seconds(&plan);
        shorten(&mut plan);
        assert_eq!(plan.steps, [(1, 0, 0), (2, 0, 0)].map(Step::Move).to_vec());
        assert!(move_seconds(&plan) < before);
    }
}
