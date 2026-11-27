//! Reads the text form written by [`crate::plan::Plan::to_text`].

use crate::plan::{ClickStep, Header, Plan, Step, FORMAT_VERSION};
use crate::world::{BlockPos, Face};

fn num<T: std::str::FromStr>(s: Option<&str>, line: usize) -> Result<T, String> {
    s.and_then(|v| v.parse().ok()).ok_or_else(|| format!("line {line}: bad number"))
}

fn click(tok: &str, line: usize) -> Result<ClickStep, String> {
    let p: Vec<&str> = tok.split(',').collect();
    if p.len() != 7 {
        return Err(format!("line {line}: bad click '{tok}'"));
    }
    let face = Face::from_name(p[3]).ok_or_else(|| format!("line {line}: bad face '{}'", p[3]))?;
    Ok(ClickStep {
        support: BlockPos::new(num(p.first().copied(), line)?, num(p.get(1).copied(), line)?, num(p.get(2).copied(), line)?),
        face,
        cursor: (num(p.get(4).copied(), line)?, num(p.get(5).copied(), line)?, num(p.get(6).copied(), line)?),
    })
}

fn header_line(h: &mut Header, rest: &str, line: usize, seen_version: &mut bool) -> Result<(), String> {
    let mut it = rest.split_whitespace();
    match it.next() {
        Some("bbplan") => {
            let v: u32 = num(it.next(), line)?;
            if v != FORMAT_VERSION {
                return Err(format!("unsupported plan format version {v}"));
            }
            *seen_version = true;
        }
        Some("schematic") => h.schematic_hash = u64::from_str_radix(it.next().unwrap_or(""), 16).map_err(|e| e.to_string())?,
        Some("origin") => h.origin = (num(it.next(), line)?, num(it.next(), line)?, num(it.next(), line)?),
        Some("protocol") => h.protocol = num(it.next(), line)?,
        Some("stat") => {
            let k = it.next().unwrap_or("").to_string();
            h.stats.push((k, it.collect::<Vec<_>>().join(" ")));
        }
        Some("unreachable") => {
            let p = BlockPos::new(num(it.next(), line)?, num(it.next(), line)?, num(it.next(), line)?);
            h.unreachable.push((p, it.next().unwrap_or("").to_string()));
        }
        _ => {}
    }
    Ok(())
}

pub fn parse(text: &str) -> Result<Plan, String> {
    let mut plan = Plan::default();
    let mut seen_version = false;
    for (i, raw) in text.lines().enumerate() {
        let line = i + 1;
        if let Some(rest) = raw.strip_prefix('#') {
            header_line(&mut plan.header, rest, line, &mut seen_version)?;
            continue;
        }
        if raw.trim().is_empty() {
            continue;
        }
        let mut it = raw.split_whitespace();
        let step = match it.next() {
            Some("MOVE") => Step::Move((num(it.next(), line)?, num(it.next(), line)?, num(it.next(), line)?)),
            Some("PILLAR") => Step::Pillar((num(it.next(), line)?, num(it.next(), line)?, num(it.next(), line)?)),
            Some("BREAK") => Step::Break(BlockPos::new(num(it.next(), line)?, num(it.next(), line)?, num(it.next(), line)?)),
            Some("CHECKPOINT") => Step::Checkpoint {
                n: num(it.next(), line)?,
                count: num(it.next(), line)?,
                hash: u64::from_str_radix(it.next().unwrap_or(""), 16).map_err(|e| e.to_string())?,
            },
            Some("PLACE") => {
                let pos = BlockPos::new(num(it.next(), line)?, num(it.next(), line)?, num(it.next(), line)?);
                let state = it.next().ok_or_else(|| format!("line {line}: no state"))?.to_string();
                let yaw = match it.next() {
                    Some("-") => None,
                    other => Some(num(other, line)?),
                };
                let clicks = it.map(|t| click(t, line)).collect::<Result<Vec<_>, _>>()?;
                Step::Place { pos, state, yaw, clicks }
            }
            other => return Err(format!("line {line}: unknown step {other:?}")),
        };
        plan.steps.push(step);
    }
    if !seen_version {
        return Err("missing '# bbplan <version>' header".into());
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> Plan {
        let mut p = Plan::default();
        p.header.schematic_hash = 0xdead_beef;
        p.header.origin = (-3, 64, 9);
        p.header.protocol = 767;
        p.header.stats.push(("blocks".into(), "2".into()));
        p.header.unreachable.push((BlockPos::new(1, 2, 3), "glass".into()));
        p.steps = vec![
            Step::Move((1, 64, 2)),
            Step::Pillar((1, 64, 2)),
            Step::Place {
                pos: BlockPos::new(2, 64, 2),
                state: "minecraft:oak_stairs[facing=east,half=top]".into(),
                yaw: Some(-90.0),
                clicks: vec![ClickStep { support: BlockPos::new(2, 63, 2), face: Face::Up, cursor: (0.5, 1.0, 0.75) }],
            },
            Step::Break(BlockPos::new(1, 64, 2)),
            Step::Checkpoint { n: 1, count: 1, hash: 0xabc },
        ];
        p
    }

    #[test]
    fn text_roundtrip_is_exact() {
        let p = sample();
        let text = p.to_text();
        assert_eq!(parse(&text).unwrap(), p);
        assert_eq!(parse(&text).unwrap().to_text(), text);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("MOVE 1 2 3\n").is_err(), "needs a header");
        assert!(parse("# bbplan 99\n").is_err());
        assert!(parse("# bbplan 1\nJUMP 1\n").is_err());
        assert!(parse("# bbplan 1\nMOVE 1 x 3\n").is_err());
    }
}
