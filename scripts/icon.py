#!/usr/bin/env python3
"""Make Joma's icons from the v2 logo (assets/joma-logo-v2.png).

The logo is flat shapes: a blocky "j" in pale blue-grey and its square dot in lavender.
They are redrawn here as vectors from the logo's edges (measured on the 1254 px PNG),
on a black tile: assets/icon/joma.svg (scalable) and joma-<size>.png. The bar widget
draws the same shapes (omarchy-plugin/.../JomaGlyph.qml).

Needs `rsvg-convert`. Run: python3 -I scripts/icon.py
"""

import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
OUT = REPO / "assets" / "icon"
J = "#c8d5e5"
DOT = "#b193f7"
TILE = "#000000"
SIZES = [32, 48, 64, 128, 256, 512]

# Edges in the source PNG's pixels.
LEFT, TOP, RIGHT, BOTTOM = 377, 297, 877, 1021
DOT_RECT = (701, 297, 877, 460)  # x0, y0, x1, y1
J_OUTLINE = [(377, 763), (556, 763), (556, 889), (701, 889), (701, 491), (877, 491), (877, 1021), (377, 1021)]


def svg(size):
    """The icon at `size` px: the logo about 62% of the tile's height, centred."""
    scale = size * 0.62 / (BOTTOM - TOP)
    ox = (size - (RIGHT - LEFT) * scale) / 2
    oy = (size - (BOTTOM - TOP) * scale) / 2

    def pt(x, y):
        return f"{ox + (x - LEFT) * scale:.2f},{oy + (y - TOP) * scale:.2f}"

    x0, y0, x1, y1 = DOT_RECT
    dot = f'<rect x="{ox + (x0 - LEFT) * scale:.2f}" y="{oy + (y0 - TOP) * scale:.2f}" width="{(x1 - x0) * scale:.2f}" height="{(y1 - y0) * scale:.2f}" fill="{DOT}"/>'
    j = f'<polygon points="{" ".join(pt(x, y) for x, y in J_OUTLINE)}" fill="{J}"/>'
    m = size / 32  # tile margin
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" width="{size}" height="{size}">
  <!-- Joma: the blocky j with its lavender dot, on black. Made by scripts/icon.py. -->
  <rect x="{m:.2f}" y="{m:.2f}" width="{size - 2 * m:.2f}" height="{size - 2 * m:.2f}" rx="{size * 0.2:.2f}" fill="{TILE}"/>
  {j}
  {dot}
</svg>
"""


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "joma.svg").write_text(svg(512))
    for size in SIZES:
        path = OUT / f"joma-{size}.png"
        subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(path)], input=svg(size).encode(), check=True)


if __name__ == "__main__":
    main()
