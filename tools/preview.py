#!/usr/bin/env python3
"""Render a blueprint (.txt) or Sponge schematic (.schem) to an isometric PNG.

Pure standard library (no Pillow). Used to generate the README demo images.

    python tools/preview.py blueprints/cottage.txt -o docs/cottage.png
    python tools/preview.py build.schem --scale 10
"""
import argparse
import gzip
import hashlib
import struct
import sys
import zlib
from collections import Counter

# ---------- loading ----------

def parse_text(src):
    palette = {".": "air", " ": "air"}
    blocks, y, z = [], None, 0
    for raw in src.splitlines():
        line = raw.split("#")[0].rstrip()
        if not line.strip():
            continue
        head = line.split(None, 1)[0]
        if head == "name":
            continue
        if head == "palette":
            _, ch, name = line.split(None, 2)
            palette[ch[0]] = name.strip()
        elif head == "layer":
            y = int(line.split()[1])
            z = 0
        else:
            if y is None:
                raise ValueError("block row before 'layer'")
            for x, c in enumerate(line):
                name = palette.get(c, "air")
                if name != "air":
                    blocks.append((x, y, z, name))
            z += 1
    return blocks


class _Nbt:
    def __init__(self, data):
        self.d, self.p = data, 0

    def take(self, n):
        s = self.d[self.p:self.p + n]
        if len(s) < n:
            raise ValueError("NBT truncated")
        self.p += n
        return s

    def string(self):
        n = struct.unpack(">H", self.take(2))[0]
        return self.take(n).decode("utf-8", "replace")

    def payload(self, tag):
        if tag == 1: return struct.unpack(">b", self.take(1))[0]
        if tag == 2: return struct.unpack(">h", self.take(2))[0]
        if tag == 3: return struct.unpack(">i", self.take(4))[0]
        if tag == 4: return struct.unpack(">q", self.take(8))[0]
        if tag == 5: return struct.unpack(">f", self.take(4))[0]
        if tag == 6: return struct.unpack(">d", self.take(8))[0]
        if tag == 7: return self.take(max(struct.unpack(">i", self.take(4))[0], 0))
        if tag == 8: return self.string()
        if tag == 9:
            elem = self.take(1)[0]
            n = struct.unpack(">i", self.take(4))[0]
            return [self.payload(elem) for _ in range(max(n, 0))]
        if tag == 10:
            out = {}
            while True:
                t = self.take(1)[0]
                if t == 0:
                    return out
                key = self.string()
                out[key] = self.payload(t)
        if tag == 11:
            n = struct.unpack(">i", self.take(4))[0]
            return list(struct.unpack(">%di" % n, self.take(4 * n)))
        if tag == 12:
            n = struct.unpack(">i", self.take(4))[0]
            return list(struct.unpack(">%dq" % n, self.take(8 * n)))
        raise ValueError("unknown NBT tag %d" % tag)


def parse_schem(raw):
    if raw[:2] == b"\x1f\x8b":
        raw = gzip.decompress(raw)
    n = _Nbt(raw)
    tag = n.take(1)[0]
    n.string()
    root = n.payload(tag)
    root = root.get("Schematic", root)
    w, h, l = root["Width"], root["Height"], root["Length"]
    if "Blocks" in root:
        pal, data = root["Blocks"]["Palette"], root["Blocks"]["Data"]
    else:
        pal, data = root["Palette"], root["BlockData"]
    names = {i: s.split("[")[0].removeprefix("minecraft:") for s, i in pal.items()}
    blocks, pos, i = [], 0, 0
    while pos < len(data) and i < w * h * l:
        val = shift = 0
        while True:
            b = data[pos]
            pos += 1
            val |= (b & 0x7F) << shift
            if not b & 0x80:
                break
            shift += 7
        name = names.get(val, "air")
        if name not in ("air", "cave_air", "void_air"):
            blocks.append((i % w, i // (w * l), (i // w) % l, name))
        i += 1
    return blocks


def load(path):
    with open(path, "rb") as f:
        raw = f.read()
    if raw[:2] == b"\x1f\x8b" or path.endswith((".schem", ".schematic")):
        return parse_schem(raw)
    return parse_text(raw.decode("utf-8"))

# ---------- colours ----------

KNOWN = {
    "stone": (125, 125, 125), "cobblestone": (110, 110, 110), "glass": (170, 215, 230),
    "dirt": (134, 96, 67), "grass_block": (95, 159, 53), "sand": (219, 207, 163),
    "oak_planks": (162, 130, 78), "spruce_planks": (114, 84, 48), "birch_planks": (192, 175, 121),
    "oak_log": (109, 85, 50), "bricks": (150, 97, 83), "stone_bricks": (122, 121, 122),
    "white_concrete": (207, 213, 214), "black_concrete": (8, 10, 15), "red_concrete": (142, 33, 33),
    "blue_concrete": (45, 47, 143), "yellow_concrete": (240, 175, 21), "green_concrete": (73, 91, 36),
    "orange_concrete": (224, 97, 1), "pink_concrete": (213, 101, 142), "lime_concrete": (94, 168, 24),
    "gold_block": (246, 208, 61), "iron_block": (220, 220, 220), "diamond_block": (98, 219, 214),
    "glowstone": (171, 131, 84), "obsidian": (20, 18, 30), "netherrack": (97, 38, 38),
}


def colour(name):
    if name in KNOWN:
        return KNOWN[name]
    h = hashlib.md5(name.encode()).digest()
    return tuple(70 + h[i] % 160 for i in range(3))

# ---------- rasteriser ----------

class Canvas:
    def __init__(self, w, h, bg):
        self.w, self.h = w, h
        self.px = bytearray(bytes(bg) * (w * h))

    def poly(self, pts, col):
        ys = [p[1] for p in pts]
        n = len(pts)
        for y in range(max(int(min(ys)), 0), min(int(max(ys)) + 1, self.h)):
            cy = y + 0.5
            xs = []
            for i in range(n):
                (x0, y0), (x1, y1) = pts[i], pts[(i + 1) % n]
                if (y0 <= cy < y1) or (y1 <= cy < y0):
                    xs.append(x0 + (cy - y0) * (x1 - x0) / (y1 - y0))
            if len(xs) >= 2:
                a, b = max(int(min(xs) + 0.5), 0), min(int(max(xs) + 0.5), self.w)
                if b > a:
                    o = (y * self.w + a) * 3
                    self.px[o:o + (b - a) * 3] = bytes(col) * (b - a)

    def png(self):
        raw = b"".join(b"\x00" + bytes(self.px[y * self.w * 3:(y + 1) * self.w * 3]) for y in range(self.h))

        def chunk(t, d):
            c = struct.pack(">I", len(d)) + t + d
            return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

        return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", self.w, self.h, 8, 2, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def shade(c, f):
    return tuple(min(255, int(v * f)) for v in c)


def render(blocks, s=12, margin=24, bg=(28, 32, 44)):
    W, H = s, s / 2  # half diamond width / height; cube height = s

    def proj(x, y, z):
        return (x - z) * W, (x + z) * H - y * s

    corners = [proj(x + dx, y + dy, z + dz) for x, y, z, _ in blocks for dx in (0, 1) for dy in (0, 1) for dz in (0, 1)]
    minx, maxx = min(c[0] for c in corners), max(c[0] for c in corners)
    miny, maxy = min(c[1] for c in corners), max(c[1] for c in corners)
    cv = Canvas(int(maxx - minx + 2 * margin), int(maxy - miny + 2 * margin), bg)
    ox, oy = margin - minx, margin - miny

    def P(x, y, z):
        px, py = proj(x, y, z)
        return (px + ox, py + oy)

    for x, y, z, name in sorted(blocks, key=lambda b: (b[0], b[1], b[2])):
        c = colour(name)
        cv.poly([P(x, y + 1, z), P(x + 1, y + 1, z), P(x + 1, y + 1, z + 1), P(x, y + 1, z + 1)], shade(c, 1.0))
        cv.poly([P(x, y + 1, z + 1), P(x + 1, y + 1, z + 1), P(x + 1, y, z + 1), P(x, y, z + 1)], shade(c, 0.78))
        cv.poly([P(x + 1, y + 1, z), P(x + 1, y + 1, z + 1), P(x + 1, y, z + 1), P(x + 1, y, z)], shade(c, 0.58))
    return cv


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input", help=".txt blueprint or .schem file")
    ap.add_argument("-o", "--output", default="preview.png")
    ap.add_argument("--scale", type=int, default=12, help="pixels per block edge (default 12)")
    a = ap.parse_args()
    blocks = load(a.input)
    if not blocks:
        sys.exit("no blocks to render")
    cv = render(blocks, a.scale)
    with open(a.output, "wb") as f:
        f.write(cv.png())
    top = ", ".join("%s x%d" % kv for kv in Counter(b[3] for b in blocks).most_common(3))
    print("wrote %s (%dx%d px, %d blocks: %s)" % (a.output, cv.w, cv.h, len(blocks), top))


if __name__ == "__main__":
    main()
