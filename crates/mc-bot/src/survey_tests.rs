use super::*;
use crate::schematic::Schematic;

const WALLS: &str = "palette S stone\nlayer 0\nSSS\nS.S\nSSS\nlayer 1\nSSS\nS.S\nSSS\n";
const OFF: Offset = Offset { dx: 100, dy: -124, dz: -40 };

fn walls() -> Plan {
    crate::slice::slice(&Schematic::parse(WALLS).unwrap(), (2, 64, 2), 767).unwrap()
}

fn walls_area(saved: Option<usize>, extras: &[Cell]) -> Area {
    area(&walls(), OFF, saved, extras).unwrap()
}

/// A superflat world (ground below y = -60) with `blocks` on or in it.
fn flat(blocks: &[(BlockPos, &str)]) -> impl Fn(BlockPos) -> Option<(String, Props)> {
    let blocks: HashMap<BlockPos, String> = blocks.iter().map(|(p, s)| (*p, s.to_string())).collect();
    move |p| {
        let ground = if p.y < -60 { "grass_block" } else { "air" };
        Some(split_state(blocks.get(&p).map_or(ground, String::as_str)))
    }
}

#[test]
fn a_flat_empty_site_is_clear_and_covers_the_whole_build() {
    let a = walls_area(None, &[]);
    assert_eq!(a.ground_y, -60);
    // blueprint (0,0,0) is at plan (2,64,2), so at world (102,-60,-38)
    assert!(a.used.contains(&BlockPos::new(102, -60, -38)) && a.used.contains(&BlockPos::new(104, -59, -36)));
    assert!(survey(&a, &flat(&[])).is_clear());
}

#[test]
fn a_block_in_a_build_cell_collides_even_if_natural() {
    let a = walls_area(None, &[]);
    for block in ["oak_planks", "dirt", "water", "poppy"] {
        let s = survey(&a, &flat(&[(BlockPos::new(103, -60, -38), block)]));
        assert_eq!((s.collisions.len(), s.nearby.len()), (1, 0), "{block}");
        assert!(!s.keep.contains(&s.collisions[0].pos));
    }
}

#[test]
fn replaceable_vegetation_is_not_in_the_way() {
    let a = walls_area(None, &[]);
    let at = BlockPos::new(103, -60, -38);
    for block in ["short_grass", "snow[layers=1]", "tall_grass[half=lower]"] {
        assert!(survey(&a, &flat(&[(at, block)])).is_clear(), "{block}");
    }
    assert!(!survey(&a, &flat(&[(at, "snow[layers=3]")])).is_clear());
}

#[test]
fn the_margin_ignores_terrain_but_not_somebodys_blocks_and_keeps_both() {
    let a = walls_area(None, &[]);
    let edge = BlockPos::new(a.hi.x + MARGIN, -60, a.lo.z);
    let natural = survey(&a, &flat(&[(edge, "oak_leaves[persistent=false]")]));
    assert!(natural.is_clear() && natural.keep.contains(&edge));
    for block in ["chest", "oak_leaves[persistent=true]", "oak_log", "torch"] {
        let s = survey(&a, &flat(&[(edge, block)]));
        assert_eq!(s.nearby, vec![Found { pos: edge, block: split_state(block).0 }], "{block}");
        assert!(s.keep.contains(&edge));
    }
    let outside = BlockPos::new(a.hi.x + MARGIN + 1, -60, a.lo.z);
    assert!(survey(&a, &flat(&[(outside, "chest")])).is_clear());
}

#[test]
fn holes_water_and_unloaded_chunks_stop_the_build() {
    let a = walls_area(None, &[]);
    for block in ["air", "water", "farmland"] {
        let s = survey(&a, &flat(&[(BlockPos::new(103, -61, -37), block)]));
        assert_eq!(s.no_ground.len(), 1, "{block}");
    }
    let s = survey(&a, &|p| (p.x < 103).then(|| flat(&[])(p).unwrap()));
    assert!(s.unknown > 0 && !s.is_clear());
    assert!(s.report(&a).iter().any(|l| l.contains(&s.unknown.to_string())));
}

#[test]
fn a_resume_accepts_what_the_build_itself_left_and_nothing_else() {
    let plan = walls();
    let first = plan.steps.iter().position(|s| matches!(s, Step::Place { .. })).unwrap();
    let Step::Place { pos, .. } = &plan.steps[first] else { unreachable!() };
    let own = OFF.pos(*pos);
    assert!(!survey(&walls_area(None, &[]), &flat(&[(own, "stone")])).is_clear());
    let resumed = walls_area(Some(first + 1), &[]);
    let s = survey(&resumed, &flat(&[(own, "stone")]));
    assert!(s.is_clear() && !s.keep.contains(&own));
    assert!(!survey(&resumed, &flat(&[(own, "gold_block")])).is_clear());
    let extra = (resumed.lo.x, -60, resumed.lo.z - 1);
    let at = BlockPos::new(extra.0, extra.1, extra.2);
    assert!(survey(&walls_area(Some(first + 1), &[extra]), &flat(&[(at, "cobblestone")])).is_clear());
    assert!(!survey(&resumed, &flat(&[(at, "cobblestone")])).is_clear());
}
