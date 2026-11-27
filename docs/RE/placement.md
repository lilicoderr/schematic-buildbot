# Block placement (use-item-on) in vanilla 1.21.1 (protocol 767)

Source: official-mapped, decompiled server (own-words description; no Mojang code reproduced).
Classes: ServerGamePacketListenerImpl.handleUseItemOn, ServerPlayerGameMode.useItemOn, ItemStack.useOn,
BlockItem (useOn / place / canPlace / placeBlock), UseOnContext, BlockPlaceContext, Player.canInteractWithBlock,
EntityGetter.isUnobstructed, CollisionGetter.isUnobstructed, BlockBehaviour.canBeReplaced.
Packet ids (reports/packets.json, play serverbound): use_item_on = 56, set_creative_mode_slot = 50,
player_command = 37, set_carried_item = 47.

## 1. Packet layout (ServerboundUseItemOnPacket)
hand (VarInt enum: 0 main, 1 off) | BlockPos (long) | Direction enum (VarInt: down, up, north, south, west, east) |
cursor x,y,z (3 x float, each RELATIVE to the clicked block pos) | inside (bool) | sequence (VarInt).
The server rebuilds the absolute hit location as blockpos + float cursor (FriendlyByteBuf.readBlockHitResult).

## 2. Server algorithm, in order (handleUseItemOn)
1. Runs on the server thread. ackBlockChangesUpTo(sequence) is called FIRST, unconditionally. It keeps the max
   sequence seen (must be >= 0, else an exception kills the connection). The ack packet
   (ClientboundBlockChangedAck) is sent at the START of the connection's next tick(). It is sent whether or not
   the placement succeeded: an ack means "processed", NOT "placed".
2. The item in the chosen hand must be feature-enabled (else silent no-op, no resync).
3. Reach check: Player.canInteractWithBlock(clickedPos, 1.0): the squared distance from the player's EYE POSITION
   to the nearest point of the clicked block's unit AABB [pos, pos+1] must be < (blockInteractionRange + 1.0)^2.
   - Attribute BLOCK_INTERACTION_RANGE default 4.5 (Player.DEFAULT_BLOCK_INTERACTION_RANGE).
     ServerPlayer.updatePlayerAttributes adds a transient +0.5 (ADD_VALUE, id creative_mode_block_range) while the
     player is in creative and removes it otherwise (once per player tick, so a gamemode change takes effect next tick).
   - => survival: distance to block box < 5.5; creative: < 6.0. (The vanilla client itself uses 4.5 / 5.0.)
   - Eye position = (x, y + eyeHeight, z) of the server-side player. Eye height: standing 1.62, crouching 1.27,
     swimming / fall-flying / spin-attack 0.4. The server uses its own last-known position and pose, not the packet.
   - Failing this check is completely silent (no log, no block resync). The ack is still sent.
4. Cursor sanity check: |hit - center(clickedBlock)| must be < 1.0000001 on EACH axis (cursor offset roughly in
   (-0.5000001, 1.5000001) per axis). The cursor is NOT checked against the clicked face or the block's shape, and
   there is no line-of-sight or raycast check. Failing: a warning is logged, nothing else happens.
   Consequence: any cursor in [0,1]^3 with any face value is accepted. Face and cursor are trusted independently.
5. Build height: clickedPos.y must be < level.getMaxBuildHeight() (top block y = max-1). Otherwise an action-bar
   message build.tooHigh is sent, but the resync below still happens. The CLICKED block's y is checked; clicking the
   top face of y = max-1 passes this test but setBlock then fails (target outside build height).
6. If awaitingPositionFromClient != null (the server sent a teleport and has not received the teleport-confirm yet)
   or ServerLevel.mayInteract(player, clickedPos) is false, the placement is skipped silently (the resync still runs
   for the mayInteract case). mayInteract = not under spawn protection AND clicked pos inside the world border.
   Spawn protection (DedicatedServer.isUnderSpawnProtection): only overworld, only if the ops list is non-empty,
   player is not op, radius > 0, and Chebyshev (max of |dx|,|dz|) distance of clicked pos to world spawn <= radius.
7. ServerPlayerGameMode.useItemOn(...):
   a. The clicked block must be feature-enabled, else FAIL. Spectators only open menus, never place.
   b. secondary use = player shift-key-down AND (main hand OR offhand non-empty).
   c. If NOT secondary use: BlockState.useItemOn(stack, ...) runs first; then, if it returns "pass to default" and the
      hand is MAIN, BlockState.useWithoutItem(...) runs. If either "consumes the action", the interaction ends THERE
      and nothing is placed. This is what happens when right-clicking doors, trapdoors, fence gates, buttons, levers,
      chests, barrels, crafting tables, furnaces, beds, anvils, note blocks, lecterns, etc. without sneaking.
      => the bot must be sneaking (PlayerCommand PRESS_SHIFT_KEY) when the clicked support block is "usable".
      (Some blocks consume specific items in useItemOn: cauldrons, flower pots, composter, cake+candle...)
   d. Else, if the stack is non-empty and the item is not on cooldown: ItemStack.useOn(UseOnContext). In creative the
      stack count is saved and restored afterwards. In adventure mode (abilities.mayBuild false) the item must carry
      a can_place_on predicate matching the CLICKED block, else PASS.
8. After all of that (still inside the cursor-ok branch, even if placement failed) the server sends
   ClientboundBlockUpdate for the clicked pos AND for clickedPos.relative(face). A rejected placement therefore shows
   up as the old state being re-sent at the target position.
9. SUCCESS swings the arm (animation packet back); FAIL/PASS do not.

## 3. Player rotation is NOT in the packet
handleUseItemOn carries no yaw/pitch. All look-dependent states use the server-side player's CURRENT yaw/pitch,
updated only by move-player packets (Pos+Rot / Rot). The bot must send a rotation packet BEFORE the use_item_on
(packets are processed in order, so ordering is enough). (Contrast: the plain use_item packet carries its own
yaw/pitch.) Sneak is server-side state from player_command packets; the pose/eye-height change is applied by the
server's player tick, so wait about 1 tick after PRESS_SHIFT_KEY if reach is marginal.

## 4. UseOnContext / BlockPlaceContext
- BlockPlaceContext computes relativePos = clickedPos.relative(face) and replaceClicked =
  clickedState.canBeReplaced(context).
- getClickedPos(): the CLICKED pos if replaceClicked, else the relative pos. This is the actual placement
  position everything below uses ("placed pos" P).
- canPlace(): replaceClicked OR the target state canBeReplaced(context).
- getHorizontalDirection() = Direction.fromYRot(player yaw) = index floor(yaw/90 + 0.5) & 3 -> 0 south, 1 west,
  2 north, 3 east. Bands: yaw in [-45,45) south, [45,135) west, [135,225) north, [225,315) east (the lower edge
  belongs to the next index). Aim at band centres (0, 90, 180, -90).
- getNearestLookingDirection() = first of Direction.orderedByNearest(player): the axis with the largest |component|
  of the look vector (x: |sin(yaw)|cos(pitch), y: |sin(pitch)|, z: |cos(yaw)|cos(pitch)), sign from the look vector.
  Positive pitch = looking down. Pure look direction; ignores the clicked face. Tie-breaks follow fixed branch order.
- getNearestLookingDirections(): the same ordered 3-element list, but when !replaceClicked the OPPOSITE of the clicked
  face is moved to the front. So for "attached" blocks, the first candidate is the support block that was clicked.
- getNearestLookingVerticalDirection(): UP if pitch < 0 else DOWN.
- getRotation(): raw player yaw (float).
- isSecondaryUseActive(): shift key down.

## 5. BlockItem.place steps (BlockItem.useOn -> place)
1. Block feature-enabled, else FAIL.
2. context.canPlace() else FAIL.
3. updatePlacementContext (identity; scaffolding overrides it).
4. getPlacementState: block.getStateForPlacement(ctx); null => FAIL. Then canPlace(ctx, state) requires
   - (if mustSurvive, default true) state.canSurvive(level, P) AND
   - level.isUnobstructed(state, P, CollisionContext.of(player)).
   Any false => FAIL (nothing placed, nothing consumed).
5. placeBlock: level.setBlock(P, state, flags 11 = UPDATE_NEIGHBORS | UPDATE_CLIENTS | UPDATE_IMMEDIATE).
   setBlock returns false if P is outside the build height OR the chunk reports no change (placing a state identical
   to what is already there) => FAIL.
6. If the block at P is still the expected block: apply the item's BLOCK_STATE component (direct setBlock with
   flag 2), BLOCK_ENTITY_DATA, then block.setPlacedBy(...) (2nd half of doors/beds/tall plants, repeater tick
   scheduling, ...), advancement trigger, sound, game event, consume one item.
7. Return SUCCESS (server side).

### 5.1 Item -> block mapping
A BlockItem holds exactly one block; Block.byItem(item) returns it. Most items share the block's registry name.
Exceptions: redstone -> redstone_wire, string -> tripwire, wheat_seeds -> wheat, beetroot_seeds -> beetroots,
carrot -> carrots, potato -> potatoes, melon_seeds -> melon_stem, pumpkin_seeds -> pumpkin_stem,
cocoa_beans -> cocoa, nether_wart -> nether_wart, torchflower_seeds -> torchflower_crop, pitcher_pod -> pitcher_crop,
sweet_berries -> sweet_berry_bush, glow_berries -> cave_vines, powder_snow_bucket -> powder_snow (the item becomes an
empty bucket afterwards).
Standing + wall pairs (StandingAndWallBlockItem; the item picks the variant): torch, soul_torch, redstone_torch,
all skulls/heads, all coral fans, all signs (SignItem), hanging signs, banners. DoubleHighBlockItem: all doors, tall
flowers/grass/fern, small_dripleaf (clears the cell above to air/water with flags 27, places the lower half, and the
upper half is added in setPlacedBy). BedItem: placeBlock uses flags 26 (2|8|16, no neighbour updates) and the head half
is added in BedBlock.setPlacedBy. PlaceOnWaterBlockItem (lily_pad, frogspawn): useOn is PASS, they only work through the
plain use-item packet with a fluid raycast. ScaffoldingBlockItem: mustSurvive false; clicking an existing scaffolding
searches along a direction (up; or player facing if the top was clicked; sneaking: clicked face, or opposite if
`inside`) for up to 7 horizontal steps for the first replaceable cell, and fails if the support distance would be 7.
GameMasterBlockItem (command blocks, structure block, jigsaw): placement itself is plain; block-entity data needs op.
reports/items.json has no block mapping; derive it from this rule plus name equality.

## 6. "Can place here" rules
### 6.1 Replaceable
BlockState.canBeReplaced(ctx) -> Block.canBeReplaced(state, ctx): the state's `replaceable` flag AND
(hand empty OR hand item != this block's own item). `replaceable` is set (Blocks.java) on exactly:
air, cave_air, void_air, water, lava, short_grass, fern, dead_bush, seagrass, tall_seagrass, fire, soul_fire,
snow (layers), vine, glow_lichen, light, tall_grass, large_fern, structure_void, bubble_column, warped_roots,
nether_sprouts, crimson_roots, hanging_roots.
Overrides: slabs (see block_states.md), snow layers (with snow item and layers < 8: replaceable if the clicked block is
the target only when face == UP, else always; with any other item only when layers == 1), candles / sea pickles /
turtle eggs (non-sneaking, same item, count < 4 -> replace = add one).
When the clicked block is replaceable, placement happens AT the clicked block whatever face was clicked (the face is
still used for face-dependent states). Otherwise at clickedPos + face.
Waterlogging: set from the target's fluid state only if it is a SOURCE water block (Fluids.WATER). Flowing water is a
different fluid type and does not waterlog. Non-waterloggable blocks simply replace fluid.

### 6.2 Entity obstruction (isUnobstructed)
Evaluated only when the new state's collision shape (computed with CollisionContext.of(player)) is non-empty.
It passes `null` as the entity, so NOTHING is excluded, including the placing player. Any non-removed entity with
flag blocksBuilding whose bounding box strictly overlaps the shape blocks placement. blocksBuilding is true for:
every LivingEntity (players incl. the bot itself, all mobs, villagers, bats), non-marker armor stands, end crystals,
falling blocks, primed TNT, minecarts, boats, striders. Spectators are ignored. NOT blocking: item entities, XP
orbs, projectiles, paintings/item frames, marker armor stands, display entities.
Creative mode gets no exemption. Touching without overlap does not block (1e-7 tolerance). Player box is 0.6 wide x 1.8
tall (1.5 crouching), centred on x,z. Blocks with an empty collision shape (torches, buttons, levers, rails, signs,
...) skip the entity test. StandingAndWall items test the final state with CollisionContext.empty() (still null entity).

### 6.3 canSurvive (checked once before placing, unless mustSurvive is false)
Default BlockBehaviour.canSurvive = true. Common overrides (placed pos P):
- Standing torch: block below supports the CENTER of its top face (Block.canSupportCenter). Wall torch / tripwire hook /
  ladder / cocoa style: block behind has a sturdy face toward the new block.
- Buttons, levers, grindstones: the connected block is sturdy on the touching face.
- Rails, repeater, comparator, redstone wire: block below has a RIGID top face (wire also accepts a hopper).
  Pressure plates: RIGID top or CENTER support.
- Carpet: block below is not air. Doors / lower double plants: below sturdy top (UP); upper half needs its lower half.
- Lantern: CENTER support from the block above (hanging) or below. Vine / multiface: see block_states.md.
- Signs, banners, bells, saplings, crops, cactus, sugar cane etc. have their own rules; the common case is "needs a
  solid neighbour on side S". Query per block when exact.
After placement, neighbour updates may delete a block that no longer survives (updateShape returns air) and it drops as an
item. Placement order therefore matters for attached blocks.

### 6.4 updateShape / neighbour effects of a placement
setBlock flags 11 notify neighbours with shape updates: fences/walls/panes recompute connections, stairs recompute
shape, chests merge, redstone wire reconnects, rails re-shape, leaves distance, etc. A later neighbour placement can
CHANGE the state of an earlier-placed block (see block_states.md, "cannot be obtained by one placement").

## 7. Creative inventory
- ServerboundSetCreativeModeSlot(slot: short, stack). Honoured only if the player is in creative. A slot is accepted
  for storing when 1 <= slot <= 45 (menu indices; hotbar 0..8 = menu slots 36..44, offhand = 45), and the stack count
  <= the item's max stack size (or empty). slot < 0 means "drop the item" (rate limited by a counter, limit 200, +20
  per drop). The item must be feature-enabled. The inventory menu broadcasts the change.
- Then select the hotbar index with ServerboundSetCarriedItem(0..8) and right-click. Hand MAIN reads the selected slot,
  OFF the offhand slot.
- In creative placement never consumes the item.
- A BLOCK_STATE component on the creative stack is applied AFTER placement via a direct setBlock (flag 2) and may
  override ANY valid property of the placed block. This is a way to force states that placement rules cannot produce.
  Whether the owner's server accepts custom components from a creative client is not verified.

## 8. What the planner/simulator must assume
1. Reach: eye -> nearest point of the clicked (support) block's cube < 5.5 (survival) / 6.0 (creative); use eye height
   1.62 standing / 1.27 sneaking; keep a margin (e.g. 0.2).
2. Sneak when the clicked support block has a use action; otherwise the click becomes a block interaction (e.g. a door
   toggles) and nothing is placed.
3. Face and cursor are free parameters; no raycast. Cursor must stay within roughly [-0.5, 1.5] of the block.
4. Send a rotation packet before the click; the server reads yaw/pitch from its last stored state.
5. Placement cell = clicked cell if replaceable, else cell + face. The cell must be replaceable (or match the slab /
   layer merge rules) and no blocksBuilding entity (bots, players, mobs, armor stands) may overlap the new collision
   shape - including the bot itself: step away from cells overlapping its own 0.6 x 1.8 box.
6. A failed placement is silent (only block-update resyncs plus the ack). Verify by watching the block update at the
   target position; the sequence ack proves processing only.
7. The ack arrives at the next server tick start; all packets queued before it are processed first.
8. Teleports lock placement until the teleport-confirm has been received.
9. The build-height check uses the clicked block's y; world border / spawn protection use the clicked pos only.
10. Placing a state identical to the present one fails (no-change setBlock).

## 9. Open questions
- Exact build heights of the owner's dimensions (overworld assumed 320; not read from the dimension type data here).
- Spawn-protection radius on the target server (server.properties).
- The set of blocks whose useItemOn / useWithoutItem consumes the action was not enumerated for every block.
- Server acceptance of custom data components in creative slots depends on server settings/permissions.
