//! How Joma looks: Omarchy's flat, square, keyboard-first style. One font (the terminal's),
//! thin borders, the accent for focus and selection, a status line at the bottom.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Focusable,
    canvas, div, point, px, size, AnyElement, Bounds, ContentMask, Context, Corners, FontWeight, Hsla, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, RenderImage, ScrollDelta, ScrollWheelEvent, SharedString, Window,
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

fn button(id: impl Into<SharedString>, glyph: &str, label: &str, t: &Theme, on: bool) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(8.))
        .py(px(3.))
        .border_1()
        .border_color(if on { t.accent } else { t.border })
        .text_color(if on { t.accent } else { t.text })
        .cursor_pointer()
        .hover(|d| d.bg(t.raised).text_color(t.bright))
        .child(glyph.to_owned())
        .when(!label.is_empty(), |d| d.child(label.to_owned()))
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
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .overflow_hidden()
                                    .when(dot != Dot::None, |d| {
                                        d.child(div().flex_none().size(px(7.)).rounded_full().bg(if dot == Dot::Active { t.green } else { t.green.opacity(0.3) }))
                                    })
                                    .when(fav, |d| d.child(div().text_color(t.yellow).child(G_STAR)))
                                    .when(has_call, |d| d.child(div().text_color(t.green).child(G_PHONE)))
                                    .child(
                                        div()
                                            .text_color(if c.unread > 0 || selected { t.bright } else { t.text })
                                            .when(c.unread > 0, |d| d.font_weight(FontWeight::BOLD))
                                            .truncate()
                                            .child(c.title.clone()),
                                    ),
                            )
                            .when(c.request, |d| d.child(div().text_color(t.yellow).child("request")))
                            .when(c.unread > 0, |d| {
                                d.child(div().px(px(5.)).bg(t.accent).text_color(t.bar).child(c.unread.to_string()))
                            }),
                    )
                    .child(div().text_color(t.muted).text_size(px(t.size - 1.5)).truncate().child(last))
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
                    .child(
                        div()
                            .flex()
                            .children([(Tab::Chats, "chats"), (Tab::Contacts, "contacts")].map(|(tab, label)| {
                                let on = self.tab == tab;
                                header(label)
                                    .id(SharedString::from(format!("tab-{label}")))
                                    .when(on, |d| d.text_color(t.bright))
                                    .border_b_2()
                                    .border_color(if on { t.accent } else { gpui::transparent_black() })
                                    .cursor_pointer()
                                    .hover(|d| d.text_color(t.bright))
                                    .on_click(cx.listener(move |this, _, window, cx| this.set_tab(tab, window, cx)))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .pt(px(6.))
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
                    ),
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
            .child(if self.tab == Tab::Contacts { self.contacts_list(t, cx) } else { list.into_any_element() })
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
            "On your other device: Joma → contacts → you → link a device, and paste this code; or in the Jami app: Link device, and scan it.",
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

    /// "You": this device and the account's others, and linking a new one.
    fn devices_group(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let armed = self.is_armed("remove-account");
        let who = if self.model.username.is_empty() { self.model.me.chars().take(8).collect::<String>() + "…" } else { self.model.username.clone() };
        let mut group = div().flex().flex_col().child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(4.))
                .child(div().flex_none().text_color(t.bright).font_weight(FontWeight::BOLD).child(self.model.my_name.clone()))
                .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.muted).text_size(px(t.size - 2.)).child(who))
                .child(
                    button("account-remove", G_TRASH, if armed || self.mode == Mode::RemoveAccount { "remove?" } else { "" }, t, armed)
                        .px(px(6.))
                        .py(px(1.))
                        .text_color(if armed || self.mode == Mode::RemoveAccount { t.red } else { t.muted })
                        .on_click(cx.listener(|this, _, window, cx| this.remove_account(window, cx))),
                ),
        );
        let mut ids: Vec<(String, String)> = self.model.devices.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        ids.sort_by_key(|(id, _)| *id != self.model.device);
        for (id, name) in ids {
            let this = id == self.model.device;
            let dot = self.device_dot(&id);
            let armed = self.is_armed(&format!("revoke:{id}"));
            let i = id.clone();
            let label = if name.is_empty() { format!("{}…", &id[..id.len().min(8)]) } else { name };
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
                                .px(px(6.))
                                .py(px(1.))
                                .on_click(cx.listener(|this, _, window, cx| this.set_mode(Mode::DeviceName, window, cx))),
                        )
                    })
                    .when(!this, |d| {
                        d.child(
                            button(SharedString::from(format!("dev-revoke-{id}")), G_BAN, if armed { "revoke?" } else { "" }, t, armed)
                                .px(px(6.))
                                .py(px(1.))
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
            None => div()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .px(px(10.))
                .py(px(4.))
                .child(
                    button("dev-link", G_LINK, "link a device", t, self.mode == Mode::LinkDevice)
                        .on_click(cx.listener(|this, _, window, cx| this.set_mode(Mode::LinkDevice, window, cx))),
                )
                .child(button("account-new", G_USER_PLUS, "other account", t, false).on_click(cx.listener(|this, _, window, cx| this.new_account(window, cx)))),
        })
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
        if self.mode != Mode::Search {
            list = list.child(small("you")).child(self.devices_group(t, cx));
            if self.accounts.len() > 1 {
                list = list.child(small("other accounts")).child(self.accounts_group(t, cx));
            }
        }
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
                    .px(px(6.))
                    .py(px(1.))
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
                        .child(div().flex_none().size(px(7.)).rounded_full().when(dot != Dot::None, |d| {
                            d.bg(if dot == Dot::Active { t.green } else { t.green.opacity(0.3) })
                        }))
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
                                .px(px(6.))
                                .py(px(1.))
                                .text_color(if block_armed { t.red } else { t.muted })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.block(&u_block, cx);
                                })),
                        )
                        .child(
                            button(SharedString::from(format!("cq-remove-{uri}")), G_USER_TIMES, if remove_armed { "remove?" } else { "" }, t, remove_armed)
                                .px(px(6.))
                                .py(px(1.))
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
            .on_mouse_down(MouseButton::Left, cx.listener(|this, e: &MouseDownEvent, window, cx| {
                if e.click_count == 2 {
                    this.stage_view.borrow_mut().drag = None;
                    this.toggle_fullscreen(window, cx);
                    return;
                }
                let mut v = this.stage_view.borrow_mut();
                v.drag = Some((e.position, v.center));
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
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
        let small = |id: &'static str, glyph: &str, label: &str, on: bool| button(id, glyph, label, t, on).px(px(6.)).py(px(1.));
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
            let row = div().flex().gap(px(10.)).py(px(1.)).child(div().flex_none().w(px(42.)).text_color(t.muted).child(hm));
            list = list.child(match m.kind {
                MsgKind::Text => row
                    .child(div().flex_none().w(px(96.)).truncate().text_color(color).child(if repeat { String::new() } else { who }))
                    .child(div().flex_1().min_w(px(0.)).text_color(if me { t.text } else { t.bright }).child(m.text.clone())),
                MsgKind::Event => match m.file.clone() {
                    // A file: click to open it, or to download one too big to come by itself.
                    Some(file) => {
                        let have = self.saved_file(&conv.id, &file.id).is_some();
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
                                .child(format!("{who} · {}{hint}", m.text)),
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
        let Some(conv) = conv else {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .when_some(call_panel, |d, p| d.child(p))
                .children(direct)
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
                    .gap(px(6.))
                    .child(button("fav", if fav { G_STAR } else { G_STAR_O }, "", t, fav).on_click(cx.listener(|this, _, _, cx| this.toggle_favorite(cx))))
                    .when(!conv.request && !self.is_removed(&conv.id), |d| {
                        let id = conv.id.clone();
                        d.child(button("remove", G_TRASH, "", t, false).on_click(cx.listener(move |this, _, _, cx| this.remove_from_list(&id, cx))))
                    })
                    .child(button("quick", G_BOLT, if self.narrow { "" } else { "quick" }, t, self.quick_open).on_click(cx.listener(|this, _, _, cx| {
                        this.quick_open = !this.quick_open;
                        cx.notify();
                    })))
                    .when(!conv.request, |d| {
                        let id = conv.id.clone();
                        d.child(button("send-file", G_CLIP, "", t, false).on_click(cx.listener(move |this, _, _, cx| this.pick_and_send(id.clone(), cx))))
                    })
                    .when(!has_call && !conv.request, |d| {
                        d.child(button("share-d", G_DESKTOP, "", t, false).on_click(cx.listener(|this, _, _, cx| {
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
            if std::mem::take(&mut self.pinned_bottom) {
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

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .child(header)
            .when_some(call_panel, |d, p| d.child(p))
            .children(direct)
            .when(!self.stage, |d| {
                d.child(body)
                    .when_some(quick, |d, q| d.child(q))
                    .when(!conv.request, |d| d.child(div().px(px(12.)).py(px(8.)).child(self.composer.clone())))
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
                Mode::Chat => "alt+j/k: chats · ctrl+shift+k: contacts · ctrl+n: add · ctrl+k: find · ctrl+shift+c/v: call · ctrl+.: quick",
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
        let body = if let Some(full) = full {
            full
        } else if self.gate == Gate::Ready && self.narrow {
            let chat = self.narrow_chat && self.selected.is_some();
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .when(chat, |d| d.child(self.chat(&t, cx)))
                .when(!chat, |d| d.child(self.sidebar(&t, cx)))
                .into_any_element()
        } else if self.gate == Gate::Ready {
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                // A stage that takes the window takes the sidebar's room too.
                .when(!(self.stage && self.has_stage()), |d| d.child(self.sidebar(&t, cx)))
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
                // Esc closes the picker, then leaves full screen.
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
    }
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
