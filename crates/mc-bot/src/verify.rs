//! Compare what a plan wants with the block states the server last reported (the state ids in
//! its block updates). Properties come from the 1.21.1 table in [`crate::state_ids`]; ids of other
//! releases are translated by block name and offset inside the block.

use crate::data::{Tables, V767};
use crate::schematic::split_state;
use crate::state_ids::state_of;
use std::collections::BTreeMap;

/// Block name and properties of a state id of the release described by `tables`. A block with the
/// same number of states as in 1.21.1 has the same layout (compared in the reports of 1.20.4,
/// 1.21.4 and 26.3); otherwise, and for blocks 1.21.1 lacks, only the name is known.
fn decode(tables: &Tables, id: u32) -> Option<(&'static str, BTreeMap<String, String>)> {
    let name = tables.block_of_state(id)?;
    let find = |t: &Tables| t.blocks.iter().find(|b| b.0 == name).map(|b| (b.1, b.3));
    let (first, count) = find(tables)?;
    let props = match find(V767) {
        Some((reference, n)) if n == count => state_of(reference + (id - first)).map(|s| s.1).unwrap_or_default(),
        _ => BTreeMap::new(),
    };
    Some((name, props))
}

/// `None` when the reported state satisfies `expected` (every listed property matches; properties
/// the schematic does not list are free). Otherwise a short description of the difference.
pub fn difference(expected: &str, reported: Option<i32>) -> Option<String> {
    let (name, props) = split_state(expected);
    let Some(id) = reported.filter(|&id| id != 0) else { return Some("missing".into()) };
    let tables = crate::version::active().tables;
    let Some((got_name, got)) = u32::try_from(id).ok().and_then(|id| decode(tables, id)) else { return Some(format!("unknown state id {id}")) };
    if got_name != name {
        return Some(format!("is {got_name}"));
    }
    let wrong: Vec<String> = props
        .iter()
        .filter(|(k, v)| got.get(*k).is_some_and(|g| g != *v))
        .map(|(k, v)| format!("{k}: want {v}, got {}", got[k]))
        .collect();
    (!wrong.is_empty()).then(|| wrong.join("; "))
}

/// A double slab takes two clicks; a refused second click leaves a single slab, which looks
/// "placed" unless the state is read.
pub fn half_done_double_slab(expected: &str, reported: Option<i32>) -> bool {
    let (name, props) = split_state(expected);
    name.ends_with("_slab") && props.get("type").is_some_and(|t| t == "double") && difference(expected, reported).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_ids::state_id;

    fn id(state: &str) -> Option<i32> {
        let (n, p) = split_state(state);
        state_id(&n, &p).map(|v| v as i32)
    }

    #[test]
    fn matches_only_the_listed_properties() {
        assert_eq!(difference("oak_stairs[facing=east]", id("oak_stairs[facing=east,half=top]")), None);
        assert_eq!(difference("stone", id("stone")), None);
    }

    #[test]
    fn reports_orientation_shape_and_connection_differences() {
        let d = difference("oak_trapdoor[facing=east,open=false]", id("oak_trapdoor[facing=west,open=false]")).unwrap();
        assert_eq!(d, "facing: want east, got west");
        assert!(difference("observer[facing=up]", id("observer[facing=down]")).is_some());
        assert!(difference("oak_stairs[shape=inner_left]", id("oak_stairs[shape=straight]")).unwrap().contains("shape"));
        assert!(difference("glass_pane[east=true]", id("glass_pane[east=false]")).is_some());
    }

    #[test]
    fn states_of_other_releases_are_translated() {
        for release in [crate::data::V765, crate::data::V769, crate::data::V777] {
            let stairs = release.blocks.iter().find(|b| b.0 == "oak_stairs").unwrap();
            // the same offset means the same properties as in 1.21.1, whatever the first id is
            let (name, props) = decode(release, stairs.1 + 7).unwrap();
            let (_, want) = state_of(V767.blocks.iter().find(|b| b.0 == "oak_stairs").unwrap().1 + 7).unwrap();
            assert_eq!((name, props), ("oak_stairs", want), "{}", release.protocol);
        }
        // 26.3 has more note_block states than 1.21.1: only the name is trusted
        let note = crate::data::V777.default_state("note_block").unwrap();
        assert_eq!(decode(crate::data::V777, note), Some(("note_block", BTreeMap::new())));
    }

    #[test]
    fn missing_and_wrong_blocks() {
        assert_eq!(difference("stone", None).as_deref(), Some("missing"));
        assert_eq!(difference("stone", Some(0)).as_deref(), Some("missing"));
        assert_eq!(difference("stone", id("glass")).as_deref(), Some("is glass"));
    }

    #[test]
    fn a_single_slab_where_a_double_one_is_wanted_is_half_done() {
        assert!(half_done_double_slab("stone_slab[type=double]", id("stone_slab[type=bottom]")));
        assert!(!half_done_double_slab("stone_slab[type=double]", id("stone_slab[type=double]")));
        assert!(!half_done_double_slab("stone_slab[type=top]", id("stone_slab[type=bottom]")));
    }
}
