//! How Joma looks: Omarchy's flat, square, keyboard-first style. One font (the terminal's),
//! thin borders, the accent for focus and selection, a status line at the bottom.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Focusable,
    canvas, div, point, px, size, AnyElement, Bounds, ContentMask, Context, Corners, FontWeight, Hsla, MouseButton,
    ExternalPaths, MouseDownEvent, MouseMoveEvent, ObjectFit, Pixels, RenderImage, ScrollDelta, ScrollWheelEvent, SharedString, Window,
};
use smallvec::smallvec;

use crate::app::*;
use joma_quick::ipc::Reply;
use crate::model::{CallState, Conversation, MsgKind};
use crate::theme::Theme;

// Nerd Font glyphs (Font Awesome range, present in every Nerd Font).
pub const G_PHONE: &str = "\u{f095}";
pub const G_VIDEO: &str = "\u{f03d}";
pub const G_DESKTOP: &str = "\u{f108}";
pub const G_MIC: &str = "\u{f130}";
pub const G_MIC_OFF: &str = "\u{f131}";
pub const G_STAR: &str = "\u{f005}";
pub const G_STAR_O: &str = "\u{f006}";
pub const G_PLUS: &str = "\u{f067}";
pub const G_SEARCH: &str = "\u{f002}";
pub const G_BOLT: &str = "\u{f0e7}";
pub const G_CHECK: &str = "\u{f00c}";
pub const G_TIMES: &str = "\u{f00d}";
pub const G_EYE: &str = "\u{f06e}";
pub const G_EXPAND: &str = "\u{f065}";
pub const G_COMPRESS: &str = "\u{f066}";
pub const G_BACK: &str = "\u{f053}";
pub const G_CLIP: &str = "\u{f0c6}";
pub const G_TRASH: &str = "\u{f1f8}";
pub const G_UNDO: &str = "\u{f0e2}";
pub const G_COMMENT: &str = "\u{f075}";
pub const G_BAN: &str = "\u{f05e}";
pub const G_USER_TIMES: &str = "\u{f235}";
pub const G_GRIP: &str = "\u{f142}";
pub const G_LAPTOP: &str = "\u{f109}";
pub const G_LINK: &str = "\u{f0c1}";
pub const G_PENCIL: &str = "\u{f040}";
pub const G_COPY: &str = "\u{f0c5}";
pub const G_USER_PLUS: &str = "\u{f234}";
pub const G_EXCHANGE: &str = "\u{f0ec}";
pub const G_WINDOW: &str = "\u{f2d0}";
pub const G_CROP: &str = "\u{f125}";
pub const G_IMAGE: &str = "\u{f03e}";
pub const G_ERASER: &str = "\u{f12d}";
pub const G_FILL: &str = "\u{f1fc}";
pub const G_MIRROR: &str = "\u{f0db}";
pub const G_USERS: &str = "\u{f0c0}";
pub const G_COG: &str = "\u{f013}";
pub const G_SMILE: &str = "\u{f118}";
pub const G_YOUTUBE: &str = "\u{f16a}";
pub const G_PLAY: &str = "\u{f04b}";
pub const G_STOP: &str = "\u{f04d}";
pub const G_POINTER: &str = "\u{f245}";
pub const G_PAUSE: &str = "\u{f04c}";
pub const G_FORWARD: &str = "\u{f051}";

/// The emoji the picker offers.
const EMOJI: &[&str] = &[
    "😀", "😃", "😄", "😁", "😆", "😅", "🤣", "😂", "🙂", "😉", "😊", "😇", "🥰", "😍", "🤩", "😘", "😋", "😛", "😜", "🤪", "😝", "🤗", "🤭", "🤫",
    "🤔", "🤐", "🤨", "😐", "😑", "😶", "😏", "😒", "🙄", "😬", "😌", "😔", "😪", "😴", "😷", "🤒", "🤕", "🤢", "🤮", "🥵", "🥶", "😵", "🤯", "🤠",
    "🥳", "😎", "🤓", "🧐", "😕", "😟", "🙁", "😮", "😯", "😲", "😳", "🥺", "😦", "😧", "😨", "😰", "😥", "😢", "😭", "😱", "😖", "😣", "😞", "😓",
    "😩", "😫", "🥱", "😤", "😡", "😠", "🤬", "😈", "💀", "💩", "🤡", "👻", "👽", "🤖", "👍", "👎", "👌", "✌️", "🤞", "🤟", "🤘", "👋", "👏", "🙌",
    "🙏", "💪", "👀", "🧠", "❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "💔", "💯", "🔥", "✨", "⭐", "🎉", "🎁", "🎂", "🍕", "🍔", "🍺", "☕", "🍷",
    "⚽", "🎮", "🎵", "🎬", "📷", "💻", "📱", "⌨️", "🚀", "✈️", "🚗", "🏠", "🌍", "☀️", "🌙", "🌧️", "❄️", "✅", "❌", "⚠️", "❓", "💡", "📌", "⏰",
];
pub const G_USER_CIRCLE: &str = "\u{f2bd}";
pub const G_ROBOT: &str = "\u{f06a9}";
pub const G_CROSSHAIRS: &str = "\u{f05b}";
pub const G_ZOOM_IN: &str = "\u{f00e}";
pub const G_ZOOM_OUT: &str = "\u{f010}";

/// Local "HH:MM" and a day key for separators.
pub fn local_time(ts: i64) -> (String, i64, String) {
    let t = ts as libc::time_t;
    // SAFETY: `localtime_r` writes into the `tm` we own.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    let day = (tm.tm_year as i64) * 400 + tm.tm_yday as i64;
    const WD: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MO: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let label = format!("{} {} {} {}", WD[tm.tm_wday as usize % 7], tm.tm_mday, MO[tm.tm_mon as usize % 12], 1900 + tm.tm_year);
    (format!("{:02}:{:02}", tm.tm_hour, tm.tm_min), day, label)
}

/// A moment for a status line: "14:32" today, "yesterday 14:32", else "Mon 26 Oct 14:32".
pub fn when(ts: i64) -> String {
    let (hm, day, label) = local_time(ts);
    let (_, today, _) = local_time(joma_desk::signal::now() as i64);
    match today - day {
        0 => hm,
        1 => format!("yesterday {hm}"),
        _ => format!("{} {hm}", label.rsplit_once(' ').map(|(d, _)| d).unwrap_or(&label)),
    }
}

/// A stable colour per author.
fn author_color(t: &Theme, uri: &str) -> Hsla {
    let palette = [t.cyan, t.magenta, t.green, t.yellow, t.red];
    let h = uri.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    palette[h as usize % palette.len()]
}

pub fn bgra_image(width: u32, height: u32, bgra: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buf = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new(smallvec![image::Frame::new(buf)])))
}

/// `inner` fitted into `outer`, centred, keeping its aspect.
pub fn fit(outer: Bounds<Pixels>, w: f32, h: f32) -> Bounds<Pixels> {
    let (ow, oh) = (f32::from(outer.size.width), f32::from(outer.size.height));
    let k = (ow / w.max(1.)).min(oh / h.max(1.));
    let (fw, fh) = (w * k, h * k);
    Bounds::new(
        point(outer.origin.x + px((ow - fw) / 2.), outer.origin.y + px((oh - fh) / 2.)),
        size(px(fw), px(fh)),
    )
}

/// A Nerd Font glyph in a cell of its own: glyphs differ in width, so each sits centred
/// in a square of the line's height.
fn glyph(g: &str, t: &Theme) -> gpui::Div {
    div().flex_none().w(px(t.size + 4.)).h(px(t.size + 4.)).flex().items_center().justify_center().child(g.to_owned())
}

/// A flat, square button: a glyph, a label, or both; `on` marks it active. Icon-only
/// buttons are square, all share one height.
fn button(id: impl Into<SharedString>, glyph_str: &str, label: &str, t: &Theme, on: bool) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let icon_only = label.is_empty();
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .h(px(t.size + 13.))
        .when(icon_only, |d| d.w(px(t.size + 13.)))
        .when(!icon_only, |d| d.pl(px(if glyph_str.is_empty() { 9. } else { 5. })).pr(px(9.)))
        .border_1()
        .border_color(if on { t.accent } else { t.border })
        .when(on, |d| d.bg(t.accent_bg))
        .text_color(if on { t.accent } else { t.text })
        .cursor_pointer()
        .hover(|d| d.bg(t.raised).text_color(t.bright).border_color(t.muted))
        .when(!glyph_str.is_empty(), |d| d.child(glyph(glyph_str, t)))
        .when(!icon_only, |d| d.child(div().whitespace_nowrap().child(label.to_owned())))
}

/// A person's picture, or their initial on their colour; a presence dot in the corner.
pub fn avatar(t: &Theme, picture: Option<std::path::PathBuf>, key: &str, name: &str, side: f32, dot: Option<Hsla>) -> AnyElement {
    let initial = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    let color = author_color(t, key);
    div()
        .relative()
        .flex_none()
        .size(px(side))
        .child(match picture {
            Some(p) => div().size(px(side)).rounded(px(3.)).overflow_hidden().child(gpui::img(p).size(px(side)).object_fit(ObjectFit::Cover)).into_any_element(),
            None => div()
                .size(px(side))
                .rounded(px(3.))
                .flex()
                .items_center()
                .justify_center()
                .bg(color.opacity(0.22))
                .border_1()
                .border_color(color.opacity(0.5))
                .text_color(color)
                .text_size(px((side * 0.48).max(9.)))
                .font_weight(FontWeight::BOLD)
                .child(initial)
                .into_any_element(),
        })
        .when_some(dot, |d, c| {
            let ds = (side * 0.3).clamp(7., 11.);
            d.child(div().absolute().right(px(-2.)).bottom(px(-2.)).size(px(ds)).rounded_full().bg(c).border_2().border_color(t.panel))
        })
        .into_any_element()
}

/// A short time for lists: "14:05" today, "yesterday", else "Thu 29".
fn short_when(ts: i64) -> String {
    let (hm, day, label) = local_time(ts);
    let (_, today, _) = local_time(joma_desk::signal::now() as i64);
    match today - day {
        0 => hm,
        1 => "yesterday".into(),
        _ => label.split(' ').take(2).collect::<Vec<_>>().join(" "),
    }
}

/// What follows the pointer while a sidebar row is dragged.
pub struct DragPreview {
    title: String,
    bg: Hsla,
    fg: Hsla,
    border: Hsla,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().px(px(10.)).py(px(4.)).bg(self.bg).border_1().border_color(self.border).text_color(self.fg).child(self.title.clone())
    }
}

/// A QR code drawn with squares (dark on light, as scanners expect).
fn qr(text: &str) -> AnyElement {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else {
        return div().into_any_element();
    };
    let w = code.width();
    let colors = code.to_colors();
    let cell = px(4.);
    let mut grid = div().flex().flex_col().p(px(16.)).bg(gpui::white());
    for row in colors.chunks(w) {
        let mut r = div().flex();
        for c in row {
            r = r.child(div().size(cell).when(*c == qrcode::Color::Dark, |d| d.bg(gpui::black())));
        }
        grid = grid.child(r);
    }
    grid.into_any_element()
}

/// A sidebar row that can be dragged and dropped on another of its list.
fn draggable(row: gpui::Stateful<gpui::Div>, dragged: Dragged, t: &Theme, cx: &mut Context<Joma>) -> gpui::Stateful<gpui::Div> {
    let (bg, fg, border, accent) = (t.raised, t.bright, t.accent, t.accent);
    let target = dragged.id.clone();
    row.on_drag(dragged, move |d: &Dragged, _, _, cx| cx.new(|_| DragPreview { title: d.title.clone(), bg, fg, border }))
        .drag_over::<Dragged>(move |s, _, _, _| s.border_t_2().border_color(accent))
        .on_drop(cx.listener(move |this, d: &Dragged, _, cx| this.drop_row(d, &target, cx)))
}

impl Joma {
    /// Copy new pictures (remote video, the desktop share) into images to paint.
    fn refresh_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sinks: Vec<String> = self.model.calls.values().flat_map(|c| c.sinks.clone()).collect();
        for id in sinks {
            let entry = self.frames.entry(id.clone());
            let frame = match entry {
                std::collections::hash_map::Entry::Occupied(o) => o.into_mut(),
                std::collections::hash_map::Entry::Vacant(v) => {
                    let mut buf = Vec::new();
                    let mut serial = 0;
                    let Some(f) = joma_jami::sink_frame(&id, &mut serial, &mut buf) else { continue };
                    let Some(image) = bgra_image(f.width, f.height, f.bgra) else { continue };
                    v.insert(Frame { image, serial, buf });
                    continue;
                }
            };
            let mut serial = frame.serial;
            if let Some(f) = joma_jami::sink_frame(&id, &mut serial, &mut frame.buf) {
                if let Some(image) = bgra_image(f.width, f.height, f.bgra) {
                    let old = std::mem::replace(&mut frame.image, image);
                    frame.serial = serial;
                    cx.drop_image(old, Some(window));
                }
            }
        }
        let state = self.viewing_key().and_then(|k| self.desk.viewer(&k)).map(|v| v.state.lock().unwrap().clone());
        match state {
            Some(st) => {
                if let Some(p) = st.picture {
                    if self.desk_frame.as_ref().is_none_or(|f| f.serial != p.serial) {
                        // Another screen or window (a new size): show it whole again.
                        let resized = self.desk_frame.as_ref().is_some_and(|f| {
                            let sz = f.image.size(0);
                            (sz.width.0 as u32, sz.height.0 as u32) != (p.width, p.height)
                        });
                        if resized {
                            self.stage_view.borrow_mut().reset();
                        }
                        if let Some(image) = bgra_image(p.width, p.height, p.bgra.as_ref().clone()) {
                            let old = self.desk_frame.replace(Frame { image, serial: p.serial, buf: Vec::new() });
                            if let Some(old) = old {
                                cx.drop_image(old.image, Some(window));
                            }
                        }
                    }
                }
                let ptr = st.pointer;
                if ptr.shape_serial > 0 && self.pointer.as_ref().is_none_or(|f| f.serial != ptr.shape_serial) {
                    if let Some(image) = bgra_image(ptr.width, ptr.height, ptr.bgra.as_ref().clone()) {
                        if let Some(old) = self.pointer.replace(Frame { image, serial: ptr.shape_serial, buf: Vec::new() }) {
                            cx.drop_image(old.image, Some(window));
                        }
                    }
                }
            }
            None => {
                if let Some(old) = self.desk_frame.take() {
                    cx.drop_image(old.image, Some(window));
                }
            }
        }
        // The sharer's own previews.
        let keys: Vec<String> = self.desk.sessions.keys().cloned().collect();
        for key in &keys {
            let Some(pic) = self.desk.shared(key).and_then(|(_, p)| p).and_then(|p| p.picture.lock().unwrap().clone()) else { continue };
            if self.previews.get(key).is_some_and(|f| f.serial == pic.serial) {
                continue;
            }
            if let Some(image) = bgra_image(pic.width, pic.height, pic.bgra.as_ref().clone()) {
                if let Some(old) = self.previews.insert(key.clone(), Frame { image, serial: pic.serial, buf: Vec::new() }) {
                    cx.drop_image(old.image, Some(window));
                }
            }
        }
        // Previews of the YouTube links in the open conversation, fetched once.
        let ids: Vec<String> = self
            .selected
            .as_ref()
            .and_then(|s| self.model.conversations.get(s))
            .map(|c| {
                c.messages
                    .iter()
                    .flat_map(|m| match crate::watch::parse_results(&m.text) {
                        Some((_, list)) => list.into_iter().map(|(_, id)| id).collect(),
                        None => crate::media::youtube_id(&m.text).into_iter().collect::<Vec<_>>(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        for id in ids {
            self.want_youtube_preview(&id);
        }
        let gone: Vec<String> = self.previews.keys().filter(|k| self.desk.shared(k).is_none()).cloned().collect();
        for k in gone {
            if let Some(old) = self.previews.remove(&k) {
                cx.drop_image(old.image, Some(window));
            }
        }
    }

    fn top_bar(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let reg = self.model.registration.as_str();
        let (dot, color) = match reg {
            "REGISTERED" => ("online", t.green),
            "" => ("", t.muted),
            "TRYING" | "INITIALIZING" => ("connecting", t.yellow),
            _ => ("offline", t.red),
        };
        let unread = self.listed_unread();
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(30.))
            .px(px(12.))
            .bg(t.bar)
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(5.))
                            .child(div().text_color(t.accent).font_weight(FontWeight::BOLD).child("joma"))
                            .when(!self.narrow, |d| d.child(div().text_color(t.muted).text_size(px(t.size - 2.)).child(concat!("v", env!("CARGO_PKG_VERSION"))))),
                    )
                    .child(div().text_color(t.muted).child("·"))
                    .child(div().text_color(t.bright).child(if self.model.my_name.is_empty() {
                        "jami".to_owned()
                    } else {
                        self.model.my_name.clone()
                    }))
                    .when(!self.model.username.is_empty(), |d| {
                        d.child(div().text_color(t.muted).child(format!("@{}", self.model.username)))
                    })
                    .when(self.model.username.is_empty() && self.gate == Gate::Ready && !self.narrow, |d| {
                        d.child(
                            div()
                                .id("choose-username")
                                .text_color(t.yellow)
                                .cursor_pointer()
                                .hover(|d| d.text_color(t.accent))
                                .child("choose a username")
                                .on_click(cx.listener(|this, _, window, cx| this.set_mode(Mode::Username, window, cx))),
                        )
                    })
                    .when(!dot.is_empty(), |d| d.child(div().text_color(color).child(format!("● {dot}"))))
                    .when(reg == "REGISTERED", |d| {
                        let invisible = self.quick.invisible;
                        d.child(
                            div()
                                .id("invisible")
                                .text_color(if invisible { t.yellow } else { t.muted })
                                .cursor_pointer()
                                .hover(|d| d.text_color(t.accent))
                                .child(if invisible { "◌ invisible" } else { "visible" })
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_invisible(cx))),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .text_color(t.muted)
                    .when(unread > 0 && !self.narrow, |d| d.child(div().text_color(t.accent).child(format!("{unread} unread"))))
                    .when(!self.narrow, |d| d.child(format!("{} conversations", self.listed().len())))
                    .child({
                        // Pop out to the full window, or back into the tray.
                        let full = crate::hypr::shape() == crate::hypr::Shape::Full;
                        div()
                            .id("pop")
                            .cursor_pointer()
                            .hover(|d| d.text_color(t.accent))
                            .child(if full { format!("{G_COMPRESS} to the tray") } else { format!("{G_EXPAND} full") })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if full {
                                    this.pop_in(cx)
                                } else {
                                    this.pop_out(window, cx)
                                }
                            }))
                    }),
            )
            .into_any_element()
    }

    /// Offers to set Joma up on this desktop, until done or closed.
    fn setup_banner(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let s = self.setup;
        if self.gate != Gate::Ready || self.quick.setup_dismissed || !s.pending() {
            return None;
        }
        let test = std::env::var_os("JOMA_PROFILE").is_some();
        Some(
            div()
                .flex()
                .flex_none()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(6.))
                .bg(t.accent_bg)
                .border_b_1()
                .border_color(t.border)
                .child(div().text_color(t.bright).child("Set Joma up here:"))
                .when(s.bar_has_tray && !s.pinned, |d| {
                    d.child(button("setup-pin", G_CHECK, "pin the icon in the bar", t, false).on_click(cx.listener(|this, _, _, cx| this.pin_in_bar(cx))))
                })
                .when(s.shortcut_possible && !s.shortcut, |d| {
                    d.child(button("setup-key", G_CHECK, "Super+Alt+M", t, false).on_click(cx.listener(|this, _, _, cx| this.add_shortcut(cx))))
                })
                .when(!s.start_at_login && !test, |d| {
                    d.child(button("setup-login", G_CHECK, "start at login", t, false).on_click(cx.listener(|this, _, _, cx| this.set_start_at_login(true, cx))))
                })
                .child(div().flex_1())
                .child(
                    div()
                        .id("setup-close")
                        .text_color(t.muted)
                        .cursor_pointer()
                        .hover(|d| d.text_color(t.accent))
                        .child(G_TIMES)
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_setup(cx))),
                )
                .into_any_element(),
        )
    }

    fn sidebar(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let convs: Vec<Conversation> = self.visible_conversations().into_iter().cloned().collect();
        let mut list = div().id("conversations").flex().flex_col().flex_1().overflow_y_scroll();
        for c in convs {
            let selected = self.selected.as_deref() == Some(c.id.as_str());
            let fav = self.quick.favorites.contains(&c.id);
            let has_call = self.model.calls.values().any(|k| k.conversation.as_deref() == Some(&c.id));
            let dot = if c.request {
                Dot::None
            } else if c.peers.iter().any(|p| self.dot_of(p) == Dot::Active) {
                Dot::Active
            } else if c.peers.iter().any(|p| self.dot_of(p) == Dot::Reachable) {
                Dot::Reachable
            } else {
                Dot::None
            };
            let last = if c.composing {
                "typing…".to_owned()
            } else {
                c.last().map(|m| m.text.lines().next().unwrap_or("").to_owned()).unwrap_or_default()
            };
            let id = c.id.clone();
            let dragged = Dragged { tab: Tab::Chats, id: c.id.clone(), title: c.title.clone() };
            let row = div()
                    .id(SharedString::from(format!("conv-{}", c.id)))
                    .flex()
                    .flex_col()
                    .px(px(10.))
                    .py(px(6.))
                    .border_l_2()
                    .border_color(if selected { t.accent } else { gpui::transparent_black() })
                    .when(selected, |d| d.bg(t.accent_bg))
                    .hover(|d| d.bg(t.raised))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.set_mode(Mode::Chat, window, cx);
                        this.select(id.clone(), cx);
                        this.narrow_chat = true;
                    }))
                    .child({
                        let peer = (c.peers.len() == 1).then(|| c.peers[0].clone());
                        let key = peer.clone().unwrap_or_else(|| c.id.clone());
                        let picture = peer.as_ref().and_then(|p| self.model.avatars.get(crate::model::bare_uri(p)).cloned());
                        let dot_color = match dot {
                            Dot::Active => Some(t.green),
                            Dot::Reachable => Some(t.green.opacity(0.35)),
                            Dot::None => None,
                        };
                        let stamp = c.last().map(|m| short_when(m.ts)).unwrap_or_default();
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(avatar(t, picture, &key, &c.title, 32., dot_color))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(5.))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .truncate()
                                                    .text_color(if c.unread > 0 || selected { t.bright } else { t.text })
                                                    .when(c.unread > 0, |d| d.font_weight(FontWeight::BOLD))
                                                    .child(c.title.clone()),
                                            )
                                            .when(fav, |d| d.child(div().flex_none().text_size(px(t.size - 2.)).text_color(t.yellow).child(G_STAR)))
                                            .when(has_call, |d| d.child(div().flex_none().text_size(px(t.size - 2.)).text_color(t.green).child(G_PHONE)))
                                            .child(div().flex_none().text_size(px(t.size - 2.5)).text_color(if c.unread > 0 { t.accent } else { t.muted }).child(stamp)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(6.))
                                            .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.muted).text_size(px(t.size - 1.5)).child(last))
                                            .when(c.request, |d| d.child(div().flex_none().text_size(px(t.size - 2.)).text_color(t.yellow).child("request")))
                                            .when(c.unread > 0, |d| {
                                                d.child(
                                                    div()
                                                        .flex_none()
                                                        .min_w(px(18.))
                                                        .h(px(16.))
                                                        .px(px(4.))
                                                        .rounded(px(8.))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .bg(t.accent)
                                                        .text_color(t.bar)
                                                        .text_size(px(t.size - 2.5))
                                                        .font_weight(FontWeight::BOLD)
                                                        .child(c.unread.to_string()),
                                                )
                                            }),
                                    ),
                            )
                    })
                    .when(self.mode == Mode::Removed, |d| {
                        let (a, b) = (c.id.clone(), c.id.clone());
                        let armed = self.confirm_delete.as_ref().is_some_and(|(k, at)| *k == c.id && at.elapsed().as_secs() < 5);
                        d.child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .pt(px(4.))
                                .child(button(SharedString::from(format!("restore-{}", c.id)), G_UNDO, "restore", t, false).on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.restore(&a, cx);
                                    },
                                )))
                                .child(
                                    button(SharedString::from(format!("delete-{}", c.id)), G_TRASH, if armed { "sure? click again" } else { "delete history" }, t, armed)
                                        .text_color(t.red)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.delete_for_good(&b, cx);
                                        })),
                                ),
                        )
                    });
            list = list.child(if c.request || self.mode == Mode::Removed { row } else { draggable(row, dragged, t, cx) });
        }
        let header = |label: &str| {
            div()
                .px(px(10.))
                .pt(px(10.))
                .pb(px(4.))
                .text_size(px(t.size - 2.))
                .text_color(t.muted)
                .child(label.to_uppercase())
        };
        div()
            .flex()
            .flex_col()
            .when(!self.narrow, |d| d.flex_none().w(px(290.)))
            .when(self.narrow, |d| d.flex_1().min_w(px(0.)))
            .h_full()
            .bg(t.panel)
            .border_r_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pr(px(8.))
                    .h(px(44.))
                    .child(
                        header(match self.tab {
                            Tab::Chats => "chats",
                            Tab::Calls => "calls",
                            Tab::Contacts => "contacts",
                            Tab::Devices => "devices",
                            Tab::Accounts => "accounts",
                            Tab::Agents => "agents",
                        })
                        .pt(px(0.))
                        .pb(px(0.))
                        .text_size(px(t.size - 1.))
                        .text_color(t.bright)
                        .font_weight(FontWeight::BOLD),
                    )
                    .when(matches!(self.tab, Tab::Chats | Tab::Contacts), |d| d.child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(
                                button("btn-search", G_SEARCH, "", t, self.mode == Mode::Search)
                                    .on_click(cx.listener(|this, _, window, cx| this.set_mode(Mode::Search, window, cx))),
                            )
                            .child(
                                button("btn-add", G_PLUS, "", t, self.mode == Mode::Add)
                                    .on_click(cx.listener(|this, _, window, cx| this.set_mode(Mode::Add, window, cx))),
                            )
                            .when(self.tab == Tab::Chats && (!self.quick.removed.is_empty() || self.mode == Mode::Removed), |d| {
                                d.child(button("btn-removed", G_TRASH, "", t, self.mode == Mode::Removed).on_click(cx.listener(|this, _, window, cx| {
                                    let next = if this.mode == Mode::Removed { Mode::Chat } else { Mode::Removed };
                                    this.set_mode(next, window, cx);
                                })))
                            }),
                    )),
            )
            .when(self.mode == Mode::Removed, |d| {
                d.child(div().px(px(10.)).pb(px(6.)).text_color(t.muted).text_size(px(t.size - 2.)).child(if self.quick.removed.is_empty() {
                    "no removed contacts"
                } else {
                    "removed contacts: restore, or delete for good"
                }))
            })
            .when(self.mode != Mode::Chat && self.mode != Mode::Removed, |d| {
                d.child(
                    div()
                        .px(px(8.))
                        .pb(px(6.))
                        .child(div().text_color(t.muted).text_size(px(t.size - 2.)).pb(px(3.)).child(match self.mode {
                            Mode::Add => "add a contact",
                            Mode::Username => "choose your username (contacts find you by it)",
                            Mode::LinkDevice => "link a device: on the new one choose \"link to an account\"",
                            Mode::DeviceName => "rename this device",
                            Mode::RemoveAccount => "remove this account: only this device has it, so it is lost for good",
                            _ => "filter",
                        }))
                        .child(self.input.clone()),
                )
            })
            .child(match self.tab {
                Tab::Chats => list.into_any_element(),
                Tab::Calls => self.calls_list(t, cx),
                Tab::Contacts => self.contacts_list(t, cx),
                Tab::Devices => div().id("devices").flex().flex_col().flex_1().overflow_y_scroll().child(self.devices_group(t, cx)).into_any_element(),
                Tab::Accounts => self.accounts_list(t, cx),
                Tab::Agents => self.agents_list(t, cx),
            })
            .into_any_element()
    }

    /// Onboarding by joining an account from another device: the code, as text and QR.
    fn link_card(&self, card: gpui::Div, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let Some(l) = &self.linking else { return card };
        let card = card.child(div().text_color(t.accent).font_weight(FontWeight::BOLD).child("Link this device to your account"));
        if !l.error.is_empty() {
            return card
                .child(div().text_color(t.red).child(l.error.clone()))
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(button("link-retry", G_UNDO, "try again", t, true).on_click(cx.listener(|this, _, _, cx| {
                            this.linking = None;
                            this.start_link(cx);
                        })))
                        .child(button("link-back", G_BACK, "back", t, false).on_click(cx.listener(|this, _, _, cx| this.cancel_link(cx)))),
                );
        }
        let token = l.token.clone();
        card.child(div().child(
            "On your other device: Joma → Devices → paste the code (or run joma link-device <code> there); or in the Jami app: Link device, and scan it.",
        ))
        .when(!token.is_empty(), |d| {
            let copy = token.clone();
            d.child(div().flex().justify_center().child(qr(&token)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().flex_1().min_w(px(0.)).text_color(t.bright).text_size(px(t.size - 1.5)).truncate().child(token))
                        .child(button("link-copy", G_COPY, "copy", t, false).on_click(cx.listener(move |this, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                            this.show_flash("code copied", cx);
                        }))),
                )
        })
        .when(l.password_scheme.is_some(), |d| d.child(self.input.clone()))
        .child(div().text_color(t.muted).child(l.status.clone()))
        .child(div().flex().child(button("link-cancel", G_TIMES, "cancel", t, false).on_click(cx.listener(|this, _, _, cx| this.cancel_link(cx)))))
    }

    /// The open account: picture, name, username, and removing it from this machine.
    fn account_card(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let armed = self.is_armed("remove-account");
        let who = if self.model.username.is_empty() { self.model.me.chars().take(8).collect::<String>() + "…" } else { self.model.username.clone() };
        div().flex().flex_col().child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(4.))
                .child(
                    div()
                        .id("my-avatar")
                        .cursor_pointer()
                        .hover(|d| d.opacity(0.8))
                        .child(avatar(t, self.model.avatars.get(&self.model.me).cloned(), &self.model.me, &self.model.my_name, 36., None))
                        .on_click(cx.listener(|this, _, _, cx| this.open_profile(cx))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(div().truncate().text_color(t.bright).font_weight(FontWeight::BOLD).child(self.model.my_name.clone()))
                        .child(div().truncate().text_color(t.muted).text_size(px(t.size - 2.)).child(who)),
                )
                .child(button("edit-profile", G_PENCIL, "", t, false).on_click(cx.listener(|this, _, _, cx| this.open_profile(cx))))
                .child(
                    button("account-remove", G_TRASH, if armed || self.mode == Mode::RemoveAccount { "remove?" } else { "" }, t, armed)
                        .text_color(if armed || self.mode == Mode::RemoveAccount { t.red } else { t.muted })
                        .on_click(cx.listener(|this, _, window, cx| this.remove_account(window, cx))),
                ),
        )
    }

    /// This account's devices, and linking a new one.
    fn devices_group(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let mut group = div().flex().flex_col();
        let mut ids: Vec<(String, String)> = self.model.devices.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        ids.sort_by_key(|(id, _)| *id != self.model.device);
        for (id, name) in ids {
            let this = id == self.model.device;
            let dot = self.device_dot(&id);
            let armed = self.is_armed(&format!("revoke:{id}"));
            let i = id.clone();
            // Equal names (two Joma on one host) get the start of the id.
            let twins = self.model.devices.values().filter(|n| **n == name).count() > 1;
            let label = match (name.is_empty(), twins) {
                (true, _) => format!("{}…", &id[..id.len().min(8)]),
                (false, true) => format!("{name} · {}", &id[..id.len().min(6)]),
                (false, false) => name,
            };
            group = group.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(4.))
                    .child(div().flex_none().text_color(t.muted).child(G_LAPTOP))
                    .child(div().flex_none().size(px(7.)).rounded_full().when(dot != Dot::None, |d| {
                        d.bg(if dot == Dot::Active { t.green } else { t.green.opacity(0.3) })
                    }))
                    .child(div().flex_1().min_w(px(0.)).truncate().text_color(if this { t.bright } else { t.text }).child(label))
                    .when(this, |d| {
                        d.child(div().text_color(t.muted).text_size(px(t.size - 2.)).child("this device")).child(
                            button(SharedString::from(format!("dev-rename-{id}")), G_PENCIL, "", t, false)
                                .on_click(cx.listener(|this, _, window, cx| this.set_mode(Mode::DeviceName, window, cx))),
                        )
                    })
                    .when(!this, |d| {
                        d.child(
                            button(SharedString::from(format!("dev-revoke-{id}")), G_BAN, if armed { "revoke?" } else { "" }, t, armed)
                                .text_color(if armed { t.red } else { t.muted })
                                .on_click(cx.listener(move |this, _, _, cx| this.revoke_device(&i, cx))),
                        )
                    }),
            );
        }
        let op = self.device_op.clone();
        group.child(match op {
            Some(d) if d.state == joma_jami::AUTH_AUTHENTICATING => div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .mx(px(10.))
                .my(px(4.))
                .p(px(6.))
                .border_1()
                .border_color(t.yellow)
                .child(div().text_color(t.bright).child(format!("A device at {} asks for your account.", if d.peer.is_empty() { "?" } else { &d.peer })))
                .child(div().text_color(t.muted).text_size(px(t.size - 2.)).child("Send it only if it is yours: it gets your account and history."))
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(button("dev-yes", G_CHECK, "send", t, true).on_click(cx.listener(|this, _, _, cx| this.answer_device(true, cx))))
                        .child(button("dev-no", G_TIMES, "cancel", t, false).on_click(cx.listener(|this, _, _, cx| this.answer_device(false, cx)))),
                ),
            Some(d) => div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(10.))
                .py(px(4.))
                .child(div().text_color(t.muted).child(if d.state == joma_jami::AUTH_IN_PROGRESS { "sending the account…" } else { "connecting to the new device…" }))
                .child(button("dev-stop", G_TIMES, "", t, false).on_click(cx.listener(|this, _, _, cx| this.answer_device(false, cx)))),
            None => self.link_steps(t, cx),
        })
    }

    /// How to add a device to this account, step by step, with the commands to copy.
    fn link_steps(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let cmd = |id: &'static str, text: &str| {
            let copy = text.to_owned();
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().flex_1().min_w(px(0.)).px(px(6.)).py(px(3.)).bg(t.bar).border_1().border_color(t.border).text_color(t.bright).truncate().child(text.to_owned()))
                .child(button(id, G_COPY, "", t, false).on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                    this.show_flash("copied", cx);
                })))
        };
        let step = |n: &str, text: &str| {
            div()
                .flex()
                .gap(px(8.))
                .child(div().flex_none().size(px(18.)).rounded_full().bg(t.accent_bg).text_color(t.accent).text_size(px(t.size - 3.)).flex().items_center().justify_center().child(n.to_owned()))
                .child(div().flex_1().min_w(px(0.)).text_color(t.text).text_size(px(t.size - 1.)).child(text.to_owned()))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .mx(px(10.))
            .my(px(8.))
            .p(px(10.))
            .border_1()
            .border_color(t.border)
            .bg(t.bg)
            .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Add a device"))
            .child(step("1", "On the new device, install Joma and run (or open Joma and choose “link to your account”):"))
            .child(cmd("cp-link", "joma link"))
            .child(step("2", "It shows a code starting with jami-auth://. Paste it here, or on this device run:"))
            .child(cmd("cp-link-dev", "joma link-device jami-auth://…"))
            .child(step("3", "Confirm here when the new device asks; it receives your account and history."))
            .child(
                div().flex().gap(px(6.)).child(
                    button("dev-link", G_LINK, "paste the code", t, self.mode == Mode::LinkDevice)
                        .on_click(cx.listener(|this, _, window, cx| this.set_mode(Mode::LinkDevice, window, cx))),
                ),
            )
            .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child("Install on a new machine: mise use -g github:dzanaga/joma-messenger (and sudo pacman -S jami-daemon)."))
    }

    /// The Calls tab: what runs now (open it or end it), then calls and shares that took
    /// place, newest first.
    fn calls_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().id("calls").flex().flex_col().flex_1().overflow_y_scroll();
        let small = |label: &str| div().px(px(10.)).pt(px(10.)).pb(px(4.)).text_size(px(t.size - 2.5)).text_color(t.muted).child(label.to_uppercase());
        let entry = |id: SharedString, glyph_str: &str, color: Hsla, who: &str, key: &str, what: String, when: String| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(10.))
                .py(px(6.))
                .child(avatar(t, self.model.avatars.get(crate::model::bare_uri(key)).cloned(), crate::model::bare_uri(key), who, 28., None))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(div().truncate().text_color(t.bright).child(who.to_owned()))
                        .child(div().flex().items_center().gap(px(4.)).text_size(px(t.size - 2.)).text_color(color).child(glyph(glyph_str, t)).child(div().truncate().child(what))),
                )
                .child(div().flex_none().text_size(px(t.size - 2.5)).text_color(t.muted).child(when))
        };
        // Now.
        let calls: Vec<crate::model::Call> = self.model.calls.values().cloned().collect();
        let shares = self.shares_running();
        let direct_asked: Vec<String> = self.desk.direct_sessions().into_iter().filter(|k| matches!(self.desk.sessions.get(k).map(|s| &s.viewing), Some(crate::desk::Viewing::Asked { .. }))).collect();
        if !calls.is_empty() || !shares.is_empty() || !direct_asked.is_empty() {
            list = list.child(small("now"));
        }
        for c in calls {
            let name = self.model.name_of(&c.peer);
            let elapsed = c.started.map(|s| s.elapsed().as_secs()).map(|s| format!("{:02}:{:02}", s / 60, s % 60)).unwrap_or_default();
            let (conv, id2) = (c.conversation.clone(), c.id.clone());
            list = list.child(
                entry(SharedString::from(format!("call-now-{}", c.id)), G_PHONE, t.green, &name, &c.peer, c.state.label().to_owned(), elapsed)
                    .bg(t.accent_bg)
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .when_some(conv, |d, conv| {
                                d.child(button(SharedString::from(format!("call-open-{}", c.id)), G_EYE, "", t, false).on_click(cx.listener(move |this, _, window, cx| {
                                    this.set_tab(Tab::Chats, window, cx);
                                    this.select(conv.clone(), cx);
                                })))
                            })
                            .child(button(SharedString::from(format!("call-end-{id2}")), G_PHONE, "", t, false).text_color(t.red).on_click(cx.listener(move |this, _, _, cx| this.hang_up(cx)))),
                    ),
            );
        }
        for (key, started, shared) in shares {
            let peer = crate::desk::direct_peer(&key).map(str::to_owned).or_else(|| self.model.calls.get(&key).map(|c| c.peer.clone())).unwrap_or_default();
            let name = self.model.name_of(&peer);
            let secs = started.elapsed().as_secs();
            let (k1, k2) = (key.clone(), key.clone());
            let direct = crate::desk::direct_peer(&key).is_some();
            list = list.child(
                entry(
                    SharedString::from(format!("share-now-{key}-{shared}")),
                    if shared { G_DESKTOP } else { G_EYE },
                    t.yellow,
                    &name,
                    &peer,
                    if shared { "you share your screen".into() } else { "you watch their screen".into() },
                    format!("{:02}:{:02}", secs / 60, secs % 60),
                )
                .bg(t.accent_bg)
                .child(
                    div()
                        .flex()
                        .gap(px(4.))
                        .when(!shared, |d| {
                            d.child(button(SharedString::from(format!("share-open-{k1}")), G_EXPAND, "", t, false).on_click(cx.listener(|this, _, window, cx| {
                                this.set_tab(Tab::Chats, window, cx);
                                this.stage = true;
                                cx.notify();
                            })))
                        })
                        .child(button(SharedString::from(format!("share-end-{k2}-{shared}")), G_TIMES, "", t, false).text_color(t.red).on_click(cx.listener(move |this, _, _, cx| {
                            if direct {
                                this.stop_direct(&k2, cx);
                            } else if shared {
                                this.toggle_share(cx);
                            } else {
                                let acc = this.model.account.clone();
                                this.desk.decline_offer(&acc, &k2);
                                cx.notify();
                            }
                        }))),
                ),
            );
        }
        for key in direct_asked {
            let peer = crate::desk::direct_peer(&key).unwrap_or_default().to_owned();
            let name = self.model.name_of(&peer);
            let (k1, k2) = (key.clone(), key.clone());
            list = list.child(
                entry(SharedString::from(format!("share-ask-{key}")), G_DESKTOP, t.accent, &name, &peer, "wants to show you their screen".into(), String::new())
                    .bg(t.accent_bg)
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(button(SharedString::from(format!("ask-watch-{k1}")), G_EYE, "watch", t, true).on_click(cx.listener(move |this, _, _, cx| this.watch_direct(&k1, cx))))
                            .child(button(SharedString::from(format!("ask-no-{k2}")), G_TIMES, "", t, false).on_click(cx.listener(move |this, _, _, cx| this.stop_direct(&k2, cx)))),
                    ),
            );
        }
        // History: calls from the conversations, shares from Joma's log.
        let mut past: Vec<(i64, String, String, bool, bool, String)> = Vec::new();
        for c in self.model.conversations.values() {
            let peer = c.peers.first().cloned().unwrap_or_default();
            for m in c.messages.iter().filter(|m| m.kind == MsgKind::Event && (m.text.starts_with("call") || m.text == "missed call")) {
                let mine = crate::model::bare_uri(&m.author) == self.model.me;
                past.push((m.ts, c.id.clone(), peer.clone(), true, mine, m.text.clone()));
            }
        }
        for l in &self.share_log {
            let what = format!("{} · {}:{:02}", if l.shared { "you shared your screen" } else { "you watched their screen" }, l.secs / 60, l.secs % 60);
            past.push((l.ts, String::new(), l.peer.clone(), false, l.shared, what));
        }
        past.sort_by_key(|p| std::cmp::Reverse(p.0));
        list = list.child(small("history"));
        if past.is_empty() {
            list = list.child(div().px(px(10.)).py(px(6.)).text_color(t.muted).child("no calls or shares yet"));
        }
        for (i, (ts, conv, peer, call, mine, text)) in past.into_iter().take(150).enumerate() {
            let name = self.model.name_of(&peer);
            let missed = text == "missed call";
            let (g, color) = if !call {
                (if mine { G_DESKTOP } else { G_EYE }, t.muted)
            } else if missed {
                (G_PHONE, t.red)
            } else {
                (G_PHONE, if mine { t.accent } else { t.green })
            };
            let label = if call { format!("{} {}", if mine { "outgoing" } else { "incoming" }, text) } else { text };
            let row = entry(SharedString::from(format!("past-{i}")), g, color, &name, &peer, label, short_when(ts)).cursor_pointer().hover(|d| d.bg(t.raised));
            list = list.child(if conv.is_empty() {
                row
            } else {
                row.on_click(cx.listener(move |this, _, window, cx| {
                    this.set_tab(Tab::Chats, window, cx);
                    this.select(conv.clone(), cx);
                }))
            });
        }
        list.into_any_element()
    }

    /// The Accounts tab: the open one, the others on this machine, adding one.
    fn accounts_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let small = |label: &str| div().px(px(10.)).pt(px(10.)).pb(px(4.)).text_size(px(t.size - 2.5)).text_color(t.muted).child(label.to_uppercase());
        let mut list = div().id("accounts").flex().flex_col().flex_1().overflow_y_scroll().child(small("open")).child(self.account_card(t, cx));
        if self.model.username.is_empty() {
            list = list.child(
                div().flex().px(px(10.)).py(px(4.)).child(
                    button("acc-username", G_PENCIL, "choose a username", t, false).on_click(cx.listener(|this, _, window, cx| this.set_mode(Mode::Username, window, cx))),
                ),
            );
        }
        if self.accounts.len() > 1 {
            list = list.child(small("on this machine")).child(self.accounts_group(t, cx));
        }
        list.child(small("add"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(4.))
                    .child(div().flex().child(button("account-new", G_USER_PLUS, "create a new account", t, false).on_click(cx.listener(|this, _, window, cx| this.new_account(window, cx)))))
                    .child(div().flex().child(button("account-link", G_LINK, "log in with another device", t, false).on_click(cx.listener(|this, _, window, cx| {
                        this.new_account(window, cx);
                        this.start_link(cx);
                    }))))
                    .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child("Logging in with another device brings an account you already have (its username and contacts come with it).")),
            )
            .child(small("agents"))
            .child(
                div().flex().px(px(10.)).py(px(4.)).child(
                    button("acc-agents", G_ROBOT, "agent accounts…", t, false).on_click(cx.listener(|this, _, window, cx| this.set_tab(Tab::Agents, window, cx))),
                ),
            )
            .into_any_element()
    }

    /// The Agents tab: a first sketch; nothing runs yet.
    fn agents_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let card = |name: &str, what: &str| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .mx(px(10.))
                .my(px(4.))
                .p(px(8.))
                .border_1()
                .border_color(t.border)
                .opacity(0.6)
                .child(div().size(px(32.)).rounded(px(3.)).bg(t.accent_bg).text_color(t.accent).flex().items_center().justify_center().child(G_ROBOT))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .child(div().text_color(t.bright).child(name.to_owned()))
                        .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(what.to_owned())),
                )
                .child(div().text_size(px(t.size - 2.5)).text_color(t.muted).child("planned"))
        };
        div()
            .id("agents")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_y_scroll()
            .child(
                div()
                    .px(px(10.))
                    .py(px(10.))
                    .text_color(t.muted)
                    .text_size(px(t.size - 1.))
                    .child("Agents get a Jami account of their own: you chat with them like with a contact, and Joma runs them through ACP (Claude Code, Codex). Not built yet: see docs/discussions/2026-10-31-agents.qmd."),
            )
            .child(card("Claude", "Claude Code through ACP · its own Jami account"))
            .child(card("Codex", "Codex through ACP · its own Jami account"))
            .child(div().px(px(10.)).py(px(8.)).flex().child(button("agent-new", G_PLUS, "new agent", t, false).on_click(cx.listener(|this, _, _, cx| {
                this.show_flash("agents are not built yet: the discussion document says how they could work", cx);
            }))))
            .into_any_element()
    }

    /// The rail: one button per tab, our picture at the bottom.
    fn rail(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let unread: u32 = self.listed_unread();
        let live = !self.model.calls.is_empty() || !self.running_shares_empty();
        let item = |tab: Tab, g: &str, title: &str, badge: Option<String>, dot: bool| {
            let on = self.tab == tab;
            div()
                .id(SharedString::from(format!("rail-{title}")))
                .relative()
                .size(px(38.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .text_size(px(t.size + 3.))
                .text_color(if on { t.accent } else { t.muted })
                .when(on, |d| d.bg(t.accent_bg))
                .cursor_pointer()
                .hover(|d| d.text_color(t.bright).bg(t.raised))
                .child(g.to_owned())
                .when_some(badge, |d, b| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .right(px(1.))
                            .min_w(px(15.))
                            .h(px(14.))
                            .px(px(3.))
                            .rounded(px(7.))
                            .bg(t.accent)
                            .text_color(t.bar)
                            .text_size(px(t.size - 4.))
                            .font_weight(FontWeight::BOLD)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(b),
                    )
                })
                .when(dot, |d| d.child(div().absolute().top(px(5.)).right(px(5.)).size(px(8.)).rounded_full().bg(t.green)))
                .on_click(cx.listener(move |this, _, window, cx| this.set_tab(tab, window, cx)))
        };
        div()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .justify_between()
            .w(px(48.))
            .h_full()
            .py(px(6.))
            .bg(t.bar)
            .border_r_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(item(Tab::Chats, G_COMMENT, "chats", (unread > 0).then(|| unread.min(99).to_string()), false))
                    .child(item(Tab::Calls, G_PHONE, "calls", None, live))
                    .child(item(Tab::Contacts, G_USERS, "contacts", None, false))
                    .child(item(Tab::Devices, G_LAPTOP, "devices", None, self.device_op.is_some()))
                    .child(item(Tab::Accounts, G_USER_CIRCLE, "accounts", None, false))
                    .child(item(Tab::Agents, G_ROBOT, "agents", None, false)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("rail-settings")
                            .size(px(38.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .text_size(px(t.size + 3.))
                            .text_color(if self.settings.is_some() { t.accent } else { t.muted })
                            .cursor_pointer()
                            .hover(|d| d.text_color(t.bright).bg(t.raised))
                            .child(G_COG)
                            .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx))),
                    )
                    .child(
                div()
                    .id("rail-me")
                    .cursor_pointer()
                    .hover(|d| d.opacity(0.8))
                    .child(avatar(t, self.model.avatars.get(&self.model.me).cloned(), &self.model.me, &self.model.my_name, 30., Some(if self.quick.invisible { t.muted } else { t.green })))
                    .on_click(cx.listener(|this, _, _, cx| this.open_profile(cx))),
                    ),
            )
            .into_any_element()
    }

    /// The machine's other accounts: a click opens one.
    fn accounts_group(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let mut group = div().flex().flex_col();
        for (id, name) in self.accounts.iter().filter(|(id, _)| *id != self.model.account) {
            let i = id.clone();
            group = group.child(
                div()
                    .id(SharedString::from(format!("account-{id}")))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(4.))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.border.opacity(0.4)))
                    .child(div().flex_none().text_color(t.muted).child(G_EXCHANGE))
                    .child(div().flex_1().min_w(px(0.)).truncate().child(name.clone()))
                    .on_click(cx.listener(move |this, _, window, cx| this.switch_account(&i, window, cx))),
            );
        }
        group
    }

    /// The Contacts tab: requests waiting for you, contacts (starred first, draggable) with
    /// their quick buttons, and the blocked ones at the bottom.
    fn contacts_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().id("contacts").flex().flex_col().flex_1().overflow_y_scroll();
        let small = |label: &str| div().px(px(10.)).pt(px(8.)).pb(px(2.)).text_size(px(t.size - 2.5)).text_color(t.muted).child(label.to_uppercase());
        let requests: Vec<Conversation> = self.model.ordered().into_iter().filter(|c| c.request).cloned().collect();
        if !requests.is_empty() {
            list = list.child(small("requests"));
            for c in requests {
                let (a, b) = (c.id.clone(), c.id.clone());
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .px(px(10.))
                        .py(px(5.))
                        .child(div().text_color(t.bright).truncate().child(c.title.clone()))
                        .child(
                            div()
                                .flex()
                                .gap(px(4.))
                                .child(button(SharedString::from(format!("cr-ok-{}", c.id)), G_CHECK, "", t, true).on_click(cx.listener(move |this, _, _, cx| this.accept_request(a.clone(), true, cx))))
                                .child(button(SharedString::from(format!("cr-no-{}", c.id)), G_TIMES, "", t, false).on_click(cx.listener(move |this, _, _, cx| this.accept_request(b.clone(), false, cx)))),
                        ),
                );
            }
        }
        let contacts = self.contact_list();
        if contacts.is_empty() {
            list = list.child(div().px(px(10.)).py(px(8.)).text_color(t.muted).child(if self.mode == Mode::Search {
                "no contact matches".to_owned()
            } else {
                format!("no contacts yet · {G_PLUS} adds one")
            }));
        }
        for c in contacts {
            let uri = c.uri.clone();
            let name = self.model.name_of(&uri);
            let starred = self.quick.starred.contains(&uri);
            let waiting = !c.confirmed && self.model.chat_with(&uri).is_none_or(|v| v.request);
            let dot = self.dot_of(&uri);
            let selected = self.model.chat_with(&uri).is_some_and(|v| self.selected.as_deref() == Some(v.id.as_str()));
            let quick = |id: &str, glyph: &str, what: Quick| {
                let u = uri.clone();
                button(SharedString::from(format!("{id}-{uri}")), glyph, "", t, false)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.quick_action(&u, what, window, cx);
                    }))
            };
            let (u_star, u_block, u_remove, u_open) = (uri.clone(), uri.clone(), uri.clone(), uri.clone());
            let block_armed = self.is_armed(&format!("block:{uri}"));
            let remove_armed = self.is_armed(&format!("remove:{uri}"));
            let row = div()
                .id(SharedString::from(format!("contact-{uri}")))
                .flex()
                .flex_col()
                .gap(px(3.))
                .px(px(10.))
                .py(px(6.))
                .border_l_2()
                .border_color(if selected { t.accent } else { gpui::transparent_black() })
                .when(selected, |d| d.bg(t.accent_bg))
                .hover(|d| d.bg(t.raised))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| this.quick_action(&u_open, Quick::Message, window, cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(div().flex_none().text_color(t.muted).text_size(px(t.size - 2.)).child(G_GRIP))
                        .child(avatar(
                            t,
                            self.model.avatars.get(crate::model::bare_uri(&uri)).cloned(),
                            crate::model::bare_uri(&uri),
                            &name,
                            26.,
                            match dot {
                                Dot::Active => Some(t.green),
                                Dot::Reachable => Some(t.green.opacity(0.35)),
                                Dot::None => None,
                            },
                        ))
                        .child(div().flex_1().min_w(px(0.)).text_color(if selected { t.bright } else { t.text }).truncate().child(name.clone()))
                        .when(waiting, |d| d.child(div().text_color(t.yellow).text_size(px(t.size - 2.)).child("waiting")))
                        .child(
                            div()
                                .id(SharedString::from(format!("star-{uri}")))
                                .flex_none()
                                .text_color(if starred { t.yellow } else { t.muted })
                                .hover(|d| d.text_color(t.yellow))
                                .child(if starred { G_STAR } else { G_STAR_O })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.toggle_starred(&u_star, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(4.))
                        .pl(px(13.))
                        .text_size(px(t.size - 1.))
                        .when(!waiting, |d| {
                            d.child(quick("cq-msg", G_COMMENT, Quick::Message))
                                .child(quick("cq-call", G_PHONE, Quick::Call))
                                .child(quick("cq-video", G_VIDEO, Quick::Video))
                                .child(quick("cq-share", G_DESKTOP, Quick::Share))
                                .child(quick("cq-file", G_CLIP, Quick::File))
                        })
                        .child(div().flex_1())
                        .child(
                            button(SharedString::from(format!("cq-block-{uri}")), G_BAN, if block_armed { "block?" } else { "" }, t, block_armed)
                                .text_color(if block_armed { t.red } else { t.muted })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.block(&u_block, cx);
                                })),
                        )
                        .child(
                            button(SharedString::from(format!("cq-remove-{uri}")), G_USER_TIMES, if remove_armed { "remove?" } else { "" }, t, remove_armed)
                                .text_color(if remove_armed { t.red } else { t.muted })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.remove_contact(&u_remove, cx);
                                })),
                        ),
                );
            list = list.child(draggable(row, Dragged { tab: Tab::Contacts, id: uri.clone(), title: name }, t, cx));
        }
        let blocked = self.blocked();
        if !blocked.is_empty() && self.mode != Mode::Search {
            list = list.child(small("blocked"));
            for c in blocked {
                let u = c.uri.clone();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .px(px(10.))
                        .py(px(4.))
                        .child(div().flex().gap(px(6.)).text_color(t.muted).truncate().child(div().text_color(t.red).child(G_BAN)).child(self.model.name_of(&c.uri)))
                        .child(button(SharedString::from(format!("unblock-{}", c.uri)), G_UNDO, "unblock", t, false).on_click(cx.listener(move |this, _, _, cx| this.unblock(&u, cx)))),
                );
            }
        }
        list.into_any_element()
    }

    fn call_panel(&self, t: &Theme, conv: Option<&Conversation>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.active_call()?.clone();
        let here = conv.is_some_and(|c| call.conversation.as_deref() == Some(c.id.as_str()));
        let name = self.model.name_of(&call.peer);
        let elapsed = call.started.map(|s| s.elapsed().as_secs()).map(|s| format!(" {:02}:{:02}", s / 60, s % 60)).unwrap_or_default();
        let session = self.desk.sessions.get(&call.id);
        let sharing = session.and_then(|s| s.sharing_label());
        let viewing = self.desk.viewer(&call.id).is_some();
        let joma_sharing = self.desk.shared(&call.id).is_some();
        let joma_badge = if call.peer_joma { "joma" } else { "jami" };

        if call.state == CallState::Incoming {
            return Some(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .py(px(10.))
                    .bg(t.accent_bg)
                    .border_b_1()
                    .border_color(t.accent)
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(div().text_color(t.accent).child(G_PHONE))
                            .child(div().text_color(t.bright).child(format!(
                                "{name} is calling{}",
                                if call.video { " (video)" } else { "" }
                            ))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(button("acc-audio", G_PHONE, "answer", t, true).on_click(cx.listener(|this, _, _, cx| this.accept_call(false, cx))))
                            .when(call.video, |d| {
                                d.child(button("acc-video", G_VIDEO, "with video", t, false).on_click(cx.listener(|this, _, _, cx| this.accept_call(true, cx))))
                            })
                            .child(
                                button("refuse", G_TIMES, "decline", t, false)
                                    .text_color(t.red)
                                    .on_click(cx.listener(|this, _, _, cx| this.hang_up(cx))),
                            ),
                    )
                    .into_any_element(),
            );
        }

        let controls = div()
            .flex()
            .gap(px(6.))
            .child(
                button("mute", if call.muted_audio { G_MIC_OFF } else { G_MIC }, "", t, call.muted_audio)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_mute(false, cx))),
            )
            .child(
                button("cam", G_VIDEO, "", t, !call.muted_video).on_click(cx.listener(|this, _, _, cx| this.toggle_mute(true, cx))),
            )
            .child(
                button("share", G_DESKTOP, if sharing.is_some() { "stop sharing" } else { "share desktop" }, t, sharing.is_some())
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_share(cx))),
            )
            .when(viewing || !call.sinks.is_empty(), |d| {
                d.child(button("stage", G_EXPAND, "", t, self.stage).on_click(cx.listener(|this, _, _, cx| {
                    this.stage = !this.stage;
                    cx.notify();
                })))
            })
            .child(
                button("hangup", G_PHONE, "hang up", t, false)
                    .text_color(t.red)
                    .border_color(t.red)
                    .on_click(cx.listener(|this, _, _, cx| this.hang_up(cx))),
            );

        let status = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(8.))
            .child(div().text_color(t.green).child(G_PHONE))
            .child(div().text_color(t.bright).child(name.clone()))
            .when(!here, |d| d.child(div().text_color(t.muted).child("(other conversation)")))
            .child(div().text_color(t.muted).child(format!("{}{elapsed}", call.state.label())))
            .child(div().px(px(4.)).border_1().border_color(t.border).text_color(if call.peer_joma { t.accent } else { t.muted }).child(joma_badge))
            .when_some(sharing.filter(|_| !joma_sharing), |d, s| d.child(div().text_color(t.yellow).child(format!("{G_BOLT} {s}"))));

        let mut panel = div()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(t.border)
            .bg(t.panel)
            .child(div().flex().flex_wrap().items_center().justify_between().gap(px(6.)).px(px(12.)).py(px(6.)).child(status).child(controls));
        if let Some(strip) = self.share_strip(t, &call.id, cx) {
            panel = panel.child(strip);
        }
        // The stage: the peer's desktop (Joma), else their video. A share without a call
        // has its own panel.
        let stage = self.stage_source().filter(|_| self.viewing_key().is_none_or(|k| k == call.id));
        if let Some((image, pointer, info)) = stage {
            let stage = self.stage_canvas(t, image, pointer, cx);
            panel = panel
                .when(self.stage, |d| d.flex_1())
                .child(self.stage_box(t, stage, cx))
                .child(self.stage_tools(t, info, cx));
        } else if let Some(note) = session.and_then(|s| s.note.clone()) {
            panel = panel.child(div().px(px(12.)).pb(px(6.)).text_size(px(t.size - 2.)).text_color(t.yellow).child(note));
        }
        Some(panel.into_any_element())
    }

    /// A peer's desktop (or video), fitted or zoomed, with their pointer on top. The wheel
    /// zooms (Ctrl + touchpad too), the touchpad pans, dragging pans, a double click fills
    /// the screen.
    fn stage_canvas(&self, t: &Theme, image: Arc<RenderImage>, pointer: Option<(joma_desk::view::Pointer, Arc<RenderImage>)>, cx: &mut Context<Self>) -> AnyElement {
        let bg = t.bar;
        let view = self.stage_view.clone();
        // What we drew on the shared screen (only while watching one).
        let ink = self.viewing_key().and_then(|k| self.desk.viewer(&k)).map(|v| v.state.lock().unwrap().ink.clone());
        let inking = self.ink_tool != InkTool::None && ink.is_some();
        let painted = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                window.paint_quad(gpui::fill(bounds, bg));
                let sz = image.size(0);
                let (w, h) = (sz.width.0 as f32, sz.height.0 as f32);
                let mut v = view.borrow_mut();
                v.layout(bounds, (w, h), window.scale_factor());
                if let Some((p, _)) = pointer.as_ref().filter(|(p, _)| p.visible) {
                    v.follow_pointer(p.x as f32, p.y as f32);
                }
                let r = v.rect();
                drop(v);
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    let _ = window.paint_image(r, Corners::default(), image.clone(), 0, false);
                    if let Some(c) = &ink {
                        paint_ink(window, c, r);
                    }
                    if let Some((p, cursor)) = &pointer {
                        if p.visible {
                            let k = f32::from(r.size.width) / w.max(1.);
                            let csz = cursor.size(0);
                            let origin = point(
                                r.origin.x + px((p.x as f32 - p.hot_x as f32) * k),
                                r.origin.y + px((p.y as f32 - p.hot_y as f32) * k),
                            );
                            // The pointer keeps a readable size when the picture is shrunk.
                            let ck = k.max(0.6);
                            let cb = Bounds::new(origin, size(px(csz.width.0 as f32 * ck), px(csz.height.0 as f32 * ck)));
                            let _ = window.paint_image(cb, Corners::default(), cursor.clone(), 0, false);
                        }
                    }
                });
            },
        )
        .size_full();
        div()
            .id("stage")
            .size_full()
            .child(painted)
            .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                let mut v = this.stage_view.borrow_mut();
                match e.delta {
                    // A touchpad pans; with Ctrl (or a pinch) it zooms.
                    // Panning by hand stops following the pointer (the tools turn it back on).
                    ScrollDelta::Pixels(d) if !e.modifiers.control => {
                        v.follow = false;
                        v.pan(f32::from(d.x), f32::from(d.y));
                    }
                    ScrollDelta::Pixels(d) => v.zoom_at(e.position, (1. + f32::from(d.y) / 200.).clamp(0.5, 2.)),
                    // A mouse wheel zooms; with Shift it pans sideways.
                    ScrollDelta::Lines(d) if e.modifiers.shift => {
                        v.follow = false;
                        v.pan(d.y * 60., 0.);
                    }
                    ScrollDelta::Lines(d) => v.zoom_at(e.position, 1.2f32.powf(d.y)),
                }
                drop(v);
                cx.notify();
            }))
            .when(inking, |d| d.cursor_crosshair())
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| {
                if e.click_count == 2 {
                    this.stage_view.borrow_mut().drag = None;
                    this.toggle_fullscreen(window, cx);
                    return;
                }
                if this.ink_tool != InkTool::None && this.viewing_key().is_some() {
                    let at = this.stage_view.borrow_mut().fraction(e.position);
                    if let Some((x, y)) = at {
                        this.ink_down(x, y, cx);
                    }
                    return;
                }
                let mut v = this.stage_view.borrow_mut();
                v.drag = Some((e.position, v.center));
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.ink_tool == InkTool::Laser {
                    this.ink(joma_desk::ink::Ink::LaserOff, cx);
                }
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.ink_tool != InkTool::None && this.viewing_key().is_some() {
                    let at = this.stage_view.borrow_mut().fraction(e.position);
                    match at {
                        Some((x, y)) => this.ink_move(x, y, e.pressed_button == Some(MouseButton::Left), cx),
                        None if this.ink_tool == InkTool::Laser => this.ink(joma_desk::ink::Ink::LaserOff, cx),
                        None => {}
                    }
                    if e.pressed_button != Some(MouseButton::Left) {
                        this.ink_stroke = None;
                    }
                    return;
                }
                let mut v = this.stage_view.borrow_mut();
                if v.drag.is_some() {
                    if e.pressed_button == Some(MouseButton::Left) {
                        v.follow = false;
                        v.drag_to(e.position);
                        drop(v);
                        cx.notify();
                    } else {
                        v.drag = None;
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| {
                this.stage_view.borrow_mut().drag = None;
                this.ink_stroke = None;
            }))
            .into_any_element()
    }

    /// The stage in the call or share panel: as tall as set, with a bar under it to drag
    /// (double click: the stage takes the whole window), or the whole window.
    fn stage_box(&self, t: &Theme, stage: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        if self.stage {
            return div().relative().flex_1().min_h(px(0.)).child(stage).into_any_element();
        }
        let dragging = self.resizing.is_some();
        div()
            .flex()
            .flex_col()
            .flex_none()
            .child(div().relative().h(px(self.stage_height)).child(stage))
            .child(
                div()
                    .id("stage-resize")
                    .h(px(7.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_row_resize()
                    .bg(if dragging { t.accent_bg } else { t.bar })
                    .hover(|d| d.bg(t.accent_bg))
                    .child(div().w(px(36.)).h(px(2.)).bg(if dragging { t.accent } else { t.border }))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| {
                        if e.click_count == 2 {
                            this.resizing = None;
                            this.stage = true;
                        } else {
                            this.resizing = Some((f32::from(e.position.y), this.stage_height));
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Under the stage: what it shows, and the zoom.
    fn stage_tools(&self, t: &Theme, info: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        use crate::stage::Zoom;
        let (zoom, pct, follow, zoomed) = {
            let v = self.stage_view.borrow();
            (v.zoom, v.percent(), v.follow, v.zoomed())
        };
        let small = |id: &'static str, glyph: &str, label: &str, on: bool| button(id, glyph, label, t, on);
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(6.))
            .px(px(12.))
            .py(px(3.))
            .text_size(px(t.size - 2.))
            .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.muted).child(info.unwrap_or_default()))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.))
                    .when(self.viewing_key().is_some(), |d| {
                        let pen = self.ink_tool == InkTool::Pen;
                        let mut d = d
                            .child(small("ink-laser", G_POINTER, "laser", self.ink_tool == InkTool::Laser).on_click(cx.listener(|this, _, _, cx| this.set_ink_tool(InkTool::Laser, cx))))
                            .child(small("ink-pen", G_PENCIL, "pen", pen).on_click(cx.listener(|this, _, _, cx| this.set_ink_tool(InkTool::Pen, cx))));
                        if pen {
                            for (i, c) in [0xff3355u32, 0xffcc00, 0x33dd77, 0x33bbff].into_iter().enumerate() {
                                let on = self.ink_color == c;
                                d = d.child(
                                    div()
                                        .id(SharedString::from(format!("ink-c{i}")))
                                        .size(px(16.))
                                        .rounded_full()
                                        .bg(gpui::rgb(c))
                                        .border_2()
                                        .border_color(if on { t.bright } else { gpui::transparent_black() })
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.ink_color = c;
                                            cx.notify();
                                        })),
                                );
                            }
                        }
                        d.child(small("ink-clear", G_ERASER, "clear", false).on_click(cx.listener(|this, _, _, cx| this.clear_ink(None, cx))))
                            .child(div().w(px(1.)).h(px(16.)).mx(px(2.)).bg(t.border))
                    })
                    .child(small("z-fit", "", "fit", zoom == Zoom::Fit).on_click(cx.listener(|this, _, _, cx| this.zoom(Zoom::Fit, cx))))
                    .child(small("z-fill", "", "fill", zoom == Zoom::Fill).on_click(cx.listener(|this, _, _, cx| this.zoom(Zoom::Fill, cx))))
                    .child(small("z-1", "", "1:1", zoom == Zoom::Actual).on_click(cx.listener(|this, _, _, cx| this.zoom(Zoom::Actual, cx))))
                    .child(small("z-out", G_ZOOM_OUT, "", false).on_click(cx.listener(|this, _, _, cx| this.zoom_by(1. / 1.25, cx))))
                    .child(div().w(px(44.)).flex().justify_center().text_color(t.text).child(format!("{pct}%")))
                    .child(small("z-in", G_ZOOM_IN, "", false).on_click(cx.listener(|this, _, _, cx| this.zoom_by(1.25, cx))))
                    .child(
                        small("z-follow", G_CROSSHAIRS, "follow", follow && zoomed).on_click(cx.listener(|this, _, _, cx| {
                            let mut v = this.stage_view.borrow_mut();
                            v.follow = !v.follow;
                            drop(v);
                            cx.notify();
                        })),
                    )
                    .child(
                        small("z-full", if self.fullscreen { G_COMPRESS } else { G_EXPAND }, if self.fullscreen { "exit" } else { "full screen" }, self.fullscreen)
                            .on_click(cx.listener(|this, _, window, cx| this.toggle_fullscreen(window, cx))),
                    ),
            )
            .into_any_element()
    }

    /// What the stage shows: the desktop being watched, else the call's video; with the
    /// peer's pointer and a line about it.
    fn stage_source(&self) -> Option<(Arc<RenderImage>, Option<(joma_desk::view::Pointer, Arc<RenderImage>)>, Option<String>)> {
        if let (Some(key), Some(f)) = (self.viewing_key(), self.desk_frame.as_ref()) {
            let v = self.desk.viewer(&key)?;
            let st = v.state.lock().unwrap();
            let who = crate::desk::direct_peer(&key)
                .map(str::to_owned)
                .or_else(|| self.model.calls.get(&key).map(|c| c.peer.clone()))
                .map(|p| self.model.name_of(&p))
                .unwrap_or_default();
            let (w, h) = st.picture.as_ref().map_or((0, 0), |p| (p.width, p.height));
            let info = format!("{G_EYE} {who}'s screen · {w}×{h} · {} · {}→{} · {:.0} fps", st.chroma, st.remote_pipeline, st.local_pipeline, st.fps);
            let pointer = self.pointer.as_ref().map(|p| (st.pointer.clone(), p.image.clone()));
            return Some((f.image.clone(), pointer, Some(info)));
        }
        let call = self.active_call()?;
        let video = call.sinks.iter().find_map(|s| self.frames.get(s))?;
        Some((video.image.clone(), None, Some(format!("{G_VIDEO} {}", self.model.name_of(&call.peer)))))
    }

    /// The stage alone, filling the screen, with its tools below.
    fn fullscreen_stage(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (image, pointer, info) = self.stage_source()?;
        Some(
            div()
                .flex()
                .flex_col()
                .size_full()
                .bg(t.bar)
                .child(div().flex_1().min_h(px(0.)).child(self.stage_canvas(t, image, pointer, cx)))
                .child(div().bg(t.panel).border_t_1().border_color(t.border).child(self.stage_tools(t, info.map(|i| format!("{i} · Esc to leave")), cx)))
                .into_any_element(),
        )
    }

    /// The composer with its buttons: emoji, YouTube, voice and video messages; while
    /// recording, the recording's time with send and cancel.
    fn composer_row(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let row = div().flex().items_center().gap(px(6.)).px(px(12.)).py(px(8.));
        if let Some(r) = &self.recording {
            let secs = r.started.elapsed().as_secs();
            return row
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .h(px(t.size + 20.))
                        .px(px(10.))
                        .border_1()
                        .border_color(t.red)
                        .child(div().size(px(9.)).rounded_full().bg(t.red))
                        .child(div().text_color(t.bright).child(format!("recording a {} message · {}:{:02}", if r.video { "video" } else { "voice" }, secs / 60, secs % 60))),
                )
                .child(button("rec-send", G_CHECK, "send", t, true).on_click(cx.listener(|this, _, _, cx| this.stop_recording(true, cx))))
                .child(button("rec-cancel", G_TIMES, "", t, false).on_click(cx.listener(|this, _, _, cx| this.stop_recording(false, cx))))
                .into_any_element();
        }
        row.child(div().flex_1().min_w(px(0.)).child(self.composer.clone()))
            .child(button("emoji", G_SMILE, "", t, self.emoji_open).on_click(cx.listener(|this, _, _, cx| {
                this.emoji_open = !this.emoji_open;
                cx.notify();
            })))
            // YouTube: the message box becomes the search box (`@youtube …`, then Enter).
            .child(button("yt", G_YOUTUBE, "", t, false).on_click(cx.listener(|this, _, _, cx| {
                this.emoji_open = false;
                this.composer.update(cx, |c, cx| c.set_text("@youtube ", cx));
                this.focus_composer = true;
                cx.notify();
            })))
            .child(button("rec-voice", G_MIC, "", t, false).on_click(cx.listener(|this, _, _, cx| this.start_recording(false, cx))))
            .child(button("rec-video", G_VIDEO, "", t, false).on_click(cx.listener(|this, _, _, cx| this.start_recording(true, cx))))
            .into_any_element()
    }

    fn emoji_popover(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.emoji_open {
            return None;
        }
        let mut grid = div().flex().flex_wrap().gap(px(2.));
        for (i, e) in EMOJI.iter().enumerate() {
            let e2 = *e;
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("emo-{i}")))
                    .size(px(30.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(t.size + 5.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.raised))
                    .child(e2)
                    .on_click(cx.listener(move |this, _, _, cx| this.insert_emoji(e2, cx))),
            );
        }
        Some(div().mx(px(12.)).mt(px(6.)).p(px(6.)).border_1().border_color(t.border).bg(t.panel).max_h(px(200.)).child(div().id("emoji-scroll").overflow_y_scroll().max_h(px(188.)).child(grid)).into_any_element())
    }

    /// A YouTube link's card: thumbnail, title and channel; a click opens it.
    fn youtube_card(&self, t: &Theme, id: &str, msg_id: &str, cx: &mut Context<Self>) -> AnyElement {
        let p = self.youtube_preview(id).unwrap_or_default();
        let url = crate::media::watch_url(id);
        let open = url.clone();
        div()
            .id(SharedString::from(format!("ytc-{msg_id}")))
            .mt(px(4.))
            .flex()
            .gap(px(10.))
            .w(px(470.))
            .max_w_full()
            .p(px(6.))
            .border_1()
            .border_color(t.border)
            .border_l_2()
            .bg(t.panel)
            .cursor_pointer()
            .hover(|d| d.bg(t.raised))
            .child(
                div()
                    .flex_none()
                    .w(px(128.))
                    .h(px(72.))
                    .bg(t.bar)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(t.red)
                    .when_some(p.thumb.clone(), |d, th| d.child(gpui::img(th).w(px(128.)).h(px(72.)).object_fit(ObjectFit::Cover)))
                    .when(p.thumb.is_none(), |d| d.child(G_YOUTUBE)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(2.))
                    .child(div().text_color(t.bright).child(if p.title.is_empty() { "YouTube video".to_owned() } else { p.title.clone() }))
                    .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(p.channel.clone()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_size(px(t.size - 2.)).text_color(t.accent).child("open in the browser"))
                            .child({
                                let (u, title) = (url.clone(), p.title.clone());
                                let (u2, title2) = (url.clone(), p.title.clone());
                                let session = self.selected.as_ref().is_some_and(|c| self.watches.get(c).is_some_and(|w| w.joined));
                                div()
                                    .flex()
                                    .gap(px(4.))
                                    .child(button(SharedString::from(format!("ytw-{msg_id}")), if session { G_EXCHANGE } else { G_USERS }, if session { "switch" } else { "together" }, t, false).on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.start_watch(&u, &title, cx);
                                    })))
                                    .when(session, |d| {
                                        d.child(button(SharedString::from(format!("ytq-{msg_id}")), G_PLUS, "queue", t, false).on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.queue_watch(&u2, &title2, cx);
                                        })))
                                    })
                            }),
                    ),
            )
            .on_click(cx.listener(move |_, _, _, _| {
                let _ = std::process::Command::new("xdg-open").arg(&open).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
            }))
            .into_any_element()
    }

    /// Files waiting to go with the next Enter: a picture shows itself, a file its name.
    fn attachment_strip(&self, t: &Theme, conv: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let files = self.attachments.get(conv).filter(|f| !f.is_empty())?;
        let items = files.iter().enumerate().map(|(i, path)| {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            let c = conv.to_owned();
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .p(px(4.))
                .border_1()
                .border_color(t.border)
                .bg(t.panel)
                .child(if is_picture(path) {
                    div().size(px(56.)).child(gpui::img(path.clone()).size(px(56.)).object_fit(ObjectFit::Contain)).into_any_element()
                } else {
                    div().text_color(t.muted).child(G_CLIP).into_any_element()
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .max_w(px(180.))
                        .child(div().truncate().text_color(t.bright).child(name))
                        .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(size_label(size))),
                )
                .child(
                    button(SharedString::from(format!("unattach-{i}")), G_TIMES, "", t, false)
                        .on_click(cx.listener(move |this, _, _, cx| this.unattach(&c, i, cx))),
                )
        });
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .pt(px(8.))
                .children(items)
                .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child("Enter sends them, with the text if any"))
                .into_any_element(),
        )
    }

    /// What we share and how it looks to the peer, with a way to change or stop it.
    fn share_strip(&self, t: &Theme, key: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::desk::Sharing;
        let s = self.desk.sessions.get(key)?;
        let (what, joma) = match &s.sharing {
            Sharing::Joma { share, .. } => (source_label(&share.source, &share.screen), true),
            Sharing::Portal { .. } => ("the screen or window chosen in the portal (Jami's share)".to_owned(), false),
            Sharing::Off | Sharing::Picking => return None,
        };
        let state = s.sharing_label().unwrap_or_default();
        let preview = self.previews.get(key).map(|f| f.image.clone());
        let drawn = self.desk.shared(key).is_some_and(|(sh, _)| !sh.ink.lock().unwrap().canvas.is_empty());
        let bg = t.bar;
        let thumb = div()
            .flex_none()
            .w(px(160.))
            .h(px(90.))
            .border_1()
            .border_color(t.yellow)
            .bg(t.bar)
            .when_some(preview, |d, image| {
                d.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            window.paint_quad(gpui::fill(bounds, bg));
                            let sz = image.size(0);
                            let r = fit(bounds, sz.width.0 as f32, sz.height.0 as f32);
                            let _ = window.paint_image(r, Corners::default(), image.clone(), 0, false);
                        },
                    )
                    .size_full(),
                )
            })
            .when(!joma, |d| d.flex().items_center().justify_center().text_color(t.muted).child(G_DESKTOP));
        let k1 = key.to_owned();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(6.))
                .border_t_1()
                .border_color(t.border)
                .child(thumb)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(2.))
                        .child(div().text_color(t.yellow).child(format!("{G_BOLT} {state}")))
                        .child(div().truncate().text_color(t.bright).child(format!("you share: {what}")))
                        .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(if joma { "the picture is what they see, about once a second" } else { "Jami's share shows no preview here" })),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(6.))
                        .when(drawn, |d| {
                            let k = key.to_owned();
                            d.child(button(SharedString::from(format!("inkclr-{key}")), G_ERASER, "clear drawings", t, false).on_click(cx.listener(move |this, _, _, cx| this.clear_ink(Some(&k), cx))))
                        })
                        .when(joma, |d| {
                            d.child(button(SharedString::from(format!("src-{key}")), G_EXCHANGE, "change", t, false).on_click(cx.listener(move |this, _, _, cx| this.change_source(&k1, cx))))
                        })
                        // A share without a call has its stop in the panel's header.
                        .when(crate::desk::direct_peer(key).is_none(), |d| {
                            d.child(button(SharedString::from(format!("unshare-{key}")), G_TIMES, "stop", t, false).text_color(t.red).on_click(cx.listener(|this, _, _, cx| this.toggle_share(cx))))
                        }),
                )
                .into_any_element(),
        )
    }

    /// Settings: audio and video devices, sound processing, video quality, the share.
    fn settings_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let v = self.settings.clone()?;
        let section = |title: &str| div().pt(px(12.)).pb(px(4.)).text_size(px(t.size - 2.5)).text_color(t.muted).child(title.to_uppercase());
        let label = |text: &str| div().w(px(120.)).flex_none().pt(px(4.)).text_color(t.text).child(text.to_owned());
        let row = |l: gpui::Div, chips: gpui::Div| div().flex().gap(px(10.)).py(px(3.)).child(l).child(chips.flex().flex_wrap().gap(px(4.)).flex_1().min_w(px(0.)));
        // A row of choices; `pick` runs with the chosen value.
        fn chips(t: &Theme, cx: &mut Context<Joma>, id: &str, options: Vec<(String, String)>, current: &str, pick: fn(&mut Joma, &str, &mut Context<Joma>)) -> gpui::Div {
            let mut d = div();
            for (i, (value, text)) in options.into_iter().enumerate() {
                let on = value == current;
                d = d.child(button(SharedString::from(format!("{id}-{i}")), "", &text, t, on).on_click(cx.listener(move |this, _, _, cx| pick(this, &value, cx))));
            }
            d
        }
        let idx = |i: usize| v.current.get(i).copied().unwrap_or(0).to_string();
        let devices = |list: &[String]| list.iter().enumerate().map(|(i, n)| (i.to_string(), if n.is_empty() { format!("device {i}") } else { n.clone() })).collect::<Vec<_>>();
        let processing = vec![("auto".to_owned(), "auto".to_owned()), ("audioProcessor".into(), "Jami's".into()), ("system".into(), "system".into()), ("none".into(), "off".into())];
        let size_now = v.camera_settings.get("size").cloned().unwrap_or_default();
        let rate_now = v.camera_settings.get("rate").cloned().unwrap_or_default();
        let rates: Vec<(String, String)> = v
            .sizes
            .iter()
            .find(|(s, _)| *s == size_now)
            .map(|(_, r)| r.iter().map(|x| (x.clone(), format!("{} fps", x.split('.').next().unwrap_or(x)))).collect())
            .unwrap_or_default();
        let body = div()
            .id("settings-body")
            .flex()
            .flex_col()
            .px(px(16.))
            .pb(px(16.))
            .max_h(px(560.))
            .overflow_y_scroll()
            .child(section("audio"))
            .child(row(label("microphone"), chips(t, cx, "ain", devices(&v.inputs), &idx(1), |j, x, cx| j.set_audio("input", x.parse().unwrap_or(0), cx))))
            .child(row(label("speakers"), chips(t, cx, "aout", devices(&v.outputs), &idx(0), |j, x, cx| j.set_audio("output", x.parse().unwrap_or(0), cx))))
            .child(row(label("ringtone on"), chips(t, cx, "aring", devices(&v.outputs), &idx(2), |j, x, cx| j.set_audio("ringtone", x.parse().unwrap_or(0), cx))))
            .child(row(label("noise removal"), chips(t, cx, "ns", processing.clone(), &v.noise, |j, x, cx| j.set_processing("noise", x, cx))))
            .child(row(label("echo removal"), chips(t, cx, "ec", processing, &v.echo, |j, x, cx| j.set_processing("echo", x, cx))))
            .child(row(
                label("auto gain"),
                chips(t, cx, "agc", vec![("on".into(), "on".into()), ("off".into(), "off".into())], if v.agc { "on" } else { "off" }, |j, x, cx| j.set_processing("agc", x, cx)),
            ))
            .child(section("video"))
            .when(v.cameras.is_empty(), |d| d.child(div().py(px(3.)).text_color(t.muted).child("no camera found")))
            .when(!v.cameras.is_empty(), |d| {
                d.child(row(label("camera"), chips(t, cx, "cam", v.cameras.iter().map(|c| (c.clone(), c.clone())).collect(), &v.camera, |j, x, cx| j.set_camera(x, cx))))
                    .child(row(label("resolution"), chips(t, cx, "size", v.sizes.iter().take(8).map(|(s, _)| (s.clone(), s.clone())).collect(), &size_now, |j, x, cx| j.set_camera_mode(Some(x), None, cx))))
                    .child(row(label("frame rate"), chips(t, cx, "rate", rates, &rate_now, |j, x, cx| j.set_camera_mode(None, Some(x), cx))))
            })
            .child(row(
                label("call quality"),
                chips(
                    t,
                    cx,
                    "vq",
                    vec![("auto".into(), "auto".into()), ("low".into(), "low · 0.5 Mb/s".into()), ("medium".into(), "medium · 1.5".into()), ("high".into(), "high · 4".into())],
                    &self.quick.video_quality,
                    |j, x, cx| j.set_video_quality(x, cx),
                ),
            ))
            .child(section("desktop share (Joma)"))
            .child(row(
                label("frame rate"),
                chips(t, cx, "sfps", vec![("30".into(), "30 fps · lighter".into()), ("60".into(), "60 fps · smoother".into())], &self.quick.share_fps.to_string(), |j, x, cx| {
                    j.set_share_fps(x.parse().unwrap_or(60), cx)
                }),
            ))
            .child(div().pt(px(10.)).text_size(px(t.size - 2.)).text_color(t.muted).child("Devices and sound apply to the next call; the share's frame rate to the next share."));
        Some(
            div()
                .id("settings-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.55))
                .occlude()
                .child(
                    div()
                        .id("settings-box")
                        .flex()
                        .flex_col()
                        .w(px(640.))
                        .max_w_full()
                        .m(px(16.))
                        .bg(t.bg)
                        .border_1()
                        .border_color(t.accent)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .px(px(12.))
                                .py(px(8.))
                                .border_b_1()
                                .border_color(t.border)
                                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Settings"))
                                .child(button("set-close", G_TIMES, "", t, false).on_click(cx.listener(|this, _, _, cx| {
                                    this.settings = None;
                                    cx.notify();
                                }))),
                        )
                        .child(body),
                )
                .into_any_element(),
        )
    }

    /// Our picture: choose one, draw one in pixels, or remove it.
    fn profile_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.profile_edit.as_ref()?;
        let me = self.model.me.clone();
        let current = self.model.avatars.get(&me).cloned();
        let body: AnyElement = match &p.drawing {
            None => div()
                .flex()
                .items_center()
                .gap(px(18.))
                .p(px(16.))
                .child(avatar(t, current.clone(), &me, &self.model.my_name, 112., None))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(self.model.my_name.clone()))
                        .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child("Contacts see it next to your name."))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(6.))
                                .child(button("pf-pick", G_IMAGE, "picture…", t, false).on_click(cx.listener(|this, _, _, cx| this.choose_picture(cx))))
                                .child(button("pf-draw", G_PENCIL, "draw pixels", t, false).on_click(cx.listener(|this, _, _, cx| this.start_drawing(cx))))
                                .when(current.is_some(), |d| {
                                    d.child(button("pf-remove", G_TRASH, "remove", t, false).text_color(t.red).on_click(cx.listener(|this, _, _, cx| this.remove_avatar(cx))))
                                }),
                        ),
                )
                .into_any_element(),
            Some(art) => self.pixel_editor(t, p, art.clone(), cx),
        };
        Some(
            div()
                .id("profile-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.55))
                .occlude()
                .child(
                    div()
                        .id("profile-box")
                        .flex()
                        .flex_col()
                        .m(px(16.))
                        .bg(t.bg)
                        .border_1()
                        .border_color(t.accent)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .px(px(12.))
                                .py(px(8.))
                                .border_b_1()
                                .border_color(t.border)
                                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(if p.drawing.is_some() { "Draw your picture" } else { "Your picture" }))
                                .child(button("pf-close", G_TIMES, "", t, false).on_click(cx.listener(|this, _, _, cx| {
                                    this.profile_edit = None;
                                    cx.notify();
                                }))),
                        )
                        .child(body),
                )
                .into_any_element(),
        )
    }

    /// A 16×16 grid to paint on, the colour grid, the tools and a live preview.
    fn pixel_editor(&self, t: &Theme, p: &ProfileEdit, art: crate::avatar::PixelArt, cx: &mut Context<Self>) -> AnyElement {
        use crate::avatar::GRID;
        let cell = 18.;
        let side = cell * GRID as f32;
        let bounds = p.grid.clone();
        let line = t.border.opacity(0.35);
        let art2 = art.clone();
        let grid = canvas(
            |_, _, _| {},
            move |b, _, window, _| {
                bounds.set(b);
                let c = f32::from(b.size.width) / GRID as f32;
                for y in 0..GRID {
                    for x in 0..GRID {
                        let rgb = art2.get(x, y).unwrap_or(art2.background);
                        let r = Bounds::new(point(b.origin.x + px(x as f32 * c), b.origin.y + px(y as f32 * c)), size(px(c), px(c)));
                        window.paint_quad(gpui::fill(r, gpui::rgb(rgb)));
                    }
                }
                for i in 0..=GRID {
                    let o = px(i as f32 * c);
                    window.paint_quad(gpui::fill(Bounds::new(point(b.origin.x + o, b.origin.y), size(px(1.), b.size.height)), line));
                    window.paint_quad(gpui::fill(Bounds::new(point(b.origin.x, b.origin.y + o), size(b.size.width, px(1.))), line));
                }
            },
        )
        .size(px(side));
        let grid = div()
            .id("pixel-grid")
            .size(px(side))
            .cursor_crosshair()
            .child(grid)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, _, cx| {
                if let Some(p) = this.profile_edit.as_mut() {
                    p.painting = p.tool != PaintTool::Fill;
                }
                this.paint_at(e.position, cx);
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                let painting = this.profile_edit.as_ref().is_some_and(|p| p.painting);
                if painting && e.pressed_button == Some(MouseButton::Left) {
                    this.paint_at(e.position, cx);
                } else if painting {
                    if let Some(p) = this.profile_edit.as_mut() {
                        p.painting = false;
                    }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| {
                if let Some(p) = this.profile_edit.as_mut() {
                    p.painting = false;
                }
            }));
        let mut swatches = Vec::new();
        for (r, row) in crate::avatar::palette().into_iter().enumerate() {
            let mut line = div().flex();
            for (i, c) in row.into_iter().enumerate() {
                line = line.child(
                    div()
                        .id(SharedString::from(format!("sw-{r}-{i}")))
                        .size(px(20.))
                        .bg(gpui::rgb(c))
                        .border_1()
                        .border_color(if c == p.color { t.bright } else { gpui::black().opacity(0.25) })
                        .cursor_pointer()
                        .hover(|d| d.border_color(gpui::white()))
                        .on_click(cx.listener(move |this, e: &gpui::ClickEvent, _, cx| {
                            if let Some(p) = this.profile_edit.as_mut() {
                                // Shift picks the background behind empty cells.
                                if e.modifiers().shift {
                                    if let Some(a) = p.drawing.as_mut() {
                                        a.background = c;
                                    }
                                } else {
                                    p.color = c;
                                    if p.tool == PaintTool::Eraser {
                                        p.tool = PaintTool::Pen;
                                    }
                                }
                            }
                            cx.notify();
                        })),
                );
            }
            swatches.push(line);
        }
        fn tool(t: &Theme, p: &ProfileEdit, cx: &mut Context<Joma>, id: &'static str, g: &str, label: &str, which: PaintTool) -> gpui::Stateful<gpui::Div> {
            button(id, g, label, t, p.tool == which).on_click(cx.listener(move |this, _, _, cx| {
                if let Some(p) = this.profile_edit.as_mut() {
                    p.tool = which;
                }
                cx.notify();
            }))
        }
        let preview = div()
            .size(px(64.))
            .rounded(px(3.))
            .overflow_hidden()
            .child({
                let a = art.clone();
                canvas(
                    |_, _, _| {},
                    move |b, _, window, _| {
                        let c = f32::from(b.size.width) / GRID as f32;
                        for y in 0..GRID {
                            for x in 0..GRID {
                                let r = Bounds::new(point(b.origin.x + px(x as f32 * c), b.origin.y + px(y as f32 * c)), size(px(c + 0.5), px(c + 0.5)));
                                window.paint_quad(gpui::fill(r, gpui::rgb(a.get(x, y).unwrap_or(a.background))));
                            }
                        }
                    },
                )
                .size_full()
            });
        div()
            .flex()
            .gap(px(16.))
            .p(px(16.))
            .child(grid)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(tool(t, p, cx, "pt-pen", G_PENCIL, "pen", PaintTool::Pen))
                            .child(tool(t, p, cx, "pt-erase", G_ERASER, "eraser", PaintTool::Eraser))
                            .child(tool(t, p, cx, "pt-fill", G_FILL, "fill", PaintTool::Fill)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(button("pt-mirror", G_MIRROR, "mirror", t, p.mirror).on_click(cx.listener(|this, _, _, cx| {
                                if let Some(p) = this.profile_edit.as_mut() {
                                    p.mirror = !p.mirror;
                                }
                                cx.notify();
                            })))
                            .child(button("pt-clear", G_TRASH, "clear", t, false).on_click(cx.listener(|this, _, _, cx| {
                                if let Some(a) = this.profile_edit.as_mut().and_then(|p| p.drawing.as_mut()) {
                                    a.cells.iter_mut().for_each(|c| *c = None);
                                }
                                cx.notify();
                            }))),
                    )
                    .child(div().flex().items_center().gap(px(8.)).child(div().size(px(22.)).bg(gpui::rgb(p.color)).border_1().border_color(t.bright)).child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(format!("#{:06x} · shift+click: background", p.color))))
                    .child(div().flex().flex_col().children(swatches))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .justify_between()
                            .gap(px(10.))
                            .child(preview)
                            .child(
                                div()
                                    .flex()
                                    .gap(px(6.))
                                    .child(button("pt-cancel", G_BACK, "back", t, false).on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(p) = this.profile_edit.as_mut() {
                                            p.drawing = None;
                                        }
                                        cx.notify();
                                    })))
                                    .child(button("pt-save", G_CHECK, "use it", t, true).on_click(cx.listener(|this, _, _, cx| this.save_drawing(cx)))),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Choosing what to share: every screen and window, with a thumbnail.
    fn picker_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.picker.as_ref()?;
        let bg = t.bar;
        let title = match &p.target {
            PickFor::Switch(_) => format!("Share something else with {}", p.peer),
            _ => format!("What do you want to show {}?", p.peer),
        };
        let cards = p.sources.iter().enumerate().map(|(i, s)| {
            let thumb = p.thumbs.get(&s.source.key()).cloned();
            let window_src = matches!(s.source, joma_desk::share::Source::Window { .. });
            let glyph = if window_src { G_WINDOW } else { G_DESKTOP };
            let accent = if s.focused { t.accent } else { t.border };
            div()
                .id(SharedString::from(format!("pick-{i}")))
                .flex()
                .flex_col()
                .w(px(236.))
                .p(px(6.))
                .gap(px(4.))
                .border_1()
                .border_color(accent)
                .bg(t.panel)
                .cursor_pointer()
                .hover(|d| d.border_color(t.accent).bg(t.raised))
                .child(
                    div()
                        .w(px(222.))
                        .h(px(125.))
                        .bg(t.bar)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(t.muted)
                        .when_some(thumb.clone(), |d, image| {
                            d.child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, _, window, _| {
                                        window.paint_quad(gpui::fill(bounds, bg));
                                        let sz = image.size(0);
                                        let r = fit(bounds, sz.width.0 as f32, sz.height.0 as f32);
                                        let _ = window.paint_image(r, Corners::default(), image.clone(), 0, false);
                                    },
                                )
                                .size_full(),
                            )
                        })
                        .when(thumb.is_none(), |d| d.child("…")),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .text_color(t.bright)
                        .child(div().text_color(if s.focused { t.accent } else { t.muted }).child(glyph))
                        .child(div().flex_1().min_w(px(0.)).truncate().child(s.label.clone()))
                        .child(div().flex_none().text_size(px(t.size - 2.)).text_color(t.muted).child(format!("{}×{}", s.screen.width, s.screen.height))),
                )
                .child(div().truncate().text_size(px(t.size - 2.)).text_color(t.muted).child(if s.detail.is_empty() { " ".to_owned() } else { s.detail.clone() }))
                .on_click(cx.listener(move |this, _, window, cx| this.pick(Some(i), window, cx)))
        });
        let portal = matches!(p.target, PickFor::Call(_));
        Some(
            div()
                .id("picker")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.55))
                .occlude()
                .child(
                    div()
                        .id("picker-box")
                        .flex()
                        .flex_col()
                        .max_w(px(1040.))
                        .max_h_full()
                        .m(px(16.))
                        .bg(t.bg)
                        .border_1()
                        .border_color(t.accent)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .px(px(12.))
                                .py(px(8.))
                                .border_b_1()
                                .border_color(t.border)
                                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(title))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(6.))
                                        .child(button("pick-area", G_CROP, "area…", t, false).on_click(cx.listener(|this, _, window, cx| this.pick_area(window, cx))))
                                        .when(portal, |d| {
                                            d.child(button("pick-portal", G_EXCHANGE, "Jami's share", t, false).on_click(cx.listener(|this, _, window, cx| this.pick(None, window, cx))))
                                        })
                                        .child(button("pick-cancel", G_TIMES, "cancel", t, false).on_click(cx.listener(|this, _, window, cx| this.close_picker(window, cx)))),
                                ),
                        )
                        .child(
                            div()
                                .id("picker-list")
                                .overflow_y_scroll()
                                .flex()
                                .flex_wrap()
                                .gap(px(10.))
                                .p(px(12.))
                                .children(cards),
                        )
                        .child(div().px(px(12.)).pb(px(8.)).text_size(px(t.size - 2.)).text_color(t.muted).child("A window is sent alone, even when covered; area… lets you draw a rectangle. You can change it while sharing. Esc closes.")),
                )
                .into_any_element(),
        )
    }

    /// Watching together in the open conversation: the invitation, or the session with
    /// its queue and the proposals to vote on.
    fn watch_panels(&self, t: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::watch::Action;
        let Some(conv) = self.selected.clone() else { return Vec::new() };
        let Some(w) = self.watches.get(&conv) else { return Vec::new() };
        let me = self.model.me.clone();
        let leader = self.model.name_of(&w.leader);
        let mut panel = div().flex().flex_col().gap(px(4.)).px(px(12.)).py(px(6.)).border_b_1().border_color(t.border).bg(if w.joined { t.panel } else { t.accent_bg });
        let (c1, c2) = (conv.clone(), conv.clone());
        if !w.joined {
            return vec![panel
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.bright).child(format!("{leader} invites you to watch together: {}", w.current.title)))
                        .child(button("w-join", G_PLAY, "join", t, true).on_click(cx.listener(move |this, _, _, cx| this.join_watch(&c1, cx))))
                        .child(button("w-no", G_TIMES, "no", t, false).on_click(cx.listener(move |this, _, _, cx| this.leave_watch(&c2, cx)))),
                )
                .into_any_element()];
        }
        let n = w.participants.len();
        let playing = w.playing;
        let pos = w.position();
        let (c3, c4, c5, c6) = (conv.clone(), conv.clone(), conv.clone(), conv.clone());
        panel = panel.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().text_color(t.green).child(if playing { G_PLAY } else { G_PAUSE }))
                .child(div().flex_none().text_size(px(t.size - 2.)).text_color(t.muted).child(format!("{n} watching · {}", crate::watch::clock(pos))))
                .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.bright).child(w.current.title.clone()))
                .child(button("w-pp", if playing { G_PAUSE } else { G_PLAY }, if playing { "pause" } else { "play" }, t, false).on_click(cx.listener(move |this, _, _, cx| {
                    let (playing, pos) = this.watches.get(&c3).map(|w| (w.playing, w.position())).unwrap_or_default();
                    this.propose(&c3, if playing { Action::Pause { pos } } else { Action::Play { pos } }, cx);
                })))
                .when(!w.queue.is_empty(), |d| d.child(button("w-skip", G_FORWARD, "next", t, false).on_click(cx.listener(move |this, _, _, cx| this.propose(&c4, Action::Skip, cx)))))
                .child(button("w-stop", G_STOP, "end", t, false).on_click(cx.listener(move |this, _, _, cx| this.propose(&c5, Action::Stop, cx))))
                .child(button("w-leave", G_TIMES, "leave", t, false).text_color(t.red).on_click(cx.listener(move |this, _, _, cx| this.leave_watch(&c6, cx)))),
        );
        if !w.queue.is_empty() {
            let list = w.queue.iter().enumerate().map(|(i, q)| format!("{}. {}", i + 1, q.title)).collect::<Vec<_>>().join("  ·  ");
            panel = panel.child(div().truncate().text_size(px(t.size - 2.)).text_color(t.muted).child(format!("queue: {list}")));
        }
        for p in &w.proposals {
            let (cy, cn, pid) = (conv.clone(), conv.clone(), p.pid);
            let mine = p.by == me;
            let voted = p.yes.contains(&me) || p.no.contains(&me);
            let need = n.div_ceil(2);
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(div().text_color(t.yellow).child(G_BOLT))
                    .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.text).child(format!(
                        "{} proposes: {} · {}/{need} yes",
                        if mine { "you".to_owned() } else { self.model.name_of(&p.by) },
                        p.action.label(),
                        p.yes.len()
                    )))
                    .when(!voted, |d| {
                        d.child(button(SharedString::from(format!("wy-{pid}")), G_CHECK, "yes", t, true).on_click(cx.listener(move |this, _, _, cx| this.vote_watch(&cy, pid, true, cx))))
                            .child(button(SharedString::from(format!("wn-{pid}")), G_TIMES, "no", t, false).on_click(cx.listener(move |this, _, _, cx| this.vote_watch(&cn, pid, false, cx))))
                    }),
            );
        }
        vec![panel.into_any_element()]
    }

    /// An `@youtube` message: its query and five results, each to play alone, or to
    /// watch together (queue or switch when a session runs).
    fn youtube_results(&self, t: &Theme, msg_id: &str, query: &str, results: Vec<(String, String)>, cx: &mut Context<Self>) -> AnyElement {
        let session = self.selected.as_ref().is_some_and(|c| self.watches.get(c).is_some_and(|w| w.joined));
        let mut col = div().mt(px(4.)).flex().flex_col().gap(px(4.)).w(px(520.)).max_w_full().child(
            div().flex().items_center().gap(px(6.)).text_size(px(t.size - 2.)).text_color(t.muted).child(div().text_color(t.red).child(G_YOUTUBE)).child(format!("YouTube · “{query}”")),
        );
        for (i, (line, id)) in results.into_iter().enumerate() {
            let p = self.youtube_preview(&id).unwrap_or_default();
            let title = if p.title.is_empty() { line.split(" · ").next().unwrap_or(&line).to_owned() } else { p.title.clone() };
            let url = crate::media::watch_url(&id);
            let (u1, u2, u3, t2, t3) = (url.clone(), url.clone(), url.clone(), title.clone(), title.clone());
            col = col.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .p(px(4.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.panel)
                    .child(
                        div()
                            .flex_none()
                            .w(px(112.))
                            .h(px(63.))
                            .bg(t.bar)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(t.red)
                            .when_some(p.thumb.clone(), |d, th| d.child(gpui::img(th).w(px(112.)).h(px(63.)).object_fit(ObjectFit::Cover)))
                            .when(p.thumb.is_none(), |d| d.child(G_YOUTUBE)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.))
                            .gap(px(3.))
                            .child(div().truncate().text_color(t.bright).child(title))
                            .child(div().truncate().text_size(px(t.size - 2.5)).text_color(t.muted).child(line.splitn(2, " · ").nth(1).unwrap_or("").to_owned()))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(4.))
                                    .child(button(SharedString::from(format!("yr-me-{msg_id}-{i}")), G_PLAY, "for me", t, false).on_click(cx.listener(move |this, _, _, cx| this.play_alone(&u1, cx))))
                                    .when(!session, |d| {
                                        d.child(button(SharedString::from(format!("yr-tg-{msg_id}-{i}")), G_USERS, "together", t, false).on_click(cx.listener(move |this, _, _, cx| this.start_watch(&u2, &t2, cx))))
                                    })
                                    .when(session, |d| {
                                        let (u4, t4) = (u3.clone(), t3.clone());
                                        d.child(button(SharedString::from(format!("yr-q-{msg_id}-{i}")), G_PLUS, "queue", t, false).on_click(cx.listener(move |this, _, _, cx| this.queue_watch(&u3, &t3, cx))))
                                            .child(button(SharedString::from(format!("yr-sw-{msg_id}-{i}")), G_EXCHANGE, "switch", t, false).on_click(cx.listener(move |this, _, _, cx| this.start_watch(&u4, &t4, cx))))
                                    }),
                            ),
                    ),
            );
        }
        col.into_any_element()
    }

    /// Shares without a call: an offer to answer, ours running, or theirs on the stage.
    fn direct_panels(&self, t: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::desk::{direct_peer, Sharing, Viewing};
        let mut out = Vec::new();
        for key in self.desk.direct_sessions() {
            let Some(s) = self.desk.sessions.get(&key) else { continue };
            let name = self.model.name_of(direct_peer(&key).unwrap_or_default());
            let (text, asked) = match (&s.viewing, &s.sharing) {
                (Viewing::Asked { .. }, _) => (format!("{name} wants to show you their screen"), true),
                (Viewing::On { .. }, _) => (format!("{G_EYE} {name}'s screen"), false),
                (Viewing::Querying { .. } | Viewing::Opening { .. }, _) => (format!("connecting to {name}'s screen…"), false),
                (Viewing::Off, Sharing::Off) => continue,
                (Viewing::Off, _) => (format!("{G_DESKTOP} sharing with {name}"), false),
            };
            let (k1, k2) = (key.clone(), key.clone());
            let controls = div()
                .flex()
                .gap(px(6.))
                .when(asked, |d| {
                    d.child(button(SharedString::from(format!("watch-{key}")), G_EYE, "watch", t, true).on_click(cx.listener(move |this, _, _, cx| this.watch_direct(&k1, cx))))
                })
                .when(self.desk.viewer(&key).is_some(), |d| {
                    d.child(button(SharedString::from(format!("stage-{key}")), G_EXPAND, "", t, self.stage).on_click(cx.listener(|this, _, _, cx| {
                        this.stage = !this.stage;
                        cx.notify();
                    })))
                })
                .child(
                    button(SharedString::from(format!("stop-{key}")), G_TIMES, if asked { "decline" } else { "stop" }, t, false)
                        .text_color(t.red)
                        .on_click(cx.listener(move |this, _, _, cx| this.stop_direct(&k2, cx))),
                );
            let mut panel = div()
                .flex()
                .flex_col()
                .border_b_1()
                .border_color(t.border)
                .bg(if asked { t.accent_bg } else { t.panel })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .px(px(12.))
                        .py(px(6.))
                        .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.bright).child(text))
                        .child(controls.flex_none()),
                );
            if let Some(strip) = self.share_strip(t, &key, cx) {
                panel = panel.child(strip);
            }
            if self.desk.viewer(&key).is_some() && self.viewing_key().as_deref() == Some(key.as_str()) {
                if let Some((image, pointer, info)) = self.stage_source() {
                    let stage = self.stage_canvas(t, image, pointer, cx);
                    panel = panel
                        .when(self.stage, |d| d.flex_1())
                        .child(self.stage_box(t, stage, cx))
                        .child(self.stage_tools(t, info, cx));
                }
            }
            out.push(panel.into_any_element());
        }
        out
    }

    fn messages(&self, t: &Theme, conv: &Conversation, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().id("messages").flex().flex_col().flex_1().overflow_y_scroll().track_scroll(&self.messages_scroll).px(px(14.)).py(px(8.));
        let mut last_day = None;
        let mut last_author = String::new();
        for m in &conv.messages {
            let (hm, day, day_label) = local_time(m.ts);
            if last_day != Some(day) {
                last_day = Some(day);
                last_author.clear();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .py(px(8.))
                        .text_color(t.muted)
                        .text_size(px(t.size - 2.))
                        .child(div().h(px(1.)).flex_1().bg(t.border))
                        .child(day_label)
                        .child(div().h(px(1.)).flex_1().bg(t.border)),
                );
            }
            let me = crate::model::bare_uri(&m.author) == self.model.me;
            let who = if me { "you".to_owned() } else { self.model.name_of(&m.author) };
            let color = if me { t.accent } else { author_color(t, &m.author) };
            let repeat = last_author == m.author && m.kind == MsgKind::Text;
            // A system line breaks a run, so the next message names its author again.
            last_author = if m.kind == MsgKind::Text { m.author.clone() } else { String::new() };
            let row = div().flex().items_start().gap(px(10.)).py(px(1.)).child(div().flex_none().w(px(42.)).text_color(t.muted).child(hm));
            list = list.child(match m.kind {
                MsgKind::Text => row
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(6.))
                            .h(px(t.size + 7.))
                            .w(px(118.))
                            .when(!repeat, |d| {
                                let key = crate::model::bare_uri(&m.author).to_owned();
                                let picture = self.model.avatars.get(&key).cloned();
                                let initial_of = if me { self.model.my_name.clone() } else { who.clone() };
                                d.child(avatar(t, picture, &key, &initial_of, 18., None)).child(div().flex_1().min_w(px(0.)).truncate().text_color(color).child(who))
                            }),
                    )
                    .child({
                        let results = crate::watch::parse_results(&m.text);
                        let yt = if results.is_some() { None } else { crate::media::youtube_id(&m.text) };
                        let body = div().flex().flex_col().flex_1().min_w(px(0.));
                        match results {
                            Some((query, list)) => body.child(self.youtube_results(t, &m.id, &query, list, cx)),
                            None => body
                                .child(div().text_color(if me { t.text } else { t.bright }).child(m.text.clone()))
                                .when_some(yt, |d, id| d.child(self.youtube_card(t, &id, &m.id, cx))),
                        }
                    }),
                MsgKind::Event => match m.file.clone() {
                    // A file: click to open it, or to download one too big to come by itself.
                    Some(file) => {
                        let saved = self.saved_file(&conv.id, &file.id);
                        let file_id = file.id.clone();
                        let have = saved.is_some();
                        let hint = match (have, me, self.downloads.contains_key(&file.id)) {
                            (true, _, _) => " · open".to_owned(),
                            (false, true, _) => String::new(),
                            (false, false, true) => " · downloading".to_owned(),
                            (false, false, false) => format!(" · {} · download", size_label(file.size)),
                        };
                        let (conv_id, msg_id) = (conv.id.clone(), m.id.clone());
                        row.child(
                            div()
                                .id(SharedString::from(format!("file-{}", m.id)))
                                .flex_1()
                                .text_color(t.muted)
                                .italic()
                                .when(!me || have, |d| {
                                    d.cursor_pointer().hover(|d| d.text_color(t.accent)).on_click(cx.listener(move |this, _, _, cx| {
                                        this.file_clicked(&conv_id, &msg_id, &file, cx);
                                        cx.notify();
                                    }))
                                })
                                .child(format!("{who} · {}{hint}", m.text))
                                // A voice or video message plays with mpv.
                                .when_some(saved.clone().filter(|p| crate::media::is_audio(p) || crate::media::is_video(p)), |d, path| {
                                    let fid = file_id.clone();
                                    let playing = self.playing.as_ref().is_some_and(|(id, _)| *id == file_id);
                                    let voice = crate::media::is_audio(&path) || path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("voice"));
                                    d.child(
                                        div().mt(px(4.)).flex().child(
                                            button(SharedString::from(format!("play-{}", m.id)), if playing { G_STOP } else { G_PLAY }, if playing { "stop" } else if voice { "play voice message" } else { "play video" }, t, playing)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.toggle_play(&fid, &path, cx);
                                                })),
                                        ),
                                    )
                                })
                                // A picture shows itself once it is here.
                                .when_some(saved.filter(|p| is_picture(p)), |d, path| {
                                    // Sized from the file's header, so the list does not jump
                                    // when the picture loads.
                                    let (w, h) = picture_box(&path, 360., 260.);
                                    d.child(
                                        div()
                                            .mt(px(4.))
                                            .w(px(w + 2.))
                                            .h(px(h + 2.))
                                            .border_1()
                                            .border_color(t.border)
                                            .child(gpui::img(path).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain)),
                                    )
                                }),
                        )
                    }
                    None => row.child(div().flex_1().text_color(t.muted).italic().child(format!("{who} · {}", m.text))),
                },
            });
        }
        if conv.composing {
            list = list.child(div().pl(px(52.)).text_color(t.muted).italic().child("typing…"));
        }
        list.into_any_element()
    }

    fn chat(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let conv = self.selected.as_ref().and_then(|s| self.model.conversations.get(s)).cloned();
        let call_panel = self.call_panel(t, conv.as_ref(), cx);
        let direct = self.direct_panels(t, cx);
        let watch = self.watch_panels(t, cx);
        let Some(conv) = conv else {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .when_some(call_panel, |d, p| d.child(p))
                .children(direct)
                .children(watch)
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .p(px(24.))
                        .text_color(t.muted)
                        .child(format!("No conversation yet · {G_PLUS} or Ctrl+N to add a contact")),
                )
                .into_any_element();
        };
        let fav = self.quick.favorites.contains(&conv.id);
        let has_call = self.active_call().is_some();
        let peers = conv.peers.iter().map(|p| self.model.name_of(p)).collect::<Vec<_>>().join(", ");
        let peer_ids = conv.peers.iter().map(|p| format!("{}…", &p[..p.len().min(12)])).collect::<Vec<_>>().join(" ");
        let status = (conv.peers.len() == 1 && !conv.request).then(|| self.presence_line(&conv, &conv.peers[0])).flatten();
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(t.border)
            .when(self.narrow, |d| {
                d.child(button("back", G_BACK, "", t, false).mr(px(8.)).on_click(cx.listener(|this, _, _, cx| {
                    this.narrow_chat = false;
                    cx.notify();
                })))
            })
            .child({
                let peer = (conv.peers.len() == 1).then(|| crate::model::bare_uri(&conv.peers[0]).to_owned());
                let picture = peer.as_ref().and_then(|p| self.model.avatars.get(p).cloned());
                let key = peer.unwrap_or_else(|| conv.id.clone());
                div().mr(px(10.)).child(avatar(t, picture, &key, &conv.title, 34., None))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(conv.title.clone()))
                    .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child({
                        let who = if peers == conv.title { peer_ids } else { format!("{peers} · {peer_ids}") };
                        match status {
                            Some(s) => format!("{s} · {who}"),
                            None => who,
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center().gap(px(4.))
                    .child(button("fav", if fav { G_STAR } else { G_STAR_O }, "", t, fav).on_click(cx.listener(|this, _, _, cx| this.toggle_favorite(cx))))
                    .when(!conv.request && !self.is_removed(&conv.id), |d| {
                        let id = conv.id.clone();
                        d.child(button("remove", G_TRASH, "", t, false).on_click(cx.listener(move |this, _, _, cx| this.remove_from_list(&id, cx))))
                    })
                    .child(div().w(px(1.)).h(px(18.)).mx(px(2.)).bg(t.border))
                    .child(button("quick", G_BOLT, if self.narrow { "" } else { "quick" }, t, self.quick_open).on_click(cx.listener(|this, _, _, cx| {
                        this.quick_open = !this.quick_open;
                        cx.notify();
                    })))
                    .when(!conv.request, |d| {
                        let id = conv.id.clone();
                        d.child(button("send-file", G_CLIP, "", t, false).on_click(cx.listener(move |this, _, _, cx| this.pick_and_send(id.clone(), cx))))
                    })
                    .when(!has_call && !conv.request, |d| {
                        d.child(div().w(px(1.)).h(px(18.)).mx(px(2.)).bg(t.border)).child(button("share-d", G_DESKTOP, "", t, false).on_click(cx.listener(|this, _, _, cx| {
                            if let Some(conv) = this.selected.clone() {
                                if let Reply::Error { message } = this.share_direct(&conv, cx) {
                                    this.show_flash(message, cx);
                                }
                            }
                        })))
                        .child(button("call-a", G_PHONE, "", t, false).on_click(cx.listener(|this, _, _, cx| this.place_call(false, cx))))
                            .child(button("call-v", G_VIDEO, "", t, false).on_click(cx.listener(|this, _, _, cx| this.place_call(true, cx))))
                    }),
            );

        let body: AnyElement = if conv.request {
            let (a, b) = (conv.id.clone(), conv.id.clone());
            div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .child(div().text_color(t.bright).child(format!("{} wants to talk with you", conv.title)))
                .child(div().text_color(t.muted).child("ctrl+enter accepts · ctrl+backspace declines"))
                .child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .child(button("req-ok", G_CHECK, "accept", t, true).on_click(cx.listener(move |this, _, _, cx| this.accept_request(a.clone(), true, cx))))
                        .child(button("req-no", G_TIMES, "decline", t, false).on_click(cx.listener(move |this, _, _, cx| this.accept_request(b.clone(), false, cx)))),
                )
                .into_any_element()
        } else {
            // Stay at the end while it grows (a picture that loads makes it taller), unless
            // the user scrolled up.
            let at_end = {
                let h = &self.messages_scroll;
                f32::from(h.max_offset().height) > 0. && f32::from(-h.offset().y) >= f32::from(h.max_offset().height) - 4.
            };
            if std::mem::take(&mut self.pinned_bottom) || at_end {
                self.messages_scroll.scroll_to_bottom();
            }
            self.messages(t, &conv, cx)
        };

        let quick = self.quick_open.then(|| {
            let mut row = div().flex().flex_wrap().gap(px(6.)).px(px(12.)).py(px(6.)).border_t_1().border_color(t.border).bg(t.panel);
            for (i, m) in self.quick.messages.iter().enumerate().take(9) {
                row = row.child(
                    button(format!("q{i}"), &format!("{}", i + 1), m, t, false)
                        .on_click(cx.listener(move |this, _, _, cx| this.send_quick(i, cx))),
                );
            }
            row
        });

        let accent_bg = t.accent_bg;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            // Files dropped on the conversation wait beside the composer.
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(accent_bg))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.attach(paths.paths().to_vec(), cx)))
            .child(header)
            .when_some(call_panel, |d, p| d.child(p))
            .children(direct)
            .children(watch)
            .when(!self.stage, |d| {
                d.child(body)
                    .when_some(quick, |d, q| d.child(q))
                    .when(!conv.request, |d| {
                        d.children(self.emoji_popover(t, cx))
                            .children(self.attachment_strip(t, &conv.id, cx))
                            .child(self.composer_row(t, cx))
                    })
            })
            .into_any_element()
    }

    fn status_line(&self, t: &Theme) -> AnyElement {
        let left = match &self.flash {
            Some((text, _)) => div().text_color(t.yellow).child(text.clone()),
            None => div().text_color(t.muted).child(match self.mode {
                Mode::Add => "enter: add · esc: cancel",
                Mode::Search => "enter: open first · esc: cancel",
                Mode::Username => "enter: register · esc: cancel",
                Mode::LinkDevice => "enter: link · esc: cancel",
                Mode::DeviceName => "enter: rename · esc: cancel",
                Mode::RemoveAccount => "delete + enter: remove the account for good · esc: keep it",
                Mode::Removed => "removed contacts · esc: back",
                Mode::Chat if self.narrow && self.narrow_chat => "esc: back · ctrl+.: quick",
                Mode::Chat if self.narrow => "ctrl+n: add · ctrl+k: find · ctrl+shift+k: contacts",
                Mode::Chat => "alt+j/k: chats · ctrl+1…6: tabs · ctrl+n: add · ctrl+k: find · ctrl+shift+c/v: call · ctrl+.: quick",
            }),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .h(px(24.))
            .px(px(10.))
            .bg(t.bar)
            .border_t_1()
            .border_color(t.border)
            .text_size(px(t.size - 2.))
            .child(left.overflow_hidden())
            .when(!self.narrow, |d| d.child(div().text_color(t.muted).child(format!("libjami · {}", self.model.account))))
            .into_any_element()
    }

    fn gate_screen(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let card = div().flex().flex_col().gap(px(12.)).w(px(520.)).max_w_full().p(px(24.)).border_1().border_color(t.border).bg(t.panel);
        let card = match &self.gate {
            Gate::QtRunning => card
                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Jami is open"))
                .child(div().child("Joma uses the same accounts and history as the Jami app, and only one of them can run at a time. Jami keeps running in the tray after its window is closed: quit it from its tray icon (or run `pkill -x jami`). Joma comes back by itself once it quits."))
                .child(button("retry", G_CHECK, "retry", t, true).on_click(cx.listener(|this, _, _, cx| this.boot(cx)))),
            Gate::Onboarding if self.linking.is_some() => self.link_card(card, t, cx),
            Gate::Onboarding if self.adding_account.is_some() => card
                .child(div().text_color(t.accent).font_weight(FontWeight::BOLD).child("Another account"))
                .child(div().child("A second Jami identity on this machine, with its own contacts and history. Switch between them in Contacts."))
                .child(self.onboarding_body(t, cx))
                .child(div().flex().child(button("account-back", G_BACK, "back", t, false).on_click(cx.listener(|this, _, window, cx| this.cancel_new_account(window, cx))))),
            Gate::Onboarding => card
                .child(div().text_color(t.accent).font_weight(FontWeight::BOLD).child("Welcome to Joma"))
                .child(div().child("Joma is a Jami client for Omarchy. Create a Jami account to start: it lives on this machine, no server and no phone number."))
                .child(self.onboarding_body(t, cx)),
            Gate::Failed(e) => card.child(div().text_color(t.red).child(format!("The Jami daemon failed to start: {e}"))),
            Gate::Ready => card,
        };
        div().flex().flex_1().items_center().justify_center().child(card).into_any_element()
    }

    /// The name and username steps, and joining from another device.
    fn onboarding_body(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
                .child(self.input.clone())
                .child(div().text_color(t.muted).text_size(px(t.size - 2.)).child(if self.onboard_name.is_none() {
                    "Enter: next, a username".to_owned()
                } else {
                    format!("Enter creates the account. A username has {USERNAME_RULE}; without one, contacts see your 40-digit ID until they get your profile.")
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(t.border)
                        .child(div().flex_1().text_color(t.muted).child("Already on Jami on another device?"))
                        .child(button("link-start", G_LINK, "link to your account", t, false).on_click(cx.listener(|this, _, _, cx| this.start_link(cx)))),
                )
    }
}

impl Render for Joma {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();
        self.refresh_frames(window, cx);
        // Keys reach actions only through a focused element: when the focused input is not
        // on screen (a request has no composer), the window itself takes the focus.
        let conv = self.selected.as_ref().and_then(|s| self.model.conversations.get(s));
        // Full screen ends with what it showed (or when the compositor took it away).
        if self.fullscreen && (!self.has_stage() || !window.is_fullscreen() && self.fullscreen_seen) {
            self.fullscreen = false;
            self.fullscreen_seen = false;
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        }
        self.fullscreen_seen |= self.fullscreen && window.is_fullscreen();
        let composer_shown = self.gate == Gate::Ready && conv.is_some_and(|c| !c.request) && !self.stage && !self.fullscreen && self.picker.is_none();
        let input_shown = self.gate == Gate::Onboarding || (self.gate == Gate::Ready && !matches!(self.mode, Mode::Chat | Mode::Removed));
        let lost = (!composer_shown && self.composer.focus_handle(cx).is_focused(window))
            || (!input_shown && self.input.focus_handle(cx).is_focused(window))
            || window.focused(cx).is_none();
        if composer_shown && std::mem::take(&mut self.focus_composer) {
            window.focus(&self.composer.focus_handle(cx));
        } else if lost {
            window.focus(&self.focus);
        }
        // A narrow window (the compact panel) shows the list or one conversation.
        self.narrow = window.viewport_size().width < px(700.);
        let full = if self.fullscreen { self.fullscreen_stage(&t, cx) } else { None };
        let picker = self.picker_overlay(&t, cx);
        let profile = self.profile_overlay(&t, cx);
        let settings = self.settings_overlay(&t, cx);
        let body = if let Some(full) = full {
            full
        } else if self.gate == Gate::Ready && self.narrow {
            let chat = self.narrow_chat && self.selected.is_some();
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .when(chat, |d| d.child(self.chat(&t, cx)))
                .when(!chat, |d| d.child(self.rail(&t, cx)).child(self.sidebar(&t, cx)))
                .into_any_element()
        } else if self.gate == Gate::Ready {
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                // A stage that takes the window takes the sidebar's room too.
                .when(!(self.stage && self.has_stage()), |d| d.child(self.rail(&t, cx)).child(self.sidebar(&t, cx)))
                .child(self.chat(&t, cx))
                .into_any_element()
        } else {
            self.gate_screen(&t, cx)
        };
        div()
            .id("joma")
            .key_context("Joma")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(t.font.clone())
            .text_size(px(t.size))
            .on_action(cx.listener(|this, _: &Quit, _, cx| {
                this.leave();
                cx.quit()
            }))
            .on_action(cx.listener(|this, _: &NextConversation, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PrevConversation, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &NewConversation, window, cx| this.set_mode(Mode::Add, window, cx)))
            .on_action(cx.listener(|this, _: &Search, window, cx| this.set_mode(Mode::Search, window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                // Esc closes the picker or the profile, then leaves full screen.
                if this.settings.is_some() {
                    this.settings = None;
                    cx.notify();
                    return;
                }
                if this.profile_edit.is_some() {
                    this.profile_edit = None;
                    cx.notify();
                    return;
                }
                if this.picker.is_some() {
                    this.close_picker(window, cx);
                    return;
                }
                if this.fullscreen {
                    this.toggle_fullscreen(window, cx);
                    return;
                }
                // In the compact panel, Esc goes back to the list.
                if this.narrow && this.narrow_chat && this.mode == Mode::Chat && !this.quick_open && !this.stage {
                    this.narrow_chat = false;
                    cx.notify();
                    return;
                }
                this.quick_open = false;
                this.stage = false;
                this.set_mode(Mode::Chat, window, cx);
            }))
            .on_action(cx.listener(|this, _: &CallAudio, _, cx| this.place_call(false, cx)))
            .on_action(cx.listener(|this, _: &CallVideo, _, cx| this.place_call(true, cx)))
            .on_action(cx.listener(|this, _: &HangUp, _, cx| this.hang_up(cx)))
            .on_action(cx.listener(|this, _: &AcceptCall, _, cx| this.accept_call(false, cx)))
            .on_action(cx.listener(|this, _: &ToggleMute, _, cx| this.toggle_mute(false, cx)))
            .on_action(cx.listener(|this, _: &ToggleCamera, _, cx| this.toggle_mute(true, cx)))
            .on_action(cx.listener(|this, _: &ShareDesktop, _, cx| this.toggle_share(cx)))
            .on_action(cx.listener(|this, _: &ToggleFavorite, _, cx| this.toggle_favorite(cx)))
            .on_action(cx.listener(|this, _: &ToggleContacts, window, cx| {
                let tab = if this.tab == Tab::Contacts { Tab::Chats } else { Tab::Contacts };
                this.set_tab(tab, window, cx);
            }))
            .on_action(cx.listener(|this, _: &AcceptRequest, _, cx| {
                if let Some(id) = this.selected.clone().filter(|s| this.model.conversations.get(s).is_some_and(|c| c.request)) {
                    this.accept_request(id, true, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &DeclineRequest, _, cx| {
                if let Some(id) = this.selected.clone().filter(|s| this.model.conversations.get(s).is_some_and(|c| c.request)) {
                    this.accept_request(id, false, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &FocusStage, _, cx| {
                this.stage = !this.stage;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleFullscreen, window, cx| this.toggle_fullscreen(window, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_by(1.25, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_by(1. / 1.25, cx)))
            .on_action(cx.listener(|this, _: &ZoomFit, _, cx| this.zoom(crate::stage::Zoom::Fit, cx)))
            .on_action(cx.listener(|this, _: &TabChats, window, cx| this.set_tab(Tab::Chats, window, cx)))
            .on_action(cx.listener(|this, _: &TabCalls, window, cx| this.set_tab(Tab::Calls, window, cx)))
            .on_action(cx.listener(|this, _: &TabContacts, window, cx| this.set_tab(Tab::Contacts, window, cx)))
            .on_action(cx.listener(|this, _: &TabDevices, window, cx| this.set_tab(Tab::Devices, window, cx)))
            .on_action(cx.listener(|this, _: &TabAccounts, window, cx| this.set_tab(Tab::Accounts, window, cx)))
            .on_action(cx.listener(|this, _: &TabAgents, window, cx| this.set_tab(Tab::Agents, window, cx)))
            .on_action(cx.listener(|this, _: &ToggleQuick, _, cx| {
                this.quick_open = !this.quick_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Quick1, _, cx| this.send_quick(0, cx)))
            .on_action(cx.listener(|this, _: &Quick2, _, cx| this.send_quick(1, cx)))
            .on_action(cx.listener(|this, _: &Quick3, _, cx| this.send_quick(2, cx)))
            .on_action(cx.listener(|this, _: &Quick4, _, cx| this.send_quick(3, cx)))
            .on_action(cx.listener(|this, _: &Quick5, _, cx| this.send_quick(4, cx)))
            .on_action(cx.listener(|this, _: &Quick6, _, cx| this.send_quick(5, cx)))
            .on_action(cx.listener(|this, _: &Quick7, _, cx| this.send_quick(6, cx)))
            .on_action(cx.listener(|this, _: &Quick8, _, cx| this.send_quick(7, cx)))
            .on_action(cx.listener(|this, _: &Quick9, _, cx| this.send_quick(8, cx)))
            .on_mouse_down(MouseButton::Left, |_, _, _| {})
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, cx| {
                let Some((y0, h0)) = this.resizing else { return };
                if e.pressed_button != Some(MouseButton::Left) {
                    this.resizing = None;
                } else {
                    // The chat keeps at least a few lines.
                    let max = (f32::from(window.viewport_size().height) - 220.).max(120.);
                    this.stage_height = (h0 + f32::from(e.position.y) - y0).clamp(120., max);
                }
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                if this.resizing.take().is_some() {
                    cx.notify();
                }
            }))
            .relative()
            .when(!self.fullscreen, |d| d.child(self.top_bar(&t, cx)).children(self.setup_banner(&t, cx)))
            .child(body)
            .when(!self.fullscreen, |d| d.child(self.status_line(&t)))
            .children(picker)
            .children(profile)
            .children(settings)
    }
}

/// Our drawing over the shared picture at `r`: strokes as chains of dots, the laser.
fn paint_ink(window: &mut Window, c: &joma_desk::ink::Canvas, r: Bounds<Pixels>) {
    let (ox, oy, w, h) = (f32::from(r.origin.x), f32::from(r.origin.y), f32::from(r.size.width), f32::from(r.size.height));
    let at = |(x, y): (f32, f32)| (ox + x * w, oy + y * h);
    let mut dot = |x: f32, y: f32, rad: f32, color: Hsla| {
        let b = Bounds::new(point(px(x - rad), px(y - rad)), size(px(rad * 2.), px(rad * 2.)));
        window.paint_quad(gpui::fill(b, color).corner_radii(px(rad)));
    };
    for s in &c.strokes {
        let color: Hsla = gpui::rgb(s.color).into();
        let rad = (s.width / 2.).max(1.5);
        let pts: Vec<(f32, f32)> = s.pts.iter().map(|&q| at(q)).collect();
        if let Some(&(x, y)) = pts.first() {
            dot(x, y, rad, color);
        }
        for p in pts.windows(2) {
            let ((ax, ay), (bx, by)) = (p[0], p[1]);
            let n = ((bx - ax).hypot(by - ay) / (rad * 0.6)).ceil().max(1.) as usize;
            for i in 1..=n {
                let k = i as f32 / n as f32;
                dot(ax + (bx - ax) * k, ay + (by - ay) * k, rad, color);
            }
        }
    }
    if let Some((x, y, _)) = c.laser {
        let (x, y) = at((x, y));
        let red: Hsla = gpui::rgb(0xff2244).into();
        dot(x, y, 14., red.opacity(0.3));
        dot(x, y, 6., red);
    }
}

/// The size a picture shows at in the conversation: its own, shrunk to fit `max`. The
/// dimensions come from the file's header (read once per file).
fn picture_box(path: &std::path::Path, max_w: f32, max_h: f32) -> (f32, f32) {
    thread_local! {
        static SIZES: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf, (u32, u32)>> = Default::default();
    }
    let (w, h) = SIZES.with(|m| {
        *m.borrow_mut().entry(path.to_path_buf()).or_insert_with(|| image::image_dimensions(path).unwrap_or((360, 260)))
    });
    let k = (max_w / w.max(1) as f32).min(max_h / h.max(1) as f32).min(1.);
    ((w as f32 * k).max(8.), (h as f32 * k).max(8.))
}

/// A file Joma shows as a picture.
fn is_picture(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"))
}

/// "screen DP-1 · 5120×1440" or "window firefox — the title".
fn source_label(source: &joma_desk::share::Source, screen: &joma_desk::share::Screen) -> String {
    match source {
        joma_desk::share::Source::Screen { output } => format!("screen {output} · {}×{}", screen.width, screen.height),
        joma_desk::share::Source::Window { class, title, .. } => format!("window {class} — {title}"),
        joma_desk::share::Source::Area { output, .. } => format!("an area of {output} · {}×{}", screen.width, screen.height),
    }
}

fn size_label(bytes: u64) -> String {
    match bytes {
        0 => "size unknown".into(),
        b if b < 1024 * 1024 => format!("{} KB", b.div_ceil(1024)),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}
