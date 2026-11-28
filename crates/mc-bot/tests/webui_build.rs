//! A whole build through `build-bot ui` against a mock server: progress, safe stop, resume, and
//! no build left running when the UI is closed.

use mc_proto::buf::PacketBuf;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[allow(dead_code)]
mod mock;
#[allow(dead_code)]
mod uiclient;
use mock::{ids, Peer};
use uiclient::{text, Ui};

const SPAWN: (i32, i32, i32) = (10, 65, 10);
const BLOCKS: usize = 60;

/// 6 x 6 walls, three high.
fn walls() -> Vec<u8> {
    let layer = "SSSSSS\nS....S\nS....S\nS....S\nS....S\nSSSSSS\n";
    format!("palette S stone\nlayer 0\n{layer}layer 1\n{layer}layer 2\n{layer}").into_bytes()
}

/// A status ping, then one session that confirms every click with stone, `delay` apiece.
/// Sends the number of placements once the bot disconnects.
fn server(delay: Duration, mode: u8) -> (String, mpsc::Receiver<usize>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut p = Peer::raw(&l);
        p.recv().expect("handshake");
        p.recv().expect("status request");
        let mut b = PacketBuf::new();
        b.write_string("{\"version\":{\"name\":\"Mock\",\"protocol\":767}}");
        p.send(ids().cb_status_status_response, &b);
        let mut p = Peer::in_play(&l);
        login(&mut p, mode);
        p.teleport(SPAWN, 1);
        let _ = tx.send(serve(p, delay));
    });
    (addr, rx)
}

/// Play Login of protocol 767 up to the game mode.
fn login(p: &mut Peer, mode: u8) {
    let mut b = PacketBuf::new();
    b.write_i32(7);
    b.write_u8(0);
    b.write_varint(1);
    b.write_string("minecraft:overworld");
    for v in [20, 8, 8] {
        b.write_varint(v);
    }
    for _ in 0..3 {
        b.write_u8(0);
    }
    b.write_varint(0);
    b.write_string("minecraft:overworld");
    b.write_i64(0);
    b.write_u8(mode);
    b.write_u8(0xff);
    p.send(ids().cb_play_login, &b);
}

fn serve(mut p: Peer, delay: Duration) -> usize {
    let (mut landed, mut placed) = (false, 0);
    while let Some((id, mut body)) = p.recv() {
        if id == ids().sb_play_move_player_pos {
            let (_, y) = (body.read_f64().unwrap(), body.read_f64().unwrap());
            if !landed && y < SPAWN.1 as f64 - 0.1 {
                landed = true;
                p.teleport(SPAWN, 2);
            }
        } else if id == ids().sb_play_use_item_on {
            let (at, face, seq) = mock::read_use_item_on(&mut body);
            let d = mock::FACES[face];
            thread::sleep(delay);
            p.block_update((at.0 + d.0, at.1 + d.1, at.2 + d.2), 1);
            p.ack(seq);
            placed += 1;
        } else if id == ids().sb_play_player_action {
            let (at, seq) = mock::read_dig(&mut body);
            p.block_update(at, 0);
            p.ack(seq);
        }
    }
    placed
}

fn upload(ui: &Ui) -> String {
    let (status, reply) = ui.api("POST", "/api/upload?name=walls.txt", &walls());
    assert_eq!(status, 200, "{reply:?}");
    text(&reply, "id").to_string()
}

fn start(ui: &Ui, id: &str, addr: &str) -> u16 {
    ui.api("POST", "/api/build", format!("{{\"id\":\"{id}\",\"server\":\"{addr}\",\"name\":\"UiBot\"}}").as_bytes()).0
}

fn number(j: &mc_bot::json::Json, key: &str) -> usize {
    text(j, key).parse().unwrap_or(0)
}

#[test]
fn build_stop_and_resume_through_the_page() {
    let ui = Ui::start("build");
    let id = upload(&ui);
    let (addr, first) = server(Duration::from_millis(100), 1);
    assert_eq!(start(&ui, &id, &addr), 200);
    let s = ui.wait("some blocks", |s| text(s, "phase") == "building" && number(s, "placed") >= 5);
    assert_eq!(number(&s, "places"), BLOCKS);
    assert_eq!(start(&ui, &id, &addr), 409, "one build at a time");
    assert_eq!(ui.api("POST", "/api/upload?name=walls.txt", &walls()).0, 409);

    let (status, reply) = ui.api("POST", "/api/stop", b"{}");
    assert_eq!((status, text(&reply, "stopping")), (200, "true"));
    let s = ui.wait("the stop", |s| text(s, "phase") == "stopped");
    let stopped_at = number(&s, "placed");
    let sent = first.recv_timeout(Duration::from_secs(10)).expect("the bot disconnected");
    assert!((5..BLOCKS).contains(&stopped_at) && sent < BLOCKS, "{stopped_at} {sent} {s:?}");
    assert!(format!("{s:?}").contains("stopped early"), "{s:?}");

    // the same server address: the saved job continues where it stopped
    let (status, reply) = ui.api("POST", "/api/stop", b"{}");
    assert_eq!((status, text(&reply, "stopping")), (200, "false"));
    let l = TcpListener::bind(&addr).expect("the port is free again");
    let second = resume_server(l);
    assert_eq!(start(&ui, &id, &addr), 200);
    let s = ui.wait("the end", |s| matches!(text(s, "phase"), "done" | "failed"));
    assert_eq!(text(&s, "phase"), "done", "{s:?}");
    assert_eq!((text(&s, "percent"), number(&s, "placed")), ("100.0", BLOCKS));
    let again = second.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(again <= BLOCKS - stopped_at + 2, "a resume does not start over: {again} after {stopped_at}");
}

fn resume_server(l: TcpListener) -> mpsc::Receiver<usize> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut p = Peer::raw(&l);
        p.recv().expect("handshake");
        p.recv().expect("status request");
        let mut b = PacketBuf::new();
        b.write_string("{\"version\":{\"name\":\"Mock\",\"protocol\":767}}");
        p.send(ids().cb_status_status_response, &b);
        let mut p = Peer::in_play(&l);
        login(&mut p, 1);
        p.teleport(SPAWN, 1);
        let _ = tx.send(serve(p, Duration::ZERO));
    });
    rx
}

#[test]
fn stop_before_the_first_step_ends_the_wait_at_once() {
    let ui = Ui::start("early");
    let id = upload(&ui);
    // survival mode: the build waits (up to 120 s) for the operator to switch it
    let (addr, session) = server(Duration::ZERO, 0);
    assert_eq!(start(&ui, &id, &addr), 200);
    ui.wait("the creative-mode hint", |s| format!("{s:?}").contains("gamemode creative UiBot"));
    assert_eq!(ui.api("POST", "/api/stop", b"{}").0, 200);
    let s = ui.wait("the stop", |s| text(s, "phase") == "stopped");
    assert_eq!(number(&s, "placed"), 0);
    assert_eq!(session.recv_timeout(Duration::from_secs(10)).unwrap(), 0);
}

#[test]
fn closing_the_ui_stops_its_build() {
    let mut ui = Ui::start("closed");
    let id = upload(&ui);
    let (addr, session) = server(Duration::from_millis(150), 1);
    assert_eq!(start(&ui, &id, &addr), 200);
    ui.wait("some blocks", |s| number(s, "placed") >= 3);
    ui.kill();
    // the whole build would take 9 s more
    let placed = session.recv_timeout(Duration::from_secs(5)).expect("no build left running");
    assert!(placed < BLOCKS / 2, "{placed}");
}
