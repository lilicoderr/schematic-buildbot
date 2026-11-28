//! The slicer: compiles a schematic into a [`Plan`] offline, with no server.
//!
//! It runs the same planner the live bot used (layer order, tiled sweep, lookahead standing
//! cells, jump/drop navigation, scaffold pillars), but against a recorder that writes plan steps.
//! The result is then replayed by [`crate::sim::simulate`]; [`slice`] refuses to return a plan
//! that fails simulation.

use crate::blocks;
use crate::guard::Budget;
use crate::orient;
use crate::nav::{blocks_cell, cell_feet, feet_cell, Cell, Terrain};
use crate::placement::{self, Action};
use crate::plan::{block_hash, ClickStep, Header, Plan, Step};
use crate::scaffold::{Mover, Site};
use crate::schematic::{Block, Schematic};
use crate::sim;
use crate::support;
use crate::tour::Order;
use crate::world::{Face, within_reach, BlockPos, Vec3};
use std::collections::{HashMap, HashSet};
use std::io;

/// Upcoming blocks considered when choosing where to stand (offline, so generous).
pub const LOOKAHEAD: usize = 96;
/// On reaching a new band of layers, scaffold this far below it is taken down again.
const TIDY_BELOW: i32 = 1;
/// A checkpoint is written after this many placements.
pub const CHECKPOINT_EVERY: usize = 250;
/// A pocket smaller than this many standing cells (no way to the open ground) counts as sealed.
const SEAL_CAP: usize = 20_000;
/// This many blocks in a row without a standing spot means the bot is stuck: stop and say where.
const STALL_LIMIT: usize = 40;
/// Other standing cells tried when none of the clicks for the wanted state works from here.
const AIM_TRIES: usize = 6;

/// Where planning time went in the last `slice` call on this thread (printed by the CLI; kept out
/// of the plan so plans stay byte-identical).
#[derive(Debug, Clone, Copy, Default)]
pub struct Times {
    pub tour: std::time::Duration,
    pub approach: std::time::Duration,
    pub sealing: std::time::Duration,
    pub chain: std::time::Duration,
    pub simulate: std::time::Duration,
    pub cleanup: std::time::Duration,
}

thread_local! {
    static TIMES: std::cell::Cell<Times> = std::cell::Cell::new(Times::default());
}

/// Timing breakdown of the last [`slice`] call on this thread.
pub fn last_times() -> Times {
    TIMES.with(|t| t.get())
}

fn add(f: impl FnOnce(&mut Times), t0: std::time::Instant, field: fn(&mut Times) -> &mut std::time::Duration) {
    TIMES.with(|t| {
        let mut v = t.get();
        *field(&mut v) += t0.elapsed();
        f(&mut v);
        t.set(v);
    });
}

/// Records movement as plan steps instead of moving anybody.
pub(crate) struct Recorder {
    pub(crate) feet: Vec3,
    pub(crate) steps: Vec<Step>,
    /// Scaffold blocks currently standing; a break of anything else is a planner slip and is dropped.
    pub(crate) alive: HashSet<BlockPos>,
}

impl Recorder {
    pub(crate) fn new(start: Cell) -> Self {
        Recorder { feet: cell_feet(start), steps: Vec::new(), alive: HashSet::new() }
    }
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
    fn pillar_up(&mut self, base: Cell, _item: i32) -> io::Result<()> {
        self.steps.push(Step::Pillar(base));
        self.alive.insert(BlockPos::new(base.0, base.1, base.2));
        self.feet = cell_feet((base.0, base.1 + 1, base.2));
        Ok(())
    }
    fn dig_down(&mut self, floor: BlockPos, land_y: i32) -> io::Result<()> {
        self.break_block(floor)?;
        self.feet = cell_feet((floor.x, land_y, floor.z));
        Ok(())
    }
    fn break_block(&mut self, pos: BlockPos) -> io::Result<()> {
        if self.alive.remove(&pos) {
            self.steps.push(Step::Break(pos));
        }
        Ok(())
    }
    fn place_support(&mut self, pos: BlockPos, click: &ClickStep) -> io::Result<()> {
        self.alive.insert(pos);
        self.steps.push(Step::Support { pos, clicks: vec![click.clone()] });
        Ok(())
    }
}

fn click_steps(plan: &placement::Plan) -> Vec<ClickStep> {
    plan.clicks.iter().map(|c| ClickStep { support: c.support, face: c.face, cursor: c.cursor }).collect()
}

/// Cut `schem` into a plan for flat ground whose top is below `origin.1`.
pub fn slice(schem: &Schematic, origin: (i32, i32, i32), protocol: i32) -> Result<Plan, String> {
    // generous limits for library use and tests; the CLI passes tighter ones
    slice_with(schem, origin, protocol, &mut Budget::new(std::time::Duration::from_secs(3600), usize::MAX))
}

/// Like [`slice`], under a time and memory [`Budget`] (progress lines every 10 s on stderr).
///
/// [`Order::BANDED`] walks much less; if it ends in a dead end (scaffold that cannot be taken
/// down, nowhere to stand) or leaves blocks out, the structure is planned again with
/// [`Order::LAYERED`] and the better result is kept.
pub fn slice_with(schem: &Schematic, origin: (i32, i32, i32), protocol: i32, budget: &mut Budget) -> Result<Plan, String> {
    TIMES.with(|t| t.set(Times::default()));
    let missing = |p: &Plan| p.header.unreachable.len();
    let banded = slice_ordered(schem, origin, protocol, budget, Order::BANDED);
    if banded.as_ref().is_ok_and(|p| missing(p) == 0) {
        return banded;
    }
    if let Err(e) = &banded {
        eprintln!("  banded build order failed ({e}); planning layer by layer");
    }
    match (banded, slice_ordered(schem, origin, protocol, budget, Order::LAYERED)) {
        (Ok(a), Ok(b)) => Ok(if missing(&b) < missing(&a) { b } else { a }),
        (Ok(a), Err(_)) => Ok(a),
        (Err(_), Ok(b)) => Ok(b),
        (Err(first), Err(second)) => Err(format!("{second} (banded order: {first})")),
    }
}

/// What one bot of a cooperative build starts with ([`crate::multi`]); the default is the single
/// bot on empty ground.
#[derive(Debug, Clone, Default)]
pub struct Setup {
    /// Feet cell the plan starts from (default: beside the origin).
    pub start: Option<Cell>,
    /// The bot stands, places and builds scaffold only inside.
    pub fence: crate::territory::Fence,
    /// Blocks other bots have finished before this plan starts.
    pub prebuilt: Vec<(BlockPos, String)>,
}

/// [`slice_with`] for one build order only.
pub fn slice_ordered(schem: &Schematic, origin: (i32, i32, i32), protocol: i32, budget: &mut Budget, kind: Order) -> Result<Plan, String> {
    slice_setup(schem, origin, protocol, budget, kind, &Setup::default())
}

/// [`slice_ordered`] for one bot's part of a cooperative build: `schem` holds only the blocks
/// this bot places.
pub fn slice_setup(schem: &Schematic, origin: (i32, i32, i32), protocol: i32, budget: &mut Budget, kind: Order, setup: &Setup) -> Result<Plan, String> {
    let t_tour = std::time::Instant::now();
    let order = crate::tour::tour_order(schem, kind);
    add(|_| {}, t_tour, |t| &mut t.tour);
    let start = setup.start.unwrap_or((origin.0 - 2, origin.1, origin.2 - 2));
    let fence = setup.fence;
    let at = |b: &Block| BlockPos::new(origin.0 + b.pos.x, origin.1 + b.pos.y, origin.2 + b.pos.z);
    let positions: Vec<BlockPos> = order.iter().map(|b| at(b)).collect();
    let mut site = Site::new(Terrain::new(origin.1, HashSet::new()));
    let mut rec = Recorder { feet: cell_feet(start), steps: Vec::new(), alive: HashSet::new() };
    let mut placed: HashSet<BlockPos> = HashSet::new();
    // placed blocks a plain click would open: never clicked against
    let mut usable: HashSet<BlockPos> = HashSet::new();
    // placed slabs, so a click never merges into its support by accident
    let mut slabs: HashMap<BlockPos, Block> = HashMap::new();
    let (mut count, mut hash, mut since_cp, mut cp_n) = (0u32, 0u64, 0usize, 0u32);
    let mut unreachable: Vec<(BlockPos, String)> = Vec::new();
    let mut deviations = 0usize;
    // blocks that will end up in another state than the schematic asks for: "x y z state props"
    let mut fallbacks: Vec<String> = Vec::new();
    let mut stalled = 0usize;

    site.terrain.fence = fence;
    for (p, state) in &setup.prebuilt {
        let (name, props) = crate::schematic::split_state(state);
        let standable = crate::nav::is_standable_surface(&name, props.get("type").map(String::as_str));
        site.terrain.add_block((p.x, p.y, p.z), standable);
        placed.insert(*p);
        if placement::is_usable(&name) {
            usable.insert(*p);
        }
        if name.ends_with("_slab") {
            slabs.insert(*p, Block::from_state(*p, state));
        }
        count += 1;
        hash = hash.wrapping_add(block_hash(*p, state));
    }
    let top = positions.iter().copied().chain(setup.prebuilt.iter().map(|b| b.0)).map(|p| p.y).max();
    site.terrain.sky_top = top.unwrap_or(origin.1) + 1;
    // world height of the lowest layer: bands of layers are counted from here
    let floor = positions.iter().map(|p| p.y).min().unwrap_or(origin.1);
    let mut todo: Vec<usize> = (0..order.len()).collect();
    let mut last_pass = false;
    loop {
    let mut deferred: Vec<usize> = Vec::new();
    let mut queue = crate::batch::Queue::new(todo.iter().copied());
    let mut layer = i32::MIN;
    loop {
        // place everything in reach before walking on; a block is "ready" when it has something to
        // rest on and its wanted state can be produced right now
        let next = queue.take(&positions, rec.feet, |a, b| kind.together(a, b, floor), |k| {
            let p = positions[k];
            let solid = |q: BlockPos| q.y < origin.1 || placed.contains(&q);
            let rests = p.y == origin.1 || [p.below(), p.offset(Face::North), p.offset(Face::South), p.offset(Face::East), p.offset(Face::West)].iter().any(|q| placed.contains(q));
            rests && matches!(placement::plan(&Block { pos: p, ..(*order[k]).clone() }, solid), Action::Place(a) if a.unhonoured.is_empty())
        });
        let Some(i) = next else { break };
        budget.check(if last_pass { "deferred blocks" } else { "placing blocks" }, &|| format!("block {i}/{} ({} placed, {} steps)", order.len(), count, rec.steps.len()))?;
        let b = order[i];
        let world = positions[i];
        let state = b.state_string();
        let base_y = crate::tour::band_base(world.y, floor);
        if !last_pass && base_y > layer {
            layer = base_y;
            let t_tidy = std::time::Instant::now();
            site.tidy(&mut rec, layer - TIDY_BELOW).map_err(|e| e.to_string())?;
            add(|_| {}, t_tidy, |t| &mut t.cleanup);
        }
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
        let upcoming = queue.upcoming(&positions, i, LOOKAHEAD);
        let t0 = std::time::Instant::now();
        site.approach(&mut rec, &upcoming).map_err(|e| e.to_string())?;
        add(|_| {}, t0, |t| &mut t.approach);
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
            if last_pass {
                unreachable.push((world, format!("{}/out_of_reach", b.name)));
            } else {
                deferred.push(i);
            }
            stalled += 1;
            if stalled >= STALL_LIMIT {
                return Err(format!(
                    "planner gave up: placing blocks (no standing spot for {STALL_LIMIT} blocks in a row; bot at {here:?}, {} cells walkable, open sky {}; last target {world:?} {}; block {i}/{})",
                    site.terrain.reachable_count(here, 200, 200_000), site.terrain.sky_open(here), b.name, order.len()
                ));
            }
            continue;
        }
        stalled = 0;
        // Never seal the bot into a pocket it cannot leave: pick another standing cell, or put the
        // block off until the end.
        let t_seal = std::time::Instant::now();
        let (mut here, mut tries) = (here, 0);
        while site.terrain.sealed_in(here, (world.x, world.y, world.z), SEAL_CAP) {
            tries += 1;
            if tries > if last_pass { 12 } else { 6 } {
                break;
            }
            site.terrain.avoid.insert(here);
            site.approach(&mut rec, &upcoming).map_err(|e| e.to_string())?;
            here = feet_cell(rec.feet);
            if !within_reach(rec.feet, world) || blocks_cell(here, world) {
                break;
            }
        }
        site.terrain.avoid.clear();
        let sealed = site.terrain.sealed_in(here, (world.x, world.y, world.z), SEAL_CAP);
        add(|_| {}, t_seal, |t| &mut t.sealing);
        if !last_pass && sealed {
            deferred.push(i);
            continue;
        }
        if !within_reach(rec.feet, world) || blocks_cell(here, world) {
            if last_pass {
                unreachable.push((world, format!("{}/out_of_reach", b.name)));
            } else {
                deferred.push(i);
            }
            continue;
        }
        // A state that needs a face no neighbour offers (a pillar axis, say) is clicked on a
        // temporary block, and that may only fit from another standing cell.
        let wanted = Block { pos: world, ..(*b).clone() };
        let rule = orient::rule(&wanted);
        let lost = |p: &placement::Plan| !rule.allows(p.clicks[0].face, p.clicks[0].cursor.1);
        let mut aimed = None;
        let unclickable = rule != orient::Rule::Any && pl.missing_support.is_none() && {
            let ok = |c: &placement::Click| {
                solid(c.support) && !usable.contains(&c.support) && crate::world::eye_to_box(rec.feet, c.support) <= crate::world::CLICK_REACH
                    && !slabs.get(&c.support).is_some_and(|s| orient::merges_into(s, &b.name, c.face, c.cursor.1))
            };
            matches!(placement::plan_clicks(&wanted, ok), Action::Place(p) if lost(&p) || !p.clicks.iter().all(|c| c.support == world || ok(c)))
        };
        if unclickable {
            for _ in 0..=AIM_TRIES {
                let feet = rec.feet;
                let cellp = |p: BlockPos| (p.x, p.y, p.z);
                // standing blocks, scaffold included
                let clickable = |p: BlockPos| {
                    (solid(p) || rec.alive.contains(&p)) && !usable.contains(&p) && crate::world::eye_to_box(feet, p) <= crate::world::CLICK_REACH
                };
                let safe = |c: &placement::Click| !slabs.get(&c.support).is_some_and(|s| orient::merges_into(s, &b.name, c.face, c.cursor.1));
                aimed = support::aim(&wanted, feet, &|p| site.terrain.solid(cellp(p)), &|p| blocks_cell(here, p) || !fence.has(p.x, p.z), &clickable, &safe);
                if aimed.is_some() {
                    break;
                }
                site.terrain.avoid.insert(here);
                site.approach(&mut rec, &upcoming).map_err(|e| e.to_string())?;
                let next = feet_cell(rec.feet);
                if next == here || !within_reach(rec.feet, world) || blocks_cell(next, world) || site.terrain.sealed_in(next, cellp(world), SEAL_CAP) {
                    here = next;
                    break;
                }
                here = next;
            }
            site.terrain.avoid.clear();
            let mut trapped = || !last_pass && site.terrain.sealed_in(here, (world.x, world.y, world.z), SEAL_CAP);
            if aimed.is_none() && (!within_reach(rec.feet, world) || blocks_cell(here, world) || trapped()) {
                if !last_pass {
                    deferred.push(i);
                    continue;
                }
                unreachable.push((world, format!("{}/out_of_reach", b.name)));
                continue;
            }
        }
        // Click only against blocks that are really within the server's click reach (docs/RE/placement.md).
        let feet_now = rec.feet;
        let near = |p: BlockPos| solid(p) && !usable.contains(&p) && crate::world::eye_to_box(feet_now, p) <= crate::world::CLICK_REACH;
        let safe = |c: &placement::Click| {
            !slabs.get(&c.support).is_some_and(|s| orient::merges_into(s, &b.name, c.face, c.cursor.1))
        };
        let mut pl = match placement::plan_clicks(&Block { pos: world, ..(*b).clone() }, |c| near(c.support) && safe(c)) {
            Action::Place(p) => p,
            Action::Skip(_) => pl,
        };
        if pl.missing_support.is_some() {
            // attached block whose support is not there yet: scaffold would pop it off later
            if last_pass {
                unreachable.push((world, format!("{}/no_support", b.name)));
            } else {
                deferred.push(i);
            }
            continue;
        }
        let mut chain: Vec<support::Link> = Vec::new();
        if let Some((links, p2)) = aimed {
            (chain, pl) = (links, p2);
            for l in &chain {
                rec.alive.insert(l.pos);
                rec.steps.push(Step::Support { pos: l.pos, clicks: vec![l.click.clone()] });
            }
        // `plan_clicks` falls back to a floor click when nothing is usable, even one that would merge
        } else if pl.clicks.iter().any(|c| c.support != world && (!near(c.support) || !safe(c))) {
            let cellp = |p: BlockPos| (p.x, p.y, p.z);
            let can_click = |p: BlockPos| site.terrain.solid(cellp(p));
            let blocked = |p: BlockPos| blocks_cell(here, p) || !fence.has(p.x, p.z);
            let t_chain = std::time::Instant::now();
            let found = support::find_chain(world, rec.feet, &can_click, &blocked);
            add(|_| {}, t_chain, |t| &mut t.chain);
            match found {
                Some(c) => chain = c,
                None if !last_pass => {
                    deferred.push(i);
                    continue;
                }
                None => {
                    unreachable.push((world, format!("{}/no_support", b.name)));
                    continue;
                }
            }
            let cells: HashSet<BlockPos> = chain.iter().map(|l| l.pos).collect();
            let with_chain = |p: BlockPos| near(p) || cells.contains(&p);
            match placement::plan_clicks(&Block { pos: world, ..(*b).clone() }, |c| with_chain(c.support) && safe(c)) {
                Action::Place(p2) if p2.clicks.iter().all(|c| c.support == world || (with_chain(c.support) && safe(c))) => pl = p2,
                _ if !last_pass => {
                    deferred.push(i);
                    continue;
                }
                _ => {
                    unreachable.push((world, format!("{}/no_support", b.name)));
                    continue;
                }
            }
            if !last_pass && lost(&pl) {
                deferred.push(i);
                continue;
            }
            for l in &chain {
                rec.alive.insert(l.pos);
                rec.steps.push(Step::Support { pos: l.pos, clicks: vec![l.click.clone()] });
            }
        }
        // from another standing spot, later, there may be room for that temporary block
        if !last_pass && lost(&pl) {
            deferred.push(i);
            continue;
        }
        rec.steps.push(Step::Place { pos: world, state: state.clone(), yaw: pl.yaw, clicks: click_steps(&pl) });
        for l in chain.iter().rev() {
            rec.alive.remove(&l.pos);
            rec.steps.push(Step::Break(l.pos));
        }
        deviations += pl.unhonoured.len();
        if !pl.unhonoured.is_empty() {
            fallbacks.push(format!("{} {} {} {state} {}", world.x, world.y, world.z, pl.unhonoured.join(",")));
        }
        placed.insert(world);
        if b.name.ends_with("_slab") {
            slabs.insert(world, Block { pos: world, ..(*b).clone() });
        }
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
    let t_clean = std::time::Instant::now();
    let mut over_budget = None;
    let left = site
        .finish_while(&mut rec, &mut |left| match budget.check("removing scaffold", &|| format!("{left} blocks left")) {
            Ok(()) => true,
            Err(e) => {
                over_budget = Some(e);
                false
            }
        })
        .map_err(|e| e.to_string())?;
    if let Some(e) = over_budget {
        return Err(e);
    }
    add(|_| {}, t_clean, |t| &mut t.cleanup);
    budget.check("cleanup", &|| format!("{} steps", rec.steps.len()))?;
    if left > 0 {
        return Err(format!("{left} scaffold block(s) could not be removed"));
    }
    cp_n += 1;
    rec.steps.push(Step::Checkpoint { n: cp_n, count, hash });

    let mut plan = Plan { header: Header::default(), steps: rec.steps };
    plan.header.schematic_hash = schem.fingerprint();
    plan.header.origin = origin;
    plan.header.start = start;
    plan.header.protocol = protocol;
    plan.header.unreachable = unreachable;
    crate::shorten::shorten_in(&mut plan, fence, &setup.prebuilt);
    crate::slice_stats::fill(&mut plan, schem.blocks.len(), deviations);
    plan.header.stats.push(("state_neighbour_derived".into(), crate::slice_stats::neighbour_derived(schem).to_string()));
    crate::slice_stats::fill_mismatches(&mut plan);
    plan.header.stats.extend(fallbacks.into_iter().map(|f| ("fallback_block".to_string(), f)));
    let t_sim = std::time::Instant::now();
    let mut whole = schem.clone();
    whole.blocks.extend(setup.prebuilt.iter().map(|(p, s)| {
        Block::from_state(BlockPos::new(p.x - origin.0, p.y - origin.1, p.z - origin.2), s)
    }));
    let sim_result = sim::simulate_in(&plan, &whole, &setup.prebuilt);
    add(|_| {}, t_sim, |t| &mut t.simulate);
    if let Err(e) = sim_result {
        if let Ok(p) = std::env::var("BUILDBOT_FAILED_PLAN") {
            let _ = std::fs::write(p, plan.to_text());
        }
        return Err(format!("slicer bug: plan fails simulation at step {}: {}", e.step + 1, e.msg));
    }
    Ok(plan)
}
