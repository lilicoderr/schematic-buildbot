//! Command line of `build-bot build`. Unknown options are errors: a mistyped `--dry-run` must
//! never start a build.

use mc_bot::tr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug)]
pub struct Args {
    pub source: String,
    /// Required unless `--dry-run`.
    pub server: Option<String>,
    pub name: String,
    pub origin: Option<(i32, i32, i32)>,
    pub version: Option<String>,
    pub dry_run: bool,
    pub allow_partial: bool,
    pub fresh: bool,
    pub walk: bool,
    pub state_dir: PathBuf,
    pub wait_creative: Duration,
    pub delay: Duration,
    pub time_limit: Duration,
    pub memory_mb: usize,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        source: String::new(),
        server: None,
        name: "BuildBot".into(),
        origin: None,
        version: None,
        dry_run: false,
        allow_partial: false,
        fresh: false,
        walk: false,
        state_dir: PathBuf::from(".buildbot"),
        wait_creative: Duration::from_secs(120),
        delay: Duration::from_millis(10),
        time_limit: Duration::from_secs(600),
        memory_mb: 2048,
    };
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if !arg.starts_with("--") {
            positional.push(arg.clone());
            continue;
        }
        match arg.as_str() {
            "--dry-run" => a.dry_run = true,
            "--allow-partial" => a.allow_partial = true,
            "--fresh" => a.fresh = true,
            "--walk" => a.walk = true,
            // accepted for symmetry with `run`; `build` always verifies
            "--verify" => {}
            flag => {
                const WITH_VALUE: [&str; 9] = ["--name", "--origin", "--version", "--lang", "--state-dir", "--wait-creative", "--delay-ms", "--time-limit", "--memory-limit-mb"];
                if !WITH_VALUE.contains(&flag) {
                    return Err(tr!("unknown option {}", "알 수 없는 옵션 {}", flag));
                }
                let v = it.next().ok_or_else(|| tr!("{} needs a value", "{} 에 값이 필요합니다", flag))?;
                let number = || v.parse::<u64>().map_err(|_| tr!("{} needs a number", "{} 에는 숫자가 필요합니다", flag));
                match flag {
                    "--name" => a.name = v.clone(),
                    "--origin" => a.origin = Some(parse_origin(v)?),
                    "--version" => a.version = Some(v.clone()),
                    "--state-dir" => a.state_dir = PathBuf::from(v),
                    "--wait-creative" => a.wait_creative = Duration::from_secs(number()?.min(3600)),
                    "--delay-ms" => a.delay = Duration::from_millis(number()?.min(10_000)),
                    "--time-limit" => a.time_limit = Duration::from_secs(number()?),
                    "--memory-limit-mb" => a.memory_mb = number()?.min(1 << 20) as usize,
                    _ => {} // --lang was applied before parsing
                }
            }
        }
    }
    let valid_name = (3..=16).contains(&a.name.len()) && a.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_name {
        return Err(tr!("--name must be 3 to 16 letters, digits or _", "--name 은 영문, 숫자, _ 3~16자여야 합니다"));
    }
    let mut positional = positional.into_iter();
    a.source = positional.next().ok_or_else(usage)?;
    a.server = positional.next();
    if positional.next().is_some() || (a.server.is_none() && !a.dry_run) {
        return Err(usage());
    }
    Ok(a)
}

fn parse_origin(s: &str) -> Result<(i32, i32, i32), String> {
    let v: Option<Vec<i32>> = s.split(',').map(|p| p.trim().parse().ok()).collect();
    match v.as_deref() {
        Some(&[x, y, z]) if x.abs() < 29_000_000 && z.abs() < 29_000_000 && y.abs() < 4096 => Ok((x, y, z)),
        _ => Err(tr!("--origin needs x,y,z (whole numbers)", "--origin 은 x,y,z (정수) 형식입니다")),
    }
}

pub fn usage() -> String {
    tr!(
        "usage: build-bot build <schematic file or URL> <host:port> [--name N] [--origin x,y,z] [--lang en|ko] [--dry-run]\n  more: --version V --fresh --allow-partial --wait-creative SECONDS --delay-ms N --walk --state-dir DIR",
        "사용법: build-bot build <설계도 파일 또는 URL> <host:port> [--name 이름] [--origin x,y,z] [--lang en|ko] [--dry-run]\n  추가: --version V --fresh --allow-partial --wait-creative 초 --delay-ms N --walk --state-dir 폴더"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(list: &[&str]) -> Result<Args, String> {
        parse(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_short_form_and_the_options() {
        let a = p(&["a b.schem", "localhost:25565"]).unwrap();
        assert_eq!((a.source.as_str(), a.server.as_deref(), a.dry_run), ("a b.schem", Some("localhost:25565"), false));
        let a = p(&["x.schem", "--dry-run", "--origin", "1, -60,3", "--lang", "ko"]).unwrap();
        assert_eq!((a.origin, a.dry_run, a.server), (Some((1, -60, 3)), true, None));
    }

    #[test]
    fn mistakes_are_rejected() {
        for bad in [&["x.schem"][..], &["x", "h:1", "extra"], &["x", "h:1", "--dryrun"], &["x", "h:1", "--origin", "1,2"], &["x", "h:1", "--name", "a b"], &[]] {
            assert!(p(bad).is_err(), "{bad:?}");
        }
    }
}
