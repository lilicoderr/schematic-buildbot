//! A cooperative plan on disk: `multi.txt` (the strips and which file holds which plan) next to
//! one ordinary `.bbplan` per part, so every part can be read, simulated and replayed alone.

use super::{MultiPlan, Part};
use crate::territory::{Axis, Fence};
use std::path::Path;

const MANIFEST: &str = "multi.txt";

fn bound(v: i32) -> String {
    match v {
        i32::MIN => "-".into(),
        i32::MAX => "+".into(),
        v => v.to_string(),
    }
}

fn unbound(s: &str) -> Result<i32, String> {
    match s {
        "-" => Ok(i32::MIN),
        "+" => Ok(i32::MAX),
        v => v.parse().map_err(|_| format!("bad strip bound '{v}'")),
    }
}

pub fn plan_file(phase: usize, bot: usize) -> String {
    format!("phase{}-bot{}.bbplan", phase + 1, bot)
}

pub fn save(multi: &MultiPlan, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut text = format!("# bbmulti 1\nbots {}\n", multi.bots);
    for (k, phase) in multi.phases.iter().enumerate() {
        for p in phase {
            let axis = if p.fence.axis == Axis::X { "x" } else { "z" };
            text += &format!("part {} {} {axis} {} {}\n", k + 1, p.bot, bound(p.fence.lo), bound(p.fence.hi));
            std::fs::write(dir.join(plan_file(k, p.bot)), p.plan.to_text())?;
        }
    }
    std::fs::write(dir.join(MANIFEST), text)
}

pub fn load(dir: &Path) -> Result<MultiPlan, String> {
    let text = std::fs::read_to_string(dir.join(MANIFEST)).map_err(|e| format!("{}: {e}", dir.join(MANIFEST).display()))?;
    let mut multi = MultiPlan::default();
    for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let f: Vec<&str> = line.split_whitespace().collect();
        match f[..] {
            ["bots", n] => multi.bots = n.parse().map_err(|_| "bad bot count")?,
            ["part", k, bot, axis, lo, hi] => {
                let (k, bot): (usize, usize) = (k.parse().map_err(|_| "bad phase")?, bot.parse().map_err(|_| "bad bot")?);
                let axis = if axis == "x" { Axis::X } else { Axis::Z };
                let fence = Fence { axis, lo: unbound(lo)?, hi: unbound(hi)? };
                let name = plan_file(k.saturating_sub(1), bot);
                let body = std::fs::read_to_string(dir.join(&name)).map_err(|e| format!("{name}: {e}"))?;
                let plan = crate::plan_parse::parse(&body).map_err(|e| format!("{name}: {e}"))?;
                while multi.phases.len() < k {
                    multi.phases.push(Vec::new());
                }
                multi.phases[k - 1].push(Part { bot, fence, plan });
            }
            _ => return Err(format!("{MANIFEST}: cannot read '{line}'")),
        }
    }
    if multi.bots == 0 || multi.phases.is_empty() {
        return Err(format!("{MANIFEST} lists no parts"));
    }
    Ok(multi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_plans_load_back_identical() {
        let schem = crate::schematic::Schematic::parse("palette S stone\nlayer 0\nSSSSSSSS\nlayer 1\nSSSSSSSS\n").unwrap();
        let multi = super::super::partition::partition(&schem, (2, 65, 2), 767, 2, &mut |_| {}).unwrap();
        let dir = std::env::temp_dir().join(format!("bb-multi-{}", std::process::id()));
        save(&multi, &dir).unwrap();
        let back = load(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(back.phases.len(), multi.phases.len());
        for (a, b) in back.phases.iter().flatten().zip(multi.phases.iter().flatten()) {
            assert_eq!((a.bot, a.fence, a.plan.to_text()), (b.bot, b.fence, b.plan.to_text()));
        }
    }
}
