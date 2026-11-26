//! 좌표·방향·시야(yaw/pitch)·도달거리 계산.

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
    /// 블록 중심 좌표.
    pub fn center(&self) -> Vec3 {
        Vec3::new(self.x as f64 + 0.5, self.y as f64 + 0.5, self.z as f64 + 0.5)
    }
    pub fn below(&self) -> BlockPos {
        BlockPos::new(self.x, self.y - 1, self.z)
    }
}

/// 블록 면(클릭 방향). 값은 프로토콜 direction 과 동일.
#[derive(Debug, Clone, Copy)]
pub enum Face {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

/// 눈높이(발 + 1.62)에서 목표 블록 중심을 바라보는 (yaw, pitch) 도.
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
pub const REACH: f64 = 4.5; // 서버 설치 도달 한계(여유 둠)

/// 발 위치 feet 에서 블록을 설치할 때 눈~블록중심 거리가 도달 범위인지.
pub fn within_reach(feet: Vec3, block: BlockPos) -> bool {
    let eye = Vec3::new(feet.x, feet.y + EYE_HEIGHT, feet.z);
    eye.dist(block.center()) <= REACH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_at_cardinals() {
        let eye = Vec3::new(0.0, 0.0, 0.0);
        // +Z 를 보면 yaw 0, -Z 는 180, +X 는 -90, -X 는 90 (마크 기준)
        let (yaw, _) = look_at(eye, Vec3::new(0.0, 0.0, 1.0));
        assert!((yaw - 0.0).abs() < 0.01);
        let (yaw, _) = look_at(eye, Vec3::new(1.0, 0.0, 0.0));
        assert!((yaw + 90.0).abs() < 0.01);
        // 바로 위를 보면 pitch -90
        let (_, pitch) = look_at(eye, Vec3::new(0.0, 1.0, 0.0));
        assert!((pitch + 90.0).abs() < 0.01);
    }

    #[test]
    fn reach_check() {
        let feet = Vec3::new(0.5, -60.0, 0.5);
        // 바로 옆 블록은 도달
        assert!(within_reach(feet, BlockPos::new(1, -60, 0)));
        // 10칸 떨어진 블록은 도달 불가
        assert!(!within_reach(feet, BlockPos::new(10, -60, 0)));
    }

    #[test]
    fn block_center_and_below() {
        let b = BlockPos::new(3, -60, 7);
        assert_eq!(b.center(), Vec3::new(3.5, -59.5, 7.5));
        assert_eq!(b.below(), BlockPos::new(3, -61, 7));
    }
}
