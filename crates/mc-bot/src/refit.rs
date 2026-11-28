//! `--refit`: break blocks known to be wrong and place them again after the plan is done. All of
//! them are broken first and then placed in plan order, so blocks that rest or click on each other
//! come back correctly.

use crate::blocks;
use crate::builder::PlayClient;
use crate::exec::{to_placement, Sent};
use crate::exec_repair::{self, Offset};
use crate::extras::Extras;
use crate::nav::{blocks_cell, feet_cell};
use crate::placement::Click;
use crate::plan::{Plan, Step};
use crate::scaffold::Site;
use crate::schematic::{split_state, Block};
use crate::sim::World;
use crate::world::{within_reach, BlockPos, Face};
use std::io;
use std::time::Duration;

const SETTLE: Duration = Duration::from_millis(300);

/// Walk to where `at` can be worked on, never on top of it (it may be about to be broken).
fn approach(bot: &mut PlayClient, site: &mut Site, at: BlockPos) -> io::Result<bool> {
    site.terrain.avoid.insert((at.x, at.y + 1, at.z));
    let result = site.approach(bot, std::slice::from_ref(&at));
    site.terrain.avoid.clear();
    result.map(|()| within_reach(bot.feet, at))
}

/// A free cell beside `at` where a temporary block can be put on top of something solid.
fn side_cell(bot: &PlayClient, site: &Site, at: BlockPos) -> Option<(BlockPos, Face)> {
    let here = feet_cell(bot.feet);
    Face::HORIZONTAL.iter().find_map(|&f| {
        let n = at.offset(f);
        let free = !site.terrain.solid((n.x, n.y, n.z)) && !blocks_cell(here, n) && within_reach(bot.feet, n);
        (free && site.terrain.solid((n.x, n.y - 1, n.z))).then(|| (n, f.opposite()))
    })
}

/// Break and re-place each of `list` (plan coordinates) as the plan's last step for it says.
#[allow(clippy::too_many_arguments)]
pub fn run(
    bot: &mut PlayClient,
    plan: &Plan,
    world: &World,
    sent: &mut Sent,
    off: Offset,
    list: &[BlockPos],
    extras: &mut Extras,
    say: &mut dyn FnMut(&str),
) -> io::Result<()> {
    if list.is_empty() {
        return Ok(());
    }
    let mut jobs = Vec::new();
    for want in list {
        let found = plan.steps.iter().enumerate().rev().find_map(|(i, st)| match st {
            Step::Place { pos, state, yaw, clicks } if pos == want => Some((i, state, yaw, clicks)),
            _ => None,
        });
        match found {
            Some(job) => jobs.push((job.0, *want, job.1, job.2, job.3)),
            None => say(&format!("refit: the plan places nothing at {want:?}")),
        }
    }
    jobs.sort_by_key(|j| j.0);
    let mut site = exec_repair::site_of(world, off, extras);
    for &(_, want, ..) in &jobs {
        let at = off.pos(want);
        if approach(bot, &mut site, at)? {
            say(&format!("refit: breaking {at:?}"));
            bot.break_block(at)?;
            bot.idle(SETTLE)?;
            site.terrain.remove_block((at.x, at.y, at.z));
        } else {
            say(&format!("refit: no way to {at:?}"));
        }
    }
    for &(_, want, state, yaw, clicks) in &jobs {
        let at = off.pos(want);
        let name = split_state(state).0;
        let mut pl = to_placement(state, clicks, *yaw, off);
        if !approach(bot, &mut site, at)? {
            say(&format!("refit: no way back to {at:?}"));
            continue;
        }
        // a slab whose planned click would merge into its support gets a temporary side support
        let first = &clicks[0];
        let under = world.blocks.get(&(first.support.x, first.support.y, first.support.z));
        let merges = under.is_some_and(|s| {
            let support = Block::from_state(first.support, s);
            crate::orient::merges_into(&support, &name, first.face, first.cursor.1)
        });
        let mut temp = None;
        if merges {
            let Some((n, face)) = side_cell(bot, &site, at) else {
                say(&format!("refit: no room for a side support at {at:?}"));
                continue;
            };
            bot.place_support(n, n.below(), Face::Up, (0.5, 1.0, 0.5))?;
            let (dx, _, dz) = face.offset();
            let cursor = (0.5 + 0.5 * dx as f32, 0.25, 0.5 + 0.5 * dz as f32);
            pl.clicks = vec![Click { support: n, face, cursor }];
            temp = Some(n);
        }
        say(&format!("refit: placing {at:?} {state}"));
        bot.blocks.forget(at);
        let item = blocks::item_id(&name).unwrap_or(1);
        bot.place(at, item, &pl)?;
        bot.idle(SETTLE)?;
        if let Some(n) = temp {
            bot.break_block(n)?;
        }
        sent.insert(at, (item, pl));
        let standable = crate::nav::is_standable_surface(&name, None);
        site.terrain.add_block((at.x, at.y, at.z), standable);
    }
    site.finish(bot)?;
    extras.set(site.standing());
    exec_repair::return_to(bot, world, off, extras, say)?;
    Ok(())
}
