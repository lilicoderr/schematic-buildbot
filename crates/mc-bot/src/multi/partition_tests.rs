use super::*;
use crate::multi::{joint, placed_by};
use std::path::Path;

fn factory() -> Schematic {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../blueprints/factory-small.schem");
    crate::fetch::parse(&crate::fetch::fetch(path.to_str().unwrap()).unwrap()).unwrap()
}

const ORIGIN: (i32, i32, i32) = (2, 65, 2);

fn plan(schem: &Schematic, bots: usize) -> MultiPlan {
    partition(schem, ORIGIN, 767, bots, &mut |m| println!("{m}")).unwrap()
}

#[test]
fn small_factory_splits_between_two_and_four_bots_and_is_exact() {
    let schem = factory();
    let single = crate::slice::slice(&schem, ORIGIN, 767).unwrap();
    for bots in [2, 4] {
        let multi = plan(&schem, bots);
        let report = joint::check(&multi, &schem, ORIGIN).unwrap();
        let blocks: usize = multi.phases.iter().flatten().map(|p| placed_by(&p.plan).len()).sum();
        assert_eq!(blocks, schem.blocks.len(), "every block exactly once");
        assert!(report.interleavings >= 6);
        for part in multi.phases.iter().flatten() {
            assert!(part.plan.header.unreachable.is_empty());
            assert!(!part.plan.header.stats.iter().any(|(k, _)| k == "fallback_block"));
        }
        let (one, many) = (crate::multi::est_seconds(&single), multi.est_seconds());
        println!("{bots} bots: estimate {many:.0} s, single {one:.0} s ({:.2}x)", one / many);
        assert!(many < one, "{bots} bots must be estimated faster than one");
    }
}

#[test]
fn the_joint_check_rejects_overlapping_strips_and_foreign_scaffold() {
    let row = "S".repeat(12);
    let schem = Schematic::parse(&format!("palette S stone\nlayer 0\n{row}\nlayer 1\n{row}\n")).unwrap();
    let good = plan(&schem, 2);
    joint::check(&good, &schem, ORIGIN).unwrap();
    let mut overlap = good.clone();
    overlap.phases[0][1].fence = Fence::OPEN;
    assert!(joint::check(&overlap, &schem, ORIGIN).unwrap_err().contains("overlap"));
    // bot 1 taking over bot 0's plan leaves its strip
    let mut foreign = good.clone();
    let steps = foreign.phases[0][0].plan.steps.clone();
    foreign.phases[0][1].plan.steps = steps;
    assert!(joint::check(&foreign, &schem, ORIGIN).unwrap_err().contains("outside its strip"));
    // a plan with a missing placement does not end in the schematic
    let mut short = good;
    let at = short.phases[0][0].plan.steps.iter().rposition(|s| matches!(s, crate::plan::Step::Place { .. })).unwrap();
    short.phases[0][0].plan.steps.remove(at);
    assert!(joint::check(&short, &schem, ORIGIN).is_err());
}

#[test]
fn cuts_never_split_a_bed_or_a_double_chest() {
    let text = "palette B red_bed[facing=east,part=foot]\npalette H red_bed[facing=east,part=head]\npalette S stone\nlayer 0\nSSSBHSSS\n";
    let schem = Schematic::parse(text).unwrap();
    let at: HashMap<BlockPos, &Block> = schem.blocks.iter().map(|b| (b.pos, b)).collect();
    assert!(!legal_cut(&at, Axis::X, 4));
    assert!(legal_cut(&at, Axis::X, 3) && legal_cut(&at, Axis::X, 5));
}
