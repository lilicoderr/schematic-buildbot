//! Expected bytes per supported release, spelled out by hand from the layouts in docs/RE/versions.md.

use super::*;
use crate::version::lookup;

fn layout(name: &str) -> &'static Layout {
    lookup(name).unwrap().layout
}

const ALL: [&str; 4] = ["1.20.4", "1.21.1", "1.21.4", "26.3"];

#[test]
fn client_information_per_version() {
    let base = vec![5, b'e', b'n', b'_', b'u', b's', 8, 0, 1, 0x7f, 1, 0, 1];
    for name in ALL {
        let mut want = base.clone();
        if name == "1.21.4" || name == "26.3" {
            want.push(0);
        }
        assert_eq!(client_information(layout(name), 8).as_slice(), want, "{name}");
    }
}

#[test]
fn use_item_on_per_version() {
    for (name, bools) in [("1.20.4", 1), ("1.21.1", 1), ("1.21.4", 2), ("26.3", 2)] {
        let b = use_item_on(layout(name), BlockPos::new(1, 2, 3), Face::Up, (0.5, 1.0, 0.5), 7);
        // hand 1 + position 8 + face 1 + cursor 12 + bools + sequence 1
        assert_eq!(b.as_slice().len(), 23 + bools, "{name}");
        assert_eq!(&b.as_slice()[b.as_slice().len() - 1 - bools..], &[vec![0; bools], vec![7]].concat(), "{name}");
    }
}

#[test]
fn creative_slot_per_version() {
    assert_eq!(creative_slot(layout("1.20.4"), 36, 35).as_slice(), [0, 36, 1, 35, 1, 0]);
    for name in ["1.21.1", "1.21.4", "26.3"] {
        assert_eq!(creative_slot(layout(name), 36, 35).as_slice(), [0, 36, 1, 35, 0, 0], "{name}");
    }
}

fn teleport_body(newer: bool) -> Vec<u8> {
    let mut b = PacketBuf::new();
    if newer {
        b.write_varint(9);
    }
    for v in [1.5f64, -60.0, 2.5] {
        b.write_f64(v);
    }
    if newer {
        for _ in 0..3 {
            b.write_f64(0.25);
        }
    }
    b.write_f32(90.0);
    b.write_f32(10.0);
    if newer {
        b.write_i32(0x108); // relative yaw plus a velocity bit
    } else {
        b.write_u8(0x08);
        b.write_varint(9);
    }
    b.into_vec()
}

#[test]
fn teleport_per_version() {
    let want = Teleport { id: 9, pos: (1.5, -60.0, 2.5), yaw: 90.0, pitch: 10.0, flags: 0x08 };
    for name in ALL {
        let l = layout(name);
        let mut body = PacketBuf::from_vec(teleport_body(l.teleport_with_velocity));
        assert_eq!(read_teleport(l, &mut body).as_ref(), Some(&want), "{name}");
        assert_eq!(body.remaining(), 0, "{name}");
        let mut short = teleport_body(l.teleport_with_velocity);
        short.pop();
        assert_eq!(read_teleport(l, &mut PacketBuf::from_vec(short)), None, "{name}");
    }
}

#[test]
fn accept_teleport_carries_the_position_only_on_26_3() {
    let at = (Vec3::new(1.0, 2.0, 3.0), 4.0, 5.0);
    for name in ["1.20.4", "1.21.1", "1.21.4"] {
        assert_eq!(accept_teleport(layout(name), 9, at).as_slice(), [9], "{name}");
    }
    let b = accept_teleport(layout("26.3"), 9, at);
    assert_eq!(b.as_slice().len(), 1 + 3 * 8 + 2 * 4);
    assert_eq!(&b.as_slice()[1..9], 1.0f64.to_be_bytes());
    assert_eq!(&b.as_slice()[29..], 5.0f32.to_be_bytes());
}

#[test]
fn sprint_and_sneak_per_version() {
    use PlayerAction::*;
    for name in ["1.20.4", "1.21.1", "1.21.4"] {
        let l = layout(name);
        assert!(!l.sneak_by_input, "{name}");
        for (action, ordinal) in [(PressShift, 0), (ReleaseShift, 1), (StartSprinting, 3), (StopSprinting, 4)] {
            assert_eq!(player_command(l, 7, action).as_slice(), [7, ordinal, 0], "{name} {action:?}");
        }
    }
    let l = layout("26.3");
    assert!(l.sneak_by_input);
    assert_eq!(player_command(l, 7, StartSprinting).as_slice(), [7, 1, 0]);
    assert_eq!(player_command(l, 7, StopSprinting).as_slice(), [7, 2, 0]);
    assert_eq!((shift_input(true).as_slice(), shift_input(false).as_slice()), (&[0x20u8][..], &[0u8][..]));
}
