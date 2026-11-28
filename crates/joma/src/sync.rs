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
/// The same Syncthing port, exposed to the members of our shared folders.
pub const PEER_SERVICE: &str = "joma-sync-peers";

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
        if text.contains("\"kind\"") {
            return None;
        }
        serde_json::from_str(text).ok().filter(|h: &Hello| h.v == 1 && !h.st.is_empty())
    }
}

/// One of our other devices as sync knows it.
#[derive(Debug, Clone, Default)]
pub struct Peer {
    /// A contact's device: its account (empty for our own devices).
    pub uri: String,
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

/// What a backup keeps of sync (discussion 2026-11-24-folder-sync, question 8): whether
/// it was on, the folders (id, label, path, type) and the shares with contacts, so a
/// restored machine offers to set them up again.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Saved {
    pub on: bool,
    /// The home folder of the machine that made the backup: paths under it move under
    /// the new machine's home.
    #[serde(default)]
    pub home: String,
    pub folders: Vec<SavedFolder>,
    #[serde(default)]
    pub shares: crate::shares::Store,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedFolder {
    pub id: String,
    pub label: String,
    pub path: String,
    pub kind: String,
}

fn unescape(s: &str) -> String {
    s.replace("&#39;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// The folders in an account's Syncthing config (read from its file: Syncthing need not run).
pub fn saved_folders(account: &str) -> Vec<SavedFolder> {
    std::fs::read_to_string(home(account).join("config.xml")).map(|x| parse_folders(&x)).unwrap_or_default()
}

/// The `<folder …>` tags of Syncthing's config.
pub fn parse_folders(xml: &str) -> Vec<SavedFolder> {
    let attr = |tag: &str, name: &str| -> String {
        let key = format!(" {name}=\"");
        tag.find(&key).and_then(|i| tag[i + key.len()..].split('"').next()).map(unescape).unwrap_or_default()
    };
    xml.match_indices("<folder ")
        .filter_map(|(i, _)| {
            let tag = &xml[i..i + xml[i..].find('>')?];
            Some(SavedFolder { id: attr(tag, "id"), label: attr(tag, "label"), path: attr(tag, "path"), kind: attr(tag, "type") })
        })
        .filter(|f| !f.id.is_empty() && !f.path.is_empty())
        .collect()
}

/// A path under the old machine's home, under the new one's (others stay).
pub fn move_home(path: &str, old: &str, new: &str) -> String {
    match path.strip_prefix(old).filter(|r| !old.is_empty() && old != new && (r.is_empty() || r.starts_with('/'))) {
        Some(rest) => format!("{new}{rest}"),
        None => path.to_owned(),
    }
}

/// This account's sync state for the backup, or None when there is nothing to keep.
pub fn saved(account: &str) -> Option<Vec<u8>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let s = Saved { on: is_on(account), home, folders: saved_folders(account), shares: crate::shares::Store::load(account) };
    if !s.on && s.folders.is_empty() && s.shares.shares.is_empty() {
        return None;
    }
    serde_json::to_vec(&s).ok()
}

/// Where a restore leaves the sync state of an account, until the user answers.
pub fn restored_path(uri: &str) -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("joma/restored-sync").join(format!("{uri}.json")))
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
    /// Folders shared with contacts and conversations, and offers.
    pub store: crate::shares::Store,
    /// Our service for contacts, and whom it lets in.
    pub peer_service: Option<String>,
    peer_allowed: Vec<String>,
    /// Contacts' devices that hold a folder we share, by Jami device id.
    pub remotes: HashMap<String, Peer>,
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
        self.store = crate::shares::Store::load(account);
        self.expose_peers(account, me);
        self.hello(account, me, device, name, false);
        self.announce(account, me, device, name);
    }

    /// Expose Syncthing to the members of our shares (again, when they change).
    pub fn expose_peers(&mut self, account: &str, me: &str) {
        let Some(st) = &self.st else { return };
        let allowed = self.store.allowed(me);
        if allowed == self.peer_allowed && (self.peer_service.is_some() || allowed.is_empty()) {
            return;
        }
        for s in joma_jami::exposed_services(account).unwrap_or_default() {
            if s.get("name").map(String::as_str) == Some(PEER_SERVICE) {
                if let Some(id) = s.get("id") {
                    let _ = joma_jami::remove_service(account, id);
                }
            }
        }
        self.peer_service = None;
        self.peer_allowed = allowed.clone();
        if allowed.is_empty() {
            return;
        }
        let list: Vec<&str> = allowed.iter().map(String::as_str).collect();
        match joma_jami::expose_service(account, PEER_SERVICE, st.listen_port, &list) {
            Ok(id) => self.peer_service = Some(id),
            Err(e) => self.error = Some(format!("cannot expose Syncthing to your contacts: {e:#}")),
        }
    }

    fn send(account: &str, to: &str, w: &crate::shares::Wire) {
        let mut p = joma_jami::Details::new();
        p.insert(MIME.into(), w.encode());
        if let Err(e) = joma_jami::send_account_message(account, to, &p, false) {
            tracing::warn!(error = %e, "could not send a sync message");
        }
    }

    fn join_wire(&self, folder: &str, device: &str, name: &str, reply: bool) -> Option<crate::shares::Wire> {
        let (st, service) = (self.st.as_ref()?, self.peer_service.as_ref()?);
        Some(crate::shares::Wire::Join(crate::shares::Join { folder: folder.into(), dev: device.into(), st: st.id.clone(), service: service.clone(), name: name.into(), reply }))
    }

    /// Tell the other members of every folder we hold where our Syncthing is (at start).
    pub fn announce(&self, account: &str, me: &str, device: &str, name: &str) {
        for s in self.store.shares.values() {
            if let Some(w) = self.join_wire(&s.id, device, name, false) {
                for u in s.others(me) {
                    Self::send(account, u, &w);
                }
            }
        }
    }

    /// Our own devices' Syncthing ids.
    fn own(&self) -> Vec<String> {
        self.peers.values().map(|p| p.st.clone()).collect()
    }

    /// Share a folder with contacts (`conv`: a conversation's files): it becomes ours, the
    /// members get an offer.
    #[allow(clippy::too_many_arguments)]
    pub fn create_share(&mut self, account: &str, me: &str, my_name: &str, id: Option<String>, label: &str, path: &Path, conv: Option<String>, members: Vec<crate::shares::Member>) -> Result<crate::shares::Share> {
        let st = self.st.as_ref().ok_or_else(|| anyhow::anyhow!("sync is off (joma sync on)"))?;
        let id = id.unwrap_or_else(|| joma_sync::folder_id(label));
        st.set_folder_as(&id, label, path, &self.own(), "sendreceive")?;
        let share = crate::shares::Share { id: id.clone(), label: label.into(), path: path.to_string_lossy().into_owned(), conv, owner: me.into(), owner_name: my_name.into(), members, off: Vec::new() };
        self.store.shares.insert(id, share.clone());
        self.store.save(account);
        self.expose_peers(account, me);
        let offer = crate::shares::Wire::Offer(crate::shares::Offer::of(&share));
        for u in share.others(me) {
            Self::send(account, u, &offer);
        }
        Ok(share)
    }

    /// Set up again what a backup kept (after a restore): the folders at their paths and
    /// the shares with contacts, who hear from us again.
    pub fn apply_saved(&mut self, account: &str, me: &str, device: &str, name: &str, saved: &Saved) -> Result<usize> {
        let st = self.st.as_ref().ok_or_else(|| anyhow::anyhow!("sync is not running"))?;
        let own = self.own();
        let here = std::env::var("HOME").unwrap_or_default();
        let moved = |p: &str| move_home(p, &saved.home, &here);
        let mut n = 0;
        for f in &saved.folders {
            let moved_path = moved(&f.path);
            let path = Path::new(&moved_path);
            std::fs::create_dir_all(path)?;
            st.set_folder_as(&f.id, &f.label, path, &own, if f.kind.is_empty() { "sendreceive" } else { &f.kind })?;
            n += 1;
        }
        for (id, s) in &saved.shares.shares {
            let mut s = s.clone();
            s.path = moved(&s.path);
            self.store.shares.entry(id.clone()).or_insert(s);
        }
        self.store.convs.extend(saved.shares.convs.iter().cloned());
        self.store.save(account);
        self.expose_peers(account, me);
        self.announce(account, me, device, name);
        Ok(n)
    }

    /// Accept an offer: the folder goes to `path` (default `~/joma-sync/<label>`, or the
    /// conversation's place).
    pub fn join(&mut self, account: &str, me: &str, device: &str, name: &str, folder: &str, path: Option<PathBuf>, conv_title: &str) -> Result<crate::shares::Share> {
        let st = self.st.as_ref().ok_or_else(|| anyhow::anyhow!("sync is off (joma sync on)"))?;
        let o = self.store.offers.get(folder).cloned().ok_or_else(|| anyhow::anyhow!("no offer for {folder}"))?;
        let path = path.unwrap_or_else(|| match &o.conv {
            Some(_) => crate::shares::conv_dir(conv_title, if o.owner_name.is_empty() { &o.owner[..8] } else { &o.owner_name }),
            None => default_dir().join(crate::shares::segment(&o.label)),
        });
        let share = crate::shares::Share { id: o.folder.clone(), label: o.label.clone(), path: path.to_string_lossy().into_owned(), conv: o.conv.clone(), owner: o.owner.clone(), owner_name: o.owner_name.clone(), members: o.members.clone(), off: Vec::new() };
        st.set_folder_as(&share.id, &share.label, &path, &self.own(), share.kind_for(me))?;
        self.store.offers.remove(folder);
        if let Some(c) = &share.conv {
            self.store.convs.insert(c.clone());
            self.store.declined.remove(c);
        }
        self.store.shares.insert(share.id.clone(), share.clone());
        self.store.save(account);
        self.expose_peers(account, me);
        if let Some(w) = self.join_wire(&share.id, device, name, false) {
            for u in share.others(me) {
                Self::send(account, u, &w);
            }
        }
        Ok(share)
    }

    pub fn decline(&mut self, account: &str, folder: &str) {
        if let Some(o) = self.store.offers.remove(folder) {
            if let Some(c) = o.conv {
                // "not now" for one folder of a conversation is not now for all of them.
                let rest: Vec<String> = self.store.offers.values().filter(|x| x.conv.as_ref() == Some(&c)).map(|x| x.folder.clone()).collect();
                for f in rest {
                    self.store.offers.remove(&f);
                }
                self.store.declined.insert(c);
            }
            self.store.save(account);
        }
    }

    /// An owner or admin lets a member write, or not (`uri` "all": every member).
    pub fn set_write(&mut self, account: &str, me: &str, folder: &str, uri: &str, write: bool) -> Result<()> {
        self.set_member(account, me, folder, uri, |m| m.write = write, false)
    }

    /// The owner makes a member an admin (who may change who writes), or not.
    pub fn set_admin(&mut self, account: &str, me: &str, folder: &str, uri: &str, admin: bool) -> Result<()> {
        self.set_member(account, me, folder, uri, |m| m.admin = admin, true)
    }

    fn set_member(&mut self, account: &str, me: &str, folder: &str, uri: &str, change: impl Fn(&mut crate::shares::Member), owner_only: bool) -> Result<()> {
        let s = self.store.shares.get_mut(folder).ok_or_else(|| anyhow::anyhow!("no shared folder {folder}"))?;
        if owner_only && s.owner != me {
            anyhow::bail!("only the folder's owner chooses its admins");
        }
        if !s.is_admin(me) {
            anyhow::bail!("only the folder's owner or an admin changes who writes");
        }
        if uri == "all" {
            s.members.iter_mut().for_each(&change);
        } else {
            let m = s.members.iter_mut().find(|m| m.uri == uri).ok_or_else(|| anyhow::anyhow!("not a member of {}", s.label))?;
            change(m);
        }
        let (w, others): (crate::shares::Wire, Vec<String>) = (crate::shares::Wire::Members { folder: folder.into(), members: s.members.clone() }, s.others(me).map(str::to_owned).collect());
        self.store.save(account);
        for u in others {
            Self::send(account, &u, &w);
        }
        Ok(())
    }

    /// A folder on (or off) for one of our own devices.
    pub fn set_device_on(&mut self, account: &str, folder: &str, device_st: &str, on: bool) -> Result<()> {
        let st = self.st.as_ref().ok_or_else(|| anyhow::anyhow!("sync is off"))?;
        st.share_with(folder, device_st, on)?;
        if let Some(s) = self.store.shares.get_mut(folder) {
            s.off.retain(|d| d != device_st);
            if !on {
                s.off.push(device_st.into());
            }
            self.store.save(account);
        }
        Ok(())
    }

    /// Leave a shared folder (the files stay).
    pub fn leave(&mut self, account: &str, me: &str, folder: &str) -> Result<()> {
        let st = self.st.as_ref().ok_or_else(|| anyhow::anyhow!("sync is off"))?;
        st.remove_folder(folder)?;
        if let Some(s) = self.store.shares.remove(folder) {
            for u in s.others(me) {
                Self::send(account, u, &crate::shares::Wire::Leave { folder: folder.into() });
            }
            self.store.save(account);
            self.expose_peers(account, me);
        }
        Ok(())
    }

    /// A message about shared folders from a contact (or our own account). Returns what
    /// to tell the user, if anything.
    pub fn heard_wire(&mut self, account: &str, me: &str, device: &str, name: &str, from: &str, w: crate::shares::Wire) -> Heard {
        use crate::shares::Wire;
        match w {
            Wire::Offer(o) if from != me => {
                let (folder, label) = (o.folder.clone(), o.label.clone());
                let auto = self.store.offered(from, o, crate::github::now());
                self.store.save(account);
                if auto {
                    Heard::AutoJoin(folder)
                } else if self.store.offers.contains_key(&folder) {
                    Heard::Offer(label)
                } else {
                    Heard::Nothing
                }
            }
            Wire::Join(j) if from != me => {
                let Some(s) = self.store.shares.get(&j.folder) else { return Heard::Nothing };
                if !s.has(from) {
                    return Heard::Nothing;
                }
                let Some(st) = &self.st else { return Heard::Nothing };
                let known = self.remotes.get(&j.dev).cloned();
                let addrs: Vec<String> = known.as_ref().and_then(|p| p.port).map(addr).into_iter().collect();
                if let Err(e) = st.set_peer_device(&j.st, &j.name, &addrs).and_then(|_| st.share_with(&j.folder, &j.st, true)) {
                    self.error = Some(format!("{e:#}"));
                }
                let mut peer = known.unwrap_or_default();
                if peer.tunnel.is_none() || peer.service != j.service || peer.st != j.st {
                    if let Some(t) = peer.tunnel.take() {
                        let _ = joma_jami::close_tunnel(account, &t);
                    }
                    match joma_jami::open_tunnel(account, from, &j.dev, &j.service, PEER_SERVICE) {
                        Ok(t) => peer.tunnel = Some(t),
                        Err(e) => self.error = Some(format!("cannot reach {}: {e:#}", if j.name.is_empty() { &j.dev } else { &j.name })),
                    }
                }
                peer.uri = from.into();
                peer.st = j.st.clone();
                peer.name = j.name.clone();
                peer.service = j.service.clone();
                self.remotes.insert(j.dev.clone(), peer);
                if !j.reply {
                    if let Some(w) = self.join_wire(&j.folder, device, name, true) {
                        Self::send(account, from, &w);
                    }
                }
                Heard::Nothing
            }
            Wire::Members { folder, members } => {
                let Some(s) = self.store.shares.get_mut(&folder) else { return Heard::Nothing };
                if !s.is_admin(from) {
                    return Heard::Nothing;
                }
                s.members = members;
                let s = s.clone();
                if let Some(st) = &self.st {
                    let devices: Vec<String> = st.folders().ok().and_then(|f| f.into_iter().find(|f| f.id == folder)).map(|f| f.devices).unwrap_or_default();
                    if let Err(e) = st.set_folder_as(&s.id, &s.label, Path::new(&s.path), &devices, s.kind_for(me)) {
                        self.error = Some(format!("{e:#}"));
                    }
                }
                self.store.save(account);
                self.expose_peers(account, me);
                Heard::Changed(format!("{}: you can {} now", s.label, if s.can_write(me) { "add and change files" } else { "only read the files" }))
            }
            Wire::Leave { folder } => {
                let gone: Vec<String> = self.remotes.values().filter(|p| p.uri == from).map(|p| p.st.clone()).collect();
                if let Some(st) = &self.st {
                    for d in gone {
                        let _ = st.share_with(&folder, &d, false);
                    }
                }
                if let Some(s) = self.store.shares.get_mut(&folder) {
                    if s.owner == me {
                        s.members.retain(|m| m.uri != from);
                        self.store.save(account);
                    }
                }
                Heard::Nothing
            }
            _ => Heard::Nothing,
        }
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
        let off: Vec<String> = self.store.shares.values().filter(|s| s.off.contains(&hello.st)).map(|s| s.id.clone()).collect();
        if let Err(e) = st.share_all_but(&hello.st, &off) {
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
        if let Some(peer) = self.remotes.values_mut().find(|p| p.tunnel.as_deref() == Some(tunnel)) {
            peer.port = Some(port);
            if let Some(st) = &self.st {
                if let Err(e) = st.set_peer_device(&peer.st, &peer.name, &[addr(port)]) {
                    self.error = Some(format!("{e:#}"));
                }
            }
            return true;
        }
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
        let Some(peer) = self.peers.values_mut().chain(self.remotes.values_mut()).find(|p| p.tunnel.as_deref() == Some(tunnel)) else { return false };
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
        for p in self.peers.values_mut().chain(self.remotes.values_mut()) {
            if let Some(t) = p.tunnel.take() {
                let _ = joma_jami::close_tunnel(account, &t);
            }
        }
        self.peers.clear();
        self.remotes.clear();
        if let Some(id) = self.service.take() {
            let _ = joma_jami::remove_service(account, &id);
        }
        if let Some(id) = self.peer_service.take() {
            let _ = joma_jami::remove_service(account, &id);
        }
        self.peer_allowed.clear();
        if let Some(mut st) = self.st.take() {
            st.stop();
        }
        self.starting = false;
    }

    /// What a report needs, to build it off the UI thread.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { api: self.st.as_ref().map(|s| s.api.clone()), peers: self.peers.clone(), remotes: self.remotes.clone(), store: self.store.clone(), error: self.error.clone(), starting: self.starting }
    }
}

/// The sync state copied out of the app, for [`Snapshot::report`].
pub struct Snapshot {
    api: Option<joma_sync::Api>,
    peers: HashMap<String, Peer>,
    remotes: HashMap<String, Peer>,
    store: crate::shares::Store,
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
            .chain(self.remotes.iter())
            .map(|(dev, p)| {
                let s = status.get(&p.st);
                let path = path_of(dev);
                DeviceLine {
                    uri: p.uri.clone(),
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
        devices.sort_by(|a, b| (!a.uri.is_empty(), &a.name).cmp(&(!b.uri.is_empty(), &b.name)));
        r.devices = devices;
        let kinds: HashMap<String, String> = st.folders().unwrap_or_default().into_iter().map(|f| (f.id, f.kind)).collect();
        for f in &mut r.folders {
            f.kind = kinds.get(&f.id).cloned().unwrap_or_default();
        }
        r.shares = self.store.shares.values().cloned().collect();
        // An offer for a folder that reached us from another of our devices is taken.
        r.offers = self.store.offers.values().filter(|o| !r.folders.iter().any(|f| f.id == o.folder)).cloned().collect();
        r
    }
}

fn addr(port: u16) -> String {
    format!("tcp://127.0.0.1:{port}")
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceLine {
    /// A contact's device: the contact's account (empty: one of ours).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub uri: String,
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
    /// Folders shared with contacts and conversations.
    pub shares: Vec<crate::shares::Share>,
    /// Offers waiting for an answer.
    pub offers: Vec<crate::shares::Offer>,
}

/// What a message about shared folders means for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    Nothing,
    /// A new offer, waiting in the conversation and the files view.
    Offer(String),
    /// A conversation whose files we joined offered another member's folder.
    AutoJoin(String),
    Changed(String),
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
    fn saved_state() {
        let xml = r#"<configuration><folder id="joma-trip-1" label="Trip &amp; more" path="/home/a/joma-sync/Trip" type="receiveonly" rescanIntervalS="3600"><device id="X"></device></folder>
<folder id="jc-c-o" label="Marta&#39;s files" path="/data/x" type="sendreceive"></folder><device id="X" name="y"></device></configuration>"#;
        let f = parse_folders(xml);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0], SavedFolder { id: "joma-trip-1".into(), label: "Trip & more".into(), path: "/home/a/joma-sync/Trip".into(), kind: "receiveonly".into() });
        assert_eq!(f[1].label, "Marta's files");
        assert_eq!(move_home("/home/a/joma-sync/Trip", "/home/a", "/home/b"), "/home/b/joma-sync/Trip");
        assert_eq!(move_home("/home/ab/x", "/home/a", "/home/b"), "/home/ab/x");
        assert_eq!(move_home("/data/x", "/home/a", "/home/b"), "/data/x");
        assert_eq!(move_home("/home/a/x", "/home/a", "/home/a"), "/home/a/x");
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
