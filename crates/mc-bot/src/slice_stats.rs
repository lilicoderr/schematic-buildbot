//! Header statistics of a freshly sliced plan.

use crate::plan::{Plan, Step};
use crate::schematic::Block;
use std::collections::BTreeMap;

/// Seconds per walk step (0.21 blocks/tick at 20 Hz), jump, pillar block, placement.
const T_STEP: f64 = 0.25;
const T_JUMP: f64 = 0.4;
const T_PILLAR: f64 = 0.35;
const T_PLACE: f64 = 0.06;

/// Blocks whose requested state is derived by the server from the neighbours (stair shape, fence /
/// pane / wall connections, docs/RE/block_states.md): the plan cannot choose it, only the build
/// order influences it, so these are reported instead of claimed as exact.
pub fn neighbour_derived(schem: &crate::schematic::Schematic) -> usize {
    schem
        .blocks
        .iter()
        .filter(|b| {
            let shape = b.prop("shape").is_some_and(|v| v != "straight");
            let connections = ["north", "east", "south", "west"].iter().any(|k| b.prop(k).is_some_and(|v| v != "false" && v != "none"));
            shape || connections
        })
        .count()
}

/// Predict the final states from the plan's build order and record how many wanted
/// neighbour-derived properties will differ, per `block.property`.
pub fn fill_mismatches(plan: &mut Plan) {
    let order: Vec<Block> = plan
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Place { pos, state, .. } | Step::Auto { pos, state } => Some(Block::from_state(*pos, state)),
            _ => None,
        })
        .collect();
    let wrong = crate::predict::mismatches(&order, plan.header.origin.1);
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for m in &wrong {
        *kinds.entry(format!("{}.{}", m.name, m.prop)).or_default() += 1;
    }
    plan.header.stats.push(("state_mismatches".into(), wrong.len().to_string()));
    if !kinds.is_empty() {
        let list: Vec<String> = kinds.iter().map(|(k, n)| format!("{k}:{n}")).collect();
        plan.header.stats.push(("state_mismatch_kinds".into(), list.join(" ")));
    }
}

pub fn fill(plan: &mut Plan, schematic_blocks: usize, unhonoured: usize) {
    let (mut moves, mut jumps, mut pillars, mut places, mut breaks) = (0, 0, 0, 0, 0);
    let mut last_y = plan.header.start.1;
    for s in &plan.steps {
        match s {
            Step::Move((_, y, _)) => {
                moves += 1;
                if *y > last_y {
                    jumps += 1;
                }
                last_y = *y;
            }
            Step::Pillar((_, y, _)) => {
                pillars += 1;
                last_y = y + 1;
            }
            Step::Place { .. } => places += 1,
            Step::Break(p) => {
                breaks += 1;
                last_y = p.y;
            }
            _ => {}
        }
    }
    let est = moves as f64 * T_STEP + jumps as f64 * T_JUMP + (pillars + breaks) as f64 * T_PILLAR + places as f64 * T_PLACE;
    let mut put = |k: &str, v: String| plan.header.stats.push((k.to_string(), v));
    put("blocks", schematic_blocks.to_string());
    put("placements", places.to_string());
    put("moves", moves.to_string());
    put("jumps", jumps.to_string());
    put("scaffold_blocks", pillars.to_string());
    put("scaffold_breaks", breaks.to_string());
    put("unreachable", plan.header.unreachable.len().to_string());
    put("default_state_fallbacks", unhonoured.to_string());
    put("est_seconds", format!("{est:.0}"));
}
