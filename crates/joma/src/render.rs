//! How Joma looks: Omarchy's flat, square, keyboard-first style. One font (the terminal's),
//! thin borders, the accent for focus and selection, a status line at the bottom.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Focusable,
    canvas, div, point, px, size, AnyElement, Bounds, Context, Corners, FontWeight, Hsla, MouseButton, Pixels,
    RenderImage, SharedString, Window,
};
use smallvec::smallvec;

use crate::app::*;
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
        let call = self.active_call().map(|c| c.id.clone());
        let state = call.as_ref().and_then(|c| self.desk.viewer(c)).map(|v| v.state.lock().unwrap().clone());
        match state {
            Some(st) => {
                if let Some(p) = st.picture {
                    if self.desk_frame.as_ref().is_none_or(|f| f.serial != p.serial) {
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
    }

    fn top_bar(&self, t: &Theme) -> AnyElement {
        let reg = self.model.registration.as_str();
        let (dot, color) = match reg {
            "REGISTERED" => ("online", t.green),
            "" => ("", t.muted),
            "TRYING" | "INITIALIZING" => ("connecting", t.yellow),
            _ => ("offline", t.red),
        };
        let unread = self.model.total_unread();
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
                    .child(div().text_color(t.accent).font_weight(FontWeight::BOLD).child("joma"))
                    .child(div().text_color(t.muted).child("·"))
                    .child(div().text_color(t.bright).child(if self.model.my_name.is_empty() {
                        "jami".to_owned()
                    } else {
                        self.model.my_name.clone()
                    }))
                    .when(!dot.is_empty(), |d| d.child(div().text_color(color).child(format!("● {dot}")))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .text_color(t.muted)
                    .when(unread > 0, |d| d.child(div().text_color(t.accent).child(format!("{unread} unread"))))
                    .child(format!("{} conversations", self.model.conversations.len())),
            )
            .into_any_element()
    }

    fn sidebar(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let convs: Vec<Conversation> = self.visible_conversations().into_iter().cloned().collect();
        let mut list = div().id("conversations").flex().flex_col().flex_1().overflow_y_scroll();
        for c in convs {
            let selected = self.selected.as_deref() == Some(c.id.as_str());
            let fav = self.quick.favorites.contains(&c.id);
            let has_call = self.model.calls.values().any(|k| k.conversation.as_deref() == Some(&c.id));
            let last = if c.composing {
                "typing…".to_owned()
            } else {
                c.last().map(|m| m.text.lines().next().unwrap_or("").to_owned()).unwrap_or_default()
            };
            let id = c.id.clone();
            list = list.child(
                div()
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
                    .child(div().text_color(t.muted).text_size(px(t.size - 1.5)).truncate().child(last)),
            );
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
            .flex_none()
            .w(px(290.))
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
                    .child(header("conversations"))
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
                            ),
                    ),
            )
            .when(self.mode != Mode::Chat, |d| {
                d.child(
                    div()
                        .px(px(8.))
                        .pb(px(6.))
                        .child(div().text_color(t.muted).text_size(px(t.size - 2.)).pb(px(3.)).child(match self.mode {
                            Mode::Add => "add a contact",
                            _ => "filter",
                        }))
                        .child(self.input.clone()),
                )
            })
            .child(list)
            .into_any_element()
    }

    fn call_panel(&self, t: &Theme, conv: Option<&Conversation>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let call = self.active_call()?.clone();
        let here = conv.is_some_and(|c| call.conversation.as_deref() == Some(c.id.as_str()));
        let name = self.model.name_of(&call.peer);
        let elapsed = call.started.map(|s| s.elapsed().as_secs()).map(|s| format!(" {:02}:{:02}", s / 60, s % 60)).unwrap_or_default();
        let session = self.desk.sessions.get(&call.id);
        let sharing = session.and_then(|s| s.sharing_label());
        let viewing = self.desk.viewer(&call.id).is_some();
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
            .items_center()
            .gap(px(8.))
            .child(div().text_color(t.green).child(G_PHONE))
            .child(div().text_color(t.bright).child(name.clone()))
            .when(!here, |d| d.child(div().text_color(t.muted).child("(other conversation)")))
            .child(div().text_color(t.muted).child(format!("{}{elapsed}", call.state.label())))
            .child(div().px(px(4.)).border_1().border_color(t.border).text_color(if call.peer_joma { t.accent } else { t.muted }).child(joma_badge))
            .when_some(sharing, |d, s| d.child(div().text_color(t.yellow).child(format!("{G_BOLT} {s}"))));

        // The stage: the peer's desktop (Joma), else their video.
        let desk = self.desk_frame.as_ref().map(|f| f.image.clone());
        let video = call.sinks.iter().find_map(|s| self.frames.get(s)).map(|f| f.image.clone());
        let stage_image = desk.clone().or(video);
        let pointer = if desk.is_some() {
            self.desk.viewer(&call.id).map(|v| v.state.lock().unwrap().pointer.clone()).zip(self.pointer.as_ref().map(|p| p.image.clone()))
        } else {
            None
        };
        let info = self.desk.viewer(&call.id).map(|v| {
            let s = v.state.lock().unwrap();
            format!("{G_EYE} {name}'s desktop · {} · {}→{} · {:.0} fps", s.chroma, s.remote_pipeline, s.local_pipeline, s.fps)
        });

        let mut panel = div()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(t.border)
            .bg(t.panel)
            .child(div().flex().items_center().justify_between().px(px(12.)).py(px(6.)).child(status).child(controls));
        if let Some(image) = stage_image {
            let bg = t.bar;
            let stage = canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    window.paint_quad(gpui::fill(bounds, bg));
                    let sz = image.size(0);
                    let (w, h) = (sz.width.0 as f32, sz.height.0 as f32);
                    let r = fit(bounds, w, h);
                    let _ = window.paint_image(r, Corners::default(), image.clone(), 0, false);
                    if let Some((p, cursor)) = &pointer {
                        if p.visible {
                            let k = f32::from(r.size.width) / w.max(1.);
                            let csz = cursor.size(0);
                            let origin = point(
                                r.origin.x + px((p.x as f32 - p.hot_x as f32) * k),
                                r.origin.y + px((p.y as f32 - p.hot_y as f32) * k),
                            );
                            let cb = Bounds::new(origin, size(px(csz.width.0 as f32 * k), px(csz.height.0 as f32 * k)));
                            let _ = window.paint_image(cb, Corners::default(), cursor.clone(), 0, false);
                        }
                    }
                },
            )
            .size_full();
            panel = panel
                .when(self.stage, |d| d.flex_1())
                .child(div().relative().when(self.stage, |d| d.flex_1()).when(!self.stage, |d| d.h(px(260.))).child(stage))
                .when_some(info, |d, i| d.child(div().px(px(12.)).py(px(3.)).text_size(px(t.size - 2.)).text_color(t.muted).child(i)));
        } else if let Some(note) = session.and_then(|s| s.note.clone()) {
            panel = panel.child(div().px(px(12.)).pb(px(6.)).text_size(px(t.size - 2.)).text_color(t.yellow).child(note));
        }
        Some(panel.into_any_element())
    }

    fn messages(&self, t: &Theme, conv: &Conversation) -> AnyElement {
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
                MsgKind::Event => row.child(div().flex_1().text_color(t.muted).italic().child(format!("{who} · {}", m.text))),
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
        let Some(conv) = conv else {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .when_some(call_panel, |d, p| d.child(p))
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
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(conv.title.clone()))
                    .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(if peers == conv.title {
                        peer_ids
                    } else {
                        format!("{peers} · {peer_ids}")
                    })),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(button("fav", if fav { G_STAR } else { G_STAR_O }, "", t, fav).on_click(cx.listener(|this, _, _, cx| this.toggle_favorite(cx))))
                    .child(button("quick", G_BOLT, "quick", t, self.quick_open).on_click(cx.listener(|this, _, _, cx| {
                        this.quick_open = !this.quick_open;
                        cx.notify();
                    })))
                    .when(!has_call && !conv.request, |d| {
                        d.child(button("call-a", G_PHONE, "", t, false).on_click(cx.listener(|this, _, _, cx| this.place_call(false, cx))))
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
            self.messages(t, &conv)
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
                Mode::Chat => "alt+j/k: conversations · ctrl+n: add · ctrl+k: find · ctrl+shift+c/v: call · ctrl+shift+s: share · ctrl+.: quick",
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
            .child(left)
            .child(div().text_color(t.muted).child(format!("libjami · {}", self.model.account)))
            .into_any_element()
    }

    fn gate_screen(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let card = div().flex().flex_col().gap(px(12.)).w(px(520.)).p(px(24.)).border_1().border_color(t.border).bg(t.panel);
        let card = match &self.gate {
            Gate::QtRunning => card
                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Jami is open"))
                .child(div().child("Joma uses the same accounts and history as the Jami app, and only one of them can run at a time. Quit Jami, then retry."))
                .child(button("retry", G_CHECK, "retry", t, true).on_click(cx.listener(|this, _, _, cx| this.boot(cx)))),
            Gate::Onboarding => card
                .child(div().text_color(t.accent).font_weight(FontWeight::BOLD).child("Welcome to Joma"))
                .child(div().child("Joma is a Jami client for Omarchy. Create a Jami account to start: it lives on this machine, no server and no phone number."))
                .child(self.input.clone())
                .child(div().text_color(t.muted).text_size(px(t.size - 2.)).child("Enter creates the account. You can register a username later.")),
            Gate::Failed(e) => card.child(div().text_color(t.red).child(format!("The Jami daemon failed to start: {e}"))),
            Gate::Ready => card,
        };
        div().flex().flex_1().items_center().justify_center().child(card).into_any_element()
    }
}

impl Render for Joma {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();
        self.refresh_frames(window, cx);
        // Keys reach actions only through a focused element: when the focused input is not
        // on screen (a request has no composer), the window itself takes the focus.
        let conv = self.selected.as_ref().and_then(|s| self.model.conversations.get(s));
        let composer_shown = self.gate == Gate::Ready && conv.is_some_and(|c| !c.request) && !self.stage;
        let input_shown = self.gate == Gate::Onboarding || (self.gate == Gate::Ready && self.mode != Mode::Chat);
        let lost = (!composer_shown && self.composer.focus_handle(cx).is_focused(window))
            || (!input_shown && self.input.focus_handle(cx).is_focused(window))
            || window.focused(cx).is_none();
        if composer_shown && std::mem::take(&mut self.focus_composer) {
            window.focus(&self.composer.focus_handle(cx));
        } else if lost {
            window.focus(&self.focus);
        }
        let body = if self.gate == Gate::Ready {
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .child(self.sidebar(&t, cx))
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
            .on_action(cx.listener(|_, _: &Quit, _, cx| cx.quit()))
            .on_action(cx.listener(|this, _: &NextConversation, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PrevConversation, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &NewConversation, window, cx| this.set_mode(Mode::Add, window, cx)))
            .on_action(cx.listener(|this, _: &Search, window, cx| this.set_mode(Mode::Search, window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
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
            .child(self.top_bar(&t))
            .child(body)
            .child(self.status_line(&t))
    }
}
