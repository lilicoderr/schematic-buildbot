//! The checker's own copy of the server rules (docs/RE/placement.md, collision.md). Deliberately
//! NOT shared with the planner (`nav`, `placement`, `world::within_reach`): a wrong rule there
//! must not be able to hide itself here. Only plain data types are imported.

use crate::nav::Cell;
use crate::world::{BlockPos, Face};
use std::collections::BTreeMap;

pub const EYE: f64 = 1.62;
/// Clicked (or broken) block: eye to its unit box.
pub const MAX_CLICK: f64 = 5.0;
/// Box of the cell that receives a block.
pub const MAX_TARGET: f64 = 4.5;

pub type Props = BTreeMap<String, String>;

/// Eye (player centred in feet cell `feet`) to the nearest point of the block's unit box.
pub fn eye_dist(feet: Cell, b: BlockPos) -> f64 {
    let eye = [feet.0 as f64 + 0.5, feet.1 as f64 + EYE, feet.2 as f64 + 0.5];
    let lo = [b.x as f64, b.y as f64, b.z as f64];
    (0..3).map(|i| (lo[i] - eye[i]).max(eye[i] - lo[i] - 1.0).max(0.0).powi(2)).sum::<f64>().sqrt()
}

/// The 0.6 x 1.8 box of a player centred in `feet` overlaps exactly the feet and head cells.
pub fn in_body(feet: Cell, c: Cell) -> bool {
    c.0 == feet.0 && c.2 == feet.2 && (c.1 == feet.1 || c.1 == feet.1 + 1)
}

/// Right clicks it takes to reach the wanted count (docs/RE/block_states.md: same item again).
pub fn clicks_needed(name: &str, props: &Props) -> usize {
    let count = |k: &str| props.get(k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 8);
    match name {
        "sea_pickle" => count("pickles"),
        "turtle_egg" => count("eggs"),
        "snow" => count("layers"),
        "pink_petals" => count("flower_amount"),
        n if n.ends_with("_slab") && props.get("type").is_some_and(|t| t == "double") => 2,
        n if n == "candle" || n.ends_with("_candle") => count("candles"),
        _ => 1,
    }
}

/// Blocks whose plain (not sneaking) right click may be consumed instead of placing
/// (every `useWithoutItem` / `useItemOn` override of the 1.21.1 server that matters here).
pub fn consumes_click(name: &str) -> bool {
    if matches!(name, "iron_door" | "iron_trapdoor") {
        return false;
    }
    const SUFFIX: [&str; 8] =
        ["_door", "trapdoor", "_fence_gate", "_button", "_bed", "shulker_box", "_sign", "anvil"];
    const EXACT: [&str; 40] = [
        "chest", "trapped_chest", "ender_chest", "barrel", "crafting_table", "furnace", "blast_furnace", "smoker",
        "note_block", "lectern", "lever", "enchanting_table", "jukebox", "hopper", "dispenser", "dropper",
        "brewing_stand", "beacon", "bell", "cake", "cartography_table", "chiseled_bookshelf", "command_block",
        "chain_command_block", "repeating_command_block", "comparator", "repeater", "composter", "crafter",
        "daylight_detector", "decorated_pot", "dragon_egg", "flower_pot", "grindstone", "loom", "redstone_wire",
        "respawn_anchor", "smithing_table", "stonecutter", "cauldron",
    ];
    SUFFIX.iter().any(|s| name.ends_with(s))
        || EXACT.contains(&name)
        || name.starts_with("potted_")
        || name.ends_with("_cauldron")
        || name.ends_with("candle_cake")
}

/// Does the top of this block hold a player at exactly the next integer height? Anything with no
/// collision (standing "on" it would hover) or a lower top (the bot would sink) does not.
pub fn full_top(name: &str, props: &Props) -> bool {
    if name.ends_with("_slab") {
        return props.get("type").map(String::as_str).is_some_and(|t| t != "bottom");
    }
    if matches!(name, "sea_lantern" | "jack_o_lantern") {
        return true;
    }
    const PART: [&str; 25] = [
        "_pane", "bars", "fence", "_wall", "carpet", "trapdoor", "_door", "ladder", "torch", "lantern", "button",
        "rail", "vine", "chain", "_sign", "pressure_plate", "sapling", "tulip", "candle", "_banner", "_skull",
        "_head", "_coral_fan", "amethyst_bud", "_bed",
    ];
    const EXACT: [&str; 62] = [
        "air", "cave_air", "water", "lava", "fire", "soul_fire", "redstone_wire", "tripwire", "tripwire_hook",
        "lever", "repeater", "comparator", "short_grass", "tall_grass", "fern", "large_fern", "dead_bush",
        "seagrass", "tall_seagrass", "kelp", "dandelion", "poppy", "blue_orchid", "allium", "azure_bluet",
        "oxeye_daisy", "cornflower", "lily_of_the_valley", "wither_rose", "sunflower", "lilac", "rose_bush",
        "peony", "brown_mushroom", "red_mushroom", "wheat", "carrots", "potatoes", "beetroots", "nether_wart",
        "sugar_cane", "bamboo", "cobweb", "snow", "soul_sand", "mud", "farmland", "dirt_path", "chest",
        "trapped_chest", "ender_chest", "cactus", "honey_block", "enchanting_table", "brewing_stand", "cake",
        "campfire", "soul_campfire", "daylight_detector", "stonecutter", "flower_pot", "end_rod",
    ];
    !(PART.iter().any(|p| name.contains(p)) || EXACT.contains(&name) || name.starts_with("potted_"))
}

fn behind(pos: BlockPos, props: &Props) -> Option<BlockPos> {
    let f = props.get("facing").and_then(|f| Face::from_name(f))?;
    Some(pos.offset(f.opposite()))
}

/// The cell a block cannot stay without (canSurvive and falling blocks).
pub fn needs_support(name: &str, props: &Props, pos: BlockPos) -> Option<BlockPos> {
    let wall = name.contains("wall_torch") || name.ends_with("wall_sign") || name == "ladder";
    if wall {
        return behind(pos, props);
    }
    if name.ends_with("lantern") && name != "sea_lantern" && name != "jack_o_lantern" {
        let up = props.get("hanging").is_some_and(|h| h == "true");
        return Some(pos.offset(if up { Face::Up } else { Face::Down }));
    }
    let falls = matches!(name, "sand" | "red_sand" | "gravel" | "dragon_egg")
        || name.ends_with("_concrete_powder")
        || name.ends_with("anvil");
    let sits = name.ends_with("torch")
        || name.ends_with("carpet")
        || name.ends_with("rail")
        || name.ends_with("pressure_plate")
        || name.ends_with("_sapling")
        || matches!(name, "redstone_wire" | "repeater" | "comparator")
        || (name.ends_with("_door") && props.get("half").is_none_or(|h| h != "upper"));
    (falls || sits).then(|| pos.below())
}

/// Where the second half appears when this block is placed, if it has one.
pub fn second_half(name: &str, props: &Props, pos: BlockPos) -> Option<BlockPos> {
    let tall = matches!(name, "tall_grass" | "large_fern" | "sunflower" | "lilac" | "rose_bush" | "peony");
    if name.ends_with("_door") || tall {
        return Some(pos.offset(Face::Up));
    }
    if name.ends_with("_bed") {
        let f = props.get("facing").and_then(|f| Face::from_name(f)).unwrap_or(Face::North);
        return Some(pos.offset(f));
    }
    None
}

fn horizontal(yaw: f32) -> &'static str {
    ["south", "west", "north", "east"][((yaw / 90.0 + 0.5).floor() as i64 & 3) as usize]
}

fn flip(d: &str) -> &'static str {
    match d {
        "south" => "north",
        "north" => "south",
        "west" => "east",
        _ => "west",
    }
}

/// Does the click really give the requested slab type / stair half / pillar axis / facing?
/// (docs/RE/block_states.md; only the properties a click decides are checked.)
pub fn derived_mismatch(name: &str, props: &Props, pos: BlockPos, support: BlockPos, face: Face, cursor_y: f32, yaw: f32) -> bool {
    let want = |k: &str| props.get(k).map(String::as_str);
    let hit_y = support.y as f64 + cursor_y as f64 - pos.y as f64;
    let top = face == Face::Down || (face != Face::Up && hit_y > 0.5);
    if name.ends_with("_slab") && want("type").is_some_and(|t| t != "double" && (t == "top") != top) {
        return true;
    }
    if name.ends_with("_stairs") && want("half").is_some_and(|h| (h == "top") != top) {
        return true;
    }
    if let Some(axis) = want("axis") {
        let got = match face {
            Face::Up | Face::Down => "y",
            Face::North | Face::South => "z",
            _ => "x",
        };
        if !name.contains("portal") && axis != got {
            return true;
        }
    }
    let Some(facing) = want("facing") else { return false };
    if name.ends_with("_stairs") || name.ends_with("_fence_gate") || name.ends_with("_bed") || name.ends_with("_door") {
        return facing != horizontal(yaw);
    }
    let opposite = matches!(name, "furnace" | "blast_furnace" | "smoker" | "chest" | "carved_pumpkin" | "jack_o_lantern" | "loom" | "stonecutter")
        || name.ends_with("_glazed_terracotta");
    opposite && facing != flip(horizontal(yaw))
}
