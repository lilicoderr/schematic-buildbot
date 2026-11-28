//! The offline half of `build`: cut (or reload) the plan, simulate it strictly and print what a
//! build will do.

use super::build_args::Args;
use mc_bot::job::Job;
use mc_bot::plan::{Plan, Step};
use mc_bot::schematic::Schematic;
use mc_bot::version::Version;
use mc_bot::{blocks, guard, plan_parse, sim, slice, slice_stats, tr};
use std::collections::BTreeMap;

/// Plan coordinates of the blueprint origin; `run` translates them to the world.
const PLAN_ORIGIN: (i32, i32, i32) = (2, 64, 2);

/// Block types this release has no item for, with how often the blueprint uses them.
pub fn unsupported(schem: &Schematic) -> BTreeMap<String, usize> {
    schem.block_counts().into_iter().filter(|(n, _)| blocks::item_id(n).is_none()).collect()
}

/// The saved plan of an interrupted build (`job`), or a new one.
pub fn obtain(job: Option<&Job>, schem: &Schematic, version: &Version, args: &Args) -> Result<(Plan, bool), String> {
    if let Some(job) = job.filter(|j| j.progress().exists()) {
        let text = std::fs::read_to_string(job.plan()).map_err(|e| {
            tr!("progress exists but its plan cannot be read ({}); use --fresh to start over", "진행 파일은 있지만 계획 파일을 읽을 수 없습니다 ({}). 처음부터 하려면 --fresh", e)
        })?;
        let plan = plan_parse::parse(&text)?;
        return Ok((plan, true));
    }
    println!("{}", tr!("planning {} blocks...", "{}블록 건축 계획 중...", schem.blocks.len()));
    let mut budget = guard::Budget::new(args.time_limit, args.memory_mb << 20);
    let plan = slice::slice_with(schem, PLAN_ORIGIN, version.protocol, &mut budget)?;
    Ok((plan, false))
}

/// Why this plan would not reproduce the schematic exactly (empty: it will).
pub fn problems(plan: &Plan, schem: &Schematic) -> Vec<String> {
    let mut out = Vec::new();
    for (name, n) in unsupported(schem) {
        out.push(tr!("unsupported block: {} x{}", "지원하지 않는 블록: {} x{}", name, n));
    }
    let unreachable: Vec<_> = plan.header.unreachable.iter().filter(|(_, n)| !n.ends_with("/unknown_block")).collect();
    if let Some((p, why)) = unreachable.first() {
        out.push(tr!(
            "{} block(s) cannot be reached or supported, e.g. {} at blueprint {},{},{}",
            "도달하거나 받칠 수 없는 블록 {}개, 예: {} (설계도 좌표 {},{},{})",
            unreachable.len(), why, p.x - plan.header.origin.0, p.y - plan.header.origin.1, p.z - plan.header.origin.2
        ));
    }
    if let Err(lines) = slice_stats::inexact_states(plan) {
        out.push(tr!("{} block state(s) will differ, e.g. {}", "블록 상태 {}개가 달라집니다, 예: {}", lines.len(), lines[0]));
    }
    out
}

/// Replay the whole plan in memory. A failure here is never overridable.
pub fn check_legal(plan: &Plan, schem: &Schematic) -> Result<(), String> {
    if plan.header.schematic_hash != schem.fingerprint() {
        return Err(tr!("the saved plan belongs to another schematic; use --fresh", "저장된 계획이 다른 설계도의 것입니다. --fresh 를 사용하세요"));
    }
    sim::simulate(plan, schem).map(|_| ()).map_err(|e| {
        tr!("the simulation rejects the plan at step {}: {}", "시뮬레이션 실패, {}단계: {}", e.step + 1, e.msg)
    })
}

pub fn print_stats(plan: &Plan, schem: &Schematic, version: &Version) {
    let (min, max) = schem.bounds();
    let count = |f: fn(&Step) -> bool| plan.steps.iter().filter(|s| f(s)).count();
    let est: u64 = plan.header.stats.iter().find(|(k, _)| k == "est_seconds").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    println!(
        "{}",
        tr!(
            "blueprint '{}': {} blocks, size {}x{}x{}, Minecraft {}",
            "설계도 '{}': {}블록, 크기 {}x{}x{}, 마인크래프트 {}",
            schem.name, schem.blocks.len(), max.x - min.x + 1, max.y - min.y + 1, max.z - min.z + 1, version.name
        )
    );
    println!(
        "{}",
        tr!(
            "plan: {} placements, {} temporary blocks (removed again), estimated time {}",
            "계획: 설치 {}회, 임시 블록 {}개 (다시 제거), 예상 시간 {}",
            count(|s| matches!(s, Step::Place { .. })),
            count(|s| matches!(s, Step::Pillar(_) | Step::Support { .. })),
            super::fmt_duration(est as f64)
        )
    );
    let n: usize = unsupported(schem).values().sum();
    println!("{}", tr!("unsupported blocks: {} | not buildable in total: {}", "지원하지 않는 블록: {}개 | 설치 불가 합계: {}개", n, plan.header.unreachable.len()));
}
