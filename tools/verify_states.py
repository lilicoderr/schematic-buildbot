"""Compare the exact block states on a test server with what a plan expects.

usage: RCON_PASSWORD=... python verify_states.py plan.bbplan START_X START_Y START_Z [--port 25576]
START is the feet cell the bot started from (printed by `build-bot run` as "from (x, y, z)"), so
plan coordinates can be translated to world coordinates. For every PLACE / AUTO step it asks the
server `execute if block X Y Z <state>` (props listed in the state must match) over RCON.
The password is read from the environment, never from the repo. Only for local test servers.
"""
import os
import socket
import struct
import sys
from collections import Counter


def rcon_session(port, password):
    s = socket.create_connection(("127.0.0.1", port), timeout=10)

    def send(i, t, body):
        data = struct.pack("<ii", i, t) + body.encode() + b"\x00\x00"
        s.sendall(struct.pack("<i", len(data)) + data)

    def recv():
        head = b""
        while len(head) < 4:
            head += s.recv(4 - len(head))
        n = struct.unpack("<i", head)[0]
        buf = b""
        while len(buf) < n:
            buf += s.recv(n - len(buf))
        return buf[8:-2].decode(errors="replace")

    send(1, 3, password)
    recv()

    def run(cmd):
        send(2, 2, cmd)
        return recv()

    return run


def main():
    plan, sx, sy, sz = sys.argv[1], *map(int, sys.argv[2:5])
    port = int(sys.argv[sys.argv.index("--port") + 1]) if "--port" in sys.argv else 25576
    start = None
    expected = []
    for line in open(plan, encoding="utf-8"):
        if line.startswith("# start "):
            start = tuple(int(v) for v in line.split()[2:5])
        elif line.startswith(("PLACE ", "AUTO ")):
            p = line.split()
            expected.append((int(p[1]), int(p[2]), int(p[3]), p[4]))
    dx, dy, dz = sx - start[0], sy - start[1], sz - start[2]
    run = rcon_session(port, os.environ["RCON_PASSWORD"])
    bad = []
    for x, y, z, state in expected:
        r = run(f"execute if block {x + dx} {y + dy} {z + dz} minecraft:{state.removeprefix('minecraft:')}")
        if "passed" not in r:
            bad.append((x + dx, y + dy, z + dz, state))
    print(f"{len(expected) - len(bad)}/{len(expected)} block states match exactly")
    for name, n in Counter(s.split("[")[0] for *_, s in bad).most_common(10):
        print(f"  differs: {name} x{n}")
    for b in bad[:10]:
        print("  e.g.", b)


if __name__ == "__main__":
    main()
