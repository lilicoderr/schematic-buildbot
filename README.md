<h1 align="center">schematic-buildbot</h1>

<p align="center"><b>English</b> · <a href="README.ko.md">한국어</a></p>

<p align="center">
  <b>One command: a Rust bot downloads a Minecraft schematic from a URL and builds it on your offline-mode creative server.</b><br>
  Hand-written protocol (1.20.4, 1.21.1, 1.21.4, 26.3), no libraries · walks like a player, never flies or speeds · block states supported
</p>

<p align="center">
  <a href="https://github.com/lilicoderr/schematic-buildbot/actions/workflows/ci.yml"><img src="https://github.com/lilicoderr/schematic-buildbot/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <img src="https://img.shields.io/badge/rust-stable-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/minecraft-1.20.4_%7C_1.21.x_%7C_26.3-62B47A" alt="Minecraft 1.20.4, 1.21, 1.21.1, 1.21.4, 26.3">
  <img src="https://img.shields.io/badge/blocks-1333-blue" alt="1333 blocks">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT"></a>
</p>

<p align="center">
  <img src="docs/cottage.png" alt="cottage preview" width="200">
  <img src="docs/stair-tower.png" alt="stair tower preview" width="200">
  <img src="docs/pixel-heart.png" alt="pixel heart preview" width="240">
</p>
<p align="center"><sub>Blueprint previews rendered by <code>tools/preview.py</code> (no server needed)</sub></p>

## Quickstart: one command

> **Supported servers:** only **offline-mode** (`online-mode=false`) servers, with the bot in **creative mode**.
> Online-mode (premium authentication) servers are not supported. Use it only on a server you run or where you have
> the operator's permission.

1. **Get the bot.** Download the Windows or Linux archive from
   [Releases](https://github.com/lilicoderr/schematic-buildbot/releases) and unpack it (or `cargo build --release`).
2. **Prepare the server.** In `server.properties`: `online-mode=false` and `gamemode=creative`, then restart. With
   online mode off anyone can join under any name, so keep such a server private or whitelisted. The bot builds on flat,
   clear ground.
3. **Build.** On Windows double-click `build.bat` and answer its questions (schematic link or file, server address, bot
   name). Or run:

```bash
build-bot build https://example.com/castle.schem 127.0.0.1:25565
```

A link from a sharing site must be the **direct** `.schem` / `.litematic` download link, not the web page that shows the
build (or download the file and give its path). That one command:

1. downloads the file (http/https only, at most 32 MB and 60 s),
2. detects the server's version with a status ping,
3. plans every step offline and **simulates** the plan; if a block cannot be reproduced exactly it says why and builds
   nothing,
4. checks the server: cannot connect, online mode, unsupported version, not creative (it names the command and waits
   120 s), refused or kicked (with the reason), each with what to do,
5. builds, checks the block states the server reports and repairs wrong blocks.

**Interrupted?** Run the same command again from the same folder: the plan and the progress are kept in `.buildbot/`
and the build continues at the same place.

| `build` option | Meaning |
|---|---|
| `--dry-run` | print blocks, estimated time and unsupported blocks, then stop **without connecting** (address optional; give `--version` if not 1.21.1) |
| `--origin x,y,z` | world position of the blueprint's (0,0,0) corner. `y` must be the height the bot stands at; at most 128 blocks from where it joins, flat clear ground on the way. Default: just beside where the bot joins |
| `--name N` | bot name (default `BuildBot`) |
| `--lang en\|ko` | output language |
| `--allow-partial` | build even if some blocks are unsupported, unreachable or would get another state |
| `--fresh` | forget the saved progress (or the "already finished" mark) and start over |
| `--version V`, `--wait-creative S`, `--delay-ms N`, `--walk`, `--state-dir D` | set the version yourself, change the creative-mode wait, slow down, do not sprint, keep job files elsewhere |

```text
$ build-bot build blueprints/factory-small.schem --dry-run
planning 2408 blocks...
blueprint 'factory-small.schem': 2408 blocks, size 27x18x24, Minecraft 1.21.1
plan: 2408 placements, 249 temporary blocks (removed again), estimated time 3m57s
unsupported blocks: 0 | not buildable in total: 0
simulation: clean (every step legal, final blocks == schematic)
dry run: not connecting to any server
```

Checked live on 2026-11-28 (local vanilla 1.21.1, small factory): `build` with an `http://` URL on a local test web
server; survival mode was detected and switched with the printed command; the bot process was killed at about 40% and
the same command run again: 2408/2408 states exact over RCON, 83,496 cells around the build all air, 0 kicks.
`--origin` was checked with `stair-tower.txt` (a walk of about 20 blocks, 51/51 by the bot's own check). The online-mode,
unsupported-version, cannot-connect and refused-login messages are tested against mock servers only.

## Web page: `build-bot ui`

```bash
build-bot ui          # prints a link (with a one-time key) and opens it on Windows; Ctrl+C closes the page
```

<p align="center"><img src="docs/webui-building.png" alt="build-bot ui while building the small factory: 3D preview following the build, progress, log (headless Chromium, real page)" width="800"></p>

Drop a `.schem` / `.litematic` / `.txt` on the page (or paste a direct link). It is read, planned and **simulated**
exactly as `build` does; you see a 3D preview with a layer slider (the plan previewer, three.js 0.160.0 included in the
program with its licence, so it works offline), size, block types, estimated time and the problems that would stop a
build. Then enter the server address and a bot name and press **Build**: percent, blocks per second, time left, repairs
and the log update live. **Stop** ends the build between two steps with the progress saved; **Build** again with the same
file and server resumes. A build started here is the same `build-bot build` process as on the command line, so
resuming also works from either side. Options: `--port N`, `--no-open`, `--state-dir D`, `--lang en|ko`.

Safety: the page listens on **127.0.0.1 only** (there is no option for another address). Every request needs a random
per-run key (it travels in the link's `#` part, which browsers never send anywhere), the `Host` and `Origin` headers must
be this server's own (DNS rebinding and other web sites in your browser cannot use it), uploads are limited to 32 MB
and links to http(s) with the usual 32 MB / 60 s limits, uploaded names cannot name a folder, the build is started with
an argument list (no shell) and only after checking server address and name, and the page may load nothing from outside
(content security policy). `crates/mc-bot/tests/webui_security.rs` attacks each of these.

Checked on 2026-11-28 in headless Chromium (screenshots above and below are of the real page; no in-game screenshots):
`factory-small.schem` loaded in the page, built on a local vanilla 1.21.1 server (127.0.0.1:25569), **stopped with the
Stop button at about 10 %, then resumed with Build**: 2408/2408 states exact over RCON (`tools/verify_rcon.py`), 0 kicks.
The page made no request to anything but the program itself. Not checked: other browsers, a phone, a non-loopback network.

<p align="center"><img src="docs/webui-preview.png" alt="build-bot ui after loading the small factory (not in-game)" width="400"> <img src="docs/webui-stopped.png" alt="build-bot ui after Stop" width="400"></p>

## Several bots building together: `--bots N`

`build-bot build castle.schem host:port --bots 4` (N = 2 to 4) builds with N bots at once, named `BuildBot1` ...
`BuildBot4` (`--name` plus a number). Nothing about the rules changes: every bot walks and jumps on the ground, never
flies, and the block states stay exact.

* **Strips.** The schematic is cut along x or z into one strip per bot. A bot stands, places and builds scaffold only in
  the columns of its own strip, so two bots can never place into or stand in each other's cells, whatever their relative
  speeds, and nobody removes scaffold another bot needs: every bot owns its scaffold and takes all of it down inside its
  strip. Cuts never split a bed, a double chest, a door with its neighbours, or a block from what it hangs on. The cut
  positions are tried and moved until the estimated finish times of the bots are even (the plan is cut again with each
  candidate, so this part of planning takes a while: tens of seconds for the small factory).
* **A later phase.** What no strip can build alone (it needs something across a cut) is left out of its strip and built
  afterwards by bot 1, alone; the other bots have left by then. There is a barrier between the phases.
* **Joint simulator** (`mc_bot::multi::joint`). The plans of all bots are replayed in one shared world, one player per
  bot, in six interleavings (by estimated time, one bot after the other in both directions, three random ones): every
  step must be legal with the strict single-bot rules, no block may be placed into or a bot moved into another bot's body,
  scaffold may be broken only by its owner and never under another bot, and the result must be the schematic exactly. A
  plan that fails is never built.
* **Runner.** One connection and thread per bot; all join, walk to their start cell and begin together. Each part has its
  own progress file in `.buildbot/<job>/botsN/`, so running the same command again resumes every bot at its own step. If
  one bot fails or is removed, all others stop at their next packet (nothing is left half-way in a state the plan cannot
  continue from).
* The verifier of each bot ignores the columns of the other strips, so a bot never "repairs" what a neighbour is placing.

Not covered: the site survey of the single-bot `build` (`--force`), `--rotate` and `--mirror` are applied before
planning and work, but undo of a multi-bot build and `ui` do not know about it yet.

## Slice, simulate, run (plan pipeline)

Like a 3D-printer slicer, the bot can compile a schematic offline into a deterministic plan and replay it:

```text
 schematic ──slice──▶ plan.bbplan ──simulate──▶ legal? ──run──▶ server
 (.schem/.litematic)  (text steps:       (every step checked   (replay, confirm each
                       MOVE PILLAR        in an in-memory       step against the
                       SUPPORT PLACE      world, final world    exact block states,
                       BREAK CHECKPOINT)  == schematic)         repair at once)
```

```bash
build-bot slice castle.schem -o castle.bbplan          # offline, no server; refuses plans that fail simulation
build-bot simulate castle.bbplan castle.schem          # re-check every step
build-bot run castle.bbplan 127.0.0.1:25566 --verify   # replay on a server
```

While running, every position a step touched, and every other block change the server reports (neighbour updates, slabs
that merged, stray blocks), is compared with the state the plan wants there at that moment, as soon as the server has
acknowledged the clicks. Wrong, missing or stray blocks are broken and placed again in plan order (at most 3 times per
position); every `CHECKPOINT` audits all known positions again. Connections and stair shapes that placing again cannot
change are only reported.

The plan format is in [docs/PLAN_FORMAT.md](docs/PLAN_FORMAT.md). The bot stays on the ground: high spots are reached by
jumping up one block, by temporary scaffold pillars (jump, place under the feet, land) or scaffold bridges, and every scaffold
block is broken again before the plan ends.

Measured on a local vanilla 1.21.1 server (small factory, 2408 blocks): sliced in 0.1 s with 0 unreachable blocks,
run in 235 s (10.3 blocks/s), 2408/2408 states exact over RCON, 0 re-placed, 0 kicks. For the 22k-block factory see the next section.
Neighbour-derived block states (stair shape, fence and pane connections) cannot be chosen by a client; the slicer counts them.

## Large factory: live build (22,481 blocks)

| <img src="docs/live-factory/hall-roof.png" alt="prismarine-viewer render of the finished large factory in the live world (not a client screenshot)" width="420"> | <img src="docs/live-factory/chimneys-silos.png" alt="prismarine-viewer render of the finished large factory in the live world (not a client screenshot)" width="420"> |
|---|---|
| <img src="docs/live-factory/office-side.png" alt="prismarine-viewer render of the finished large factory in the live world (not a client screenshot)" width="420"> | <img src="docs/live-factory/from-above.png" alt="prismarine-viewer render of the finished large factory in the live world (not a client screenshot)" width="420"> |

<p align="center"><sub><b>prismarine-viewer renders of the live world</b> after the build, not Minecraft client screenshots (<code>tools/viewer/capture.js</code>)</sub></p>

`blueprints/factory-large.schem` (22,481 blocks, 54,780 plan steps), built by `build-bot run --verify` on a local vanilla
1.21.1 server (creative, offline, 2 GB heap, default gamerules, 2026-11-28), from an empty world, **in one run with no
resume**, with no viewer or other player connected. (The renders above are of an earlier build of the same schematic.)

* 22,481 / 22,481 placed, 2,822 s (47 min), **8.0 blocks/s**, 0 kicks, 0 refused, nothing missing, no lag or
  movement warning in the server log. Walking took 2,186 s of it (11,581 moves; the slicer estimated 2,244 s walking,
  2,875 s total), at the same vanilla sprint and jump speed as before: the gain comes from the build order. Median
  speed per 100 placements: 22 blocks/s in the first third, 11 in the middle, 16 in the last.
* The run checks every step and every block change the server reports against the exact planned state, and repairs
  at once. This run needed **no repair**, and its final report lists 0 differing states.
* Independent check over RCON (`tools/verify_rcon.py`), right after the build: **22,481 / 22,481 states exact**.
* Leftover scaffold: 721,018 cells around the build that are not in the schematic (16 blocks to each side and 8 above,
  from the ground up) were queried; all are air.
* Copper: all 37 `copper_block`s were unaged at the time of the check. `randomTickSpeed` was left at the default 3, so
  this is a point-in-time result: vanilla keeps ageing unwaxed copper. In earlier runs, copper that aged during the
  build was seen and placed again by the same checks; nothing keeps it unaged after the bot has left.
* This is one run on one schematic, one server version and a flat world; it is not a guarantee for other builds.
* History with this schematic: 6,898 s with the older layer-by-layer order (22,479 exact), then 4,685 s with the
  4-layers-per-16x16-tile order, one layer after another (22,475 exact, 6 declared log-axis fallbacks), and 4,646 s
  (4.8 blocks/s, 22,481 exact) once temporary blocks had removed those fallbacks. The current order finishes a band
  of 4 layers in an 8x8 tile in any layer order, so a wall is built from one spot instead of once per layer.

An earlier build the same day, also with the older order, needed two resumes (7,338 s active, 3.1 blocks/s). It exposed a planner bug (a slab clicked
onto an equal slab) and a walk-only way back after repairs; both are fixed. Its checkpoints only counted non-air blocks, so
three swapped slab / lantern pairs were only found at the end. At its checkpoint 11 four blocks were placed again for a
**cause that is still unknown**: replaying that plan up to there with the new build gave 2750/2750 and no repair.

## Slicing preview

Like the layer preview of a 3D-printer slicer, a `.bbplan` can be inspected before any server is involved:

```bash
cargo run --release --bin build-bot -- slice blueprints/factory-small.schem -o small.bbplan
python tools/plan_preview/build.py small.bbplan -o preview.html   # open preview.html in a browser
```

<p align="center"><img src="docs/plan-preview.gif" alt="animated plan preview of the small factory (not in-game)" width="800"></p>
<p align="center"><sub><b>Plan preview, not in-game:</b> the small factory plan (5639 steps) replayed offline in the browser</sub></p>

| Temporary blocks and bot path | Layer slider (layers 0 to 5) |
|---|---|
| <img src="docs/plan-preview-scaffold.png" alt="plan preview with temporary scaffold highlighted (not in-game)" width="420"> | <img src="docs/plan-preview-layer.png" alt="plan preview cut at layer 5 (not in-game)" width="420"> |

Step and layer sliders, play button, "only this layer" switch, the current step as text with its cell outlined, the bot and
its last 60 positions, temporary supports (orange) and scaffold pillars (yellow) visible through walls, and live and header
statistics. The page is a single HTML file; three.js 0.160.0 is loaded from jsDelivr, so the browser needs network access.
Built without a plan argument, it has a file picker instead. Blocks are drawn as flat-coloured cubes (stairs, slabs and
other shapes are not modelled). It shows what the plan says; legality is checked by `build-bot simulate`, not by the viewer.

To regenerate the images:

```bash
cd tools/plan_preview && npm install                     # playwright-core; needs its Chromium installed
node render.js ../../preview.html shots frames           # two PNGs + GIF frames
python gif.py frames shots/plan-preview.gif              # needs Pillow
```

## How the rules were derived

The reach, placement, movement and packet rules are read from the official Minecraft 1.21.1 server (decompiled with the
official Mojang mappings, tables from the server's own data generator) and written down in our own words in
[docs/RE](docs/RE). No Mojang code or jars are distributed; `tools/re` holds the scripts that fetch and map them locally.
Examples: a click is accepted when the eye is within 5.5 (survival) / 6.0 (creative) blocks of the clicked block's box, the
bot must send a rotation packet before clicking, answer chunk batches and confirm each teleport exactly once.

## Features

| Feature | Status |
|---|---|
| Hand-written protocol: handshake, login, Configuration, Play, zlib, offline UUIDs | ✅ |
| `.schem` (Sponge v2/v3), `.litematic` (multi-region) and a plain-text layer format | ✅ |
| Build straight from a URL (`http(s)://…/x.schem`) | ✅ |
| 1333 block types (block → item id table) | ✅ |
| **Block states**: stairs facing/half, slabs bottom/top/double, log axis, door/bed/furnace/chest facing | ✅ see [How states work](#block-states) |
| Anchored build order: overhangs are built from neighbours, floating islands are reported | ✅ |
| `--dry-run`: layer map, unregistered blocks, state report, floating-block warning | ✅ |
| Walks on the ground (`on_ground`, below walking speed): no fly/speed kicks | ✅ |
| `--rate` / `--delay-ms`: cap the placement rate for strict servers | ✅ |
| `--resume`: continue an interrupted build from a progress file | ✅ |
| `--lang en\|ko`, `BUILDBOT_LANG`: English and Korean output | ✅ |
| `tools/preview.py`: blueprint → isometric PNG (Python standard library only) | ✅ |
| Several Minecraft versions (`--version`) | ✅ see [Supported versions](#supported-versions); other releases via [ViaProxy](https://github.com/ViaVersion/ViaProxy) |
| Survival mode, taking materials from chests | 🚧 needed packet ids are mapped |

## Supported versions

Pick one with `--version` (default 1.21.1). With the plan pipeline, give it to `slice`; the plan records the protocol.
All of them have generated id tables, and the packet layouts that differ have byte-level unit tests. The last column is
what ran on a local vanilla server of that release (creative, offline).

| Version | Protocol | Tables + unit tests | Live |
|---|---|---|---|
| 1.20.4 | 765 | ✅ | ✅ current planner: short builds; 2408-block factory 2408/2408 exact, 235 s |
| 1.21 | 767 | ✅ (shares 1.21.1's) | not run separately |
| 1.21.1 | 767 | ✅ | ✅ current planner: 22,481-block factory 22,481/22,481 exact, 2408-block factory 2408/2408 exact; short builds with an older planner |
| 1.21.4 | 769 | ✅ | ✅ current planner: short builds; 2408-block factory 2408/2408 exact, 235 s |
| 26.3 | 777 | ✅ | ✅ current planner: short builds; 2408-block factory 2408/2408 exact, 236 s |

Re-verified on 2026-11-28 on 1.20.4, 1.21.4 and 26.3 with the current build order (code `2d5146e`, through the plan
pipeline, `run --verify`, each build on a cleared area, then compared block by block over RCON): `factory-small.schem`
2408/2408 (against the schematic), `cottage.txt` 25/25, `stair-tower.txt` 51/51, `state-lab.txt` 27/27 (including the
door's upper half), `pixel-heart.txt` 27/27, a 5-wide 9-high wall 45/45 (builds and removes scaffold) and three single
chests in a row 3/3 (needs sneaking; the wall and chest blueprints are not in the repo). All exact; 0 re-placed,
0 kicks, no movement or lag warning in the server logs. In a box around each build (16 blocks to each side, 8 above,
from the ground surface up) every cell outside the build was air. `stair-tower.txt` in direct mode: 51/51 confirmed by
the server's block updates (the bot's own check, not RCON). The large factory has **not** been built on these three
releases.
Survival mode, online mode, Paper/Spigot and releases not listed are untested. Differences between releases:
[docs/RE/versions.md](docs/RE/versions.md).

## Install & run (lower-level modes)

For building, use the [one command](#quickstart-one-command) above. The direct mode below places blocks without a plan.

```bash
cargo build --release

./target/release/build-bot 127.0.0.1:25566 BuildBot blueprints/cottage.txt         # build
./target/release/build-bot x x blueprints/stair-tower.txt --dry-run                # validate only
./target/release/packet-probe probe 127.0.0.1:25566 ProbeBot 6                     # sample and classify packets for 6 s
python tools/preview.py blueprints/cottage.txt -o docs/cottage.png --scale 36      # preview PNG
```

Targets `online-mode=false` (offline) servers. For local testing run vanilla 1.21.1 with
`online-mode=false` and `gamemode=creative` (the bot puts blocks in hand via creative slots).

> Online mode (Mojang auth/encryption) is out of scope. Only use this on servers whose operator allows it.

### Options

| Option | Meaning |
|---|---|
| `--dry-run` | validate and print a report, then exit (no server needed) |
| `--delay-ms N` / `--rate N` | pause after each placement / cap placements per second |
| `--version V` | Minecraft version of the server (default `1.21.1`, see [Supported versions](#supported-versions)) |
| `--resume`, `--progress-file F` | continue from `.buildbot-progress` (or `F`) |
| `--lang en\|ko` | output language (also `BUILDBOT_LANG`; defaults to your locale) |

## Block states

In vanilla the placed state is decided by the **click**, not the item: clicked face, cursor
position inside that face, and the player's horizontal facing. `placement.rs` turns a wanted
state into those inputs:

| Block family | State | How the bot gets it |
|---|---|---|
| stairs, doors, fence gates, beds | `facing` | looks along `facing` while clicking |
| furnaces, chests, carved pumpkins, glazed terracotta, … | `facing` | looks the opposite way |
| stairs | `half=top` | clicks the underside of the block above, or the upper half of a neighbour's side |
| slabs | `type=top/double` | same as stairs; `double` clicks twice |
| logs, pillars, hay/bone blocks | `axis=x/y/z` | clicks a face along the axis on a neighbouring block; the planner (`slice`) puts a temporary block there if there is none and removes it again |

With `slice`, every blueprint in this repository now has 0 such fallbacks (checked by `tests/exact_axes.rs`). Otherwise,
anything the surroundings do not allow (no neighbour to click) falls back to the default state and is
listed in the run summary and in `--dry-run`. Upper door halves and bed heads are skipped because the
server creates them with the first half. Trapdoors, buttons, levers and 6-way blocks currently use
the default state.

## Blueprint format

Besides `.schem` / `.litematic`, a hand-drawable layer/palette text format is supported (see `blueprints/`):

```text
name Small cottage
palette W white_concrete
palette T oak_stairs[facing=east,half=bottom]   # states go in brackets
layer 0          # the y=0 plane; each row below is one z, each character one x
WWW
W.W              # '.' or a space is air
WWW
```

## How it works

```mermaid
flowchart LR
  A[".schem / .litematic / URL / .txt"] --> B["parsers (nbt · schem · litematic · schematic)"]
  B --> C["anchored build order, bottom-up"]
  C --> P["placement planner: clicks + yaw per block state"]
  P --> D{"within reach?"}
  D -- no --> E["walk to a ground cell (anti-cheat-safe speed)"]
  E --> D
  D -- yes --> F["Use Item On: click the planned face"]
  F --> G["answer keepalive / teleports to stay connected"]
  F --> H["progress file for --resume"]
```

```text
crates/mc-proto/      VarInt/VarLong, strings, Position, UUID, length framing + zlib (dep: flate2)
crates/mc-bot/
  net.rs  proto.rs    TCP · handshake / status ping / offline login / Configuration
  data/               per-version packet, item and block tables (generated by tools/re/gen_tables.py)
  version.rs          version table behind --version and per-version packet layouts
  codec.rs            packet bodies that differ between releases
  world.rs            coordinates · faces · view angles · reach
  nbt.rs schem.rs litematic.rs   NBT reader · Sponge .schem and Litematica parsers
  schematic.rs        blueprint model · block states · anchored build order · ASCII map
  placement.rs        block state → clicks, cursor and yaw (pure, unit-tested)
  builder.rs          Play client: movement · placement · keepalive
  progress.rs         resume file
  i18n.rs             en/ko message switch
  blocks.rs           block → item id of the chosen version
  bin/                build-bot, packet-probe
tools/preview.py      blueprint → isometric PNG
```

## Why hand-written?

- To understand and verify the protocol itself: VarInt (LEB128), Position packing (26/12/26), zlib framing,
  the Configuration transition and offline UUIDs (MD5) are implemented here and checked against official test vectors.
- Packet ids are cross-checked against [minecraft-data](https://github.com/PrismarineJS/minecraft-data) 1.21.1.

## Movement and anti-cheat

- Always `on_ground=true`, and per-tick movement stays below walking speed (~0.26 blocks), so no
  "moved too quickly" / "flying is not enabled" kicks.
- Blocks are placed by clicking a face of something that already exists, never in mid-air. Blocks with
  nothing to rest on are reported by `--dry-run`.
- Synchronize Player Position (teleport) is answered with Confirm Teleport immediately.

## What is verified

- `cargo test --workspace` (50+ unit tests, no server needed): VarInt/Position/zlib/MD5 vectors,
  blueprint and `.schem`/`.litematic` parsing, build ordering, the block-state planner, progress file,
  version table. CI also runs `clippy -D warnings` and a bilingual `--dry-run` smoke test.
- Live server (local vanilla 1.21.1, creative, offline), checked block by block over RCON
  (`execute if block … <state>`), no kick in any run:

  | Blueprint | Result |
  |---|---|
  | `cottage.txt` | 25/25 blocks |
  | `stair-tower.txt` | 51/51: double slab, stairs `facing=north/east`, bottom slabs, vertical logs all matched |
  | `state-lab.txt` | 26 placed + door upper half auto-created: top-half stairs, `axis=x` logs, furnace, chest, door matched |
  | `--resume` | killed at 39/51, resumed: 12 more blocks, structure complete, progress file removed |

- **Not verified live:** other servers/forks (Paper, Spigot, …), releases outside [Supported versions](#supported-versions), door hinges, trapdoors,
  buttons and other families listed as unsupported. No Minecraft client screenshots were taken (the factory images are
  prismarine-viewer renders); the table above is from server-side block queries.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) · changes in [CHANGELOG.md](CHANGELOG.md).
Good first areas: more state families (trapdoors, buttons, 6-way blocks), support for
another version ([how](docs/RE/versions.md)), survival mode with chests.

## License

MIT. Packet ids were cross-checked with PrismarineJS/minecraft-data (1.21.1).
