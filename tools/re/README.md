# tools/re: reading the rules from Minecraft itself

Scripts only. Jars, mappings and decompiled sources live in a cache outside the repo
(`%LOCALAPPDATA%\mc-re-cache\<version>`) and are never committed; Mojang's code is not distributed.

1. `fetch.ps1 -Version 1.21.1` (or `latest`) downloads the official server jar, and the official Mojang mappings
   where the release has them (26.x ships unobfuscated), into the cache; SHA-1 is checked against `piston-meta.mojang.com`.
2. Data tables from the server's own generator, run in an empty directory:
   `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --server --output out`
   (blocks.json, registries.json, and in newer releases packets.json). The run also unpacks the inner server jar and
   libraries into `versions/` and `libraries/`.
3. Mapped releases: `python -I mojmap_to_tiny.py server_mappings.txt mojmap.tiny`, then
   `java -jar tiny-remapper-fat.jar inner.jar named.jar mojmap.tiny official named <library jars>`.
4. Decompile only what is needed with Vineflower (a Java decompiler is the right tool for a Java server):
   `java -jar vineflower.jar --only=net/minecraft/network --only=net/minecraft/server/network named.jar decompiled/`
   (add a `--only=` per extra class, e.g. `net/minecraft/world/phys/BlockHitResult`).
5. `gen_tables.py` turns the reports into `crates/mc-bot/src/data/v<protocol>.rs` (usage at the top of the script).
   Only these numeric tables with their registry names are committed.

[docs/RE/versions.md](../../docs/RE/versions.md) lists what to compare when adding a release.
