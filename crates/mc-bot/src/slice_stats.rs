//! Header statistics of a freshly sliced plan.

use crate::plan::{Plan, Step};

/// Seconds per walk step (0.21 blocks/tick at 20 Hz), jump, pillar block, placement.
const T_STEP: f64 = 0.25;
const T_JUMP: f64 = 0.4;
const T_PILLAR: f64 = 0.35;
const T_PLACE: f64 = 0.06;

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
