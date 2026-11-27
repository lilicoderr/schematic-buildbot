//! Live regression (large factory): a bottom slab above a bottom slab, with no side neighbour yet,
//! was planned as a click on the lower slab's top face. The server turns that into a double slab and
//! leaves the upper cell empty.

use mc_bot::plan::Step;
use mc_bot::schematic::{Block, Schematic};
use mc_bot::world::{BlockPos, Face};
use mc_bot::{sim, slice};

fn stacked() -> Schematic {
    let slab = |y| Block::from_state(BlockPos::new(0, y, 0), "smooth_quartz_slab[type=bottom]");
    Schematic { name: "stack".into(), blocks: vec![Block::new(BlockPos::new(1, 0, 0), "stone"), slab(0), slab(1)] }
}

#[test]
fn a_slab_over_a_slab_is_not_clicked_onto_it_and_the_checker_rejects_that_click() {
    let schem = stacked();
    let mut plan = slice::slice(&schem, (2, 64, 2), 767).expect("slices");
    assert!(plan.header.unreachable.is_empty(), "{:?}", plan.header.unreachable);
    sim::simulate(&plan, &schem).expect("legal");
    let upper = BlockPos::new(2, 65, 2);
    let click = plan
        .steps
        .iter_mut()
        .find_map(|s| match s {
            Step::Place { pos, clicks, .. } if *pos == upper => Some(&mut clicks[0]),
            _ => None,
        })
        .expect("upper slab placed");
    assert!(!(click.support == upper.below() && click.face == Face::Up), "merging click planned");
    // the click the old planner wrote
    (click.support, click.face, click.cursor) = (upper.below(), Face::Up, (0.5, 1.0, 0.5));
    let err = sim::simulate(&plan, &schem).expect_err("must be rejected");
    assert!(err.msg.contains("merges"), "{}", err.msg);
}
