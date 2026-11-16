//! The frame around the views: top bar, setup banner, tab rail, status line, first-run screens.

use super::*;

impl Joma {
    pub(super) fn top_bar(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
                            .child(if full { labelled(G_COMPRESS, "to the tray") } else { labelled(G_EXPAND, "full") })
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
    pub(super) fn setup_banner(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
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

    /// The rail: one button per tab, our picture at the bottom.
    pub(super) fn rail(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let unread: u32 = self.listed_unread();
        let live = !self.model.calls.is_empty() || !self.running_shares_empty();
        let item = |tab: Tab, g: Icon, title: &str, badge: Option<String>, dot: bool| {
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
                .child(g)
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

    pub(super) fn status_line(&self, t: &Theme) -> AnyElement {
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

    pub(super) fn gate_screen(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
    pub(super) fn onboarding_body(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
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
