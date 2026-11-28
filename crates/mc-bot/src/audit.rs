//! Exact state audit of a running build: what the server must show in a cell right now (the
//! plan's block with its neighbour-derived properties for the blocks standing at this moment),
//! compared with the state ids in the server's block updates.

use crate::nav::Cell;
use crate::schematic::{split_state, Block};
use crate::sim::World;
use crate::state_ids::state_of;
use crate::world::{BlockPos, Face};
use std::collections::{HashMap, HashSet};

/// Properties decided by how a block is placed: breaking and placing again can repair them.
/// Everything else is computed by the server (connections, stair shape, redstone, water ...).
const PLACED_PROPS: [&str; 16] = [
    "type", "half", "facing", "axis", "face", "rotation", "hanging", "layers", "candles", "pickles", "eggs",
    "flower_amount", "hinge", "orientation", "part", "attachment",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Ok,
    /// Differs only in what placing again cannot change; reported, not repaired.
    Soft(String),
    /// Wrong block, missing, or wrongly placed: break and place again.
    Wrong(String),
}

/// Plan positions with the properties the slicer already declared it cannot honour.
pub type Allowed = HashMap<Cell, HashSet<String>>;

/// Declared fallbacks of a plan: `fallback_block` header lines and neighbour-forced states.
pub fn allowed(plan: &crate::plan::Plan) -> Allowed {
    let mut out = Allowed::new();
    for (_, v) in plan.header.stats.iter().filter(|(k, _)| k == "fallback_block") {
        let f: Vec<&str> = v.split_whitespace().collect();
        let [x, y, z, _, props] = f[..] else { continue };
        if let (Ok(x), Ok(y), Ok(z)) = (x.parse(), y.parse(), z.parse()) {
            out.entry((x, y, z)).or_default().extend(props.split(',').map(String::from));
        }
    }
    let order: Vec<Block> = plan
        .steps
        .iter()
        .filter_map(|s| match s {
            crate::plan::Step::Place { pos, state, .. } | crate::plan::Step::Auto { pos, state } => Some(Block::from_state(*pos, state)),
            _ => None,
        })
        .collect();
    for m in crate::predict::mismatches(&order, plan.header.origin.1) {
        out.entry((m.pos.x, m.pos.y, m.pos.z)).or_default().insert(m.prop);
    }
    out
}

fn text(name: &str, props: &std::collections::BTreeMap<String, String>) -> String {
    if props.is_empty() {
        return name.to_string();
    }
    let list: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{name}[{}]", list.join(","))
}

/// The state the server must show at plan cell `c` for the blocks standing now; `None` = air.
pub fn expected(world: &World, c: Cell) -> Option<String> {
    let state = world.blocks.get(&c)?;
    let (name, mut props) = split_state(state);
    let n = name.as_str();
    let chest = matches!(n, "chest" | "trapped_chest");
    let connects = crate::shapes::is_fence(n) || crate::shapes::is_pane(n) || crate::shapes::is_wall(n);
    if !(chest || connects || n.ends_with("_stairs")) {
        return Some(state.clone());
    }
    let pos = BlockPos::new(c.0, c.1, c.2);
    if chest {
        // half of a double chest stays single until its partner stands
        let joined = Face::HORIZONTAL.iter().any(|f| {
            let q = pos.offset(*f);
            world.blocks.get(&(q.x, q.y, q.z)).is_some_and(|s| split_state(s).0 == name)
        });
        if !joined && props.contains_key("type") {
            props.insert("type".into(), "single".into());
        }
        return Some(text(&name, &props));
    }
    let mut layout = crate::derive::Layout::new(world.ground_y);
    for dy in -1..=3 {
        for dx in -2..=2 {
            for dz in -2..=2 {
                let q = (c.0 + dx, c.1 + dy, c.2 + dz);
                if let Some(s) = world.blocks.get(&q) {
                    layout.insert(Block::from_state(BlockPos::new(q.0, q.1, q.2), s));
                }
            }
        }
    }
    for (key, value) in crate::derive::derived(&Block::from_state(pos, state), &layout) {
        if let Some(v) = props.get_mut(key) {
            *v = value;
        }
    }
    Some(text(&name, &props))
}

/// Compare a reported 1.21.1-translatable state id with the wanted state (`None` = air).
pub fn judge(want: Option<&str>, reported: i32, allowed: Option<&HashSet<String>>) -> Verdict {
    let Some(want) = want else {
        return if reported == 0 { Verdict::Ok } else { Verdict::Wrong(format!("should be air, is {}", name_of(reported))) };
    };
    let (name, props) = split_state(want);
    // the exact id first: the usual case, and the only one when every property is listed
    let exact = crate::version::active().protocol == crate::version::DEFAULT.protocol;
    if exact && u32::try_from(reported).ok().and_then(state_of).is_some_and(|(n, got)| n == name && props.iter().all(|(k, v)| got.get(k) == Some(v))) {
        return Verdict::Ok;
    }
    let Some(difference) = crate::verify::difference(want, Some(reported)) else { return Verdict::Ok };
    let Some(list) = difference.split("; ").map(|d| d.split_once(": want ").map(|x| x.0)).collect::<Option<Vec<_>>>() else {
        return Verdict::Wrong(difference); // missing, another block, unknown id
    };
    let repairable = |k: &&str| PLACED_PROPS.contains(k) && props.contains_key(*k) && !allowed.is_some_and(|a| a.contains(*k));
    if list.iter().any(repairable) {
        Verdict::Wrong(difference)
    } else {
        Verdict::Soft(difference)
    }
}

fn name_of(id: i32) -> String {
    let tables = crate::version::active().tables;
    u32::try_from(id).ok().and_then(|id| tables.block_of_state(id)).map_or_else(|| format!("state {id}"), String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_ids::state_id;

    fn id(state: &str) -> i32 {
        let (n, p) = split_state(state);
        state_id(&n, &p).unwrap() as i32
    }

    #[test]
    fn wrong_block_wrong_slab_and_stray_are_repaired_but_connections_are_not() {
        let want = "smooth_quartz_slab[type=bottom,waterlogged=false]";
        assert_eq!(judge(Some(want), id(want), None), Verdict::Ok);
        assert!(matches!(judge(Some(want), id("sea_lantern"), None), Verdict::Wrong(_)));
        assert!(matches!(judge(Some(want), id("smooth_quartz_slab[type=double]"), None), Verdict::Wrong(_)));
        assert!(matches!(judge(Some(want), 0, None), Verdict::Wrong(_)));
        assert!(matches!(judge(None, id("stone"), None), Verdict::Wrong(_)));
        assert!(matches!(judge(Some("glass_pane[east=true]"), id("glass_pane[east=false]"), None), Verdict::Soft(_)));
        let fallback: HashSet<String> = ["axis".to_string()].into();
        assert!(matches!(judge(Some("oak_log[axis=z]"), id("oak_log[axis=y]"), Some(&fallback)), Verdict::Soft(_)));
    }

    #[test]
    fn connections_are_expected_only_once_the_neighbour_stands() {
        let mut w = World::new(0, (5, 0, 5));
        w.blocks.insert((0, 0, 0), "glass_pane[east=true,north=false,south=false,west=false]".into());
        assert!(expected(&w, (0, 0, 0)).unwrap().contains("east=false"));
        w.blocks.insert((1, 0, 0), "stone".into());
        assert!(expected(&w, (0, 0, 0)).unwrap().contains("east=true"));
        assert_eq!(expected(&w, (9, 0, 0)), None);
    }
}
