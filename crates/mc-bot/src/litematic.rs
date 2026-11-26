//! Litematica `.litematic` parser → [`Schematic`].
//!
//! Each region stores a block-state palette and a bit-packed long array
//! (entries may straddle two longs). Block properties are ignored, like in `schem.rs`.

use crate::nbt::{self, Nbt};
use crate::schematic::{Block, Schematic};
use crate::world::BlockPos;

fn is_air(name: &str) -> bool {
    matches!(name, "air" | "cave_air" | "void_air")
}

fn xyz(tag: Option<&Nbt>) -> Option<(i32, i32, i32)> {
    let t = tag?;
    Some((t.get("x")?.as_i32()?, t.get("y")?.as_i32()?, t.get("z")?.as_i32()?))
}

/// Read the `idx`-th `bits`-wide value from a packed long array.
fn packed_value(longs: &[i64], idx: u64, bits: u32) -> Option<u64> {
    let start = idx * bits as u64;
    let li = (start >> 6) as usize;
    let off = (start & 63) as u32;
    let mut v = (*longs.get(li)? as u64) >> off;
    if off + bits > 64 {
        v |= (*longs.get(li + 1)? as u64) << (64 - off);
    }
    Some(v & ((1u64 << bits) - 1))
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Schematic, String> {
    let (_n, root) = nbt::read(bytes)?;
    let regions = root.get("Regions").and_then(Nbt::as_compound).ok_or("Regions 없음")?;

    let mut raw: Vec<(i32, i32, i32, String)> = Vec::new();
    for region in regions.values() {
        let (px, py, pz) = xyz(region.get("Position")).ok_or("Position 없음")?;
        let (sx, sy, sz) = xyz(region.get("Size")).ok_or("Size 없음")?;
        let palette: Vec<String> = region
            .get("BlockStatePalette")
            .and_then(Nbt::as_list)
            .ok_or("BlockStatePalette 없음")?
            .iter()
            .map(|e| {
                let n = e.get("Name").and_then(Nbt::as_str).unwrap_or("air");
                n.strip_prefix("minecraft:").unwrap_or(n).to_string()
            })
            .collect();
        let longs = region.get("BlockStates").and_then(Nbt::as_long_array).ok_or("BlockStates 없음")?;
        let bits = (usize::BITS - palette.len().saturating_sub(1).leading_zeros()).max(2);
        let (w, h, l) = (sx.abs(), sy.abs(), sz.abs());
        // 음수 크기는 Position 에서 반대 방향으로 뻗는다.
        let (ox, oy, oz) = (
            px + if sx < 0 { sx + 1 } else { 0 },
            py + if sy < 0 { sy + 1 } else { 0 },
            pz + if sz < 0 { sz + 1 } else { 0 },
        );
        for y in 0..h {
            for z in 0..l {
                for x in 0..w {
                    let idx = ((y as u64) * (l as u64) + z as u64) * (w as u64) + x as u64;
                    let v = packed_value(longs, idx, bits).ok_or("BlockStates 길이 부족")? as usize;
                    if let Some(bn) = palette.get(v) {
                        if !is_air(bn) {
                            raw.push((ox + x, oy + y, oz + z, bn.clone()));
                        }
                    }
                }
            }
        }
    }
    if raw.is_empty() {
        return Err("블록이 하나도 없습니다(전부 공기이거나 파싱 실패).".into());
    }
    let minx = raw.iter().map(|b| b.0).min().unwrap();
    let miny = raw.iter().map(|b| b.1).min().unwrap();
    let minz = raw.iter().map(|b| b.2).min().unwrap();
    let blocks = raw
        .into_iter()
        .map(|(x, y, z, n)| Block { pos: BlockPos::new(x - minx, y - miny, z - minz), name: n })
        .collect();
    Ok(Schematic { name: name.to_string(), blocks })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_name(out: &mut Vec<u8>, tag: u8, name: &str) {
        out.push(tag);
        out.extend((name.len() as u16).to_be_bytes());
        out.extend(name.as_bytes());
    }
    fn int_xyz(out: &mut Vec<u8>, name: &str, v: (i32, i32, i32)) {
        tag_name(out, 10, name);
        for (k, n) in [("x", v.0), ("y", v.1), ("z", v.2)] {
            tag_name(out, 3, k);
            out.extend(n.to_be_bytes());
        }
        out.push(0);
    }

    // 2x1x2 region, palette [air, stone, glass], 2 bits/entry: stone, glass, air, stone
    fn sample(size: (i32, i32, i32)) -> Vec<u8> {
        let mut o = Vec::new();
        tag_name(&mut o, 10, "");
        tag_name(&mut o, 10, "Regions");
        tag_name(&mut o, 10, "main");
        int_xyz(&mut o, "Position", (0, 0, 0));
        int_xyz(&mut o, "Size", size);
        tag_name(&mut o, 9, "BlockStatePalette");
        o.push(10);
        o.extend(3i32.to_be_bytes());
        for n in ["minecraft:air", "minecraft:stone", "minecraft:glass"] {
            tag_name(&mut o, 8, "Name");
            o.extend((n.len() as u16).to_be_bytes());
            o.extend(n.as_bytes());
            o.push(0);
        }
        tag_name(&mut o, 12, "BlockStates");
        o.extend(1i32.to_be_bytes());
        let packed: i64 = 1 | (2 << 2) | (1 << 6);
        o.extend(packed.to_be_bytes());
        o.extend([0, 0, 0]);
        o
    }

    #[test]
    fn packed_values_across_longs() {
        // 5-bit entries: index 12 starts at bit 60 and straddles two longs
        let longs = [(0b10110i64) << 60, 0b1];
        assert_eq!(packed_value(&longs, 12, 5), Some(0b10110));
    }

    #[test]
    fn parses_region() {
        let s = parse(&sample((2, 1, 2)), "t").unwrap();
        assert_eq!(s.blocks.len(), 3);
        let counts = s.block_counts();
        assert_eq!(counts[0], ("stone".to_string(), 2));
        assert_eq!(counts[1], ("glass".to_string(), 1));
        assert!(s.blocks.iter().any(|b| b.pos == BlockPos::new(1, 0, 0) && b.name == "glass"));
    }

    #[test]
    fn negative_size_is_normalised_to_origin() {
        let s = parse(&sample((-2, 1, -2)), "t").unwrap();
        let (min, _) = s.bounds();
        assert_eq!((min.x, min.y, min.z), (0, 0, 0));
    }
}
