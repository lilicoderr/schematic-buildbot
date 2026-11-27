//! The 1.21.1 (protocol 767) packet ids this bot uses.
//!
//! sb_* = serverbound (client → server), cb_* = clientbound (server → client).
//! Each connection state has its own id space.

// ── Handshaking ──
pub const SB_HANDSHAKE: i32 = 0x00;

// ── Status ──
pub const SB_STATUS_REQUEST: i32 = 0x00;
pub const SB_STATUS_PING: i32 = 0x01;
pub const CB_STATUS_RESPONSE: i32 = 0x00;
pub const CB_STATUS_PONG: i32 = 0x01;

// ── Login ──
pub const SB_LOGIN_START: i32 = 0x00;
pub const SB_LOGIN_ACK: i32 = 0x03; // Login Acknowledged
pub const CB_LOGIN_DISCONNECT: i32 = 0x00;
pub const CB_LOGIN_ENCRYPTION_REQUEST: i32 = 0x01;
pub const CB_LOGIN_SUCCESS: i32 = 0x02;
pub const CB_LOGIN_SET_COMPRESSION: i32 = 0x03;

// ── Configuration (client side, 1.21.1) ──
pub const SB_CONFIG_CLIENT_INFO: i32 = 0x00;
pub const SB_CONFIG_FINISH_ACK: i32 = 0x03; // Acknowledge Finish Configuration
pub const SB_CONFIG_KEEPALIVE: i32 = 0x04;
pub const SB_CONFIG_PONG: i32 = 0x05;
pub const SB_CONFIG_KNOWN_PACKS: i32 = 0x07;
pub const CB_CONFIG_DISCONNECT: i32 = 0x02;
pub const CB_CONFIG_FINISH: i32 = 0x03; // Finish Configuration
pub const CB_CONFIG_KEEPALIVE: i32 = 0x04;
pub const CB_CONFIG_PING: i32 = 0x05;
pub const CB_CONFIG_KNOWN_PACKS: i32 = 0x0E;

// ── Play (serverbound) ──
pub const SB_PLAY_KEEPALIVE: i32 = 0x18;
pub const SB_PLAY_CHAT_MESSAGE: i32 = 0x06;
pub const SB_PLAY_CONFIRM_TELEPORT: i32 = 0x00;
pub const SB_PLAY_PLAYER_LOADED: i32 = 0x2A; // 1.21.x
pub const SB_PLAY_SET_POS: i32 = 0x1A; // Set Player Position
pub const SB_PLAY_SET_POS_ROT: i32 = 0x1B; // Set Player Position and Rotation
pub const SB_PLAY_SET_ROT: i32 = 0x1C;
pub const SB_PLAY_SWING_ARM: i32 = 0x36; // Swing Arm
pub const SB_PLAY_USE_ITEM_ON: i32 = 0x38; // Use Item On (place a block)
pub const SB_PLAY_HELD_ITEM: i32 = 0x2F; // Set Held Item
pub const SB_PLAY_CLICK_CONTAINER: i32 = 0x0E;
pub const SB_PLAY_CLOSE_CONTAINER: i32 = 0x0F;
pub const SB_PLAY_SET_CREATIVE_SLOT: i32 = 0x32;
pub const SB_PLAY_BLOCK_DIG: i32 = 0x24;
pub const SB_PLAY_USE_ITEM: i32 = 0x39;
pub const SB_PLAY_ABILITIES: i32 = 0x23; // flags bit 0x02 = flying

// ── Play (clientbound) ──  (1.21.1 / protocol 767, per minecraft-data)
pub const CB_PLAY_KEEPALIVE: i32 = 0x26;
pub const CB_PLAY_PING: i32 = 0x35; // transaction ping
pub const CB_PLAY_LOGIN: i32 = 0x2B; // Login (join game)
pub const CB_PLAY_SYNC_POS: i32 = 0x40; // Synchronize Player Position (teleport)
pub const CB_PLAY_DISCONNECT: i32 = 0x1D;
pub const CB_PLAY_CHUNK: i32 = 0x27; // Chunk Data and Light (map_chunk)
pub const CB_PLAY_BLOCK_CHANGE: i32 = 0x09;
pub const CB_PLAY_MULTI_BLOCK_CHANGE: i32 = 0x49; // section blocks update
pub const CB_PLAY_ABILITIES: i32 = 0x38; // flags: 0x02 flying, 0x04 may fly, 0x08 creative
pub const CB_PLAY_OPEN_SCREEN: i32 = 0x33;
pub const CB_PLAY_CONTAINER_CONTENT: i32 = 0x13; // window_items
pub const CB_PLAY_CONTAINER_SLOT: i32 = 0x15; // set_slot
pub const CB_PLAY_UPDATE_HEALTH: i32 = 0x5D;

/// For debug output: state + direction + id → name (known ones only).
pub fn name(state: mc_proto::State, clientbound: bool, id: i32) -> &'static str {
    use mc_proto::State::*;
    match (state, clientbound, id) {
        (Status, true, 0x00) => "StatusResponse",
        (Status, true, 0x01) => "Pong",
        (Login, true, 0x00) => "LoginDisconnect",
        (Login, true, 0x01) => "EncryptionRequest",
        (Login, true, 0x02) => "LoginSuccess",
        (Login, true, 0x03) => "SetCompression",
        (Configuration, true, 0x02) => "Config/Disconnect",
        (Configuration, true, 0x03) => "Config/Finish",
        (Configuration, true, 0x04) => "Config/KeepAlive",
        (Configuration, true, 0x05) => "Config/Ping",
        (Configuration, true, 0x0E) => "Config/KnownPacks",
        (Configuration, true, _) => "Config/other",
        (Play, true, 0x26) => "KeepAlive",
        (Play, true, 0x2B) => "Login",
        (Play, true, 0x40) => "SyncPosition",
        (Play, true, 0x35) => "Ping",
        (Play, true, 0x1D) => "Disconnect",
        (Play, true, 0x27) => "ChunkData",
        (Play, true, 0x09) => "BlockChange",
        (Play, true, 0x49) => "MultiBlockChange",
        (Play, true, 0x33) => "OpenScreen",
        (Play, true, 0x13) => "ContainerContent",
        (Play, true, 0x15) => "SetSlot",
        (Play, true, 0x56) => "SpawnPosition",
        (Play, true, 0x5D) => "UpdateHealth",
        (Play, true, _) => "other",
        _ => "?",
    }
}
