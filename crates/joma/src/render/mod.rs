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
pub use crate::ui::*;

/// The emoji the picker offers.
const EMOJI: &[&str] = &[
    "😀", "😃", "😄", "😁", "😆", "😅", "🤣", "😂", "🙂", "😉", "😊", "😇", "🥰", "😍", "🤩", "😘", "😋", "😛", "😜", "🤪", "😝", "🤗", "🤭", "🤫",
    "🤔", "🤐", "🤨", "😐", "😑", "😶", "😏", "😒", "🙄", "😬", "😌", "😔", "😪", "😴", "😷", "🤒", "🤕", "🤢", "🤮", "🥵", "🥶", "😵", "🤯", "🤠",
    "🥳", "😎", "🤓", "🧐", "😕", "😟", "🙁", "😮", "😯", "😲", "😳", "🥺", "😦", "😧", "😨", "😰", "😥", "😢", "😭", "😱", "😖", "😣", "😞", "😓",
    "😩", "😫", "🥱", "😤", "😡", "😠", "🤬", "😈", "💀", "💩", "🤡", "👻", "👽", "🤖", "👍", "👎", "👌", "✌️", "🤞", "🤟", "🤘", "👋", "👏", "🙌",
    "🙏", "💪", "👀", "🧠", "❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "💔", "💯", "🔥", "✨", "⭐", "🎉", "🎁", "🎂", "🍕", "🍔", "🍺", "☕", "🍷",
    "⚽", "🎮", "🎵", "🎬", "📷", "💻", "📱", "⌨️", "🚀", "✈️", "🚗", "🏠", "🌍", "☀️", "🌙", "🌧️", "❄️", "✅", "❌", "⚠️", "❓", "💡", "📌", "⏰",
];
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

/// A person's picture, or their initial on their colour; a presence dot in the corner.
pub fn avatar(t: &Theme, picture: Option<std::path::PathBuf>, key: &str, name: &str, side: f32, dot: Option<Hsla>) -> AnyElement {
    let initial = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into());
    let color = author_color(t, key);
    div()
        .relative()
        .flex_none()
        .size(px(side))
        .child(match picture {
            Some(p) => div().size(px(side)).rounded(px(radius())).overflow_hidden().child(gpui::img(p).size(px(side)).object_fit(ObjectFit::Cover)).into_any_element(),
            None => div()
                .size(px(side))
                .rounded(px(radius()))
                .flex()
                .items_center()
                .justify_center()
                .bg(color.opacity(0.22))
                .border_1()
                .border_color(color.opacity(0.5)).rounded(px(radius()))
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
        div().px(px(10.)).py(px(4.)).bg(self.bg).border_1().border_color(self.border).rounded(px(radius())).text_color(self.fg).child(self.title.clone())
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

mod shell;
mod sidebar;
mod people;
mod calls;
mod stage_view;
mod chat;
mod overlays;

impl Joma {
    /// Copy new pictures (remote video, the desktop share) into images to paint.
    pub(super) fn refresh_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        self.tab_labels = window.viewport_size().width >= px(900.);
        let chat_width = f32::from(window.viewport_size().width) - if self.narrow || (self.stage && self.has_stage()) { 2. * GAP } else { 290. + 3. * GAP };
        self.bubble_max = ((chat_width - 30.) * 0.74).max(160.);
        self.composer_focused = composer_shown && self.composer.focus_handle(cx).is_focused(window);
        let full = if self.fullscreen { self.fullscreen_stage(&t, cx) } else { None };
        let picker = self.picker_overlay(&t, cx);
        let profile = self.profile_overlay(&t, cx);
        let settings = if self.gate == Gate::Ready && !self.fullscreen { self.settings_page(&t, cx) } else { None };
        let body = if let Some(full) = full {
            full
        } else if let Some(page) = settings {
            page
        } else if self.gate == Gate::Ready && self.narrow {
            let chat = self.narrow_chat && self.selected.is_some();
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .px(px(GAP))
                .pb(px(GAP))
                .when(chat, |d| d.child(tile(&t, true).flex_1().bg(t.bg).child(self.chat(&t, cx))))
                .when(!chat, |d| d.child(self.sidebar(&t, cx)))
                .into_any_element()
        } else if self.gate == Gate::Ready {
            div()
                .flex()
                .flex_1()
                .min_h(px(0.))
                .gap(px(GAP))
                .px(px(GAP))
                .pb(px(GAP))
                // A stage that takes the window takes the sidebar's room too.
                .when(!(self.stage && self.has_stage()), |d| d.child(self.sidebar(&t, cx)))
                .child(tile(&t, self.mode == Mode::Chat).flex_1().bg(t.bg).child(self.chat(&t, cx)))
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
            .bg(t.bar)
            .text_color(t.text)
            .font_family(t.font.clone())
            .text_size(px(t.size))
            .on_action(cx.listener(|this, _: &Quit, _, cx| {
                this.leave();
                cx.quit()
            }))
            .on_action(cx.listener(|this, _: &ShowKeys, _, cx| {
                this.keys_open = !this.keys_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextConversation, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PrevConversation, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &NewConversation, window, cx| this.set_mode(Mode::Add, window, cx)))
            .on_action(cx.listener(|this, _: &Search, window, cx| this.set_mode(Mode::Search, window, cx)))
            .on_action(cx.listener(|this, _: &Cancel, window, cx| {
                // Esc closes a menu, the keys, the picker or the profile, then leaves full screen.
                if this.viewer.take().is_some() {
                    cx.notify();
                    return;
                }
                if this.menu.is_some() || this.keys_open {
                    this.menu = None;
                    this.keys_open = false;
                    cx.notify();
                    return;
                }
                if this.replying.is_some() || this.editing.is_some() || this.msg_actions.is_some() {
                    this.msg_actions = None;
                    this.cancel_reply(cx);
                    return;
                }
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
            .when(!self.fullscreen && self.gate == Gate::Ready, |d| d.child(self.tab_strip(&t, cx)).children(self.setup_banner(&t, cx)))
            .child(body)
            .children(self.menu_overlay(&t, cx))
            .children(self.keys_overlay(&t, cx))
            .children(self.viewer_overlay(&t, cx))
            .children(self.incoming_overlay(&t, cx))
            .children(self.toast(&t))
            .children(picker)
            .children(profile)
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
pub fn is_picture(path: &std::path::Path) -> bool {
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
