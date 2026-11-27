use super::*;
use crate::world::{within_reach, Face};

/// Apply a path to the terrain, asserting after every act that the bot stands on something solid.
fn walk_and_check(t: &mut Terrain, scaffold: &mut HashSet<Cell>, start: Cell, path: &[Act]) -> Cell {
    let mut at = start;
    for act in path {
        match *act {
            Act::Walk(c) => assert!(t.can_stand(c), "walk onto {c:?} without support"),
            Act::Bridge(c) | Act::StepDown(c) => {
                let floor = (c.0, c.1 - 1, c.2);
                if matches!(act, Act::Bridge(_)) {
                    assert_eq!(c.1, at.1, "bridging stays level");
                } else {
                    assert!((1..=2).contains(&(at.1 - c.1)), "a step down drops 1 or 2");
                    let held = Face::HORIZONTAL.iter().any(|f| t.solid((floor.0 + f.offset().0, floor.1, floor.2 + f.offset().2)));
                    assert!(held, "the new floor {floor:?} is clicked against a block beside it");
                }
                assert!(!t.solid(floor));
                t.add_block(floor, true);
                scaffold.insert(floor);
            }
            Act::Pillar(c) => {
                assert_eq!((c.0, c.1 - 1, c.2), at, "pillar is straight up");
                t.add_block(at, true);
                scaffold.insert(at);
            }
            Act::Dig(_) => {
                let under = (at.0, at.1 - 1, at.2);
                assert!(scaffold.remove(&under), "only scaffold may be dug");
                t.remove_block(under);
            }
        }
        at = act.to();
        assert!(t.can_stand(at), "no solid support under {at:?} after {act:?}");
    }
    at
}

fn run(t: &mut Terrain, start: Cell, target: BlockPos) -> Option<(Cell, Vec<Act>)> {
    let mut scaffold = HashSet::new();
    let keep = [(target.x, target.y, target.z)];
    let path = Query { terrain: t, scaffold: &scaffold, keep_free: &keep, spent: None, max_nodes: 60_000 }.to_place(start, target)?;
    let end = walk_and_check(t, &mut scaffold, start, &path);
    Some((end, path))
}

/// The 3x3 well of `climbs_out_of_a_deep_walled_pocket`, again (acts change the terrain).
fn flat_copy(t: &Terrain) -> Terrain {
    let mut copy = flat(t.sky_top);
    for y in 0..10 {
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                if t.solid((x, y, z)) {
                    copy.add_block((x, y, z), true);
                }
            }
        }
    }
    copy
}

fn flat(sky_top: i32) -> Terrain {
    let mut t = Terrain::new(0, HashSet::new());
    t.sky_top = sky_top;
    t
}

#[test]
fn pillars_up_to_a_high_block_standing_on_support_all_the_way() {
    let mut t = flat(20);
    let target = BlockPos::new(3, 12, 0);
    let (end, path) = run(&mut t, (0, 0, 0), target).expect("reachable by pillar");
    assert!(path.iter().any(|a| matches!(a, Act::Pillar(_))));
    assert!(within_reach(cell_feet(end), target));
}

#[test]
fn climbs_out_of_a_deep_walled_pocket() {
    let mut t = flat(14);
    // a 3x3 well with walls 10 high, open to the sky
    for y in 0..10 {
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                if x.abs() == 2 || z.abs() == 2 {
                    t.add_block((x, y, z), true);
                }
            }
        }
    }
    // to the ground outside: down the wall on steps placed against it
    let (_, down) = run(&mut flat_copy(&t), (0, 0, 0), BlockPos::new(12, 0, 0)).expect("down the outer wall");
    assert!(down.iter().any(|a| matches!(a, Act::StepDown(_))));
    let (end, path) = run(&mut t, (0, 0, 0), BlockPos::new(12, 10, 0)).expect("out of the well");
    assert!(end.0 > 2, "left the well: {end:?}");
    assert!(path.iter().any(|a| matches!(a, Act::Pillar(_))));
}

#[test]
fn bridges_off_an_island() {
    let mut t = flat(10);
    for y in 0..6 {
        for x in 0..3 {
            for z in 0..3 {
                t.add_block((x, y, z), true);
            }
        }
    }
    let (_, path) = run(&mut t, (1, 6, 1), BlockPos::new(11, 6, 1)).expect("bridge");
    assert!(path.iter().any(|a| matches!(a, Act::Bridge(_))));
}

#[test]
fn a_sealed_room_is_reported_unreachable_not_flown_out_of() {
    let mut t = flat(8);
    for x in -2i32..=2 {
        for y in 0..=3 {
            for z in -2i32..=2 {
                if x.abs() == 2 || z.abs() == 2 || y == 3 {
                    t.add_block((x, y, z), true);
                }
            }
        }
    }
    assert!(run(&mut t, (0, 0, 0), BlockPos::new(20, 0, 0)).is_none());
}
