//! Placements per standing position: before the bot walks anywhere, it places every upcoming
//! block of the current group of layers that it can already place from where it stands. Walking
//! dominates
//! the run time, so this matters far more than the speed of anything else.

use crate::nav::{blocks_cell, feet_cell};
use crate::world::{within_reach, BlockPos, Vec3};
use std::collections::VecDeque;

/// How far down the build order a block may be pulled forward.
pub const WINDOW: usize = 192;

/// The blocks still to place, as indices into the build order.
pub struct Queue {
    pending: VecDeque<usize>,
}

impl Queue {
    pub fn new(indices: impl IntoIterator<Item = usize>) -> Self {
        Queue { pending: indices.into_iter().collect() }
    }

    /// The next block: the first one in the window, in a layer that goes `together` with that of
    /// the oldest pending one (called as oldest, candidate), that is reachable from `feet` without
    /// moving and that `ready` accepts (it has something to rest on and its state can be produced
    /// now). Otherwise the oldest pending block.
    pub fn take(&mut self, positions: &[BlockPos], feet: Vec3, together: impl Fn(i32, i32) -> bool, ready: impl Fn(usize) -> bool) -> Option<usize> {
        let layer = positions[*self.pending.front()?].y;
        let here = feet_cell(feet);
        let at = self
            .pending
            .iter()
            .take(WINDOW)
            .position(|&i| {
                let p = positions[i];
                together(layer, p.y) && within_reach(feet, p) && !blocks_cell(here, p) && ready(i)
            })
            .unwrap_or(0);
        self.pending.remove(at)
    }

    /// `current` followed by the next pending positions, `n` in all: what the standing-cell
    /// chooser should try to serve.
    pub fn upcoming(&self, positions: &[BlockPos], current: usize, n: usize) -> Vec<BlockPos> {
        std::iter::once(current).chain(self.pending.iter().copied()).take(n).map(|i| positions[i]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, y: i32) -> BlockPos {
        BlockPos::new(x, y, 3)
    }

    #[test]
    fn places_what_is_in_reach_before_walking_but_only_in_layers_that_go_together() {
        let positions = [at(40, 0), at(41, 0), at(1, 0), at(2, 1)];
        let feet = Vec3::new(0.5, 0.0, 0.5);
        let mut q = Queue::new(0..4);
        let same = |a: i32, b: i32| a == b;
        assert_eq!(q.take(&positions, feet, same, |_| true), Some(2), "the one in reach goes first");
        // nothing else of layer 0 is in reach: the oldest pending block, not the layer above
        assert_eq!(q.take(&positions, feet, same, |_| true), Some(0));
        assert_eq!(q.upcoming(&positions, 0, 3), vec![positions[0], positions[1], positions[3]]);
        // layers that go together: the block above, in reach, comes before the one far away
        assert_eq!(q.take(&positions, feet, |_, _| true, |_| true), Some(3));
    }

    #[test]
    fn a_block_that_is_not_ready_waits() {
        let positions = [at(40, 0), at(1, 0)];
        let mut q = Queue::new(0..2);
        assert_eq!(q.take(&positions, Vec3::new(0.5, 0.0, 0.5), |a, b| a == b, |i| i != 1), Some(0));
    }
}
