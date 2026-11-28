//! Temporary support blocks, like the supports a 3D-printing slicer adds under overhangs.
//!
//! A block can only be placed by clicking an existing neighbour. When none of its neighbours
//! stands yet (they are unreachable, or the design really floats), a short chain of scaffold
//! blocks is placed from a solid anchor out to the block, the block is placed against the chain,
//! and the chain is broken again.

use crate::orient;
use crate::placement::{self, Action, Click, Plan};
use crate::plan::ClickStep;
use crate::schematic::Block;
use crate::world::{eye_to_box, within_reach, BlockPos, Face, Vec3, CLICK_REACH};
use std::collections::{HashMap, HashSet, VecDeque};

/// Longest chain of scaffold blocks between a solid anchor and the block.
pub const MAX_CHAIN: usize = 8;

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
    let free = |p: BlockPos| !solid(p) && !blocked(p) && p != target && within_reach(feet, p);
    let starts = Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]).map(|f| target.offset(*f));
    search(starts, feet, &free, solid)
}

/// A click that gives `block` its wanted state when no neighbour standing now offers the needed
/// face (a lone horizontal log, an upper half with nothing beside it): either on something already
/// there, or on a temporary block put exactly where that face is (the last link of the returned
/// chain, which the caller removes again). `clickable` says which standing blocks can be clicked
/// from `feet`, `safe` refuses clicks that would merge into the support.
pub fn aim(
    block: &Block,
    feet: Vec3,
    solid: &dyn Fn(BlockPos) -> bool,
    blocked: &dyn Fn(BlockPos) -> bool,
    clickable: &dyn Fn(BlockPos) -> bool,
    safe: &dyn Fn(&Click) -> bool,
) -> Option<(Vec<Link>, Plan)> {
    let target = block.pos;
    let rule = orient::rule(block);
    let plan_with = |chain: &[Link]| {
        let ok = |c: &Click| (clickable(c.support) || chain.iter().any(|l| l.pos == c.support)) && safe(c);
        match placement::plan_clicks(block, ok) {
            Action::Place(p) => {
                let good = rule.allows(p.clicks[0].face, p.clicks[0].cursor.1)
                    && p.missing_support.is_none()
                    && p.clicks.iter().all(|c| c.support == target || ok(c));
                good.then_some(p)
            }
            Action::Skip(_) => None,
        }
    };
    if let Some(p) = plan_with(&[]) {
        return Some((Vec::new(), p));
    }
    let free = |p: BlockPos| !solid(p) && !blocked(p) && p != target && within_reach(feet, p);
    let chain = placement::candidates(target, false)
        .iter()
        .filter(|c| rule.allows(c.face, c.cursor.1))
        .filter_map(|c| search(std::iter::once(c.support), feet, &free, clickable))
        .min_by_key(Vec::len)?;
    plan_with(&chain).map(|p| (chain, p))
}

/// Breadth-first from `starts` (each may hold the chain's last link) to a `free` cell that
/// touches an `anchor`.
fn search(
    starts: impl Iterator<Item = BlockPos>,
    feet: Vec3,
    free: &dyn Fn(BlockPos) -> bool,
    anchored: &dyn Fn(BlockPos) -> bool,
) -> Option<Vec<Link>> {
    // BFS outward from the starting cells towards a cell that touches something solid.
    let mut came: HashMap<BlockPos, BlockPos> = HashMap::new();
    let mut queue: VecDeque<(BlockPos, usize)> = VecDeque::new();
    let mut seen: HashSet<BlockPos> = HashSet::new();
    for n in starts {
        if free(n) && seen.insert(n) {
            queue.push_back((n, 1));
        }
    }
    while let Some((cur, len)) = queue.pop_front() {
        for f in Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]) {
            let anchor = cur.offset(*f);
            if anchored(anchor) && eye_to_box(feet, anchor) <= CLICK_REACH {
                return Some(chain_from(cur, *f, &came));
            }
        }
        if len < MAX_CHAIN {
            for f in Face::HORIZONTAL.iter().chain([&Face::Up, &Face::Down]) {
                let n = cur.offset(*f);
                if free(n) && seen.insert(n) {
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
