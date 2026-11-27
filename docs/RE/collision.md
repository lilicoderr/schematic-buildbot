# Collision, dimensions and physics constants (Minecraft 1.21.1)

Source: Player (STANDING_DIMENSIONS, POSES, attributes, updatePlayerPose, canInteractWithBlock, maybeBackOffFromEdge),
Entity (move, collide, collideWithShapes, collectCandidateStepUpHeights), LivingEntity (travel, jumpFromGround, getJumpPower,
fall damage), CollisionGetter / BlockCollisions / EntityGetter, Attributes, block classes. Described in our own words.

## 1. Player box and eye height

| Pose | Width x height | Eye height | Notes |
|---|---|---|---|
| standing | 0.6 x 1.8 | 1.62 | box is x,z +-0.3 around the feet position, y from feet to feet+1.8 |
| crouching | 0.6 x 1.5 | 1.27 | set by player command PRESS_SHIFT_KEY (flag), not by PlayerInput, in 1.21.1 |
| swimming / gliding / riptide | 0.6 x 0.6 | 0.4 | |
| sleeping / dying | small boxes | | |

- Pose is recomputed every tick (updatePlayerPose). If the desired pose does not fit, the server falls back to crouching (1.5) and then swimming (0.6):
  in a 1-block high gap the bot's server-side box is 0.6 tall. Fit test = no block/entity collision with the pose box deflated 1.0E-7.
- Reach: block interaction range 4.5 (attribute), entity 3.0. Placement/digging also accept +1.0 leniency: the distance from the eye position to the
  clicked block's unit AABB must be below 5.5. The eye position uses the pose eye height (crouching lowers it by 0.35).
- Step height attribute 0.6 (not modifiable by a vanilla client). Gravity attribute 0.08, jump strength 0.42, safe fall distance 3.0, fall damage multiplier 1.0,
  movement speed 0.1.

## 2. Motion constants (vanilla client / shared code; the SERVER does not run them for a player)

- Jump: velocity.y := 0.42 (times block jump factor; honey 0.5; plus 0.1 per Jump Boost level); sprinting adds 0.2 horizontally in the facing direction.
- Per tick: position += velocity; then velocity.y = (velocity.y - 0.08) * 0.98. Horizontal: velocity.xz *= friction*0.91 on ground (block friction default 0.6 -> 0.546), 0.91 in air.
  Ground acceleration = speed * (0.216 / friction^3) = 0.1 for walking; air acceleration 0.02 (0.026 sprinting).
- Derived (computed, steady state): walking 0.2159 blocks/tick = 4.317 b/s; sprint (x1.3) 5.612 b/s; sneak (x0.3) 1.295 b/s. The 0.98 input scale, sprint
  and sneak factors live in client code (not in the server jar), taken from memory.
- Jump arc height above take-off per tick: 0.4200, 0.7532, 1.0013, 1.1661, 1.2492, 1.2522 (apex, tick 6), 1.1768, 1.0244, 0.7967, 0.4952, 0.1213, then below.
  Max jump about 1.252 blocks: clears 1 block, never 2. Needs 3 ticks to rise above 1.0.
- Free fall from rest: cumulative drop per tick 0, 0.0784, 0.2336, 0.4642, 0.7685, 1.1451, 1.5926, 2.1096, 2.6946, 3.3463 (tick 10), 4.06 (tick 11). Terminal velocity about 3.92 per tick.
- Friction/speed/jump factors: soul sand speed 0.4; honey speed 0.4, jump 0.5; ice and packed ice friction 0.98; blue ice 0.989; slime 0.8.
  The factors apply from the block at the feet and the block 0.500001 below.
- Ladders / vines / scaffolding: horizontal clamp 0.15, fall clamp -0.15, climb 0.2; fall distance reset while on a climbable.
- Water: drag 0.8 (0.9 sprint-swim), vertical 0.8, buoyant jump +0.04/tick; lava drag 0.5.

## 3. Collision algorithm (Entity.move / collide)

1. Movement is resolved axis by axis against all collision shapes in the swept box: Y first, then the smaller horizontal component first (Z if |dx| < |dz|, else X), then the other.
   Shapes.collide treats gaps under 1.0E-7 as zero.
2. Step-up: if step height > 0 and (landed this move or on ground) and horizontal movement was blocked, try candidate rise heights (every shape top edge within
   0.6 above the feet, ascending) and keep the first that gives more horizontal progress. So 0.6 or less (carpets, slabs, snow layers up to 0.5) is walkable without a jump; 1.0 needs a jump.
3. Sneaking on ground (server flag set): horizontal movement is shortened in 0.05 steps so that the box would not move over a drop of at least step height.
4. verticalCollisionBelow = collided while moving down; that sets onGround on the server's own move.
5. "Inside a block" test (noCollision / the new-collision test): iterate blocks in the box expanded by 1 in every direction; for each non-empty collision shape (shape of that block, offset)
   test strict volume overlap with the box. Full cubes are tested by plain AABB intersection. Entities that can be collided with (boats, shulkers, minecarts, ...) count; ordinary mobs and players do not block movement.
   World border counts when the box is near it.
6. Collision shape depends on the context entity: scaffolding (solid only when above / not descending), powder snow (only walkable with leather boots), lava-standing fluids.
   For a plain player: air, water, lava (no collision), tall grass, flowers, torches, rails, buttons, pressure plates, open doors/fence gates (gate open = none) and similar are non-solid.

## 4. Block shapes relevant to standing (pixel = 1/16)

| Block | Collision shape (height above block bottom) |
|---|---|
| full cube | 1.0 |
| slab bottom / top / double | 0.5 (y 0-0.5) / y 0.5-1.0 / 1.0 |
| stairs | two stacked octets: lower half full 0.5 + upper half only on the back side (0.5-1.0 on half the footprint); inner/outer corners use 1 to 3 octets; top-half stairs are mirrored |
| fence, fence gate (closed) | 1.5 (24 px), post 6-10 px wide, arms 6-10 px wide; open gate: none |
| wall | 1.5 (24 px) collision, post 8 px wide (4-12), arms 6 px wide (5-11); visual shape is 14 px |
| carpet / moss carpet | 1 px (0.0625) |
| snow layers | layer n collides with height (n-1)*2 px: layer 1 has NO collision, layer 2 = 2 px, ... layer 8 = 14 px (0.875) |
| soul sand | 14 px (0.875), full footprint |
| honey block | 15 px (0.9375), footprint 1-15 px |
| farmland, dirt path | 15 px |
| cactus | 15 px, footprint 1-15 |
| chest / ender chest | 14 px, footprint 1-15 |
| bed | 9 px (base y 3-9) plus 3 px legs |
| cauldron | outer 16 px with 3 px floor... hollow, treat as full for standing on rim; interior hole 12x12 |
| trapdoor closed | 3 px at bottom or top (y 13-16); open: 3 px wall |
| ladder | 3 px thick plate on the supporting side; climbable |
| bars, glass panes | full height 16 px, 2 px thick posts/arms |
| water, lava, bubble column | none (swimming) |
| lily pad, sea pickle, flowers, torches, rails | tiny/none |

Standing on top of a block: the feet y equals the collision top (or a multiple of the pixel height above the block bottom). A bot planned on integer feet cells can only stand
on tops at exactly y.0 (full cubes, upper slabs, upside-down stairs, double slab, glass, leaves, etc.). Tops at fractions (bottom slab 0.5, stairs 0.5/1.0, carpet 1/16, soul sand 0.875,
snow layers, farmland 0.9375) put the feet at a non-integer y.

## 5. Fall damage

- Fall distance accumulates from real downward dy while claimed airborne. On a landing claim: damage = ceil((fallDistance - 3.0) * 1.0 * fall_damage_multiplier).
- Creative (abilities.mayfly): causeFallDamage returns immediately, no damage at all. Spectators the same.
- Void: y below minBuildHeight - 64 (overworld: -128) deals out-of-world damage that bypasses creative invulnerability.

## 6. What the planner/simulator must assume

1. 0.6 x 1.8 box, eye 1.62 (1.27 crouching). A standing cell needs two free blocks (feet, head) with non-solid collision (slabs/stairs/carpets reduce clearance partially: use shapes, not names).
2. Jump reaches 1.25 blocks: only +1 steps. Steps of at most 0.6 need no jump. Drops: safe up to 3 in survival; free in creative.
3. Fences/walls have 1.5 collision height: they are walls for the bot even though they look 1 high; the bot cannot jump onto them (1.25 < 1.5).
4. Layer-1 snow is walk-through; layer 2..7 are partial steps; layer 8 is 0.875 high (step-up legal). Soul sand 0.875 and honey 0.9375 sink the feet.
5. Placement is refused when the new block's collision shape overlaps any living entity box (including the bot). Strict overlap: touching is fine.
6. Because the server only checks the destination box, a "teleport-like" path (e.g. instant 1.0 block step) is accepted if the final box is free; do not over-model gravity in the legality check,
   but keep the fall ticks if the bot must survive in survival mode.

## 7. Open questions

- Stairs and corner shapes: exact octet table not expanded here; use blocks.json states plus the shape builder if feet-exact standing on stairs is needed.
- Cauldron / scaffolding / powder snow contexts were not traced in full.
