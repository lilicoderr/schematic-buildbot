//! `build-bot slice | simulate | run`: the slicer pipeline commands.

use mc_bot::plan::Plan;
use mc_bot::schematic::Schematic;
use mc_bot::{plan_parse, sim, slice, tr};
use std::time::Instant;

fn fail(msg: String) -> ! {
    eprintln!("{}", tr!("[error] {}", "[오류] {}", msg));
    std::process::exit(1);
}

fn load(src: &str) -> Schematic {
    super::load_schematic(src).unwrap_or_else(|e| fail(tr!("failed to load blueprint ({}): {}", "설계도 불러오기 실패({}): {}", src, e)))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn parse_origin(s: &str) -> Option<(i32, i32, i32)> {
    let v: Vec<i32> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
    (v.len() == 3).then(|| (v[0], v[1], v[2]))
}

pub fn read_plan(path: &str) -> Plan {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| fail(format!("{path}: {e}")));
    plan_parse::parse(&text).unwrap_or_else(|e| fail(format!("{path}: {e}")))
}

/// `slice <schematic> -o plan.bbplan [--origin x,y,z] [--version V]`
pub fn slice_cmd(args: &[String]) {
    let Some(src) = args.first() else { fail(tr!("slice needs a schematic", "slice 에는 설계도가 필요합니다")) };
    let out = flag(args, "-o").unwrap_or_else(|| "plan.bbplan".into());
    let origin = flag(args, "--origin").map(|s| parse_origin(&s).unwrap_or_else(|| fail("--origin needs x,y,z".into()))).unwrap_or((2, 64, 2));
    let version = mc_bot::version::lookup(&flag(args, "--version").unwrap_or_else(|| mc_bot::version::DEFAULT.name.to_string()))
        .unwrap_or_else(|e| fail(e.to_string()));
    let schem = load(src);
    let t = Instant::now();
    let plan = slice::slice(&schem, origin, version.protocol).unwrap_or_else(|e| fail(e));
    std::fs::write(&out, plan.to_text()).unwrap_or_else(|e| fail(format!("{out}: {e}")));
    println!("{}", tr!("sliced '{}' in {:.1}s -> {}", "'{}' 슬라이스 {:.1}초 -> {}", schem.name, t.elapsed().as_secs_f64(), out));
    for (k, v) in &plan.header.stats {
        println!("  {k}: {v}");
    }
    let mut hist: std::collections::BTreeMap<&str, usize> = Default::default();
    for (_, n) in &plan.header.unreachable {
        *hist.entry(n.rsplit_once('/').map_or("?", |x| x.1)).or_default() += 1;
    }
    for (reason, n) in &hist {
        println!("  unreachable by reason: {reason} x{n}");
    }
    for (p, n) in plan.header.unreachable.iter().take(10) {
        println!("  unreachable: ({}, {}, {}) {n}", p.x, p.y, p.z);
    }
    println!("{}", tr!("simulation: passed (every step legal, final world == schematic)", "시뮬레이션: 통과 (모든 단계 합법, 최종 월드 == 설계도)"));
}

/// `simulate <plan> <schematic>`
pub fn simulate_cmd(args: &[String]) {
    let (Some(path), Some(src)) = (args.first(), args.get(1)) else {
        fail(tr!("usage: simulate <plan.bbplan> <schematic>", "사용법: simulate <plan.bbplan> <설계도>"))
    };
    let plan = read_plan(path);
    let schem = load(src);
    if plan.header.schematic_hash != schem.fingerprint() {
        fail(tr!("the plan was cut from a different schematic", "이 계획은 다른 설계도에서 만들어졌습니다"));
    }
    match sim::simulate(&plan, &schem) {
        Ok(r) => println!(
            "{}",
            tr!(
                "simulation passed: {} placements, {} moves, {} scaffold blocks, {} breaks, {} checkpoints",
                "시뮬레이션 통과: 설치 {}, 이동 {}, 비계 {}, 제거 {}, 체크포인트 {}",
                r.placed, r.moves, r.pillars, r.breaks, r.checkpoints
            )
        ),
        Err(e) => fail(format!("step {}: {}", e.step + 1, e.msg)),
    }
}

/// `run <plan> [host:port] [--name N] [--rate R | --delay-ms N] [--verify]`
pub fn run_cmd(args: &[String]) {
    use mc_bot::builder::PlayClient;
    let Some(path) = args.first() else { fail(tr!("usage: run <plan.bbplan> [host:port]", "사용법: run <plan.bbplan> [host:port]")) };
    let plan = read_plan(path);
    let target = args.get(1).filter(|a| !a.starts_with("--")).map(String::as_str).unwrap_or("127.0.0.1:25566");
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(25565)),
        None => (target.to_string(), 25565),
    };
    let name = flag(args, "--name").unwrap_or_else(|| "BuildBot".into());
    let delay = match (flag(args, "--rate"), flag(args, "--delay-ms")) {
        (Some(r), _) => std::time::Duration::from_secs_f64(1.0 / r.parse::<f64>().unwrap_or(40.0).max(0.1)),
        (None, Some(ms)) => std::time::Duration::from_millis(ms.parse().unwrap_or(25)),
        _ => std::time::Duration::from_millis(25),
    };
    let (mut bot, _) = PlayClient::join_v(target, &host, port, &name, plan.header.protocol)
        .unwrap_or_else(|e| fail(format!("{target}: {e}")));
    if let Some(r) = &bot.kicked {
        fail(tr!("kicked: {}", "킥됨: {}", r));
    }
    bot.place_delay = delay;
    if let Err(e) = bot.land() {
        fail(e.to_string());
    }
    println!("{}", tr!("running {} steps from ({:.1}, {:.1}, {:.1})", "{}단계 실행, 시작 ({:.1}, {:.1}, {:.1})", plan.steps.len(), bot.feet.x, bot.feet.y, bot.feet.z));
    let opts = mc_bot::exec::Opts { verify: args.iter().any(|a| a == "--verify") };
    let mut say = |m: &str| eprintln!("  {m}");
    let r = mc_bot::exec::run(&mut bot, &plan, &opts, &mut say).unwrap_or_else(|e| fail(e.to_string()));
    let placed_expected = plan.steps.iter().filter(|s| matches!(s, mc_bot::plan::Step::Place { .. })).count();
    println!("{}", tr!("=== run finished ===", "=== 실행 종료 ==="));
    println!(
        "{}",
        tr!(
            "steps {}/{} | placed {}/{} | refused {} | re-placed {} | still missing {} | {:.0}s ({:.1} blocks/s) | kicks {} | teleports {}",
            "단계 {}/{} | 설치 {}/{} | 거부 {} | 재설치 {} | 누락 {} | {:.0}초 ({:.1}블록/초) | 킥 {} | 텔레포트 {}",
            r.steps_done, plan.steps.len(), r.placed, placed_expected, r.refused, r.repaired, r.missing.len(),
            r.elapsed.as_secs_f64(), r.placed as f64 / r.elapsed.as_secs_f64().max(0.001), u8::from(r.kicked.is_some()), bot.teleports
        )
    );
    if !r.windows.is_empty() {
        let w: Vec<String> = r.windows.iter().map(|x| format!("{x:.1}")).collect();
        println!("{}", tr!("blocks/s per 100 placements: {}", "100개당 블록/초: {}", w.join(" ")));
    }
    for p in r.missing.iter().take(20) {
        println!("  missing: ({}, {}, {})", p.x, p.y, p.z);
    }
    if let Some(a) = &r.aborted {
        println!("aborted: {a}");
    }
    if let Some(k) = &r.kicked {
        println!("kicked: {k}");
    }
}
