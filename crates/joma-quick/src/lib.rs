//! Quick Messages: a short list of canned messages sent in one step to a favourite
//! contact, from Joma, from the Omarchy bar (the `io.github.dzanaga.joma-quick` shell
//! plugin) or from the command line (`joma send`). All three talk to the running Joma
//! through [`ipc`], since only one process may run the Jami daemon.

pub mod ipc;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
    fn partial_config_keeps_defaults() {
        let cfg: QuickConfig = toml::from_str("messages = [\"hi\"]").unwrap();
        assert_eq!(cfg.messages, ["hi"]);
        assert_eq!(cfg.recent, 6);
    }
}
