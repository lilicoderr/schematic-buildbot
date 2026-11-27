//! Protocol flow: handshake, status ping, (offline) login and configuration → play.

use crate::codec;
use crate::net::Connection;
use crate::tr;
use crate::version;
use mc_proto::buf::PacketBuf;
use mc_proto::State;
use std::io;

/// Send the handshake. `next_state`: 1 = Status, 2 = Login.
pub fn handshake(conn: &mut Connection, host: &str, port: u16, protocol: i32, next_state: i32) -> io::Result<()> {
    let mut b = PacketBuf::new();
    b.write_varint(protocol);
    b.write_string(host);
    b.write_u16(port);
    b.write_varint(next_state);
    conn.send(version::active().packets().sb_handshake_intention, b.as_slice())
}

/// Server-list ping (status). Returns the JSON string.
pub fn status_ping(addr: &str, host: &str, port: u16) -> io::Result<String> {
    let mut conn = Connection::connect(addr)?;
    let ver = version::active();
    handshake(&mut conn, host, port, ver.protocol, 1)?;
    conn.state = State::Status;
    conn.send(ver.packets().sb_status_status_request, &[])?;
    loop {
        let (id, mut body) = conn.recv()?;
        if id == ver.packets().cb_status_status_response {
            return body
                .read_string()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()));
        }
    }
}

/// Login result.
pub struct JoinInfo {
    pub uuid: u128,
    pub username: String,
    pub entity_id: i32,
}

/// Offline-mode login → Configuration → Play.
/// Only works on `online-mode=false` servers (no encryption/authentication).
///
/// `on_packet` observes every packet that passes by (for packet analysis).
pub fn login_offline(
    addr: &str,
    host: &str,
    port: u16,
    username: &str,
    on_packet: impl FnMut(State, i32, &PacketBuf),
) -> io::Result<(Connection, JoinInfo)> {
    login_offline_v(addr, host, port, username, version::active().protocol, on_packet)
}

/// Like [`login_offline`] for an explicit protocol number. It must be one of the supported
/// releases, which becomes the active version (several names can share a protocol).
pub fn login_offline_v(
    addr: &str,
    host: &str,
    port: u16,
    username: &str,
    protocol: i32,
    mut on_packet: impl FnMut(State, i32, &PacketBuf),
) -> io::Result<(Connection, JoinInfo)> {
    if version::active().protocol != protocol {
        let v = version::by_protocol(protocol).ok_or_else(|| {
            io::Error::new(io::ErrorKind::Unsupported, tr!("unsupported protocol {}", "지원하지 않는 프로토콜 {}", protocol))
        })?;
        version::set_active(v);
    }
    let ver = version::active();
    let p = ver.packets();
    let mut conn = Connection::connect(addr)?;
    handshake(&mut conn, host, port, protocol, 2)?;
    conn.state = State::Login;

    // Login Start: name + the offline UUID
    let offline_uuid = offline_player_uuid(username);
    let mut ls = PacketBuf::new();
    ls.write_string(username);
    ls.write_uuid(offline_uuid);
    conn.send(p.sb_login_hello, ls.as_slice())?;

    let mut join = JoinInfo { uuid: offline_uuid, username: username.to_string(), entity_id: 0 };

    // ── Login state ──
    loop {
        let (id, mut body) = conn.recv()?;
        on_packet(State::Login, id, &body);
        match id {
            x if x == p.cb_login_login_compression => {
                let threshold = body.read_varint().unwrap_or(-1);
                conn.set_threshold(if threshold < 0 { None } else { Some(threshold as usize) });
            }
            x if x == p.cb_login_login_finished => {
                join.uuid = body.read_uuid().unwrap_or(offline_uuid);
                join.username = body.read_string().unwrap_or_else(|_| username.to_string());
                // Properties after this are ignored
                conn.send(p.sb_login_login_acknowledged, &[])?; // → Configuration
                conn.state = State::Configuration;
                let info = codec::client_information(ver.layout, VIEW_DISTANCE);
                conn.send(p.sb_config_client_information, info.as_slice())?;
                break;
            }
            x if x == p.cb_login_hello => {
                return Err(io::Error::other(tr!(
                    "the server requires encryption (online mode); use an online-mode=false server",
                    "서버가 암호화(온라인 모드)를 요구합니다. 테스트는 online-mode=false 서버로 하세요."
                )));
            }
            x if x == p.cb_login_login_disconnect => {
                let reason = body.read_string().unwrap_or_default();
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused, tr!("login refused: {}", "로그인 거부: {}", reason)));
            }
            _ => {}
        }
    }

    // ── Configuration state: answer keepalive/ping and wait for Finish ──
    loop {
        let (id, mut body) = conn.recv()?;
        on_packet(State::Configuration, id, &body);
        match id {
            x if x == p.cb_config_keep_alive => {
                let k = body.read_i64().unwrap_or(0);
                let mut r = PacketBuf::new();
                r.write_i64(k);
                conn.send(p.sb_config_keep_alive, r.as_slice())?;
            }
            x if x == p.cb_config_ping => {
                let ping = body.read_i32().unwrap_or(0);
                let mut r = PacketBuf::new();
                r.write_i32(ping);
                conn.send(p.sb_config_pong, r.as_slice())?;
            }
            x if x == p.cb_config_select_known_packs => {
                // Reply with an empty known-packs list (accepted by vanilla servers)
                let mut r = PacketBuf::new();
                r.write_varint(0);
                conn.send(p.sb_config_select_known_packs, r.as_slice())?;
            }
            x if x == p.cb_config_finish_configuration => {
                conn.send(p.sb_config_finish_configuration, &[])?;
                conn.state = State::Play;
                break;
            }
            x if x == p.cb_config_disconnect => {
                let reason = crate::text::disconnect_reason(&mut body);
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused, tr!("configuration refused: {}", "설정 단계 거부: {}", reason)));
            }
            _ => {}
        }
    }

    Ok((conn, join))
}

/// View distance the bot announces (chunks); the server clamps it to its own limit.
pub const VIEW_DISTANCE: u8 = 8;

/// Serverbound chunk_batch_received: the server sends the next chunk batch only after
/// this answer to every chunk_batch_finished.
pub fn chunk_batch_received(chunks_per_tick: f32) -> PacketBuf {
    let mut b = PacketBuf::new();
    b.write_f32(chunks_per_tick);
    b
}

/// Offline player UUID = MD5("OfflinePlayer:<name>") turned into a v3 UUID
/// (the same rule vanilla offline mode uses).
pub fn offline_player_uuid(username: &str) -> u128 {
    let mut bytes = md5(format!("OfflinePlayer:{username}").as_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x30; // version 3
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
    u128::from_be_bytes(bytes)
}

// Small MD5 implementation (offline UUIDs only; avoids a dependency)
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    let (mut a0, mut b0, mut c0, mut d0) =
        (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f
                .wrapping_add(a)
                .wrapping_add(K[i])
                .wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunk_batch_ack_is_one_float() {
        assert_eq!(chunk_batch_received(9.0).as_slice(), 9.0f32.to_be_bytes());
    }

    #[test]
    fn md5_known() {
        // RFC 1321 test vectors
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(&md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
    }
    #[test]
    fn offline_uuid_shape() {
        let u = offline_player_uuid("Notch").to_be_bytes();
        assert_eq!(u[6] & 0xf0, 0x30, "version 3");
        assert_eq!(u[8] & 0xc0, 0x80, "variant");
    }
    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
