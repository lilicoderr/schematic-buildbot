# Movement validation (Minecraft 1.21.1, protocol 767)

Source: ServerGamePacketListenerImpl (handleMovePlayer, handleAcceptTeleportPacket, teleport, tick,
updateAwaitingTeleport, isPlayerCollidingWithAnythingNew, noBlocksAround), ServerCommonPacketListenerImpl
(keep-alive), Connection / RateKickingConnection, Player / LivingEntity / Entity. Rules are described in
our own words; no Mojang code is reproduced.

## 1. Purpose

The server does NOT simulate the player. It trusts the client's claimed position and only checks
(a) speed per tick, (b) whether the destination box is newly inside solid shapes, (c) an anti-float
counter. Gravity is not applied to a ServerPlayer (travel only runs for locally controlled entities), so
the server-side player velocity stays about zero.

## 2. handleMovePlayer, step by step

1. Packet variants (same handler): Pos (id 26), PosRot (27), Rot (28), StatusOnly (29).
   Fields: doubles x,y,z / floats yRot,xRot / one byte onGround (bit 0). Missing parts default to the server's current value.
2. NaN position or non-finite rotation: disconnect (multiplayer.disconnect.invalid_player_movement).
   Infinite positions pass but are clamped: horizontal +-3.0E7, vertical +-2.0E7. Rotation is wrapped to +-180.
3. If tickCount == 0 the good positions are reset to the player's position.
4. updateAwaitingTeleport: while a teleport is unconfirmed the packet is silently dropped (section 4).
5. Passenger: only rotation applied. Sleeping: if moved more than 1.0 (squared) from firstGood, re-teleport.
6. "Moved too quickly" (skipped if tick rate is frozen, player changing dimension, or singleplayer owner):
   - n = move packets received since the last server tick (knownMovePacketCount is synced to receivedMovePacketCount in tick()). If n > 5 it counts as 1.
   - d2 = squared distance from firstGood (position at the START of this server tick) to the claim; v2 = squared server-side velocity (about 0).
   - Reject if d2 - v2 > 100 * n (300 * n while fall-flying; gamerule disableElytraMovementCheck skips the check while gliding).
     Reject = teleport back to the server position (new teleport id) and return.
   - One packet per tick: up to 10 blocks per tick passes. Two packets in one tick double the allowance.
7. Server re-simulation: delta = claim - lastGood (lastGood = last accepted position this tick, reset to the player position at tick start).
   If the server thinks the player is on ground and the packet says not-on-ground with dy > 0, it calls jumpFromGround (velocity and exhaustion only).
   Then player.move(PLAYER, delta) with normal collision resolution (step height 0.6; edge back-off only if the server-side shift flag is set).
8. Error = claim - resulting server position. The Y component is ALWAYS zeroed (the code tests `y > -0.5 || y < 0.5`, always true),
   so only horizontal error counts. "Moved wrongly" = horizontal error^2 > 0.0625 (0.25 blocks), and only when not changing dimension,
   not sleeping, not creative, not spectator.
9. Accept unless (not noPhysics and not sleeping) and ( (movedWrongly and the OLD bounding box was collision free) or "colliding with anything new" ).
   - Colliding with anything new: take the box at the CLAIMED position deflated by 1.0E-5; every collision shape (blocks, world border, collidable entities)
     in it that does not overlap the player's PREVIOUS box (also deflated 1.0E-5) is "new" and rejects the move. Touching faces never count.
     Shapes already overlapped before the move are tolerated.
   - Rejected: teleport(old position, new rotation); the client must confirm.
10. Accepted: position := the claim (not the simulated result), then
   - clientIsFloating = noBlocksAround && !verticalCollisionBelow (value before the move) && not spectator && allow-flight off && !abilities.mayfly &&
     no levitation && not gliding && not riptide. (The `y >= -0.03125` term is always true because of the zeroing.)
   - onGround := the CLAIMED flag (trusted). Fall distance grows with real downward dy while claimed airborne; a landing claim applies fall damage
     (none when abilities.mayfly, i.e. creative). A claimed upward move resets fall distance.
   - lastGood := new position.

## 3. Constants

| What | Value | Where |
|---|---|---|
| speed threshold per packet | 100 (squared blocks) | handleMovePlayer |
| elytra threshold | 300 | handleMovePlayer |
| burst clamp | more than 5 packets since last tick counts as 1 | handleMovePlayer |
| moved wrongly | 0.0625 squared (0.25 blocks), horizontal only | handleMovePlayer |
| new-collision epsilon | 1.0E-5 deflate | isPlayerCollidingWithAnythingNew |
| floating box | inflate 0.0625, extend down 0.55; all block states must be air | noBlocksAround |
| max flying ticks | ceil(80 * max(0.08 / gravity, 1)) = 80 at default gravity | getMaximumFlyingTicks |
| teleport resend | unconfirmed for more than 20 ticks | updateAwaitingTeleport |
| coordinate clamp | 3.0E7 horizontal, 2.0E7 vertical | clampHorizontal / clampVertical |

## 4. Teleport handshake

- teleport(): stores the pending position, increments awaitingTeleport (first id is 1; wraps to 0 at Integer.MAX_VALUE), records the tick,
  moves the player immediately, sends ClientboundPlayerPosition (play id 64): x,y,z doubles, yRot,xRot floats, relative-flags byte, VarInt id.
  Relative bits: X 0x01, Y 0x02, Z 0x04, Y_ROT 0x08, X_ROT 0x10 (yaw before pitch). The server's own corrections use no relative flags.
- Client answers ServerboundAcceptTeleportation (id 0, VarInt id):
  - id == awaitingTeleport and a position pending: player moved to the pending position (rotation kept), lastGood := it, pending cleared.
  - id == awaitingTeleport but nothing pending (a DUPLICATE confirm of the latest id): DISCONNECT invalid_player_movement.
  - any other id: ignored silently.
- While pending: all move packets are dropped silently; after more than 20 ticks the same position is re-sent with a NEW id (older id no longer matches).
  UseItemOn is also ignored (the server still answers with two block updates, which look like a rejected placement).
- Vanilla follows the confirm with a PosRot at the confirmed spot. Do the same, and never confirm twice.
- The first teleport is sent on join (id 1), right after the Login packet.

## 5. tick(): floating kick, keep-alive, idle, rates

- Each server tick: if clientIsFloating (and not sleeping/passenger/dead) the counter increments; above 80 the player is kicked with
  multiplayer.disconnect.flying ("Flying is not enabled on this server"). Otherwise flag and counter reset. The flag only changes when a move is accepted,
  so a bot that stops sending packets while hovering is still counted.
- Exempt: creative/spectator (abilities.mayfly), allow-flight=true, levitation, elytra, riptide, and any position with a non-air block (water, ladder, vine count)
  within the box inflated 0.0625 and extended 0.55 downward.
- Keep-alive: server sends ClientboundKeepAlive (play 38, config 4; long = millisecond clock) once 15000 ms passed since the last; if the previous is still
  unanswered at that moment: disconnect "Timed out". Serverbound keep-alive (play 24, config 4) must echo the pending id; an unsolicited or wrong id disconnects too.
  Config and play listeners each start their own 15 s clock.
- Netty ReadTimeoutHandler(30): no bytes from the client for 30 s closes the connection.
- Idle kick: only if player-idle-timeout is set (default 0 = off). Reset by actions (chat, swing, block actions, use item, carried-slot change, player command), NOT by movement.
- Packet rate: rate-limit property, default 0 (off). If set, the average of received packets per second (0.25 * last second + 0.75 * old average,
  evaluated every 20 connection ticks) above the limit kicks with disconnect.exceeded_packet_rate.
- Chat/command spam: each chat or command adds 20 to a counter that drops 1 per tick; above 200 and not op: disconnect.spam (about the 11th message in 10 s).
  Creative drop (slot -1) is throttled by a separate counter at 200.

## 6. What a legitimate bot sends

- Required: keep-alive answers (exact echo) and teleport confirms. No position packet is required for staying connected.
- Vanilla behaviour to mimic: Pos/PosRot when moved, a full position at least every 20 ticks, Rot when only turning, StatusOnly when only onGround changed.
- Pace: one move packet per 50 ms. A burst after lag counts as one packet beyond 5, which can trigger moved-too-quickly.
- onGround honest: true on a surface, false while rising/falling.
- Answer each teleport with the exact latest id, once.

## 7. What the planner/simulator must assume

1. Legality is checked at the destination only (new-collision test) plus 10 blocks per tick; the path is not validated in creative.
   In survival the server also compares with its own collision-resolved move (0.25 block horizontal tolerance).
2. The bot's box is 0.6 x 1.8 (collision.md). Every claimed position needs its box free of any block shape not already overlapped, otherwise it is teleported back and everything until the confirm is lost.
3. Creative bot: no moved-wrongly rejection, no fall damage, no floating kick. Survival bot: never stay more than 0.6125 above non-air blocks for 80 ticks.
4. After any teleport stop acting until confirmed; placements during that window silently fail.
5. Walking 4.2 blocks per second is far below the 10 blocks per tick ceiling.

## 8. Open questions

- Plugins or mods on the owner's server may add checks; only vanilla was analysed.
- Client-side input scaling (0.98 input, sprint x1.3, sneak x0.3) is not in the server jar; see collision.md (derived from memory).
