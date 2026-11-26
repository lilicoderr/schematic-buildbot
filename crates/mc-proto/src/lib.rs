//! mc-proto — Minecraft 1.21.1 프로토콜 기본기 (직접 구현).
//!
//! · [`buf`]   와이어 타입 읽기/쓰기 (VarInt, 문자열, Position 등)
//! · [`frame`] 길이 프리픽스 + zlib 압축 프레이밍
//!
//! 상위 로직(핸드셰이크/로그인/플레이, 패킷 분석, 봇)은 mc-bot 크레이트에 있다.

pub mod buf;
pub mod frame;

pub use buf::{PacketBuf, ProtoError};

/// 연결 상태. 1.21.1 은 로그인과 플레이 사이에 Configuration 단계가 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Handshaking,
    Status,
    Login,
    Configuration,
    Play,
}

/// 1.21.1 프로토콜 버전 번호.
pub const PROTOCOL_VERSION: i32 = 767;

#[cfg(test)]
mod tests;
