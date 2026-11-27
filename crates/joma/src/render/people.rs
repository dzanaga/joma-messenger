//! Contacts, accounts, agents and devices (with linking).

use super::*;

impl Joma {
    /// Onboarding by joining an account from another device: the code, as text and QR.
    /// The connection info under a contact or a device, when it is the open one.
    fn conn_panel(&self, key: &str, matches: impl Fn(&crate::conninfo::Row) -> bool, t: &Theme) -> Option<gpui::Div> {
        let (open, rows) = self.conn_info.as_ref()?;
        if open != key {
            return None;
        }
        let small = |s: String, c| div().text_size(px(t.size - 2.)).text_color(c).child(s);
        let mut panel = div().flex().flex_col().gap(px(3.)).mx(px(10.)).my(px(2.)).p(px(6.)).border_1().border_color(t.border).rounded(px(radius()));
        let Some(rows) = rows else { return Some(panel.child(small("reading the connections…".into(), t.muted))) };
        let mine: Vec<&crate::conninfo::Row> = rows.iter().filter(|r| matches(r)).collect();
        if mine.is_empty() {
            return Some(panel.child(small("not connected now: devices connect when there is something to send".into(), t.muted)));
        }
        for r in mine {
            let dev = if r.device_name.is_empty() { format!("device {}…", &r.device[..r.device.len().min(8)]) } else { r.device_name.clone() };
            let (path_colour, note) = match r.path {
                _ if r.relayed => (t.yellow, " · slow: through a relay"),
                joma_jami::links::Path::Unknown => (t.muted, ""),
                _ => (t.green, ""),
            };
            panel = panel
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .child(div().text_color(t.text).child(dev))
                        .child(div().text_color(t.muted).child(r.status.clone()))
                        .child(div().text_color(path_colour).child(format!("{}{note}", r.path.label()))),
                )
                .when(!r.address.is_empty(), |d| d.child(small(r.address.clone(), t.muted)))
                .when(!r.channels.is_empty(), |d| d.child(small(format!("channels: {}", r.channels.join(", ")), t.muted)));
        }
        Some(panel)
    }

    /// Folder sync with our other devices (D-23): folders, devices, the path to each.
    pub(super) fn sync_group(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let small = |s: String, c| div().text_size(px(t.size - 2.)).text_color(c).child(s);
        let head = div().px(px(10.)).pt(px(10.)).pb(px(2.)).text_size(px(t.size - 2.5)).text_color(t.muted).child("SYNCED FOLDERS");
        let mut g = div().flex().flex_col().gap(px(3.)).child(head);
        let row = || div().flex().flex_col().px(px(10.)).py(px(2.));
        if let Some(e) = &self.sync.error {
            g = g.child(row().child(small(e.clone(), t.red)));
        }
        if self.sync.st.is_none() {
            if self.sync.starting {
                return g.child(row().child(small("starting Syncthing…".into(), t.muted)));
            }
            return g.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .px(px(10.))
                    .child(small("keep folders the same on all your devices".into(), t.muted))
                    .child(button("sync-on", G_EXCHANGE, "turn on", t, false).on_click(cx.listener(|this, _, _, cx| this.sync_switch(true, cx)))),
            );
        }
        let Some(r) = &self.sync_view else { return g.child(row().child(small("reading…".into(), t.muted))) };
        if r.folders.is_empty() {
            g = g.child(row().child(small("no folders yet".into(), t.muted)));
        }
        for f in &r.folders {
            let need = if f.need_files > 0 { format!(" · needs {} files, {}", f.need_files, crate::sync::size(f.need_bytes)) } else { String::new() };
            g = g.child(
                row()
                    .child(div().flex().gap(px(6.)).child(div().text_color(t.bright).child(f.label.clone())).child(small(format!("{} · {} files, {}{need}", f.state, f.files, crate::sync::size(f.bytes)), if need.is_empty() { t.muted } else { t.yellow })))
                    .child(small(f.path.clone(), t.muted))
                    .when(!f.error.is_empty(), |d| d.child(small(f.error.clone(), t.red))),
            );
        }
        if r.devices.is_empty() {
            g = g.child(row().child(small("no other device heard yet: start Joma on it with sync on".into(), t.muted)));
        }
        for d in &r.devices {
            let (path, colour) = match d.path {
                Some(p) if d.relayed => (format!("{} · slow for large files", p.label()), t.yellow),
                Some(p) => (p.label().to_string(), t.green),
                None => ("not connected now".to_string(), t.muted),
            };
            let st = if d.connected { "syncing with it" } else if d.tunnel { "tunnel open" } else { "waiting for a tunnel" };
            g = g.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(2.))
                    .child(div().text_color(t.text).child(if d.name.is_empty() { format!("{}…", &d.device[..d.device.len().min(8)]) } else { d.name.clone() }))
                    .child(small(st.into(), t.muted))
                    .child(small(path, colour)),
            );
        }
        g.child(
            div()
                .flex()
                .gap(px(6.))
                .px(px(10.))
                .pt(px(4.))
                .child(button("sync-add", G_PLUS, "add folder", t, false).on_click(cx.listener(|this, _, _, cx| this.choose_sync_folder(cx))))
                .child(button("sync-off", G_TIMES, "turn off", t, false).on_click(cx.listener(|this, _, _, cx| this.sync_switch(false, cx)))),
        )
    }

    pub(super) fn link_card(&self, card: gpui::Div, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let Some(l) = &self.linking else { return card };
        let card = card.child(div().text_color(t.bright).font_weight(FontWeight::BOLD).child("Link this device to your account"));
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
    pub(super) fn account_card(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
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
        // The ✓ GitHub badge: public, apart from the (private) backup.
        .child({
            let public = self.gh.mine.get(&self.model.me).cloned();
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(4.))
                .child(div().flex_1().min_w(px(0.)).text_size(px(t.size - 2.)).text_color(t.muted).child(match &public {
                    Some(login) => format!("contacts see ✓ github.com/{login}"),
                    None => "no GitHub badge: contacts don't see who you are on GitHub".to_owned(),
                }))
                .child(
                    button("gh-publish", G_CHECK, if self.gh_busy { "…" } else if public.is_some() { "hide" } else { "show a GitHub badge" }, t, public.is_some())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let on = !this.gh.mine.contains_key(&this.model.me);
                            this.github_publish(on, cx);
                        })),
                )
        })
    }

    /// This account's devices, and linking a new one.
    pub(super) fn devices_group(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let mut group = div().flex().flex_col();
        // Devices that joined without this machine linking them (a restore, a link made
        // elsewhere): ours, or someone else's? Kept or revoked here.
        for id in &self.new_devices {
            let name = self.model.devices.get(id).cloned().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("{}…", &id[..id.len().min(8)]));
            let (k, r) = (id.clone(), id.clone());
            group = group.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .mx(px(10.))
                    .my(px(4.))
                    .p(px(8.))
                    .bg(t.raised)
                    .child(div().text_color(t.bright).child(format!("New device {name} joined your account")))
                    .child(div().text_color(t.muted).text_size(px(t.size - 2.)).child("Restored from your backup, or linked from another of your devices. If it is not yours, revoke it: it loses the account."))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(button(SharedString::from(format!("dev-keep-{id}")), G_CHECK, "keep", t, true).on_click(cx.listener(move |this, _, _, cx| this.keep_device(&k, cx))))
                            .child(button(SharedString::from(format!("dev-new-revoke-{id}")), G_BAN, if self.is_armed(&format!("revoke:{id}")) { "revoke?" } else { "revoke" }, t, false).on_click(cx.listener(move |this, _, _, cx| this.revoke_device(&r, cx)))),
                    ),
            );
        }
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
            let (k, key) = (format!("device:{id}"), format!("device:{id}"));
            let dev_id = id.clone();
            let info = self.conn_panel(&key, |r| r.device == dev_id, t);
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
                            button(SharedString::from(format!("dev-conn-{id}")), G_SIGNAL, "", t, info.is_some())
                                .text_color(t.muted)
                                .on_click(cx.listener(move |this, _, _, cx| this.toggle_conn_info(k.clone(), cx))),
                        )
                        .child(
                            button(SharedString::from(format!("dev-revoke-{id}")), G_BAN, if armed { "revoke?" } else { "" }, t, armed)
                                .text_color(if armed { t.red } else { t.muted })
                                .on_click(cx.listener(move |this, _, _, cx| this.revoke_device(&i, cx))),
                        )
                    }),
            );
            if let Some(p) = info {
                group = group.child(p);
            }
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
                .border_color(t.yellow).rounded(px(radius()))
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
    pub(super) fn link_steps(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let cmd = |id: &'static str, text: &str| {
            let copy = text.to_owned();
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().flex_1().min_w(px(0.)).px(px(6.)).py(px(3.)).bg(t.bar).text_color(t.bright).truncate().child(text.to_owned()))
                .child(button(id, G_COPY, "", t, false).on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(copy.clone()));
                    this.show_flash("copied", cx);
                })))
        };
        let step = |n: &str, text: &str| {
            div()
                .flex()
                .gap(px(8.))
                .child(div().flex_none().size(px(18.)).rounded_full().bg(t.raised).text_color(t.bright).text_size(px(t.size - 3.)).flex().items_center().justify_center().child(n.to_owned()))
                .child(div().flex_1().min_w(px(0.)).text_color(t.text).text_size(px(t.size - 1.)).child(text.to_owned()))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .mx(px(10.))
            .my(px(8.))
            .p(px(10.))
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

    /// The Accounts tab: the open one, the others on this machine, adding one.
    pub(super) fn accounts_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
    pub(super) fn agents_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let card = |name: &str, what: &str| {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .mx(px(10.))
                .my(px(4.))
                .p(px(8.))
                .border_1()
                .border_color(t.border).rounded(px(radius()))
                .opacity(0.6)
                .child(div().size(px(32.)).rounded(px(radius())).bg(t.accent_bg).text_color(t.accent).flex().items_center().justify_center().child(G_ROBOT))
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

    /// The machine's other accounts: a click opens one.
    pub(super) fn accounts_group(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
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
    pub(super) fn contacts_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
                "no contacts yet · + adds one".to_string()
            }));
        }
        for c in contacts {
            let uri = c.uri.clone();
            let name = self.model.name_of(&uri);
            let starred = self.quick.starred.contains(&uri);
            let waiting = !c.confirmed && self.model.chat_with(&uri).is_none_or(|v| v.request);
            let dot = self.dot_of(&uri);
            let selected = self.model.chat_with(&uri).is_some_and(|v| self.selected.as_deref() == Some(v.id.as_str()));
            let quick = |id: &str, glyph: Icon, what: Quick| {
                let u = uri.clone();
                button(SharedString::from(format!("{id}-{uri}")), glyph, "", t, false)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.quick_action(&u, what, window, cx);
                    }))
            };
            let (u_star, u_block, u_remove, u_open) = (uri.clone(), uri.clone(), uri.clone(), uri.clone());
            let conn_key = format!("peer:{}", crate::model::bare_uri(&uri));
            let bare = crate::model::bare_uri(&uri).to_string();
            let info = self.conn_panel(&conn_key, |r| crate::model::bare_uri(&r.peer) == bare, t);
            let info_open = info.is_some();
            let block_armed = self.is_armed(&format!("block:{uri}"));
            let remove_armed = self.is_armed(&format!("remove:{uri}"));
            let row = div()
                .id(SharedString::from(format!("contact-{uri}")))
                .flex()
                .flex_col()
                .gap(px(3.))
                .mx(px(S1))
                .px(px(S2))
                .py(px(6.))
                .rounded(px(radius()))
                .when(selected, |d| d.bg(t.sel))
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
                            button(SharedString::from(format!("cq-conn-{uri}")), G_SIGNAL, "", t, info_open)
                                .text_color(t.muted)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.toggle_conn_info(conn_key.clone(), cx);
                                })),
                        )
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
                )
                .when_some(info, |d, p| d.child(p));
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
}
