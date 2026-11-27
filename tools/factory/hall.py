"""Main production hall: floor, walls, gable roof, trusses, catwalk, conveyors, lighting."""

WALL = "stone_bricks"
PILLAR = "iron_block"
FLOOR = "smooth_stone"
ROOF_STAIR = "stone_brick_stairs"
ROOF_BODY = "dark_oak_planks"
BEAM = "stripped_dark_oak_log"


def floor(g, p):
    g.fill(0, 0, 0, p["hx"], 0, p["hz"], FLOOR)


def walls(g, p):
    hx, hz, wh = p["hx"], p["hz"], p["wh"]
    win_lo, win_hi = wh // 2, wh - 3
    for y in range(1, wh + 1):
        for x in range(hx + 1):
            for z in (0, hz):
                pillar = x % 8 == 0
                window = win_lo <= y <= win_hi and 2 <= x % 8 <= 6
                g.set(x, y, z, PILLAR if pillar else ("glass" if window else WALL))
        for z in range(1, hz):
            for x in (0, hx):
                pillar = z % 8 == 0
                window = win_lo <= y <= win_hi and 2 <= z % 8 <= 6
                g.set(x, y, z, PILLAR if pillar else ("glass" if window else WALL))
    gate_lo = hx // 2 - 4
    for z in (0, hz):  # truck gates in both long walls
        g.fill(gate_lo, 1, z, gate_lo + 8, min(6, wh - 2), z, "air")
        g.fill(gate_lo - 1, min(7, wh - 1), z, gate_lo + 9, min(7, wh - 1), z, "yellow_concrete")


def roof(g, p):
    """Gable roof along x: a 2-thick stair/plank band per side, skylights, ridge slab."""
    hx, hz, wh = p["hx"], p["hz"], p["wh"]
    half = hz // 2
    for x in range(-1, hx + 2):
        sky = x % 8 in (2, 3, 4, 5) and 0 <= x <= hx
        for k in range(half):
            y = wh + 1 + k
            for z, face in ((k, "south"), (hz - k, "north")):
                if 8 <= k <= half - 4 and sky:
                    g.set(x, y, z, "glass")
                    g.set(x, y - 1, z, "glass")
                    continue
                g.set(x, y, z, f"{ROOF_STAIR}[facing={face},half=bottom]")
                if k:
                    g.set(x, y - 1, z, ROOF_BODY)
        g.set(x, wh + 1 + half, half, "stone_brick_slab[type=bottom]")
        g.set(x, wh + half, half, ROOF_BODY)


def trusses(g, p):
    hx, hz, wh = p["hx"], p["hz"], p["wh"]
    half = hz // 2
    for x in range(8, hx, 8):
        for z in range(1, hz):
            g.set(x, wh, z, f"{BEAM}[axis=z]")
        for z in range(hz // 4, hz - hz // 4 + 1, max(hz // 4, 4)):
            top = wh + min(z, hz - z)
            for y in range(wh + 1, top):
                g.set(x, y, z, f"{BEAM}[axis=y]")
        for z in range(hz // 4, hz, hz // 2):
            g.set(x, wh - 1, z, "sea_lantern")
        g.set(x, wh - 1, half, "sea_lantern")


def catwalk(g, p):
    hx, hz, wh = p["hx"], p["hz"], p["wh"]
    y = max(wh * 2 // 3, 4)
    zc = hz // 2
    x_start = y + 3
    for i in range(y - 1):  # solid stair ramp up to the walkway
        x = 4 + i
        if i:
            g.fill(x, 1, zc - 1, x, i, zc + 1, WALL)
        for z in (zc - 1, zc, zc + 1):
            g.set(x, 1 + i, z, "stone_brick_stairs[facing=east,half=bottom]")
    for x in range(x_start, hx - 1):
        for z in range(zc - 2, zc + 3):
            g.set(x, y, z, "smooth_stone_slab[type=bottom]")
        for z in (zc - 2, zc + 2):
            g.set(x, y + 1, z, "iron_bars")
        if x % 8 == 0:
            for z in (zc - 2, zc + 2):
                g.fill(x, 1, z, x, y - 1, z, PILLAR)


def conveyors(g, p):
    hx, hz, wh = p["hx"], p["hz"], p["wh"]
    for z in (hz // 4, hz - hz // 4):
        for x in range(4, hx - 3):
            g.set(x, 1, z, "smooth_stone_slab[type=bottom]")
            g.set(x, 1, z - 1, "black_concrete")
            g.set(x, 1, z + 1, "black_concrete")
            if x % 4 == 0:
                g.set(x, 2, z, "yellow_concrete")
        for x in range(10, hx - 8, 14):  # machines straddling the belt
            g.box(x, 1, z - 2, x + 4, 5, z + 2, "gray_concrete")
            g.set(x + 2, 3, z - 2, "orange_stained_glass")
            g.set(x + 2, 3, z + 2, "orange_stained_glass")
            g.fill(x + 1, 6, z - 1, x + 3, 6, z + 1, "iron_block")
            g.set(x + 2, 7, z, "sea_lantern")
            g.fill(x + 2, 1, z - 2, x + 2, 2, z - 2, "air")  # feed opening


def build(g, p):
    floor(g, p)
    walls(g, p)
    trusses(g, p)
    roof(g, p)
    catwalk(g, p)
    conveyors(g, p)
