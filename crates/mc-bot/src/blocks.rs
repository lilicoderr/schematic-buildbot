//! Block name → item id of the active version ([`crate::version::active`]), used to put a blueprint
//! block in hand before placing it. The tables are generated, see [`crate::data`].

/// Name of the item that places `block` (wall variants use the standing item;
/// docs/RE/placement.md 5.1).
pub(crate) fn item_name(block: &str) -> std::borrow::Cow<'_, str> {
    match block {
        "redstone_wire" => "redstone".into(),
        "tripwire" => "string".into(),
        "wall_torch" => "torch".into(),
        "soul_wall_torch" => "soul_torch".into(),
        "redstone_wall_torch" => "redstone_torch".into(),
        _ => {
            for kind in ["hanging_sign", "sign", "banner", "head", "skull", "fan"] {
                if let Some(stem) = block.strip_suffix(kind).and_then(|s| s.strip_suffix("wall_")) {
                    return format!("{stem}{kind}").into();
                }
            }
            block.into()
        }
    }
}

/// Item/block name (with or without the `minecraft:` prefix) → numeric id, or None if unknown.
pub fn item_id(name: &str) -> Option<i32> {
    let key = name.strip_prefix("minecraft:").unwrap_or(name);
    crate::version::active().tables.item_id(&item_name(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_version_ids() {
        assert_eq!(item_id("air"), Some(0));
        assert_eq!(item_id("minecraft:stone"), Some(1));
        assert_eq!(item_id("cobblestone"), Some(35));
        assert_eq!(item_id("no_such_block"), None);
        assert_eq!(item_id("minecraft:wall_torch"), item_id("torch"));
        assert_eq!(item_id("oak_wall_sign"), item_id("oak_sign"));
    }
}
