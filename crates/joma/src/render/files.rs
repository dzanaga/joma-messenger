//! The files view (discussion `2026-11-27-files`, mock-ups `joma-sync-mockups`): synced
//! folders on the left (offers, yours, shared with people, conversations, received), the
//! chosen folder on the right with its members, the path to each device, and its files.

use super::*;
use crate::app::{FileEntry, FilesKey};

fn small(t: &Theme, s: impl Into<SharedString>, c: Hsla) -> gpui::Div {
    div().text_size(px(t.size - 2.)).text_color(c).child(s.into())
}

fn head(t: &Theme, s: &str) -> gpui::Div {
    div().px(px(10.)).pt(px(10.)).pb(px(4.)).text_size(px(t.size - 2.5)).text_color(t.muted).child(s.to_uppercase())
}

impl Joma {
    /// The left column: what syncs, grouped.
    pub(super) fn files_list(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().id("files").flex().flex_col().flex_1().overflow_y_scroll();
        if self.sync.st.is_none() {
            if let Some(s) = &self.restored_sync {
                list = list.child(self.restored_card(t, s, cx));
            }
            return list
                .child(head(t, "synced folders"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .px(px(10.))
                        .child(small(t, if self.sync.starting { "starting Syncthing…" } else { "Folders sync with your devices and, when you share them, with people and conversations." }, t.muted))
                        .when_some(self.sync.error.clone(), |d, e| d.child(small(t, e, t.red)))
                        .when(!self.sync.starting, |d| d.child(div().flex().child(button("files-sync-on", G_EXCHANGE, "turn sync on", t, true).on_click(cx.listener(|this, _, _, cx| this.sync_switch(true, cx)))))),
                )
                .child(self.files_received_row(t, cx))
                .into_any_element();
        }
        let Some(r) = &self.sync_view else { return list.child(head(t, "synced folders")).child(div().px(px(10.)).child(small(t, "reading…", t.muted))).into_any_element() };
        if let Some(s) = &self.restored_sync {
            list = list.child(self.restored_card(t, s, cx));
        }
        if !r.offers.is_empty() {
            list = list.child(head(t, "offered to you"));
            for o in &r.offers {
                let (j, d) = (o.folder.clone(), o.folder.clone());
                let from = self.model.name_of(&o.owner);
                let what = match &o.conv {
                    Some(c) => format!("{from}'s files in {}", self.conv_title_of(c)),
                    None => format!("from {from}"),
                };
                list = list.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .mx(px(10.))
                        .my(px(3.))
                        .p(px(8.))
                        .bg(t.raised)
                        .child(div().flex().items_center().gap(px(6.)).text_color(t.bright).child(G_FOLDER).child(div().truncate().child(o.label.clone())))
                        .child(small(t, format!("{what} · you read it, {from} writes"), t.muted))
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(button(SharedString::from(format!("offer-join-{}", o.folder)), G_CHECK, "join", t, true).on_click(cx.listener(move |this, _, _, cx| this.join_share_ui(&j, cx))))
                                .child(button(SharedString::from(format!("offer-no-{}", o.folder)), G_TIMES, "not now", t, false).on_click(cx.listener(move |this, _, _, cx| this.decline_share(&d, cx)))),
                        ),
                );
            }
        }
        let shares: std::collections::HashMap<&str, &crate::shares::Share> = r.shares.iter().map(|s| (s.id.as_str(), s)).collect();
        let row = |key: FilesKey, label: String, sub: String, warn: bool| {
            let on = self.files.key.as_ref() == Some(&key);
            let id = SharedString::from(format!("files-{}", key.id()));
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(5.))
                .cursor_pointer()
                .when(on, |d| d.bg(t.sel))
                .hover(|d| d.bg(t.sel))
                .child(div().flex_none().text_color(if on { t.bright } else { t.muted }).child(G_FOLDER))
                .child(div().flex().flex_col().flex_1().min_w(px(0.)).child(div().truncate().text_color(t.bright).child(label)).child(small(t, sub, if warn { t.yellow } else { t.muted }).truncate()))
                .on_click(cx.listener(move |this, _, _, cx| this.files_open(key.clone(), cx)))
        };
        let state = |f: &joma_sync::FolderStatus| {
            let need = if f.need_files > 0 { format!(" · ↓ {} files, {}", f.need_files, crate::sync::size(f.need_bytes)) } else { String::new() };
            (format!("{} · {} files, {}{need}", f.state, f.files, crate::sync::size(f.bytes)), !need.is_empty() || !f.error.is_empty())
        };
        // Yours: on your devices only.
        let own: Vec<&joma_sync::FolderStatus> = r.folders.iter().filter(|f| !shares.contains_key(f.id.as_str())).collect();
        let people: Vec<&joma_sync::FolderStatus> = r.folders.iter().filter(|f| shares.get(f.id.as_str()).is_some_and(|s| s.conv.is_none())).collect();
        list = list.child(head(t, "your devices"));
        if own.is_empty() {
            list = list.child(div().px(px(10.)).child(small(t, "no folder yet", t.muted)));
        }
        for f in own {
            let (sub, warn) = state(f);
            list = list.child(row(FilesKey::Folder(f.id.clone()), f.label.clone(), sub, warn));
        }
        list = list.child(div().flex().px(px(10.)).py(px(4.)).child(button("sync-add", G_PLUS, "add folder", t, false).on_click(cx.listener(|this, _, _, cx| this.choose_sync_folder(cx)))));
        if !people.is_empty() {
            list = list.child(head(t, "shared with people"));
            for f in people {
                let s = shares[f.id.as_str()];
                let (sub, warn) = state(f);
                let who: Vec<String> = s.others(&self.model.me).map(|u| self.model.name_of(u)).collect();
                list = list.child(row(FilesKey::Folder(f.id.clone()), f.label.clone(), format!("{} · {sub}", who.join(", ")), warn));
            }
        }
        // Conversations: one row each, its members' folders inside.
        let mut convs: Vec<(String, usize)> = Vec::new();
        for s in r.shares.iter().filter(|s| s.conv.is_some()) {
            let c = s.conv.clone().unwrap_or_default();
            match convs.iter_mut().find(|(k, _)| *k == c) {
                Some((_, n)) => *n += 1,
                None => convs.push((c, 1)),
            }
        }
        if !convs.is_empty() {
            list = list.child(head(t, "conversations"));
            for (c, n) in convs {
                let title = self.conv_title_of(&c);
                list = list.child(row(FilesKey::Conversation(c), title, format!("{n} {}", if n == 1 { "member's files" } else { "members' files" }), false));
            }
        }
        list.child(self.files_received_row(t, cx)).into_any_element()
    }

    /// After a restore: the folders the backup kept, to set up again.
    fn restored_card(&self, t: &Theme, s: &crate::sync::Saved, cx: &mut Context<Self>) -> gpui::Div {
        let names: Vec<&str> = s.folders.iter().map(|f| f.label.as_str()).collect();
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .mx(px(10.))
            .my(px(6.))
            .p(px(8.))
            .bg(t.raised)
            .child(div().text_color(t.bright).child("From your backup"))
            .child(small(t, format!("{} synced folders: {}", names.len(), names.join(", ")), t.muted))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(button("restored-yes", G_CHECK, "set them up again", t, true).on_click(cx.listener(|this, _, _, cx| this.restore_sync(true, cx))))
                    .child(button("restored-no", G_TIMES, "not now", t, false).on_click(cx.listener(|this, _, _, cx| this.restore_sync(false, cx)))),
            )
    }

    fn files_received_row(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let on = self.files.key == Some(FilesKey::Received);
        div().child(head(t, "received")).child(
            div()
                .id("files-received")
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .py(px(5.))
                .cursor_pointer()
                .when(on, |d| d.bg(t.sel))
                .hover(|d| d.bg(t.sel))
                .child(div().text_color(t.muted).child(G_DOWNLOAD))
                .child(div().flex().flex_col().min_w(px(0.)).child(div().text_color(t.bright).child("From conversations")).child(small(t, crate::app::download_dir().display().to_string(), t.muted).truncate()))
                .on_click(cx.listener(|this, _, _, cx| this.files_open(FilesKey::Received, cx))),
        )
    }

    /// The right side: the chosen folder.
    pub(super) fn files_browser(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(key) = self.files.key.clone() else {
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .text_color(t.muted)
                .child(div().text_size(px(t.size + 6.)).child(G_FOLDER))
                .child("Pick a folder")
                .child(small(t, "Files dropped in a conversation also land in its folder, sorted into images, gifs, audio, video and documents.", t.muted))
                .into_any_element();
        };
        let r = self.sync_view.clone().unwrap_or_default();
        let me = self.model.me.clone();
        let (title, root, share, status) = match &key {
            FilesKey::Received => ("From conversations".to_owned(), crate::app::download_dir(), None, None),
            FilesKey::Folder(id) => {
                let f = r.folders.iter().find(|f| f.id == *id).cloned();
                let s = r.shares.iter().find(|s| s.id == *id).cloned();
                (f.as_ref().map(|f| f.label.clone()).unwrap_or_else(|| id.clone()), f.as_ref().map(|f| std::path::PathBuf::from(&f.path)).unwrap_or_default(), s, f)
            }
            FilesKey::Conversation(c) => {
                let mine = r.shares.iter().find(|s| s.conv.as_ref() == Some(c) && s.owner == me).or_else(|| r.shares.iter().find(|s| s.conv.as_ref() == Some(c)));
                let root = mine.map(|s| std::path::PathBuf::from(&s.path).parent().map(|p| p.to_path_buf()).unwrap_or_default()).unwrap_or_default();
                (self.conv_title_of(c), root, None, None)
            }
        };
        let dir = self.files.dir.clone().unwrap_or_else(|| root.clone());
        let readonly = status.as_ref().is_some_and(|f| f.kind == "receiveonly");
        let mut top = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .px(px(S3 + 4.))
            .py(px(8.))
            .border_b_1()
            .border_color(t.border)
            .when(self.narrow, |d| d.child(button("files-back", G_BACK, "", t, false).on_click(cx.listener(|this, _, _, cx| this.files_close(cx)))))
            .child(div().flex().flex_col().flex_1().min_w(px(0.)).child(div().truncate().text_color(t.bright).font_weight(FontWeight::BOLD).child(title)).child(small(t, dir.display().to_string(), t.muted).truncate()));
        if dir != root && dir.starts_with(&root) {
            top = top.child(button("files-up", G_UNDO, "up", t, false).on_click(cx.listener(|this, _, _, cx| this.files_up(cx))));
        }
        let d2 = dir.clone();
        top = top.child(button("files-manager", G_FOLDER, "open in file manager", t, false).on_click(cx.listener(move |this, _, _, cx| this.open_outside_checked(&d2, cx))));
        if let FilesKey::Folder(id) = &key {
            let can_share = share.as_ref().is_none_or(|s| s.owner == me);
            if can_share {
                let id2 = id.clone();
                top = top.child(button("files-share", G_USER_PLUS, "share…", t, false).on_click(cx.listener(move |this, _, window, cx| this.ask_share_with(&id2, window, cx))));
            }
            if let Some(s) = &share {
                let id3 = s.id.clone();
                if s.owner != me {
                    top = top.child(button("files-leave", G_TIMES, if self.is_armed(&format!("leave:{id3}")) { "leave?" } else { "leave" }, t, false).on_click(cx.listener(move |this, _, _, cx| this.leave_share_ui(&id3, cx))));
                }
            }
            if readonly && status.as_ref().is_some_and(|f| f.local_changed > 0) {
                let id4 = id.clone();
                top = top.child(button("files-revert", G_UNDO, "undo my changes", t, false).on_click(cx.listener(move |this, _, _, cx| this.revert_folder(&id4, cx))));
            }
        }
        let mut body = div().id("files-body").flex().flex_col().flex_1().overflow_y_scroll();
        if let Some(f) = &status {
            if !f.error.is_empty() {
                body = body.child(div().px(px(S3 + 4.)).pt(px(6.)).child(small(t, f.error.clone(), t.red)));
            }
            if readonly {
                body = body.child(div().px(px(S3 + 4.)).pt(px(6.)).child(small(
                    t,
                    "You read this folder: what you change or delete stays on your side (\"undo my changes\" brings the shared files back).",
                    t.muted,
                )));
            }
        }
        body = body.child(self.files_members(t, &key, share.as_ref(), &r, cx));
        // The files.
        body = body.child(head(t, "files"));
        if self.files.entries.is_empty() {
            body = body.child(div().px(px(S3 + 4.)).child(small(t, if readonly { "empty" } else { "empty: drop files here" }, t.muted)));
        }
        for e in &self.files.entries {
            body = body.child(self.file_row(t, e, cx));
        }
        let accent_bg = t.accent_bg;
        div()
            .id("files-pane")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .child(top)
            .child(body)
            .when(!readonly && !matches!(key, FilesKey::Conversation(_)), |d| {
                d.drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(accent_bg)).on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| this.files_drop(paths.paths().to_vec(), cx)))
            })
            .into_any_element()
    }

    fn file_row(&self, t: &Theme, e: &FileEntry, cx: &mut Context<Self>) -> AnyElement {
        let on = self.files.picked.as_ref() == Some(&e.path);
        let p = e.path.clone();
        let dir = e.dir;
        let when = if e.modified > 0 { crate::render::when(e.modified) } else { String::new() };
        let mut row = div()
            .id(SharedString::from(format!("file-{}", e.path.display())))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(S3 + 4.))
            .py(px(4.))
            .cursor_pointer()
            .when(on, |d| d.bg(t.sel))
            .hover(|d| d.bg(t.sel))
            .child(div().flex_none().text_color(if dir { t.accent } else { t.muted }).child(if dir { G_FOLDER } else { G_FILE }))
            .child(div().flex_1().min_w(px(0.)).truncate().text_color(t.bright).child(e.name.clone()))
            .child(div().flex_none().text_size(px(t.size - 2.)).text_color(t.muted).child(if dir { String::new() } else { crate::sync::size(e.size) }))
            .child(div().flex_none().w(px(90.)).text_size(px(t.size - 2.)).text_color(t.muted).child(when))
            .on_mouse_down(MouseButton::Left, cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                if dir {
                    this.files_enter(&p, cx);
                } else if ev.click_count == 2 {
                    this.files_activate(&p, cx);
                } else {
                    this.files.picked = Some(p.clone());
                    cx.notify();
                }
            }));
        let mut col = None;
        if !e.conflicts.is_empty() {
            let mut warn = div().flex().flex_col().gap(px(2.)).px(px(S3 + 30.)).pb(px(4.));
            for (i, copy) in e.conflicts.iter().enumerate() {
                let (f1, c1, f2, c2) = (e.path.clone(), copy.clone(), e.path.clone(), copy.clone());
                let from = copy.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                warn = warn.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(6.))
                        .child(small(t, format!("conflict: another device changed it too ({from})"), t.yellow))
                        .child(button(SharedString::from(format!("cf-this-{}-{i}", e.path.display())), G_CHECK, "keep this", t, false).on_click(cx.listener(move |this, _, _, cx| this.resolve_conflict(&f1, &c1, false, cx))))
                        .child(button(SharedString::from(format!("cf-other-{}-{i}", e.path.display())), G_EXCHANGE, "keep the other", t, false).on_click(cx.listener(move |this, _, _, cx| this.resolve_conflict(&f2, &c2, true, cx)))),
                );
            }
            col = Some(warn);
        }
        if on && !dir {
            let (a, b, c) = (e.path.clone(), e.path.clone(), e.path.clone());
            let d = e.path.clone();
            let armed = self.is_armed(&format!("del:{}", e.path.display()));
            row = row
                .child(button("file-open", G_EXPAND, "open", t, false).on_click(cx.listener(move |this, _, _, cx| this.files_activate(&a, cx))))
                .child(button("file-show", G_FOLDER, "show in folder", t, false).on_click(cx.listener(move |this, _, _, cx| this.open_outside_checked(b.parent().unwrap_or(&b), cx))))
                .child(button("file-copy", G_COPY, "copy path", t, false).on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(c.display().to_string()));
                    this.show_flash("path copied", cx);
                })))
                .child(button("file-del", G_TRASH, if armed { "delete?" } else { "" }, t, armed).text_color(if armed { t.red } else { t.muted }).on_click(cx.listener(move |this, _, _, cx| this.files_delete(&d, cx))));
        }
        match col {
            Some(w) => div().flex().flex_col().child(row).child(w).into_any_element(),
            None => row.into_any_element(),
        }
    }

    /// Who has the folder: the owner and members (reading or writing), your devices, and
    /// the path to each device that syncs it.
    fn files_members(&self, t: &Theme, key: &FilesKey, share: Option<&crate::shares::Share>, r: &crate::sync::Report, cx: &mut Context<Self>) -> gpui::Div {
        let me = self.model.me.clone();
        let mut g = div().flex().flex_col().child(head(t, "members"));
        let line = |name: String, what: String, c: Hsla| div().flex().items_center().gap(px(8.)).px(px(S3 + 4.)).py(px(3.)).child(div().flex_1().min_w(px(0.)).truncate().text_color(t.text).child(name)).child(small(t, what, c));
        // Devices that sync it, with the path (relayed: slow for large files).
        let path_of = |d: &crate::sync::DeviceLine| match d.path {
            Some(p) if d.relayed => (format!("{} · slow for large files", p.label()), t.yellow),
            Some(p) => (p.label().to_string(), t.green),
            None if d.connected => ("connected".to_string(), t.green),
            None => ("not connected now".to_string(), t.muted),
        };
        match (key, share) {
            (FilesKey::Received, _) => return g.child(line("this device".into(), "files you received in conversations".into(), t.muted)),
            (FilesKey::Conversation(c), _) => {
                for s in r.shares.iter().filter(|s| s.conv.as_ref() == Some(c)) {
                    let k = FilesKey::Folder(s.id.clone());
                    let who = if s.owner == me { "you".to_owned() } else { self.model.name_of(&s.owner) };
                    g = g.child(
                        div()
                            .id(SharedString::from(format!("conv-share-{}", s.id)))
                            .cursor_pointer()
                            .hover(|d| d.bg(t.sel))
                            .child(line(format!("{who}: {}", s.label), if s.owner == me { "you write".into() } else if s.can_write(&me) { "you write too".into() } else { "you read".into() }, t.muted))
                            .on_click(cx.listener(move |this, _, _, cx| this.files_open(k.clone(), cx))),
                    );
                }
                return g;
            }
            _ => {}
        }
        let Some(s) = share else {
            // A folder of your own devices.
            for d in r.devices.iter().filter(|d| d.uri.is_empty()) {
                let (p, c) = path_of(d);
                g = g.child(line(if d.name.is_empty() { d.device[..d.device.len().min(8)].to_owned() } else { d.name.clone() }, p, c));
            }
            if r.devices.iter().all(|d| !d.uri.is_empty()) {
                g = g.child(line("your other devices".into(), "none heard yet: start Joma with sync on there".into(), t.muted));
            }
            return g;
        };
        let admin = s.is_admin(&me);
        let owner = if s.owner == me { "you".to_owned() } else { self.model.name_of(&s.owner) };
        g = g.child(line(owner, "owner · writes".into(), t.muted));
        if admin && s.members.len() > 1 {
            let all = s.members.iter().all(|m| m.write);
            let f = s.id.clone();
            g = g.child(
                line("everybody".into(), String::new(), t.muted)
                    .child(button("member-w-all", if all { G_PENCIL } else { G_EYE }, if all { "everybody writes" } else { "set everybody to write" }, t, all).on_click(cx.listener(move |this, _, _, cx| this.set_member_write(&f, "all", !all, cx)))),
            );
        }
        for m in &s.members {
            let name = if m.uri == me { "you".to_owned() } else { self.model.name_of(&m.uri) };
            let mut row = line(name, String::new(), t.muted);
            if admin && m.uri != me {
                let (f, u, w) = (s.id.clone(), m.uri.clone(), !m.write);
                row = row.child(button(SharedString::from(format!("member-w-{}", m.uri)), if m.write { G_PENCIL } else { G_EYE }, if m.write { "writes" } else { "reads" }, t, m.write).on_click(cx.listener(move |this, _, _, cx| this.set_member_write(&f, &u, w, cx))));
                if s.owner == me {
                    let (f, u, a) = (s.id.clone(), m.uri.clone(), !m.admin);
                    row = row.child(button(SharedString::from(format!("member-a-{}", m.uri)), G_USERS, if m.admin { "admin" } else { "member" }, t, m.admin).on_click(cx.listener(move |this, _, _, cx| this.set_member_admin(&f, &u, a, cx))));
                }
            } else {
                row = row.child(small(t, if m.write { "writes" } else { "reads" }, t.muted));
            }
            g = g.child(row);
        }
        // Their devices and ours, and how each is reached.
        let devices: Vec<&crate::sync::DeviceLine> = r.devices.iter().filter(|d| d.uri.is_empty() || s.has(&d.uri)).collect();
        if !devices.is_empty() {
            g = g.child(head(t, "devices"));
        }
        for d in devices {
            let (p, c) = path_of(d);
            let name = if d.uri.is_empty() { format!("your {}", if d.name.is_empty() { "device" } else { &d.name }) } else { format!("{} · {}", self.model.name_of(&d.uri), d.name) };
            let mut row = line(name, p, c);
            if d.uri.is_empty() {
                let on = !s.off.contains(&d.syncthing);
                let (f, st) = (s.id.clone(), d.syncthing.clone());
                row = row.child(button(SharedString::from(format!("dev-on-{}", d.device)), G_CHECK, if on { "on" } else { "off" }, t, on).on_click(cx.listener(move |this, _, _, cx| this.set_folder_device(&f, &st, !on, cx))));
            }
            g = g.child(row);
        }
        g
    }
}
