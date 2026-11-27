#!/usr/bin/env python3
"""Write joma's colours and font aliases as Android resources.

The palette is an Omarchy theme's `colors.toml` when one is given (the build passes the
desktop's current theme, so the phone looks like the desktop it was built on), read like
`Palette::parse` in crates/joma-theme; without one, the desktop's fallback (Tokyo Night,
`Palette::default()`, read from the source so the two never drift). The roles follow
the desktop's (crates/joma/src/theme.rs): flat fills, hairlines, the accent for state
and the main action only, icons in the bright text colour. Jami
Android's colour names are given Joma's values; the night set is the real look (joma
is dark by default), the day set only takes the accent.

Jami's fonts (Ubuntu, Mulish) are aliased to JetBrains Mono, the desktop's terminal
font, so every screen that names them follows.

Usage: brand.py <crates/joma-theme/src/lib.rs> <res dir> [colors.toml]
"""
import re
import sys
from pathlib import Path


def palette(src: str) -> dict:
    body = src[src.index("fn default()"):]
    body = body[:body.index("}\n    }")]
    return {k: int(v, 16) for k, v in re.findall(r"(\w+): 0x([0-9a-fA-F]{6})", body)}


def parse_toml(text: str, p: dict) -> dict:
    """Palette::parse: keys override the default; missing shades follow the background."""
    p = dict(p)
    seen = set()
    for line in text.splitlines():
        if "=" not in line:
            continue
        k, v = (x.strip() for x in line.split("=", 1))
        v = v.strip('"').lstrip("#")
        if k in p and re.fullmatch(r"[0-9a-fA-F]{6}", v):
            p[k] = int(v, 16)
            seen.add(k)
    toward = 0x000000
    for k, t in (("dark_background", 0.25), ("darker_background", 0.45)):
        if k not in seen:
            p[k] = mix(p["background"], toward, t)
    if "lighter_background" not in seen:
        p["lighter_background"] = mix(p["background"], p["foreground"], 0.08)
    return p


def mix(a: int, b: int, t: float) -> int:
    ch = lambda c, s: (c >> s) & 0xFF
    return sum(round(ch(a, s) + (ch(b, s) - ch(a, s)) * t) << s for s in (16, 8, 0))


def hx(c: int, alpha: int | None = None) -> str:
    return f"#{alpha:02X}{c:06X}" if alpha is not None else f"#{c:06X}"


def night(p: dict) -> dict:
    bg, fg, acc = p["background"], p["foreground"], p["accent"]
    border = mix(bg, fg, 0.12)  # the desktop's border token
    hair = mix(bg, fg, 0.07)  # where two areas of one fill meet
    mine = mix(p["dark_background"], acc, 0.10)  # the desktop's `sent`: a little accent
    quiet = p["dark_foreground"]  # the desktop's `muted`
    bright = p["bright_foreground"]  # icons: white and crisp, like the desktop's
    return {
        "background": hx(bg),
        "background_secondary": hx(p["dark_background"]),
        "background_on_surface": hx(p["lighter_background"]),
        "surface": hx(p["lighter_background"]),
        "icon_tint": hx(bright),
        "colorPrimary": hx(acc),
        "colorPrimaryTranslucent": hx(acc, 0xC0),
        "colorOnPrimary": hx(bg),
        "colorSecondary": hx(acc),
        "colorSecondaryTranslucent": hx(acc, 0xB0),
        "colorSecondaryContainer": hx(p["selection"]),
        "colorOnSecondary": hx(bg),
        "colorOnSecondaryContainer": hx(fg),
        "colorError": hx(p["red"]),
        "colorOnError": hx(bg),
        "colorOnSurface": hx(p["bright_foreground"]),
        "textColorSecondary": hx(quiet),
        "call_accept": hx(p["green"]),
        "call_decline": hx(p["red"]),
        "conversation_primary_background": hx(mine),
        "conversation_secondary_background": hx(p["dark_background"]),
        "conversation_message_input_background": hx(p["lighter_background"]),
        "conversation_primary_selection": hx(p["selection"]),
        "online_indicator": hx(p["green"]),
        "available_indicator": hx(p["yellow"]),
        "icon_color": hx(bright),
        "edit_text_background": hx(p["lighter_background"]),
        "text_color": hx(p["bright_foreground"]),
        "text_hint": hx(quiet),
        "button_border": hx(hair),
        "chip_background": hx(p["selection"]),
        "settings_option_background": hx(p["lighter_background"]),
        "settings_submenu_header": hx(quiet),
        "material_switch_thumb_checked": hx(bg),
        "material_switch_thumb_unchecked": hx(quiet),
        "material_switch_track_checked": hx(acc),
        "material_switch_track_unchecked": hx(p["lighter_background"]),
        "appbar_gradient_color": hx(bg, 0xCC),
        "searchbar_color": hx(p["lighter_background"]),
        "contact_label_details_background": hx(p["selection"]),
        "contact_label_details_text": hx(fg),
        "fab_background_color": hx(acc),
        "fab_text_color": hx(bg),
        "invitation_card_background_color": hx(p["lighter_background"]),
        "invitation_card_badge_color": hx(acc),
        "invitation_card_text_color": hx(p["bright_foreground"]),
        "invitation_card_on_surface_text_color": hx(p["bright_foreground"]),
        "invitation_card_on_badge_text_color": hx(bg),
        "invitation_drawable_p1_color": hx(fg),
        "invitation_drawable_p2_color": hx(acc),
        "jami_id_background_color": hx(p["lighter_background"]),
        "jami_id_surface_color": hx(acc),
        "jami_id_edit_text_underline_color": hx(hair),
        "jami_id_validate_background_disabled_color": hx(acc, 0x4C),
        "jami_id_validate_background_enabled_color": hx(acc, 0xCC),
        "jami_id_validate_icon_enabled_color": hx(bg),
        "jami_id_validate_icon_disabled_color": hx(bg, 0x4D),
        "jami_id_info_color": hx(quiet),
        "call_missed": hx(p["red"]),
        "icon_background_color": hx(p["lighter_background"]),
        "link_peer_color": hx(p["cyan"]),
        "msg_display_name": hx(quiet),
        "menu_conversation_icons": hx(bright),
        "accept_call_button": hx(p["green"]),
        "conversation_palette_dark_blue": hx(acc),
        "donation_card_background_color": hx(p["lighter_background"]),
        "donation_card_button_color": hx(acc),
        "color_primary_light": hx(acc),
        "color_primary_dark": hx(quiet),
        "color_primary_light_container": hx(p["selection"]),
        "color_secondary": hx(p["dark_background"]),
        "color_primary_light_translucent": hx(acc, 0xB0),
        "color_primary_dark_translucent": hx(p["selection"], 0xB0),
        "color_bottom_sheet_background": hx(p["dark_background"], 0xE6),
        "ic_launcher_background": "#000000",
        "joma_background": hx(bg),
        "joma_foreground": hx(fg),
        "joma_accent": hx(acc),
        "joma_border": hx(border),
        "joma_lavender": "#B193F7",
        # Material's surface roles (cards, sheets, menus, fields), from the background
        # toward the foreground like the desktop's tiles.
        "joma_surface_lowest": hx(p["dark_background"]),
        "joma_surface_low": hx(mix(bg, fg, 0.04)),
        "joma_surface": hx(p["lighter_background"]),
        "joma_surface_high": hx(mix(bg, fg, 0.12)),
        "joma_surface_highest": hx(mix(bg, fg, 0.16)),
        "joma_on_surface_variant": hx(quiet),
        "joma_outline": hx(border),
        "joma_outline_variant": hx(hair),
    }


def day(p: dict) -> dict:
    acc = p["accent"]
    return {
        "color_primary_light": hx(acc),
        # Jami's primary in the day theme, also the colour of notifications.
        "color_primary_dark": hx(mix(acc, 0x000000, 0.40)),
        "color_primary_light_translucent": hx(acc, 0xB0),
        "fab_background_color": hx(acc),
        "jami_id_surface_color": hx(acc),
        "conversation_palette_dark_blue": hx(acc),
        "ic_launcher_background": "#000000",
        # Material's light defaults, for the day theme (joma is dark unless asked).
        "joma_surface_lowest": "#FFFFFF",
        "joma_surface_low": "#F7F7FA",
        "joma_surface": "#F1F1F5",
        "joma_surface_high": "#EBEBF0",
        "joma_surface_highest": "#E5E5EB",
        "joma_on_surface_variant": "#45464F",
        "joma_outline": "#767680",
        "joma_outline_variant": "#C6C6D0",
    }


def colors_xml(colors: dict) -> str:
    rows = "\n".join(f'    <color name="{k}">{v}</color>' for k, v in colors.items())
    return ('<?xml version="1.0" encoding="utf-8"?>\n'
            "<!-- Generated by android/tools/brand.py from crates/joma-theme. -->\n"
            f"<resources>\n{rows}\n</resources>\n")


FONT_ALIASES = {
    "ubuntu_regular": ("jetbrains_mono_regular", 400),
    "ubuntu_light": ("jetbrains_mono_light", 300),
    "ubuntu_medium": ("jetbrains_mono_medium", 500),
    "mulish_regular": ("jetbrains_mono_regular", 400),
    "mulish_semibold": ("jetbrains_mono_medium", 500),
}


def font_alias(target: str, weight: int) -> str:
    return ('<?xml version="1.0" encoding="utf-8"?>\n'
            "<!-- Generated by android/tools/brand.py: Jami's font shows as the terminal font. -->\n"
            '<font-family xmlns:android="http://schemas.android.com/apk/res/android"\n'
            '    xmlns:app="http://schemas.android.com/apk/res-auto">\n'
            f'    <font android:font="@font/{target}" android:fontStyle="normal" android:fontWeight="{weight}"\n'
            f'        app:font="@font/{target}" app:fontStyle="normal" app:fontWeight="{weight}" />\n'
            "</font-family>\n")


def main():
    p = palette(Path(sys.argv[1]).read_text())
    if len(sys.argv) > 3 and Path(sys.argv[3]).is_file():
        p = parse_toml(Path(sys.argv[3]).read_text(), p)
        print(f"palette: {sys.argv[3]}")
    res = Path(sys.argv[2])
    for d, colors in (("values", day(p)), ("values-night", night(p))):
        (res / d).mkdir(parents=True, exist_ok=True)
        (res / d / "joma_colors.xml").write_text(colors_xml(colors))
    (res / "font").mkdir(parents=True, exist_ok=True)
    for name, (target, weight) in FONT_ALIASES.items():
        (res / "font" / f"{name}.xml").write_text(font_alias(target, weight))


if __name__ == "__main__":
    main()
