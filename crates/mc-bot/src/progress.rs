//! Resume support: a tiny text file that records how far a build got.
//!
//! The file stores a fingerprint of the blueprint, the build origin and the number
//! of processed blocks, so `--resume` can continue the same build in the same place
//! after a kick, crash or Ctrl-C. Plain text, no secrets.

use crate::world::BlockPos;
use std::io;
use std::path::Path;

const MAGIC: &str = "schematic-buildbot-progress v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub fingerprint: u64,
    pub origin: BlockPos,
    pub done: usize,
    pub total: usize,
}

impl Progress {
    pub fn render(&self) -> String {
        format!(
            "{MAGIC}\nfingerprint {:016x}\norigin {} {} {}\ndone {}\ntotal {}\n",
            self.fingerprint, self.origin.x, self.origin.y, self.origin.z, self.done, self.total
        )
    }

    pub fn parse(text: &str) -> Option<Progress> {
        let mut lines = text.lines();
        if lines.next()?.trim() != MAGIC {
            return None;
        }
        let (mut fp, mut origin, mut done, mut total) = (None, None, None, None);
        for line in lines {
            let mut it = line.split_whitespace();
            match it.next()? {
                "fingerprint" => fp = u64::from_str_radix(it.next()?, 16).ok(),
                "origin" => {
                    let v: Vec<i32> = it.filter_map(|s| s.parse().ok()).collect();
                    if v.len() == 3 {
                        origin = Some(BlockPos::new(v[0], v[1], v[2]));
                    }
                }
                "done" => done = it.next()?.parse().ok(),
                "total" => total = it.next()?.parse().ok(),
                _ => {}
            }
        }
        Some(Progress { fingerprint: fp?, origin: origin?, done: done?, total: total? })
    }

    pub fn load(path: &Path) -> Option<Progress> {
        Progress::parse(&std::fs::read_to_string(path).ok()?)
    }

    /// Write atomically (temp file + rename) so a crash never leaves a half-written file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.render())?;
        std::fs::rename(&tmp, path)
    }

    pub fn clear(path: &Path) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Progress {
        Progress { fingerprint: 0xdead_beef_0123_4567, origin: BlockPos::new(-12, 64, 30), done: 17, total: 250 }
    }

    #[test]
    fn roundtrip() {
        let p = sample();
        assert_eq!(Progress::parse(&p.render()), Some(p));
    }

    #[test]
    fn rejects_foreign_or_broken_files() {
        assert_eq!(Progress::parse("hello"), None);
        assert_eq!(Progress::parse(&format!("{MAGIC}\nfingerprint zz\n")), None);
    }

    #[test]
    fn save_and_load_on_disk() {
        let dir = std::env::temp_dir().join(format!("buildbot-progress-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.txt");
        sample().save(&path).unwrap();
        assert_eq!(Progress::load(&path), Some(sample()));
        Progress::clear(&path);
        assert_eq!(Progress::load(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
