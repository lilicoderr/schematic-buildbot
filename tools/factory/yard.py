"""Factory yard: office block, smokestacks, storage tanks, pipeline, loading gantry."""
import math


def yard(g, p):
    g.fill(p["yx0"], 0, 0, p["yx1"], 0, p["yz1"], "gray_concrete")
    for x in range(p["yx0"], p["yx1"] + 1, 6):  # hazard stripes
        g.set(x, 0, 18 if p["office"] else 1, "yellow_concrete")


def office(g, p):
    x0, x1, z1 = p["ox0"], p["ox1"], p["oz1"]
    storeys, sh = 3, 5
    top = storeys * sh
    for y in range(1, top + 1):
        for x in range(x0, x1 + 1):
            for z in (0, z1):
                band = y % sh in (2, 3)
                g.set(x, y, z, "glass" if band and (x - x0) % 4 else "white_concrete")
        for z in range(1, z1):
            for x in (x0, x1):
                band = y % sh in (2, 3)
                g.set(x, y, z, "glass" if band and z % 4 else "white_concrete")
    for s in range(1, storeys):
        g.fill(x0 + 1, s * sh, 1, x1 - 1, s * sh, z1 - 1, "smooth_stone_slab[type=bottom]")
    g.fill(x0 - 1, top + 1, -1, x1 + 1, top + 1, z1 + 1, "smooth_stone_slab[type=bottom]")
    g.box(x0 - 1, top + 2, -1, x1 + 1, top + 2, z1 + 1, "light_gray_concrete")
    g.fill(x0 + 3, 1, 0, x0 + 5, 3, 0, "air")  # entrance
    g.fill(x0 + 2, 4, 0, x0 + 6, 4, 0, "orange_concrete")
    for s in range(storeys - 1):  # internal stair run, one step per block
        g.fill(x0 + 2, (s + 1) * sh, 2, x0 + 5, (s + 1) * sh, 3, "air")  # stairwell hole
        for i in range(sh):
            x = x0 + 2 + i
            if i:
                g.fill(x, s * sh + 1, 2, x, s * sh + i, 3, "stone_bricks")
            for z in (2, 3):
                g.set(x, s * sh + 1 + i, z, "stone_brick_stairs[facing=east,half=bottom]")
    for s in range(storeys):  # ceiling lamps
        for x in range(x0 + 4, x1 - 2, 6):
            g.set(x, (s + 1) * sh - 1, z1 // 2, "sea_lantern")


def stack(g, cx, cz, height, base="white_concrete"):
    g.fill(cx - 3, 1, cz - 3, cx + 3, 2, cz + 3, "gray_concrete")
    for y in range(3, height + 1):
        mat = "red_concrete" if (y // 4) % 2 else base
        if y >= height - 1:
            mat = "black_concrete"
        for dx in range(-2, 3):
            for dz in range(-2, 3):
                if max(abs(dx), abs(dz)) == 2:
                    g.set(cx + dx, y, cz + dz, mat)


def tank(g, cx, cz, r, h):
    for y in range(1, h + 1):
        for dx in range(-r - 1, r + 2):
            for dz in range(-r - 1, r + 2):
                d = math.hypot(dx, dz)
                if r - 0.6 <= d <= r + 0.4:
                    band = y in (1, h // 2, h)
                    g.set(cx + dx, y, cz + dz, "iron_block" if band else "white_concrete")
    for k in range(r):  # shallow dome made of slabs
        rr = r - k
        for dx in range(-rr, rr + 1):
            for dz in range(-rr, rr + 1):
                if rr - 1 <= math.hypot(dx, dz) <= rr:
                    g.set(cx + dx, h + 1 + k // 2, cz + dz, "smooth_quartz_slab[type=bottom]")
    g.set(cx, h + 1 + r // 2, cz, "sea_lantern")


def pipeline(g, p):
    z, y = p["pipe_z"], 9
    for x in range(p["hx"] + 1, p["yx1"] - 1):
        g.set(x, y, z, "copper_block")
        if x % 5 == 0:
            g.fill(x, 1, z, x, y - 1, z, "iron_block")
        if x % 10 == 0:
            g.set(x, y + 1, z, "iron_bars")


def gantry(g, p):
    x0, x1, z0 = p["gx0"], p["gx1"], p["hz"] + 2
    for x in (x0, x1):
        for z in (z0, z0 + 5):
            g.fill(x, 1, z, x, 9, z, "iron_block")
    for x in range(x0, x1 + 1):
        for z in (z0, z0 + 5):
            g.set(x, 10, z, "yellow_concrete")
    for z in range(z0, z0 + 6):
        for x in (x0, x1):
            g.set(x, 10, z, "yellow_concrete")
    g.fill(x0 + 1, 0, z0 + 1, x1 - 1, 0, z0 + 4, "black_concrete")


def build(g, p):
    if not p["office"]:
        gantry(g, p)
        return
    yard(g, p)
    office(g, p)
    for cx, h in zip(p["stack_x"], p["stack_h"]):
        stack(g, cx, p["stack_z"], h)
    for cx in p["tank_x"]:
        tank(g, cx, p["tank_z"], p["tank_r"], p["tank_h"])
    pipeline(g, p)
    gantry(g, p)
