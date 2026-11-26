//! Blueprint model plus the hand-written layer/palette text format.
//!
//! ```text
//! name Small house
//! palette S stone
//! palette G glass
//! palette T oak_stairs[facing=east,half=bottom]
//! palette . air
//! layer 0
//! SSSSS
//! S...S
//! S...S
//! SSSSS
//! layer 1
//! SSSSS
//! S...S
//! SGGGS        # z grows downwards, x grows to the right
//! SSSSS
//! ```
//!
//! * Each row under `layer <y>` is one plane at that y. A row is one z, a character is one x.
//! * The palette character `.` (or a space) is air and is skipped.
//! * Coordinates are relative to (0,0,0); the builder adds its origin.
//! * A palette entry may carry block-state properties: `oak_stairs[facing=east]`.

use crate::tr;
use crate::world::{BlockPos, Face};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub pos: BlockPos,
    pub name: String,
    /// Block-state properties, e.g. `facing=east`. Empty when the default state is wanted.
    pub props: BTreeMap<String, String>,
}

impl Block {
    pub fn new(pos: BlockPos, name: impl Into<String>) -> Self {
        Block { pos, name: name.into(), props: BTreeMap::new() }
    }

    /// Build a block from a state string such as `minecraft:oak_stairs[facing=east,half=top]`.
    pub fn from_state(pos: BlockPos, state: &str) -> Self {
        let (name, props) = split_state(state);
        Block { pos, name, props }
    }

    pub fn prop(&self, key: &str) -> Option<&str> {
        self.props.get(key).map(String::as_str)
    }

    /// `name[k=v,...]` (or just `name` with no properties).
    pub fn state_string(&self) -> String {
        if self.props.is_empty() {
            return self.name.clone();
        }
        let p: Vec<String> = self.props.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{}[{}]", self.name, p.join(","))
    }
}

/// Split `minecraft:oak_stairs[facing=east]` into (`oak_stairs`, {facing: east}).
pub fn split_state(state: &str) -> (String, BTreeMap<String, String>) {
    let (head, rest) = match state.split_once('[') {
        Some((h, r)) => (h, r.trim_end_matches(']')),
        None => (state, ""),
    };
    let name = head.strip_prefix("minecraft:").unwrap_or(head).trim().to_string();
    let mut props = BTreeMap::new();
    for kv in rest.split(',').filter(|s| !s.is_empty()) {
        if let Some((k, v)) = kv.split_once('=') {
            props.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    (name, props)
}

#[derive(Debug, Clone, Default)]
pub struct Schematic {
    pub name: String,
    pub blocks: Vec<Block>,
}

impl Schematic {
    pub fn parse(text: &str) -> Result<Schematic, String> {
        let mut name = String::from("blueprint");
        let mut palette: HashMap<char, String> = HashMap::new();
        palette.insert('.', "air".into());
        palette.insert(' ', "air".into());
        let mut blocks = Vec::new();
        let mut cur_y: Option<i32> = None;
        let mut z = 0i32;

        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("");
            let trimmed = line.trim_end();
            if trimmed.trim().is_empty() {
                continue;
            }
            let ln = lineno + 1;
            let mut words = trimmed.trim_start().splitn(3, char::is_whitespace);
            let head = words.next().unwrap_or("");
            match head {
                "name" => {
                    name = trimmed.trim_start()[4..].trim().to_string();
                }
                "palette" => {
                    let ch = words.next().ok_or(tr!("line {ln}: palette character missing", "{ln}: palette 글자 없음"))?;
                    let block = words.next().ok_or(tr!("line {ln}: palette block missing", "{ln}: palette 블록 없음"))?;
                    let c = ch.chars().next().unwrap();
                    palette.insert(c, block.trim().to_string());
                }
                "layer" => {
                    let y: i32 = words
                        .next()
                        .ok_or(tr!("line {ln}: layer y missing", "{ln}: layer y 없음"))?
                        .trim()
                        .parse()
                        .map_err(|_| tr!("line {ln}: layer y is not a number", "{ln}: layer y 가 숫자가 아님"))?;
                    cur_y = Some(y);
                    z = 0;
                }
                _ => {
                    // A block row inside the current layer. Leading spaces count as air cells.
                    let y = cur_y.ok_or(tr!(
                        "line {ln}: block row before any `layer` line",
                        "{ln}: layer 선언 전에 블록 행이 나옴"
                    ))?;
                    for (x, c) in line.trim_end_matches(['\r', '\n']).chars().enumerate() {
                        // Characters missing from the palette are treated as air.
                        let state = palette.get(&c).map(String::as_str).unwrap_or("air");
                        let block = Block::from_state(BlockPos::new(x as i32, y, z), state);
                        if block.name != "air" {
                            blocks.push(block);
                        }
                    }
                    z += 1;
                }
            }
        }

        if blocks.is_empty() {
            return Err(tr!("the blueprint contains no blocks", "블록이 하나도 없습니다."));
        }
        Ok(Schematic { name, blocks })
    }

    /// Build order that keeps every block anchored when possible.
    ///
    /// Bottom-up by layer; inside a layer, blocks resting on something come first and
    /// blocks that only touch an already-placed neighbour follow. Returns
    /// `(order, unanchored)` where `unanchored` blocks have no possible support at all
    /// (they would float) and are appended at the end of the order.
    pub fn plan_order(&self) -> (Vec<&Block>, Vec<&Block>) {
        let (min, _) = self.bounds();
        let mut sorted: Vec<&Block> = self.blocks.iter().collect();
        sorted.sort_by_key(|b| (b.pos.y, b.pos.z, b.pos.x));

        let mut placed: HashSet<BlockPos> = HashSet::new();
        let mut order: Vec<&Block> = Vec::with_capacity(sorted.len());
        let mut unanchored: Vec<&Block> = Vec::new();

        let mut i = 0;
        while i < sorted.len() {
            let y = sorted[i].pos.y;
            let mut j = i;
            while j < sorted.len() && sorted[j].pos.y == y {
                j += 1;
            }
            let mut pending: Vec<&Block> = sorted[i..j].to_vec();
            loop {
                let mut progressed = false;
                let mut rest = Vec::new();
                for b in pending {
                    let below = b.pos.below();
                    let anchored = b.pos.y == min.y
                        || placed.contains(&below)
                        || Face::HORIZONTAL.iter().any(|f| placed.contains(&b.pos.offset(*f)));
                    if anchored {
                        placed.insert(b.pos);
                        order.push(b);
                        progressed = true;
                    } else {
                        rest.push(b);
                    }
                }
                pending = rest;
                if !progressed || pending.is_empty() {
                    break;
                }
            }
            // Leftovers have nothing to rest on. They are not marked as placed, so blocks that
            // would only rest on them are reported as unanchored too.
            for b in pending {
                unanchored.push(b);
                order.push(b);
            }
            i = j;
        }
        (order, unanchored)
    }

    /// Anchored build order (see [`Schematic::plan_order`]).
    pub fn build_order(&self) -> Vec<&Block> {
        self.plan_order().0
    }

    /// Blocks that can never be placed against a neighbour (they would float).
    pub fn unanchored_blocks(&self) -> Vec<&Block> {
        self.plan_order().1
    }

    /// Blocks with nothing directly below them (above the lowest layer).
    pub fn floating_blocks(&self) -> Vec<&Block> {
        let set: HashSet<(i32, i32, i32)> = self.blocks.iter().map(|b| (b.pos.x, b.pos.y, b.pos.z)).collect();
        let (min, _) = self.bounds();
        self.blocks
            .iter()
            .filter(|b| b.pos.y > min.y && !set.contains(&(b.pos.x, b.pos.y - 1, b.pos.z)))
            .collect()
    }

    /// Count per block name, most common first.
    pub fn block_counts(&self) -> Vec<(String, usize)> {
        let mut map: BTreeMap<String, usize> = BTreeMap::new();
        for b in &self.blocks {
            *map.entry(b.name.clone()).or_insert(0) += 1;
        }
        let mut v: Vec<_> = map.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// Number of blocks that carry state properties.
    pub fn stateful_count(&self) -> usize {
        self.blocks.iter().filter(|b| !b.props.is_empty()).count()
    }

    /// Layer-by-layer ASCII view (one symbol per distinct block, most common first).
    pub fn ascii_layers(&self) -> String {
        const SYMS: &[u8] = b"#ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let counts = self.block_counts();
        let sym = |i: usize| *SYMS.get(i).unwrap_or(&b'?') as char;
        let sym_of = |name: &str| sym(counts.iter().position(|(n, _)| n == name).unwrap_or(0));
        let (min, max) = self.bounds();
        let w = (max.x - min.x + 1) as usize;
        let d = (max.z - min.z + 1) as usize;
        let mut out = String::new();
        for y in min.y..=max.y {
            let mut grid = vec![vec!['.'; w]; d];
            let mut any = false;
            for b in self.blocks.iter().filter(|b| b.pos.y == y) {
                grid[(b.pos.z - min.z) as usize][(b.pos.x - min.x) as usize] = sym_of(&b.name);
                any = true;
            }
            if !any {
                continue;
            }
            out.push_str(&format!("layer y={y}\n"));
            for row in grid {
                out.push_str(&row.into_iter().collect::<String>());
                out.push('\n');
            }
        }
        let legend: Vec<String> =
            counts.iter().enumerate().map(|(i, (n, _))| format!("{}={}", sym(i), n)).collect();
        out.push_str(&format!("legend: {}\n", legend.join(" ")));
        out
    }

    pub fn bounds(&self) -> (BlockPos, BlockPos) {
        let mut min = self.blocks[0].pos;
        let mut max = self.blocks[0].pos;
        for b in &self.blocks {
            min.x = min.x.min(b.pos.x);
            min.y = min.y.min(b.pos.y);
            min.z = min.z.min(b.pos.z);
            max.x = max.x.max(b.pos.x);
            max.y = max.y.max(b.pos.y);
            max.z = max.z.max(b.pos.z);
        }
        (min, max)
    }

    /// Stable 64-bit fingerprint of the block list (FNV-1a); used to match resume files.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut feed = |bytes: &[u8]| {
            for b in bytes {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        };
        for b in &self.blocks {
            feed(&b.pos.x.to_le_bytes());
            feed(&b.pos.y.to_le_bytes());
            feed(&b.pos.z.to_le_bytes());
            feed(b.state_string().as_bytes());
            feed(&[0xff]);
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_layers_and_palette() {
        let src = "name Test\npalette S stone\npalette . air\nlayer 0\nSS\nS.\nlayer 1\n.S\n";
        let s = Schematic::parse(src).unwrap();
        assert_eq!(s.name, "Test");
        // layer 0: (0,0,0) (1,0,0) (0,0,1) are stone, (1,0,1) is air; layer 1: (1,1,0)
        assert_eq!(s.blocks.len(), 4);
        assert!(s.blocks.iter().any(|b| b.pos == BlockPos::new(0, 0, 0) && b.name == "stone"));
        assert!(s.blocks.iter().any(|b| b.pos == BlockPos::new(1, 1, 0) && b.name == "stone"));
    }

    #[test]
    fn palette_accepts_block_state() {
        let src = "palette T minecraft:oak_stairs[facing=east,half=top]\nlayer 0\nT\n";
        let s = Schematic::parse(src).unwrap();
        let b = &s.blocks[0];
        assert_eq!(b.name, "oak_stairs");
        assert_eq!(b.prop("facing"), Some("east"));
        assert_eq!(b.prop("half"), Some("top"));
        assert_eq!(b.state_string(), "oak_stairs[facing=east,half=top]");
        assert_eq!(s.stateful_count(), 1);
    }

    #[test]
    fn split_state_handles_plain_and_bracketed() {
        assert_eq!(split_state("minecraft:stone"), ("stone".to_string(), BTreeMap::new()));
        let (n, p) = split_state("oak_log[axis=x]");
        assert_eq!(n, "oak_log");
        assert_eq!(p["axis"], "x");
    }

    #[test]
    fn build_order_is_bottom_up() {
        let s = Schematic::parse("palette S stone\nlayer 2\nS\nlayer 0\nS\n").unwrap();
        let order = s.build_order();
        assert_eq!(order[0].pos.y, 0);
        assert_eq!(order[1].pos.y, 2);
    }

    #[test]
    fn empty_is_error() {
        assert!(Schematic::parse("palette S stone\nlayer 0\n...\n").is_err());
    }

    #[test]
    fn block_counts_sorted() {
        let s = Schematic::parse("palette S stone\npalette G glass\nlayer 0\nSSG\nSSS\n").unwrap();
        let counts = s.block_counts();
        assert_eq!(counts[0], ("stone".to_string(), 5));
        assert_eq!(counts[1], ("glass".to_string(), 1));
    }

    #[test]
    fn ascii_layers_shows_grid_and_legend() {
        let s = Schematic::parse("palette S stone\npalette G glass\nlayer 0\nSS\nSG\n").unwrap();
        let a = s.ascii_layers();
        assert!(a.contains("layer y=0\n##\n#A\n"));
        assert!(a.contains("#=stone"));
        assert!(a.contains("A=glass"));
    }

    #[test]
    fn floating_blocks_detected() {
        let s = Schematic::parse("palette S stone\nlayer 0\nS.\nlayer 1\nSS\n").unwrap();
        let f = s.floating_blocks();
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].pos.x, f[0].pos.y), (1, 1));
    }

    #[test]
    fn supported_tower_has_no_floating() {
        let s = Schematic::parse("palette S stone\nlayer 0\nS\nlayer 1\nS\nlayer 2\nS\n").unwrap();
        assert!(s.floating_blocks().is_empty());
    }

    #[test]
    fn bounds_ok() {
        let s = Schematic::parse("palette S stone\nlayer 0\nSSS\nlayer 3\nS\n").unwrap();
        let (min, max) = s.bounds();
        assert_eq!((min.x, min.y, max.x, max.y), (0, 0, 2, 3));
    }

    #[test]
    fn overhang_is_placed_after_its_anchor() {
        // Layer 1 row is "SS" but only x=1 has support below, so x=1 must come first.
        let s = Schematic::parse("palette S stone\nlayer 0\n.S\nlayer 1\nSS\n").unwrap();
        let (order, unanchored) = s.plan_order();
        let layer1: Vec<i32> = order.iter().filter(|b| b.pos.y == 1).map(|b| b.pos.x).collect();
        assert_eq!(layer1, vec![1, 0]);
        assert!(unanchored.is_empty());
    }

    #[test]
    fn every_block_comes_after_something_it_can_rest_on() {
        let s = Schematic::parse(
            "palette S stone\nlayer 0\n..S\nlayer 1\n.SS\nlayer 2\nSSS\n",
        )
        .unwrap();
        let (order, unanchored) = s.plan_order();
        assert!(unanchored.is_empty());
        let mut placed: HashSet<BlockPos> = HashSet::new();
        let (min, _) = s.bounds();
        for b in order {
            let ok = b.pos.y == min.y
                || placed.contains(&b.pos.below())
                || Face::HORIZONTAL.iter().any(|f| placed.contains(&b.pos.offset(*f)));
            assert!(ok, "block {:?} placed without support", b.pos);
            placed.insert(b.pos);
        }
    }

    #[test]
    fn island_in_the_air_is_reported() {
        let s = Schematic::parse("palette S stone\nlayer 0\nS\nlayer 2\n..S\n").unwrap();
        let un = s.unanchored_blocks();
        assert_eq!(un.len(), 1);
        assert_eq!(un[0].pos.y, 2);
    }

    #[test]
    fn fingerprint_changes_with_state() {
        let a = Schematic::parse("palette T oak_stairs[facing=east]\nlayer 0\nT\n").unwrap();
        let b = Schematic::parse("palette T oak_stairs[facing=west]\nlayer 0\nT\n").unwrap();
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint(), a.clone().fingerprint());
    }
}
