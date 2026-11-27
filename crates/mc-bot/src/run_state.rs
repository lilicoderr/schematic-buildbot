//! Resume file of `build-bot run`: the last finished step index and the plan-to-world offset.

use crate::exec_repair::Offset;
use crate::plan::Plan;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunState {
    pub step: usize,
    pub offset: Offset,
}

/// Identifies the plan the progress belongs to (FNV-1a over its text).
pub fn fingerprint(plan: &Plan) -> u64 {
    plan.to_text().bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

pub fn save(path: &Path, fingerprint: u64, step: usize, offset: Offset) -> io::Result<()> {
    std::fs::write(path, format!("{fingerprint:016x} {step} {} {} {}\n", offset.dx, offset.dy, offset.dz))
}

/// `None` when there is no file, it is damaged, or it belongs to a different plan.
pub fn load(path: &Path, expected: u64) -> Option<RunState> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut it = text.split_whitespace();
    if u64::from_str_radix(it.next()?, 16).ok()? != expected {
        return None;
    }
    let step = it.next()?.parse().ok()?;
    let mut n = || it.next()?.parse::<i32>().ok();
    let offset = Offset { dx: n()?, dy: n()?, dz: n()? };
    Some(RunState { step, offset })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_wrong_plan_is_ignored() {
        let path = std::env::temp_dir().join(format!("bb-run-state-{}", std::process::id()));
        let off = Offset { dx: -3, dy: 0, dz: 12 };
        save(&path, 0xabc, 75, off).unwrap();
        assert_eq!(load(&path, 0xabc), Some(RunState { step: 75, offset: off }));
        assert_eq!(load(&path, 0xdef), None);
        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(load(&path, 0xabc), None);
        std::fs::remove_file(&path).unwrap();
    }
}
