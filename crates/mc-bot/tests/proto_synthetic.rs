//! Synthetic packets against the play client (docs/RE/protocol.md, movement.md).

use mc_bot::builder::PlayClient;
use mc_bot::data::Packets;
use mc_bot::version;
use mc_proto::buf::PacketBuf;
use mc_proto::frame::{try_parse_frame, write_frame};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const PONG: i32 = 39;
/// serverbound player_abilities: the only way to tell the server we are flying.
const ABILITIES: i32 = 0x23;

/// Packet ids of protocol 767.
fn ids() -> &'static Packets {
    version::DEFAULT.packets()
}

fn read(stream: &mut TcpStream, inbox: &mut Vec<u8>) -> Option<(i32, PacketBuf)> {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some((used, id, body)) = try_parse_frame(inbox, None).unwrap() {
            inbox.drain(..used);
            return Some((id, body));
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => inbox.extend_from_slice(&chunk[..n]),
        }
    }
}

fn teleport(y: f64, id: i32) -> PacketBuf {
    let mut b = PacketBuf::new();
    for v in [0.5, y, 0.5] {
        b.write_f64(v);
    }
    b.write_f32(0.0);
    b.write_f32(0.0);
    b.write_u8(0);
    b.write_varint(id);
    b
}

type Received = Vec<(i32, Vec<u8>)>;

/// Logs a bot in, sends `script` as play packets and returns every play packet the bot sent.
fn server(script: Vec<(i32, PacketBuf)>) -> (u16, JoinHandle<Received>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut inbox = Vec::new();
        let send = |s: &mut TcpStream, id: i32, b: &PacketBuf| {
            write_frame(s, id, b.as_slice(), None).unwrap();
            s.flush().unwrap();
        };
        read(&mut s, &mut inbox).expect("handshake");
        read(&mut s, &mut inbox).expect("login start");
        let mut ok = PacketBuf::new();
        ok.write_uuid(1);
        ok.write_string("Mock");
        ok.write_varint(0);
        send(&mut s, ids().cb_login_login_finished, &ok);
        read(&mut s, &mut inbox).expect("login ack");
        read(&mut s, &mut inbox).expect("client information");
        send(&mut s, ids().cb_config_finish_configuration, &PacketBuf::new());
        read(&mut s, &mut inbox).expect("finish ack");
        for (id, b) in &script {
            send(&mut s, *id, b);
        }
        let mut got = Vec::new();
        while let Some((id, mut b)) = read(&mut s, &mut inbox) {
            got.push((id, b.read_rest()));
        }
        got
    });
    (port, handle)
}

fn join(port: u16) -> PlayClient {
    PlayClient::join_v(&format!("127.0.0.1:{port}"), "127.0.0.1", port, "Mock", 767).unwrap().0
}

#[test]
fn each_server_packet_gets_exactly_its_answer_and_nothing_else() {
    let mut keep = PacketBuf::new();
    keep.write_i64(0x1122_3344_5566);
    let mut ping = PacketBuf::new();
    ping.write_i32(77);
    let mut batch = PacketBuf::new();
    batch.write_varint(9);
    let (port, server) = server(vec![
        (ids().cb_play_player_position, teleport(64.0, 7)),
        (ids().cb_play_keep_alive, keep.clone()),
        (ids().cb_play_ping, ping.clone()),
        (ids().cb_play_chunk_batch_finished, batch),
        (ids().cb_play_player_position, teleport(65.0, 8)),
    ]);
    let mut bot = join(port);
    bot.idle(Duration::from_millis(400)).unwrap();
    drop(bot);
    let got = server.join().unwrap();
    let kinds: Vec<i32> = got.iter().map(|p| p.0).collect();
    // nothing (above all no movement) before the first confirm; each confirm once, then the position
    assert_eq!(
        kinds,
        [
            ids().sb_play_accept_teleportation,
            ids().sb_play_move_player_pos_rot,
            ids().sb_play_keep_alive,
            PONG,
            ids().sb_play_chunk_batch_received,
            ids().sb_play_accept_teleportation,
            ids().sb_play_move_player_pos_rot,
        ]
    );
    assert!(!kinds.contains(&ABILITIES));
    assert_eq!((got[0].1.as_slice(), got[5].1.as_slice()), (&[7u8][..], &[8u8][..]));
    assert_eq!(got[2].1, keep.as_slice());
    assert_eq!(got[3].1, ping.as_slice());
    // the confirmed spot is reported back exactly: x, y, z
    assert_eq!(got[6].1[8..16], 65.0f64.to_be_bytes());
}

#[test]
fn a_refused_pillar_block_is_never_stood_on() {
    let (port, server) = server(vec![(ids().cb_play_player_position, teleport(64.0, 1))]);
    let mut bot = join(port);
    bot.place_delay = Duration::from_millis(5);
    let err = bot.pillar_up((0, 64, 0), 1, 1).unwrap_err();
    assert!(err.to_string().contains("hovering"), "{err}");
    assert_eq!(bot.feet.y, 64.0);
    drop(bot);
    let got = server.join().unwrap();
    let last = got.iter().rev().find(|p| p.0 == ids().sb_play_move_player_pos).expect("a final position");
    assert_eq!(last.1[8..16], 64.0f64.to_be_bytes(), "ends on the floor");
}
