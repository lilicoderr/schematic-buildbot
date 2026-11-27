//! Chat components in disconnect packets. In the configuration and play states the reason is a
//! network NBT component (a string tag or a compound with `text`/`translate`/`extra`), not a
//! string (docs/RE/protocol.md). Login-state disconnects are JSON strings and read as such.

use crate::nbt::{self, Nbt};
use mc_proto::buf::PacketBuf;

fn flatten(n: &Nbt, out: &mut String) {
    match n {
        Nbt::String(s) => out.push_str(s),
        Nbt::Compound(m) => {
            if let Some(t) = m.get("text").and_then(Nbt::as_str) {
                out.push_str(t);
            } else if let Some(t) = m.get("translate").and_then(Nbt::as_str) {
                out.push_str(t);
            }
            if let Some(extra) = m.get("extra").and_then(Nbt::as_list) {
                for e in extra {
                    flatten(e, out);
                }
            }
        }
        Nbt::List(l) => l.iter().for_each(|e| flatten(e, out)),
        _ => {}
    }
}

/// Plain text of a network-NBT component; the translation key if the server sent one.
pub fn component_text(bytes: &[u8]) -> String {
    match nbt::read_network(bytes) {
        Ok(n) => {
            let mut s = String::new();
            flatten(&n, &mut s);
            if s.is_empty() { "(no text)".into() } else { s }
        }
        Err(_) => "(unreadable reason)".into(),
    }
}

/// Reason of a play/configuration disconnect packet body.
pub fn disconnect_reason(body: &mut PacketBuf) -> String {
    let n = body.remaining();
    component_text(&body.read_bytes(n).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compound_component_with_text() {
        // TAG_Compound (no name) { "text": "Flying is not enabled" } END
        let mut b = vec![0x0A, 0x08, 0, 4];
        b.extend_from_slice(b"text");
        b.extend_from_slice(&[0, 21]);
        b.extend_from_slice(b"Flying is not enabled");
        b.push(0);
        assert_eq!(component_text(&b), "Flying is not enabled");
    }

    #[test]
    fn plain_string_and_translate_forms() {
        let mut s = vec![0x08, 0, 5];
        s.extend_from_slice(b"Hello");
        assert_eq!(component_text(&s), "Hello");
        let mut t = vec![0x0A, 0x08, 0, 9];
        t.extend_from_slice(b"translate");
        t.extend_from_slice(&[0, 26]);
        t.extend_from_slice(b"disconnect.exceeded_packet");
        t.push(0);
        assert!(component_text(&t).starts_with("disconnect."));
    }

    #[test]
    fn garbage_does_not_panic() {
        assert_eq!(component_text(&[0x0A, 0xFF]), "(unreadable reason)");
    }
}
