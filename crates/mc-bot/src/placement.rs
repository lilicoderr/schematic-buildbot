//! Decide *how* to place a block so the server derives the wanted block state.
//!
//! In vanilla the placed state comes from the click, not from the item: the clicked
//! face, the cursor position inside that face, and the player's horizontal facing.
//! This module turns a wanted state into one or more clicks plus a yaw.
//!
//! Supported (verified against the vanilla placement rules, not yet against every server):
//!
//! | family                                   | state       | how                                   |
//! |------------------------------------------|-------------|---------------------------------------|
//! | stairs, doors, fence gates, beds         | `facing`    | player looks along `facing`           |
//! | furnaces, chests, carved pumpkins, ...   | `facing`    | player looks opposite of `facing`     |
//! | stairs                                   | `half`      | click upper/lower half or the top face|
//! | slabs                                    | `type`      | top/bottom like stairs; double = 2 clicks |
//! | logs, pillars, bone/hay blocks, ...      | `axis`      | click a face perpendicular to the axis|
//!
//! Everything else is placed in its default state and reported as "unhonoured".

use crate::schematic::Block;
use crate::world::{BlockPos, Face};

/// One right-click on a block face.
#[derive(Debug, Clone, PartialEq)]
pub struct Click {
    pub support: BlockPos,
    pub face: Face,
    pub cursor: (f32, f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub clicks: Vec<Click>,
    /// Horizontal facing (degrees) the player must have while clicking, if the state needs one.
    pub yaw: Option<f32>,
    /// State properties that could not be honoured and fall back to the default.
    pub unhonoured: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Place(Plan),
    /// Nothing to click: the block appears as part of another one (e.g. the upper half of a door).
    Skip(&'static str),
}

/// Properties that change how a block looks or works when placed; if present and not
/// honoured we report them. (Connection/shape properties are derived by the server.)
const ORIENTATION_PROPS: &[&str] = &["facing", "half", "type", "axis", "hinge", "part", "face", "attachment", "orientation"];

/// Blocks that react to a plain right click (docs/RE/placement.md section 2.7c): clicking one
/// without sneaking opens or toggles it instead of placing, so it is never used as click support.
pub fn is_usable(name: &str) -> bool {
    const EXACT: [&str; 14] = [
        "chest", "trapped_chest", "ender_chest", "barrel", "crafting_table", "furnace", "blast_furnace", "smoker",
        "anvil", "note_block", "lectern", "lever", "enchanting_table", "jukebox",
    ];
    (name.ends_with("_door") && name != "iron_door")
        || name.ends_with("_trapdoor")
        || name.ends_with("_fence_gate")
        || name.ends_with("_button")
        || name.ends_with("_bed")
        || name.ends_with("shulker_box")
        || name.ends_with("_sign")
        || EXACT.contains(&name)
}

fn is_same_as_player(name: &str) -> bool {
    name.ends_with("_stairs")
        || (name.ends_with("_door") && !name.ends_with("trapdoor"))
        || name.ends_with("_fence_gate")
        || name.ends_with("_bed")
}

fn is_opposite_of_player(name: &str) -> bool {
    matches!(
        name,
        "furnace"
            | "blast_furnace"
            | "smoker"
            | "chest"
            | "trapped_chest"
            | "ender_chest"
            | "carved_pumpkin"
            | "jack_o_lantern"
            | "stonecutter"
            | "lectern"
            | "loom"
            | "repeater"
            | "comparator"
    ) || name.ends_with("_glazed_terracotta")
}

fn is_slab(name: &str) -> bool {
    name.ends_with("_slab")
}

fn is_stairs(name: &str) -> bool {
    name.ends_with("_stairs")
}

/// Blocks whose second half is created together with the first one.
fn second_half_is_automatic(block: &Block) -> bool {
    let upper = block.prop("half") == Some("upper") && (block.name.ends_with("_door") || is_tall_plant(&block.name));
    let head = block.name.ends_with("_bed") && block.prop("part") == Some("head");
    upper || head
}

fn is_tall_plant(name: &str) -> bool {
    matches!(
        name,
        "tall_grass" | "large_fern" | "sunflower" | "lilac" | "rose_bush" | "peony" | "tall_seagrass"
    )
}

/// Plan the clicks for `block` (world coordinates). `solid(p)` says whether a face of
/// the block at `p` can be clicked (ground or an already placed block).
pub fn plan(block: &Block, solid: impl Fn(BlockPos) -> bool) -> Action {
    if second_half_is_automatic(block) {
        return Action::Skip("second half is placed together with the first");
    }
    let pos = block.pos;
    let mut honoured: Vec<&str> = Vec::new();
    let mut yaw: Option<f32> = None;

    // Horizontal facing → player yaw.
    if let Some(facing) = block.prop("facing").and_then(Face::from_name) {
        if is_same_as_player(&block.name) {
            yaw = facing.yaw();
        } else if is_opposite_of_player(&block.name) {
            yaw = facing.opposite().yaw();
        }
        if yaw.is_some() {
            honoured.push("facing");
        }
    }

    // Vertical half: stairs `half`, slabs `type`.
    let want_top = (is_stairs(&block.name) && block.prop("half") == Some("top"))
        || (is_slab(&block.name) && block.prop("type") == Some("top"));
    let want_double = is_slab(&block.name) && block.prop("type") == Some("double");
    if is_stairs(&block.name) && block.prop("half").is_some() {
        honoured.push("half");
    }
    if is_slab(&block.name) && block.prop("type").is_some() {
        honoured.push("type");
    }
    // The lower door half / bed foot is what a normal placement creates.
    let lower_half = block.prop("half") == Some("lower");
    if lower_half && (block.name.ends_with("_door") || is_tall_plant(&block.name)) {
        honoured.push("half");
    }
    if block.name.ends_with("_bed") && block.prop("part") == Some("foot") {
        honoured.push("part");
    }

    // Axis: click a face perpendicular to the wanted axis.
    let axis = block.prop("axis").filter(|_| !block.name.contains("portal"));

    let first = pick_click(pos, &solid, want_top, axis);
    let (first, axis_ok, top_ok) = match first {
        Some(c) => c,
        None => {
            // Nothing to click at all: fall back to the classic "top of the block below".
            (Click { support: pos.below(), face: Face::Up, cursor: (0.5, 1.0, 0.5) }, axis.is_none(), !want_top)
        }
    };
    if axis.is_some() && axis_ok {
        honoured.push("axis");
    }

    let mut clicks = vec![first];
    if want_double {
        // Second click on the top of the freshly placed bottom slab merges it into a double slab.
        clicks.push(Click { support: pos, face: Face::Up, cursor: (0.5, 1.0, 0.5) });
        honoured.push("type");
    }

    let mut unhonoured: Vec<String> = block
        .props
        .keys()
        .filter(|k| ORIENTATION_PROPS.contains(&k.as_str()) && !honoured.contains(&k.as_str()))
        .cloned()
        .collect();
    // `half=top` / `axis` were requested but the surroundings did not allow the needed click.
    if want_top && !top_ok {
        let key = if is_slab(&block.name) { "type" } else { "half" };
        if !unhonoured.iter().any(|k| k == key) {
            unhonoured.push(key.to_string());
        }
    }
    if axis.is_some() && !axis_ok && !unhonoured.iter().any(|k| k == "axis") {
        unhonoured.push("axis".to_string());
    }
    // An axis of `y` is satisfied by the default top-face click.
    if axis == Some("y") {
        unhonoured.retain(|k| k != "axis");
    }
    unhonoured.sort();
    unhonoured.dedup();
    Action::Place(Plan { clicks, yaw, unhonoured })
}

/// Choose a click that yields the wanted vertical half / axis. Returns
/// `(click, axis_satisfied, half_satisfied)`.
fn pick_click(
    pos: BlockPos,
    solid: &impl Fn(BlockPos) -> bool,
    want_top: bool,
    axis: Option<&str>,
) -> Option<(Click, bool, bool)> {
    // Axis x / z need a click on a face that points along the axis.
    if let Some(a @ ("x" | "z")) = axis {
        let faces: [Face; 2] = if a == "x" { [Face::West, Face::East] } else { [Face::North, Face::South] };
        for f in faces {
            // Click the face of the neighbour that touches `pos`.
            let support = pos.offset(f);
            if solid(support) {
                let face = f.opposite();
                return Some((Click { support, face, cursor: side_cursor(face, 0.5) }, true, true));
            }
        }
        return None;
    }

    if want_top {
        // Upper half: click the lower face of the block above, or the upper half of a neighbour's side.
        let above = pos.offset(Face::Up);
        if solid(above) {
            return Some((Click { support: above, face: Face::Down, cursor: (0.5, 0.0, 0.5) }, true, true));
        }
        for f in Face::HORIZONTAL {
            let support = pos.offset(f);
            if solid(support) {
                let face = f.opposite();
                return Some((Click { support, face, cursor: side_cursor(face, 0.75) }, true, true));
            }
        }
        // Cannot reach the upper half: place the bottom variant instead.
        if solid(pos.below()) {
            return Some((Click { support: pos.below(), face: Face::Up, cursor: (0.5, 1.0, 0.5) }, true, false));
        }
        return None;
    }

    // Default: the top of the block below, else a side face of a neighbour, else the block above.
    if solid(pos.below()) {
        return Some((Click { support: pos.below(), face: Face::Up, cursor: (0.5, 1.0, 0.5) }, axis.is_none() || axis == Some("y"), true));
    }
    for f in Face::HORIZONTAL {
        let support = pos.offset(f);
        if solid(support) {
            let face = f.opposite();
            return Some((Click { support, face, cursor: side_cursor(face, 0.25) }, axis.is_none() || axis == Some("y"), true));
        }
    }
    let above = pos.offset(Face::Up);
    if solid(above) {
        return Some((Click { support: above, face: Face::Down, cursor: (0.5, 0.0, 0.5) }, axis.is_none() || axis == Some("y"), true));
    }
    None
}

/// Cursor position on a horizontal face at relative height `y` (0..1).
fn side_cursor(face: Face, y: f32) -> (f32, f32, f32) {
    match face {
        Face::North => (0.5, y, 0.0),
        Face::South => (0.5, y, 1.0),
        Face::West => (0.0, y, 0.5),
        Face::East => (1.0, y, 0.5),
        Face::Up => (0.5, 1.0, 0.5),
        Face::Down => (0.5, 0.0, 0.5),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn world(solid: &[(i32, i32, i32)]) -> impl Fn(BlockPos) -> bool {
        let set: HashSet<BlockPos> = solid.iter().map(|&(x, y, z)| BlockPos::new(x, y, z)).collect();
        move |p| set.contains(&p)
    }

    fn block(state: &str) -> Block {
        Block::from_state(BlockPos::new(0, 1, 0), state)
    }

    fn unwrap_plan(a: Action) -> Plan {
        match a {
            Action::Place(p) => p,
            Action::Skip(r) => panic!("unexpected skip: {r}"),
        }
    }

    #[test]
    fn plain_block_clicks_top_of_floor() {
        let p = unwrap_plan(plan(&block("stone"), world(&[(0, 0, 0)])));
        assert_eq!(p.clicks.len(), 1);
        assert_eq!(p.clicks[0].support, BlockPos::new(0, 0, 0));
        assert_eq!(p.clicks[0].face, Face::Up);
        assert!(p.yaw.is_none());
        assert!(p.unhonoured.is_empty());
    }

    #[test]
    fn stairs_face_the_way_the_player_looks() {
        let p = unwrap_plan(plan(&block("oak_stairs[facing=east,half=bottom]"), world(&[(0, 0, 0)])));
        assert_eq!(p.yaw, Some(-90.0));
        assert!(p.unhonoured.is_empty(), "{:?}", p.unhonoured);
        let p = unwrap_plan(plan(&block("oak_stairs[facing=north]"), world(&[(0, 0, 0)])));
        assert_eq!(p.yaw, Some(180.0));
    }

    #[test]
    fn furnace_faces_the_player() {
        // furnace facing=south means the player stood to the south looking north.
        let p = unwrap_plan(plan(&block("furnace[facing=south]"), world(&[(0, 0, 0)])));
        assert_eq!(p.yaw, Some(180.0));
    }

    #[test]
    fn top_stairs_click_underside_of_block_above() {
        let p = unwrap_plan(plan(&block("stone_stairs[facing=east,half=top]"), world(&[(0, 2, 0), (0, 0, 0)])));
        assert_eq!(p.clicks[0].support, BlockPos::new(0, 2, 0));
        assert_eq!(p.clicks[0].face, Face::Down);
        assert!(p.unhonoured.is_empty());
    }

    #[test]
    fn top_stairs_click_upper_half_of_neighbour_side() {
        // West neighbour exists: click its east face, upper half.
        let p = unwrap_plan(plan(&block("stone_stairs[facing=east,half=top]"), world(&[(-1, 1, 0), (0, 0, 0)])));
        let c = &p.clicks[0];
        assert_eq!(c.support, BlockPos::new(-1, 1, 0));
        assert_eq!(c.face, Face::East);
        assert!(c.cursor.1 > 0.5);
        assert!(p.unhonoured.is_empty());
    }

    #[test]
    fn top_half_without_any_handle_is_reported() {
        let p = unwrap_plan(plan(&block("stone_stairs[facing=east,half=top]"), world(&[(0, 0, 0)])));
        assert_eq!(p.clicks[0].face, Face::Up);
        assert_eq!(p.unhonoured, vec!["half".to_string()]);
    }

    #[test]
    fn double_slab_needs_two_clicks() {
        let p = unwrap_plan(plan(&block("stone_slab[type=double]"), world(&[(0, 0, 0)])));
        assert_eq!(p.clicks.len(), 2);
        assert_eq!(p.clicks[1].support, BlockPos::new(0, 1, 0));
        assert_eq!(p.clicks[1].face, Face::Up);
        assert!(p.unhonoured.is_empty());
    }

    #[test]
    fn bottom_slab_is_default_click() {
        let p = unwrap_plan(plan(&block("stone_slab[type=bottom]"), world(&[(0, 0, 0)])));
        assert_eq!(p.clicks.len(), 1);
        assert!(p.unhonoured.is_empty());
    }

    #[test]
    fn vertical_log_uses_floor_click() {
        let p = unwrap_plan(plan(&block("oak_log[axis=y]"), world(&[(0, 0, 0)])));
        assert_eq!(p.clicks[0].face, Face::Up);
        assert!(p.unhonoured.is_empty());
    }

    #[test]
    fn horizontal_log_clicks_the_side_of_a_neighbour() {
        let p = unwrap_plan(plan(&block("oak_log[axis=x]"), world(&[(0, 0, 0), (1, 1, 0)])));
        let c = &p.clicks[0];
        assert_eq!(c.support, BlockPos::new(1, 1, 0));
        assert_eq!(c.face, Face::West);
        assert!(p.unhonoured.is_empty());
    }

    #[test]
    fn horizontal_log_without_neighbour_reports_axis() {
        let p = unwrap_plan(plan(&block("oak_log[axis=z]"), world(&[(0, 0, 0)])));
        assert_eq!(p.unhonoured, vec!["axis".to_string()]);
    }

    #[test]
    fn floating_block_uses_neighbour_when_floor_is_missing() {
        let p = unwrap_plan(plan(&block("stone"), world(&[(0, 1, 1)])));
        assert_eq!(p.clicks[0].support, BlockPos::new(0, 1, 1));
        assert_eq!(p.clicks[0].face, Face::North);
    }

    #[test]
    fn upper_door_half_is_skipped() {
        assert!(matches!(plan(&block("oak_door[half=upper,facing=north]"), world(&[])), Action::Skip(_)));
        assert!(matches!(plan(&block("oak_door[half=lower,facing=north]"), world(&[(0, 0, 0)])), Action::Place(_)));
    }

    #[test]
    fn trapdoor_is_not_treated_as_door() {
        let p = unwrap_plan(plan(&block("oak_trapdoor[facing=north,half=bottom]"), world(&[(0, 0, 0)])));
        assert!(p.yaw.is_none());
        assert!(!p.unhonoured.is_empty());
    }

    #[test]
    fn lower_door_and_bed_foot_are_fully_honoured() {
        let p = unwrap_plan(plan(&block("oak_door[facing=north,half=lower]"), world(&[(0, 0, 0)])));
        assert_eq!(p.yaw, Some(180.0));
        assert!(p.unhonoured.is_empty(), "{:?}", p.unhonoured);
        let p = unwrap_plan(plan(&block("red_bed[part=foot,facing=east]"), world(&[(0, 0, 0)])));
        assert_eq!(p.yaw, Some(-90.0));
        assert!(p.unhonoured.is_empty(), "{:?}", p.unhonoured);
    }

    #[test]
    fn bed_head_is_skipped() {
        assert!(matches!(plan(&block("red_bed[part=head,facing=east]"), world(&[])), Action::Skip(_)));
    }
}
