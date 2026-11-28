//! Fault injection: a mock server that answers with the exact planned state ids, but once puts the
//! wrong block, merges a slab into the one below, changes an earlier block like a neighbour update
//! would and drops a stray block into the build. The executor must notice each from the block
//! updates alone and leave the world exactly as planned.

use mc_bot::builder::PlayClient;
use mc_bot::exec::{self, Opts};
use mc_bot::plan::{Plan, Step};
use mc_bot::schematic::{split_state, Schematic};
use mc_bot::slice;
use mc_bot::state_ids::state_id;
use std::collections::HashMap;
use std::net::TcpListener;
use std::thread;

mod mock;
use mock::{ids, Peer};

type Cell = (i32, i32, i32);

fn id(state: &str) -> i32 {
    let (name, props) = split_state(state);
    state_id(&name, &props).unwrap() as i32
}

/// Which injection happens at the n-th placement of a schematic block.
struct Faults {
    wrong: usize,
    merge: usize,
    neighbour: usize,
}

/// Runs the server; returns its final non-air blocks and how many faults it injected.
fn serve(listener: TcpListener, spawn: Cell, wanted: HashMap<Cell, i32>, faults: Faults) -> (HashMap<Cell, i32>, usize) {
    let mut p = Peer::accept(listener, spawn);
    let mut world: HashMap<Cell, i32> = HashMap::new();
    let (mut placements, mut injected) = (0, 0);
    let mut first_block: Option<Cell> = None;
    let cobble = id("cobblestone");
    let double = id("smooth_quartz_slab[type=double]");
    while let Some((packet, mut body)) = p.recv() {
        if packet == ids().sb_play_use_item_on {
            let (at, face, seq) = mock::read_use_item_on(&mut body);
            let d = mock::FACES[face];
            let target = (at.0 + d.0, at.1 + d.1, at.2 + d.2);
            if !world.contains_key(&target) {
                // a cell the plan fills gets its planned state, anything else is scaffold
                let mut state = wanted.get(&target).copied().unwrap_or(cobble);
                if let Some(&planned) = wanted.get(&target) {
                    placements += 1;
                    first_block.get_or_insert(target);
                    if placements == faults.wrong {
                        state = if planned == id("sea_lantern") { id("glass") } else { id("sea_lantern") };
                        injected += 1;
                    }
                }
                let below = (target.0, target.1 - 1, target.2);
                if placements >= faults.merge && injected == 1 && wanted.contains_key(&target) && world.contains_key(&below) {
                    // the live bug: the block below becomes a double slab, the target stays empty
                    world.insert(below, double);
                    p.block_update(below, double);
                    injected += 1;
                } else {
                    world.insert(target, state);
                    p.block_update(target, state);
                }
                if placements == faults.neighbour && injected == 2 {
                    // changes nobody asked for: an old block turns into another, and a stray appears
                    let old = first_block.unwrap();
                    world.insert(old, id("dirt"));
                    p.block_update(old, id("dirt"));
                    let stray = (old.0 - 1, old.1, old.2 - 1);
                    assert!(!wanted.contains_key(&stray) && !world.contains_key(&stray));
                    world.insert(stray, id("sponge"));
                    p.block_update(stray, id("sponge"));
                    injected += 1;
                }
            }
            p.ack(seq);
        } else if packet == ids().sb_play_player_action {
            let (at, seq) = mock::read_dig(&mut body);
            world.remove(&at);
            p.block_update(at, 0);
            p.ack(seq);
        }
    }
    (world, injected)
}

fn wanted_states(plan: &Plan) -> HashMap<Cell, i32> {
    plan.steps
        .iter()
        .filter_map(|s| match s {
            Step::Place { pos, state, .. } => Some(((pos.x, pos.y, pos.z), id(state))),
            _ => None,
        })
        .collect()
}

#[test]
fn wrong_merged_changed_and_stray_blocks_are_noticed_and_put_right() {
    let schem = Schematic::parse(
        "palette S stone\npalette L sea_lantern\npalette Q smooth_quartz_slab[type=bottom]\n\
         layer 0\nSSSSS\nS...S\nSSSSS\nlayer 1\nQLQLQ\nQ...Q\nQLQLQ\n",
    )
    .unwrap();
    let plan = slice::slice(&schem, (2, 65, 2), 767).unwrap();
    assert!(plan.header.unreachable.is_empty());
    let wanted = wanted_states(&plan);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let spawn = plan.header.start;
    let faults = Faults { wrong: 4, merge: 15, neighbour: 18 };
    let for_server = wanted.clone();
    let server = thread::spawn(move || serve(listener, spawn, for_server, faults));

    let (mut bot, _) = PlayClient::join_v(&format!("127.0.0.1:{port}"), "127.0.0.1", port, "Mock", 767).unwrap();
    bot.place_delay = std::time::Duration::from_millis(5);
    // no final --verify audit: the checks during the run must be enough
    let opts = Opts { verify: false, from_step: 0, offset: None, extras_file: None, refit: Vec::new() };
    let mut log = Vec::new();
    let report = exec::run(&mut bot, &plan, &opts, &mut |m| log.push(m.to_string()), &mut |_, _| {}).unwrap();
    drop(bot);
    let (world, injected) = server.join().unwrap();

    assert_eq!(injected, 3, "every fault happened: {log:#?}");
    assert_eq!(report.aborted, None, "{log:#?}");
    assert_eq!(report.steps_done, plan.steps.len());
    for (cell, state) in &wanted {
        assert_eq!(world.get(cell), Some(state), "{cell:?}: {log:#?}");
    }
    assert_eq!(world.len(), wanted.len(), "nothing extra is left: {world:?}\n{log:#?}");
    assert!(report.differs.is_empty() && report.missing.is_empty(), "{:?} {:?}", report.differs, report.missing);
    for part in ["is smooth_quartz_slab", "should be air", "missing"] {
        assert!(log.iter().any(|l| l.contains(part)), "no repair line mentions {part}: {log:#?}");
    }
}
