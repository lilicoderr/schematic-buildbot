//! Neighbour-derived states (docs/RE/block_states.md section 2): stair shape and fence / pane /
//! wall connections are a function of the final layout. The order-dependent ones (chest side,
//! door hinge) live in [`crate::predict`].

use crate::schematic::Block;
use crate::shapes::{clockwise, is_connection_exception, is_fence, is_pane, is_sturdy, is_wall};
use crate::world::{BlockPos, Face};
use std::collections::HashMap;

/// Blocks standing on flat ground whose top is below `ground_y`.
pub struct Layout {
    blocks: HashMap<BlockPos, Block>,
    ground_y: i32,
    ground: Block,
}

impl Layout {
    pub fn new(ground_y: i32) -> Self {
        let ground = Block::new(BlockPos::new(0, 0, 0), "stone");
        Layout { blocks: HashMap::new(), ground_y, ground }
    }
    pub fn insert(&mut self, b: Block) {
        self.blocks.insert(b.pos, b);
    }
    pub fn get(&self, p: BlockPos) -> Option<&Block> {
        self.blocks.get(&p).or_else(|| (p.y < self.ground_y).then_some(&self.ground))
    }
}

fn facing(b: &Block) -> Option<Face> {
    b.prop("facing").and_then(Face::from_name)
}

fn same_axis(a: Face, b: Face) -> bool {
    a == b || a == b.opposite()
}

fn stair(l: &Layout, p: BlockPos) -> Option<&Block> {
    l.get(p).filter(|b| b.name.ends_with("_stairs"))
}

fn half(b: &Block) -> &str {
    b.prop("half").unwrap_or("bottom")
}

/// The shape the server gives the stair `b` with its neighbours in `l`.
pub fn stair_shape(b: &Block, l: &Layout) -> &'static str {
    let Some(f) = facing(b) else { return "straight" };
    // no identical stair on `side`
    let can_take = |side: Face| {
        stair(l, b.pos.offset(side)).is_none_or(|s| facing(s) != Some(f) || half(s) != half(b))
    };
    let ccw = clockwise(f).opposite();
    let corner = |from: Face| {
        let other = stair(l, b.pos.offset(from)).filter(|s| half(s) == half(b))?;
        facing(other).filter(|&o| !same_axis(o, f))
    };
    if let Some(o) = corner(f) {
        if can_take(o.opposite()) {
            return if o == ccw { "outer_left" } else { "outer_right" };
        }
    }
    if let Some(o) = corner(f.opposite()) {
        if can_take(o) {
            return if o == ccw { "inner_left" } else { "inner_right" };
        }
    }
    "straight"
}

/// Does the fence / pane / wall `b` connect towards `dir`?
pub fn connects(b: &Block, l: &Layout, dir: Face) -> bool {
    let Some(n) = l.get(b.pos.offset(dir)) else { return false };
    let name = n.name.as_str();
    let sturdy = || {
        // neighbouring stairs are judged with their derived shape
        let mut n = n.clone();
        if n.name.ends_with("_stairs") {
            let shape = stair_shape(&n, l);
            n.props.insert("shape".into(), shape.into());
        }
        !is_connection_exception(&n.name) && is_sturdy(&n, dir.opposite())
    };
    let gate = || name.ends_with("_fence_gate") && facing(n).is_some_and(|f| !same_axis(f, dir));
    if is_fence(&b.name) {
        let nether = |s: &str| s == "nether_brick_fence";
        (is_fence(name) && nether(name) == nether(&b.name)) || gate() || sturdy()
    } else if is_pane(&b.name) {
        is_pane(name) || is_wall(name) || sturdy()
    } else {
        is_wall(name) || is_pane(name) || gate() || sturdy()
    }
}

/// Does the block above a wall cover the arm towards `arm` (`None`: the centre post)?
fn covers(above: &Block, l: &Layout, arm: Option<Face>) -> bool {
    let n = above.name.as_str();
    if is_sturdy(above, Face::Down) {
        return true;
    }
    if is_wall(n) {
        return match arm {
            Some(d) => connects(above, l, d),
            None => wall_up(above, l) || Face::HORIZONTAL.iter().any(|&d| connects(above, l, d)),
        };
    }
    (is_fence(n) || is_pane(n)) && arm.is_none_or(|d| connects(above, l, d))
}

/// `none` / `low` / `tall` for one side of a wall.
pub fn wall_side(b: &Block, l: &Layout, dir: Face) -> &'static str {
    if !connects(b, l, dir) {
        return "none";
    }
    let above = l.get(b.pos.offset(Face::Up));
    if above.is_some_and(|a| covers(a, l, Some(dir))) { "tall" } else { "low" }
}

fn post_override(n: &str) -> bool {
    matches!(n, "torch" | "soul_torch" | "redstone_torch" | "tripwire")
        || (n.ends_with("_sign") && !n.ends_with("_hanging_sign"))
        || n.ends_with("_banner")
        || n.ends_with("_pressure_plate")
}

/// Whether the wall has its centre post (`up`).
pub fn wall_up(b: &Block, l: &Layout) -> bool {
    let above = l.get(b.pos.offset(Face::Up));
    if above.is_some_and(|a| is_wall(&a.name) && wall_up(a, l)) {
        return true;
    }
    let side = |d: Face| wall_side(b, l, d);
    let [n, s, e, w] = [side(Face::North), side(Face::South), side(Face::East), side(Face::West)];
    let lone = [n, s, e, w] == ["none"; 4];
    if lone || (n == "none") != (s == "none") || (e == "none") != (w == "none") {
        return true;
    }
    if (n == "tall" && s == "tall") || (e == "tall" && w == "tall") {
        return false;
    }
    above.is_some_and(|a| post_override(&a.name) || covers(a, l, None))
}

/// The layout-derived properties of `b`: (name, value the server will compute).
pub fn derived(b: &Block, l: &Layout) -> Vec<(&'static str, String)> {
    let n = b.name.as_str();
    let sides = [("north", Face::North), ("east", Face::East), ("south", Face::South), ("west", Face::West)];
    if n.ends_with("_stairs") {
        vec![("shape", stair_shape(b, l).to_string())]
    } else if is_fence(n) || is_pane(n) {
        sides.iter().map(|&(k, d)| (k, connects(b, l, d).to_string())).collect()
    } else if is_wall(n) {
        let mut out: Vec<_> = sides.iter().map(|&(k, d)| (k, wall_side(b, l, d).to_string())).collect();
        out.push(("up", wall_up(b, l).to_string()));
        out
    } else {
        Vec::new()
    }
}
