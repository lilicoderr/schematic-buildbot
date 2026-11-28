//! Undo planning offline: build a plan, replay it to the finished world, plan the way down and let
//! the strict simulator judge it.
use mc_bot::nav::Cell;
use mc_bot::schematic::{split_state, Schematic};
use mc_bot::state_ids::state_id;
use mc_bot::undo;
use std::collections::{BTreeMap, HashMap};

fn blueprint(name: &str) -> Schematic {
    let path = format!("{}/../../blueprints/{name}", env!("CARGO_MANIFEST_DIR"));
    if name.ends_with(".txt") {
        Schematic::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    } else {
        mc_bot::schem::parse(&std::fs::read(path).unwrap(), name).unwrap()
    }
}

const MIXED: &str = "palette S stone\npalette D oak_door[facing=north,half=lower]\npalette U oak_door[facing=north,half=upper]\n\
palette T wall_torch[facing=west]\npalette L lantern[hanging=true]\npalette K oak_stairs[facing=east]\npalette B red_bed[part=foot,facing=south]\n\
palette H red_bed[part=head,facing=south]\nlayer 0\nSSSSS\nS...S\nSB.TS\nSH..S\nSSDSS\nlayer 1\nSSSSS\nS...S\nS...S\nS...S\nSSUSS\nlayer 2\nSSSSS\nSKKKS\nSLLLS\nS...S\nSSSSS\nlayer 3\nSSSSS\nSSSSS\nSSSSS\nSSSSS\nSSSSS\n";

fn id(state: &str) -> u32 {
    let (n, p) = split_state(state);
    state_id(&n, &p).unwrap()
}

/// Plan the build, then the undo; the world between is exactly the plan's finished blocks.
fn undo_of(s: &Schematic, foreign: &[(Cell, &str)]) -> (undo::Undo, usize) {
    let plan = mc_bot::slice::slice(s, (2, 64, 2), 767).unwrap();
    let placed = undo::placed_blocks(&plan, None).unwrap();
    let sorted = undo::sort(&placed, plan.header.origin.1, &|c| placed.get(&c).map(|s| id(s)));
    assert_eq!((sorted.ours.len(), sorted.changed.len(), sorted.unknown), (placed.len(), 0, 0));
    let foreign: HashMap<Cell, String> = foreign.iter().map(|(c, s)| (*c, s.to_string())).collect();
    (undo::plan(&plan.header, &sorted.ours, &foreign).unwrap(), placed.len())
}

#[test]
fn every_small_blueprint_comes_down_and_the_bot_ends_on_the_ground() {
    for name in ["cottage.txt", "stair-tower.txt", "state-lab.txt", "pixel-heart.txt"] {
        let (u, n) = undo_of(&blueprint(name), &[]);
        assert!(u.unreachable.is_empty(), "{name}: {:?}", u.unreachable);
        let breaks = u.plan.steps.iter().filter(|s| matches!(s, mc_bot::plan::Step::Break(_))).count();
        assert!(breaks >= n - u.demolition.blocks.len().min(n) && breaks > 0, "{name}");
    }
}

#[test]
fn doors_beds_torches_and_lanterns_come_away_in_a_legal_order() {
    let s = Schematic::parse(MIXED).unwrap();
    let (u, _) = undo_of(&s, &[]);
    assert!(u.unreachable.is_empty());
    // a hanging lantern goes before the block it hangs from, a torch before its wall
    let order = undo::order(&u.demolition.blocks);
    let pos: HashMap<Cell, usize> = order.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    let mut attached = 0;
    for (&c, state) in &u.demolition.blocks {
        let (name, props) = split_state(state);
        let Some(support) = mc_bot::sim_rules::needs_support(&name, &props, mc_bot::world::BlockPos::new(c.0, c.1, c.2)) else { continue };
        if let (Some(a), Some(b)) = (pos.get(&c), pos.get(&(support.x, support.y, support.z))) {
            attached += 1;
            assert!(a < b, "{state} at {c:?} must go before its support");
        }
    }
    assert!(attached >= 4, "the torch, the lanterns and the door are attached: {attached}");
}

#[test]
fn a_block_that_is_not_ours_beside_or_inside_stays_and_is_never_in_the_way_of_the_check() {
    let s = Schematic::parse(MIXED).unwrap();
    // plan coordinates: blueprint (0,0,0) is at (2,64,2); this is one cell outside the east wall, and one cell
    // the build never uses at ground level in front of the door
    let beside: Cell = (2 + 5, 64, 2 + 2);
    let (u, _) = undo_of(&s, &[(beside, "oak_planks")]);
    assert!(u.demolition.blocks.contains_key(&beside) && !u.demolition.targets.contains(&beside));
    assert!(u.plan.steps.iter().all(|s| !matches!(s, mc_bot::plan::Step::Break(p) if (p.x, p.y, p.z) == beside)));
}

#[test]
fn what_changed_or_is_gone_is_sorted_out() {
    let s = Schematic::parse(MIXED).unwrap();
    let plan = mc_bot::slice::slice(&s, (2, 64, 2), 767).unwrap();
    let placed = undo::placed_blocks(&plan, None).unwrap();
    let cells: BTreeMap<Cell, &String> = placed.iter().map(|(c, s)| (*c, s)).collect();
    let (&swapped, _) = cells.iter().find(|(_, s)| s.starts_with("stone")).unwrap();
    let (&removed, _) = cells.iter().find(|(c, s)| s.starts_with("stone") && **c != swapped).unwrap();
    let (&unloaded, _) = cells.iter().find(|(c, s)| s.starts_with("lantern") && **c != swapped).unwrap();
    let state_at = |c: Cell| {
        if c == swapped {
            Some(id("gold_block"))
        } else if c == removed {
            Some(0)
        } else if c == unloaded {
            None
        } else {
            placed.get(&c).map(|s| id(s))
        }
    };
    let sorted = undo::sort(&placed, plan.header.origin.1, &state_at);
    assert_eq!((sorted.gone, sorted.unknown), (1, 1));
    assert_eq!(sorted.changed, vec![(swapped, "gold_block".to_string())]);
    assert_eq!(sorted.ours.len(), placed.len() - 3);
}

#[test]
fn a_whole_factory_comes_down_with_scaffold_and_ends_clean() {
    let (u, n) = undo_of(&blueprint("factory-small.schem"), &[]);
    assert!(u.unreachable.is_empty(), "{} blocks unreachable", u.unreachable.len());
    assert!(n > 2000 && u.plan.steps.len() > n);
}
