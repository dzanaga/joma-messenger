//! The running Joma listens on `$XDG_RUNTIME_DIR/joma.sock`. Each connection sends one
//! JSON request line and reads one JSON reply line.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Request {
    /// Raise the window.
    Show,
    /// Open the quick messages popup.
    QuickPopup,
    /// The quick-message targets and messages, for the bar plugin.
    Quick,
    /// Send `text` to a conversation (by id, or by a member's name or URI).
    Send { to: String, text: String },
    /// Send quick message number `index` (0-based) to `to`.
    SendQuick { to: String, index: usize },
}

/// One conversation offered by Quick Messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub conversation: String,
    pub title: String,
    pub unread: u32,
    pub favorite: bool,
    /// The last message, shortened.
    pub last: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct QuickState {
    pub account: String,
    pub registered: bool,
    pub unread: u32,
    pub targets: Vec<Target>,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ok", rename_all = "kebab-case")]
pub enum Reply {
    Done,
    Quick(QuickState),
    Error { message: String },
}

/// `joma.sock`, or `joma-<name>.sock` for a `JOMA_PROFILE` (so test profiles never reach
/// the user's Joma).
pub fn socket_path() -> Option<PathBuf> {
    let name = match std::env::var_os("JOMA_PROFILE") {
        Some(p) => {
            let p = PathBuf::from(p);
            format!("joma-{}.sock", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        }
        None => "joma.sock".to_owned(),
    };
    Some(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join(name))
}

/// Send one request to the running Joma.
pub fn request(req: &Request) -> Result<Reply> {
    let path = socket_path().ok_or_else(|| anyhow!("XDG_RUNTIME_DIR is not set"))?;
    let mut s = UnixStream::connect(&path).map_err(|_| anyhow!("Joma is not running"))?;
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    s.write_all(format!("{}\n", serde_json::to_string(req)?).as_bytes())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format() {
        let r = Request::Send { to: "bob".into(), text: "hi".into() };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"cmd":"send","to":"bob","text":"hi"}"#);
        let r: Reply = serde_json::from_str(r#"{"ok":"done"}"#).unwrap();
        assert_eq!(r, Reply::Done);
    }
}
