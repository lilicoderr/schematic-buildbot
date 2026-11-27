//! Predicts the final block states of a build and reports where the server will differ from the
//! wanted ones: shapes and connections ([`crate::derive`]) from the final layout, chest sides
//! and door hinges from the build order (docs/RE/block_states.md 2.3 and 2.4).

use crate::derive::{derived, Layout};
use crate::orient;
use crate::schematic::Block;
use crate::shapes::{clockwise, is_full_cube};
use crate::world::{BlockPos, Face};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Mismatch {
    pub pos: BlockPos,
    pub name: String,
    pub prop: String,
    pub wanted: String,
    pub predicted: String,
}

/// The hinge forced by what stands around a door being placed; `None` = the cursor decides.
pub fn forced_hinge(door: &Block, l: &Layout) -> Option<&'static str> {
    let f = door.prop("facing").and_then(Face::from_name)?;
    let beside = |dir: Face| [door.pos.offset(dir), door.pos.offset(dir).offset(Face::Up)];
    let (left, right) = (beside(clockwise(f).opposite()), beside(clockwise(f)));
    let full = |cells: [BlockPos; 2]| cells.iter().filter(|&&p| l.get(p).is_some_and(is_full_cube)).count() as i32;
    let lower_door = |p: BlockPos| {
        l.get(p).is_some_and(|b| b.name.ends_with("_door") && b.prop("half") == Some("lower"))
    };
    let score = full(right) - full(left);
    let (left_door, right_door) = (lower_door(left[0]), lower_door(right[0]));
    if (left_door && !right_door) || score > 0 {
        Some("right")
    } else if (right_door && !left_door) || score < 0 {
        Some("left")
    } else {
        None
    }
}

fn is_chest(n: &str) -> bool {
    matches!(n, "chest" | "trapped_chest")
}

/// The neighbour a `left` / `right` chest is joined to.
fn partner_side(facing: Face, kind: &str) -> Face {
    if kind == "left" { clockwise(facing) } else { clockwise(facing).opposite() }
}

/// Place one chest into `types` the way the server does (we never click a chest).
fn place_chest(types: &mut HashMap<BlockPos, Block>, mut chest: Block) {
    let facing = chest.prop("facing").and_then(Face::from_name).unwrap_or(Face::North);
    let mut kind = "single";
    if !orient::needs_sneak(&chest) {
        for candidate in ["left", "right"] {
            let side = partner_side(facing, candidate);
            let Some(other) = types.get_mut(&chest.pos.offset(side)) else { continue };
            let same = other.name == chest.name && other.prop("facing") == Some(chest.prop("facing").unwrap_or("north"));
            if same && other.prop("type") == Some("single") {
                let twin = if candidate == "left" { "right" } else { "left" };
                other.props.insert("type".into(), twin.into());
                kind = candidate;
                break;
            }
        }
    }
    chest.props.insert("type".into(), kind.into());
    types.insert(chest.pos, chest);
}

/// All differences between the wanted states and what building `order` (world positions,
/// second halves included) on ground below `ground_y` produces. Only listed props count.
pub fn mismatches(order: &[Block], ground_y: i32) -> Vec<Mismatch> {
    let mut layout = Layout::new(ground_y);
    let mut chests = HashMap::new();
    let mut hinges: Vec<(&Block, &'static str)> = Vec::new();
    for b in order {
        if b.name.ends_with("_door") && b.prop("half") == Some("lower") {
            if let Some(h) = forced_hinge(b, &layout) {
                hinges.push((b, h));
            }
        }
        if is_chest(&b.name) {
            place_chest(&mut chests, b.clone());
        }
        layout.insert(b.clone());
    }
    let mut out = Vec::new();
    let mut check = |b: &Block, prop: &str, got: &str| {
        if let Some(wanted) = b.prop(prop).filter(|&w| w != got) {
            out.push(Mismatch {
                pos: b.pos,
                name: b.name.clone(),
                prop: prop.into(),
                wanted: wanted.into(),
                predicted: got.into(),
            });
        }
    };
    for (b, h) in hinges {
        check(b, "hinge", h);
    }
    for b in order {
        for (prop, got) in derived(b, &layout) {
            check(b, prop, &got);
        }
        if let Some(got) = chests.get(&b.pos).and_then(|c| c.prop("type")).filter(|_| is_chest(&b.name)) {
            check(b, "type", got);
        }
    }
    out
}
