//! Grid path finding on the ground plane, so the bot walks *around* what it has built
//! instead of pushing into walls (which makes the server rubber-band it).

use std::collections::{HashMap, VecDeque};

pub type Cell = (i32, i32);

/// Shortest 4-neighbour path from `start` to `goal` (both included) that avoids
/// `blocked` cells. The search is limited to the bounding box of both points plus `margin`.
/// `start` itself is allowed to be blocked (the bot may stand next to a wall).
pub fn find_path(start: Cell, goal: Cell, blocked: &dyn Fn(i32, i32) -> bool, margin: i32) -> Option<Vec<Cell>> {
    if start == goal {
        return Some(vec![start]);
    }
    if blocked(goal.0, goal.1) {
        return None;
    }
    let (min_x, max_x) = (start.0.min(goal.0) - margin, start.0.max(goal.0) + margin);
    let (min_z, max_z) = (start.1.min(goal.1) - margin, start.1.max(goal.1) + margin);

    let mut came: HashMap<Cell, Cell> = HashMap::new();
    let mut queue = VecDeque::from([start]);
    came.insert(start, start);
    while let Some(cur) = queue.pop_front() {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < min_x || next.0 > max_x || next.1 < min_z || next.1 > max_z {
                continue;
            }
            if came.contains_key(&next) || blocked(next.0, next.1) {
                continue;
            }
            came.insert(next, cur);
            if next == goal {
                let mut path = vec![goal];
                let mut at = goal;
                while at != start {
                    at = came[&at];
                    path.push(at);
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(next);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn wall(cells: &[Cell]) -> impl Fn(i32, i32) -> bool {
        let set: HashSet<Cell> = cells.iter().copied().collect();
        move |x, z| set.contains(&(x, z))
    }

    #[test]
    fn straight_line_when_open() {
        let p = find_path((0, 0), (3, 0), &wall(&[]), 3).unwrap();
        assert_eq!(p.len(), 4);
        assert_eq!(p.first(), Some(&(0, 0)));
        assert_eq!(p.last(), Some(&(3, 0)));
    }

    #[test]
    fn routes_around_a_wall() {
        let w = wall(&[(1, -1), (1, 0), (1, 1)]);
        let p = find_path((0, 0), (2, 0), &w, 3).unwrap();
        assert!(p.len() > 3);
        assert!(p.iter().all(|&(x, z)| !w(x, z) || (x, z) == (0, 0)));
    }

    #[test]
    fn walled_in_goal_has_no_path() {
        let w = wall(&[(2, 1), (2, -1), (3, 0), (1, 0)]);
        assert!(find_path((0, 0), (2, 0), &w, 4).is_none());
    }

    #[test]
    fn blocked_goal_is_rejected() {
        assert!(find_path((0, 0), (1, 0), &wall(&[(1, 0)]), 2).is_none());
    }

    #[test]
    fn start_may_be_blocked() {
        assert!(find_path((0, 0), (2, 0), &wall(&[(0, 0)]), 2).is_some());
    }
}
