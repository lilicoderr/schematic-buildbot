//! The blocks of the loaded chunks, read from level_chunk_with_light and kept up to date with
//! block updates (docs/RE/protocol.md; layout differences in docs/RE/versions.md).

use crate::version::Layout;
use crate::world::BlockPos;
use mc_proto::buf::PacketBuf;
use std::collections::HashMap;

const CELLS: usize = 4096;
/// Widest entry any release needs; anything larger is a decoding error.
const MAX_BITS: u32 = 20;

/// One 16x16x16 section, indexed `y << 8 | z << 4 | x`.
#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    Uniform(u32),
    Dense(Box<[u32; CELLS]>),
}

impl Section {
    fn get(&self, i: usize) -> u32 {
        match self {
            Section::Uniform(s) => *s,
            Section::Dense(d) => d[i],
        }
    }

    fn set(&mut self, i: usize, state: u32) {
        if let Section::Uniform(s) = *self {
            if s == state {
                return;
            }
            *self = Section::Dense(Box::new([s; CELLS]));
        }
        if let Section::Dense(d) = self {
            d[i] = state;
        }
    }
}

/// A paletted container of `len` entries; palettes are used up to `max_palette_bits`.
fn read_container(b: &mut PacketBuf, len: usize, max_palette_bits: u32, sized: bool) -> Option<Section> {
    let bits = u32::from(b.read_u8().ok()?);
    if bits > MAX_BITS {
        return None;
    }
    let varint = |b: &mut PacketBuf| b.read_varint().ok().and_then(|v| u32::try_from(v).ok());
    let mut palette = Vec::new();
    if bits == 0 {
        let single = varint(b)?;
        // releases that send the length say 0 here
        if sized && varint(b)? != 0 {
            return None;
        }
        return Some(Section::Uniform(single));
    }
    if bits <= max_palette_bits {
        let n = varint(b)? as usize;
        if n > 1 << bits {
            return None;
        }
        for _ in 0..n {
            palette.push(varint(b)?);
        }
    }
    let per_long = (64 / bits) as usize;
    let longs = len.div_ceil(per_long);
    if sized && varint(b)? as usize != longs {
        return None;
    }
    let mut out = Box::new([0u32; CELLS]);
    let mask = (1u64 << bits) - 1;
    for l in 0..longs {
        // entries never span two longs
        let word = b.read_u64().ok()?;
        for k in 0..per_long.min(len - l * per_long) {
            let raw = ((word >> (k as u32 * bits)) & mask) as u32;
            let i = l * per_long + k;
            out[i] = if bits <= max_palette_bits { *palette.get(raw as usize)? } else { raw };
        }
    }
    Some(Section::Dense(out))
}

/// Skip the heightmaps in front of the section data.
fn skip_heightmaps(layout: &Layout, b: &mut PacketBuf) -> Option<()> {
    if layout.binary_chunks {
        // map: heightmap type -> long array
        for _ in 0..b.read_varint().ok().filter(|n| (0..=16).contains(n))? {
            b.read_varint().ok()?;
            let n = usize::try_from(b.read_varint().ok()?).ok()?;
            b.read_bytes(n.checked_mul(8)?).ok()?;
        }
        return Some(());
    }
    let mut rest = b.clone();
    let used = crate::nbt::network_len(&rest.read_rest())?;
    b.read_bytes(used).ok()?;
    Some(())
}

/// Parse level_chunk_with_light up to the end of the block data: (chunk x, chunk z, sections
/// from the bottom of the world).
pub fn parse_chunk(layout: &Layout, b: &mut PacketBuf) -> Option<((i32, i32), Vec<Section>)> {
    let at = (b.read_i32().ok()?, b.read_i32().ok()?);
    skip_heightmaps(layout, b)?;
    let size = usize::try_from(b.read_varint().ok()?).ok()?;
    let mut data = PacketBuf::from_vec(b.read_bytes(size).ok()?);
    let mut sections = Vec::new();
    let sized = !layout.binary_chunks;
    while data.remaining() > 0 {
        data.read_i16().ok()?; // non-air count
        if layout.binary_chunks {
            data.read_i16().ok()?; // fluid count
        }
        sections.push(read_container(&mut data, CELLS, 8, sized)?);
        read_container(&mut data, 64, 3, sized)?; // biomes
        if sections.len() > 256 {
            return None;
        }
    }
    Some((at, sections))
}

/// forget_level_chunk: one long, x in the low half, z in the high half.
pub fn parse_forget(b: &mut PacketBuf) -> Option<(i32, i32)> {
    let v = b.read_i64().ok()?;
    Some((v as i32, (v >> 32) as i32))
}

struct Column {
    min_y: i32,
    sections: Vec<Section>,
}

#[derive(Default)]
pub struct ChunkStore {
    columns: HashMap<(i32, i32), Column>,
    /// Chunk packets that could not be decoded (never silently treated as air).
    pub unreadable: u32,
}

/// Bottom of the world for a vanilla dimension with `sections` sections (overworld 24, nether
/// and end 16). Custom world heights are not decoded: their chunks stay unknown.
fn min_y_for(sections: usize) -> Option<i32> {
    match sections {
        24 => Some(-64),
        16 => Some(0),
        _ => None,
    }
}

impl ChunkStore {
    pub fn load(&mut self, at: (i32, i32), sections: Vec<Section>) {
        match min_y_for(sections.len()) {
            Some(min_y) => {
                self.columns.insert(at, Column { min_y, sections });
            }
            None => self.unreadable += 1,
        }
    }

    pub fn forget(&mut self, at: (i32, i32)) {
        self.columns.remove(&at);
    }

    pub fn clear(&mut self) {
        self.columns.clear();
    }

    pub fn is_loaded(&self, x: i32, z: i32) -> bool {
        self.columns.contains_key(&(x >> 4, z >> 4))
    }

    /// State id at `p`; `None` while its chunk is not loaded. Above and below the world is air.
    pub fn state(&self, p: BlockPos) -> Option<u32> {
        let c = self.columns.get(&(p.x >> 4, p.z >> 4))?;
        let s = usize::try_from((p.y - c.min_y) >> 4).ok().and_then(|i| c.sections.get(i));
        Some(s.map_or(0, |s| s.get(index(p))))
    }

    /// Apply a block update; ignored for chunks that are not loaded.
    pub fn set(&mut self, p: BlockPos, state: u32) {
        let Some(c) = self.columns.get_mut(&(p.x >> 4, p.z >> 4)) else { return };
        let i = usize::try_from((p.y - c.min_y) >> 4).ok();
        if let Some(s) = i.and_then(|i| c.sections.get_mut(i)) {
            s.set(index(p), state);
        }
    }
}

fn index(p: BlockPos) -> usize {
    (((p.y & 15) << 8) | ((p.z & 15) << 4) | (p.x & 15)) as usize
}

#[cfg(test)]
#[path = "chunks_tests.rs"]
mod tests;
