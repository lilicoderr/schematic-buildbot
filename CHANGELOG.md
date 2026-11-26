# Changelog

## [Unreleased]

### Added
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

### Changed
- README is now English-first; the Korean version moved to `README.ko.md`.
- Source comments, blueprints, CHANGELOG and CONTRIBUTING are in English.
- `PlayClient::place_block` replaced by `PlayClient::place(pos, item, &Plan)`.
- `proto::handshake` takes the protocol number; `login_offline_v` announces an explicit one.

### Fixed
- Remaining clippy warnings.
- `preview.py` strips block-state brackets and colours stairs/slabs like their base material.

## 0.2.0 (unreleased tag)

- `.litematic` parser (multi-region, bit-packed palette), `--dry-run` ASCII layer map with floating-block warnings,
  `--delay-ms`, stdlib-only `tools/preview.py`.

## 0.1.0

- Hand-written 1.21.1 (767) protocol: handshake, login, Configuration, Play.
- `.schem` (Sponge v2/v3) and direct-URL builds, 1333 block types.
- Ground-walking builds with no anti-cheat kicks.
