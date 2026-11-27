//! The account's live connections to other devices, and which path each one takes.
//!
//! libjami's `getConnectionList` gives one entry per connection (`id`, `peer`, `device`,
//! `status`, `remoteAddress`), `getChannelList` the channels open on it (`id`, `name`).
//! Joma sorts the remote address into a [`Path`]: through the account's TURN server
//! (a public relay), over Tailscale (itself possibly relayed by DERP), on the local
//! network, or straight to a public address.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use anyhow::Result;
use serde::Serialize;
use serde_json::json;

use crate::{typed, Details};

/// How a connection reaches the other device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Path {
    /// Through the account's TURN server (turn.jami.net by default): slow, shared.
    Relayed,
    /// Over Tailscale, peer to peer.
    Tailscale,
    /// Over Tailscale, but through one of its DERP relays.
    TailscaleRelayed,
    /// A private address: the same network.
    Local,
    /// A public address, peer to peer.
    Direct,
    /// No address yet (still connecting) or one Joma cannot read.
    Unknown,
}

impl Path {
    pub fn label(self) -> &'static str {
        match self {
            Path::Relayed => "relayed (TURN)",
            Path::Tailscale => "Tailscale",
            Path::TailscaleRelayed => "Tailscale, relayed (DERP)",
            Path::Local => "local network",
            Path::Direct => "direct",
            Path::Unknown => "unknown",
        }
    }

    /// Slow paths: fine for messages, not for large files or a desktop share.
    pub fn relayed(self) -> bool {
        matches!(self, Path::Relayed | Path::TailscaleRelayed)
    }
}

/// dhtnet's `ConnectionStatus`, as the Jami app's connection monitor labels it.
pub fn status_label(status: &str) -> &'static str {
    match status {
        "0" => "connected",
        "1" => "TLS handshake",
        "2" => "ICE negotiation",
        "3" => "connecting",
        "4" => "waiting",
        _ => "unknown",
    }
}

/// One connection to one device.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Link {
    pub id: String,
    /// The peer's account URI.
    pub peer: String,
    pub device: String,
    pub status: String,
    pub remote_address: String,
    pub path: Path,
    /// The channels open on it (`sync`, `git://…`, `file://…`, service tunnels…).
    pub channels: Vec<String>,
}

/// The IP in `1.2.3.4:5`, `[::1]:5`, `::1` or `1.2.3.4`.
pub fn parse_ip(addr: &str) -> Option<IpAddr> {
    let a = addr.trim();
    if let Ok(ip) = a.parse() {
        return Some(ip);
    }
    if let Some(rest) = a.strip_prefix('[') {
        return rest.split(']').next()?.parse().ok();
    }
    let (host, _port) = a.rsplit_once(':')?;
    host.parse().ok()
}

fn tailscale_ip(ip: IpAddr) -> bool {
    match ip {
        // 100.64.0.0/10 (CGNAT range Tailscale uses).
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (o[1] & 0xc0) == 64
        }
        // fd7a:115c:a1e0::/48
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

fn local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            v6.is_loopback() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Sort an address. `relays` are the TURN server's addresses; `derp` says whether
/// Tailscale reaches that Tailscale address only through a DERP relay.
pub fn classify(addr: &str, relays: &[IpAddr], derp: impl Fn(IpAddr) -> bool) -> Path {
    let Some(ip) = parse_ip(addr) else { return Path::Unknown };
    let ip = match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    };
    if relays.contains(&ip) {
        Path::Relayed
    } else if tailscale_ip(ip) {
        if derp(ip) { Path::TailscaleRelayed } else { Path::Tailscale }
    } else if local_ip(ip) {
        Path::Local
    } else if ip == IpAddr::V4(Ipv4Addr::UNSPECIFIED) || ip == IpAddr::V6(Ipv6Addr::UNSPECIFIED) {
        Path::Unknown
    } else {
        Path::Direct
    }
}

/// The addresses of the account's TURN server (`TURN.server`, default turn.jami.net).
pub fn relay_addresses(account: &str) -> Vec<IpAddr> {
    let details = crate::account_details(account).unwrap_or_default();
    if details.get("TURN.enable").is_some_and(|v| v == "false") {
        return Vec::new();
    }
    let server = details.get("TURN.server").map(String::as_str).filter(|s| !s.is_empty()).unwrap_or("turn.jami.net");
    let host = if server.contains(':') && !server.starts_with('[') && server.parse::<IpAddr>().is_err() {
        server.to_string()
    } else {
        format!("{server}:3478")
    };
    host.to_socket_addrs().map(|a| a.map(|s| s.ip()).collect()).unwrap_or_default()
}

/// Tailscale addresses reached only through DERP, from `tailscale status --json` (empty
/// without Tailscale). A peer whose `CurAddr` is empty and whose `Relay` is set goes
/// through DERP.
pub fn derp_peers() -> Vec<IpAddr> {
    let Ok(out) = std::process::Command::new("tailscale").args(["status", "--json"]).output() else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    derp_from_status(&String::from_utf8_lossy(&out.stdout))
}

fn derp_from_status(json: &str) -> Vec<IpAddr> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let Some(peers) = v.get("Peer").and_then(|p| p.as_object()) else { return Vec::new() };
    peers
        .values()
        .filter(|p| {
            p.get("CurAddr").and_then(|a| a.as_str()).unwrap_or("").is_empty()
                && !p.get("Relay").and_then(|a| a.as_str()).unwrap_or("").is_empty()
        })
        .flat_map(|p| p.get("TailscaleIPs").and_then(|a| a.as_array()).cloned().unwrap_or_default())
        .filter_map(|ip| ip.as_str()?.parse().ok())
        .collect()
}

/// The account's connections (all, or those of one conversation's members).
pub fn list(account: &str, conversation: &str) -> Result<Vec<Link>> {
    let raw: Vec<Details> = typed("getConnectionList", json!([account, conversation]))?;
    let relays = relay_addresses(account);
    let derp = derp_peers();
    Ok(raw
        .into_iter()
        .map(|d| {
            let get = |k: &str| d.get(k).cloned().unwrap_or_default();
            let id = get("id");
            let channels: Vec<Details> = typed("getChannelList", json!([account, id])).unwrap_or_default();
            let remote_address = get("remoteAddress");
            Link {
                path: classify(&remote_address, &relays, |ip| derp.contains(&ip)),
                peer: get("peer"),
                device: get("device"),
                status: get("status"),
                channels: channels.into_iter().filter_map(|c| c.get("name").cloned()).collect(),
                remote_address,
                id,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(addr: &str) -> Path {
        classify(addr, &["203.0.113.7".parse().unwrap()], |ip| ip == "100.100.1.2".parse::<IpAddr>().unwrap())
    }

    #[test]
    fn sorts_addresses() {
        assert_eq!(c("203.0.113.7:3478"), Path::Relayed);
        assert_eq!(c("[::ffff:203.0.113.7]:50000"), Path::Relayed);
        assert_eq!(c("100.101.102.103:41641"), Path::Tailscale);
        assert_eq!(c("100.100.1.2:41641"), Path::TailscaleRelayed);
        assert_eq!(c("[fd7a:115c:a1e0::1]:5"), Path::Tailscale);
        assert_eq!(c("192.168.1.20:4000"), Path::Local);
        assert_eq!(c("10.0.0.5"), Path::Local);
        assert_eq!(c("[fe80::1]:9"), Path::Local);
        assert_eq!(c("8.8.8.8:5000"), Path::Direct);
        assert_eq!(c("[2001:db8::1]:5000"), Path::Direct);
        assert_eq!(c(""), Path::Unknown);
        assert_eq!(c("0.0.0.0:0"), Path::Unknown);
        // 100.128.x is outside Tailscale's /10.
        assert_eq!(c("100.128.0.1:1"), Path::Direct);
    }

    #[test]
    fn reads_derp_from_tailscale_status() {
        let json = r#"{"Peer":{
            "a":{"TailscaleIPs":["100.64.0.2","fd7a:115c:a1e0::2"],"CurAddr":"","Relay":"fra"},
            "b":{"TailscaleIPs":["100.64.0.3"],"CurAddr":"1.2.3.4:41641","Relay":"fra"}}}"#;
        let ips = derp_from_status(json);
        assert!(ips.contains(&"100.64.0.2".parse().unwrap()));
        assert!(!ips.contains(&"100.64.0.3".parse().unwrap()));
        assert!(derp_from_status("not json").is_empty());
    }

    #[test]
    fn labels() {
        assert_eq!(status_label("0"), "connected");
        assert_eq!(status_label("4"), "waiting");
        assert!(Path::Relayed.relayed() && !Path::Direct.relayed());
    }
}
