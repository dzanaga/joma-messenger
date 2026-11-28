//! The frame around the views: the icon rail, setup banner, first-run screens.

use super::*;

impl Joma {
    /// The rail on the left: the six views as icons (their names and keys in tooltips),
    /// then an update, settings, the panel/window switch and our picture (its menu holds
    /// the rest). The accent only for unread messages and an update.
    pub(super) fn rail(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let unread: u32 = self.listed_unread();
        let live = !self.model.calls.is_empty() || !self.running_shares_empty();
        let item = |n: usize, tab: Tab, g: Icon, title: &'static str, count: Option<String>, dot: bool| {
            let on = self.tab == tab && self.settings.is_none();
            div()
                .id(SharedString::from(format!("tab-{title}")))
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(36.))
                .text_color(if on { t.bright } else { t.muted })
                .when(on, |d| d.bg(t.raised))
                .cursor_pointer()
                .hover(|d| d.text_color(t.bright))
                .tooltip(tip(t, format!("{title} · Ctrl+{n}")))
                .child(g)
                .when_some(count, |d, c| {
                    d.child(div().absolute().top(px(3.)).right(px(3.)).text_size(px(t.size - 4.)).font_weight(FontWeight::BOLD).text_color(t.accent).child(c))
                })
                .when(dot, |d| d.child(div().absolute().top(px(6.)).right(px(6.)).size(px(6.)).rounded_full().bg(t.green)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.settings = None;
                    this.set_tab(tab, window, cx)
                }))
        };
        let reg = self.model.registration.as_str();
        let status = match reg {
            "REGISTERED" if self.quick.invisible => t.muted,
            "REGISTERED" => t.green,
            "" | "TRYING" | "INITIALIZING" => t.yellow,
            _ => t.red,
        };
        let full = crate::hypr::shape() == crate::hypr::Shape::Full;
        let small = |id: &'static str, g: Icon, on: bool, hint: String| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .size(px(36.))
                .text_color(if on { t.bright } else { t.muted })
                .when(on, |d| d.bg(t.raised))
                .cursor_pointer()
                .hover(|d| d.text_color(t.bright))
                .tooltip(tip(t, hint))
                .child(g)
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
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .child(item(1, Tab::Chats, G_COMMENT, "chats", (unread > 0).then(|| unread.min(99).to_string()), false))
                    .child(item(2, Tab::Calls, G_PHONE, "calls", None, live))
                    .child(item(3, Tab::Contacts, G_USERS, "contacts", None, false))
                    .child(item(4, Tab::Devices, G_LAPTOP, "devices", None, self.device_op.is_some()))
                    .child(item(5, Tab::Accounts, G_USER_CIRCLE, "accounts", None, false))
                    .child(item(6, Tab::Agents, G_ROBOT, "agents", None, false)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(2.))
                    .when_some(self.update.clone(), |d, v| {
                        d.child(small("strip-update", G_DOWNLOAD, false, format!("update to {}", joma_quick::version::pretty(&v))).text_color(t.accent).on_click(cx.listener(|this, _, _, cx| {
                            // The About page says what happens and has the button.
                            this.settings = None;
                            this.open_settings(cx);
                            if let Some(s) = this.settings.as_mut() {
                                s.section = 4;
                            }
                        })))
                    })
                    .child(small("strip-settings", G_COG, self.settings.is_some(), "settings".into()).on_click(cx.listener(|this, _, _, cx| {
                        if this.settings.take().is_none() {
                            this.open_settings(cx);
                        }
                        cx.notify();
                    })))
                    .child(small("strip-pop", if full { G_TRAY } else { G_EXPAND }, false, if full { "back to the panel".into() } else { "open as a window".into() }).on_click(cx.listener(move |this, _, window, cx| {
                        if full {
                            this.pop_in(cx)
                        } else {
                            this.pop_out(window, cx)
                        }
                    })))
                    .child(
                        div()
                            .id("strip-me")
                            .mt(px(S1))
                            .cursor_pointer()
                            .hover(|d| d.opacity(0.8))
                            .child(avatar(t, self.model.avatars.get(&self.model.me).cloned(), &self.model.me, &self.model.my_name, 26., Some(status)))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(Menu::Me, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The open menu, over everything: a click outside closes it.
    pub(super) fn menu_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu?;
        let close = |this: &mut Joma, cx: &mut Context<Joma>| {
            this.menu = None;
            cx.notify();
        };
        let body = match menu {
            Menu::Me => {
                let reg = self.model.registration.as_str();
                let state = match reg {
                    "REGISTERED" if self.quick.invisible => "online · invisible",
                    "REGISTERED" => "online · visible",
                    "" | "TRYING" | "INITIALIZING" => "connecting",
                    _ => "offline",
                };
                let name = if self.model.my_name.is_empty() { "jami".to_owned() } else { self.model.my_name.clone() };
                let handle = if self.model.username.is_empty() { "no username yet".to_owned() } else { format!("@{}", self.model.username) };
                let invisible = self.quick.invisible;
                let full = crate::hypr::shape() == crate::hypr::Shape::Full;
                menu_box(t)
                    .absolute()
                    .top(px(36.))
                    .right(px(GAP))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .px(px(S2))
                            .pt(px(S1))
                            .pb(px(S2))
                            .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child(name))
                            .child(div().text_size(px(t.size - 2.)).text_color(t.muted).child(format!("{handle} · {state}"))),
                    )
                    .child(div().h(px(1.)).mb(px(S1)).bg(t.border))
                    .child(menu_item("me-picture", G_PENCIL, "your picture", t).on_click(cx.listener(move |this, _, _, cx| {
                        close(this, cx);
                        this.open_profile(cx);
                    })))
                    .when(reg == "REGISTERED", |d| {
                        d.child(menu_item("me-invisible", if invisible { G_EYE } else { G_EYE_OFF }, if invisible { "become visible" } else { "become invisible" }, t).on_click(cx.listener(
                            move |this, _, _, cx| {
                                close(this, cx);
                                this.toggle_invisible(cx);
                            },
                        )))
                    })
                    .when(self.model.username.is_empty(), |d| {
                        d.child(menu_item("me-username", G_USER_PLUS, "choose a username", t).on_click(cx.listener(move |this, _, window, cx| {
                            close(this, cx);
                            this.set_mode(Mode::Username, window, cx);
                        })))
                    })
                    .child(menu_item("me-settings", G_COG, "settings", t).on_click(cx.listener(move |this, _, _, cx| {
                        close(this, cx);
                        this.open_settings(cx);
                    })))
                    .child(menu_item("me-keys", G_BOLT, "keys · F1", t).on_click(cx.listener(move |this, _, _, cx| {
                        close(this, cx);
                        this.keys_open = true;
                    })))
                    .child(menu_item("me-pop", if full { G_TRAY } else { G_EXPAND }, if full { "to the tray" } else { "full window" }, t).on_click(cx.listener(
                        move |this, _, window, cx| {
                            close(this, cx);
                            if full {
                                this.pop_in(cx)
                            } else {
                                this.pop_out(window, cx)
                            }
                        },
                    )))
            }
            Menu::Chat => {
                let conv = self.selected.as_ref().and_then(|s| self.model.conversations.get(s))?;
                let fav = self.quick.favorites.contains(&conv.id);
                let (id1, id2) = (conv.id.clone(), conv.id.clone());
                let listed = !conv.request && !self.is_removed(&conv.id);
                menu_box(t)
                    .absolute()
                    .top(px(38. + 44.))
                    .right(px(GAP + S2))
                    .child(menu_item("chat-star", if fav { G_STAR } else { G_STAR_O }, if fav { "unstar" } else { "star · keep on top" }, t).on_click(cx.listener(
                        move |this, _, _, cx| {
                            close(this, cx);
                            this.toggle_favorite(cx);
                        },
                    )))
                    .child(menu_item("chat-quick", G_BOLT, "quick messages · ctrl+.", t).on_click(cx.listener(move |this, _, _, cx| {
                        close(this, cx);
                        this.quick_open = !this.quick_open;
                    })))
                    .when(!conv.request, |d| {
                        d.child(menu_item("chat-file", G_CLIP, "send a file", t).on_click(cx.listener(move |this, _, _, cx| {
                            close(this, cx);
                            this.pick_and_send(id1.clone(), cx);
                        })))
                    })
                    .when(listed, |d| {
                        d.child(div().h(px(1.)).my(px(S1)).bg(t.border)).child(menu_item("chat-remove", G_TRASH, "remove from the list", t).on_click(cx.listener(
                            move |this, _, _, cx| {
                                close(this, cx);
                                this.remove_from_list(&id2, cx);
                            },
                        )))
                    })
            }
        };
        Some(
            div()
                .id("menu-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_click(cx.listener(move |this, _, _, cx| close(this, cx)))
                .child(body)
                .into_any_element(),
        )
    }

    /// A passing note (what the status line used to say), at the bottom of the window.
    pub(super) fn toast(&self, t: &Theme) -> Option<AnyElement> {
        let (text, _) = self.flash.as_ref()?;
        Some(
            div()
                .absolute()
                .bottom(px(14.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .max_w(px(560.))
                        .mx(px(S3))
                        .px(px(S3))
                        .py(px(6.))
                        .bg(t.raised)
                        .border_1()
                        .border_color(t.border)
                        .rounded(px(radius()))
                        .shadow_md()
                        .text_size(px(t.size - 1.))
                        .text_color(t.yellow)
                        .child(text.clone()),
                )
                .into_any_element(),
        )
    }

    /// Every key, in one overlay (F1).
    pub(super) fn keys_overlay(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.keys_open {
            return None;
        }
        const KEYS: &[(&str, &[(&str, &str)])] = &[
            ("move", &[("ctrl+1…6", "tabs"), ("alt+j / alt+k", "next, previous chat"), ("ctrl+k", "find"), ("ctrl+n", "add a contact"), ("ctrl+shift+k", "contacts"), ("esc", "back, close")]),
            ("conversation", &[("enter", "send"), ("shift+enter", "new line"), ("ctrl+v", "paste text or files"), ("ctrl+.", "quick messages"), ("alt+1…9", "send a quick message"), ("ctrl+shift+f", "star"), ("ctrl+enter", "accept a request"), ("ctrl+backspace", "decline a request")]),
            ("calls and sharing", &[("ctrl+shift+c", "call"), ("ctrl+shift+v", "video call"), ("ctrl+shift+a", "answer"), ("ctrl+shift+e", "hang up"), ("ctrl+shift+m", "mute"), ("ctrl+shift+o", "camera"), ("ctrl+shift+s", "share the desktop")]),
            ("stage", &[("f11", "full screen"), ("ctrl+shift+enter", "stage takes the window"), ("ctrl+= / ctrl+-", "zoom"), ("ctrl+0", "fit")]),
            ("joma", &[("f1", "this list"), ("super+alt+m", "the panel under the bar"), ("ctrl+q", "quit")]),
        ];
        let mut body = div().id("keys-body").flex().flex_wrap().gap(px(20.)).p(px(16.)).max_h(px(560.)).overflow_y_scroll();
        for (group, keys) in KEYS {
            let mut col = div().flex().flex_col().gap(px(3.)).w(px(300.)).child(caption(t, group).pb(px(S1)));
            for (k, what) in *keys {
                col = col.child(div().flex().gap(px(S2)).child(div().w(px(132.)).flex_none().text_color(t.bright).child(*k)).child(div().text_color(t.muted).child(*what)));
            }
            body = body.child(col);
        }
        Some(
            div()
                .id("keys-overlay")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::black().opacity(0.55))
                .occlude()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.keys_open = false;
                    cx.notify();
                }))
                .child(div().flex().flex_col().w(px(690.)).max_w_full().m(px(16.)).bg(t.bg).border_1().border_color(t.border).rounded(px(radius())).child(
                    div().px(px(S3)).py(px(S2)).border_b_1().border_color(t.border).text_color(t.bright).font_weight(FontWeight::BOLD).child("Keys"),
                ).child(body))
                .into_any_element(),
        )
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
                .bg(t.panel)
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

    pub(super) fn gate_screen(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let card = div().flex().flex_col().gap(px(12.)).w(px(520.)).max_w_full().p(px(24.)).bg(t.panel);
        let card = match &self.gate {
            Gate::QtRunning => card
                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Jami is open"))
                .child(div().child("Joma uses the same accounts and history as the Jami app, and only one of them can run at a time. Jami keeps running in the tray after its window is closed: quit it from its tray icon (or run `pkill -x jami`). Joma comes back by itself once it quits."))
                .child(button("retry", G_CHECK, "retry", t, true).on_click(cx.listener(|this, _, _, cx| this.boot(cx)))),
            Gate::Onboarding if self.linking.is_some() => self.link_card(card, t, cx),
            Gate::Onboarding if self.restoring.is_some() || self.password_ask.is_some() => self.restore_card(card, t, cx),
            Gate::Onboarding if self.adding_account.is_some() => card
                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Another account"))
                .child(div().child("A second Jami identity on this machine, with its own contacts and history. Switch between them in Contacts."))
                .child(self.onboarding_body(t, cx))
                .child(div().flex().child(button("account-back", G_BACK, "back", t, false).on_click(cx.listener(|this, _, window, cx| this.cancel_new_account(window, cx))))),
            Gate::Onboarding => card
                .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Welcome to Joma"))
                .child(div().child("Joma is a Jami client for Omarchy. Create a Jami account to start: it lives on this machine, no server and no phone number."))
                .child(self.onboarding_body(t, cx)),
            Gate::Failed(e) => card.child(div().text_color(t.red).child(format!("The Jami daemon failed to start: {e}"))),
            Gate::Ready => card,
        };
        div().flex().flex_1().items_center().justify_center().child(card).into_any_element()
    }

    /// Signing in with the Joma GitHub App: the code to type on GitHub.
    pub(super) fn github_signin_card(&self, t: &Theme, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let s = self.gh_signin.as_ref()?;
        let mut card = div().flex().flex_col().gap(px(8.)).p(px(10.)).bg(t.raised).child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Sign in with GitHub"));
        if let Some(e) = &s.error {
            card = card.child(div().text_color(t.red).child(e.clone())).child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(button("gh-signin-again", G_UNDO, "try again", t, true).on_click(cx.listener({
                        let after = s.after;
                        move |this, _, _, cx| {
                            this.gh_signin = None;
                            this.github_signin(after, cx)
                        }
                    })))
                    .child(button("gh-signin-close", G_TIMES, "close", t, false).on_click(cx.listener(|this, _, _, cx| this.cancel_github_signin(cx)))),
            );
            return Some(card);
        }
        let Some(code) = &s.code else {
            return Some(card.child(div().text_color(t.muted).child("asking GitHub for a code…")));
        };
        let (url, user_code) = (code.verification_uri.clone(), code.user_code.clone());
        Some(
            card.child(div().text_color(t.muted).child(format!("On GitHub ({url}), type this code:")))
                .child(div().text_size(px(t.size + 10.)).text_color(t.bright).font_weight(FontWeight::BOLD).child(user_code.clone()))
                .child(div().text_color(t.muted).text_size(px(t.size - 2.)).child(
                    "Joma's GitHub App may then write your ✓ badge gist and the private joma-backup repo, nothing else. Joma keeps no token after it quits, unless automatic backups are on.",
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(button("gh-signin-open", G_LINK, "copy the code and open GitHub", t, true).on_click(cx.listener(move |_, _, _, cx| {
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(user_code.clone()));
                            cx.open_url(&url);
                        })))
                        .child(button("gh-signin-cancel", G_TIMES, "cancel", t, false).on_click(cx.listener(|this, _, _, cx| this.cancel_github_signin(cx)))),
                )
                .child(div().text_color(t.muted).text_size(px(t.size - 2.)).child("waiting for GitHub…")),
        )
    }

    /// Restore accounts from the GitHub backup: pick them, type the password once.
    pub(super) fn restore_card(&self, card: gpui::Div, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let v = self.restoring.clone().unwrap_or_default();
        let asking = self.password_ask.is_some();
        let card = card
            .child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Restore from your backup"))
            .child(div().text_color(t.muted).child(
                "Your accounts come back with the same Jami ID, contacts and username; messages return from your contacts and other devices. If an account still runs elsewhere, this machine joins it as a new device.",
            ));
        let here: Vec<String> = self.accounts.iter().filter_map(|(id, _)| joma_jami::account_uri(id).ok()).collect();
        let body = if let Some(card) = self.github_signin_card(t, cx) {
            card
        } else if let Some(from) = &v.cli {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(div().text_color(t.muted).text_size(px(t.size - 2.)).child(format!("joma restore: from {from}")))
                .when(asking, |d| d.child(self.input.clone()))
                .when(v.working, |d| d.child(div().text_color(t.muted).child("restoring… the account opens by itself")))
        } else if v.loading {
            div().text_color(t.muted).child("reading github.com/…/joma-backup")
        } else if v.working {
            div().text_color(t.muted).child("restoring… the account opens by itself")
        } else if v.entries.is_empty() && !asking {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(div().text_color(t.red).child(if v.error.is_empty() { "No accounts in the backup.".to_owned() } else { v.error.clone() }))
                .child(div().text_color(t.muted).text_size(px(t.size - 2.)).child("Sign in with GitHub (or run gh auth login in a terminal), then try again. A backup file: joma restore --file <path>."))
                .child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .child(button("restore-signin", G_LINK, "sign in with GitHub", t, true).on_click(cx.listener(|this, _, _, cx| this.github_signin(crate::signin::After::Restore, cx))))
                        .child(button("restore-retry", G_UNDO, "try again", t, false).on_click(cx.listener(|this, _, _, cx| this.start_restore(cx)))),
                )
        } else {
            let mut list = div().flex().flex_col();
            for e in &v.entries {
                let on = v.picked.contains(&e.jami);
                let present = here.contains(&e.jami);
                let j = e.jami.clone();
                list = list.child(
                    div()
                        .id(SharedString::from(format!("restore-{}", e.jami)))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(8.))
                        .py(px(6.))
                        .when(on, |d| d.bg(t.sel))
                        .when(!present && !asking, |d| d.cursor_pointer().hover(|d| d.bg(t.sel)))
                        .child(div().flex_none().w(px(14.)).text_color(if on { t.bright } else { t.muted }).child(if present { "·" } else if on { "✓" } else { "○" }))
                        .child(div().flex_1().min_w(px(0.)).truncate().text_color(if on { t.bright } else { t.text }).child(e.label.clone()))
                        .child(div().flex_none().text_color(t.muted).text_size(px(t.size - 2.)).child(if present { "already here".to_owned() } else { format!("{}…", &e.jami[..e.jami.len().min(8)]) }))
                        .when(!present && !asking, |d| d.on_click(cx.listener(move |this, _, _, cx| this.toggle_restore_pick(&j, cx)))),
                );
            }
            let mut col = div().flex().flex_col().gap(px(8.)).child(div().text_color(t.muted).text_size(px(t.size - 2.)).child(format!("github.com/{}/joma-backup", v.login))).child(list);
            if !v.error.is_empty() {
                col = col.child(div().text_color(t.red).child(v.error.clone()));
            }
            if asking {
                col = col.child(self.input.clone()).child(div().text_color(t.muted).text_size(px(t.size - 2.)).child("The password you chose for the backup (the same for all accounts)."));
            } else {
                col = col.child(div().flex().child(button("restore-go", G_CHECK, "restore", t, true).on_click(cx.listener(|this, _, window, cx| this.restore_continue(window, cx)))));
            }
            col
        };
        card.child(body).child(div().flex().child(button("restore-back", G_BACK, "back", t, false).on_click(cx.listener(|this, _, window, cx| this.cancel_restore(window, cx)))))
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().flex_1().text_color(t.muted).child("Backed up on GitHub?"))
                        .child(button("restore-start", G_DOWNLOAD, "restore from your backup", t, false).on_click(cx.listener(|this, _, _, cx| this.start_restore(cx)))),
                )
    }
}
