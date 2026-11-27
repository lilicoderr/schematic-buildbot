# tools/re: reading the rules from Minecraft itself

Scripts only. Jars, mappings and decompiled sources live in a cache outside the repo
(`%LOCALAPPDATA%\mc-re-cache\<version>`) and are never committed; Mojang's code is not distributed.

1. `fetch.ps1 -Version 1.21.1` downloads the official server jar and official Mojang mappings
   (from `piston-meta.mojang.com`) into the cache.
2. Extract the inner jar `META-INF/versions/<v>/server-<v>.jar` from the bundler jar.
3. `python -I mojmap_to_tiny.py server_mappings.txt mojmap.tiny` converts the mappings to Tiny v2.
4. Remap with tiny-remapper (`java -jar tiny-remapper-fat.jar in.jar out.jar mojmap.tiny official named`).
5. Decompile with Vineflower (a Java decompiler is the right tool for a Java server; Ghidra is for
   native binaries): `java -jar vineflower.jar out.jar decompiled/`.
6. Data tables from the server's own generator:
   `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --server --output reports/`
   (blocks.json, registries.json, packets.json).

Steps 2 to 6 run third-party or downloaded jars, so they are run by the maintainer, not by the agent
session (the session's permission policy refused to execute them).
