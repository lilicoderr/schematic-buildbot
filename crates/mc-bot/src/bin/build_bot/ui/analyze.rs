//! An uploaded or downloaded blueprint: keep it under the state folder, then parse, plan and
//! simulate it offline with the same code as `build` and describe the result for the page.

use super::super::{build_args, build_plan};
use super::{jsonw, security};
use mc_bot::fetch::{self, Source};
use mc_bot::plan::{Plan, Step};
use mc_bot::schematic::Schematic;
use mc_bot::version::{self, Version};
use mc_bot::{litematic, schem, tr};
use std::path::{Path, PathBuf};

/// Larger plans are not sent to the page (the browser could not draw them smoothly).
const PREVIEW_STEPS: usize = 500_000;

#[derive(Clone)]
pub struct Stored {
    /// Names this upload in `/api/build`; derived from the contents and the name.
    pub id: String,
    pub path: PathBuf,
}

/// A name for a downloaded file: links often end without a usable extension.
pub fn downloaded_name(source: &Source) -> String {
    let mut name: String = source.name.chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    while name.contains("..") {
        name = name.replace("..", ".");
    }
    if let Ok(safe) = security::safe_file_name(&name) {
        return safe;
    }
    let gzip = source.bytes.starts_with(&[0x1f, 0x8b]);
    let ext = match gzip {
        false => "txt",
        true if schem::parse(&source.bytes, &name).is_err() && litematic::parse(&source.bytes, &name).is_ok() => "litematic",
        true => "schem",
    };
    let stem: String = name.split('.').next().unwrap_or("").chars().take(60).collect();
    security::safe_file_name(&format!("{stem}.{ext}")).unwrap_or_else(|_| format!("blueprint.{ext}"))
}

/// Write the file to `<state>/uploads/<hash>/<name>`; `name` has passed `safe_file_name`.
pub fn store(state_dir: &Path, name: &str, bytes: &[u8]) -> Result<Stored, String> {
    let hash = name.bytes().chain([0]).chain(bytes.iter().copied()).fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let id = format!("{hash:016x}");
    let dir = state_dir.join("uploads").join(&id);
    let path = dir.join(name);
    std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, bytes)).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Stored { id, path })
}

/// The JSON describing a blueprint. Planning runs on its own thread with a roomy stack, and a
/// panic on hostile input becomes an error message.
pub fn describe(stored: &Stored, source: Source, release: Option<String>) -> Result<String, String> {
    let id = stored.id.clone();
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(move || describe_here(&id, &source, release.as_deref()))
        .map_err(|e| e.to_string())?
        .join()
        .unwrap_or_else(|_| Err(tr!("the blueprint could not be processed", "설계도를 처리할 수 없습니다")))
}

fn describe_here(id: &str, source: &Source, release: Option<&str>) -> Result<String, String> {
    let version: &Version = version::lookup(release.unwrap_or(version::DEFAULT.name))?;
    version::set_active(version);
    let schem = fetch::parse(source).map_err(|e| tr!("cannot read the schematic: {}", "설계도를 읽을 수 없습니다: {}", e))?;
    if schem.blocks.is_empty() {
        return Err(tr!("the schematic has no blocks", "설계도에 블록이 없습니다"));
    }
    let args = build_args::parse(&["preview".to_string(), "--dry-run".to_string()])?;
    let (plan, _) = build_plan::obtain(None, &schem, version, &args)?;
    build_plan::check_legal(&plan, &schem)?;
    Ok(report(id, &source.name, &schem, &plan, version))
}

fn report(id: &str, file: &str, schem: &Schematic, plan: &Plan, version: &Version) -> String {
    let (min, max) = schem.bounds();
    let count = |f: fn(&Step) -> bool| plan.steps.iter().filter(|s| f(s)).count();
    let mut types = schem.block_counts();
    types.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let unsupported = build_plan::unsupported(schem);
    let mut warnings = Vec::new();
    let floating = schem.unanchored_blocks().len();
    if floating > 0 {
        warnings.push(tr!("{} block(s) touch no other block and would float", "이웃이 전혀 없어 공중에 뜨는 블록 {}개", floating));
    }
    let preview = plan.steps.len() <= PREVIEW_STEPS;
    if !preview {
        warnings.push(tr!("too large for the 3D preview", "3D 미리보기로 보기에는 너무 큽니다"));
    }
    let est = plan.header.stats.iter().find(|(k, _)| k == "est_seconds").and_then(|(_, v)| v.parse::<u64>().ok());
    jsonw::object(&[
        ("id", jsonw::text(id)),
        ("file", jsonw::text(file)),
        ("name", jsonw::text(&schem.name)),
        ("version", jsonw::text(version.name)),
        ("blocks", schem.blocks.len().to_string()),
        ("size", jsonw::array([max.x - min.x + 1, max.y - min.y + 1, max.z - min.z + 1].map(|v| v.to_string()))),
        ("placements", count(|s| matches!(s, Step::Place { .. })).to_string()),
        ("temporary", count(|s| matches!(s, Step::Pillar(_) | Step::Support { .. })).to_string()),
        ("steps", plan.steps.len().to_string()),
        ("est_seconds", est.map_or("null".into(), |v| v.to_string())),
        ("types", jsonw::array(types.iter().take(100).map(|(n, c)| jsonw::array([jsonw::text(n), c.to_string(), (!unsupported.contains_key(n)).to_string()])))),
        ("problems", jsonw::texts(&build_plan::problems(plan, schem))),
        ("warnings", jsonw::texts(&warnings)),
        ("plan", if preview { jsonw::text(&plan.to_text()) } else { "null".into() }),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_names_get_a_usable_extension() {
        let named = |n: &str, b: &[u8]| downloaded_name(&Source { name: n.into(), bytes: b.to_vec() });
        assert_eq!(named("castle.schem", b""), "castle.schem");
        assert_eq!(named("download", b"layer 0"), "download.txt");
        assert_eq!(named("..", b"layer 0"), "blueprint.txt");
        assert_eq!(named("a%2e..b.schem", b""), "a_2e_b.schem");
        assert_eq!(named("x.php", &[0x1f, 0x8b, 0]), "x.schem");
    }

    #[test]
    fn a_blueprint_is_described_with_its_plan() {
        let bytes = b"palette S stone\nlayer 0\nSS\nlayer 1\nSS\n".to_vec();
        let json = describe(&Stored { id: "1".into(), path: PathBuf::new() }, Source { name: "two.txt".into(), bytes }, None).unwrap();
        assert!(json.contains("\"blocks\":4") && json.contains("\"problems\":[]") && json.contains("# bbplan 1"), "{json}");
        let bad = Source { name: "x.txt".into(), bytes: b"nonsense".to_vec() };
        assert!(describe(&Stored { id: "2".into(), path: PathBuf::new() }, bad, None).is_err());
    }
}
