//! build-bot — 설계도(blueprint)대로 1.21.1 오프라인 서버에 블록을 짓는다.
//!
//!   build-bot [host:port] [name] [blueprint.txt]
//!
//! 지면을 걸어 다니며(공중부양/과속 없이) 받침 있는 칸부터 설치한다.
//! 크리에이티브 서버에서 동작(블록을 set_creative_slot 으로 손에 들어 설치).

use mc_bot::builder::PlayClient;
use mc_bot::schematic::Schematic;
use mc_bot::world::{within_reach, BlockPos, Vec3};
use mc_bot::{blocks, schem};
use std::collections::HashSet;
use std::io::Read;
use std::io;
use std::time::Duration;

const DEFAULT_BLUEPRINT: &str = "name 작은 오두막
palette W white_concrete
palette G glass
palette R red_concrete
layer 0
WWW
W.W
WWW
layer 1
WGW
G.G
WGW
layer 2
RRR
RRR
RRR
";

const USAGE: &str = "\
build-bot — 설계도대로 Minecraft 1.21.1 오프라인 서버에 건축

사용법:
  build-bot [host:port] [이름] [설계도.txt]        건축 실행 (기본 127.0.0.1:25566, BuildBot)
  build-bot [host:port] [이름] <설계도.txt> --dry-run   서버 없이 설계도만 검증
  build-bot ... --delay-ms <N>                      설치 간 대기(기본 25ms). 패킷 속도 제한이 엄격한 서버용
  build-bot --help                                  이 도움말

예시:
  build-bot 127.0.0.1:25566 MyBot blueprints/cottage.txt
  build-bot x x blueprints/cottage.txt --dry-run

설계도 포맷은 blueprints/ 예시와 README 참고.";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    let target = args.get(1).map(String::as_str).unwrap_or("127.0.0.1:25566");
    let name = args.get(2).map(String::as_str).unwrap_or("BuildBot");
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(25565)),
        None => (target.to_string(), 25565),
    };
    let schem = match args.get(3) {
        Some(src) => load_schematic(src).unwrap_or_else(|e| {
            eprintln!("설계도 불러오기 실패({src}): {e}");
            std::process::exit(1);
        }),
        None => Schematic::parse(DEFAULT_BLUEPRINT).unwrap(),
    };

    // --dry-run: 서버 접속 없이 설계도만 검증·요약
    if args.iter().any(|a| a == "--dry-run" || a == "--validate") {
        print_summary(&schem);
        return;
    }

    let delay_ms: u64 = args
        .iter()
        .position(|a| a == "--delay-ms")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(25);
    if let Err(e) = run(target, &host, port, name, &schem, Duration::from_millis(delay_ms)) {
        eprintln!("[오류] {e}");
        std::process::exit(1);
    }
}

// 설계도 로드: 로컬 파일 또는 http(s) URL. .schem(gzip NBT) 과 텍스트(.txt) 자동 판별.
// "딸깍 한 번" — 공유 사이트의 .schem 직링크를 그대로 넣으면 받아서 짓는다.
fn load_schematic(src: &str) -> Result<Schematic, String> {
    let name = src.rsplit(['/', '\\']).next().unwrap_or("blueprint").to_string();
    let bytes: Vec<u8> = if src.starts_with("http://") || src.starts_with("https://") {
        eprintln!("다운로드: {src}");
        let resp = ureq::get(src).call().map_err(|e| format!("HTTP: {e}"))?;
        let mut buf = Vec::new();
        resp.into_reader().read_to_end(&mut buf).map_err(|e| format!("읽기: {e}"))?;
        buf
    } else {
        std::fs::read(src).map_err(|e| e.to_string())?
    };

    // gzip 매직(1f 8b) 또는 .schem/.litematic 확장자 → NBT 스키매틱
    let is_nbt = bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
        || src.ends_with(".schem") || src.ends_with(".schematic") || src.ends_with(".litematic");
    if is_nbt {
        if src.ends_with(".litematic") {
            return Err(".litematic 은 아직 미지원입니다. .schem 으로 변환해 주세요.".into());
        }
        schem::parse(&bytes, &name)
    } else {
        let text = String::from_utf8(bytes).map_err(|_| "텍스트 설계도가 UTF-8 이 아닙니다.".to_string())?;
        Schematic::parse(&text)
    }
}

// 설계도 검증·요약 (서버 불필요). 알 수 없는 블록도 표시.
fn print_summary(schem: &Schematic) {
    let (min, max) = schem.bounds();
    println!("=== 설계도 검증: '{}' ===", schem.name);
    println!(
        "크기 {}x{}x{} (x {}..{}, y {}..{}, z {}..{}), 총 {}블록",
        max.x - min.x + 1, max.y - min.y + 1, max.z - min.z + 1,
        min.x, max.x, min.y, max.y, min.z, max.z, schem.blocks.len()
    );
    println!("\n{}", schem.ascii_layers());
    let floating = schem.floating_blocks();
    if !floating.is_empty() {
        println!("※ 아래가 비어 있는(공중) 블록 {}개 — 받침이 없어 설치가 거부될 수 있습니다.\n", floating.len());
    }
    let mut unknown = Vec::new();
    println!("블록 종류:");
    for (name, count) in schem.block_counts() {
        let ok = blocks::item_id(&name).is_some();
        if !ok { unknown.push(name.clone()); }
        println!("  {:>5}  {}{}", count, name, if ok { "" } else { "  ← 미등록(설치 불가)" });
    }
    if unknown.is_empty() {
        println!("모든 블록이 설치 가능합니다. ✅");
    } else {
        println!("미등록 블록 {}종: {} — blocks.rs 에 없는 이름입니다.", unknown.len(), unknown.join(", "));
    }
}

fn run(addr: &str, host: &str, port: u16, name: &str, schem: &Schematic, delay: Duration) -> io::Result<()> {
    println!("=== 접속 ({addr} as {name}) ===");
    let (mut bot, _join) = PlayClient::join(addr, host, port, name)?;
    bot.place_delay = delay;
    if let Some(reason) = &bot.kicked {
        println!("킥됨: {reason}");
        return Ok(());
    }
    let spawn = bot.feet;
    println!("스폰 위치: ({:.1}, {:.1}, {:.1})", spawn.x, spawn.y, spawn.z);

    // 구조물 원점: 스폰에서 x+2, z+2 (겹침 방지), y 는 지면 높이
    let origin = BlockPos::new(spawn.x.floor() as i32 + 2, spawn.y.floor() as i32, spawn.z.floor() as i32 + 2);
    let (min, max) = schem.bounds();
    println!(
        "설계도 '{}': {}블록, 크기 {}x{}x{}, 원점 ({},{},{})",
        schem.name,
        schem.blocks.len(),
        max.x - min.x + 1,
        max.y - min.y + 1,
        max.z - min.z + 1,
        origin.x, origin.y, origin.z
    );

    // 월드 좌표 집합(서 있을 칸 판별용)
    let occupied: HashSet<(i32, i32, i32)> = schem
        .blocks
        .iter()
        .map(|b| (origin.x + b.pos.x, origin.y + b.pos.y, origin.z + b.pos.z))
        .collect();

    let mut placed = 0u32;
    let mut skipped = 0u32;
    let mut unknown = 0u32;
    let total = schem.blocks.len();
    let mut processed = 0usize;

    for b in schem.build_order() {
        if bot.kicked.is_some() {
            break;
        }
        processed += 1;
        // 진행률은 stderr 로(버퍼링 없이) — 긴 건축 중 상태 확인용
        if processed % 10 == 0 || processed == total {
            eprintln!("  …건축 {processed}/{total} (설치 {placed})");
        }
        let world = BlockPos::new(origin.x + b.pos.x, origin.y + b.pos.y, origin.z + b.pos.z);
        let Some(item) = blocks::item_id(&b.name) else {
            unknown += 1;
            continue;
        };

        // 도달 못 하면 받침 지면 칸으로 이동
        if !within_reach(bot.feet, world) {
            if let Some((sx, sz)) = stand_cell(bot.feet, world, origin.y, &occupied) {
                bot.walk_to(sx + 0.5, sz + 0.5)?;
            }
        }
        match bot.place_block(world, item)? {
            true => placed += 1,
            false => skipped += 1,
        }
    }

    bot.idle(Duration::from_millis(500))?;
    println!("\n=== 완료 ===");
    println!("설치 {placed} · 도달실패 {skipped} · 미등록블록 {unknown}");
    match &bot.kicked {
        Some(r) => println!("※ 도중 연결 종료: {r}"),
        None => println!("킥 없이 연결 유지됨 ✅"),
    }
    Ok(())
}

/// 지면(feet_y)에 서서 world 블록을 설치할 수 있는 칸을 찾는다.
/// 구조물 발밑/머리 칸은 피하고, 현재 위치에 가장 가까운 도달 가능 칸.
fn stand_cell(
    cur: Vec3,
    world: BlockPos,
    feet_y: i32,
    occupied: &HashSet<(i32, i32, i32)>,
) -> Option<(f64, f64)> {
    let mut best: Option<(f64, (f64, f64))> = None;
    for r in 1i32..=4 {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue; // 링의 테두리만
                }
                let cx = world.x + dx;
                let cz = world.z + dz;
                // 서 있을 칸의 발/머리가 구조물과 겹치면 제외
                if occupied.contains(&(cx, feet_y, cz)) || occupied.contains(&(cx, feet_y + 1, cz)) {
                    continue;
                }
                let feet = Vec3::new(cx as f64 + 0.5, feet_y as f64, cz as f64 + 0.5);
                if within_reach(feet, world) {
                    let d = cur.horiz_dist(feet);
                    if best.map_or(true, |(bd, _)| d < bd) {
                        best = Some((d, (feet.x, feet.z)));
                    }
                }
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map(|(_, c)| c)
}
