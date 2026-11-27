//! Ground-legal navigation: where the bot may stand and how it gets there.
//!
//! The bot only ever stands on something solid. It walks, jumps up one block, or drops a few
//! blocks, exactly like a player on foot, so it can climb onto floors it has just built.
//! Cells that already hold a block are never stood in. Cells the blueprint will fill later may be
//! stood in (that is how a floor is laid from above), but a block is never placed into the bot's
//! own feet or head cell ([`blocks_cell`]): the bot steps aside first.

use crate::world::{within_reach, BlockPos, Vec3};
use std::collections::{HashMap, HashSet, VecDeque};

/// A feet cell: the block the player's feet are in.
pub type Cell = (i32, i32, i32);

/// Largest drop the bot accepts (vanilla fall damage starts above 3 blocks).
const MAX_DROP: i32 = 3;

/// Blocks whose top face is not a full-height walkable surface; standing "on" them would leave
/// the player hovering or sunk in.
pub fn is_standable_surface(name: &str, type_prop: Option<&str>) -> bool {
    if name.ends_with("_slab") {
        return type_prop != Some("bottom");
    }
    // no collision at all, or a top below the block above: standing there hovers or sinks
    const LOW: [&str; 33] = [
        "redstone_wire", "tripwire", "tripwire_hook", "lever", "repeater", "comparator", "short_grass", "fern",
        "dead_bush", "dandelion", "poppy", "blue_orchid", "allium", "azure_bluet", "oxeye_daisy", "cornflower",
        "lily_of_the_valley", "wither_rose", "brown_mushroom", "red_mushroom", "sugar_cane", "cobweb", "snow",
        "soul_sand", "mud", "farmland", "dirt_path", "chest", "trapped_chest", "ender_chest", "cactus",
        "honey_block", "enchanting_table",
    ];
    const LOW_PARTS: [&str; 6] = ["_sign", "pressure_plate", "sapling", "tulip", "candle", "_banner"];
    if LOW.contains(&name) || LOW_PARTS.iter().any(|p| name.contains(p)) {
        return false;
    }
    if name == "jack_o_lantern" {
        return true;
    }
    const THIN: [&str; 14] = [
        "_pane", "bars", "fence", "_wall", "carpet", "trapdoor", "_door", "ladder", "torch", "lantern", "button",
        "rail", "vine", "chain",
    ];
    !THIN.iter().any(|t| name.contains(t)) || name == "sea_lantern"
}

#[derive(Default)]
pub struct Terrain {
    pub ground_y: i32,
    /// Cells with a collision body (placed blocks, scaffold). The bot cannot walk through them.
    body: HashSet<Cell>,
    /// The subset of `body` that can carry the player.
    support: HashSet<Cell>,
    /// Every cell of the finished blueprint: never stood in, never walked through.
    reserved: HashSet<Cell>,
    /// Standing cells the planner must not pick right now (they would seal the bot in).
    pub avoid: HashSet<Cell>,
    /// Highest cell row the blueprint reaches; a standing cell with nothing solid above it up to
    /// here sees the open sky (the bot can always pillar out of it).
    pub sky_top: i32,
    /// Set by `route_for_run` when its search was cut short by the radius or node limit, i.e. a
    /// wider search could still find a route.
    pub pruned: std::cell::Cell<bool>,
}

impl Terrain {
    pub fn new(ground_y: i32, reserved: HashSet<Cell>) -> Self {
        Terrain { ground_y, reserved, ..Default::default() }
    }

    pub fn add_block(&mut self, c: Cell, standable: bool) {
        self.body.insert(c);
        if standable {
            self.support.insert(c);
        }
    }

    pub fn remove_block(&mut self, c: Cell) {
        self.body.remove(&c);
        self.support.remove(&c);
    }

    pub(crate) fn solid(&self, c: Cell) -> bool {
        c.1 < self.ground_y || self.body.contains(&c)
    }

    pub(crate) fn carries(&self, c: Cell) -> bool {
        c.1 < self.ground_y || self.support.contains(&c)
    }

    /// Feet fit at `c`: carried from below, body and head cells free and not reserved.
    pub fn can_stand(&self, c: Cell) -> bool {
        let (x, y, z) = c;
        self.carries((x, y - 1, z))
            && !self.solid(c)
            && !self.solid((x, y + 1, z))
            && !self.reserved.contains(&c)
            && !self.reserved.contains(&(x, y + 1, z))
    }

    /// Cells reachable in one step from `c` (walk, jump up one block, drop up to `MAX_DROP`).
    pub fn steps_from(&self, c: Cell) -> Vec<Cell> {
        let (x, y, z) = c;
        let mut out = Vec::new();
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (nx, nz) = (x + dx, z + dz);
            if self.can_stand((nx, y, nz)) {
                out.push((nx, y, nz));
            } else if self.can_stand((nx, y + 1, nz)) && !self.solid((x, y + 2, z)) {
                out.push((nx, y + 1, nz));
            } else {
                for drop in 1..=MAX_DROP {
                    let landing = (nx, y - drop, nz);
                    let column_clear = (y - drop + 1..=y + 1).all(|yy| !self.solid((nx, yy, nz)));
                    if column_clear && self.can_stand(landing) {
                        out.push(landing);
                        break;
                    }
                }
            }
        }
        out
    }

    /// Breadth-first route from `start` to the nearest cell satisfying `goal` (start included).
    /// `start` itself may be anywhere; the search stays within `radius` cells of it.
    pub fn route(&self, start: Cell, goal: &dyn Fn(Cell) -> bool, radius: i32, max_nodes: usize) -> Option<Vec<Cell>> {
        let mut came: HashMap<Cell, Cell> = HashMap::from([(start, start)]);
        let mut queue = VecDeque::from([start]);
        while let Some(cur) = queue.pop_front() {
            if goal(cur) {
                let mut path = vec![cur];
                let mut at = cur;
                while at != start {
                    at = came[&at];
                    path.push(at);
                }
                path.reverse();
                return Some(path);
            }
            if came.len() > max_nodes {
                return None;
            }
            for next in self.steps_from(cur) {
                let far = (next.0 - start.0).abs().max((next.2 - start.2).abs()) > radius;
                if !far && !came.contains_key(&next) {
                    came.insert(next, cur);
                    queue.push_back(next);
                }
            }
        }
        None
    }
}

/// How many upcoming blocks (in build order) can be placed from `c` without moving.
pub fn run_from(c: Cell, upcoming: &[BlockPos]) -> usize {
    let feet = cell_feet(c);
    upcoming.iter().take_while(|p| within_reach(feet, **p) && !blocks_cell(c, **p)).count()
}

/// A block placed at `p` would land inside the body of a player whose feet are in `c`.
pub fn blocks_cell(c: Cell, p: BlockPos) -> bool {
    p.x == c.0 && p.z == c.2 && (p.y == c.1 || p.y == c.1 + 1)
}

impl Terrain {
    /// With a block added at `extra`, can a player standing at `from` still walk (jump, drop) to
    /// open ground? False means the area is a pocket of fewer than `cap` cells: placing there
    /// would seal the player in. Searches at most `cap` cells, so a huge open area counts as open.
    pub fn sealed_in(&mut self, from: Cell, extra: Cell, cap: usize) -> bool {
        let had = self.body.contains(&extra);
        if !had {
            self.add_block(extra, false);
        }
        let mut seen: HashSet<Cell> = HashSet::from([from]);
        let mut queue = VecDeque::from([from]);
        let mut open = false;
        while let Some(cur) = queue.pop_front() {
            if self.sky_open(cur) || seen.len() >= cap {
                open = true;
                break;
            }
            for next in self.steps_from(cur) {
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        if !had {
            self.remove_block(extra);
        }
        !open
    }

    /// Nothing solid above the standing cell up to the top of the blueprint.
    pub fn sky_open(&self, c: Cell) -> bool {
        (c.1 + 2..=self.sky_top).all(|y| !self.solid((c.0, y, c.2)))
    }

    /// How many cells a walk from `start` can reach (diagnostics and tests).
    pub fn reachable_count(&self, start: Cell, radius: i32, max_nodes: usize) -> usize {
        let mut seen: HashSet<Cell> = HashSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(cur) = queue.pop_front() {
            if seen.len() > max_nodes {
                break;
            }
            for next in self.steps_from(cur) {
                let far = (next.0 - start.0).abs().max((next.2 - start.2).abs()) > radius;
                if !far && seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        seen.len()
    }

    /// Route to a standing cell that reaches `upcoming[0]` and, among the cells found a few steps
    /// beyond the nearest one, serves the longest run of following blocks without moving again.
    /// Walking costs far more time than placing, so a slightly longer walk to a better spot wins.
    pub fn route_for_run(&self, start: Cell, upcoming: &[BlockPos], radius: i32, max_nodes: usize) -> Option<Vec<Cell>> {
        const SLACK: u32 = 3;
        const STEP_COST: i64 = 1;
        // Stand at the target's height or above (on what is already built): never below a ceiling.
        const BELOW_COST: i64 = 3;
        let first = *upcoming.first()?;
        self.pruned.set(false);
        let mut came: HashMap<Cell, (Cell, u32)> = HashMap::from([(start, (start, 0))]);
        let mut queue = VecDeque::from([start]);
        let mut best: Option<(i64, Cell)> = None;
        let mut limit = u32::MAX;
        while let Some(cur) = queue.pop_front() {
            let depth = came[&cur].1;
            if depth > limit {
                break;
            }
            if self.can_stand(cur) && within_reach(cell_feet(cur), first) && !blocks_cell(cur, first) && !self.avoid.contains(&cur) {
                if limit == u32::MAX {
                    limit = depth + SLACK;
                }
                let score = 3 * run_from(cur, upcoming) as i64 - BELOW_COST * (first.y - cur.1).max(0) as i64 - STEP_COST * (depth - (limit - SLACK)) as i64;
                if best.is_none_or(|(b, _)| score > b) {
                    best = Some((score, cur));
                }
            }
            if came.len() > max_nodes {
                self.pruned.set(true);
                break;
            }
            for next in self.steps_from(cur) {
                let far = (next.0 - start.0).abs().max((next.2 - start.2).abs()) > radius;
                if far {
                    self.pruned.set(true);
                }
                if !far && !came.contains_key(&next) {
                    came.insert(next, (cur, depth + 1));
                    queue.push_back(next);
                }
            }
        }
        let (_, goal) = best?;
        let mut path = vec![goal];
        let mut at = goal;
        while at != start {
            at = came[&at].0;
            path.push(at);
        }
        path.reverse();
        Some(path)
    }
}

/// Where the feet of a player standing in `c` are (cell centre, surface height).
pub fn cell_feet(c: Cell) -> Vec3 {
    Vec3::new(c.0 as f64 + 0.5, c.1 as f64, c.2 as f64 + 0.5)
}

/// The feet cell containing a position (tolerant of float noise just below an integer height).
pub fn feet_cell(p: Vec3) -> Cell {
    (p.x.floor() as i32, (p.y + 1e-3).floor() as i32, p.z.floor() as i32)
}

/// Goal predicate: a standable cell from which `target` is within placement reach.
pub fn reaches<'a>(t: &'a Terrain, target: BlockPos) -> impl Fn(Cell) -> bool + 'a {
    move |c| t.can_stand(c) && within_reach(cell_feet(c), target) && !blocks_cell(c, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> Terrain {
        Terrain::new(0, HashSet::new())
    }

    #[test]
    fn run_route_stands_where_many_upcoming_blocks_are_in_reach() {
        let t = flat();
        // a row of blocks along x = 0..20 at y 0: the first is reachable from many cells, but only
        // standing a bit further along serves the whole run.
        let row: Vec<BlockPos> = (0..8).map(|x| BlockPos::new(x, 0, 3)).collect();
        let near = t.route((-10, 0, 0), &nav_goal(&t, row[0]), 30, 5000).unwrap();
        let smart = t.route_for_run((-10, 0, 0), &row, 30, 5000).unwrap();
        let end = *smart.last().unwrap();
        assert!(t.can_stand(end));
        assert!(run_from(end, &row) >= run_from(*near.last().unwrap(), &row));
        assert!(run_from(end, &row) > run_from(*near.last().unwrap(), &row), "lookahead should beat nearest");
    }

    fn nav_goal<'a>(t: &'a Terrain, p: BlockPos) -> impl Fn(Cell) -> bool + 'a {
        reaches(t, p)
    }

    #[test]
    fn every_planned_standing_position_has_solid_support_below() {
        // A stepped pyramid of standable blocks plus a bars column that must not carry anyone.
        let mut t = flat();
        for (x, z, h) in [(2, 0, 1), (3, 0, 2), (4, 0, 3), (3, 1, 1), (4, 1, 2)] {
            for y in 0..h {
                t.add_block((x, y, z), true);
            }
        }
        t.add_block((1, 0, 0), false);
        let row: Vec<BlockPos> = vec![BlockPos::new(5, 3, 0), BlockPos::new(5, 3, 1), BlockPos::new(6, 3, 1)];
        for start in [(0, 0, 0), (6, 0, 3), (-3, 0, -2)] {
            let route = t.route_for_run(start, &row, 24, 20_000).expect("reachable");
            for c in route.iter().skip(1) {
                assert!(t.can_stand(*c), "{c:?} not standable");
                assert!(t.carries((c.0, c.1 - 1, c.2)), "no support under {c:?}");
            }
        }
        assert!(!t.can_stand((1, 1, 0)), "bars do not carry the player");
    }

    #[test]
    fn walks_on_flat_ground() {
        let t = flat();
        let r = t.route((0, 0, 0), &|c| c == (3, 0, 0), 10, 1000).unwrap();
        assert_eq!(r.len(), 4);
        assert!(r.iter().all(|c| t.can_stand(*c) || *c == (0, 0, 0)));
    }

    #[test]
    fn climbs_onto_a_one_block_step_but_not_two() {
        let mut t = flat();
        t.add_block((2, 0, 0), true);
        let up = t.route((0, 0, 0), &|c| c == (2, 1, 0), 10, 1000).unwrap();
        assert_eq!(*up.last().unwrap(), (2, 1, 0));
        t.add_block((2, 1, 0), true); // a two-high wall cannot be jumped
        assert!(t.route((0, 0, 0), &|c| c == (2, 2, 0), 3, 1000).is_none());
    }

    #[test]
    fn never_stands_where_the_blueprint_will_build() {
        let reserved: HashSet<Cell> = [(1, 0, 0)].into();
        let t = Terrain::new(0, reserved);
        assert!(!t.can_stand((1, 0, 0)));
        // head cell reserved also blocks standing below it
        let t2 = Terrain::new(0, [(5, 1, 5)].into());
        assert!(!t2.can_stand((5, 0, 5)));
    }

    #[test]
    fn standing_on_a_built_floor_reaches_the_interior() {
        let mut t = Terrain::new(0, (0..10).map(|x| (x, 0, 0)).collect());
        for x in 0..10 {
            t.add_block((x, 0, 0), true);
        }
        let want = |c: Cell| c == (9, 1, 0);
        let r = t.route((-1, 0, 0), &want, 20, 1000).unwrap();
        assert_eq!(r[0], (-1, 0, 0));
        assert_eq!(r[1], (0, 1, 0), "steps up onto the first floor block");
        assert!(r.iter().skip(1).all(|c| c.1 == 1));
    }

    #[test]
    fn drops_at_most_three_blocks() {
        let mut t = flat();
        for y in 0..5 {
            t.add_block((0, y, 0), true);
        }
        let from_tower = t.steps_from((0, 5, 0));
        assert!(from_tower.iter().all(|c| c.1 >= 5 - MAX_DROP), "{from_tower:?}");
    }

    #[test]
    fn surfaces_that_cannot_carry_the_player() {
        assert!(is_standable_surface("stone", None));
        assert!(is_standable_surface("oak_stairs", None));
        assert!(is_standable_surface("smooth_stone_slab", Some("top")));
        assert!(!is_standable_surface("smooth_stone_slab", Some("bottom")));
        assert!(!is_standable_surface("iron_bars", None));
        assert!(!is_standable_surface("oak_fence", None));
        assert!(is_standable_surface("sea_lantern", None));
    }
}
