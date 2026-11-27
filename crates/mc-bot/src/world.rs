//! Coordinates, directions, view angles (yaw/pitch) and reach checks.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    pub fn dist(&self, o: Vec3) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2) + (self.z - o.z).powi(2)).sqrt()
    }
    pub fn horiz_dist(&self, o: Vec3) -> f64 {
        ((self.x - o.x).powi(2) + (self.z - o.z).powi(2)).sqrt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
impl BlockPos {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
    /// Centre of the block.
    pub fn center(&self) -> Vec3 {
        Vec3::new(self.x as f64 + 0.5, self.y as f64 + 0.5, self.z as f64 + 0.5)
    }
    pub fn below(&self) -> BlockPos {
        BlockPos::new(self.x, self.y - 1, self.z)
    }
    /// The neighbouring position across `face`.
    pub fn offset(&self, face: Face) -> BlockPos {
        let (dx, dy, dz) = face.offset();
        BlockPos::new(self.x + dx, self.y + dy, self.z + dz)
    }
}

/// Block face (click direction). Values match the protocol's direction enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl Face {
    pub const HORIZONTAL: [Face; 4] = [Face::North, Face::East, Face::South, Face::West];

    /// Unit step of this face (north = -z, east = +x).
    pub fn offset(self) -> (i32, i32, i32) {
        match self {
            Face::Down => (0, -1, 0),
            Face::Up => (0, 1, 0),
            Face::North => (0, 0, -1),
            Face::South => (0, 0, 1),
            Face::West => (-1, 0, 0),
            Face::East => (1, 0, 0),
        }
    }

    pub fn opposite(self) -> Face {
        match self {
            Face::Down => Face::Up,
            Face::Up => Face::Down,
            Face::North => Face::South,
            Face::South => Face::North,
            Face::West => Face::East,
            Face::East => Face::West,
        }
    }

    pub fn from_name(name: &str) -> Option<Face> {
        Some(match name {
            "down" => Face::Down,
            "up" => Face::Up,
            "north" => Face::North,
            "south" => Face::South,
            "west" => Face::West,
            "east" => Face::East,
            _ => return None,
        })
    }

    /// Yaw (degrees) of a player looking along a horizontal face; `None` for up/down.
    pub fn yaw(self) -> Option<f32> {
        match self {
            Face::South => Some(0.0),
            Face::West => Some(90.0),
            Face::North => Some(180.0),
            Face::East => Some(-90.0),
            _ => None,
        }
    }
}

/// (yaw, pitch) in degrees for looking from eye height (feet + 1.62) at a target point.
pub fn look_at(eye: Vec3, target: Vec3) -> (f32, f32) {
    let dx = target.x - eye.x;
    let dy = target.y - eye.y;
    let dz = target.z - eye.z;
    let r = (dx * dx + dz * dz).sqrt();
    let yaw = (-dx.atan2(dz)).to_degrees();
    let pitch = (-dy.atan2(r)).to_degrees();
    (yaw as f32, pitch as f32)
}

pub const EYE_HEIGHT: f64 = 1.62;
/// Reach rule of the server (docs/RE/placement.md, Player.canInteractWithBlock): a click on a block
/// is accepted when the squared distance from the player's eye to the nearest point of that
/// block's unit box is < (range + 1.0)^2, range 4.5 in survival and 5.0 in creative, so the hard
/// limit is 5.5 (survival) / 6.0 (creative). Failing is silent, so the bot keeps a margin and
/// works with 5.0 for the CLICKED block, which holds in both game modes.
pub const CLICK_REACH: f64 = 5.0;

/// Reach used when choosing where to stand for a block that is to be placed: the block it will
/// be clicked against touches it, so the target's own box must be about one block closer.
pub const REACH: f64 = 4.5;

/// Whether the eye is close enough to the *target* block's box to place it (the clicked
/// neighbour is then within [`CLICK_REACH`]; the slicer re-checks the real click support).
pub fn within_reach(feet: Vec3, block: BlockPos) -> bool {
    eye_to_box(feet, block) <= REACH
}

/// Distance from the eye of a player standing at `feet` to the nearest point of the block's box.
pub fn eye_to_box(feet: Vec3, block: BlockPos) -> f64 {
    let eye = Vec3::new(feet.x, feet.y + EYE_HEIGHT, feet.z);
    let d = |e: f64, lo: i32| (lo as f64 - e).max(e - (lo as f64 + 1.0)).max(0.0);
    let (dx, dy, dz) = (d(eye.x, block.x), d(eye.y, block.y), d(eye.z, block.z));
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_distance_is_at_most_center_distance_minus_half_a_block() {
        let feet = Vec3::new(0.5, 0.0, 0.5);
        for b in [BlockPos::new(3, 0, 0), BlockPos::new(-2, -3, 4), BlockPos::new(0, 3, 0)] {
            let eye = Vec3::new(feet.x, feet.y + EYE_HEIGHT, feet.z);
            assert!(eye_to_box(feet, b) <= eye.dist(b.center()) - 0.5 + 1e-9, "{b:?}");
        }
    }

    #[test]
    fn look_at_cardinals() {
        let eye = Vec3::new(0.0, 0.0, 0.0);
        // Looking +Z gives yaw 0, -Z 180, +X -90, -X 90 (Minecraft convention)
        let (yaw, _) = look_at(eye, Vec3::new(0.0, 0.0, 1.0));
        assert!((yaw - 0.0).abs() < 0.01);
        let (yaw, _) = look_at(eye, Vec3::new(1.0, 0.0, 0.0));
        assert!((yaw + 90.0).abs() < 0.01);
        // Looking straight up gives pitch -90
        let (_, pitch) = look_at(eye, Vec3::new(0.0, 1.0, 0.0));
        assert!((pitch + 90.0).abs() < 0.01);
    }

    #[test]
    fn reach_check() {
        let feet = Vec3::new(0.5, -60.0, 0.5);
        // The adjacent block is reachable
        assert!(within_reach(feet, BlockPos::new(1, -60, 0)));
        // A block 10 cells away is not
        assert!(!within_reach(feet, BlockPos::new(10, -60, 0)));
    }

    #[test]
    fn block_center_and_below() {
        let b = BlockPos::new(3, -60, 7);
        assert_eq!(b.center(), Vec3::new(3.5, -59.5, 7.5));
        assert_eq!(b.below(), BlockPos::new(3, -61, 7));
    }
}
