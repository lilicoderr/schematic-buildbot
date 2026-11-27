//! Decide *how* to place a block so the server derives the wanted block state.
//!
//! In vanilla the placed state comes from the click, not from the item: the clicked
//! face, the cursor position inside that face, and the player's horizontal facing.
//! This module turns a wanted state into one or more clicks plus a yaw.
//!
//! The per-family rules (which face, cursor, rotation and sneaking) live in [`crate::orient`];
//! this module picks, among the supports that exist, a click satisfying them. What cannot be
//! honoured is reported in [`Plan::unhonoured`]; neighbour-derived states (stair shape,
//! connections, chest side, forced door hinges) are predicted by [`crate::derive`].

use crate::orient::{self, Rule};
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
    /// Pitch the player must have (only 6-way blocks: observer, dispenser, piston, barrel ...).
    pub pitch: Option<f32>,
    /// Hold sneak while clicking.
    pub sneak: bool,
    /// Set when an attached block's only valid support is not (yet) there: do not substitute
    /// scaffold, place it later or report it.
    pub missing_support: Option<BlockPos>,
    /// State properties that could not be honoured and fall back to the default.
    pub unhonoured: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Place(Plan),
    /// Nothing to click: the block appears as part of another one (e.g. the upper half of a door).
    Skip(&'static str),
}

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

/// Properties that change how a block looks or works when placed; if present and not
/// honoured we report them. (Connection/shape properties are derived by the server.)
const ORIENTATION_PROPS: &[&str] =
    &["facing", "half", "type", "axis", "hinge", "part", "face", "attachment", "orientation", "rotation", "hanging"];

/// Plan the clicks for `block` (world coordinates). `solid(p)` says whether a face of
/// the block at `p` can be clicked (ground or an already placed block).
pub fn plan(block: &Block, solid: impl Fn(BlockPos) -> bool) -> Action {
    plan_clicks(block, |c| solid(c.support))
}

/// Like [`plan`], but `usable(click)` judges whole clicks (so a caller can also refuse a face
/// that would merge into the support, see [`orient::merges_into`]).
pub fn plan_clicks(block: &Block, usable: impl Fn(&Click) -> bool) -> Action {
    if second_half_is_automatic(block) {
        return Action::Skip("second half is placed together with the first");
    }
    let pos = block.pos;
    let rule = orient::rule(block);
    let (yaw, pitch) = orient::look(block);
    let all = candidates(pos, matches!(rule, Rule::Half(true) | Rule::Trapdoor { top: true, .. }));
    let allowed = |c: &&Click| rule.allows(c.face, c.cursor.1);

    let mut missing_support = None;
    let (mut first, rule_ok) = match all.iter().filter(allowed).find(|c| usable(c)) {
        Some(c) => (c.clone(), true),
        None => match (rule, all.iter().find(allowed)) {
            // Attached blocks: only this support will do; the caller must wait for it.
            (Rule::Exact(_), Some(ideal)) => {
                missing_support = Some(ideal.support);
                (ideal.clone(), true)
            }
            // Otherwise place it anyway (any click) and report the property that is lost.
            _ => {
                let any = all.iter().find(|c| usable(c));
                let floor = || Click { support: pos.below(), face: Face::Up, cursor: (0.5, 1.0, 0.5) };
                let click = any.cloned().unwrap_or_else(floor);
                let ok = rule.allows(click.face, click.cursor.1);
                (click, ok)
            }
        },
    };
    if block.name.ends_with("_door") && first.face == Face::Up {
        if let (Some(f), Some(h)) = (block.prop("facing").and_then(Face::from_name), block.prop("hinge")) {
            let (x, z) = orient::door_cursor(f, h == "right");
            first.cursor = (x, 1.0, z);
        }
    }

    let mut clicks = vec![first];
    for _ in 0..orient::extra_clicks(block) {
        // Same item on the freshly placed block: bottom slab -> double, one more candle, ...
        clicks.push(Click { support: pos, face: Face::Up, cursor: (0.5, 1.0, 0.5) });
    }

    let honoured = |key: &str, value: &str| match key {
        "facing" => yaw.is_some() || pitch.is_some() || (rule_ok && !matches!(rule, Rule::Any | Rule::Half(_))),
        "rotation" => yaw.is_some(),
        "orientation" => rule_ok && (yaw.is_some() || pitch.is_some()),
        "half" => match rule {
            Rule::Half(_) | Rule::Trapdoor { .. } => rule_ok,
            // the lower half of a door / tall plant is what a placement creates
            _ => value == "lower",
        },
        // slab halves come from the click; chest left/right comes from the partner chest
        "type" => !block.name.ends_with("_slab") || rule_ok,
        "axis" => rule_ok,
        "face" | "hanging" => rule_ok && matches!(rule, Rule::Exact(_)),
        "hinge" => block.name.ends_with("_door"),
        "part" => value == "foot",
        _ => false,
    };
    let unhonoured: Vec<String> = block
        .props
        .iter()
        .filter(|(k, v)| {
            orient::unobtainable(k, v) || (ORIENTATION_PROPS.contains(&k.as_str()) && !honoured(k, v))
        })
        .map(|(k, _)| k.clone())
        .collect();
    let sneak = orient::needs_sneak(block);
    Action::Place(Plan { clicks, yaw, pitch, sneak, missing_support, unhonoured })
}

/// Every way to click a neighbour of `pos` so the block lands in `pos`, in order of preference.
fn candidates(pos: BlockPos, upper_first: bool) -> Vec<Click> {
    let floor = Click { support: pos.below(), face: Face::Up, cursor: (0.5, 1.0, 0.5) };
    let ceiling = Click { support: pos.offset(Face::Up), face: Face::Down, cursor: (0.5, 0.0, 0.5) };
    let sides = |y: f32| {
        Face::HORIZONTAL.into_iter().map(move |f| {
            let face = f.opposite();
            Click { support: pos.offset(f), face, cursor: side_cursor(face, y) }
        })
    };
    let (first, last) = if upper_first { (ceiling, floor) } else { (floor, ceiling) };
    let mut out = vec![first];
    out.extend(sides(0.25));
    out.extend(sides(0.75));
    out.push(last);
    out
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
