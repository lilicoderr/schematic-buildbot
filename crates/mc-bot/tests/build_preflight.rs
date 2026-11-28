//! The `build-bot build` binary against mock servers: every preflight failure must name what to
//! do, and one good run must build, verify and remember that it finished.

use mc_proto::buf::PacketBuf;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::thread;

#[allow(dead_code)]
mod mock;
use mock::{ids, Peer};

const WALLS: &str = "palette S stone\nlayer 0\nSSS\nS.S\nSSS\nlayer 1\nSSS\nS.S\nSSS\n";
const SPAWN: (i32, i32, i32) = (10, 65, 10);

struct Dir(PathBuf);

impl Dir {
    fn new(test: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("bb-build-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("walls.txt"), WALLS).unwrap();
        Dir(d)
    }

    /// Run `build-bot build walls.txt <addr> extra...`: (success, stdout + stderr).
    fn build(&self, addr: &str, extra: &[&str]) -> (bool, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_build-bot"))
            .current_dir(&self.0)
            .args(["build", "walls.txt", addr, "--lang", "en"])
            .args(extra)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
        (out.status.success(), text)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn listen() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    (l, addr)
}

fn answer_status(l: &TcpListener, protocol: i32) {
    let mut p = Peer::raw(l);
    p.recv().expect("handshake");
    p.recv().expect("status request");
    let mut b = PacketBuf::new();
    b.write_string(&format!("{{\"version\":{{\"name\":\"Mock {protocol}\",\"protocol\":{protocol}}}}}"));
    p.send(ids().cb_status_status_response, &b);
}

/// Play Login packet of protocol 767 up to the game mode.
fn play_login(p: &mut Peer, mode: u8) {
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

fn game_mode_event(p: &mut Peer, mode: u8) {
    let mut b = PacketBuf::new();
    b.write_u8(3);
    b.write_f32(mode as f32);
    p.send(ids().cb_play_game_event, &b);
}

/// A building server: confirms clicks with stone, bounces the landing probe off the floor and,
/// when `kick_after` placements are done, kicks.
fn serve_play(mut p: Peer, kick_after: usize) -> usize {
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
            p.block_update((at.0 + d.0, at.1 + d.1, at.2 + d.2), 1);
            p.ack(seq);
            placed += 1;
            if placed == kick_after {
                let mut reason = PacketBuf::new();
                reason.write_u8(0x08); // NBT string component
                reason.write_bytes(&[0, 17]);
                reason.write_bytes(b"Kicked by a human");
                p.send(ids().cb_play_disconnect, &reason);
            }
        } else if id == ids().sb_play_player_action {
            let (at, seq) = mock::read_dig(&mut body);
            p.block_update(at, 0);
            p.ack(seq);
        }
    }
    placed
}

#[test]
fn cannot_connect_says_what_to_check() {
    let (l, addr) = listen();
    drop(l);
    let (ok, out) = Dir::new("noconnect").build(&addr, &[]);
    assert!(!ok);
    assert!(out.contains("cannot connect") && out.contains("server-port"), "{out}");
}

#[test]
fn unsupported_version_lists_the_supported_ones() {
    let (l, addr) = listen();
    let server = thread::spawn(move || answer_status(&l, 999));
    let (ok, out) = Dir::new("version").build(&addr, &[]);
    server.join().unwrap();
    assert!(!ok);
    assert!(out.contains("protocol 999") && out.contains("1.20.4, 1.21, 1.21.1") && out.contains("ViaProxy"), "{out}");
}

#[test]
fn online_mode_is_explained() {
    let (l, addr) = listen();
    let server = thread::spawn(move || {
        answer_status(&l, 767);
        let mut p = Peer::raw(&l);
        p.recv().expect("handshake");
        p.recv().expect("login start");
        p.send(ids().cb_login_hello, &PacketBuf::new()); // encryption request
        while p.recv().is_some() {}
    });
    let (ok, out) = Dir::new("online").build(&addr, &[]);
    server.join().unwrap();
    assert!(!ok);
    assert!(out.contains("online mode") && out.contains("offline-mode servers only"), "{out}");
}

#[test]
fn a_refused_login_shows_the_reason_and_the_fix() {
    let (l, addr) = listen();
    let server = thread::spawn(move || {
        answer_status(&l, 767);
        let mut p = Peer::raw(&l);
        p.recv().expect("handshake");
        p.recv().expect("login start");
        let mut b = PacketBuf::new();
        b.write_string("{\"translate\":\"multiplayer.disconnect.not_whitelisted\"}");
        p.send(ids().cb_login_login_disconnect, &b);
        while p.recv().is_some() {}
    });
    let (ok, out) = Dir::new("refused").build(&addr, &["--name", "Helper"]);
    server.join().unwrap();
    assert!(!ok);
    assert!(out.contains("not_whitelisted") && out.contains("whitelist add Helper"), "{out}");
}

#[test]
fn survival_mode_names_the_command_and_places_nothing() {
    let (l, addr) = listen();
    let server = thread::spawn(move || {
        answer_status(&l, 767);
        let mut p = Peer::in_play(&l);
        play_login(&mut p, 0);
        p.teleport(SPAWN, 1);
        serve_play(p, 0)
    });
    let (ok, out) = Dir::new("survival").build(&addr, &["--wait-creative", "0"]);
    assert_eq!(server.join().unwrap(), 0);
    assert!(!ok);
    assert!(out.contains("survival mode") && out.contains("gamemode creative BuildBot"), "{out}");
}

#[test]
fn builds_after_the_switch_to_creative_resumes_after_a_kick_and_remembers_the_end() {
    let dir = Dir::new("good");
    let (l, addr) = listen();
    let (l2, l3) = (l.try_clone().unwrap(), l.try_clone().unwrap());
    let server = thread::spawn(move || {
        answer_status(&l, 767);
        let mut p = Peer::in_play(&l);
        play_login(&mut p, 0);
        p.teleport(SPAWN, 1);
        game_mode_event(&mut p, 1); // the operator runs the command while the bot waits
        serve_play(p, 5)
    });
    let (ok, out) = dir.build(&addr, &["--wait-creative", "20", "--delay-ms", "2"]);
    server.join().unwrap();
    assert!(!ok);
    assert!(out.contains("creative mode, continuing"), "{out}");
    assert!(out.contains("Kicked by a human") && out.contains("resumes"), "{out}");

    let server = thread::spawn(move || {
        answer_status(&l2, 767);
        let mut p = Peer::in_play(&l2);
        play_login(&mut p, 1);
        p.teleport(SPAWN, 1);
        serve_play(p, 0)
    });
    let (ok, out) = dir.build(&addr, &["--delay-ms", "2"]);
    let placed = server.join().unwrap();
    assert!(out.contains("resuming after step"), "{out}");
    assert!(ok && out.contains("done: verified"), "{out}");
    assert!(placed < 16, "a resume does not start over: {placed}");

    // a third run needs no building; the status ping is all it asks the server
    let server = thread::spawn(move || answer_status(&l3, 767));
    let (ok, out) = dir.build(&addr, &[]);
    server.join().unwrap();
    assert!(ok && out.contains("already finished"), "{out}");
}

#[test]
fn dry_run_never_connects() {
    let (l, addr) = listen();
    l.set_nonblocking(true).unwrap();
    let dir = Dir::new("dry");
    let (ok, out) = dir.build(&addr, &["--dry-run"]);
    assert!(ok && out.contains("estimated time") && out.contains("unsupported blocks: 0"), "{out}");
    assert!(l.accept().is_err(), "nothing connected");
    assert!(!dir.0.join(".buildbot").exists(), "a dry run writes nothing");
}
