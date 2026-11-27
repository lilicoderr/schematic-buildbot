//! Compare what a plan wants with the block states the server last reported (the state ids in
//! its block updates, decoded with the 1.21.1 table in [`crate::state_ids`]).

use crate::schematic::split_state;
use crate::state_ids::state_of;

/// `None` when the reported state satisfies `expected` (every listed property matches; properties
/// the schematic does not list are free). Otherwise a short description of the difference.
pub fn difference(expected: &str, reported: Option<i32>) -> Option<String> {
    let (name, props) = split_state(expected);
    let Some(id) = reported.filter(|&id| id != 0) else { return Some("missing".into()) };
    let Some((got_name, got)) = u32::try_from(id).ok().and_then(state_of) else { return Some(format!("unknown state id {id}")) };
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
