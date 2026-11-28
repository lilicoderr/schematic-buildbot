//! A bot's territory in a cooperative build: a strip of the world along x or z. A bot with a
//! territory stands, places and puts scaffold only inside it, so bots whose territories are
//! disjoint can never meet, whatever their relative speeds.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Z,
}

/// Cells with `lo <= coordinate < hi` along `axis` (any y, any other coordinate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fence {
    pub axis: Axis,
    pub lo: i32,
    pub hi: i32,
}

impl Default for Fence {
    fn default() -> Self {
        Fence::OPEN
    }
}

impl Fence {
    /// No limit: the single bot.
    pub const OPEN: Fence = Fence { axis: Axis::X, lo: i32::MIN, hi: i32::MAX };

    /// Whether the column (x, z) is inside.
    pub fn has(&self, x: i32, z: i32) -> bool {
        let v = match self.axis {
            Axis::X => x,
            Axis::Z => z,
        };
        v >= self.lo && (v < self.hi || self.hi == i32::MAX)
    }

    /// The same strip after moving the world by (dx, dz). Open ends stay open.
    pub fn shifted(&self, dx: i32, dz: i32) -> Fence {
        let d = match self.axis {
            Axis::X => dx,
            Axis::Z => dz,
        };
        let shift = |v: i32| if v == i32::MIN || v == i32::MAX { v } else { v + d };
        Fence { lo: shift(self.lo), hi: shift(self.hi), ..*self }
    }

    /// Whether no column is in both strips (strips on different axes always cross).
    pub fn disjoint(&self, other: &Fence) -> bool {
        self.axis == other.axis && (self.hi <= other.lo || other.hi <= self.lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_are_half_open_and_shift_with_the_world() {
        let a = Fence { axis: Axis::X, lo: i32::MIN, hi: 8 };
        let b = Fence { axis: Axis::X, lo: 8, hi: i32::MAX };
        assert!(a.has(7, 100) && !a.has(8, 0) && b.has(8, 0));
        assert!(a.disjoint(&b) && b.disjoint(&a) && !a.disjoint(&a));
        assert_eq!(a.shifted(5, 0), Fence { hi: 13, ..a });
        assert!(Fence::OPEN.has(i32::MAX, 0));
        assert!(!a.disjoint(&Fence { axis: Axis::Z, lo: 0, hi: 1 }));
    }
}
