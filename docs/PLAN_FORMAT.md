# Build plan format (`.bbplan`, version 1)

A plan is a text file: header lines starting with `#`, then one step per line. It is produced
offline by `build-bot slice`, checked by `build-bot simulate` and replayed by `build-bot run`.
The same input always produces a byte-identical file.

## Header

| line | meaning |
|---|---|
| `# bbplan 1` | format version (required, first line) |
| `# schematic <hex>` | 64-bit fingerprint of the schematic the plan was cut from |
| `# origin x y z` | world cell of schematic position (0,0,0) |
| `# protocol 767` | Minecraft protocol the plan was calibrated for |
| `# stat <key> <value>` | statistics: blocks, steps, supports, pillars, est_seconds, ... |
| `# unreachable x y z <block>` | schematic block no legal standing position reaches (not placed) |

## Steps

Cells are integer block coordinates; a *feet cell* is the block the player's feet are in.

| step | meaning |
|---|---|
| `MOVE x y z` | go to a feet cell: one horizontal step, a jump up exactly 1, or a drop of 1 to 3 |
| `PILLAR x y z` | standing in that cell: jump, put a scaffold block into the cell, land on it (feet end at y+1) |
| `PLACE x y z <state> <yaw or -> <click>...` | place the block state; each click is `sx,sy,sz,face,cx,cy,cz` (support block, clicked face, cursor inside the face) |
| `BREAK x y z` | break one block; only scaffold may be broken (creative: instant). The player falls if it stood on it |
| `CHECKPOINT n count hash` | exactly `count` schematic blocks stand, with order-independent `hash` |

## Legality (enforced by `simulate`, which `slice` must pass before writing a plan)

* the player always stands on a block that can carry them, with free feet and head cells;
* a `PLACE` target is free, within reach of the eye, not inside the player, and is clicked against an
  existing block (or the ground);
* `BREAK` only removes scaffold; at the end no scaffold remains;
* the final world equals the schematic except for the blocks listed as unreachable.
