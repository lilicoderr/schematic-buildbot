# Versions: what is cached and how to diff more

## 1. Cached versions

Only 1.21.1 exists under `%LOCALAPPDATA%\mc-re-cache` (server.jar, server_mappings.txt, version.json, plus work/ with the remapped and decompiled server and reports/).
Facts from the bundled version.json: protocol_version 767, world_version 3955, data pack 48, resource pack 34, Java 21.
1.21 (also protocol 767) shares the packet table; the bot's version.rs already lists it as supported.

Baseline numbers for later diffs (1.21.1):
- Block states: 26684 (ids 0..26683); air 0, stone 1, cobblestone 14, water 80..95, lava 96..111, ladder 4654..4661, snow 5772..5779, soul_sand 5850, honey_block 19445, oak_stairs 2874..2953.
- Items: 1333 registry entries (matches the bot's table exactly).
- Packets: configuration C 17 (0..16), S 8; play C 124 (ids 0..123), S 58 (ids 0..57). Full table in protocol.md.

## 2. Generating another version

All with tools/re (scripts only; jars and outputs stay outside the repo):

1. `pwsh tools/re/fetch.ps1 -Version <id>` downloads server.jar, server_mappings.txt and version.json into `%LOCALAPPDATA%\mc-re-cache\<id>`.
2. Extract the inner jar `META-INF/versions/<id>/server-<id>.jar` from the bundler jar.
3. `python -I tools/re/mojmap_to_tiny.py server_mappings.txt mojmap.tiny`.
4. Remap with tiny-remapper (`official` -> `named`), then decompile with Vineflower.
5. Reports: `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --server --output reports/` (blocks.json, items.json, registries.json, packets.json, commands.json).
6. Diff: compare packets.json per state/direction by name (ids shift), blocks.json by per-state id and property lists, registries.json `minecraft:item` by name -> protocol_id.
   Also read version.json protocol_version and set `PROTOCOL_VERSION` / the bot's version table accordingly.

Note: mojmap names are stable across versions, so class names such as ServerGamePacketListenerImpl can be diffed directly after decompiling.

## 3. What usually changes between releases (from memory, to verify with a real diff)

- Play packet ids shift whenever a packet is added or removed (1.21.2 and 1.21.4 reshuffled clientbound play; 1.21.2 added/renamed several movement-related packets).
- Block state ids shift whenever a block or a property is added (e.g. new wood sets, pale garden blocks, copper variants, creaking heart); item ids shift too.
- Serialization details changed around 1.21.2 to 1.21.5: teleport packet (adds delta movement), move-player packets (extra flags bit for horizontal collision), BlockHitResult (extra worldBorderHit flag),
  player input (bit flags incl. sprint), a `player_loaded` packet, login packet field order, item component encoding (hashed components in later versions), chat checksum byte in chat packets (1.21.5), registry data entries.
- Protocol numbers (wiki, unverified here): 1.21/1.21.1 = 767, 1.21.2/1.21.3 = 768, 1.21.4 = 769, 1.21.5 = 770.

Treat each item as a hypothesis until packets.json and the codec classes of the target version confirm it. The bot's id table is only valid for 767.

## 4. What the planner/simulator must assume

- Everything in this folder is 1.21.1-specific. A version bump requires re-verifying: ids (ids.rs), state ids (anything using numeric states), item ids, teleport/move/use-item layouts, and the movement thresholds in movement.md.

## 5. Open questions

- No second version was available to diff; the items above are not measured.
