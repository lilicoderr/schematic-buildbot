//! The families slice into a plan the simulator accepts.

use mc_bot::schematic::Schematic;

#[test]
fn double_slab_wall_torch_and_pair_chest_slice() {
    let text = "name t\npalette S stone\npalette D stone_slab[type=double]\npalette T wall_torch[facing=south]\n\
                palette L chest[facing=south,type=right]\npalette R chest[facing=south,type=left]\n\
                layer 0\nSSSS\n....\nlayer 1\nSSSS\nT.LR\nlayer 2\n.D..\n....\n";
    let schem = Schematic::parse(text).unwrap();
    let plan = mc_bot::slice::slice(&schem, (0, 64, 0), 767).unwrap();
    assert!(plan.header.unreachable.is_empty(), "{:?}", plan.header.unreachable);
    let stat = plan.header.stats.iter().find(|(k, _)| k == "state_mismatches").unwrap();
    assert_eq!(stat.1, "0", "{:?}", plan.header.stats);
}
