//! Joma's building blocks: its own icons, drawn from `assets/icons` instead of the
//! glyphs of whichever Nerd Font the terminal uses, and the controls every view shares.

use std::borrow::Cow;

use gpui::prelude::*;
use gpui::{
    div, px, size, App, AssetSource, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, SharedString,
    Style, TransformationMatrix, Window,
};

use crate::theme::Theme;

/// One of Joma's icons: a line drawing on a 24-unit grid (`assets/icons/<name>.svg`),
/// painted in the colour of the text around it and sized from that text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icon(&'static str);

macro_rules! icons {
    ($($konst:ident = $name:literal),* $(,)?) => {
        $(#[allow(dead_code)] pub const $konst: Icon = Icon($name);)*
        /// Every icon with its SVG, embedded in the binary.
        const FILES: &[(&str, &[u8])] = &[
            $(($name, include_bytes!(concat!("../../../assets/icons/", $name, ".svg")))),*
        ];
    };
}

icons! {
    G_PHONE = "phone", G_VIDEO = "video", G_DESKTOP = "screen", G_MIC = "mic", G_MIC_OFF = "mic-off",
    G_STAR = "star-filled", G_STAR_O = "star", G_PLUS = "plus", G_SEARCH = "search", G_BOLT = "bolt",
    G_CHECK = "check", G_TIMES = "x", G_EYE = "eye", G_EXPAND = "expand", G_COMPRESS = "compress",
    G_BACK = "back", G_CLIP = "clip", G_TRASH = "trash", G_UNDO = "undo", G_COMMENT = "chat", G_BAN = "ban",
    G_USER_TIMES = "user-x", G_GRIP = "grip", G_LAPTOP = "laptop", G_LINK = "link", G_PENCIL = "pencil",
    G_COPY = "copy", G_USER_PLUS = "user-plus", G_EXCHANGE = "swap", G_WINDOW = "window", G_CROP = "crop",
    G_IMAGE = "image", G_ERASER = "eraser", G_FILL = "fill", G_MIRROR = "mirror", G_USERS = "users",
    G_COG = "settings", G_SMILE = "smile", G_YOUTUBE = "youtube", G_PLAY = "play", G_STOP = "stop",
    G_POINTER = "pointer", G_PAUSE = "pause", G_FORWARD = "next", G_USER_CIRCLE = "user", G_ROBOT = "bot",
    G_CROSSHAIRS = "crosshair", G_ZOOM_IN = "zoom-in", G_ZOOM_OUT = "zoom-out", G_SIGNAL = "signal",
    // Drawn for the restyle, not used yet.
    G_MORE = "more", G_SEND = "send", G_EYE_OFF = "eye-off", G_TRAY = "tray", G_CAMERA = "camera",
    G_DOWNLOAD = "download", G_REPLY = "reply", G_BELL = "bell",
}

impl Icon {
    fn path(self) -> SharedString {
        SharedString::from(format!("icons/{}.svg", self.0))
    }

    /// The icon at a fixed side, whatever the text size around it.
    #[allow(dead_code)]
    pub fn sized(self, side: f32) -> IconElement {
        IconElement { icon: self, side: Some(px(side)) }
    }
}

/// The embedded files GPUI loads icons from.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let name = path.strip_prefix("icons/").and_then(|p| p.strip_suffix(".svg"));
        Ok(name.and_then(|n| FILES.iter().find(|(k, _)| *k == n)).map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, _: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(FILES.iter().map(|(n, _)| SharedString::from(format!("icons/{n}.svg"))).collect())
    }
}

/// An icon in the tree. GPUI's `svg()` needs its colour set on itself; this one takes the
/// text colour in effect where it is painted, so a button's hover or "on" colour reaches
/// its icon like its label.
pub struct IconElement {
    icon: Icon,
    side: Option<Pixels>,
}

/// The side of an icon beside text of this size: 16 px at the 13 px base.
pub fn icon_side(text: f32) -> f32 {
    (text * 1.25).round()
}

impl IntoElement for Icon {
    type Element = IconElement;

    fn into_element(self) -> IconElement {
        IconElement { icon: self, side: None }
    }
}

impl IntoElement for IconElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for IconElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let side = self.side.unwrap_or_else(|| {
            let text = window.text_style().font_size.to_pixels(window.rem_size());
            px(icon_side(f32::from(text)))
        });
        let mut style = Style::default();
        style.size = size(side.into(), side.into());
        style.flex_shrink = 0.;
        (window.request_layout(style, None, cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, _: Bounds<Pixels>, _: &mut (), _: &mut Window, _: &mut App) {}

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        let color = window.text_style().color;
        let _ = window.paint_svg(bounds, self.icon.path(), TransformationMatrix::default(), color, cx);
    }
}

/// What leads a button: nothing, an icon, or a short text (a number, a key).
pub enum Mark {
    None,
    Icon(Icon),
    Text(SharedString),
}

impl From<Icon> for Mark {
    fn from(i: Icon) -> Mark {
        Mark::Icon(i)
    }
}

impl From<&str> for Mark {
    fn from(s: &str) -> Mark {
        if s.is_empty() { Mark::None } else { Mark::Text(s.to_owned().into()) }
    }
}

impl From<&String> for Mark {
    fn from(s: &String) -> Mark {
        s.as_str().into()
    }
}

/// An icon followed by text, on one line.
pub fn labelled(icon: Icon, text: impl Into<SharedString>) -> gpui::Div {
    div().flex().items_center().gap(px(5.)).child(icon).child(div().child(text.into()))
}

/// A button: a mark, a label, or both; `on` marks it active. All share one height.
/// Icon-only buttons are white and bare until hovered; labelled ones sit on a quiet fill.
pub fn button(id: impl Into<SharedString>, mark: impl Into<Mark>, label: &str, t: &Theme, on: bool) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let mark = mark.into();
    let icon_only = label.is_empty();
    let bare = matches!(mark, Mark::None);
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(5.))
        .h(px(t.size + 13.))
        // White icons, a lighter fill when on; labelled buttons are a quiet fill, no frame.
        .when(icon_only, |d| d.w(px(t.size + 13.)).text_color(if on { t.bright } else { t.text }).hover(|d| d.bg(t.raised).text_color(t.bright)))
        .when(!icon_only, |d| {
            d.pl(px(if bare { 9. } else { 6. }))
                .pr(px(9.))
                .bg(t.raised)
                .text_color(if on { t.bright } else { t.muted })
                .hover(|d| d.bg(t.sel).text_color(t.bright))
        })
        .when(on, |d| d.bg(t.sel))
        .cursor_pointer()
        .map(|d| match mark {
            Mark::None => d,
            Mark::Icon(i) => d.child(i),
            Mark::Text(s) => d.child(div().flex_none().child(s)),
        })
        .when(!icon_only, |d| d.child(div().whitespace_nowrap().child(label.to_owned())))
}

/// No corners inside Joma: Hyprland rounds the window when Omarchy says so.
pub fn radius() -> f32 {
    0.
}

/// Spacing steps.
pub const S1: f32 = 4.;
pub const S2: f32 = 8.;
pub const S3: f32 = 12.;
/// The gap between tiles and around them.
pub const GAP: f32 = 6.;

/// A panel of the window: set apart by its fill, no frame.
pub fn tile(_t: &Theme, _focused: bool) -> gpui::Div {
    div().flex().flex_col().min_w(px(0.)).min_h(px(0.)).overflow_hidden()
}

/// A small count on the accent (unread messages).
pub fn badge(t: &Theme, text: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .min_w(px(16.))
        .h(px(15.))
        .px(px(4.))
        .rounded(px(radius()))
        .flex()
        .items_center()
        .justify_center()
        .bg(t.accent)
        .text_color(t.bar)
        .text_size(px(t.size - 3.))
        .font_weight(gpui::FontWeight::BOLD)
        .child(text.into())
}

/// A small caps label over a group.
pub fn caption(t: &Theme, text: &str) -> gpui::Div {
    div().text_size(px(t.size - 2.5)).text_color(t.muted).child(text.to_uppercase())
}

/// One line of a menu: an icon, a label, and what it does on click.
pub fn menu_item(id: impl Into<SharedString>, icon: Icon, label: impl Into<SharedString>, t: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(S2))
        .h(px(28.))
        .px(px(S2))
        .rounded(px(radius()))
        .text_color(t.text)
        .cursor_pointer()
        .hover(|d| d.bg(t.raised).text_color(t.bright))
        .child(div().text_color(t.muted).child(icon))
        .child(div().whitespace_nowrap().child(label.into()))
}

/// The box of a menu, to be placed by its caller.
pub fn menu_box(t: &Theme) -> gpui::Div {
    div().flex().flex_col().p(px(S1)).min_w(px(190.)).bg(t.panel).border_1().border_color(t.border).rounded(px(radius())).shadow_lg().occlude()
}

/// A tooltip: a short line on the panel's fill.
pub struct Tip {
    text: SharedString,
    bg: gpui::Hsla,
    fg: gpui::Hsla,
    font: SharedString,
    size: f32,
}

impl gpui::Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div().px(px(7.)).py(px(3.)).bg(self.bg).text_color(self.fg).font_family(self.font.clone()).text_size(px(self.size - 1.)).child(self.text.clone())
    }
}

/// What `.tooltip(…)` takes, for a line of text.
pub fn tip(t: &Theme, text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    let (text, bg, fg, font, size) = (text.into(), t.raised, t.bright, t.font.clone(), t.size);
    move |_, cx| cx.new(|_| Tip { text: text.clone(), bg, fg, font: font.clone(), size }).into()
}

