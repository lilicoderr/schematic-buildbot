#!/usr/bin/env python3
"""Check a built schematic against a live server over RCON (independent of the bot's own report).

For every block it runs `execute if block X Y Z <state>` and counts passes. Block states from the
schematic (stairs facing, slab type, ...) must match too. Standard library only.

    python tools/verify_rcon.py blueprints/factory-small.schem --origin -5 -60 2
    python tools/verify_rcon.py build.schem --origin 10 -60 10 --port 25576 --password secret
"""
import argparse
import gzip
import os
import socket
import struct
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from preview import _Nbt  # noqa: E402


class Rcon:
    def __init__(self, host, port, password):
        self.s = socket.create_connection((host, port), timeout=15)
        self._send(1, 3, password)
        if self._recv()[0] == -1:
            raise SystemExit("RCON login failed (wrong password?)")

    def _send(self, rid, kind, body):
        data = struct.pack("<ii", rid, kind) + body.encode() + b"\x00\x00"
        self.s.sendall(struct.pack("<i", len(data)) + data)

    def _recv(self):
        n = struct.unpack("<i", self._exact(4))[0]
        buf = self._exact(n)
        return struct.unpack("<i", buf[:4])[0], buf[8:-2].decode(errors="replace")

    def _exact(self, n):
        out = b""
        while len(out) < n:
            chunk = self.s.recv(n - len(out))
            if not chunk:
                raise SystemExit("RCON connection closed")
            out += chunk
        return out

    def run(self, cmd):
        self._send(2, 2, cmd)
        return self._recv()[1]


def load_states(path):
    with open(path, "rb") as f:
        raw = f.read()
    if raw[:2] == b"\x1f\x8b":
        raw = gzip.decompress(raw)
    n = _Nbt(raw)
    tag = n.take(1)[0]
    n.string()
    root = n.payload(tag)
    root = root.get("Schematic", root)
    w, h, l = root["Width"], root["Height"], root["Length"]
    pal, data = (root["Blocks"]["Palette"], root["Blocks"]["Data"]) if "Blocks" in root \
        else (root["Palette"], root["BlockData"])
    states = {i: s for s, i in pal.items()}
    out, pos, i = [], 0, 0
    while pos < len(data) and i < w * h * l:
        val = shift = 0
        while True:
            b = data[pos]
            pos += 1
            val |= (b & 0x7F) << shift
            if not b & 0x80:
                break
            shift += 7
        st = states.get(val, "minecraft:air")
        if st.split("[")[0].removeprefix("minecraft:") not in ("air", "cave_air", "void_air"):
            out.append((i % w, i // (w * l), (i // w) % l, st if st.startswith("minecraft:") else "minecraft:" + st))
        i += 1
    return out, (w, h, l)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("schem")
    ap.add_argument("--origin", nargs=3, type=int, required=True, metavar=("X", "Y", "Z"),
                    help="world position of the schematic's (0,0,0) corner (the bot prints it)")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=25576)
    ap.add_argument("--password", default="buildbot")
    args = ap.parse_args()

    blocks, (w, h, l) = load_states(args.schem)
    ox, oy, oz = args.origin
    rc = Rcon(args.host, args.port, args.password)
    # `execute if block` only sees loaded chunks, and the bot has usually left by now.
    rc.run(f"forceload add {ox} {oz} {ox + w} {oz + l}")
    rc.run(f"execute if block {ox} {oy} {oz} minecraft:air")  # touch the area, let it load

    bad = Counter()
    first_bad = {}
    for k, (x, y, z, st) in enumerate(blocks, 1):
        ok = "passed" in rc.run(f"execute if block {ox + x} {oy + y} {oz + z} {st}")
        if not ok:
            key = st
            bad[key] += 1
            first_bad.setdefault(key, (ox + x, oy + y, oz + z))
        if k % 5000 == 0:
            print(f"  ...{k}/{len(blocks)}", file=sys.stderr)
    rc.run(f"forceload remove {ox} {oz} {ox + w} {oz + l}")

    good = len(blocks) - sum(bad.values())
    print(f"{args.schem}: {good}/{len(blocks)} blocks match the server ({100 * good / len(blocks):.2f}%)")
    for st, n in bad.most_common(10):
        print(f"  MISSING/WRONG x{n}  {st}  e.g. at {first_bad[st]}")
    sys.exit(0 if not bad else 1)


if __name__ == "__main__":
    main()
