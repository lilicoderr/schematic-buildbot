//! Zero default states: a pillar block whose axis has no neighbour face to click (live: log axes
//! declared as fallbacks) is clicked on a temporary block that is removed again, and every
//! blueprint of the repository ends exactly as drawn.

use mc_bot::plan::{Plan, Step};
use mc_bot::schematic::{Block, Schematic};
use mc_bot::world::BlockPos;
use mc_bot::{guard::Budget, sim, slice};
use std::time::Duration;

fn assert_exact(plan: &Plan, schem: &Schematic) {
    let name = &schem.name;
    assert!(plan.header.unreachable.is_empty(), "{name}: {:?}", plan.header.unreachable);
    let stat = |key: &str| plan.header.stats.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    assert_eq!(stat("default_state_fallbacks"), Some("0"), "{name}");
    assert_eq!(mc_bot::slice_stats::inexact_states(plan), Ok(()), "{name}");
    // strict: every step legal, no scaffold left, final world == schematic
    sim::simulate(plan, schem).unwrap_or_else(|e| panic!("{name}: {e:?}"));
}

/// One log with nothing beside it along its axis, on the ground (`y` = 0 or standing on stone).
fn lone(axis: &str, on_pillar: i32) -> Schematic {
    let mut blocks: Vec<Block> = (0..on_pillar).map(|y| Block::new(BlockPos::new(0, y, 0), "stone")).collect();
    blocks.push(Block::from_state(BlockPos::new(0, on_pillar, 0), &format!("oak_log[axis={axis}]")));
    Schematic { name: format!("lone-{axis}-{on_pillar}"), blocks }
}

#[test]
fn a_lone_log_gets_its_axis_from_a_temporary_block_that_is_removed_again() {
    for (axis, height) in [("x", 0), ("z", 0), ("x", 3), ("z", 3)] {
        let schem = lone(axis, height);
        let plan = slice::slice(&schem, (2, 64, 2), 767).expect("slices");
        assert_exact(&plan, &schem);
        let log = BlockPos::new(2, 64 + height, 2);
        let at = plan.steps.iter().position(|s| matches!(s, Step::Place { pos, .. } if *pos == log)).expect("log placed");
        let Step::Place { clicks, .. } = &plan.steps[at] else { unreachable!() };
        let helper = clicks[0].support;
        assert!(plan.steps[..at].iter().any(|s| matches!(s, Step::Support { pos, .. } if *pos == helper)), "{}", schem.name);
        assert!(plan.steps[at..].contains(&Step::Break(helper)), "{}", schem.name);
        // without the removal the strict checker refuses the plan
        let mut left = plan.clone();
        left.steps.retain(|s| *s != Step::Break(helper));
        assert!(sim::simulate(&left, &schem).is_err(), "{}", schem.name);
    }
}

fn blueprint(file: &str) -> Schematic {
    let path = format!("{}/../../blueprints/{file}", env!("CARGO_MANIFEST_DIR"));
    let (name, ext) = file.split_once('.').unwrap();
    if ext == "schem" {
        mc_bot::schem::parse(&std::fs::read(path).unwrap(), name).unwrap()
    } else {
        let mut s = Schematic::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        s.name = name.into();
        s
    }
}

#[test]
fn the_small_blueprints_have_no_fallback() {
    for file in ["cottage.txt", "pixel-heart.txt", "stair-tower.txt", "state-lab.txt", "factory-small.schem"] {
        let schem = blueprint(file);
        assert_exact(&slice::slice(&schem, (2, 64, 2), 767).expect(file), &schem);
    }
}

#[test]
#[cfg_attr(debug_assertions, ignore = "run with --release")]
fn the_large_factory_has_no_fallback() {
    let schem = blueprint("factory-large.schem");
    let mut budget = Budget::new(Duration::from_secs(120), 2048 << 20);
    assert_exact(&slice::slice_with(&schem, (2, 64, 2), 767, &mut budget).expect("planned"), &schem);
}
