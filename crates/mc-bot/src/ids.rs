//! Packet names for debug output. The ids themselves are per version, see [`crate::data`].

use crate::version;
use mc_proto::State;

/// State + direction + id → short name in the active version (known packets only).
pub fn name(state: State, clientbound: bool, id: i32) -> &'static str {
    use State::*;
    if !clientbound {
        return "?";
    }
    let p = version::active().packets();
    let table: &[(i32, &'static str)] = match state {
        Status => &[(p.cb_status_status_response, "StatusResponse")],
        Login => &[
            (p.cb_login_login_disconnect, "LoginDisconnect"),
            (p.cb_login_hello, "EncryptionRequest"),
            (p.cb_login_login_finished, "LoginSuccess"),
            (p.cb_login_login_compression, "SetCompression"),
        ],
        Configuration => &[
            (p.cb_config_disconnect, "Config/Disconnect"),
            (p.cb_config_finish_configuration, "Config/Finish"),
            (p.cb_config_keep_alive, "Config/KeepAlive"),
            (p.cb_config_ping, "Config/Ping"),
            (p.cb_config_select_known_packs, "Config/KnownPacks"),
        ],
        Play => &[
            (p.cb_play_keep_alive, "KeepAlive"),
            (p.cb_play_login, "Login"),
            (p.cb_play_player_position, "SyncPosition"),
            (p.cb_play_ping, "Ping"),
            (p.cb_play_disconnect, "Disconnect"),
            (p.cb_play_level_chunk_with_light, "ChunkData"),
            (p.cb_play_block_update, "BlockChange"),
            (p.cb_play_section_blocks_update, "MultiBlockChange"),
            (p.cb_play_set_health, "UpdateHealth"),
        ],
        Handshaking => &[],
    };
    table.iter().find(|e| e.0 == id).map_or("other", |e| e.1)
}
