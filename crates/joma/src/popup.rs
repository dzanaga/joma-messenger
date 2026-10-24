//! The Quick Messages popup: a small window (app id `joma-quick`) to send a canned
//! message to a favourite contact without opening Joma. `joma quick` opens it in the
//! running Joma, so it can be bound to a key in Hyprland.

use gpui::prelude::*;
use gpui::{
    div, px, size, App, Bounds, Context, Entity, FocusHandle, Focusable, FontWeight, KeyDownEvent, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use joma_quick::ipc::{QuickState, Reply};

use crate::app::Joma;
use crate::render::{G_BOLT, G_STAR};

pub struct QuickPopup {
    joma: Entity<Joma>,
    state: QuickState,
    target: usize,
    focus: FocusHandle,
    note: Option<String>,
}

/// Open the popup, or bring it forward. Call it outside an update of `joma`.
pub fn open(joma: Entity<Joma>, cx: &mut App) {
    if let Some(h) = joma.read(cx).quick_popup {
        let brought = h.update(cx, |p, window, cx| {
            p.state = p.joma.read(cx).quick_state();
            window.activate_window();
        });
        if brought.is_ok() {
            return;
        }
    }
    let state = joma.read(cx).quick_state();
    let bounds = Bounds::centered(None, size(px(560.), px(380.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions { title: Some("Joma Quick".into()), ..Default::default() }),
        app_id: Some("joma-quick".into()),
        ..Default::default()
    };
    let j = joma.clone();
    let handle = cx.open_window(options, |window, cx| {
        cx.new(|cx| {
            let focus = cx.focus_handle();
            window.focus(&focus);
            QuickPopup { joma: j, state, target: 0, focus, note: None }
        })
    });
    joma.update(cx, |j, _| j.quick_popup = handle.ok());
}

impl Focusable for QuickPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl QuickPopup {
    fn send(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.state.targets.get(self.target).cloned() else { return };
        let Some(text) = self.state.messages.get(index).cloned() else { return };
        let reply = self.joma.update(cx, |j, cx| {
            let r = j.send_to(&target.conversation, &text);
            cx.notify();
            r
        });
        match reply {
            Reply::Error { message } => {
                self.note = Some(message);
                cx.notify();
            }
            _ => window.remove_window(),
        }
    }

    fn key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.state.targets.len().max(1);
        match ev.keystroke.key.as_str() {
            "escape" => window.remove_window(),
            "down" | "j" | "tab" => self.target = (self.target + 1) % n,
            "up" | "k" => self.target = (self.target + n - 1) % n,
            d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() && d != "0" => {
                self.send((d.as_bytes()[0] - b'1') as usize, window, cx);
                return;
            }
            _ => return,
        }
        cx.notify();
    }
}

impl Render for QuickPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.joma.read(cx).theme.clone();
        let mut targets = div().flex().flex_col().w(px(230.)).border_r_1().border_color(t.border);
        for (i, tg) in self.state.targets.iter().enumerate() {
            let on = i == self.target;
            targets = targets.child(
                div()
                    .id(SharedString::from(format!("t{i}")))
                    .flex()
                    .flex_col()
                    .px(px(10.))
                    .py(px(5.))
                    .border_l_2()
                    .border_color(if on { t.accent } else { gpui::transparent_black() })
                    .when(on, |d| d.bg(t.accent_bg))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.target = i;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .when(tg.favorite, |d| d.child(div().text_color(t.yellow).child(G_STAR)))
                            .child(div().truncate().text_color(if on { t.bright } else { t.text }).child(tg.title.clone()))
                            .when(tg.unread > 0, |d| d.child(div().text_color(t.accent).child(format!("{}", tg.unread)))),
                    )
                    .child(div().truncate().text_size(px(t.size - 2.)).text_color(t.muted).child(tg.last.clone())),
            );
        }
        let mut messages = div().flex().flex_col().flex_1().p(px(8.)).gap(px(4.));
        for (i, m) in self.state.messages.iter().enumerate().take(9) {
            messages = messages.child(
                div()
                    .id(SharedString::from(format!("m{i}")))
                    .flex()
                    .gap(px(10.))
                    .px(px(8.))
                    .py(px(4.))
                    .border_1()
                    .border_color(t.border)
                    .cursor_pointer()
                    .hover(|d| d.bg(t.raised).border_color(t.accent))
                    .on_click(cx.listener(move |this, _, window, cx| this.send(i, window, cx)))
                    .child(div().text_color(t.accent).child(format!("{}", i + 1)))
                    .child(div().text_color(t.bright).child(m.clone())),
            );
        }
        div()
            .id("quick")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(t.font.clone())
            .text_size(px(t.size))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px(px(10.))
                    .py(px(6.))
                    .bg(t.bar)
                    .border_b_1()
                    .border_color(t.border)
                    .child(div().text_color(t.accent).font_weight(FontWeight::BOLD).child(format!("{G_BOLT} quick message")))
                    .child(div().text_color(t.muted).child("j/k: contact · 1-9: send · esc")),
            )
            .child(if self.state.targets.is_empty() {
                div().flex_1().p(px(16.)).text_color(t.muted).child("No conversations yet.").into_any_element()
            } else {
                div().flex().flex_1().min_h(px(0.)).child(targets).child(messages).into_any_element()
            })
            .when_some(self.note.clone(), |d, n| d.child(div().px(px(10.)).py(px(4.)).text_color(t.red).child(n)))
    }
}
