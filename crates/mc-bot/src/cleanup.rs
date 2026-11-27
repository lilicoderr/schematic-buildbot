//! Taking all scaffold down again, top band first.
//!
//! One band of the highest scaffold is worked at a time, so the access scaffold below stays
//! until nothing above needs it. At each standing cell every block of the band in reach goes
//! (while a way down remains); then one cheapest-first search finds the nearest cell from which
//! more of the band can be taken. A band that nothing reaches is set aside until other removals
//! change something.

use super::{pos, Mover, Site};
use crate::nav::{cell_feet, feet_cell, Cell};
use crate::navplan::{Act, Query};
use crate::world::{eye_to_box, BlockPos, CLICK_REACH};
use std::collections::{HashMap, HashSet};
use std::io;

/// Height of the band of scaffold worked at once.
const BAND: i32 = 6;
/// Rounds without any net removal before cleanup gives up.
const RETRIES: u32 = 3;

/// `c` can be broken by a player in `g` (never from above it).
fn takes(g: Cell, c: Cell) -> bool {
    !(g.0 == c.0 && g.2 == c.2 && g.1 > c.1) && eye_to_box(cell_feet(g), pos(c)) <= CLICK_REACH
}

impl Site {
    /// Remove every remaining scaffold block. Returns how many could not be removed.
    pub fn finish(&mut self, bot: &mut impl Mover) -> io::Result<usize> {
        self.finish_while(bot, &mut |_| true)
    }

    /// Take down, while building goes on, the scaffold below height `below`: construction has
    /// moved past it, and a room may soon be closed over it. What does not come away easily
    /// stays for [`Site::finish`].
    pub fn tidy(&mut self, bot: &mut impl Mover, below: i32) -> io::Result<()> {
        if self.scaffold.iter().any(|c| c.1 < below) {
            self.spent = Some(HashSet::new());
            self.removed.clear();
            let result = self.clean(bot, below, &mut |_| true);
            self.spent = None;
            result?;
        }
        Ok(())
    }

    /// [`Site::finish`] that stops as soon as `keep_going(blocks left)` says so.
    pub fn finish_while(&mut self, bot: &mut impl Mover, keep_going: &mut dyn FnMut(usize) -> bool) -> io::Result<usize> {
        self.spent = Some(HashSet::new());
        self.removed.clear();
        let mut result = self.clean(bot, i32::MAX, keep_going);
        // Rebuilding was barred where it happened twice; while that still helps, lift the bar
        // for what is left.
        let mut before = usize::MAX;
        while let Ok(left) = result {
            if left == 0 || left >= before || !keep_going(left) {
                break;
            }
            before = left;
            self.spent = Some(HashSet::new());
            self.removed.clear();
            result = self.clean(bot, i32::MAX, keep_going);
        }
        self.spent = None;
        result
    }

    /// Break everything of `band` that can go from where the bot stands. Blocks that must stay
    /// for now are put into `later`. True if something was removed.
    fn take_here(&mut self, bot: &mut impl Mover, band: &[Cell], later: &mut HashSet<Cell>) -> io::Result<bool> {
        let mut any = false;
        for &c in band {
            let here = feet_cell(bot.feet());
            if !self.scaffold.contains(&c) {
                continue;
            }
            if c == (here.0, here.1 - 1, here.2) {
                match self.dig_landing(here) {
                    Some(land) if self.safe_without((here.0, land, here.2), c) => {
                        bot.dig_down(pos(c), land)?;
                        self.forget(c);
                        any = true;
                    }
                    _ => {
                        later.insert(c);
                    }
                }
            } else if takes(here, c) {
                if self.safe_without(here, c) {
                    self.remove(bot, c)?;
                    any = true;
                } else {
                    later.insert(c);
                }
            }
        }
        Ok(any)
    }

    fn clean(&mut self, bot: &mut impl Mover, below: i32, keep_going: &mut dyn FnMut(usize) -> bool) -> io::Result<usize> {
        let mut later: HashSet<Cell> = HashSet::new();
        // blocks no search could get near; looked at again only when nothing else is left
        let mut far: HashSet<Cell> = HashSet::new();
        let (mut at_last_retry, mut retries) = (usize::MAX, 0);
        while !bot.kicked() && keep_going(self.scaffold.len()) {
            let open = |c: &&Cell| c.1 < below && !later.contains(c) && !far.contains(c);
            let Some(top) = self.scaffold.iter().filter(open).map(|c| c.1).max() else {
                retries = if self.scaffold.len() < at_last_retry { 0 } else { retries + 1 };
                if below != i32::MAX || self.scaffold.is_empty() || retries >= RETRIES {
                    break;
                }
                at_last_retry = self.scaffold.len();
                // what is left may be what holds the bot up: go down, then come back for it
                let here = feet_cell(bot.feet());
                let ground = self.terrain.ground_y;
                if here.1 > ground {
                    let down = BlockPos::new(here.0, ground, here.2);
                    if let Some(acts) = self.search_far(here, down, &|g| g.1 <= ground) {
                        self.perform(bot, &acts)?;
                    }
                }
                far.clear();
                later.clear();
                continue;
            };
            let mut band: Vec<Cell> = self.scaffold.iter().filter(open).copied().filter(|c| c.1 >= top - BAND).collect();
            band.sort_unstable_by_key(|&c| (-c.1, c));
            if self.take_here(bot, &band, &mut later)? {
                later.clear();
                continue;
            }
            band.retain(|c| !later.contains(c));
            if band.is_empty() {
                continue;
            }
            let mut columns: HashMap<(i32, i32), Vec<Cell>> = HashMap::new();
            for &c in &band {
                columns.entry((c.0 >> 3, c.2 >> 3)).or_default().push(c);
            }
            let goal = |g: Cell| {
                ((g.0 - 6) >> 3..=(g.0 + 6) >> 3).any(|x| {
                    ((g.2 - 6) >> 3..=(g.2 + 6) >> 3)
                        .any(|z| columns.get(&(x, z)).is_some_and(|v| v.iter().any(|&c| takes(g, c))))
                })
            };
            let here = feet_cell(bot.feet());
            // a long way off, the cheapest-first search runs out: then head for the top block
            let found = self.nearest(here, &goal).or_else(|| match below {
                i32::MAX => self.search_far(here, pos(band[0]), &goal),
                _ => None,
            });
            match found {
                Some(acts) if !acts.is_empty() => {
                    self.perform(bot, &acts)?;
                    if feet_cell(bot.feet()) == here {
                        later.extend(band);
                    }
                }
                _ => far.extend(band),
            }
        }
        if !self.scaffold.is_empty() && std::env::var("BUILDBOT_DEBUG").is_ok() {
            let here = feet_cell(bot.feet());
            eprintln!("left standing: {:?}; bot at {here:?}", self.scaffold);
        }
        Ok(self.scaffold.len())
    }

    /// Where the bot lands when it digs the scaffold block under its feet.
    pub(super) fn dig_landing(&self, here: Cell) -> Option<i32> {
        let query = Query { terrain: &self.terrain, scaffold: &self.scaffold, keep_free: &[], spent: None, max_nodes: 0 };
        query.acts(here).into_iter().find_map(|a| match a {
            Act::Dig(to) => Some(to.1),
            _ => None,
        })
    }
}
