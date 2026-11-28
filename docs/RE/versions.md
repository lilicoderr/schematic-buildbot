# Versions: measured differences and how to add one

Compared: 1.20.4 (765), 1.21.1 (767, the baseline of the other docs here), 1.21.4 (769) and 26.3 (777, the latest
release when this was written). Sources: each official server jar's `version.json`, its data generator reports, and the
packet classes of the decompiled server. Releases in between (1.20.5/6, 1.21.2/3, 1.21.5 to 1.21.11, 26.1, 26.2) were
**not** compared: which of them introduced a change below is unknown, and the bot rejects them.

## 1. Tables

| | 1.20.4 | 1.21.1 | 1.21.4 | 26.3 |
|---|---|---|---|---|
| items | 1312 | 1333 | 1385 | 1658 |
| blocks / block states | 1058 / 26644 | 1060 / 26684 | 1095 / 27866 | 1286 / 35723 |
| configuration packets to client / to server | 10 / 7 | 17 / 8 | 17 / 8 | 21 / 10 |
| play packets to client / to server | 116 / 55 | 124 / 58 | 131 / 62 | 144 / 69 |
| Java used for the server | 21 (needs 17) | 21 | 21 | 25 |

- Nearly every packet id and most item ids shift between releases, so each protocol has its own generated table
  (`crates/mc-bot/src/data/v<protocol>.rs`). Air is block state 0 in all four.
- Block names are not stable: 26.3 has no `chain` block or item, so a schematic that uses it is reported as
  unregistered there. Blocks missing in older releases (for example `vault` and `heavy_core` in 1.20.4) are too.

## 2. Layout differences in packets the bot uses

| packet | 1.20.4 | 1.21.1 | 1.21.4 | 26.3 |
|---|---|---|---|---|
| client_information | 8 fields | same | + particle status VarInt | same as 1.21.4 |
| select_known_packs (configuration) | does not exist | exists | same | same |
| login success packet name | game_profile | game_profile | login_finished | login_finished, ends with a session UUID |
| item stack (set_creative_mode_slot) | bool present, id, count byte, NBT | count, id, added, removed | same | same bytes for a plain item (component values would be length-prefixed) |
| use_item_on hit result | ... inside bool | same | + world-border-hit bool | same as 1.21.4 |
| player_position | pos, yaw, pitch, flags byte, id | same | id, pos, velocity (3 doubles), yaw, pitch, flags int (9 bits) | same as 1.21.4 |
| accept_teleportation | id | id | id | id, x, y, z (doubles), yaw, pitch (floats) |
| move_player_* last byte | on-ground bool | same | bit 0 on ground, bit 1 horizontal collision | same as 1.21.4 |
| swing | hand VarInt | same | same | removed (`punch`, no body, is new; the server animates item use itself) |
| player_command actions | press shift 0, release 1, start sprint 3, stop 4 | same | same | the shift actions are gone: start sprint 1, stop 2 |
| sneaking | player_command | same | same | player_input flag byte, shift = 0x20 |
| player_action actions | START = 0 | same | same | START = 0, two new actions (one inserted at 1) |

The bot reads only the UUID and name of the login success packet, so the trailing fields do not matter, and it
always writes 0 or 1 as the movement byte, which is valid in both forms. The bytes of every row are covered by
`codec_tests.rs` (use_item_on: length and trailing bytes).

## 3. Server behaviour that changed

- **Client load gate (1.21.4, 26.3).** The play listener ignores movement and use_item_on until `hasClientLoaded()`:
  the client sends `player_loaded`, or a 60 tick timer runs out. The bot sends it after the first teleport.
- **One position per client tick (26.3).** A second movement packet with a position, before `client_tick_end`, disconnects
  with "invalid player movement". The bot sends `client_tick_end` after every movement packet on releases that have it
  (the 1.21.4 handler only updates the known movement; 1.21.4 does not have the disconnect).
- **Teleport confirmation (26.3).** The server applies the position and rotation inside accept_teleportation.
- **Unchanged** in all four: block update, section blocks update (VarLong entries), set_health, keep-alive, ping,
  chunk_batch_received, set_carried_item, set_creative_mode_slot slot range (checked in 1.21.4 and 26.3),
  disconnect reasons as network NBT in play.

The reach, placement, collision and movement notes in this folder, and the planner's block-state table
(`state_table.rs`) and orientation rules, were derived from 1.21.1 only. For other releases the
evidence is the live builds below and in the README, not a re-derivation.

### Live re-verification with the current planner (2026-11-28)

Code `2d5146e` (banded build order), a fresh vanilla flat-world server per release (creative, offline, 2 GB heap),
`slice` + `run --verify`, then `execute if block` over RCON for every block.

| build, exact states | 1.20.4 | 1.21.4 | 26.3 |
|---|---|---|---|
| factory-small (2408, judged against the schematic) | 2408, 235 s | 2408, 235 s | 2408, 236 s |
| cottage / stair-tower | 25/25, 51/51 | 25/25, 51/51 | 25/25, 51/51 |
| state-lab / pixel-heart | 27/27, 27/27 | 27/27, 27/27 | 27/27, 27/27 |
| 9-high wall / 3 single chests (not in the repo) | 45/45, 3/3 | 45/45, 3/3 | 45/45, 3/3 |
| direct mode, stair-tower (block updates only, no RCON) | 51/51 | 51/51 | 51/51 |

All plan-pipeline runs: 0 refused, 0 re-placed, 0 missing, 0 kicks, no cell outside the build non-air in the box around
it (16 to each side, 8 above, ground surface up), no "moved too quickly" / "moved wrongly" / "Can't keep up" in the
server logs. The previous run (code `7d7ce4c`, layer-by-layer order inside each tile) was exact too, with factory-small
at 464 to 466 s. Not covered: the large factory, `--resume`, and live repair paths (nothing needed a repair).

A new 26.3 server whose `server.properties` omits `white-list` refused the bot (`not_whitelisted`); set
`white-list=false` on a local test server.

## 4. Adding a release

Everything runs from `tools/re`; jars, mappings and decompiled sources stay in `%LOCALAPPDATA%\mc-re-cache`.

1. `fetch.ps1 -Version <id>` (or `latest`) downloads the server jar, checks its SHA-1 against piston-meta, and the
   mappings where the release has them. 26.3 ships unobfuscated: no mappings, no remap step.
2. Data generator, run in an empty directory:
   `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --server --output out`.
   Read `protocol_version` from the jar's `version.json`.
3. Decompile the packet classes (see [tools/re/README.md](../../tools/re/README.md)). 1.20.4 writes no `packets.json`;
   its ids come from the registration order in `ConnectionProtocol`.
4. `gen_tables.py` writes `data/v<protocol>.rs`. Register it in `data/mod.rs` and `version.rs`.
5. Diff the classes of every packet in section 2 and the matching handlers in `ServerGamePacketListenerImpl`; add a
   `Layout` field and a byte test for each difference. Do not copy a neighbouring release's layout without checking.
6. Only list a release as live in the README after building on a real server and checking the blocks with
   `tools/verify_states.py`.
