//! Play 상태 클라이언트 + 건축 로직.
//!
//! 핵심 설계(공중부양/과속 킥 회피):
//!  · 항상 지면(on_ground=true)에 서서, 한 틱당 이동량을 걷기 속도 이하로 제한해 조금씩 이동.
//!  · keepalive·텔레포트 동기화에 즉시 응답.
//!  · 블록은 "아래 블록의 윗면"을 눌러(Use Item On) 설치 — 반드시 받침이 있는 칸만.
//!  · 눈~블록 거리가 도달범위(4.5) 안일 때만 설치, 아니면 받침 지면 칸으로 걸어가서.

use crate::ids;
use crate::net::Connection;
use crate::proto::{self, JoinInfo};
use crate::world::{look_at, within_reach, BlockPos, Face, Vec3, EYE_HEIGHT};
use mc_proto::buf::PacketBuf;
use std::io;
use std::thread::sleep;
use std::time::{Duration, Instant};

pub struct PlayClient {
    pub conn: Connection,
    pub feet: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub seq: i32,
    held_item: i32,
    pub kicked: Option<String>,
}

impl PlayClient {
    pub fn join(addr: &str, host: &str, port: u16, name: &str) -> io::Result<(Self, JoinInfo)> {
        let (conn, join) = proto::login_offline(addr, host, port, name, |_, _, _| {})?;
        let mut client = PlayClient {
            conn,
            feet: Vec3::new(0.0, 0.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            seq: 0,
            held_item: -1,
            kicked: None,
        };
        client.conn.set_read_timeout(Some(Duration::from_millis(50)))?;
        // 첫 동기화(teleport)를 받아 위치를 확정할 때까지 펌프
        let deadline = Instant::now() + Duration::from_secs(10);
        while client.feet == Vec3::new(0.0, 0.0, 0.0) && Instant::now() < deadline {
            client.pump_once()?;
            if client.kicked.is_some() {
                break;
            }
        }
        Ok((client, join))
    }

    /// 수신 패킷 하나를 처리(없으면 타임아웃). 킥/텔레포트/keepalive 를 다룬다.
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
                        // flags 비트: 0x01=X상대 ... 간단히 절대값으로 간주(서버가 보통 절대)
                        if flags & 0x01 == 0 { self.feet.x = x; } else { self.feet.x += x; }
                        if flags & 0x02 == 0 { self.feet.y = y; } else { self.feet.y += y; }
                        if flags & 0x04 == 0 { self.feet.z = z; } else { self.feet.z += z; }
                        if flags & 0x08 == 0 { self.pitch = pitch; }
                        if flags & 0x10 == 0 { self.yaw = yaw; }
                        let mut r = PacketBuf::new();
                        r.write_varint(tp);
                        self.conn.send(ids::SB_PLAY_CONFIRM_TELEPORT, r.as_slice())?;
                        // 확인 후 현재 위치를 서버에 보고
                        self.send_pos_look(true)?;
                    }
                    ids::CB_PLAY_DISCONNECT => {
                        let reason = body.read_string().unwrap_or_default();
                        self.kicked = Some(reason);
                    }
                    ids::CB_PLAY_UPDATE_HEALTH => {
                        let health = body.read_f32().unwrap_or(20.0);
                        if health <= 0.0 {
                            self.kicked = Some("사망".into());
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

    /// dur 동안 들어오는 패킷에 응답하며 대기(바쁜 대기 아님).
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

    /// 지면을 따라 (tx,tz) 로 걸어간다. y 는 그대로(평지 가정). 과속 킥 방지로 조금씩.
    pub fn walk_to(&mut self, tx: f64, tz: f64) -> io::Result<()> {
        const STEP: f64 = 0.26; // 틱당 최대 이동(걷기 ~0.21)
        let target = Vec3::new(tx, self.feet.y, tz);
        let mut guard = 0;
        while self.feet.horiz_dist(target) > 0.05 && guard < 2000 {
            guard += 1;
            let dx = target.x - self.feet.x;
            let dz = target.z - self.feet.z;
            let d = (dx * dx + dz * dz).sqrt();
            let s = d.min(STEP);
            self.feet.x += dx / d * s;
            self.feet.z += dz / d * s;
            self.send_pos(true)?;
            self.pump_once()?;
            if self.kicked.is_some() {
                break;
            }
            sleep(Duration::from_millis(40));
        }
        Ok(())
    }

    fn set_creative_held(&mut self, item_id: i32) -> io::Result<()> {
        if self.held_item == item_id {
            return Ok(());
        }
        // 핫바 0번 = 인벤토리 슬롯 36
        let mut b = PacketBuf::new();
        b.write_i16(36);
        // Slot: count=1, itemId, add=0, remove=0
        b.write_varint(1);
        b.write_varint(item_id);
        b.write_varint(0);
        b.write_varint(0);
        self.conn.send(ids::SB_PLAY_SET_CREATIVE_SLOT, b.as_slice())?;
        // 손에 들기(핫바 0)
        let mut h = PacketBuf::new();
        h.write_i16(0);
        self.conn.send(ids::SB_PLAY_HELD_ITEM, h.as_slice())?;
        self.held_item = item_id;
        Ok(())
    }

    /// support 블록의 face 면을 눌러 그 옆에 블록 설치.
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
        // 팔 휘두르기(애니메이션)
        let mut s = PacketBuf::new();
        s.write_varint(0);
        self.conn.send(ids::SB_PLAY_SWING_ARM, s.as_slice())
    }

    /// (bx,by,bz) 에 블록 설치 시도. 받침(아래 블록)이 있다고 가정하고 윗면을 누른다.
    /// 도달 못 하면 Err 가 아니라 false.
    pub fn place_block(&mut self, pos: BlockPos, item_id: i32) -> io::Result<bool> {
        let support = pos.below(); // 아래 블록 윗면 클릭
        if !within_reach(self.feet, pos) {
            return Ok(false);
        }
        // 목표를 바라보게
        let eye = Vec3::new(self.feet.x, self.feet.y + EYE_HEIGHT, self.feet.z);
        let (yaw, pitch) = look_at(eye, pos.center());
        self.yaw = yaw;
        self.pitch = pitch;
        self.set_creative_held(item_id)?;
        self.send_pos_look(true)?;
        self.use_item_on(support, Face::Up, (0.5, 1.0, 0.5))?;
        self.idle(Duration::from_millis(25))?;
        Ok(true)
    }
}
