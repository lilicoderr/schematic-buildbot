//! Planner performance: time must grow about linearly with the structure, and the large factory
//! must stay inside its limits.

use mc_bot::plan::Step;
use mc_bot::schematic::{Block, Schematic};
use mc_bot::tour::Order;
use mc_bot::world::BlockPos;
use mc_bot::{guard::Budget, slice};
use std::time::{Duration, Instant};

/// `bays` closed rooms in a row (floor, walls five high, roof): walls to climb, a roof that needs
/// scaffold, and rooms that close over the work.
fn rooms(bays: i32) -> Schematic {
    const SIDE: i32 = 9;
    let mut blocks = Vec::new();
    for bay in 0..bays {
        for x in 0..SIDE {
            for z in 0..SIDE {
                for y in 0..7 {
                    let wall = x == 0 || z == 0 || x == SIDE - 1 || z == SIDE - 1;
                    if y == 0 || y == 6 || wall {
                        blocks.push(Block::new(BlockPos::new(bay * (SIDE + 2) + x, y, z), "stone"));
                    }
                }
            }
        }
    }
    Schematic { name: format!("rooms{bays}"), blocks }
}

/// Fastest of two runs, so that a busy machine does not decide the result.
fn plan_time(s: &Schematic) -> Duration {
    (0..2)
        .map(|_| {
            let t = Instant::now();
            let plan = slice::slice(s, (2, 64, 2), 767).expect("slices");
            assert!(plan.header.unreachable.is_empty(), "{} unreachable", plan.header.unreachable.len());
            t.elapsed()
        })
        .min()
        .unwrap()
}

#[test]
fn twice_the_structure_takes_less_than_three_times_as_long() {
    let (small, big) = (plan_time(&rooms(8)), plan_time(&rooms(16)));
    // the constant covers timer noise on runs of a few milliseconds
    assert!(big < small * 3 + Duration::from_millis(100), "{small:?} for 8 rooms, {big:?} for 16");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "run with --release")]
fn the_large_factory_is_planned_completely_within_its_limits() {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../blueprints/factory-large.schem")).unwrap();
    let schem = mc_bot::schem::parse(&bytes, "factory-large").unwrap();
    let mut budget = Budget::new(Duration::from_secs(120), 2048 << 20);
    let plan = slice::slice_with(&schem, (2, 64, 2), 767, &mut budget).expect("planned in time, simulates clean");
    assert!(plan.header.unreachable.is_empty(), "{:?}", plan.header.unreachable);
    mc_bot::sim::simulate(&plan, &schem).expect("strict simulation");
    // walking decides the live build time: 11581 moves, 4901 scaffold breaks and 2244 s of
    // estimated walking when this was written (docs/PROGRESS.md); do not get worse
    let count = |f: fn(&Step) -> bool| plan.steps.iter().filter(|s| f(s)).count();
    let moves = count(|s| matches!(s, Step::Move(_)));
    let breaks = count(|s| matches!(s, Step::Break(_)));
    let walking = mc_bot::shorten::move_seconds(&plan);
    assert!(moves <= 11_800, "{moves} moves");
    assert!(breaks <= 5_000, "{breaks} scaffold breaks");
    assert!(walking <= 2_300.0, "{walking:.0} s of walking");
}

/// The second-try order is only used when the banded one fails, so it is exercised here.
#[test]
fn the_layered_order_still_plans_the_small_factory() {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../blueprints/factory-small.schem")).unwrap();
    let schem = mc_bot::schem::parse(&bytes, "factory-small").unwrap();
    let mut budget = Budget::new(Duration::from_secs(120), 2048 << 20);
    let plan = slice::slice_ordered(&schem, (2, 64, 2), 767, &mut budget, Order::LAYERED).expect("planned");
    assert!(plan.header.unreachable.is_empty(), "{:?}", plan.header.unreachable);
    mc_bot::sim::simulate(&plan, &schem).expect("strict simulation");
}
