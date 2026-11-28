//! `build-bot build <file-or-URL> <host:port>`: download, parse, plan, simulate, check the
//! server, build and verify, in one command. Running it again resumes.

use super::build_args::{self, Args};
use super::build_plan;
use super::plan_cmds::fail;
use mc_bot::builder::PlayClient;
use mc_bot::exec::{self, Report};
use mc_bot::exec_repair::Offset;
use mc_bot::job::Job;
use mc_bot::nav::{feet_cell, Terrain};
use mc_bot::plan::Plan;
use mc_bot::scaffold::Site;
use mc_bot::version::Version;
use mc_bot::world::BlockPos;
use mc_bot::{fetch, guard, preflight, run_state, site_check, survey, tr, version};
use std::collections::HashSet;
use std::time::Duration;

/// Farthest the bot walks from where it joins to an `--origin`.
const MAX_ORIGIN_WALK: i32 = 128;

pub fn build_cmd(raw: &[String]) {
    mc_bot::ui_link::listen();
    let mut raw = raw.to_vec();
    if raw.iter().any(|a| a == "--ask") {
        raw.retain(|a| a != "--ask");
        raw = super::ask::ask_console(&raw).unwrap_or_else(|e| fail(e));
    }
    let args = build_args::parse(&raw).unwrap_or_else(|e| fail(e));
    if let Err(e) = build(&args) {
        fail(e);
    }
}

fn build(args: &Args) -> Result<(), String> {
    if args.source.contains("://") {
        println!("{}", tr!("downloading: {}", "다운로드: {}", args.source));
    }
    let source = fetch::fetch(&args.source).map_err(|e| tr!("failed to load blueprint ({}): {}", "설계도 불러오기 실패({}): {}", args.source, e))?;

    // the release decides which item and block names exist, so it comes before parsing
    let server = args.server.as_deref().filter(|_| !args.dry_run);
    let version: &Version = match server {
        Some(addr) => preflight::detect_version(addr, args.version.as_deref())?,
        None => version::lookup(args.version.as_deref().unwrap_or(version::DEFAULT.name))?,
    };
    version::set_active(version);
    let schem = fetch::parse(&source).map_err(|e| tr!("cannot read the schematic: {}", "설계도를 읽을 수 없습니다: {}", e))?;
    let schem = args.transform.apply(&schem);
    if schem.blocks.is_empty() {
        return Err(tr!("the schematic has no blocks", "설계도에 블록이 없습니다"));
    }
    let unsupported = build_plan::unsupported(&schem);
    if !unsupported.is_empty() && !args.allow_partial && !args.dry_run {
        // fail before the (possibly long) planning
        let list: Vec<String> = unsupported.iter().map(|(n, c)| format!("{n} x{c}")).collect();
        return Err(tr!(
            "Minecraft {} items are missing for: {}\n  -> Nothing was built. Add --allow-partial to build everything else.",
            "마인크래프트 {} 에서 지원하지 않는 블록: {}\n  -> 아무것도 짓지 않았습니다. 나머지만 지으려면 --allow-partial 을 붙이세요.",
            version.name, list.join(", ")
        ));
    }

    let job = Job::new(&args.state_dir, &source.name, schem.fingerprint(), version.protocol, args.server.as_deref().unwrap_or(""), args.origin);
    if args.bots > 1 {
        return super::build_multi::build_multi(args, &schem, version, &job);
    }
    if !args.dry_run {
        if args.fresh {
            job.reset();
        }
        if job.done().exists() {
            println!(
                "{}",
                tr!(
                    "this build already finished ({}). Add --fresh to build it again.",
                    "이미 완료한 건축입니다 ({}). 다시 지으려면 --fresh 를 붙이세요.",
                    job.dir.display()
                )
            );
            return Ok(());
        }
    }
    guard::lower_priority();
    // a dry run always plans afresh and writes nothing
    let (plan, resuming) = build_plan::obtain((!args.dry_run).then_some(&job), &schem, version, args)?;
    build_plan::check_legal(&plan, &schem)?;
    build_plan::print_stats(&plan, &schem, version);
    let problems = build_plan::problems(&plan, &schem);
    for p in &problems {
        println!("  {p}");
    }
    if !problems.is_empty() && !args.allow_partial {
        return Err(tr!(
            "the plan is not clean (see above), so nothing is built. Add --allow-partial to build it anyway.",
            "계획에 문제가 있어(위 목록) 건축하지 않습니다. 그래도 지으려면 --allow-partial 을 붙이세요."
        ));
    }
    if problems.is_empty() {
        println!("{}", tr!("simulation: clean (every step legal, final blocks == schematic)", "시뮬레이션: 통과 (모든 단계 합법, 결과 == 설계도)"));
    }
    if args.dry_run {
        println!("{}", tr!("dry run: not connecting to any server", "드라이런: 서버에 접속하지 않습니다"));
        return Ok(());
    }
    if !resuming {
        job.save_plan(&plan.to_text()).map_err(|e| format!("{}: {e}", job.dir.display()))?;
    }
    online(args, &job, &plan, version, problems.is_empty())
}

/// Where the plan goes in the world, walking there first when `--origin` asks for it.
pub(super) fn place_origin(bot: &mut PlayClient, plan: &Plan, origin: (i32, i32, i32)) -> Result<Offset, String> {
    let o = plan.header.origin;
    let off = Offset { dx: origin.0 - o.0, dy: origin.1 - o.1, dz: origin.2 - o.2 };
    let (from, start) = (feet_cell(bot.feet), off.cell(plan.header.start));
    if from.1 != start.1 {
        return Err(tr!(
            "--origin y must be the height the bot stands at (flat ground): use --origin {},{},{}",
            "--origin 의 y 는 봇이 서 있는 높이(평지)여야 합니다: --origin {},{},{}",
            origin.0, from.1, origin.2
        ));
    }
    let distance = (from.0 - start.0).abs().max((from.2 - start.2).abs());
    if distance > MAX_ORIGIN_WALK {
        return Err(tr!(
            "--origin is {} blocks from where the bot joined (limit {}). Move the spawn point closer (/setworldspawn) or pick another origin.",
            "--origin 이 접속 위치에서 {}칸 떨어져 있습니다 (제한 {}). 스폰 지점을 옮기거나(/setworldspawn) 다른 위치를 고르세요.",
            distance, MAX_ORIGIN_WALK
        ));
    }
    let teleports = bot.teleports;
    let mut site = Site::new(Terrain::new(start.1, HashSet::new()));
    let arrived = from == start || site.go_to(bot, start).map_err(|e| e.to_string())?;
    bot.idle(Duration::from_millis(500)).map_err(|e| e.to_string())?;
    if !arrived || bot.teleports != teleports || feet_cell(bot.feet) != start {
        return Err(tr!("could not walk to the --origin; the ground on the way must be flat and clear", "--origin 까지 걸어갈 수 없습니다. 가는 길이 평평하고 비어 있어야 합니다"));
    }
    Ok(off)
}

fn online(args: &Args, job: &Job, plan: &Plan, version: &Version, clean: bool) -> Result<(), String> {
    let server = args.server.as_deref().unwrap_or_default();
    let io = |e: std::io::Error| e.to_string();
    let fingerprint = run_state::fingerprint(plan);
    let saved = run_state::load(&job.progress(), fingerprint);
    if saved.is_none() && job.progress().exists() {
        return Err(tr!("the progress file does not match the plan; use --fresh to start over", "진행 파일이 계획과 맞지 않습니다. 처음부터 하려면 --fresh"));
    }
    println!("{}", tr!("connecting to {} as {}...", "{} 에 {} 이름으로 접속 중...", server, args.name));
    let mut bot = preflight::join(server, &args.name, version)?;
    preflight::require_creative(&mut bot, &args.name, args.wait_creative, &mut |m| println!("{m}"))?;
    bot.place_delay = args.delay;
    bot.land().map_err(io)?;
    let offset = match (saved, args.origin) {
        (Some(s), _) => Some(s.offset),
        (None, Some(origin)) => Some(place_origin(&mut bot, plan, origin)?),
        (None, None) => None,
    };
    let offset = offset.unwrap_or_else(|| Offset::between(plan.header.start, feet_cell(bot.feet)));
    let from_step = saved.map_or(0, |s| s.step);
    // so that `undo` knows where this build stands, also long after it finished
    std::fs::write(job.offset(), format!("{} {} {}
", offset.dx, offset.dy, offset.dz)).map_err(io)?;
    // saved before the first step too: an early interruption must keep the same place
    run_state::save(&job.progress(), fingerprint, from_step, offset).map_err(io)?;
    if !args.walk {
        bot.set_sprint(true).map_err(io)?;
    }
    let keep = site_survey(&mut bot, args, job, plan, offset, saved.map(|s| s.step))?;
    let at = offset.pos(mc_bot::world::BlockPos::new(plan.header.origin.0, plan.header.origin.1, plan.header.origin.2));
    println!("{}", tr!("blueprint origin in the world: {},{},{}", "설계도 원점(월드 좌표): {},{},{}", at.x, at.y, at.z));
    if from_step > 0 {
        println!("{}", tr!("resuming after step {} of {}", "{}단계 다음부터 이어서 건축 (전체 {}단계)", from_step, plan.steps.len()));
    }
    let opts = exec::Opts { verify: true, from_step, offset: Some(offset), extras_file: Some(job.extras()), refit: Vec::new(), keep, demolition: None, strip: None };
    let total = plan.steps.len();
    let mut shown = from_step * 20 / total.max(1);
    let mut link = mc_bot::ui_link::Reporter::start(plan, from_step);
    let mut on_step = |done: usize, off: Offset| {
        link.step(done);
        if let Err(e) = run_state::save(&job.progress(), fingerprint, done, off) {
            eprintln!("  progress: {e}");
        }
        if done * 20 / total.max(1) > shown {
            shown = done * 20 / total.max(1);
            println!("  {}%", shown * 5);
        }
    };
    let resume_hint = tr!("run the same command again to resume", "같은 명령을 다시 실행하면 이어서 건축합니다");
    let report = exec::run(&mut bot, plan, &opts, &mut |m| eprintln!("  {m}"), &mut on_step).map_err(|e| match &bot.kicked {
        Some(r) => preflight::kicked_message(r, &args.name),
        None => tr!("the connection failed: {}\n  -> {}", "연결이 끊겼습니다: {}\n  -> {}", e, resume_hint),
    })?;
    super::run_cmd::report(plan, &report, bot.teleports);
    conclude(args, job, plan, &report, clean, &resume_hint)
}

/// Read the loaded chunks around the build and stop if it would run into existing blocks.
/// Returns the blocks beside the build that must never be touched.
fn site_survey(bot: &mut PlayClient, args: &Args, job: &Job, plan: &Plan, off: Offset, saved: Option<usize>) -> Result<HashSet<BlockPos>, String> {
    let extras: Vec<(i32, i32, i32)> = std::fs::read_to_string(job.extras()).unwrap_or_default().lines().filter_map(|l| {
        let mut v = l.split_whitespace().filter_map(|n| n.parse().ok());
        Some((v.next()?, v.next()?, v.next()?))
    }).collect();
    let area = survey::area(plan, off, saved, &extras)?;
    let found = site_check::check(bot, &area, site_check::CHUNK_WAIT).map_err(|e| e.to_string())?;
    if bot.chunks.unreadable > 0 {
        println!("{}", tr!("{} chunk packet(s) could not be decoded", "해독하지 못한 청크 패킷 {}개", bot.chunks.unreadable));
    }
    if found.is_clear() {
        println!("{}", tr!("site survey: clear", "부지 조사: 이상 없음"));
        return Ok(found.keep);
    }
    for line in found.report(&area) {
        println!("{line}");
    }
    if !args.force {
        return Err(tr!(
            "the site is not clear, so nothing was built.
  -> Pick another place with --origin, clear the blocks listed above, or add --force to build anyway (blocks inside the build area are then overwritten).",
            "부지가 비어 있지 않아 건축하지 않았습니다.
  -> --origin 으로 다른 곳을 고르거나 위에 나온 블록을 치우세요. 그래도 지으려면 --force 를 붙이세요 (건축 영역 안의 블록은 덮어씁니다)."
        ));
    }
    println!("{}", tr!("--force: building anyway", "--force: 그래도 건축합니다"));
    Ok(found.keep)
}

fn conclude(args: &Args, job: &Job, plan: &Plan, report: &Report, clean: bool, resume_hint: &str) -> Result<(), String> {
    if let Some(reason) = &report.kicked {
        return Err(preflight::kicked_message(reason, &args.name));
    }
    if report.steps_done < plan.steps.len() || report.aborted.is_some() {
        return Err(tr!("the build stopped early; {}", "건축이 중간에 멈췄습니다. {}", resume_hint));
    }
    job.finish().map_err(|e| e.to_string())?;
    let wrong = report.missing.len() + report.differs.len();
    if wrong > 0 && clean {
        return Err(tr!("finished, but verification found {} wrong block(s) (listed above)", "건축은 끝났지만 검증에서 다른 블록 {}개가 발견되었습니다 (위 목록)", wrong));
    }
    println!(
        "{}",
        if clean {
            tr!("done: verified, the server reported no block that differs from the schematic", "완료: 검증 결과 서버가 보고한 블록 중 설계도와 다른 것이 없습니다")
        } else {
            tr!("done (partial build: see the problems listed before the build)", "완료 (부분 건축: 시작 전에 나열한 문제 참고)")
        }
    );
    Ok(())
}
