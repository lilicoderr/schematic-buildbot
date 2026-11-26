//! 프로토콜 흐름: 핸드셰이크 · 상태 핑 · (오프라인) 로그인/설정 → 플레이.

use crate::ids;
use crate::net::Connection;
use mc_proto::buf::PacketBuf;
use mc_proto::{State, PROTOCOL_VERSION};
use std::io;

/// 핸드셰이크 전송. next_state: 1=Status, 2=Login.
pub fn handshake(conn: &mut Connection, host: &str, port: u16, next_state: i32) -> io::Result<()> {
    let mut b = PacketBuf::new();
    b.write_varint(PROTOCOL_VERSION);
    b.write_string(host);
    b.write_u16(port);
    b.write_varint(next_state);
    conn.send(ids::SB_HANDSHAKE, b.as_slice())
}

/// 서버 목록 핑(status). JSON 문자열을 돌려준다.
pub fn status_ping(addr: &str, host: &str, port: u16) -> io::Result<String> {
    let mut conn = Connection::connect(addr)?;
    handshake(&mut conn, host, port, 1)?;
    conn.state = State::Status;
    conn.send(ids::SB_STATUS_REQUEST, &[])?;
    loop {
        let (id, mut body) = conn.recv()?;
        if id == ids::CB_STATUS_RESPONSE {
            return body
                .read_string()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()));
        }
    }
}

/// 로그인 결과.
pub struct JoinInfo {
    pub uuid: u128,
    pub username: String,
    pub entity_id: i32,
}

/// 오프라인 모드 로그인 → Configuration 통과 → Play 진입.
/// online-mode=false 서버에서만 동작 (암호화/인증 없음).
///
/// `on_packet` 으로 지나가는 모든 수신 패킷을 관찰할 수 있다(패킷 분석용).
pub fn login_offline(
    addr: &str,
    host: &str,
    port: u16,
    username: &str,
    mut on_packet: impl FnMut(State, i32, &PacketBuf),
) -> io::Result<(Connection, JoinInfo)> {
    let mut conn = Connection::connect(addr)?;
    handshake(&mut conn, host, port, 2)?;
    conn.state = State::Login;

    // Login Start: 이름 + (오프라인용) 임의 UUID
    let offline_uuid = offline_player_uuid(username);
    let mut ls = PacketBuf::new();
    ls.write_string(username);
    ls.write_uuid(offline_uuid);
    conn.send(ids::SB_LOGIN_START, ls.as_slice())?;

    let mut join = JoinInfo { uuid: offline_uuid, username: username.to_string(), entity_id: 0 };

    // ── Login 상태 처리 ──
    loop {
        let (id, mut body) = conn.recv()?;
        on_packet(State::Login, id, &body);
        match id {
            ids::CB_LOGIN_SET_COMPRESSION => {
                let threshold = body.read_varint().unwrap_or(-1);
                conn.set_threshold(if threshold < 0 { None } else { Some(threshold as usize) });
            }
            ids::CB_LOGIN_SUCCESS => {
                join.uuid = body.read_uuid().unwrap_or(offline_uuid);
                join.username = body.read_string().unwrap_or_else(|_| username.to_string());
                // 이후 properties 등은 무시
                conn.send(ids::SB_LOGIN_ACK, &[])?; // Login Acknowledged → Configuration
                conn.state = State::Configuration;
                break;
            }
            ids::CB_LOGIN_ENCRYPTION_REQUEST => {
                return Err(io::Error::other(
                    "서버가 암호화(온라인 모드)를 요구합니다. 테스트는 online-mode=false 서버로 하세요.",
                ));
            }
            ids::CB_LOGIN_DISCONNECT => {
                let reason = body.read_string().unwrap_or_default();
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused, format!("로그인 거부: {reason}")));
            }
            _ => {}
        }
    }

    // ── Configuration 상태: keepalive/ping 에 응답하고 Finish 를 기다림 ──
    loop {
        let (id, mut body) = conn.recv()?;
        on_packet(State::Configuration, id, &body);
        match id {
            ids::CB_CONFIG_KEEPALIVE => {
                let k = body.read_i64().unwrap_or(0);
                let mut r = PacketBuf::new();
                r.write_i64(k);
                conn.send(ids::SB_CONFIG_KEEPALIVE, r.as_slice())?;
            }
            ids::CB_CONFIG_PING => {
                let p = body.read_i32().unwrap_or(0);
                let mut r = PacketBuf::new();
                r.write_i32(p);
                conn.send(ids::SB_CONFIG_PONG, r.as_slice())?;
            }
            ids::CB_CONFIG_KNOWN_PACKS => {
                // 서버가 보낸 데이터팩 목록을 그대로 되돌려줌 (빈 목록으로도 대개 통과)
                let mut r = PacketBuf::new();
                r.write_varint(0);
                conn.send(ids::SB_CONFIG_KNOWN_PACKS, r.as_slice())?;
            }
            ids::CB_CONFIG_FINISH => {
                conn.send(ids::SB_CONFIG_FINISH_ACK, &[])?;
                conn.state = State::Play;
                break;
            }
            ids::CB_CONFIG_DISCONNECT => {
                let reason = body.read_string().unwrap_or_default();
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused, format!("설정 단계 거부: {reason}")));
            }
            _ => {}
        }
    }

    Ok((conn, join))
}

/// 오프라인 플레이어 UUID = MD5("OfflinePlayer:<name>") 를 v3 로 변형.
/// (바닐라 오프라인 모드와 동일한 규칙)
pub fn offline_player_uuid(username: &str) -> u128 {
    let mut bytes = md5(format!("OfflinePlayer:{username}").as_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x30; // version 3
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
    u128::from_be_bytes(bytes)
}

// 작은 MD5 구현 (오프라인 UUID 전용, 의존성 회피)
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
    fn md5_known() {
        // RFC1321 테스트 벡터
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
