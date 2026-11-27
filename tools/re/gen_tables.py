#!/usr/bin/env python3
"""Generate one Rust table module (numbers and registry names only) from the server's data reports.

    python -I gen_tables.py --version 1.21.4 --protocol 769 --reports <dir with blocks.json,
        registries.json, packets.json> --out crates/mc-bot/src/data/v769.rs

Releases before 1.21 write no packets.json; pass --connection-protocol <decompiled
ConnectionProtocol.java> instead (kept outside the repo) and the ids are the registration order.
"""
import argparse, json, os, re

# (state, direction, field, accepted report names). A field whose packet does not exist gets -1.
S, C = "serverbound", "clientbound"
FIELDS = [("handshake", S, "intention"), ("status", S, "status_request"), ("status", C, "status_response")]
FIELDS += [("login", S, n) for n in ("hello", "login_acknowledged")]
FIELDS += [("login", C, n) for n in ("login_disconnect", "hello", "login_finished|game_profile", "login_compression")]
FIELDS += [("configuration", S, n) for n in ("client_information", "finish_configuration", "keep_alive", "pong", "select_known_packs")]
FIELDS += [("configuration", C, n) for n in ("disconnect", "finish_configuration", "keep_alive", "ping", "select_known_packs")]
FIELDS += [("play", S, n) for n in (
    "accept_teleportation", "chunk_batch_received", "client_tick_end", "keep_alive", "move_player_pos",
    "move_player_pos_rot", "pong", "player_action", "player_loaded", "set_carried_item",
    "set_creative_mode_slot", "swing", "use_item_on", "player_command", "player_input")]
FIELDS += [("play", C, n) for n in (
    "block_update", "chunk_batch_finished", "disconnect", "keep_alive", "level_chunk_with_light", "login",
    "ping", "player_position", "section_blocks_update", "set_health")]

# Class name (without the "Packet" suffix and flow prefix) -> report name, where snake case differs.
RENAMES = {
    "client_intention": "intention", "status_request": "status_request", "pong_response": "pong_response",
    "login_compression": "login_compression", "move_player.pos": "move_player_pos",
    "move_player.pos_rot": "move_player_pos_rot", "set_health": "set_health",
}


def snake(cls):
    cls = re.sub(r"^(Clientbound|Serverbound)", "", cls).replace("Packet", "")
    s = ".".join(re.sub(r"(?<!^)(?=[A-Z])", "_", p).lower() for p in cls.split("."))
    return RENAMES.get(s, s)


def from_connection_protocol(path):
    """Ids = registration order per (state, flow); the bundle delimiter takes the first id."""
    out, state, flow, n = {}, None, None, 0
    for line in open(path, encoding="utf-8"):
        m = re.match(r"   ([A-Z]+)\(\s*$", line)
        if m:
            state = {"HANDSHAKING": "handshake"}.get(m.group(1), m.group(1).lower())
        m = re.search(r"PacketFlow\.(CLIENTBOUND|SERVERBOUND),", line)
        if m:
            flow, n = m.group(1).lower(), 0
        if "withBundlePacket(" in line:
            n += 1
        m = re.search(r"\.addPacket\(([\w.]+)\.class", line)
        if m:
            name = snake(m.group(1))
            if state == "login" and flow == C:
                name = {"login_disconnect": "login_disconnect"}.get(name, name)
            out.setdefault(state, {}).setdefault(flow, {})[name] = n
            n += 1
    return out


def from_report(path):
    raw = json.load(open(path, encoding="utf-8"))
    return {st: {fl: {k.split(":", 1)[1]: v["protocol_id"] for k, v in pk.items()}
                 for fl, pk in flows.items()} for st, flows in raw.items()}


def rust_field(state, flow, name):
    return f"{'sb' if flow == S else 'cb'}_{state.replace('configuration', 'config')}_{name.split('|')[0]}"


def wrap(items, width=118):
    line, out = "   ", []
    for it in items:
        if len(line) + len(it) + 2 > width:
            out.append(line)
            line = "   "
        line += " " + it + ","
    out.append(line)
    return "\n".join(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", required=True)
    ap.add_argument("--protocol", type=int, required=True)
    ap.add_argument("--reports", required=True)
    ap.add_argument("--connection-protocol")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    load = lambda n: json.load(open(os.path.join(a.reports, n), encoding="utf-8"))
    packets = (from_connection_protocol(a.connection_protocol) if a.connection_protocol
               else from_report(os.path.join(a.reports, "packets.json")))
    ids, lines = {}, []
    for state, flow, names in FIELDS:
        table = packets.get(state, {}).get(flow, {})
        pid = next((table[n] for n in names.split("|") if n in table), -1)
        lines.append(f"    {rust_field(state, flow, names)}: {pid},")

    items = load("registries.json")["minecraft:item"]["entries"]
    by_id = {v["protocol_id"]: k.split(":", 1)[1] for k, v in items.items()}
    assert sorted(by_id) == list(range(len(by_id))), "item ids are not dense"
    blocks, count = [], 0
    for name, b in load("blocks.json").items():
        sids = [s["id"] for s in b["states"]]
        default = next(s["id"] for s in b["states"] if s.get("default"))
        blocks.append((name.split(":", 1)[1], min(sids), default, len(sids)))
        count += len(sids)

    body = [
        f"//! Generated by tools/re/gen_tables.py from the {a.version} server reports. Do not edit.",
        "", "use super::{Packets, Tables};", "",
        "#[rustfmt::skip]", "const PACKETS: Packets = Packets {", *lines, "};", "",
        "/// Item names; the index is the item registry id.",
        "#[rustfmt::skip]", "const ITEMS: &[&str] = &[",
        wrap([f'"{by_id[i]}"' for i in range(len(by_id))]), "];", "",
        "/// (block, first state id, default state id, number of states), sorted by first id.",
        "#[rustfmt::skip]", "const BLOCKS: &[(&str, u32, u32, u32)] = &[",
        wrap([f'("{n}", {lo}, {d}, {c})' for n, lo, d, c in sorted(blocks, key=lambda x: x[1])]), "];", "",
        "pub static TABLES: Tables = Tables {",
        f"    protocol: {a.protocol},", f"    state_count: {count},", "    packets: PACKETS,",
        "    items: ITEMS,", "    blocks: BLOCKS,", "};", "",
    ]
    open(a.out, "w", encoding="utf-8", newline="\n").write("\n".join(body))
    print(f"{a.out}: {len(by_id)} items, {len(blocks)} blocks, {count} states")


if __name__ == "__main__":
    main()
