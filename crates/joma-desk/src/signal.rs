//! The desktop-share handshake: messages of one MIME type, carried in a call, or straight
//! between the two accounts for a share without a call. Peers that never answer `Hello`
//! are plain Jami clients and get Jami's own screen share.

use serde::{Deserialize, Serialize};

/// MIME type of Joma's in-call messages; other Jami clients ignore it.
pub const MIME: &str = "application/x-joma-desk+json";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum DeskMsg {
    /// Sent when a call starts: this peer runs Joma, and what it can do.
    Hello {
        v: u32,
        /// Can capture and stream its Hyprland desktop.
        share: bool,
        /// Can show a stream.
        view: bool,
    },
    /// The sharer exposed a tunnel endpoint for this call.
    Offer {
        v: u32,
        /// Exposed service id, on device `device` of account `uri`.
        service: String,
        device: String,
        uri: String,
        /// The viewer presents it on connect: the endpoint is on the sharer's loopback,
        /// where any local process could reach it.
        token: String,
        output: String,
        width: u32,
        height: u32,
        /// Unix seconds when offered. A share without a call can be delivered late; the
        /// viewer ignores old offers.
        #[serde(default)]
        ts: u64,
    },
    /// The viewer is receiving the stream.
    Accept,
    /// The viewer cannot take the stream; the sharer falls back to Jami's screen share.
    Decline { reason: String },
    /// The sharer stopped (or, without a call, the viewer stopped watching).
    Stop,
}

impl DeskMsg {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("plain data")
    }

    pub fn decode(text: &str) -> Option<DeskMsg> {
        serde_json::from_str(text).ok()
    }
}

/// Seconds since the epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A random token, 32 hex digits, from the kernel's generator.
pub fn token() -> String {
    let mut b = [0u8; 16];
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).expect("/dev/urandom");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Constant-time comparison, so the token cannot be guessed byte by byte.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let m = DeskMsg::Hello { v: VERSION, share: true, view: true };
        assert_eq!(m.encode(), r#"{"t":"hello","v":1,"share":true,"view":true}"#);
        assert_eq!(DeskMsg::decode(&m.encode()), Some(m));
        assert_eq!(DeskMsg::decode("{\"t\":\"stop\"}"), Some(DeskMsg::Stop));
        assert_eq!(DeskMsg::decode("nonsense"), None);
    }

    #[test]
    fn offers_from_older_jomas_decode_without_a_time() {
        let old = r#"{"t":"offer","v":1,"service":"s","device":"d","uri":"u","token":"k","output":"o","width":1,"height":2}"#;
        assert!(matches!(DeskMsg::decode(old), Some(DeskMsg::Offer { ts: 0, .. })));
    }

    #[test]
    fn tokens() {
        let t = token();
        assert_eq!(t.len(), 32);
        assert_ne!(t, token());
        assert!(same(t.as_bytes(), t.as_bytes()));
        assert!(!same(t.as_bytes(), token().as_bytes()));
    }
}
