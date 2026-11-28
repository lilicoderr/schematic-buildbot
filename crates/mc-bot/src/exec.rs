//! Plan executor: replays a `.bbplan` step by step with the protocol client.
//!
//! Every step is first applied to the same world model the simulator uses, so the executor knows
//! where the player must be (also after falls). Each `PLACE` is confirmed through the server's
//! block-update packets: every position a step touched, and every other change the server reports,
//! is compared with the exact state the plan wants there at that moment ([`crate::ledger`]).
//! A wrong or missing block is broken and placed again at once ([`crate::exec_repair`]); every
//! `CHECKPOINT` audits all known positions again before the plan continues.

use crate::blocks;
use crate::builder::PlayClient;
use crate::exec_repair::{self, Offset, Run};
use crate::extras::{self, Extras};
use crate::ledger::Ledger;
use crate::nav::{cell_feet, feet_cell, Cell};
use crate::placement::{Click, Plan as Placement};
use crate::plan::{Plan, Step};
use crate::schematic::{split_state, Block};
use crate::sim::{self, World};
use crate::world::BlockPos;
use std::collections::HashSet;
use std::io;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct Report {
    pub steps_done: usize,
    pub placed: usize,
    pub refused: usize,
    pub checkpoints: usize,
    pub repaired: usize,
    /// Times the server teleported the bot mid-run and it resynchronised.
    pub resyncs: usize,
    pub missing: Vec<BlockPos>,
    /// Blocks whose server-reported state differs from the plan: (world position, wanted state,
    /// difference). Only positions placed in this session are checked.
    pub differs: Vec<(BlockPos, String, String)>,
    pub elapsed: Duration,
    /// Placements per second in each window of 100 placements.
    pub windows: Vec<f64>,
    /// Seconds spent per step kind: move, pillar, support, place, break, checkpoint.
    pub spent: [f64; 6],
    /// Step counts per kind in the same order.
    pub counts: [u32; 6],
    pub kicked: Option<String>,
    pub aborted: Option<String>,
}

pub struct Opts {
    pub verify: bool,
    /// Resume: steps before this index are assumed done in an earlier run.
    pub from_step: usize,
    /// Plan-to-world translation of the run being resumed (otherwise taken from where we stand).
    pub offset: Option<Offset>,
    /// Where to remember scaffold built outside the plan (read again when resuming).
    pub extras_file: Option<std::path::PathBuf>,
    /// Plan positions to break and place again after the last step (blocks known to be wrong).
    pub refit: Vec<BlockPos>,
    /// World positions that were occupied before the build and are not part of it; the repair
    /// leaves them alone.
    pub keep: HashSet<BlockPos>,
    /// An undo: the blocks standing before the first step, in plan coordinates.
    pub demolition: Option<Demolition>,
    /// Cooperative build: the strip this bot works in and the blocks other bots finished before this plan.
    pub strip: Option<(crate::territory::Fence, Vec<(BlockPos, String)>)>,
}

/// The world an undo plan starts from.
#[derive(Debug, Clone, Default)]
pub struct Demolition {
    pub blocks: std::collections::HashMap<Cell, String>,
    /// The cells of `blocks` the plan may break.
    pub targets: HashSet<Cell>,
}

/// Pitch and sneak are not stored in the plan: they follow from the wanted state alone.
pub(crate) fn to_placement(state: &str, clicks: &[crate::plan::ClickStep], yaw: Option<f32>, off: Offset) -> Placement {
    let block = Block::from_state(BlockPos::new(0, 0, 0), state);
    Placement {
        clicks: clicks.iter().map(|c| Click { support: off.pos(c.support), face: c.face, cursor: c.cursor }).collect(),
        yaw,
        pitch: crate::orient::look(&block).1,
        sneak: crate::orient::needs_sneak(&block),
        missing_support: None,
        unhonoured: Vec::new(),
    }
}

/// Final server-reported states against the plan, in a stable order.
fn state_diffs(bot: &PlayClient, world: &World, ledger: &Ledger) -> Vec<(BlockPos, String, String)> {
    let off = ledger.off;
    let mut out: Vec<(BlockPos, String, String)> = world
        .blocks
        .iter()
        .filter(|(c, _)| !world.scaffold.contains(*c) && world.fence.has(c.0, c.2))
        .map(|(c, s)| (off.pos(BlockPos::new(c.0, c.1, c.2)), s))
        .filter(|(p, _)| ledger.placed.contains(p) || bot.blocks.state(*p).is_some())
        .filter_map(|(p, s)| crate::verify::difference(s, bot.blocks.state(p)).map(|d| (p, s.clone(), d)))
        .collect();
    out.sort_by_key(|(p, ..)| (p.y, p.x, p.z));
    out
}

/// Audit every position the server has reported or this session has placed.
fn full_audit(bot: &mut PlayClient, run: &mut Run, say: &mut dyn FnMut(&str)) -> io::Result<usize> {
    exec_repair::settle(bot)?;
    run.ledger.collect(bot);
    run.ledger.due(bot.acked);
    let all = run.ledger.everything(bot).into_iter().map(|(y, x, z)| BlockPos::new(x, y, z)).collect();
    exec_repair::heal(bot, run, all, say)
}

pub fn run(
    bot: &mut PlayClient,
    plan: &Plan,
    opts: &Opts,
    say: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(usize, Offset),
) -> io::Result<Report> {
    let off = opts.offset.unwrap_or_else(|| Offset::between(plan.header.start, feet_cell(bot.feet)));
    let mut world = World::new(plan.header.origin.1, plan.header.start);
    if let Some(d) = &opts.demolition {
        world.blocks.extend(d.blocks.clone());
        world.demolish = d.targets.clone();
    }
    if let Some((fence, prebuilt)) = &opts.strip {
        world.fence = *fence;
        world.preload(prebuilt);
    }
    let mut ledger = Ledger::new(plan, off);
    ledger.keep = opts.keep.clone();
    let mut rep = Report::default();
    let started = Instant::now();
    let (mut window_start, mut window_n) = (Instant::now(), 0usize);
    let scaffold_item = blocks::item_id("cobblestone").unwrap_or(1);

    let first = opts.from_step.min(plan.steps.len());
    let mut extras = Extras::open(opts.extras_file.clone(), first > 0);
    for (i, st) in plan.steps[..first].iter().enumerate() {
        ledger.own(i, st);
        sim::apply(&mut world, st).map_err(|m| io::Error::other(format!("step {}: {m}", i + 1)))?;
    }
    if first > 0 {
        rep.steps_done = first;
        if !exec_repair::return_to(bot, &world, off, &mut extras, say)? {
            rep.aborted = Some(format!("resume: no ground route to the position of step {first}"));
            rep.elapsed = started.elapsed();
            return Ok(rep);
        }
    }
    // level moves collected so the bot walks them in one go
    let mut level: Vec<Cell> = Vec::new();

    for (i, st) in plan.steps.iter().enumerate().skip(first) {
        if bot.kicked.is_some() {
            rep.kicked = bot.kicked.clone();
            break;
        }
        let before: Cell = world.feet;
        if let Err(msg) = sim::apply(&mut world, st) {
            rep.aborted = Some(format!("step {}: {st}: {msg}", i + 1));
            break;
        }
        let kind = match st {
            Step::Move(_) => 0,
            Step::Pillar(_) => 1,
            Step::Support { .. } => 2,
            Step::Place { .. } => 3,
            Step::Break(_) => 4,
            Step::Checkpoint { .. } => 5,
            Step::Auto { .. } => 3,
        };
        if !extras.cells.is_empty() {
            let needed = extras::needed(st, before, world.feet, off);
            if extras.hits(&needed) {
                bot.walk_level(&std::mem::take(&mut level))?;
                extras.clear(bot, &needed)?;
            }
        }
        let t_step = Instant::now();
        let teleports = bot.teleports;
        match st {
            Step::Move(c) => {
                if c.1 == before.1 {
                    level.push(off.cell(*c));
                    let more = matches!(plan.steps.get(i + 1), Some(Step::Move(n)) if n.1 == c.1);
                    if !more {
                        bot.walk_level(&std::mem::take(&mut level))?;
                    }
                } else {
                    bot.walk_cells(&[off.cell(before), off.cell(*c)])?;
                }
            }
            Step::Pillar(c) => bot.pillar_up(off.cell(*c), 1, scaffold_item)?,
            Step::Support { pos, clicks } => {
                let c = &clicks[0];
                bot.place_support(off.pos(*pos), off.pos(c.support), c.face, c.cursor)?;
            }
            Step::Place { pos, state, yaw, clicks } => {
                let item = blocks::item_id(&split_state(state).0).unwrap_or(1);
                let pl = to_placement(state, clicks, *yaw, off);
                let at = off.pos(*pos);
                if bot.place(at, item, &pl)? {
                    rep.placed += 1;
                    window_n += 1;
                    if window_n == 100 {
                        rep.windows.push(100.0 / window_start.elapsed().as_secs_f64().max(0.001));
                        window_start = Instant::now();
                        window_n = 0;
                    }
                } else {
                    rep.refused += 1;
                }
            }
            Step::Auto { .. } => {}
            Step::Break(p) => {
                bot.break_block(off.pos(*p))?;
                if world.feet.1 < before.1 {
                    bot.fall_to(cell_feet(off.cell(world.feet)).y)?;
                }
            }
            Step::Checkpoint { n, .. } => {
                rep.checkpoints += 1;
                let mut run = Run { plan, world: &world, ledger: &mut ledger, extras: &mut extras };
                let fixed = full_audit(bot, &mut run, say)?;
                rep.repaired += fixed;
                say(&format!("checkpoint {n}: {} blocks expected, {} re-placed", world.built().0, fixed));
            }
        }
        if bot.kicked.is_some() {
            // the step may be half done: do not record it as finished
            break;
        }
        ledger.collect(bot);
        ledger.sent(i, st, bot.seq);
        // The server moved us (for example a lag spike made queued moves exceed its speed limit):
        // replaying on from the planned cell would walk through the air, and whatever was sent since
        // may have failed silently. Walk back, then re-place what is missing right away.
        if bot.teleports != teleports && level.is_empty() {
            rep.resyncs += 1;
            bot.idle(Duration::from_millis(500))?;
            say(&format!("step {}: the server moved the player, walking back", i + 1));
            if !exec_repair::return_to(bot, &world, off, &mut extras, say)? {
                rep.aborted = Some(format!("step {}: lost the planned position after a server teleport", i + 1));
                break;
            }
            let mut run = Run { plan, world: &world, ledger: &mut ledger, extras: &mut extras };
            rep.repaired += full_audit(bot, &mut run, say)?;
        }
        // standing where the plan expects (no walk queued): put right what the server disagrees with
        if level.is_empty() {
            let due = ledger.due(bot.acked);
            let mut run = Run { plan, world: &world, ledger: &mut ledger, extras: &mut extras };
            rep.repaired += exec_repair::heal(bot, &mut run, due, say)?;
        }
        rep.spent[kind] += t_step.elapsed().as_secs_f64();
        rep.counts[kind] += 1;
        rep.steps_done = i + 1;
        if level.is_empty() {
            on_step(i + 1, off);
            // a safe stop asked for by `build-bot ui`: between steps, with the progress saved
            if crate::ui_link::stop_requested() && i + 1 < plan.steps.len() {
                rep.aborted = Some("stopped on request".into());
                break;
            }
        }
    }
    bot.idle(Duration::from_millis(500))?;
    let complete = rep.steps_done == plan.steps.len() && rep.aborted.is_none() && bot.kicked.is_none();
    if complete && !opts.refit.is_empty() {
        // blocks the caller knows to be wrong (for example from an earlier session)
        let list: Vec<BlockPos> = opts.refit.iter().map(|&p| off.pos(p)).collect();
        let mut run = Run { plan, world: &world, ledger: &mut ledger, extras: &mut extras };
        for p in &list {
            say(&format!("refit: {p:?}"));
        }
        crate::refit::fix(bot, &mut run, &list, say)?;
    }
    if opts.verify && rep.aborted.is_none() && bot.kicked.is_none() {
        let mut run = Run { plan, world: &world, ledger: &mut ledger, extras: &mut extras };
        rep.repaired += full_audit(bot, &mut run, say)?;
    }
    if complete && !extras.cells.is_empty() {
        say(&format!("taking down {} blocks of extra scaffold", extras.cells.len()));
        let mut site = exec_repair::site_of(&world, off, &extras);
        site.finish(bot)?;
        extras.set(site.standing());
        say(&format!("extra scaffold left standing: {:?}", extras.cells));
        bot.idle(Duration::from_millis(300))?;
    }
    rep.missing = ledger.placed.iter().copied().filter(|&p| !bot.blocks.is_confirmed(p)).collect();
    rep.missing.retain(|p| world.blocks.contains_key(&(p.x - off.dx, p.y - off.dy, p.z - off.dz)));
    rep.missing.sort_by_key(|p| (p.y, p.x, p.z));
    rep.differs = state_diffs(bot, &world, &ledger);
    rep.elapsed = started.elapsed();
    rep.kicked = bot.kicked.clone();
    Ok(rep)
}
