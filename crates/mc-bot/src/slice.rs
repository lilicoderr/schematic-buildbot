//! The slicer: compiles a schematic into a [`Plan`] offline, with no server.
//!
//! It runs the same planner the live bot used (layer order, tiled sweep, lookahead standing
//! cells, jump/drop navigation, scaffold pillars), but against a recorder that writes plan steps.
//! The result is then replayed by [`crate::sim::simulate`]; [`slice`] refuses to return a plan
//! that fails simulation.

use crate::blocks;
use crate::nav::{blocks_cell, cell_feet, feet_cell, Cell, Terrain};
use crate::placement::{self, Action};
use crate::plan::{block_hash, ClickStep, Header, Plan, Step};
use crate::scaffold::{Mover, Site};
use crate::schematic::{Block, Schematic};
use crate::sim;
use crate::support;
use crate::world::{within_reach, BlockPos, Vec3};
use std::collections::HashSet;
use std::io;

/// Upcoming blocks considered when choosing where to stand (offline, so generous).
pub const LOOKAHEAD: usize = 96;
/// A checkpoint is written after this many placements.
pub const CHECKPOINT_EVERY: usize = 250;
/// A pocket smaller than this many standing cells (no way to the open ground) counts as sealed.
const SEAL_CAP: usize = 20_000;

/// Records movement as plan steps instead of moving anybody.
struct Recorder {
    feet: Vec3,
    steps: Vec<Step>,
}

impl Mover for Recorder {
    fn feet(&self) -> Vec3 {
        self.feet
    }
    fn kicked(&self) -> bool {
        false
    }
    fn walk_cells(&mut self, route: &[Cell]) -> io::Result<()> {
        for &c in route.iter().skip(1) {
            self.steps.push(Step::Move(c));
            self.feet = cell_feet(c);
        }
        Ok(())
    }
    fn pillar_up(&mut self, base: Cell, height: i32, _item: i32) -> io::Result<()> {
        for k in 0..height {
            self.steps.push(Step::Pillar((base.0, base.1 + k, base.2)));
        }
        self.feet = cell_feet((base.0, base.1 + height, base.2));
        Ok(())
    }
    fn pillar_down(&mut self, base: Cell, height: i32) -> io::Result<()> {
        for k in (0..height).rev() {
            self.steps.push(Step::Break(BlockPos::new(base.0, base.1 + k, base.2)));
        }
        self.feet = cell_feet(base);
        Ok(())
    }
    fn break_block(&mut self, pos: BlockPos) -> io::Result<()> {
        self.steps.push(Step::Break(pos));
        Ok(())
    }
    fn place_support(&mut self, pos: BlockPos, click: &ClickStep) -> io::Result<()> {
        self.steps.push(Step::Support { pos, clicks: vec![click.clone()] });
        Ok(())
    }
}

fn click_steps(plan: &placement::Plan) -> Vec<ClickStep> {
    plan.clicks.iter().map(|c| ClickStep { support: c.support, face: c.face, cursor: c.cursor }).collect()
}

/// Cut `schem` into a plan for flat ground whose top is below `origin.1`.
pub fn slice(schem: &Schematic, origin: (i32, i32, i32), protocol: i32) -> Result<Plan, String> {
    let order = crate::tour::tour_order(schem);
    let start = (origin.0 - 2, origin.1, origin.2 - 2);
    let at = |b: &Block| BlockPos::new(origin.0 + b.pos.x, origin.1 + b.pos.y, origin.2 + b.pos.z);
    let positions: Vec<BlockPos> = order.iter().map(|b| at(b)).collect();
    let mut site = Site::new(Terrain::new(origin.1, HashSet::new()));
    let mut rec = Recorder { feet: cell_feet(start), steps: Vec::new() };
    let mut placed: HashSet<BlockPos> = HashSet::new();
    // placed blocks a plain click would open: never clicked against
    let mut usable: HashSet<BlockPos> = HashSet::new();
    let (mut count, mut hash, mut since_cp, mut cp_n) = (0u32, 0u64, 0usize, 0u32);
    let mut unreachable: Vec<(BlockPos, String)> = Vec::new();
    let mut deviations = 0usize;

    site.terrain.sky_top = positions.iter().map(|p| p.y).max().unwrap_or(origin.1) + 1;
    let mut todo: Vec<usize> = (0..order.len()).collect();
    let mut last_pass = false;
    loop {
    let mut deferred: Vec<usize> = Vec::new();
    for &i in &todo {
        let b = order[i];
        let world = positions[i];
        let state = b.state_string();
        if blocks::item_id(&b.name).is_none() {
            unreachable.push((world, format!("{}/unknown_block", b.name)));
            continue;
        }
        let solid = |p: BlockPos| p.y < origin.1 || placed.contains(&p);
        let act = placement::plan(&Block { pos: world, ..(*b).clone() }, solid);
        let pl = match act {
            Action::Skip(_) => {
                rec.steps.push(Step::Auto { pos: world, state: state.clone() });
                placed.insert(world);
                site.terrain.add_block((world.x, world.y, world.z), crate::nav::is_standable_surface(&b.name, b.prop("type")));
                count += 1;
                hash = hash.wrapping_add(block_hash(world, &state));
                continue;
            }
            Action::Place(p) => p,
        };
        let end = positions.len().min(i + LOOKAHEAD);
        site.approach(&mut rec, &positions[i..end]).map_err(|e| e.to_string())?;
        let here = feet_cell(rec.feet);
        if !within_reach(rec.feet, world) || blocks_cell(here, world) {
            if std::env::var("BUILDBOT_DUMP").is_ok() && unreachable.is_empty() {
                eprintln!("first out of reach: {world:?} {} from {here:?} (index {i}, last_pass {last_pass})", b.name);
                for y in (here.1 - 3..here.1 + 9).rev() {
                    let row: String = (here.0 - 14..here.0 + 14)
                        .map(|x| if (x, y, here.2) == here { '@' } else if (x, y, here.2) == (world.x, world.y, world.z) { 'T' } else if site.terrain.solid((x, y, here.2)) { '#' } else { '.' })
                        .collect();
                    eprintln!("y{y} {row}");
                }
            }
            unreachable.push((world, format!("{}/out_of_reach", b.name)));
            continue;
        }
        // Never seal the bot into a pocket it cannot leave: pick another standing cell, or put the
        // block off until the end.
        let (mut here, mut tries) = (here, 0);
        while site.terrain.sealed_in(here, (world.x, world.y, world.z), SEAL_CAP) {
            tries += 1;
            if tries > if last_pass { 12 } else { 6 } {
                break;
            }
            site.terrain.avoid.insert(here);
            site.approach(&mut rec, &positions[i..end]).map_err(|e| e.to_string())?;
            here = feet_cell(rec.feet);
            if !within_reach(rec.feet, world) || blocks_cell(here, world) {
                break;
            }
        }
        site.terrain.avoid.clear();
        let sealed = site.terrain.sealed_in(here, (world.x, world.y, world.z), SEAL_CAP);
        if !last_pass && sealed {
            deferred.push(i);
            continue;
        }
        if !within_reach(rec.feet, world) || blocks_cell(here, world) {
            unreachable.push((world, format!("{}/out_of_reach", b.name)));
            continue;
        }
        // Click only against blocks that are really within the server's click reach (docs/RE/placement.md).
        let feet_now = rec.feet;
        let near = |p: BlockPos| solid(p) && !usable.contains(&p) && crate::world::eye_to_box(feet_now, p) <= crate::world::CLICK_REACH;
        let mut pl = match placement::plan(&Block { pos: world, ..(*b).clone() }, near) {
            Action::Place(p) => p,
            Action::Skip(_) => pl,
        };
        let mut chain: Vec<support::Link> = Vec::new();
        if pl.clicks.iter().any(|c| !near(c.support) && c.support != world) {
            let cellp = |p: BlockPos| (p.x, p.y, p.z);
            let can_click = |p: BlockPos| site.terrain.solid(cellp(p));
            let blocked = |p: BlockPos| blocks_cell(here, p);
            match support::find_chain(world, rec.feet, &can_click, &blocked) {
                Some(c) => chain = c,
                None => {
                    unreachable.push((world, format!("{}/no_support", b.name)));
                    continue;
                }
            }
            let cells: HashSet<BlockPos> = chain.iter().map(|l| l.pos).collect();
            let with_chain = |p: BlockPos| near(p) || cells.contains(&p);
            match placement::plan(&Block { pos: world, ..(*b).clone() }, with_chain) {
                Action::Place(p2) if p2.clicks.iter().all(|c| with_chain(c.support) || c.support == world) => pl = p2,
                _ => {
                    unreachable.push((world, format!("{}/no_support", b.name)));
                    continue;
                }
            }
            for l in &chain {
                rec.steps.push(Step::Support { pos: l.pos, clicks: vec![l.click.clone()] });
            }
        }
        rec.steps.push(Step::Place { pos: world, state: state.clone(), yaw: pl.yaw, clicks: click_steps(&pl) });
        for l in chain.iter().rev() {
            rec.steps.push(Step::Break(l.pos));
        }
        deviations += pl.unhonoured.len();
        if last_pass {
            site.finish(&mut rec).map_err(|e| e.to_string())?;
        }
        placed.insert(world);
        site.terrain.add_block((world.x, world.y, world.z), crate::nav::is_standable_surface(&b.name, b.prop("type")));
        if placement::is_usable(&b.name) {
            usable.insert(world);
        }
        count += 1;
        hash = hash.wrapping_add(block_hash(world, &state));
        since_cp += 1;
        if since_cp >= CHECKPOINT_EVERY {
            since_cp = 0;
            cp_n += 1;
            rec.steps.push(Step::Checkpoint { n: cp_n, count, hash });
        }
    }
    if deferred.is_empty() || last_pass {
        break;
    }
    site.finish(&mut rec).map_err(|e| e.to_string())?;
    todo = deferred;
    last_pass = true;
    }
    let left = site.finish(&mut rec).map_err(|e| e.to_string())?;
    if left > 0 {
        return Err(format!("{left} scaffold pillar(s) could not be removed"));
    }
    cp_n += 1;
    rec.steps.push(Step::Checkpoint { n: cp_n, count, hash });

    let mut plan = Plan { header: Header::default(), steps: rec.steps };
    plan.header.schematic_hash = schem.fingerprint();
    plan.header.origin = origin;
    plan.header.start = start;
    plan.header.protocol = protocol;
    plan.header.unreachable = unreachable;
    crate::slice_stats::fill(&mut plan, schem.blocks.len(), deviations);
    plan.header.stats.push(("state_neighbour_derived".into(), crate::slice_stats::neighbour_derived(schem).to_string()));
    if let Err(e) = sim::simulate(&plan, schem) {
        if let Ok(p) = std::env::var("BUILDBOT_FAILED_PLAN") {
            let _ = std::fs::write(p, plan.to_text());
        }
        return Err(format!("slicer bug: plan fails simulation at step {}: {}", e.step + 1, e.msg));
    }
    sim::simulate(&plan, schem).map_err(|e| format!("slicer bug: plan fails simulation at step {}: {}", e.step, e.msg))?;
    Ok(plan)
}
