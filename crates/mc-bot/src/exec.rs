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
use crate::schematic::split_state;
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
    pub missing: Vec<BlockPos>,
    pub elapsed: Duration,
    /// Placements per second in each window of 100 placements.
    pub windows: Vec<f64>,
    pub kicked: Option<String>,
    pub aborted: Option<String>,
}

pub struct Opts {
    pub verify: bool,
}

/// What a placement step needs to be repeated later: item id and click plan (world coordinates).
pub type Sent = HashMap<BlockPos, (i32, Placement)>;

fn to_placement(clicks: &[crate::plan::ClickStep], yaw: Option<f32>, off: Offset) -> Placement {
    Placement {
        clicks: clicks.iter().map(|c| Click { support: off.pos(c.support), face: c.face, cursor: c.cursor }).collect(),
        yaw,
        unhonoured: Vec::new(),
    }
}

pub fn run(bot: &mut PlayClient, plan: &Plan, opts: &Opts, say: &mut dyn FnMut(&str)) -> io::Result<Report> {
    let off = Offset::between(plan.header.start, feet_cell(bot.feet));
    let mut world = World::new(plan.header.origin.1, plan.header.start);
    let mut sent: Sent = HashMap::new();
    let mut rep = Report::default();
    let started = Instant::now();
    let (mut window_start, mut window_n) = (Instant::now(), 0usize);
    let scaffold_item = blocks::item_id("cobblestone").unwrap_or(1);

    for (i, st) in plan.steps.iter().enumerate() {
        if bot.kicked.is_some() {
            rep.kicked = bot.kicked.clone();
            break;
        }
        let before: Cell = world.feet;
        if let Err(msg) = sim::apply(&mut world, st) {
            rep.aborted = Some(format!("step {}: {st}: {msg}", i + 1));
            break;
        }
        match st {
            Step::Move(c) => bot.walk_cells(&[off.cell(before), off.cell(*c)])?,
            Step::Pillar(c) => bot.pillar_up(off.cell(*c), 1, scaffold_item)?,
            Step::Support { pos, clicks } => {
                let c = &clicks[0];
                bot.place_support(off.pos(*pos), off.pos(c.support), c.face, c.cursor)?;
            }
            Step::Place { pos, state, yaw, clicks } => {
                let item = blocks::item_id(&split_state(state).0).unwrap_or(1);
                let pl = to_placement(clicks, *yaw, off);
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
        rep.steps_done = i + 1;
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
