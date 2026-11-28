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
        ShowKeys,
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
        TabFiles,
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
    /// A recording was stopped (its file closed): send it, or throw it away.
    Recorded { conv: String, path: String, send: bool },
    /// A screenshot of a share was saved (or not): send it to the conversation.
    Shot { conv: Option<String>, result: Result<String, String> },
    /// The release tags GitHub lists (or why not).
    Releases(Result<Vec<String>, String>),
    /// `mise` installed the update (or not).
    Updated(Result<String, String>),
    /// A voice message was transcribed (file id, the text or why not).
    Transcribed(String, Result<String, String>),
    /// A voice or video message's length, bars and still picture (by file id).
    Media(String, Option<MediaView>),
    /// The watch-together player of a conversation's session reported something.
    Watch(String, crate::watch::PlayerEvent),
    /// An `@youtube` search finished: the message to send to the conversation.
    SearchDone(String, anyhow::Result<String>),
    /// A link preview was fetched (or failed) for this video id.
    Preview(String),
    /// A contact's GitHub claim was checked: account, contact, login, and the login's id
    /// and gist (`None`: no such login).
    GithubChecked(String, String, String, Result<Option<(u64, Option<crate::github::Profile>)>, String>),
    /// "gh:<login>" in the add field: what that login lists.
    GithubFound(String, Result<Option<(u64, Option<crate::github::Profile>)>, String>),
    /// Our account was listed in (Some label) or taken off (None) our gist: account, Jami
    /// id, and our login or why not.
    GithubPublished(String, String, bool, Result<String, String>),
    /// A contact was added by GitHub name (from the command line): account, Jami id, login, id.
    GithubPin(String, String, String, u64),
    /// The GitHub sign-in has a code to show.
    GithubCode(crate::signin::After, joma_backup::device::Code),
    /// The GitHub sign-in finished: the token and our login, or why not.
    GithubSignedIn(crate::signin::After, Result<(joma_backup::device::Token, String), String>),
    /// Whether GitHub can be reached without signing in (gh, or a token).
    GithubReady(crate::signin::After, bool),
    /// An automatic backup finished.
    AutoBackupDone(Result<String, String>),
    /// Turning automatic backups on kept the password (or could not).
    AutoKept(Result<(), String>),
    /// The backup repo's accounts, for the restore screen (or why not).
    RestoreList(Result<(String, Vec<joma_backup::Entry>), String>),
    /// A backup or restore finished: was it a restore, and what to tell the user.
    BackupDone(bool, Result<String, String>),
    /// Joma's Syncthing for this account started (or not).
    SyncStarted(String, Result<joma_sync::Syncthing, String>),
    /// A fresh sync report for the devices tab.
    SyncView(String, crate::sync::Report),
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

/// The settings panel: what the daemon reported when it opened.
#[derive(Clone, Default)]
pub struct SettingsView {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    /// [output, input, ringtone] device numbers.
    pub current: Vec<usize>,
    pub noise: String,
    pub echo: String,
    pub agc: bool,
    pub cameras: Vec<String>,
    pub camera: String,
    /// The default camera's sizes (largest first) and, per size, its rates.
    pub sizes: Vec<(String, Vec<String>)>,
    pub camera_settings: joma_jami::Details,
    /// The section shown: 1 audio, 2 video, 3 desktop share, 4 about (colours, font and
    /// corners come from Omarchy, so there is no interface section).
    pub section: usize,
}

/// Video bitrates for the quality presets, in kbit/s.
pub fn quality_bitrate(q: &str) -> Option<u32> {
    match q {
        "low" => Some(500),
        "medium" => Some(1500),
        "high" => Some(4000),
        _ => None,
    }
}

/// A voice or video message being recorded.
pub struct Recording {
    pub conv: String,
    /// The file the daemon writes (with its extension).
    pub path: String,
    pub started: Instant,
    pub video: bool,
}


/// What the pointer does on a shared screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InkTool {
    /// Zoom and pan (the default).
    None,
    /// Our pointer shows on their screen as a red dot.
    Laser,
    /// Drag to draw; the strokes stay until cleared.
    Pen,
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
    /// The input takes the backup password (`joma backup` / `joma restore`), masked.
    Password,
    /// The input takes a contact or conversation to share a folder with.
    ShareWith,
}

/// A backup or restore waiting for its password in the window.
pub struct PasswordAsk {
    pub restore: bool,
    /// Backup: the accounts; restore: the wanted labels or ids.
    pub accounts: Vec<crate::backup::Account>,
    pub wanted: Vec<String>,
    pub file: Option<String>,
    /// Backup: the first entry, waiting to be typed again.
    pub first: Option<String>,
    /// Turning automatic backups on: keep the password in the keyring once it worked.
    pub remember: bool,
    pub reply: std::sync::mpsc::Sender<Reply>,
}

/// Restoring accounts from the GitHub backup on the welcome screen (D-24 step 2).
#[derive(Clone, Default)]
pub struct RestoreView {
    /// Reading the backup repo.
    pub loading: bool,
    pub login: String,
    pub entries: Vec<joma_backup::Entry>,
    /// The Jami ids to restore.
    pub picked: HashSet<String>,
    /// Restoring now (after the password).
    pub working: bool,
    pub error: String,
    /// Started by `joma restore`: where from (a file, or GitHub); the command gets the answer.
    pub cli: Option<String>,
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

/// A menu that drops from a button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    /// Ours: profile, invisible, username, to the tray.
    Me,
    /// The open conversation's: star, quick messages, send a file, remove.
    Chat,
}

/// What the sidebar lists (the tab strip on top picks it).
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
    /// Synced folders and their files (discussion 2026-11-27-files).
    Files,
}

/// What the files view shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesKey {
    /// A Syncthing folder (ours, or shared).
    Folder(String),
    /// A conversation's files: every member's folder.
    Conversation(String),
    /// Files received in conversations (the download folder).
    Received,
}

impl FilesKey {
    pub fn id(&self) -> String {
        match self {
            FilesKey::Folder(f) => format!("f-{f}"),
            FilesKey::Conversation(c) => format!("c-{c}"),
            FilesKey::Received => "received".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: std::path::PathBuf,
    pub dir: bool,
    pub size: u64,
    pub modified: i64,
    /// Syncthing's copies of this file from a conflict (two devices changed it at once).
    pub conflicts: Vec<std::path::PathBuf>,
}

/// `report.sync-conflict-20261128-031500-ABCDEFG.pdf` → `report.pdf`.
pub fn conflict_of(name: &str) -> Option<String> {
    let i = name.find(".sync-conflict-")?;
    let rest = &name[i + ".sync-conflict-".len()..];
    let ext = rest.split_once('.').map(|(_, e)| format!(".{e}")).unwrap_or_default();
    Some(format!("{}{ext}", &name[..i]))
}

#[derive(Debug, Clone, Default)]
pub struct FilesView {
    pub key: Option<FilesKey>,
    /// The folder shown (inside the chosen one); None: its top.
    pub dir: Option<std::path::PathBuf>,
    pub entries: Vec<FileEntry>,
    pub picked: Option<std::path::PathBuf>,
    /// The narrow window shows the folder instead of the list.
    pub open: bool,
    pub read_at: Option<Instant>,
    /// The folder `share…` asks a name for.
    pub share_for: Option<String>,
}

/// Into the desktop's trash (`gio trash`), so a deletion can be undone.
pub fn trash(path: &std::path::Path) -> std::io::Result<()> {
    let out = std::process::Command::new("gio").arg("trash").arg(path).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_owned()))
    }
}

/// Folders first, then by name; dot files hidden (Syncthing's own among them).
pub fn read_dir(dir: &std::path::Path) -> Vec<FileEntry> {
    let mut out: Vec<FileEntry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let modified = m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
            Some(FileEntry { name: e.file_name().to_string_lossy().into_owned(), path: e.path(), dir: m.is_dir(), size: m.len(), modified, conflicts: Vec::new() })
        })
        .collect();
    // Conflict copies go under the file they belong to.
    let copies: Vec<(String, std::path::PathBuf)> = out.iter().filter_map(|e| Some((conflict_of(&e.name)?, e.path.clone()))).collect();
    out.retain(|e| conflict_of(&e.name).is_none() || !copies.iter().any(|(orig, p)| *p == e.path && dir.join(orig).exists()));
    for (orig, p) in copies {
        if let Some(e) = out.iter_mut().find(|e| e.name == orig) {
            e.conflicts.push(p);
        }
    }
    out.sort_by(|a, b| (!a.dir, a.name.to_lowercase()).cmp(&(!b.dir, b.name.to_lowercase())));
    out
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
/// What a voice or video message shows before it plays.
#[derive(Clone)]
pub struct MediaView {
    pub info: crate::player::Info,
    pub bars: Vec<f32>,
    pub thumb: Option<std::path::PathBuf>,
}

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
    /// The settings panel, when open.
    pub settings: Option<SettingsView>,
    pub recording: Option<Recording>,
    /// Watching together, by conversation id.
    /// Watching together, by conversation.
    pub watches: HashMap<String, crate::watch::Session>,
    pub ink_tool: InkTool,
    pub ink_color: u32,
    /// The stroke being drawn, and the last laser message's time.
    pub ink_stroke: Option<u32>,
    ink_next: u32,
    pub ink_laser_at: Option<Instant>,
    /// The voice or video message playing or paused in the chat.
    pub player: Option<crate::player::Player>,
    /// Its latest picture, for the chat.
    pub player_frame: Option<Frame>,
    /// The playing video fills the window (Telegram's corner button).
    pub video_big: bool,
    /// Voice and video messages measured (None: being measured or failed).
    pub media: HashMap<String, Option<MediaView>>,
    /// Subtitles of voice and video messages, by file id.
    pub cues: HashMap<String, Vec<crate::media::Cue>>,
    pub emoji_open: bool,
    /// Link previews by video id (None: being fetched or failed).
    pub previews_yt: HashMap<String, Option<crate::media::Preview>>,
    /// Shares that took place (Calls tab), newest last.
    pub share_log: Vec<ShareLog>,
    /// Shares running now: session key → (started, unix time, we share).
    running_shares: HashMap<String, (Instant, i64, bool)>,
    picker_gen: u64,
    /// The sharer's preview of each running Joma share, by session key.
    pub previews: HashMap<String, Frame>,
    pub flash: Option<(String, Instant)>,
    /// The menu that is open: ours (the picture in the tab strip) or the conversation's.
    pub menu: Option<Menu>,
    /// The GIF picker: the folder's GIFs, read when it opens.
    pub gifs: Option<Vec<std::path::PathBuf>>,
    /// Transcripts of voice messages by file id: `None` while one is being made.
    pub transcripts: HashMap<String, Option<Result<String, String>>>,
    transcripts_seen: std::collections::HashSet<String>,
    /// When this run started: only messages after it are transcribed by themselves (the
    /// older ones on demand, user 2026-11-27).
    started_ts: i64,
    /// Transcripts shown whole (the long ones are folded).
    pub transcript_open: std::collections::HashSet<String>,
    /// Videos whose subtitles are on (the cc button).
    pub subs_on: std::collections::HashSet<String>,
    /// Names of voice and video messages sent from this device.
    sent_media: std::collections::HashSet<String>,
    /// Transcripts to attach once made: file id → (conversation, message).
    ship: HashMap<String, (String, String)>,
    /// Voice messages already fetched or transcribed by themselves (once each).
    voice_fetched: std::collections::HashSet<String>,
    voice_transcribed: std::collections::HashSet<String>,
    /// A voice message to play once it has arrived.
    pub play_after: Option<String>,
    /// The picture shown large, over the window.
    pub viewer: Option<std::path::PathBuf>,
    /// The message whose actions show (reply, react, edit, delete).
    pub msg_actions: Option<String>,
    /// The message the next one answers, or the one being edited.
    pub replying: Option<String>,
    pub editing: Option<String>,
    /// A newer version GitHub has, for the channel chosen.
    pub update: Option<String>,
    pub updating: bool,
    update_checked: Option<Instant>,
    booted: Instant,
    update_told: Option<String>,
    /// The last check failed, or found nothing newer (for the About page).
    pub update_note: String,
    /// The overlay listing the keys (F1).
    pub keys_open: bool,
    /// The message box has the keyboard (its field shows it).
    pub composer_focused: bool,
    /// The window is wide enough for the tabs' names.
    /// How wide a bubble may get: three quarters of the conversation (GPUI's text does
    /// not wrap under a percentage width).
    pub bubble_max: f32,
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
    /// The connection-info view open under a contact (`peer:<uri>`) or a device
    /// (`device:<id>`), with the last rows read (None while reading).
    pub conn_info: Option<(String, Option<Vec<crate::conninfo::Row>>)>,
    /// Folder sync with our other devices.
    pub sync: crate::sync::Sync,
    /// A backup or restore waiting for its password.
    pub password_ask: Option<PasswordAsk>,
    /// The welcome screen's restore from the backup.
    pub restoring: Option<RestoreView>,
    /// GitHub badges and pinned contacts of the open account.
    pub gh: crate::github::Store,
    /// Contacts being checked against GitHub now.
    pub gh_checking: HashSet<String>,
    /// "gh:<login>" in the add field: the login, its id and what its gist lists.
    pub gh_found: Option<(String, u64, Vec<crate::github::Listed>)>,
    /// Our gist is being written.
    pub gh_busy: bool,
    /// Signing in with the Joma GitHub App: the card with the code.
    pub gh_signin: Option<crate::signin::SignIn>,
    /// The app's token for this run (also given to `joma_backup::gh`).
    pub gh_token: Option<joma_backup::device::Token>,
    gh_cancel: Arc<std::sync::atomic::AtomicBool>,
    /// Automatic backups (off unless turned on) and when the last backup worked.
    pub auto: crate::signin::Auto,
    auto_running: bool,
    /// Devices that joined the account without this machine linking them: keep / revoke.
    pub new_devices: Vec<String>,
    /// When this machine last linked a device, or an account arrived here: devices that
    /// appear in the next minutes are not "new".
    pub last_link: Option<Instant>,
    /// The devices tab's last sync report, refreshed in the background.
    pub sync_view: Option<crate::sync::Report>,
    sync_view_busy: bool,
    sync_view_at: Option<Instant>,
    /// The files view.
    pub files: FilesView,
    /// A restore brought back this account's sync state: offered in the files view.
    pub restored_sync: Option<crate::sync::Saved>,
    /// "set them up again" was chosen: done once Syncthing runs.
    restored_sync_go: bool,
    /// When the restored sync state was last looked for (a restore writes it after the
    /// account opens).
    restored_seen: Option<Instant>,
    /// When our devices last heard the theme (again every half hour: a phone that was
    /// off misses account messages).
    theme_told: Option<Instant>,
    /// Our other devices' transcription roles, by Jami device id.
    pub transcribers: HashMap<String, TranscriberInfo>,
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

/// A voice or video message, by its name.
pub fn media_name(name: &str) -> bool {
    let p = std::path::Path::new(name);
    crate::media::is_audio(p) || crate::media::is_video(p)
}

/// Received files up to this size download by themselves (D-9: 200 MB; the Jami app uses 20 MB).
const AUTO_DOWNLOAD: u64 = 200 * 1024 * 1024;

/// Where received files go: `~/Downloads/joma` (the XDG download dir, if set).
/// The Omarchy theme in use, to our own devices (joma for Android follows it).
pub const THEME_MIME: &str = "application/x-joma-theme+json";

/// What a device of ours transcribes (discussion 2026-11-27-shared-transcription, Q3).
pub const ROLE_MIME: &str = "application/x-joma-transcriber+json";

/// Another of our devices, as it describes its transcription.
#[derive(Debug, Clone)]
pub struct TranscriberInfo {
    pub name: String,
    /// "" for itself, "all", "off".
    pub role: String,
    pub engine: String,
    pub heard: Instant,
}

pub fn download_dir() -> std::path::PathBuf {
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
pub fn own_profile() -> bool {
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
                    if j.model.calls.values().any(|c| !c.sinks.is_empty()) || j.player.as_ref().is_some_and(|p| p.playing()) {
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
            settings: None,
            recording: None,
            watches: HashMap::new(),
            ink_tool: InkTool::None,
            ink_color: 0xff3355,
            ink_stroke: None,
            ink_next: 1,
            ink_laser_at: None,
            player: None,
            player_frame: None,
            video_big: false,
            media: HashMap::new(),
            cues: HashMap::new(),
            emoji_open: false,
            previews_yt: HashMap::new(),
            share_log: Vec::new(),
            running_shares: HashMap::new(),
            picker_gen: 0,
            previews: HashMap::new(),
            flash: None,
            menu: None,
            gifs: None,
            transcripts: HashMap::new(),
            transcripts_seen: std::collections::HashSet::new(),
            started_ts: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64),
            transcript_open: std::collections::HashSet::new(),
            subs_on: std::collections::HashSet::new(),
            sent_media: std::collections::HashSet::new(),
            ship: HashMap::new(),
            voice_fetched: std::collections::HashSet::new(),
            voice_transcribed: std::collections::HashSet::new(),
            play_after: None,
            viewer: None,
            msg_actions: None,
            replying: None,
            editing: None,
            update: None,
            updating: false,
            update_checked: None,
            booted: Instant::now(),
            update_told: None,
            update_note: String::new(),
            keys_open: false,
            composer_focused: false,
            bubble_max: 480.,
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
            conn_info: None,
            sync: Default::default(),
            password_ask: None,
            restoring: None,
            gh: Default::default(),
            gh_checking: HashSet::new(),
            gh_found: None,
            gh_busy: false,
            gh_signin: None,
            gh_token: None,
            gh_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            auto: crate::signin::Auto::load(),
            auto_running: false,
            new_devices: Vec::new(),
            last_link: None,
            sync_view: None,
            sync_view_busy: false,
            sync_view_at: None,
            files: FilesView::default(),
            restored_sync: None,
            restored_sync_go: false,
            restored_seen: None,
            theme_told: None,
            transcribers: HashMap::new(),
            my_devices: HashMap::new(),
            self_told: None,
            accounts: Vec::new(),
            adding_account: None,
        };
        joma_desk::share::set_max_fps(j.quick.share_fps);
        j.composer.update(cx, |c, _| c.set_bare(true));
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
        self.new_devices.clear();
        self.gh = crate::github::Store::load(&account);
        self.sync.store = crate::shares::Store::load(&account);
        self.restored_sync = None;
        self.restored_sync_go = false;
        self.restored_seen = None;
        self.gh_found = None;
        if self.restoring.as_ref().is_some_and(|r| r.working) {
            self.restoring = None;
        }
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
        if self.quick.video_quality != "auto" && !self.quick.video_quality.is_empty() {
            let q = self.quick.video_quality.clone();
            self.apply_video_quality(&q);
        }
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
        self.theme_told = None;
        if crate::sync::is_on(&self.model.account) {
            self.start_sync();
        }
    }

    /// Start Joma's Syncthing for this account in the background.
    pub fn start_sync(&mut self) {
        if self.sync.st.is_some() || self.sync.starting {
            return;
        }
        self.sync.starting = true;
        self.sync.error = None;
        let (tx, acc) = (self.tx.clone(), self.model.account.clone());
        std::thread::spawn(move || {
            let r = crate::sync::start_syncthing(&acc);
            let _ = tx.unbounded_send(AppMsg::SyncStarted(acc, r));
        });
    }

    /// The files view's "set them up again": sync on, then the folders and shares the
    /// backup kept.
    pub fn restore_sync(&mut self, yes: bool, cx: &mut Context<Self>) {
        if !yes {
            self.restored_sync = None;
            if let Some(p) = crate::sync::restored_path(&self.model.me) {
                let _ = std::fs::remove_file(p);
            }
            return cx.notify();
        }
        self.restored_sync_go = true;
        if self.sync.st.is_some() {
            self.apply_restored_sync(cx);
        } else {
            let acc = self.model.account.clone();
            let _ = crate::sync::set_on(&acc, true);
            self.start_sync();
            self.show_flash("starting sync, then your folders come back…", cx);
        }
        cx.notify();
    }

    fn apply_restored_sync(&mut self, cx: &mut Context<Self>) {
        self.restored_sync_go = false;
        let Some(saved) = self.restored_sync.take() else { return };
        let name = self.model.devices.get(&self.model.device).cloned().unwrap_or_default();
        let (acc, me, dev) = (self.model.account.clone(), self.model.me.clone(), self.model.device.clone());
        match self.sync.apply_saved(&acc, &me, &dev, &name, &saved) {
            Ok(n) => {
                self.show_flash(format!("{n} synced folders set up again; they fill from your other devices and contacts"), cx);
                if let Some(p) = crate::sync::restored_path(&me) {
                    let _ = std::fs::remove_file(p);
                }
            }
            Err(e) => {
                self.restored_sync = Some(saved);
                self.show_flash(format!("could not set the folders up again: {e:#}"), cx);
            }
        }
        self.refresh_sync_view();
        cx.notify();
    }

    /// A message about folders shared with contacts.
    fn sync_wire(&mut self, from: String, w: crate::shares::Wire, cx: &mut Context<Self>) {
        let name = self.model.devices.get(&self.model.device).cloned().unwrap_or_default();
        let (acc, me, dev) = (self.model.account.clone(), self.model.me.clone(), self.model.device.clone());
        match self.sync.heard_wire(&acc, &me, &dev, &name, &from, w) {
            crate::sync::Heard::Nothing => {}
            crate::sync::Heard::Offer(label) => {
                let who = self.model.name_of(&from);
                self.show_flash(format!("{who} shares a folder: {label} (files, or the conversation)"), cx);
                notify_desktop(&format!("{who} shares a folder"), &format!("{label}: join it in Joma"));
            }
            crate::sync::Heard::AutoJoin(folder) => {
                if let Err(e) = self.join_share(&folder, None) {
                    self.show_flash(format!("could not join the conversation's new folder: {e}"), cx);
                }
            }
            crate::sync::Heard::Changed(text) => self.show_flash(text, cx),
        }
        cx.notify();
    }

    /// A folder by its id, label, path or conversation (for the command line).
    fn files_key(&self, name: &str) -> Option<FilesKey> {
        if name == "received" {
            return Some(FilesKey::Received);
        }
        let low = name.to_lowercase();
        let folders = self.sync.st.as_ref().and_then(|s| s.folders().ok()).unwrap_or_default();
        if let Some(f) = folders.iter().find(|f| f.id == name || f.label.to_lowercase() == low || f.path == name) {
            return Some(FilesKey::Folder(f.id.clone()));
        }
        self.sync.store.shares.values().filter_map(|s| s.conv.clone()).find(|c| *c == name || self.conv_title(c).to_lowercase() == low).map(FilesKey::Conversation)
    }

    fn files_request(&mut self, what: &str, arg: &str, path: Option<&str>, json: bool, cx: &mut Context<Self>) -> Reply {
        match what {
            "list" => {
                let r = self.sync_report();
                let me = self.model.me.clone();
                let mut rows = Vec::new();
                for f in &r.folders {
                    let s = r.shares.iter().find(|s| s.id == f.id);
                    let (kind, with) = match s {
                        None => ("yours", "your devices".to_owned()),
                        Some(s) if s.conv.is_some() => ("conversation", self.conv_title(s.conv.as_deref().unwrap_or(""))),
                        Some(s) => ("shared", s.others(&me).map(|u| self.model.name_of(u)).collect::<Vec<_>>().join(", ")),
                    };
                    let access = if f.kind == "receiveonly" { "read" } else { "write" };
                    rows.push(serde_json::json!({"id": f.id, "label": f.label, "path": f.path, "kind": kind, "with": with, "you": access, "state": f.state, "files": f.files, "bytes": f.bytes, "need_files": f.need_files}));
                }
                let offers: Vec<serde_json::Value> = r.offers.iter().map(|o| serde_json::json!({"id": o.folder, "label": o.label, "from": self.model.name_of(&o.owner), "conversation": o.conv})).collect();
                if json {
                    return Reply::Text { text: serde_json::json!({"folders": rows, "offered": offers, "received": download_dir()}).to_string() };
                }
                let mut out: Vec<String> = rows.iter().map(|r| format!("{}  {} ({}, {}; you {})  {} · {} files", r["id"].as_str().unwrap_or(""), r["label"].as_str().unwrap_or(""), r["kind"].as_str().unwrap_or(""), r["with"].as_str().unwrap_or(""), r["you"].as_str().unwrap_or(""), r["state"].as_str().unwrap_or(""), r["files"])).collect();
                if out.is_empty() {
                    out.push(if r.on { "no synced folders".into() } else { "sync is off (joma sync on)".into() });
                }
                for o in &offers {
                    out.push(format!("offered: {}  {} from {} (joma sync join {})", o["id"].as_str().unwrap_or(""), o["label"].as_str().unwrap_or(""), o["from"].as_str().unwrap_or(""), o["id"].as_str().unwrap_or("")));
                }
                out.push(format!("received: {}", download_dir().display()));
                Reply::Text { text: out.join("\n") }
            }
            "ls" | "show" => {
                let Some(key) = self.files_key(arg) else { return Reply::Error { message: format!("no folder {arg} (joma files)") } };
                if what == "show" {
                    self.tab = Tab::Files;
                    self.refresh_sync_view();
                    self.files_open(key, cx);
                    return Reply::Done;
                }
                let saved = (self.files.key.clone(), self.files.dir.clone());
                self.files.key = Some(key);
                let root = self.files_root();
                self.files.key = saved.0;
                self.files.dir = saved.1;
                let Some(root) = root else { return Reply::Error { message: "that folder is not on this machine".into() } };
                let dir = path.map(|p| root.join(p)).unwrap_or(root.clone());
                if !dir.starts_with(&root) || !dir.is_dir() {
                    return Reply::Error { message: format!("{} is not a folder in it", dir.display()) };
                }
                let entries = read_dir(&dir);
                if json {
                    let v: Vec<serde_json::Value> = entries.iter().map(|e| serde_json::json!({"name": e.name, "path": e.path, "dir": e.dir, "size": e.size, "modified": e.modified})).collect();
                    return Reply::Text { text: serde_json::Value::Array(v).to_string() };
                }
                Reply::Text { text: if entries.is_empty() { "empty".into() } else { entries.iter().map(|e| if e.dir { format!("{}/", e.name) } else { format!("{}  {}", e.name, crate::sync::size(e.size)) }).collect::<Vec<_>>().join("\n") } }
            }
            "open" => {
                let p = std::path::PathBuf::from(arg);
                if !p.exists() {
                    return Reply::Error { message: format!("{arg} does not exist") };
                }
                if own_profile() {
                    return Reply::Text { text: format!("would open {arg} (skipped in a test profile)") };
                }
                Joma::open_outside(&p);
                Reply::Done
            }
            other => Reply::Error { message: format!("joma files: unknown {other}") },
        }
    }

    pub fn conv_title_of(&self, conv: &str) -> String {
        self.conv_title(conv)
    }

    /// Show a folder in the files view.
    pub fn files_open(&mut self, key: FilesKey, cx: &mut Context<Self>) {
        if self.files.key.as_ref() != Some(&key) {
            self.files.dir = None;
            self.files.picked = None;
        }
        self.files.key = Some(key);
        self.files.open = true;
        self.files_read();
        cx.notify();
    }

    /// j/k (or the arrows, Alt+J/K) in the files view: the next or previous row.
    pub fn files_step(&mut self, by: i32, cx: &mut Context<Self>) {
        let n = self.files.entries.len() as i32;
        if n == 0 {
            return;
        }
        let at = self.files.picked.as_ref().and_then(|p| self.files.entries.iter().position(|e| e.path == *p)).map_or(if by > 0 { -1 } else { n }, |i| i as i32);
        let next = (at + by).clamp(0, n - 1) as usize;
        self.files.picked = Some(self.files.entries[next].path.clone());
        cx.notify();
    }

    /// The files view's keys (discussion 2026-11-27-files, question 4): j/k and the
    /// arrows move, Enter opens (a folder: goes in), Space shows a picture in Joma,
    /// Backspace goes up, Delete moves to the trash (asking first).
    pub fn files_keypress(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let picked = self.files.picked.clone();
        match key {
            "j" | "down" => self.files_step(1, cx),
            "k" | "up" => self.files_step(-1, cx),
            "enter" => match picked {
                Some(p) if p.is_dir() => self.files_enter(&p, cx),
                Some(p) => self.files_activate(&p, cx),
                None => return false,
            },
            "space" => match picked {
                Some(p) if crate::render::is_picture(&p) => {
                    self.viewer = Some(p);
                    cx.notify();
                }
                _ => return false,
            },
            "backspace" => self.files_up(cx),
            "delete" => match picked {
                Some(p) if p.is_file() => self.files_delete(&p, cx),
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    pub fn files_close(&mut self, cx: &mut Context<Self>) {
        self.files.open = false;
        cx.notify();
    }

    /// The chosen folder's top on this machine.
    pub fn files_root(&self) -> Option<std::path::PathBuf> {
        let r = self.sync_view.as_ref();
        match self.files.key.as_ref()? {
            FilesKey::Received => Some(download_dir()),
            FilesKey::Folder(id) => r
                .and_then(|r| r.folders.iter().find(|f| f.id == *id).map(|f| std::path::PathBuf::from(&f.path)))
                .or_else(|| self.sync.store.shares.get(id).map(|s| std::path::PathBuf::from(&s.path)))
                .or_else(|| self.sync.st.as_ref()?.folders().ok()?.into_iter().find(|f| f.id == *id).map(|f| std::path::PathBuf::from(f.path))),
            FilesKey::Conversation(c) => self.sync.store.of_conv(c).next().and_then(|s| std::path::Path::new(&s.path).parent().map(|p| p.to_path_buf())),
        }
    }

    /// Read the folder shown (on opening it and now and then).
    pub fn files_read(&mut self) {
        let Some(root) = self.files_root() else {
            self.files.entries.clear();
            return;
        };
        let dir = self.files.dir.clone().filter(|d| d.starts_with(&root)).unwrap_or(root);
        self.files.entries = read_dir(&dir);
        self.files.read_at = Some(Instant::now());
    }

    pub fn files_enter(&mut self, dir: &std::path::Path, cx: &mut Context<Self>) {
        self.files.dir = Some(dir.to_path_buf());
        self.files.picked = None;
        self.files_read();
        cx.notify();
    }

    pub fn files_up(&mut self, cx: &mut Context<Self>) {
        if let Some(d) = self.files.dir.take() {
            let root = self.files_root();
            self.files.dir = d.parent().map(|p| p.to_path_buf()).filter(|p| root.as_ref().is_some_and(|r| p.starts_with(r) && p != r));
        }
        self.files_read();
        cx.notify();
    }

    /// Open a file: pictures in Joma's viewer, the rest with the system's app.
    pub fn files_activate(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if crate::render::is_picture(path) {
            self.viewer = Some(path.to_path_buf());
            return cx.notify();
        }
        self.open_outside_checked(path, cx);
    }

    /// xdg-open, but never on the real desktop from a test profile.
    pub fn open_outside_checked(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        if own_profile() {
            return self.show_flash(format!("would open {} (skipped in a test profile)", path.display()), cx);
        }
        Joma::open_outside(path);
    }

    /// Files dropped on the folder shown: copied in.
    pub fn files_drop(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let Some(root) = self.files_root() else { return };
        let dir = self.files.dir.clone().unwrap_or(root);
        // Copied off the UI thread (large files take a while); the list shows them as they land.
        let jobs: Vec<(std::path::PathBuf, std::path::PathBuf)> = paths
            .iter()
            .filter(|p| p.is_file())
            .map(|p| (p.clone(), crate::shares::free_name(&dir, &p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into()))))
            .collect();
        let n = jobs.len();
        std::thread::spawn(move || {
            for (from, to) in jobs {
                if let Err(e) = std::fs::copy(&from, &to) {
                    tracing::warn!(error = %e, to = %to.display(), "could not copy a dropped file");
                }
            }
        });
        if n > 0 {
            self.show_flash(format!("copying {n} {} into {}", if n == 1 { "file" } else { "files" }, dir.display()), cx);
        }
        cx.notify();
    }

    /// A conflict: keep this file (the other copy goes), or the other (it replaces this).
    pub fn resolve_conflict(&mut self, file: &std::path::Path, copy: &std::path::Path, keep_other: bool, cx: &mut Context<Self>) {
        let r = if keep_other { std::fs::rename(copy, file) } else { trash(copy) };
        if let Err(e) = r {
            self.show_flash(format!("{e}"), cx);
        }
        self.files_read();
        cx.notify();
    }

    /// Delete a file of the folder shown: into the trash, after a second click (it goes
    /// from every device that syncs the folder).
    pub fn files_delete(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        let devices = self.sync_view.as_ref().map_or(0, |r| r.devices.len());
        let ask = if matches!(self.files.key, Some(FilesKey::Received)) {
            "click again: into the trash".to_owned()
        } else {
            format!("click again: into the trash here, and gone from the folder on the {devices} other devices that sync it")
        };
        if !self.armed(&format!("del:{}", path.display()), &ask, cx) {
            return;
        }
        match trash(path) {
            Ok(()) => self.show_flash(format!("{} is in the trash", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), cx),
            Err(e) => self.show_flash(format!("{e}"), cx),
        }
        self.files.picked = None;
        self.files_read();
        cx.notify();
    }

    pub fn ask_share_with(&mut self, folder: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.files.share_for = Some(folder.to_owned());
        self.set_mode(Mode::ShareWith, window, cx);
    }

    pub fn leave_share_ui(&mut self, folder: &str, cx: &mut Context<Self>) {
        if !self.armed(&format!("leave:{folder}"), "click again to leave the folder (its files stay here)", cx) {
            return;
        }
        let (acc, me) = (self.model.account.clone(), self.model.me.clone());
        match self.sync.leave(&acc, &me, folder) {
            Ok(()) => {
                self.files.key = None;
                self.show_flash("left; the files stay where they are", cx);
            }
            Err(e) => self.show_flash(format!("{e:#}"), cx),
        }
        self.refresh_sync_view();
        cx.notify();
    }

    pub fn revert_folder(&mut self, folder: &str, cx: &mut Context<Self>) {
        let r = self.sync.st.as_ref().map(|s| s.revert(folder));
        match r {
            Some(Ok(())) => self.show_flash("your changes are undone: the shared files come back", cx),
            Some(Err(e)) => self.show_flash(format!("{e:#}"), cx),
            None => {}
        }
        self.refresh_sync_view();
    }

    pub fn set_member_write(&mut self, folder: &str, uri: &str, write: bool, cx: &mut Context<Self>) {
        let (acc, me) = (self.model.account.clone(), self.model.me.clone());
        if let Err(e) = self.sync.set_write(&acc, &me, folder, uri, write) {
            self.show_flash(format!("{e:#}"), cx);
        }
        self.refresh_sync_view();
        cx.notify();
    }

    pub fn set_member_admin(&mut self, folder: &str, uri: &str, admin: bool, cx: &mut Context<Self>) {
        let (acc, me) = (self.model.account.clone(), self.model.me.clone());
        if let Err(e) = self.sync.set_admin(&acc, &me, folder, uri, admin) {
            self.show_flash(format!("{e:#}"), cx);
        }
        self.refresh_sync_view();
        cx.notify();
    }

    pub fn set_folder_device(&mut self, folder: &str, device_st: &str, on: bool, cx: &mut Context<Self>) {
        let acc = self.model.account.clone();
        if let Err(e) = self.sync.set_device_on(&acc, folder, device_st, on) {
            self.show_flash(format!("{e:#}"), cx);
        }
        self.refresh_sync_view();
        cx.notify();
    }

    /// The title of a conversation (for its files folder).
    fn conv_title(&self, conv: &str) -> String {
        self.model.conversations.get(conv).map(|c| c.title.clone()).unwrap_or_else(|| conv[..conv.len().min(8)].to_owned())
    }

    /// Join an offered folder (`path`: where; default under `~/joma-sync/`).
    pub fn join_share(&mut self, folder: &str, path: Option<std::path::PathBuf>) -> Result<crate::shares::Share, String> {
        let name = self.model.devices.get(&self.model.device).cloned().unwrap_or_default();
        let (acc, me, dev) = (self.model.account.clone(), self.model.me.clone(), self.model.device.clone());
        let conv = self.sync.store.offers.get(folder).and_then(|o| o.conv.clone());
        let title = conv.as_deref().map(|c| self.conv_title(c)).unwrap_or_default();
        let s = self.sync.join(&acc, &me, &dev, &name, folder, path, &title).map_err(|e| format!("{e:#}"))?;
        // A conversation's other folders already offered join with it.
        if let Some(c) = &conv {
            let more: Vec<String> = self.sync.store.offers.values().filter(|o| o.conv.as_ref() == Some(c)).map(|o| o.folder.clone()).collect();
            for f in more {
                let _ = self.sync.join(&acc, &me, &dev, &name, &f, None, &title);
            }
        }
        Ok(s)
    }

    pub fn join_share_ui(&mut self, folder: &str, cx: &mut Context<Self>) {
        match self.join_share(folder, None) {
            Ok(s) => self.show_flash(format!("{} joins your synced folders: {}", s.label, s.path), cx),
            Err(e) => self.show_flash(format!("not joined: {e}"), cx),
        }
        self.refresh_sync_view();
        cx.notify();
    }

    pub fn decline_share(&mut self, folder: &str, cx: &mut Context<Self>) {
        let acc = self.model.account.clone();
        self.sync.decline(&acc, folder);
        cx.notify();
    }

    /// A file sent in a conversation also goes into the conversation's shared folder
    /// (ours, in the sub-folder for its kind), when sync is on. The first one makes the
    /// folder and offers it to the members.
    fn share_conv_file(&mut self, conv: &str, file: &std::path::Path, message: bool) -> Result<Option<std::path::PathBuf>, String> {
        if self.sync.st.is_none() {
            return Ok(None);
        }
        let Some(c) = self.model.conversations.get(conv) else { return Ok(None) };
        if c.request || c.peers.is_empty() {
            return Ok(None);
        }
        let me = self.model.me.clone();
        let my_name = if self.model.my_name.is_empty() { self.model.username.clone() } else { self.model.my_name.clone() };
        let share = match self.sync.store.mine_in(conv, &me).cloned() {
            Some(s) => s,
            None => {
                let title = c.title.clone();
                let members: Vec<crate::shares::Member> = c.peers.iter().filter(|p| **p != me).map(|p| crate::shares::Member { uri: p.clone(), name: self.model.name_of(p), ..Default::default() }).collect();
                let path = crate::shares::conv_dir(&title, if my_name.is_empty() { "me" } else { &my_name });
                for cat in crate::shares::CATEGORIES {
                    std::fs::create_dir_all(path.join(cat)).map_err(|e| format!("{}: {e}", path.display()))?;
                }
                let acc = self.model.account.clone();
                let id = crate::shares::conv_folder_id(conv, &me);
                // Sharing our files in a conversation takes the others' files too.
                self.sync.store.convs.insert(conv.to_owned());
                self.sync
                    .create_share(&acc, &me, &my_name, Some(id), &format!("{}'s files", if my_name.is_empty() { "me" } else { &my_name }), &path, Some(conv.to_owned()), members)
                    .map_err(|e| format!("{e:#}"))?
            }
        };
        // Already in the folder (sent from the files view): nothing to copy.
        if file.starts_with(&share.path) {
            return Ok(Some(file.to_path_buf()));
        }
        let dir = std::path::Path::new(&share.path).join(crate::shares::category(file, message));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let to = crate::shares::free_name(&dir, &name);
        // A large video takes a while: copied off the UI thread.
        let (from, dest) = (file.to_path_buf(), to.clone());
        std::thread::spawn(move || {
            if let Err(e) = std::fs::copy(&from, &dest) {
                tracing::warn!(error = %e, to = %dest.display(), "the file did not go into the conversation's folder");
            }
        });
        Ok(Some(to))
    }

    fn sync_hello(&mut self, reply: bool) {
        let name = self.model.devices.get(&self.model.device).cloned().unwrap_or_default();
        let (acc, me, dev) = (self.model.account.clone(), self.model.me.clone(), self.model.device.clone());
        self.sync.hello(&acc, &me, &dev, &name, reply);
    }

    /// The sync report (folders, devices, paths), for the socket (blocking).
    pub fn sync_report(&self) -> crate::sync::Report {
        self.sync.snapshot().report(&self.model.account)
    }

    /// `joma github publish|unpublish|find|add`: GitHub is slow, so off the UI thread.
    fn github_ipc(&mut self, p: crate::ipc::Pending, window: &mut Window, cx: &mut Context<Self>) {
        let Request::Github { what, arg, label, json } = p.request.clone() else { return };
        let reply = p.reply;
        match (what.as_str(), arg.as_deref().unwrap_or("")) {
            ("signin", _) => {
                self.github_signin(crate::signin::After::Nothing, cx);
                raise(window);
                let _ = reply.send(Reply::Text { text: "Joma's window shows the code to type at github.com/login/device".into() });
                return;
            }
            ("auto", "on") if self.gate == Gate::Ready => {
                self.auto_switch(true, window, cx);
                let _ = reply.send(Reply::Text { text: "Joma asks for the backup password in its window".into() });
                return;
            }
            ("auto", "off") => {
                self.auto_switch(false, window, cx);
                let _ = reply.send(Reply::Text { text: "automatic backups are off; the keyring entries are deleted".into() });
                return;
            }
            ("auto", _) => {
                let last = if self.auto.last == 0 { "never".to_owned() } else { format!("{} h ago", crate::github::now().saturating_sub(self.auto.last) / 3600) };
                let text = if json {
                    serde_json::json!({"auto": self.auto.on, "last": self.auto.last}).to_string()
                } else {
                    format!("automatic backups {} · last backup {last}", if self.auto.on { "on" } else { "off" })
                };
                let _ = reply.send(Reply::Text { text });
                return;
            }
            _ => {}
        }
        if self.gate != Gate::Ready {
            let _ = reply.send(Reply::Error { message: "Joma is not on an account now".into() });
            return;
        }
        let (tx, account, me) = (self.tx.clone(), self.model.account.clone(), self.model.me.clone());
        match what.as_str() {
            "publish" | "unpublish" => {
                let on = what == "publish";
                let name = if self.model.username.is_empty() { self.model.my_name.clone() } else { self.model.username.clone() };
                let label = label.unwrap_or(name);
                self.gh_busy = true;
                std::thread::spawn(move || {
                    let r = crate::github::publish(&me, on.then_some(label.as_str())).map_err(|e| format!("{e:#}"));
                    let _ = reply.send(match &r {
                        Ok((login, p)) => Reply::Text {
                            text: format!("github.com/{login}: {}", if p.accounts.is_empty() { "no accounts listed".to_owned() } else { p.accounts.iter().map(|a| a.label.clone()).collect::<Vec<_>>().join(", ") }),
                        },
                        Err(e) => Reply::Error { message: e.clone() },
                    });
                    let _ = tx.unbounded_send(AppMsg::GithubPublished(account, me, on, r.map(|(l, _)| l)));
                });
            }
            "find" | "add" => {
                let login = arg.unwrap_or_default();
                let add = what == "add";
                let _ = cx;
                std::thread::spawn(move || {
                    let r = match crate::github::fetch(&login) {
                        Err(e) => Reply::Error { message: format!("{e:#}") },
                        Ok(None) => Reply::Error { message: format!("no GitHub user {login}") },
                        Ok(Some((id, p))) => {
                            let listed: Vec<crate::github::Listed> = p.filter(|p| p.github_id == id).map(|p| p.accounts).unwrap_or_default();
                            if !add {
                                if json {
                                    Reply::Text { text: serde_json::json!({"login": login, "github_id": id, "accounts": listed}).to_string() }
                                } else if listed.is_empty() {
                                    Reply::Text { text: format!("github.com/{login} lists no Joma accounts") }
                                } else {
                                    Reply::Text { text: listed.iter().map(|a| format!("{}  {}", a.label, a.jami)).collect::<Vec<_>>().join("\n") }
                                }
                            } else {
                                let pick: Vec<&crate::github::Listed> = match &label {
                                    Some(l) => listed.iter().filter(|a| a.label.eq_ignore_ascii_case(l) || a.jami.starts_with(l.as_str())).collect(),
                                    None => listed.iter().collect(),
                                };
                                match pick.as_slice() {
                                    [] if listed.is_empty() => Reply::Error { message: format!("github.com/{login} lists no Joma accounts") },
                                    [] => Reply::Error { message: "no listed account has that label (joma github find)".into() },
                                    [one] => match joma_jami::add_contact(&account, &one.jami) {
                                        Ok(()) => {
                                            let _ = tx.unbounded_send(AppMsg::GithubPin(account.clone(), one.jami.clone(), login.clone(), id));
                                            Reply::Text { text: format!("added {} (github.com/{login})", if one.label.is_empty() { &one.jami } else { &one.label }) }
                                        }
                                        Err(e) => Reply::Error { message: format!("could not add: {e}") },
                                    },
                                    _ => Reply::Error { message: format!("github.com/{login} lists several accounts: name one (joma github add {login} <label>)") },
                                }
                            }
                        }
                    };
                    let _ = reply.send(r);
                });
            }
            _ => {
                let _ = reply.send(Reply::Error { message: "joma github publish|unpublish|find|add".into() });
            }
        }
    }

    /// A `joma backup` / `joma restore` arrived: ask for the password in the window.
    fn ask_password(&mut self, p: crate::ipc::Pending, window: &mut Window, cx: &mut Context<Self>) {
        let refuse = |p: crate::ipc::Pending, m: &str| {
            let _ = p.reply.send(Reply::Error { message: m.into() });
        };
        if self.password_ask.is_some() {
            return refuse(p, "Joma is already asking for a backup password");
        }
        let restoring = matches!(p.request, Request::Restore { .. });
        if self.gate != Gate::Ready && !(restoring && self.gate == Gate::Onboarding && self.linking.is_none()) {
            return refuse(p, if restoring { "Joma is busy linking a device" } else { "open an account first" });
        }
        self.load_accounts();
        let here: Vec<crate::backup::Account> = self
            .accounts
            .iter()
            .map(|(id, name)| crate::backup::Account { id: id.clone(), uri: joma_jami::account_uri(id).unwrap_or_default(), label: name.clone() })
            .collect();
        let (restore, wanted, all, file) = match &p.request {
            Request::Backup { accounts, all, file } => (false, accounts.clone(), *all, file.clone()),
            Request::Restore { accounts, file } => (true, accounts.clone(), false, file.clone()),
            _ => return,
        };
        let accounts = if restore {
            Vec::new()
        } else if all {
            here
        } else if wanted.is_empty() {
            here.into_iter().filter(|a| a.id == self.model.account).collect()
        } else {
            let low: Vec<String> = wanted.iter().map(|w| w.to_lowercase()).collect();
            here.into_iter().filter(|a| low.iter().any(|w| a.label.to_lowercase() == *w || a.id == *w || a.uri.starts_with(w.as_str()))).collect()
        };
        if !restore && accounts.iter().any(|a| a.uri.is_empty()) {
            return refuse(p, "an account is not ready yet");
        }
        if !restore && accounts.is_empty() {
            return refuse(p, "no account matches (joma accounts)");
        }
        if restore && self.gate == Gate::Onboarding {
            let from = file.clone().unwrap_or_else(|| "your GitHub backup".into());
            self.restoring = Some(RestoreView { cli: Some(from), ..Default::default() });
        }
        self.password_ask = Some(PasswordAsk { restore, accounts, wanted, file, first: None, remember: false, reply: p.reply });
        self.set_mode(Mode::Password, window, cx);
        raise(window);
    }

    fn password_entered(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |c, cx| c.clear(cx));
        let Some(ask) = self.password_ask.as_mut() else { return };
        if text.is_empty() {
            return;
        }
        if !ask.restore {
            if ask.first.is_none() {
                if let Some(why) = joma_backup::weak(&text) {
                    return self.show_flash(format!("password too weak: {why}"), cx);
                }
                ask.first = Some(text);
                self.configure_inputs(cx);
                cx.notify();
                return;
            }
            if ask.first.as_deref() != Some(text.as_str()) {
                ask.first = None;
                self.configure_inputs(cx);
                return self.show_flash("the two passwords differ: type it again", cx);
            }
        }
        let Some(ask) = self.password_ask.take() else { return };
        self.set_mode(Mode::Chat, window, cx);
        if let Some(r) = self.restoring.as_mut() {
            r.working = true;
            r.error.clear();
        }
        self.show_flash(if ask.restore { "restoring…" } else { "backing up…" }, cx);
        let present: Vec<String> = self.accounts.iter().filter_map(|(id, _)| joma_jami::account_uri(id).ok()).collect();
        let tx = self.tx.clone();
        let token = self.gh_token.clone();
        std::thread::spawn(move || {
            let file = ask.file.as_deref().map(std::path::Path::new);
            let r = if ask.restore {
                crate::backup::restore(&ask.wanted, &text, file, &present)
            } else {
                crate::backup::backup(&ask.accounts, &text, file)
            }
            .map_err(|e| format!("{e:#}"));
            let _ = ask.reply.send(match &r {
                Ok(t) => Reply::Text { text: t.clone() },
                Err(e) => Reply::Error { message: e.clone() },
            });
            let ok = r.is_ok();
            let _ = tx.unbounded_send(AppMsg::BackupDone(ask.restore, r));
            if ask.remember && ok {
                let _ = tx.unbounded_send(AppMsg::AutoKept(crate::signin::remember(&text, token.as_ref()).map_err(|e| format!("{e:#}"))));
            }
        });
    }

    pub fn cancel_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ask) = self.password_ask.take() {
            let _ = ask.reply.send(Reply::Error { message: "cancelled in Joma".into() });
        }
        self.set_mode(Mode::Chat, window, cx);
    }

    /// The devices tab's switch.
    pub fn sync_switch(&mut self, on: bool, cx: &mut Context<Self>) {
        let acc = self.model.account.clone();
        if let Err(e) = crate::sync::set_on(&acc, on) {
            return self.show_flash(format!("cannot save the sync switch: {e}"), cx);
        }
        if on {
            self.start_sync();
        } else {
            self.sync.stop(&acc);
            self.sync_view = None;
        }
        cx.notify();
    }

    /// Pick a folder to sync with all our devices.
    pub fn choose_sync_folder(&mut self, cx: &mut Context<Self>) {
        if own_profile() && std::env::var_os("JOMA_PORTAL").is_none() {
            return self.show_flash("folder chooser skipped in a test profile (joma sync add <path>)", cx);
        }
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Sync".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let _ = this.update(cx, |j, cx| {
                if let Some(p) = paths.first() {
                    let label = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "folder".into());
                    match j.sync.add_folder(p, &label) {
                        Ok(_) => j.show_flash(format!("{label} syncs with your devices"), cx),
                        Err(e) => j.show_flash(format!("cannot sync {label}: {e:#}"), cx),
                    }
                    j.refresh_sync_view();
                }
            });
        })
        .detach();
    }

    /// Refresh the devices tab's sync section in the background.
    pub fn refresh_sync_view(&mut self) {
        if self.sync_view_busy {
            return;
        }
        self.sync_view_busy = true;
        let (snap, acc, tx) = (self.sync.snapshot(), self.model.account.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let r = snap.report(&acc);
            let _ = tx.unbounded_send(AppMsg::SyncView(acc, r));
        });
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
        self.note_devices();
        self.github_upkeep();
    }

    /// Devices that joined without this machine linking them become "new" (keep / revoke).
    fn note_devices(&mut self) {
        let acc = self.model.account.clone();
        if acc.is_empty() || self.model.device.is_empty() || self.model.devices.is_empty() {
            return;
        }
        let now: Vec<String> = self.model.devices.keys().cloned().collect();
        let seen = crate::seen::load(&acc);
        if seen.is_none() {
            // The account is new here (created, linked or restored): the devices it
            // learns about in the next minutes were there before this one.
            self.last_link = Some(Instant::now());
        }
        let quiet = self.device_op.is_some() || self.last_link.is_some_and(|t| t.elapsed() < Duration::from_secs(300));
        let fresh = crate::seen::fresh(seen.as_ref(), &now, &self.model.device, quiet);
        self.new_devices.retain(|d| now.contains(d));
        for d in fresh {
            if !self.new_devices.contains(&d) {
                let name = self.model.devices.get(&d).cloned().filter(|n| !n.is_empty()).unwrap_or_else(|| d.chars().take(8).collect());
                self.flash = Some((format!("new device {name} joined your account: keep or revoke it in Devices"), Instant::now()));
                self.new_devices.push(d);
            }
        }
        let mut keep: std::collections::BTreeSet<String> = seen.unwrap_or_default();
        keep.extend(now.into_iter().filter(|d| !self.new_devices.contains(d)));
        crate::seen::save(&acc, &keep);
    }

    /// A profile arrived: check its GitHub line (our own: make sure it is there).
    fn github_profile(&mut self, uri: String, vcard: &str) {
        if uri == self.model.me || uri.is_empty() {
            return;
        }
        let text = crate::model::vcard_text(vcard);
        match crate::github::vcard_login(&text) {
            Some(login) if self.gh.stale(&uri, &login) => self.github_check(uri, login),
            Some(_) => {}
            // The line is gone: so is the badge (a pinned contact keeps its login).
            None => {
                if let Some(k) = self.gh.contacts.get_mut(&uri) {
                    if k.verified && !k.pinned {
                        k.verified = false;
                        self.gh.save(&self.model.account);
                    }
                }
            }
        }
    }

    /// Check a contact's GitHub claim off the UI thread.
    fn github_check(&mut self, uri: String, login: String) {
        if !self.gh_checking.insert(uri.clone()) {
            return;
        }
        let (tx, account) = (self.tx.clone(), self.model.account.clone());
        std::thread::spawn(move || {
            let r = crate::github::fetch(&login).map_err(|e| format!("{e:#}"));
            let _ = tx.unbounded_send(AppMsg::GithubChecked(account, uri, login, r));
        });
    }

    /// At start: our public account keeps its profile line (linking drops it), and
    /// contacts are checked again every few days.
    fn github_upkeep(&mut self) {
        let me = self.model.me.clone();
        if let Some(login) = self.gh.mine.get(&me).cloned() {
            let has = self.account_dir().and_then(|d| std::fs::read_to_string(d.join("profile.vcf")).ok()).and_then(|v| crate::github::vcard_login(&v));
            if has.as_deref() != Some(login.as_str()) {
                self.set_github_line(Some(&login));
            }
        }
        let due: Vec<(String, String)> = self
            .gh
            .contacts
            .iter()
            .filter(|(_, k)| (k.verified || k.pinned) && crate::github::now().saturating_sub(k.checked) > crate::github::RECHECK)
            .map(|(u, k)| (u.clone(), k.login.clone()))
            .collect();
        for (uri, login) in due {
            self.github_check(uri, login);
        }
    }

    /// Put our `X-JOMA-GITHUB` line in the profile (or take it out) and send the profile
    /// to contacts again.
    fn set_github_line(&mut self, login: Option<&str>) {
        let Some(path) = self.account_dir().map(|d| d.join("profile.vcf")) else { return };
        let vcf = std::fs::read_to_string(&path).unwrap_or_else(|_| "BEGIN:VCARD\r\nVERSION:2.1\r\nEND:VCARD\r\n".into());
        if std::fs::write(&path, crate::github::vcard_with(&vcf, login)).is_ok() {
            let _ = joma_jami::update_profile(&self.model.account, &self.model.my_name);
        }
    }

    /// The accounts tab's switch: list this account in our public gist, or not.
    pub fn github_publish(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.gh_busy {
            return;
        }
        self.gh_busy = true;
        let (tx, account, uri) = (self.tx.clone(), self.model.account.clone(), self.model.me.clone());
        let label = if self.model.username.is_empty() { self.model.my_name.clone() } else { self.model.username.clone() };
        std::thread::spawn(move || {
            let r = crate::github::publish(&uri, on.then_some(label.as_str())).map(|(login, _)| login).map_err(|e| format!("{e:#}"));
            let _ = tx.unbounded_send(AppMsg::GithubPublished(account, uri, on, r));
        });
        self.show_flash(if on { "listing this account on GitHub…" } else { "taking this account off GitHub…" }, cx);
    }

    /// "gh:<login>" in the add field: look the login up.
    /// Sign in with the Joma GitHub App: show a code, poll in the background.
    pub fn github_signin(&mut self, after: crate::signin::After, cx: &mut Context<Self>) {
        if self.gh_signin.as_ref().is_some_and(|s| s.error.is_none()) {
            return;
        }
        self.gh_cancel.store(false, Ordering::SeqCst);
        self.gh_signin = Some(crate::signin::SignIn { after, code: None, error: None });
        let (tx, tx2, cancel) = (self.tx.clone(), self.tx.clone(), self.gh_cancel.clone());
        std::thread::spawn(move || {
            let r = crate::signin::run(move |c| drop(tx2.unbounded_send(AppMsg::GithubCode(after, c))), move || cancel.load(Ordering::SeqCst));
            let _ = tx.unbounded_send(AppMsg::GithubSignedIn(after, r));
        });
        cx.notify();
    }

    pub fn cancel_github_signin(&mut self, cx: &mut Context<Self>) {
        self.gh_cancel.store(true, Ordering::SeqCst);
        self.gh_signin = None;
        cx.notify();
    }

    /// Make sure GitHub can be reached (gh or the app's token), signing in if not, then
    /// do `after`.
    pub fn need_github(&mut self, after: crate::signin::After, window: &mut Window, cx: &mut Context<Self>) {
        if joma_backup::gh::has_token() {
            return self.github_continue(after, window, cx);
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.unbounded_send(AppMsg::GithubReady(after, crate::signin::ready()));
        });
    }

    fn github_continue(&mut self, after: crate::signin::After, window: &mut Window, cx: &mut Context<Self>) {
        use crate::signin::After;
        match after {
            After::Nothing => {}
            After::Restore => self.start_restore(cx),
            After::Backup => self.backup_now(false, window, cx),
            After::Auto => self.backup_now(true, window, cx),
            After::Publish(on) => self.github_publish(on, cx),
        }
    }

    /// "back up now" on the accounts tab: every account here, the password asked in the
    /// window (`remember`: then kept for automatic backups).
    fn backup_now(&mut self, remember: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (reply, _) = std::sync::mpsc::channel();
        self.ask_password(crate::ipc::Pending { request: Request::Backup { accounts: Vec::new(), all: true, file: None }, reply }, window, cx);
        if let Some(a) = self.password_ask.as_mut() {
            a.remember = remember;
        }
    }

    pub fn backup_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.need_github(crate::signin::After::Backup, window, cx);
    }

    /// The accounts tab's "automatic backups" switch.
    pub fn auto_switch(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if on {
            if !crate::keyring::available() {
                return self.show_flash("automatic backups need the system keyring (secret-tool, from libsecret)", cx);
            }
            self.need_github(crate::signin::After::Auto, window, cx);
        } else {
            crate::signin::forget();
            self.auto.on = false;
            self.auto.save();
            self.show_flash("automatic backups are off; the password is no longer kept", cx);
        }
        cx.notify();
    }

    /// Once a day, when automatic backups are on.
    fn auto_tick(&mut self) {
        if self.auto_running || self.gate != Gate::Ready || self.password_ask.is_some() || !self.auto.due(crate::github::now()) {
            return;
        }
        self.auto_running = true;
        let accounts: Vec<crate::backup::Account> = self
            .accounts
            .iter()
            .filter_map(|(id, name)| Some(crate::backup::Account { id: id.clone(), uri: joma_jami::account_uri(id).ok()?, label: name.clone() }))
            .collect();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.unbounded_send(AppMsg::AutoBackupDone(crate::signin::auto_backup(&accounts)));
        });
    }

    fn github_lookup(&mut self, login: String, cx: &mut Context<Self>) {
        if !crate::github::valid_login(&login) {
            return self.show_flash(format!("{login} is not a GitHub name"), cx);
        }
        self.gh_found = None;
        let tx = self.tx.clone();
        let l = login.clone();
        std::thread::spawn(move || {
            let r = crate::github::fetch(&l).map_err(|e| format!("{e:#}"));
            let _ = tx.unbounded_send(AppMsg::GithubFound(l, r));
        });
        self.show_flash(format!("looking up github.com/{login}…"), cx);
    }

    /// Add a contact found through GitHub, pinned to that login.
    pub fn add_github(&mut self, login: &str, github_id: u64, uri: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = joma_jami::add_contact(&self.model.account, uri) {
            return self.show_flash(format!("could not add: {e}"), cx);
        }
        let acc = self.model.account.clone();
        let k = self.gh.contacts.entry(uri.to_owned()).or_default();
        k.login = login.to_owned();
        k.github_id = github_id;
        k.pinned = true;
        k.verified = true;
        k.checked = crate::github::now();
        // A pinned contact that moved: the old account no longer waits for an answer.
        for k in self.gh.contacts.values_mut() {
            if k.login == login {
                k.moved_to.retain(|l| l.jami != uri);
            }
        }
        self.gh.save(&acc);
        self.gh_found = None;
        self.show_flash(format!("added github.com/{login} · the conversation starts when they accept"), cx);
        self.set_mode(Mode::Chat, window, cx);
    }

    /// Leave a pinned contact on its old account.
    pub fn dismiss_moved(&mut self, uri: &str, cx: &mut Context<Self>) {
        if let Some(k) = self.gh.contacts.get_mut(uri) {
            k.moved_to.clear();
            self.gh.save(&self.model.account);
        }
        cx.notify();
    }

    /// Keep a new device: it is ours.
    pub fn keep_device(&mut self, id: &str, cx: &mut Context<Self>) {
        self.new_devices.retain(|d| d != id);
        let acc = self.model.account.clone();
        let mut seen = crate::seen::load(&acc).unwrap_or_default();
        seen.insert(id.to_owned());
        crate::seen::save(&acc, &seen);
        cx.notify();
    }

    /// The welcome screen's "restore from your backup": read the repo off the UI thread.
    pub fn start_restore(&mut self, cx: &mut Context<Self>) {
        self.restoring = Some(RestoreView { loading: true, ..Default::default() });
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = (|| -> anyhow::Result<(String, Vec<joma_backup::Entry>)> {
                let login = joma_backup::gh::login()?;
                if joma_backup::gh::repo(&login)?.is_none() {
                    anyhow::bail!("github.com/{login}/{} does not exist: nothing backed up yet", joma_backup::REPO);
                }
                Ok((login.clone(), joma_backup::gh::index(&login)?.accounts))
            })()
            .map_err(|e| format!("{e:#}"));
            let _ = tx.unbounded_send(AppMsg::RestoreList(r));
        });
        cx.notify();
    }

    pub fn toggle_restore_pick(&mut self, jami: &str, cx: &mut Context<Self>) {
        if let Some(v) = self.restoring.as_mut() {
            if !v.picked.remove(jami) {
                v.picked.insert(jami.to_owned());
            }
        }
        cx.notify();
    }

    /// The accounts are picked: ask for the backup's password.
    pub fn restore_continue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(v) = self.restoring.as_ref() else { return };
        if v.picked.is_empty() {
            return self.show_flash("pick an account to restore", cx);
        }
        let wanted: Vec<String> = v.entries.iter().filter(|e| v.picked.contains(&e.jami)).map(|e| e.jami.clone()).collect();
        let (reply, _) = std::sync::mpsc::channel();
        self.password_ask = Some(PasswordAsk { restore: true, accounts: Vec::new(), wanted, file: None, first: None, remember: false, reply });
        self.set_mode(Mode::Password, window, cx);
    }

    pub fn cancel_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restoring = None;
        if self.password_ask.is_some() {
            self.cancel_password(window, cx);
        }
        self.configure_inputs(cx);
        cx.notify();
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
            (_, Mode::Password) => match &self.password_ask {
                Some(a) if a.restore => ("password", "The backup's password, then Enter"),
                Some(a) if a.first.is_some() => ("password-again", "The same password again, then Enter"),
                _ => ("password", "A password for the backup (the same for all accounts), then Enter"),
            },
            (Gate::Onboarding, _) if self.linking.as_ref().is_some_and(|l| l.password_scheme.is_some()) => {
                ("link-password", "The account's password, then Enter")
            }
            (Gate::Onboarding, _) if self.onboard_name.is_none() => ("onboarding", "Your name (shown to your contacts)"),
            (Gate::Onboarding, _) => ("onboarding-user", "A username, so contacts can find you (Enter alone skips)"),
            (_, Mode::Username) => ("username", "Your username, then Enter"),
            (_, Mode::Add) => ("add", "Jami username, 40-digit ID or gh:<GitHub name>, then Enter"),
            (_, Mode::Search) => ("search", "Filter conversations"),
            (_, Mode::LinkDevice) => ("link", "The jami-auth:// code the new device shows, then Enter"),
            (_, Mode::DeviceName) => ("device-name", "A name for this device, then Enter"),
            (_, Mode::ShareWith) => ("share-with", "A contact or a conversation, then Enter"),
            (_, Mode::RemoveAccount) => ("remove-account", "Type delete, then Enter"),
            _ => ("idle", ""),
        };
        self.input.update(cx, |c, _| {
            c.configure(key.into(), theme, true);
            c.set_placeholder(placeholder);
            c.set_allow_empty(key == "onboarding-user");
            c.set_masked(matches!(key, "password" | "password-again" | "link-password"));
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
            self.tell_theme();
            cx.notify();
        }
    }

    /// Tell our other devices (joma for Android follows it when asked) the Omarchy theme
    /// in use: at start and when it changes.
    pub fn tell_theme(&mut self) {
        if self.gate != Gate::Ready {
            return;
        }
        self.theme_told = Some(Instant::now());
        let Some(name) = joma_theme::name() else { return };
        let mode = joma_theme::colors_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("mode").map(|v| v.trim_start_matches([' ', '=']).trim_matches('"').to_owned())))
            .unwrap_or_else(|| "dark".into());
        let mut p = joma_jami::Details::new();
        p.insert(THEME_MIME.into(), serde_json::json!({"name": name, "mode": mode}).to_string());
        let (acc, me) = (self.model.account.clone(), self.model.me.clone());
        if let Err(e) = joma_jami::send_account_message(&acc, &me, &p, false) {
            tracing::warn!(error = %e, "could not tell our devices the theme");
        }
    }

    /// Tell our other devices what this one transcribes (with the theme, every half hour,
    /// and when the role changes).
    pub fn tell_role(&mut self) {
        if self.gate != Gate::Ready {
            return;
        }
        let engine = crate::media::transcriber(&self.quick.transcribe).unwrap_or("none");
        let name = self.model.devices.get(&self.model.device).cloned().unwrap_or_default();
        let mut p = joma_jami::Details::new();
        p.insert(ROLE_MIME.into(), serde_json::json!({"dev": self.model.device, "name": name, "role": self.quick.transcriber, "engine": engine}).to_string());
        let (acc, me) = (self.model.account.clone(), self.model.me.clone());
        let _ = joma_jami::send_account_message(&acc, &me, &p, false);
    }

    /// Another of our devices transcribes for all and was heard lately: new messages wait
    /// a little for its words before this device does them itself.
    /// Our devices and what each transcribes, for settings and `joma transcription`.
    pub fn transcription_devices(&self) -> Vec<(String, String, String)> {
        let role = |r: &str| match r {
            "all" => "for all my devices",
            "off" => "does not transcribe",
            _ => "for itself",
        };
        let me = self.model.devices.get(&self.model.device).cloned().unwrap_or_else(|| "this device".into());
        let mut out = vec![(format!("{me} (this one)"), role(&self.quick.transcriber).to_owned(), crate::media::transcriber(&self.quick.transcribe).unwrap_or("none").to_owned())];
        for t in self.transcribers.values() {
            out.push((if t.name.is_empty() { "a device".into() } else { t.name.clone() }, role(&t.role).to_owned(), t.engine.clone()));
        }
        out
    }

    fn other_transcribes_for_all(&self) -> bool {
        self.transcribers.iter().any(|(d, t)| *d != self.model.device && t.role == "all" && t.engine != "none" && t.heard.elapsed() < Duration::from_secs(40 * 60))
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
        if self.restored_sync.is_none() && !self.model.me.is_empty() && self.restored_seen.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.restored_seen = Some(Instant::now());
            if let Some(s) = crate::sync::restored_path(&self.model.me).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<crate::sync::Saved>(&b).ok()) {
                self.restored_sync = Some(s);
                cx.notify();
            }
        }
        if self.theme_told.is_none_or(|t| t.elapsed() > Duration::from_secs(30 * 60)) {
            self.theme_told = Some(Instant::now());
            self.tell_theme();
            self.tell_role();
        }
        self.auto_tick();
        self.transcriber_tick(cx);
        if self.sync.st.is_some() && self.sync.last_hello.is_none_or(|t| t.elapsed() > crate::sync::HELLO_EVERY) {
            self.sync_hello(false);
        }
        if self.tab == Tab::Files && self.files.read_at.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.files_read();
            cx.notify();
        }
        if self.sync.st.is_some() && matches!(self.tab, Tab::Devices | Tab::Files) && self.sync_view_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
            self.sync_view_at = Some(Instant::now());
            self.refresh_sync_view();
        }
        self.update_tray();
        if self.presence.expire(Instant::now()) {
            cx.notify();
        }
        self.track_shares();
        self.tick_watch();
        // Played to the end: back to the start.
        if self.player.as_mut().is_some_and(|p| p.ended()) {
            if let Some(p) = self.player.as_mut() {
                p.rewind();
            }
            self.video_big = false;
            cx.notify();
        }
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
        // Releases: shortly after the start, then every six hours.
        if self.gate == Gate::Ready && self.update_checked.is_none_or(|t| t.elapsed() > Duration::from_secs(6 * 3600)) && self.booted.elapsed() > Duration::from_secs(if self.update_checked.is_none() { 20 } else { 0 }) {
            self.check_updates(false, cx);
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
                    // The strip shows the drawing's state.
                    ShareEvent::Ink => cx.notify(),
                }
                let _ = call;
            }
            AppMsg::Repaint => {
                self.repaint_queued.store(false, Ordering::SeqCst);
                cx.notify();
            }
            AppMsg::Ipc(p) => match p.request {
                Request::Backup { .. } | Request::Restore { .. } => self.ask_password(p, window, cx),
                Request::Github { .. } => self.github_ipc(p, window, cx),
                Request::BackupList | Request::GithubStatus => {
                    // gh talks to GitHub: off the UI thread.
                    let list = matches!(p.request, Request::BackupList);
                    std::thread::spawn(move || {
                        let reply = if list {
                            match crate::backup::list() {
                                Ok(text) => Reply::Text { text },
                                Err(e) => Reply::Error { message: format!("{e:#}") },
                            }
                        } else {
                            Reply::Text { text: crate::backup::status() }
                        };
                        let _ = p.reply.send(reply);
                    });
                }
                _ => {
                    let reply = self.ipc(p.request, window, cx);
                    let _ = p.reply.send(reply);
                }
            },
            AppMsg::GithubChecked(account, uri, login, r) => {
                self.gh_checking.remove(&uri);
                if account != self.model.account {
                    return;
                }
                let (id, v) = match r {
                    Ok(Some((id, p))) => (id, crate::github::verdict(p.as_ref(), id, &uri)),
                    Ok(None) => (0, crate::github::Verdict::Missing),
                    Err(e) => {
                        tracing::warn!("GitHub check of {login}: {e}");
                        return;
                    }
                };
                let was = self.gh.badge(&uri).is_some();
                self.gh.record(&uri, &login, id, &v);
                self.gh.save(&account);
                let name = self.model.name_of(&uri);
                if was && self.gh.badge(&uri).is_none() {
                    self.show_flash(format!("{name}: the GitHub badge is gone (github.com/{login} no longer lists this account)"), cx);
                }
                if self.gh.contacts.get(&uri).is_some_and(|k| !k.moved_to.is_empty()) {
                    self.show_flash(format!("{name}'s GitHub now names another Jami account: see the conversation"), cx);
                }
                cx.notify();
            }
            AppMsg::GithubFound(login, r) => {
                match r {
                    Ok(Some((id, Some(p)))) if p.github_id == id && !p.accounts.is_empty() => {
                        self.gh_found = Some((login, id, p.accounts));
                    }
                    Ok(Some(_)) => self.show_flash(format!("github.com/{login} has no Joma accounts listed"), cx),
                    Ok(None) => self.show_flash(format!("no GitHub user {login}"), cx),
                    Err(e) => self.show_flash(e, cx),
                }
                cx.notify();
            }
            AppMsg::GithubPublished(account, uri, on, r) => {
                self.gh_busy = false;
                match r {
                    Ok(login) => {
                        if account == self.model.account {
                            if on {
                                self.gh.mine.insert(uri.clone(), login.clone());
                            } else {
                                self.gh.mine.remove(&uri);
                            }
                            self.gh.save(&account);
                            self.set_github_line(on.then_some(login.as_str()));
                        }
                        self.show_flash(if on { format!("contacts now see ✓ github.com/{login}") } else { "the GitHub badge is off; github.com/… no longer lists this account".to_owned() }, cx);
                    }
                    Err(e) if joma_backup::gh::not_signed_in(&e) => self.github_signin(crate::signin::After::Publish(on), cx),
                    Err(e) => self.show_flash(format!("GitHub: {e}"), cx),
                }
                cx.notify();
            }
            AppMsg::GithubCode(after, code) => {
                if let Some(s) = self.gh_signin.as_mut().filter(|s| s.after == after) {
                    s.code = Some(code);
                }
                cx.notify();
            }
            AppMsg::GithubSignedIn(after, r) => {
                match r {
                    Ok((token, login)) => {
                        self.gh_signin = None;
                        self.gh_token = Some(token);
                        self.show_flash(format!("signed in to GitHub as {login}"), cx);
                        self.github_continue(after, window, cx);
                    }
                    Err(e) if e == "cancelled" => self.gh_signin = None,
                    Err(e) => {
                        if let Some(s) = self.gh_signin.as_mut() {
                            s.error = Some(e);
                            s.code = None;
                        }
                    }
                }
                cx.notify();
            }
            AppMsg::GithubReady(after, ready) => {
                if ready {
                    self.github_continue(after, window, cx);
                } else {
                    self.github_signin(after, cx);
                }
            }
            AppMsg::AutoBackupDone(r) => {
                self.auto_running = false;
                match r {
                    Ok(_) => {
                        self.auto.last = crate::github::now();
                        self.auto.save();
                    }
                    Err(e) => {
                        // Try again in an hour, not at every tick.
                        self.auto.last = crate::github::now().saturating_sub(crate::signin::EVERY - 3600);
                        self.show_flash(format!("automatic backup failed: {e}"), cx);
                    }
                }
                cx.notify();
            }
            AppMsg::AutoKept(r) => {
                match r {
                    Ok(()) => {
                        self.auto.on = true;
                        self.auto.save();
                        self.show_flash("automatic backups are on: once a day, the password is in the system keyring", cx);
                    }
                    Err(e) => self.show_flash(format!("automatic backups stay off: {e}"), cx),
                }
                cx.notify();
            }
            AppMsg::GithubPin(account, uri, login, id) => {
                if account == self.model.account {
                    let k = self.gh.contacts.entry(uri.clone()).or_default();
                    k.login = login;
                    k.github_id = id;
                    k.pinned = true;
                    k.verified = true;
                    k.checked = crate::github::now();
                    k.moved_to.clear();
                    self.gh.save(&account);
                }
                cx.notify();
            }
            AppMsg::RestoreList(r) => {
                if let Some(v) = self.restoring.as_mut() {
                    v.loading = false;
                    match r {
                        Ok((login, entries)) => {
                            let here: Vec<String> = joma_jami::accounts().unwrap_or_default().iter().filter_map(|a| joma_jami::account_uri(a).ok()).collect();
                            v.picked = entries.iter().filter(|e| !here.contains(&e.jami)).map(|e| e.jami.clone()).collect();
                            v.login = login;
                            v.entries = entries;
                        }
                        Err(e) if joma_backup::gh::not_signed_in(&e) => v.error = "Not signed in to GitHub on this machine.".into(),
                        Err(e) => v.error = e,
                    }
                }
                cx.notify();
            }
            AppMsg::BackupDone(true, Err(e)) if self.restoring.as_ref().is_some_and(|v| v.cli.is_some()) => {
                // `joma restore` got the answer; back to the welcome screen.
                self.restoring = None;
                self.show_flash(format!("restore failed: {e}"), cx);
            }
            AppMsg::BackupDone(true, Err(e)) if self.restoring.is_some() => {
                if let Some(v) = self.restoring.as_mut() {
                    v.working = false;
                    v.error = e.clone();
                }
                // A wrong password: type it again.
                if e == "wrong password" {
                    self.restore_continue(window, cx);
                }
                cx.notify();
            }
            AppMsg::BackupDone(false, Err(e)) if joma_backup::gh::not_signed_in(&e) => {
                self.show_flash("backup: not signed in to GitHub", cx);
                self.github_signin(crate::signin::After::Backup, cx);
            }
            AppMsg::BackupDone(restore, r) => {
                if restore {
                    self.restoring = None;
                }
                if !restore && r.is_ok() {
                    self.auto.last = crate::github::now();
                    self.auto.save();
                }
                match r {
                    Ok(text) => self.show_flash(text.lines().next().unwrap_or("done").to_owned(), cx),
                    Err(e) => self.show_flash(format!("{} failed: {e}", if restore { "restore" } else { "backup" }), cx),
                }
                if restore {
                    self.load_accounts();
                }
                cx.notify();
            }
            AppMsg::Portal(call, result) => {
                let acc = self.model.account.clone();
                self.desk.portal_ready(&acc, &call, result);
                cx.notify();
            }
            AppMsg::SearchDone(conv, r) => match r {
                Ok(text) => self.send_text(&conv, &text, cx),
                Err(e) => self.show_flash(format!("YouTube search failed: {e}"), cx),
            },
            AppMsg::Watch(conv, ev) => {
                self.on_player(&conv, ev, cx);
                cx.notify();
            }
            AppMsg::Recorded { conv, path, send } => {
                if send {
                    if let Reply::Error { message } = self.send_file_as(&conv, &path, true) {
                        self.show_flash(format!("not sent: {message}"), cx);
                    }
                    self.pinned_bottom = true;
                } else {
                    let _ = std::fs::remove_file(&path);
                }
                cx.notify();
            }
            AppMsg::Shot { conv, result } => {
                match result {
                    Ok(path) => {
                        let sent = conv.as_ref().is_some_and(|c| !matches!(self.send_file(c, &path), Reply::Error { .. }));
                        self.pinned_bottom = true;
                        self.show_flash(format!("screenshot saved: {path}{}", if sent { " · sent to the conversation" } else { "" }), cx);
                    }
                    Err(e) => self.show_flash(format!("no screenshot: {e}"), cx),
                }
                cx.notify();
            }
            AppMsg::SyncView(acc, r) => {
                self.sync_view_busy = false;
                if acc == self.model.account {
                    self.sync_view = Some(r);
                    cx.notify();
                }
            }
            AppMsg::SyncStarted(acc, r) => {
                if acc != self.model.account {
                    // The account changed meanwhile: this Syncthing stops when dropped.
                    return;
                }
                match r {
                    Ok(st) => {
                        let name = self.model.devices.get(&self.model.device).cloned().unwrap_or_default();
                        let (me, dev) = (self.model.me.clone(), self.model.device.clone());
                        self.sync.started(&acc, &me, &dev, &name, st);
                        if self.restored_sync_go {
                            self.apply_restored_sync(cx);
                        }
                    }
                    Err(e) => {
                        self.sync.starting = false;
                        self.sync.error = Some(e);
                    }
                }
                cx.notify();
            }
            AppMsg::Releases(r) => {
                match r {
                    Ok(tags) => {
                        let found = joma_quick::version::newest(env!("CARGO_PKG_VERSION"), tags.iter().map(String::as_str), self.quick.prereleases);
                        self.update_note = match &found {
                            Some(v) => format!("{} is available", joma_quick::version::pretty(v)),
                            None => "this is the newest".into(),
                        };
                        if let Some(v) = &found {
                            if self.update_told.as_ref() != Some(v) {
                                self.update_told = Some(v.clone());
                                notify_desktop("Joma", &format!("Joma {} is available: update from Joma's tab strip", joma_quick::version::pretty(v)));
                            }
                        }
                        self.update = found;
                    }
                    Err(e) => self.update_note = format!("could not check: {e}"),
                }
                cx.notify();
            }
            AppMsg::Updated(r) => {
                self.updating = false;
                match r {
                    Ok(v) => {
                        self.update = None;
                        self.show_flash(format!("Joma {} is installed: restarting", joma_quick::version::pretty(&v)), cx);
                        // The new version starts once this one has left Jami's data.
                        let shim = crate::setup::exe();
                        let hidden = if crate::hypr::hidden() { " --hidden" } else { "" };
                        let _ = std::process::Command::new("sh").arg("-c").arg(format!("sleep 2; exec '{}'{hidden}", shim.display())).stdin(std::process::Stdio::null()).spawn();
                        self.leave();
                        cx.quit();
                    }
                    Err(e) => self.show_flash(format!("update failed: {e}"), cx),
                }
                cx.notify();
            }
            AppMsg::Transcribed(id, r) => {
                if let Some(c) = crate::media::cached_cues(&id) {
                    self.cues.insert(id.clone(), c);
                }
                if let Some(text) = r.as_ref().ok().filter(|t| !t.trim().is_empty()).cloned() {
                    self.after_transcript(&id, &text);
                }
                self.transcripts.insert(id, Some(r));
                cx.notify();
            }
            AppMsg::Media(id, m) => {
                self.media.insert(id, m);
                cx.notify();
            }
            AppMsg::Preview(id) => {
                self.previews_yt.insert(id.clone(), crate::media::cached(&id));
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
            Event::AccountMessage { account, from, payloads } if *account == acc && payloads.contains_key(ROLE_MIME) => {
                if bare_uri(from) == self.model.me {
                    if let Some(v) = payloads.get(ROLE_MIME).and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok()) {
                        if let Some(dev) = v["dev"].as_str().filter(|d| *d != self.model.device) {
                            let known = self.transcribers.contains_key(dev);
                            self.transcribers.insert(
                                dev.to_owned(),
                                TranscriberInfo { name: v["name"].as_str().unwrap_or("").to_owned(), role: v["role"].as_str().unwrap_or("").to_owned(), engine: v["engine"].as_str().unwrap_or("none").to_owned(), heard: Instant::now() },
                            );
                            // A device we did not know: tell it ours too.
                            if !known {
                                self.tell_role();
                            }
                            cx.notify();
                        }
                    }
                }
                return;
            }
            Event::AccountMessage { account, from, payloads } if *account == acc && payloads.contains_key(crate::media::TRANSCRIPT_MIME) => {
                if bare_uri(from) == self.model.me {
                    if let Some(v) = payloads.get(crate::media::TRANSCRIPT_MIME).and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok()) {
                        if let (Some(file), Some(text)) = (v["file"].as_str(), v["text"].as_str()) {
                            crate::media::save_transcript(file, text);
                            if !matches!(self.transcripts.get(file), Some(Some(Ok(_)))) {
                                self.transcripts.insert(file.to_owned(), Some(Ok(text.to_owned())));
                            }
                            cx.notify();
                        }
                    }
                }
                return;
            }
            Event::AccountMessage { account, from, payloads } if *account == acc && payloads.contains_key(crate::sync::MIME) => {
                if let Some(w) = payloads.get(crate::sync::MIME).and_then(|p| crate::shares::Wire::decode(p)) {
                    self.sync_wire(bare_uri(from).to_owned(), w, cx);
                    return;
                }
                if let Some(h) = payloads.get(crate::sync::MIME).and_then(|p| crate::sync::Hello::decode(p)) {
                    // Only our own devices take part.
                    if bare_uri(from) == self.model.me && h.dev != self.model.device {
                        let me = self.model.me.clone();
                        if self.sync.heard(&acc, &me, h) {
                            self.sync_hello(true);
                        }
                        cx.notify();
                    }
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
                                // A device that just came online may have missed our sync Hello.
                                self.sync_hello(false);
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
                if let Some(w) = payloads.get(crate::watch::MIME).and_then(|p| crate::watch::Wire::decode(p)) {
                    let peer = bare_uri(from).to_owned();
                    self.on_watch(&peer, w, cx);
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
                } else {
                    self.note_devices();
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
            Event::TunnelOpened { tunnel, port, .. } if self.sync.tunnel_opened(tunnel, *port) => {
                cx.notify();
                return;
            }
            Event::TunnelClosed { tunnel, .. } if self.sync.tunnel_closed(tunnel) => {
                cx.notify();
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
        if let Event::ProfileReceived { account, from, vcard } = &ev {
            if *account == acc {
                self.github_profile(bare_uri(from).to_owned(), vcard);
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
        // One short word: online, or when last seen.
        if self.model.online.contains(peer) {
            Some("online".into())
        } else {
            (seen > 0).then(|| format!("seen {}", crate::render::when(seen)))
        }
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

    // ---- Watching together -------------------------------------------------------

    /// Send to every other member of the conversation.
    fn send_watch(&self, conv: &str, sid: &str, m: crate::watch::WatchMsg) {
        let wire = crate::watch::Wire { conv: conv.to_owned(), sid: sid.to_owned(), m };
        let mut payloads = joma_jami::Details::new();
        payloads.insert(crate::watch::MIME.into(), wire.encode());
        let peers: Vec<String> = self.model.conversations.get(conv).map(|c| c.peers.clone()).unwrap_or_default();
        for p in peers.iter().filter(|p| bare_uri(p) != self.model.me) {
            if let Err(e) = joma_jami::send_account_message(&self.model.account, bare_uri(p), &payloads, false) {
                tracing::warn!(error = %e, "could not send a watch message");
            }
        }
    }

    fn start_player(&mut self, conv: &str, url: &str) -> Option<crate::watch::Player> {
        let tx = self.tx.clone();
        let k = conv.to_owned();
        match crate::watch::Player::start(url, own_profile(), move |ev| {
            let _ = tx.unbounded_send(AppMsg::Watch(k.clone(), ev));
        }) {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "no player");
                None
            }
        }
    }

    /// Play a video for ourselves only.
    pub fn play_alone(&mut self, url: &str, cx: &mut Context<Self>) {
        if let Err(e) = std::process::Command::new("mpv").arg("--force-window=yes").arg(url).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
            self.show_flash(format!("mpv: {e}"), cx);
        }
    }

    /// Watch `item` together in the open conversation; with a session running, propose
    /// to switch to it instead.
    pub fn start_watch(&mut self, url: &str, title: &str, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone() else { return };
        let item = crate::watch::Item { url: url.to_owned(), title: title.to_owned() };
        if self.watches.get(&conv).is_some_and(|w| w.joined) {
            return self.propose(&conv, crate::watch::Action::Switch { item }, cx);
        }
        let sid = format!("{:x}", crate::watch::now_ms());
        let me = self.model.me.clone();
        let mut w = crate::watch::Session::new(&conv, &sid, &me, item.clone());
        w.joined = true;
        w.player = self.start_player(&conv, url);
        if w.player.is_none() {
            return self.show_flash("mpv could not open it", cx);
        }
        w.last_state = Some(Instant::now());
        self.watches.insert(conv.clone(), w);
        self.send_watch(&conv, &sid, crate::watch::WatchMsg::Start { item });
        self.show_flash("invited the conversation; press play when ready", cx);
        cx.notify();
    }

    /// Add a video to the queue of the running session (no vote needed).
    pub fn queue_watch(&mut self, url: &str, title: &str, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone() else { return };
        let item = crate::watch::Item { url: url.to_owned(), title: title.to_owned() };
        let Some(w) = self.watches.get_mut(&conv) else { return };
        if !w.queue.iter().any(|q| q.url == item.url) {
            w.queue.push(item.clone());
        }
        let sid = w.sid.clone();
        self.send_watch(&conv, &sid, crate::watch::WatchMsg::Queue { item });
        self.show_flash("added to the queue", cx);
        cx.notify();
    }

    pub fn join_watch(&mut self, conv: &str, cx: &mut Context<Self>) {
        let Some(url) = self.watches.get(conv).filter(|w| !w.joined).map(|w| w.current.url.clone()) else { return };
        let Some(player) = self.start_player(conv, &url) else {
            return self.show_flash("mpv could not open it", cx);
        };
        let me = self.model.me.clone();
        let sid = {
            let w = self.watches.get_mut(conv).expect("checked above");
            w.player = Some(player);
            w.joined = true;
            w.participants.insert(me);
            w.sync_player();
            w.sid.clone()
        };
        self.send_watch(conv, &sid, crate::watch::WatchMsg::Join);
        cx.notify();
    }

    /// Leave (or decline): only for us; the others go on.
    pub fn leave_watch(&mut self, conv: &str, cx: &mut Context<Self>) {
        if let Some(w) = self.watches.remove(conv) {
            if w.joined {
                self.send_watch(conv, &w.sid, crate::watch::WatchMsg::Leave);
            }
        }
        cx.notify();
    }

    /// Propose an action: done at once when we are half of the participants or more,
    /// else put to the vote.
    pub fn propose(&mut self, conv: &str, action: crate::watch::Action, cx: &mut Context<Self>) {
        let me = self.model.me.clone();
        let Some(w) = self.watches.get_mut(conv) else { return };
        let pid = crate::watch::now_ms() ^ (w.proposals.len() as u64) << 48;
        let sid = w.sid.clone();
        if w.passes(1) {
            let at = crate::watch::now_ms();
            self.send_watch(conv, &sid, crate::watch::WatchMsg::Apply { pid, action: action.clone(), at_ms: at });
            self.apply_watch(conv, &action, at, cx);
            return;
        }
        w.proposals.push(crate::watch::Proposal { pid, by: me.clone(), action: action.clone(), yes: [me].into(), no: Default::default(), at: Instant::now() });
        // Until it passes, our player stays with the others.
        w.sync_player();
        self.send_watch(conv, &sid, crate::watch::WatchMsg::Propose { pid, action: action.clone() });
        self.show_flash(format!("proposed: {} (waiting for votes)", action.label()), cx);
        cx.notify();
    }

    pub fn vote_watch(&mut self, conv: &str, pid: u64, yes: bool, cx: &mut Context<Self>) {
        let me = self.model.me.clone();
        let Some(w) = self.watches.get_mut(conv) else { return };
        let sid = w.sid.clone();
        if let Some(p) = w.proposals.iter_mut().find(|p| p.pid == pid) {
            if yes {
                p.yes.insert(me);
            } else {
                p.no.insert(me);
            }
        }
        self.send_watch(conv, &sid, crate::watch::WatchMsg::Vote { pid, yes });
        cx.notify();
    }

    fn apply_watch(&mut self, conv: &str, action: &crate::watch::Action, at: u64, cx: &mut Context<Self>) {
        let Some(w) = self.watches.get_mut(conv) else { return };
        if w.apply(action, at) {
            self.watches.remove(conv);
            self.show_flash("watching together ended", cx);
        } else {
            w.sync_player();
            if let crate::watch::Action::Switch { item } = action {
                self.show_flash(format!("now: {}", item.title), cx);
            }
        }
        cx.notify();
    }

    /// The player reported something.
    fn on_player(&mut self, conv: &str, ev: crate::watch::PlayerEvent, cx: &mut Context<Self>) {
        use crate::watch::PlayerEvent;
        match ev {
            PlayerEvent::Changed => {
                if let Some(action) = self.watches.get(conv).and_then(|w| w.local_action()) {
                    self.propose(conv, action, cx);
                }
            }
            PlayerEvent::Ended => {
                // The leader moves on to the queue's next: no vote for the natural order.
                let next = self.watches.get(conv).is_some_and(|w| w.leader == self.model.me && !w.queue.is_empty());
                if next {
                    let sid = self.watches[conv].sid.clone();
                    let at = crate::watch::now_ms();
                    self.send_watch(conv, &sid, crate::watch::WatchMsg::Apply { pid: 0, action: crate::watch::Action::Skip, at_ms: at });
                    self.apply_watch(conv, &crate::watch::Action::Skip, at, cx);
                }
            }
            PlayerEvent::Closed => {
                if self.watches.get(conv).is_some_and(|w| w.player.is_some()) {
                    self.leave_watch(conv, cx);
                    self.show_flash("you left watching together", cx);
                }
            }
        }
    }

    /// Every half second: the leader's state against drift, and old proposals go.
    fn tick_watch(&mut self) {
        let me = self.model.me.clone();
        let mut out = Vec::new();
        for (conv, w) in self.watches.iter_mut() {
            w.proposals.retain(|p| p.at.elapsed() < crate::watch::PROPOSAL_TTL);
            // Our own player keeps to the agreed time (it may lag while a video loads).
            if w.joined && w.last_sync.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
                w.last_sync = Some(Instant::now());
                w.sync_player();
            }
            if w.leader == me && w.joined && w.last_state.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
                w.last_state = Some(Instant::now());
                out.push((conv.clone(), w.sid.clone(), crate::watch::WatchMsg::State { playing: w.playing, pos: w.position(), at_ms: crate::watch::now_ms() }));
            }
        }
        for (conv, sid, m) in out {
            self.send_watch(&conv, &sid, m);
        }
    }

    fn on_watch(&mut self, peer: &str, wire: crate::watch::Wire, cx: &mut Context<Self>) {
        use crate::watch::WatchMsg;
        let crate::watch::Wire { conv, sid, m } = wire;
        // Only members of that conversation.
        if !self.model.conversations.get(&conv).is_some_and(|c| c.peers.iter().any(|p| bare_uri(p) == peer)) {
            return;
        }
        let who = self.model.name_of(peer);
        if let WatchMsg::Start { item } = &m {
            notify_desktop(&format!("{who} wants to watch together"), &item.title);
            self.show_flash(format!("{who} wants to watch “{}” together", item.title), cx);
            self.watches.insert(conv.clone(), crate::watch::Session::new(&conv, &sid, peer, item.clone()));
            cx.notify();
            return;
        }
        let me = self.model.me.clone();
        let Some(w) = self.watches.get_mut(&conv).filter(|w| w.sid == sid) else { return };
        match m {
            WatchMsg::Start { .. } => {}
            WatchMsg::Join => {
                w.participants.insert(peer.to_owned());
                // The leader brings the newcomer up to date.
                if w.leader == me {
                    let sync = WatchMsg::Sync {
                        participants: w.participants.iter().cloned().collect(),
                        current: w.current.clone(),
                        queue: w.queue.clone(),
                        playing: w.playing,
                        pos: w.position(),
                        at_ms: crate::watch::now_ms(),
                    };
                    self.send_watch(&conv, &sid, sync);
                }
                self.show_flash(format!("{who} joined"), cx);
            }
            WatchMsg::Leave => {
                w.participants.remove(peer);
                self.show_flash(format!("{who} left"), cx);
            }
            WatchMsg::Sync { participants, current, queue, playing, pos, at_ms } => {
                w.participants = participants.into_iter().collect();
                if w.joined {
                    w.participants.insert(me);
                }
                (w.current, w.queue, w.playing, w.pos, w.at_ms) = (current, queue, playing, pos, at_ms);
                w.sync_player();
            }
            WatchMsg::State { playing, pos, at_ms } => {
                if peer == w.leader {
                    (w.playing, w.pos, w.at_ms) = (playing, pos, at_ms);
                    w.sync_player();
                }
            }
            WatchMsg::Queue { item } => {
                if !w.queue.iter().any(|q| q.url == item.url) {
                    w.queue.push(item);
                }
            }
            WatchMsg::Propose { pid, action } => {
                w.proposals.push(crate::watch::Proposal { pid, by: peer.to_owned(), action, yes: [peer.to_owned()].into(), no: Default::default(), at: Instant::now() });
            }
            WatchMsg::Vote { pid, yes } => {
                let Some(i) = w.proposals.iter().position(|p| p.pid == pid) else { return };
                if yes {
                    w.proposals[i].yes.insert(peer.to_owned());
                } else {
                    w.proposals[i].no.insert(peer.to_owned());
                }
                // Only the proposer settles it.
                let p = w.proposals[i].clone();
                if p.by == me {
                    if w.passes(p.yes.len()) {
                        w.proposals.remove(i);
                        let at = crate::watch::now_ms();
                        self.send_watch(&conv, &sid, WatchMsg::Apply { pid, action: p.action.clone(), at_ms: at });
                        self.apply_watch(&conv, &p.action, at, cx);
                        return;
                    }
                    if w.fails(p.no.len()) {
                        w.proposals.remove(i);
                        self.show_flash(format!("not accepted: {}", p.action.label()), cx);
                    }
                }
            }
            WatchMsg::Apply { pid, action, at_ms } => {
                w.proposals.retain(|p| p.pid != pid);
                self.apply_watch(&conv, &action, at_ms, cx);
                return;
            }
        }
        cx.notify();
    }

    /// `@youtube <query>`: search, then send the first results as a message everyone sees.
    fn send_youtube_search(&mut self, conv: &str, query: &str, cx: &mut Context<Self>) {
        let (tx, conv, query) = (self.tx.clone(), conv.to_owned(), query.to_owned());
        self.composer.update(cx, |c, cx| c.clear(cx));
        self.show_flash(format!("searching YouTube for “{query}”…"), cx);
        std::thread::spawn(move || {
            let r = crate::media::search(&query, 5).map(|v| crate::watch::results_message(&query, &v));
            let _ = tx.unbounded_send(AppMsg::SearchDone(conv, r));
        });
    }

    // ---- Drawing on a shared screen ------------------------------------------------

    /// Send a drawing message to the share being watched.
    pub fn ink(&mut self, msg: joma_desk::ink::Ink, cx: &mut Context<Self>) {
        if let Some(v) = self.viewing_key().and_then(|k| self.desk.viewer(&k)) {
            v.ink(msg);
            cx.notify();
        }
    }

    /// The pointer on the stage at a fraction (x, y) of the picture.
    pub fn ink_move(&mut self, x: f32, y: f32, pressed: bool, cx: &mut Context<Self>) {
        use joma_desk::ink::Ink;
        match self.ink_tool {
            InkTool::Laser => {
                // About 30 a second is enough for a smooth dot.
                if self.ink_laser_at.is_none_or(|t| t.elapsed() > Duration::from_millis(30)) {
                    self.ink_laser_at = Some(Instant::now());
                    self.ink(Ink::Laser { x, y }, cx);
                }
            }
            InkTool::Pen if pressed => {
                if let Some(id) = self.ink_stroke {
                    self.ink(Ink::Stroke { id, color: self.ink_color, width: 5., pts: vec![(x, y)] }, cx);
                }
            }
            _ => {}
        }
    }

    pub fn ink_down(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        if self.ink_tool == InkTool::Pen {
            let id = self.ink_next;
            self.ink_next += 1;
            self.ink_stroke = Some(id);
            self.ink(joma_desk::ink::Ink::Stroke { id, color: self.ink_color, width: 5., pts: vec![(x, y)] }, cx);
        }
    }

    pub fn set_ink_tool(&mut self, tool: InkTool, cx: &mut Context<Self>) {
        if self.ink_tool == InkTool::Laser && tool != InkTool::Laser {
            self.ink(joma_desk::ink::Ink::LaserOff, cx);
        }
        self.ink_tool = if self.ink_tool == tool { InkTool::None } else { tool };
        cx.notify();
    }

    /// The Joma share of ours that runs (the open conversation's first).
    pub fn sharing_key(&self) -> Option<String> {
        let mut keys: Vec<String> = self.desk.sessions.keys().filter(|k| self.desk.shared(k).is_some()).cloned().collect();
        keys.sort();
        let here = self.active_call().map(|c| c.id.clone());
        keys.iter().find(|k| Some(*k) == here.as_ref()).or(keys.first()).cloned()
    }

    /// Our own tool on the screen we share: the same one again puts it away.
    pub fn set_sharer_tool(&mut self, key: &str, tool: joma_desk::ink::Tool, cx: &mut Context<Self>) {
        use joma_desk::ink::Tool;
        if let Some((share, _)) = self.desk.shared(key) {
            let (now, _) = share.ink_state();
            let same = matches!((now, tool), (Tool::Laser, Tool::Laser) | (Tool::Pen(_), Tool::Pen(_)));
            let next = if same { Tool::Off } else { tool };
            share.set_tool(next);
            if next != Tool::Off {
                self.show_flash("drawing on the shared screen: a right click there puts the tool away", cx);
            }
        }
        cx.notify();
    }

    /// Another colour for our pen on the shared screen (and for the next strokes).
    pub fn set_sharer_color(&mut self, key: &str, color: u32, cx: &mut Context<Self>) {
        self.ink_color = color;
        if let Some((share, _)) = self.desk.shared(key) {
            share.set_tool(joma_desk::ink::Tool::Pen(color));
        }
        cx.notify();
    }

    /// The conversation a share belongs to.
    fn share_conversation(&self, key: &str) -> Option<String> {
        match crate::desk::direct_peer(key) {
            Some(peer) => self.model.chat_with(peer).map(|c| c.id.clone()),
            None => self.model.calls.get(key).and_then(|c| c.conversation.clone()),
        }
    }

    /// A screenshot of the shared screen with the drawings on it (ours, or the one we
    /// watch), saved in `~/Pictures/joma` and sent to the conversation.
    pub fn share_screenshot(&mut self, cx: &mut Context<Self>) {
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else { return };
        let dir = home.join("Pictures/joma");
        let path = dir.join(format!("share-{}.png", file_stamp()));
        let tx = self.tx.clone();
        let done = move |conv: Option<String>, r: anyhow::Result<()>, path: &std::path::Path| {
            let _ = tx.unbounded_send(AppMsg::Shot { conv, result: r.map(|()| path.to_string_lossy().into_owned()).map_err(|e| format!("{e:#}")) });
        };
        if let Some(key) = self.sharing_key() {
            let Some((share, _)) = self.desk.shared(&key) else { return };
            let (source, conv) = (share.source.clone(), self.share_conversation(&key));
            std::thread::spawn(move || {
                let r = (|| -> anyhow::Result<()> {
                    std::fs::create_dir_all(&dir)?;
                    // The compositor's picture of the output has the overlay's drawings.
                    let (output, place) = joma_desk::share::place_of(&source);
                    let out = std::process::Command::new("grim").args(["-o", &output]).arg(&path).output()?;
                    anyhow::ensure!(out.status.success(), "grim: {}", String::from_utf8_lossy(&out.stderr).trim());
                    if place != (0., 0., 1., 1.) {
                        let img = image::open(&path)?;
                        let (w, h) = (img.width() as f32, img.height() as f32);
                        let (x, y) = ((place.0 * w).max(0.) as u32, (place.1 * h).max(0.) as u32);
                        let (cw, ch) = (((place.2 * w) as u32).min(img.width().saturating_sub(x)).max(1), ((place.3 * h) as u32).min(img.height().saturating_sub(y)).max(1));
                        img.crop_imm(x, y, cw, ch).save(&path)?;
                    }
                    Ok(())
                })();
                done(conv, r, &path);
            });
        } else if let Some(key) = self.viewing_key() {
            let (Some(frame), Some(viewer)) = (self.desk_frame.as_ref(), self.desk.viewer(&key)) else {
                return self.show_flash("no picture of the shared screen yet", cx);
            };
            let size = frame.image.size(0);
            let (w, h) = (size.width.0 as usize, size.height.0 as usize);
            let mut bgra = frame.image.as_bytes(0).map(<[u8]>::to_vec).unwrap_or_default();
            let canvas = viewer.state.lock().unwrap().ink.clone();
            let conv = self.share_conversation(&key);
            std::thread::spawn(move || {
                let r = (|| -> anyhow::Result<()> {
                    std::fs::create_dir_all(&dir)?;
                    anyhow::ensure!(bgra.len() >= w * h * 4 && w > 0, "the picture is not ready");
                    joma_desk::ink::paint_over(&mut bgra, w, h, &canvas);
                    for p in bgra.chunks_exact_mut(4) {
                        p.swap(0, 2);
                        p[3] = 255;
                    }
                    image::RgbaImage::from_raw(w as u32, h as u32, bgra).ok_or_else(|| anyhow::anyhow!("bad picture"))?.save(&path)?;
                    Ok(())
                })();
                done(conv, r, &path);
            });
        } else {
            self.show_flash("no shared screen to take a picture of", cx);
        }
    }

    /// Clear the drawing of the share we watch, or of ours.
    pub fn clear_ink(&mut self, key: Option<&str>, cx: &mut Context<Self>) {
        match key {
            Some(k) => {
                if let Some((share, _)) = self.desk.shared(k) {
                    share.clear_ink();
                }
            }
            None => self.ink(joma_desk::ink::Ink::Clear, cx),
        }
        cx.notify();
    }

    // ---- Voice and video messages, emoji, YouTube ---------------------------------

    /// Start recording a message for the open conversation (`video`: from the camera).
    pub fn start_recording(&mut self, video: bool, cx: &mut Context<Self>) {
        if self.recording.is_some() {
            return;
        }
        let Some(conv) = self.selected.clone() else { return };
        let input = if video { camera_source() } else { String::new() };
        if video && input.is_empty() {
            return self.show_flash("no camera to record with", cx);
        }
        let dir = std::env::current_dir().unwrap_or_default().join("recorded");
        let _ = std::fs::create_dir_all(&dir);
        let stamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let base = dir.join(format!("{}-{stamp}", if video { "video" } else { "voice" }));
        match joma_jami::start_recorder(&input, &base.to_string_lossy()) {
            Ok(path) if !path.is_empty() => {
                self.recording = Some(Recording { conv, path, started: Instant::now(), video });
            }
            Ok(_) => self.show_flash("could not start recording", cx),
            Err(e) => self.show_flash(format!("could not record: {e}"), cx),
        }
        cx.notify();
    }

    /// Stop recording; send what was recorded, or throw it away. Stopping waits for the
    /// recorder's threads, so it runs off the UI thread.
    pub fn stop_recording(&mut self, send: bool, cx: &mut Context<Self>) {
        let Some(r) = self.recording.take() else { return };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = joma_jami::stop_recorder(&r.path);
            // The file is complete once the recorder closed it.
            for _ in 0..20 {
                if std::fs::metadata(&r.path).is_ok_and(|m| m.len() > 0) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            std::thread::sleep(Duration::from_millis(300));
            let _ = tx.unbounded_send(AppMsg::Recorded { conv: r.conv, path: r.path, send });
        });
        if send {
            self.show_flash("sending the recording…", cx);
        }
        cx.notify();
    }

    /// Play a voice or video message with mpv, or stop the one playing.
    /// Play or pause a voice or video message inside Joma (another one stops).
    pub fn toggle_play(&mut self, file_id: &str, path: &std::path::Path, cx: &mut Context<Self>) {
        if let Some(p) = self.player.as_mut().filter(|p| p.file_id == file_id) {
            if p.playing() {
                p.pause();
            } else if let Err(e) = p.play() {
                self.show_flash(format!("cannot play it: {e:#}"), cx);
            }
            cx.notify();
            return;
        }
        self.player = None;
        self.player_frame = None;
        self.video_big = false;
        match crate::player::Player::new(file_id, path).and_then(|mut p| p.play().map(|_| p)) {
            Ok(p) => self.player = Some(p),
            Err(e) => self.show_flash(format!("cannot play it: {e:#}"), cx),
        }
        cx.notify();
    }

    /// Jump to `frac` (0 to 1) of a message, starting it when it is not the one playing.
    pub fn seek_media(&mut self, file_id: &str, path: &std::path::Path, frac: f64, cx: &mut Context<Self>) {
        if self.player.as_ref().is_none_or(|p| p.file_id != file_id) {
            self.toggle_play(file_id, path, cx);
        }
        if let Some(p) = self.player.as_mut().filter(|p| p.file_id == file_id) {
            let to = frac.clamp(0., 1.) * p.info.duration;
            if let Err(e) = p.seek(to) {
                self.show_flash(format!("cannot play it: {e:#}"), cx);
            }
        }
        cx.notify();
    }

    /// Where the message `file_id` is (seconds), playing or paused there; 0 otherwise.
    pub fn media_position(&self, file_id: &str) -> (bool, f64) {
        self.player.as_ref().filter(|p| p.file_id == file_id).map_or((false, 0.), |p| (p.playing(), p.position()))
    }

    /// Measure a voice or video message once, in the background.
    pub fn want_media(&mut self, file_id: &str, path: &std::path::Path) {
        if self.media.contains_key(file_id) {
            return;
        }
        self.media.insert(file_id.to_owned(), None);
        let (tx, id, path) = (self.tx.clone(), file_id.to_owned(), path.to_owned());
        std::thread::spawn(move || {
            let view = (|| -> anyhow::Result<MediaView> {
                let info = crate::player::probe(&path)?;
                let bars = if info.audio { crate::player::waveform(&path).unwrap_or_default() } else { Vec::new() };
                let thumb = if info.video.is_some() { crate::player::thumbnail(&id, &path, &info).ok() } else { None };
                Ok(MediaView { info, bars, thumb })
            })();
            let _ = tx.unbounded_send(AppMsg::Media(id, view.ok()));
        });
    }

    pub fn insert_emoji(&mut self, e: &str, cx: &mut Context<Self>) {
        self.composer.update(cx, |c, cx| c.insert(e, cx));
        self.focus_composer = true;
        cx.notify();
    }

    /// The preview of a YouTube link: cached, or fetched once in the background.
    pub fn youtube_preview(&self, id: &str) -> Option<crate::media::Preview> {
        if let Some(p) = self.previews_yt.get(id) {
            return p.clone();
        }
        crate::media::cached(id)
    }

    /// Load a transcript made earlier (looked for once per message).
    pub fn want_transcript(&mut self, file_id: &str) {
        if self.transcripts_seen.insert(file_id.to_owned()) {
            if let Some(text) = crate::media::cached_transcript(file_id) {
                self.transcripts.insert(file_id.to_owned(), Some(Ok(text)));
            }
            if let Some(c) = crate::media::cached_cues(file_id) {
                self.cues.insert(file_id.to_owned(), c);
            }
        }
    }

    /// The open conversation's voice and video messages: download those not here (one sent before
    /// this device joined, or while Joma was off), and transcribe them one at a time when
    /// a transcriber is set up. Each is tried once by itself; the buttons try again.
    pub fn voice_messages(&mut self, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.as_ref().and_then(|s| self.model.conversations.get(s)) else { return };
        let conv_id = conv.id.clone();
        self.absorb_transcripts(&conv_id);
        let Some(conv) = self.model.conversations.get(&conv_id) else { return };
        let me_uri = self.model.me.clone();
        let voices: Vec<(String, FileRef, i64, bool)> = conv
            .messages
            .iter()
            .filter_map(|m| Some((m.id.clone(), m.file.clone()?, m.ts, bare_uri(&m.author) == me_uri)))
            .filter(|(_, f, _, _)| media_name(&f.name))
            .collect();
        let auto = self.quick.transcriber != "off" && self.quick.transcribe != "off" && crate::media::transcriber(&self.quick.transcribe).is_some();
        let mut busy = self.transcripts.values().any(Option::is_none);
        for (msg, f, ts, mine) in voices {
            self.want_transcript(&f.id);
            // Only what arrives from now on is transcribed by itself; the cc button does
            // the older ones.
            // Another device transcribes for all: give its words three minutes to arrive.
            let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
            if !mine && self.quick.transcriber != "all" && self.other_transcribes_for_all() && now - ts < 180 {
                self.want_transcript(&f.id);
                continue;
            }
            if ts < self.started_ts - 120 {
                if self.saved_file(&conv_id, &f.id).is_none() && f.size > 0 && f.size <= AUTO_DOWNLOAD && !self.downloads.contains_key(&f.id) && self.voice_fetched.insert(f.id.clone()) {
                    self.download(&conv_id, &msg, &f, cx);
                }
                if let Some(path) = self.saved_file(&conv_id, &f.id) {
                    self.want_media(&f.id, &path);
                }
                continue;
            }
            self.maybe_ship(&conv_id, &msg, &f);
            match self.saved_file(&conv_id, &f.id) {
                None => {
                    if f.size > 0 && f.size <= AUTO_DOWNLOAD && !self.downloads.contains_key(&f.id) && self.voice_fetched.insert(f.id.clone()) {
                        self.download(&conv_id, &msg, &f, cx);
                    }
                }
                Some(path) => {
                    self.want_media(&f.id, &path);
                    if auto && !busy && !self.transcripts.contains_key(&f.id) && self.voice_transcribed.insert(f.id.clone()) {
                        self.transcribe(&f.id, &path, cx);
                        busy = true;
                    }
                }
            }
        }
    }

    /// Replies that carry a voice or video message's transcript (📝): the words go under
    /// that message (and into the cache).
    fn absorb_transcripts(&mut self, conv: &str) {
        let Some(c) = self.model.conversations.get(conv) else { return };
        let found: Vec<(String, String)> = c
            .messages
            .iter()
            .filter_map(|m| {
                let text = m.text.strip_prefix(crate::media::MARK)?;
                let parent = c.messages.iter().find(|p| Some(&p.id) == m.reply_to.as_ref())?;
                Some((parent.file.as_ref()?.id.clone(), text.trim().to_owned()))
            })
            .collect();
        for (file, text) in found {
            if !matches!(self.transcripts.get(&file), Some(Some(Ok(_)))) {
                crate::media::save_transcript(&file, &text);
                self.transcripts.insert(file, Some(Ok(text)));
            }
        }
    }

    /// Is `m` a transcript reply (hidden: its words show under the message it answers)?
    pub fn is_transcript_reply(conv: &crate::model::Conversation, m: &crate::model::Msg) -> bool {
        m.text.starts_with(crate::media::MARK) && m.reply_to.as_ref().is_some_and(|r| conv.messages.iter().any(|p| p.id == *r && p.file.is_some()))
    }

    /// Our own new voice or video message: attach its transcript once it is made (sent
    /// from this device, or from any of ours when this one transcribes for all).
    fn maybe_ship(&mut self, conv: &str, msg: &str, f: &FileRef) {
        if !self.quick.send_transcripts || self.ship.contains_key(&f.id) {
            return;
        }
        let Some(c) = self.model.conversations.get(conv) else { return };
        let Some(m) = c.messages.iter().find(|m| m.id == msg) else { return };
        if bare_uri(&m.author) != self.model.me {
            return;
        }
        let here = self.sent_media.contains(&f.name);
        let all = self.quick.transcriber == "all";
        let answered = c.messages.iter().any(|r| r.reply_to.as_deref() == Some(msg) && r.text.starts_with(crate::media::MARK));
        if answered || !(here || all) {
            return;
        }
        self.ship.insert(f.id.clone(), (conv.to_owned(), msg.to_owned()));
    }

    /// A transcript was made: attach it to our own message, or bring it to our other
    /// devices when this one transcribes for all of them.
    fn after_transcript(&mut self, file: &str, text: &str) {
        let acc = self.model.account.clone();
        if let Some((conv, msg)) = self.ship.get(file).cloned() {
            let answered = self.model.conversations.get(&conv).is_some_and(|c| c.messages.iter().any(|r| r.reply_to.as_deref() == Some(msg.as_str()) && r.text.starts_with(crate::media::MARK)));
            if !answered {
                if let Err(e) = joma_jami::send_message(&acc, &conv, &format!("{}{}", crate::media::MARK, text.trim()), &msg) {
                    tracing::warn!(error = %e, "could not attach the transcript");
                }
            }
            return;
        }
        if self.quick.transcriber == "all" {
            let mut p = joma_jami::Details::new();
            p.insert(crate::media::TRANSCRIPT_MIME.into(), serde_json::json!({"file": file, "text": text, "by": self.model.device}).to_string());
            let me = self.model.me.clone();
            let _ = joma_jami::send_account_message(&acc, &me, &p, false);
        }
    }

    /// A device that transcribes for all: every conversation's new voice and video
    /// messages, one at a time.
    fn transcriber_tick(&mut self, cx: &mut Context<Self>) {
        if self.quick.transcriber != "all" || self.transcripts.values().any(Option::is_none) || crate::media::transcriber(&self.quick.transcribe).is_none() {
            return;
        }
        let since = self.started_ts - 120;
        let todo: Vec<(String, String, FileRef)> = self
            .model
            .conversations
            .values()
            .flat_map(|c| c.messages.iter().filter(|m| m.ts >= since).filter_map(move |m| Some((c.id.clone(), m.id.clone(), m.file.clone()?))))
            .filter(|(_, _, f)| media_name(&f.name))
            .collect();
        for (conv, msg, f) in todo {
            self.want_transcript(&f.id);
            self.absorb_transcripts(&conv);
            if self.transcripts.contains_key(&f.id) || self.voice_transcribed.contains(&f.id) {
                continue;
            }
            match self.saved_file(&conv, &f.id) {
                None => {
                    if f.size > 0 && f.size <= AUTO_DOWNLOAD && !self.downloads.contains_key(&f.id) && self.voice_fetched.insert(f.id.clone()) {
                        self.download(&conv, &msg, &f, cx);
                    }
                }
                Some(path) => {
                    self.maybe_ship(&conv, &msg, &f);
                    self.voice_transcribed.insert(f.id.clone());
                    self.transcribe(&f.id, &path, cx);
                    return;
                }
            }
        }
    }

    /// The cc button: transcribe this message now (an older one), or show a video's
    /// subtitles.
    pub fn cc(&mut self, conv: &str, file: &FileRef, video: bool, cx: &mut Context<Self>) {
        let made = matches!(self.transcripts.get(&file.id), Some(Some(Ok(_))));
        if video && made {
            if !self.subs_on.remove(&file.id) {
                self.subs_on.insert(file.id.clone());
            }
            return cx.notify();
        }
        if video {
            self.subs_on.insert(file.id.clone());
        }
        match self.saved_file(conv, &file.id) {
            Some(path) => self.transcribe(&file.id, &path, cx),
            None => self.show_flash("the message is still arriving", cx),
        }
    }

    pub fn set_transcriber(&mut self, role: &str, cx: &mut Context<Self>) {
        self.quick.transcriber = role.to_owned();
        self.save_quick(cx);
        self.tell_role();
    }

    pub fn set_send_transcripts(&mut self, on: bool, cx: &mut Context<Self>) {
        self.quick.send_transcripts = on;
        self.save_quick(cx);
    }

    /// Transcribe a voice message, in the background (Superwhisper).
    pub fn transcribe(&mut self, file_id: &str, path: &std::path::Path, cx: &mut Context<Self>) {
        let Some(engine) = crate::media::transcriber(&self.quick.transcribe) else {
            return self.show_flash("no transcription: Superwhisper is not installed, or not set to work offline (settings → audio → superwhisper uses it in any mode)", cx);
        };
        if matches!(self.transcripts.get(file_id), Some(None)) {
            return;
        }
        self.transcripts.insert(file_id.to_owned(), None);
        let (tx, id, path) = (self.tx.clone(), file_id.to_owned(), path.to_owned());
        std::thread::spawn(move || {
            let r = (|| -> anyhow::Result<String> {
                crate::media::transcribe(engine, &id, &path)
            })();
            let _ = tx.unbounded_send(AppMsg::Transcribed(id, r.map_err(|e| format!("{e:#}"))));
        });
        cx.notify();
    }

    /// Where the picker's GIFs are.
    pub fn gif_dir(&self) -> std::path::PathBuf {
        if self.quick.gif_dir.is_empty() {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Pictures/gifs")
        } else {
            std::path::PathBuf::from(&self.quick.gif_dir)
        }
    }

    /// Open or close the GIF picker (it reads the folder each time it opens).
    pub fn toggle_gifs(&mut self, cx: &mut Context<Self>) {
        self.gifs = match self.gifs.take() {
            Some(_) => None,
            None => {
                let mut list: Vec<std::path::PathBuf> = std::fs::read_dir(self.gif_dir())
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "gif" | "webp")))
                    .collect();
                list.sort();
                Some(list)
            }
        };
        self.emoji_open = false;
        cx.notify();
    }

    /// Send a GIF of the picker to the open conversation.
    pub fn send_gif(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.gifs = None;
        let Some(conv) = self.selected.clone() else { return };
        if let Reply::Error { message } = self.send_file(&conv, &path.to_string_lossy()) {
            self.show_flash(format!("not sent: {message}"), cx);
        }
        self.pinned_bottom = true;
        cx.notify();
    }

    pub fn set_transcribe(&mut self, engine: &str, cx: &mut Context<Self>) {
        self.quick.transcribe = engine.to_owned();
        self.save_quick(cx);
    }

    pub fn want_youtube_preview(&mut self, id: &str) {
        if self.previews_yt.contains_key(id) {
            return;
        }
        if let Some(p) = crate::media::cached(id) {
            self.previews_yt.insert(id.to_owned(), Some(p));
            return;
        }
        self.previews_yt.insert(id.to_owned(), None);
        let tx = self.tx.clone();
        let id = id.to_owned();
        std::thread::spawn(move || {
            if crate::media::fetch(&id).is_ok() {
                let _ = tx.unbounded_send(AppMsg::Preview(id));
            }
        });
    }

    // ---- Settings -----------------------------------------------------------------

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        // The same button closes the page.
        if self.settings.take().is_some() {
            cx.notify();
            return;
        }
        let (inputs, outputs, current) = joma_jami::audio_devices().unwrap_or_default();
        let (noise, echo, agc) = joma_jami::audio_processing().unwrap_or_default();
        let cameras = joma_jami::video_devices().unwrap_or_default();
        let camera = joma_jami::default_video_device().unwrap_or_default();
        let mut v = SettingsView { inputs, outputs, current, noise, echo, agc, cameras, camera, section: 1, ..Default::default() };
        self.load_camera_modes(&mut v);
        self.settings = Some(v);
        cx.notify();
    }

    fn load_camera_modes(&self, v: &mut SettingsView) {
        v.sizes.clear();
        if v.camera.is_empty() {
            return;
        }
        let caps = joma_jami::camera_capabilities(&v.camera).unwrap_or_default();
        v.camera_settings = joma_jami::camera_settings(&v.camera).unwrap_or_default();
        let channel = v.camera_settings.get("channel").cloned().filter(|c| caps.contains_key(c)).or_else(|| caps.keys().next().cloned());
        if let Some(sizes) = channel.and_then(|c| caps.get(&c).cloned()) {
            let area = |s: &str| s.split_once('x').map(|(w, h)| w.parse::<u64>().unwrap_or(0) * h.parse::<u64>().unwrap_or(0)).unwrap_or(0);
            let mut list: Vec<(String, Vec<String>)> = sizes.into_iter().collect();
            list.sort_by_key(|(s, _)| std::cmp::Reverse(area(s)));
            v.sizes = list;
        }
    }

    pub fn set_audio(&mut self, kind: &str, index: usize, cx: &mut Context<Self>) {
        if let Err(e) = joma_jami::set_audio_device(kind, index) {
            self.show_flash(format!("could not change the device: {e}"), cx);
        }
        self.open_settings(cx);
    }

    pub fn set_processing(&mut self, what: &str, value: &str, cx: &mut Context<Self>) {
        let r = match what {
            "noise" => joma_jami::set_noise_suppression(value),
            "echo" => joma_jami::set_echo_cancellation(value),
            _ => joma_jami::set_agc(value == "on"),
        };
        if let Err(e) = r {
            self.show_flash(format!("could not change it: {e}"), cx);
        }
        self.open_settings(cx);
    }

    pub fn set_camera(&mut self, device: &str, cx: &mut Context<Self>) {
        if let Err(e) = joma_jami::set_default_camera(device) {
            self.show_flash(format!("could not change the camera: {e}"), cx);
        }
        self.open_settings(cx);
    }

    pub fn set_camera_mode(&mut self, size: Option<&str>, rate: Option<&str>, cx: &mut Context<Self>) {
        let Some(v) = self.settings.as_ref() else { return };
        let mut st = v.camera_settings.clone();
        if let Some(s) = size {
            st.insert("size".into(), s.to_owned());
            // Keep the rate if that size has it, else its first.
            let rates = v.sizes.iter().find(|(z, _)| z == s).map(|(_, r)| r.clone()).unwrap_or_default();
            if !st.get("rate").is_some_and(|r| rates.contains(r)) {
                if let Some(r) = rates.first() {
                    st.insert("rate".into(), r.clone());
                }
            }
        }
        if let Some(r) = rate {
            st.insert("rate".into(), r.to_owned());
        }
        let cam = v.camera.clone();
        if let Err(e) = joma_jami::apply_camera_settings(&cam, &st) {
            self.show_flash(format!("could not change the camera: {e}"), cx);
        }
        self.open_settings(cx);
    }

    /// Choose a video quality preset: applied now and at every start.
    pub fn set_video_quality(&mut self, q: &str, cx: &mut Context<Self>) {
        self.apply_video_quality(q);
        self.quick.video_quality = q.to_owned();
        self.save_quick(cx);
        cx.notify();
    }

    /// Set this account's video codecs to a preset. The daemon keeps codec details in
    /// memory only, so this runs whenever an account opens.
    fn apply_video_quality(&self, q: &str) {
        let acc = self.model.account.clone();
        for (id, mut d) in joma_jami::codecs(&acc).unwrap_or_default() {
            if d.get("CodecInfo.type").map(String::as_str) != Some("VIDEO") {
                continue;
            }
            match quality_bitrate(q) {
                Some(b) => {
                    d.insert("CodecInfo.autoQualityEnabled".into(), "false".into());
                    d.insert("CodecInfo.bitrate".into(), b.to_string());
                }
                None => {
                    d.insert("CodecInfo.autoQualityEnabled".into(), "true".into());
                }
            }
            let _ = joma_jami::set_codec_details(&acc, id, &d);
        }
    }

    /// Ask GitHub which releases exist (in the background). Test profiles only with
    /// `JOMA_RELEASES_URL` (a file of the same JSON).
    pub fn check_updates(&mut self, by_hand: bool, cx: &mut Context<Self>) {
        self.update_checked = Some(Instant::now());
        let url = match std::env::var("JOMA_RELEASES_URL") {
            Ok(u) => u,
            Err(_) if own_profile() => return,
            Err(_) => "https://api.github.com/repos/dzanaga/joma-messenger/releases?per_page=30".into(),
        };
        if by_hand {
            self.update_note = "checking…".into();
            cx.notify();
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = (|| -> anyhow::Result<Vec<String>> {
                let out = std::process::Command::new("curl").args(["-fsSL", "--max-time", "20", "-A", "joma", "-H", "Accept: application/vnd.github+json", &url]).output()?;
                anyhow::ensure!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr).trim().to_owned());
                let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
                Ok(v.as_array().map(|a| a.iter().filter(|r| r["draft"].as_bool() != Some(true)).filter_map(|r| r["tag_name"].as_str().map(str::to_owned)).collect()).unwrap_or_default())
            })();
            let _ = tx.unbounded_send(AppMsg::Releases(r.map_err(|e| format!("{e:#}"))));
        });
    }

    /// Stable releases only, or pre-releases too.
    pub fn set_prereleases(&mut self, on: bool, cx: &mut Context<Self>) {
        self.quick.prereleases = on;
        self.save_quick(cx);
        self.check_updates(true, cx);
    }

    /// Was this Joma installed with mise (so mise can install another version)?
    pub fn installed_with_mise() -> bool {
        crate::setup::exe().to_string_lossy().contains("/mise/shims/")
    }

    /// Install the update with mise, then restart.
    pub fn update_now(&mut self, cx: &mut Context<Self>) {
        let Some(v) = self.update.clone() else { return };
        if own_profile() {
            return self.show_flash(format!("a test profile does not update (would install {v})"), cx);
        }
        if !Self::installed_with_mise() {
            return self.show_flash(format!("this Joma was not installed with mise: mise use -g github:dzanaga/joma-messenger@{v}"), cx);
        }
        if self.updating {
            return;
        }
        self.updating = true;
        self.show_flash(format!("installing Joma {}…", joma_quick::version::pretty(&v)), cx);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let out = std::process::Command::new("mise").args(["use", "-g", &format!("github:dzanaga/joma-messenger@{v}")]).output();
            let r = match out {
                Ok(o) if o.status.success() => Ok(v),
                Ok(o) => Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("mise failed").to_owned()),
                Err(e) => Err(format!("mise: {e}")),
            };
            let _ = tx.unbounded_send(AppMsg::Updated(r));
        });
        cx.notify();
    }

    // ---- Replies, reactions, edits ---------------------------------------------------

    pub fn toggle_msg_actions(&mut self, id: &str, cx: &mut Context<Self>) {
        self.msg_actions = if self.msg_actions.as_deref() == Some(id) { None } else { Some(id.to_owned()) };
        cx.notify();
    }

    pub fn reply_to(&mut self, id: &str, cx: &mut Context<Self>) {
        self.replying = Some(id.to_owned());
        self.editing = None;
        self.msg_actions = None;
        self.focus_composer = true;
        cx.notify();
    }

    /// Put our message back in the message field to change it.
    pub fn edit_message(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(text) = self.selected.as_ref().and_then(|c| self.model.conversations.get(c)).and_then(|c| c.messages.iter().find(|m| m.id == id)).map(|m| m.text.clone()) else { return };
        self.editing = Some(id.to_owned());
        self.replying = None;
        self.msg_actions = None;
        self.composer.update(cx, |c, cx| c.set_text(&text, cx));
        self.focus_composer = true;
        cx.notify();
    }

    /// Take our message back, for everyone (click twice).
    pub fn delete_message(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.armed(&format!("msg:{id}"), "click again to delete the message for everyone", cx) {
            return;
        }
        let Some(conv) = self.selected.clone() else { return };
        if let Err(e) = joma_jami::edit_message(&self.model.account, &conv, id, "") {
            self.show_flash(format!("not deleted: {e}"), cx);
        }
        self.msg_actions = None;
        cx.notify();
    }

    /// Put an emoji on a message, or take ours back when it is there already.
    pub fn react(&mut self, id: &str, emoji: &str, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone() else { return };
        let mine = self
            .model
            .conversations
            .get(&conv)
            .and_then(|c| c.messages.iter().find(|m| m.id == id))
            .and_then(|m| m.reactions.iter().find(|r| r.emoji == emoji && crate::model::bare_uri(&r.author) == self.model.me))
            .map(|r| r.id.clone());
        let r = match mine {
            Some(reaction) => joma_jami::edit_message(&self.model.account, &conv, &reaction, ""),
            None => joma_jami::react(&self.model.account, &conv, id, emoji),
        };
        if let Err(e) = r {
            self.show_flash(format!("no reaction: {e}"), cx);
        }
        self.msg_actions = None;
        cx.notify();
    }

    /// Stop answering or editing.
    pub fn cancel_reply(&mut self, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            self.composer.update(cx, |c, cx| c.clear(cx));
        }
        self.replying = None;
        cx.notify();
    }

    /// `joma ui <what>`: open a part of the interface from a script.
    fn ui(&mut self, what: &str, cx: &mut Context<Self>) -> Reply {
        match what {
            "settings" => {
                self.settings = None;
                self.open_settings(cx)
            }
            s if s.starts_with("settings:") => {
                self.settings = None;
                self.open_settings(cx);
                if let (Some(v), Ok(n)) = (self.settings.as_mut(), s[9..].parse()) {
                    v.section = n;
                }
            }
            "profile" => self.open_profile(cx),
            "keys" => self.keys_open = true,
            "menu" => self.menu = Some(Menu::Me),
            "chat-menu" => self.menu = Some(Menu::Chat),
            "emoji" => self.emoji_open = !self.emoji_open,
            "gifs" => self.toggle_gifs(cx),
            "viewer" => self.viewer = self.pictures().pop().map(|(p, _)| p),
            // The actions of the newest text message of the open conversation.
            "msg" => {
                self.msg_actions = self.selected.as_ref().and_then(|c| self.model.conversations.get(c)).and_then(|c| c.messages.iter().rev().find(|m| m.kind == crate::model::MsgKind::Text)).map(|m| m.id.clone());
            }
            "quick" => self.quick_open = !self.quick_open,
            "close" => {
                self.settings = None;
                self.profile_edit = None;
                self.viewer = None;
                self.gifs = None;
                self.keys_open = false;
                self.menu = None;
                self.emoji_open = false;
                self.quick_open = false;
            }
            other => return Reply::Error { message: format!("no such part: {other}") },
        }
        cx.notify();
        Reply::Done
    }

    pub fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.menu = if self.menu == Some(menu) { None } else { Some(menu) };
        cx.notify();
    }

    pub fn set_share_fps(&mut self, fps: u32, cx: &mut Context<Self>) {
        self.quick.share_fps = fps;
        joma_desk::share::set_max_fps(fps);
        self.save_quick(cx);
        cx.notify();
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
        if self.tab == Tab::Files {
            return self.files_step(by, cx);
        }
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

    /// Files waiting beside the composer for the open conversation.
    pub fn attachments_pending(&self) -> bool {
        self.selected.as_ref().is_some_and(|c| self.attachments.get(c).is_some_and(|v| !v.is_empty()))
    }

    pub fn send_composed(&mut self, cx: &mut Context<Self>) {
        let Some(conv) = self.selected.clone() else { return };
        let files = self.attachments.remove(&conv).unwrap_or_default();
        let text = self.composer.read(cx).text();
        if let Some(q) = text.trim().strip_prefix("@youtube ").or_else(|| text.trim().strip_prefix("@yt ")).map(str::trim).filter(|q| !q.is_empty()) {
            let q = q.to_owned();
            return self.send_youtube_search(&conv, &q, cx);
        }
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
        // An edit replaces the message; a reply names the one it answers.
        if let Some(id) = self.editing.take() {
            match joma_jami::edit_message(&self.model.account, &conv, &id, text.trim_end()) {
                Ok(()) => self.composer.update(cx, |c, cx| c.clear(cx)),
                Err(e) => self.show_flash(format!("not changed: {e}"), cx),
            }
            cx.notify();
            return;
        }
        let reply = self.replying.take().unwrap_or_default();
        match joma_jami::send_message(&self.model.account, &conv, text.trim_end(), &reply) {
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
        if tab == Tab::Files {
            self.files.open = false;
            if self.sync.st.is_some() {
                self.refresh_sync_view();
            }
            self.files_read();
        }
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
            Tab::Calls | Tab::Devices | Tab::Accounts | Tab::Agents | Tab::Files => {}
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
            self.last_link = Some(Instant::now());
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
        let acc = self.model.account.clone();
        self.sync.stop(&acc);
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
        if self.mode == Mode::Password {
            // A password keeps its spaces.
            let text = self.input.read(cx).text();
            return self.password_entered(text, window, cx);
        }
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
                if let Some(login) = text.strip_prefix("gh:").or_else(|| text.strip_prefix("github:")) {
                    self.github_lookup(login.trim().to_owned(), cx);
                } else if text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit()) {
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
            Mode::ShareWith => {
                let Some(folder) = self.files.share_for.clone() else { return self.set_mode(Mode::Chat, window, cx) };
                if text.is_empty() {
                    return;
                }
                match self.share_with(&folder, &text) {
                    Ok(t) => {
                        self.files.share_for = None;
                        self.show_flash(t, cx);
                        self.set_mode(Mode::Chat, window, cx);
                        self.refresh_sync_view();
                    }
                    Err(e) => self.show_flash(e, cx),
                }
            }
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
            // Handled before the gate (see the top of this function).
            Mode::Password => {}
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

    /// Refuse the call that is ringing (not the one we may be in).
    pub fn decline_incoming(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.model.calls.values().find(|c| c.state == CallState::Incoming).cloned() else { return };
        if let Err(e) = joma_jami::refuse_call(&self.model.account, &call.id) {
            self.show_flash(e.to_string(), cx);
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
            Request::Ui { what } => self.ui(&what, cx),
            Request::Draw { what } => {
                // A screenshot also works on the watching side.
                if what == "shot" && (self.sharing_key().is_some() || self.viewing_key().is_some()) {
                    self.share_screenshot(cx);
                    return Reply::Done;
                }
                let Some(key) = self.sharing_key() else { return Reply::Error { message: "no Joma share of yours is running".into() } };
                use joma_desk::ink::Tool;
                match what.as_str() {
                    "laser" => self.set_sharer_tool(&key, Tool::Laser, cx),
                    "pen" => self.set_sharer_tool(&key, Tool::Pen(self.ink_color), cx),
                    "off" => self.set_sharer_tool(&key, Tool::Off, cx),
                    "clear" => self.clear_ink(Some(&key), cx),
                    "shot" => self.share_screenshot(cx),
                    other => return Reply::Error { message: format!("draw what? {other} (laser, pen, off, clear, shot)") },
                }
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
            Request::WatchTogether { to, url } => match self.target(&to) {
                Ok(conv) => {
                    self.select(conv, cx);
                    let title = crate::media::youtube_id(&url).and_then(|id| crate::media::cached(&id)).map(|p| p.title).unwrap_or_else(|| url.clone());
                    self.start_watch(&url, &title, cx);
                    Reply::Done
                }
                Err(e) => e,
            },
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
            Request::Transcription { what, arg, n, json } => {
                if self.gate != Gate::Ready {
                    return Reply::Error { message: "Joma is not on an account now".into() };
                }
                if what == "role" {
                    match arg.as_deref() {
                        Some("self") => self.set_transcriber("", cx),
                        Some(r @ ("all" | "off")) => self.set_transcriber(r, cx),
                        Some("send") => self.set_send_transcripts(true, cx),
                        Some("nosend") => self.set_send_transcripts(false, cx),
                        _ => {}
                    }
                    let role = match self.quick.transcriber.as_str() {
                        "all" => "for all your devices",
                        "off" => "off",
                        _ => "for itself",
                    };
                    let engine = crate::media::transcriber(&self.quick.transcribe).unwrap_or("none");
                    let devices = self.transcription_devices();
                    return Reply::Text {
                        text: if json {
                            serde_json::json!({
                                "role": if self.quick.transcriber.is_empty() { "self" } else { self.quick.transcriber.as_str() },
                                "engine": engine,
                                "send": self.quick.send_transcripts,
                                "devices": devices.iter().map(|(n, r, e)| serde_json::json!({"name": n, "role": r, "engine": e})).collect::<Vec<_>>(),
                            })
                            .to_string()
                        } else {
                            let mut t = format!("this device transcribes {role} · engine {engine} · your voice messages {}", if self.quick.send_transcripts { "carry their transcript" } else { "go alone" });
                            for (n, r, e) in devices.iter().skip(1) {
                                t.push_str(&format!("\n  {n}: {r} · {e}"));
                            }
                            t
                        },
                    };
                }
                let conv = match self.target(arg.as_deref().unwrap_or("")) {
                    Ok(c) => c,
                    Err(e) => return e,
                };
                let Some(f) = self.model.conversations.get(&conv).and_then(|c| c.messages.iter().rev().filter_map(|m| m.file.clone()).filter(|f| media_name(&f.name)).nth(n.max(1) - 1)) else {
                    return Reply::Error { message: "no voice or video message there".into() };
                };
                let video = crate::media::is_video(std::path::Path::new(&f.name));
                self.cc(&conv, &f, video, cx);
                Reply::Text { text: format!("transcribing {}", f.name) }
            }
            Request::Files { what, arg, path, json } => {
                if self.gate != Gate::Ready {
                    return Reply::Error { message: "Joma is not on an account now".into() };
                }
                self.files_request(&what, arg.as_deref().unwrap_or(""), path.as_deref(), json, cx)
            }
            Request::Contact { what, arg } => {
                if self.gate != Gate::Ready {
                    return Reply::Error { message: "Joma is not on an account now".into() };
                }
                let acc = self.model.account.clone();
                let arg = arg.unwrap_or_default();
                let requests: Vec<(String, String)> = self.model.conversations.values().filter(|c| c.request).map(|c| (c.id.clone(), c.title.clone())).collect();
                match what.as_str() {
                    "add" => {
                        let uri = arg.trim().to_lowercase();
                        if uri.len() != 40 || !uri.chars().all(|c| c.is_ascii_hexdigit()) {
                            return Reply::Error { message: "joma add <40-digit Jami ID>".into() };
                        }
                        match joma_jami::add_contact(&acc, &uri) {
                            Ok(()) => Reply::Text { text: "request sent; the conversation starts when they accept".into() },
                            Err(e) => Reply::Error { message: e.to_string() },
                        }
                    }
                    "requests" => Reply::Text { text: if requests.is_empty() { "no requests".into() } else { requests.iter().map(|(id, t)| format!("{t}  {id}")).collect::<Vec<_>>().join("\n") } },
                    _ => {
                        let low = arg.to_lowercase();
                        let Some((id, _)) = requests.iter().find(|(id, t)| *id == arg || t.to_lowercase() == low || (low.len() >= 6 && t.to_lowercase().contains(&low))).cloned() else {
                            return Reply::Error { message: "no such request (joma requests)".into() };
                        };
                        self.accept_request(id, what == "accept", cx);
                        Reply::Done
                    }
                }
            }
            Request::Devices { what, arg, json } => {
                if self.gate != Gate::Ready {
                    return Reply::Error { message: "Joma is not on an account now".into() };
                }
                if what == "keep" {
                    let want = arg.unwrap_or_default().to_lowercase();
                    let found = self.new_devices.iter().find(|d| d.starts_with(want.as_str()) || self.model.devices.get(*d).is_some_and(|n| n.to_lowercase() == want)).cloned();
                    return match found {
                        Some(d) => {
                            self.keep_device(&d, cx);
                            Reply::Done
                        }
                        None => Reply::Error { message: format!("no new device {want}: see joma devices") },
                    };
                }
                let rows: Vec<serde_json::Value> = self
                    .model
                    .devices
                    .iter()
                    .map(|(id, name)| {
                        serde_json::json!({"id": id, "name": name, "this": *id == self.model.device, "new": self.new_devices.contains(id)})
                    })
                    .collect();
                if json {
                    return Reply::Text { text: serde_json::to_string(&rows).unwrap_or_default() };
                }
                let text = rows
                    .iter()
                    .map(|r| {
                        let tag = if r["this"] == true { "  (this device)" } else if r["new"] == true { "  (new: keep or revoke)" } else { "" };
                        format!("{}  {}{tag}", r["name"].as_str().unwrap_or(""), r["id"].as_str().unwrap_or(""))
                    })
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
            // Answered from AppMsg::Ipc (a password or the network).
            Request::Backup { .. } | Request::Restore { .. } | Request::BackupList | Request::GithubStatus | Request::Github { .. } => {
                Reply::Error { message: "busy".into() }
            }
            Request::Connections { to, json } => {
                let conv = match to {
                    Some(to) => match self.target(&to) {
                        Ok(c) => c,
                        Err(e) => return e,
                    },
                    None if self.gate != Gate::Ready => return Reply::Error { message: "Joma is not connected to Jami yet".into() },
                    None => String::new(),
                };
                match joma_jami::links::list(&self.model.account, &conv) {
                    Ok(links) => {
                        let rows = crate::conninfo::rows(links, |p| self.model.name_of(p), |d| self.model.devices.get(d).cloned());
                        let text = if json { serde_json::to_string(&rows).unwrap_or_default() } else { crate::conninfo::text(&rows) };
                        Reply::Text { text }
                    }
                    Err(e) => Reply::Error { message: e.to_string() },
                }
            }
            Request::Sync { what, arg, label, json } => self.sync_request(&what, arg, label, json, cx),
            Request::Quit => {
                self.leave();
                let acc = self.model.account.clone();
                self.sync.stop(&acc);
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

    /// Share a synced folder (its id, label or path; a path becomes a new folder) with a
    /// contact or the members of a conversation.
    pub fn share_with(&mut self, what: &str, with: &str) -> Result<String, String> {
        if self.sync.st.is_none() {
            return Err("sync is off (joma sync on)".into());
        }
        let me = self.model.me.clone();
        let members: Vec<String> = if let Some(uri) = self.model.contact_named(with) {
            vec![uri]
        } else if let Some(c) = self.model.conversations.values().find(|c| c.id == with || c.title.eq_ignore_ascii_case(with)) {
            c.peers.iter().filter(|p| **p != me).cloned().collect()
        } else {
            return Err(format!("no contact or conversation {with}"));
        };
        let folders = self.sync.st.as_ref().and_then(|s| s.folders().ok()).unwrap_or_default();
        let found = folders.iter().find(|f| f.id == what || f.label == what || std::path::Path::new(&f.path) == std::path::Path::new(what)).cloned();
        let acc = self.model.account.clone();
        let my_name = self.model.my_name.clone();
        // A folder we share already: more members, a new offer.
        if let Some(s) = found.as_ref().and_then(|f| self.sync.store.shares.get(&f.id)).cloned() {
            if s.owner != me {
                return Err(format!("{} is {}'s: only the owner shares it further", s.label, self.model.name_of(&s.owner)));
            }
            let mut s2 = s.clone();
            for u in &members {
                if !s2.has(u) {
                    s2.members.push(crate::shares::Member { uri: u.clone(), name: self.model.name_of(u), ..Default::default() });
                }
            }
            self.sync.store.shares.remove(&s.id);
            let path = std::path::PathBuf::from(&s2.path);
            let s3 = self.sync.create_share(&acc, &me, &my_name, Some(s2.id.clone()), &s2.label, &path, s2.conv.clone(), s2.members.clone()).map_err(|e| format!("{e:#}"))?;
            return Ok(format!("{} offered to {}", s3.label, members.iter().map(|u| self.model.name_of(u)).collect::<Vec<_>>().join(", ")));
        }
        let (id, label, path) = match found {
            Some(f) => (Some(f.id), f.label, std::path::PathBuf::from(f.path)),
            None => {
                let p = std::path::PathBuf::from(what);
                if !p.is_dir() {
                    return Err(format!("{what} is neither a synced folder nor a folder"));
                }
                let label = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "folder".into());
                (None, label, p)
            }
        };
        let list: Vec<crate::shares::Member> = members.iter().map(|u| crate::shares::Member { uri: u.clone(), name: self.model.name_of(u), ..Default::default() }).collect();
        let s = self.sync.create_share(&acc, &me, &my_name, id, &label, &path, None, list).map_err(|e| format!("{e:#}"))?;
        Ok(format!("{} offered to {} (they read; joma sync write gives writing)", s.label, members.iter().map(|u| self.model.name_of(u)).collect::<Vec<_>>().join(", ")))
    }

    fn sync_request(&mut self, what: &str, arg: Option<String>, label: Option<String>, json: bool, cx: &mut Context<Self>) -> Reply {
        if self.gate != Gate::Ready {
            return Reply::Error { message: "Joma is not connected to Jami yet".into() };
        }
        let acc = self.model.account.clone();
        match what {
            "status" => {
                let r = self.sync_report();
                Reply::Text { text: if json { serde_json::to_string(&r).unwrap_or_default() } else { r.text() } }
            }
            "on" => match crate::sync::set_on(&acc, true) {
                Ok(()) => {
                    self.start_sync();
                    Reply::Text { text: "sync is starting (joma sync status)".into() }
                }
                Err(e) => Reply::Error { message: e.to_string() },
            },
            "off" => {
                self.sync.stop(&acc);
                match crate::sync::set_on(&acc, false) {
                    Ok(()) => Reply::Done,
                    Err(e) => Reply::Error { message: e.to_string() },
                }
            }
            "add" => {
                let Some(path) = arg else { return Reply::Error { message: "which folder?".into() } };
                let path = std::path::PathBuf::from(path);
                if !path.is_dir() {
                    return Reply::Error { message: format!("{} is not a folder", path.display()) };
                }
                if self.sync.st.is_none() {
                    return Reply::Error { message: if self.sync.starting { "sync is still starting: try again in a moment".into() } else { "sync is off: joma sync on".into() } };
                }
                let label = label.unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "folder".into()));
                match self.sync.add_folder(&path, &label) {
                    Ok(id) => Reply::Text { text: format!("{label} syncs with your devices ({id})") },
                    Err(e) => Reply::Error { message: format!("{e:#}") },
                }
            }
            "remove" => match self.sync.remove_folder(arg.as_deref().unwrap_or("")) {
                Ok(()) => Reply::Done,
                Err(e) => Reply::Error { message: format!("{e:#}") },
            },
            // joma sync share <folder|path> <contact|conversation>
            "share" => {
                let (Some(what), Some(with)) = (arg, label) else { return Reply::Error { message: "joma sync share <folder> <contact|conversation>".into() } };
                match self.share_with(&what, &with) {
                    Ok(t) => Reply::Text { text: t },
                    Err(e) => Reply::Error { message: e },
                }
            }
            // joma sync restored [yes|no]: what a restore brought back, set it up or not.
            "restored" => match arg.as_deref() {
                None => match &self.restored_sync {
                    Some(s) => Reply::Text {
                        text: format!(
                            "from your backup: sync was {} with {} folders ({}) and {} shares (joma sync restored yes|no)",
                            if s.on { "on" } else { "off" },
                            s.folders.len(),
                            s.folders.iter().map(|f| f.label.as_str()).collect::<Vec<_>>().join(", "),
                            s.shares.shares.len()
                        ),
                    },
                    None => Reply::Text { text: "nothing restored to set up".into() },
                },
                Some(a) if self.restored_sync.is_some() => {
                    self.restore_sync(a == "yes", cx);
                    Reply::Done
                }
                _ => Reply::Error { message: "nothing restored to set up".into() },
            },
            "offers" => {
                let offers: Vec<&crate::shares::Offer> = self.sync.store.offers.values().collect();
                if json {
                    return Reply::Text { text: serde_json::to_string(&offers).unwrap_or_default() };
                }
                if offers.is_empty() {
                    return Reply::Text { text: "no folders offered".into() };
                }
                Reply::Text { text: offers.iter().map(|o| format!("{}  {}  from {}{}", o.folder, o.label, self.model.name_of(&o.owner), if o.conv.is_some() { "  (conversation)" } else { "" })).collect::<Vec<_>>().join("\n") }
            }
            "join" => match self.join_share(arg.as_deref().unwrap_or(""), label.map(std::path::PathBuf::from)) {
                Ok(s) => Reply::Text { text: format!("{} → {}", s.label, s.path) },
                Err(e) => Reply::Error { message: e },
            },
            "decline" => {
                if !self.sync.store.offers.contains_key(arg.as_deref().unwrap_or("")) {
                    return Reply::Error { message: "no such offer (joma sync offers)".into() };
                }
                self.sync.decline(&acc, arg.as_deref().unwrap_or(""));
                Reply::Done
            }
            "leave" => {
                let me = self.model.me.clone();
                match self.sync.leave(&acc, &me, arg.as_deref().unwrap_or("")) {
                    Ok(()) => Reply::Text { text: "left; the files stay where they are".into() },
                    Err(e) => Reply::Error { message: format!("{e:#}") },
                }
            }
            // joma sync write|admin <folder> <contact|all> on|off
            w @ ("write" | "admin") => {
                let (Some(folder), Some(rest)) = (arg, label) else { return Reply::Error { message: format!("joma sync {w} <folder> <contact|all> on|off") } };
                let (who, on) = rest.rsplit_once(' ').map(|(w, o)| (w.to_owned(), o == "on")).unwrap_or((rest.clone(), true));
                let folder = self.files_key(&folder).and_then(|k| if let FilesKey::Folder(f) = k { Some(f) } else { None }).unwrap_or(folder);
                let uri = if who == "all" && w == "write" {
                    "all".to_owned()
                } else {
                    match self.model.contact_named(&who).or_else(|| (who.len() == 40).then(|| who.clone())) {
                        Some(u) => u,
                        None => return Reply::Error { message: format!("no contact {who}") },
                    }
                };
                let me = self.model.me.clone();
                let r = if w == "write" { self.sync.set_write(&acc, &me, &folder, &uri, on) } else { self.sync.set_admin(&acc, &me, &folder, &uri, on) };
                let who = if uri == "all" { "every member".to_owned() } else { self.model.name_of(&uri) };
                match r {
                    Ok(()) if w == "write" => Reply::Text { text: format!("{who} {} write in it", if on { "may" } else { "may not" }) },
                    Ok(()) => Reply::Text { text: format!("{who} {} an admin of it", if on { "is" } else { "is no longer" }) },
                    Err(e) => Reply::Error { message: format!("{e:#}") },
                }
            }
            // joma sync device <folder> <device> on|off
            "device" => {
                let (Some(folder), Some(rest)) = (arg, label) else { return Reply::Error { message: "joma sync device <folder> <device> on|off".into() } };
                let (who, on) = rest.rsplit_once(' ').map(|(w, o)| (w.to_owned(), o == "on")).unwrap_or((rest.clone(), true));
                let Some(st) = self.sync.peers.iter().find(|(d, p)| p.name == who || d.starts_with(&who)).map(|(_, p)| p.st.clone()) else { return Reply::Error { message: format!("no device {who} with sync") } };
                match self.sync.set_device_on(&acc, &folder, &st, on) {
                    Ok(()) => Reply::Done,
                    Err(e) => Reply::Error { message: format!("{e:#}") },
                }
            }
            other => Reply::Error { message: format!("unknown sync command {other}") },
        }
    }

    /// Open or close the connection info under a contact or a device, reading the
    /// connections off the UI thread (it resolves the TURN server's name).
    pub fn toggle_conn_info(&mut self, key: String, cx: &mut Context<Self>) {
        if self.conn_info.as_ref().is_some_and(|(k, _)| *k == key) {
            self.conn_info = None;
            cx.notify();
            return;
        }
        self.conn_info = Some((key.clone(), None));
        cx.notify();
        let account = self.model.account.clone();
        let read = cx.background_executor().spawn(async move { joma_jami::links::list(&account, "") });
        cx.spawn(async move |this, cx| {
            let links = read.await.unwrap_or_default();
            let _ = this.update(cx, |j, cx| {
                if j.conn_info.as_ref().is_some_and(|(k, _)| *k == key) {
                    let rows = crate::conninfo::rows(links, |p| j.model.name_of(p), |d| j.model.devices.get(d).cloned());
                    j.conn_info = Some((key, Some(rows)));
                    cx.notify();
                }
            });
        })
        .detach();
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
        self.send_file_as(conv, path, false)
    }

    /// Send a file (`message`: a voice or video message) through Jami, and put it in the
    /// conversation's shared folder.
    fn send_file_as(&mut self, conv: &str, path: &str, message: bool) -> Reply {
        let path = std::path::Path::new(path);
        if !path.is_file() {
            return Reply::Error { message: format!("{} is not a file", path.display()) };
        }
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(n) = abs.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| media_name(n)) {
            self.sent_media.insert(n);
        }
        if let Err(e) = self.share_conv_file(conv, &abs, message) {
            tracing::warn!(error = %e, "the file did not go into the conversation's folder");
        }
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
    /// The pictures of the open conversation that are on disk, oldest first.
    pub fn pictures(&self) -> Vec<(std::path::PathBuf, String)> {
        let Some(conv) = self.selected.as_ref().and_then(|c| self.model.conversations.get(c)) else { return Vec::new() };
        conv.messages
            .iter()
            .filter_map(|m| m.file.as_ref())
            .filter_map(|f| self.saved_file(&conv.id, &f.id).map(|p| (p, f.name.clone())))
            .filter(|(p, _)| crate::render::is_picture(p))
            .collect()
    }

    /// Show the picture before or after the one in the viewer.
    pub fn viewer_step(&mut self, by: isize, cx: &mut Context<Self>) {
        let all = self.pictures();
        let Some(i) = self.viewer.as_ref().and_then(|v| all.iter().position(|(p, _)| p == v)) else { return };
        let j = (i as isize + by).clamp(0, all.len() as isize - 1) as usize;
        self.viewer = Some(all[j].0.clone());
        cx.notify();
    }

    pub fn open_outside(path: &std::path::Path) {
        let _ = std::process::Command::new("xdg-open").arg(path).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    }

    pub fn file_clicked(&mut self, conv: &str, interaction: &str, file: &FileRef, cx: &mut Context<Self>) {
        // A picture opens in Joma's own viewer.
        if let Some(p) = self.saved_file(conv, &file.id).filter(|p| crate::render::is_picture(p)) {
            self.viewer = Some(p);
            cx.notify();
            return;
        }
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
                if self.play_after.as_deref() == Some(file) {
                    self.play_after = None;
                    self.toggle_play(file, &path, cx);
                }
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


/// Local time for a file name: 20261117-031502.
fn file_stamp() -> String {
    let t = joma_desk::signal::now() as libc::time_t;
    // SAFETY: `localtime_r` writes into the `tm` we own.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", 1900 + tm.tm_year, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

#[cfg(test)]
mod files_tests {
    use super::*;

    #[test]
    fn conflict_names() {
        assert_eq!(conflict_of("report.sync-conflict-20261128-031500-ABCDEFG.pdf").as_deref(), Some("report.pdf"));
        assert_eq!(conflict_of("notes.sync-conflict-20261128-031500-ABCDEFG").as_deref(), Some("notes"));
        assert_eq!(conflict_of("plain.txt"), None);
    }

    #[test]
    fn conflicts_go_under_their_file() {
        let d = std::env::temp_dir().join(format!("joma-files-{}", std::process::id()));
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.txt"), "1").unwrap();
        std::fs::write(d.join("a.sync-conflict-20261128-031500-ABCDEFG.txt"), "2").unwrap();
        std::fs::write(d.join(".hidden"), "x").unwrap();
        let list = read_dir(&d);
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["sub", "a.txt"]);
        assert_eq!(list[1].conflicts.len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
