//! Level walking along several cells without stopping at each one.

use crate::world::Vec3;

/// Ground distance per 20 Hz tick when walking (4.2 blocks/s; vanilla walks 4.317).
pub const WALK_STEP: f64 = 0.21;
/// Ground distance per tick when sprinting (5.6 blocks/s; vanilla sprints 5.612). Far below the
/// server's 10 blocks per tick limit (docs/RE/movement.md).
pub const SPRINT_STEP: f64 = 0.28;

/// One tick of movement along the polyline `from -> path[*next..]`: moves exactly `step` blocks
/// (or to the end), carrying the remainder across corners. The height never changes.
pub fn advance(from: Vec3, path: &[Vec3], next: &mut usize, step: f64) -> Vec3 {
    let mut at = from;
    let mut left = step;
    while *next < path.len() {
        let t = path[*next];
        let d = (t.x - at.x).hypot(t.z - at.z);
        if d <= left + 1e-9 {
            at = Vec3::new(t.x, at.y, t.z);
            left -= d;
            *next += 1;
        } else {
            return Vec3::new(at.x + (t.x - at.x) / d * left, at.y, at.z + (t.z - at.z) / d * left);
        }
    }
    at
}

/// Feet height above the take-off surface on ticks 1..=3 of a vanilla jump (0.42 initial velocity).
pub const JUMP_ARC: [f64; 3] = [0.42, 0.7532, 1.0013];
/// How far the player can drift from a cell centre before its 0.6 wide box enters the next cell.
const EDGE_ROOM: f64 = 0.2;

/// The airborne positions, one per tick, of a jump from the cell centre `from` up one block onto
/// the neighbouring cell centre `to`. Like a vanilla jump it drifts while rising, but never into
/// the target column before the feet are above its block; at the top it moves on at `stride`.
pub fn jump_ticks(from: Vec3, to: Vec3, stride: f64) -> Vec<Vec3> {
    let (dx, dz) = (to.x - from.x, to.z - from.z);
    let dist = dx.hypot(dz).max(1e-9);
    let at = |gone: f64, y: f64| Vec3::new(from.x + dx / dist * gone, y, from.z + dz / dist * gone);
    let mut out = Vec::new();
    let mut gone = 0.0;
    for (k, rise) in JUMP_ARC.into_iter().enumerate() {
        let limit = if k + 1 < JUMP_ARC.len() { EDGE_ROOM.min(dist) } else { dist };
        let share = if k + 1 < JUMP_ARC.len() { EDGE_ROOM / (JUMP_ARC.len() - 1) as f64 } else { stride };
        gone = (gone + share).min(limit);
        out.push(at(gone, from.y + rise));
    }
    while dist - gone > 1e-9 {
        gone = (gone + stride).min(dist);
        out.push(at(gone, from.y + JUMP_ARC[JUMP_ARC.len() - 1]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, z: f64) -> Vec3 {
        Vec3::new(x, 65.0, z)
    }

    #[test]
    fn carries_the_remainder_around_corners_without_speeding_or_leaving_the_path() {
        let path = [p(1.5, 0.5), p(2.5, 0.5), p(2.5, 1.5), p(2.5, 2.5)];
        let (mut at, mut next, mut ticks) = (p(0.5, 0.5), 0, 0);
        while next < path.len() {
            let n = advance(at, &path, &mut next, WALK_STEP);
            assert!(at.horiz_dist(n) <= WALK_STEP + 1e-9, "no speeding");
            assert_eq!(n.y, 65.0, "level walking never changes height");
            // on one of the two straight legs
            assert!((n.z - 0.5).abs() < 1e-9 || (n.x - 2.5).abs() < 1e-9, "{n:?} left the path");
            at = n;
            ticks += 1;
        }
        assert_eq!(ticks, (4.0 / WALK_STEP).ceil() as i32, "no time lost at the cells in between");
        assert!((at.x - 2.5).abs() < 1e-9 && (at.z - 2.5).abs() < 1e-9);
    }

    #[test]
    fn a_jump_drifts_while_rising_without_entering_the_block_or_speeding() {
        let (from, to) = (p(0.5, 0.5), Vec3::new(1.5, 66.0, 0.5));
        let ticks = jump_ticks(from, to, SPRINT_STEP);
        assert_eq!(ticks.len(), 5, "5 ticks in the air plus the landing: {ticks:?}");
        let mut last = from;
        for t in &ticks {
            assert!(last.horiz_dist(*t) <= SPRINT_STEP + 1e-9, "no speeding");
            // the box reaches 0.3 ahead; the block fills x 1..2 below y 66
            assert!(t.x + 0.3 <= 1.0 + 1e-9 || t.y >= 66.0, "{t:?} is inside the block");
            last = *t;
        }
        assert!((last.x - to.x).abs() < 1e-9 && last.y > to.y);
        assert_eq!(jump_ticks(from, to, WALK_STEP).len(), 6);
    }

    #[test]
    fn sprint_is_within_vanilla_sprint_speed() {
        let per_second = SPRINT_STEP * 20.0;
        assert!(per_second <= 5.612);
    }
}
