use super::*;

#[test]
fn default_is_1_21_1() {
    assert_eq!((DEFAULT.name, DEFAULT.protocol), ("1.21.1", 767));
    assert_eq!(lookup("v1.21").unwrap().protocol, 767);
    assert_eq!(by_protocol(767).unwrap().name, "1.21.1");
}

#[test]
fn unsupported_and_unknown_explain_themselves() {
    let e = lookup("1.21.11").unwrap_err();
    assert!(e.contains("1.21.11") && e.contains("26.3"), "{e}");
    assert!(lookup("9.9").is_err());
    assert!(by_protocol(768).is_none());
}

#[test]
fn tables_match_their_version() {
    for v in &SUPPORTED {
        assert_eq!(v.tables.protocol, v.protocol);
        assert_eq!(v.tables.default_state("air"), Some(0), "{}: the block log relies on air = 0", v.name);
        let total: u32 = v.tables.blocks.iter().map(|b| b.3).sum();
        assert_eq!(total, v.tables.state_count);
        assert!(v.tables.item_id("minecraft:cobblestone").is_some());
    }
}

/// Anchors from the reports; the 1.21.1 row repeats the former hand-written tables.
#[test]
fn per_version_anchors() {
    let anchor = |n: &str| {
        let t = lookup(n).unwrap().tables;
        (t.items.len(), t.state_count, t.packets.sb_play_use_item_on, t.packets.cb_play_player_position)
    };
    assert_eq!(anchor("1.20.4"), (1312, 26644, 53, 62));
    assert_eq!(anchor("1.21.1"), (1333, 26684, 56, 64));
    assert_eq!(anchor("1.21.4"), (1385, 27866, 60, 66));
    assert_eq!(anchor("26.3"), (1658, 35723, 66, 73));
}

#[test]
fn state_lookup_finds_the_owner() {
    let t = DEFAULT.tables;
    assert_eq!(t.block_of_state(0), Some("air"));
    assert_eq!(t.block_of_state(14), Some("cobblestone"));
    assert_eq!(t.block_of_state(2874), Some("oak_stairs"));
    assert_eq!(t.block_of_state(2953), Some("oak_stairs"));
    assert_eq!(t.block_of_state(t.state_count), None);
}

#[test]
fn optional_packets_follow_the_releases() {
    let p = |n: &str| lookup(n).unwrap().packets();
    assert_eq!(p("1.20.4").sb_config_select_known_packs, -1);
    assert_eq!(p("1.21.1").sb_play_player_loaded, -1);
    assert!(p("1.21.4").sb_play_player_loaded >= 0 && p("1.21.4").sb_play_client_tick_end >= 0);
    assert_eq!(p("26.3").sb_play_swing, -1);
}
