//! Hand-written cheating plans. Every one of them must be rejected.

use crate::plan::{ClickStep, Step};
use crate::sim::{apply, World};
use crate::world::{BlockPos, Face};

/// Flat ground below y 0, player in (0, 0, 0).
fn world() -> World {
    World::new(0, (0, 0, 0))
}

fn click(x: i32, y: i32, z: i32, face: Face, cy: f32) -> ClickStep {
    ClickStep { support: BlockPos::new(x, y, z), face, cursor: (0.5, cy, 0.5) }
}

fn put(pos: (i32, i32, i32), state: &str, clicks: Vec<ClickStep>) -> Step {
    Step::Place { pos: BlockPos::new(pos.0, pos.1, pos.2), state: state.into(), yaw: None, clicks }
}

fn on_ground(x: i32, z: i32, state: &str) -> Step {
    put((x, 0, z), state, vec![click(x, -1, z, Face::Up, 1.0)])
}

fn refused(w: &mut World, st: Step, why: &str) {
    let e = apply(w, &st).expect_err(&format!("accepted: {st}"));
    assert!(e.contains(why), "{st}: {e}");
}

#[test]
fn auto_cannot_conjure_blocks() {
    let mut w = world();
    apply(&mut w, &on_ground(2, 0, "stone")).unwrap();
    // "same kind next door" used to be enough: free blocks anywhere along a wall
    refused(&mut w, Step::Auto { pos: BlockPos::new(3, 0, 0), state: "stone".into() }, "AUTO");
    refused(&mut w, Step::Auto { pos: BlockPos::new(2, 1, 0), state: "stone".into() }, "AUTO");
}

#[test]
fn a_door_creates_its_upper_half_at_once() {
    let mut w = world();
    apply(&mut w, &on_ground(1, 0, "oak_door[half=lower]")).unwrap();
    // before any AUTO names it, the upper half is already in the way
    refused(&mut w, put((1, 1, 0), "stone", vec![click(1, 0, 0, Face::Up, 1.0)]), "not free");
    apply(&mut w, &Step::Auto { pos: BlockPos::new(1, 1, 0), state: "oak_door[half=upper]".into() }).unwrap();
    refused(&mut w, put((2, 1, 0), "oak_door[half=upper]", vec![click(1, 1, 0, Face::East, 0.5)]), "by itself");
}

#[test]
fn a_bed_head_may_not_land_in_the_player() {
    let mut w = world();
    let mut st = on_ground(1, 0, "red_bed[facing=west,part=foot]");
    if let Step::Place { yaw, .. } = &mut st {
        *yaw = Some(90.0);
    }
    refused(&mut w, st, "second half");
}

#[test]
fn double_slab_is_two_merging_clicks() {
    let first = click(2, -1, 0, Face::Up, 1.0);
    let state = "stone_slab[type=double]";
    refused(&mut world(), put((2, 0, 0), state, vec![first.clone()]), "needed");
    let wrong = click(2, 0, 0, Face::East, 0.5);
    refused(&mut world(), put((2, 0, 0), state, vec![first.clone(), wrong]), "merge");
    apply(&mut world(), &put((2, 0, 0), state, vec![first.clone(), click(2, 0, 0, Face::Up, 1.0)])).unwrap();
    // a second click on a plain block would silently build a second block somewhere else
    refused(&mut world(), put((2, 0, 0), "stone", vec![first.clone(), first]), "needed");
}

#[test]
fn cursor_and_face_must_be_real() {
    for bad in [f32::NAN, 7.0, -0.2] {
        refused(&mut world(), put((2, 0, 0), "stone", vec![click(2, -1, 0, Face::Up, bad)]), "cursor");
    }
    refused(&mut world(), put((2, 0, 0), "stone", vec![click(2, -1, 0, Face::East, 0.5)]), "lead");
}

#[test]
fn reach_is_measured_from_the_eye_to_the_box() {
    // box of x=5 starts 4.5 from the eye in x: with the height offset it is just too far
    refused(&mut world(), on_ground(5, 0, "stone"), "out of reach");
    apply(&mut world(), &on_ground(4, 0, "stone")).unwrap();
}

#[test]
fn head_cell_counts_as_the_players_body() {
    let mut w = world();
    apply(&mut w, &on_ground(1, 0, "stone")).unwrap();
    apply(&mut w, &put((1, 1, 0), "stone", vec![click(1, 0, 0, Face::Up, 1.0)])).unwrap();
    refused(&mut w, put((0, 1, 0), "stone", vec![click(1, 1, 0, Face::West, 0.5)]), "own cells");
}

#[test]
fn usable_supports_the_planner_may_forget_are_refused() {
    for usable in ["hopper", "dispenser", "repeater[facing=north]", "chipped_anvil", "loom", "redstone_wire"] {
        let mut w = world();
        apply(&mut w, &on_ground(2, 0, usable)).unwrap();
        refused(&mut w, put((2, 1, 0), "stone", vec![click(2, 0, 0, Face::Up, 1.0)]), "usable");
    }
}

#[test]
fn pillar_may_not_click_a_usable_floor() {
    let mut w = world();
    w.blocks.insert((0, -1, 0), "crafting_table".into());
    refused(&mut w, Step::Pillar((0, 0, 0)), "usable");
}

#[test]
fn never_stands_on_blocks_without_a_full_top() {
    for thin in ["redstone_wire", "poppy", "oak_sign", "stone_pressure_plate", "soul_sand", "chest", "oak_slab[type=bottom]"] {
        let mut w = world();
        w.blocks.insert((1, 0, 0), thin.into());
        refused(&mut w, Step::Move((1, 1, 0)), "not standable");
    }
    for full in ["grass_block", "jack_o_lantern", "snow_block", "hay_block", "mushroom_stem"] {
        let mut w = world();
        w.blocks.insert((1, 0, 0), full.into());
        apply(&mut w, &Step::Move((1, 1, 0))).expect(full);
    }
}

#[test]
fn no_flying_steps() {
    refused(&mut world(), Step::Move((1, 1, 0)), "not standable");
    refused(&mut world(), Step::Move((0, 1, 0)), "horizontal");
    refused(&mut world(), Step::Pillar((0, 1, 0)), "feet cell");
}

#[test]
fn removing_a_support_may_not_drop_or_pop_blocks() {
    for state in ["sand", "torch", "white_carpet"] {
        let mut w = world();
        let support = Step::Support { pos: BlockPos::new(2, 0, 0), clicks: vec![click(2, -1, 0, Face::Up, 1.0)] };
        apply(&mut w, &support).unwrap();
        apply(&mut w, &put((2, 1, 0), state, vec![click(2, 0, 0, Face::Up, 1.0)])).unwrap();
        refused(&mut w, Step::Break(BlockPos::new(2, 0, 0)), "fall or pop off");
    }
}

#[test]
fn sand_needs_something_under_it() {
    let mut w = world();
    apply(&mut w, &on_ground(2, 0, "stone")).unwrap();
    apply(&mut w, &put((2, 1, 0), "stone", vec![click(2, 0, 0, Face::Up, 1.0)])).unwrap();
    refused(&mut w, put((3, 1, 0), "sand", vec![click(2, 1, 0, Face::East, 0.5)]), "cannot stay");
}

#[test]
fn wrong_derived_states_are_counted() {
    let mut w = world();
    apply(&mut w, &on_ground(2, 0, "stone_slab[type=top]")).unwrap();
    apply(&mut w, &on_ground(0, 2, "oak_log[axis=x]")).unwrap();
    apply(&mut w, &on_ground(-2, 0, "oak_stairs[facing=east,half=bottom]")).unwrap();
    assert_eq!(w.mismatches, 3, "stairs looked at from the east face west");
}
