//! Minecraft version table.
//!
//! Packet ids and the registry change between versions, so a version is only
//! `supported` when the bot ships a verified id table for it. Everything else is
//! listed so the CLI can explain *why* it refuses (and how to add one, see
//! CONTRIBUTING.md) instead of failing with a confusing disconnect.

use crate::tr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub name: &'static str,
    pub protocol: i32,
    /// A packet-id table for this protocol is implemented and was tested against a real server.
    pub supported: bool,
}

pub const DEFAULT: Version = Version { name: "1.21.1", protocol: 767, supported: true };

/// Known protocol numbers (from the Minecraft wiki "Protocol version IDs").
pub const VERSIONS: &[Version] = &[
    Version { name: "1.20.2", protocol: 764, supported: false },
    Version { name: "1.20.4", protocol: 765, supported: false },
    Version { name: "1.20.6", protocol: 766, supported: false },
    Version { name: "1.21", protocol: 767, supported: true },
    DEFAULT,
    Version { name: "1.21.3", protocol: 768, supported: false },
    Version { name: "1.21.4", protocol: 769, supported: false },
    Version { name: "1.21.5", protocol: 770, supported: false },
];

pub fn lookup(name: &str) -> Result<Version, String> {
    let name = name.trim().trim_start_matches('v');
    match VERSIONS.iter().find(|v| v.name == name) {
        Some(v) if v.supported => Ok(*v),
        Some(v) => Err(tr!(
            "Minecraft {} (protocol {}) is known but has no verified packet-id table yet. Supported: {}. Newer servers can be reached through ViaProxy, see the README.",
            "Minecraft {} (프로토콜 {})은 알려져 있지만 검증된 패킷 ID 표가 아직 없습니다. 지원: {}. ViaProxy 경유는 README 참고.",
            v.name,
            v.protocol,
            supported_names()
        )),
        None => Err(tr!(
            "unknown Minecraft version '{}'. Supported: {}",
            "알 수 없는 마인크래프트 버전 '{}'. 지원: {}",
            name,
            supported_names()
        )),
    }
}

pub fn supported_names() -> String {
    VERSIONS.iter().filter(|v| v.supported).map(|v| v.name).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_supported() {
        assert_eq!(lookup("1.21.1").unwrap(), DEFAULT);
        assert_eq!(lookup("v1.21").unwrap().protocol, 767);
    }

    #[test]
    fn known_but_unsupported_explains_itself() {
        let e = lookup("1.20.4").unwrap_err();
        assert!(e.contains("1.20.4") && e.contains("1.21.1"), "{e}");
    }

    #[test]
    fn unknown_version_is_rejected() {
        assert!(lookup("9.9").is_err());
    }

    #[test]
    fn protocol_numbers_are_unique_per_name() {
        let mut names: Vec<_> = VERSIONS.iter().map(|v| v.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), VERSIONS.len());
    }
}
