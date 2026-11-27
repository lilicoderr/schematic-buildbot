//! The executor against a mock server that speaks just enough protocol 767: it confirms
//! placements with block updates and silently drops one, which the checkpoint repair must fix.

use mc_bot::builder::PlayClient;
use mc_bot::exec::{self, Opts};
use mc_bot::schematic::Schematic;
use mc_bot::{ids, slice};
use mc_proto::buf::PacketBuf;
use mc_proto::frame::{try_parse_frame, write_frame};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

struct Peer {
    stream: TcpStream,
    inbox: Vec<u8>,
}

impl Peer {
    /// Next packet from the bot, or `None` once it disconnects.
    fn recv(&mut self) -> Option<(i32, PacketBuf)> {
        let mut chunk = [0u8; 4096];
        loop {
            if let Some((used, id, body)) = try_parse_frame(&self.inbox, None).unwrap() {
                self.inbox.drain(..used);
                return Some((id, body));
            }
            match self.stream.read(&mut chunk) {
                Ok(0) | Err(_) => return None,
                Ok(n) => self.inbox.extend_from_slice(&chunk[..n]),
            }
        }
    }

    fn send(&mut self, id: i32, body: &PacketBuf) {
        let _ = write_frame(&mut self.stream, id, body.as_slice(), None);
        let _ = self.stream.flush();
    }

    fn block_update(&mut self, at: (i32, i32, i32), state: i32) {
        let mut b = PacketBuf::new();
        b.write_position(at.0, at.1, at.2);
        b.write_varint(state);
        self.send(ids::CB_PLAY_BLOCK_CHANGE, &b);
    }
}

fn serve(listener: TcpListener, spawn: (i32, i32, i32), drop_nth: usize, bounce_nth_move: usize) {
    let (stream, _) = listener.accept().unwrap();
    let mut p = Peer { stream, inbox: Vec::new() };
    p.recv().expect("handshake");
    p.recv().expect("login start");
    let mut ok = PacketBuf::new();
    ok.write_uuid(1);
    ok.write_string("Mock");
    ok.write_varint(0);
    p.send(ids::CB_LOGIN_SUCCESS, &ok);
    p.recv().expect("login ack");
    p.recv().expect("client information");
    p.send(ids::CB_CONFIG_FINISH, &PacketBuf::new());
    p.recv().expect("finish ack");
    let mut tp = PacketBuf::new();
    for v in [spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5] {
        tp.write_f64(v);
    }
    tp.write_f32(0.0);
    tp.write_f32(0.0);
    tp.write_u8(0);
    tp.write_varint(1);
    p.send(ids::CB_PLAY_SYNC_POS, &tp);
    let mut moves = 0;
    let mut placements = 0;
    while let Some((id, mut body)) = p.recv() {
        if id == ids::SB_PLAY_SET_POS {
            moves += 1;
            if moves == bounce_nth_move {
                // "moved too quickly": send the bot back to the spawn with a new teleport id
                let mut back = PacketBuf::new();
                for v in [spawn.0 as f64 + 0.5, spawn.1 as f64, spawn.2 as f64 + 0.5] {
                    back.write_f64(v);
                }
                back.write_f32(0.0);
                back.write_f32(0.0);
                back.write_u8(0);
                back.write_varint(2);
                p.send(ids::CB_PLAY_SYNC_POS, &back);
            }
        } else if id == ids::SB_PLAY_USE_ITEM_ON {
            let _hand = body.read_varint().unwrap();
            let at = body.read_position().unwrap();
            let d = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)][body.read_varint().unwrap() as usize];
            placements += 1;
            if placements != drop_nth {
                p.block_update((at.0 + d.0, at.1 + d.1, at.2 + d.2), 1);
            }
        } else if id == ids::SB_PLAY_BLOCK_DIG {
            let _status = body.read_varint().unwrap();
            let at = body.read_position().unwrap();
            p.block_update(at, 0);
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
    let opts = Opts { verify: true, from_step: 0, offset: None };
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
