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
    fn sprint_is_within_vanilla_sprint_speed() {
        let per_second = SPRINT_STEP * 20.0;
        assert!(per_second <= 5.612);
    }
}
