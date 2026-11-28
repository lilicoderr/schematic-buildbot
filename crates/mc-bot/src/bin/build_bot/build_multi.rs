//! `build-bot build ... --bots N`: several bots build the same schematic together.

use super::build_args::Args;
use mc_bot::job::Job;
use mc_bot::multi::{files, joint, partition, runner, MultiPlan};
use mc_bot::schematic::Schematic;
use mc_bot::version::Version;
use mc_bot::{guard, tr};

/// Plan coordinates of the blueprint origin, as in the single-bot plan.
const PLAN_ORIGIN: (i32, i32, i32) = (2, 64, 2);

fn plan(args: &Args, schem: &Schematic, version: &Version, dir: &std::path::Path) -> Result<MultiPlan, String> {
    if dir.join("multi.txt").exists() {
        return files::load(dir);
    }
    println!("{}", tr!("planning {} blocks for {} bots...", "{}블록을 봇 {}대용으로 계획 중...", schem.blocks.len(), args.bots));
    let multi = partition::partition(schem, PLAN_ORIGIN, version.protocol, args.bots, &mut |m| eprintln!("{m}"))?;
    if !args.dry_run {
        files::save(&multi, dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(multi)
}

pub fn build_multi(args: &Args, schem: &Schematic, version: &'static Version, job: &Job) -> Result<(), String> {
    let dir = job.dir.join(format!("bots{}", args.bots));
    let done = dir.join("done");
    if args.fresh && !args.dry_run {
        let _ = std::fs::remove_dir_all(&dir);
    }
    if done.exists() && !args.dry_run {
        println!("{}", tr!("this build already finished ({}). Add --fresh to build it again.", "이미 완료한 건축입니다 ({}). 다시 지으려면 --fresh 를 붙이세요.", dir.display()));
        return Ok(());
    }
    guard::lower_priority();
    let multi = plan(args, schem, version, &dir)?;
    let report = joint::check(&multi, schem, PLAN_ORIGIN)?;
    let single = mc_bot::slice::slice(schem, PLAN_ORIGIN, version.protocol).ok().map(|p| mc_bot::multi::est_seconds(&p));
    println!(
        "{}",
        tr!(
            "{} bots, {} phase(s), {} placements, {} temporary blocks; estimated {:.0} s{}",
            "봇 {}대, {}단계, 설치 {}개, 임시 블록 {}개, 예상 {:.0}초{}",
            multi.bots, multi.phases.len(), report.placed, report.scaffold, multi.est_seconds(),
            single.map_or(String::new(), |s| format!(" (one bot: {s:.0} s)"))
        )
    );
    println!("{}", tr!("joint simulation: clean ({} interleavings, every step legal, no bot in another's cells, result == schematic)", "공동 시뮬레이션: 통과 ({}가지 순서, 모든 단계 합법, 다른 봇 칸 침범 없음, 결과 == 설계도)", report.interleavings));
    if args.dry_run {
        println!("{}", tr!("dry run: not connecting to any server", "드라이런: 서버에 접속하지 않습니다"));
        return Ok(());
    }
    if args.name.len() + 1 > 16 {
        return Err(tr!("--name may have at most 15 characters with --bots (a number is added)", "--bots 와 함께 쓰는 --name 은 15자까지입니다 (숫자가 붙습니다)"));
    }
    let cfg = runner::Config {
        server: args.server.clone().unwrap_or_default(),
        names: (1..=multi.bots).map(|i| format!("{}{i}", args.name)).collect(),
        version,
        delay: args.delay,
        walk: args.walk,
        wait_creative: args.wait_creative,
        state_dir: dir.clone(),
        origin: args.origin,
    };
    let out = runner::run(&multi, &cfg)?;
    for (phase, bot, r) in &out.reports {
        println!("  phase {} bot {}: {} placed, {} repaired, {} missing, {} differing, {:.0} s", phase + 1, bot + 1, r.placed, r.repaired, r.missing.len(), r.differs.len(), r.elapsed.as_secs_f64());
    }
    if let Some((bot, e)) = out.errors.first() {
        return Err(tr!("bot {} failed: {}\n  -> run the same command again to resume", "봇 {} 실패: {}\n  -> 같은 명령을 다시 실행하면 이어서 건축합니다", bot + 1, e));
    }
    if !out.clean() {
        return Err(tr!("finished, but verification found wrong blocks (listed above)", "건축은 끝났지만 검증에서 다른 블록이 발견되었습니다"));
    }
    std::fs::write(&done, "complete\n").map_err(|e| e.to_string())?;
    println!("{}", tr!("done in {:.0} s: verified by every bot, no block differs from the schematic", "{:.0}초에 완료: 모든 봇이 검증, 설계도와 다른 블록 없음", out.elapsed.as_secs_f64()));
    Ok(())
}
