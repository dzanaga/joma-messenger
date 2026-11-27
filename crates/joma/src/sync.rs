//! Folder sync between your own devices (D-23): Joma's own Syncthing, paired and carried
//! by Jami.
//!
//! Each Joma with sync on runs its Syncthing (listening on 127.0.0.1 only, no discovery,
//! no relays) and exposes Syncthing's port as the Jami service `joma-sync` to its own
//! account. It tells your other devices its Syncthing device ID with an account message
//! ([`MIME`]); each device that hears it opens a Jami tunnel to the sender's service and
//! gives its Syncthing the tunnel's local end as the sender's address. Every folder is
//! shared with every device of the account; a folder another device shares appears under
//! `~/joma-sync/` by itself.
//!
//! No size limit for now (the user, 2026-11-27): files sync on any path, and the
//! interface warns when the Jami connection to a device is relayed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use joma_sync::Syncthing;
use serde::{Deserialize, Serialize};

pub const MIME: &str = "application/x-joma-sync+json";
pub const SERVICE: &str = "joma-sync";

/// What a device says about its Syncthing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub v: u32,
    /// The sending Jami device.
    pub dev: String,
    /// Its Syncthing device ID.
    pub st: String,
    /// Its `joma-sync` service id.
    pub service: String,
    /// Its device name.
    #[serde(default)]
    pub name: String,
    /// An answer to a Hello: don't answer back.
    #[serde(default)]
    pub reply: bool,
}

impl Hello {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("plain data")
    }

    pub fn decode(text: &str) -> Option<Hello> {
        serde_json::from_str(text).ok().filter(|h: &Hello| h.v == 1 && !h.st.is_empty())
    }
}

/// One of our other devices as sync knows it.
#[derive(Debug, Clone, Default)]
pub struct Peer {
    pub st: String,
    pub name: String,
    pub service: String,
    pub tunnel: Option<String>,
    pub port: Option<u16>,
}

/// The on/off switch, kept in `$XDG_CONFIG_HOME/joma/sync.toml` per account.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    on: Vec<String>,
}

fn settings_path() -> Option<PathBuf> {
    Some(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?.join("joma/sync.toml"))
}

fn settings() -> Settings {
    settings_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
}

/// Sync is on for this account.
pub fn is_on(account: &str) -> bool {
    settings().on.iter().any(|a| a == account)
}

pub fn set_on(account: &str, on: bool) -> Result<()> {
    let mut s = settings();
    s.on.retain(|a| a != account);
    if on {
        s.on.push(account.to_owned());
    }
    let p = settings_path().ok_or_else(|| anyhow::anyhow!("no config directory"))?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(p, toml::to_string(&s)?)?;
    Ok(())
}

/// Syncthing's home for an account: under Joma's data, never the user's Syncthing.
pub fn home(account: &str) -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    data.join("joma/syncthing").join(account)
}

/// Where folders from your other devices appear.
pub fn default_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp")).join("joma-sync")
}

#[derive(Default)]
pub struct Sync {
    pub st: Option<Syncthing>,
    /// Starting in the background.
    pub starting: bool,
    pub service: Option<String>,
    /// Our other devices, by Jami device id.
    pub peers: HashMap<String, Peer>,
    pub error: Option<String>,
    /// When we last told our devices.
    pub last_hello: Option<std::time::Instant>,
}

/// Devices that were offline hear from us again this often.
pub const HELLO_EVERY: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Start Syncthing off the UI thread (it waits for Syncthing to answer).
pub fn start_syncthing(account: &str) -> Result<Syncthing, String> {
    if !joma_sync::available() {
        return Err("Syncthing is not installed (pacman -S syncthing)".into());
    }
    Syncthing::start(&home(account), &default_dir()).map_err(|e| format!("{e:#}"))
}

impl Sync {
    /// Syncthing is up: expose it to our own devices and tell them.
    pub fn started(&mut self, account: &str, me: &str, device: &str, name: &str, st: Syncthing) {
        self.starting = false;
        self.error = None;
        // A service left from an earlier run points at the same port, but drop it so
        // exactly one is advertised.
        for s in joma_jami::exposed_services(account).unwrap_or_default() {
            if s.get("name").map(String::as_str) == Some(SERVICE) {
                if let Some(id) = s.get("id") {
                    let _ = joma_jami::remove_service(account, id);
                }
            }
        }
        match joma_jami::expose_service(account, SERVICE, st.listen_port, &[me]) {
            Ok(id) => self.service = Some(id),
            Err(e) => self.error = Some(format!("cannot expose Syncthing to your devices: {e:#}")),
        }
        self.st = Some(st);
        self.hello(account, me, device, name, false);
    }

    /// Tell our other devices where our Syncthing is.
    pub fn hello(&mut self, account: &str, me: &str, device: &str, name: &str, reply: bool) {
        let (Some(st), Some(service)) = (&self.st, &self.service) else { return };
        self.last_hello = Some(std::time::Instant::now());
        let h = Hello { v: 1, dev: device.to_owned(), st: st.id.clone(), service: service.clone(), name: name.to_owned(), reply };
        let mut p = joma_jami::Details::new();
        p.insert(MIME.into(), h.encode());
        if let Err(e) = joma_jami::send_account_message(account, me, &p, false) {
            tracing::warn!(error = %e, "could not tell our devices about sync");
        }
    }

    /// Another of our devices told us its Syncthing: pair with it and reach it through a
    /// tunnel. Returns whether to answer (it does not know us yet).
    pub fn heard(&mut self, account: &str, me: &str, hello: Hello) -> bool {
        let Some(st) = &self.st else { return false };
        let known = self.peers.get(&hello.dev).cloned();
        let fresh = known.as_ref().is_none_or(|p| p.st != hello.st || p.service != hello.service);
        if let Err(e) = st.set_device(&hello.st, &hello.name, &known.as_ref().and_then(|p| p.port).map(addr).into_iter().collect::<Vec<_>>()) {
            self.error = Some(format!("{e:#}"));
        }
        if let Err(e) = st.share_all_with(&hello.st) {
            self.error = Some(format!("{e:#}"));
        }
        let mut peer = known.unwrap_or_default();
        if fresh || peer.tunnel.is_none() {
            if let Some(t) = peer.tunnel.take() {
                let _ = joma_jami::close_tunnel(account, &t);
            }
            match joma_jami::open_tunnel(account, me, &hello.dev, &hello.service, SERVICE) {
                Ok(t) => peer.tunnel = Some(t),
                Err(e) => self.error = Some(format!("cannot reach {}: {e:#}", if hello.name.is_empty() { &hello.dev } else { &hello.name })),
            }
        }
        peer.st = hello.st.clone();
        peer.name = hello.name.clone();
        peer.service = hello.service.clone();
        self.peers.insert(hello.dev.clone(), peer);
        !hello.reply
    }

    /// A tunnel opened: is it one of ours? Then give Syncthing its port.
    pub fn tunnel_opened(&mut self, tunnel: &str, port: u16) -> bool {
        let Some(peer) = self.peers.values_mut().find(|p| p.tunnel.as_deref() == Some(tunnel)) else { return false };
        peer.port = Some(port);
        if let Some(st) = &self.st {
            if let Err(e) = st.set_device(&peer.st, &peer.name, &[addr(port)]) {
                self.error = Some(format!("{e:#}"));
            }
        }
        true
    }

    /// A tunnel closed: ours? Then it is reopened at the next Hello.
    pub fn tunnel_closed(&mut self, tunnel: &str) -> bool {
        let Some(peer) = self.peers.values_mut().find(|p| p.tunnel.as_deref() == Some(tunnel)) else { return false };
        peer.tunnel = None;
        peer.port = None;
        true
    }

    /// Share a folder with all our devices.
    pub fn add_folder(&self, path: &Path, label: &str) -> Result<String> {
        let st = self.st.as_ref().ok_or_else(|| anyhow::anyhow!("sync is not running"))?;
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(f) = st.folders()?.into_iter().find(|f| Path::new(&f.path) == path) {
            return Ok(f.id);
        }
        let id = joma_sync::folder_id(label);
        let devices: Vec<String> = self.peers.values().map(|p| p.st.clone()).collect();
        st.set_folder(&id, label, &path, &devices)?;
        Ok(id)
    }

    pub fn remove_folder(&self, id_or_label: &str) -> Result<()> {
        let st = self.st.as_ref().ok_or_else(|| anyhow::anyhow!("sync is not running"))?;
        let f = st
            .folders()?
            .into_iter()
            .find(|f| f.id == id_or_label || f.label == id_or_label)
            .ok_or_else(|| anyhow::anyhow!("no synced folder {id_or_label}"))?;
        // Syncthing only forgets the folder; the files stay where they are.
        st.remove_folder(&f.id)
    }

    /// Stop Syncthing and withdraw the service.
    pub fn stop(&mut self, account: &str) {
        for p in self.peers.values_mut() {
            if let Some(t) = p.tunnel.take() {
                let _ = joma_jami::close_tunnel(account, &t);
            }
        }
        self.peers.clear();
        if let Some(id) = self.service.take() {
            let _ = joma_jami::remove_service(account, &id);
        }
        if let Some(mut st) = self.st.take() {
            st.stop();
        }
        self.starting = false;
    }

    /// What a report needs, to build it off the UI thread.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { api: self.st.as_ref().map(|s| s.api.clone()), peers: self.peers.clone(), error: self.error.clone(), starting: self.starting }
    }
}

/// The sync state copied out of the app, for [`Snapshot::report`].
pub struct Snapshot {
    api: Option<joma_sync::Api>,
    peers: HashMap<String, Peer>,
    error: Option<String>,
    starting: bool,
}

impl Snapshot {
    /// The report `joma sync status` prints and the devices tab shows (blocking: it asks
    /// Syncthing and reads the Jami connections).
    pub fn report(&self, account: &str) -> Report {
        let mut r = Report { on: self.api.is_some() || self.starting, starting: self.starting, error: self.error.clone(), ..Default::default() };
        let Some(st) = &self.api else { return r };
        let links = joma_jami::links::list(account, "").unwrap_or_default();
        let path_of = |dev: &str| {
            let mut best: Option<joma_jami::links::Path> = None;
            for l in links.iter().filter(|l| l.device == dev && l.status == "0") {
                // A direct path wins over a relayed one when both exist.
                if best.is_none_or(|b| b.relayed()) {
                    best = Some(l.path);
                }
            }
            best
        };
        r.id = st.id.clone();
        r.folders = st.folder_status().unwrap_or_default();
        let status: HashMap<String, joma_sync::DeviceStatus> = st.device_status().unwrap_or_default().into_iter().map(|d| (d.id.clone(), d)).collect();
        let mut devices: Vec<DeviceLine> = self
            .peers
            .iter()
            .map(|(dev, p)| {
                let s = status.get(&p.st);
                let path = path_of(dev);
                DeviceLine {
                    device: dev.clone(),
                    name: p.name.clone(),
                    syncthing: p.st.clone(),
                    tunnel: p.port.is_some(),
                    connected: s.is_some_and(|s| s.connected),
                    path,
                    relayed: path.is_some_and(|p| p.relayed()),
                    in_bytes: s.map_or(0, |s| s.in_bytes),
                    out_bytes: s.map_or(0, |s| s.out_bytes),
                }
            })
            .collect();
        devices.sort_by(|a, b| a.name.cmp(&b.name));
        r.devices = devices;
        r
    }
}

fn addr(port: u16) -> String {
    format!("tcp://127.0.0.1:{port}")
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceLine {
    pub device: String,
    pub name: String,
    pub syncthing: String,
    /// The Jami tunnel to its Syncthing is open.
    pub tunnel: bool,
    /// Syncthing is connected to it.
    pub connected: bool,
    pub path: Option<joma_jami::links::Path>,
    pub relayed: bool,
    pub in_bytes: u64,
    pub out_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub on: bool,
    pub starting: bool,
    pub error: Option<String>,
    /// Our Syncthing device ID.
    pub id: String,
    pub folders: Vec<joma_sync::FolderStatus>,
    pub devices: Vec<DeviceLine>,
}

pub fn size(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

impl Report {
    pub fn text(&self) -> String {
        if !self.on {
            return "sync is off (joma sync on)".into();
        }
        if self.starting {
            return "sync is starting…".into();
        }
        let mut out = vec![format!("syncthing {}", self.id)];
        if let Some(e) = &self.error {
            out.push(format!("! {e}"));
        }
        out.push(if self.folders.is_empty() { "no folders (joma sync add <path>)".into() } else { "folders:".into() });
        for f in &self.folders {
            let need = if f.need_files > 0 { format!(", needs {} files, {}", f.need_files, size(f.need_bytes)) } else { String::new() };
            let err = if f.error.is_empty() { String::new() } else { format!(" ! {}", f.error) };
            out.push(format!("  {}  {}  {} · {} files, {}{need}{err}", f.label, f.path, f.state, f.files, size(f.bytes)));
        }
        out.push(if self.devices.is_empty() { "no other device heard yet".into() } else { "devices:".into() });
        for d in &self.devices {
            let path = d.path.map(|p| p.label()).unwrap_or("not connected");
            let warn = if d.relayed { "  ! relayed: slow for large files" } else { "" };
            out.push(format!(
                "  {}  tunnel {}  syncthing {}  {path}  in {} out {}{warn}",
                if d.name.is_empty() { &d.device } else { &d.name },
                if d.tunnel { "open" } else { "closed" },
                if d.connected { "connected" } else { "not connected" },
                size(d.in_bytes),
                size(d.out_bytes)
            ));
        }
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trip() {
        let h = Hello { v: 1, dev: "d".into(), st: "ST-ID".into(), service: "s1".into(), name: "laptop".into(), reply: false };
        assert_eq!(Hello::decode(&h.encode()), Some(h));
        assert_eq!(Hello::decode(r#"{"v":2,"dev":"d","st":"x","service":"s"}"#), None);
        assert_eq!(Hello::decode(r#"{"v":1,"dev":"d","st":"","service":"s"}"#), None);
        assert_eq!(Hello::decode("nope"), None);
    }

    #[test]
    fn report_text() {
        assert_eq!(Report::default().text(), "sync is off (joma sync on)");
        let r = Report {
            on: true,
            id: "AAA".into(),
            folders: vec![joma_sync::FolderStatus { label: "notes".into(), path: "/n".into(), state: "idle".into(), files: 3, bytes: 2048, ..Default::default() }],
            devices: vec![DeviceLine { name: "desk".into(), tunnel: true, connected: true, path: Some(joma_jami::links::Path::Relayed), relayed: true, ..Default::default() }],
            ..Default::default()
        };
        let t = r.text();
        assert!(t.contains("notes  /n  idle · 3 files, 2 KB"), "{t}");
        assert!(t.contains("desk  tunnel open  syncthing connected  relayed (TURN)"), "{t}");
        assert!(t.contains("! relayed"), "{t}");
        assert_eq!(size(5 << 20), "5.0 MB");
    }
}
