# Block state derivation at placement (1.21.1)

Source: each block class's getStateForPlacement (plus canSurvive / updateShape / setPlacedBy), cross-checked with
reports/blocks.json property names. Rules are described in own words. See placement.md for the shared context
(placed pos P, face F = clicked face, cursor c = hit location relative to P's block, yaw/pitch of the server-side player).

## 0. Vocabulary
- H = player's horizontal direction = Direction.fromYRot(yaw): yaw 0 -> south, 90 -> west, 180 -> north, -90 -> east.
  H.opp = opposite, H.cw / H.ccw = clockwise / counter-clockwise seen from above (north -> east -> south -> west).
- L = nearest looking direction (6-way, from yaw+pitch; pitch < 0 = up). Aim pitch +-90 for pure up/down,
  pitch 0 for horizontal. Using pitch within +-45 deg and yaw at a band centre gives an unambiguous horizontal L.
- LIST = ordered 3 nearest looking directions; if the clicked block was not replaced, the opposite of F is moved to the
  front. So "first candidate" = opposite(F) = direction from P toward the clicked support block.
- cy = cursor y relative to the clicked block (0..1). Note: when the clicked block is not replaced, P = clicked + F, so for
  side faces (F horizontal) cy is relative to the same y as P. For F = UP/DOWN the y rule is not used by slab/stair/trapdoor.
- Items "waterlogged": any waterloggable block gets waterlogged=true iff P holds SOURCE water.
- "needs support" = the state must also pass canSurvive, otherwise the next candidate / FAIL.

## 1. Table: desired property -> click face / cursor / look
(F = click face on the support block; "any" = not used; Y/P = yaw/pitch to send first)

| Family (block) | Property | Rule in code | To obtain value |
|---|---|---|---|
| Stairs | facing | = H | face the direction you want (yaw band centre) |
| Stairs | half | F == DOWN -> top; F == UP -> bottom; side face: cy > 0.5 -> top else bottom | click top face of a block below = bottom; bottom face of block above = top; sides: cursor y 0.25 / 0.75 |
| Stairs | shape | recomputed from neighbours (see 2.1), never from input | cannot be chosen; depends on neighbouring stairs |
| Slabs | type | existing same slab (replaceable) -> double; else F == DOWN -> top; F == UP -> bottom; side: cy > 0.5 top else bottom | same as stairs half; double = place a second slab into a slab cell (see 2.2) |
| Log / pillar (any axis block: logs, wood, stripped, hay, bone, basalt, quartz/purpur pillar, deepslate, froglights, chain (axis), ...) | axis | = axis of F | click an up/down face -> y; east/west face -> x; north/south face -> z. Independent of look |
| Doors | facing | = H | face as wanted |
| Doors | half | lower placed; upper added by setPlacedBy | place at P, needs P+1 replaceable and P.y < maxBuild-1 |
| Doors | hinge | see 2.3 (solid neighbour counting, adjacent door, else cursor) | best control: click the TOP face of the block below with cursor x/z offsets |
| Doors | open/powered | open = powered = redstone signal at P or P+1 | place away from power to get closed |
| Trapdoors | facing, half | F horizontal and clicked block not replaced: facing = F, half = (cy > 0.5 ? top : bottom). Otherwise (F vertical or clicked block replaced): facing = H.opp, half = (F == UP ? bottom : top) | wall-mounted: click side face with cy 0.25 (bottom) or 0.75 (top). Floor/ceiling: click up/down face; face the opposite way you want "facing" |
| Trapdoors | open | = powered at P | |
| Fence gates | facing, in_wall | facing = H; in_wall = true iff a block in the walls tag lies on either side along the axis perpendicular to H | face as wanted |
| Buttons / levers / grindstone (face + facing) | face, facing | iterate LIST: UP -> face = ceiling; DOWN -> face = floor; facing = H (both). Horizontal d -> face = wall, facing = d.opp. First candidate that passes canSurvive wins | click top face of support block -> face = floor (needs support below); bottom face -> ceiling; side face S -> wall with facing = S. facing on floor/ceiling = H (yaw) |
| Wall torch / wall sign / wall banner / wall skull(head) / tripwire hook / ladder / cocoa / wall hanging sign | facing | iterate LIST: first horizontal d whose state survives: facing = d.opp (cocoa: facing = d, not opposite; ladder, hook, wall signs as stated) | click the side face S of a support block -> facing = S (the block faces away from the wall) |
| Standing/wall item pairs (torch etc.) | variant | LIST order with attachment dir DOWN: skip UP; DOWN-> standing (needs support below); horizontal d -> wall variant facing d.opp | click top face -> standing; side face -> wall; bottom face -> fails |
| Chest / trapped chest | facing | = H.opp (faces the player) | face away from where the front should point... i.e. facing X needs H = X.opp |
| Chest | type | 2.4 | |
| Ender chest, furnace/smoker/blast furnace, loom, stonecutter, carved pumpkin/jack-o-lantern, glazed terracotta, lectern, chiseled bookshelf, end portal frame, repeater, comparator | facing | = H.opp | |
| Campfire, fence gate, decorated pot, bell(floor/ceiling) | facing | = H | |
| Anvil | facing | = H.cw | |
| Beds | facing | = H; head cell = P + H must be replaceable and inside world border | foot placed at P, head at P+H by setPlacedBy |
| Piston / sticky piston, dispenser, dropper, barrel, command blocks | facing (6-way) | = opposite of L (faces the player) | look toward the opposite of the wanted facing: to get facing X, aim at X.opp |
| Observer | facing | = L (the observing face points away from player; output side toward player) | aim in the wanted direction |
| Hopper | facing | = opposite(F), but if that is vertical -> down | click side face S of the block you want to feed: facing = S.opp (points into that block); top/bottom face -> down |
| End rod | facing | = F, unless the clicked block is an end rod already facing F, then F.opp | click face = direction the rod points |
| Lightning rod, amethyst cluster/buds, shulker box | facing | = F | click face = facing |
| Crafter | orientation | front = opposite(L); top = H.opp when front is down, H when up, else UP | |
| Jigsaw | orientation | front = F; top = (F vertical ? H.opp : UP) | |
| Bell | attachment/facing | F vertical: floor (F UP) / ceiling (F DOWN), facing = H; F horizontal: double_wall if both opposite neighbours on that axis are sturdy toward it else single_wall, facing = F.opp; falls back to floor/ceiling by support | |
| Skulls/banners/signs (standing, 16-step rotation) | rotation | segment = floor(yaw*16/360 + 0.5) & 15 (skull: yaw as is; sign/banner: yaw+180) | set yaw: skull rotation r -> yaw = r*22.5; sign/banner r -> yaw = r*22.5 - 180 |
| Ceiling hanging sign | rotation/attached | see source: rotation from yaw, attached unless chain-supported | |
| Lantern | hanging | LIST vertical only: UP candidate -> hanging; DOWN -> standing; must survive | click bottom face of a ceiling block -> hanging; top face -> standing |
| Vines / glow lichen / sculk vein (multiface) | face booleans | add the first LIST direction (vine: not DOWN) that has attachable support; existing vine in the cell is extended | click side face S: vine face = S.opp (touches the block clicked); top-face click of a ceiling support = up face |
| Fences, panes, iron bars, walls | north/east/south/west (+ up for walls) | from neighbours at placement (2.5); never from input | not directly choosable |
| Redstone wire | north/east/south/west | from neighbours (2.6) | not choosable |
| Rails | shape | start: east_west if H is east/west else north_south; then RailState rearranges using neighbouring rails (2.7) | face along desired axis; curves/slopes only via neighbours |
| Powered/detector/activator rail | shape | same but never curves | |
| Torch (standing), pressure plates, carpets, etc. | none | default state | |
| Candles / sea pickle / turtle egg / snow layers | count | placing into an existing same block adds one (max 4 / 4 / 8) | click the existing block (non-sneaking for the first three) |
| Big dripleaf | facing | = H.opp, or copies the dripleaf/stem below | |
| Small dripleaf | facing, half | facing = H.opp; lower placed, upper via setPlacedBy | |
| Pointed dripstone | vertical_direction (TIP_DIRECTION), thickness | tip dir = opposite of pitch-vertical (look up -> tip down) if a support exists on the other side, else flipped; thickness from neighbouring dripstone, sneaking affects merge | |
| Leaves | persistent | placed leaves are persistent = true, distance recomputed | |
| Scaffolding | distance, bottom | computed from surroundings | |
| Note block | instrument | from the block below at placement/updates | note itself via later interaction |
| Cake, flower pot, cauldron, composter | | default state | |

(Property names per reports/blocks.json: pointed_dripstone uses `vertical_direction` + `thickness`; chest `type`
single/left/right; bed `part` head/foot, `occupied`; end_portal_frame `eye`; bell `attachment`
floor/ceiling/single_wall/double_wall; crafter/jigsaw `orientation` e.g. up_north, north_up; lantern `hanging`;
stairs `shape` straight/inner_left/inner_right/outer_left/outer_right; walls `none/low/tall` per side + `up`.)

## 2. Details for the hard families

### 2.1 Stairs shape (StairBlock.getStairsShape; recomputed on every horizontal neighbour update)
Let f = facing, h = half. Look at the block in FRONT (P+f): if it is a stair with the same half and its facing is on the
other axis, and the cell on the side it points away from ("canTakeShape" check: P + that.opp is not an identical stair)
is OK -> OUTER corner: outer_left if front.facing == f.ccw, else outer_right. Otherwise look BEHIND (P+f.opp): same-half
stair on the other axis -> INNER: inner_left if back.facing == f.ccw else inner_right (subject to its own canTakeShape test).
Otherwise straight. The shape is a pure function of the final neighbour layout (re-evaluated whenever a horizontal
neighbour changes), so it cannot be forced independently. To avoid unintended corners keep adjacent same-half stairs
parallel (same facing) or separated by non-stairs; to get a corner, place two stairs with perpendicular facings.
Both orders produce the same result, because the earlier one is re-shaped by the update.

### 2.2 Slabs
Placed: bottom or top as in the table (waterlogged from source water). A slab can be added to an existing SINGLE slab of
the same item: the clicked slab is replaceable when (type bottom and (F == UP, or cursor y > 0.5 on a horizontal face))
or (type top and (F == DOWN, or cy <= 0.5 on a horizontal face)); then the result is DOUBLE at the clicked cell.
If the clicked cell is not a replaceable slab, then a neighbouring (placed pos) single slab of the same item is replaceable
unconditionally and becomes double. A double slab cannot be replaced.
Rule of thumb for a full-block-looking pair: place bottom slab, then click its top face with the same slab.

### 2.3 Door hinge (DoorBlock.getHinge)
Inputs: left cells = P + H.ccw (lower and upper), right cells = P + H.cw (lower and upper). score = -1 for each of the two
LEFT cells whose collision shape is a full block, +1 for each of the two RIGHT cells that is a full block.
Also: leftDoor = left lower cell is a door's lower half; rightDoor likewise.
- If (leftDoor and not rightDoor) or score > 0 -> hinge RIGHT.
- Else if (rightDoor and not leftDoor) or score < 0 -> hinge LEFT.
- Else (tie, no doors): by cursor, using cursor coordinates relative to P (the placed pos, NOT the clicked block).
  With step (sx,sz) of H and cursor (cx,cz): result is RIGHT if (sx<0 and cz<0.5) or (sx>0 and cz>0.5) or
  (sz<0 and cx>0.5) or (sz>0 and cx<0.5); otherwise LEFT. I.e. facing south: x<0.5 -> right, x>=0.5 -> left;
  north: x>0.5 -> right; east: z>0.5 -> right; west: z<0.5 -> right.
Caveat: clicking a side face of a support block shifts the reference (P differs from the clicked block by 1 in x or z), so
cursor values are interpreted against P. Cleanest: click the TOP face of the block under P and use cursor x/z directly.
A solid wall/door neighbour decides first, the cursor only breaks ties.

### 2.4 Chests
Default facing = H.opp, type single. Non-sneaking: if the cell at P + facing.cw (or ccw) holds a SINGLE chest with
the SAME facing, the new chest becomes left/right (merges). Sneaking: no auto-merge, except when clicking a horizontal
face of an existing single chest: the new chest takes that chest's facing and the matching left/right type to merge
along the clicked wall (only if the clicked chest's facing axis differs from the click axis). Neighbour updates also
re-type the existing chest. To avoid an accidental double chest, sneak and do not click a chest face.
Note chests (a usable block) require sneaking to be placed against; non-sneak clicking opens them instead.

### 2.5 Fences / panes / bars / walls
Each side boolean is computed from the 4 horizontal neighbours at placement and again on every neighbour update:
- Fence: side true if the neighbour has a sturdy face toward the fence and is not an exception (leaves, barrier, carved
  pumpkin, jack-o'-lantern, shulker boxes, etc. per Block.isExceptionForConnection), OR is a fence of the same material
  class (wooden vs nether brick), OR a fence gate whose axis fits.
- Pane/bars: sturdy neighbour face (not an exception), OR another pane/bars, OR a wall.
- Wall: side is none/low/tall: connects like panes + walls + fence gate; low/tall decided by whether the block above
  covers the arm shape; `up` (post) decided by shouldRaisePost (post if above wall has post, or the arm layout is not a
  straight line, or a wall-post-override tag block above). Updated when the block above changes.
All of this is derived data: the bot cannot set them, but final states are a deterministic function of the neighbourhood,
so a simulator can compute them from the final layout (order independent).

### 2.6 Redstone wire
Placement state = connections to neighbours (side at same level, up over a conductive block with wire on top, down
step), with a dot if none; a lone connection is extended to a straight line. Power value defaults to 0 and is later
recomputed. Fully neighbour-derived.

### 2.7 Rails
Initial shape from H axis (east_west for H east/west else north_south), then onPlace asks RailState to re-evaluate
using neighbouring rails at the same level or one above/below: connects to neighbours, curves (normal rail only) when
two neighbours are perpendicular, ascending when a rail exists one block higher in the run direction. Without
neighbouring rails the result is the straight axis shape. Powered/detector/activator rails never curve. Powered state
comes from redstone later.

## 3. States that cannot be obtained by one placement (need neighbours or later updates)
- stairs.shape; fence/pane/bars/wall connections and wall.up; redstone wire connections/power; rail shapes beyond the
  straight axis; chest left/right (needs a partner chest); double slab (needs an existing slab); door hinge when a solid
  block or door is adjacent (forced by the rule); candle/pickle/egg/snow counts > 1 (repeat placements);
  leaves.distance/persistent (always persistent=true when player-placed); scaffolding distance/bottom; note-block instrument;
  doors/trapdoors/gates open/powered (come from redstone); lit/powered/triggered/charges (runtime state);
  waterlogged (depends on a source water at P); bed occupied, furnace lit, etc.
- Properties that require a second block: bed head half, door/plant upper half (added automatically by setPlacedBy).
- Anything not producible may still be forced in creative by a BLOCK_STATE item component (placement.md section 7).

## 4. What the planner/simulator must assume
1. Compute the target state with the family rules above; choose F by support geometry, yaw from H or rotation, pitch only
   for 6-way blocks (L) and for ordering ties; choose cursor y (and x/z for doors) away from the 0.5 boundaries
   (use 0.25 / 0.75).
2. For attached blocks the click face fixes the facing through the support direction: wall torch/sign/button/ladder/hook
   facing = clicked face (cocoa and 6-way look-based blocks differ); candidates after the first only apply if the clicked
   support is unsuitable.
3. 6-way look-facing blocks (piston, dispenser, observer, barrel, crafter, command block) need pitch +-90 for vertical,
   else yaw only with pitch within +-45 deg. The face clicked does not matter for them (but must be a valid support).
4. Order of placement matters only for attached blocks and for neighbour-derived states; the latter converge to a function
   of the final layout.
5. Sneak when the support is usable (chest, door, lever, crafting table, ...) or when clicking chests to place a chest.
6. Always make sure P is replaceable and entity-free (see placement.md).

## 5. Open questions
- Exact semantics of canTakeShape corner edge cases for stairs in 3-stair clusters (rule transcribed, not simulated).
- Wall arm tall/low shape tables (NORTH_TEST etc. voxel constants) were not extracted; a simulator should assume tall when
  a full block sits above the arm, low otherwise, and verify.
- Hanging-sign rotation/attached logic only summarised.
- Which blocks consume the click as a "use" (needs sneak) was not enumerated.
- Blocks not listed here (about 60 more classes with getStateForPlacement: coral, fire, multiface variants, bamboo, kelp,
  chorus, hanging roots, farm/dirt path conversions, sculk sensors, vault, heavy core, etc.) were inspected only by name.
