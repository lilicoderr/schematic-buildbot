//! `build-bot slice | simulate | run`: the slicer pipeline commands.

use mc_bot::plan::Plan;
use mc_bot::schematic::Schematic;
use mc_bot::{plan_parse, sim, slice, tr};
use std::time::Instant;

pub fn fail(msg: String) -> ! {
    eprintln!("{}", tr!("[error] {}", "[오류] {}", msg));
    std::process::exit(1);
}

fn load(src: &str) -> Schematic {
    super::load_schematic(src).unwrap_or_else(|e| fail(tr!("failed to load blueprint ({}): {}", "설계도 불러오기 실패({}): {}", src, e)))
}

pub fn flag(args: &[String], name: &str) -> Option<String> {
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
    mc_bot::guard::lower_priority();
    let secs: u64 = flag(args, "--time-limit").and_then(|v| v.parse().ok()).unwrap_or(180);
    let mb: usize = flag(args, "--memory-limit-mb").and_then(|v| v.parse().ok()).unwrap_or(2048);
    let mut budget = mc_bot::guard::Budget::new(std::time::Duration::from_secs(secs), mb << 20);
    let plan = slice::slice_with(&schem, origin, version.protocol, &mut budget).unwrap_or_else(|e| fail(e));
    std::fs::write(&out, plan.to_text()).unwrap_or_else(|e| fail(format!("{out}: {e}")));
    println!("{}", tr!("sliced '{}' in {:.1}s -> {}", "'{}' 슬라이스 {:.1}초 -> {}", schem.name, t.elapsed().as_secs_f64(), out));
    let t = slice::last_times();
    println!("  planning time: tour {:.1}s, approach/path search {:.1}s, seal checks {:.1}s, support chains {:.1}s, cleanup {:.1}s, simulation {:.1}s", t.tour.as_secs_f64(), t.approach.as_secs_f64(), t.sealing.as_secs_f64(), t.chain.as_secs_f64(), t.cleanup.as_secs_f64(), t.simulate.as_secs_f64());
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
