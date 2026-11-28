//! A mock server speaking just enough protocol 767 for the plan executor.

use mc_bot::data::Packets;
use mc_bot::version;
use mc_proto::buf::PacketBuf;
use mc_proto::frame::{try_parse_frame, write_frame};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

/// Packet ids of protocol 767.
pub fn ids() -> &'static Packets {
    version::DEFAULT.packets()
}

pub struct Peer {
    stream: TcpStream,
    inbox: Vec<u8>,
}

impl Peer {
    /// Accept the bot, log it in and teleport it onto the cell `spawn`.
    pub fn accept(listener: TcpListener, spawn: (i32, i32, i32)) -> Peer {
        let (stream, _) = listener.accept().unwrap();
        let mut p = Peer { stream, inbox: Vec::new() };
        p.recv().expect("handshake");
        p.recv().expect("login start");
        let mut ok = PacketBuf::new();
        ok.write_uuid(1);
        ok.write_string("Mock");
        ok.write_varint(0);
        p.send(ids().cb_login_login_finished, &ok);
        p.recv().expect("login ack");
        p.recv().expect("client information");
        p.send(ids().cb_config_finish_configuration, &PacketBuf::new());
        p.recv().expect("finish ack");
        p.teleport(spawn, 1);
        p
    }

    pub fn teleport(&mut self, to: (i32, i32, i32), id: i32) {
        let mut tp = PacketBuf::new();
        for v in [to.0 as f64 + 0.5, to.1 as f64, to.2 as f64 + 0.5] {
            tp.write_f64(v);
        }
        tp.write_f32(0.0);
        tp.write_f32(0.0);
        tp.write_u8(0);
        tp.write_varint(id);
        self.send(ids().cb_play_player_position, &tp);
    }

    /// Next packet from the bot, or `None` once it disconnects.
    pub fn recv(&mut self) -> Option<(i32, PacketBuf)> {
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

    pub fn send(&mut self, id: i32, body: &PacketBuf) {
        let _ = write_frame(&mut self.stream, id, body.as_slice(), None);
        let _ = self.stream.flush();
    }

    pub fn block_update(&mut self, at: (i32, i32, i32), state: i32) {
        let mut b = PacketBuf::new();
        b.write_position(at.0, at.1, at.2);
        b.write_varint(state);
        self.send(ids().cb_play_block_update, &b);
    }

    /// Block Changed Ack: sent after the updates the action caused, like vanilla.
    pub fn ack(&mut self, sequence: i32) {
        let mut b = PacketBuf::new();
        b.write_varint(sequence);
        self.send(ids().cb_play_block_changed_ack, &b);
    }
}

pub const FACES: [(i32, i32, i32); 6] = [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)];

/// Use Item On: (clicked block, face index, sequence).
pub fn read_use_item_on(body: &mut PacketBuf) -> ((i32, i32, i32), usize, i32) {
    let _hand = body.read_varint().unwrap();
    let at = body.read_position().unwrap();
    let face = body.read_varint().unwrap() as usize;
    for _ in 0..3 {
        body.read_f32().unwrap();
    }
    let _inside = body.read_u8().unwrap();
    (at, face, body.read_varint().unwrap())
}

/// Player Action (digging): (block, sequence).
pub fn read_dig(body: &mut PacketBuf) -> ((i32, i32, i32), i32) {
    let _status = body.read_varint().unwrap();
    let at = body.read_position().unwrap();
    let _face = body.read_u8().unwrap();
    (at, body.read_varint().unwrap())
}
