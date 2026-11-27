//! `build-bot run <plan> [host:port]`: replay a plan on a server.

use super::plan_cmds::{fail, flag, read_plan};
use mc_bot::builder::PlayClient;
use mc_bot::exec::{self, Report};
use mc_bot::exec_repair::Offset;
use mc_bot::plan::{Plan, Step};
use mc_bot::run_state;
use mc_bot::tr;
use mc_bot::world::BlockPos;
use std::path::PathBuf;
use std::time::Duration;

/// `run <plan> [host:port] [--name N] [--rate R | --delay-ms N] [--walk] [--verify] [--resume]
/// [--refit x,y,z;...]`
pub fn run_cmd(args: &[String]) {
    let Some(path) = args.first() else { fail(tr!("usage: run <plan.bbplan> [host:port]", "사용법: run <plan.bbplan> [host:port]")) };
    let plan = read_plan(path);
    let target = args.get(1).filter(|a| !a.starts_with("--")).map(String::as_str).unwrap_or("127.0.0.1:25566");
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(25565)),
        None => (target.to_string(), 25565),
    };
    let name = flag(args, "--name").unwrap_or_else(|| "BuildBot".into());
    let delay = match (flag(args, "--rate"), flag(args, "--delay-ms")) {
        (Some(r), _) => Duration::from_secs_f64(1.0 / r.parse::<f64>().unwrap_or(40.0).max(0.1)),
        (None, Some(ms)) => Duration::from_millis(ms.parse().unwrap_or(10)),
        _ => Duration::from_millis(10),
    };
    let progress = PathBuf::from(flag(args, "--progress-file").unwrap_or_else(|| format!("{path}.progress")));
    let fingerprint = run_state::fingerprint(&plan);
    let resume = if args.iter().any(|a| a == "--resume") { run_state::load(&progress, fingerprint) } else { None };

    mc_bot::guard::lower_priority();
    let (mut bot, _) = PlayClient::join_v(target, &host, port, &name, plan.header.protocol)
        .unwrap_or_else(|e| fail(format!("{target}: {e}")));
    if let Some(r) = &bot.kicked {
        fail(tr!("kicked: {}", "킥됨: {}", r));
    }
    bot.place_delay = delay;
    bot.land().unwrap_or_else(|e| fail(e.to_string()));
    if !args.iter().any(|a| a == "--walk") {
        bot.set_sprint(true).unwrap_or_else(|e| fail(e.to_string()));
    }
    println!("{}", tr!("running {} steps from ({:.1}, {:.1}, {:.1})", "{}단계 실행, 시작 ({:.1}, {:.1}, {:.1})", plan.steps.len(), bot.feet.x, bot.feet.y, bot.feet.z));
    let opts = exec::Opts {
        verify: args.iter().any(|a| a == "--verify"),
        from_step: resume.as_ref().map_or(0, |r| r.step),
        offset: resume.map(|r| r.offset),
        extras_file: Some(PathBuf::from(format!("{}.extras", progress.display()))),
        refit: flag(args, "--refit").map(|s| parse_cells(&s)).unwrap_or_default(),
    };
    if opts.from_step > 0 {
        println!("{}", tr!("resuming after step {}", "{}단계 다음부터 이어서 실행", opts.from_step));
    }
    let mut say = |m: &str| eprintln!("  {m}");
    // saved after every step: replaying already finished moves or breaks would walk over air
    let mut on_step = |done: usize, off: Offset| {
        let _ = run_state::save(&progress, fingerprint, done, off);
    };
    let r = exec::run(&mut bot, &plan, &opts, &mut say, &mut on_step).unwrap_or_else(|e| fail(e.to_string()));
    report(&plan, &r, bot.teleports);
    if r.steps_done == plan.steps.len() {
        let _ = std::fs::remove_file(&progress);
    }
}

/// `x,y,z;x,y,z` in plan coordinates.
fn parse_cells(list: &str) -> Vec<BlockPos> {
    list.split(';')
        .map(|c| {
            let v: Vec<i32> = c.split(',').filter_map(|n| n.trim().parse().ok()).collect();
            if v.len() != 3 {
                fail(tr!("--refit wants x,y,z;x,y,z", "--refit 형식: x,y,z;x,y,z"));
            }
            BlockPos::new(v[0], v[1], v[2])
        })
        .collect()
}

fn report(plan: &Plan, r: &Report, teleports: u32) {
    let expected = plan.steps.iter().filter(|s| matches!(s, Step::Place { .. })).count();
    println!("{}", tr!("=== run finished ===", "=== 실행 종료 ==="));
    println!(
        "{}",
        tr!(
            "steps {}/{} | placed {}/{} | refused {} | re-placed {} | still missing {} | {:.0}s ({:.1} blocks/s) | kicks {} | teleports {}",
            "단계 {}/{} | 설치 {}/{} | 거부 {} | 재설치 {} | 누락 {} | {:.0}초 ({:.1}블록/초) | 킥 {} | 텔레포트 {}",
            r.steps_done, plan.steps.len(), r.placed, expected, r.refused, r.repaired, r.missing.len(),
            r.elapsed.as_secs_f64(), r.placed as f64 / r.elapsed.as_secs_f64().max(0.001), u8::from(r.kicked.is_some()), teleports
        )
    );
    println!("{}", tr!("resynchronisations after server teleports: {}", "서버 텔레포트 후 재동기화: {}", r.resyncs));
    if !r.windows.is_empty() {
        let w: Vec<String> = r.windows.iter().map(|x| format!("{x:.1}")).collect();
        println!("{}", tr!("blocks/s per 100 placements: {}", "100개당 블록/초: {}", w.join(" ")));
    }
    let names = ["move", "pillar", "support", "place", "break", "checkpoint"];
    let split: Vec<String> = (0..6).map(|k| format!("{} {:.0}s/{}", names[k], r.spent[k], r.counts[k])).collect();
    println!("{}", tr!("time per step kind: {}", "단계별 시간: {}", split.join(", ")));
    for p in r.missing.iter().take(20) {
        println!("  missing: ({}, {}, {})", p.x, p.y, p.z);
    }
    println!("{}", tr!("server-reported states: {} differ from the plan", "서버가 보고한 블록 상태: 계획과 다른 것 {}개", r.differs.len()));
    for (p, want, how) in r.differs.iter().take(20) {
        println!("  differs: ({}, {}, {}) {want}: {how}", p.x, p.y, p.z);
    }
    if let Some(a) = &r.aborted {
        println!("aborted: {a}");
    }
    if let Some(k) = &r.kicked {
        println!("kicked: {k}");
    }
}
