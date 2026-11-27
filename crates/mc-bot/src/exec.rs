//! Plan executor: replays a `.bbplan` step by step with the protocol client.
//!
//! Every step is first applied to the same world model the simulator uses, so the executor knows
//! where the player must be (also after falls). Each `PLACE` is confirmed through the server's
//! block-update packets; at every `CHECKPOINT` the confirmed set is compared with the expected one
//! and anything missing is re-placed ([`crate::exec_repair`]) before the plan continues.

use crate::blocks;
use crate::builder::PlayClient;
use crate::exec_repair::{self, Offset};
use crate::nav::{cell_feet, feet_cell, Cell};
use crate::placement::{Click, Plan as Placement};
use crate::plan::{Plan, Step};
use crate::schematic::{split_state, Block};
use crate::sim::{self, World};
use crate::world::BlockPos;
use std::collections::HashMap;
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
}

/// What a placement step needs to be repeated later: item id and click plan (world coordinates).
pub type Sent = HashMap<BlockPos, (i32, Placement)>;

/// Pitch and sneak are not stored in the plan: they follow from the wanted state alone.
fn to_placement(state: &str, clicks: &[crate::plan::ClickStep], yaw: Option<f32>, off: Offset) -> Placement {
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

pub fn run(
    bot: &mut PlayClient,
    plan: &Plan,
    opts: &Opts,
    say: &mut dyn FnMut(&str),
    on_step: &mut dyn FnMut(usize, Offset),
) -> io::Result<Report> {
    let off = opts.offset.unwrap_or_else(|| Offset::between(plan.header.start, feet_cell(bot.feet)));
    let mut world = World::new(plan.header.origin.1, plan.header.start);
    let mut sent: Sent = HashMap::new();
    let mut rep = Report::default();
    let started = Instant::now();
    let (mut window_start, mut window_n) = (Instant::now(), 0usize);
    let scaffold_item = blocks::item_id("cobblestone").unwrap_or(1);

    let first = opts.from_step.min(plan.steps.len());
    for (i, st) in plan.steps[..first].iter().enumerate() {
        sim::apply(&mut world, st).map_err(|m| io::Error::other(format!("step {}: {m}", i + 1)))?;
    }
    if first > 0 {
        rep.steps_done = first;
        if !exec_repair::return_to(bot, &world, off, say)? {
            rep.aborted = Some(format!("resume: no ground route to the position of step {first}"));
            rep.elapsed = started.elapsed();
            return Ok(rep);
        }
    }
    let mut teleports = bot.teleports;
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
                    sent.insert(at, (item, pl));
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
                bot.idle(Duration::from_millis(300))?;
                let fixed = exec_repair::repair(bot, &world, &sent, off, say)?;
                rep.repaired += fixed;
                say(&format!("checkpoint {n}: {} blocks expected, {} re-placed", world.built().0, fixed));
            }
        }
        if bot.kicked.is_some() {
            // the step may be half done: do not record it as finished
            break;
        }
        // The server moved us: replaying on from the planned cell would walk through the air.
        if bot.teleports != teleports && level.is_empty() {
            bot.idle(Duration::from_millis(100))?;
            say(&format!("step {}: the server moved the player, walking back", i + 1));
            if !exec_repair::return_to(bot, &world, off, say)? {
                rep.aborted = Some(format!("step {}: lost the planned position after a server teleport", i + 1));
                break;
            }
        }
        rep.spent[kind] += t_step.elapsed().as_secs_f64();
        rep.counts[kind] += 1;
        if bot.teleports != teleports && level.is_empty() {
            // The server rejected a move (for example after a lag spike) and put us back: everything
            // since then may have failed silently. Walk back and re-place what is missing now.
            rep.resyncs += 1;
            say(&format!("server teleported the bot at step {} ({st}): resynchronising", i + 1));
            bot.idle(Duration::from_millis(500))?;
            exec_repair::return_to(bot, &world, off, say)?;
            rep.repaired += exec_repair::repair(bot, &world, &sent, off, say)?;
            teleports = bot.teleports;
        }
        rep.steps_done = i + 1;
        if level.is_empty() {
            on_step(i + 1, off);
        }
    }
    bot.idle(Duration::from_millis(500))?;
    if opts.verify && rep.aborted.is_none() && bot.kicked.is_none() {
        rep.repaired += exec_repair::repair(bot, &world, &sent, off, say)?;
    }
    rep.missing = exec_repair::missing(bot, &world, &sent, off);
    rep.elapsed = started.elapsed();
    rep.kicked = bot.kicked.clone();
    Ok(rep)
}
