use super::*;
use crate::version::lookup;

/// Encode a container the way the server does: single value, palette or global ids.
fn container(b: &mut PacketBuf, values: &[u32], bits: u32, global: bool, sized: bool) {
    b.write_u8(bits as u8);
    if bits == 0 {
        b.write_varint(values[0] as i32);
        if sized {
            b.write_varint(0);
        }
        return;
    }
    let mut palette: Vec<u32> = values.to_vec();
    palette.sort_unstable();
    palette.dedup();
    if !global {
        b.write_varint(palette.len() as i32);
        for &p in &palette {
            b.write_varint(p as i32);
        }
    }
    let per_long = (64 / bits) as usize;
    let words: Vec<u64> = values
        .chunks(per_long)
        .map(|c| {
            c.iter().enumerate().fold(0u64, |w, (k, v)| {
                let raw = if global { *v } else { palette.binary_search(v).unwrap() as u32 };
                w | (u64::from(raw) << (k as u32 * bits))
            })
        })
        .collect();
    if sized {
        b.write_varint(words.len() as i32);
    }
    for w in words {
        b.write_u64(w);
    }
}

/// A chunk packet with `n` sections: section 0 uses `bits`, the others are air.
fn packet(binary: bool, n: usize, first: &[u32], bits: u32) -> PacketBuf {
    let mut data = PacketBuf::new();
    for s in 0..n {
        data.write_i16(0);
        if binary {
            data.write_i16(0);
        }
        if s == 0 {
            container(&mut data, first, bits, bits > 8, !binary);
        } else {
            container(&mut data, &[0], 0, false, !binary);
        }
        container(&mut data, &[1; 64], 0, false, !binary);
    }
    let mut p = PacketBuf::new();
    p.write_i32(-1);
    p.write_i32(2);
    if binary {
        p.write_varint(1); // one heightmap: type, two longs
        p.write_varint(4);
        p.write_varint(2);
        p.write_i64(7);
        p.write_i64(8);
    } else {
        // network NBT: compound { "H": long array [5] }
        p.write_bytes(&[10, 12, 0, 1, b'H', 0, 0, 0, 1]);
        p.write_i64(5);
        p.write_u8(0);
    }
    p.write_varint(data.as_slice().len() as i32);
    p.write_bytes(data.as_slice());
    p.write_varint(0); // block entities; light data would follow
    p
}

fn pattern(kinds: u32, step: u32) -> Vec<u32> {
    (0..4096u32).map(|i| (i * 7 % kinds) * step).collect()
}

#[test]
fn every_palette_form_decodes_in_both_layouts() {
    for name in ["1.20.4", "1.21.1", "1.21.4", "26.3"] {
        let layout = lookup(name).unwrap().layout;
        // linear (4 bits), hashed (5..8 bits), global (15 and 16 bits)
        for (kinds, step, bits) in [(3, 1, 4), (30, 9, 5), (200, 100, 8), (500, 50, 15), (900, 39, 16)] {
            let want = pattern(kinds, step);
            let mut p = packet(layout.binary_chunks, 24, &want, bits);
            let (at, sections) = parse_chunk(layout, &mut p).unwrap_or_else(|| panic!("{name} {bits}"));
            assert_eq!((at, sections.len()), ((-1, 2), 24), "{name}");
            let mut store = ChunkStore::default();
            store.load(at, sections);
            for i in [0usize, 1, 15, 16, 255, 256, 4095] {
                let at = BlockPos::new(-16 + (i & 15) as i32, -64 + (i >> 8) as i32, 32 + ((i >> 4) & 15) as i32);
                assert_eq!(store.state(at), Some(want[i]), "{name} {bits} bits, cell {i}");
            }
            assert_eq!(store.state(BlockPos::new(-16, 100, 32)), Some(0));
        }
    }
}

#[test]
fn single_value_sections_updates_and_forgetting() {
    let layout = lookup("1.21.1").unwrap().layout;
    let (at, sections) = parse_chunk(layout, &mut packet(false, 24, &[9], 0)).unwrap();
    let mut store = ChunkStore::default();
    let p = BlockPos::new(-3, -60, 40);
    store.set(p, 5); // not loaded yet: ignored, not invented
    assert_eq!(store.state(p), None);
    store.load(at, sections);
    assert_eq!(store.state(p), Some(9));
    store.set(p, 5);
    assert_eq!((store.state(p), store.state(BlockPos::new(-4, -60, 40))), (Some(5), Some(9)));
    let mut f = PacketBuf::new();
    f.write_i64((2i64 << 32) | (-1i32 as u32 as i64));
    store.forget(parse_forget(&mut f).unwrap());
    assert_eq!(store.state(p), None);
}

#[test]
fn truncated_or_unknown_height_data_is_not_air() {
    let layout = lookup("1.21.1").unwrap().layout;
    let whole = packet(false, 24, &pattern(3, 1), 4).into_vec();
    for cut in [3, 9, 40, 600] {
        assert!(parse_chunk(layout, &mut PacketBuf::from_vec(whole[..cut].to_vec())).is_none(), "{cut}");
    }
    let (at, sections) = parse_chunk(layout, &mut packet(false, 20, &[0], 0)).unwrap();
    let mut store = ChunkStore::default();
    store.load(at, sections);
    assert_eq!((store.unreadable, store.is_loaded(-16, 32)), (1, false));
}
