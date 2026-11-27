#!/usr/bin/env python3
"""Turn Joma's line icons (assets/icons/*.svg: 24-unit grid, 1.5 stroke, square ends)
into Android vector drawables named `joma_ic_<name>.xml`. Handles the elements the
icons use: path, circle, rect (with rx). Strokes and fills are white and the drawable is
tinted with ?attr/colorControlNormal, so the theme colours them like the desktop does.

Usage: svg2vd.py <assets/icons> <res/drawable>
"""
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

NS = "{http://www.w3.org/2000/svg}"


def circle(cx, cy, r):
    return f"M{cx - r},{cy}a{r},{r} 0 1,0 {2 * r},0a{r},{r} 0 1,0 {-2 * r},0z"


def rect(x, y, w, h, rx):
    if not rx:
        return f"M{x},{y}h{w}v{h}h{-w}z"
    rx = min(rx, w / 2, h / 2)
    return (f"M{x + rx},{y}h{w - 2 * rx}a{rx},{rx} 0 0 1 {rx},{rx}v{h - 2 * rx}"
            f"a{rx},{rx} 0 0 1 {-rx},{rx}h{-(w - 2 * rx)}a{rx},{rx} 0 0 1 {-rx},{-rx}"
            f"v{-(h - 2 * rx)}a{rx},{rx} 0 0 1 {rx},{-rx}z")


def convert(svg: Path) -> str:
    root = ET.parse(svg).getroot()
    width = root.get("stroke-width", "1.5")
    out = []
    for el in root.iter():
        tag = el.tag.replace(NS, "")
        f = lambda k, d=0.0: float(el.get(k, d))
        if tag == "path":
            d = el.get("d")
        elif tag == "circle":
            d = circle(f("cx"), f("cy"), f("r"))
        elif tag == "rect":
            d = rect(f("x"), f("y"), f("width"), f("height"), f("rx"))
        else:
            continue
        filled = el.get("fill", root.get("fill", "none")) != "none"
        attrs = [f'android:pathData="{d}"']
        if filled:
            attrs.append('android:fillColor="#FFFFFFFF"')
        attrs += ['android:strokeColor="#FFFFFFFF"', f'android:strokeWidth="{width}"',
                  'android:strokeLineCap="square"', 'android:strokeLineJoin="miter"']
        out.append("    <path " + "\n        ".join(attrs) + " />")
    return ('<?xml version="1.0" encoding="utf-8"?>\n'
            f"<!-- Generated from assets/icons/{svg.name} by android/tools/svg2vd.py -->\n"
            '<vector xmlns:android="http://schemas.android.com/apk/res/android"\n'
            '    android:width="24dp" android:height="24dp"\n'
            '    android:viewportWidth="24" android:viewportHeight="24"\n'
            '    android:tint="?attr/colorControlNormal">\n' + "\n".join(out) + "\n</vector>\n")


def main():
    src, dst = Path(sys.argv[1]), Path(sys.argv[2])
    dst.mkdir(parents=True, exist_ok=True)
    for svg in sorted(src.glob("*.svg")):
        name = "joma_ic_" + svg.stem.replace("-", "_")
        (dst / f"{name}.xml").write_text(convert(svg))
        print(name)


if __name__ == "__main__":
    main()
