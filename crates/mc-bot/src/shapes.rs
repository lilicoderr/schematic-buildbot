//! Coarse shape facts needed to predict neighbour-derived states: which faces are sturdy and
//! which blocks are full cubes. A name-based approximation of the vanilla voxel shapes; open
//! doors / trapdoors and other odd shapes count as not sturdy.

use crate::schematic::Block;
use crate::world::Face;

const NOT_FULL_EXACT: &[&str] = &[
    "air", "cave_air", "water", "lava", "iron_bars", "ladder", "chain", "lantern", "soul_lantern", "lever",
    "chest", "trapped_chest", "ender_chest", "redstone_wire", "repeater", "comparator", "vine", "glow_lichen",
    "hopper", "enchanting_table", "end_portal_frame", "brewing_stand", "flower_pot", "cake",
    "daylight_detector", "lectern", "grindstone", "stonecutter", "bell", "campfire", "soul_campfire",
    "scaffolding", "cobweb", "short_grass", "tall_grass", "fern", "large_fern", "dead_bush", "farmland",
    "dirt_path", "end_rod", "lightning_rod", "conduit", "pointed_dripstone", "composter", "sea_pickle",
    "turtle_egg", "honey_block", "cactus", "anvil", "chipped_anvil", "damaged_anvil", "tripwire",
    "tripwire_hook", "decorated_pot",
];

const NOT_FULL_SUFFIX: &[&str] = &[
    "_stairs", "_fence", "_fence_gate", "_wall", "_pane", "_door", "torch", "_sign", "_banner", "_button",
    "_carpet", "_pressure_plate", "rail", "_sapling", "_bed", "_head", "_skull", "candle", "cauldron",
    "_fan", "_coral", "amethyst_bud", "amethyst_cluster",
];

pub fn is_wall(n: &str) -> bool {
    n.ends_with("_wall")
}

pub fn is_pane(n: &str) -> bool {
    n == "iron_bars" || n.ends_with("_pane")
}

pub fn is_fence(n: &str) -> bool {
    n.ends_with("_fence")
}

/// Collision shape is a whole cube (what the door hinge rule counts).
pub fn is_full_cube(b: &Block) -> bool {
    let n = b.name.as_str();
    if n.ends_with("_slab") {
        return b.prop("type") == Some("double");
    }
    if n == "snow" {
        return b.prop("layers") == Some("8");
    }
    if n.ends_with("_trapdoor") {
        return false;
    }
    !NOT_FULL_EXACT.contains(&n) && !NOT_FULL_SUFFIX.iter().any(|s| n.ends_with(s))
}

pub fn clockwise(f: Face) -> Face {
    match f {
        Face::North => Face::East,
        Face::East => Face::South,
        Face::South => Face::West,
        Face::West => Face::North,
        other => other,
    }
}

/// Is the whole face `side` of `b` solid (able to carry connections and covers)?
pub fn is_sturdy(b: &Block, side: Face) -> bool {
    let n = b.name.as_str();
    if n.ends_with("_slab") {
        return match b.prop("type") {
            Some("double") => true,
            Some("top") => side == Face::Up,
            _ => side == Face::Down,
        };
    }
    if n.ends_with("_stairs") {
        let top = b.prop("half") == Some("top");
        let Some(facing) = b.prop("facing").and_then(Face::from_name) else { return false };
        return match side {
            Face::Up => top,
            Face::Down => !top,
            s => match b.prop("shape") {
                Some("outer_left" | "outer_right") => false,
                Some("inner_left") => s == facing || s == clockwise(facing).opposite(),
                Some("inner_right") => s == facing || s == clockwise(facing),
                _ => s == facing,
            },
        };
    }
    if n.ends_with("_trapdoor") {
        let closed = b.prop("open") != Some("true");
        let top = b.prop("half") == Some("top");
        return closed && side == if top { Face::Up } else { Face::Down };
    }
    is_full_cube(b)
}

/// Blocks fences, panes and walls refuse to connect to even though they are sturdy.
pub fn is_connection_exception(n: &str) -> bool {
    n.ends_with("_leaves")
        || n.ends_with("shulker_box")
        || matches!(n, "barrier" | "carved_pumpkin" | "jack_o_lantern" | "melon" | "pumpkin")
}
