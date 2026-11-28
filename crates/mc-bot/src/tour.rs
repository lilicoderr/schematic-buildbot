//! Build order for the slicer: a few layers at a time, tile by tile, and inside a tile's band a
//! nearest-neighbour tour in all three directions.
//!
//! Strict whole-layer order made the bot visit every far-apart structure once per layer, high up,
//! bridging out to each and breaking the bridges again (most of the walking on the large factory).
//! So the footprint is cut into tiles and a band of [`BAND_LAYERS`] layers is finished in one tile
//! before the next tile is started. Inside that box the layers are not finished one by one: a wall
//! four high is within reach from one spot, so the bot places all of it instead of walking along it
//! once per layer ([`Order::BANDED`]). The stricter [`Order::LAYERED`] is the slicer's second try
//! for a structure it cannot finish in the banded order (for example scaffold that cannot be taken
//! down again).
//!
//! The row/tile sweep of [`crate::schematic::Schematic::plan_order`] is fine for solid floors but
//! makes the bot cross the layer again and again for sparse layers (machines, stairs, windows).
//! A tour that always continues with the closest block that can already be placed keeps the
//! standing position nearly still. Anchoring rules are the same as in `plan_order`.

use crate::schematic::{Block, Schematic};
use crate::world::{BlockPos, Face};
use std::collections::{BTreeMap, HashSet};

/// Layers finished in one tile before moving to the next one.
pub const BAND_LAYERS: i32 = 4;

/// How the blocks of a schematic are ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order {
    /// Side length of a tile (blocks, x and z).
    pub tile: i32,
    /// Finish each layer of a tile before the next one.
    pub layered: bool,
}

impl Order {
    pub const BANDED: Order = Order { tile: 8, layered: false };
    pub const LAYERED: Order = Order { tile: 16, layered: true };

    /// May blocks at heights `a` and `b` be placed in either order from one standing spot, in a
    /// build whose lowest layer is `floor`?
    pub fn together(self, a: i32, b: i32, floor: i32) -> bool {
        if self.layered {
            a == b
        } else {
            band_base(a, floor) == band_base(b, floor)
        }
    }
}

/// The lowest layer of the band that holds height `y`, in a build whose lowest layer is `floor`.
pub fn band_base(y: i32, floor: i32) -> i32 {
    floor + (y - floor).div_euclid(BAND_LAYERS) * BAND_LAYERS
}

fn dist(a: BlockPos, b: BlockPos) -> i32 {
    (a.x - b.x).abs() + (a.z - b.z).abs() + 2 * (a.y - b.y).abs()
}

/// Same blocks as `plan_order`: band by band, tile by tile (back and forth), then (for a layered
/// order) layer by layer; nearest-neighbour order inside each such group.
pub fn tour_order(schem: &Schematic, order: Order) -> Vec<&Block> {
    let (base, _) = schem.plan_order();
    let (min, _) = schem.bounds();
    let mut layers: BTreeMap<(i32, i32, i32, i32), Vec<&Block>> = BTreeMap::new();
    for b in base {
        let (tx, tz) = ((b.pos.x - min.x) / order.tile, (b.pos.z - min.z) / order.tile);
        // every other column of tiles runs backwards, so neighbours follow each other
        let tz = if tx % 2 == 0 { tz } else { -tz };
        layers.entry((band_base(b.pos.y, min.y), tx, tz, if order.layered { b.pos.y } else { 0 })).or_default().push(b);
    }
    let mut placed: HashSet<BlockPos> = HashSet::new();
    let mut out: Vec<&Block> = Vec::with_capacity(schem.blocks.len());
    let mut at = BlockPos::new(min.x, min.y, min.z);
    for (_, mut rest) in layers {
        while !rest.is_empty() {
            let anchored = |b: &Block| {
                b.pos.y == min.y || placed.contains(&b.pos.below()) || Face::HORIZONTAL.iter().any(|f| placed.contains(&b.pos.offset(*f)))
            };
            // nearest anchored block; ties keep the sweep order of plan_order
            let pick = rest
                .iter()
                .enumerate()
                .filter(|(_, b)| anchored(b))
                .min_by_key(|(i, b)| (dist(at, b.pos), *i))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let b = rest.remove(pick);
            placed.insert(b.pos);
            at = b.pos;
            out.push(b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_blocks_and_every_block_rests_on_something_placed() {
        let s = Schematic::parse("palette S stone\nlayer 0\nSSSS\nS..S\nSSSS\nlayer 1\nS..S\n....\nS..S\n").unwrap();
        let (min, _) = s.bounds();
        for kind in [Order::BANDED, Order::LAYERED] {
            let order = tour_order(&s, kind);
            assert_eq!(order.len(), s.blocks.len());
            let mut placed: HashSet<BlockPos> = HashSet::new();
            for b in &order {
                let ok = b.pos.y == min.y || placed.contains(&b.pos.below()) || Face::HORIZONTAL.iter().any(|f| placed.contains(&b.pos.offset(*f)));
                assert!(ok, "{:?} placed with nothing to rest on", b.pos);
                placed.insert(b.pos);
            }
        }
    }

    #[test]
    fn tour_is_shorter_than_the_sweep_on_a_sparse_layer() {
        let s = Schematic::parse("palette S stone\nlayer 0\nS.......S\n.........\n....S....\n.........\nS.......S\n").unwrap();
        let len = |o: &[&Block]| o.windows(2).map(|w| dist(w[0].pos, w[1].pos)).sum::<i32>();
        assert!(len(&tour_order(&s, Order::BANDED)) <= len(&s.plan_order().0));
    }
}
