//! Signing in with the Joma GitHub App (D-24 step 4) and automatic backups (step 5).
//!
//! Without a `gh` login Joma signs in with its GitHub App through the device flow: it
//! shows a code, the user types it at github.com/login/device. The token stays in memory
//! for this run (question 4: Joma keeps no token), unless automatic backups are on: then
//! the refresh token and the backup password live in the system keyring (user,
//! 2026-11-28: optional, a password prompt by default).

use std::path::PathBuf;

use anyhow::Result;
use joma_backup::device::{Code, Token};
use serde::{Deserialize, Serialize};

/// The app's public parts, written by `scripts/github-app.sh`.
const APP: &str = include_str!("../github-app.json");

#[derive(Deserialize)]
struct App {
    client_id: String,
    slug: String,
}

pub fn client_id() -> String {
    serde_json::from_str::<App>(APP).map(|a| a.client_id).unwrap_or_default()
}

/// Where the user installs the app on their joma-backup repo.
pub fn install_url() -> String {
    let slug = serde_json::from_str::<App>(APP).map(|a| a.slug).unwrap_or_else(|_| "joma-messenger".into());
    format!("https://github.com/apps/{slug}/installations/new")
}

/// What to do once signed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Nothing,
    Restore,
    Backup,
    /// Turn automatic backups on (asks the password, then keeps it).
    Auto,
    Publish(bool),
}

/// The sign-in card: the code to type, or why it failed.
#[derive(Debug, Clone)]
pub struct SignIn {
    pub after: After,
    pub code: Option<Code>,
    pub error: Option<String>,
}

/// Ask GitHub for a code, then poll until the user typed it (off the UI thread).
pub fn run(on_code: impl Fn(Code), cancelled: impl Fn() -> bool) -> Result<(Token, String), String> {
    let id = client_id();
    let code = joma_backup::device::start(&id).map_err(|e| format!("{e:#}"))?;
    on_code(code.clone());
    let started = std::time::Instant::now();
    let mut every = code.interval.max(5);
    let limit = if code.expires_in == 0 { 900 } else { code.expires_in };
    loop {
        for _ in 0..every * 4 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if cancelled() {
                return Err("cancelled".into());
            }
        }
        if started.elapsed().as_secs() > limit {
            return Err("the code expired: sign in again".into());
        }
        match joma_backup::device::poll(&id, &code).map_err(|e| format!("{e:#}"))? {
            joma_backup::device::Poll::Pending => {}
            joma_backup::device::Poll::SlowDown(s) => every = s.max(every + 5),
            joma_backup::device::Poll::Failed(m) => return Err(m),
            joma_backup::device::Poll::Token(t) => {
                joma_backup::gh::set_token(Some(t.access.clone()));
                let login = joma_backup::gh::login().map_err(|e| format!("{e:#}"))?;
                return Ok((t, login));
            }
        }
    }
}

/// GitHub can be reached now: gh is logged in, or we hold the app's token.
pub fn ready() -> bool {
    joma_backup::gh::has_token() || joma_backup::gh::cli_ready()
}

// ---- Automatic backups ---------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Auto {
    #[serde(default)]
    pub on: bool,
    /// The last backup that worked (any kind), unix seconds.
    #[serde(default)]
    pub last: u64,
}

/// One automatic backup a day.
pub const EVERY: u64 = 24 * 3600;

fn auto_path() -> Option<PathBuf> {
    Some(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?.join("joma/backup.toml"))
}

impl Auto {
    pub fn load() -> Auto {
        auto_path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = auto_path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, toml::to_string(self).unwrap_or_default());
        }
    }

    pub fn due(&self, now: u64) -> bool {
        self.on && now.saturating_sub(self.last) >= EVERY
    }
}

/// Keep what an automatic backup needs: the password, and the refresh token when Joma
/// reaches GitHub through its app (gh needs none).
pub fn remember(password: &str, token: Option<&Token>) -> Result<()> {
    crate::keyring::store(crate::keyring::PASSWORD, password)?;
    match token.filter(|t| !t.refresh.is_empty()) {
        Some(t) => crate::keyring::store(crate::keyring::REFRESH, &t.refresh)?,
        None => crate::keyring::clear(crate::keyring::REFRESH),
    }
    Ok(())
}

pub fn forget() {
    crate::keyring::clear(crate::keyring::PASSWORD);
    crate::keyring::clear(crate::keyring::REFRESH);
}

/// One automatic backup, off the UI thread: the password from the keyring; GitHub through
/// gh, the token in memory, or a fresh one from the kept refresh token.
pub fn auto_backup(accounts: &[crate::backup::Account]) -> Result<String, String> {
    let password = crate::keyring::lookup(crate::keyring::PASSWORD).ok_or("the backup password is no longer in the keyring: turn automatic backups on again")?;
    if !ready() {
        let refresh = crate::keyring::lookup(crate::keyring::REFRESH).ok_or("not signed in to GitHub: sign in with GitHub, then turn automatic backups on again")?;
        let t = joma_backup::device::refresh(&client_id(), &refresh).map_err(|e| format!("{e:#}"))?;
        joma_backup::gh::set_token(Some(t.access.clone()));
        if !t.refresh.is_empty() {
            let _ = crate::keyring::store(crate::keyring::REFRESH, &t.refresh);
        }
    }
    crate::backup::backup(accounts, &password, None).map_err(|e| format!("{e:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_is_there() {
        assert!(client_id().starts_with("Iv"), "{}", client_id());
        assert!(install_url().ends_with("/installations/new"));
    }

    #[test]
    fn due_daily() {
        let a = Auto { on: true, last: 1000 };
        assert!(!a.due(1000 + EVERY - 1));
        assert!(a.due(1000 + EVERY));
        assert!(!Auto { on: false, last: 0 }.due(EVERY * 9));
    }
}
