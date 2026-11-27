//! A conversation: header, messages, cards, composer, watching together.

use super::*;

impl Joma {
    /// Above the message field while answering or editing: what, and a way out.
    pub(super) fn reply_strip(&self, t: &Theme, conv: &Conversation, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (id, editing) = match (&self.editing, &self.replying) {
            (Some(id), _) => (id, true),
            (None, Some(id)) => (id, false),
            _ => return None,
        };
        let m = conv.messages.iter().find(|m| m.id == *id)?;
        let whom = if crate::model::bare_uri(&m.author) == self.model.me { "yourself".to_owned() } else { self.model.name_of(&m.author) };
        let line: String = m.text.lines().next().unwrap_or("").chars().take(90).collect();
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(S2))
                .mx(px(S3))
                .pl(px(S2))
                .border_l_2()
                .border_color(t.accent)
                .text_size(px(t.size - 2.))
                .child(div().flex_none().text_color(t.accent).child(if editing { "editing".to_owned() } else { format!("replying to {whom}") }))
                .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.muted).child(line))
                .child(button("reply-cancel", G_TIMES, "", t, false).on_click(cx.listener(|this, _, _, cx| this.cancel_reply(cx))))
                .into_any_element(),
        )
    }

    /// The message field: the text, then attach, emoji, YouTube, voice and video messages
    /// inside it; while recording, the recording's time with send and cancel.
    pub(super) fn composer_row(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let field = div().flex().flex_none().items_end().gap(px(2.)).mx(px(S3)).mt(px(S1)).mb(px(S3)).pl(px(S2)).pr(px(S1)).bg(t.panel).border_1().rounded(px(radius()));
        let tools = div().flex().flex_none().items_center().gap(px(2.)).h(px(t.size * 1.6 + 14.));
        if let Some(r) = &self.recording {
            let secs = r.started.elapsed().as_secs();
            return field
                .border_color(t.red)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap(px(S2))
                        .h(px(t.size * 1.6 + 14.))
                        .child(div().size(px(9.)).rounded_full().bg(t.red))
                        .child(div().text_color(t.bright).child(format!("recording a {} message · {}:{:02}", if r.video { "video" } else { "voice" }, secs / 60, secs % 60))),
                )
                .child(
                    tools
                        .child(button("rec-send", G_CHECK, "send", t, true).on_click(cx.listener(|this, _, _, cx| this.stop_recording(true, cx))))
                        .child(button("rec-cancel", G_TIMES, "", t, false).on_click(cx.listener(|this, _, _, cx| this.stop_recording(false, cx)))),
                )
                .into_any_element();
        }
        let conv = self.selected.clone().unwrap_or_default();
        field
            .border_color(if self.composer_focused { t.accent.opacity(0.6) } else { t.border })
            .child(div().flex_1().min_w(px(0.)).child(self.composer.clone()))
            .child(
                tools
                    .child(button("send-file", G_CLIP, "", t, false).on_click(cx.listener(move |this, _, _, cx| this.pick_and_send(conv.clone(), cx))))
                    .child(button("emoji", G_SMILE, "", t, self.emoji_open).on_click(cx.listener(|this, _, _, cx| {
                        this.emoji_open = !this.emoji_open;
                        this.gifs = None;
                        cx.notify();
                    })))
                    .child(button("gifs", G_IMAGE, "", t, self.gifs.is_some()).on_click(cx.listener(|this, _, _, cx| this.toggle_gifs(cx))))
                    // YouTube: the message box becomes the search box (`@youtube …`, then Enter).
                    .child(button("yt", G_YOUTUBE, "", t, false).on_click(cx.listener(|this, _, _, cx| {
                        this.emoji_open = false;
                        this.composer.update(cx, |c, cx| c.set_text("@youtube ", cx));
                        this.focus_composer = true;
                        cx.notify();
                    })))
                    .child(button("rec-voice", G_MIC, "", t, false).on_click(cx.listener(|this, _, _, cx| this.start_recording(false, cx))))
                    .child(button("rec-video", G_VIDEO, "", t, false).on_click(cx.listener(|this, _, _, cx| this.start_recording(true, cx)))),
            )
            .into_any_element()
    }

    pub(super) fn emoji_popover(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
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
                    .rounded(px(radius()))
                    .cursor_pointer()
                    .hover(|d| d.bg(t.raised))
                    .child(e2)
                    .on_click(cx.listener(move |this, _, _, cx| this.insert_emoji(e2, cx))),
            );
        }
        Some(div().mx(px(12.)).mt(px(6.)).p(px(6.)).border_1().border_color(t.border).rounded(px(radius())).bg(t.panel).max_h(px(200.)).child(div().id("emoji-scroll").overflow_y_scroll().max_h(px(188.)).child(grid)).into_any_element())
    }

    /// The GIF picker: the GIFs of the user's folder, filtered by what is typed in the
    /// message field; a click sends one.
    pub(super) fn gif_popover(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let all = self.gifs.as_ref()?;
        let filter = self.composer.read(cx).text().trim().to_lowercase();
        let shown: Vec<&std::path::PathBuf> = all.iter().filter(|p| filter.is_empty() || p.file_stem().is_some_and(|n| n.to_string_lossy().to_lowercase().contains(&filter))).take(36).collect();
        let frame = div().flex_none().mx(px(S3)).mt(px(6.)).p(px(6.)).border_1().border_color(t.border).rounded(px(radius())).bg(t.panel);
        if shown.is_empty() {
            let dir = self.gif_dir();
            let why = if all.is_empty() { format!("no GIFs yet: put some in {} (or drop one on the conversation)", dir.display()) } else { format!("no GIF named “{filter}”") };
            return Some(frame.text_size(px(t.size - 2.)).text_color(t.muted).child(why).into_any_element());
        }
        let mut grid = div().flex().flex_wrap().gap(px(S1));
        for (i, p) in shown.into_iter().enumerate() {
            let path = p.clone();
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("gif-{i}")))
                    .w(px(104.))
                    .h(px(78.))
                    .rounded(px(radius()))
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(|d| d.opacity(0.8))
                    .child(gpui::img(p.clone()).w(px(104.)).h(px(78.)).object_fit(ObjectFit::Cover))
                    .on_click(cx.listener(move |this, _, _, cx| this.send_gif(&path, cx))),
            );
        }
        Some(
            frame
                .child(div().pb(px(S1)).text_size(px(t.size - 2.5)).text_color(t.muted).child("your GIFs · type in the message field to filter by name · a click sends one"))
                .child(div().id("gif-scroll").max_h(px(250.)).overflow_y_scroll().child(grid))
                .into_any_element(),
        )
    }

    /// A YouTube link's card: thumbnail, title and channel; a click opens it.
    pub(super) fn youtube_card(&self, t: &Theme, id: &str, msg_id: &str, cx: &mut Context<Self>) -> AnyElement {
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
            .border_color(t.border).rounded(px(radius()))
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
    pub(super) fn attachment_strip(&self, t: &Theme, conv: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
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
                .border_color(t.border).rounded(px(radius()))
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

    /// Watching together in the open conversation: the invitation, or the session with
    /// its queue and the proposals to vote on.
    pub(super) fn watch_panels(&self, t: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
    pub(super) fn youtube_results(&self, t: &Theme, msg_id: &str, query: &str, results: Vec<(String, String)>, cx: &mut Context<Self>) -> AnyElement {
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
                    .border_color(t.border).rounded(px(radius()))
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

    /// The conversation as bubbles: ours on the right on the accent's tint, the others'
    /// on the left; what happened (joined, a call) as a quiet centred line.
    pub(super) fn messages(&self, t: &Theme, conv: &Conversation, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().id("messages").flex().flex_col().flex_1().min_h(px(0.)).overflow_y_scroll().track_scroll(&self.messages_scroll).px(px(14.)).py(px(S2));
        let group = conv.peers.len() > 1;
        let mut last_day = None;
        let mut last_author = String::new();
        for m in &conv.messages {
            let (hm, day, day_label) = local_time(m.ts);
            if last_day != Some(day) {
                last_day = Some(day);
                last_author.clear();
                list = list.child(div().flex().justify_center().pt(px(S3)).pb(px(S1)).child(caption(t, &day_label)));
            }
            let me = crate::model::bare_uri(&m.author) == self.model.me;
            let who = if me { "you".to_owned() } else { self.model.name_of(&m.author) };
            let color = if me { t.accent } else { author_color(t, &m.author) };
            let file = m.file.clone();
            let bubbled = m.kind == MsgKind::Text || file.is_some();
            let repeat = bubbled && last_author == m.author;
            // A system line breaks a run, so the next bubble names its author again.
            last_author = if bubbled { m.author.clone() } else { String::new() };
            if !bubbled {
                list = list.child(div().flex().justify_center().py(px(3.)).text_size(px(t.size - 2.)).text_color(t.muted).child(format!("{who} · {} · {hm}", m.text)));
                continue;
            }
            // GPUI measures text at its full width before the limit applies, so a bubble
            // whose text will wrap gets the limit as its width (the font is monospace:
            // the estimate holds).
            let cells = m.text.lines().map(|l| l.chars().map(|c| if c.is_ascii() { 1. } else { 1.8 }).sum::<f32>()).fold(0., f32::max);
            let inner = self.bubble_max - 22.;
            let wraps = cells * t.size * 0.6 > inner - 8.;
            let (id_click, id_right) = (m.id.clone(), m.id.clone());
            let is_text = file.is_none();
            let bubble = div()
                .id(SharedString::from(format!("bubble-{}", m.id)))
                .flex()
                .flex_col()
                .flex_none()
                .max_w(px(self.bubble_max))
                // A click on a text (a right click on anything) shows what can be done
                // with it: reply, react, edit, delete.
                .when(is_text && !m.deleted, |d| d.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| this.toggle_msg_actions(&id_click, cx))))
                .on_mouse_down(MouseButton::Right, cx.listener(move |this, _, _, cx| this.toggle_msg_actions(&id_right, cx)))
                // What it answers, quoted.
                .when_some(m.reply_to.clone(), |d, parent| {
                    let quoted = conv.messages.iter().find(|o| o.id == parent);
                    let (whom, what) = match quoted {
                        Some(o) => (
                            if crate::model::bare_uri(&o.author) == self.model.me { "you".to_owned() } else { self.model.name_of(&o.author) },
                            if o.deleted { "message deleted".to_owned() } else { o.text.lines().next().unwrap_or("").chars().take(80).collect() },
                        ),
                        None => (String::new(), "an earlier message".to_owned()),
                    };
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .mb(px(S1))
                            .pl(px(S2))
                            .border_l_2()
                            .border_color(t.accent.opacity(0.6))
                            .text_size(px(t.size - 2.))
                            .when(!whom.is_empty(), |d| d.child(div().text_color(t.accent).child(whom)))
                            .child(div().max_w(px(self.bubble_max - 40.)).truncate().text_color(t.muted).child(what)),
                    )
                })
                .when(wraps, |d| d.w(px(self.bubble_max)))
                .px(px(10.))
                .py(px(5.))
                .rounded(px(radius()))
                .border_1()
                .map(|d| if me { d.bg(t.accent_bg).border_color(t.accent.opacity(0.3)) } else { d.bg(t.raised).border_color(t.raised) })
                .when(group && !me && !repeat, |d| d.child(div().text_size(px(t.size - 2.)).font_weight(FontWeight::BOLD).text_color(color).child(who.clone())));
            let bubble = match file {
                None => {
                    let results = crate::watch::parse_results(&m.text);
                    let yt = if results.is_some() { None } else { crate::media::youtube_id(&m.text) };
                    match results {
                        Some((query, found)) => bubble.child(self.youtube_results(t, &m.id, &query, found, cx)),
                        None if m.deleted => bubble.child(div().text_color(t.muted).italic().child("message deleted")),
                        None => bubble.child(div().text_color(t.bright).when(wraps, |d| d.w(px(inner))).child(m.text.clone())).when_some(yt, |d, id| d.child(self.youtube_card(t, &id, &m.id, cx))),
                    }
                }
                // A file: click to open it, or to download one too big to come by itself.
                Some(file) => {
                    let saved = self.saved_file(&conv.id, &file.id);
                    let file_id = file.id.clone();
                    let (file_name, file_ref) = (file.name.clone(), file.clone());
                    let have = saved.is_some();
                    let hint = match (have, me, self.downloads.contains_key(&file.id)) {
                        (true, _, _) => " · open".to_owned(),
                        (false, true, _) => String::new(),
                        (false, false, true) => " · downloading".to_owned(),
                        (false, false, false) => format!(" · {} · download", size_label(file.size)),
                    };
                    let (conv_id, msg_id) = (conv.id.clone(), m.id.clone());
                    bubble.child(
                        div()
                            .id(SharedString::from(format!("file-{}", m.id)))
                            .flex()
                            .flex_col()
                            .text_color(t.text)
                            .when(!me || have, |d| {
                                d.cursor_pointer().hover(|d| d.text_color(t.accent)).on_click(cx.listener(move |this, _, _, cx| {
                                    this.file_clicked(&conv_id, &msg_id, &file, cx);
                                    cx.notify();
                                }))
                            })
                            .child(div().flex().items_center().gap(px(6.)).child(div().text_color(t.muted).child(G_CLIP)).child(div().min_w(px(0.)).child(format!("{}{hint}", m.text))))
                            // A voice message not here yet: the button fetches it and plays it.
                            .when(saved.is_none() && crate::media::is_audio(std::path::Path::new(&file_name)), |d| {
                                let busy = self.downloads.contains_key(&file_id);
                                let (conv_id, msg_id, file) = (conv.id.clone(), m.id.clone(), file_ref.clone());
                                d.child(div().mt(px(S1)).flex().child(
                                    button(SharedString::from(format!("play-{}", m.id)), G_PLAY, if busy { "voice message arriving…" } else { "play voice message" }, t, busy).on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.play_after = Some(file.id.clone());
                                        this.file_clicked(&conv_id, &msg_id, &file, cx);
                                        cx.notify();
                                    })),
                                ))
                            })
                            // A voice or video message plays with mpv.
                            .when_some(saved.clone().filter(|p| crate::media::is_audio(p) || crate::media::is_video(p)), |d, path| {
                                let fid = file_id.clone();
                                let playing = self.playing.as_ref().is_some_and(|(id, _)| *id == file_id);
                                let voice = crate::media::is_audio(&path) || path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("voice"));
                                let (tid, tpath) = (file_id.clone(), path.clone());
                                let transcript = self.transcripts.get(&file_id).cloned();
                                let can = voice && crate::media::is_audio(&path);
                                d.child(
                                    div()
                                        .mt(px(S1))
                                        .flex()
                                        .gap(px(S1))
                                        .child(
                                            button(SharedString::from(format!("play-{}", m.id)), if playing { G_STOP } else { G_PLAY }, if playing { "stop" } else if voice { "play voice message" } else { "play video" }, t, playing)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.toggle_play(&fid, &path, cx);
                                                })),
                                        )
                                        // The words of a voice message, made on this machine.
                                        .when(can && !matches!(transcript, Some(Some(Ok(_)))), |d| {
                                            let busy = matches!(transcript, Some(None));
                                            d.child(button(SharedString::from(format!("tr-{}", m.id)), G_COMMENT, if busy { "transcribing…" } else { "transcribe" }, t, busy).on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.transcribe(&tid, &tpath, cx);
                                            })))
                                        }),
                                )
                                .when_some(transcript.flatten(), |d, r| {
                                    d.child(match r {
                                        Ok(text) => div().mt(px(S1)).w(px(inner.min(420.))).text_color(t.bright).child(text),
                                        Err(e) => div().mt(px(S1)).w(px(inner.min(420.))).text_size(px(t.size - 2.)).text_color(t.red).child(format!("not transcribed: {e}")),
                                    })
                                })
                            })
                            // A picture shows itself once it is here.
                            .when_some(saved.filter(|p| is_picture(p)), |d, path| {
                                // Sized from the file's header, so the list does not jump
                                // when the picture loads.
                                let (w, h) = picture_box(&path, inner.min(360.), 260.);
                                d.child(div().mt(px(S1)).w(px(w)).h(px(h)).rounded(px(radius())).overflow_hidden().child(gpui::img(path).w(px(w)).h(px(h)).object_fit(ObjectFit::Contain)))
                            }),
                    )
                }
            };
            let bubble = bubble.child(div().flex().justify_end().text_size(px(t.size - 3.5)).text_color(t.muted).child(if m.edited { format!("edited · {hm}") } else { hm }));
            // Reactions under it, one chip per emoji; ours in the accent, a click toggles ours.
            let mut chips: Vec<(String, usize, bool)> = Vec::new();
            for r in &m.reactions {
                let ours = crate::model::bare_uri(&r.author) == self.model.me;
                match chips.iter_mut().find(|(e, _, _)| *e == r.emoji) {
                    Some(c) => {
                        c.1 += 1;
                        c.2 |= ours;
                    }
                    None => chips.push((r.emoji.clone(), 1, ours)),
                }
            }
            let reactions = (!chips.is_empty()).then(|| {
                let mut row = div().flex().flex_wrap().gap(px(S1));
                for (i, (emoji, n, ours)) in chips.into_iter().enumerate() {
                    let (mid, e) = (m.id.clone(), emoji.clone());
                    row = row.child(
                        div()
                            .id(SharedString::from(format!("react-{}-{i}", m.id)))
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .h(px(20.))
                            .px(px(6.))
                            .rounded(px(radius()))
                            .border_1()
                            .border_color(if ours { t.accent.opacity(0.5) } else { t.border })
                            .when(ours, |d| d.bg(t.accent_bg))
                            .text_size(px(t.size - 2.))
                            .cursor_pointer()
                            .hover(|d| d.bg(t.raised))
                            .child(emoji)
                            .when(n > 1, |d| d.child(div().text_color(t.muted).child(n.to_string())))
                            .on_click(cx.listener(move |this, _, _, cx| this.react(&mid, &e, cx))),
                    );
                }
                row
            });
            let actions = (self.msg_actions.as_deref() == Some(m.id.as_str())).then(|| {
                let mut row = div().flex().flex_wrap().items_center().gap(px(2.)).p(px(2.)).bg(t.panel).border_1().border_color(t.border).rounded(px(radius()));
                let mid = m.id.clone();
                row = row.child(button(SharedString::from(format!("act-reply-{}", m.id)), G_REPLY, "reply", t, false).on_click(cx.listener(move |this, _, _, cx| this.reply_to(&mid, cx))));
                for (i, e) in ["👍", "❤️", "😂", "😮", "😢", "🙏"].into_iter().enumerate() {
                    let mid = m.id.clone();
                    row = row.child(
                        div()
                            .id(SharedString::from(format!("act-e{i}-{}", m.id)))
                            .size(px(t.size + 13.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(radius()))
                            .cursor_pointer()
                            .hover(|d| d.bg(t.raised))
                            .child(e)
                            .on_click(cx.listener(move |this, _, _, cx| this.react(&mid, e, cx))),
                    );
                }
                if me && is_text && !m.deleted {
                    let (m1, m2) = (m.id.clone(), m.id.clone());
                    let armed = self.is_armed(&format!("msg:{}", m.id));
                    row = row
                        .child(button(SharedString::from(format!("act-edit-{}", m.id)), G_PENCIL, "edit", t, false).on_click(cx.listener(move |this, _, _, cx| this.edit_message(&m1, cx))))
                        .child(button(SharedString::from(format!("act-del-{}", m.id)), G_TRASH, if armed { "sure?" } else { "delete" }, t, armed).on_click(cx.listener(move |this, _, _, cx| this.delete_message(&m2, cx))));
                }
                row
            });
            let col = div().flex().flex_col().gap(px(3.)).when(me, |d| d.items_end()).child(bubble).children(reactions).children(actions);
            list = list.child(div().flex().flex_none().w_full().mt(px(if repeat { 3. } else { S2 })).when(me, |d| d.justify_end()).child(col));
        }
        if conv.composing {
            list = list.child(div().pt(px(S1)).text_size(px(t.size - 2.)).text_color(t.muted).child("typing…"));
        }
        list.into_any_element()
    }

    pub(super) fn chat(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let conv = self.selected.as_ref().and_then(|s| self.model.conversations.get(s)).cloned();
        let call_panel = self.call_panel(t, conv.as_ref(), cx);
        let direct = self.direct_panels(t, cx);
        let watch = self.watch_panels(t, cx);
        let Some(conv) = conv else {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
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
                        .child("No conversation yet · + or Ctrl+N to add a contact"),
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
            .gap(px(10.))
            .h(px(46.))
            .pl(px(S3))
            .pr(px(S2))
            .border_b_1()
            .border_color(t.border)
            .when(self.narrow, |d| {
                d.child(button("back", G_BACK, "", t, false).on_click(cx.listener(|this, _, _, cx| {
                    this.narrow_chat = false;
                    cx.notify();
                })))
            })
            .child({
                let peer = (conv.peers.len() == 1).then(|| crate::model::bare_uri(&conv.peers[0]).to_owned());
                let picture = peer.as_ref().and_then(|p| self.model.avatars.get(p).cloned());
                let key = peer.unwrap_or_else(|| conv.id.clone());
                avatar(t, picture, &key, &conv.title, 28., None)
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .child(div().flex().items_center().gap(px(6.)).child(div().truncate().text_color(t.bright).font_weight(FontWeight::BOLD).child(conv.title.clone())).when(fav, |d| {
                        d.child(div().flex_none().text_size(px(t.size - 3.)).text_color(t.yellow).child(G_STAR))
                    }))
                    .child(div().truncate().text_size(px(t.size - 2.)).text_color(t.muted).child({
                        let who = if peers == conv.title { peer_ids } else { format!("{peers} · {peer_ids}") };
                        status.unwrap_or(who)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .when(!has_call && !conv.request, |d| {
                        d.child(button("call-a", G_PHONE, "", t, false).text_color(t.text).on_click(cx.listener(|this, _, _, cx| this.place_call(false, cx))))
                            .child(button("call-v", G_VIDEO, "", t, false).text_color(t.text).on_click(cx.listener(|this, _, _, cx| this.place_call(true, cx))))
                            .child(button("share-d", G_DESKTOP, "", t, false).text_color(t.text).on_click(cx.listener(|this, _, _, cx| {
                                if let Some(conv) = this.selected.clone() {
                                    if let Reply::Error { message } = this.share_direct(&conv, cx) {
                                        this.show_flash(message, cx);
                                    }
                                }
                            })))
                            .child(div().w(px(1.)).h(px(16.)).mx(px(S1)).bg(t.border))
                    })
                    .child(button("chat-more", G_MORE, "", t, self.menu == Some(Menu::Chat)).on_click(cx.listener(|this, _, _, cx| this.toggle_menu(Menu::Chat, cx)))),
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
            let mut row = div().flex().flex_none().flex_wrap().gap(px(6.)).px(px(S3)).py(px(6.));
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
            .min_h(px(0.))
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
                        d.children(self.emoji_popover(t, cx)).children(self.gif_popover(t, cx))
                            .children(self.attachment_strip(t, &conv.id, cx))
                            .children(self.reply_strip(t, &conv, cx))
                            .child(self.composer_row(t, cx))
                    })
            })
            .into_any_element()
    }
}
