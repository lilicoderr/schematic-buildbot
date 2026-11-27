//! Per-family placement rules (docs/RE/block_states.md section 1): where to look, which click
//! gives the wanted state, and whether to sneak. Pure functions of the wanted state; the choice
//! among existing support blocks is made by [`crate::placement::plan`].

use crate::schematic::Block;
use crate::world::Face;

/// What a click must look like to produce the wanted state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rule {
    /// Any face works.
    Any,
    /// Upper (`true`) or lower half: stairs and slabs.
    Half(bool),
    /// Trapdoor: wall click on face `facing`, or the floor / ceiling click.
    Trapdoor { facing: Face, top: bool },
    /// The clicked face must be one of two opposite faces (pillar axis, vertical hopper).
    Axis(Face, Face),
    /// Exactly this face of a permanent support (attached blocks: it must not be scaffold).
    Exact(Face),
    /// Exactly this face, on any support.
    Face(Face),
}

impl Rule {
    /// Does a click on `face` at relative height `cy` yield the wanted state?
    pub fn allows(self, face: Face, cy: f32) -> bool {
        let side = face.yaw().is_some();
        match self {
            Rule::Any => true,
            Rule::Half(top) if side => (cy > 0.5) == top,
            Rule::Half(top) => (face == Face::Down) == top,
            Rule::Trapdoor { facing, top } if side => face == facing && (cy > 0.5) == top,
            Rule::Trapdoor { top, .. } => (face == Face::Down) == top,
            Rule::Axis(a, b) => face == a || face == b,
            Rule::Exact(f) | Rule::Face(f) => face == f,
        }
    }
}

fn is_wall_variant(n: &str) -> bool {
    matches!(n, "wall_torch" | "soul_wall_torch" | "redstone_wall_torch" | "ladder" | "tripwire_hook")
        || ["_wall_sign", "_wall_banner", "_wall_head", "_wall_skull", "_wall_fan"].iter().any(|s| n.ends_with(s))
}

fn is_face_attached(n: &str) -> bool {
    n.ends_with("_button") || n == "lever" || n == "grindstone"
}

/// Standing signs, banners and skulls: 16-step `rotation`.
fn is_rotatable(n: &str) -> bool {
    let kind = ["_sign", "_banner", "_head", "_skull"].iter().any(|s| n.ends_with(s));
    kind && !n.ends_with("_hanging_sign") && !is_wall_variant(n) && n != "piston_head"
}

fn is_lantern(n: &str) -> bool {
    matches!(n, "lantern" | "soul_lantern")
}

fn facing_of(b: &Block) -> Option<Face> {
    b.prop("facing").and_then(Face::from_name)
}

/// Would `click` on `support` (holding the item of `name`) add to the support itself instead
/// of placing a new block next to it? (docs/RE/block_states.md 2.2, slabs.)
pub fn merges_into(support: &Block, name: &str, face: Face, cursor_y: f32) -> bool {
    if support.name != name || !name.ends_with("_slab") {
        return false;
    }
    let side = face.yaw().is_some();
    match support.prop("type") {
        Some("double") => false,
        Some("top") => face == Face::Down || (side && cursor_y <= 0.5),
        _ => face == Face::Up || (side && cursor_y > 0.5),
    }
}

/// The click requirement of `b`.
pub fn rule(b: &Block) -> Rule {
    let n = b.name.as_str();
    let facing = facing_of(b);
    if n.ends_with("_stairs") {
        return b.prop("half").map_or(Rule::Any, |h| Rule::Half(h == "top"));
    }
    if n.ends_with("_slab") {
        // a double slab starts as a bottom slab
        return Rule::Half(b.prop("type") == Some("top"));
    }
    if n.ends_with("_trapdoor") {
        let top = b.prop("half") == Some("top");
        return facing.map_or(Rule::Half(top), |facing| Rule::Trapdoor { facing, top });
    }
    if let Some(axis) = b.prop("axis").filter(|_| !n.contains("portal")) {
        return match axis {
            "x" => Rule::Axis(Face::West, Face::East),
            "z" => Rule::Axis(Face::North, Face::South),
            _ => Rule::Axis(Face::Up, Face::Down),
        };
    }
    if is_wall_variant(n) {
        return facing.map_or(Rule::Any, Rule::Exact);
    }
    let torch = matches!(n, "torch" | "soul_torch" | "redstone_torch");
    if torch || is_rotatable(n) || n.ends_with("_door") || n.ends_with("_bed") {
        return Rule::Exact(Face::Up);
    }
    if is_face_attached(n) {
        return match b.prop("face") {
            Some("floor") => Rule::Exact(Face::Up),
            Some("ceiling") => Rule::Exact(Face::Down),
            Some("wall") => facing.map_or(Rule::Any, Rule::Exact),
            _ => Rule::Any,
        };
    }
    if is_lantern(n) {
        return Rule::Exact(if b.prop("hanging") == Some("true") { Face::Down } else { Face::Up });
    }
    if n == "hopper" {
        return match facing {
            Some(Face::Down) => Rule::Axis(Face::Up, Face::Down),
            Some(f) => Rule::Face(f.opposite()),
            None => Rule::Any,
        };
    }
    if n == "jigsaw" {
        let front = b.prop("orientation").and_then(|o| o.split_once('_')).and_then(|(f, _)| Face::from_name(f));
        return front.map_or(Rule::Any, Rule::Face);
    }
    let rod = n == "end_rod" || n == "lightning_rod" || n == "amethyst_cluster" || n.ends_with("amethyst_bud");
    if rod || n.ends_with("shulker_box") {
        return facing.map_or(Rule::Any, Rule::Face);
    }
    Rule::Any
}

#[path = "orient_turn.rs"]
mod turn;
pub use turn::{door_cursor, extra_clicks, look, needs_sneak, unobtainable};

