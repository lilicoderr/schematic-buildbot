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
use crate::world::{within_reach, BlockPos, Vec3};
use std::collections::HashSet;
use std::io;

/// Upcoming blocks considered when choosing where to stand (offline, so generous).
pub const LOOKAHEAD: usize = 96;
/// A checkpoint is written after this many placements.
pub const CHECKPOINT_EVERY: usize = 250;

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
}

fn click_steps(plan: &placement::Plan) -> Vec<ClickStep> {
    plan.clicks.iter().map(|c| ClickStep { support: c.support, face: c.face, cursor: c.cursor }).collect()
}

/// Cut `schem` into a plan for flat ground whose top is below `origin.1`.
pub fn slice(schem: &Schematic, origin: (i32, i32, i32), protocol: i32) -> Result<Plan, String> {
    let (order, _) = schem.plan_order();
    let start = (origin.0 - 2, origin.1, origin.2 - 2);
    let at = |b: &Block| BlockPos::new(origin.0 + b.pos.x, origin.1 + b.pos.y, origin.2 + b.pos.z);
    let positions: Vec<BlockPos> = order.iter().map(|b| at(b)).collect();
    let mut site = Site::new(Terrain::new(origin.1, HashSet::new()));
    let mut rec = Recorder { feet: cell_feet(start), steps: Vec::new() };
    let mut placed: HashSet<BlockPos> = HashSet::new();
    let (mut count, mut hash, mut since_cp, mut cp_n) = (0u32, 0u64, 0usize, 0u32);
    let mut unreachable: Vec<(BlockPos, String)> = Vec::new();
    let mut deviations = 0usize;

    for (i, b) in order.iter().enumerate() {
        let world = positions[i];
        let state = b.state_string();
        if blocks::item_id(&b.name).is_none() {
            unreachable.push((world, b.name.clone()));
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
        if pl.clicks.iter().any(|c| !solid(c.support) && c.support != world) {
            unreachable.push((world, format!("{} (nothing to click against)", b.name)));
            continue;
        }
        let end = positions.len().min(i + LOOKAHEAD);
        site.approach(&mut rec, &positions[i..end]).map_err(|e| e.to_string())?;
        let here = feet_cell(rec.feet);
        if !within_reach(rec.feet, world) || blocks_cell(here, world) {
            unreachable.push((world, b.name.clone()));
            continue;
        }
        rec.steps.push(Step::Place { pos: world, state: state.clone(), yaw: pl.yaw, clicks: click_steps(&pl) });
        deviations += pl.unhonoured.len();
        placed.insert(world);
        site.terrain.add_block((world.x, world.y, world.z), crate::nav::is_standable_surface(&b.name, b.prop("type")));
        count += 1;
        hash = hash.wrapping_add(block_hash(world, &state));
        since_cp += 1;
        if since_cp >= CHECKPOINT_EVERY {
            since_cp = 0;
            cp_n += 1;
            rec.steps.push(Step::Checkpoint { n: cp_n, count, hash });
        }
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
    sim::simulate(&plan, schem).map_err(|e| format!("slicer bug: plan fails simulation at step {}: {}", e.step, e.msg))?;
    Ok(plan)
}
