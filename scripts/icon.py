#!/usr/bin/env python3
"""Make Joma's icons from the pixel-art logo (assets/joma-messenger-pixel.png).

The logo is black outline pixels on 10x10 blocks. This draws them neon green with a
light glow on a dark tile: assets/icon/joma.svg (scalable) and joma-<size>.png, each
PNG at a whole number of screen pixels per logo pixel so the edges stay sharp. It also
prints the grid for the bar widget (omarchy-plugin/.../JomaGlyph.qml).

Needs ImageMagick (`magick`) and `rsvg-convert`. Run: python3 -I scripts/icon.py
"""

import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SOURCE = REPO / "assets" / "joma-messenger-pixel.png"
OUT = REPO / "assets" / "icon"
BLOCK = 10
NEON = "#39ff88"
TILE = ("#26292d", "#141618")
SIZES = [32, 48, 64, 128, 256, 512]


def grid():
    """The logo's pixels as rows of '#' and '.', trimmed to the drawing."""
    size = subprocess.run(["magick", "identify", "-format", "%w %h", str(SOURCE)], capture_output=True, text=True, check=True)
    w, h = (int(v) // BLOCK for v in size.stdout.split())
    # One sample per block: the alpha at its centre.
    txt = subprocess.run(
        ["magick", str(SOURCE), "-alpha", "extract", "-filter", "point", "-sample", f"{w}x{h}!", "-threshold", "50%", "txt:-"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    cells = [["."] * w for _ in range(h)]
    for line in txt.splitlines()[1:]:
        pos, rest = line.split(":", 1)
        x, y = (int(v) for v in pos.split(","))
        if "#FFFFFF" in rest.upper() or "white" in rest:
            cells[y][x] = "#"
    rows = ["".join(r) for r in cells]
    used = [i for i, r in enumerate(rows) if "#" in r]
    cols = [i for i in range(w) if any(r[i] == "#" for r in rows)]
    return [r[cols[0] : cols[-1] + 1] for r in rows[used[0] : used[-1] + 1]]


def svg(rows, size, cell, ox, oy):
    """The icon at `size` px, logo pixels `cell` px wide, drawn from (ox, oy)."""
    rects = "".join(
        f'<rect x="{ox + x * cell}" y="{oy + y * cell}" width="{cell}" height="{cell}"/>'
        for y, r in enumerate(rows)
        for x, c in enumerate(r)
        if c == "#"
    )
    radius = size * 0.2
    m = size / 32  # tile margin
    glow = max(cell * 0.9, size / 64)
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" width="{size}" height="{size}">
  <!-- Joma: the pixel-art jm in neon green, a light glow, on dark grey. Made by scripts/icon.py. -->
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="{TILE[0]}"/>
      <stop offset="1" stop-color="{TILE[1]}"/>
    </linearGradient>
    <filter id="glow" x="-20%" y="-20%" width="140%" height="140%">
      <feGaussianBlur stdDeviation="{glow:.2f}"/>
    </filter>
  </defs>
  <rect x="{m:.2f}" y="{m:.2f}" width="{size - 2 * m:.2f}" height="{size - 2 * m:.2f}" rx="{radius:.2f}" fill="url(#bg)"/>
  <g fill="{NEON}" filter="url(#glow)" opacity="0.45">{rects}</g>
  <g fill="{NEON}" shape-rendering="crispEdges">{rects}</g>
</svg>
"""


def layout(rows, size):
    """Whole pixels per logo pixel, about 70% of the tile, centred."""
    h, w = len(rows), len(rows[0])
    cell = max(1, int(size * 0.7 / max(w, h)))
    return cell, (size - w * cell) // 2, (size - h * cell) // 2


def main():
    rows = grid()
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "joma.svg").write_text(svg(rows, 512, *layout(rows, 512)))
    for size in SIZES:
        path = OUT / f"joma-{size}.png"
        subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(path)], input=svg(rows, size, *layout(rows, size)).encode(), check=True)
    print("grid for JomaGlyph.qml:", file=sys.stderr)
    for r in rows:
        print(f'    "{r}",')


if __name__ == "__main__":
    main()
