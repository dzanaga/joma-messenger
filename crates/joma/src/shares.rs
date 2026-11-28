//! Folders shared with contacts and conversations (discussion `2026-11-27-files`, user
//! answer to question 2): one model for every shared folder.
//!
//! - A **share** is one Syncthing folder with an owner (who made it) and members (Jami
//!   accounts, each reading or writing). The owner writes; members read unless the owner
//!   (or an admin) lets them write. A reader's folder is Syncthing's "receive only": what
//!   they delete or change stays on their side (the file stays in the folder, unsynced for
//!   them).
//! - A **conversation's files** are one share per member who dropped files in it, under
//!   `~/joma-sync/conversations/<conversation>/<member>/` with the sub-folders of
//!   [`category`]. Everyone writes their own; the others read it.
//! - Your own devices hold every share you joined (they sync with your device that
//!   joined), unless a folder is switched off for one of them.
//!
//! The wire: account messages [`crate::sync::MIME`] with a `kind` ([`Wire`]): `offer`
//! (the owner to each member), `join` (a member's device that accepted, to every member:
//! its Syncthing id and Jami service, so the devices that hold the folder pair with it),
//! `members` (new permissions), `leave`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    /// The member's Jami account (40 hex digits).
    pub uri: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub write: bool,
    #[serde(default)]
    pub admin: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    /// The Syncthing folder id, the same on every side.
    pub id: String,
    pub label: String,
    /// Where it is on this machine.
    pub path: String,
    /// A conversation's files: the conversation's id.
    #[serde(default)]
    pub conv: Option<String>,
    pub owner: String,
    #[serde(default)]
    pub owner_name: String,
    #[serde(default)]
    pub members: Vec<Member>,
    /// Own devices (Syncthing ids) this folder is switched off for.
    #[serde(default)]
    pub off: Vec<String>,
}

impl Share {
    pub fn can_write(&self, uri: &str) -> bool {
        uri == self.owner || self.members.iter().any(|m| m.uri == uri && m.write)
    }

    pub fn is_admin(&self, uri: &str) -> bool {
        uri == self.owner || self.members.iter().any(|m| m.uri == uri && m.admin)
    }

    pub fn has(&self, uri: &str) -> bool {
        uri == self.owner || self.members.iter().any(|m| m.uri == uri)
    }

    /// Syncthing's folder type on our side.
    pub fn kind_for(&self, me: &str) -> &'static str {
        if self.can_write(me) {
            "sendreceive"
        } else {
            "receiveonly"
        }
    }

    /// Everyone but us.
    pub fn others<'a>(&'a self, me: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        std::iter::once(self.owner.as_str()).chain(self.members.iter().map(|m| m.uri.as_str())).filter(move |u| *u != me)
    }
}

/// An offer to join a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub folder: String,
    pub label: String,
    pub owner: String,
    #[serde(default)]
    pub owner_name: String,
    #[serde(default)]
    pub conv: Option<String>,
    #[serde(default)]
    pub members: Vec<Member>,
    /// Unix seconds, when it arrived.
    #[serde(default)]
    pub at: u64,
}

impl Offer {
    pub fn of(s: &Share) -> Offer {
        Offer { folder: s.id.clone(), label: s.label.clone(), owner: s.owner.clone(), owner_name: s.owner_name.clone(), conv: s.conv.clone(), members: s.members.clone(), at: 0 }
    }
}

/// A member's device that holds a folder (or wants it): pair with it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Join {
    pub folder: String,
    /// The Jami device.
    pub dev: String,
    /// Its Syncthing id.
    pub st: String,
    /// Its Jami service for contacts.
    pub service: String,
    #[serde(default)]
    pub name: String,
    /// An answer to a join: don't answer back.
    #[serde(default)]
    pub reply: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Wire {
    Offer(Offer),
    Join(Join),
    Members { folder: String, members: Vec<Member> },
    Leave { folder: String },
}

impl Wire {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("plain data")
    }

    pub fn decode(text: &str) -> Option<Wire> {
        serde_json::from_str(text).ok()
    }
}

/// What this account shares and was offered, kept in `$XDG_DATA_HOME/joma/shares/<account>.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub shares: BTreeMap<String, Share>,
    #[serde(default)]
    pub offers: BTreeMap<String, Offer>,
    /// Conversations whose files we joined: their new folders join by themselves.
    #[serde(default)]
    pub convs: BTreeSet<String>,
    /// Conversations whose offers we declined ("not now").
    #[serde(default)]
    pub declined: BTreeSet<String>,
}

fn store_path(account: &str) -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("joma/shares").join(format!("{account}.json")))
}

impl Store {
    pub fn load(account: &str) -> Store {
        store_path(account).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, account: &str) {
        if let Some(p) = store_path(account) {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, serde_json::to_vec_pretty(self).unwrap_or_default());
        }
    }

    /// The accounts that may open a tunnel to our Syncthing (members of our shares).
    pub fn allowed(&self, me: &str) -> Vec<String> {
        let mut all: BTreeSet<String> = BTreeSet::new();
        for s in self.shares.values() {
            all.extend(s.others(me).map(str::to_owned));
        }
        all.into_iter().collect()
    }

    /// Our share of a conversation's files, if there is one.
    pub fn mine_in(&self, conv: &str, me: &str) -> Option<&Share> {
        self.shares.values().find(|s| s.conv.as_deref() == Some(conv) && s.owner == me)
    }

    pub fn of_conv<'a>(&'a self, conv: &'a str) -> impl Iterator<Item = &'a Share> + 'a {
        self.shares.values().filter(move |s| s.conv.as_deref() == Some(conv))
    }

    /// An offer arrived from `from`: keep it, unless it is ours already. Returns whether
    /// to join it by itself (a conversation whose files we joined).
    pub fn offered(&mut self, from: &str, mut o: Offer, now: u64) -> bool {
        if o.owner != from && !self.shares.get(&o.folder).is_some_and(|s| s.is_admin(from)) {
            return false;
        }
        if let Some(s) = self.shares.get_mut(&o.folder) {
            // Known: only the members may change.
            s.members = o.members;
            return false;
        }
        // A conversation whose files we take part in: its new folders join by themselves.
        let auto = o.conv.as_ref().is_some_and(|c| self.convs.contains(c) || self.shares.values().any(|s| s.conv.as_ref() == Some(c)));
        o.at = now;
        self.offers.insert(o.folder.clone(), o);
        auto
    }
}

/// A conversation's file goes into one of these sub-folders.
pub fn category(file: &Path, message: bool) -> &'static str {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "gif" => "gifs",
        "png" | "jpg" | "jpeg" | "webp" | "heic" | "heif" | "bmp" | "svg" | "avif" | "tif" | "tiff" => "images",
        "ogg" | "opus" | "m4a" | "mp3" | "wav" | "flac" | "aac" if message => "audio/messages",
        "ogg" | "opus" | "m4a" | "mp3" | "wav" | "flac" | "aac" => "audio",
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v" if message => "video/messages",
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "m4v" => "video",
        _ => "documents",
    }
}

pub const CATEGORIES: [&str; 7] = ["images", "gifs", "audio", "audio/messages", "video", "video/messages", "documents"];

/// A name that is safe as one path segment.
pub fn segment(name: &str) -> String {
    let s: String = name.chars().map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_start_matches('.').to_owned();
    if s.is_empty() {
        "unnamed".into()
    } else {
        s.chars().take(80).collect()
    }
}

/// Where a conversation's files live: `~/joma-sync/conversations/<conversation>/<member>`.
pub fn conv_dir(conv_title: &str, member: &str) -> PathBuf {
    crate::sync::default_dir().join("conversations").join(segment(conv_title)).join(segment(member))
}

/// The folder id of a member's files in a conversation: the same on every side.
pub fn conv_folder_id(conv: &str, owner: &str) -> String {
    format!("jc-{}-{}", &conv[..conv.len().min(10)], &owner[..owner.len().min(10)])
}

/// A file name not taken yet in `dir` (`name (2).ext`, …).
pub fn free_name(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    (2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).expect("some name is free")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn share() -> Share {
        Share {
            id: "f".into(),
            owner: "alice".into(),
            members: vec![Member { uri: "bob".into(), ..Default::default() }, Member { uri: "carol".into(), write: true, ..Default::default() }],
            ..Default::default()
        }
    }

    #[test]
    fn permissions() {
        let s = share();
        assert!(s.can_write("alice") && s.can_write("carol") && !s.can_write("bob") && !s.can_write("eve"));
        assert_eq!(s.kind_for("bob"), "receiveonly");
        assert_eq!(s.kind_for("alice"), "sendreceive");
        assert_eq!(s.others("bob").collect::<Vec<_>>(), ["alice", "carol"]);
        assert!(s.is_admin("alice") && !s.is_admin("carol"));
    }

    #[test]
    fn wire_round_trip() {
        let w = Wire::Join(Join { folder: "f".into(), dev: "d".into(), st: "S".into(), service: "s".into(), name: "n".into(), reply: false });
        let t = w.encode();
        assert!(t.contains(r#""kind":"join""#), "{t}");
        assert_eq!(Wire::decode(&t), Some(w));
        let o = Wire::Offer(Offer::of(&share()));
        assert_eq!(Wire::decode(&o.encode()), Some(o));
        // A plain Hello (own devices) is not a Wire.
        assert_eq!(Wire::decode(r#"{"v":1,"dev":"d","st":"x","service":"s"}"#), None);
    }

    #[test]
    fn offers() {
        let mut st = Store::default();
        let o = Offer::of(&share());
        // Only the owner offers.
        assert!(!st.offered("mallory", o.clone(), 1));
        assert!(st.offers.is_empty());
        assert!(!st.offered("alice", o.clone(), 1));
        assert_eq!(st.offers["f"].at, 1);
        // A conversation we joined: its folders join by themselves.
        st.convs.insert("c1".into());
        let mut o2 = o.clone();
        o2.folder = "g".into();
        o2.conv = Some("c1".into());
        assert!(st.offered("alice", o2, 2));
        let mut s = share();
        s.members.clear();
        st.shares.insert("f".into(), s);
        assert!(!st.offered("alice", o, 3));
        assert_eq!(st.shares["f"].members.len(), 2);
        assert_eq!(st.allowed("bob"), ["alice", "carol"]);
        // A conversation we take part in (we share our files there) joins by itself too.
        let mut mine = share();
        mine.id = "h".into();
        mine.conv = Some("c2".into());
        st.shares.insert("h".into(), mine);
        let mut o3 = Offer::of(&share());
        o3.folder = "k".into();
        o3.conv = Some("c2".into());
        assert!(st.offered("alice", o3, 4));
    }

    #[test]
    fn categories() {
        assert_eq!(category(Path::new("a.GIF"), false), "gifs");
        assert_eq!(category(Path::new("a.jpg"), false), "images");
        assert_eq!(category(Path::new("v.ogg"), true), "audio/messages");
        assert_eq!(category(Path::new("v.ogg"), false), "audio");
        assert_eq!(category(Path::new("m.webm"), true), "video/messages");
        assert_eq!(category(Path::new("x.pdf"), false), "documents");
        assert_eq!(segment("../a/b"), "_a_b");
        assert_eq!(conv_folder_id("0123456789abc", "fedcba9876543"), "jc-0123456789-fedcba9876");
    }

    #[test]
    fn free_names() {
        let d = std::env::temp_dir().join(format!("joma-free-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(free_name(&d, "a.txt"), d.join("a.txt"));
        std::fs::write(d.join("a.txt"), "x").unwrap();
        assert_eq!(free_name(&d, "a.txt"), d.join("a (2).txt"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
