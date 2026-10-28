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
    /// Conversation ids shown first. Empty: the most recent conversations.
    pub favorites: Vec<String>,
    /// How many recent conversations fill the list when `favorites` is short.
    pub recent: usize,
    /// Conversations removed from the lists (Joma's "removed contacts"): kept in Jami,
    /// hidden here until restored or deleted for good.
    pub removed: Vec<String>,
    /// Send no Joma presence (active, away, seen) to contacts.
    pub invisible: bool,
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
            recent: 6,
            removed: Vec::new(),
            invisible: false,
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

    #[test]
    fn partial_config_keeps_defaults() {
        let cfg: QuickConfig = toml::from_str("messages = [\"hi\"]").unwrap();
        assert_eq!(cfg.messages, ["hi"]);
        assert_eq!(cfg.recent, 6);
    }
}
