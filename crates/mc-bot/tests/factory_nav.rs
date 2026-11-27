//! Navigation against the real small-factory blueprint (no server needed).
use mc_bot::nav::{self, Terrain};
use mc_bot::world::BlockPos;
use std::collections::HashSet;

fn small() -> mc_bot::schematic::Schematic {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../blueprints/factory-small.schem")).unwrap();
    mc_bot::schem::parse(&bytes, "factory-small").unwrap()
}

#[test]
fn walled_in_machine_top_needs_a_pillar() {
    let s = small();
    let (order, _) = s.plan_order();
    let mut t = Terrain::new(0, HashSet::new());
    let o = (5, 0, 11);
    for b in order.iter().filter(|b| b.pos.y <= 3) {
        t.add_block((o.0 + b.pos.x, b.pos.y, o.2 + b.pos.z), nav::is_standable_surface(&b.name, b.prop("type")));
    }
    let start = (17, 2, 22);
    let target = BlockPos::new(6, 4, 16);
    assert!(t.can_stand(start));
    assert!(t.reachable_count(start, 24, 40_000) < 50, "the machine top is an enclosed island");
    assert!(t.route_for_run(start, &[target], 24, 40_000).is_none());
    // A scaffold pillar on the spot lifts the bot over the surrounding walls.
    let mut escaped = None;
    for h in 1..=6 {
        for k in 0..h {
            t.add_block((start.0, start.1 + k, start.2), true);
        }
        if let Some(r) = t.route_for_run((start.0, start.1 + h, start.2), &[target], 24, 40_000) {
            escaped = Some((h, r));
            break;
        }
    }
    let (h, route) = escaped.expect("a pillar of at most 6 gets out");
    for c in route.iter().skip(1) {
        assert!(t.can_stand(*c), "step {c:?} (pillar {h}) has no support");
    }
}
