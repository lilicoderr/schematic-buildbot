//! Cuts a schematic into one strip per bot and plans every strip on its own, then moves the
//! cuts to even out the bots' estimated times.

use super::{joint, MultiPlan, Part, MAX_BOTS};
use crate::schematic::{Block, Schematic};
use crate::territory::{Axis, Fence};
use crate::world::{BlockPos, Face};
use std::collections::HashMap;

#[path = "part_plan.rs"]
mod part_plan;

/// Narrowest strip, in blocks.
pub const MIN_WIDTH: i32 = 3;
/// Cut positions tried per axis while balancing.
const EVALS: usize = 24;

pub(super) fn along(axis: Axis, p: BlockPos) -> i32 {
    match axis {
        Axis::X => p.x,
        Axis::Z => p.z,
    }
}

/// A cut between coordinates `c - 1` and `c` must not separate blocks that only come out right
/// when one bot orders them: bed halves, a block and what it hangs on, double chests, and doors
/// with anything beside them (the neighbours decide the hinge).
pub fn legal_cut(at: &HashMap<BlockPos, &Block>, axis: Axis, c: i32) -> bool {
    let cross = |a: BlockPos, b: BlockPos| (along(axis, a) < c) != (along(axis, b) < c);
    at.iter().filter(|(p, _)| (along(axis, **p) - c).abs() <= 1).all(|(&p, b)| {
        let props = &b.props;
        let hangs = crate::sim_rules::needs_support(&b.name, props, p).is_some_and(|s| cross(p, s));
        let bed = b.name.ends_with("_bed") && b.prop("part") != Some("head") && crate::sim_rules::second_half(&b.name, props, p).is_some_and(|o| cross(p, o));
        let beside = |f: &Face| {
            let q = p.offset(*f);
            cross(p, q) && at.get(&q).is_some_and(|n| b.name.ends_with("_door") || (b.name.ends_with("chest") && n.name == b.name))
        };
        !(hangs || bed || Face::HORIZONTAL.iter().any(beside))
    })
}

/// The strips between sorted `cuts`; the outer ones are open towards the outside.
pub fn fences(axis: Axis, cuts: &[i32]) -> Vec<Fence> {
    (0..=cuts.len())
        .map(|i| Fence {
            axis,
            lo: if i == 0 { i32::MIN } else { cuts[i - 1] },
            hi: cuts.get(i).copied().unwrap_or(i32::MAX),
        })
        .collect()
}

/// Cuts that give every strip about the same number of blocks.
fn even_cuts(coords: &[i32], bots: usize) -> Vec<i32> {
    (1..bots).map(|i| coords[coords.len() * i / bots] ).collect()
}

/// Strips at least [`MIN_WIDTH`] wide inside `lo..=hi`, with legal cuts only.
fn valid(cuts: &[i32], lo: i32, hi: i32, legal: &dyn Fn(i32) -> bool) -> bool {
    let mut prev = lo;
    for &c in cuts {
        if c - prev < MIN_WIDTH || !legal(c) {
            return false;
        }
        prev = c;
    }
    hi + 1 - prev >= MIN_WIDTH
}

/// The nearest cuts to `wanted` that are legal and leave wide enough strips.
fn snap(wanted: &[i32], lo: i32, hi: i32, legal: &dyn Fn(i32) -> bool) -> Option<Vec<i32>> {
    let mut out: Vec<i32> = Vec::new();
    for (i, &w) in wanted.iter().enumerate() {
        let from = out.last().copied().unwrap_or(lo) + MIN_WIDTH;
        let to = hi + 1 - MIN_WIDTH * (wanted.len() - i) as i32;
        let c = (from..=to).filter(|&c| legal(c)).min_by_key(|&c| ((c - w).abs(), c))?;
        out.push(c);
    }
    Some(out)
}

struct Search<'a> {
    job: part_plan::Job<'a>,
    axis: Axis,
    seen: HashMap<Vec<i32>, Option<(f64, Vec<f64>)>>,
    best: Option<(f64, MultiPlan)>,
    say: &'a mut dyn FnMut(&str),
}

impl Search<'_> {
    /// Plan the strips between `cuts`: (finish time, time of each strip). `None`: no legal plan.
    fn eval(&mut self, cuts: &[i32]) -> Option<(f64, Vec<f64>)> {
        if let Some(known) = self.seen.get(cuts) {
            return known.clone();
        }
        let found = self.job.plan(&fences(self.axis, cuts)).and_then(|multi| {
            joint::check(&multi, self.job.schem, self.job.origin)?;
            Ok(multi)
        });
        let result = match found {
            Ok(multi) => {
                let cost = multi.est_seconds();
                let times: Vec<f64> = multi.phases[0].iter().map(|p| super::est_seconds(&p.plan)).collect();
                (self.say)(&format!("  cuts {:?} {cuts:?}: {cost:.0} s, strips {times:?}, {} phase(s)", self.axis, multi.phases.len()));
                if self.best.as_ref().is_none_or(|b| cost < b.0) {
                    self.best = Some((cost, multi));
                }
                Some((cost, times))
            }
            Err(e) => {
                (self.say)(&format!("  cuts {:?} {cuts:?}: rejected: {e}", self.axis));
                None
            }
        };
        self.seen.insert(cuts.to_vec(), result.clone());
        result
    }
}

/// Plan a cooperative build for `bots` bots: the best of the strip layouts tried on both axes.
pub fn partition(schem: &Schematic, origin: (i32, i32, i32), protocol: i32, bots: usize, say: &mut dyn FnMut(&str)) -> Result<MultiPlan, String> {
    if !(2..=MAX_BOTS).contains(&bots) {
        return Err(format!("--bots must be 2..={MAX_BOTS}"));
    }
    let at: HashMap<BlockPos, &Block> = schem
        .blocks
        .iter()
        .map(|b| (BlockPos::new(origin.0 + b.pos.x, origin.1 + b.pos.y, origin.2 + b.pos.z), b))
        .collect();
    let mut best: Option<(f64, MultiPlan)> = None;
    for axis in [Axis::X, Axis::Z] {
        let mut coords: Vec<i32> = at.keys().map(|&p| along(axis, p)).collect();
        coords.sort_unstable();
        let (Some(&lo), Some(&hi)) = (coords.first(), coords.last()) else { return Err("the schematic has no blocks".into()) };
        let legal = |c: i32| legal_cut(&at, axis, c);
        let Some(start) = snap(&even_cuts(&coords, bots), lo, hi, &legal) else { continue };
        let job = part_plan::Job { schem, origin, protocol };
        let mut search = Search { job, axis, seen: HashMap::new(), best: None, say: &mut *say };
        balance(&mut search, start, lo, hi, &legal);
        if let Some(found) = search.best {
            if best.as_ref().is_none_or(|b| found.0 < b.0) {
                best = Some(found);
            }
        }
    }
    best.map(|b| b.1).ok_or_else(|| format!("no way to cut the schematic into {bots} strips that all plan cleanly"))
}

/// Hill climb: move one or all cuts by a shrinking step while the finish time improves.
fn balance(search: &mut Search, mut cuts: Vec<i32>, lo: i32, hi: i32, legal: &dyn Fn(i32) -> bool) {
    let mut cost = search.eval(&cuts).map_or(f64::MAX, |r| r.0);
    let mut step = ((hi - lo + 1) / (2 * (cuts.len() as i32 + 1))).max(1);
    while search.seen.len() < EVALS {
        let mut improved = false;
        for i in 0..cuts.len() {
            for delta in [-step, step] {
                let mut next = cuts.clone();
                next[i] += delta;
                if search.seen.len() >= EVALS || !valid(&next, lo, hi, legal) {
                    continue;
                }
                if let Some((c, _)) = search.eval(&next) {
                    if c < cost - 0.5 {
                        (cost, cuts, improved) = (c, next, true);
                        break;
                    }
                }
            }
        }
        if !improved {
            if step == 1 {
                break;
            }
            step /= 2;
        }
    }
}

#[cfg(test)]
#[path = "partition_tests.rs"]
mod tests;
