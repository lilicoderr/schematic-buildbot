//! Build order for the slicer: layer by layer, and inside a layer a nearest-neighbour tour.
//!
//! The row/tile sweep of [`crate::schematic::Schematic::plan_order`] is fine for solid floors but
//! makes the bot cross the layer again and again for sparse layers (machines, stairs, windows).
//! A tour that always continues with the closest block that can already be placed keeps the
//! standing position nearly still. Anchoring rules are the same as in `plan_order`.

use crate::schematic::{Block, Schematic};
use crate::world::{BlockPos, Face};
use std::collections::{BTreeMap, HashSet};

fn dist(a: BlockPos, b: BlockPos) -> i32 {
    (a.x - b.x).abs() + (a.z - b.z).abs() + 2 * (a.y - b.y).abs()
}

/// Same blocks as `plan_order`, same layer order, nearest-neighbour order inside each layer.
pub fn tour_order(schem: &Schematic) -> Vec<&Block> {
    let (base, _) = schem.plan_order();
    let (min, _) = schem.bounds();
    let mut layers: BTreeMap<i32, Vec<&Block>> = BTreeMap::new();
    for b in base {
        layers.entry(b.pos.y).or_default().push(b);
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
        let order = tour_order(&s);
        assert_eq!(order.len(), s.blocks.len());
        let mut placed: HashSet<BlockPos> = HashSet::new();
        let (min, _) = s.bounds();
        for b in &order {
            let ok = b.pos.y == min.y || placed.contains(&b.pos.below()) || Face::HORIZONTAL.iter().any(|f| placed.contains(&b.pos.offset(*f)));
            assert!(ok, "{:?} placed with nothing to rest on", b.pos);
            placed.insert(b.pos);
        }
    }

    #[test]
    fn tour_is_shorter_than_the_sweep_on_a_sparse_layer() {
        let s = Schematic::parse("palette S stone\nlayer 0\nS.......S\n.........\n....S....\n.........\nS.......S\n").unwrap();
        let len = |o: &[&Block]| o.windows(2).map(|w| dist(w[0].pos, w[1].pos)).sum::<i32>();
        assert!(len(&tour_order(&s)) <= len(&s.plan_order().0));
    }
}
