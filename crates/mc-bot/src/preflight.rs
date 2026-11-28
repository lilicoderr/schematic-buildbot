//! Checks before a build, with messages that say what to do: can the server be reached, which
//! release is it, does it accept an offline-mode bot, and is the bot in creative mode.

use crate::builder::PlayClient;
use crate::gamemode::{self, CREATIVE};
use crate::json::{self, Json};
use crate::version::{self, Version};
use crate::{proto, tr};
use std::io;
use std::time::{Duration, Instant};

/// `host:port` (port 25565 if left out). SRV records are not looked up.
pub fn split_address(addr: &str) -> Result<(String, u16), String> {
    let bad = || tr!("'{}' is not a server address; use host:port, e.g. 127.0.0.1:25565", "'{}' 는 서버 주소가 아닙니다. host:port 형식으로 입력하세요 (예: 127.0.0.1:25565)", addr);
    let (host, port) = match addr.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().map_err(|_| bad())?),
        None => (addr, 25565),
    };
    if host.is_empty() || host.contains(['/', ' ']) {
        return Err(bad());
    }
    Ok((host.to_string(), port))
}

fn cannot_connect(addr: &str, e: &io::Error) -> String {
    tr!(
        "cannot connect to {}: {}\n  -> Is the server running and fully started? Check the address and port (server.properties: server-port), and the firewall.",
        "{} 에 접속할 수 없습니다: {}\n  -> 서버가 켜져 있고 로딩이 끝났는지, 주소와 포트(server.properties 의 server-port), 방화벽을 확인하세요.",
        addr, e
    )
}

pub fn unsupported_version(server: &str, protocol: i32) -> String {
    tr!(
        "the server runs '{}' (protocol {}), which this bot cannot speak. Supported: {}.\n  -> Use a server of one of these releases, or connect through ViaProxy (see the README).",
        "서버 버전 '{}' (프로토콜 {}) 은 지원하지 않습니다. 지원 버전: {}\n  -> 지원 버전 서버를 사용하거나 ViaProxy 를 거쳐 접속하세요 (README 참고).",
        server, protocol, version::supported_names()
    )
}

/// The release name and protocol number in a status response.
pub fn parse_status(text: &str) -> Option<(String, i32)> {
    let status = json::parse(text)?;
    let v = status.get("version")?;
    let Json::Raw(protocol) = v.get("protocol")? else { return None };
    let name = match v.get("name") {
        // shown on the terminal: drop control characters a server could smuggle in
        Some(Json::Str(s)) => s.chars().filter(|c| !c.is_control()).take(60).collect(),
        _ => "?".to_string(),
    };
    Some((name, protocol.parse().ok()?))
}

/// Pick the release to speak: `--version` if given (it must match what the server says),
/// otherwise the one detected with a status ping.
pub fn detect_version(addr: &str, forced: Option<&str>) -> Result<&'static Version, String> {
    let (host, port) = split_address(addr)?;
    let forced = forced.map(version::lookup).transpose()?;
    let target = format!("{host}:{port}");
    let status = match proto::status_ping(&target, &host, port) {
        Ok(text) => parse_status(&text),
        // nothing answered at all: no point in guessing a version
        Err(e) if proto::is_connect_error(&e) => return Err(cannot_connect(&target, &e)),
        Err(_) => None,
    };
    match (forced, status) {
        (Some(v), None) => Ok(v),
        (Some(v), Some((_, p))) if v.protocol == p => Ok(v),
        (Some(v), Some((name, p))) => Err(tr!(
            "--version {} was given, but the server says '{}' (protocol {}). Leave --version out to detect it.",
            "--version {} 을 지정했지만 서버는 '{}' (프로토콜 {}) 입니다. --version 을 빼면 자동으로 감지합니다.",
            v.name, name, p
        )),
        (None, Some((name, p))) => version::by_protocol(p).ok_or_else(|| unsupported_version(&name, p)),
        (None, None) => Err(tr!(
            "{} did not answer the status ping, so its version is unknown.\n  -> Is it a Minecraft Java server that finished starting? Otherwise give --version ({}).",
            "{} 이 상태 요청에 응답하지 않아 버전을 알 수 없습니다.\n  -> 로딩이 끝난 마인크래프트 자바 서버인지 확인하거나 --version 을 지정하세요 ({}).",
            target, version::supported_names()
        )),
    }
}

/// What to do about a refusal or kick reason (a text, JSON or translation key).
pub fn kick_hint(reason: &str, name: &str) -> String {
    let r = reason.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| r.contains(w));
    if has(&["whitelist", "white-list", "white_list"]) {
        tr!("add the bot to the whitelist (server console: whitelist add {})", "봇을 화이트리스트에 추가하세요 (서버 콘솔: whitelist add {})", name)
    } else if has(&["banned"]) {
        tr!("the bot or your address is banned; only the server's operator can lift that", "봇이나 IP 가 차단되었습니다. 서버 관리자만 해제할 수 있습니다")
    } else if has(&["duplicate_login", "already", "another location"]) {
        tr!("a player named {} is already online; stop it or use --name", "{} 이름의 플레이어가 이미 접속 중입니다. 종료하거나 --name 을 사용하세요", name)
    } else if has(&["outdated", "incompatible", "unsupported version"]) {
        tr!("the server rejected this version; supported: {}", "서버가 이 버전을 거부했습니다. 지원 버전: {}", version::supported_names())
    } else if has(&["server_full", "server is full"]) {
        tr!("the server is full; free a slot or raise max-players", "서버가 가득 찼습니다. 자리를 비우거나 max-players 를 늘리세요")
    } else if has(&["throttl", "wait before"]) {
        tr!("the server limits reconnects; wait a few seconds and run the same command again", "재접속 제한입니다. 몇 초 뒤 같은 명령을 다시 실행하세요")
    } else if has(&["flying", "moved too quickly", "moved wrongly"]) {
        tr!("movement was rejected; run the same command again to resume (anti-cheat plugins may not work with the bot)", "이동이 거부되었습니다. 같은 명령으로 이어서 실행하세요 (안티치트 플러그인과는 함께 못 쓸 수 있습니다)")
    } else if has(&["spam", "packet"]) {
        tr!("too many packets; run again with --delay-ms 50", "패킷이 너무 많습니다. --delay-ms 50 을 붙여 다시 실행하세요")
    } else if has(&["name", "invalid characters"]) {
        tr!("check --name: 3 to 16 letters, digits or _", "--name 을 확인하세요: 영문, 숫자, _ 3~16자")
    } else {
        tr!("check the server console; running the same command again resumes the build", "서버 콘솔을 확인하세요. 같은 명령을 다시 실행하면 이어서 건축합니다")
    }
}

pub fn kicked_message(reason: &str, name: &str) -> String {
    let shown: String = reason.chars().filter(|c| !c.is_control()).take(300).collect();
    tr!("the server removed the bot: {}\n  -> {}", "서버가 봇을 내보냈습니다: {}\n  -> {}", shown, kick_hint(reason, name))
}

/// Log in. Refusals, online mode and kicks become messages that say what to do.
pub fn join(addr: &str, name: &str, version: &Version) -> Result<PlayClient, String> {
    let (host, port) = split_address(addr)?;
    let target = format!("{host}:{port}");
    match PlayClient::join_v(&target, &host, port, name, version.protocol) {
        Ok((bot, _)) => match &bot.kicked {
            Some(reason) => Err(kicked_message(reason, name)),
            None => Ok(bot),
        },
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Err(tr!(
            "the server is in online mode (premium account authentication). This bot supports offline-mode servers only.\n  -> On a server you run yourself: set online-mode=false in server.properties and restart. Know that anyone can then join under any name, so keep it private or whitelisted.",
            "서버가 온라인 모드(정품 계정 인증)입니다. 이 봇은 오프라인 모드 서버만 지원합니다.\n  -> 직접 운영하는 서버라면 server.properties 에서 online-mode=false 로 바꾸고 재시작하세요. 이 경우 누구나 아무 이름으로 접속할 수 있으니 비공개나 화이트리스트로 운영하세요."
        )),
        Err(e) if proto::is_connect_error(&e) => Err(cannot_connect(&target, &e)),
        Err(e) if e.kind() == io::ErrorKind::ConnectionAborted => Err(kicked_message(&e.to_string(), name)),
        Err(e) => Err(tr!(
            "logging in to {} failed: {}\n  -> Check the server console; is the version right and the server done starting?",
            "{} 로그인 실패: {}\n  -> 서버 콘솔을 확인하세요. 버전이 맞고 로딩이 끝났는지 확인하세요.",
            target, e
        )),
    }
}

/// Require creative mode. While the bot is online the operator can still switch it, so wait up to
/// `wait` for that after saying which command does it.
pub fn require_creative(bot: &mut PlayClient, name: &str, wait: Duration, say: &mut dyn FnMut(&str)) -> Result<(), String> {
    // a server that never told us is given the benefit of the doubt (placements are confirmed)
    let Some(mode) = bot.game_mode.filter(|&m| m != CREATIVE) else { return Ok(()) };
    let how = tr!(
        "the bot is in {} mode, but it needs creative mode.\n  -> Server console: gamemode creative {}\n  -> or in game as an operator: /gamemode creative {}\n  -> or in server.properties: gamemode=creative and force-gamemode=true, then restart",
        "봇이 {} 모드입니다. 크리에이티브 모드가 필요합니다.\n  -> 서버 콘솔: gamemode creative {}\n  -> 또는 게임에서 관리자(OP)가: /gamemode creative {}\n  -> 또는 server.properties 에서 gamemode=creative, force-gamemode=true 로 바꾸고 재시작",
        gamemode::name(mode), name, name
    );
    say(&how);
    if !wait.is_zero() {
        say(&tr!("waiting up to {} s for the switch...", "모드 변경을 최대 {}초 기다립니다...", wait.as_secs()));
    }
    let end = Instant::now() + wait;
    while Instant::now() < end && bot.kicked.is_none() {
        bot.pump_once().map_err(|e| e.to_string())?;
        if bot.game_mode == Some(CREATIVE) {
            say(&tr!("creative mode, continuing", "크리에이티브 모드 확인, 계속합니다"));
            return Ok(());
        }
    }
    match &bot.kicked {
        Some(r) => Err(kicked_message(r, name)),
        None => Err(tr!("not in creative mode; run the command above, then try again", "크리에이티브 모드가 아닙니다. 위 명령을 실행한 뒤 다시 시도하세요")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(split_address("host").unwrap(), ("host".into(), 25565));
        assert_eq!(split_address("127.0.0.1:25566").unwrap().1, 25566);
        for bad in ["", ":1", "host:abc", "host:99999", "http://x:1"] {
            assert!(split_address(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn status_fields_are_extracted_and_cleaned() {
        let s = "{\"version\":{\"protocol\":767,\"name\":\"Paper \\u001b1.21.1\"}}";
        assert_eq!(parse_status(s), Some(("Paper 1.21.1".into(), 767)));
        assert_eq!(parse_status("{}"), None);
    }

    #[test]
    fn hints_follow_the_reason() {
        assert!(kick_hint("{\"translate\":\"multiplayer.disconnect.not_whitelisted\"}", "Bob").contains("whitelist add Bob"));
        assert!(kick_hint("multiplayer.disconnect.duplicate_login", "Bob").contains("--name"));
        assert!(kick_hint("Flying is not enabled on this server", "Bob").contains("resume"));
        assert!(kick_hint("something else", "Bob").contains("console"));
    }
}
