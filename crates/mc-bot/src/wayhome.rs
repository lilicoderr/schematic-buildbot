//! A remembered way down to the ground. Cleanup asks "may this scaffold block go without
//! stranding the bot?" before every removal; one search answers that for every later question
//! about blocks the remembered way does not use.

use crate::nav::Cell;
use crate::navplan::{Act, Query};
use std::collections::HashMap;

#[derive(Default)]
pub struct WayHome {
    /// Cell on the way -> its index (0 = where the way was found from).
    at: HashMap<Cell, usize>,
    /// Block or cell -> the last index whose move still depends on it.
    needed: HashMap<Cell, usize>,
}

fn under(c: Cell) -> Cell {
    (c.0, c.1 - 1, c.2)
}

impl WayHome {
    /// Remember the way `acts` leading from `start`.
    pub fn new(query: &Query, start: Cell, acts: &[Act]) -> Self {
        let mut way = WayHome::default();
        let mut cur = start;
        for (i, &act) in acts.iter().enumerate() {
            way.at.insert(cur, i);
            way.needed.insert(under(cur), i);
            match act {
                Act::Walk(_) | Act::Dig(_) => {}
                Act::Bridge(to) => {
                    way.needed.insert(under(to), i);
                }
                Act::StepDown(to) => {
                    way.needed.insert(under(to), i);
                    if let Some((side, _)) = query.side_support(under(to)) {
                        way.needed.insert(side, i);
                    }
                }
                Act::Pillar(_) => {
                    way.needed.insert(cur, i);
                }
            }
            cur = act.to();
        }
        way.at.insert(cur, acts.len());
        way.needed.insert(under(cur), acts.len());
        way
    }

    fn holds_from(&self, cell: Cell, gone: Cell) -> bool {
        self.at.get(&cell).is_some_and(|&i| self.needed.get(&gone).is_none_or(|&last| last < i))
    }

    /// Whether the remembered way surely still works from `here` once the block `gone` is
    /// removed (`false` only means a new search is needed).
    pub fn still_leads_down(&self, query: &Query, here: Cell, gone: Cell) -> bool {
        if self.holds_from(here, gone) {
            return true;
        }
        // one level step off the way: walking back onto it is enough
        under(here) != gone
            && query.acts(here).iter().any(|a| matches!(a, Act::Walk(to) if self.holds_from(*to, gone)))
    }
}
