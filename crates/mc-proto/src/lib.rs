//! mc-proto: Minecraft 1.21.1 protocol basics (hand-implemented).
//!
//! · [`buf`]   wire-type reading/writing (VarInt, strings, Position, ...)
//! · [`frame`] length-prefix + zlib compression framing
//!
//! Higher-level logic (handshake/login/play, packet analysis, the bot) lives in the mc-bot crate.

pub mod buf;
pub mod frame;

pub use buf::{PacketBuf, ProtoError};

/// Connection state. 1.21.1 has a Configuration phase between login and play.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Handshaking,
    Status,
    Login,
    Configuration,
    Play,
}

/// Protocol version number of 1.21.1.
pub const PROTOCOL_VERSION: i32 = 767;

#[cfg(test)]
mod tests;
