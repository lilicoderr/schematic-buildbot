use super::*;
use crate::schematic::split_state;
use crate::state_ids::{state_id, state_of};
use crate::state_table::STATE_COUNT;
use crate::world::Face;

fn all() -> Vec<Transform> {
    let mut v = Vec::new();
    for turns in 0..4 {
        for mirror in [Mirror::None, Mirror::X, Mirror::Z] {
            v.push(Transform { turns, mirror });
        }
    }
    v
}

fn t(rotate: &str, mirror: Option<&str>) -> Transform {
    Transform::parse(Some(rotate), mirror).unwrap()
}

/// `name[props]` through `tr`, as text.
fn go(tr: Transform, state: &str) -> String {
    let (name, props) = split_state(state);
    Block { pos: BlockPos::new(0, 0, 0), name, props: tr.state(&props) }.state_string()
}

#[test]
fn rotation_follows_the_compass_and_mirrors_swap_the_axes() {
    assert_eq!(go(t("90", None), "oak_stairs[facing=north]"), "oak_stairs[facing=east]");
    assert_eq!(go(t("180", None), "oak_stairs[facing=east]"), "oak_stairs[facing=west]");
    assert_eq!(go(t("270", None), "oak_stairs[facing=north]"), "oak_stairs[facing=west]");
    assert_eq!(go(t("0", Some("x")), "furnace[facing=east]"), "furnace[facing=west]");
    assert_eq!(go(t("0", Some("x")), "furnace[facing=north]"), "furnace[facing=north]");
    assert_eq!(go(t("0", Some("z")), "furnace[facing=north]"), "furnace[facing=south]");
    assert_eq!(go(t("90", None), "hopper[facing=down]"), "hopper[facing=down]");
    assert_eq!(go(t("90", None), "oak_log[axis=x]"), "oak_log[axis=z]");
    assert_eq!(go(t("90", None), "oak_log[axis=y]"), "oak_log[axis=y]");
    assert_eq!(go(t("180", None), "oak_log[axis=x]"), "oak_log[axis=x]");
}

#[test]
fn the_facing_of_a_state_goes_where_its_position_goes() {
    for tr in all() {
        for (face, name) in Face::HORIZONTAL.iter().zip(["north", "east", "south", "west"]) {
            let (dx, _, dz) = face.offset();
            let image = Face::from_name(&tr.direction(name)).unwrap().offset();
            assert_eq!(tr.point((dx, dz)), (image.0, image.2), "{tr:?} {name}");
        }
    }
}

#[test]
fn handed_properties_flip_only_under_a_mirror() {
    assert_eq!(go(t("90", None), "oak_stairs[facing=north,shape=inner_left]"), "oak_stairs[facing=east,shape=inner_left]");
    assert_eq!(go(t("0", Some("x")), "oak_stairs[facing=east,shape=inner_left]"), "oak_stairs[facing=west,shape=inner_right]");
    assert_eq!(go(t("0", Some("z")), "oak_stairs[shape=outer_right]"), "oak_stairs[shape=outer_left]");
    assert_eq!(go(t("180", Some("x")), "oak_door[hinge=left]"), "oak_door[hinge=right]");
    assert_eq!(go(t("90", None), "oak_door[hinge=left]"), "oak_door[hinge=left]");
    assert_eq!(go(t("0", Some("z")), "chest[type=left]"), "chest[type=right]");
    assert_eq!(go(t("0", Some("z")), "chest[type=single]"), "chest[type=single]");
    assert_eq!(go(t("0", Some("z")), "stone_slab[type=top]"), "stone_slab[type=top]");
}

#[test]
fn rotation_steps_connections_rails_and_orientation() {
    assert_eq!(
        go(t("90", None), "glass_pane[east=true,north=false,south=false,west=false]"),
        "glass_pane[east=false,north=false,south=true,west=false]"
    );
    assert_eq!(go(t("0", Some("x")), "oak_fence[east=true,west=false]"), "oak_fence[east=false,west=true]");
    assert_eq!(go(t("90", None), "rail[shape=north_south]"), "rail[shape=east_west]");
    assert_eq!(go(t("90", None), "rail[shape=south_east]"), "rail[shape=south_west]");
    assert_eq!(go(t("90", None), "rail[shape=ascending_north]"), "rail[shape=ascending_east]");
    assert_eq!(go(t("0", Some("x")), "rail[shape=north_east]"), "rail[shape=north_west]");
    assert_eq!(go(t("90", None), "crafter[orientation=down_east]"), "crafter[orientation=down_south]");
    assert_eq!(go(t("90", None), "jigsaw[orientation=west_up]"), "jigsaw[orientation=north_up]");
    // 16 step rotation: 0 south, 4 west, 8 north, 12 east
    assert_eq!(go(t("90", None), "oak_sign[rotation=0]"), "oak_sign[rotation=4]");
    assert_eq!(go(t("0", Some("x")), "oak_sign[rotation=4]"), "oak_sign[rotation=12]");
    assert_eq!(go(t("0", Some("z")), "oak_sign[rotation=0]"), "oak_sign[rotation=8]");
    assert_eq!(go(t("0", Some("z")), "oak_sign[rotation=3]"), "oak_sign[rotation=5]");
}

#[test]
fn every_block_state_stays_a_valid_state_of_the_same_block() {
    for tr in all() {
        for id in 0..STATE_COUNT {
            let (name, props) = state_of(id).unwrap();
            let moved = tr.state(&props);
            let new = state_id(name, &moved).unwrap_or_else(|| panic!("{tr:?}: {name} {props:?} -> {moved:?}"));
            assert_eq!(state_of(new).unwrap().1, moved, "{tr:?} {name}");
        }
    }
}

#[test]
fn turning_back_and_flipping_twice_restore_every_state() {
    let (quarter, back) = (t("90", None), t("270", None));
    let flips = [t("0", Some("x")), t("0", Some("z"))];
    for sid in 0..STATE_COUNT {
        let (_, p) = state_of(sid).unwrap();
        let four = quarter.state(&quarter.state(&quarter.state(&quarter.state(&p))));
        assert_eq!(four, p, "four quarter turns {sid}");
        assert_eq!(back.state(&quarter.state(&p)), p, "90 then 270 {sid}");
        for f in flips {
            assert_eq!(f.state(&f.state(&p)), p, "{f:?} twice {sid}");
            // mirror, turn, mirror is the turn the other way
            assert_eq!(f.state(&quarter.state(&f.state(&p))), back.state(&p), "{f:?} conjugation {sid}");
        }
        assert_eq!(flips[1].state(&flips[0].state(&p)), t("180", None).state(&p), "x then z is 180 {sid}");
    }
}

#[test]
fn blueprints_turn_with_their_corner_kept_and_come_back() {
    let text = "palette S oak_stairs[facing=north,shape=inner_left]\npalette D oak_door[facing=east,hinge=left]\nlayer 0\nSDD\n...\n.S.\nlayer 1\nD..\n";
    let s = Schematic::parse(text).unwrap();
    let (min, max) = s.bounds();
    let r = t("90", None).apply(&s);
    let (rmin, rmax) = r.bounds();
    assert_eq!((rmin, (rmax.x - rmin.x, rmax.z - rmin.z)), (min, (max.z - min.z, max.x - min.x)));
    assert_eq!(r.blocks.len(), s.blocks.len());
    // clockwise from above (x, z) -> (-z, x): the stairs at (0,0,0) and (1,0,2) land on (2,0,0) and (0,0,1)
    let mut at: Vec<_> = r.blocks.iter().filter(|b| b.name == "oak_stairs").map(|b| (b.pos.x, b.pos.z, b.prop("facing"))).collect();
    at.sort();
    assert_eq!(at, vec![(0, 1, Some("east")), (2, 0, Some("east"))]);
    for turns in 0..4 {
        let turn = Transform { turns, mirror: Mirror::None };
        let undo = Transform { turns: (4 - turns) % 4, mirror: Mirror::None };
        assert_eq!(undo.apply(&turn.apply(&s)).fingerprint(), s.fingerprint(), "{turns} turns");
    }
    for m in ["x", "z"] {
        let flipped = t("0", Some(m)).apply(&s);
        assert_ne!(flipped.fingerprint(), s.fingerprint());
        assert_eq!(t("0", Some(m)).apply(&flipped).fingerprint(), s.fingerprint(), "mirror {m} twice");
    }
    let both = t("0", Some("z")).apply(&t("0", Some("x")).apply(&s));
    assert_eq!(t("180", None).apply(&s).fingerprint(), both.fingerprint());
    assert!(Transform::parse(Some("45"), None).is_err() && Transform::parse(None, Some("y")).is_err());
    assert!(Transform::parse(None, None).unwrap().is_identity());
}
