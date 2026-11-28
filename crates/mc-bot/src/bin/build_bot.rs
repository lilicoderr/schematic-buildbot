//! build-bot — builds a blueprint on an offline-mode Minecraft server.
//!
//!   build-bot [host:port] [name] [blueprint] [options]
//!
//! The bot walks on the ground (no flying, no speeding) and places supported blocks
//! first. Works on creative-mode servers (it puts blocks in hand via set-creative-slot).

#[path = "build_bot/ask.rs"]
mod ask;
#[path = "build_bot/build_args.rs"]
mod build_args;
#[path = "build_bot/build_cmd.rs"]
mod build_cmd;
#[path = "build_bot/build_plan.rs"]
mod build_plan;
#[path = "build_bot/plan_cmds.rs"]
mod plan_cmds;
#[path = "build_bot/run_cmd.rs"]
mod run_cmd;
#[path = "build_bot/ui/mod.rs"]
mod ui;

use mc_bot::builder::PlayClient;
use mc_bot::nav::{self, Terrain};
use mc_bot::placement::{self, Action};
use mc_bot::progress::Progress;
use mc_bot::scaffold::Site;
use mc_bot::schematic::{Block, Schematic};
use mc_bot::version::{self, Version};
use mc_bot::world::BlockPos;
use mc_bot::{blocks, fetch, i18n, tr};
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DEFAULT_BLUEPRINT: &str = "name Small cottage
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

fn usage() -> String {
    tr!(
        "\
build-bot — build a blueprint on a Minecraft offline-mode server

ONE COMMAND (download, plan, check the server, build, verify; run again to resume):
  build-bot build <file or URL> <host:port> [--name N] [--origin x,y,z] [--dry-run]

WEB PAGE on this computer (drop a schematic, 3D preview, build, stop):
  build-bot ui [--port N] [--no-open]

USAGE:
  build-bot [host:port] [name] [blueprint]   build (default 127.0.0.1:25566, BuildBot)
  build-bot x x <blueprint> --dry-run        validate offline, no server needed

  <blueprint> is a .schem / .litematic / .txt file or an http(s) URL to one.

OPTIONS:
  --dry-run            print size, block list, layer map and state report, then exit
  --delay-ms <N>       pause after each placement (default 25)
  --rate <N>           cap placements per second (overrides --delay-ms)
  --version <V>        Minecraft version: 1.20.4, 1.21, 1.21.1, 1.21.4, 26.3 (default 1.21.1)
  --verify             re-place blocks the server did not confirm (a report is always printed)
  --resume             continue an interrupted build from the progress file
  --progress-file <F>  progress file path (default .buildbot-progress)
  --lang <en|ko>       output language (also BUILDBOT_LANG)
  -h, --help           show this help

EXAMPLE:
  build-bot 127.0.0.1:25566 MyBot https://example.com/castle.schem",
        "\
build-bot — 설계도를 Minecraft 오프라인 서버에 건축

한 번에 짓기 (다운로드, 계획, 서버 점검, 건축, 검증. 다시 실행하면 이어서):
  build-bot build <파일 또는 URL> <host:port> [--name 이름] [--origin x,y,z] [--dry-run]

이 컴퓨터의 웹 화면 (설계도 끌어다 놓기, 3D 미리보기, 건축, 중지):
  build-bot ui [--port 번호] [--no-open]

사용법:
  build-bot [host:port] [이름] [설계도]       건축 (기본 127.0.0.1:25566, BuildBot)
  build-bot x x <설계도> --dry-run            서버 없이 설계도만 검증

  <설계도>는 .schem / .litematic / .txt 파일 또는 그 http(s) URL.

옵션:
  --dry-run            크기·블록 목록·층별 도면·상태 보고를 출력하고 종료
  --delay-ms <N>       설치마다 대기(기본 25)
  --rate <N>           초당 설치 수 상한 (--delay-ms 보다 우선)
  --version <V>        마인크래프트 버전: 1.20.4, 1.21, 1.21.1, 1.21.4, 26.3 (기본 1.21.1)
  --verify             서버가 확인하지 않은 블록을 다시 설치 (확인 보고는 항상 출력)
  --resume             중단된 건축을 진행 파일에서 이어서 진행
  --progress-file <F>  진행 파일 경로 (기본 .buildbot-progress)
  --lang <en|ko>       출력 언어 (환경변수 BUILDBOT_LANG)
  -h, --help           도움말

예시:
  build-bot 127.0.0.1:25566 MyBot https://example.com/castle.schem"
    )
}

#[derive(Default)]
struct Opts {
    positional: Vec<String>,
    dry_run: bool,
    delay_ms: Option<u64>,
    rate: Option<f64>,
    version: Option<String>,
    resume: bool,
    verify: bool,
    progress_file: Option<String>,
    lang: Option<String>,
    help: bool,
}

fn parse_opts(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |flag: &str| {
            it.next().cloned().ok_or_else(|| tr!("{} needs a value", "{} 에 값이 필요합니다", flag))
        };
        match a.as_str() {
            "-h" | "--help" => o.help = true,
            "--dry-run" | "--validate" => o.dry_run = true,
            "--resume" => o.resume = true,
            "--verify" => o.verify = true,
            "--delay-ms" => {
                o.delay_ms = Some(value("--delay-ms")?.parse().map_err(|_| tr!("--delay-ms must be a number", "--delay-ms 는 숫자여야 합니다"))?)
            }
            "--rate" => {
                let r: f64 = value("--rate")?.parse().map_err(|_| tr!("--rate must be a number", "--rate 는 숫자여야 합니다"))?;
                if r <= 0.0 {
                    return Err(tr!("--rate must be positive", "--rate 는 0보다 커야 합니다"));
                }
                o.rate = Some(r);
            }
            "--version" => o.version = Some(value("--version")?),
            "--progress-file" => o.progress_file = Some(value("--progress-file")?),
            "--lang" => o.lang = Some(value("--lang")?),
            s if s.starts_with("--") => return Err(tr!("unknown option {}", "알 수 없는 옵션 {}", s)),
            _ => o.positional.push(a.clone()),
        }
    }
    Ok(o)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(cmd) = args.first().map(String::as_str) {
        let rest = &args[1..];
        let lang = rest.iter().position(|a| a == "--lang").and_then(|i| rest.get(i + 1)).cloned();
        i18n::set(i18n::detect(lang.as_deref()));
        match cmd {
            "build" => return build_cmd::build_cmd(rest),
            "slice" => return plan_cmds::slice_cmd(rest),
            "simulate" => return plan_cmds::simulate_cmd(rest),
            "run" => return run_cmd::run_cmd(rest),
            "ui" => return ui::ui_cmd(rest),
            _ => {}
        }
    }
    // Language first, so even option errors are localized.
    let lang_flag = args.iter().position(|a| a == "--lang").and_then(|i| args.get(i + 1)).cloned();
    i18n::set(i18n::detect(lang_flag.as_deref()));

    let opts = parse_opts(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    if opts.help {
        println!("{}", usage());
        return;
    }
    let version = version::lookup(opts.version.as_deref().unwrap_or(version::DEFAULT.name))
        .unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2);
        });
    version::set_active(version); // item names are checked against this release

    let target = opts.positional.first().map(String::as_str).unwrap_or("127.0.0.1:25566");
    let name = opts.positional.get(1).map(String::as_str).unwrap_or("BuildBot");
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(25565)),
        None => (target.to_string(), 25565),
    };
    let schem = match opts.positional.get(2) {
        Some(src) => load_schematic(src).unwrap_or_else(|e| {
            eprintln!("{}", tr!("failed to load blueprint ({}): {}", "설계도 불러오기 실패({}): {}", src, e));
            std::process::exit(1);
        }),
        None => Schematic::parse(DEFAULT_BLUEPRINT).unwrap(),
    };

    if opts.dry_run {
        print_summary(&schem);
        return;
    }

    if let Err(e) = run(target, &host, port, name, &schem, &opts, version) {
        eprintln!("{}", tr!("[error] {}", "[오류] {}", e));
        std::process::exit(1);
    }
}

/// Load a blueprint from a local file or an http(s) URL ([`mc_bot::fetch`]).
pub(crate) fn load_schematic(src: &str) -> Result<Schematic, String> {
    if src.contains("://") {
        eprintln!("{}", tr!("downloading: {}", "다운로드: {}", src));
    }
    fetch::parse(&fetch::fetch(src)?)
}

/// Simulate placement offline: which requested states can be honoured?
fn state_report(schem: &Schematic) -> (usize, BTreeMap<String, usize>, usize) {
    let (order, _) = schem.plan_order();
    let (min, _) = schem.bounds();
    let mut placed: HashSet<BlockPos> = HashSet::new();
    let (mut honoured, mut skipped) = (0usize, 0usize);
    let mut lost: BTreeMap<String, usize> = BTreeMap::new();
    for b in order {
        let solid = |p: BlockPos| p.y < min.y || placed.contains(&p);
        match placement::plan(b, solid) {
            Action::Skip(_) => skipped += 1,
            Action::Place(plan) => {
                if !b.props.is_empty() && plan.unhonoured.is_empty() {
                    honoured += 1;
                }
                for k in plan.unhonoured {
                    *lost.entry(format!("{}:{}", b.name, k)).or_default() += 1;
                }
            }
        }
        placed.insert(b.pos);
    }
    (honoured, lost, skipped)
}

fn print_summary(schem: &Schematic) {
    let (min, max) = schem.bounds();
    println!("{}", tr!("=== blueprint check: '{}' ===", "=== 설계도 검증: '{}' ===", schem.name));
    println!(
        "{}",
        tr!(
            "size {}x{}x{} (x {}..{}, y {}..{}, z {}..{}), {} blocks",
            "크기 {}x{}x{} (x {}..{}, y {}..{}, z {}..{}), 총 {}블록",
            max.x - min.x + 1, max.y - min.y + 1, max.z - min.z + 1,
            min.x, max.x, min.y, max.y, min.z, max.z, schem.blocks.len()
        )
    );
    let mut unknown = Vec::new();
    println!("{}", tr!("block types:", "블록 종류:"));
    for (name, count) in schem.block_counts() {
        let ok = blocks::item_id(&name).is_some();
        if !ok {
            unknown.push(name.clone());
        }
        let tag = if ok { String::new() } else { tr!("  <- not registered (cannot place)", "  ← 미등록(설치 불가)") };
        println!("  {count:>5}  {name}{tag}");
    }
    if unknown.is_empty() {
        println!("{}", tr!("All blocks can be placed.", "모든 블록이 설치 가능합니다."));
    } else {
        println!(
            "{}",
            tr!("{} unregistered block type(s): {}", "미등록 블록 {}종: {}", unknown.len(), unknown.join(", "))
        );
    }
    print!("\n{}", schem.ascii_layers());
    print_state_and_anchor_report(schem);
}

fn print_state_and_anchor_report(schem: &Schematic) {
    let stateful = schem.stateful_count();
    if stateful > 0 {
        let (honoured, lost, skipped) = state_report(schem);
        println!(
            "\n{}",
            tr!(
                "block states: {} blocks carry states; {} will match exactly, {} second-half blocks are placed automatically",
                "블록 상태: 상태 보유 {}개, 정확히 재현 {}개, 자동 생성되는 두 번째 칸 {}개",
                stateful, honoured, skipped
            )
        );
        for (k, n) in lost {
            println!("  {}", tr!("{} x{}: default state (no click available)", "{} x{}: 기본 상태로 설치(클릭할 면 없음)", k, n));
        }
    }
    let un = schem.unanchored_blocks();
    if !un.is_empty() {
        println!(
            "\n{}",
            tr!(
                "warning: {} block(s) have no neighbour to rest against and would float",
                "경고: 받침이 될 이웃이 전혀 없어 공중에 뜨는 블록 {}개",
                un.len()
            )
        );
    }
}

fn place_delay(opts: &Opts) -> Duration {
    match (opts.rate, opts.delay_ms) {
        (Some(r), _) => Duration::from_secs_f64(1.0 / r),
        (None, Some(ms)) => Duration::from_millis(ms),
        _ => Duration::from_millis(25),
    }
}

/// Where to resume: the saved start index and origin when the progress file matches this blueprint.
fn resume_point(opts: &Opts, path: &Path, fingerprint: u64, total: usize) -> Option<Progress> {
    if !opts.resume {
        return None;
    }
    match Progress::load(path) {
        Some(p) if p.fingerprint == fingerprint && p.total == total => Some(p),
        _ => {
            println!(
                "{}",
                tr!("no matching progress file, starting from the beginning", "맞는 진행 파일이 없어 처음부터 시작합니다")
            );
            None
        }
    }
}

/// Upcoming blocks considered when choosing where to stand.
const LOOKAHEAD: usize = 48;

fn run(addr: &str, host: &str, port: u16, name: &str, schem: &Schematic, opts: &Opts, version: &Version) -> io::Result<()> {
    println!(
        "{}",
        tr!("=== connecting ({} as {}, Minecraft {}) ===", "=== 접속 ({} as {}, Minecraft {}) ===", addr, name, version.name)
    );
    let (mut bot, _join) = PlayClient::join_v(addr, host, port, name, version.protocol)?;
    if let Some(reason) = &bot.kicked {
        println!("{}", tr!("kicked: {}", "킥됨: {}", reason));
        return Ok(());
    }
    bot.place_delay = place_delay(opts);
    let dropped = bot.land()?;
    if dropped > 0.5 {
        println!("{}", tr!("joined {:.1} blocks above the ground, landed first", "지면보다 {:.1}칸 위에서 접속해 먼저 착지했습니다", dropped));
    }
    let spawn = bot.feet;
    println!("{}", tr!("spawn: ({:.1}, {:.1}, {:.1})", "스폰 위치: ({:.1}, {:.1}, {:.1})", spawn.x, spawn.y, spawn.z));

    let (order, _) = schem.plan_order();
    let total = order.len();
    let progress_path = PathBuf::from(opts.progress_file.as_deref().unwrap_or(".buildbot-progress"));
    let fingerprint = schem.fingerprint();

    let mut origin = BlockPos::new(spawn.x.floor() as i32 + 2, spawn.y.floor() as i32, spawn.z.floor() as i32 + 2);
    let mut start = 0usize;
    if let Some(p) = resume_point(opts, &progress_path, fingerprint, total) {
        start = p.done.min(total);
        origin = p.origin;
        println!("{}", tr!("resuming at block {}/{}", "이어서 진행: {}/{}", start, total));
    }
    let (min, max) = schem.bounds();
    println!(
        "{}",
        tr!(
            "blueprint '{}': {} blocks, size {}x{}x{}, origin ({},{},{})",
            "설계도 '{}': {}블록, 크기 {}x{}x{}, 원점 ({},{},{})",
            schem.name, total, max.x - min.x + 1, max.y - min.y + 1, max.z - min.z + 1, origin.x, origin.y, origin.z
        )
    );

    let at = |b: &Block| BlockPos::new(origin.x + b.pos.x, origin.y + b.pos.y, origin.z + b.pos.z);
    // Blocks from an earlier run already exist in the world.
    let mut placed: HashSet<BlockPos> = order.iter().take(start).map(|b| at(b)).collect();
    let mut site = Site::new(Terrain::new(origin.y, HashSet::new()));
    for b in order.iter().take(start) {
        terrain_add(&mut site.terrain, at(b), b);
    }

    let mut stats = Stats::default();
    let started = Instant::now();
    let first = start;
    let mut attempted: Vec<(Block, i32)> = Vec::new();
    let save = |idx: usize| {
        let _ = Progress { fingerprint, origin, done: idx, total }.save(&progress_path);
    };

    let positions: Vec<BlockPos> = order.iter().map(|b| at(b)).collect();
    let mut window_start = Instant::now();
    let mut window_timing = site.timing;
    let mut processed = start;
    for b in order.iter().skip(start) {
        if bot.kicked.is_some() {
            break;
        }
        processed += 1;
        if processed.is_multiple_of(25) {
            save(processed - 1);
        }
        if processed.is_multiple_of(100) || processed == total {
            let rate = (processed - first) as f64 / started.elapsed().as_secs_f64().max(0.001);
            let eta = (total - processed) as f64 / rate.max(0.001);
            eprintln!(
                "{}",
                tr!(
                    "  ...building {}/{} (placed {}) {:.1} blocks/s, ETA {}",
                    "  …건축 {}/{} (설치 {}) {:.1}블록/초, 남은 시간 {}",
                    processed, total, stats.done, rate, fmt_duration(eta)
                )
            );
            let (now, w) = (site.timing, window_start.elapsed().as_secs_f64().max(0.001));
            let d = |a: Duration, b: Duration| (a - b).as_secs_f64();
            eprintln!(
                "{}",
                tr!(
                    "     last 100: {:.1} blocks/s | walk {:.0}% pillar {:.0}% search {:.1}s | {} routes, {} steps, {} pillars",
                    "     최근 100: {:.1}블록/초 | 이동 {:.0}% 기둥 {:.0}% 탐색 {:.1}초 | 경로 {}회, {}걸음, 기둥 {}개",
                    100.0 / w,
                    100.0 * d(now.walk, window_timing.walk) / w,
                    100.0 * d(now.pillar, window_timing.pillar) / w,
                    d(now.search, window_timing.search),
                    now.routes - window_timing.routes,
                    now.steps - window_timing.steps,
                    now.pillars_built - window_timing.pillars_built
                )
            );
            window_start = Instant::now();
            window_timing = now;
        }
        let world = at(b);
        let Some(item) = blocks::item_id(&b.name) else {
            stats.unknown += 1;
            continue;
        };
        let solid = |p: BlockPos| p.y < origin.y || placed.contains(&p);
        let plan = match placement::plan(&Block { pos: world, ..(*b).clone() }, solid) {
            Action::Skip(_) => {
                stats.auto_half += 1;
                continue;
            }
            Action::Place(p) => p,
        };
        let t0 = Instant::now();
        site.approach(&mut bot, &positions[processed - 1..positions.len().min(processed - 1 + LOOKAHEAD)])?;
        let t1 = Instant::now();
        let ok = bot.place(world, item, &plan)?;
        stats.moving += t1 - t0;
        stats.placing += t1.elapsed();
        if ok {
            stats.done += 1;
            placed.insert(world);
            terrain_add(&mut site.terrain, world, b);
            attempted.push((Block { pos: world, ..(*b).clone() }, item));
            stats.note_defaults(&b.name, &plan.unhonoured);
        } else {
            stats.out_of_reach += 1;
        }
    }

    let left = site.finish(&mut bot)?;
    if left > 0 {
        println!("{}", tr!("{} scaffold pillar(s) could not be removed", "비계 기둥 {}개를 제거하지 못했습니다", left));
    }
    bot.idle(Duration::from_millis(500))?;
    if opts.verify {
        verify_pass(&mut bot, &attempted, &placed, &mut site, origin.y)?;
    }
    stats.confirmed = attempted.iter().filter(|(b, _)| bot.blocks.is_confirmed(b.pos)).count();
    stats.attempted = attempted.len();
    stats.elapsed = started.elapsed();
    stats.print();
    let t = site.timing;
    println!(
        "{}",
        tr!(
            "walking {:.0}s ({} routes, {} steps) | pillars {:.0}s ({} built, {} removed) | path search {:.1}s",
            "이동 {:.0}초 (경로 {}회, {}걸음) | 기둥 {:.0}초 (설치 {}, 제거 {}) | 경로 탐색 {:.1}초",
            t.walk.as_secs_f64(), t.routes, t.steps, t.pillar.as_secs_f64(), t.pillars_built, t.pillars_removed, t.search.as_secs_f64()
        )
    );
    match &bot.kicked {
        Some(r) => {
            save(processed.saturating_sub(1));
            println!(
                "{}",
                tr!(
                    "connection ended early: {} (re-run with --resume to continue)",
                    "※ 도중 연결 종료: {} (--resume 으로 이어서 진행 가능)",
                    r
                )
            );
        }
        None => {
            Progress::clear(&progress_path);
            println!("{}", tr!("stayed connected, no kick", "킥 없이 연결 유지됨"));
        }
    }
    Ok(())
}

#[derive(Default)]
struct Stats {
    done: u32,
    out_of_reach: u32,
    unknown: u32,
    auto_half: u32,
    default_state: u32,
    lost: BTreeMap<String, u32>,
    confirmed: usize,
    attempted: usize,
    elapsed: Duration,
    moving: Duration,
    placing: Duration,
}

impl Stats {
    fn note_defaults(&mut self, name: &str, unhonoured: &[String]) {
        if unhonoured.is_empty() {
            return;
        }
        self.default_state += 1;
        for k in unhonoured {
            *self.lost.entry(format!("{name}:{k}")).or_default() += 1;
        }
    }

    fn print(&self) {
        println!("\n{}", tr!("=== done ===", "=== 완료 ==="));
        println!(
            "{}",
            tr!(
                "placed {} | out of reach {} | unregistered {} | auto second halves {}",
                "설치 {} · 도달실패 {} · 미등록블록 {} · 자동 생성 {}",
                self.done, self.out_of_reach, self.unknown, self.auto_half
            )
        );
        println!(
            "{}",
            tr!(
                "server confirmed {}/{} placements, {} in {} ({:.1} blocks/s)",
                "서버가 확인한 설치 {}/{}, {} 걸림 {} ({:.1}블록/초)",
                self.confirmed,
                self.attempted,
                tr!("took", "소요"),
                fmt_duration(self.elapsed.as_secs_f64()),
                self.done as f64 / self.elapsed.as_secs_f64().max(0.001)
            )
        );
        println!(
            "{}",
            tr!(
                "time split: moving {:.0}s, placing {:.0}s",
                "시간 배분: 이동 {:.0}초, 설치 {:.0}초",
                self.moving.as_secs_f64(),
                self.placing.as_secs_f64()
            )
        );
        if self.default_state > 0 {
            println!(
                "{}",
                tr!(
                    "{} block(s) placed with the default state instead of the requested one:",
                    "요청한 상태 대신 기본 상태로 설치된 블록 {}개:",
                    self.default_state
                )
            );
            for (k, n) in &self.lost {
                println!("  {k} x{n}");
            }
        }
    }
}

fn fmt_duration(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m{:02}s", s / 60, s % 60),
        _ => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

fn terrain_add(terrain: &mut Terrain, world: BlockPos, b: &Block) {
    terrain.add_block((world.x, world.y, world.z), nav::is_standable_surface(&b.name, b.prop("type")));
}

/// Re-place every block the server never confirmed (up to two rounds).
fn verify_pass(
    bot: &mut PlayClient,
    attempted: &[(Block, i32)],
    placed: &HashSet<BlockPos>,
    site: &mut Site,
    feet_y: i32,
) -> io::Result<()> {
    for round in 1..=2 {
        let todo: Vec<&(Block, i32)> = attempted.iter().filter(|(b, _)| !bot.blocks.is_confirmed(b.pos)).collect();
        if todo.is_empty() {
            break;
        }
        println!("{}", tr!("verify: re-placing {} unconfirmed block(s), round {}", "검증: 확인 안 된 블록 {}개를 다시 설치합니다 ({}차)", todo.len(), round));
        for (b, item) in todo {
            bot.blocks.forget(b.pos);
            let solid = |p: BlockPos| p.y < feet_y || placed.contains(&p);
            if let Action::Place(plan) = placement::plan(b, solid) {
                site.approach(bot, std::slice::from_ref(&b.pos))?;
                bot.place(b.pos, *item, &plan)?;
            }
            if bot.kicked.is_some() {
                return Ok(());
            }
        }
        site.finish(bot)?;
        bot.idle(Duration::from_millis(500))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_overrides_delay() {
        let o = Opts { rate: Some(4.0), delay_ms: Some(999), ..Default::default() };
        assert_eq!(place_delay(&o), Duration::from_millis(250));
        let o = Opts { delay_ms: Some(40), ..Default::default() };
        assert_eq!(place_delay(&o), Duration::from_millis(40));
    }

    #[test]
    fn parses_flags_and_positionals() {
        let a: Vec<String> =
            ["host:1", "Bot", "x.schem", "--rate", "5", "--resume", "--lang", "ko"].map(String::from).to_vec();
        let o = parse_opts(&a).unwrap();
        assert_eq!(o.positional, vec!["host:1", "Bot", "x.schem"]);
        assert_eq!(o.rate, Some(5.0));
        assert!(o.resume);
        assert_eq!(o.lang.as_deref(), Some("ko"));
    }

    #[test]
    fn parses_verify() {
        let a: Vec<String> = ["x", "--verify"].map(String::from).to_vec();
        let o = parse_opts(&a).unwrap();
        assert!(o.verify);
        assert!(parse_opts(&["--fly".to_string()]).is_err(), "flying is not supported");
    }

    #[test]
    fn durations_are_compact() {
        assert_eq!(fmt_duration(7.0), "7s");
        assert_eq!(fmt_duration(125.0), "2m05s");
        assert_eq!(fmt_duration(7260.0), "2h01m");
    }

    #[test]
    fn rejects_bad_options() {
        assert!(parse_opts(&["--nope".to_string()]).is_err());
        assert!(parse_opts(&["--rate".to_string()]).is_err());
        assert!(parse_opts(&["--rate".to_string(), "0".to_string()]).is_err());
    }

    #[test]
    fn stats_collect_unhonoured_states() {
        let mut s = Stats::default();
        s.note_defaults("oak_stairs", &["half".to_string()]);
        s.note_defaults("oak_stairs", &["half".to_string()]);
        s.note_defaults("stone", &[]);
        assert_eq!(s.default_state, 2);
        assert_eq!(s.lost["oak_stairs:half"], 2);
    }
}
