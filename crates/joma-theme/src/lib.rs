//! Omarchy keeps the active theme in `~/.local/state/omarchy/current/theme/`; its
//! `colors.toml` holds the palette as `key = "#rrggbb"` lines. Joma follows it, and falls
//! back to Tokyo Night (Omarchy's default) elsewhere. The font comes from Alacritty's
//! config, which `omarchy-font-set` keeps current.

use std::path::PathBuf;
use std::time::SystemTime;

/// Colors as 0xRRGGBB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub accent: u32,
    pub selection: u32,
    pub muted: u32,
    pub background: u32,
    pub dark_background: u32,
    pub darker_background: u32,
    pub lighter_background: u32,
    pub foreground: u32,
    pub dark_foreground: u32,
    pub bright_foreground: u32,
    pub red: u32,
    pub yellow: u32,
    pub green: u32,
    pub cyan: u32,
    pub magenta: u32,
}

impl Default for Palette {
    /// Tokyo Night, Omarchy's default theme.
    fn default() -> Self {
        Palette {
            dark: true,
            accent: 0x7aa2f7,
            selection: 0x292e42,
            muted: 0x414868,
            background: 0x1a1b26,
            dark_background: 0x13141c,
            darker_background: 0x0e0e14,
            lighter_background: 0x24283b,
            foreground: 0xa9b1d6,
            dark_foreground: 0x565f89,
            bright_foreground: 0xc0caf5,
            red: 0xf7768e,
            yellow: 0xe0af68,
            green: 0x9ece6a,
            cyan: 0x449dab,
            magenta: 0xad8ee6,
        }
    }
}

fn hex(v: &str) -> Option<u32> {
    let v = v.trim().trim_matches('"').trim_start_matches('#');
    (v.len() == 6).then(|| u32::from_str_radix(v, 16).ok()).flatten()
}

/// Mix two colors: `t` = 0 gives `a`, 1 gives `b`.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |c: u32, s: u32| ((c >> s) & 0xff) as f32;
    let one = |s: u32| ((ch(a, s) + (ch(b, s) - ch(a, s)) * t).round() as u32) << s;
    one(16) | one(8) | one(0)
}

impl Palette {
    /// Read a `colors.toml`. Missing keys keep the default, and the derived shades follow
    /// the background so a partial theme still reads well.
    pub fn parse(text: &str) -> Palette {
        let mut p = Palette::default();
        let mut seen_shades = (false, false, false);
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            if k == "mode" {
                p.dark = v.trim_matches('"') != "light";
                continue;
            }
            let Some(c) = hex(v) else { continue };
            match k {
                "accent" => p.accent = c,
                "selection" => p.selection = c,
                "muted" => p.muted = c,
                "background" => p.background = c,
                "dark_background" => (p.dark_background, seen_shades.0) = (c, true),
                "darker_background" => (p.darker_background, seen_shades.1) = (c, true),
                "lighter_background" => (p.lighter_background, seen_shades.2) = (c, true),
                "foreground" => p.foreground = c,
                "dark_foreground" => p.dark_foreground = c,
                "bright_foreground" => p.bright_foreground = c,
                "red" => p.red = c,
                "yellow" => p.yellow = c,
                "green" => p.green = c,
                "cyan" => p.cyan = c,
                "magenta" => p.magenta = c,
                _ => {}
            }
        }
        let toward = if p.dark { 0x000000 } else { 0xffffff };
        if !seen_shades.0 {
            p.dark_background = mix(p.background, toward, 0.25);
        }
        if !seen_shades.1 {
            p.darker_background = mix(p.background, toward, 0.45);
        }
        if !seen_shades.2 {
            p.lighter_background = mix(p.background, p.foreground, 0.08);
        }
        p
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn colors_path() -> Option<PathBuf> {
    Some(home()?.join(".local/state/omarchy/current/theme/colors.toml"))
}

/// The theme file's modification time, to notice `omarchy-theme-set`.
pub fn stamp() -> Option<SystemTime> {
    std::fs::metadata(colors_path()?).and_then(|m| m.modified()).ok()
}

/// The current Omarchy palette, or the default when Omarchy is not here.
pub fn current() -> Palette {
    colors_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Palette::parse(&t)).unwrap_or_default()
}

/// The terminal font family (`[font] normal = { family = "…" }` in Alacritty's config).
pub fn font_family() -> String {
    const FALLBACK: &str = "JetBrainsMono Nerd Font";
    let Some(text) = home().and_then(|h| std::fs::read_to_string(h.join(".config/alacritty/alacritty.toml")).ok())
    else {
        return FALLBACK.into();
    };
    text.lines()
        .filter(|l| l.trim_start().starts_with("normal"))
        .find_map(|l| {
            let rest = &l[l.find("family")? + 6..];
            let start = rest.find('"')? + 1;
            let end = rest[start..].find('"')? + start;
            Some(rest[start..end].to_owned())
        })
        .unwrap_or_else(|| FALLBACK.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_omarchy_colors() {
        let p = Palette::parse("mode = \"light\"\naccent = \"#123456\"\nbackground = \"#ffffff\"\n# comment\nbogus\n");
        assert!(!p.dark);
        assert_eq!(p.accent, 0x123456);
        assert_eq!(p.background, 0xffffff);
        // Shades not given are derived from the background.
        assert_eq!(p.dark_background, 0xffffff);
    }

    #[test]
    fn mix_ends() {
        assert_eq!(mix(0x000000, 0xffffff, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xffffff, 1.0), 0xffffff);
        assert_eq!(mix(0x000000, 0xff0000, 0.5), 0x800000);
    }
}
