//! The chats sidebar: requests, starred and other conversations, search, add, removed.

use super::*;

impl Joma {
    pub(super) fn sidebar(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
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
}
