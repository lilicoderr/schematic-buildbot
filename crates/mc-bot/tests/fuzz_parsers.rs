//! Deterministic fuzzing of everything that reads untrusted bytes: no panics, no unbounded
//! allocation. (A failed allocation aborts the process, so "the test finishes" covers memory.)

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use mc_bot::schematic::Schematic;
use mc_bot::{blocklog, litematic, nbt, plan_parse, schem, text};
use mc_proto::buf::PacketBuf;
use mc_proto::frame;
use std::io::{Read, Write};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// A few random edits: bytes, "huge number" words, truncation, duplication.
fn mutate(r: &mut Rng, seed: &[u8]) -> Vec<u8> {
    let mut v = seed.to_vec();
    for _ in 0..1 + r.below(6) {
        if v.is_empty() {
            break;
        }
        let at = r.below(v.len());
        match r.below(5) {
            0 => v[at] = r.next() as u8,
            1 => {
                let word: [i32; 6] = [i32::MAX, i32::MIN, -1, 0, 65_536, 0x7fff];
                for (i, b) in word[r.below(6)].to_be_bytes().iter().enumerate() {
                    if let Some(x) = v.get_mut(at + i) {
                        *x = *b;
                    }
                }
            }
            2 => v.truncate(at),
            3 => {
                let copy = v[at..(at + 32).min(v.len())].to_vec();
                v.splice(at..at, copy);
            }
            _ => v[at] ^= 1 << r.below(8),
        }
    }
    v
}

fn parse_all(bytes: &[u8]) {
    let _ = nbt::read(bytes);
    let _ = nbt::read_network(bytes);
    let _ = schem::parse(bytes, "f");
    let _ = litematic::parse(bytes, "f");
    let _ = text::component_text(bytes);
    let _ = blocklog::parse_block_change(&mut PacketBuf::from_vec(bytes.to_vec()));
    let _ = blocklog::parse_multi_block_change(&mut PacketBuf::from_vec(bytes.to_vec()));
    for threshold in [None, Some(16)] {
        let _ = frame::try_parse_frame(bytes, threshold);
        let _ = frame::read_frame(&mut std::io::Cursor::new(bytes), threshold);
    }
}

fn sample_nbt() -> Vec<u8> {
    let mut out = Vec::new();
    let gz: &[u8] = include_bytes!("fixtures/sample.schem");
    GzDecoder::new(gz).read_to_end(&mut out).unwrap();
    out
}

#[test]
fn random_and_mutated_bytes_never_panic() {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let seed = sample_nbt();
    let factory: &[u8] = include_bytes!("../../../blueprints/factory-small.schem");
    for i in 0..6000 {
        let junk: Vec<u8> = (0..r.below(200)).map(|_| r.next() as u8).collect();
        parse_all(&junk);
        parse_all(&mutate(&mut r, &seed));
        if i % 200 == 0 {
            parse_all(&mutate(&mut r, factory));
        }
    }
}

/// `{ name: <tag>` header of one named entry.
fn named(tag: u8, name: &str) -> Vec<u8> {
    let mut v = vec![tag, 0, name.len() as u8];
    v.extend_from_slice(name.as_bytes());
    v
}

#[test]
fn forged_lengths_do_not_allocate() {
    for tag in [7u8, 9, 11, 12] {
        let mut b = named(10, "");
        b.extend(named(tag, "x"));
        if tag == 9 {
            b.push(10);
        }
        b.extend(i32::MAX.to_be_bytes());
        assert!(nbt::read(&b).is_err(), "tag {tag}");
    }
}

#[test]
fn deep_nesting_is_an_error_not_a_stack_overflow() {
    let mut b = named(10, "");
    for _ in 0..100_000 {
        b.extend(named(10, "a"));
    }
    assert!(nbt::read(&b).is_err());
    let mut lists = named(9, "");
    for _ in 0..100_000 {
        lists.extend([9, 0, 0, 0, 1]);
    }
    assert!(nbt::read(&lists).is_err());
}

#[test]
fn a_gzip_bomb_is_refused() {
    let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let zeros = vec![0u8; 1 << 20];
    for _ in 0..=(nbt::MAX_DECOMPRESSED >> 20) {
        enc.write_all(&zeros).unwrap();
    }
    assert!(nbt::read(&enc.finish().unwrap()).unwrap_err().contains("decompressed"));
}

#[test]
fn schem_with_impossible_dimensions_is_an_error() {
    for (w, h, l) in [(0, 1, 1), (1, 1, 0), (-1, 1, 1), (i32::MAX, i32::MAX, i32::MAX)] {
        let mut b = named(10, "");
        for (name, v) in [("Width", w), ("Height", h), ("Length", l)] {
            b.extend(named(3, name));
            b.extend(i32::to_be_bytes(v));
        }
        b.extend(named(10, "Palette"));
        b.extend(named(3, "stone"));
        b.extend(0i32.to_be_bytes());
        b.push(0);
        b.extend(named(7, "BlockData"));
        b.extend(4i32.to_be_bytes());
        b.extend([0, 0, 0, 0, 0]);
        assert!(schem::parse(&b, "t").is_err(), "{w} {h} {l}");
    }
}

#[test]
fn text_blueprints_and_plans_never_panic() {
    let mut r = Rng(42);
    let blueprint = include_bytes!("../../../blueprints/state-lab.txt");
    let plan = "# bbplan 1\n# origin 0 64 0\n# start -2 64 -2\nMOVE 1 64 2\nPILLAR 1 64 2\n\
                PLACE 2 64 2 stone - 2,63,2,up,0.5,1,0.5\nSUPPORT 1 1 1\nAUTO 1 2 3 x[\nBREAK 1 2 3\nCHECKPOINT 1 1 ff\n";
    for _ in 0..4000 {
        let t = String::from_utf8_lossy(&mutate(&mut r, blueprint)).into_owned();
        let _ = Schematic::parse(&t);
        let p = String::from_utf8_lossy(&mutate(&mut r, plan.as_bytes())).into_owned();
        if let Ok(p) = plan_parse::parse(&p) {
            let s = Schematic::parse("palette S stone\nlayer 0\nS\n").unwrap();
            let _ = mc_bot::sim::simulate(&p, &s);
        }
    }
    assert!(plan_parse::parse("# bbplan 1\nMOVE 2147483647 0 0\n").is_err());
    assert!(Schematic::parse("palette S stone\nlayer 2147483647\nS\n").is_err());
}

#[test]
fn oversized_frames_are_refused_before_allocating() {
    let mut huge = PacketBuf::new();
    huge.write_varint(i32::MAX);
    assert!(frame::try_parse_frame(huge.as_slice(), None).is_err());
    assert!(frame::read_frame(&mut std::io::Cursor::new(huge.as_slice()), None).is_err());
    // a tiny frame claiming to inflate to 2 GB
    let mut inner = PacketBuf::new();
    inner.write_varint(i32::MAX);
    inner.write_bytes(&[0x78, 0x9c, 3, 0, 0, 0, 0, 1]);
    let mut wire = PacketBuf::new();
    wire.write_varint(inner.as_slice().len() as i32);
    wire.write_bytes(inner.as_slice());
    assert!(frame::try_parse_frame(wire.as_slice(), Some(1)).is_err());
}
