//! Server-confirmed block states, read from Block Update / Section Blocks Update packets.
//!
//! After every placement the server tells us what actually ended up in the world. A rejected
//! placement is answered with the old state (air), so "confirmed" means the last state the
//! server reported for the position is not air.

use crate::world::BlockPos;
use mc_proto::buf::PacketBuf;
use std::collections::HashMap;

/// Block state id of plain air in every release this bot supports.
const AIR_STATE: i32 = 0;

#[derive(Default)]
pub struct BlockLog {
    last: HashMap<BlockPos, i32>,
    /// Positions reported since the last [`BlockLog::drain_changed`].
    changed: Vec<BlockPos>,
}

impl BlockLog {
    pub fn record(&mut self, pos: BlockPos, state: i32) {
        self.last.insert(pos, state);
        self.changed.push(pos);
    }

    /// The server's last report for `pos` was a non-air block.
    pub fn is_confirmed(&self, pos: BlockPos) -> bool {
        self.last.get(&pos).is_some_and(|&s| s != AIR_STATE)
    }

    pub fn confirmed_count(&self) -> usize {
        self.last.values().filter(|&&s| s != AIR_STATE).count()
    }

    /// The state id the server last reported for `pos`.
    pub fn state(&self, pos: BlockPos) -> Option<i32> {
        self.last.get(&pos).copied()
    }

    /// Every position the server reported since the last call, including changes the bot did
    /// not ask for (neighbour updates, slab merges).
    pub fn drain_changed(&mut self) -> Vec<BlockPos> {
        std::mem::take(&mut self.changed)
    }

    /// All positions with a report, with the last reported state.
    pub fn reports(&self) -> impl Iterator<Item = (BlockPos, i32)> + '_ {
        self.last.iter().map(|(&p, &s)| (p, s))
    }

    /// Forget a position (used before a retry so an old answer cannot satisfy it).
    pub fn forget(&mut self, pos: BlockPos) {
        self.last.remove(&pos);
    }
}

/// Block Update: position + new state id.
pub fn parse_block_change(body: &mut PacketBuf) -> Option<(BlockPos, i32)> {
    let (x, y, z) = body.read_position().ok()?;
    let state = body.read_varint().ok()?;
    Some((BlockPos::new(x, y, z), state))
}

/// Section Blocks Update: packed section coordinates + one VarLong record per changed block
/// (`state << 12 | x << 8 | z << 4 | y`, coordinates local to the 16-block section).
pub fn parse_multi_block_change(body: &mut PacketBuf) -> Option<Vec<(BlockPos, i32)>> {
    let section = body.read_i64().ok()?;
    let sx = (section >> 42) as i32;
    let sz = ((section << 22) >> 42) as i32;
    let sy = ((section << 44) >> 44) as i32;
    let count = body.read_varint().ok()?;
    // a section has 4096 cells; a forged count must not allocate
    if !(0..=4096).contains(&count) {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let rec = body.read_varlong().ok()? as u64;
        let state = (rec >> 12) as i32;
        let (lx, lz, ly) = ((rec >> 8) & 0xF, (rec >> 4) & 0xF, rec & 0xF);
        out.push((BlockPos::new(sx * 16 + lx as i32, sy * 16 + ly as i32, sz * 16 + lz as i32), state));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_update_roundtrip() {
        let mut b = PacketBuf::new();
        b.write_position(-12, -60, 7);
        b.write_varint(2041);
        let (pos, state) = parse_block_change(&mut PacketBuf::from_vec(b.into_vec())).unwrap();
        assert_eq!((pos, state), (BlockPos::new(-12, -60, 7), 2041));
    }

    #[test]
    fn section_update_decodes_local_coordinates() {
        // Section (-1, 3, 2): x = -1, y = 3, z = 2 packed as x:22 | z:22 | y:20.
        let (sx, sy, sz) = (-1i64, 3i64, 2i64);
        let packed = ((sx & 0x3F_FFFF) << 42) | ((sz & 0x3F_FFFF) << 20) | (sy & 0xF_FFFF);
        let mut b = PacketBuf::new();
        b.write_i64(packed);
        b.write_varint(2);
        b.write_varint((77 << 12) | (5 << 8) | (9 << 4) | 15);
        b.write_varint(0);
        let got = parse_multi_block_change(&mut PacketBuf::from_vec(b.into_vec())).unwrap();
        assert_eq!(got[0], (BlockPos::new(-16 + 5, 48 + 15, 32 + 9), 77));
        assert_eq!(got[1].1, 0);
    }

    #[test]
    fn air_is_not_confirmed_and_last_report_wins() {
        let mut log = BlockLog::default();
        let p = BlockPos::new(1, 2, 3);
        assert!(!log.is_confirmed(p));
        log.record(p, 1);
        assert!(log.is_confirmed(p));
        log.record(p, 0); // placement reverted by the server
        assert!(!log.is_confirmed(p));
        assert_eq!(log.confirmed_count(), 0);
    }
}
