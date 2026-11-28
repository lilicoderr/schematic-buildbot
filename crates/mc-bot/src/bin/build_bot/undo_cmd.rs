//! `build-bot undo <plan.bbplan> <host:port>`: take down what a build placed, and only that.

use super::build_cmd::place_origin;
use super::plan_cmds::{fail, flag, read_plan};
use mc_bot::exec::{self, Opts};
use mc_bot::exec_repair::Offset;
use mc_bot::nav::Cell;
use mc_bot::schematic::Block;
use mc_bot::survey::{classify, Kind, MARGIN};
use mc_bot::world::BlockPos;
use mc_bot::{preflight, run_state, site_check, tr, undo, version};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn num3(s: &str) -> Option<(i32, i32, i32)> {
    let v: Vec<i32> = s.split([',', ' ']).filter(|p| !p.is_empty()).map(|p| p.trim().parse().ok()).collect::<Option<_>>()?;
    (v.len() == 3).then(|| (v[0], v[1], v[2]))
}

pub fn undo_cmd(args: &[String]) {
    if let Err(e) = undo(args) {
        fail(e);
    }
}

fn undo(args: &[String]) -> Result<(), String> {
    let usage = || tr!("usage: build-bot undo <plan.bbplan> <host:port> [--name N] [--origin x,y,z] [--delay-ms N]", "사용법: build-bot undo <plan.bbplan> <host:port> [--name 이름] [--origin x,y,z] [--delay-ms N]");
    let pos: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !args[i - 1].starts_with("--") || args[i - 1] == "--walk")).map(|x| x.1).collect();
    let (path, server) = match pos[..] {
        [p, s] => (p.as_str(), s.as_str()),
        _ => return Err(usage()),
    };
    let name = flag(args, "--name").unwrap_or_else(|| "BuildBot".into());
    let version = version::by_protocol(read_plan(path).header.protocol).ok_or_else(|| tr!("the plan's Minecraft release is not supported", "계획의 마인크래프트 버전을 지원하지 않습니다"))?;
    version::set_active(version);
    let plan = read_plan(path);
    let dir = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let (progress, offset_file) = (dir.join("plan.progress"), dir.join("plan.offset"));
    let saved = run_state::load(&progress, run_state::fingerprint(&plan));
    let off = match (&saved, flag(args, "--origin")) {
        (Some(s), _) => s.offset,
        (None, Some(o)) => {
            let o = num3(&o).ok_or_else(|| tr!("--origin needs x,y,z", "--origin 은 x,y,z 형식입니다"))?;
            let p = plan.header.origin;
            Offset { dx: o.0 - p.0, dy: o.1 - p.1, dz: o.2 - p.2 }
        }
        (None, None) => std::fs::read_to_string(&offset_file).ok().and_then(|t| num3(&t)).map(|(dx, dy, dz)| Offset { dx, dy, dz }).ok_or_else(|| {
            tr!(
                "do not know where this build stands (no {} next to the plan).\n  -> give the world position of the blueprint's (0,0,0) corner: --origin x,y,z",
                "이 건축이 어디에 있는지 모릅니다 (계획 옆에 {} 없음).\n  -> 설계도 (0,0,0) 모서리의 월드 좌표를 --origin x,y,z 로 주세요",
                offset_file.display()
            )
        })?,
    };
    // what the bot placed: the plan's finished blocks, or those of the saved step; plus loose scaffold
    let mut placed = undo::placed_blocks(&plan, saved.as_ref().map(|s| s.step))?;
    for l in std::fs::read_to_string(dir.join("plan.progress.extras")).unwrap_or_default().lines() {
        if let Some((x, y, z)) = num3(l) {
            placed.entry((x - off.dx, y - off.dy, z - off.dz)).or_insert_with(|| "cobblestone".into());
        }
    }
    let world = |c: Cell| off.pos(BlockPos::new(c.0, c.1, c.2));
    println!("{}", tr!("connecting to {} as {}...", "{} 에 {} 이름으로 접속 중...", server, name));
    let mut bot = preflight::join(server, &name, version)?;
    preflight::require_creative(&mut bot, &name, Duration::from_secs(flag(args, "--wait-creative").and_then(|v| v.parse().ok()).unwrap_or(120)), &mut |m| println!("{m}"))?;
    bot.place_delay = Duration::from_millis(flag(args, "--delay-ms").and_then(|v| v.parse().ok()).unwrap_or(10));
    let io = |e: std::io::Error| e.to_string();
    bot.land().map_err(io)?;
    let o = plan.header.origin;
    place_origin(&mut bot, &plan, (o.0 + off.dx, o.1 + off.dy, o.2 + off.dz))?;
    bot.set_sprint(true).map_err(io)?;

    let (lo, hi) = bounds(placed.keys().map(|&c| world(c)));
    let columns: Vec<(i32, i32)> = ((lo.x - MARGIN) >> 4..=(hi.x + MARGIN) >> 4).flat_map(|x| ((lo.z - MARGIN) >> 4..=(hi.z + MARGIN) >> 4).map(move |z| (x, z))).collect();
    site_check::wait_for_chunks(&mut bot, &columns, site_check::CHUNK_WAIT).map_err(io)?;
    bot.idle(Duration::from_millis(300)).map_err(io)?;
    let ground = plan.header.origin.1;
    let sorted = undo::sort(&placed, ground, &|c| bot.chunks.state(world(c)));
    if sorted.unknown > 0 {
        return Err(tr!("{} cells are in chunks the server did not send; nothing was removed", "{}칸이 서버가 보내지 않은 청크에 있습니다. 아무것도 제거하지 않았습니다", sorted.unknown));
    }
    println!("{}", tr!("to remove: {} block(s) | already gone: {} | changed by someone else, kept: {}", "제거할 블록 {}개 | 이미 없음 {}개 | 다른 블록으로 바뀌어 그대로 둠 {}개", sorted.ours.len(), sorted.gone, sorted.changed.len()));
    for (c, now) in sorted.changed.iter().take(10) {
        let w = world(*c);
        println!("  {}", tr!("kept {},{},{}: {} stands there now", "유지 {},{},{}: 지금은 {}", w.x, w.y, w.z, now));
    }
    // every other block around the site stays, and is in the way
    let mut foreign: HashMap<Cell, String> = HashMap::new();
    for y in ground + off.dy..=hi.y + MARGIN {
        for z in lo.z - MARGIN..=hi.z + MARGIN {
            for x in lo.x - MARGIN..=hi.x + MARGIN {
                let c = (x - off.dx, y - off.dy, z - off.dz);
                if sorted.ours.contains_key(&c) {
                    continue;
                }
                if let Some((n, props)) = site_check::block_at(&bot, BlockPos::new(x, y, z)).filter(|(n, p)| classify(n, p) != Kind::Free) {
                    foreign.insert(c, Block { pos: BlockPos::new(0, 0, 0), name: n, props }.state_string());
                }
            }
        }
    }
    if !sorted.ours.is_empty() {
        let plan_undo = undo::plan(&plan.header, &sorted.ours, &foreign)?;
        println!("{}", tr!("undo plan: {} steps, checked by the simulator", "철거 계획: {}단계, 시뮬레이터 검사 통과", plan_undo.plan.steps.len()));
        let opts = Opts { verify: false, from_step: 0, offset: Some(off), extras_file: None, refit: Vec::new(), keep: Default::default(), demolition: Some(plan_undo.demolition.clone()) };
        let report = exec::run(&mut bot, &plan_undo.plan, &opts, &mut |m| eprintln!("  {m}"), &mut |_, _| {}).map_err(|e| match &bot.kicked {
            Some(r) => preflight::kicked_message(r, &name),
            None => tr!("the connection failed: {}\n  -> run the same command again; blocks already removed are skipped", "연결이 끊겼습니다: {}\n  -> 같은 명령을 다시 실행하세요. 이미 제거한 블록은 건너뜁니다", e),
        })?;
        if report.steps_done < plan_undo.plan.steps.len() {
            return Err(tr!("the undo stopped early; run the same command again", "철거가 중간에 멈췄습니다. 같은 명령을 다시 실행하세요"));
        }
        let after = undo::sort(&placed, ground, &|c| bot.chunks.state(world(c)));
        let stuck = after.ours.len();
        println!("{}", tr!("removed {} block(s); {} of ours still stand, {} could not be reached", "{}개 제거 | 남은 블록 {}개 | 닿지 않은 블록 {}개", sorted.ours.len() - stuck, stuck, plan_undo.unreachable.len()));
        if stuck > 0 {
            return Err(tr!("{} block(s) are still standing; run the same command again", "{}개가 남아 있습니다. 같은 명령을 다시 실행하세요", stuck));
        }
    } else {
        println!("{}", tr!("nothing of the build is left to remove", "제거할 블록이 남아 있지 않습니다"));
    }
    for f in [dir.join("done"), progress, offset_file, dir.join("plan.progress.extras")] {
        let _ = std::fs::remove_file(f);
    }
    println!("{}", tr!("undone: every block the bot placed is gone, nothing else was touched", "완료: 봇이 설치한 블록이 모두 없어졌고 다른 블록은 건드리지 않았습니다"));
    Ok(())
}

fn bounds(cells: impl Iterator<Item = BlockPos>) -> (BlockPos, BlockPos) {
    let (mut lo, mut hi) = (BlockPos::new(i32::MAX, i32::MAX, i32::MAX), BlockPos::new(i32::MIN, i32::MIN, i32::MIN));
    for p in cells {
        lo = BlockPos::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = BlockPos::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    (lo, hi)
}
