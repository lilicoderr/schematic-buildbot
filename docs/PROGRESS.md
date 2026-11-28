# Progress notes (resume file)

## State (all on master, builds, tests and clippy green)

Done
* Ground-only navigation with lookahead standing-cell choice (`nav.rs`), scaffold pillars (jump-place up,
  break down) and escape pillars for walled-in spots (`scaffold.rs`, `Site`, `Mover` trait).
* Live-build timing breakdown (walk / pillar / search per 100 blocks) in `build-bot`.
* Windows timed-out read (os error 997) no longer kills the connection.
* Slicer pipeline, offline: `build-bot slice <schem> -o plan.bbplan` (`slice.rs`, `support.rs`, `slice_stats.rs`),
  `build-bot simulate plan.bbplan <schem>` (`sim.rs`, independent legality checker, tests in `sim_tests.rs`).
  Plan format: `docs/PLAN_FORMAT.md`, code `plan.rs` / `plan_parse.rs`. Steps: MOVE PILLAR SUPPORT PLACE AUTO BREAK CHECKPOINT.
* Slicing refuses to write a plan that fails simulation. Small factory slices in ~2 s.

Measured on the live test server (before the slicer, live planner): small factory 2098/2408 placed, 14m55s (2.3 blocks/s),
throughput decays as the structure grows (walking dominates: 810 s moving vs 66 s placing), no kicks.
(The earlier 2395/2408 result used the removed fly mode.) Small-factory slice result: 2093 placements, 315 unreachable.

Next
1. `build-bot run plan.bbplan host:port`: replay steps with `PlayClient` (translate plan coords by spawn - plan start),
   confirm each PLACE through `BlockLog`, resync at CHECKPOINT, resume by step index, `--verify` diff report.
2. Cut `unreachable` (315 of 2408): taller/limited pillars, better support chains, check what really blocks them.
3. Determinism test (same input, byte-identical plan) and fuzz tests on random small schematics (slice must always simulate clean).
4. RE phase (owner request): decompile vanilla 1.21.1 to read reach, placement validation, movement checks. See below.
   Then replace guessed constants (REACH 4.5 centre distance, CLICK_REACH 5.0 box distance, jump/drop rules) and write docs/RE/*.md, docs/PHYSICS.md.
5. Large factory (22k blocks) through slice -> simulate -> run, then screenshots (see viewer notes) and README (en + ko) sections.

## Reverse-engineering cache (outside the repo)

`%LOCALAPPDATA%\mc-re-cache\1.21.1\` holds server.jar, server_mappings.txt (official Mojang mappings), version.json,
`x\META-INF\versions\1.21.1\server-1.21.1.jar` (the inner server jar). `%LOCALAPPDATA%\mc-re-cache\tools\` holds
tiny-remapper-fat.jar and vineflower.jar. Scripts: `tools/re/fetch.ps1`, `tools/re/mojmap_to_tiny.py`, `tools/re/README.md`.
Not yet run (the agent session's permission policy refused to execute the downloaded jars): converting mappings, remapping,
decompiling, and `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --server --output reports`.
A maintainer must run steps 3 to 6 of `tools/re/README.md`, or allow them. Nothing from the cache is ever committed.

## Test server

Folder: `C:\Temp\claude\aff4454cecae1ca7\aded79c3-5b9a-4eb2-a535-c60305393385\scratchpad\mcserver` (may be gone after a reboot;
otherwise recreate: vanilla 1.21.1 server.jar from the cache, eula=true, online-mode=false, gamemode=creative, port 25566,
rcon 25576 with a local random password in server.properties only, spawn-protection=0, view-distance 6).
`generator-settings` was changed to a tall flat world (bedrock 1, dirt 127, grass 1: ground near y=65) because
prismarine-viewer cannot render negative Y. The server was stopped gracefully via RCON. Never touch port 25565.
Start: `java -Xmx3G -jar server.jar nogui` in that folder. Reset world: delete `world\`.

## Screenshots (not done)

`tools/viewer/capture.js` (mineflayer spectator + prismarine-viewer + playwright-core). Needs `npm install` in tools/viewer,
env `RCON_PASSWORD`. Finding: prismarine-viewer 1.33 renders nothing for worlds with negative Y (no minY support), hence the tall flat world.
Untested on the tall world yet. No images were produced or faked.


## Update (after the RE docs)

Done since: real reach rule (eye to block box; click support <= 5.0, target box <= 4.5), usable blocks never clicked,
client_information + chunk batch replies, NBT disconnect reasons, teleport relative bits, `run` command with checkpoint repair,
determinism and fuzz tests, bridges and escape pillars, pillar height capped at 4 so every scaffold block stays breakable.

Live results (vanilla 1.21.1, tall flat world, small factory 2408 blocks, plan from `slice`, `run --verify`):
* old client, old plan: 2408/2408 confirmed, 950 s (2.5 blocks/s), 0 kicks, 0 re-placed.
* fixed client, new plan: 2408/2408 confirmed, 748 s (3.2 blocks/s), 0 kicks, 0 re-placed. Throughput per 100 placements:
  9.6 12.4 7.7 10.7 7.7 7.2 3.2 2.8 3.5 3.3 8.1 4.1 2.5 1.3 1.4 1.6 2.2 3.3 3.1 2.4 3.0 2.3 5.1 5.8 (not steady: the middle layers need
  many bridges/pillars).
* Only block counts are checked against the server; exact block-state ids are not compared.

Large factory (22k): `slice` takes 12+ minutes (BFS fallback radius 160) and has not produced a clean plan yet; see the last
commit for the state. Not run live. Screenshots not done (prismarine-viewer cannot render negative Y; tall flat world configured).


## Update 2026-11-28: live pipeline (lead builder)

Work is now split across agents (planner speed, block states, tables, viewer, review). This section covers the live pipeline.

Live, small factory (2408 blocks), local vanilla 1.21.1 (2 GB heap), `slice` then `run --verify`:

| run | code | time | blocks/s | re-placed | missing | state diffs | kicks |
|---|---|---|---|---|---|---|---|
| run4 | continuous walking + sprint, before the review fixes | 722 s | 3.3 | 91 (after a 9.9 s server stall) | 0 | 1 | 0 |
| run5 | after review fixes 5704486/ef54a94 (independent checker) | 635 s | 3.8 | 0 | 0 | 1 (RCON check) | 0 |
| run6 | + place-everything-in-reach batching (7c6ab4d) | 567 s | 4.2 | 0 | 0 | 1 (built-in diff) | 0 |

The one differing state is a log placed with no side face to click yet (`stripped_dark_oak_log[axis=z]` became y);
`slice` lists it as a fallback instead of claiming an exact result. The run4 stall: the log shows "Can't keep up,
9883 ms behind", then "moved too quickly" (queued moves add up). `run` now walks back and re-places right after any
server teleport (covered by `tests/run_mock.rs`; no live teleport happened in run5/run6).

Target of 5 blocks/s is NOT reached. 502 of 567 s are movement (2366 moves, many jumps in the machine layers) and the
curve is not flat: about 19 blocks/s on the first layers, 2 to 5 in the middle. Next lever: a better in-layer tour and
standing-cell choice; sprint is already at vanilla speed.

Large factory: NOT run live. At 7c6ab4d `slice` gives up at a 90 s limit in the deferred-block phase
(22439 placed so far); planner work continues. Screenshots belong to the viewer work.

`run` options: sprint by default (`--walk` disables), `--delay-ms` (default 10), `--resume` (progress file next to the
plan; unit-tested only, not exercised live), `--verify`. Temporary supports are confirmed by block update.
Test server: `scratchpad\mcserver` as above, `-Xmx2G`, below-normal priority; stopped via RCON after run6.


## Update 2026-11-28: large factory built live (3 sessions)

Plan `factory-large` (70,794 steps, 22,481 blocks), vanilla 1.21.1, 2 GB. Session 1: 06:49:09 to 08:00:36, 15,250 placed,
aborted with "repair: cannot get back to the planned position on the ground". Session 2: 08:19:15 to 09:08:56, 7,231
placed, all steps done. Session 3: 09:10:29 to 09:11:38, refit of 6 blocks. Active 7,338 s, 3.1 blocks/s, 0 kicks,
13 blocks re-placed by checkpoint repair, 2 resumes. `verify_rcon.py`: 22,475/22,481 (4 copper aged to exposed_copper,
2 declared log-axis fallbacks); 721,018 non-schematic cells around the build all air.

Causes and fixes
* Slab merge (planner): the `plan_clicks` fallback could click the top of an equal bottom slab. Server: lower slab becomes
  double, target stays air. This, not the spectator "Viewer" that joined 07:57:11 to 07:57:43, explains checkpoint 60:
  the missing blocks were plan steps 40840, 40841, 40880 (and 41007 at checkpoint 61). Fixed in `slice.rs`; `simulate`
  rejects it (`sim_rules::slab_merges`, not in `sim::apply` so old plans still replay). Test `tests/slab_stack.rs`.
* Way back: repair ended on the ground, the planned feet cell was on a bridge about 18 blocks up, `return_to` only walked.
  Now `Site::go_to` searches with scaffold in the current world (`way_back.rs`, mock-mover tests); extra blocks are kept in
  `<progress>.extras`, broken before a step needs the cell, and removed at the end (`extras.rs`).
* Cascade: the cell a merge left empty took the next sea lantern (its click support was air); the checkpoint only checks
  "not air", so three slab/lantern pairs stayed swapped until the final diff. `--refit x,y,z;...` (plan coordinates)
  fixed them (`refit.rs`). Session 2 first used it for the 9 slabs the merges had doubled (old one-pass version).
  Session 3 needed a progress file written by hand at the last step, because a finished run removes it.

Open: checkpoint 11 re-placed 4 blocks (cause unknown; positions were not logged then). Checkpoint repair should also treat
a wrong block type, not only air, as missing. The fixed planner has not been run live. Throughput is still walking-bound
(session 2: 2,439 s of 2,981 s moving). Renders in `docs/live-factory/` come from `tools/viewer/capture.js` (prismarine-viewer,
spectator, after the build); the "Screenshots (not done)" and "Large factory: NOT run live" notes above are out of date.
The test server was stopped via RCON afterwards.
