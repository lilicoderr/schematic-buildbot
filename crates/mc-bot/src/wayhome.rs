//! Remembered ways down to the ground. Cleanup asks "may this scaffold block go without
//! stranding the bot?" before every removal. The ways found so far form a tree rooted at the
//! ground; a new search only has to reach any branch that still works.

use crate::nav::Cell;
use crate::navplan::{Act, Query};
use std::collections::HashMap;

#[derive(Default)]
pub struct WayHome {
    /// Cell -> the act that leads one step further down from it.
    next: HashMap<Cell, Act>,
}

impl WayHome {
    /// Whether the remembered acts still lead from `from` to the ground in `query`'s terrain.
    /// Every act is checked again, so changes to the terrain never make the answer stale.
    /// (`false` only means a new search is needed.)
    pub fn leads_down(&self, query: &Query, from: Cell) -> bool {
        let mut at = from;
        // a chain longer than the tree would be a loop
        for _ in 0..=self.next.len() {
            if at.1 <= query.terrain.ground_y {
                return true;
            }
            match self.next.get(&at) {
                Some(act) if query.acts(at).contains(act) => at = act.to(),
                _ => return false,
            }
        }
        false
    }

    /// Add the way `acts` leading from `start`; it ends on the ground or on a working branch.
    pub fn remember(&mut self, start: Cell, acts: &[Act]) {
        let mut at = start;
        for &act in acts {
            self.next.insert(at, act);
            at = act.to();
        }
    }
}
