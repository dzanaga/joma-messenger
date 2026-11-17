//! The calls tab and the call panel above a conversation.

use super::*;

impl Joma {
    /// The Calls tab: what runs now (open it or end it), then calls and shares that took
    /// place, newest first.
    pub(super) fn calls_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().id("calls").flex().flex_col().flex_1().overflow_y_scroll();
        let small = |label: &str| div().px(px(10.)).pt(px(10.)).pb(px(4.)).text_size(px(t.size - 2.5)).text_color(t.muted).child(label.to_uppercase());
        let entry = |id: SharedString, glyph_str: Icon, color: Hsla, who: &str, key: &str, what: String, when: String| {
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
                        .child(div().flex().items_center().gap(px(4.)).text_size(px(t.size - 2.)).text_color(color).child(glyph_str).child(div().truncate().child(what))),
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

    pub(super) fn call_panel(&self, t: &Theme, conv: Option<&Conversation>, cx: &mut Context<Self>) -> Option<AnyElement> {
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
            .child(div().px(px(4.)).border_1().border_color(t.border).rounded(px(radius())).text_color(if call.peer_joma { t.accent } else { t.muted }).child(joma_badge))
            .when_some(sharing.filter(|_| !joma_sharing), |d, s| d.child(labelled(G_BOLT, s).text_color(t.yellow)));

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
}
