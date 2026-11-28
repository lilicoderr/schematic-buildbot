//! The files of one `build` job. The same schematic, server, release and origin map to the same
//! folder, so running the same command again finds the saved plan and progress and resumes.

use std::io;
use std::path::{Path, PathBuf};

pub struct Job {
    pub dir: PathBuf,
}

impl Job {
    /// `root/<schematic name>-<hash>`.
    pub fn new(root: &Path, source_name: &str, schematic: u64, protocol: i32, server: &str, origin: Option<(i32, i32, i32)>) -> Job {
        let id = format!("{schematic:016x} {protocol} {} {origin:?}", server.to_ascii_lowercase());
        let hash = id.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
        let stem = source_name.rsplit_once('.').map_or(source_name, |s| s.0);
        // the name may come from a URL: keep only characters that are safe in any file system
        let safe: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).take(40).collect();
        Job { dir: root.join(format!("{safe}-{hash:016x}")) }
    }

    pub fn plan(&self) -> PathBuf {
        self.dir.join("plan.bbplan")
    }

    pub fn progress(&self) -> PathBuf {
        self.dir.join("plan.progress")
    }

    pub fn extras(&self) -> PathBuf {
        self.dir.join("plan.progress.extras")
    }

    /// Where the plan was put in the world (`dx dy dz`), kept after the build for `undo`.
    pub fn offset(&self) -> PathBuf {
        self.dir.join("plan.offset")
    }

    /// Written when every step has run.
    pub fn done(&self) -> PathBuf {
        self.dir.join("done")
    }

    pub fn save_plan(&self, text: &str) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join("plan.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, self.plan())
    }

    /// Forget an earlier build (`--fresh`).
    pub fn reset(&self) {
        for f in [self.plan(), self.progress(), self.extras(), self.done(), self.offset()] {
            let _ = std::fs::remove_file(f);
        }
    }

    pub fn finish(&self) -> io::Result<()> {
        let _ = std::fs::remove_file(self.progress());
        std::fs::write(self.done(), "complete\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_same_folder_and_names_cannot_escape() {
        let make = |server: &str, origin| Job::new(Path::new("root"), "..\\..\\x?.schem", 7, 767, server, origin).dir;
        assert_eq!(make("Host:1", None), make("host:1", None));
        assert_ne!(make("host:1", None), make("host:2", None));
        assert_ne!(make("host:1", None), make("host:1", Some((1, 2, 3))));
        assert_eq!(make("h:1", None).parent(), Some(Path::new("root")));
        assert!(!make("h:1", None).file_name().unwrap().to_string_lossy().contains(['.', '\\', '?']));
    }
}
