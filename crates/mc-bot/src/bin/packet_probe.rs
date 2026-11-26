//! packet-probe — 1.21.1 서버에 붙어 프로토콜을 "리버스"로 관찰하는 도구.
//!
//!   packet-probe status [host:port]            서버 목록 핑(JSON)
//!   packet-probe probe  [host:port] [name] [초] 로그인 후 Play 패킷을 수집·분류
//!
//! probe 모드는 keepalive/텔레포트에 정확히 응답해 "킥당하지 않고" 패킷 흐름을 샘플링한다.
//! online-mode=false 서버 전용.

use mc_bot::ids;
use mc_bot::proto;
use mc_proto::buf::PacketBuf;
use mc_proto::State;
use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("status");
    let target = args.get(2).map(String::as_str).unwrap_or("127.0.0.1:25566");
    let (host, port) = split_addr(target);

    let result = match mode {
        "status" => run_status(target, &host, port),
        "probe" => {
            let name = args.get(3).map(String::as_str).unwrap_or("ProbeBot");
            let secs: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(6);
            run_probe(target, &host, port, name, secs)
        }
        _ => {
            eprintln!("사용법: packet-probe <status|probe> [host:port] [name] [초]");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("[오류] {e}");
        std::process::exit(1);
    }
}

fn split_addr(target: &str) -> (String, u16) {
    match target.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(25565)),
        None => (target.to_string(), 25565),
    }
}

fn run_status(addr: &str, host: &str, port: u16) -> io::Result<()> {
    let json = proto::status_ping(addr, host, port)?;
    println!("=== status ({addr}) ===");
    for key in ["\"name\"", "\"protocol\"", "\"online\"", "\"max\""] {
        if let Some(pos) = json.find(key) {
            let snippet = &json[pos..(pos + 48).min(json.len())];
            println!("  {}", snippet.replace('\n', " "));
        }
    }
    println!("  raw {} bytes", json.len());
    Ok(())
}

#[derive(Default)]
struct Stats {
    counts: BTreeMap<String, u64>,
    bytes: BTreeMap<String, u64>,
    total: u64,
    total_bytes: u64,
}
impl Stats {
    fn record(&mut self, name: &str, len: usize) {
        *self.counts.entry(name.to_string()).or_default() += 1;
        *self.bytes.entry(name.to_string()).or_default() += len as u64;
        self.total += 1;
        self.total_bytes += len as u64;
    }
}

fn run_probe(addr: &str, host: &str, port: u16, name: &str, secs: u64) -> io::Result<()> {
    let mut stats = Stats::default();
    println!("=== 로그인 ({addr} as {name}) ===");

    let (mut conn, join) = proto::login_offline(addr, host, port, name, |state, id, body| {
        let n = mc_bot::ids::name(state, true, id);
        stats.record(&format!("{:?}/{} (0x{:02X})", state, n, id), body.remaining() + 1);
    })?;
    println!(
        "  로그인 성공: user={} uuid={:032x}, 압축임계값={:?}",
        join.username, join.uuid, conn.threshold()
    );
    println!("=== Play 패킷 {secs}초 수집 (keepalive/텔레포트 응답하며 유지) ===");

    conn.set_read_timeout(Some(Duration::from_millis(500)))?;
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut pos = (0.0f64, 0.0f64, 0.0f64);
    let mut kept_alive = 0u32;
    let mut teleports = 0u32;

    while Instant::now() < deadline {
        match conn.recv() {
            Ok((id, mut body)) => {
                let len = body.remaining() + 1;
                let n = mc_bot::ids::name(State::Play, true, id);
                stats.record(&format!("Play/{} (0x{:02X})", n, id), len);

                match id {
                    ids::CB_PLAY_KEEPALIVE => {
                        let k = body.read_i64().unwrap_or(0);
                        let mut r = PacketBuf::new();
                        r.write_i64(k);
                        conn.send(ids::SB_PLAY_KEEPALIVE, r.as_slice())?;
                        kept_alive += 1;
                    }
                    ids::CB_PLAY_SYNC_POS => {
                        let x = body.read_f64().unwrap_or(0.0);
                        let y = body.read_f64().unwrap_or(0.0);
                        let z = body.read_f64().unwrap_or(0.0);
                        let _yaw = body.read_f32().unwrap_or(0.0);
                        let _pitch = body.read_f32().unwrap_or(0.0);
                        let _flags = body.read_u8().unwrap_or(0);
                        let tp_id = body.read_varint().unwrap_or(0);
                        pos = (x, y, z);
                        let mut r = PacketBuf::new();
                        r.write_varint(tp_id);
                        conn.send(ids::SB_PLAY_CONFIRM_TELEPORT, r.as_slice())?;
                        teleports += 1;
                    }
                    ids::CB_PLAY_DISCONNECT => {
                        println!("  [서버가 연결 종료]");
                        break;
                    }
                    _ => {}
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    println!("\n=== 패킷 분석 요약 ===");
    println!("  위치(spawn): ({:.1}, {:.1}, {:.1})", pos.0, pos.1, pos.2);
    println!("  keepalive 응답 {kept_alive}회, 텔레포트 확인 {teleports}회 → 킥 없이 유지됨");
    println!(
        "  수신 패킷 {} 종류, 총 {} 개, {} 바이트\n",
        stats.counts.len(),
        stats.total,
        stats.total_bytes
    );

    let mut rows: Vec<_> = stats
        .counts
        .iter()
        .map(|(k, c)| (*c, stats.bytes[k], k.clone()))
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    println!("  {:<42} {:>6} {:>10}", "packet", "count", "bytes");
    for (count, bytes, name) in rows.iter().take(25) {
        println!("  {:<42} {:>6} {:>10}", name, count, bytes);
    }
    Ok(())
}
