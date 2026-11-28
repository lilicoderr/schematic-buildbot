//! Minecraft version table.
//!
//! A version is usable only when the bot ships generated tables ([`crate::data`]) and its packet
//! layouts were compared with that release (docs/RE/versions.md). Other names are rejected with an
//! explanation instead of failing later with a confusing disconnect.

use crate::data::{self, Packets, Tables};
use crate::tr;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Packet layout differences between the supported releases (docs/RE/versions.md).
#[derive(Debug, PartialEq, Eq)]
pub struct Layout {
    /// client_information ends with a particle status VarInt.
    pub particle_status: bool,
    /// use_item_on carries a world-border-hit bool after inside-block.
    pub world_border_hit: bool,
    /// player_position is id, position, velocity, yaw, pitch, int flags.
    pub teleport_with_velocity: bool,
    /// accept_teleportation also carries the resulting position and rotation.
    pub accept_has_position: bool,
    /// Item stacks are bool, id, count byte, NBT instead of count, id, components.
    pub nbt_item_stack: bool,
    /// Sneaking is the shift bit of player_input; player_command lost its two shift actions.
    pub sneak_by_input: bool,
    /// Chunk data: heightmaps are a binary map, sections carry a fluid count and paletted
    /// containers have no long-array length.
    pub binary_chunks: bool,
}

const V1_20_4: Layout = Layout {
    particle_status: false,
    world_border_hit: false,
    teleport_with_velocity: false,
    accept_has_position: false,
    nbt_item_stack: true,
    sneak_by_input: false,
    binary_chunks: false,
};
const V1_21: Layout = Layout { nbt_item_stack: false, ..V1_20_4 };
const V1_21_4: Layout = Layout { particle_status: true, world_border_hit: true, teleport_with_velocity: true, ..V1_21 };
const V26_3: Layout = Layout { accept_has_position: true, sneak_by_input: true, binary_chunks: true, ..V1_21_4 };

#[derive(Debug)]
pub struct Version {
    pub name: &'static str,
    pub protocol: i32,
    pub tables: &'static Tables,
    pub layout: &'static Layout,
}

impl Version {
    pub fn packets(&self) -> &'static Packets {
        &self.tables.packets
    }
}

pub static SUPPORTED: [Version; 5] = [
    Version { name: "1.20.4", protocol: 765, tables: data::V765, layout: &V1_20_4 },
    Version { name: "1.21", protocol: 767, tables: data::V767, layout: &V1_21 },
    Version { name: "1.21.1", protocol: 767, tables: data::V767, layout: &V1_21 },
    Version { name: "1.21.4", protocol: 769, tables: data::V769, layout: &V1_21_4 },
    Version { name: "26.3", protocol: 777, tables: data::V777, layout: &V26_3 },
];

const DEFAULT_INDEX: usize = 2;
pub static DEFAULT: &Version = &SUPPORTED[DEFAULT_INDEX];

/// Releases a user may ask for that have no tables.
const KNOWN_UNSUPPORTED: &[&str] = &["1.20.2", "1.20.6", "1.21.3", "1.21.5", "1.21.8", "1.21.11", "26.1", "26.2"];

static ACTIVE: AtomicUsize = AtomicUsize::new(DEFAULT_INDEX);

/// The version of this process (chosen by `--version` or a plan header, applied before joining).
pub fn active() -> &'static Version {
    &SUPPORTED[ACTIVE.load(Ordering::Relaxed)]
}

pub fn set_active(v: &Version) {
    if let Some(i) = SUPPORTED.iter().position(|s| s.name == v.name) {
        ACTIVE.store(i, Ordering::Relaxed);
    }
}

/// The newest supported version speaking this protocol number.
pub fn by_protocol(protocol: i32) -> Option<&'static Version> {
    SUPPORTED.iter().rev().find(|v| v.protocol == protocol)
}

pub fn lookup(name: &str) -> Result<&'static Version, String> {
    let name = name.trim().trim_start_matches('v');
    if let Some(v) = SUPPORTED.iter().find(|v| v.name == name) {
        return Ok(v);
    }
    if KNOWN_UNSUPPORTED.contains(&name) {
        return Err(tr!(
            "Minecraft {} is known but has no packet tables yet. Supported: {}. Other versions can be reached through ViaProxy, see the README.",
            "Minecraft {}은 알려져 있지만 패킷 표가 아직 없습니다. 지원: {}. ViaProxy 경유는 README 참고.",
            name,
            supported_names()
        ));
    }
    Err(tr!("unknown Minecraft version '{}'. Supported: {}", "알 수 없는 마인크래프트 버전 '{}'. 지원: {}", name, supported_names()))
}

pub fn supported_names() -> String {
    SUPPORTED.iter().map(|v| v.name).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
#[path = "version_tests.rs"]
mod tests;
