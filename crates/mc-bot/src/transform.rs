//! `--rotate` and `--mirror`: turn a blueprint about the vertical axis or flip it, positions and
//! block states together (facing, axis, rotation, connections, hinge, stair shape, chest side,
//! rail shape, crafter orientation).
//!
//! Rotation is clockwise seen from above (north -> east -> south -> west); `x` mirroring swaps
//! east and west, `z` mirroring swaps north and south. Rotation is applied before mirroring.

use crate::schematic::{Block, Schematic};
use crate::world::BlockPos;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mirror {
    #[default]
    None,
    X,
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Transform {
    /// Quarter turns clockwise, 0..=3.
    pub turns: u8,
    pub mirror: Mirror,
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        *self == Transform::default()
    }

    pub fn parse(rotate: Option<&str>, mirror: Option<&str>) -> Result<Transform, String> {
        let turns = match rotate {
            None => 0,
            Some("0") => 0,
            Some("90") => 1,
            Some("180") => 2,
            Some("270") => 3,
            Some(_) => return Err(crate::tr!("--rotate must be 90, 180 or 270", "--rotate 는 90, 180, 270 중 하나입니다")),
        };
        let mirror = match mirror {
            None => Mirror::None,
            Some("x") => Mirror::X,
            Some("z") => Mirror::Z,
            Some(_) => return Err(crate::tr!("--mirror must be x or z", "--mirror 는 x 또는 z 입니다")),
        };
        Ok(Transform { turns, mirror })
    }

    /// Image of the horizontal offset (x, z).
    pub fn point(&self, (mut x, mut z): (i32, i32)) -> (i32, i32) {
        for _ in 0..self.turns {
            (x, z) = (-z, x);
        }
        match self.mirror {
            Mirror::None => (x, z),
            Mirror::X => (-x, z),
            Mirror::Z => (x, -z),
        }
    }

    /// Image of a compass direction name; anything else (up, down, ...) is returned unchanged.
    fn direction(&self, d: &str) -> String {
        const RING: [&str; 4] = ["north", "east", "south", "west"];
        let Some(i) = RING.iter().position(|r| *r == d) else { return d.to_string() };
        let turned = RING[(i + usize::from(self.turns)) % 4];
        let flipped = match (self.mirror, turned) {
            (Mirror::X, "east") => "west",
            (Mirror::X, "west") => "east",
            (Mirror::Z, "north") => "south",
            (Mirror::Z, "south") => "north",
            _ => turned,
        };
        flipped.to_string()
    }

    fn mirrored(&self) -> bool {
        self.mirror != Mirror::None
    }

    /// The `_`-joined compass directions of a rail or orientation value, each transformed.
    fn compound(&self, value: &str, rail: bool) -> String {
        let parts: Vec<String> = value.split('_').map(|p| self.direction(p)).collect();
        if rail && parts.len() == 2 && parts[0] != "ascending" {
            const NAMES: [&str; 6] = ["north_south", "east_west", "south_east", "south_west", "north_west", "north_east"];
            let (ab, ba) = (format!("{}_{}", parts[0], parts[1]), format!("{}_{}", parts[1], parts[0]));
            return if NAMES.contains(&ab.as_str()) { ab } else { ba };
        }
        parts.join("_")
    }

    /// Image of one property.
    fn property(&self, name: &str, value: &str) -> (String, String) {
        let mirrored = self.mirrored();
        let swap = |a: &'static str, b: &'static str| -> &str { if value == a { b } else if value == b { a } else { value } };
        let same = |v: &str| (name.to_string(), v.to_string());
        match name {
            "north" | "east" | "south" | "west" => (self.direction(name), value.to_string()),
            "facing" => same(&self.direction(value)),
            "axis" => same(if self.turns % 2 == 1 { swap("x", "z") } else { value }),
            "rotation" => {
                let Ok(r) = value.parse::<i32>() else { return same(value) };
                let r = r + 4 * i32::from(self.turns);
                let r = match self.mirror {
                    Mirror::None => r,
                    Mirror::X => -r,
                    Mirror::Z => 8 - r,
                };
                same(&r.rem_euclid(16).to_string())
            }
            "shape" if value.starts_with("inner_") || value.starts_with("outer_") => same(if mirrored { swap_lr(value) } else { value }),
            "shape" => same(&self.compound(value, true)),
            "orientation" => same(&self.compound(value, false)),
            "hinge" if mirrored => same(swap("left", "right")),
            "type" if mirrored && (value == "left" || value == "right") => same(swap("left", "right")),
            _ => same(value),
        }
    }

    pub fn state(&self, props: &BTreeMap<String, String>) -> BTreeMap<String, String> {
        props.iter().map(|(k, v)| self.property(k, v)).collect()
    }

    /// The blueprint turned and flipped; its minimum corner stays where it was.
    pub fn apply(&self, s: &Schematic) -> Schematic {
        if self.is_identity() || s.blocks.is_empty() {
            return s.clone();
        }
        let (min, _) = s.bounds();
        let moved: Vec<(i32, i32)> = s.blocks.iter().map(|b| self.point((b.pos.x, b.pos.z))).collect();
        let (lx, lz) = (moved.iter().map(|p| p.0).min().unwrap(), moved.iter().map(|p| p.1).min().unwrap());
        let blocks = s
            .blocks
            .iter()
            .zip(moved)
            .map(|(b, (x, z))| Block { pos: BlockPos::new(x - lx + min.x, b.pos.y, z - lz + min.z), name: b.name.clone(), props: self.state(&b.props) })
            .collect();
        Schematic { name: s.name.clone(), blocks }
    }
}

fn swap_lr(v: &str) -> &str {
    match v {
        "inner_left" => "inner_right",
        "inner_right" => "inner_left",
        "outer_left" => "outer_right",
        "outer_right" => "outer_left",
        other => other,
    }
}

#[cfg(test)]
#[path = "transform_tests.rs"]
mod tests;
