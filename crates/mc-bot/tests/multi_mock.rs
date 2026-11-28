//! The cooperative runner against a mock server: several connections, barriers, a stop when one
//! bot's connection dies, and a resume from the per-bot progress files.

use mc_bot::multi::{partition, runner, MultiPlan};
use mc_bot::schematic::Schematic;
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;

mod mock;
use mock::{ids, Peer};

const SPAWN: (i32, i32, i32) = (0, 65, 0);

/// One bot's connection: confirms placements and digs, bounces a bot that sinks below the ground,
/// and hangs up at its `die_after`-th click.
fn serve(listener: TcpListener, die_after: Vec<Option<usize>>) {
    let mut conns = Vec::new();
    for die in die_after {
        let mut p = Peer::accept(listener.try_clone().unwrap(), SPAWN);
        conns.push(thread::spawn(move || {
            let (mut moves, mut clicks) = (0, 0usize);
            while let Some((id, mut body)) = p.recv() {
                if id == ids().sb_play_move_player_pos || id == ids().sb_play_move_player_pos_rot {
                    moves += 1usize;
                    let (_, y) = (body.read_f64().unwrap(), body.read_f64().unwrap());
                    if y < f64::from(SPAWN.1) - 0.01 {
                        p.teleport(SPAWN, moves as i32);
                    }
                } else if id == ids().sb_play_use_item_on {
                    let (at, face, seq) = mock::read_use_item_on(&mut body);
                    let d = mock::FACES[face];
                    clicks += 1;
                    if die.is_some_and(|n| clicks > n) {
                        return;
                    }
                    p.block_update((at.0 + d.0, at.1 + d.1, at.2 + d.2), 1);
                    p.ack(seq);
                } else if id == ids().sb_play_player_action {
                    let (at, seq) = mock::read_dig(&mut body);
                    p.block_update(at, 0);
                    p.ack(seq);
                }
            }
        }));
    }
    conns.into_iter().for_each(|c| c.join().unwrap());
}

fn plan(bots: usize) -> (Schematic, MultiPlan) {
    let row = "S".repeat(14);
    let schem = Schematic::parse(&format!("palette S stone\nlayer 0\n{row}\nlayer 1\n{row}\n")).unwrap();
    let multi = partition::partition(&schem, (2, 65, 2), 767, bots, &mut |_| {}).unwrap();
    (schem, multi)
}

fn config(port: u16, dir: PathBuf, bots: usize) -> runner::Config {
    runner::Config {
        server: format!("127.0.0.1:{port}"),
        names: (1..=bots).map(|i| format!("Mock{i}")).collect(),
        version: mc_bot::version::active(),
        delay: std::time::Duration::from_millis(1),
        walk: false,
        wait_creative: std::time::Duration::ZERO,
        state_dir: dir,
        origin: None,
    }
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("bb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn run_with(multi: &MultiPlan, dir: &std::path::Path, die: Vec<Option<usize>>) -> runner::Outcome {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bots = die.len();
    let server = thread::spawn(move || serve(listener, die));
    let out = runner::run(multi, &config(port, dir.to_path_buf(), bots)).unwrap();
    server.join().unwrap();
    out
}

#[test]
fn two_bots_build_their_strips_and_pass_the_barriers() {
    let (schem, multi) = plan(2);
    let d = dir("two");
    let out = run_with(&multi, &d, vec![None, None]);
    let _ = std::fs::remove_dir_all(&d);
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert!(out.clean(), "{:?}", out.reports);
    let placed: usize = out.reports.iter().map(|r| r.2.placed).sum();
    assert_eq!(placed, schem.blocks.len());
    assert!(out.reports.iter().all(|r| r.2.placed > 0), "both bots worked");
}

#[test]
fn one_dead_connection_stops_the_others_and_the_rerun_finishes_exactly_the_rest() {
    let (schem, multi) = plan(2);
    let d = dir("resume");
    let first = run_with(&multi, &d, vec![None, Some(9)]);
    assert!(first.errors.len() == 1, "exactly one bot failed (the one whose connection died): {:?} {:?}", first.errors, first.reports.iter().map(|r| (r.1, r.2.steps_done, r.2.placed, r.2.kicked.clone(), r.2.aborted.clone())).collect::<Vec<_>>());
    assert!(first.reports.iter().any(|r| r.2.kicked.as_deref() == Some(mc_bot::builder::STOPPED)), "the other bot was stopped");
    let second = run_with(&multi, &d, vec![None, None]);
    assert!(second.errors.is_empty(), "{:?}", second.errors);
    let placed_second: usize = second.reports.iter().map(|r| r.2.placed).sum();
    assert!(placed_second > 0 && placed_second < schem.blocks.len(), "the rerun only did the rest: {placed_second} of {}", schem.blocks.len());
    for part in multi.phases.iter().flatten() {
        let file = d.join(format!("phase1-bot{}.progress", part.bot));
        let done = mc_bot::run_state::load(&file, mc_bot::run_state::fingerprint(&part.plan)).map(|s| s.step);
        assert_eq!(done, Some(part.plan.steps.len()), "bot {} finished its plan", part.bot);
    }
    let _ = std::fs::remove_dir_all(&d);
}
