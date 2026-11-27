//! Play-state client and building logic.
//!
//! Core design (ground only: no flying, no hovering, no speeding):
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
const TICK: Duration = Duration::from_millis(50);

/// One walking step from `feet` toward `target`: horizontal only, at most [`WALK_STEP`], and
/// never any change in height, so a walking bot always stays on the surface it stands on.
pub fn walk_step(feet: Vec3, target: Vec3) -> Vec3 {
    let (dx, dz) = (target.x - feet.x, target.z - feet.z);
    let d = dx.hypot(dz);
    if d < 1e-9 {
        return feet;
    }
    let s = d.min(WALK_STEP);
    Vec3::new(feet.x + dx / d * s, feet.y, feet.z + dz / d * s)
}

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
                        self.send_pos_look(true)?;
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
            self.feet = walk_step(self.feet, target);
            self.send_pos(true)?;
            self.tick()?;
            if self.kicked.is_some() {
                break;
            }
        }
        Ok(())
    }

    /// Settle onto the ground below. A bot that joins in mid-air (the server restores the last saved
    /// position, e.g. after an interrupted build) would otherwise take that height as the ground level
    /// and build the whole structure floating. We lower ourselves in small steps until the server
    /// bounces us off the floor with a teleport, which leaves `feet` on the surface.
    /// Returns the number of blocks descended.
    pub fn land(&mut self) -> io::Result<f64> {
        const STEP: f64 = 0.25;
        let start_y = self.feet.y;
        let teleports_at_start = self.teleports;
        for _ in 0..1600 {
            if self.teleports != teleports_at_start || self.kicked.is_some() {
                break;
            }
            self.feet.y -= STEP;
            self.send_pos(false)?;
            self.tick()?;
        }
        self.send_pos(true)?;
        self.idle(Duration::from_millis(100))?;
        Ok(start_y - self.feet.y)
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
        self.send_pos_look(true)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The block under the player's feet is solid (flat ground: every cell below `ground_y`).
    fn supported(p: Vec3, ground_y: i32) -> bool {
        let solid = |_x: i32, y: i32, _z: i32| y < ground_y;
        solid(p.x.floor() as i32, p.y.floor() as i32 - 1, p.z.floor() as i32)
    }

    #[test]
    fn supported_detects_air_standing() {
        assert!(supported(Vec3::new(0.5, -60.0, 0.5), -60));
        assert!(!supported(Vec3::new(0.5, -58.0, 0.5), -60));
    }

    #[test]
    fn every_walking_position_stands_on_solid_ground() {
        let ground_y = -60;
        let route = [(0.5, 0.5), (12.5, 0.5), (12.5, -9.5), (-7.5, 4.5)];
        let mut feet = Vec3::new(route[0].0, ground_y as f64, route[0].1);
        for &(tx, tz) in &route[1..] {
            let target = Vec3::new(tx, ground_y as f64, tz);
            let mut guard = 0;
            while feet.horiz_dist(target) > 0.05 && guard < 1000 {
                let next = walk_step(feet, target);
                assert!(feet.horiz_dist(next) <= WALK_STEP + 1e-9, "no speeding");
                assert_eq!(next.y, feet.y, "walking never changes height");
                assert!(supported(next, ground_y), "no air-standing at {next:?}");
                feet = next;
                guard += 1;
            }
            assert!(guard < 1000, "walk_step must converge");
        }
    }

    #[test]
    fn walk_step_is_idempotent_at_the_target() {
        let t = Vec3::new(3.5, -60.0, 3.5);
        assert_eq!(walk_step(t, t), t);
    }
}
