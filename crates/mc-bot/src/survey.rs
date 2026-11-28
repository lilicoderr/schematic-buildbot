//! Site survey: before anything is placed, compare the loaded chunk data around the footprint
//! with what the plan needs, so a build never runs into somebody's blocks.
//!
//! * A cell the plan places into, pillars through or walks through must be empty (air or
//!   vegetation a placement replaces), whatever stands there.
//! * In the rest of the box (the footprint plus [`MARGIN`]) natural terrain is ignored; any
//!   other block stops the build.
//! * Every column of the footprint needs ground that carries the player.

use crate::exec_repair::Offset;
use crate::nav::Cell;
use crate::plan::{Plan, Step};
use crate::schematic::split_state;
use crate::sim::{self, World, SCAFFOLD};
use crate::tr;
use crate::world::BlockPos;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Clearance kept around the plan, to the sides and above. At least as wide as the strip the
/// repair watches ([`crate::ledger`]).
pub const MARGIN: i32 = 2;
/// On resume, steps after the saved one that may already stand in the world.
const UNSAVED_STEPS: usize = 8;

pub type Props = BTreeMap<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Air, or vegetation that a placed block replaces.
    Free,
    /// World generation: harmless beside the build, in the way inside it.
    Natural,
    /// Anything else: assumed to be somebody's work.
    Built,
}

pub fn classify(name: &str, props: &Props) -> Kind {
    const FREE: [&str; 9] = ["air", "cave_air", "void_air", "short_grass", "grass", "tall_grass", "fern", "large_fern", "dead_bush"];
    if FREE.contains(&name) || (name == "snow" && props.get("layers").is_some_and(|l| l == "1")) {
        return Kind::Free;
    }
    const NATURAL: [&str; 62] = [
        "grass_block", "dirt", "coarse_dirt", "rooted_dirt", "podzol", "mycelium", "mud", "clay", "gravel", "sand",
        "red_sand", "sandstone", "red_sandstone", "stone", "granite", "diorite", "andesite", "deepslate", "tuff",
        "calcite", "dripstone_block", "pointed_dripstone", "bedrock", "water", "lava", "bubble_column", "snow",
        "snow_block", "powder_snow", "ice", "packed_ice", "blue_ice", "moss_block", "moss_carpet", "seagrass",
        "tall_seagrass", "kelp", "kelp_plant", "vine", "glow_lichen", "lily_pad", "sugar_cane", "cactus", "bamboo",
        "sweet_berry_bush", "dandelion", "poppy", "blue_orchid", "allium", "azure_bluet", "oxeye_daisy",
        "cornflower", "lily_of_the_valley", "wither_rose", "sunflower", "lilac", "rose_bush", "peony",
        "brown_mushroom", "red_mushroom", "netherrack", "end_stone",
    ];
    let wild_leaves = name.ends_with("_leaves") && props.get("persistent").is_some_and(|p| p == "false");
    if NATURAL.contains(&name) || wild_leaves || name.ends_with("_ore") || name.ends_with("_tulip") {
        Kind::Natural
    } else {
        Kind::Built
    }
}

/// What a plan needs of the world, in world coordinates.
pub struct Area {
    /// Cells the plan places into or moves the player through.
    pub used: HashSet<BlockPos>,
    /// Lowest and highest used cell.
    pub lo: BlockPos,
    pub hi: BlockPos,
    /// Lowest feet height: the ground is the layer below.
    pub ground_y: i32,
    /// Blocks an earlier run of this very build may have left standing (resume).
    pub ours: HashMap<BlockPos, HashSet<String>>,
}

/// Cells the player passes through going from `from` to `to`.
fn passed(from: Cell, to: Cell) -> Vec<Cell> {
    let mut out: Vec<Cell> = (from.1.min(to.1)..=from.1.max(to.1) + 1).map(|y| (to.0, y, to.2)).collect();
    if to.1 > from.1 {
        out.push((from.0, from.1 + 2, from.2)); // head room for the jump
    }
    out
}

/// The cells of `plan` moved by `off`. On a resume, `saved` is the last saved step of the earlier
/// run and `extras` the scaffold it left outside the plan.
pub fn area(plan: &Plan, off: Offset, saved: Option<usize>, extras: &[Cell]) -> Result<Area, String> {
    let ground = plan.header.origin.1;
    let mut world = World::new(ground, plan.header.start);
    let mut used: HashSet<BlockPos> = HashSet::new();
    let mut ours: HashMap<BlockPos, HashSet<String>> = HashMap::new();
    let at = |c: Cell| {
        let c = off.cell(c);
        BlockPos::new(c.0, c.1, c.2)
    };
    let start = plan.header.start;
    used.extend([at(start), at((start.0, start.1 + 1, start.2))]);
    let resumed = saved.map_or(0, |s| s + UNSAVED_STEPS);
    for (i, st) in plan.steps.iter().enumerate() {
        let before = world.feet;
        sim::apply(&mut world, st).map_err(|m| format!("step {}: {m}", i + 1))?;
        let block = match st {
            Step::Place { pos, state, .. } | Step::Auto { pos, state } => Some(((pos.x, pos.y, pos.z), split_state(state).0)),
            Step::Support { pos, .. } => Some(((pos.x, pos.y, pos.z), SCAFFOLD.to_string())),
            Step::Pillar(c) => Some((*c, SCAFFOLD.to_string())),
            _ => None,
        };
        if let Some((c, name)) = block {
            used.insert(at(c));
            if i < resumed {
                ours.entry(at(c)).or_default().insert(name);
            }
        }
        if world.feet != before {
            used.extend(passed(before, world.feet).into_iter().map(at));
        }
    }
    if saved.is_some() {
        for &c in extras {
            ours.entry(BlockPos::new(c.0, c.1, c.2)).or_default().insert(SCAFFOLD.into());
        }
    }
    let first = at(start);
    let (mut lo, mut hi) = (first, first);
    for p in &used {
        lo = BlockPos::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = BlockPos::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    Ok(Area { used, lo, hi, ground_y: ground + off.dy, ours })
}

impl Area {
    /// Every chunk column the survey reads.
    pub fn chunks(&self) -> BTreeSet<(i32, i32)> {
        let mut out = BTreeSet::new();
        for x in (self.lo.x - MARGIN) >> 4..=(self.hi.x + MARGIN) >> 4 {
            for z in (self.lo.z - MARGIN) >> 4..=(self.hi.z + MARGIN) >> 4 {
                out.insert((x, z));
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub pos: BlockPos,
    pub block: String,
}

#[derive(Debug, Default)]
pub struct Survey {
    /// Occupied cells the plan needs.
    pub collisions: Vec<Found>,
    /// Non-natural blocks beside or above the plan, within the margin.
    pub nearby: Vec<Found>,
    /// Footprint columns whose ground does not carry the player (the block found there).
    pub no_ground: Vec<Found>,
    /// Cells whose chunk was never received: unknown, so not safe.
    pub unknown: usize,
    /// Blocks that stood beside the plan before the build: never to be broken.
    pub keep: HashSet<BlockPos>,
}

/// Survey `area`; `block_at` gives name and properties, `None` for an unloaded chunk.
pub fn survey(area: &Area, block_at: &dyn Fn(BlockPos) -> Option<(String, Props)>) -> Survey {
    let mut out = Survey::default();
    for y in area.ground_y..=area.hi.y + MARGIN {
        for z in area.lo.z - MARGIN..=area.hi.z + MARGIN {
            for x in area.lo.x - MARGIN..=area.hi.x + MARGIN {
                let pos = BlockPos::new(x, y, z);
                let Some((name, props)) = block_at(pos) else {
                    out.unknown += 1;
                    continue;
                };
                let kind = classify(&name, &props);
                if area.ours.get(&pos).is_some_and(|names| names.contains(&name)) {
                    continue;
                }
                let used = area.used.contains(&pos);
                if !used && !matches!(name.as_str(), "air" | "cave_air" | "void_air") {
                    out.keep.insert(pos);
                }
                match kind {
                    Kind::Free => {}
                    _ if used => out.collisions.push(Found { pos, block: name }),
                    Kind::Built => out.nearby.push(Found { pos, block: name }),
                    Kind::Natural => {}
                }
            }
        }
    }
    for z in area.lo.z..=area.hi.z {
        for x in area.lo.x..=area.hi.x {
            let pos = BlockPos::new(x, area.ground_y - 1, z);
            match block_at(pos) {
                Some((name, props)) if !crate::sim_rules::full_top(&name, &props) => out.no_ground.push(Found { pos, block: name }),
                Some(_) => {}
                None => out.unknown += 1,
            }
        }
    }
    out
}

/// `title`, how often each block occurs and where the first few are.
fn section(out: &mut Vec<String>, title: String, list: &[Found]) {
    if list.is_empty() {
        return;
    }
    out.push(format!("  {title}: {}", list.len()));
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for f in list {
        *counts.entry(&f.block).or_default() += 1;
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by_key(|&(name, n)| (std::cmp::Reverse(n), name));
    let shown: Vec<String> = counts.iter().take(6).map(|(name, n)| format!("{name} x{n}")).collect();
    out.push(format!("    {}{}", shown.join(", "), if counts.len() > 6 { ", ..." } else { "" }));
    for f in list.iter().take(5) {
        out.push(format!("    {},{},{}  {}", f.pos.x, f.pos.y, f.pos.z, f.block));
    }
}

impl Survey {
    pub fn is_clear(&self) -> bool {
        self.collisions.is_empty() && self.nearby.is_empty() && self.no_ground.is_empty() && self.unknown == 0
    }

    pub fn report(&self, area: &Area) -> Vec<String> {
        let (a, b) = (area.lo, area.hi);
        let mut out = vec![tr!(
            "site survey: footprint {},{},{} .. {},{},{} plus a {}-block margin",
            "부지 조사: 작업 영역 {},{},{} .. {},{},{} 과 둘레 {}칸",
            a.x, a.y, a.z, b.x, b.y, b.z, MARGIN
        )];
        section(&mut out, tr!("blocks where the build or the bot has to go", "건축이나 봇이 지나갈 자리를 막는 블록"), &self.collisions);
        section(&mut out, tr!("non-natural blocks within the margin", "둘레 안에 있는 자연 지형이 아닌 블록"), &self.nearby);
        section(&mut out, tr!("footprint columns without solid ground (block found there)", "바닥이 없는 칸 (그 자리의 블록)"), &self.no_ground);
        if self.unknown > 0 {
            out.push(tr!(
                "  cells in chunks the server did not send: {} (footprint larger than the view distance?)",
                "  서버가 청크를 보내지 않아 확인할 수 없는 칸: {} (작업 영역이 시야 거리보다 큰가요?)",
                self.unknown
            ));
        }
        if self.is_clear() {
            out.push(tr!("  the site is clear", "  부지가 비어 있습니다"));
        }
        out
    }
}

#[cfg(test)]
#[path = "survey_tests.rs"]
mod tests;
