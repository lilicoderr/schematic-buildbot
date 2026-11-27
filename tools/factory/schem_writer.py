"""Minimal Sponge .schem (v2) writer: sparse block grid -> gzip NBT. Standard library only."""
import gzip
import struct

DATA_VERSION_1_21_1 = 3955


def _s(text):
    b = text.encode("utf-8")
    return struct.pack(">H", len(b)) + b


def _named(tag, name, payload):
    return bytes([tag]) + _s(name) + payload


def _varint(n):
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        out.append(b | (0x80 if n else 0))
        if not n:
            return bytes(out)


class Grid:
    """Sparse 3D block map; coordinates are (x, y, z) with y up."""

    def __init__(self):
        self.cells = {}

    def set(self, x, y, z, state):
        if state in (None, "air"):
            self.cells.pop((x, y, z), None)
        else:
            self.cells[(x, y, z)] = state

    def fill(self, x0, y0, z0, x1, y1, z1, state):
        for x in range(min(x0, x1), max(x0, x1) + 1):
            for y in range(min(y0, y1), max(y0, y1) + 1):
                for z in range(min(z0, z1), max(z0, z1) + 1):
                    self.set(x, y, z, state)

    def box(self, x0, y0, z0, x1, y1, z1, state):
        """Hollow box: only the outer shell."""
        for x in range(min(x0, x1), max(x0, x1) + 1):
            for y in range(min(y0, y1), max(y0, y1) + 1):
                for z in range(min(z0, z1), max(z0, z1) + 1):
                    edge = x in (x0, x1) or y in (y0, y1) or z in (z0, z1)
                    if edge:
                        self.set(x, y, z, state)

    def bounds(self):
        xs, ys, zs = zip(*self.cells)
        return (min(xs), min(ys), min(zs)), (max(xs), max(ys), max(zs))

    def to_schem(self, path):
        (x0, y0, z0), (x1, y1, z1) = self.bounds()
        w, h, l = x1 - x0 + 1, y1 - y0 + 1, z1 - z0 + 1
        palette = {"minecraft:air": 0}
        data = bytearray()
        for y in range(y0, y1 + 1):
            for z in range(z0, z1 + 1):
                for x in range(x0, x1 + 1):
                    st = self.cells.get((x, y, z))
                    key = "minecraft:air" if st is None else "minecraft:" + st
                    if key not in palette:
                        palette[key] = len(palette)
                    data += _varint(palette[key])
        body = b"".join([
            _named(3, "Version", struct.pack(">i", 2)),
            _named(3, "DataVersion", struct.pack(">i", DATA_VERSION_1_21_1)),
            _named(2, "Width", struct.pack(">h", w)),
            _named(2, "Height", struct.pack(">h", h)),
            _named(2, "Length", struct.pack(">h", l)),
            _named(3, "PaletteMax", struct.pack(">i", len(palette))),
            _named(10, "Palette", b"".join(
                _named(3, k, struct.pack(">i", v)) for k, v in palette.items()) + b"\x00"),
            _named(7, "BlockData", struct.pack(">i", len(data)) + bytes(data)),
        ]) + b"\x00"
        raw = bytes([10]) + _s("Schematic") + body
        with gzip.GzipFile(path, "wb", mtime=0) as f:  # mtime=0 keeps output reproducible
            f.write(raw)
        return w, h, l, len(self.cells)
