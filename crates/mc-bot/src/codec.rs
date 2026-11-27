//! Packet bodies whose layout differs between releases (see [`crate::version::Layout`]), as pure
//! functions so every supported layout is unit-tested. Packet ids are in [`crate::data`].

use crate::version::Layout;
use crate::world::{BlockPos, Face, Vec3};
use mc_proto::buf::PacketBuf;

/// Serverbound client_information: without it the server assumes view distance 2.
pub fn client_information(layout: &Layout, view_distance: u8) -> PacketBuf {
    let mut b = PacketBuf::new();
    b.write_string("en_us");
    b.write_u8(view_distance);
    b.write_varint(0); // chat visibility: full
    b.write_bool(true); // chat colors
    b.write_u8(0x7f); // all skin parts
    b.write_varint(1); // main hand: right
    b.write_bool(false); // text filtering
    b.write_bool(true); // allows listing
    if layout.particle_status {
        b.write_varint(0); // particles: all
    }
    b
}

/// Serverbound use_item_on with the main hand. `cursor` is the offset inside the clicked block.
pub fn use_item_on(layout: &Layout, support: BlockPos, face: Face, cursor: (f32, f32, f32), sequence: i32) -> PacketBuf {
    let mut b = PacketBuf::new();
    b.write_varint(0); // hand = main
    b.write_position(support.x, support.y, support.z);
    b.write_varint(face as i32);
    b.write_f32(cursor.0);
    b.write_f32(cursor.1);
    b.write_f32(cursor.2);
    b.write_bool(false); // inside block
    if layout.world_border_hit {
        b.write_bool(false);
    }
    b.write_varint(sequence);
    b
}

/// Serverbound set_creative_mode_slot: one plain item without extra data.
pub fn creative_slot(layout: &Layout, slot: i16, item_id: i32) -> PacketBuf {
    let mut b = PacketBuf::new();
    b.write_i16(slot);
    if layout.nbt_item_stack {
        b.write_bool(true);
        b.write_varint(item_id);
        b.write_u8(1); // count
        b.write_u8(0); // no NBT (TAG_End)
    } else {
        b.write_varint(1); // count
        b.write_varint(item_id);
        b.write_varint(0); // added components
        b.write_varint(0); // removed components
    }
    b
}

/// The player_command actions the bot sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerAction {
    PressShift,
    ReleaseShift,
    StartSprinting,
    StopSprinting,
}

/// Serverbound player_command. Releases with `sneak_by_input` removed the two shift actions (the
/// following ordinals moved down by two); send [`shift_input`] instead of those there.
pub fn player_command(layout: &Layout, entity_id: i32, action: PlayerAction) -> PacketBuf {
    let ordinal = match action {
        PlayerAction::PressShift => 0,
        PlayerAction::ReleaseShift => 1,
        PlayerAction::StartSprinting if layout.sneak_by_input => 1,
        PlayerAction::StopSprinting if layout.sneak_by_input => 2,
        PlayerAction::StartSprinting => 3,
        PlayerAction::StopSprinting => 4,
    };
    let mut b = PacketBuf::new();
    b.write_varint(entity_id);
    b.write_varint(ordinal);
    b.write_varint(0); // data
    b
}

/// Serverbound player_input as one flag byte (26.3): only the shift bit.
pub fn shift_input(shift: bool) -> PacketBuf {
    let mut b = PacketBuf::new();
    b.write_u8(if shift { 0x20 } else { 0 });
    b
}

/// A clientbound player_position packet. `flags`: X 0x01, Y 0x02, Z 0x04, yaw 0x08, pitch 0x10.
#[derive(Debug, PartialEq)]
pub struct Teleport {
    pub id: i32,
    pub pos: (f64, f64, f64),
    pub yaw: f32,
    pub pitch: f32,
    pub flags: u8,
}

/// None for a truncated body (confirming a made-up teleport would desync or disconnect us).
pub fn read_teleport(layout: &Layout, body: &mut PacketBuf) -> Option<Teleport> {
    let newer = layout.teleport_with_velocity;
    let id = if newer { body.read_varint().ok()? } else { 0 };
    let pos = (body.read_f64().ok()?, body.read_f64().ok()?, body.read_f64().ok()?);
    if newer {
        // velocity: the bot does not simulate momentum
        for _ in 0..3 {
            body.read_f64().ok()?;
        }
    }
    let (yaw, pitch) = (body.read_f32().ok()?, body.read_f32().ok()?);
    if newer {
        // int bit set; the velocity bits above 0x1F are irrelevant here
        let flags = (body.read_i32().ok()? & 0x1f) as u8;
        return Some(Teleport { id, pos, yaw, pitch, flags });
    }
    let flags = body.read_u8().ok()?;
    let id = body.read_varint().ok()?;
    Some(Teleport { id, pos, yaw, pitch, flags })
}

/// Serverbound accept_teleportation; `at` is our position and rotation after applying it.
pub fn accept_teleport(layout: &Layout, id: i32, at: (Vec3, f32, f32)) -> PacketBuf {
    let mut b = PacketBuf::new();
    b.write_varint(id);
    if layout.accept_has_position {
        let (p, yaw, pitch) = at;
        b.write_f64(p.x);
        b.write_f64(p.y);
        b.write_f64(p.z);
        b.write_f32(yaw);
        b.write_f32(pitch);
    }
    b
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
