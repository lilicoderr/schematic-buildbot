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


## Update 2026-11-28 (later): exact-state checks, movement, one clean large-factory run

Newest state; older "open" notes above that concern these points are done.

* `run` (`audit.rs`, `ledger.rs`, `refit.rs`, `exec_repair::heal`): expected state = plan state with stair shape /
  connections / lone chest half recomputed from the blocks standing now. A position is judged when `block_changed_ack`
  (new packet id `cb_play_block_changed_ack`) covers the clicks sent before it was marked (3 s fallback if a server never
  acks, 30 s otherwise). Wrong = missing, other block, stray inside the plan box +2, or a placement-controlled property
  not declared as fallback; other differences are only reported. Max 3 repairs per position. Tests:
  `tests/exact_states_mock.rs` (wrong block, slab merge, neighbour change, stray), `tests/run_mock.rs`, shared `tests/mock`.
* Checkpoint 11 of the old run (4 re-places): segment 11 is 250 plain `gray_concrete`, no slab merge, no server lag or
  movement warning in the server log. Replaying the old plan up to CHECKPOINT 11 on a fresh server with the new build:
  2750/2750, 0 repairs, 0 diffs. NOT reproduced, cause unknown (old logs have no positions). One unproven candidate: the
  old code judged after a fixed 300 ms without waiting for the server; the new one waits for the ack and logs positions.
* Movement (offline, large factory): 32,405 -> 32,321 moves; estimated walking 6,542 s (old 8-tick jumps) -> 6,177 s,
  mostly from 6-tick jumps (`motion::jump_ticks`) over 3,494 jumps; `shorten.rs` saves 84 moves. Scaffold unchanged
  (1,030 pillars, 7,918 breaks). Limits in `tests/slice_scaling.rs`. Tried and rejected (more moves, or scaffold left
  standing): strip / hole-avoiding tour, count-all standing score, time-weighted standing search, other LOOKAHEAD,
  WINDOW, SLACK, TIDY_BELOW, band height and bridge / pillar costs. A real gain needs less scaffold: 7,056 moves lead to
  a BREAK and 4,607 to a SUPPORT; the cleanup is fragile when parameters change.
* Live, small factory with this build: 2408/2408, 539 s (was 567 s), 0 re-placed, 0 kicks, 1 diff (declared fallback).
* Live, large factory, ONE run from an empty world, no resume, no viewer: 10:13:48 to 12:08:46, 22,481/22,481, 6,898 s,
  3.3 blocks/s, walking 6,076 s, 0 kicks, 2 repairs (aged copper), no "moved too quickly" or "Can't keep up" in the
  server log. `verify_rcon.py`: 22,479/22,481 (2 declared log-axis fallbacks), 37/37 copper unaged at check time, 721,018
  cells around the build all air. World origin 7 65 -5. Log: `scratchpad/large/run4.log` (outside the repo). The test
  server was stopped via RCON.



## Update 2026-11-28: tile-and-band build order (upper-layer walking)

Measured first (old plan, large factory; walking attributed to the layer being built): of about 6,180 s walking,
4,370 s went into getting to the next block (23,123 moves) and 1,810 s into cleanup (9,191 moves). Scaffold was mostly
bridges, not pillars: 6,720 bridge blocks against 1,030 pillars. Layers 8 to 30 had roughly 300 to 1,050 blocks each,
in far-apart structures (the two roof slopes, trusses, stacks), but cost 480 to 1,100 approach moves per layer, so whole
layers meant crossing between structures high up on every layer. Over the whole build: 5,341 standing spots.

Change (`tour.rs`): finish a band of 4 layers inside one 16x16 tile before the next tile; scaffold more than 1 layer
below a new band is taken down (`TIDY_BELOW`). In `slice.rs`, a block with no standing spot or support chain is now
deferred to the last pass instead of being declared unreachable at once.

| large factory, offline | before | after |
|---|---|---|
| moves | 32,321 | 21,197 |
| estimated walking | 6,177 s | 4,058 s |
| estimated total | 6,993 s | 4,721 s (-32%) |
| standing spots | 5,341 | 4,006 |
| bridge blocks / pillars / breaks (all) | 6,720 / 1,030 / 7,918 | 4,630 / 693 / 5,498 |
| cleanup moves | 9,198 | 5,782 |

Simulation clean, unreachable 0, no scaffold left, two plans byte-identical, 6 declared log-axis fallbacks (the old order had
2; an earlier note here said 2).
Small factory offline: 547 -> 468 s (-14%; moves 2,289 -> 1,968, breaks 512 -> 277).
Small factory LIVE (vanilla 1.21.1, empty world): 539 s -> 464 s (-14%, 5.2 blocks/s), 2408/2408, 0 re-placed, 0 kicks,
no movement warnings; `verify_rcon.py` 2407/2408 (the declared log), 83,496 cells around it all air. The large factory
was NOT run live with this order.

Cleanup is still parameter-sensitive: several other band / tile sizes ended with scaffold that could not be removed
(the slicer then refuses the plan; in one case the leftovers appeared to be inside a closed room, not proven). On the
large factory, 4/16 with TIDY_BELOW 0, 1 or 2, and 5/16 and 4/14 with TIDY_BELOW 1, planned fully (estimates 4,649 to
4,921 s). A schematic that hits this needs a
cleanup that takes scaffold out before a room closes. The test server was stopped via RCON.


## Update 2026-11-28: large factory live with the tile-and-band order

Code e8bb08d, vanilla 1.21.1, 2 GB, fresh world, ONE run, no resume, no viewer: 12:50:15 to 14:08:20, 54,764 steps.

| large factory, live | previous clean run | tile-and-band |
|---|---|---|
| time | 6,898 s | 4,685 s (-32%) |
| blocks/s | 3.3 | 4.8 |
| moves / walking | 32,321 / 6,076 s | 21,197 / 4,000 s |
| pillar / support / place / break | 198 / 103 / 339 / 144 s | 131 / 72 / 340 / 93 s |
| repairs / kicks / resumes | 2 / 0 / 0 | 1 / 0 / 0 |
| exact states (`verify_rcon.py`) | 22,479 / 22,481 | 22,475 / 22,481 |

Offline estimate was 4,721 s total, 4,058 s walking. 22,481 placed, 0 refused, 0 missing. The one repair was a copper block
aged to exposed_copper (plan 92,73,21); the way back added 4 blocks of extra scaffold, all taken down at the end. No
"moved too quickly" / "moved wrongly" / "Can't keep up" in the server log. 721,018 non-schematic cells around the build all
air; 37/37 copper unaged at check time. World origin 10 65 -2. All 6 differing states are the 6 log-axis fallbacks that
`slice` declares (4 more than the old order). No new renders: same schematic, `docs/live-factory/` still represents it.
Log: `scratchpad/large/tile/run.log` (outside the repo). The test server was stopped via RCON.

Open: get the 4 extra log-axis fallbacks back (place those logs while a side face of the right axis exists); 5 blocks/s is
not reached on the large factory.


## Update 2026-11-28: no more default-state fallbacks

All 7 fallbacks (6 large, 1 small) were pillar-block axes with no clickable face of that axis at placement time (some
had no neighbour yet, others had one that could not be clicked from the standing cell). `support::aim` now clicks a
standing block (scaffold included) or places a temporary support chain ending exactly in the needed neighbour cell,
removed right after (ordinary SUPPORT / BREAK steps, so the strict simulator checks them). `slice.rs` tries up to
`AIM_TRIES` other standing cells before deferring. Deferring alone was tried first and rejected: the large estimate
rose to 4,878 s (21,882 moves). `refit.rs` (live repair) uses the same helper; that path was not triggered live.

| offline, before -> after | large | small |
|---|---|---|
| default-state fallbacks | 6 -> 0 | 1 -> 0 |
| moves | 21,197 -> 21,197 | 1,968 -> 1,968 |
| estimated walking | 4,058 -> 4,058 s | 413 -> 413 s |
| estimated total | 4,721 -> 4,721 s | 468 -> 468 s |
| scaffold breaks | 5,498 -> 5,506 | 277 -> 279 |

All six repo blueprints: unreachable 0, fallbacks 0, strict simulation passes (`tests/exact_axes.rs`, also a lone
x / z log on the ground and on a pillar; the large one runs only with `--release`). Large and small plans are
deterministic. Small factory LIVE (vanilla 1.21.1, empty world): 464 s, 2408/2408, 0 re-placed, 0 kicks, no movement
warnings; `verify_rcon.py` 2408/2408 exact, 83,496 cells around it all air. World origin 3 65 8. Log:
`scratchpad/axisfix/small-run.log` (outside the repo). The test server was stopped via RCON. The large factory was NOT
rebuilt live. This closes the "4 extra log-axis fallbacks" item above.


## Update 2026-11-28: large factory live, exact (22,481 / 22,481)

Newest state. Code df859e8 (planner of 48ee50e, no code change in this update; CI for df859e8 green). Vanilla 1.21.1,
2 GB, below-normal priority, test world deleted first, ONE run of `run large.bbplan 127.0.0.1:25566 --verify`, no
resume, no viewer: 14:34:20 to 15:51:46, 54,780 steps.

* 22,481 placed, 0 refused, 0 missing, 4,646 s (4.8 blocks/s), 21,197 moves, walking 3,979 s (offline estimate 4,721 /
  4,058 s), pillar / support / place / break 130 / 72 / 339 / 93 s. 0 kicks, all 90 checkpoints 0 re-placed, no
  extra scaffold, no resync, 0 "server-reported states differ". No "moved too quickly" / "moved wrongly" /
  "Can't keep up" in the test server log.
* `verify_rcon.py`: 22,481 / 22,481 exact. Surroundings: 721,018 non-schematic cells, 0 not air (box from y=65 up, so
  the ground below is not checked). Copper: 37 / 37 `copper_block`. World origin -3 65 -8.
* The scratch wrapper crashed when it read the verifier's output (piped Python prints the Korean user path in cp949, read
  as UTF-8) and stopped the test server. The server was restarted on the saved world (no reset, no bot) and checked
  about a minute after the build, with `PYTHONIOENCODING=utf-8`.
* Gamerules unchanged (`randomTickSpeed` 3). Copper: the checks treat an aged block as a wrong block and place it again
  (seen in earlier runs; this run had none to repair). Copper judged at the final audit can still age afterwards, so
  for a repeatable exact check of a test build `randomTickSpeed 0` would be the option; not needed or used here.
* Refit / temporary-aim live repair and `--resume` were not exercised (nothing needed them). No new renders (same
  schematic). Logs (outside the repo): `scratchpad/livefinal/run.log`, `verify.log`. Test server stopped via RCON.

This supersedes the "large factory was NOT rebuilt live" note above. Still open: 5 blocks/s on the large factory; the
unexplained checkpoint 11 re-places of the oldest run (not seen since).


## Update 2026-11-28: 1.20.4, 1.21.4 and 26.3 re-verified with the current planner

Code 7d7ce4c, no code change. All repo blueprints except factory-large, plus the 9-high wall and 3-chest scratch
blueprints: every state exact over RCON on all three, small factory 2408/2408 (466 / 464 / 466 s). Details in
[RE/versions.md](RE/versions.md). No bot bug found. The one failed attempt was the scratch harness: it left
`white-list` out of the 26.3 server.properties, so every login was refused. Logs and harness (outside the repo):
`scratchpad/reverify/`. Test servers on 127.0.0.1:25567 / 25577, one at a time, each stopped via RCON.


## Update 2026-11-28: banded build order, large factory live at 8.0 blocks/s

Newest state. Target was 5.5 blocks/s on the large factory.

Measured on the previous plan: 11,712 of 21,197 moves were level moves leading to a placement, 4,400 led to a BREAK,
4,300 to a SUPPORT / PILLAR. Cause: a band of 4 layers was still finished layer by layer inside each tile, so the bot
walked along the same walls once per layer. Cutting corners in level walks was estimated at only about 95 s and not done.
Walking speed, jumps and the executor are unchanged.

Change (`tour.rs`, `batch.rs`, `slice.rs`): `Order::BANDED` = 8x8 tiles, nearest-neighbour order in 3D inside a tile's
4-layer band, and any block of the oldest pending block's band that is ready and in reach is placed before walking. If
that order fails or leaves blocks out, `slice` plans again with `Order::LAYERED` (the previous order). No repo
blueprint needs the second try; a test plans the small factory with it. In `exact_states_mock.rs` two fault
injection points moved (they depend on the placement order).

| offline, previous -> new | large | small |
|---|---|---|
| moves | 21,197 -> 11,581 | 1,968 -> 876 |
| estimated walking | 4,058 -> 2,244 s | 413 -> 181 s |
| estimated total | 4,721 -> 2,875 s (-39%) | 468 -> 237 s (-49%) |
| scaffold pillars / breaks | 693 / 5,506 -> 631 / 4,901 | 44 / 279 -> 48 / 249 |

All six repo blueprints: strict simulation passes, unreachable 0, default-state fallbacks 0, state mismatches 0. Large
plan sliced twice: byte-identical.

Tried (offline, scratch knobs, not committed): 8x8 tiles with the old layered order alone gave 3,644 s (large). With upward-only batching, band 5
gave 2,856 s, but some of its neighbouring settings and band 6 left scaffold that could not be removed; 4 was kept.
Cleanup is still parameter-sensitive (other failures: standing-score slack 8 and count-all standing score at band 5, band
4 with upward-only batching and TIDY_BELOW 0).

Live (vanilla 1.21.1, 2 GB, below-normal, empty world each time, no viewer, no resume; `--verify`):

| live | previous | new |
|---|---|---|
| small factory | 464 s, 5.2 blocks/s | 235 s, 10.3 blocks/s, 2408/2408 exact |
| large factory | 4,646 s, 4.8 blocks/s | 2,822 s, 8.0 blocks/s, 22,481/22,481 exact |

Both runs: 0 refused, 0 re-placed (all checkpoints), 0 kicks, 0 differing states, no "moved too quickly" / "moved
wrongly" / "Can't keep up" in the server log, `verify_rcon.py` exact right after the build, surroundings all air (83,496
and 721,018 cells). Large: 16:56:42 to 17:43:44, 43,954 steps, move 2,186 s / 11,581, pillar 119 s, support 64 s, place
338 s, break 83 s; 37/37 copper unaged at check time; world origin 3 65 -2. Logs (outside the repo):
`scratchpad/speed/{small,large}/run.log`. Test server stopped via RCON. Not re-run live with this order: 1.20.4, 1.21.4,
26.3. No new renders (same schematic).


## Update 2026-11-28: banded order verified live on 1.20.4, 1.21.4 and 26.3

Code `2d5146e`, same harness and checks as the earlier re-verification (fresh flat server per release, `run --verify`,
then RCON). All three: factory-small 2408/2408 against the schematic (235 / 235 / 236 s), cottage 25/25, stair-tower
51/51, state-lab 27/27, pixel-heart 27/27, 9-high wall 45/45, 3 chests 3/3; direct-mode stair-tower 51/51 (block updates
only). 0 refused, 0 re-placed, 0 missing, 0 kicks, surroundings all air, no movement or lag warning. No bug found. Not
covered: large factory, `--resume`, live repair paths. Logs: `scratchpad/reverify/` (older ones in `prev-7d7ce4c/`). This
supersedes the "not re-run live" line above.


## Update 2026-11-28: one command, release v0.2.0

Newest state. `build-bot build <file-or-URL> <host:port>` (`bin/build_bot/build_*.rs`, `fetch.rs`, `preflight.rs`, `job.rs`),
`build.bat` (`build --ask`), `.github/workflows/release.yml` (on `v*` tags). v0.2.0 assets (Windows zip, Linux tar.gz,
SHA256SUMS) exist; checksums verified and the Windows zip passed a dry run; the Linux binary only ran in the workflow.
Live (vanilla 1.21.1, small factory): `build` from a local http URL, survival detected then switched, bot killed at about
40%, rerun resumed: 2408/2408 exact (`verify_rcon.py`), 83,496 surrounding cells air, 0 kicks; `--origin` with
stair-tower (51/51, bot check). Online mode, unsupported version, cannot connect, refused login: mock tests only
(`tests/build_preflight.rs`). Logs: `scratchpad/buildlive/`. Test server stopped via RCON.
Open: Actions warn that the Node 20 actions (checkout / upload / download-artifact v4) are deprecated.
