//! libjami, the Jami daemon library, running inside this process.
//!
//! The C++ API is reached through a small shim (`shim/jami_shim.cpp`) that speaks JSON:
//! [`call`] runs one daemon method, and daemon signals arrive as [`Event`]s on the sink
//! given to [`start`]. Jami keeps its data in the XDG directories (`$XDG_DATA_HOME/jami`,
//! `$XDG_CONFIG_HOME/jami`, `$XDG_CACHE_HOME/jami`), the same as the Qt client, so Joma
//! and the Qt client share accounts and history — but only one of them may run at a time.

use std::collections::BTreeMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::OnceLock;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod event;
pub use event::Event;

pub type Details = BTreeMap<String, String>;

/// libjami init flags (`libjami::InitFlag`).
pub mod flags {
    pub const DEBUG: u16 = 1 << 0;
    pub const CONSOLE_LOG: u16 = 1 << 1;
    pub const NO_LOCAL_AUDIO: u16 = 1 << 6;
    pub const NO_LOCAL_VIDEO: u16 = 1 << 7;
    pub const NO_LOCAL_MEDIA: u16 = NO_LOCAL_AUDIO | NO_LOCAL_VIDEO;
}

extern "C" {
    fn joma_jami_init(flags: u16, cb: extern "C" fn(*const c_char, *mut c_void), user: *mut c_void) -> bool;
    fn joma_jami_fini();
    fn joma_jami_call(request: *const c_char) -> *mut c_char;
    fn joma_jami_free(s: *mut c_char);
    fn joma_jami_sink_frame(
        id: *const c_char,
        serial: *mut u64,
        dst: *mut u8,
        cap: usize,
        width: *mut i32,
        height: *mut i32,
    ) -> i32;
}

type Sink = Box<dyn Fn(Event) + Send + Sync>;
static SINK: OnceLock<Sink> = OnceLock::new();

extern "C" fn on_signal(json: *const c_char, _user: *mut c_void) {
    // SAFETY: the shim passes a NUL-terminated string that lives for this call.
    let text = unsafe { CStr::from_ptr(json) }.to_string_lossy();
    let Some(sink) = SINK.get() else { return };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => sink(Event::parse(&v)),
        Err(e) => tracing::warn!(error = %e, "unreadable daemon signal"),
    }
}

/// Start the daemon in this process. Signals go to `sink`, on daemon threads. Call once.
pub fn start(flags: u16, sink: impl Fn(Event) + Send + Sync + 'static) -> Result<()> {
    if SINK.set(Box::new(sink)).is_err() {
        bail!("the Jami daemon is already started");
    }
    // SAFETY: `on_signal` is a valid callback for the life of the process.
    if !unsafe { joma_jami_init(flags, on_signal, std::ptr::null_mut()) } {
        bail!("libjami failed to start");
    }
    Ok(())
}

/// Stop the daemon (unregisters accounts and closes calls).
pub fn stop() {
    // SAFETY: plain call into the daemon; safe to call once at shutdown.
    unsafe { joma_jami_fini() }
}

/// Run one daemon method by name with JSON arguments, returning its JSON result.
pub fn call(method: &str, args: Value) -> Result<Value> {
    let req = CString::new(json!({ "m": method, "a": args }).to_string())?;
    // SAFETY: the shim returns a malloc'd NUL-terminated string that we free below.
    let out = unsafe { joma_jami_call(req.as_ptr()) };
    if out.is_null() {
        bail!("{method}: no answer from the daemon");
    }
    // SAFETY: `out` is valid until freed.
    let text = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
    // SAFETY: `out` came from `joma_jami_call`.
    unsafe { joma_jami_free(out) };
    let mut v: Value = serde_json::from_str(&text)?;
    if let Some(e) = v.get("e").and_then(Value::as_str) {
        return Err(anyhow!("{method}: {e}"));
    }
    Ok(v.get_mut("r").map(Value::take).unwrap_or(Value::Null))
}

fn typed<T: for<'de> Deserialize<'de>>(method: &str, args: Value) -> Result<T> {
    Ok(serde_json::from_value(call(method, args)?)?)
}

/// One message of a swarm conversation (`libjami::SwarmMessage`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwarmMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub linearized_parent: String,
    #[serde(default)]
    pub body: Details,
    #[serde(default)]
    pub reactions: Vec<Details>,
    #[serde(default)]
    pub editions: Vec<Details>,
    #[serde(default)]
    pub status: BTreeMap<String, i32>,
}

impl SwarmMessage {
    pub fn author(&self) -> &str {
        self.body.get("author").map(String::as_str).unwrap_or("")
    }

    pub fn text(&self) -> &str {
        self.body.get("body").map(String::as_str).unwrap_or("")
    }

    /// Seconds since the epoch.
    pub fn timestamp(&self) -> i64 {
        self.body.get("timestamp").and_then(|t| t.parse().ok()).unwrap_or(0)
    }
}

/// A media of a call (`libjami::MediaMap`): `MEDIA_TYPE`, `ENABLED`, `MUTED`, `SOURCE`, `LABEL`.
pub type Media = Details;

pub fn media(kind: &str, label: &str, source: &str, muted: bool) -> Media {
    let mut m = Media::new();
    m.insert("MEDIA_TYPE".into(), kind.into());
    m.insert("ENABLED".into(), "true".into());
    m.insert("MUTED".into(), muted.to_string());
    m.insert("SOURCE".into(), source.into());
    m.insert("LABEL".into(), label.into());
    m
}

pub const AUDIO: &str = "MEDIA_TYPE_AUDIO";
pub const VIDEO: &str = "MEDIA_TYPE_VIDEO";

// ---- Accounts ----------------------------------------------------------------

pub fn accounts() -> Result<Vec<String>> {
    typed("getAccountList", json!([]))
}

pub fn account_details(account: &str) -> Result<Details> {
    typed("getAccountDetails", json!([account]))
}

pub fn volatile_details(account: &str) -> Result<Details> {
    typed("getVolatileAccountDetails", json!([account]))
}

/// Create a new Jami (DHT) account with this display name. Returns its id.
pub fn create_account(display_name: &str) -> Result<String> {
    let mut details: Details = typed("getAccountTemplate", json!(["RING"]))?;
    details.insert("Account.type".into(), "RING".into());
    details.insert("Account.displayName".into(), display_name.into());
    details.insert("Account.alias".into(), display_name.into());
    details.insert("Account.archivePassword".into(), String::new());
    typed("addAccount", json!([details]))
}

pub fn register_name(account: &str, name: &str) -> Result<bool> {
    typed("registerName", json!([account, name]))
}

/// Resolve a registered name; the answer arrives as [`Event::NameFound`].
pub fn lookup_name(account: &str, name: &str) -> Result<bool> {
    typed("lookupName", json!([account, "", name]))
}

pub fn lookup_address(account: &str, address: &str) -> Result<bool> {
    typed("lookupAddress", json!([account, "", address]))
}

// ---- Contacts ----------------------------------------------------------------

pub fn contacts(account: &str) -> Result<Vec<Details>> {
    typed("getContacts", json!([account]))
}

pub fn add_contact(account: &str, uri: &str) -> Result<()> {
    call("addContact", json!([account, uri])).map(drop)
}

pub fn remove_contact(account: &str, uri: &str, ban: bool) -> Result<()> {
    call("removeContact", json!([account, uri, ban])).map(drop)
}

// ---- Conversations -----------------------------------------------------------

pub fn conversations(account: &str) -> Result<Vec<String>> {
    typed("getConversations", json!([account]))
}

pub fn conversation_requests(account: &str) -> Result<Vec<Details>> {
    typed("getConversationRequests", json!([account]))
}

pub fn conversation_infos(account: &str, conversation: &str) -> Result<Details> {
    typed("conversationInfos", json!([account, conversation]))
}

pub fn conversation_members(account: &str, conversation: &str) -> Result<Vec<Details>> {
    typed("getConversationMembers", json!([account, conversation]))
}

pub fn accept_request(account: &str, conversation: &str) -> Result<()> {
    call("acceptConversationRequest", json!([account, conversation])).map(drop)
}

pub fn decline_request(account: &str, conversation: &str) -> Result<()> {
    call("declineConversationRequest", json!([account, conversation])).map(drop)
}

/// Ask for up to `n` messages before `from` (empty: the newest). They arrive as
/// [`Event::SwarmLoaded`] with the returned request id.
pub fn load_conversation(account: &str, conversation: &str, from: &str, n: u32) -> Result<u32> {
    typed("loadConversation", json!([account, conversation, from, n]))
}

pub fn send_message(account: &str, conversation: &str, text: &str, reply_to: &str) -> Result<()> {
    call("sendMessage", json!([account, conversation, text, reply_to, 0])).map(drop)
}

pub fn set_displayed(account: &str, conversation: &str, message: &str) -> Result<bool> {
    typed("setMessageDisplayed", json!([account, format!("swarm:{conversation}"), message, 3]))
}

pub fn set_composing(account: &str, conversation: &str, composing: bool) -> Result<()> {
    call("setIsComposing", json!([account, format!("swarm:{conversation}"), composing])).map(drop)
}

// ---- Presence and files ------------------------------------------------------

/// Follow a contact's presence; changes arrive as [`Event::Presence`].
pub fn subscribe_presence(account: &str, uri: &str) -> Result<()> {
    call("subscribeBuddy", json!([account, uri, true])).map(drop)
}

/// Send a file into a conversation. Progress arrives as [`Event::Transfer`].
pub fn send_file(account: &str, conversation: &str, path: &str) -> Result<()> {
    let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    call("sendFile", json!([account, conversation, path, name, ""])).map(drop)
}

/// Fetch a received file into `path`.
pub fn download_file(account: &str, conversation: &str, interaction: &str, file: &str, path: &str) -> Result<bool> {
    typed("downloadFile", json!([account, conversation, interaction, file, path]))
}

// ---- Calls -------------------------------------------------------------------

/// Call a contact (`to` is a Jami URI or `swarm:<conversation id>`). Returns the call id.
pub fn place_call(account: &str, to: &str, media: &[Media]) -> Result<String> {
    typed("placeCallWithMedia", json!([account, to, media]))
}

pub fn accept_call(account: &str, call_id: &str, media: &[Media]) -> Result<bool> {
    typed("acceptWithMedia", json!([account, call_id, media]))
}

pub fn refuse_call(account: &str, call_id: &str) -> Result<bool> {
    typed("refuse", json!([account, call_id]))
}

pub fn hang_up(account: &str, call_id: &str) -> Result<bool> {
    typed("hangUp", json!([account, call_id]))
}

pub fn mute_local(account: &str, call_id: &str, kind: &str, mute: bool) -> Result<bool> {
    typed("muteLocalMedia", json!([account, call_id, kind, mute]))
}

pub fn request_media_change(account: &str, call_id: &str, media: &[Media]) -> Result<bool> {
    typed("requestMediaChange", json!([account, call_id, media]))
}

pub fn answer_media_change(account: &str, call_id: &str, media: &[Media]) -> Result<bool> {
    typed("answerMediaChangeRequest", json!([account, call_id, media]))
}

pub fn current_media(account: &str, call_id: &str) -> Result<Vec<Media>> {
    typed("currentMediaList", json!([account, call_id]))
}

pub fn call_details(account: &str, call_id: &str) -> Result<Details> {
    typed("getCallDetails", json!([account, call_id]))
}

/// Send an in-call message: a map of MIME type to payload, delivered to the peer as
/// [`Event::CallMessage`].
pub fn send_call_message(account: &str, call_id: &str, payloads: &Details) -> Result<()> {
    call("sendTextMessage", json!([account, call_id, payloads, "", false])).map(drop)
}

// ---- Video -------------------------------------------------------------------

pub fn video_devices() -> Result<Vec<String>> {
    typed("getDeviceList", json!([]))
}

pub fn default_video_device() -> Result<String> {
    typed("getDefaultDevice", json!([]))
}

pub fn open_video_input(resource: &str) -> Result<String> {
    typed("openVideoInput", json!([resource]))
}

pub fn close_video_input(id: &str) -> Result<bool> {
    typed("closeVideoInput", json!([id]))
}

/// Receive BGRA frames of a decoder (its id comes with [`Event::DecodingStarted`]).
pub fn attach_sink(id: &str) -> Result<bool> {
    typed("attachSink", json!([id]))
}

pub fn detach_sink(id: &str) -> Result<()> {
    call("detachSink", json!([id])).map(drop)
}

/// A BGRA frame copied out of a sink.
pub struct SinkFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// The newest frame of sink `id`, if it changed since `serial` (updated on success).
pub fn sink_frame(id: &str, serial: &mut u64, buf: &mut Vec<u8>) -> Option<SinkFrame> {
    let cid = CString::new(id).ok()?;
    let (mut w, mut h) = (0i32, 0i32);
    loop {
        // SAFETY: `buf` is valid for `buf.len()` bytes; the shim writes at most that.
        let r = unsafe { joma_jami_sink_frame(cid.as_ptr(), serial, buf.as_mut_ptr(), buf.len(), &mut w, &mut h) };
        match r {
            1 => {
                let len = w as usize * h as usize * 4;
                return Some(SinkFrame { width: w as u32, height: h as u32, bgra: buf[..len].to_vec() });
            }
            -1 => buf.resize(w as usize * h as usize * 4, 0),
            _ => return None,
        }
    }
}

// ---- Exposed services and tunnels --------------------------------------------

/// Expose `127.0.0.1:port` to the given contacts only. Returns the service id.
pub fn expose_service(account: &str, name: &str, port: u16, allowed: &[&str]) -> Result<String> {
    let mut d = Details::new();
    d.insert("name".into(), name.into());
    d.insert("description".into(), "Joma".into());
    d.insert("localHost".into(), "127.0.0.1".into());
    d.insert("localPort".into(), port.to_string());
    d.insert("policy".into(), "specific".into());
    d.insert("allowedContacts".into(), allowed.join(","));
    d.insert("enabled".into(), "true".into());
    let id: String = typed("addExposedService", json!([account, d]))?;
    if id.is_empty() {
        bail!("the daemon refused to expose {name}");
    }
    Ok(id)
}

pub fn remove_service(account: &str, id: &str) -> Result<bool> {
    typed("removeExposedService", json!([account, id]))
}

pub fn exposed_services(account: &str) -> Result<Vec<Details>> {
    typed("getExposedServices", json!([account]))
}

/// Ask a peer which services it exposes to us; the answer arrives as
/// [`Event::PeerServices`] with the returned request id.
pub fn query_peer_services(account: &str, peer: &str) -> Result<u32> {
    typed("queryPeerServices", json!([account, peer]))
}

/// Open a local listener (0: any port) forwarding to a peer device's service. The port
/// arrives as [`Event::TunnelOpened`]; returns the tunnel id.
pub fn open_tunnel(account: &str, peer: &str, device: &str, service: &str, name: &str) -> Result<String> {
    let id: String = typed("openServiceTunnel", json!([account, peer, device, service, name, 0]))?;
    if id.is_empty() {
        bail!("the daemon could not open a tunnel to {peer}");
    }
    Ok(id)
}

pub fn close_tunnel(account: &str, id: &str) -> Result<bool> {
    typed("closeServiceTunnel", json!([account, id]))
}

/// The id of this device for `account` (other peers address tunnels to it).
pub fn device_id(account: &str) -> Result<String> {
    Ok(account_details(account)?.get("Account.deviceID").cloned().unwrap_or_default())
}

/// The Jami URI (40 hex digits) of `account`.
pub fn account_uri(account: &str) -> Result<String> {
    Ok(account_details(account)?.get("Account.username").cloned().unwrap_or_default())
}
