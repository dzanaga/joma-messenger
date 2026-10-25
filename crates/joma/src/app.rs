//! The Joma window: state, daemon events and user actions. Rendering is in `render.rs`.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use futures::channel::mpsc::UnboundedSender;
use futures::StreamExt;
use gpui::{actions, App, AppContext, Context, Entity, FocusHandle, Focusable, RenderImage, ScrollHandle, Window};
use joma_desk::portal::PortalShare;
use joma_desk::share::ShareEvent;
use joma_desk::signal::{DeskMsg, MIME};
use joma_jami::Event;
use joma_quick::ipc::{ChatLine, ChatState, QuickState, Reply, Request, Target};
use joma_quick::{Candidate, QuickConfig};

use crate::composer::{Composer, Edited, Submit};
use crate::desk::{After, Desk};
use crate::ipc::Pending;
use crate::model::{bare_uri, Call, CallState, Model};
use crate::theme::Theme;

actions!(
    joma,
    [
        Quit,
        NextConversation,
        PrevConversation,
        NewConversation,
        Search,
        Cancel,
        CallAudio,
        CallVideo,
        HangUp,
        AcceptCall,
        ToggleMute,
        ToggleCamera,
        ShareDesktop,
        ToggleQuick,
        ToggleFavorite,
        Quick1,
        Quick2,
        Quick3,
        Quick4,
        Quick5,
        Quick6,
        Quick7,
        Quick8,
        Quick9,
        FocusStage,
        AcceptRequest,
        DeclineRequest,
    ]
);

/// Everything that reaches the window from other threads.
pub enum AppMsg {
    Jami(Event),
    Share(String, ShareEvent),
    /// A viewer has a new picture.
    Repaint,
    Ipc(Pending),
    Portal(String, anyhow::Result<PortalShare>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Chat,
    /// The input adds a contact by name or ID.
    Add,
    /// The input filters conversations.
    Search,
}

/// Why the daemon is not running yet.
#[derive(Clone, PartialEq, Eq)]
pub enum Gate {
    /// The Qt client holds the same accounts.
    QtRunning,
    /// No account: create one.
    Onboarding,
    Ready,
    Failed(String),
}

/// A picture ready to paint, and the serial it came from.
pub struct Frame {
    pub image: Arc<RenderImage>,
    pub serial: u64,
    pub buf: Vec<u8>,
}

pub struct Joma {
    pub tx: UnboundedSender<AppMsg>,
    pub theme: Rc<Theme>,
    theme_stamp: Option<SystemTime>,
    pub gate: Gate,
    pub model: Model,
    pub selected: Option<String>,
    pub mode: Mode,
    /// The input's text in search mode.
    pub search: String,
    pub composer: Entity<Composer>,
    pub input: Entity<Composer>,
    pub quick: QuickConfig,
    pub quick_open: bool,
    pub desk: Desk,
    /// Remote video by sink id.
    pub frames: HashMap<String, Frame>,
    pub desk_frame: Option<Frame>,
    pub pointer: Option<Frame>,
    /// The call view fills the window (desktop share).
    pub stage: bool,
    pub flash: Option<(String, Instant)>,
    pub focus: FocusHandle,
    pub messages_scroll: ScrollHandle,
    pub pinned_bottom: bool,
    /// Move the keyboard focus to the composer at the next frame.
    pub focus_composer: bool,
    /// Add-contact lookups in flight: the typed name.
    adding: Option<String>,
    repaint_queued: Arc<AtomicBool>,
    composing_sent: Option<Instant>,
    pub quick_popup: Option<gpui::WindowHandle<crate::popup::QuickPopup>>,
    /// Peers whose presence we follow.
    subscribed: HashSet<String>,
    /// Calls placed to share the desktop: the share starts once they connect.
    share_on_connect: HashSet<String>,
}

/// Is the Jami Qt client running? It would share our accounts, and two daemons on the
/// same data corrupt it.
pub fn qt_client_running() -> bool {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
        .any(|c| c.trim() == "jami")
}

/// Is this a test profile (its own XDG data dir), where the Qt client cannot interfere?
fn own_profile() -> bool {
    std::env::var_os("JOMA_PROFILE").is_some()
}

fn notify_desktop(title: &str, body: &str) {
    // Test profiles stay off the user's real notification daemon.
    if own_profile() && std::env::var_os("JOMA_NOTIFY").is_none() {
        return;
    }
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "Joma", "-i", "internet-chat", title, body])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

impl Joma {
    pub fn new(tx: UnboundedSender<AppMsg>, rx: futures::channel::mpsc::UnboundedReceiver<AppMsg>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = Rc::new(Theme::current());
        let composer = cx.new(Composer::new);
        let input = cx.new(Composer::new);
        cx.subscribe(&composer, |this, _, _: &Submit, cx| this.send_composed(cx)).detach();
        cx.subscribe(&composer, |this, _, _: &Edited, cx| this.typing(cx)).detach();
        cx.subscribe_in(&input, window, |this, _, _: &Submit, window, cx| this.submit_input(window, cx)).detach();
        cx.subscribe(&input, |this, input, _: &Edited, cx| {
            this.search = input.read(cx).text();
            cx.notify();
        })
        .detach();

        // Events from the daemon, the share threads, the viewer and the socket.
        cx.spawn_in(window, async move |this, cx| {
            let mut rx = rx;
            while let Some(msg) = rx.next().await {
                if this.update_in(cx, |j, window, cx| j.handle(msg, window, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        // Video frames, the theme and the desk handshake's timeouts.
        cx.spawn_in(window, async move |this, cx| {
            let mut n = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                n += 1;
                let alive = this.update(cx, |j, cx| {
                    if j.model.calls.values().any(|c| !c.sinks.is_empty()) {
                        cx.notify();
                    }
                    if n % 15 == 0 {
                        j.tick(cx);
                    }
                    if n % 60 == 0 {
                        j.check_theme(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();

        let mut j = Joma {
            tx,
            theme,
            theme_stamp: joma_theme::stamp(),
            gate: Gate::Ready,
            model: Model::default(),
            selected: None,
            mode: Mode::Chat,
            search: String::new(),
            composer,
            input,
            quick: QuickConfig::load(),
            quick_open: false,
            desk: Desk::default(),
            frames: HashMap::new(),
            desk_frame: None,
            pointer: None,
            stage: false,
            flash: None,
            focus: cx.focus_handle(),
            messages_scroll: ScrollHandle::new(),
            pinned_bottom: true,
            focus_composer: false,
            adding: None,
            repaint_queued: Arc::new(AtomicBool::new(false)),
            composing_sent: None,
            quick_popup: None,
            subscribed: HashSet::new(),
            share_on_connect: HashSet::new(),
        };
        j.configure_inputs(cx);
        j.boot(cx);
        // Keys reach the actions only through a focused element.
        if j.gate == Gate::Onboarding {
            window.focus(&j.input.focus_handle(cx));
        } else if j.selected.is_some() {
            window.focus(&j.composer.focus_handle(cx));
        } else {
            window.focus(&j.focus);
        }
        j
    }

    /// Start the daemon, unless the Qt client holds the same data.
    pub fn boot(&mut self, cx: &mut Context<Self>) {
        if !own_profile() && qt_client_running() {
            self.gate = Gate::QtRunning;
            cx.notify();
            return;
        }
        static STARTED: AtomicBool = AtomicBool::new(false);
        if !STARTED.swap(true, Ordering::SeqCst) {
            let tx = self.tx.clone();
            // No NO_LOCAL_* flags: libjami keeps audio inputs in its video manager, which
            // NO_LOCAL_VIDEO leaves out, and then crashes when a call starts. Test profiles
            // silence audio through ALSA instead (scripts/test-profile.sh).
            let flags = if std::env::var_os("JOMA_JAMI_LOG").is_some() {
                joma_jami::flags::DEBUG | joma_jami::flags::CONSOLE_LOG
            } else {
                0
            };
            if let Err(e) = joma_jami::start(flags, move |ev| {
                let _ = tx.unbounded_send(AppMsg::Jami(ev));
            }) {
                self.gate = Gate::Failed(e.to_string());
                return;
            }
        }
        let accounts = joma_jami::accounts().unwrap_or_default();
        match accounts.first() {
            Some(a) => self.open_account(a.clone(), cx),
            None => {
                self.gate = Gate::Onboarding;
                self.configure_inputs(cx);
            }
        }
        cx.notify();
    }

    fn open_account(&mut self, account: String, cx: &mut Context<Self>) {
        self.gate = Gate::Ready;
        let details = joma_jami::account_details(&account).unwrap_or_default();
        let volatile = joma_jami::volatile_details(&account).unwrap_or_default();
        self.model = Model {
            me: details.get("Account.username").map(|u| bare_uri(u).to_owned()).unwrap_or_default(),
            my_name: volatile
                .get("Account.registeredName")
                .filter(|n| !n.is_empty())
                .or_else(|| details.get("Account.displayName"))
                .cloned()
                .unwrap_or_default(),
            registration: volatile.get("Account.registrationStatus").cloned().unwrap_or_default(),
            account,
            ..Default::default()
        };
        self.load_profiles();
        self.subscribed.clear();
        for c in joma_jami::contacts(&self.model.account).unwrap_or_default() {
            if let Some(uri) = c.get("id") {
                self.follow(uri);
            }
        }
        for id in joma_jami::conversations(&self.model.account).unwrap_or_default() {
            self.reload_conversation(&id, false);
        }
        for r in joma_jami::conversation_requests(&self.model.account).unwrap_or_default() {
            if let Some(id) = r.get("id") {
                self.model.add_request(id, &r);
                self.lookup(r.get("from").map(String::as_str).unwrap_or(""));
            }
        }
        self.selected = self.model.ordered().first().map(|c| c.id.clone());
        if let Some(id) = self.selected.clone() {
            self.select(id, cx);
        }
        self.configure_inputs(cx);
    }

    /// Display names from the vCards the daemon keeps per peer.
    fn load_profiles(&mut self) {
        let Some(data) = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
        else {
            return;
        };
        let dir = data.join("jami").join(&self.model.account).join("profiles");
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            let Some(uri) = base64_decode(stem).and_then(|b| String::from_utf8(b).ok()) else { continue };
            if let Some(name) = std::fs::read_to_string(&path).ok().and_then(|v| crate::model::vcard_name(&v)) {
                self.model.names.insert(bare_uri(&uri).to_owned(), name);
            }
        }
    }

    /// Follow a peer's presence (once).
    fn follow(&mut self, uri: &str) {
        let bare = bare_uri(uri).to_owned();
        if !bare.is_empty() && bare != self.model.me && self.subscribed.insert(bare.clone()) {
            let _ = joma_jami::subscribe_presence(&self.model.account, &bare);
        }
    }

    fn lookup(&mut self, uri: &str) {
        let bare = bare_uri(uri);
        if !bare.is_empty() && !self.model.names.contains_key(bare) {
            let _ = joma_jami::lookup_address(&self.model.account, bare);
        }
    }

    fn reload_conversation(&mut self, id: &str, request: bool) {
        let acc = self.model.account.clone();
        let infos = joma_jami::conversation_infos(&acc, id).unwrap_or_default();
        let members = joma_jami::conversation_members(&acc, id).unwrap_or_default();
        self.model.set_conversation(id, &infos, &members, request);
        let peers = self.model.conversations[id].peers.clone();
        for p in peers {
            self.lookup(&p);
            self.follow(&p);
        }
        if let Ok(req) = joma_jami::load_conversation(&acc, id, "", 30) {
            self.model.loading.insert(req, id.to_owned());
        }
    }

    pub fn configure_inputs(&mut self, cx: &mut Context<Self>) {
        let theme = self.theme.clone();
        let key = self.selected.clone().unwrap_or_default();
        let chat = self.gate == Gate::Ready && self.selected.as_ref().is_some_and(|s| !self.model.conversations.get(s).is_some_and(|c| c.request));
        self.composer.update(cx, |c, _| c.configure(key, theme.clone(), chat));
        let (key, placeholder) = match (self.gate.clone(), self.mode) {
            (Gate::Onboarding, _) => ("onboarding", "Your name (shown to your contacts)"),
            (_, Mode::Add) => ("add", "Jami username or 40-digit ID, then Enter"),
            (_, Mode::Search) => ("search", "Filter conversations"),
            _ => ("idle", ""),
        };
        self.input.update(cx, |c, _| {
            c.configure(key.into(), theme, true);
            c.set_placeholder(placeholder);
        });
    }

    pub fn show_flash(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.flash = Some((text.into(), Instant::now()));
        cx.notify();
    }

    fn check_theme(&mut self, cx: &mut Context<Self>) {
        let stamp = joma_theme::stamp();
        if stamp != self.theme_stamp {
            self.theme_stamp = stamp;
            self.theme = Rc::new(Theme::current());
            self.configure_inputs(cx);
            cx.notify();
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let acc = self.model.account.clone();
        for after in self.desk.tick(&acc) {
            self.after(after, cx);
        }
        // A call placed to share starts sharing once connected: at the peer's Joma hello,
        // or after three seconds without one (then Jami's own share).
        self.share_on_connect.retain(|id| self.model.calls.contains_key(id));
        let due: Vec<String> = self
            .share_on_connect
            .iter()
            .filter(|id| {
                self.model.calls.get(*id).is_some_and(|c| {
                    c.state == CallState::Current && (c.peer_joma || c.started.is_some_and(|t| t.elapsed() > Duration::from_secs(3)))
                })
            })
            .cloned()
            .collect();
        for id in due {
            self.share_on_connect.remove(&id);
            self.share_in(&id, cx);
        }
        if self.flash.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(5)) {
            self.flash = None;
        }
        cx.notify();
    }

    // ---- Messages from other threads ------------------------------------------

    fn handle(&mut self, msg: AppMsg, window: &mut Window, cx: &mut Context<Self>) {
        match msg {
            AppMsg::Jami(ev) => self.on_event(ev, window, cx),
            AppMsg::Share(call, ev) => {
                match ev {
                    ShareEvent::ViewerConnected { pipeline } => self.show_flash(format!("peer is watching · {pipeline} encoding"), cx),
                    ShareEvent::ViewerLeft => self.show_flash("peer stopped watching", cx),
                    ShareEvent::Error(e) => self.show_flash(format!("desktop share: {e}"), cx),
                }
                let _ = call;
            }
            AppMsg::Repaint => {
                self.repaint_queued.store(false, Ordering::SeqCst);
                cx.notify();
            }
            AppMsg::Ipc(p) => {
                let reply = self.ipc(p.request, window, cx);
                let _ = p.reply.send(reply);
            }
            AppMsg::Portal(call, result) => {
                let acc = self.model.account.clone();
                self.desk.portal_ready(&acc, &call, result);
                cx.notify();
            }
        }
    }

    fn on_event(&mut self, ev: Event, window: &mut Window, cx: &mut Context<Self>) {
        if self.gate == Gate::Onboarding {
            if let Event::AccountsChanged = ev {
                if let Some(a) = joma_jami::accounts().unwrap_or_default().first() {
                    self.open_account(a.clone(), cx);
                }
            }
            cx.notify();
            return;
        }
        let acc = self.model.account.clone();
        match &ev {
            Event::CallMessage { call, from, payloads, .. } => {
                if let Some(msg) = payloads.get(MIME).and_then(|p| DeskMsg::decode(p)) {
                    let peer = bare_uri(from).to_owned();
                    let (hello, after) = self.desk.message(&acc, call, &peer, msg);
                    if let Some((share, _view)) = hello {
                        if let Some(c) = self.model.calls.get_mut(call) {
                            c.peer_joma = true;
                            c.peer_can_share = share;
                            if !c.hello_sent {
                                c.hello_sent = true;
                                self.desk.hello(&acc, call);
                            }
                        }
                    }
                    self.after(after, cx);
                    cx.notify();
                }
                return;
            }
            Event::PeerServices { request, peer, services, .. } => {
                self.desk.peer_services(&acc, *request, peer, services);
                return;
            }
            Event::TunnelOpened { tunnel, port, .. } => {
                let tx = self.tx.clone();
                let queued = self.repaint_queued.clone();
                self.desk.tunnel_opened(tunnel, *port, move || {
                    if !queued.swap(true, Ordering::SeqCst) {
                        let _ = tx.unbounded_send(AppMsg::Repaint);
                    }
                });
                self.stage = true;
                cx.notify();
                return;
            }
            Event::TunnelClosed { tunnel, reason, .. } => {
                if self.desk.tunnel_closed(&acc, tunnel, reason).is_some() {
                    self.desk_frame = None;
                    self.show_flash(format!("desktop share closed ({reason})"), cx);
                }
                return;
            }
            Event::MediaChangeRequested { call, media, .. } => {
                // The peer adds or changes media (often a screen share): take it, sending
                // our own video only if the camera is on.
                let muted = self.model.calls.get(call).map(|c| c.muted_video).unwrap_or(true);
                let camera = camera_source();
                let answer: Vec<_> = media
                    .iter()
                    .map(|m| {
                        let mut a = m.clone();
                        if m.get("MEDIA_TYPE").map(String::as_str) == Some(joma_jami::VIDEO) {
                            a.insert("SOURCE".into(), camera.clone());
                            a.insert("MUTED".into(), muted.to_string());
                        }
                        a
                    })
                    .collect();
                let _ = joma_jami::answer_media_change(&acc, call, &answer);
                return;
            }
            _ => {}
        }
        let ended_calls: Vec<String> = match &ev {
            Event::CallState { call, state, .. } if CallState::parse(state) == CallState::Over => vec![call.clone()],
            _ => vec![],
        };
        let changed = self.model.apply(&ev);
        for call in ended_calls {
            self.desk.end(&acc, &call);
            if self.model.calls.is_empty() {
                self.stage = false;
                self.desk_frame = None;
            }
        }
        if let Event::CallState { call, state, .. } = &ev {
            if CallState::parse(state) == CallState::Current {
                if let Some(c) = self.model.calls.get_mut(call) {
                    if !c.hello_sent {
                        c.hello_sent = true;
                        self.desk.hello(&acc, call);
                    }
                }
            }
        }
        if let Event::DecodingStarted { id, .. } = &ev {
            if self.model.calls.values().any(|c| c.sinks.contains(id)) {
                let _ = joma_jami::attach_sink(id);
            }
        }
        if let Event::DecodingStopped { id } = &ev {
            self.frames.remove(id);
        }
        if let Event::NameFound { query, state, address, .. } = &ev {
            if self.adding.as_deref() == Some(query.as_str()) {
                self.adding = None;
                if *state == 0 {
                    self.add_contact(address.clone(), window, cx);
                } else {
                    self.show_flash(format!("no Jami user named {query}"), cx);
                }
            }
        }
        for id in &changed.reload {
            self.reload_conversation(id, false);
        }
        for uri in &changed.lookup {
            self.lookup(uri);
        }
        if let Some((conv, text)) = &changed.incoming {
            let active = window.is_window_active();
            if self.selected.as_deref() == Some(conv) && active {
                self.mark_read(conv);
            } else {
                let title = self.model.conversations.get(conv).map(|c| c.title.clone()).unwrap_or_default();
                notify_desktop(&title, text);
            }
            self.pinned_bottom = true;
        }
        if let Some(call) = &changed.incoming_call {
            if let Some(c) = self.model.calls.get(call) {
                let name = self.model.name_of(&c.peer);
                notify_desktop(&format!("{name} is calling"), if c.video { "video call" } else { "audio call" });
                if let Some(conv) = c.conversation.clone() {
                    self.select(conv, cx);
                }
            }
            window.activate_window();
        }
        if self.selected.is_none() {
            if let Some(id) = self.model.ordered().first().map(|c| c.id.clone()) {
                self.select(id, cx);
            }
        }
        if changed.any || !changed.reload.is_empty() {
            cx.notify();
        }
    }

    fn after(&mut self, after: After, cx: &mut Context<Self>) {
        if let After::Portal(call) = after {
            self.start_portal(call, cx);
        }
    }

    // ---- Conversations ----------------------------------------------------------

    pub fn select(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected = Some(id.clone());
        self.focus_composer = self.mode == Mode::Chat;
        self.mark_read(&id);
        self.pinned_bottom = true;
        if let Some(c) = self.model.conversations.get(&id) {
            if !c.loaded && !c.request {
                if let Ok(req) = joma_jami::load_conversation(&self.model.account, &id, "", 50) {
                    self.model.loading.insert(req, id.clone());
                }
            }
        }
        self.configure_inputs(cx);
        cx.notify();
    }

    fn mark_read(&mut self, id: &str) {
        if let Some(c) = self.model.conversations.get_mut(id) {
            if c.unread > 0 {
                c.unread = 0;
                if let Some(last) = c.messages.last() {
                    let _ = joma_jami::set_displayed(&self.model.account, id, &last.id);
                }
            }
        }
    }

    pub fn step(&mut self, by: i32, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.visible_conversations().iter().map(|c| c.id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        let at = self.selected.as_ref().and_then(|s| ids.iter().position(|i| i == s)).unwrap_or(0) as i32;
        let next = (at + by).rem_euclid(ids.len() as i32) as usize;
        self.select(ids[next].clone(), cx);
    }

    /// Conversations shown in the sidebar, filtered in search mode.
    pub fn visible_conversations(&self) -> Vec<&crate::model::Conversation> {
        let all = self.model.ordered();
        if self.mode != Mode::Search {
            return all;
        }
        let q = self.search.to_lowercase();
        all.into_iter().filter(|c| q.is_empty() || c.title.to_lowercase().contains(&q)).collect()
    }

    fn send_composed(&mut self, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone() else { return };
        let text = self.composer.read(cx).text();
        if text.trim().is_empty() {
            return;
        }
        match joma_jami::send_message(&self.model.account, &conv, text.trim_end(), "") {
            Ok(()) => {
                self.composer.update(cx, |c, cx| c.clear(cx));
                let _ = joma_jami::set_composing(&self.model.account, &conv, false);
                self.composing_sent = None;
                self.pinned_bottom = true;
            }
            Err(e) => self.show_flash(format!("not sent: {e}"), cx),
        }
    }

    fn typing(&mut self, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone() else { return };
        if self.composing_sent.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            let _ = joma_jami::set_composing(&self.model.account, &conv, true);
            self.composing_sent = Some(Instant::now());
        }
        cx.notify();
    }

    pub fn send_quick(&mut self, index: usize, cx: &mut Context<Self>) {
        let (Some(conv), Some(text)) = (self.selected.clone(), self.quick.messages.get(index).cloned()) else { return };
        match joma_jami::send_message(&self.model.account, &conv, &text, "") {
            Ok(()) => {
                self.quick_open = false;
                self.show_flash(format!("sent “{text}”"), cx);
            }
            Err(e) => self.show_flash(format!("not sent: {e}"), cx),
        }
    }

    pub fn toggle_favorite(&mut self, cx: &mut Context<Self>) {
        if let Some(conv) = self.selected.clone() {
            self.quick.toggle_favorite(&conv);
            if let Err(e) = self.quick.save() {
                self.show_flash(format!("could not save favourites: {e}"), cx);
            }
            cx.notify();
        }
    }

    pub fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.input.update(cx, |c, cx| c.clear(cx));
        self.search.clear();
        self.configure_inputs(cx);
        match mode {
            Mode::Chat => window.focus(&self.composer.focus_handle(cx)),
            _ => window.focus(&self.input.focus_handle(cx)),
        }
        cx.notify();
    }

    fn submit_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().trim().to_owned();
        if self.gate == Gate::Onboarding {
            match joma_jami::create_account(&text) {
                Ok(_) => self.show_flash("creating your account…", cx),
                Err(e) => self.show_flash(format!("could not create the account: {e}"), cx),
            }
            return;
        }
        match self.mode {
            Mode::Add => {
                if text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit()) {
                    self.add_contact(text, window, cx);
                } else if !text.is_empty() {
                    self.adding = Some(text.clone());
                    let _ = joma_jami::lookup_name(&self.model.account, &text);
                    self.show_flash(format!("looking up {text}…"), cx);
                }
            }
            Mode::Search => {
                if let Some(id) = self.visible_conversations().first().map(|c| c.id.clone()) {
                    self.set_mode(Mode::Chat, window, cx);
                    self.select(id, cx);
                }
            }
            Mode::Chat => {}
        }
    }

    fn add_contact(&mut self, uri: String, window: &mut Window, cx: &mut Context<Self>) {
        match joma_jami::add_contact(&self.model.account, &uri) {
            Ok(()) => {
                self.show_flash("contact added · the conversation starts when they accept", cx);
                self.set_mode(Mode::Chat, window, cx);
            }
            Err(e) => self.show_flash(format!("could not add: {e}"), cx),
        }
    }

    pub fn accept_request(&mut self, id: String, accept: bool, cx: &mut Context<Self>) {
        self.pinned_bottom = true;
        self.focus_composer = accept;
        let acc = self.model.account.clone();
        let r = if accept { joma_jami::accept_request(&acc, &id) } else { joma_jami::decline_request(&acc, &id) };
        if let Err(e) = r {
            self.show_flash(e.to_string(), cx);
        }
        if !accept {
            self.model.conversations.remove(&id);
        } else if let Some(c) = self.model.conversations.get_mut(&id) {
            c.request = false;
        }
        self.configure_inputs(cx);
        cx.notify();
    }

    // ---- Calls ------------------------------------------------------------------

    /// The call of the selected conversation, or else any call.
    pub fn active_call(&self) -> Option<&Call> {
        let sel = self.selected.as_deref();
        self.model
            .calls
            .values()
            .find(|c| c.conversation.as_deref() == sel && sel.is_some())
            .or_else(|| self.model.calls.values().next())
    }

    pub fn place_call(&mut self, video: bool, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone() else { return };
        if self.active_call().is_some() {
            return;
        }
        self.place_call_to(&conv, video, cx);
    }

    /// Call a conversation; returns the call id.
    pub fn place_call_to(&mut self, conv: &str, video: bool, cx: &mut Context<Self>) -> Option<String> {
        let conv = self.model.conversations.get(conv).cloned()?;
        let peer = conv.peers.first().cloned()?;
        let to = if conv.peers.len() == 1 { peer.clone() } else { format!("swarm:{}", conv.id) };
        let camera = camera_source();
        let mut media = vec![joma_jami::media(joma_jami::AUDIO, "audio_0", "", false)];
        if video {
            media.push(joma_jami::media(joma_jami::VIDEO, "video_0", &camera, camera.is_empty()));
        }
        match joma_jami::place_call(&self.model.account, &to, &media) {
            Ok(id) if !id.is_empty() => {
                let mut call = Call::new(&id, &peer, Some(conv.id.clone()), CallState::Connecting, video);
                call.muted_video = !video || camera.is_empty();
                self.model.calls.insert(id.clone(), call);
                cx.notify();
                return Some(id);
            }
            Ok(_) => self.show_flash("the call could not start", cx),
            Err(e) => self.show_flash(format!("call failed: {e}"), cx),
        }
        cx.notify();
        None
    }

    pub fn accept_call(&mut self, video: bool, cx: &mut Context<Self>) {
        let Some(call) = self.model.calls.values().find(|c| c.state == CallState::Incoming).cloned() else { return };
        let camera = camera_source();
        let mut media = vec![joma_jami::media(joma_jami::AUDIO, "audio_0", "", false)];
        if call.video {
            media.push(joma_jami::media(joma_jami::VIDEO, "video_0", &camera, !video || camera.is_empty()));
        }
        if let Some(c) = self.model.calls.get_mut(&call.id) {
            c.muted_video = !video || camera.is_empty();
        }
        if let Err(e) = joma_jami::accept_call(&self.model.account, &call.id, &media) {
            self.show_flash(format!("could not answer: {e}"), cx);
        }
        cx.notify();
    }

    pub fn hang_up(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.active_call().cloned() else { return };
        let acc = self.model.account.clone();
        self.desk.end(&acc, &call.id);
        let r = if call.state == CallState::Incoming {
            joma_jami::refuse_call(&acc, &call.id)
        } else {
            joma_jami::hang_up(&acc, &call.id)
        };
        if let Err(e) = r {
            self.show_flash(e.to_string(), cx);
        }
        self.stage = false;
        cx.notify();
    }

    pub fn toggle_mute(&mut self, video: bool, cx: &mut Context<Self>) {
        let Some(call) = self.active_call().cloned() else { return };
        let (kind, now) = if video { (joma_jami::VIDEO, !call.muted_video) } else { (joma_jami::AUDIO, !call.muted_audio) };
        if video && now == false && !call.video {
            // An audio call gains video: ask for a camera stream.
            let mut media = joma_jami::current_media(&self.model.account, &call.id).unwrap_or_default();
            media.push(joma_jami::media(joma_jami::VIDEO, "video_0", &camera_source(), false));
            let _ = joma_jami::request_media_change(&self.model.account, &call.id, &media);
        } else {
            let _ = joma_jami::mute_local(&self.model.account, &call.id, kind, now);
        }
        if let Some(c) = self.model.calls.get_mut(&call.id) {
            if video {
                c.muted_video = now;
                c.video |= !now;
            } else {
                c.muted_audio = now;
            }
        }
        cx.notify();
    }

    /// Share the desktop in the current call, or stop sharing.
    pub fn toggle_share(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.active_call().cloned() else {
            self.show_flash("start a call to share your desktop", cx);
            return;
        };
        if call.state != CallState::Current {
            return;
        }
        if self.desk.session(&call.id).sharing_label().is_some() {
            let acc = self.model.account.clone();
            self.desk.stop(&acc, &call.id, &camera_source(), call.muted_video);
            cx.notify();
            return;
        }
        self.share_in(&call.id.clone(), cx);
    }

    /// Start sharing the desktop in a connected call.
    fn share_in(&mut self, call_id: &str, cx: &mut Context<Self>) {
        let Some(call) = self.model.calls.get(call_id).cloned() else { return };
        if self.desk.session(&call.id).sharing_label().is_some() {
            return;
        }
        let acc = self.model.account.clone();
        let tx = self.tx.clone();
        let id = call.id.clone();
        let after = self.desk.start(&acc, &call.id, &call.peer, call.peer_joma, move |ev| {
            let _ = tx.unbounded_send(AppMsg::Share(id.clone(), ev));
        });
        self.after(after, cx);
        cx.notify();
    }

    fn start_portal(&mut self, call: String, cx: &mut Context<Self>) {
        // A test profile must not open the real desktop's screen picker.
        if own_profile() && std::env::var_os("JOMA_PORTAL").is_none() {
            self.show_flash("portal share skipped in a test profile (JOMA_PORTAL=1 allows it)", cx);
            return;
        }
        self.desk.picking(&call);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = PortalShare::start();
            let _ = tx.unbounded_send(AppMsg::Portal(call, r));
        });
        cx.notify();
    }

    // ---- The socket -------------------------------------------------------------

    pub fn quick_state(&self) -> QuickState {
        let candidates: Vec<Candidate> = self
            .model
            .ordered()
            .iter()
            .filter(|c| !c.request)
            .map(|c| Candidate { id: c.id.clone(), unread: c.unread, online: self.model.is_online(c) })
            .collect();
        let targets = self
            .quick
            .rank(&candidates)
            .iter()
            .filter_map(|id| self.model.conversations.get(id))
            .map(|c| Target {
                conversation: c.id.clone(),
                title: c.title.clone(),
                unread: c.unread,
                favorite: self.quick.favorites.contains(&c.id),
                last: c.last().map(|m| m.text.chars().take(60).collect()).unwrap_or_default(),
                online: self.model.is_online(c),
            })
            .collect();
        QuickState {
            account: self.model.my_name.clone(),
            registered: self.model.registration == "REGISTERED",
            unread: self.model.total_unread(),
            targets,
            messages: self.quick.messages.clone(),
        }
    }

    pub fn send_to(&mut self, to: &str, text: &str) -> Reply {
        let conv = match self.target(to) {
            Ok(c) => c,
            Err(e) => return e,
        };
        match joma_jami::send_message(&self.model.account, &conv, text, "") {
            Ok(()) => Reply::Done,
            Err(e) => Reply::Error { message: e.to_string() },
        }
    }

    fn ipc(&mut self, req: Request, window: &mut Window, cx: &mut Context<Self>) -> Reply {
        match req {
            Request::Show => {
                window.activate_window();
                Reply::Done
            }
            Request::QuickPopup => {
                self.quick = QuickConfig::load();
                // Opening a window inside this update would block on Wayland.
                let me = cx.entity();
                cx.defer(move |cx| crate::popup::open(me, cx));
                Reply::Done
            }
            Request::Quick => {
                self.quick = QuickConfig::load();
                Reply::Quick(self.quick_state())
            }
            Request::Send { to, text } => {
                let r = self.send_to(&to, &text);
                cx.notify();
                r
            }
            Request::SendQuick { to, index } => match self.quick.messages.get(index).cloned() {
                Some(text) => self.send_to(&to, &text),
                None => Reply::Error { message: format!("no quick message {}", index + 1) },
            },
            Request::Chat { to, limit } => match self.target(&to) {
                Ok(conv) => {
                    self.mark_read(&conv);
                    cx.notify();
                    Reply::Chat(self.chat_state(&conv, limit))
                }
                Err(e) => e,
            },
            Request::Call { to, video } => match self.target(&to) {
                Ok(_) if self.active_call().is_some() => Reply::Error { message: "a call is already running".into() },
                Ok(conv) => match self.place_call_to(&conv, video, cx) {
                    Some(_) => Reply::Done,
                    None => Reply::Error { message: "the call could not start".into() },
                },
                Err(e) => e,
            },
            Request::ShareScreen { to } => match self.target(&to) {
                Ok(conv) => {
                    let running = self.model.calls.values().find(|c| c.conversation.as_deref() == Some(conv.as_str())).cloned();
                    match running {
                        Some(c) if c.state == CallState::Current => self.share_in(&c.id, cx),
                        Some(c) => {
                            self.share_on_connect.insert(c.id);
                        }
                        None if self.active_call().is_some() => {
                            return Reply::Error { message: "another call is running".into() };
                        }
                        None => match self.place_call_to(&conv, false, cx) {
                            Some(id) => {
                                self.share_on_connect.insert(id);
                            }
                            None => return Reply::Error { message: "the call could not start".into() },
                        },
                    }
                    Reply::Done
                }
                Err(e) => e,
            },
            Request::SendFile { to, path } => match self.target(&to) {
                Ok(conv) => match path {
                    Some(p) => self.send_file(&conv, &p),
                    None => {
                        self.pick_and_send(conv, cx);
                        Reply::Done
                    }
                },
                Err(e) => e,
            },
            Request::Open { to } => match self.target(&to) {
                Ok(conv) => {
                    if self.mode != Mode::Chat {
                        self.set_mode(Mode::Chat, window, cx);
                    }
                    self.select(conv, cx);
                    window.activate_window();
                    Reply::Done
                }
                Err(e) => e,
            },
        }
    }

    /// The conversation a socket request names.
    fn target(&self, to: &str) -> Result<String, Reply> {
        if self.gate != Gate::Ready {
            return Err(Reply::Error { message: "Joma is not connected to Jami yet".into() });
        }
        self.model.resolve(to).map(|c| c.id.clone()).ok_or_else(|| Reply::Error { message: format!("no conversation matches “{to}”") })
    }

    pub fn chat_state(&self, conv: &str, limit: usize) -> ChatState {
        let Some(c) = self.model.conversations.get(conv) else { return ChatState::default() };
        let skip = c.messages.len().saturating_sub(limit.max(1));
        ChatState {
            conversation: c.id.clone(),
            title: c.title.clone(),
            online: self.model.is_online(c),
            composing: c.composing,
            in_call: self.model.calls.values().any(|k| k.conversation.as_deref() == Some(conv)),
            messages: c.messages[skip..]
                .iter()
                .map(|m| ChatLine {
                    mine: bare_uri(&m.author) == self.model.me,
                    author: self.model.name_of(&m.author),
                    text: m.text.clone(),
                    ts: m.ts,
                    event: m.kind == crate::model::MsgKind::Event,
                })
                .collect(),
        }
    }

    fn send_file(&mut self, conv: &str, path: &str) -> Reply {
        let path = std::path::Path::new(path);
        if !path.is_file() {
            return Reply::Error { message: format!("{} is not a file", path.display()) };
        }
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        match joma_jami::send_file(&self.model.account, conv, &abs.to_string_lossy()) {
            Ok(()) => Reply::Done,
            Err(e) => Reply::Error { message: e.to_string() },
        }
    }

    /// Ask for a file (the desktop's file chooser) and send it.
    pub fn pick_and_send(&mut self, conv: String, cx: &mut Context<Self>) {
        // A test profile must not open the real desktop's file chooser.
        if own_profile() && std::env::var_os("JOMA_PORTAL").is_none() {
            self.show_flash("file chooser skipped in a test profile (JOMA_PORTAL=1 allows it)", cx);
            return;
        }
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Send".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let _ = this.update(cx, |j, cx| {
                for p in paths {
                    if let Reply::Error { message } = j.send_file(&conv, &p.to_string_lossy()) {
                        j.show_flash(format!("not sent: {message}"), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Focusable for Joma {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The camera to send, as a daemon source (`camera://<device>`), or empty without one.
pub fn camera_source() -> String {
    match joma_jami::default_video_device() {
        Ok(d) if !d.is_empty() => format!("camera://{d}"),
        _ => String::new(),
    }
}

/// Standard base64 (the daemon names profile files by the base64 of the peer URI).
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_profile_names() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("YWJjZA").unwrap(), b"abcd");
    }
}
