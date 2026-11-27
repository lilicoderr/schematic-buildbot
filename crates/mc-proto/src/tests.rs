use crate::buf::PacketBuf;
use crate::frame;
use std::io::Cursor;

// Official VarInt test vectors from the protocol wiki
#[test]
fn varint_known_vectors() {
    let cases: &[(i32, &[u8])] = &[
        (0, &[0x00]),
        (1, &[0x01]),
        (2, &[0x02]),
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (255, &[0xff, 0x01]),
        (25565, &[0xdd, 0xc7, 0x01]),
        (2097151, &[0xff, 0xff, 0x7f]),
        (2147483647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
        (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
        (-2147483648, &[0x80, 0x80, 0x80, 0x80, 0x08]),
    ];
    for (value, bytes) in cases {
        let mut buf = PacketBuf::new();
        buf.write_varint(*value);
        assert_eq!(buf.as_slice(), *bytes, "encode {value}");
        assert_eq!(PacketBuf::varint_len(*value), bytes.len(), "len {value}");
        let mut rb = PacketBuf::from_vec(bytes.to_vec());
        assert_eq!(rb.read_varint().unwrap(), *value, "decode {value}");
    }
}

#[test]
fn varlong_roundtrip() {
    for v in [0i64, 1, -1, 127, 128, i64::MAX, i64::MIN, 9223372036854775807] {
        let mut buf = PacketBuf::new();
        buf.write_varlong(v);
        let mut rb = PacketBuf::from_vec(buf.into_vec());
        assert_eq!(rb.read_varlong().unwrap(), v);
    }
}

#[test]
fn fixed_width_and_string() {
    let mut b = PacketBuf::new();
    b.write_u16(0xBEEF);
    b.write_i32(-123456);
    b.write_i64(i64::MIN);
    b.write_f64(1234.5678);
    b.write_bool(true);
    b.write_string("안녕 minecraft");
    let mut r = PacketBuf::from_vec(b.into_vec());
    assert_eq!(r.read_u16().unwrap(), 0xBEEF);
    assert_eq!(r.read_i32().unwrap(), -123456);
    assert_eq!(r.read_i64().unwrap(), i64::MIN);
    assert!((r.read_f64().unwrap() - 1234.5678).abs() < 1e-9);
    assert!(r.read_bool().unwrap());
    assert_eq!(r.read_string().unwrap(), "안녕 minecraft");
    assert_eq!(r.remaining(), 0);
}

#[test]
fn position_pack_roundtrip() {
    // includes the wiki example + negative coordinates
    let cases = [
        (0i32, 0i32, 0i32),
        (1, 2, 3),
        (-1, -1, -1),
        (18357644, 831, -20882616), // wiki example
        (-33554432, -2048, 33554431), // boundary values
    ];
    for (x, y, z) in cases {
        let mut b = PacketBuf::new();
        b.write_position(x, y, z);
        let mut r = PacketBuf::from_vec(b.into_vec());
        assert_eq!(r.read_position().unwrap(), (x, y, z), "pos {x},{y},{z}");
    }
}

#[test]
fn uuid_roundtrip() {
    let id = 0x1234_5678_9abc_def0_0fed_cba9_8765_4321u128;
    let mut b = PacketBuf::new();
    b.write_uuid(id);
    let mut r = PacketBuf::from_vec(b.into_vec());
    assert_eq!(r.read_uuid().unwrap(), id);
}

#[test]
fn eof_is_error() {
    let mut r = PacketBuf::from_vec(vec![0x01]);
    assert!(r.read_i32().is_err());
}

// ── Framing ──
#[test]
fn frame_uncompressed_roundtrip() {
    let body = b"hello-body";
    let wire = frame::encode(0x2A, body, None);
    let mut cur = Cursor::new(wire);
    let (id, mut pb) = frame::read_frame(&mut cur, None).unwrap();
    assert_eq!(id, 0x2A);
    assert_eq!(pb.read_rest(), body);
}

#[test]
fn frame_compressed_small_below_threshold() {
    // below the threshold it is sent with dataLen=0 (uncompressed)
    let body = b"tiny";
    let wire = frame::encode(0x01, body, Some(256));
    let mut cur = Cursor::new(wire);
    let (id, mut pb) = frame::read_frame(&mut cur, Some(256)).unwrap();
    assert_eq!(id, 0x01);
    assert_eq!(pb.read_rest(), body);
}

#[test]
fn frame_compressed_large_roundtrip() {
    // at or above the threshold it is zlib-compressed → decompress round trip
    let body = vec![0xABu8; 5000];
    let wire = frame::encode(0x33, &body, Some(256));
    assert!(wire.len() < body.len(), "compression should shrink it");
    let mut cur = Cursor::new(wire);
    let (id, mut pb) = frame::read_frame(&mut cur, Some(256)).unwrap();
    assert_eq!(id, 0x33);
    assert_eq!(pb.read_rest(), body);
}

#[test]
fn multiple_frames_in_stream() {
    let mut stream = Vec::new();
    stream.extend(frame::encode(1, b"aaa", None));
    stream.extend(frame::encode(2, b"bbbb", None));
    let mut cur = Cursor::new(stream);
    let (id1, mut p1) = frame::read_frame(&mut cur, None).unwrap();
    let (id2, mut p2) = frame::read_frame(&mut cur, None).unwrap();
    assert_eq!((id1, p1.read_rest()), (1, b"aaa".to_vec()));
    assert_eq!((id2, p2.read_rest()), (2, b"bbbb".to_vec()));
}

#[test]
fn partial_frame_waits_then_parses() {
    let body: Vec<u8> = (0..200u8).collect();
    for threshold in [None, Some(64)] {
        let wire = frame::encode(0x27, &body, threshold);
        // Every strict prefix (a read timeout mid-frame) must report "incomplete", never an error.
        for cut in 0..wire.len() {
            assert!(frame::try_parse_frame(&wire[..cut], threshold).unwrap().is_none(), "cut {cut}");
        }
        let (used, id, mut pb) = frame::try_parse_frame(&wire, threshold).unwrap().unwrap();
        assert_eq!((used, id), (wire.len(), 0x27));
        assert_eq!(pb.read_rest(), body);
    }
}

#[test]
fn back_to_back_frames_are_split_correctly() {
    let mut wire = frame::encode(1, b"abc", None);
    wire.extend(frame::encode(2, b"defg", None));
    let (used, id, _) = frame::try_parse_frame(&wire, None).unwrap().unwrap();
    assert_eq!(id, 1);
    let (_, id2, mut pb) = frame::try_parse_frame(&wire[used..], None).unwrap().unwrap();
    assert_eq!(id2, 2);
    assert_eq!(pb.read_rest(), b"defg");
}
