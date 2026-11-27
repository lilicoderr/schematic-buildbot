//! Exact numeric block-state ids (protocol 767), backed by the generated [`crate::state_table`].
//! Ids are laid out per block as a mixed-radix number over its properties (last one fastest).

use crate::state_table::{BLOCKS, PROPS, STATE_COUNT};
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

type Row = (&'static str, u32, u16, &'static [u8]);

fn row(name: &str) -> Option<&'static Row> {
    static INDEX: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    let index = INDEX.get_or_init(|| BLOCKS.iter().enumerate().map(|(i, r)| (r.0, i)).collect());
    index.get(name.strip_prefix("minecraft:").unwrap_or(name)).map(|&i| &BLOCKS[i])
}

fn radix(prop: u8) -> u32 {
    PROPS[prop as usize].1.split(',').count() as u32
}

/// Property values of the state at `offset` inside a block's id range.
fn decode(refs: &[u8], mut offset: u32) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for &p in refs.iter().rev() {
        let (name, values) = PROPS[p as usize];
        let value = values.split(',').nth((offset % radix(p)) as usize).unwrap_or("");
        out.insert(name.to_string(), value.to_string());
        offset /= radix(p);
    }
    out
}

/// All properties of the block's default state.
pub fn default_props(name: &str) -> Option<BTreeMap<String, String>> {
    row(name).map(|r| decode(r.3, r.2 as u32))
}

/// Id of `name` with `props`; properties not given take their default. `None` for an unknown
/// block, property or value.
pub fn state_id(name: &str, props: &BTreeMap<String, String>) -> Option<u32> {
    let r = row(name)?;
    let mut all = decode(r.3, r.2 as u32);
    for (k, v) in props {
        *all.get_mut(k)? = v.clone();
    }
    let mut offset = 0;
    for &p in r.3 {
        let (key, values) = PROPS[p as usize];
        let i = values.split(',').position(|v| v == all[key])?;
        offset = offset * radix(p) + i as u32;
    }
    Some(r.1 + offset)
}

/// Block name and full property map of a state id.
pub fn state_of(id: u32) -> Option<(&'static str, BTreeMap<String, String>)> {
    if id >= STATE_COUNT {
        return None;
    }
    let i = BLOCKS.partition_point(|r| r.1 <= id) - 1;
    let r = &BLOCKS[i];
    Some((r.0, decode(r.3, id - r.1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schematic::split_state;

    #[test]
    fn known_ids_from_the_report() {
        assert_eq!(state_id("air", &BTreeMap::new()), Some(0));
        assert_eq!(state_id("stone", &BTreeMap::new()), Some(1));
        let (n, p) = split_state("minecraft:oak_door[facing=north,half=upper,hinge=left,open=true,powered=true]");
        assert_eq!(state_id(&n, &p), Some(4590));
    }

    #[test]
    fn every_id_round_trips() {
        for id in 0..STATE_COUNT {
            let (name, props) = state_of(id).unwrap();
            assert_eq!(state_id(name, &props), Some(id), "{name}");
        }
        assert!(state_of(STATE_COUNT).is_none());
    }

    #[test]
    fn unknown_names_and_values_are_rejected() {
        assert!(state_id("no_such_block", &BTreeMap::new()).is_none());
        let (n, p) = split_state("oak_log[axis=w]");
        assert!(state_id(&n, &p).is_none());
    }
}
