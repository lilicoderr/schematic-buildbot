# Changelog

## [Unreleased]

### Changed
- `slice`: inside a band of 4 layers of an 8x8 tile, blocks are no longer placed layer by layer, so one standing spot
  serves several layers (22,481-block factory live: 4,646 s to 2,822 s, exact). If this order cannot be completed,
  the slicer plans again with the previous layer-by-layer order.
- `run` compares every step and every reported block change with the exact planned state (once the server acknowledged the
  clicks) and repairs wrong, missing and stray blocks at once; checkpoints audit all known positions. It used to only
  check at checkpoints that placed blocks were not air.
- Jumps take 6 ticks instead of 8 (drifting while rising); `slice` shortens every walk. `est_seconds` models the executor
  and `est_move_seconds` is new.

### Fixed
- `run`: after a checkpoint repair or on resume the way back may use temporary scaffold (it used to only walk).
- `slice`: a slab is no longer clicked onto an equal slab (that made a double slab); `simulate` rejects such clicks.
- `run --refit x,y,z;...` breaks and re-places listed plan positions.

### Added
- Minecraft 1.20.4, 1.21.4 and 26.3 besides 1.21 / 1.21.1: packet ids, items and block states are generated from each
  server's data reports (`tools/re/gen_tables.py`); differing packet layouts are selected per version.
- `tools/re/fetch.ps1` verifies SHA-1 and handles releases without mappings (26.x).
- Block-state support: stairs `facing`/`half`, slabs `type` (incl. double), log/pillar `axis`,
  and `facing` for doors, fence gates, beds, furnaces, chests and similar (`placement.rs`).
  States are read from `.schem`, `.litematic` (palette `Properties`) and the text format (`oak_stairs[facing=east]`).
- Anchored build order (`Schematic::plan_order`): overhangs are built from neighbours; unplaceable floating blocks are reported.
- `--resume` / `--progress-file`: continue an interrupted build at the same origin.
- `--rate N` placements per second; `--version V` with a version table (`version.rs`).
- `--lang en|ko` and `BUILDBOT_LANG`; the CLI is English by default (Korean locale auto-detected).
- `.litematic` is now reachable from the CLI (the parser existed but was not wired in).
- `blueprints/stair-tower.txt` showcasing block states; `docs/stair-tower.png`.
- `--dry-run` reports which block states will be reproduced exactly and which fall back to the default.
- CI: `clippy -D warnings` and a bilingual `--dry-run` smoke test.

- Collision-aware walking (`path.rs` BFS): the bot routes around its own structure; `walk_to` gives up after ~1 s without progress instead of ~80 s.
- `blueprints/state-lab.txt` (top-half stairs, horizontal logs, furnace, chest, door).
- Verified live on vanilla 1.21.1 (RCON block queries): stair-tower 51/51, state-lab 26 + door half, `--resume`.

### Changed
- README is now English-first; the Korean version moved to `README.ko.md`.
- Source comments, blueprints, CHANGELOG and CONTRIBUTING are in English.
- `PlayClient::place_block` replaced by `PlayClient::place(pos, item, &Plan)`.
- `proto::handshake` takes the protocol number; `login_offline_v` announces an explicit one.

### Fixed
- Remaining clippy warnings.
- `preview.py` strips block-state brackets and colours stairs/slabs like their base material.

## Earlier (untagged)

- `.litematic` parser (multi-region, bit-packed palette), `--dry-run` ASCII layer map with floating-block warnings,
  `--delay-ms`, stdlib-only `tools/preview.py`.

## 0.1.0

- Hand-written 1.21.1 (767) protocol: handshake, login, Configuration, Play.
- `.schem` (Sponge v2/v3) and direct-URL builds, 1333 block types.
- Ground-walking builds with no anti-cheat kicks.
