//! Scaffold the executor had to build that is not in the plan (access made by a repair or a
//! resume). It is remembered across runs, broken before any plan step needs its cell, and taken
//! down completely after the last step.

use crate::builder::PlayClient;
use crate::exec_repair::Offset;
use crate::nav::Cell;
use crate::plan::Step;
use crate::world::BlockPos;
use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;

/// World cells of the standing extra blocks, mirrored into `file` on every change.
#[derive(Default)]
pub struct Extras {
    pub cells: BTreeSet<Cell>,
    file: Option<PathBuf>,
}

impl Extras {
    /// Start empty, or with what an earlier run left standing (`load`: resuming).
    pub fn open(file: Option<PathBuf>, load: bool) -> Self {
        let text = file.as_ref().filter(|_| load).and_then(|f| std::fs::read_to_string(f).ok());
        let cells = text.iter().flat_map(|t| t.lines()).filter_map(parse).collect();
        let me = Extras { cells, file };
        me.save();
        me
    }

    pub fn set(&mut self, cells: impl IntoIterator<Item = Cell>) {
        self.cells = cells.into_iter().collect();
        self.save();
    }

    fn save(&self) {
        let Some(f) = &self.file else { return };
        if self.cells.is_empty() {
            let _ = std::fs::remove_file(f);
        } else {
            let text: String = self.cells.iter().map(|c| format!("{} {} {}\n", c.0, c.1, c.2)).collect();
            let _ = std::fs::write(f, text);
        }
    }

    pub fn hits(&self, needed: &[Cell]) -> bool {
        needed.iter().any(|c| self.cells.contains(c))
    }

    /// Break the extra blocks standing in any of `needed` (world cells).
    pub fn clear(&mut self, bot: &mut PlayClient, needed: &[Cell]) -> io::Result<()> {
        for c in needed {
            if self.cells.remove(c) {
                bot.break_block(BlockPos::new(c.0, c.1, c.2))?;
                self.save();
            }
        }
        Ok(())
    }
}

fn parse(line: &str) -> Option<Cell> {
    let mut it = line.split_whitespace().map(|v| v.parse::<i32>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

/// World cells that must hold nothing foreign for `step` to run as the plan expects; `from` and
/// `to` are the planned feet cells before and after it.
pub fn needed(step: &Step, from: Cell, to: Cell, off: Offset) -> Vec<Cell> {
    let mut out = Vec::new();
    // the body at both ends, jump head room, and any column fallen or jumped through
    if from != to {
        for y in from.1.min(to.1)..=from.1.max(to.1) + 2 {
            out.push((to.0, y, to.2));
        }
        out.push((from.0, from.1 + 2, from.2));
    }
    match step {
        Step::Place { pos, .. } => out.extend([(pos.x, pos.y, pos.z), (pos.x, pos.y + 1, pos.z)]),
        Step::Support { pos, .. } | Step::Auto { pos, .. } => out.push((pos.x, pos.y, pos.z)),
        _ => {}
    }
    out.into_iter().map(|c| off.cell(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_cells_come_back_only_when_resuming() {
        let f = std::env::temp_dir().join(format!("bb-extras-{}", std::process::id()));
        Extras::open(Some(f.clone()), false).set([(1, -2, 3), (4, 5, 6)]);
        assert_eq!(Extras::open(Some(f.clone()), true).cells.len(), 2);
        assert!(Extras::open(Some(f.clone()), false).cells.is_empty());
        assert!(!f.exists());
    }

    #[test]
    fn a_drop_needs_its_whole_column_in_world_coordinates() {
        let off = Offset { dx: 10, dy: 0, dz: 0 };
        let cells = needed(&Step::Move((1, 2, 0)), (0, 4, 0), (1, 2, 0), off);
        for y in 2..=6 {
            assert!(cells.contains(&(11, y, 0)), "y{y}");
        }
    }
}
