//! 설계도(blueprint) 포맷 — 사람이 손으로 쓰기 쉬운 레이어/팔레트 텍스트.
//!
//! ```text
//! name 작은 집
//! palette S stone
//! palette G glass
//! palette . air
//! layer 0
//! SSSSS
//! S...S
//! S...S
//! SSSSS
//! layer 1
//! SSSSS
//! S...S
//! SGGGS        # z 는 아래로, x 는 오른쪽으로 증가
//! SSSSS
//! ```
//!
//! · `layer <y>` 아래의 각 줄이 한 평면(그 y). 줄 = z 증가, 글자 = x 증가.
//! · 팔레트 글자 `.`(또는 공백)은 공기로 간주해 건너뛴다.
//! · 좌표는 (0,0,0) 기준 상대. 실제 건축 시 origin 을 더한다.

use crate::world::BlockPos;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Block {
    pub pos: BlockPos,
    pub name: String,
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
            let mut words = trimmed.trim_start().splitn(3, char::is_whitespace);
            let head = words.next().unwrap_or("");
            match head {
                "name" => {
                    name = trimmed.trim_start()[4..].trim().to_string();
                }
                "palette" => {
                    let ch = words.next().ok_or(format!("{}: palette 글자 없음", lineno + 1))?;
                    let block = words.next().ok_or(format!("{}: palette 블록 없음", lineno + 1))?;
                    let c = ch.chars().next().unwrap();
                    palette.insert(c, block.trim().to_string());
                }
                "layer" => {
                    let y: i32 = words
                        .next()
                        .ok_or(format!("{}: layer y 없음", lineno + 1))?
                        .trim()
                        .parse()
                        .map_err(|_| format!("{}: layer y 가 숫자가 아님", lineno + 1))?;
                    cur_y = Some(y);
                    z = 0;
                }
                _ => {
                    // 블록 행 (현재 레이어 안)
                    let y = cur_y.ok_or(format!("{}: layer 선언 전에 블록 행이 나옴", lineno + 1))?;
                    // 들여쓰기를 유지하려고 원본(주석 제거본)에서 좌측 공백도 글자로 취급
                    for (x, c) in line.trim_end_matches(['\r', '\n']).chars().enumerate() {
                        let block = palette.get(&c).cloned().unwrap_or_else(|| {
                            // 팔레트에 없는 글자는 공기로 두되 경고 대신 무시
                            "air".to_string()
                        });
                        if block != "air" {
                            blocks.push(Block {
                                pos: BlockPos::new(x as i32, y, z),
                                name: block,
                            });
                        }
                    }
                    z += 1;
                }
            }
        }

        if blocks.is_empty() {
            return Err("블록이 하나도 없습니다.".into());
        }
        Ok(Schematic { name, blocks })
    }

    /// 건축 순서: 아래층부터(y 오름차순), 같은 층은 (z,x) 순.
    pub fn build_order(&self) -> Vec<&Block> {
        let mut v: Vec<&Block> = self.blocks.iter().collect();
        v.sort_by_key(|b| (b.pos.y, b.pos.z, b.pos.x));
        v
    }

    /// Blocks with nothing directly below them (above the lowest layer).
    /// The bot places by clicking the block underneath, so these need a temporary support.
    pub fn floating_blocks(&self) -> Vec<&Block> {
        use std::collections::HashSet;
        let set: HashSet<(i32, i32, i32)> = self.blocks.iter().map(|b| (b.pos.x, b.pos.y, b.pos.z)).collect();
        let (min, _) = self.bounds();
        self.blocks
            .iter()
            .filter(|b| b.pos.y > min.y && !set.contains(&(b.pos.x, b.pos.y - 1, b.pos.z)))
            .collect()
    }

    /// 블록 이름별 개수 (많은 순).
    pub fn block_counts(&self) -> Vec<(String, usize)> {
        use std::collections::BTreeMap;
        let mut map: BTreeMap<String, usize> = BTreeMap::new();
        for b in &self.blocks {
            *map.entry(b.name.clone()).or_insert(0) += 1;
        }
        let mut v: Vec<_> = map.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_layers_and_palette() {
        let src = "name 테스트\npalette S stone\npalette . air\nlayer 0\nSS\nS.\nlayer 1\n.S\n";
        let s = Schematic::parse(src).unwrap();
        assert_eq!(s.name, "테스트");
        // layer0: (0,0,0)S (1,0,0)S (0,0,1)S ; (1,0,1)=air → 3개
        // layer1: (1,1,1)? z=0 ".S" → x0=air x1=S => (1,1,0)S  → 1개
        assert_eq!(s.blocks.len(), 4);
        assert!(s.blocks.iter().any(|b| b.pos == BlockPos::new(0, 0, 0) && b.name == "stone"));
        assert!(s.blocks.iter().any(|b| b.pos == BlockPos::new(1, 1, 0) && b.name == "stone"));
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
}
