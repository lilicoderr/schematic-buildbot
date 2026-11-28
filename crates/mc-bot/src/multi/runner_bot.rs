//! The thread of one bot of a cooperative run.

use super::Shared;
use crate::builder::PlayClient;
use crate::exec::{self, Opts};
use crate::exec_repair::{self, Offset};
use crate::extras::Extras;
use crate::multi::Part;
use crate::nav::{feet_cell, Terrain};
use crate::run_state;
use crate::scaffold::Site;
use crate::sim::World;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

fn file(sh: &Shared, k: usize, bot: usize, ext: &str) -> PathBuf {
    sh.cfg.state_dir.join(format!("phase{}-bot{bot}.{ext}", k + 1))
}

/// The part of `bot` in phase `k`.
fn part<'a>(sh: &'a Shared, k: usize, bot: usize) -> Option<&'a Part> {
    sh.multi.phases.get(k)?.iter().find(|p| p.bot == bot)
}

fn saved(sh: &Shared, k: usize, bot: usize) -> Option<run_state::RunState> {
    let p = part(sh, k, bot)?;
    run_state::load(&file(sh, k, bot, "progress"), run_state::fingerprint(&p.plan))
}

fn connect(sh: &Shared, bot: usize) -> Result<PlayClient, String> {
    let name = &sh.cfg.names[bot];
    let mut c = crate::preflight::join(&sh.cfg.server, name, sh.cfg.version)?;
    crate::preflight::require_creative(&mut c, name, sh.cfg.wait_creative, &mut |m| eprintln!("  [bot{bot}] {m}"))?;
    c.place_delay = sh.cfg.delay;
    c.stop = Some(sh.stop.clone());
    c.land().map_err(|e| e.to_string())?;
    Ok(c)
}

/// Bot 0 decides where the blueprint goes; the others use the same place.
fn decide_offset(sh: &Shared, c: &PlayClient) -> Offset {
    let first = &sh.multi.phases[0];
    let from_files = (0..sh.multi.bots).find_map(|b| saved(sh, 0, b));
    let p0 = first.iter().find(|p| p.bot == 0).map_or((0, 0, 0), |p| p.plan.header.start);
    let o = first[0].plan.header.origin;
    match (from_files, sh.cfg.origin) {
        (Some(s), _) => s.offset,
        (None, Some(w)) => Offset { dx: w.0 - o.0, dy: w.1 - o.1, dz: w.2 - o.2 },
        (None, None) => Offset::between(p0, feet_cell(c.feet)),
    }
}

/// Walk over the open ground to a cell (nothing is built there yet).
fn walk_to(c: &mut PlayClient, goal: (i32, i32, i32)) -> Result<(), String> {
    if feet_cell(c.feet) == goal {
        return Ok(());
    }
    let mut site = Site::new(Terrain::new(goal.1, HashSet::new()));
    let teleports = c.teleports;
    let arrived = site.go_to(c, goal).map_err(|e| e.to_string())?;
    c.idle(Duration::from_millis(500)).map_err(|e| e.to_string())?;
    match arrived && c.teleports == teleports && feet_cell(c.feet) == goal {
        true => Ok(()),
        false => Err(format!("cannot walk over the open ground to {goal:?} (is the way flat and clear?)")),
    }
}

pub(super) fn bot_main(sh: &Shared, bot: usize) {
    let mut client = connect(sh, bot).map_err(|e| sh.fail(bot, e)).ok();
    sh.joined.wait();
    if let (0, Some(c)) = (bot, &client) {
        let off = decide_offset(sh, c);
        let o = off.pos(crate::world::BlockPos::new(sh.multi.phases[0][0].plan.header.origin.0, sh.multi.phases[0][0].plan.header.origin.1, sh.multi.phases[0][0].plan.header.origin.2));
        println!("blueprint origin in the world: {},{},{}", o.x, o.y, o.z);
        *sh.offset.lock().unwrap_or_else(|e| e.into_inner()) = Some(off);
    }
    sh.offset_known.wait();
    let off = *sh.offset.lock().unwrap_or_else(|e| e.into_inner());
    if let (Some(c), Some(off), false) = (client.as_mut(), off, sh.stopped()) {
        let start = part(sh, 0, bot).map(|p| off.cell(p.plan.header.start));
        if let Some(r) = start.map(|s| walk_to(c, s)).and_then(Result::err) {
            sh.fail(bot, r);
        }
    }
    sh.placed.wait();
    let phases = sh.multi.phases.len();
    for k in 0..phases {
        if let (Some(c), Some(off), Some(p), false) = (client.as_mut(), off, part(sh, k, bot), sh.stopped()) {
            if let Err(e) = run_part(sh, bot, k, p, c, off) {
                sh.fail(bot, e);
            }
        }
        // a bot with nothing more to do leaves, so nobody stands in the way of the next phase
        if (k + 1..phases).all(|j| part(sh, j, bot).is_none()) {
            client = None;
        }
        sh.phase_end.wait();
        if client.is_some() && part(sh, k + 1, bot).is_some() {
            if let Some(c) = client.as_mut() {
                let _ = c.idle(Duration::from_millis(1500));
            }
        }
    }
}

fn run_part(sh: &Shared, bot: usize, k: usize, p: &Part, c: &mut PlayClient, off: Offset) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    let say = |m: &str| eprintln!("  [bot{bot}] {m}");
    let fingerprint = run_state::fingerprint(&p.plan);
    let total = p.plan.steps.len();
    let from_step = saved(sh, k, bot).map_or(0, |s| s.step);
    if from_step >= total && total > 0 {
        return Ok(()); // finished in an earlier run
    }
    let progress = file(sh, k, bot, "progress");
    run_state::save(&progress, fingerprint, from_step, off).map_err(io)?;
    let prebuilt = sh.standing(k);
    if from_step == 0 && feet_cell(c.feet) != off.cell(p.plan.header.start) {
        // joined again for a later phase: get to where the previous phases left this bot
        let mut world = World::new(p.plan.header.origin.1, p.plan.header.start);
        world.preload(&prebuilt);
        let mut extras = Extras::open(Some(file(sh, k, bot, "extras")), true);
        if !exec_repair::return_to(c, &world, off, &mut extras, &mut |m| say(m)).map_err(io)? {
            return Err(format!("no way over the finished blocks to {:?}", p.plan.header.start));
        }
    }
    if !sh.cfg.walk {
        c.set_sprint(true).map_err(io)?;
    }
    let opts = Opts {
        verify: true,
        from_step,
        offset: Some(off),
        extras_file: Some(file(sh, k, bot, "extras")),
        refit: Vec::new(),
        keep: Default::default(),
        demolition: None,
        strip: Some((p.fence, prebuilt)),
    };
    let mut on_step = |done: usize, o: Offset| {
        let _ = run_state::save(&progress, fingerprint, done, o);
    };
    let report = exec::run(c, &p.plan, &opts, &mut |m| say(m), &mut on_step).map_err(|e| match &c.kicked {
        Some(r) => format!("removed from the server: {r}"),
        None => format!("connection failed: {e}"),
    })?;
    let stopped = report.kicked.as_deref() == Some(crate::builder::STOPPED);
    let (steps, aborted, kicked) = (report.steps_done, report.aborted.clone(), report.kicked.clone());
    say(&format!("phase {}: {} placed, {} repaired, {} missing, {} differing, {:.0} s", k + 1, report.placed, report.repaired, report.missing.len(), report.differs.len(), report.elapsed.as_secs_f64()));
    sh.out.lock().unwrap_or_else(|e| e.into_inner()).reports.push((k, bot, report));
    match (kicked, aborted) {
        (Some(_), _) if stopped => Ok(()),
        (Some(r), _) => Err(format!("removed from the server: {r}")),
        (_, Some(a)) => Err(format!("stopped: {a}")),
        _ if steps < total => Err(format!("stopped after step {steps} of {total}")),
        _ => Ok(()),
    }
}
