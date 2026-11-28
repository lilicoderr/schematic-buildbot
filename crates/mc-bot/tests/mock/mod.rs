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
        let mut p = Peer::in_play(&listener);
        p.teleport(spawn, 1);
        p
    }

    /// A raw connection, nothing read yet.
    pub fn raw(listener: &TcpListener) -> Peer {
        let (stream, _) = listener.accept().unwrap();
        Peer { stream, inbox: Vec::new() }
    }

    /// Accept a login and go through configuration; the caller sends what Play starts with.
    pub fn in_play(listener: &TcpListener) -> Peer {
        let mut p = Peer::raw(listener);
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

#[allow(dead_code)]
impl Peer {
    /// Send the chunk (cx, cz) of a world whose ground (stone) reaches up to and including
    /// `top_y`, with `extra` state ids at world cells. 24 sections from y = -64, 1.21.1 layout.
    pub fn chunk(&mut self, (cx, cz): (i32, i32), top_y: i32, extra: &[((i32, i32, i32), u32)]) {
        let mut data = PacketBuf::new();
        for s in 0..24 {
            let cell = |i: usize| {
                let (x, y, z) = (cx * 16 + (i & 15) as i32, -64 + s * 16 + (i >> 8) as i32, cz * 16 + ((i >> 4) & 15) as i32);
                extra.iter().find(|e| e.0 == (x, y, z)).map_or(u32::from(y <= top_y), |e| e.1)
            };
            let values: Vec<u32> = (0..4096).map(cell).collect();
            let mut palette = values.clone();
            palette.sort_unstable();
            palette.dedup();
            data.write_i16(0);
            data.write_u8(if palette.len() == 1 { 0 } else { 4 });
            if palette.len() == 1 {
                data.write_varint(palette[0] as i32);
                data.write_varint(0);
            } else {
                data.write_varint(palette.len() as i32);
                for &p in &palette {
                    data.write_varint(p as i32);
                }
                data.write_varint(256);
                for w in values.chunks(16) {
                    let word = w.iter().enumerate().fold(0u64, |a, (k, v)| a | (palette.binary_search(v).unwrap() as u64) << (k * 4));
                    data.write_u64(word);
                }
            }
            data.write_u8(0); // biomes: single value 0
            data.write_varint(0);
            data.write_varint(0);
        }
        let mut b = PacketBuf::new();
        b.write_i32(cx);
        b.write_i32(cz);
        b.write_bytes(&[10, 0]); // empty heightmap compound
        b.write_varint(data.as_slice().len() as i32);
        b.write_bytes(data.as_slice());
        b.write_varint(0);
        self.send(ids().cb_play_level_chunk_with_light, &b);
    }

    /// The chunks of a 5x5 square around the spawn.
    pub fn flat_world(&mut self, top_y: i32, extra: &[((i32, i32, i32), u32)]) {
        for cx in -1..=2 {
            for cz in -1..=2 {
                self.chunk((cx, cz), top_y, extra);
            }
        }
    }
}
