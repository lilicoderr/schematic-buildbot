//! The player's game mode, read from the play Login and Game Event packets. The bot takes blocks
//! out of the creative inventory, so anything else must be reported before building.

use mc_proto::buf::PacketBuf;

pub const CREATIVE: u8 = 1;
/// Game Event id "change game mode"; its float value is the new mode.
const CHANGE_GAME_MODE: u8 = 3;

/// Game mode from a play Login packet, after its leading entity id has been read.
/// `named_dimension_type`: releases up to 1.20.4 send the dimension type as a name, not a number.
pub fn from_login(body: &mut PacketBuf, named_dimension_type: bool) -> Option<u8> {
    body.read_u8().ok()?; // hardcore
    let levels = body.read_varint().ok()?;
    for _ in 0..levels.clamp(0, 1024) {
        body.read_string().ok()?;
    }
    for _ in 0..3 {
        body.read_varint().ok()?; // max players, view and simulation distance
    }
    for _ in 0..3 {
        body.read_u8().ok()?; // reduced debug info, death screen, limited crafting
    }
    if named_dimension_type {
        body.read_string().ok()?;
    } else {
        body.read_varint().ok()?;
    }
    body.read_string().ok()?; // dimension
    body.read_i64().ok()?; // hashed seed
    body.read_u8().ok().filter(|&m| m <= 3)
}

/// The new game mode if this Game Event packet changes it.
pub fn from_game_event(body: &mut PacketBuf) -> Option<u8> {
    if body.read_u8().ok()? != CHANGE_GAME_MODE {
        return None;
    }
    let mode = body.read_f32().ok()?;
    (0.0..=3.0).contains(&mode).then_some(mode as u8)
}

pub fn name(mode: u8) -> String {
    match mode {
        0 => crate::tr!("survival", "서바이벌"),
        1 => crate::tr!("creative", "크리에이티브"),
        2 => crate::tr!("adventure", "모험"),
        _ => crate::tr!("spectator", "관전"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(named: bool, mode: u8) -> PacketBuf {
        let mut b = PacketBuf::new();
        b.write_u8(0);
        b.write_varint(2);
        b.write_string("minecraft:overworld");
        b.write_string("minecraft:the_nether");
        for v in [20, 10, 10] {
            b.write_varint(v);
        }
        for _ in 0..3 {
            b.write_u8(0);
        }
        if named {
            b.write_string("minecraft:overworld");
        } else {
            b.write_varint(0);
        }
        b.write_string("minecraft:overworld");
        b.write_i64(-5);
        b.write_u8(mode);
        b.write_u8(0xff);
        b
    }

    #[test]
    fn reads_the_mode_in_both_login_layouts() {
        for named in [true, false] {
            let mut b = login(named, 2);
            assert_eq!(from_login(&mut b, named), Some(2), "named={named}");
        }
        assert_eq!(from_login(&mut PacketBuf::new(), false), None);
    }

    #[test]
    fn only_the_game_mode_event_counts() {
        let event = |id: u8, v: f32| {
            let mut b = PacketBuf::new();
            b.write_u8(id);
            b.write_f32(v);
            from_game_event(&mut b)
        };
        assert_eq!(event(3, 1.0), Some(CREATIVE));
        assert_eq!(event(3, 0.0), Some(0));
        assert_eq!(event(1, 1.0), None);
        assert_eq!(event(3, 9.0), None);
    }
}
