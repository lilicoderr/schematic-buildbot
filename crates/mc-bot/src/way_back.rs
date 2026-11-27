//! Getting back to a given cell from wherever the bot ended up (checkpoint repair, resume). The
//! planned position can be high on temporary bridges whose access the plan has since taken down,
//! so walking alone is not enough: new scaffold may be needed. It is remembered by the caller and
//! broken as soon as it is within reach, at the latest after the last step.

use super::{pos, Mover, Site};
use crate::nav::{feet_cell, Cell};
use crate::navplan::Act;
use crate::world::{eye_to_box, CLICK_REACH};
use std::io;

/// Searches tried before giving up (execution may stop early, then we search again).
const TRIES: u32 = 8;

impl Site {
    /// Treat `c` as a standing scaffold block that may be dug again.
    pub fn adopt(&mut self, c: Cell) {
        self.terrain.add_block(c, true);
        self.scaffold.insert(c);
    }

    /// Scaffold blocks still standing, in a stable order.
    pub fn standing(&self) -> Vec<Cell> {
        let mut v: Vec<Cell> = self.scaffold.iter().copied().collect();
        v.sort_unstable();
        v
    }

    /// Get the feet to exactly `goal`: walk if possible, otherwise with scaffold moves, searched
    /// in the world as it is now. False if no legal way was found.
    pub fn go_to(&mut self, bot: &mut impl Mover, goal: Cell) -> io::Result<bool> {
        for _ in 0..TRIES {
            let here = feet_cell(bot.feet());
            if here == goal || bot.kicked() {
                break;
            }
            let acts = match self.terrain.route(here, &|c| c == goal, 160, 400_000) {
                Some(route) => route.iter().skip(1).map(|&c| Act::Walk(c)).collect(),
                None => match self.search_far(here, pos(goal), &|c| c == goal) {
                    Some(acts) => acts,
                    None => return Ok(false),
                },
            };
            self.perform(bot, &acts)?;
        }
        Ok(feet_cell(bot.feet()) == goal)
    }

    /// Break every standing scaffold block within reach, except the one under the feet. Only for
    /// use where the bot's own footing does not depend on scaffold of this site.
    pub fn drop_in_reach(&mut self, bot: &mut impl Mover) -> io::Result<()> {
        let mut all = self.standing();
        all.sort_unstable_by_key(|c| (-c.1, *c));
        for c in all {
            let here = feet_cell(bot.feet());
            if c != (here.0, here.1 - 1, here.2) && eye_to_box(bot.feet(), pos(c)) <= CLICK_REACH {
                self.remove(bot, c)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nav::{cell_feet, Terrain};
    use crate::plan::ClickStep;
    use crate::world::{BlockPos, Vec3};
    use std::collections::HashSet;

    /// Moves the feet and refuses any position that nothing carries (no hovering).
    struct Walker {
        feet: Vec3,
        /// Solid cells above the ground (y >= 0).
        solid: HashSet<Cell>,
    }

    impl Walker {
        fn stand(&mut self, c: Cell) {
            assert!(c.1 == 0 || self.solid.contains(&(c.0, c.1 - 1, c.2)), "hovering at {c:?}");
            assert!(!self.solid.contains(&c), "inside a block at {c:?}");
            self.feet = cell_feet(c);
        }
    }

    impl Mover for Walker {
        fn feet(&self) -> Vec3 {
            self.feet
        }
        fn kicked(&self) -> bool {
            false
        }
        fn walk_cells(&mut self, route: &[Cell]) -> io::Result<()> {
            route.iter().skip(1).for_each(|&c| self.stand(c));
            Ok(())
        }
        fn pillar_up(&mut self, base: Cell, _item: i32) -> io::Result<()> {
            self.solid.insert(base);
            self.stand((base.0, base.1 + 1, base.2));
            Ok(())
        }
        fn dig_down(&mut self, floor: BlockPos, land_y: i32) -> io::Result<()> {
            self.solid.remove(&(floor.x, floor.y, floor.z));
            self.stand((floor.x, land_y, floor.z));
            Ok(())
        }
        fn break_block(&mut self, at: BlockPos) -> io::Result<()> {
            assert!(eye_to_box(self.feet, at) <= CLICK_REACH, "break out of reach");
            assert!(self.solid.remove(&(at.x, at.y, at.z)));
            Ok(())
        }
        fn place_support(&mut self, at: BlockPos, click: &ClickStep) -> io::Result<()> {
            let s = click.support;
            assert!(s.y < 0 || self.solid.contains(&(s.x, s.y, s.z)), "clicked air");
            self.solid.insert((at.x, at.y, at.z));
            Ok(())
        }
    }

    /// The failure of the large factory run: after a repair the bot is on the ground, and the plan
    /// expects it on a bridge high up whose access has been taken down.
    #[test]
    fn gets_back_onto_a_high_bridge_with_scaffold_and_remembers_it() {
        // a wall 12 high with a two-block ledge sticking out of its top
        let mut planned: HashSet<Cell> = (0..12).map(|y| (8, y, 0)).collect();
        planned.extend([(7, 11, 0), (6, 11, 0)]);
        let mut terrain = Terrain::new(0, HashSet::new());
        planned.iter().for_each(|&c| terrain.add_block(c, true));
        terrain.sky_top = 14;
        let goal = (6, 12, 0);
        assert!(terrain.route((0, 0, 0), &|c| c == goal, 160, 400_000).is_none(), "walking alone cannot");

        let mut bot = Walker { feet: cell_feet((0, 0, 0)), solid: planned.clone() };
        let mut site = Site::new(terrain);
        assert!(site.go_to(&mut bot, goal).unwrap());
        assert_eq!(feet_cell(bot.feet), goal);
        site.drop_in_reach(&mut bot).unwrap();
        // what is left is exactly what the site reports, and no planned block was dug
        let extra: HashSet<Cell> = bot.solid.difference(&planned).copied().collect();
        assert_eq!(extra, site.standing().into_iter().collect());
        assert!(planned.is_subset(&bot.solid));
        assert!(extra.iter().all(|&c| eye_to_box(bot.feet, pos(c)) > CLICK_REACH));
    }

    #[test]
    fn a_goal_sealed_in_blocks_is_reported_not_faked() {
        let mut terrain = Terrain::new(0, HashSet::new());
        for c in [(5, 0, 0), (5, 1, 0)] {
            terrain.add_block(c, true);
        }
        let mut bot = Walker { feet: cell_feet((0, 0, 0)), solid: [(5, 0, 0), (5, 1, 0)].into() };
        assert!(!Site::new(terrain).go_to(&mut bot, (5, 0, 0)).unwrap());
    }
}
