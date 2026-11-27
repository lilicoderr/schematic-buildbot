#!/usr/bin/env python3
"""python gif.py <frameDir> <out.gif> : joins the frames of render.js (needs Pillow)."""
import sys
from pathlib import Path

from PIL import Image

files = sorted(Path(sys.argv[1]).glob("frame_*.png"))
frames = [Image.open(f).convert("RGB").resize((800, 480), Image.LANCZOS) for f in files]
# One shared palette (taken from the final frame) avoids colour flicker.
palette = frames[-1].quantize(colors=128, dither=Image.Dither.NONE)
frames = [f.quantize(palette=palette, dither=Image.Dither.NONE) for f in frames]
durations = [160] * (len(frames) - 1) + [2500]
frames[0].save(sys.argv[2], save_all=True, append_images=frames[1:], duration=durations, loop=0, optimize=True)
print("wrote %s (%d frames, %d KB)" % (sys.argv[2], len(frames), Path(sys.argv[2]).stat().st_size // 1024))
