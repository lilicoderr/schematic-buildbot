//! Scaffold bridges: the ground-legal way off an island.
//!
//! A player can extend a floor sideways ("bridging"): standing at the edge, click the side of
//! the block under your feet, place a block next to it, step onto it. The bot uses this only when
//! no walking route and no pillar reaches a block (for example when it is standing on a machine
//! top higher than it may drop from). Every bridge block is removed again.

use crate::nav::{blocks_cell, cell_feet, Cell, Terrain};
use crate::world::{within_reach, BlockPos};
use std::collections::{HashMap, VecDeque};

/// Longest bridge planned in one go.
pub const MAX_BRIDGE: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub struct Hop {
    /// Feet cell to step to.
    pub to: Cell,
    /// True when the floor under `to` must be built first (it is air).
    pub bridge: bool,
}

/// One bridging step from `c`: the neighbour is free with a free head cell, and has no floor.
fn bridge_hops(t: &Terrain, c: Cell, on_bridge: bool) -> Vec<Cell> {
    let (x, y, z) = c;
    let mut out = Vec::new();
    if !on_bridge && !t.can_stand(c) {
        return out;
    }
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let n = (x + dx, y, z + dz);
        let floor = (n.0, n.1 - 1, n.2);
        if !t.solid(n) && !t.solid((n.0, n.1 + 1, n.2)) && !t.solid(floor) && !t.avoid.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// Shortest mixed route (walk, jump, drop, bridge) from `start` to a standing cell reaching
/// `target`. Bridge hops are only taken when walking cannot do the job; at most [`MAX_BRIDGE`].
pub fn plan_bridge(t: &Terrain, start: Cell, target: BlockPos, radius: i32, max_nodes: usize) -> Option<Vec<Hop>> {
    // a cell reached by bridging gets its floor when the bot arrives, so it counts as standable
    let ok = |c: Cell, used: usize| {
        (t.can_stand(c) || (used > 0 && !t.solid(c) && !t.solid((c.0, c.1 + 1, c.2))))
            && within_reach(cell_feet(c), target)
            && !blocks_cell(c, target)
            && !t.avoid.contains(&c)
    };
    // state = (cell, bridges used)
    let mut came: HashMap<(Cell, usize), ((Cell, usize), bool)> = HashMap::new();
    let mut queue = VecDeque::from([(start, 0usize)]);
    came.insert((start, 0), ((start, 0), false));
    let mut visited_cells: HashMap<Cell, usize> = HashMap::from([(start, 0)]);
    while let Some((cur, used)) = queue.pop_front() {
        if ok(cur, used) && used > 0 {
            let mut hops = Vec::new();
            let mut at = (cur, used);
            while at.0 != start || at.1 != 0 {
                let (prev, bridge) = came[&at];
                hops.push(Hop { to: at.0, bridge });
                at = prev;
            }
            hops.reverse();
            return Some(hops);
        }
        if came.len() > max_nodes {
            return None;
        }
        let mut next: Vec<(Cell, bool)> = t.steps_from(cur).into_iter().map(|n| (n, false)).collect();
        if used < MAX_BRIDGE {
            next.extend(bridge_hops(t, cur, used > 0).into_iter().map(|n| (n, true)));
        }
        for (n, b) in next {
            let cost = used + usize::from(b);
            let far = (n.0 - start.0).abs().max((n.2 - start.2).abs()) > radius;
            // keep the cheapest (fewest bridges) arrival per cell
            if far || visited_cells.get(&n).is_some_and(|&u| u <= cost) {
                continue;
            }
            visited_cells.insert(n, cost);
            came.insert((n, cost), ((cur, used), b));
            if b {
                queue.push_back((n, cost));
            } else {
                queue.push_front((n, cost)); // free hops first (0-1 BFS)
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A 3x3 tower, 6 high, on flat ground: the only way off it that is not a drop of > 3 is a bridge.
    fn island() -> Terrain {
        let mut t = Terrain::new(0, HashSet::new());
        for y in 0..6 {
            for x in 0..3 {
                for z in 0..3 {
                    t.add_block((x, y, z), true);
                }
            }
        }
        t
    }

    #[test]
    fn a_bridge_leaves_an_island_that_walking_cannot() {
        let t = island();
        let start = (1, 6, 1);
        let target = BlockPos::new(8, 6, 1);
        assert!(t.route_for_run(start, &[target], 24, 20_000).is_none(), "walking cannot leave the tower");
        let hops = plan_bridge(&t, start, target, 24, 20_000).expect("bridge");
        assert!(hops.iter().any(|h| h.bridge));
        assert!(hops.iter().filter(|h| h.bridge).count() <= MAX_BRIDGE);
        // every bridge hop goes to a cell with no floor yet, and the previous cell can carry the click
        let mut at = start;
        for h in &hops {
            if h.bridge {
                assert!(!t.solid((h.to.0, h.to.1 - 1, h.to.2)));
            }
            assert_eq!(h.to.1, at.1, "bridging stays level");
            at = h.to;
        }
        assert!(within_reach(cell_feet(at), target));
    }
}
