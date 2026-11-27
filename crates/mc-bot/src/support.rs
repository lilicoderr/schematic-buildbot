//! Temporary support blocks, like the supports a 3D-printing slicer adds under overhangs.
//!
//! A block can only be placed by clicking an existing neighbour. When none of its neighbours
//! stands yet (they are unreachable, or the design really floats), a short chain of scaffold
//! blocks is placed from a solid anchor out to the block, the block is placed against the chain,
//! and the chain is broken again.

use crate::plan::ClickStep;
use crate::world::{eye_to_box, within_reach, BlockPos, Face, Vec3, CLICK_REACH};
use std::collections::{HashMap, HashSet, VecDeque};

/// Longest chain of scaffold blocks between a solid anchor and the block.
pub const MAX_CHAIN: usize = 3;

/// Cursor in the middle of the clicked face (all vanilla placements accept it).
pub fn face_cursor(face: Face) -> (f32, f32, f32) {
    match face {
        Face::Down => (0.5, 0.0, 0.5),
        Face::Up => (0.5, 1.0, 0.5),
        Face::North => (0.5, 0.5, 0.0),
        Face::South => (0.5, 0.5, 1.0),
        Face::West => (0.0, 0.5, 0.5),
        Face::East => (1.0, 0.5, 0.5),
    }
}

/// One scaffold block of a chain and the click that puts it there.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub pos: BlockPos,
    pub click: ClickStep,
}

/// Shortest chain (anchor side first) of free cells that ends next to `target`, starts next to a
/// solid block (a placed block or the ground), and lies entirely within reach of `feet`.
/// `solid` says what can be clicked; `blocked` cells (the player, future blocks that must not
/// be disturbed) are never used.
pub fn find_chain(
    target: BlockPos,
    feet: Vec3,
    solid: &dyn Fn(BlockPos) -> bool,
    blocked: &dyn Fn(BlockPos) -> bool,
) -> Option<Vec<Link>> {
    let usable = |p: BlockPos| !solid(p) && !blocked(p) && p != target && within_reach(feet, p);
    // BFS outward from the target's neighbours towards a cell that touches something solid.
    let mut came: HashMap<BlockPos, BlockPos> = HashMap::new();
    let mut queue: VecDeque<(BlockPos, usize)> = VecDeque::new();
    let mut seen: HashSet<BlockPos> = HashSet::new();
    for f in Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]) {
        let n = target.offset(*f);
        if usable(n) && seen.insert(n) {
            queue.push_back((n, 1));
        }
    }
    while let Some((cur, len)) = queue.pop_front() {
        for f in Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]) {
            let anchor = cur.offset(*f);
            if solid(anchor) && eye_to_box(feet, anchor) <= CLICK_REACH {
                return Some(chain_from(cur, *f, &came));
            }
        }
        if len < MAX_CHAIN {
            for f in Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]) {
                let n = cur.offset(*f);
                if usable(n) && seen.insert(n) {
                    came.insert(n, cur);
                    queue.push_back((n, len + 1));
                }
            }
        }
    }
    None
}

/// Links from the anchor side: `first` touches the solid block across `anchor_face`; every later
/// link is clicked against the previous scaffold block.
fn chain_from(first: BlockPos, anchor_face: Face, came: &HashMap<BlockPos, BlockPos>) -> Vec<Link> {
    let mut cells = vec![first];
    while let Some(next) = came.get(cells.last().unwrap()) {
        cells.push(*next);
    }
    let mut links = Vec::new();
    for (i, &pos) in cells.iter().enumerate() {
        let (support, face) = if i == 0 {
            (pos.offset(anchor_face), anchor_face.opposite())
        } else {
            let prev = cells[i - 1];
            let f = Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]).copied().find(|f| pos.offset(*f) == prev).unwrap();
            (prev, f.opposite())
        };
        links.push(Link { pos, click: ClickStep { support, face, cursor: face_cursor(face) } });
    }
    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_runs_from_solid_ground_to_a_floating_block() {
        let ground = 0;
        let solid = |p: BlockPos| p.y < ground;
        let target = BlockPos::new(2, 2, 0);
        let feet = Vec3::new(0.5, 0.0, 0.5);
        let chain = find_chain(target, feet, &solid, &|_| false).expect("chain");
        assert!(chain.len() <= MAX_CHAIN);
        // first link clicks solid ground, each next link clicks the previous one
        assert!(solid(chain[0].click.support));
        for w in chain.windows(2) {
            assert_eq!(w[1].click.support, w[0].pos);
        }
        // last link touches the target
        let last = chain.last().unwrap().pos;
        assert!(Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]).any(|f| last.offset(*f) == target));
    }

    #[test]
    fn no_chain_when_the_ground_is_too_far() {
        let solid = |p: BlockPos| p.y < 0;
        assert!(find_chain(BlockPos::new(0, 9, 0), Vec3::new(0.5, 6.0, 0.5), &solid, &|_| false).is_none());
    }
}
