use crate::plan::{block_hash, ClickStep, Plan, Step};
use crate::schematic::Schematic;
use crate::sim::simulate;
use crate::world::{BlockPos, Face};

fn two_stack() -> Schematic {
    Schematic::parse("palette S stone\nlayer 0\nS\nlayer 1\nS\n").unwrap()
}

fn place(x: i32, y: i32, z: i32, sx: i32, sy: i32, sz: i32, face: Face) -> Step {
    Step::Place {
        pos: BlockPos::new(x, y, z),
        state: "stone".into(),
        yaw: None,
        clicks: vec![ClickStep { support: BlockPos::new(sx, sy, sz), face, cursor: (0.5, 1.0, 0.5) }],
    }
}

fn base_plan(steps: Vec<Step>) -> Plan {
    let mut p = Plan::default();
    p.header.origin = (5, 0, 5);
    p.header.start = (3, 0, 5);
    p.steps = steps;
    p
}

fn good_steps() -> Vec<Step> {
    vec![place(5, 0, 5, 5, -1, 5, Face::Up), place(5, 1, 5, 5, 0, 5, Face::Up)]
}

#[test]
fn a_legal_plan_passes_and_builds_exactly_the_schematic() {
    let rep = simulate(&base_plan(good_steps()), &two_stack()).unwrap();
    assert_eq!(rep.placed, 2);
}

#[test]
fn rejects_out_of_reach_placement() {
    let mut p = base_plan(good_steps());
    p.header.start = (-5, 0, 5);
    let e = simulate(&p, &two_stack()).unwrap_err();
    assert!(e.msg.contains("out of reach"), "{e:?}");
}

#[test]
fn rejects_clicking_air() {
    let p = base_plan(vec![place(5, 1, 5, 5, 0, 5, Face::Up)]);
    assert!(simulate(&p, &two_stack()).unwrap_err().msg.contains("is air"));
}

#[test]
fn rejects_standing_in_the_air_and_big_steps() {
    let mut steps = vec![Step::Move((4, 0, 5))];
    steps.push(Step::Move((5, 2, 5)));
    let e = simulate(&base_plan(steps), &two_stack()).unwrap_err();
    assert!(e.msg.contains("height change"), "{e:?}");
    let e = simulate(&base_plan(vec![Step::Move((4, 0, 6))]), &two_stack()).unwrap_err();
    assert!(e.msg.contains("not one horizontal step"));
}

#[test]
fn rejects_placing_into_the_players_own_cell() {
    let mut p = base_plan(good_steps());
    p.header.start = (5, 0, 5);
    assert!(simulate(&p, &two_stack()).is_err());
}

#[test]
fn scaffold_must_be_removed_and_only_scaffold_may_be_broken() {
    let mut steps = good_steps();
    steps.insert(0, Step::Pillar((3, 0, 5)));
    assert!(simulate(&base_plan(steps.clone()), &two_stack()).unwrap_err().msg.contains("left standing"));
    steps.push(Step::Break(BlockPos::new(3, 0, 5)));
    let ok = simulate(&base_plan(steps.clone()), &two_stack());
    assert!(ok.is_ok(), "{ok:?}");
    let bad = base_plan(vec![place(5, 0, 5, 5, -1, 5, Face::Up), Step::Break(BlockPos::new(5, 0, 5))]);
    assert!(simulate(&bad, &two_stack()).unwrap_err().msg.contains("only scaffold"));
}

#[test]
fn checkpoints_compare_count_and_hash() {
    let mut steps = vec![good_steps().remove(0)];
    let hash = block_hash(BlockPos::new(5, 0, 5), "stone");
    steps.push(Step::Checkpoint { n: 1, count: 1, hash });
    steps.push(good_steps().remove(1));
    assert!(simulate(&base_plan(steps.clone()), &two_stack()).is_ok());
    steps[1] = Step::Checkpoint { n: 1, count: 1, hash: hash ^ 1 };
    assert!(simulate(&base_plan(steps), &two_stack()).unwrap_err().msg.contains("checkpoint"));
}

#[test]
fn unreachable_blocks_are_allowed_to_be_missing_only_when_declared() {
    let steps = vec![good_steps().remove(0)];
    assert!(simulate(&base_plan(steps.clone()), &two_stack()).is_err());
    let mut p = base_plan(steps);
    p.header.unreachable.push((BlockPos::new(5, 1, 5), "stone".into()));
    assert!(simulate(&p, &two_stack()).is_ok());
}
