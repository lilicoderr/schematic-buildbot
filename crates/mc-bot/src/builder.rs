//! Play-state client and building logic.
//!
//! Core design (ground only: no flying, no hovering, no speeding):
//!  · Always stand on the ground (`on_ground = true`) and move in small steps below walking speed.
//!  · Answer keepalive and teleport sync immediately.
//!  · Blocks are placed by clicking a face of a supported neighbour (Use Item On);
//!    the click face/cursor/yaw come from [`crate::placement`].
//!  · Only place when the eye-to-block distance is within reach, otherwise walk closer first.

use crate::blocklog::{self, BlockLog};
use crate::chunks::{self, ChunkStore};
use crate::codec::{self, PlayerAction};
use crate::gamemode;
use crate::motion::{self, JUMP_ARC, SPRINT_STEP, WALK_STEP};
use crate::nav::{cell_feet, Cell};
use crate::net::Connection;
use crate::placement::Plan;
use crate::proto::{self, JoinInfo};
use crate::tr;
use crate::version::{self, Version};
use crate::world::{look_at, within_reach, BlockPos, Face, Vec3, EYE_HEIGHT};
use mc_proto::buf::PacketBuf;
use std::io;
use std::time::{Duration, Instant};

/// Fall distance per tick while dropping off an edge.
const FALL_STEP: f64 = 0.5;
const TICK: Duration = Duration::from_millis(50);
/// The "kick" reason of a bot that stopped because another bot of its build failed.
pub const STOPPED: &str = "stopped: another bot of this build failed";

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

/// New (position, yaw, pitch) after a player_position packet. Relative bits (docs/RE/movement.md):
/// X 0x01, Y 0x02, Z 0x04, Y_ROT (yaw) 0x08, X_ROT (pitch) 0x10.
pub fn apply_teleport(cur: (Vec3, f32, f32), new: (f64, f64, f64, f32, f32), flags: u8) -> (Vec3, f32, f32) {
    let (feet, yaw, pitch) = cur;
    let rel = |bit: u8| flags & bit != 0;
    let pos = Vec3::new(
        if rel(0x01) { feet.x + new.0 } else { new.0 },
        if rel(0x02) { feet.y + new.1 } else { new.1 },
        if rel(0x04) { feet.z + new.2 } else { new.2 },
    );
    (pos, if rel(0x08) { yaw + new.3 } else { new.3 }, if rel(0x10) { pitch + new.4 } else { new.4 })
}

/// A read that simply timed out. Windows can also report a timed-out socket read as
/// "overlapped I/O in progress" (os error 997) or WSAETIMEDOUT (10060).
fn is_timeout(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) || matches!(e.raw_os_error(), Some(997 | 10060 | 10035))
}

pub struct PlayClient {
    pub conn: Connection,
    /// Release spoken on this connection (packet ids and layouts).
    pub ver: &'static Version,
    pub feet: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub seq: i32,
    held_item: i32,
    pub kicked: Option<String>,
    /// Game mode the server reported (see [`crate::gamemode`]); `None` if it never said.
    pub game_mode: Option<u8>,
    /// Pause after each placement; raise it on servers with strict packet-rate limits.
    pub place_delay: Duration,
    /// Last state the server reported per block position (see [`BlockLog`]).
    pub blocks: BlockLog,
    /// Blocks of the loaded chunks, for the site survey and undo.
    pub chunks: ChunkStore,
    /// Highest block action sequence the server has acknowledged: its block updates for every
    /// click and dig up to this one have arrived.
    pub acked: i32,
    /// Teleports forced by the server (a sign we are pushing into something).
    pub teleports: u32,
    next_tick: Instant,
    /// Set by a cooperative build when another bot failed: this one stops at its next packet.
    pub stop: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Entity id from the login packet (needed by player_command).
    entity_id: i32,
    /// Ground distance per tick for level walking: walking, or sprinting after [`PlayClient::set_sprint`].
    stride: f64,
}

impl PlayClient {
    pub fn join(addr: &str, host: &str, port: u16, name: &str) -> io::Result<(Self, JoinInfo)> {
        Self::join_v(addr, host, port, name, version::active().protocol)
    }

    /// Join announcing an explicit protocol number (see [`crate::version`]).
    pub fn join_v(addr: &str, host: &str, port: u16, name: &str, protocol: i32) -> io::Result<(Self, JoinInfo)> {
        let (conn, join) = proto::login_offline_v(addr, host, port, name, protocol, |_, _, _| {})?;
        let mut client = PlayClient {
            conn,
            ver: version::active(),
            feet: Vec3::new(0.0, 0.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            seq: 0,
            held_item: -1,
            kicked: None,
            game_mode: None,
            place_delay: Duration::from_millis(25),
            blocks: BlockLog::default(),
            chunks: ChunkStore::default(),
            acked: 0,
            teleports: 0,
            next_tick: Instant::now(),
            stop: None,
            entity_id: 0,
            stride: WALK_STEP,
        };
        client.conn.set_read_timeout(Some(Duration::from_millis(5)))?;
        // Pump packets until the first teleport sync fixes our position
        let deadline = Instant::now() + Duration::from_secs(10);
        while client.teleports == 0 && Instant::now() < deadline {
            client.pump_once()?;
            if client.kicked.is_some() {
                break;
            }
        }
        // without a position from the server nothing may be sent that claims one
        if client.teleports == 0 && client.kicked.is_none() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "the server sent no player position within 10 s"));
        }
        Ok((client, join))
    }

    /// Handle one incoming packet (or time out): kicks, teleports and keepalives.
    pub fn pump_once(&mut self) -> io::Result<()> {
        if self.kicked.is_none() && self.stop.as_ref().is_some_and(|s| s.load(std::sync::atomic::Ordering::Relaxed)) {
            self.kicked = Some(STOPPED.into());
        }
        let p = self.ver.packets();
        match self.conn.recv() {
            Ok((id, mut body)) => {
                match id {
                    x if x == p.cb_play_login => {
                        self.entity_id = body.read_i32().unwrap_or(0);
                        self.game_mode = gamemode::from_login(&mut body, self.ver.protocol <= 765).or(self.game_mode);
                    }
                    x if x == p.cb_play_game_event => {
                        self.game_mode = gamemode::from_game_event(&mut body).or(self.game_mode);
                    }
                    x if x == p.cb_play_keep_alive => {
                        let k = body.read_i64().unwrap_or(0);
                        let mut r = PacketBuf::new();
                        r.write_i64(k);
                        self.conn.send(p.sb_play_keep_alive, r.as_slice())?;
                    }
                    x if x == p.cb_play_ping => {
                        let v = body.read_i32().unwrap_or(0);
                        let mut r = PacketBuf::new();
                        r.write_i32(v);
                        self.conn.send(p.sb_play_pong, r.as_slice())?;
                    }
                    x if x == p.cb_play_player_position => self.on_teleport(&mut body)?,
                    x if x == p.cb_play_chunk_batch_finished => {
                        // the server waits for this answer before it sends the next batch
                        self.conn.send(p.sb_play_chunk_batch_received, proto::chunk_batch_received(9.0).as_slice())?;
                    }
                    x if x == p.cb_play_block_changed_ack => {
                        self.acked = self.acked.max(body.read_varint().unwrap_or(0));
                    }
                    x if x == p.cb_play_block_update => {
                        if let Some((pos, state)) = blocklog::parse_block_change(&mut body) {
                            self.block_changed(pos, state);
                        }
                    }
                    x if x == p.cb_play_section_blocks_update => {
                        for (pos, state) in blocklog::parse_multi_block_change(&mut body).unwrap_or_default() {
                            self.block_changed(pos, state);
                        }
                    }
                    x if x == p.cb_play_level_chunk_with_light => match chunks::parse_chunk(self.ver.layout, &mut body) {
                        Some((at, sections)) => self.chunks.load(at, sections),
                        None => self.chunks.unreadable += 1,
                    },
                    x if x == p.cb_play_forget_level_chunk => {
                        if let Some(at) = chunks::parse_forget(&mut body) {
                            self.chunks.forget(at);
                        }
                    }
                    x if x == p.cb_play_disconnect => {
                        // play-state reason is a network NBT component (docs/RE/protocol.md)
                        self.kicked = Some(crate::text::disconnect_reason(&mut body));
                    }
                    x if x == p.cb_play_set_health => {
                        let health = body.read_f32().unwrap_or(20.0);
                        if health <= 0.0 {
                            self.kicked = Some(tr!("died", "사망"));
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            Err(e) if is_timeout(&e) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn block_changed(&mut self, pos: BlockPos, state: i32) {
        self.blocks.record(pos, state);
        if let Ok(state) = u32::try_from(state) {
            self.chunks.set(pos, state);
        }
    }

    /// Apply a server teleport, confirm it, and report the resulting position.
    fn on_teleport(&mut self, body: &mut PacketBuf) -> io::Result<()> {
        let Some(t) = codec::read_teleport(self.ver.layout, body) else {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unreadable player_position packet"));
        };
        let new = (t.pos.0, t.pos.1, t.pos.2, t.yaw, t.pitch);
        (self.feet, self.yaw, self.pitch) = apply_teleport((self.feet, self.yaw, self.pitch), new, t.flags);
        let p = self.ver.packets();
        let confirm = codec::accept_teleport(self.ver.layout, t.id, (self.feet, self.yaw, self.pitch));
        self.conn.send(p.sb_play_accept_teleportation, confirm.as_slice())?;
        self.teleports += 1;
        // Newer releases ignore movement and clicks until the client says it has loaded
        // (or 60 server ticks pass).
        if self.teleports == 1 && p.sb_play_player_loaded >= 0 {
            self.conn.send(p.sb_play_player_loaded, &[])?;
        }
        // After confirming, report the current position
        self.send_pos_look(true)
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
        b.write_bool(on_ground); // newer releases read this byte as flags: bit 0 = on ground
        self.send_move(self.ver.packets().sb_play_move_player_pos, b.as_slice())
    }

    fn send_pos_look(&mut self, on_ground: bool) -> io::Result<()> {
        let mut b = PacketBuf::new();
        b.write_f64(self.feet.x);
        b.write_f64(self.feet.y);
        b.write_f64(self.feet.z);
        b.write_f32(self.yaw);
        b.write_f32(self.pitch);
        b.write_bool(on_ground);
        self.send_move(self.ver.packets().sb_play_move_player_pos_rot, b.as_slice())
    }

    /// A movement packet, closed with client_tick_end where the release has it: 26.3 disconnects
    /// a client that sends two positions within one of its own ticks.
    fn send_move(&mut self, id: i32, body: &[u8]) -> io::Result<()> {
        self.conn.send(id, body)?;
        let end = self.ver.packets().sb_play_client_tick_end;
        if end >= 0 {
            self.conn.send(end, &[])?;
        }
        Ok(())
    }

    /// Arm swing animation; releases without the swing packet animate on the server.
    fn swing(&mut self) -> io::Result<()> {
        let id = self.ver.packets().sb_play_swing;
        if id < 0 {
            return Ok(());
        }
        let mut s = PacketBuf::new();
        s.write_varint(0); // main hand
        self.conn.send(id, s.as_slice())
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
        // after a pause start over: catching up would send several moves in one server tick
        self.next_tick = if now > self.next_tick + TICK / 2 { now + TICK } else { self.next_tick + TICK };
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

    /// Sprint (or walk again). Tells the server, like the vanilla client, and changes the stride.
    pub fn set_sprint(&mut self, on: bool) -> io::Result<()> {
        let action = if on { PlayerAction::StartSprinting } else { PlayerAction::StopSprinting };
        let b = codec::player_command(self.ver.layout, self.entity_id, action);
        self.conn.send(self.ver.packets().sb_play_player_command, b.as_slice())?;
        self.stride = if on { SPRINT_STEP } else { WALK_STEP };
        Ok(())
    }

    /// Walk along level cell centres without stopping at each one.
    pub fn walk_level(&mut self, cells: &[Cell]) -> io::Result<()> {
        let path: Vec<Vec3> = cells.iter().map(|&c| cell_feet(c)).collect();
        let mut next = 0;
        let limit = 40 + path.len() * 8;
        for _ in 0..limit {
            if next >= path.len() || self.kicked.is_some() {
                break;
            }
            self.feet = motion::advance(self.feet, &path, &mut next, self.stride);
            self.send_pos(true)?;
            self.tick()?;
        }
        Ok(())
    }

    /// Follow a route of feet cells: walk on the level, jump up one block, drop down a few.
    pub fn walk_cells(&mut self, route: &[Cell]) -> io::Result<()> {
        for pair in route.windows(2) {
            let target = cell_feet(pair[1]);
            match pair[1].1.cmp(&pair[0].1) {
                std::cmp::Ordering::Equal => self.walk_to(target.x, target.z)?,
                std::cmp::Ordering::Greater => self.jump_to(target)?,
                std::cmp::Ordering::Less => self.drop_to(target)?,
            }
            if self.kicked.is_some() {
                break;
            }
        }
        Ok(())
    }

    /// Jump the vanilla arc ([`motion::jump_ticks`]) onto `target`, one block up and one over.
    fn jump_to(&mut self, target: Vec3) -> io::Result<()> {
        let teleports = self.teleports;
        for at in motion::jump_ticks(self.feet, target, self.stride) {
            if self.kicked.is_some() || self.teleports != teleports {
                break;
            }
            self.feet = at;
            self.send_pos(false)?;
            self.tick()?;
        }
        if self.teleports != teleports {
            return Ok(()); // the server's position wins
        }
        self.feet = target;
        self.send_pos(true)?;
        self.tick()
    }

    /// Step off an edge toward `target` (lower than we are) and fall onto it.
    fn drop_to(&mut self, target: Vec3) -> io::Result<()> {
        let teleports = self.teleports;
        let level = Vec3::new(target.x, self.feet.y, target.z);
        for _ in 0..40 {
            if self.feet.horiz_dist(level) <= 0.05 || self.kicked.is_some() || self.teleports != teleports {
                break;
            }
            self.feet = motion::advance(self.feet, &[level], &mut 0, self.stride);
            self.send_pos(self.feet.horiz_dist(level) > 0.3)?;
            self.tick()?;
        }
        while self.feet.y > target.y + 1e-9 && self.kicked.is_none() && self.teleports == teleports {
            self.feet.y = (self.feet.y - FALL_STEP).max(target.y);
            self.send_pos(false)?;
            self.tick()?;
        }
        if self.teleports != teleports {
            return Ok(()); // the server's position wins
        }
        self.feet = target;
        self.send_pos(true)?;
        self.tick()
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

    /// Pillar jumping: jump, place a block under our own feet, land on it; `height` times.
    /// `item_id` is the scaffold block. The bot starts and ends on the ground it stands on.
    pub fn pillar_up(&mut self, base: Cell, height: i32, item_id: i32) -> io::Result<()> {
        self.set_creative_held(item_id)?;
        for k in 0..height {
            let y = base.1 + k;
            let floor = BlockPos::new(base.0, y - 1, base.2);
            for rise in JUMP_ARC {
                self.feet.y = y as f64 + rise;
                self.send_pos(false)?;
                self.tick()?;
            }
            // Feet are now above the cell to fill: click the top face of the block below it.
            self.pitch = 90.0;
            self.send_pos_look(false)?;
            let cell = BlockPos::new(base.0, y, base.2);
            self.blocks.forget(cell);
            self.use_item_on(floor, Face::Up, (0.5, 1.0, 0.5))?;
            self.idle(self.place_delay)?;
            if !self.wait_confirmed(cell, Duration::from_secs(2))? {
                self.fall_to(y as f64)?;
                return Err(io::Error::other(format!("pillar block {cell:?} was not accepted; standing on it would be hovering")));
            }
            self.feet.y = (y + 1) as f64;
            self.send_pos_look(true)?;
            self.tick()?;
            if self.kicked.is_some() {
                break;
            }
        }
        Ok(())
    }

    /// Wait until the server reports a block at `pos`; false after `limit` (or a kick).
    fn wait_confirmed(&mut self, pos: BlockPos, limit: Duration) -> io::Result<bool> {
        let end = Instant::now() + limit;
        while !self.blocks.is_confirmed(pos) && self.kicked.is_none() && Instant::now() < end {
            self.pump_once()?;
        }
        Ok(self.blocks.is_confirmed(pos) || self.kicked.is_some())
    }

    /// Wait until the server has acknowledged every click and dig sent so far; false on timeout
    /// (a server that stalls longer than `limit`).
    pub fn sync(&mut self, limit: Duration) -> io::Result<bool> {
        let end = Instant::now() + limit;
        while self.acked < self.seq && self.kicked.is_none() && Instant::now() < end {
            self.pump_once()?;
        }
        Ok(self.acked >= self.seq)
    }

    /// Break one block (creative mode: instantly) at `pos`.
    fn dig(&mut self, pos: BlockPos) -> io::Result<()> {
        self.seq += 1;
        let mut b = PacketBuf::new();
        b.write_varint(0); // status: started digging
        b.write_position(pos.x, pos.y, pos.z);
        b.write_u8(Face::Up as u8);
        b.write_varint(self.seq);
        self.conn.send(self.ver.packets().sb_play_player_action, b.as_slice())?;
        self.swing()
    }

    /// Place one scaffold block by clicking `face` of `support` (bridging).
    pub fn place_support(&mut self, pos: BlockPos, support: BlockPos, face: Face, cursor: (f32, f32, f32)) -> io::Result<()> {
        let item = crate::blocks::item_id("cobblestone").unwrap_or(1);
        let eye = Vec3::new(self.feet.x, self.feet.y + EYE_HEIGHT, self.feet.z);
        let (yaw, pitch) = look_at(eye, pos.center());
        self.yaw = yaw;
        self.pitch = pitch;
        self.set_creative_held(item)?;
        self.send_pos_look(true)?;
        // Later steps click this block or stand on it, so the server must confirm it (one retry).
        for _ in 0..2 {
            self.blocks.forget(pos);
            self.use_item_on(support, face, cursor)?;
            self.idle(self.place_delay)?;
            if self.wait_confirmed(pos, Duration::from_secs(2))? {
                return Ok(());
            }
        }
        Err(io::Error::other(format!("temporary support {pos:?} was refused; continuing could mean hovering")))
    }

    /// Fall straight down to feet height `y` (after the block under us was broken).
    pub fn fall_to(&mut self, y: f64) -> io::Result<()> {
        while self.feet.y > y + 1e-9 && self.kicked.is_none() {
            self.feet.y = (self.feet.y - FALL_STEP).max(y);
            self.send_pos(false)?;
            self.tick()?;
        }
        self.send_pos(true)?;
        self.tick()
    }

    /// Break a scaffold block within reach and wait for the server to apply it.
    pub fn break_block(&mut self, pos: BlockPos) -> io::Result<()> {
        self.dig(pos)?;
        self.idle(self.place_delay)
    }

    /// Take a pillar down from the top: dig the block under our feet, drop onto the next one.
    pub fn pillar_down(&mut self, base: Cell, height: i32) -> io::Result<()> {
        for k in (0..height).rev() {
            self.dig(BlockPos::new(base.0, base.1 + k, base.2))?;
            self.idle(self.place_delay)?;
            let landing = base.1 as f64 + k as f64;
            while self.feet.y > landing + 1e-9 && self.kicked.is_none() {
                self.feet.y = (self.feet.y - FALL_STEP).max(landing);
                self.send_pos(false)?;
                self.tick()?;
            }
            self.send_pos(true)?;
            self.tick()?;
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
        let p = self.ver.packets();
        self.conn.send(p.sb_play_set_creative_mode_slot, codec::creative_slot(self.ver.layout, 36, item_id).as_slice())?;
        // Select hotbar slot 0
        let mut h = PacketBuf::new();
        h.write_i16(0);
        self.conn.send(p.sb_play_set_carried_item, h.as_slice())?;
        self.held_item = item_id;
        Ok(())
    }

    /// Right-click the `face` of the `support` block, placing a block next to it.
    fn use_item_on(&mut self, support: BlockPos, face: Face, cursor: (f32, f32, f32)) -> io::Result<()> {
        self.seq += 1;
        let b = codec::use_item_on(self.ver.layout, support, face, cursor, self.seq);
        self.conn.send(self.ver.packets().sb_play_use_item_on, b.as_slice())?;
        self.swing()
    }

    /// Press or release shift; the pose follows next tick.
    fn set_sneak(&mut self, on: bool) -> io::Result<()> {
        let p = self.ver.packets();
        if self.ver.layout.sneak_by_input {
            self.conn.send(p.sb_play_player_input, codec::shift_input(on).as_slice())?;
        } else {
            let action = if on { PlayerAction::PressShift } else { PlayerAction::ReleaseShift };
            let b = codec::player_command(self.ver.layout, self.entity_id, action);
            self.conn.send(p.sb_play_player_command, b.as_slice())?;
        }
        self.idle(Duration::from_millis(60))
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
        self.pitch = plan.pitch.unwrap_or(pitch);
        self.set_creative_held(item_id)?;
        self.send_pos_look(true)?;
        if plan.sneak {
            self.set_sneak(true)?;
        }
        for click in &plan.clicks {
            self.use_item_on(click.support, click.face, click.cursor)?;
            self.idle(self.place_delay)?;
            if self.kicked.is_some() {
                break;
            }
        }
        if plan.sneak {
            self.set_sneak(false)?;
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
    fn relative_flags_use_the_real_bit_order() {
        let cur = (Vec3::new(1.0, 2.0, 3.0), 10.0, 20.0);
        // only yaw relative (0x08): pitch is absolute
        let (_, yaw, pitch) = apply_teleport(cur, (0.0, 0.0, 0.0, 5.0, 7.0), 0x08);
        assert_eq!((yaw, pitch), (15.0, 7.0));
        // only pitch relative (0x10)
        let (_, yaw, pitch) = apply_teleport(cur, (0.0, 0.0, 0.0, 5.0, 7.0), 0x10);
        assert_eq!((yaw, pitch), (5.0, 27.0));
        let (p, _, _) = apply_teleport(cur, (1.0, 1.0, 1.0, 0.0, 0.0), 0x07);
        assert_eq!((p.x, p.y, p.z), (2.0, 3.0, 4.0));
    }

    #[test]
    fn windows_timeouts_are_not_fatal() {
        assert!(is_timeout(&io::Error::from_raw_os_error(997)));
        assert!(is_timeout(&io::Error::from(io::ErrorKind::WouldBlock)));
        assert!(!is_timeout(&io::Error::from(io::ErrorKind::ConnectionReset)));
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
