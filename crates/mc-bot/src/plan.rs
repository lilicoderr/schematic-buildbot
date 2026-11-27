//! The build plan ("G-code for Minecraft"): a versioned, human-readable list of steps.
//! See docs/PLAN_FORMAT.md. Parsing and printing are exact inverses (see tests).

use crate::nav::Cell;
use crate::world::{BlockPos, Face};
use std::fmt::{self, Write as _};

pub const FORMAT_VERSION: u32 = 1;

/// One click of a placement: click `face` of `support` at `cursor` inside that face.
#[derive(Debug, Clone, PartialEq)]
pub struct ClickStep {
    pub support: BlockPos,
    pub face: Face,
    pub cursor: (f32, f32, f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Go to the feet cell: one walk step, a jump up one block, or a drop of 1..=3 blocks.
    Move(Cell),
    /// Standing in the feet cell: jump, put a scaffold block into that cell, land on it.
    Pillar(Cell),
    /// Place `state` at `pos` by clicking (all clicks, in order) while standing still.
    Place { pos: BlockPos, state: String, yaw: Option<f32>, clicks: Vec<ClickStep> },
    /// Temporary support block (like a 3D-printing support) so a later block has something to click.
    /// Removed again with `Break` before the plan ends.
    Support { pos: BlockPos, clicks: Vec<ClickStep> },
    /// A block that appears by itself as part of the previous placement (e.g. a door's upper half).
    Auto { pos: BlockPos, state: String },
    /// Break one block (creative: instant). Only scaffold may be broken.
    Break(BlockPos),
    /// After this step exactly `count` schematic blocks must stand, with order-independent `hash`.
    Checkpoint { n: u32, count: u32, hash: u64 },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Header {
    pub schematic_hash: u64,
    pub origin: (i32, i32, i32),
    /// Feet cell the plan starts from.
    pub start: (i32, i32, i32),
    pub protocol: i32,
    /// Free-form `key value` statistics (blocks, steps, supports, estimated seconds ...).
    pub stats: Vec<(String, String)>,
    /// Schematic blocks no legal standing position reaches (never placed).
    pub unreachable: Vec<(BlockPos, String)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub header: Header,
    pub steps: Vec<Step>,
}

fn face_name(f: Face) -> &'static str {
    ["down", "up", "north", "south", "west", "east"][f as usize]
}

fn write_clicks(f: &mut fmt::Formatter<'_>, clicks: &[ClickStep]) -> fmt::Result {
    for c in clicks {
        write!(f, " {},{},{},{},{},{},{}", c.support.x, c.support.y, c.support.z, face_name(c.face), c.cursor.0, c.cursor.1, c.cursor.2)?;
    }
    Ok(())
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Step::Move((x, y, z)) => write!(f, "MOVE {x} {y} {z}"),
            Step::Pillar((x, y, z)) => write!(f, "PILLAR {x} {y} {z}"),
            Step::Auto { pos, state } => write!(f, "AUTO {} {} {} {state}", pos.x, pos.y, pos.z),
            Step::Break(p) => write!(f, "BREAK {} {} {}", p.x, p.y, p.z),
            Step::Checkpoint { n, count, hash } => write!(f, "CHECKPOINT {n} {count} {hash:016x}"),
            Step::Support { pos, clicks } => {
                write!(f, "SUPPORT {} {} {}", pos.x, pos.y, pos.z)?;
                write_clicks(f, clicks)
            }
            Step::Place { pos, state, yaw, clicks } => {
                write!(f, "PLACE {} {} {} {state} ", pos.x, pos.y, pos.z)?;
                match yaw {
                    Some(y) => write!(f, "{y}")?,
                    None => write!(f, "-")?,
                }
                write_clicks(f, clicks)
            }
        }
    }
}

impl Plan {
    pub fn to_text(&self) -> String {
        let h = &self.header;
        let mut s = String::new();
        let _ = writeln!(s, "# bbplan {FORMAT_VERSION}");
        let _ = writeln!(s, "# schematic {:016x}", h.schematic_hash);
        let _ = writeln!(s, "# origin {} {} {}", h.origin.0, h.origin.1, h.origin.2);
        let _ = writeln!(s, "# start {} {} {}", h.start.0, h.start.1, h.start.2);
        let _ = writeln!(s, "# protocol {}", h.protocol);
        for (k, v) in &h.stats {
            let _ = writeln!(s, "# stat {k} {v}");
        }
        for (p, name) in &h.unreachable {
            let _ = writeln!(s, "# unreachable {} {} {} {name}", p.x, p.y, p.z);
        }
        for st in &self.steps {
            let _ = writeln!(s, "{st}");
        }
        s
    }
}

/// Order-independent hash contribution of one block.
pub fn block_hash(pos: BlockPos, state: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{} {} {} {state}", pos.x, pos.y, pos.z).bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}
