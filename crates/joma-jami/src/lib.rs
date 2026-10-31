//! libjami, the Jami daemon library, running inside this process.
//!
//! The C++ API is reached through a small shim (`shim/jami_shim.cpp`) that speaks JSON:
//! [`call`] runs one daemon method, and daemon signals arrive as [`Event`]s on the sink
//! given to [`start`]. Jami keeps its data in the XDG directories (`$XDG_DATA_HOME/jami`,
//! `$XDG_CONFIG_HOME/jami`, `$XDG_CACHE_HOME/jami`), the same as the Qt client, so Joma
//! and the Qt client share accounts and history — but only one of them may run at a time.

use std::collections::BTreeMap;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
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
/// Between [`start`] and [`stop`]: calls reach the daemon only then.
static RUNNING: AtomicBool = AtomicBool::new(false);

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
    RUNNING.store(true, Ordering::SeqCst);
    Ok(())
}

/// Stop the daemon (unregisters accounts and closes calls). Later calls fail, and it
/// cannot start again in this process. Does nothing when it is not running.
pub fn stop() {
    if RUNNING.swap(false, Ordering::SeqCst) {
        // SAFETY: plain call into the daemon, once: RUNNING guards it.
        unsafe { joma_jami_fini() }
    }
}

/// Is the daemon running (started and not stopped)?
pub fn running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Run one daemon method by name with JSON arguments, returning its JSON result.
pub fn call(method: &str, args: Value) -> Result<Value> {
    if !running() {
        bail!("{method}: the Jami daemon is not running");
    }
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

/// Join an existing account from another device: the account starts empty and the
/// daemon announces a `jami-auth://` token ([`Event::DeviceAuth`], state
/// [`AUTH_TOKEN`]); the other device enters it with [`add_device`].
pub fn link_account() -> Result<String> {
    let mut details: Details = typed("getAccountTemplate", json!(["RING"]))?;
    details.insert("Account.type".into(), "RING".into());
    details.insert("Account.archiveURL".into(), "jami-auth".into());
    typed("addAccount", json!([details]))
}

/// Remove an account from this machine (a link that failed).
pub fn remove_account(account: &str) -> Result<()> {
    call("removeAccount", json!([account])).map(drop)
}

// ---- Devices -----------------------------------------------------------------
// The device-linking states of `DeviceAuthStateChanged` and `AddDeviceStateChanged`.
pub const AUTH_TOKEN: i64 = 1;
pub const AUTH_CONNECTING: i64 = 2;
pub const AUTH_AUTHENTICATING: i64 = 3;
pub const AUTH_IN_PROGRESS: i64 = 4;
pub const AUTH_DONE: i64 = 5;

/// The account's devices: device id to name.
pub fn known_devices(account: &str) -> Result<Details> {
    typed("getKnownRingDevices", json!([account]))
}

/// Start sending the account to a new device that shows `token`. Returns the operation
/// id that [`Event::AddDevice`] reports on.
pub fn add_device(account: &str, token: &str) -> Result<u32> {
    let id: i64 = typed("addDevice", json!([account, token.trim()]))?;
    u32::try_from(id).map_err(|_| anyhow::anyhow!("the token was not accepted ({id})"))
}

/// The new device connected (state [`AUTH_AUTHENTICATING`]): go on and send it.
pub fn confirm_add_device(account: &str, op: u32) -> Result<bool> {
    typed("confirmAddDevice", json!([account, op]))
}

pub fn cancel_add_device(account: &str, op: u32) -> Result<bool> {
    typed("cancelAddDevice", json!([account, op]))
}

/// On the new device, once the old one confirmed: the account password, or empty for
/// an account without one (`scheme` as the daemon asked: "" or "password").
pub fn provide_authentication(account: &str, password: &str, scheme: &str) -> Result<bool> {
    typed("provideAccountAuthentication", json!([account, password, scheme]))
}

/// Revoke a device of the account (an account without password: empty scheme and password).
pub fn revoke_device(account: &str, device: &str) -> Result<bool> {
    typed("revokeDevice", json!([account, device, "", ""]))
}

/// Rename this device.
pub fn set_device_name(account: &str, name: &str) -> Result<()> {
    let mut d = Details::new();
    d.insert("Account.deviceName".into(), name.into());
    call("setAccountDetails", json!([account, d])).map(drop)
}

/// Set the profile peers receive (the vCard's name; no avatar change). The daemon sends
/// it to contacts.
pub fn update_profile(account: &str, display_name: &str) -> Result<()> {
    call("updateProfile", json!([account, display_name, "", "", "", 0])).map(drop)
}

/// Set (a PNG file) or remove (`None`) the picture in the profile peers receive; the
/// name is kept as `display_name`.
pub fn set_avatar(account: &str, display_name: &str, png: Option<&str>) -> Result<()> {
    match png {
        Some(path) => call("updateProfile", json!([account, display_name, path, "PNG", "", 0])).map(drop),
        None => call("updateProfile", json!([account, display_name, "", "", "", 2])).map(drop),
    }
}

/// Register `name` on the name server for this account; the answer arrives as
/// [`Event::NameRegistered`].
///
/// Never from a test profile (`JOMA_PROFILE`): a name on the public server cannot be
/// taken back, and a test account's name would stay there forever.
pub fn register_name(account: &str, name: &str) -> Result<bool> {
    if std::env::var_os("JOMA_PROFILE").is_some() {
        anyhow::bail!("test profiles never register usernames (the name server is public)");
    }
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

/// Leave a conversation and drop it from this device.
pub fn remove_conversation(account: &str, conversation: &str) -> Result<bool> {
    typed("removeConversation", json!([account, conversation]))
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

/// Send a message straight to a peer's devices, outside calls and conversations: not
/// kept in any history, delivered as [`Event::AccountMessage`]. `connected_only` skips
/// devices without an open connection instead of going through the DHT.
pub fn send_account_message(account: &str, to: &str, payloads: &Details, connected_only: bool) -> Result<()> {
    call("sendAccountTextMessage", json!([account, to, payloads, connected_only])).map(drop)
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
/// Receive the decoded frames of `id` as BGRA, `width` x `height` (the decoder's size, from
/// [`Event::DecodingStarted`]; attach again when it changes).
pub fn attach_sink(id: &str, width: i64, height: i64) -> Result<bool> {
    typed("attachSink", json!([id, width, height]))
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
    if !running() {
        return None;
    }
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

// ---- Settings -------------------------------------------------------------------

/// Audio devices: (inputs, outputs, and the current [output, input, ringtone] indexes).
pub fn audio_devices() -> Result<(Vec<String>, Vec<String>, Vec<usize>)> {
    let list = |m: &str| -> Result<Vec<String>> { Ok(serde_json::from_value(call(m, json!([]))?).unwrap_or_default()) };
    let current: Vec<String> = serde_json::from_value(call("getCurrentAudioDevicesIndex", json!([]))?).unwrap_or_default();
    Ok((list("getAudioInputDeviceList")?, list("getAudioOutputDeviceList")?, current.iter().map(|i| i.parse().unwrap_or(0)).collect()))
}

/// Use audio input (`input`), output, or ringtone device number `index`.
pub fn set_audio_device(kind: &str, index: usize) -> Result<()> {
    let m = match kind {
        "input" => "setAudioInputDevice",
        "output" => "setAudioOutputDevice",
        _ => "setAudioRingtoneDevice",
    };
    call(m, json!([index])).map(drop)
}

/// Noise suppression, echo cancellation ("auto", "audioProcessor", "system", "false"…)
/// and automatic gain.
pub fn audio_processing() -> Result<(String, String, bool)> {
    let s = |m: &str| -> Result<String> { Ok(call(m, json!([]))?.as_str().unwrap_or_default().to_owned()) };
    Ok((s("getNoiseSuppressState")?, s("getEchoCancellationState")?, call("isAgcEnabled", json!([]))?.as_bool().unwrap_or(false)))
}

pub fn set_noise_suppression(state: &str) -> Result<()> {
    call("setNoiseSuppressState", json!([state])).map(drop)
}

pub fn set_echo_cancellation(state: &str) -> Result<()> {
    call("setEchoCancellationState", json!([state])).map(drop)
}

pub fn set_agc(on: bool) -> Result<()> {
    call("setAgcState", json!([on])).map(drop)
}

/// The camera's modes: channel → size ("1280x720") → frame rates.
pub fn camera_capabilities(device: &str) -> Result<std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<String>>>> {
    Ok(serde_json::from_value(call("getCapabilities", json!([device]))?).unwrap_or_default())
}

/// The camera's current settings (`size`, `rate`, `channel`, `name`, `id`).
pub fn camera_settings(device: &str) -> Result<Details> {
    Ok(serde_json::from_value(call("getSettings", json!([device]))?).unwrap_or_default())
}

pub fn apply_camera_settings(device: &str, settings: &Details) -> Result<()> {
    call("applySettings", json!([device, settings])).map(drop)
}

pub fn set_default_camera(device: &str) -> Result<()> {
    call("setDefaultDevice", json!([device])).map(drop)
}

/// The account's active codecs with their details (`CodecInfo.name`, `.type`,
/// `.bitrate`, `.autoQualityEnabled`…), by codec id.
pub fn codecs(account: &str) -> Result<Vec<(u32, Details)>> {
    let ids: Vec<u32> = serde_json::from_value(call("getActiveCodecList", json!([account]))?).unwrap_or_default();
    ids.into_iter().map(|id| Ok((id, serde_json::from_value(call("getCodecDetails", json!([account, id]))?).unwrap_or_default()))).collect()
}

pub fn set_codec_details(account: &str, id: u32, details: &Details) -> Result<bool> {
    Ok(call("setCodecDetails", json!([account, id, details]))?.as_bool().unwrap_or(false))
}

/// Record a message: audio only with an empty `video_input`, else from that camera.
/// `path` has no extension; the daemon adds one and returns the full path ("" failed).
pub fn start_recorder(video_input: &str, path: &str) -> Result<String> {
    Ok(call("startLocalMediaRecorder", json!([video_input, path]))?.as_str().unwrap_or_default().to_owned())
}

pub fn stop_recorder(path: &str) -> Result<()> {
    call("stopLocalRecorder", json!([path])).map(drop)
}
