#!/usr/bin/env python3
"""Build the single-file plan previewer.

    python tools/plan_preview/build.py plan.bbplan -o preview.html
    python tools/plan_preview/build.py -o preview.html      # empty: pick a .bbplan in the page
"""
import argparse
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from preview import colour  # noqa: E402  same block colours as the blueprint renderer


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("plan", nargs="?", help=".bbplan to embed")
    ap.add_argument("-o", "--output", default="plan-preview.html")
    a = ap.parse_args()
    embed = None
    if a.plan:
        text = Path(a.plan).read_text(encoding="utf-8")
        states = re.findall(r"^(?:PLACE|AUTO) \S+ \S+ \S+ (\S+)", text, re.M)
        names = {s.removeprefix("minecraft:").split("[")[0] for s in states}
        embed = {"plan": text, "colours": {n: colour(n) for n in sorted(names)}}
    html = (HERE / "template.html").read_text(encoding="utf-8")
    # "<" is escaped so that no plan text can close the script element.
    parts = {"/*PLAN_JS*/": (HERE / "plan.js").read_text(encoding="utf-8"),
             "/*VIEWER_JS*/": (HERE / "viewer.js").read_text(encoding="utf-8"),
             "/*EMBED*/null": json.dumps(embed).replace("<", "\u003c")}
    html = re.sub("|".join(map(re.escape, parts)), lambda m: parts[m[0]], html)
    Path(a.output).write_text(html, encoding="utf-8")
    print("wrote %s (%d KB)" % (a.output, len(html.encode()) // 1024))


if __name__ == "__main__":
    main()
