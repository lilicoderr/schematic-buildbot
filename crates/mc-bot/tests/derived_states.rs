//! Neighbour-derived states, expected values from docs/RE/block_states.md section 2.

use mc_bot::derive::{derived, Layout};
use mc_bot::predict::mismatches;
use mc_bot::schematic::Block;
use mc_bot::world::BlockPos;

fn blocks(list: &[((i32, i32, i32), &str)]) -> Vec<Block> {
    list.iter().map(|&((x, y, z), s)| Block::from_state(BlockPos::new(x, y, z), s)).collect()
}

/// Derived properties of the first block of `list`, as "k=v,..."; ground is below y = 0.
fn first(list: &[((i32, i32, i32), &str)]) -> String {
    let all = blocks(list);
    let mut l = Layout::new(0);
    all.iter().for_each(|b| l.insert(b.clone()));
    derived(&all[0], &l).iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(",")
}

#[test]
fn stair_corners() {
    const S: &str = "oak_stairs[facing=north,half=bottom]";
    // (neighbour position relative to the stair, neighbour facing, expected shape); north = -z
    for (at, facing, shape) in [
        ((0, 0, -1), "west", "outer_left"),
        ((0, 0, -1), "east", "outer_right"),
        ((0, 0, 1), "west", "inner_left"),
        ((0, 0, 1), "east", "inner_right"),
        ((0, 0, -1), "north", "straight"),
        ((1, 0, 0), "west", "straight"),
    ] {
        let other = format!("oak_stairs[facing={facing},half=bottom]");
        assert_eq!(first(&[((0, 0, 0), S), (at, &other)]), format!("shape={shape}"), "{facing} {at:?}");
    }
    let top = "oak_stairs[facing=west,half=top]";
    assert_eq!(first(&[((0, 0, 0), S), ((0, 0, -1), top)]), "shape=straight");
    // an identical stair on the checked side forbids the outer corner (canTakeShape)
    let front = "oak_stairs[facing=west,half=bottom]";
    assert_eq!(first(&[((0, 0, 0), S), ((0, 0, -1), front), ((1, 0, 0), S)]), "shape=straight");
}

#[test]
fn fence_and_pane_connections() {
    let around = |me: &str, east: &str| first(&[((0, 0, 0), me), ((1, 0, 0), east)]);
    let only_east = "north=false,east=true,south=false,west=false";
    let none = "north=false,east=false,south=false,west=false";
    for (me, other, connected) in [
        ("oak_fence", "stone", true),
        ("oak_fence", "spruce_fence", true),
        ("oak_fence", "nether_brick_fence", false),
        ("oak_fence", "oak_leaves", false),
        ("oak_fence", "pumpkin", false),
        ("oak_fence", "oak_fence_gate[facing=north]", true),
        ("oak_fence", "oak_fence_gate[facing=east]", false),
        ("oak_fence", "oak_stairs[facing=west,half=bottom]", true),
        ("oak_fence", "oak_stairs[facing=east,half=bottom]", false),
        ("oak_fence", "stone_slab[type=bottom]", false),
        ("glass_pane", "iron_bars", true),
        ("glass_pane", "cobblestone_wall", true),
        ("iron_bars", "glass", true),
        ("glass_pane", "oak_fence", false),
    ] {
        assert_eq!(around(me, other), if connected { only_east } else { none }, "{me} next to {other}");
    }
}

#[test]
fn wall_sides_and_post() {
    const W: &str = "cobblestone_wall";
    let line = [((0, 0, 0), W), ((1, 0, 0), W), ((-1, 0, 0), W)];
    assert_eq!(first(&line), "north=none,east=low,south=none,west=low,up=false");
    assert_eq!(first(&[((0, 0, 0), W)]), "north=none,east=none,south=none,west=none,up=true");
    // a full block on top raises the arms and, on a straight run, keeps no post
    let mut capped = line.to_vec();
    capped.push(((0, 1, 0), "stone"));
    assert_eq!(first(&capped), "north=none,east=tall,south=none,west=tall,up=false");
    // a torch on top is in the wall_post_override tag
    let mut torch = line.to_vec();
    torch.push(((0, 1, 0), "torch"));
    assert_eq!(first(&torch), "north=none,east=low,south=none,west=low,up=true");
}

fn report(list: &[((i32, i32, i32), &str)]) -> Vec<String> {
    mismatches(&blocks(list), 0).iter().map(|m| format!("{}.{}={}", m.name, m.prop, m.predicted)).collect()
}

#[test]
fn chest_pairs_follow_the_build_order() {
    // facing north: the left chest has its partner clockwise (east)
    let pair = [((0, 0, 0), "chest[facing=north,type=left]"), ((1, 0, 0), "chest[facing=north,type=right]")];
    assert!(report(&pair).is_empty(), "{:?}", report(&pair));
    // two singles side by side stay single because singles are placed sneaking
    let singles = [((0, 0, 0), "chest[facing=north,type=single]"), ((1, 0, 0), "chest[facing=north,type=single]")];
    assert!(report(&singles).is_empty());
    // a left chest without its partner stays single
    assert_eq!(report(&[((0, 0, 0), "chest[facing=north,type=left]")]), ["chest.type=single"]);
}

#[test]
fn a_neighbour_forces_the_door_hinge() {
    const DOOR: &str = "oak_door[facing=north,half=lower,hinge=left]";
    // a full block on the right (clockwise of north = east) placed before the door
    let before = [((1, 0, 0), "stone"), ((0, 0, 0), DOOR)];
    assert_eq!(report(&before), ["oak_door.hinge=right"]);
    // placed afterwards it has no effect: hinges are never recomputed
    assert!(report(&[((0, 0, 0), DOOR), ((1, 0, 0), "stone")]).is_empty());
}

#[test]
fn listed_connections_that_cannot_happen_are_reported() {
    let lone = [((0, 0, 0), "oak_fence[east=true,north=false]")];
    assert_eq!(report(&lone), ["oak_fence.east=false"]);
}
