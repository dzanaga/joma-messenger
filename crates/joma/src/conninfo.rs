//! Connection info: which devices the account is connected to and by which path
//! (`joma connections`, and the info view on contacts and devices).

use joma_jami::links::{status_label, Link, Path};
use serde::Serialize;

/// One connection, with names resolved, for `--json` and the interface.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    pub peer: String,
    pub name: String,
    pub device: String,
    /// Our own device's name when the peer is us, else empty.
    pub device_name: String,
    pub status: String,
    pub address: String,
    pub path: Path,
    pub relayed: bool,
    pub channels: Vec<String>,
}

/// Sort and name the connections: the peer's name from `name_of`, our own devices'
/// names from `own_device`.
pub fn rows(links: Vec<Link>, name_of: impl Fn(&str) -> String, own_device: impl Fn(&str) -> Option<String>) -> Vec<Row> {
    let mut rows: Vec<Row> = links
        .into_iter()
        .map(|l| Row {
            name: name_of(&l.peer),
            device_name: own_device(&l.device).unwrap_or_default(),
            status: status_label(&l.status).into(),
            relayed: l.path.relayed(),
            address: l.remote_address,
            path: l.path,
            channels: l.channels,
            device: l.device,
            peer: l.peer,
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.device.cmp(&b.device)));
    rows
}

/// The rows as text for the terminal, one block per peer.
pub fn text(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "no connections right now (devices connect when there is something to send)".into();
    }
    let mut out = Vec::new();
    let mut last = "";
    for r in rows {
        if r.peer != last {
            out.push(format!("{}  {}", r.name, r.peer));
            last = &r.peer;
        }
        let dev = if r.device_name.is_empty() { short(&r.device) } else { format!("{} ({})", r.device_name, short(&r.device)) };
        let addr = if r.address.is_empty() { String::new() } else { format!("  {}", r.address) };
        let warn = if r.relayed { "  ! slow: through a relay" } else { "" };
        out.push(format!("  device {dev}  {}  {}{addr}{warn}", r.status, r.path.label()));
        if !r.channels.is_empty() {
            out.push(format!("    channels: {}", r.channels.join(", ")));
        }
    }
    out.join("\n")
}

fn short(id: &str) -> String {
    format!("{}…", &id[..id.len().min(8)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(peer: &str, device: &str, addr: &str, path: Path) -> Link {
        Link {
            id: "1".into(),
            peer: peer.into(),
            device: device.into(),
            status: "0".into(),
            remote_address: addr.into(),
            path,
            channels: vec!["sync".into()],
        }
    }

    #[test]
    fn names_sorts_and_warns() {
        let rows = rows(
            vec![link("bbbb", "d2", "203.0.113.7:3478", Path::Relayed), link("aaaa", "d1aaaaaaaaaa", "192.168.1.2:5", Path::Local)],
            |p| if p == "aaaa" { "you".into() } else { "bob".into() },
            |d| (d == "d1aaaaaaaaaa").then(|| "laptop".into()),
        );
        assert_eq!(rows[0].name, "bob");
        assert!(rows[0].relayed);
        assert_eq!(rows[1].device_name, "laptop");
        let t = text(&rows);
        assert!(t.contains("bob  bbbb"));
        assert!(t.contains("relayed (TURN)  203.0.113.7:3478  ! slow"));
        assert!(t.contains("device laptop (d1aaaaaa…)  connected  local network"));
        assert!(t.contains("channels: sync"));
        assert!(text(&[]).starts_with("no connections"));
        let json = serde_json::to_string(&rows).unwrap();
        assert!(json.contains("\"path\":\"relayed\""));
    }
}
