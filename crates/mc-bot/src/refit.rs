//! Put wrong cells right: break every wrong block first and then place in plan order, so blocks
//! that rest or click on each other come back correctly. Used by the repair during a run and by
//! `--refit`.

use crate::blocks;
use crate::builder::PlayClient;
use crate::exec::to_placement;
use crate::exec_repair::{self, settle, Run};
use crate::nav::{blocks_cell, feet_cell, Cell};
use crate::placement::{self, Action, Click};
use crate::plan::Step;
use crate::scaffold::Site;
use crate::schematic::{split_state, Block};
use crate::sim::SCAFFOLD;
use crate::support;
use crate::world::{eye_to_box, within_reach, BlockPos, CLICK_REACH};
use std::io;

/// Walk to where `at` can be worked on, never on top of it (it may be about to be broken).
fn approach(bot: &mut PlayClient, site: &mut Site, at: BlockPos) -> io::Result<bool> {
    site.terrain.avoid.insert((at.x, at.y + 1, at.z));
    let result = site.approach(bot, std::slice::from_ref(&at));
    site.terrain.avoid.clear();
    result.map(|()| within_reach(bot.feet, at))
}

fn plan_cell(run: &Run, p: BlockPos) -> Cell {
    let off = run.ledger.off;
    (p.x - off.dx, p.y - off.dy, p.z - off.dz)
}

/// Break what stands in `list` (world positions) and place what the plan wants there now, then
/// return to the planned feet cell.
pub fn fix(bot: &mut PlayClient, run: &mut Run, list: &[BlockPos], say: &mut dyn FnMut(&str)) -> io::Result<()> {
    if list.is_empty() {
        return Ok(());
    }
    let off = run.ledger.off;
    let mut site = exec_repair::site_of(run.world, off, run.extras);
    // known to be empty: reported as air, or placed this session and never reported
    let placed = run.ledger.placed.clone();
    let empty = |bot: &PlayClient, p: BlockPos| bot.blocks.state(p).map_or(placed.contains(&p), |s| s == 0);
    // the model has the planned blocks; for these cells believe the server instead
    for &p in list {
        let c = (p.x, p.y, p.z);
        if empty(bot, p) {
            site.terrain.remove_block(c);
        } else if bot.blocks.state(p).is_some() {
            site.terrain.remove_block(c);
            site.terrain.add_block(c, false);
        }
    }
    let mut order = list.to_vec();
    order.sort_by_key(|p| (-p.y, p.x, p.z));
    // top down, so nothing is left resting on a block that is about to go
    for at in order {
        if empty(bot, at) {
            continue;
        }
        if approach(bot, &mut site, at)? {
            bot.break_block(at)?;
            settle(bot)?;
            site.terrain.remove_block((at.x, at.y, at.z));
        } else {
            say(&format!("repair: no way to break {at:?}"));
        }
    }
    // plan order; cells without a placement step (pillar blocks) first, they only carry others
    let mut jobs: Vec<(Option<usize>, BlockPos)> = list
        .iter()
        .filter(|&&p| run.world.blocks.contains_key(&plan_cell(run, p)))
        .map(|&p| (run.ledger.jobs.get(&p).copied(), p))
        .collect();
    jobs.sort_by_key(|&(i, p)| (i, p.y, p.x, p.z));
    jobs.dedup();
    for (job, at) in jobs {
        if bot.kicked.is_some() {
            break;
        }
        // a second half only comes back with its first one
        if job.is_none() && run.world.blocks[&plan_cell(run, at)].contains("half=upper") {
            continue;
        }
        place_again(bot, run, &mut site, job, at, say)?;
    }
    site.finish(bot)?;
    run.extras.set(site.standing());
    if !exec_repair::return_to(bot, run.world, off, run.extras, say)? {
        return Err(io::Error::other("repair: cannot get back to the planned position on the ground"));
    }
    Ok(())
}

/// Place the planned block at `at` again. The planned click is used when its support still stands,
/// otherwise another neighbour, or a temporary support chain.
fn place_again(bot: &mut PlayClient, run: &mut Run, site: &mut Site, step: Option<usize>, at: BlockPos, say: &mut dyn FnMut(&str)) -> io::Result<()> {
    let off = run.ledger.off;
    let (state, mut pl) = match step.map(|i| &run.plan.steps[i]) {
        Some(Step::Place { state, yaw, clicks, .. }) => (state.clone(), to_placement(state, clicks, *yaw, off)),
        _ => (SCAFFOLD.to_string(), to_placement(SCAFFOLD, &[], None, off)),
    };
    let (name, props) = split_state(&state);
    if !approach(bot, site, at)? {
        say(&format!("repair: no way back to {at:?}"));
        return Ok(());
    }
    let block = Block::from_state(at, &state);
    let world = run.world;
    // a click that merges into a slab would leave `at` empty; a usable block would just open
    let safe = |c: &Click| {
        let s = c.support;
        let under = world.blocks.get(&(s.x - off.dx, s.y - off.dy, s.z - off.dz)).map(|v| Block::from_state(s, v));
        !under.is_some_and(|b| placement::is_usable(&b.name) || crate::orient::merges_into(&b, &name, c.face, c.cursor.1))
    };
    let feet = bot.feet;
    let near = |c: &Click| site.terrain.solid((c.support.x, c.support.y, c.support.z)) && eye_to_box(feet, c.support) <= CLICK_REACH && safe(c);
    let good = |clicks: &[Click], ok: &dyn Fn(&Click) -> bool| !clicks.is_empty() && clicks.iter().all(|c| c.support == at || ok(c));
    let mut chain = Vec::new();
    if !good(&pl.clicks, &near) {
        let here = feet_cell(feet);
        let solid = |q: BlockPos| site.terrain.solid((q.x, q.y, q.z));
        // never a usable block: it would open instead
        let clickable = |q: BlockPos| {
            let under = world.blocks.get(&(q.x - off.dx, q.y - off.dy, q.z - off.dz));
            solid(q) && eye_to_box(feet, q) <= CLICK_REACH && !under.is_some_and(|v| placement::is_usable(&split_state(v).0))
        };
        // the wanted state first (with a temporary block if a face is missing); otherwise any click
        let found = support::aim(&block, feet, &solid, &|q| blocks_cell(here, q), &clickable, &safe).or_else(|| match placement::plan_clicks(&block, near) {
            Action::Place(p) if p.missing_support.is_none() && good(&p.clicks, &near) => Some((Vec::new(), p)),
            _ => None,
        });
        let Some((links, p)) = found else {
            say(&format!("repair: nothing to click for {at:?}"));
            return Ok(());
        };
        (chain, pl) = (links, placement::Plan { yaw: pl.yaw.or(p.yaw), ..p });
    }
    for l in &chain {
        bot.place_support(l.pos, l.click.support, l.click.face, l.click.cursor)?;
    }
    bot.blocks.forget(at);
    let item = blocks::item_id(&name).unwrap_or(1);
    if !bot.place(at, item, &pl)? {
        say(&format!("repair: {at:?} still out of reach"));
    }
    settle(bot)?;
    for l in chain.iter().rev() {
        bot.break_block(l.pos)?;
    }
    run.ledger.placed.insert(at);
    site.terrain.add_block((at.x, at.y, at.z), crate::nav::is_standable_surface(&name, props.get("type").map(String::as_str)));
    Ok(())
}
