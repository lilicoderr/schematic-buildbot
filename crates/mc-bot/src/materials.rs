//! `build-bot materials`: what a blueprint costs in items, stacks and shulker boxes.

use crate::blocks::item_name;
use crate::schematic::{Block, Schematic};
use std::collections::BTreeMap;

pub const SHULKER_SLOTS: usize = 27;

/// Largest stack of an item (creative or survival).
pub fn stack_size(item: &str) -> usize {
    if item.ends_with("_bed") || item.ends_with("shulker_box") || item.ends_with("_boat") || item.ends_with("_minecart")
        || matches!(item, "cake" | "water_bucket" | "lava_bucket" | "powder_snow_bucket")
    {
        1
    } else if item.ends_with("_sign") || item.ends_with("_banner") || matches!(item, "bucket" | "snowball" | "ender_pearl" | "egg") {
        16
    } else {
        64
    }
}

/// Items that stand in for one block, with how many of each. Parts the server adds by itself
/// (upper door half, bed head, tall plant top) cost nothing; a double slab costs two.
pub fn items_for(b: &Block) -> Vec<(String, usize)> {
    let name = b.name.as_str();
    let count = |k: &str| b.prop(k).and_then(|v| v.parse().ok()).unwrap_or(1);
    let upper = b.prop("half") == Some("upper") && (name.ends_with("_door") || b.prop("part").is_none() && is_tall_plant(name));
    if upper || (name.ends_with("_bed") && b.prop("part") == Some("head")) {
        return Vec::new();
    }
    let n = match name {
        "water" | "lava" | "bubble_column" | "fire" | "soul_fire" | "moving_piston" | "piston_head" | "nether_portal" | "end_portal" | "end_gateway" => return Vec::new(),
        _ if name.ends_with("_slab") && b.prop("type") == Some("double") => 2,
        _ if name.ends_with("candle") => count("candles"),
        "sea_pickle" => count("pickles"),
        "turtle_egg" => count("eggs"),
        "snow" => count("layers"),
        "pink_petals" => count("flower_amount"),
        _ => 1,
    };
    if let Some(plant) = name.strip_prefix("potted_") {
        return vec![("flower_pot".into(), 1), (plant.into(), 1)];
    }
    vec![(item_name(name).into_owned(), n)]
}

fn is_tall_plant(n: &str) -> bool {
    matches!(n, "tall_grass" | "large_fern" | "sunflower" | "lilac" | "rose_bush" | "peony" | "tall_seagrass")
}

#[derive(Debug, PartialEq)]
pub struct Line {
    pub item: String,
    pub count: usize,
    pub stack: usize,
}

impl Line {
    pub fn stacks(&self) -> usize {
        self.count.div_ceil(self.stack)
    }
}

/// Items needed, most numerous first.
pub fn list(s: &Schematic) -> Vec<Line> {
    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    for b in &s.blocks {
        for (item, n) in items_for(b) {
            *total.entry(item).or_default() += n;
        }
    }
    let mut out: Vec<Line> = total.into_iter().map(|(item, count)| Line { stack: stack_size(&item), item, count }).collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.item.cmp(&b.item)));
    out
}

/// Inventory slots in total and shulker boxes that hold them.
pub fn totals(lines: &[Line]) -> (usize, usize) {
    let slots: usize = lines.iter().map(Line::stacks).sum();
    (slots, slots.div_ceil(SHULKER_SLOTS))
}

/// `3x64+12`, `64`, `12`.
pub fn stack_text(l: &Line) -> String {
    let (full, rest) = (l.count / l.stack, l.count % l.stack);
    match (full, rest) {
        (0, r) => r.to_string(),
        (f, 0) => format!("{f}x{}", l.stack),
        (f, r) => format!("{f}x{}+{r}", l.stack),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Schematic {
        Schematic::parse(text).unwrap()
    }

    #[test]
    fn counts_stacks_and_boxes() {
        let text = format!("palette S stone\nlayer 0\n{}\n", "S".repeat(150));
        let lines = list(&s(&text));
        assert_eq!((lines[0].item.as_str(), lines[0].count, lines[0].stacks()), ("stone", 150, 3));
        assert_eq!(stack_text(&lines[0]), "2x64+22");
        assert_eq!(totals(&lines), (3, 1));
        let many: Vec<Line> = (0..28).map(|i| Line { item: format!("i{i}"), count: 1, stack: 64 }).collect();
        assert_eq!(totals(&many), (28, 2));
    }

    #[test]
    fn odd_blocks_cost_what_they_cost() {
        let text = "palette D oak_door[half=lower]\npalette U oak_door[half=upper]\npalette B red_bed[part=foot]\npalette H red_bed[part=head]\n\
                    palette L stone_slab[type=double]\npalette T oak_wall_sign\npalette P potted_poppy\npalette C candle[candles=3]\npalette W water\nlayer 0\nDUBHLTPCW\n";
        let got: BTreeMap<String, usize> = list(&s(text)).into_iter().map(|l| (l.item, l.count)).collect();
        let want: BTreeMap<String, usize> = [("oak_door", 1), ("red_bed", 1), ("stone_slab", 2), ("oak_sign", 1), ("flower_pot", 1), ("poppy", 1), ("candle", 3)]
            .into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        assert_eq!(got, want);
        assert_eq!((stack_size("red_bed"), stack_size("oak_sign"), stack_size("stone")), (1, 16, 64));
    }
}
