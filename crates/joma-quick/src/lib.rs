//! Quick Messages: a short list of canned messages sent in one step to a favourite
//! contact, from Joma, from the Omarchy bar (the `io.github.dzanaga.joma-quick` shell
//! plugin) or from the command line (`joma send`). All three talk to the running Joma
//! through [`ipc`], since only one process may run the Jami daemon.

pub mod ipc;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A conversation the bar plugin may list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub id: String,
    pub unread: u32,
    pub online: bool,
}

/// `~/.config/joma/quick.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuickConfig {
    /// The canned messages, in menu order (Alt+1…9 in Joma).
    pub messages: Vec<String>,
    /// Starred conversations: shown first, in this order (drag to reorder).
    pub favorites: Vec<String>,
    /// Starred contacts (URIs): first in the contacts list, in this order.
    pub starred: Vec<String>,
    /// The other contacts in the order they were dragged to; new ones go last.
    pub contact_order: Vec<String>,
    /// How many recent conversations fill the list when `favorites` is short.
    pub recent: usize,
    /// Conversations removed from the lists (Joma's "removed contacts"): kept in Jami,
    /// hidden here until restored or deleted for good.
    pub removed: Vec<String>,
    /// Send no Joma presence (active, away, seen) to contacts.
    pub invisible: bool,
    /// The setup banner was closed.
    pub setup_dismissed: bool,
}

impl Default for QuickConfig {
    fn default() -> Self {
        QuickConfig {
            messages: [
                "On my way",
                "Give me five minutes",
                "In a meeting, I'll call you back",
                "Can you call me?",
                "Thanks!",
                "👍",
            ]
            .map(String::from)
            .to_vec(),
            favorites: Vec::new(),
            starred: Vec::new(),
            contact_order: Vec::new(),
            recent: 6,
            removed: Vec::new(),
            invisible: false,
            setup_dismissed: false,
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("joma/quick.toml"))
}

impl QuickConfig {
    /// The saved config, or the defaults when there is none (or it does not parse).
    pub fn load() -> QuickConfig {
        config_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path().ok_or_else(|| anyhow::anyhow!("no config directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn toggle_favorite(&mut self, conversation: &str) {
        if let Some(i) = self.favorites.iter().position(|f| f == conversation) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(conversation.to_owned());
        }
    }

    pub fn toggle_starred(&mut self, uri: &str) {
        if let Some(i) = self.starred.iter().position(|f| f == uri) {
            self.starred.remove(i);
        } else {
            self.starred.push(uri.to_owned());
        }
    }

    /// The contacts the bar plugin lists: pinned ones and those with unread messages on
    /// top, then whoever is online; when that makes fewer than `recent`, the most recent
    /// offline ones fill the list. `candidates` is newest first.
    pub fn rank(&self, candidates: &[Candidate]) -> Vec<String> {
        let pinned = |c: &Candidate| self.favorites.iter().position(|f| *f == c.id);
        let mut top: Vec<&Candidate> = candidates.iter().filter(|c| pinned(c).is_some() || c.unread > 0).collect();
        // Pinned in their order, then unread (newest first, as given).
        top.sort_by_key(|c| (pinned(c).is_none(), pinned(c)));
        let mut out: Vec<String> = top.iter().map(|c| c.id.clone()).collect();
        out.extend(candidates.iter().filter(|c| c.online && !out.contains(&c.id)).map(|c| c.id.clone()).collect::<Vec<_>>());
        let short = self.recent.saturating_sub(out.len());
        let fill: Vec<String> = candidates.iter().filter(|c| !out.contains(&c.id)).take(short).map(|c| c.id.clone()).collect();
        out.extend(fill);
        out
    }

    /// Favourites first (in their order), then the most recent others, up to `recent`
    /// in total beyond the favourites. `recent_ids` is newest first.
    pub fn targets<'a>(&'a self, recent_ids: &'a [String]) -> Vec<&'a str> {
        let mut out: Vec<&str> =
            self.favorites.iter().map(String::as_str).filter(|f| recent_ids.iter().any(|r| r == f)).collect();
        let extra = recent_ids.iter().map(String::as_str).filter(|r| !out.contains(r)).take(self.recent).collect::<Vec<_>>();
        out.extend(extra);
        out
    }
}

/// Move `item` to where `target` is in `order`: before it when moving up, after it when
/// moving down. An `item` not in `order` lands before `target`.
pub fn reorder(order: &mut Vec<String>, item: &str, target: &str) {
    if item == target {
        return;
    }
    let from = order.iter().position(|o| o == item);
    if let Some(from) = from {
        order.remove(from);
    }
    let Some(to) = order.iter().position(|o| o == target) else {
        order.push(item.to_owned());
        return;
    };
    let after = from.is_some_and(|f| f <= to);
    order.insert(if after { to + 1 } else { to }, item.to_owned());
}

/// A drop in a list with a starred group on top. On a starred `target`, `item` joins the
/// stars at its place. On an unstarred one, `item` leaves the stars, and when the rest has
/// an order of its own (`rest`, as shown), it moves there.
pub fn drop_on(stars: &mut Vec<String>, rest: Option<&mut Vec<String>>, item: &str, target: &str) {
    if stars.iter().any(|s| s == target) {
        reorder(stars, item, target);
        return;
    }
    stars.retain(|s| s != item);
    if let Some(rest) = rest {
        reorder(rest, item, target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favourites_come_first_and_must_exist() {
        let cfg = QuickConfig { favorites: vec!["b".into(), "gone".into()], recent: 2, ..Default::default() };
        let recent: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        assert_eq!(cfg.targets(&recent), ["b", "a", "c"]);
    }

    #[test]
    fn rank_puts_pinned_and_unread_first_then_online() {
        let cfg = QuickConfig { favorites: vec!["p".into()], recent: 5, ..Default::default() };
        let c = |id: &str, unread, online| Candidate { id: id.into(), unread, online };
        // Newest first.
        let all = [c("a", 0, false), c("on", 0, true), c("u", 2, false), c("p", 0, false), c("old", 0, false)];
        assert_eq!(cfg.rank(&all), ["p", "u", "on", "a", "old"]);
        let cfg = QuickConfig { recent: 3, ..cfg };
        assert_eq!(cfg.rank(&all), ["p", "u", "on"]);
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reorder_moves_up_before_and_down_after() {
        let mut o = v(&["a", "b", "c", "d"]);
        reorder(&mut o, "d", "b");
        assert_eq!(o, ["a", "d", "b", "c"]);
        reorder(&mut o, "a", "b");
        assert_eq!(o, ["d", "b", "a", "c"]);
        reorder(&mut o, "new", "b");
        assert_eq!(o, ["d", "new", "b", "a", "c"]);
        reorder(&mut o, "x", "missing");
        assert_eq!(o.last().unwrap(), "x");
    }

    #[test]
    fn drops_star_unstar_and_reorder() {
        let mut stars = v(&["s1", "s2"]);
        // An unstarred item dropped on a star joins the stars there.
        drop_on(&mut stars, None, "x", "s2");
        assert_eq!(stars, ["s1", "x", "s2"]);
        // A star dropped on the rest leaves the stars; the rest keeps its own order.
        let mut rest = v(&["a", "b"]);
        drop_on(&mut stars, Some(&mut rest), "s1", "b");
        assert_eq!(stars, ["x", "s2"]);
        assert_eq!(rest, ["a", "s1", "b"]);
        // Chats have no order of their own beyond the stars.
        drop_on(&mut stars, None, "s2", "a");
        assert_eq!(stars, ["x"]);
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let cfg: QuickConfig = toml::from_str("messages = [\"hi\"]").unwrap();
        assert_eq!(cfg.messages, ["hi"]);
        assert_eq!(cfg.recent, 6);
    }
}
