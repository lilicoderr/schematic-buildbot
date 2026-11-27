//! Slicer properties: determinism, and "simulates clean or reports unreachable explicitly" on
//! random small schematics.

use mc_bot::plan::Step;
use mc_bot::schematic::{Block, Schematic};
use mc_bot::world::BlockPos;
use mc_bot::{plan_parse, sim, slice};

fn small_factory() -> Schematic {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../blueprints/factory-small.schem")).unwrap();
    mc_bot::schem::parse(&bytes, "factory-small").unwrap()
}

#[test]
fn same_input_gives_a_byte_identical_plan() {
    let s = small_factory();
    let a = slice::slice(&s, (2, 64, 2), 767).unwrap().to_text();
    let b = slice::slice(&s, (2, 64, 2), 767).unwrap().to_text();
    assert_eq!(a, b);
    assert!(a.len() > 10_000);
}

#[test]
fn small_factory_plan_has_nothing_unreachable_and_roundtrips() {
    let s = small_factory();
    let plan = slice::slice(&s, (2, 64, 2), 767).unwrap();
    assert!(plan.header.unreachable.is_empty(), "{:?}", plan.header.unreachable.len());
    let text = plan.to_text();
    let again = plan_parse::parse(&text).unwrap();
    assert_eq!(again.to_text(), text);
    assert!(sim::simulate(&again, &s).is_ok());
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn random_schematic(seed: u64) -> Schematic {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let (w, h, d) = (2 + r.below(6) as i32, 1 + r.below(7) as i32, 2 + r.below(6) as i32);
    let density = 20 + r.below(70);
    let mut blocks = Vec::new();
    for y in 0..h {
        for x in 0..w {
            for z in 0..d {
                if r.below(100) < density {
                    let pos = BlockPos::new(x, y, z);
                    let state = match r.below(5) {
                        0 => "minecraft:stone",
                        1 => "minecraft:glass",
                        2 => "minecraft:oak_planks",
                        3 => "minecraft:oak_stairs[facing=east,half=bottom]",
                        _ => "minecraft:smooth_stone_slab[type=bottom]",
                    };
                    blocks.push(Block::from_state(pos, state));
                }
            }
        }
    }
    if blocks.is_empty() {
        blocks.push(Block::new(BlockPos::new(0, 0, 0), "stone"));
    }
    Schematic { name: format!("fuzz{seed}"), blocks }
}

#[test]
fn random_schematics_simulate_clean_or_report_unreachable() {
    for seed in 0..150u64 {
        let s = random_schematic(seed);
        match slice::slice(&s, (2, 64, 2), 767) {
            Ok(plan) => {
                let placed = plan.steps.iter().filter(|st| matches!(st, Step::Place { .. } | Step::Auto { .. })).count();
                assert_eq!(placed + plan.header.unreachable.len(), s.blocks.len(), "seed {seed}: every block is placed or reported");
                assert!(sim::simulate(&plan, &s).is_ok(), "seed {seed}");
            }
            Err(e) => assert!(!e.contains("slicer bug"), "seed {seed}: {e}"),
        }
    }
}
