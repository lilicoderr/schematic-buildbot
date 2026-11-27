//! Play-state client and building logic.
//!
//! Core design (avoids fly/speed kicks):
//!  · Always stand on the ground (`on_ground = true`) and move in small steps below walking speed.
//!  · Answer keepalive and teleport sync immediately.
//!  · Blocks are placed by clicking a face of a supported neighbour (Use Item On);
//!    the click face/cursor/yaw come from [`crate::placement`].
//!  · Only place when the eye-to-block distance is within reach, otherwise walk closer first.

use crate::blocklog::{self, BlockLog};
use crate::ids;
use crate::net::Connection;
use crate::placement::Plan;
use crate::proto::{self, JoinInfo};
use crate::tr;
use crate::world::{look_at, within_reach, BlockPos, Face, Vec3, EYE_HEIGHT};
use mc_proto::buf::PacketBuf;
use std::io;
use std::time::{Duration, Instant};

/// Ground speed per 20 Hz tick: 4.2 blocks/s, just under vanilla walking speed (4.317).
const WALK_STEP: f64 = 0.21;
/// Flight speed per tick: 9 blocks/s, under the creative flying speed (10.9).
const FLY_STEP: f64 = 0.45;
const TICK: Duration = Duration::from_millis(50);

pub struct PlayClient {
    pub conn: Connection,
    pub feet: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub seq: i32,
    held_item: i32,
    pub kicked: Option<String>,
    /// Pause after each placement; raise it on servers with strict packet-rate limits.
    pub place_delay: Duration,
    /// Last state the server reported per block position (see [`BlockLog`]).
    pub blocks: BlockLog,
    /// The server granted creative flight (abilities packet).
    pub may_fly: bool,
    pub flying: bool,
    /// Teleports forced by the server (a sign we are pushing into something).
    pub teleports: u32,
    next_tick: Instant,
}

impl PlayClient {
    pub fn join(addr: &str, host: &str, port: u16, name: &str) -> io::Result<(Self, JoinInfo)> {
        Self::join_v(addr, host, port, name, mc_proto::PROTOCOL_VERSION)
    }

    /// Join announcing an explicit protocol number (see [`crate::version`]).
    pub fn join_v(addr: &str, host: &str, port: u16, name: &str, protocol: i32) -> io::Result<(Self, JoinInfo)> {
        let (conn, join) = proto::login_offline_v(addr, host, port, name, protocol, |_, _, _| {})?;
        let mut client = PlayClient {
            conn,
            feet: Vec3::new(0.0, 0.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            seq: 0,
            held_item: -1,
            kicked: None,
            place_delay: Duration::from_millis(25),
            blocks: BlockLog::default(),
            may_fly: false,
            flying: false,
            teleports: 0,
            next_tick: Instant::now(),
        };
        client.conn.set_read_timeout(Some(Duration::from_millis(5)))?;
        // Pump packets until the first teleport sync fixes our position
        let deadline = Instant::now() + Duration::from_secs(10);
        while client.feet == Vec3::new(0.0, 0.0, 0.0) && Instant::now() < deadline {
            client.pump_once()?;
            if client.kicked.is_some() {
                break;
            }
        }
        Ok((client, join))
    }

    /// Handle one incoming packet (or time out): kicks, teleports and keepalives.
    pub fn pump_once(&mut self) -> io::Result<()> {
        match self.conn.recv() {
            Ok((id, mut body)) => {
                match id {
                    ids::CB_PLAY_KEEPALIVE => {
                        let k = body.read_i64().unwrap_or(0);
                        let mut r = PacketBuf::new();
                        r.write_i64(k);
                        self.conn.send(ids::SB_PLAY_KEEPALIVE, r.as_slice())?;
                    }
                    ids::CB_PLAY_PING => {
                        // transaction ping → pong (play) 0x27
                        let p = body.read_i32().unwrap_or(0);
                        let mut r = PacketBuf::new();
                        r.write_i32(p);
                        self.conn.send(0x27, r.as_slice())?;
                    }
                    ids::CB_PLAY_SYNC_POS => {
                        let x = body.read_f64().unwrap_or(self.feet.x);
                        let y = body.read_f64().unwrap_or(self.feet.y);
                        let z = body.read_f64().unwrap_or(self.feet.z);
                        let yaw = body.read_f32().unwrap_or(self.yaw);
                        let pitch = body.read_f32().unwrap_or(self.pitch);
                        let flags = body.read_u8().unwrap_or(0);
                        let tp = body.read_varint().unwrap_or(0);
                        // flag bits: 0x01 = X relative, ... (servers normally send absolute values)
                        if flags & 0x01 == 0 { self.feet.x = x; } else { self.feet.x += x; }
                        if flags & 0x02 == 0 { self.feet.y = y; } else { self.feet.y += y; }
                        if flags & 0x04 == 0 { self.feet.z = z; } else { self.feet.z += z; }
                        if flags & 0x08 == 0 { self.pitch = pitch; }
                        if flags & 0x10 == 0 { self.yaw = yaw; }
                        let mut r = PacketBuf::new();
                        r.write_varint(tp);
                        self.conn.send(ids::SB_PLAY_CONFIRM_TELEPORT, r.as_slice())?;
                        self.teleports += 1;
                        // After confirming, report the current position
                        self.send_pos_look(!self.flying)?;
                    }
                    ids::CB_PLAY_ABILITIES => {
                        let flags = body.read_i8().unwrap_or(0);
                        self.may_fly = flags & 0x04 != 0;
                    }
                    ids::CB_PLAY_BLOCK_CHANGE => {
                        if let Some((pos, state)) = blocklog::parse_block_change(&mut body) {
                            self.blocks.record(pos, state);
                        }
                    }
                    ids::CB_PLAY_MULTI_BLOCK_CHANGE => {
                        for (pos, state) in blocklog::parse_multi_block_change(&mut body).unwrap_or_default() {
                            self.blocks.record(pos, state);
                        }
                    }
                    ids::CB_PLAY_DISCONNECT => {
                        let reason = body.read_string().unwrap_or_default();
                        self.kicked = Some(reason);
                    }
                    ids::CB_PLAY_UPDATE_HEALTH => {
                        let health = body.read_f32().unwrap_or(20.0);
                        if health <= 0.0 {
                            self.kicked = Some(tr!("died", "사망"));
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Wait for `dur` while answering incoming packets (not a busy loop).
    pub fn idle(&mut self, dur: Duration) -> io::Result<()> {
        let end = Instant::now() + dur;
        while Instant::now() < end {
            self.pump_once()?;
            if self.kicked.is_some() {
                break;
            }
        }
        Ok(())
    }

    fn send_pos(&mut self, on_ground: bool) -> io::Result<()> {
        let mut b = PacketBuf::new();
        b.write_f64(self.feet.x);
        b.write_f64(self.feet.y);
        b.write_f64(self.feet.z);
        b.write_bool(on_ground);
        self.conn.send(ids::SB_PLAY_SET_POS, b.as_slice())
    }

    fn send_pos_look(&mut self, on_ground: bool) -> io::Result<()> {
        let mut b = PacketBuf::new();
        b.write_f64(self.feet.x);
        b.write_f64(self.feet.y);
        b.write_f64(self.feet.z);
        b.write_f32(self.yaw);
        b.write_f32(self.pitch);
        b.write_bool(on_ground);
        self.conn.send(ids::SB_PLAY_SET_POS_ROT, b.as_slice())
    }

    /// Wait for the next 20 Hz tick boundary while answering incoming packets.
    fn tick(&mut self) -> io::Result<()> {
        loop {
            self.pump_once()?;
            if self.kicked.is_some() || Instant::now() >= self.next_tick {
                break;
            }
        }
        let now = Instant::now();
        self.next_tick = if now > self.next_tick + TICK * 4 { now + TICK } else { self.next_tick + TICK };
        Ok(())
    }

    /// Walk along the ground to (tx, tz), keeping y (flat ground assumed), one fixed step per tick.
    pub fn walk_to(&mut self, tx: f64, tz: f64) -> io::Result<()> {
        let target = Vec3::new(tx, self.feet.y, tz);
        let mut guard = 0;
        let mut best = f64::MAX;
        let mut stalled = 0;
        while self.feet.horiz_dist(target) > 0.05 && guard < 2000 {
            guard += 1;
            // Give up quickly when the server keeps pushing us back (blocked by a wall).
            let d_now = self.feet.horiz_dist(target);
            if d_now < best - 0.01 {
                best = d_now;
                stalled = 0;
            } else {
                stalled += 1;
                if stalled > 25 {
                    break;
                }
            }
            let dx = target.x - self.feet.x;
            let dz = target.z - self.feet.z;
            let d = (dx * dx + dz * dz).sqrt();
            let s = d.min(WALK_STEP);
            self.feet.x += dx / d * s;
            self.feet.z += dz / d * s;
            self.send_pos(!self.flying)?;
            self.tick()?;
            if self.kicked.is_some() {
                break;
            }
        }
        Ok(())
    }

    /// Turn creative flight on or off. Returns `false` when the server did not grant flight.
    pub fn set_flying(&mut self, on: bool) -> io::Result<bool> {
        if on && !self.may_fly {
            return Ok(false);
        }
        let mut b = PacketBuf::new();
        b.write_i8(if on { 0x02 } else { 0 });
        self.conn.send(ids::SB_PLAY_ABILITIES, b.as_slice())?;
        self.flying = on;
        Ok(true)
    }

    /// Fly in a straight line to `target` (3D), one fixed step per tick. Gives up after the
    /// server teleports us back several times (something solid is in the way).
    pub fn fly_to(&mut self, target: Vec3) -> io::Result<()> {
        let teleports_at_start = self.teleports;
        let mut guard = 0;
        while self.feet.dist(target) > 0.05 && guard < 2000 && self.teleports - teleports_at_start < 3 {
            guard += 1;
            let d = self.feet.dist(target);
            let s = d.min(FLY_STEP) / d;
            self.feet.x += (target.x - self.feet.x) * s;
            self.feet.y += (target.y - self.feet.y) * s;
            self.feet.z += (target.z - self.feet.z) * s;
            self.send_pos(false)?;
            self.tick()?;
            if self.kicked.is_some() {
                break;
            }
        }
        Ok(())
    }

    /// Walk along a precomputed route of ground cells (see [`crate::path::find_path`]).
    pub fn walk_route(&mut self, route: &[(i32, i32)]) -> io::Result<()> {
        for &(cx, cz) in route.iter().skip(1) {
            self.walk_to(cx as f64 + 0.5, cz as f64 + 0.5)?;
            if self.kicked.is_some() {
                break;
            }
        }
        Ok(())
    }

    fn set_creative_held(&mut self, item_id: i32) -> io::Result<()> {
        if self.held_item == item_id {
            return Ok(());
        }
        // Hotbar slot 0 = inventory slot 36
        let mut b = PacketBuf::new();
        b.write_i16(36);
        // Slot: count=1, itemId, add=0, remove=0
        b.write_varint(1);
        b.write_varint(item_id);
        b.write_varint(0);
        b.write_varint(0);
        self.conn.send(ids::SB_PLAY_SET_CREATIVE_SLOT, b.as_slice())?;
        // Select hotbar slot 0
        let mut h = PacketBuf::new();
        h.write_i16(0);
        self.conn.send(ids::SB_PLAY_HELD_ITEM, h.as_slice())?;
        self.held_item = item_id;
        Ok(())
    }

    /// Right-click the `face` of the `support` block, placing a block next to it.
    fn use_item_on(&mut self, support: BlockPos, face: Face, cursor: (f32, f32, f32)) -> io::Result<()> {
        self.seq += 1;
        let mut b = PacketBuf::new();
        b.write_varint(0); // hand = main
        b.write_position(support.x, support.y, support.z);
        b.write_varint(face as i32);
        b.write_f32(cursor.0);
        b.write_f32(cursor.1);
        b.write_f32(cursor.2);
        b.write_bool(false); // insideBlock
        b.write_varint(self.seq);
        self.conn.send(ids::SB_PLAY_USE_ITEM_ON, b.as_slice())?;
        // Swing arm (animation)
        let mut s = PacketBuf::new();
        s.write_varint(0);
        self.conn.send(ids::SB_PLAY_SWING_ARM, s.as_slice())
    }

    /// Try to place `item_id` at `pos` following `plan` (click faces, cursor, yaw).
    /// Returns `Ok(false)` (not an error) when the position is out of reach.
    pub fn place(&mut self, pos: BlockPos, item_id: i32, plan: &Plan) -> io::Result<bool> {
        if !within_reach(self.feet, pos) {
            return Ok(false);
        }
        // Look at the target, but keep the horizontal facing the block state needs.
        let eye = Vec3::new(self.feet.x, self.feet.y + EYE_HEIGHT, self.feet.z);
        let (look_yaw, pitch) = look_at(eye, pos.center());
        self.yaw = plan.yaw.unwrap_or(look_yaw);
        self.pitch = pitch;
        self.set_creative_held(item_id)?;
        self.send_pos_look(!self.flying)?;
        for click in &plan.clicks {
            self.use_item_on(click.support, click.face, click.cursor)?;
            self.idle(self.place_delay)?;
            if self.kicked.is_some() {
                break;
            }
        }
        Ok(true)
    }
}
