//! Table-driven checks; expected values come from docs/RE/block_states.md (section 1 table).

use mc_bot::placement::{plan, Action, Plan};
use mc_bot::schematic::Block;
use mc_bot::world::{BlockPos, Face};
use Face::{Down, East, North, South, Up, West};

const AT: BlockPos = BlockPos { x: 0, y: 5, z: 0 };

/// Plan with every neighbour clickable.
fn free(state: &str) -> Plan {
    match plan(&Block::from_state(AT, state), |p| p != AT) {
        Action::Place(p) => p,
        Action::Skip(r) => panic!("{state}: skipped ({r})"),
    }
}

/// (state, clicked face (None = any), cursor y, yaw, pitch)
type Row = (&'static str, Option<Face>, Option<f32>, Option<f32>, Option<f32>);

const N: Option<f32> = None;
const fn y(v: f32) -> Option<f32> {
    Some(v)
}

const TABLE: &[Row] = &[
    // stairs: facing = player direction; half from the face / cursor
    ("oak_stairs[facing=south,half=bottom]", Some(Up), y(1.0), y(0.0), N),
    ("oak_stairs[facing=west,half=top]", Some(Down), y(0.0), y(90.0), N),
    ("stone_slab[type=bottom]", Some(Up), N, N, N),
    ("stone_slab[type=top]", Some(Down), N, N, N),
    // pillars: axis of the clicked face
    ("oak_log[axis=y]", Some(Up), N, N, N),
    ("chain[axis=y]", Some(Up), N, N, N),
    // trapdoors on a wall: facing = clicked face; the yaw only matters for floor clicks
    ("oak_trapdoor[facing=east,half=bottom]", Some(Up), N, y(90.0), N),
    ("oak_trapdoor[facing=north,half=top]", Some(Down), N, y(0.0), N),
    // buttons and levers: face from the clicked face
    ("stone_button[face=floor,facing=east]", Some(Up), N, y(-90.0), N),
    ("lever[face=ceiling,facing=north]", Some(Down), N, y(180.0), N),
    ("lever[face=wall,facing=west]", Some(West), N, N, N),
    // torches, wall variants, lanterns
    ("torch", Some(Up), N, N, N),
    ("wall_torch[facing=north]", Some(North), N, N, N),
    ("oak_wall_sign[facing=east]", Some(East), N, N, N),
    ("red_wall_banner[facing=south]", Some(South), N, N, N),
    ("ladder[facing=west]", Some(West), N, N, N),
    ("lantern[hanging=true]", Some(Down), N, N, N),
    ("lantern[hanging=false]", Some(Up), N, N, N),
    // 16-step rotation: sign / banner yaw = r*22.5 - 180, skull yaw = r*22.5
    ("oak_sign[rotation=0]", Some(Up), N, y(-180.0), N),
    ("oak_sign[rotation=8]", Some(Up), N, y(0.0), N),
    ("white_banner[rotation=4]", Some(Up), N, y(-90.0), N),
    ("skeleton_skull[rotation=4]", Some(Up), N, y(90.0), N),
    // facing the player
    ("furnace[facing=north]", None, N, y(0.0), N),
    ("chest[facing=east,type=single]", None, N, y(90.0), N),
    ("anvil[facing=west]", None, N, y(0.0), N),
    // 6-way: dispenser faces against the look, observer along it
    ("dispenser[facing=up]", None, N, N, y(90.0)),
    ("dispenser[facing=down]", None, N, N, y(-90.0)),
    ("piston[facing=north]", None, N, y(0.0), y(0.0)),
    ("barrel[facing=east]", None, N, y(90.0), y(0.0)),
    ("observer[facing=up]", None, N, N, y(-90.0)),
    ("observer[facing=east]", None, N, y(-90.0), y(0.0)),
    ("crafter[orientation=down_north]", None, N, y(0.0), y(-90.0)),
    // clicked face decides
    ("hopper[facing=east]", Some(West), N, N, N),
    ("hopper[facing=down]", Some(Up), N, N, N),
    ("end_rod[facing=north]", Some(North), N, N, N),
    ("white_shulker_box[facing=down]", Some(Down), N, N, N),
    ("red_bed[facing=north,part=foot]", Some(Up), N, y(180.0), N),
    ("oak_door[facing=east,half=lower,hinge=left]", Some(Up), N, y(-90.0), N),
];

#[test]
fn every_family_gets_the_documented_click_and_look() {
    for &(state, face, cy, yaw, pitch) in TABLE {
        let p = free(state);
        let c = &p.clicks[0];
        if let Some(face) = face {
            assert_eq!(c.face, face, "{state}");
            assert_eq!(c.support.offset(face), AT, "{state}");
        }
        if let Some(cy) = cy {
            assert_eq!(c.cursor.1, cy, "{state}");
        }
        assert_eq!((p.yaw, p.pitch), (yaw, pitch), "{state}");
        assert!(p.unhonoured.is_empty(), "{state}: {:?}", p.unhonoured);
        assert!(p.missing_support.is_none(), "{state}");
    }
}

#[test]
fn axis_clicks_a_perpendicular_face() {
    for (state, faces) in [("oak_log[axis=x]", [West, East]), ("basalt[axis=z]", [North, South])] {
        assert!(faces.contains(&free(state).clicks[0].face), "{state}");
    }
}

#[test]
fn only_single_chests_sneak() {
    assert!(free("chest[facing=north,type=single]").sneak);
    assert!(!free("chest[facing=north,type=left]").sneak);
    assert!(!free("furnace[facing=north]").sneak);
}

#[test]
fn door_cursor_picks_the_hinge() {
    // (facing, hinge, x, z): facing south x<0.5 -> right; north x>0.5; east z>0.5; west z<0.5
    for (facing, hinge, x, z) in [
        ("south", "right", 0.25, 0.5),
        ("south", "left", 0.75, 0.5),
        ("north", "right", 0.75, 0.5),
        ("east", "right", 0.5, 0.75),
        ("west", "right", 0.5, 0.25),
        ("west", "left", 0.5, 0.75),
    ] {
        let c = free(&format!("oak_door[facing={facing},half=lower,hinge={hinge}]")).clicks[0].cursor;
        assert_eq!((c.0, c.2), (x, z), "{facing} {hinge}");
    }
}

#[test]
fn stacked_items_repeat_the_click_on_the_block_itself() {
    for (state, clicks) in [("stone_slab[type=double]", 2), ("candle[candles=3]", 3), ("snow[layers=4]", 4)] {
        let p = free(state);
        assert_eq!(p.clicks.len(), clicks, "{state}");
        assert!(p.clicks[1..].iter().all(|c| c.support == AT && c.face == Up), "{state}");
    }
}

#[test]
fn attached_blocks_wait_for_their_support() {
    // only the floor exists: a wall torch must not be turned into something else
    let b = Block::from_state(AT, "wall_torch[facing=east]");
    let Action::Place(p) = plan(&b, |q| q == AT.below()) else { panic!() };
    assert_eq!(p.missing_support, Some(AT.offset(West)));
}

#[test]
fn redstone_and_water_states_are_reported_as_unobtainable() {
    assert_eq!(free("oak_trapdoor[facing=north,half=bottom,open=true]").unhonoured, ["open"]);
    assert_eq!(free("oak_slab[type=bottom,waterlogged=true]").unhonoured, ["waterlogged"]);
}

#[test]
fn wall_variants_use_the_standing_item() {
    use mc_bot::blocks::item_id;
    for (block, item) in [
        ("wall_torch", "torch"),
        ("oak_wall_sign", "oak_sign"),
        ("oak_wall_hanging_sign", "oak_hanging_sign"),
        ("red_wall_banner", "red_banner"),
        ("skeleton_wall_skull", "skeleton_skull"),
        ("redstone_wire", "redstone"),
    ] {
        assert!(item_id(block).is_some(), "{block}");
        assert_eq!(item_id(block), item_id(item), "{block}");
    }
}
