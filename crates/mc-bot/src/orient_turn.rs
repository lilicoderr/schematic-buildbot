//! Second half of [`super`]: player rotation, sneaking and repeat clicks.

use super::{facing_of, is_face_attached, is_rotatable};
use crate::schematic::Block;
use crate::world::Face;

fn faces_like_player(n: &str) -> bool {
    n.ends_with("_stairs")
        || n.ends_with("_door")
        || n.ends_with("_fence_gate")
        || n.ends_with("_bed")
        || matches!(n, "campfire" | "soul_campfire" | "decorated_pot" | "bell")
}

fn faces_the_player(n: &str) -> bool {
    matches!(
        n,
        "furnace" | "blast_furnace" | "smoker" | "chest" | "trapped_chest" | "ender_chest" | "carved_pumpkin"
            | "jack_o_lantern" | "stonecutter" | "lectern" | "loom" | "repeater" | "comparator" | "beehive"
            | "bee_nest" | "chiseled_bookshelf" | "end_portal_frame" | "vault" | "pink_petals"
            | "big_dripleaf" | "small_dripleaf"
    ) || n.ends_with("_glazed_terracotta")
}

/// Blocks whose facing comes from the 6-way look direction: `Some(true)` = facing equals the
/// look direction (observer), `Some(false)` = its opposite.
fn look_based(n: &str) -> Option<bool> {
    match n {
        "observer" => Some(true),
        "piston" | "sticky_piston" | "dispenser" | "dropper" | "barrel" | "command_block"
        | "chain_command_block" | "repeating_command_block" => Some(false),
        _ => None,
    }
}

/// (yaw, pitch) that looks along `dir`; yaw is `None` for straight up / down.
fn look_along(dir: Face) -> (Option<f32>, f32) {
    match dir {
        Face::Up => (None, -90.0),
        Face::Down => (None, 90.0),
        f => (f.yaw(), 0.0),
    }
}

fn wrap_degrees(d: f32) -> f32 {
    (d + 180.0).rem_euclid(360.0) - 180.0
}

/// Required player rotation `(yaw, pitch)`, each `None` when the state does not depend on it.
pub fn look(b: &Block) -> (Option<f32>, Option<f32>) {
    let n = b.name.as_str();
    if n == "crafter" || n == "jigsaw" {
        return orientation_look(b);
    }
    if is_rotatable(n) {
        let Some(r) = b.prop("rotation").and_then(|r| r.parse::<u8>().ok()) else { return (None, None) };
        let turn = if n.ends_with("_sign") || n.ends_with("_banner") { 180.0 } else { 0.0 };
        return (Some(wrap_degrees(r as f32 * 22.5 - turn)), None);
    }
    let Some(facing) = facing_of(b) else { return (None, None) };
    if let Some(same) = look_based(n) {
        let (yaw, pitch) = look_along(if same { facing } else { facing.opposite() });
        return (yaw, Some(pitch));
    }
    // trapdoors only use the yaw for floor / ceiling clicks: facing = opposite of the player
    let yaw = if n.ends_with("_trapdoor") || faces_the_player(n) {
        facing.opposite().yaw()
    } else if faces_like_player(n) || (is_face_attached(n) && b.prop("face") != Some("wall")) {
        facing.yaw()
    } else if n.ends_with("anvil") {
        // facing = clockwise of the player's direction
        facing.yaw().map(|y| wrap_degrees(y - 90.0))
    } else {
        None
    };
    (yaw, None)
}

/// Crafter / jigsaw `orientation` = `<front>_<top>`.
fn orientation_look(b: &Block) -> (Option<f32>, Option<f32>) {
    let parts = b.prop("orientation").and_then(|o| o.split_once('_'));
    let Some((front, top)) = parts.and_then(|(f, t)| Some((Face::from_name(f)?, Face::from_name(t)?))) else {
        return (None, None);
    };
    if b.name == "jigsaw" {
        // front is the clicked face; only a vertical front uses the yaw (top = H.opposite)
        return (top.opposite().yaw(), None);
    }
    let (yaw, pitch) = look_along(front.opposite());
    let yaw = match front {
        Face::Down => top.opposite().yaw(),
        Face::Up => top.yaw(),
        _ => yaw,
    };
    (yaw, Some(pitch))
}

/// Sneak while clicking: a chest that must stay single would otherwise merge with a neighbour.
/// (Clicks never target usable blocks, so no other sneaking is needed.)
pub fn needs_sneak(b: &Block) -> bool {
    matches!(b.name.as_str(), "chest" | "trapped_chest") && b.prop("type").is_none_or(|t| t == "single")
}

/// Extra clicks on the block itself after the first one (double slab, candles, snow layers...).
pub fn extra_clicks(b: &Block) -> usize {
    let count = |k: &str| b.prop(k).and_then(|v| v.parse::<usize>().ok()).map_or(0, |v| v.saturating_sub(1));
    match b.name.as_str() {
        n if n.ends_with("_slab") => usize::from(b.prop("type") == Some("double")),
        n if n.ends_with("candle") => count("candles"),
        "sea_pickle" => count("pickles"),
        "turtle_egg" => count("eggs"),
        "snow" => count("layers"),
        _ => 0,
    }
}

/// Door cursor (x, z) relative to the placed cell that picks the hinge when no neighbour forces it.
pub fn door_cursor(facing: Face, hinge_right: bool) -> (f32, f32) {
    // the free axis and the value on it that gives a right hinge
    let (along_x, right_at) = match facing {
        Face::South => (true, 0.25),
        Face::North => (true, 0.75),
        Face::East => (false, 0.75),
        _ => (false, 0.25),
    };
    let v = if hinge_right { right_at } else { 1.0 - right_at };
    if along_x { (v, 0.5) } else { (0.5, v) }
}

/// Property values no placement can produce (redstone, water, later interaction).
pub fn unobtainable(key: &str, value: &str) -> bool {
    matches!(key, "open" | "powered" | "waterlogged" | "triggered" | "occupied" | "eye") && value == "true"
}
