"""Convert Mojang's proguard-style mappings to Tiny v2 (official -> named) for tiny-remapper.

usage: python -I mojmap_to_tiny.py server_mappings.txt out.tiny
"""
import re
import sys

PRIM = {"int": "I", "long": "J", "boolean": "Z", "byte": "B", "short": "S", "char": "C", "float": "F", "double": "D", "void": "V"}
src, dst = sys.argv[1], sys.argv[2]
classes = {}  # mojang dotted name -> obfuscated dotted name
cur = None
entries = []
for line in open(src, encoding="utf-8"):
    if line.startswith("#") or not line.strip():
        continue
    if not line.startswith(" "):
        m, o = line.strip().rstrip(":").split(" -> ")
        classes[m] = o
        cur = m
        entries.append(("c", m))
    else:
        entries.append(("m", cur, line.strip()))


def desc(t):
    dims = t.count("[]")
    t = t.replace("[]", "")
    d = PRIM.get(t) or "L" + classes.get(t, t).replace(".", "/") + ";"
    return "[" * dims + d


out = ["tiny\t2\t0\tofficial\tnamed"]
for e in entries:
    if e[0] == "c":
        out.append("c\t%s\t%s" % (classes[e[1]].replace(".", "/"), e[1].replace(".", "/")))
        continue
    body = re.sub(r"^\d+:\d+:(?:\d+:\d+:)?", "", e[2])
    lhs, obf = body.split(" -> ")
    typ, rest = lhs.split(" ", 1)
    if "(" in rest:
        name, args = rest[:-1].split("(")
        d = "(" + "".join(desc(a) for a in args.split(",") if a) + ")" + desc(typ)
        out.append("\tm\t%s\t%s\t%s" % (d, obf, name))
    else:
        out.append("\tf\t%s\t%s\t%s" % (desc(typ), obf, rest))
with open(dst, "w", encoding="utf-8") as f:
    f.write("\n".join(out) + "\n")
print("classes", len(classes), "lines", len(out))
