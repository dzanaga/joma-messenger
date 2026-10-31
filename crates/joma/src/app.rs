//! The Joma window: state, daemon events and user actions. Rendering is in `render.rs`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use futures::channel::mpsc::UnboundedSender;
use futures::StreamExt;
use gpui::{actions, App, AppContext, Context, Entity, FocusHandle, Focusable, RenderImage, ScrollHandle, Window};
use joma_desk::portal::PortalShare;
use joma_desk::share::{ShareEvent, Source, SourceInfo};
use joma_desk::view::Picture;
use joma_desk::signal::{DeskMsg, MIME};
use joma_jami::Event;
use joma_quick::ipc::{ChatLine, ChatState, QuickState, Reply, Request, Target};
use joma_quick::{Candidate, QuickConfig};

use crate::composer::{Attach, Composer, Edited, Submit};
use crate::desk::{After, Desk};
use crate::ipc::Pending;
use crate::model::{bare_uri, Call, CallState, FileRef, Model};
use crate::hypr::Shape;
use crate::presence::{Presence, State, Wire};
use crate::tray::{Look, Tray, TrayAction};
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
        ToggleContacts,
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
        ToggleFullscreen,
        ZoomIn,
        ZoomOut,
        ZoomFit,
        TabChats,
        TabCalls,
        TabContacts,
        TabDevices,
        TabAccounts,
        TabAgents,
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
    Tray(crate::tray::TrayAction),
    /// A thumbnail for the source picker: the picker's generation, the source's key.
    Thumb(u64, String, Picture),
    /// The area drawn with `slurp`, for what the picker was asked for.
    Area(PickFor, anyhow::Result<Source>),
}

/// What the source picker starts once the user chooses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickFor {
    /// Share in this call.
    Call(String),
    /// Share without a call with this conversation's peer.
    Direct(String),
    /// Switch a running share (by session key) to another source.
    Switch(String),
}

/// What a click on the pixel grid does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintTool {
    Pen,
    Eraser,
    Fill,
}

/// The profile editor: the picture, and the pixel-art drawing when one is open.
pub struct ProfileEdit {
    pub drawing: Option<crate::avatar::PixelArt>,
    pub color: u32,
    pub tool: PaintTool,
    pub mirror: bool,
    /// Where the grid was painted last, for the pointer's cell.
    pub grid: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    pub painting: bool,
}

/// The source picker: the screens and windows to share, with thumbnails as they come.
pub struct Picker {
    pub target: PickFor,
    pub gen: u64,
    pub sources: Vec<SourceInfo>,
    pub thumbs: HashMap<String, Arc<RenderImage>>,
    /// Who sees it.
    pub peer: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    None,
    /// A device is on the Jami network (or Joma says away).
    Reachable,
    /// Joma says the user is active.
    Active,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Chat,
    /// The input adds a contact by name or ID.
    Add,
    /// The input filters conversations.
    Search,
    /// The input registers our username.
    Username,
    /// The list shows the removed conversations, to restore or delete.
    Removed,
    /// The input takes the code a new device shows, to link it to this account.
    LinkDevice,
    /// The input renames this device.
    DeviceName,
    /// The input asks for "delete" before removing an account no other device has.
    RemoveAccount,
}

/// This machine joining an account that lives on another device.
#[derive(Clone, Default)]
pub struct Linking {
    /// The new, still empty account.
    pub account: String,
    /// The `jami-auth://` code to enter on the other device.
    pub token: String,
    /// What the daemon said last.
    pub status: String,
    /// The other device sent the account, and it has a password: type it.
    pub password_scheme: Option<String>,
    pub error: String,
}

/// Sending this account to a new device.
#[derive(Clone)]
pub struct DeviceOp {
    pub op: u32,
    /// Started from the command line: confirm without asking.
    pub auto: bool,
    pub state: i64,
    /// The new device's network address, once it connected.
    pub peer: String,
}

/// What the sidebar lists (the rail on the left picks it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Chats,
    /// Running calls and shares, and their history.
    Calls,
    Contacts,
    /// This account's devices, and linking another.
    Devices,
    /// The accounts on this machine: open one, add one, remove one.
    Accounts,
    /// Agent accounts (a placeholder for now).
    Agents,
}

/// A desktop share that took place, for the Calls tab (kept per account).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ShareLog {
    pub ts: i64,
    pub peer: String,
    /// true: we shared; false: we watched.
    pub shared: bool,
    pub secs: u64,
}

pub fn share_log_path(account: &str) -> Option<std::path::PathBuf> {
    Some(joma_quick::config_path()?.parent()?.join(format!("shares-{account}.json")))
}

pub fn load_share_log(account: &str) -> Vec<ShareLog> {
    share_log_path(account).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// A row being dragged in the sidebar: a conversation id or a contact URI.
#[derive(Clone)]
pub struct Dragged {
    pub tab: Tab,
    pub id: String,
    pub title: String,
}

/// What a contact's quick buttons do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Quick {
    Message,
    Call,
    Video,
    Share,
    File,
}

/// A username the name server can take: 3 to 32 of a-z, 0-9, `-` and `_`, lower-cased.
pub fn valid_username(name: &str) -> Option<String> {
    let n = name.trim().trim_start_matches('@').to_lowercase();
    let ok = (3..=32).contains(&n.len()) && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(n)
}

pub const USERNAME_RULE: &str = "3 to 32 letters, digits, - or _";

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
    pub tab: Tab,
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
    /// The stage's height above the chat, while it does not fill the window.
    pub stage_height: f32,
    /// Dragging the bar under the stage: where the drag started and the height then.
    pub resizing: Option<(f32, f32)>,
    /// The stage fills the screen.
    pub fullscreen: bool,
    /// The window has been full screen since we asked (so leaving it is the user's doing).
    pub fullscreen_seen: bool,
    /// Zoom and pan on the stage; the paint code updates its layout.
    pub stage_view: Rc<RefCell<crate::stage::StageView>>,
    /// Choosing what to share.
    pub picker: Option<Picker>,
    /// Editing our picture.
    pub profile_edit: Option<ProfileEdit>,
    /// Shares that took place (Calls tab), newest last.
    pub share_log: Vec<ShareLog>,
    /// Shares running now: session key → (started, unix time, we share).
    running_shares: HashMap<String, (Instant, i64, bool)>,
    picker_gen: u64,
    /// The sharer's preview of each running Joma share, by session key.
    pub previews: HashMap<String, Frame>,
    pub flash: Option<(String, Instant)>,
    pub focus: FocusHandle,
    pub messages_scroll: ScrollHandle,
    pub pinned_bottom: bool,
    /// Move the keyboard focus to the composer at the next frame.
    pub focus_composer: bool,
    /// Add-contact lookups in flight: the typed name.
    adding: Option<String>,
    /// Creating an account: the name typed in the first step.
    pub onboard_name: Option<String>,
    /// A username to register once the new account is online.
    pending_username: Option<String>,
    /// A step that asks twice ("delete for good" on a conversation id, "block:<uri>",
    /// "remove:<uri>") was clicked once, at this time.
    pub confirm_delete: Option<(String, Instant)>,
    repaint_queued: Arc<AtomicBool>,
    composing_sent: Option<Instant>,
    /// Peers whose presence we follow.
    subscribed: HashSet<String>,
    /// Calls placed to share the desktop: the share starts once they connect.
    share_on_connect: HashSet<String>,
    /// Files being or already downloaded this session: file id to where.
    pub downloads: HashMap<String, std::path::PathBuf>,
    /// Files pasted or dropped, waiting for Enter, by conversation.
    pub attachments: HashMap<String, Vec<std::path::PathBuf>>,
    /// Joma presence: ours and our contacts'.
    pub presence: Presence,
    pub tray: Tray,
    /// The window is narrow (the compact panel): one column.
    pub narrow: bool,
    /// In a narrow window, the conversation is open (else the list shows).
    pub narrow_chat: bool,
    /// What the setup banner offers, as found at start and after each step.
    pub setup: SetupState,
    /// Joining an account from another device, during onboarding.
    pub linking: Option<Linking>,
    /// Sending the account to a new device.
    pub device_op: Option<DeviceOp>,
    /// Our other devices' Joma presence, by device id.
    pub my_devices: HashMap<String, crate::presence::Peer>,
    /// When we last told our other devices, and what.
    self_told: Option<(State, Instant)>,
    /// This machine's accounts: id and the name to show.
    pub accounts: Vec<(String, String)>,
    /// Creating or linking another account: the one to go back to, and the accounts
    /// there were (the new one is the one not among them).
    pub adding_account: Option<(String, Vec<String>)>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SetupState {
    pub bar_has_tray: bool,
    pub pinned: bool,
    pub shortcut_possible: bool,
    pub shortcut: bool,
    pub start_at_login: bool,
}

impl SetupState {
    pub fn read() -> SetupState {
        SetupState {
            bar_has_tray: crate::setup::bar_has_tray(),
            pinned: crate::setup::pinned_in_bar(),
            shortcut_possible: crate::setup::shortcut_possible(),
            shortcut: crate::setup::has_shortcut(),
            start_at_login: crate::setup::starts_at_login(),
        }
    }

    /// Something left to offer.
    pub fn pending(&self) -> bool {
        (self.bar_has_tray && !self.pinned) || (self.shortcut_possible && !self.shortcut) || (!self.start_at_login && std::env::var_os("JOMA_PROFILE").is_none())
    }
}

/// Received files up to this size download by themselves (the Jami app's default).
const AUTO_DOWNLOAD: u64 = 20 * 1024 * 1024;

/// Where received files go: `~/Downloads/joma` (the XDG download dir, if set).
fn download_dir() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let base = std::process::Command::new("xdg-user-dir")
        .arg("DOWNLOAD")
        .env("HOME", &home)
        .output()
        .ok()
        .map(|o| std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .filter(|p| p.is_absolute() && *p != home)
        .unwrap_or_else(|| home.join("Downloads"));
    base.join("joma")
}

/// `dir/name`, or `dir/name (2).ext` and so on when that exists. The name loses any path.
fn free_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let name = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "file".into());
    let p = dir.join(&name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    (2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).expect("a free name")
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

/// The daemon was started in this process (libjami cannot start twice in one).
static DAEMON_STARTED: AtomicBool = AtomicBool::new(false);

/// Start Joma over in this process, to run the daemon again after it stepped aside for
/// the Qt client. A hidden window stays hidden (`JOMA_HIDDEN`).
fn restart() -> std::io::Error {
    use std::os::unix::process::CommandExt;
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return e,
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1));
    if crate::hypr::hidden() {
        cmd.env("JOMA_HIDDEN", "1");
    } else {
        cmd.env_remove("JOMA_HIDDEN");
    }
    cmd.exec()
}

/// Is this a test profile (its own XDG data dir), where the Qt client cannot interfere?
fn own_profile() -> bool {
    std::env::var_os("JOMA_PROFILE").is_some()
}

/// Does the Qt client share our data, so Joma must not run beside it? Tests set
/// `JOMA_GUARD_QT` to check the guard in a test profile.
fn guard_qt() -> bool {
    !own_profile() || std::env::var_os("JOMA_GUARD_QT").is_some()
}

/// Show the window, also when closing it had hidden it.
fn raise(window: &mut Window) {
    crate::hypr::show_as(crate::hypr::shape());
    window.activate_window();
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
        let tray_tx = tx.clone();
        let composer = cx.new(Composer::new);
        let input = cx.new(Composer::new);
        cx.subscribe(&composer, |this, _, _: &Submit, cx| this.send_composed(cx)).detach();
        cx.subscribe(&composer, |this, _, _: &Edited, cx| this.typing(cx)).detach();
        cx.subscribe(&composer, |this, _, a: &Attach, cx| match a {
            Attach::Files(paths) => this.attach(paths.clone(), cx),
            Attach::Image(ext, bytes) => this.attach_image(ext, bytes, cx),
        })
        .detach();
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
            tab: Tab::Chats,
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
            stage_height: 260.,
            resizing: None,
            fullscreen: false,
            fullscreen_seen: false,
            stage_view: Rc::new(RefCell::new(crate::stage::StageView::default())),
            picker: None,
            profile_edit: None,
            share_log: Vec::new(),
            running_shares: HashMap::new(),
            picker_gen: 0,
            previews: HashMap::new(),
            flash: None,
            focus: cx.focus_handle(),
            messages_scroll: ScrollHandle::new(),
            pinned_bottom: true,
            focus_composer: false,
            adding: None,
            onboard_name: None,
            pending_username: None,
            confirm_delete: None,
            repaint_queued: Arc::new(AtomicBool::new(false)),
            composing_sent: None,
            subscribed: HashSet::new(),
            share_on_connect: HashSet::new(),
            downloads: HashMap::new(),
            attachments: HashMap::new(),
            presence: Presence::start(),
            tray: Tray::start(tray_tx, Look { status: crate::tray::Status::Offline, fg: joma_theme::current().foreground, unread: 0, invisible: false, start_at_login: false }),
            narrow: false,
            narrow_chat: false,
            setup: SetupState::read(),
            linking: None,
            device_op: None,
            my_devices: HashMap::new(),
            self_told: None,
            accounts: Vec::new(),
            adding_account: None,
        };
        j.configure_inputs(cx);
        j.boot(cx);
        // Started in the tray: the window maps into `special:joma` (a rule on its first
        // title, see `hypr::start_hidden`), or is hidden once mapped; then it takes its
        // real title. A new account shows the window to be created.
        if crate::hypr::START_HIDDEN.load(Ordering::SeqCst) {
            let onboarding = j.gate == Gate::Onboarding;
            cx.spawn_in(window, async move |_, cx| {
                for _ in 0..20 {
                    cx.background_executor().timer(Duration::from_millis(100)).await;
                    if crate::hypr::hidden() || crate::hypr::hide() {
                        break;
                    }
                }
                let _ = cx.update(|window, _| window.set_window_title("Joma"));
                if onboarding {
                    crate::hypr::show_as(Shape::Full);
                }
            })
            .detach();
        }
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
        if guard_qt() && qt_client_running() {
            self.gate = Gate::QtRunning;
            cx.notify();
            return;
        }
        if DAEMON_STARTED.load(Ordering::SeqCst) && !joma_jami::running() {
            let e = restart();
            self.gate = Gate::Failed(format!("could not restart Joma: {e}"));
            cx.notify();
            return;
        }
        if !DAEMON_STARTED.swap(true, Ordering::SeqCst) {
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
        match accounts.iter().find(|a| **a == self.quick.account).or(accounts.first()) {
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
        self.adding_account = None;
        if self.quick.account != account {
            self.quick.account = account.clone();
            self.save_quick(cx);
        }
        let details = joma_jami::account_details(&account).unwrap_or_default();
        let volatile = joma_jami::volatile_details(&account).unwrap_or_default();
        self.model = Model {
            me: details.get("Account.username").map(|u| bare_uri(u).to_owned()).unwrap_or_default(),
            my_name: details
                .get("Account.displayName")
                .filter(|n| !n.is_empty())
                .or_else(|| volatile.get("Account.registeredName"))
                .cloned()
                .unwrap_or_default(),
            username: volatile.get("Account.registeredName").cloned().unwrap_or_default(),
            registration: volatile.get("Account.registrationStatus").cloned().unwrap_or_default(),
            account,
            ..Default::default()
        };
        self.refresh_identity();
        self.load_profiles();
        self.share_log = load_share_log(&self.model.account);
        self.running_shares.clear();
        self.subscribed.clear();
        self.load_contacts();
        for id in joma_jami::conversations(&self.model.account).unwrap_or_default() {
            self.reload_conversation(&id, false);
        }
        for r in joma_jami::conversation_requests(&self.model.account).unwrap_or_default() {
            if let Some(id) = r.get("id") {
                self.model.add_request(id, &r);
                self.lookup(r.get("from").map(String::as_str).unwrap_or(""));
            }
        }
        self.selected = self.listed().first().map(|c| c.id.clone());
        if let Some(id) = self.selected.clone() {
            self.select(id, cx);
        }
        self.load_accounts();
        self.configure_inputs(cx);
    }

    /// Fill what an account learns late (a linked device gets its archive after opening):
    /// our URI, this device's id, our name (from the profile when the config has none),
    /// and the account's devices.
    fn refresh_identity(&mut self) {
        let acc = self.model.account.clone();
        let details = joma_jami::account_details(&acc).unwrap_or_default();
        if self.model.me.is_empty() {
            self.model.me = details.get("Account.username").map(|u| bare_uri(u).to_owned()).unwrap_or_default();
        }
        let volatile = joma_jami::volatile_details(&acc).unwrap_or_default();
        if let Some(d) = volatile.get("Account.deviceID").filter(|d| !d.is_empty()) {
            self.model.device = d.clone();
        }
        let vcf = self.account_dir().map(|d| d.join("profile.vcf")).and_then(|p| std::fs::read_to_string(p).ok());
        if self.model.my_name.is_empty() {
            if let Some(name) = vcf.as_deref().and_then(crate::model::vcard_name) {
                self.model.my_name = name;
            }
        }
        if let Some(p) = vcf.as_deref().and_then(|v| crate::avatar::from_vcard(&self.model.me, v)) {
            self.model.avatars.insert(self.model.me.clone(), p);
        }
        self.model.devices = joma_jami::known_devices(&acc).unwrap_or_default().into_iter().collect();
    }

    /// The daemon's folder for this account.
    fn account_dir(&self) -> Option<std::path::PathBuf> {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
        Some(data.join("jami").join(&self.model.account))
    }

    /// Where a file of a conversation is on disk, once received or sent: the daemon links
    /// each one from its conversation folder (also those the Jami app downloaded): a
    /// symbolic link to a download, a hard link (the file itself) for one we sent.
    pub fn saved_file(&self, conv: &str, file_id: &str) -> Option<std::path::PathBuf> {
        let link = self.account_dir()?.join("conversation_data").join(conv).join(file_id);
        match std::fs::read_link(&link) {
            Ok(target) => target.exists().then_some(target),
            Err(_) => link.is_file().then_some(link),
        }
    }

    /// Display names from the vCards the daemon keeps per peer.
    fn load_profiles(&mut self) {
        let Some(dir) = self.account_dir().map(|d| d.join("profiles")) else { return };
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            let Some(uri) = crate::avatar::base64_decode(stem).and_then(|b| String::from_utf8(b).ok()) else { continue };
            let Ok(v) = std::fs::read_to_string(&path) else { continue };
            if let Some(name) = crate::model::vcard_name(&v) {
                self.model.names.insert(bare_uri(&uri).to_owned(), name);
            }
            if let Some(p) = crate::avatar::from_vcard(bare_uri(&uri), &v) {
                self.model.avatars.insert(bare_uri(&uri).to_owned(), p);
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

    fn load_contacts(&mut self) {
        let list = joma_jami::contacts(&self.model.account).unwrap_or_default();
        self.model.set_contacts(&list);
        let uris: Vec<String> = self.model.contacts.values().filter(|c| !c.banned).map(|c| c.uri.clone()).collect();
        for uri in uris {
            self.lookup(&uri);
            self.follow(&uri);
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
        self.composer.update(cx, |c, _| {
            c.configure(key, theme.clone(), chat);
            // Enter alone sends what is attached; an empty text alone is ignored.
            c.set_allow_empty(true);
        });
        let (key, placeholder) = match (self.gate.clone(), self.mode) {
            (Gate::Onboarding, _) if self.linking.as_ref().is_some_and(|l| l.password_scheme.is_some()) => {
                ("link-password", "The account's password, then Enter")
            }
            (Gate::Onboarding, _) if self.onboard_name.is_none() => ("onboarding", "Your name (shown to your contacts)"),
            (Gate::Onboarding, _) => ("onboarding-user", "A username, so contacts can find you (Enter alone skips)"),
            (_, Mode::Username) => ("username", "Your username, then Enter"),
            (_, Mode::Add) => ("add", "Jami username or 40-digit ID, then Enter"),
            (_, Mode::Search) => ("search", "Filter conversations"),
            (_, Mode::LinkDevice) => ("link", "The jami-auth:// code the new device shows, then Enter"),
            (_, Mode::DeviceName) => ("device-name", "A name for this device, then Enter"),
            (_, Mode::RemoveAccount) => ("remove-account", "Type delete, then Enter"),
            _ => ("idle", ""),
        };
        self.input.update(cx, |c, _| {
            c.configure(key.into(), theme, true);
            c.set_placeholder(placeholder);
            c.set_allow_empty(key == "onboarding-user");
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

    /// The Qt client started while our daemon runs: stop it, as two daemons on the same
    /// data corrupt it. The Qt client does not check for Joma, so Joma has to.
    fn step_aside(&mut self, cx: &mut Context<Self>) {
        tracing::warn!("the Jami app started: stopping the daemon until it quits");
        self.gate = Gate::QtRunning;
        self.model = Model::default();
        self.selected = None;
        self.frames.clear();
        self.desk_frame = None;
        self.pointer = None;
        self.stage = false;
        // Shares and views end with the daemon; drop them so no capture keeps running.
        self.desk = Desk::default();
        self.share_on_connect.clear();
        self.configure_inputs(cx);
        joma_jami::stop();
        notify_desktop("Joma paused", "The Jami app is open and uses the same accounts. Joma resumes when it quits.");
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if guard_qt() {
            let qt = qt_client_running();
            if qt && joma_jami::running() {
                self.step_aside(cx);
            } else if !qt && self.gate == Gate::QtRunning {
                self.boot(cx);
            }
        }
        if !joma_jami::running() {
            return;
        }
        for (conv, text) in self.model.take_unsent(Instant::now()) {
            let title = self.model.conversations.get(&conv).map(|c| c.title.clone()).unwrap_or_else(|| "a conversation that is gone".into());
            self.show_flash(format!("not sent to {title}: the daemon dropped it"), cx);
            if !text.is_empty() {
                self.composer.update(cx, |c, cx| c.restore(&conv, text, cx));
            }
        }
        self.tell_presence();
        self.update_tray();
        if self.presence.expire(Instant::now()) {
            cx.notify();
        }
        self.track_shares();
        let acc = self.model.account.clone();
        for after in self.desk.tick(&acc) {
            self.after(after, cx);
        }
        for (uri, note) in self.desk.reap_direct() {
            if let Some(note) = note {
                self.show_flash(format!("{}: {note}", self.model.name_of(&uri)), cx);
            }
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
            // Signals still queued when the daemon stepped aside are stale.
            AppMsg::Jami(ev) if joma_jami::running() => self.on_event(ev, window, cx),
            AppMsg::Jami(_) => {}
            AppMsg::Share(call, ev) => {
                match ev {
                    ShareEvent::ViewerConnected { pipeline } => self.show_flash(format!("peer is watching · {pipeline} encoding"), cx),
                    ShareEvent::ViewerLeft => {
                        // Without a call there is nothing else to keep the share for.
                        if crate::desk::direct_peer(&call).is_some() {
                            let acc = self.model.account.clone();
                            self.desk.stop(&acc, &call, "", true);
                        }
                        self.show_flash("peer stopped watching", cx)
                    }
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
            AppMsg::Area(target, area) => match area {
                Ok(src) => self.apply_pick(target, Some(src), cx),
                Err(e) => self.show_flash(format!("no area: {e}"), cx),
            },
            AppMsg::Thumb(gen, key, pic) => {
                if let Some(p) = self.picker.as_mut().filter(|p| p.gen == gen) {
                    if let Some(image) = crate::render::bgra_image(pic.width, pic.height, pic.bgra.as_ref().clone()) {
                        if let Some(old) = p.thumbs.insert(key, image) {
                            cx.drop_image(old, Some(window));
                        }
                    }
                    cx.notify();
                }
            }
            AppMsg::Tray(action) => match action {
                TrayAction::Toggle => self.toggle_window(window, cx),
                TrayAction::Open => raise(window),
                TrayAction::Invisible => self.toggle_invisible(cx),
                TrayAction::StartAtLogin => self.set_start_at_login(!self.setup.start_at_login, cx),
                TrayAction::Quit => {
                    self.leave();
                    cx.quit();
                }
            },
        }
    }

    fn on_event(&mut self, ev: Event, window: &mut Window, cx: &mut Context<Self>) {
        if self.gate == Gate::Onboarding && self.linking.is_some() {
            self.on_link_event(ev, cx);
            cx.notify();
            return;
        }
        if self.gate == Gate::Onboarding {
            if let Event::AccountsChanged = ev {
                let had = self.adding_account.as_ref().map(|(_, had)| had.clone()).unwrap_or_default();
                if let Some(a) = joma_jami::accounts().unwrap_or_default().into_iter().find(|a| !had.contains(a)) {
                    self.open_account(a, cx);
                }
            }
            cx.notify();
            return;
        }
        if let Event::AccountsChanged = ev {
            self.load_accounts();
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
            Event::AccountMessage { account, from, payloads } if *account == acc => {
                if let Some(wire) = payloads.get(crate::presence::MIME).and_then(|p| Wire::decode(p)) {
                    let peer = bare_uri(from).to_owned();
                    if peer == self.model.me {
                        // One of our other devices.
                        if !wire.dev.is_empty() && wire.dev != self.model.device {
                            let fresh = !self.my_devices.contains_key(&wire.dev);
                            self.my_devices.insert(wire.dev.clone(), crate::presence::Peer { state: wire.state, received: Instant::now() });
                            if fresh {
                                self.self_told = None;
                                self.tell_presence();
                            }
                        }
                        cx.notify();
                        return;
                    }
                    if self.presence.received(&peer, wire, Instant::now()) {
                        self.tell_presence();
                    }
                    cx.notify();
                    return;
                }
                if let Some(msg) = payloads.get(MIME).and_then(|p| DeskMsg::decode(p)) {
                    let peer = bare_uri(from).to_owned();
                    let key = crate::desk::direct_key(&peer);
                    let (_, after) = self.desk.message(&acc, &key, &peer, msg);
                    self.after(after, cx);
                    cx.notify();
                }
                return;
            }
            Event::KnownDevices { account, devices } if *account == acc => {
                self.model.devices = devices.clone().into_iter().collect();
                if self.model.device.is_empty() {
                    self.refresh_identity();
                }
                cx.notify();
                return;
            }
            Event::AddDevice { account, op, state, detail } if *account == acc => {
                self.on_add_device(*op, *state, detail, cx);
                return;
            }
            Event::DeviceRevoked { account, device, status } if *account == acc => {
                let name = self.model.devices.get(device).cloned().unwrap_or_else(|| device.chars().take(8).collect());
                self.show_flash(if *status == 0 { format!("{name} revoked") } else { format!("could not revoke {name} ({status})") }, cx);
                self.model.devices = joma_jami::known_devices(&acc).unwrap_or_default().into_iter().collect();
                return;
            }
            Event::PeerServices { request, peer, services, .. } => {
                self.desk.peer_services(&acc, *request, peer, services);
                return;
            }
            Event::TunnelOpened { tunnel, port, .. } => {
                self.desk.tunnel_opened(tunnel, *port, self.repaint());
                self.stage = true;
                self.stage_view.borrow_mut().reset();
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
        if let Event::Presence { account, uri, online: false } = &ev {
            if *account == acc {
                self.presence.gone(bare_uri(uri));
            }
        }
        let changed = self.model.apply(&ev);
        for (conv, interaction, file) in &changed.files {
            if file.size > 0 && file.size <= AUTO_DOWNLOAD {
                self.download(conv, interaction, file, cx);
            }
        }
        if let Event::Transfer { account, file, code, .. } = &ev {
            if *account == acc {
                self.transfer_moved(file, *code, cx);
            }
        }
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
        if let Event::DecodingStarted { id, width, height, .. } = &ev {
            if self.model.calls.values().any(|c| c.sinks.contains(id)) {
                let _ = joma_jami::attach_sink(id, *width, *height);
            }
        }
        if let Event::DecodingStopped { id } = &ev {
            self.frames.remove(id);
        }
        if let Event::Registration { account, state } = &ev {
            // Once online: a new account registers its username, and an account without
            // a profile (created by Joma 0.2 and earlier) gets one, so contacts see its
            // name instead of its ID.
            if *account == acc && state == "REGISTERED" {
                // A linked account knows its username only from the name server: ask
                // for it before offering to choose one.
                if self.model.username.is_empty() && self.pending_username.is_none() {
                    let me = self.model.me.clone();
                    let vol = joma_jami::volatile_details(&acc).unwrap_or_default();
                    match vol.get("Account.registeredName").filter(|n| !n.is_empty()) {
                        Some(n) => self.model.username = n.clone(),
                        None if !me.is_empty() => {
                            let _ = joma_jami::lookup_address(&acc, &me);
                        }
                        None => {}
                    }
                }
                // A new account has no URI until it is generated: until then our own
                // URI counted as a peer.
                if self.model.me.is_empty() || self.model.device.is_empty() {
                    self.refresh_identity();
                    for id in self.model.conversations.keys().cloned().collect::<Vec<_>>() {
                        self.reload_conversation(&id, false);
                    }
                }
                let has_profile = self.account_dir().is_some_and(|d| d.join("profile.vcf").exists());
                if !has_profile && !self.model.my_name.is_empty() {
                    if let Err(e) = joma_jami::update_profile(&acc, &self.model.my_name) {
                        tracing::warn!(error = %e, "could not write the profile");
                    }
                }
                if let Some(u) = self.pending_username.take() {
                    if let Err(e) = joma_jami::register_name(&acc, &u) {
                        self.show_flash(format!("could not register {u}: {e}"), cx);
                    }
                }
            }
        }
        if let Event::NameRegistered { account, state, name } = &ev {
            if *account == acc {
                let text = match state {
                    0 => {
                        self.model.username = name.clone();
                        if self.model.my_name.is_empty() {
                            self.model.my_name = name.clone();
                        }
                        format!("your username is now {name}")
                    }
                    1 => format!("“{name}” is not a valid username ({USERNAME_RULE})"),
                    3 => format!("“{name}” is taken: choose another (click “choose a username”)"),
                    _ => format!("could not register “{name}” (name server error {state})"),
                };
                self.show_flash(text, cx);
            }
        }
        if let Event::NameFound { account, state, address, name, .. } = &ev {
            // Our own name, asked for when the account came without it (a linked one).
            if *account == acc && *state == 0 && bare_uri(address) == self.model.me && self.model.username.is_empty() && !name.is_empty() {
                self.model.username = name.clone();
                if self.model.my_name.is_empty() {
                    self.model.my_name = name.clone();
                }
                cx.notify();
            }
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
        if changed.contacts {
            self.load_contacts();
        }
        for id in &changed.reload {
            self.reload_conversation(id, false);
        }
        for uri in &changed.lookup {
            self.lookup(uri);
        }
        if let Some((conv, text)) = &changed.incoming {
            let active = window.is_window_active();
            if self.is_removed(conv) {
                // A removed contact stays quiet until restored.
            } else if self.selected.as_deref() == Some(conv) && active {
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
            raise(window);
        }
        if self.selected.is_none() {
            if let Some(id) = self.listed().first().map(|c| c.id.clone()) {
                self.select(id, cx);
            }
        }
        if changed.any || !changed.reload.is_empty() {
            cx.notify();
        }
    }

    fn after(&mut self, after: After, cx: &mut Context<Self>) {
        match after {
            After::Nothing => {}
            After::Portal(call) => self.start_portal(call, cx),
            After::Call(uri) => {
                // The peer has no Joma: call them with the microphone off and share there.
                let conv = self.model.ordered().iter().find(|c| c.peers.len() == 1 && bare_uri(&c.peers[0]) == bare_uri(&uri)).map(|c| c.id.clone());
                match conv {
                    Some(conv) if self.active_call().is_none() => {
                        if let Some(id) = self.place_call_with(&conv, false, true, cx) {
                            self.share_on_connect.insert(id);
                            self.show_flash("no Joma there: calling with the microphone off to share", cx);
                        }
                    }
                    _ => self.show_flash("could not share the desktop", cx),
                }
            }
            After::Asked(key) => {
                let who = crate::desk::direct_peer(&key).map(|u| self.model.name_of(u)).unwrap_or_default();
                notify_desktop(&format!("{who} wants to show you their screen"), "Open Joma to watch");
                self.show_flash(format!("{who} wants to show you their screen"), cx);
            }
        }
    }

    /// Share the desktop with a conversation's peer without a call.
    /// The tray icon or Super+Alt+M: show the window, or hide it when it has the focus.
    pub fn toggle_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if crate::hypr::hidden() || !crate::hypr::focused() {
            raise(window);
        } else {
            crate::hypr::hide();
        }
        cx.notify();
    }

    /// From the compact panel to the full window.
    pub fn pop_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::hypr::set_shape(Shape::Full);
        raise(window);
        cx.notify();
    }

    /// Back into the tray; the next show is the compact panel.
    pub fn pop_in(&mut self, cx: &mut Context<Self>) {
        crate::hypr::set_shape(Shape::Compact);
        crate::hypr::hide();
        cx.notify();
    }

    pub fn set_start_at_login(&mut self, on: bool, cx: &mut Context<Self>) {
        match crate::setup::set_start_at_login(on) {
            Ok(()) => self.show_flash(if on { "Joma starts at login, in the tray" } else { "Joma no longer starts at login" }, cx),
            Err(e) => self.show_flash(format!("start at login: {e:#}"), cx),
        }
        self.setup = SetupState::read();
        cx.notify();
    }

    pub fn pin_in_bar(&mut self, cx: &mut Context<Self>) {
        match crate::setup::pin_in_bar() {
            Ok(()) => self.show_flash("Joma's icon is pinned in the bar", cx),
            Err(e) => self.show_flash(format!("pin in the bar: {e:#}"), cx),
        }
        self.setup = SetupState::read();
        cx.notify();
    }

    pub fn add_shortcut(&mut self, cx: &mut Context<Self>) {
        match crate::setup::add_shortcut() {
            Ok(()) => self.show_flash("Super+Alt+M opens and closes Joma", cx),
            Err(e) => self.show_flash(format!("shortcut: {e:#}"), cx),
        }
        self.setup = SetupState::read();
        cx.notify();
    }

    pub fn dismiss_setup(&mut self, cx: &mut Context<Self>) {
        self.quick.setup_dismissed = true;
        self.save_quick(cx);
    }

    /// The tray icon follows the status, the theme and the settings.
    fn update_tray(&mut self) {
        let unread = self.listed_unread();
        let registered = self.model.registration == "REGISTERED";
        let status = if !registered {
            crate::tray::Status::Offline
        } else if unread > 0 {
            crate::tray::Status::Unread
        } else if self.quick.invisible {
            crate::tray::Status::Invisible
        } else {
            crate::tray::Status::Online
        };
        self.tray.set(Look {
            status,
            fg: joma_theme::current().foreground,
            unread,
            invisible: self.quick.invisible,
            start_at_login: self.setup.start_at_login,
        });
    }

    /// Peers of our one-to-one conversations still in the list: who hears our presence.
    fn presence_contacts(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for c in self.listed() {
            if !c.request && c.peers.len() == 1 && !out.contains(&c.peers[0]) {
                out.push(c.peers[0].clone());
            }
        }
        out
    }

    /// Tell contacts our presence when it changed or is due.
    fn tell_presence(&mut self) {
        if self.model.registration != "REGISTERED" || self.gate != Gate::Ready {
            return;
        }
        let state = self.presence.mine(self.quick.invisible);
        let contacts = self.presence_contacts();
        let online = &self.model.online;
        let due = self.presence.due(state, &contacts, |c| online.contains(c), Instant::now());
        for (to, wire) in due {
            self.send_presence(&to, &wire);
        }
        // Our other devices always hear the real state (invisible is for contacts).
        if self.model.devices.len() > 1 {
            let state = self.presence.mine(false);
            let now = Instant::now();
            let due = self.self_told.is_none_or(|(s, t)| s != state || now.duration_since(t) >= crate::presence::REFRESH);
            if due {
                self.self_told = Some((state, now));
                let wire = Wire { state, ts: joma_desk::signal::now(), dev: self.model.device.clone() };
                self.send_presence(&self.model.me.clone(), &wire);
            }
        }
        self.my_devices.retain(|_, p| p.received.elapsed() < crate::presence::EXPIRE);
    }

    fn send_presence(&self, to: &str, wire: &Wire) {
        let mut payloads = joma_jami::Details::new();
        payloads.insert(crate::presence::MIME.into(), wire.encode());
        let _ = joma_jami::send_account_message(&self.model.account, to, &payloads, false);
    }

    /// Quitting: contacts we told forget us now instead of in ten minutes.
    pub fn leave(&mut self) {
        if self.quick.invisible || self.model.registration != "REGISTERED" {
            return;
        }
        let wire = Wire { state: State::Hidden, ts: joma_desk::signal::now(), dev: String::new() };
        for to in self.presence_contacts() {
            if self.model.online.contains(&to) {
                self.send_presence(&to, &wire);
            }
        }
    }

    pub fn toggle_invisible(&mut self, cx: &mut Context<Self>) {
        self.quick.invisible = !self.quick.invisible;
        self.save_quick(cx);
        self.tell_presence();
        self.show_flash(if self.quick.invisible { "invisible: contacts no longer see when you are active" } else { "visible: Joma contacts see when you are active" }, cx);
        cx.notify();
    }

    /// How present a peer is: Joma says active, or only Jami says reachable.
    pub fn dot_of(&self, peer: &str) -> Dot {
        match self.presence.state_of(peer) {
            Some(State::Active) => Dot::Active,
            Some(_) => Dot::Reachable,
            None if self.model.online.contains(peer) => Dot::Reachable,
            None => Dot::None,
        }
    }

    /// The header's status line for a peer: "active now", "away since 14:20",
    /// "on Jami · seen 14:32", or nothing known.
    pub fn presence_line(&self, conv: &crate::model::Conversation, peer: &str) -> Option<String> {
        match self.presence.state_of(peer) {
            Some(State::Active) => return Some("active now".into()),
            Some(State::Away { since }) => return Some(format!("away since {}", crate::render::when(since as i64))),
            _ => {}
        }
        let last_msg = conv.messages.iter().rev().find(|m| m.author == peer).map(|m| m.ts).unwrap_or(0);
        let seen = self.presence.seen.get(peer).map(|s| *s as i64).unwrap_or(0).max(last_msg);
        let mut parts = Vec::new();
        if self.model.online.contains(peer) {
            parts.push("on Jami".to_owned());
        }
        if seen > 0 {
            parts.push(format!("seen {}", crate::render::when(seen)));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    /// Share with a conversation's peer without a call: ask what to share first, when
    /// sharing the Joma way is possible here.
    pub fn share_direct(&mut self, conv: &str, cx: &mut Context<Self>) -> Reply {
        let Some(peer) = self.model.conversations.get(conv).filter(|c| c.peers.len() == 1).map(|c| c.peers[0].clone()) else {
            return Reply::Error { message: "sharing without a call needs a one-to-one conversation".into() };
        };
        if joma_desk::share::available() {
            let name = self.model.name_of(&peer);
            self.open_picker(PickFor::Direct(conv.to_owned()), name, cx);
            return Reply::Done;
        }
        self.share_direct_with(conv, None, cx)
    }

    /// Share `source` (the focused screen if none) with a conversation's peer without a call.
    fn share_direct_with(&mut self, conv: &str, source: Option<Source>, cx: &mut Context<Self>) -> Reply {
        let Some(peer) = self.model.conversations.get(conv).filter(|c| c.peers.len() == 1).map(|c| c.peers[0].clone()) else {
            return Reply::Error { message: "sharing without a call needs a one-to-one conversation".into() };
        };
        let source = match source.map(Ok).unwrap_or_else(|| Source::parse("focused")) {
            Ok(s) => s,
            // Not on Hyprland: the desk calls instead and shares through the portal.
            Err(_) => Source::Screen { output: String::new() },
        };
        let acc = self.model.account.clone();
        let tx = self.tx.clone();
        let key = crate::desk::direct_key(&peer);
        let after = self.desk.start_direct(
            &acc,
            &peer,
            source,
            move |ev| {
                let _ = tx.unbounded_send(AppMsg::Share(key.clone(), ev));
            },
            self.repaint(),
        );
        self.after(after, cx);
        cx.notify();
        Reply::Done
    }

    /// Wake the window from another thread, once per frame at most.
    fn repaint(&self) -> impl Fn() + Send + 'static {
        let tx = self.tx.clone();
        let queued = self.repaint_queued.clone();
        move || {
            if !queued.swap(true, Ordering::SeqCst) {
                let _ = tx.unbounded_send(AppMsg::Repaint);
            }
        }
    }

    // ---- Choosing what to share -------------------------------------------------

    /// List the screens and windows and ask which one to share; thumbnails follow.
    pub fn open_picker(&mut self, target: PickFor, peer: String, cx: &mut Context<Self>) {
        let sources = match joma_desk::share::sources() {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => return self.show_flash("nothing to share: no screen found", cx),
            Err(e) => return self.show_flash(format!("cannot list screens: {e}"), cx),
        };
        self.picker_gen += 1;
        let gen = self.picker_gen;
        let list: Vec<Source> = sources.iter().map(|s| s.source.clone()).collect();
        // One thread each: a screen that never draws must not hold up the others.
        for src in list {
            let tx = self.tx.clone();
            std::thread::Builder::new()
                .name("joma-thumb".into())
                .spawn(move || match joma_desk::preview::thumbnail(&src, (320, 180)) {
                    Ok(p) => {
                        let _ = tx.unbounded_send(AppMsg::Thumb(gen, src.key(), p));
                    }
                    Err(e) => tracing::debug!(source = %src.key(), error = %e, "no thumbnail"),
                })
                .ok();
        }
        self.picker = Some(Picker { target, gen, sources, thumbs: HashMap::new(), peer });
        cx.notify();
    }

    pub fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(p) = self.picker.take() {
            for (_, image) in p.thumbs {
                cx.drop_image(image, Some(window));
            }
        }
        cx.notify();
    }

    /// The user chose source `index` of the picker (or `None`: Jami's own share).
    pub fn pick(&mut self, index: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.picker.as_ref() else { return };
        let target = p.target.clone();
        let source = index.and_then(|i| p.sources.get(i)).map(|s| s.source.clone());
        self.close_picker(window, cx);
        self.apply_pick(target, source, cx);
    }

    /// Draw the area to share (the picker closes so the screen is free to draw on).
    pub fn pick_area(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.picker.as_ref().map(|p| p.target.clone()) else { return };
        self.close_picker(window, cx);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.unbounded_send(AppMsg::Area(target, Source::draw_area()));
        });
    }

    fn apply_pick(&mut self, target: PickFor, source: Option<Source>, cx: &mut Context<Self>) {
        match (target, source) {
            (PickFor::Call(id), None) => self.start_portal(id, cx),
            (PickFor::Call(id), Some(src)) => self.share_in_with(&id, src, cx),
            (PickFor::Direct(conv), src) => {
                if let Reply::Error { message } = self.share_direct_with(&conv, src, cx) {
                    self.show_flash(message, cx);
                }
            }
            (PickFor::Switch(key), Some(src)) => {
                let label = src.key();
                match self.desk.set_source(&key, src) {
                    Ok(()) => self.show_flash(format!("now sharing {label}"), cx),
                    Err(e) => self.show_flash(format!("could not switch: {e}"), cx),
                }
            }
            (PickFor::Switch(_), None) => {}
        }
        cx.notify();
    }

    /// Ask what to share instead, in a running Joma share.
    pub fn change_source(&mut self, key: &str, cx: &mut Context<Self>) {
        let peer = crate::desk::direct_peer(key)
            .map(str::to_owned)
            .or_else(|| self.model.calls.get(key).map(|c| c.peer.clone()))
            .unwrap_or_default();
        let name = self.model.name_of(&peer);
        self.open_picker(PickFor::Switch(key.to_owned()), name, cx);
    }

    /// Note shares as they start and end, for the Calls tab.
    fn track_shares(&mut self) {
        let mut now: HashMap<String, bool> = HashMap::new();
        for (k, s) in &self.desk.sessions {
            if matches!(s.sharing, crate::desk::Sharing::Joma { accepted: true, .. } | crate::desk::Sharing::Portal { .. }) {
                now.insert(format!("s:{k}"), true);
            }
            if self.desk.viewer(k).is_some() {
                now.insert(format!("v:{k}"), false);
            }
        }
        for (k, shared) in &now {
            self.running_shares.entry(k.clone()).or_insert((Instant::now(), joma_desk::signal::now() as i64, *shared));
        }
        let ended: Vec<String> = self.running_shares.keys().filter(|k| !now.contains_key(*k)).cloned().collect();
        for k in ended {
            let Some((t, ts, shared)) = self.running_shares.remove(&k) else { continue };
            let key = &k[2..];
            let peer = crate::desk::direct_peer(key)
                .map(str::to_owned)
                .or_else(|| self.model.calls.get(key).map(|c| c.peer.clone()))
                .unwrap_or_default();
            self.share_log.push(ShareLog { ts, peer, shared, secs: t.elapsed().as_secs() });
            if self.share_log.len() > 200 {
                self.share_log.remove(0);
            }
            if let Some(p) = share_log_path(&self.model.account) {
                let _ = std::fs::write(p, serde_json::to_string(&self.share_log).unwrap_or_default());
            }
        }
    }

    pub fn running_shares_empty(&self) -> bool {
        self.running_shares.is_empty()
    }

    /// Shares running now: (session key, started, we share).
    pub fn shares_running(&self) -> Vec<(String, Instant, bool)> {
        let mut v: Vec<(String, Instant, bool)> = self.running_shares.iter().map(|(k, (t, _, s))| (k[2..].to_owned(), *t, *s)).collect();
        v.sort_by_key(|(_, t, _)| *t);
        v
    }

    // ---- Our picture --------------------------------------------------------------

    pub fn open_profile(&mut self, cx: &mut Context<Self>) {
        self.profile_edit = Some(ProfileEdit {
            drawing: None,
            color: 0x7aa2f7,
            tool: PaintTool::Pen,
            mirror: false,
            grid: Rc::new(std::cell::Cell::new(gpui::Bounds::default())),
            painting: false,
        });
        cx.notify();
    }

    /// Start drawing: the last drawing of this account, else an empty grid.
    pub fn start_drawing(&mut self, cx: &mut Context<Self>) {
        let art = crate::avatar::PixelArt::load(&self.model.account).unwrap_or_default();
        if let Some(p) = self.profile_edit.as_mut() {
            p.drawing = Some(art);
        }
        cx.notify();
    }

    /// Paint the cell under `at` (window coordinates) with the current tool.
    pub fn paint_at(&mut self, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        let Some(p) = self.profile_edit.as_mut() else { return };
        let b = p.grid.get();
        let cell = f32::from(b.size.width) / crate::avatar::GRID as f32;
        if cell <= 0. {
            return;
        }
        let x = (f32::from(at.x - b.origin.x) / cell).floor();
        let y = (f32::from(at.y - b.origin.y) / cell).floor();
        let n = crate::avatar::GRID as f32;
        if x < 0. || y < 0. || x >= n || y >= n {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        let (tool, color, mirror) = (p.tool, p.color, p.mirror);
        let Some(art) = p.drawing.as_mut() else { return };
        match tool {
            PaintTool::Pen => art.set(x, y, Some(color), mirror),
            PaintTool::Eraser => art.set(x, y, None, mirror),
            PaintTool::Fill => art.fill(x, y, Some(color)),
        }
        cx.notify();
    }

    /// Make the drawing our picture.
    pub fn save_drawing(&mut self, cx: &mut Context<Self>) {
        let Some(art) = self.profile_edit.as_ref().and_then(|p| p.drawing.clone()) else { return };
        art.save(&self.model.account);
        self.set_avatar_png(&art.png(8), cx);
        if let Some(p) = self.profile_edit.as_mut() {
            p.drawing = None;
        }
    }

    /// Choose a picture file (the desktop's file chooser), cropped square.
    pub fn choose_picture(&mut self, cx: &mut Context<Self>) {
        if own_profile() && std::env::var_os("JOMA_PORTAL").is_none() {
            self.show_flash("file chooser skipped in a test profile (joma avatar <file> sets one)", cx);
            return;
        }
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Use".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let _ = this.update(cx, |j, cx| {
                if let Some(p) = paths.first() {
                    j.set_avatar_file(p, cx);
                }
            });
        })
        .detach();
    }

    pub fn set_avatar_file(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        match crate::avatar::square_png(path, 256) {
            Ok(png) => self.set_avatar_png(&png, cx),
            Err(e) => self.show_flash(format!("not a picture: {e}"), cx),
        }
    }

    fn set_avatar_png(&mut self, png: &[u8], cx: &mut Context<Self>) {
        let Some(path) = crate::avatar::own_path(&self.model.account) else { return };
        let written = path.parent().map(std::fs::create_dir_all).transpose().and_then(|_| std::fs::write(&path, png));
        if let Err(e) = written {
            return self.show_flash(format!("cannot keep the picture: {e}"), cx);
        }
        match joma_jami::set_avatar(&self.model.account, &self.model.my_name, Some(&path.to_string_lossy())) {
            Ok(()) => {
                if let Some(p) = crate::avatar::cache(&self.model.me, png) {
                    self.model.avatars.insert(self.model.me.clone(), p);
                }
                self.show_flash("picture set; contacts get it with your profile", cx);
            }
            Err(e) => self.show_flash(format!("could not set the picture: {e}"), cx),
        }
        cx.notify();
    }

    pub fn remove_avatar(&mut self, cx: &mut Context<Self>) {
        match joma_jami::set_avatar(&self.model.account, &self.model.my_name, None) {
            Ok(()) => {
                self.model.avatars.remove(&self.model.me);
                self.show_flash("picture removed", cx);
            }
            Err(e) => self.show_flash(format!("could not remove the picture: {e}"), cx),
        }
        cx.notify();
    }

    // ---- The stage --------------------------------------------------------------

    /// Fill the screen with the stage, or come back.
    pub fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.fullscreen && !self.has_stage() {
            self.show_flash("nothing to show full screen: no shared desktop or video", cx);
            return;
        }
        self.fullscreen = !self.fullscreen;
        self.fullscreen_seen = false;
        if self.fullscreen {
            self.stage = true;
        }
        if window.is_fullscreen() != self.fullscreen {
            window.toggle_fullscreen();
        }
        cx.notify();
    }

    /// A peer's desktop or video is there to show.
    pub fn has_stage(&self) -> bool {
        self.desk_frame.is_some() || self.active_call().is_some_and(|c| c.sinks.iter().any(|s| self.frames.contains_key(s)))
    }

    pub fn zoom(&mut self, how: crate::stage::Zoom, cx: &mut Context<Self>) {
        if self.has_stage() {
            self.stage_view.borrow_mut().set(how);
            cx.notify();
        }
    }

    pub fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        if self.has_stage() {
            self.stage_view.borrow_mut().zoom_by(factor);
            cx.notify();
        }
    }

    /// Stop a share without a call (`key` from [`crate::desk::direct_key`]): ours, or the
    /// one we watch or were asked to watch.
    pub fn stop_direct(&mut self, key: &str, cx: &mut Context<Self>) {
        let acc = self.model.account.clone();
        let s = self.desk.session(key);
        if !matches!(s.sharing, crate::desk::Sharing::Off) {
            self.desk.stop(&acc, key, "", true);
        }
        if !matches!(self.desk.session(key).viewing, crate::desk::Viewing::Off) {
            self.desk.decline_offer(&acc, key);
        }
        cx.notify();
    }

    pub fn watch_direct(&mut self, key: &str, cx: &mut Context<Self>) {
        let acc = self.model.account.clone();
        self.desk.accept_offer(&acc, key);
        self.stage = true;
        cx.notify();
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
    pub fn is_removed(&self, conv: &str) -> bool {
        self.quick.removed.iter().any(|r| r == conv)
    }

    /// Requests, then the starred conversations in their order, then the others newest
    /// first; without the removed ones and those with blocked contacts.
    pub fn listed(&self) -> Vec<&crate::model::Conversation> {
        let mut v: Vec<&crate::model::Conversation> =
            self.model.ordered().into_iter().filter(|c| !self.is_removed(&c.id) && !self.model.with_banned(c) && !self.model.is_duplicate(c))
            .collect();
        let star = |id: &str| self.quick.favorites.iter().position(|f| f == id);
        // A stable sort keeps the newest-first order among the unstarred.
        v.sort_by_key(|c| (!c.request, star(&c.id).is_none(), star(&c.id)));
        v
    }

    /// Contacts we talk with: starred first in their order, then the dragged order, then
    /// by name. Blocked ones are apart ([`Joma::blocked`]).
    pub fn contact_list(&self) -> Vec<crate::model::Contact> {
        let mut v: Vec<crate::model::Contact> = self.model.contacts.values().filter(|c| !c.banned).cloned().collect();
        let star = |u: &str| self.quick.starred.iter().position(|s| s == u);
        let placed = |u: &str| self.quick.contact_order.iter().position(|s| s == u);
        v.sort_by_cached_key(|c| {
            (star(&c.uri).is_none(), star(&c.uri), placed(&c.uri).is_none(), placed(&c.uri), self.model.name_of(&c.uri).to_lowercase())
        });
        if self.mode == Mode::Search {
            let q = self.search.to_lowercase();
            v.retain(|c| q.is_empty() || self.model.name_of(&c.uri).to_lowercase().contains(&q) || c.uri.starts_with(&q));
        }
        v
    }

    pub fn blocked(&self) -> Vec<crate::model::Contact> {
        let mut v: Vec<crate::model::Contact> = self.model.contacts.values().filter(|c| c.banned).cloned().collect();
        v.sort_by_cached_key(|c| self.model.name_of(&c.uri).to_lowercase());
        v
    }

    /// Unread messages outside the removed conversations.
    pub fn listed_unread(&self) -> u32 {
        self.listed().iter().map(|c| c.unread).sum()
    }

    pub fn visible_conversations(&self) -> Vec<&crate::model::Conversation> {
        if self.mode == Mode::Removed {
            return self.model.ordered().into_iter().filter(|c| self.is_removed(&c.id)).collect();
        }
        let all = self.listed();
        if self.mode != Mode::Search {
            return all;
        }
        let q = self.search.to_lowercase();
        all.into_iter().filter(|c| q.is_empty() || c.title.to_lowercase().contains(&q)).collect()
    }

    fn send_composed(&mut self, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone() else { return };
        let files = self.attachments.remove(&conv).unwrap_or_default();
        let text = self.composer.read(cx).text();
        // The text first: it reads as the pictures' caption.
        if !text.trim().is_empty() {
            self.send_text(&conv, &text, cx);
        }
        for f in &files {
            if let Reply::Error { message } = self.send_file(&conv, &f.to_string_lossy()) {
                self.show_flash(format!("not sent: {message}"), cx);
            }
        }
        if !files.is_empty() {
            self.pinned_bottom = true;
            cx.notify();
        }
    }

    fn send_text(&mut self, conv: &str, text: &str, cx: &mut Context<Self>) {
        let conv = conv.to_owned();
        match joma_jami::send_message(&self.model.account, &conv, text.trim_end(), "") {
            Ok(()) => {
                self.model.sent(&conv, text.trim_end(), Instant::now());
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
                self.model.sent(&conv, "", Instant::now());
                self.quick_open = false;
                self.show_flash(format!("sent “{text}”"), cx);
            }
            Err(e) => self.show_flash(format!("not sent: {e}"), cx),
        }
    }

    /// Take a conversation off the lists; it stays in Jami and can be restored.
    pub fn remove_from_list(&mut self, conv: &str, cx: &mut Context<Self>) {
        if !self.is_removed(conv) {
            self.quick.removed.push(conv.to_owned());
            self.quick.favorites.retain(|f| f != conv);
        }
        self.save_quick(cx);
        let title = self.model.conversations.get(conv).map(|c| c.title.clone()).unwrap_or_default();
        if self.selected.as_deref() == Some(conv) && self.mode != Mode::Removed {
            self.selected = self.listed().first().map(|c| c.id.clone());
            if let Some(id) = self.selected.clone() {
                self.select(id, cx);
            }
        }
        self.show_flash(format!("{title} removed · the bin icon in the list restores it"), cx);
    }

    pub fn restore(&mut self, conv: &str, cx: &mut Context<Self>) {
        self.quick.removed.retain(|r| r != conv);
        self.save_quick(cx);
        let title = self.model.conversations.get(conv).map(|c| c.title.clone()).unwrap_or_default();
        self.show_flash(format!("{title} is back in the list"), cx);
    }

    /// A step that asks first: true on a second click within 5 s, else arms it and says
    /// `ask`.
    pub fn armed(&mut self, key: &str, ask: &str, cx: &mut Context<Self>) -> bool {
        let armed = self.confirm_delete.as_ref().is_some_and(|(c, t)| c == key && t.elapsed() < Duration::from_secs(5));
        if armed {
            self.confirm_delete = None;
        } else {
            self.confirm_delete = Some((key.to_owned(), Instant::now()));
            self.show_flash(ask.to_owned(), cx);
        }
        armed
    }

    /// The step `key` was clicked once and waits for the second click.
    pub fn is_armed(&self, key: &str) -> bool {
        self.confirm_delete.as_ref().is_some_and(|(c, t)| c == key && t.elapsed() < Duration::from_secs(5))
    }

    /// Delete a removed conversation's history from this machine's devices. The contact
    /// stays: for a one-to-one, Jami starts a new empty conversation with them.
    pub fn delete_for_good(&mut self, conv: &str, cx: &mut Context<Self>) {
        if !self.armed(conv, "click again to delete the history (only on your devices; the contact stays)", cx) {
            return;
        }
        let acc = self.model.account.clone();
        let Some(c) = self.model.conversations.get(conv).cloned() else { return };
        if let Err(e) = joma_jami::remove_conversation(&acc, conv) {
            self.show_flash(format!("could not delete the conversation: {e}"), cx);
            return;
        }
        self.model.conversations.remove(conv);
        self.quick.removed.retain(|r| r != conv);
        self.save_quick(cx);
        if self.selected.as_deref() == Some(conv) {
            self.selected = None;
        }
        self.show_flash(format!("{}: history deleted on your devices", c.title), cx);
    }

    fn save_quick(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.quick.save() {
            self.show_flash(format!("could not save the lists: {e}"), cx);
        }
        cx.notify();
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

    pub fn set_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        if self.mode == Mode::Removed {
            self.set_mode(Mode::Chat, window, cx);
        }
        cx.notify();
    }

    pub fn toggle_starred(&mut self, uri: &str, cx: &mut Context<Self>) {
        self.quick.toggle_starred(uri);
        self.save_quick(cx);
    }

    /// A row dropped on another: see [`joma_quick::drop_on`].
    pub fn drop_row(&mut self, dragged: &Dragged, target: &str, cx: &mut Context<Self>) {
        match dragged.tab {
            Tab::Calls | Tab::Devices | Tab::Accounts | Tab::Agents => {}
            Tab::Chats => {
                let was = self.quick.favorites.contains(&dragged.id);
                joma_quick::drop_on(&mut self.quick.favorites, None, &dragged.id, target);
                if !was && !self.quick.favorites.contains(&dragged.id) {
                    self.show_flash("star a chat to give it a place; the others follow the latest message", cx);
                    return;
                }
            }
            Tab::Contacts => {
                let mut rest: Vec<String> =
                    self.contact_list().into_iter().map(|c| c.uri).filter(|u| !self.quick.starred.contains(u)).collect();
                joma_quick::drop_on(&mut self.quick.starred, Some(&mut rest), &dragged.id, target);
                if !self.quick.starred.contains(&target.to_owned()) {
                    self.quick.contact_order = rest;
                }
            }
        }
        self.save_quick(cx);
    }

    /// A contact's quick button: open the chat, call, share the desktop or send files.
    pub fn quick_action(&mut self, uri: &str, what: Quick, window: &mut Window, cx: &mut Context<Self>) {
        let Some(conv) = self.model.chat_with(uri).filter(|c| !c.request).map(|c| c.id.clone()) else {
            self.show_flash(format!("{} has not accepted yet", self.model.name_of(uri)), cx);
            return;
        };
        if self.is_removed(&conv) {
            self.restore(&conv, cx);
        }
        match what {
            Quick::Message => {
                self.set_mode(Mode::Chat, window, cx);
                self.select(conv, cx);
                self.narrow_chat = true;
            }
            Quick::Call | Quick::Video => {
                self.select(conv.clone(), cx);
                self.place_call_to(&conv, what == Quick::Video, cx);
            }
            Quick::Share => {
                self.select(conv.clone(), cx);
                if let Reply::Error { message } = self.share_direct(&conv, cx) {
                    self.show_flash(message, cx);
                }
            }
            Quick::File => self.pick_and_send(conv, cx),
        }
    }

    /// Block a contact: Jami bans them (no messages, calls or requests from them) and keeps
    /// the history. They are not told.
    pub fn block(&mut self, uri: &str, cx: &mut Context<Self>) {
        let name = self.model.name_of(uri);
        if self.armed(&format!("block:{uri}"), &format!("click again to block {name}"), cx) {
            self.block_now(uri, cx);
        }
    }

    pub fn block_now(&mut self, uri: &str, cx: &mut Context<Self>) {
        let name = self.model.name_of(uri);
        if let Err(e) = joma_jami::remove_contact(&self.model.account, uri, true) {
            self.show_flash(format!("could not block: {e}"), cx);
            return;
        }
        if let Some(c) = self.model.contacts.get_mut(uri) {
            c.banned = true;
        }
        self.presence.gone(uri);
        if self.selected.as_ref().and_then(|s| self.model.conversations.get(s)).is_some_and(|c| self.model.with_banned(c)) {
            self.selected = self.listed().first().map(|c| c.id.clone());
        }
        self.show_flash(format!("{name} is blocked · unblock them in contacts"), cx);
    }

    /// Unblock: they become a contact again, with the old history.
    pub fn unblock(&mut self, uri: &str, cx: &mut Context<Self>) {
        if let Err(e) = joma_jami::add_contact(&self.model.account, uri) {
            self.show_flash(format!("could not unblock: {e}"), cx);
            return;
        }
        if let Some(c) = self.model.contacts.get_mut(uri) {
            c.banned = false;
        }
        self.show_flash(format!("{} is unblocked", self.model.name_of(uri)), cx);
    }

    /// Remove a contact: Jami drops them and your one-to-one conversation with them.
    pub fn remove_contact(&mut self, uri: &str, cx: &mut Context<Self>) {
        let name = self.model.name_of(uri);
        if !self.armed(&format!("remove:{uri}"), &format!("click again to remove {name} and your chat with them"), cx) {
            return;
        }
        if let Err(e) = joma_jami::remove_contact(&self.model.account, uri, false) {
            self.show_flash(format!("could not remove: {e}"), cx);
            return;
        }
        self.model.contacts.remove(uri);
        self.quick.starred.retain(|s| s != uri);
        self.quick.contact_order.retain(|s| s != uri);
        self.save_quick(cx);
        self.show_flash(format!("{name} removed · add them again with + if needed"), cx);
    }

    // ---- Devices -----------------------------------------------------------------

    /// Onboarding: join an account from another device instead of creating one.
    pub fn start_link(&mut self, cx: &mut Context<Self>) {
        match joma_jami::link_account() {
            Ok(account) => {
                self.linking = Some(Linking { account, status: "preparing a code…".into(), ..Default::default() });
                self.input.update(cx, |c, cx| c.clear(cx));
            }
            Err(e) => self.show_flash(format!("could not start linking: {e}"), cx),
        }
        cx.notify();
    }

    /// Give up joining: the empty account goes.
    pub fn cancel_link(&mut self, cx: &mut Context<Self>) {
        if let Some(l) = self.linking.take() {
            let _ = joma_jami::remove_account(&l.account);
        }
        self.configure_inputs(cx);
        cx.notify();
    }

    fn on_link_event(&mut self, ev: Event, cx: &mut Context<Self>) {
        let Some(l) = self.linking.as_mut() else { return };
        let Event::DeviceAuth { account, state, detail } = ev else { return };
        if account != l.account {
            return;
        }
        let error = detail.get("error").cloned().unwrap_or_default();
        match state {
            joma_jami::AUTH_TOKEN => {
                l.token = detail.get("token").cloned().unwrap_or_default();
                l.status = "waiting for your other device".into();
            }
            joma_jami::AUTH_CONNECTING => l.status = "your other device is connecting…".into(),
            joma_jami::AUTH_AUTHENTICATING => {
                let scheme = detail.get("auth_scheme").cloned().unwrap_or_default();
                if detail.get("auth_error").is_some_and(|e| !e.is_empty()) {
                    l.status = "wrong password, try again".into();
                }
                if scheme.is_empty() {
                    let acc = l.account.clone();
                    l.status = "receiving the account…".into();
                    let _ = joma_jami::provide_authentication(&acc, "", "");
                } else {
                    l.password_scheme = Some(scheme);
                    if l.status.is_empty() || !l.status.starts_with("wrong") {
                        l.status = "the account has a password: type it".into();
                    }
                    self.configure_inputs(cx);
                }
            }
            joma_jami::AUTH_IN_PROGRESS => l.status = "receiving the account…".into(),
            joma_jami::AUTH_DONE if error.is_empty() => {
                let acc = l.account.clone();
                self.linking = None;
                self.open_account(acc, cx);
                self.show_flash("this device is linked to your account", cx);
            }
            joma_jami::AUTH_DONE => {
                let _ = joma_jami::remove_account(&l.account);
                l.error = match error.as_str() {
                    "timeout" => "the code expired".into(),
                    "canceled" => "the other device canceled".into(),
                    "auth_error" => "the password was wrong".into(),
                    "network" => "the devices could not connect".into(),
                    e => format!("linking failed ({e})"),
                };
                l.token.clear();
            }
            _ => {}
        }
    }

    /// Our side of linking a new device: the code it shows starts it.
    fn link_device(&mut self, token: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !token.trim().starts_with("jami-auth://") {
            self.show_flash("the code starts with jami-auth://", cx);
            return;
        }
        match joma_jami::add_device(&self.model.account, token) {
            Ok(op) => {
                self.device_op = Some(DeviceOp { op, auto: false, state: 0, peer: String::new() });
                self.set_mode(Mode::Chat, window, cx);
                self.show_flash("connecting to the new device…", cx);
            }
            Err(e) => self.show_flash(format!("could not link: {e}"), cx),
        }
    }

    fn on_add_device(&mut self, op: u32, state: i64, detail: &joma_jami::Details, cx: &mut Context<Self>) {
        let Some(d) = self.device_op.as_mut().filter(|d| d.op == op) else { return };
        d.state = state;
        if let Some(p) = detail.get("peer_address") {
            d.peer = p.clone();
        }
        if state == joma_jami::AUTH_AUTHENTICATING && d.auto {
            let _ = joma_jami::confirm_add_device(&self.model.account, op);
        }
        if state == joma_jami::AUTH_DONE {
            self.device_op = None;
            let error = detail.get("error").cloned().unwrap_or_default();
            self.show_flash(if error.is_empty() { "the new device is linked".to_owned() } else { format!("linking failed ({error})") }, cx);
            self.model.devices = joma_jami::known_devices(&self.model.account).unwrap_or_default().into_iter().collect();
        }
        cx.notify();
    }

    /// The new device connected: send it the account (or not).
    pub fn answer_device(&mut self, yes: bool, cx: &mut Context<Self>) {
        let Some(d) = self.device_op.clone() else { return };
        let acc = self.model.account.clone();
        if yes {
            let _ = joma_jami::confirm_add_device(&acc, d.op);
        } else {
            let _ = joma_jami::cancel_add_device(&acc, d.op);
            self.device_op = None;
        }
        cx.notify();
    }

    pub fn revoke_device(&mut self, id: &str, cx: &mut Context<Self>) {
        let name = self.model.devices.get(id).cloned().unwrap_or_default();
        if !self.armed(&format!("revoke:{id}"), &format!("click again to revoke {name}: it loses the account"), cx) {
            return;
        }
        if let Err(e) = joma_jami::revoke_device(&self.model.account, id) {
            self.show_flash(format!("could not revoke: {e}"), cx);
        }
    }

    // ---- Accounts ----------------------------------------------------------------

    /// This machine's accounts with the name each shows.
    pub fn load_accounts(&mut self) {
        self.accounts = joma_jami::accounts()
            .unwrap_or_default()
            .into_iter()
            .map(|id| {
                let details = joma_jami::account_details(&id).unwrap_or_default();
                let volatile = joma_jami::volatile_details(&id).unwrap_or_default();
                let name = [details.get("Account.displayName"), volatile.get("Account.registeredName"), details.get("Account.username")]
                    .into_iter()
                    .flatten()
                    .find(|n| !n.is_empty())
                    .map(|n| bare_uri(n).to_owned())
                    .unwrap_or_else(|| id.clone());
                (id, name)
            })
            .collect();
    }

    /// Leave the open account for another: what was on screen goes, the daemon keeps
    /// both connected.
    fn close_account(&mut self) -> bool {
        if !self.model.calls.is_empty() {
            return false;
        }
        self.leave();
        self.my_devices.clear();
        self.device_op = None;
        self.self_told = None;
        self.frames.clear();
        self.desk_frame = None;
        self.pointer = None;
        self.selected = None;
        self.confirm_delete = None;
        true
    }

    pub fn switch_account(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if id == self.model.account {
            return;
        }
        if !self.close_account() {
            self.show_flash("hang up first", cx);
            return;
        }
        self.set_mode(Mode::Chat, window, cx);
        self.open_account(id.to_owned(), cx);
        let name = self.accounts.iter().find(|(a, _)| a == id).map(|(_, n)| n.clone()).unwrap_or_default();
        self.show_flash(format!("now on {name}"), cx);
    }

    /// The welcome screen, to create or link another account; Back returns here.
    pub fn new_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.close_account() {
            self.show_flash("hang up first", cx);
            return;
        }
        self.set_mode(Mode::Chat, window, cx);
        let had = joma_jami::accounts().unwrap_or_default();
        self.adding_account = Some((self.model.account.clone(), had));
        self.model = Model::default();
        self.onboard_name = None;
        self.gate = Gate::Onboarding;
        self.input.update(cx, |c, cx| c.clear(cx));
        self.configure_inputs(cx);
        window.focus(&self.input.focus_handle(cx));
        cx.notify();
    }

    /// Back from the welcome screen to the account that was open.
    pub fn cancel_new_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((back, _)) = self.adding_account.take() else { return };
        self.cancel_link(cx);
        self.onboard_name = None;
        self.open_account(back, cx);
        window.focus(&self.composer.focus_handle(cx));
    }

    /// Remove the open account from this machine. Kept by other devices: two clicks.
    /// Only here: it is gone for good, so the input asks for "delete".
    pub fn remove_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.devices.len() > 1 {
            if self.armed("remove-account", "click again to remove this account from this machine (your other devices keep it)", cx) {
                self.remove_account_now(window, cx);
            }
            return;
        }
        self.set_mode(Mode::RemoveAccount, window, cx);
        self.show_flash("this account is only on this device: removing it loses it for good", cx);
    }

    fn remove_account_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let acc = self.model.account.clone();
        if !self.close_account() {
            self.show_flash("hang up first", cx);
            return;
        }
        if let Err(e) = joma_jami::remove_account(&acc) {
            self.show_flash(format!("could not remove the account: {e}"), cx);
            return;
        }
        self.quick.account.clear();
        self.save_quick(cx);
        self.model = Model::default();
        match joma_jami::accounts().unwrap_or_default().into_iter().find(|a| *a != acc) {
            Some(a) => {
                self.open_account(a, cx);
                self.show_flash("account removed from this machine", cx);
            }
            None => {
                self.accounts.clear();
                self.onboard_name = None;
                self.gate = Gate::Onboarding;
                self.configure_inputs(cx);
                window.focus(&self.input.focus_handle(cx));
                self.show_flash("account removed: create or link one", cx);
            }
        }
        cx.notify();
    }

    /// Our other devices: how present they are on Joma.
    pub fn device_dot(&self, id: &str) -> Dot {
        if id == self.model.device {
            return Dot::Active;
        }
        match self.my_devices.get(id).map(|p| p.state) {
            Some(State::Active) => Dot::Active,
            Some(State::Away { .. }) => Dot::Reachable,
            _ => Dot::None,
        }
    }

    pub fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        self.input.update(cx, |c, cx| c.clear(cx));
        self.search.clear();
        self.configure_inputs(cx);
        match mode {
            Mode::Chat => window.focus(&self.composer.focus_handle(cx)),
            Mode::Removed => window.focus(&self.focus),
            _ => window.focus(&self.input.focus_handle(cx)),
        }
        cx.notify();
    }

    fn submit_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().trim().to_owned();
        if let Some(l) = self.linking.clone() {
            if let Some(scheme) = l.password_scheme {
                let _ = joma_jami::provide_authentication(&l.account, &text, &scheme);
                self.input.update(cx, |c, cx| c.clear(cx));
                if let Some(l) = self.linking.as_mut() {
                    l.status = "checking…".into();
                }
            }
            return;
        }
        if self.gate == Gate::Onboarding {
            // Two steps: the name contacts see, then an optional username.
            let Some(name) = self.onboard_name.clone() else {
                if text.is_empty() {
                    self.show_flash("type your name first", cx);
                    return;
                }
                self.onboard_name = Some(text);
                self.input.update(cx, |c, cx| c.clear(cx));
                self.configure_inputs(cx);
                cx.notify();
                return;
            };
            if !text.is_empty() {
                match valid_username(&text) {
                    Some(u) => self.pending_username = Some(u),
                    None => {
                        self.show_flash(format!("a username has {USERNAME_RULE}"), cx);
                        return;
                    }
                }
            }
            match joma_jami::create_account(&name) {
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
            Mode::Username => match valid_username(&text) {
                Some(u) => match joma_jami::register_name(&self.model.account, &u) {
                    Ok(_) => {
                        self.show_flash(format!("registering {u}…"), cx);
                        self.set_mode(Mode::Chat, window, cx);
                    }
                    Err(e) => self.show_flash(format!("could not register: {e}"), cx),
                },
                None => self.show_flash(format!("a username has {USERNAME_RULE}"), cx),
            },
            Mode::LinkDevice => self.link_device(&text, window, cx),
            Mode::DeviceName => {
                if text.is_empty() {
                    return;
                }
                match joma_jami::set_device_name(&self.model.account, &text) {
                    Ok(()) => {
                        let id = self.model.device.clone();
                        self.model.devices.insert(id, text);
                        self.set_mode(Mode::Chat, window, cx);
                    }
                    Err(e) => self.show_flash(format!("could not rename: {e}"), cx),
                }
            }
            Mode::RemoveAccount => {
                if text != "delete" {
                    self.show_flash("type delete to remove the account, or Esc", cx);
                    return;
                }
                self.set_mode(Mode::Chat, window, cx);
                self.remove_account_now(window, cx);
            }
            Mode::Chat | Mode::Removed => {}
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
    /// The share being watched: the current call's, else one without a call.
    pub fn viewing_key(&self) -> Option<String> {
        if let Some(c) = self.active_call() {
            if self.desk.viewer(&c.id).is_some() {
                return Some(c.id.clone());
            }
        }
        self.desk.direct_sessions().into_iter().find(|k| self.desk.viewer(k).is_some())
    }

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
        self.place_call_with(conv, video, false, cx)
    }

    /// Call a conversation, with the microphone off if `mic_off`; returns the call id.
    fn place_call_with(&mut self, conv: &str, video: bool, mic_off: bool, cx: &mut Context<Self>) -> Option<String> {
        let conv = self.model.conversations.get(conv).cloned()?;
        let peer = conv.peers.first().cloned()?;
        let to = if conv.peers.len() == 1 { peer.clone() } else { format!("swarm:{}", conv.id) };
        let camera = camera_source();
        let mut media = vec![joma_jami::media(joma_jami::AUDIO, "audio_0", "", mic_off)];
        if video {
            media.push(joma_jami::media(joma_jami::VIDEO, "video_0", &camera, camera.is_empty()));
        }
        match joma_jami::place_call(&self.model.account, &to, &media) {
            Ok(id) if !id.is_empty() => {
                let mut call = Call::new(&id, &peer, Some(conv.id.clone()), CallState::Connecting, video);
                call.muted_video = !video || camera.is_empty();
                call.muted_audio = mic_off;
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

    /// Start sharing the desktop in a connected call: ask what to share when the peer
    /// takes Joma's share, else go straight to Jami's (its portal asks).
    fn share_in(&mut self, call_id: &str, cx: &mut Context<Self>) {
        let Some(call) = self.model.calls.get(call_id).cloned() else { return };
        if self.desk.session(&call.id).sharing_label().is_some() {
            return;
        }
        if call.peer_joma && joma_desk::share::available() {
            let name = self.model.name_of(&call.peer);
            self.open_picker(PickFor::Call(call.id.clone()), name, cx);
            return;
        }
        let source = Source::parse("focused").unwrap_or(Source::Screen { output: String::new() });
        self.share_in_with(call_id, source, cx);
    }

    fn share_in_with(&mut self, call_id: &str, source: Source, cx: &mut Context<Self>) {
        let Some(call) = self.model.calls.get(call_id).cloned() else { return };
        if self.desk.session(&call.id).sharing_label().is_some() {
            return;
        }
        let acc = self.model.account.clone();
        let tx = self.tx.clone();
        let id = call.id.clone();
        let repaint = self.repaint();
        let after = self.desk.start(
            &acc,
            &call.id,
            &call.peer,
            call.peer_joma,
            source,
            move |ev| {
                let _ = tx.unbounded_send(AppMsg::Share(id.clone(), ev));
            },
            repaint,
        );
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
            .listed()
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
            invisible: self.quick.invisible,
            unread: self.listed_unread(),
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
            Ok(()) => {
                self.model.sent(&conv, "", Instant::now());
                Reply::Done
            }
            Err(e) => Reply::Error { message: e.to_string() },
        }
    }

    fn ipc(&mut self, req: Request, window: &mut Window, cx: &mut Context<Self>) -> Reply {
        match req {
            Request::Show => {
                raise(window);
                Reply::Done
            }
            Request::Toggle => {
                self.toggle_window(window, cx);
                Reply::Done
            }
            Request::Quick => {
                // The file can change outside Joma (edited by hand, or another tool).
                let quick = QuickConfig::load();
                if quick != self.quick {
                    self.quick = quick;
                    cx.notify();
                }
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
            Request::ShareScreen { to, source } => match self.target(&to) {
                Ok(conv) => {
                    let source = match source.as_deref().map(Source::parse).transpose() {
                        Ok(s) => s,
                        Err(e) => return Reply::Error { message: e.to_string() },
                    };
                    let running = self.model.calls.values().find(|c| c.conversation.as_deref() == Some(conv.as_str())).cloned();
                    match (running, source) {
                        (Some(c), Some(src)) if c.state == CallState::Current => self.share_in_with(&c.id, src, cx),
                        (Some(c), None) if c.state == CallState::Current => self.share_in(&c.id, cx),
                        (Some(c), _) => {
                            self.share_on_connect.insert(c.id);
                        }
                        // No call: share straight away, without one.
                        (None, Some(src)) => return self.share_direct_with(&conv, Some(src), cx),
                        (None, None) => {
                            let r = self.share_direct(&conv, cx);
                            if self.picker.is_some() {
                                raise(window);
                            }
                            return r;
                        }
                    }
                    if self.picker.is_some() {
                        raise(window);
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
            Request::Block { to, block } => {
                if self.gate != Gate::Ready {
                    return Reply::Error { message: "Joma is not connected to Jami yet".into() };
                }
                let Some(uri) = self.model.contact_named(&to) else {
                    return Reply::Error { message: format!("no contact matches “{to}”") };
                };
                if block {
                    self.block_now(&uri, cx);
                } else {
                    self.unblock(&uri, cx);
                }
                Reply::Done
            }
            Request::Avatar { path } => {
                if self.gate != Gate::Ready {
                    return Reply::Error { message: "Joma is not connected to Jami yet".into() };
                }
                match path {
                    Some(p) => self.set_avatar_file(std::path::Path::new(&p), cx),
                    None => self.remove_avatar(cx),
                }
                Reply::Done
            }
            Request::Watch { to } => match self.target(&to) {
                Ok(conv) => match self.model.conversations.get(&conv).and_then(|c| c.peers.first()).cloned() {
                    Some(peer) => {
                        let key = crate::desk::direct_key(&peer);
                        if !matches!(self.desk.session(&key).viewing, crate::desk::Viewing::Asked { .. }) {
                            return Reply::Error { message: "no desktop offered".into() };
                        }
                        self.watch_direct(&key, cx);
                        raise(window);
                        Reply::Done
                    }
                    None => Reply::Error { message: "no peer in that conversation".into() },
                },
                Err(e) => e,
            },
            Request::Link => {
                if self.gate != Gate::Onboarding {
                    return Reply::Error { message: "this Joma already has an account".into() };
                }
                if self.linking.as_ref().is_none_or(|l| !l.error.is_empty()) {
                    self.linking = None;
                    self.start_link(cx);
                }
                match &self.linking {
                    Some(l) if !l.token.is_empty() => Reply::Text {
                        text: format!("{}\n\nOn your other device: joma link-device <this code>, or Joma → Devices → paste the code.", l.token),
                    },
                    _ => Reply::Error { message: "preparing the code: run `joma link` again in a few seconds".into() },
                }
            }
            Request::LinkDevice { token } => {
                if self.gate != Gate::Ready {
                    return Reply::Error { message: "Joma is not connected to Jami yet".into() };
                }
                match joma_jami::add_device(&self.model.account, &token) {
                    Ok(op) => {
                        self.device_op = Some(DeviceOp { op, auto: true, state: 0, peer: String::new() });
                        cx.notify();
                        Reply::Done
                    }
                    Err(e) => Reply::Error { message: format!("could not link: {e}") },
                }
            }
            Request::Accounts => {
                self.load_accounts();
                let text = self
                    .accounts
                    .iter()
                    .map(|(id, name)| format!("{} {name}  {id}", if *id == self.model.account { "*" } else { " " }))
                    .collect::<Vec<_>>()
                    .join("\n");
                Reply::Text { text }
            }
            Request::UseAccount { to } => {
                self.load_accounts();
                let low = to.to_lowercase();
                let found = self.accounts.iter().find(|(id, name)| *id == to || name.to_lowercase() == low).map(|(id, _)| id.clone());
                match found {
                    Some(_) if self.gate != Gate::Ready => Reply::Error { message: "Joma is not on an account now".into() },
                    Some(id) if id == self.model.account => Reply::Done,
                    Some(_) if !self.model.calls.is_empty() => Reply::Error { message: "hang up first".into() },
                    Some(id) => {
                        self.switch_account(&id, window, cx);
                        Reply::Done
                    }
                    None => Reply::Error { message: format!("no account {to}: see joma accounts") },
                }
            }
            Request::Quit => {
                self.leave();
                cx.defer(|cx| cx.quit());
                Reply::Done
            }
            Request::Open { to } => match self.target(&to) {
                Ok(conv) => {
                    if self.mode != Mode::Chat {
                        self.set_mode(Mode::Chat, window, cx);
                    }
                    self.select(conv, cx);
                    self.narrow_chat = true;
                    raise(window);
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

    /// Put files beside the composer of the open conversation; Enter sends them.
    pub fn attach(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone().filter(|c| self.model.conversations.get(c).is_some_and(|c| !c.request)) else {
            return self.show_flash("open a conversation to send files", cx);
        };
        let list = self.attachments.entry(conv).or_default();
        for p in paths.into_iter().filter(|p| p.is_file()) {
            if !list.contains(&p) {
                list.push(p);
            }
        }
        self.focus_composer = true;
        cx.notify();
    }

    /// A pasted picture: kept as a file (Jami sends files by path) and attached.
    fn attach_image(&mut self, ext: &str, bytes: &[u8], cx: &mut Context<Self>) {
        let dir = std::env::current_dir().unwrap_or_default().join("pasted");
        let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let path = dir.join(format!("picture-{stamp}.{ext}"));
        match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, bytes)) {
            Ok(()) => self.attach(vec![path], cx),
            Err(e) => self.show_flash(format!("cannot keep the picture: {e}"), cx),
        }
    }

    pub fn unattach(&mut self, conv: &str, index: usize, cx: &mut Context<Self>) {
        if let Some(list) = self.attachments.get_mut(conv) {
            if index < list.len() {
                list.remove(index);
            }
            if list.is_empty() {
                self.attachments.remove(conv);
            }
        }
        cx.notify();
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

impl Joma {
    /// Fetch a received file into the download folder.
    pub fn download(&mut self, conv: &str, interaction: &str, file: &FileRef, cx: &mut Context<Self>) {
        if self.downloads.contains_key(&file.id) {
            return;
        }
        let dir = download_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.show_flash(format!("cannot create {}: {e}", dir.display()), cx);
            return;
        }
        let path = free_path(&dir, &file.name);
        match joma_jami::download_file(&self.model.account, conv, interaction, &file.id, &path.to_string_lossy()) {
            Ok(_) => {
                self.downloads.insert(file.id.clone(), path);
            }
            Err(e) => self.show_flash(format!("{}: {e}", file.name), cx),
        }
    }

    /// A file message was clicked: open what was downloaded, or download it.
    pub fn file_clicked(&mut self, conv: &str, interaction: &str, file: &FileRef, cx: &mut Context<Self>) {
        if let Some(p) = self.saved_file(conv, &file.id) {
            let _ = std::process::Command::new("xdg-open")
                .arg(p)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            return;
        }
        match self.downloads.get(&file.id) {
            Some(_) => self.show_flash(format!("{} is still downloading", file.name), cx),
            None => {
                self.download(conv, interaction, file, cx);
                self.show_flash(format!("downloading {}", file.name), cx);
            }
        }
    }

    /// libjami's `DataTransferEventCode`: 6 finished, 7 to 11 failed.
    fn transfer_moved(&mut self, file: &str, code: i64, cx: &mut Context<Self>) {
        let Some(path) = self.downloads.get(file).cloned() else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match code {
            6 => {
                notify_desktop("File received", &format!("{name} · in {}", path.parent().map(|p| p.display().to_string()).unwrap_or_default()));
                self.show_flash(format!("received {name}"), cx);
            }
            7..=11 => {
                self.downloads.remove(file);
                self.show_flash(format!("{name} did not arrive"), cx);
            }
            _ => {}
        }
    }
}

impl Focusable for Joma {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The camera to send, as a daemon source (`camera://<device>`), or empty without one.
pub fn camera_source() -> String {
    // A test profile never opens the real camera; `JOMA_CAMERA` gives it a source instead
    // (for example `file:///path/clip.mp4`).
    if own_profile() {
        return std::env::var("JOMA_CAMERA").unwrap_or_default();
    }
    match joma_jami::default_video_device() {
        Ok(d) if !d.is_empty() => format!("camera://{d}"),
        _ => String::new(),
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_profile_names() {
        assert_eq!(crate::avatar::base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(crate::avatar::base64_decode("YWJjZA").unwrap(), b"abcd");
    }

    #[test]
    fn usernames() {
        assert_eq!(valid_username(" @Diego_G "), Some("diego_g".into()));
        assert_eq!(valid_username("ab"), None);
        assert_eq!(valid_username("no spaces"), None);
        assert_eq!(valid_username("dé"), None);
    }
}

