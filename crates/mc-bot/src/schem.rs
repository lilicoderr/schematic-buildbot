//! Sponge `.schem`(v2/v3) 설계도 파서 → [`Schematic`].
//!
//! 공유 사이트에서 받은 WorldEdit/Sponge 스키매틱을 그대로 건축할 수 있게 한다.
//! 블록 상태(방향 등)는 무시하고 블록 이름만 사용한다(설치는 기본 상태로).

use crate::nbt::{self, Nbt};
use crate::schematic::{Block, Schematic};
use crate::world::BlockPos;
use std::collections::HashMap;

fn is_air(name: &str) -> bool {
    matches!(name, "air" | "cave_air" | "void_air")
}

// "minecraft:oak_stairs[facing=east]" → "oak_stairs"
fn clean_state(state: &str) -> String {
    let no_props = state.split('[').next().unwrap_or(state);
    no_props.strip_prefix("minecraft:").unwrap_or(no_props).to_string()
}

// 바이트 슬라이스에서 LEB128 varint 하나 읽기 → (값, 소비한 바이트 수)
fn read_varint(data: &[u8], start: usize) -> Option<(i32, usize)> {
    let mut result: u32 = 0;
    let mut shift = 0;
    let mut i = start;
    loop {
        let byte = *data.get(i)?;
        i += 1;
        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return None;
        }
    }
    Some((result as i32, i - start))
}

pub fn parse(bytes: &[u8], name: &str) -> Result<Schematic, String> {
    let (_root_name, root) = nbt::read(bytes)?;
    // 일부 파일은 { "Schematic": {...} } 로 한 번 감싼다.
    let schem = root.get("Schematic").unwrap_or(&root);

    let width = schem.get("Width").and_then(Nbt::as_i32).ok_or("Width 없음")?;
    let height = schem.get("Height").and_then(Nbt::as_i32).ok_or("Height 없음")?;
    let length = schem.get("Length").and_then(Nbt::as_i32).ok_or("Length 없음")?;

    // v2: 최상위 Palette+BlockData / v3: Blocks { Palette, Data }
    let (palette_tag, data_bytes) = if let Some(blocks) = schem.get("Blocks") {
        (blocks.get("Palette"), blocks.get("Data").and_then(Nbt::as_byte_array))
    } else {
        (schem.get("Palette"), schem.get("BlockData").and_then(Nbt::as_byte_array))
    };
    let palette_map = palette_tag.and_then(Nbt::as_compound).ok_or("Palette 없음")?;
    let data = data_bytes.ok_or("BlockData/Data 없음")?;

    // 인덱스 → 블록 이름
    let mut index_to_name: HashMap<i32, String> = HashMap::new();
    for (state, idx) in palette_map {
        if let Some(i) = idx.as_i32() {
            index_to_name.insert(i, clean_state(state));
        }
    }

    // BlockData: varint 배열, 순서 = y, z, x
    let mut blocks = Vec::new();
    let mut pos = 0usize;
    let mut i = 0i64;
    let total = (width as i64) * (height as i64) * (length as i64);
    while pos < data.len() && i < total {
        let (idx, used) = read_varint(data, pos).ok_or("BlockData varint 파싱 오류")?;
        pos += used;
        let x = (i % width as i64) as i32;
        let z = ((i / width as i64) % length as i64) as i32;
        let y = (i / (width as i64 * length as i64)) as i32;
        i += 1;
        if let Some(bn) = index_to_name.get(&idx) {
            if !is_air(bn) {
                blocks.push(Block { pos: BlockPos::new(x, y, z), name: bn.clone() });
            }
        }
    }

    if blocks.is_empty() {
        return Err("블록이 하나도 없습니다(전부 공기이거나 파싱 실패).".into());
    }
    Ok(Schematic { name: name.to_string(), blocks })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_reader() {
        assert_eq!(read_varint(&[0x00], 0), Some((0, 1)));
        assert_eq!(read_varint(&[0x7f], 0), Some((127, 1)));
        assert_eq!(read_varint(&[0x80, 0x01], 0), Some((128, 2)));
        assert_eq!(read_varint(&[0xdd, 0xc7, 0x01], 0), Some((25565, 3)));
    }

    #[test]
    fn clean_state_strips() {
        assert_eq!(clean_state("minecraft:oak_stairs[facing=east,half=bottom]"), "oak_stairs");
        assert_eq!(clean_state("stone"), "stone");
    }

    #[test]
    fn parse_sample_schem() {
        // 실제 Sponge v2 포맷 샘플(gzip NBT): 2x2x2, stone 7 + glass 1
        let bytes = include_bytes!("../tests/fixtures/sample.schem");
        let s = parse(bytes, "sample").unwrap();
        assert_eq!(s.blocks.len(), 8);
        let counts = s.block_counts();
        assert_eq!(counts[0], ("stone".to_string(), 7));
        assert_eq!(counts[1], ("glass".to_string(), 1));
        let (min, max) = s.bounds();
        assert_eq!((max.x - min.x + 1, max.y - min.y + 1, max.z - min.z + 1), (2, 2, 2));
    }
}
