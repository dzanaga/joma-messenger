#!/usr/bin/env python3
"""Square Jami Android's drawables like the desktop: every drawable of the client that
writes its corner radius in place (`android:radius="12dp"`) is copied into joma's
overlay with that radius at 0dp. Radii that name a dimen follow joma_dimens.xml.
Dots, handles and the TV screens keep theirs.

Usage: square.py <client res dir> <overlay res dir>
"""
import re
import sys
from pathlib import Path

KEEP = {"background_amber_dot_6dp.xml", "drawer_handlebar.xml", "tv_item_selected_background.xml",
        "tv_item_unselected_background.xml"}
LITERAL = re.compile(r'(android:(?:top|bottom)?(?:Left|Right)?(?:r|R)adius=")[0-9.]+dp(")')


def main():
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    n = 0
    for f in sorted(src.glob("drawable*/*.xml")):
        if f.name in KEEP or "<corners" not in (text := f.read_text()):
            continue
        squared = LITERAL.sub(r"\g<1>0dp\2", text)
        target = out / f.parent.name / f.name
        if squared == text or target.exists():  # joma's own drawable wins
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(squared)
        n += 1
    print(f"squared {n} drawables")


if __name__ == "__main__":
    main()
