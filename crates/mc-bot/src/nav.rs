//! Ground-legal navigation: where the bot may stand and how it gets there.
//!
//! The bot only ever stands on something solid. It walks, jumps up one block, or drops a few
//! blocks, exactly like a player on foot, so it can climb onto floors it has just built.
//! Cells the finished blueprint will occupy are off limits for standing: a block placed into the
//! bot's own cell would be rejected (or shove the bot).

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

    fn solid(&self, c: Cell) -> bool {
        c.1 < self.ground_y || self.body.contains(&c)
    }

    fn carries(&self, c: Cell) -> bool {
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
    move |c| t.can_stand(c) && within_reach(cell_feet(c), target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> Terrain {
        Terrain::new(0, HashSet::new())
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
