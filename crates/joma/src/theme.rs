//! Joma's colours, from the current Omarchy theme. Omarchy's look, Flux's manner: flat
//! and square, areas set apart by their fill (rail darkest, list, then the chat), a
//! hairline where two areas of one fill meet, selection as a lighter fill, the accent
//! only for state (unread, played, on), the terminal font everywhere.

use gpui::{rgb, Hsla, SharedString};
use joma_theme::{mix, Palette};

#[derive(Clone)]
pub struct Theme {
    pub bg: Hsla,
    pub panel: Hsla,
    pub bar: Hsla,
    pub raised: Hsla,
    pub border: Hsla,
    /// Where two areas of one fill meet: the text colour at 7 %.
    pub hair: Hsla,
    /// What is selected: the open conversation, the chosen option.
    pub sel: Hsla,
    /// The fill of our own messages: the list's fill with a little accent.
    pub sent: Hsla,
    pub text: Hsla,
    pub bright: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub accent_bg: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub magenta: Hsla,
    pub cyan: Hsla,
    pub font: SharedString,
    pub size: f32,
}

fn c(v: u32) -> Hsla {
    rgb(v).into()
}

impl Theme {
    pub fn from_palette(p: &Palette, font: String) -> Theme {
        Theme {
            bg: c(p.background),
            panel: c(p.dark_background),
            bar: c(p.darker_background),
            raised: c(p.lighter_background),
            // A hairline on every theme: `muted` is a text colour, bright on some.
            border: c(mix(p.background, p.foreground, 0.12)),
            hair: c(mix(p.background, p.foreground, 0.07)),
            sel: c(p.selection),
            sent: c(mix(p.dark_background, p.accent, 0.10)),
            text: c(p.foreground),
            bright: c(p.bright_foreground),
            muted: c(p.dark_foreground),
            accent: c(p.accent),
            accent_bg: c(mix(p.background, p.accent, 0.18)),
            red: c(p.red),
            green: c(p.green),
            yellow: c(p.yellow),
            magenta: c(p.magenta),
            cyan: c(p.cyan),
            font: font.into(),
            size: 13.,
        }
    }

    pub fn current() -> Theme {
        Theme::from_palette(&joma_theme::current(), joma_theme::font_family())
    }
}
