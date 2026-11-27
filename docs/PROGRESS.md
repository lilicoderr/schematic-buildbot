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
