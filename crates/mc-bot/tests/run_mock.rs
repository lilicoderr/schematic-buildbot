//! The executor against a mock server that speaks just enough protocol 767: it confirms
//! placements with block updates and silently drops one, which the repair must fix.

use mc_bot::builder::PlayClient;
use mc_bot::exec::{self, Opts};
use mc_bot::schematic::Schematic;
use mc_bot::slice;
use std::net::TcpListener;
use std::thread;

mod mock;
use mock::{ids, Peer};

fn serve(listener: TcpListener, spawn: (i32, i32, i32), drop_nth: usize, bounce_nth_move: usize) {
    let mut p = Peer::accept(listener, spawn);
    let mut moves = 0;
    let mut placements = 0;
    while let Some((id, mut body)) = p.recv() {
        if id == ids().sb_play_move_player_pos {
            moves += 1;
            if moves == bounce_nth_move {
                // "moved too quickly": send the bot back to the spawn with a new teleport id
                p.teleport(spawn, 2);
            }
        } else if id == ids().sb_play_use_item_on {
            let (at, face, seq) = mock::read_use_item_on(&mut body);
            let d = mock::FACES[face];
            placements += 1;
            if placements != drop_nth {
                p.block_update((at.0 + d.0, at.1 + d.1, at.2 + d.2), 1);
            }
            p.ack(seq);
        } else if id == ids().sb_play_player_action {
            let (at, seq) = mock::read_dig(&mut body);
            p.block_update(at, 0);
            p.ack(seq);
        }
    }
}

#[test]
fn replays_a_plan_and_repairs_a_placement_the_server_dropped() {
    let schem = Schematic::parse("palette S stone\nlayer 0\nSSS\nS.S\nSSS\nlayer 1\nSSS\nS.S\nSSS\n").unwrap();
    let plan = slice::slice(&schem, (2, 65, 2), 767).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let spawn = plan.header.start;
    let server = thread::spawn(move || serve(listener, spawn, 5, 3));

    let (mut bot, _) = PlayClient::join_v(&format!("127.0.0.1:{port}"), "127.0.0.1", port, "Mock", 767).unwrap();
    bot.place_delay = std::time::Duration::from_millis(5);
    let opts = Opts { verify: true, from_step: 0, offset: None, extras_file: None, refit: Vec::new(), keep: Default::default(), demolition: None, strip: None };
    let mut log = Vec::new();
    let mut steps_seen = 0;
    let report = exec::run(&mut bot, &plan, &opts, &mut |m| log.push(m.to_string()), &mut |n, _| steps_seen = n).unwrap();
    drop(bot);
    server.join().unwrap();

    assert_eq!(report.aborted, None, "{log:?}");
    assert_eq!(report.steps_done, plan.steps.len());
    assert_eq!(steps_seen, plan.steps.len());
    assert!(report.repaired >= 1, "the dropped block was placed again: {log:?}");
    assert!(report.missing.is_empty(), "missing {:?}", report.missing);
    // the mock confirms every block with state id 1, which is stone
    assert!(report.differs.is_empty(), "differs {:?}", report.differs);
    assert_eq!(report.resyncs, 1, "the rejected move was noticed: {log:?}");
}
