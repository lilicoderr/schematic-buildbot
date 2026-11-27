#!/usr/bin/env python3
"""Generate the stress-test factory as a Sponge .schem (deterministic, stdlib only).

    python tools/gen_factory.py --size large -o blueprints/factory-large.schem
    python tools/gen_factory.py --size small -o blueprints/factory-small.schem

large ~100x60x48 blocks (tens of thousands of blocks); small is a CI-sized variant.
"""
import argparse
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "factory"))
import hall  # noqa: E402
import yard  # noqa: E402
from schem_writer import Grid  # noqa: E402

PARAMS = {
    "large": dict(
        hx=64, hz=40, wh=12, office=True,
        yx0=65, yx1=103, yz1=43,
        ox0=70, ox1=100, oz1=16, pipe_z=18,
        stack_x=[72, 82, 92], stack_h=[58, 50, 44], stack_z=22,
        tank_x=[74, 86, 98], tank_z=34, tank_r=5, tank_h=14,
        gx0=26, gx1=38,
    ),
    "small": dict(hx=24, hz=16, wh=8, office=False, gx0=8, gx1=16),
}


def build(size):
    g = Grid()
    p = PARAMS[size]
    hall.build(g, p)
    yard.build(g, p)
    return g


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--size", choices=sorted(PARAMS), default="large")
    ap.add_argument("-o", "--out", default="factory.schem")
    args = ap.parse_args()
    w, h, l, n = build(args.size).to_schem(args.out)
    print(f"{args.out}: {w}x{h}x{l}, {n} blocks")


if __name__ == "__main__":
    main()
