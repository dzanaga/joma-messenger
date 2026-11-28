//! The ✓ GitHub badge and adding contacts by GitHub name (D-24 step 3, discussion
//! 2026-11-26-github questions 6–8 and 14).
//!
//! - Your public gist `joma.json` lists the Jami accounts you made public:
//!   `{"joma":1, "github_id":<n>, "accounts":[{"jami":"<id>", "label":"…"}]}`. An account
//!   can be backed up without being listed (user, 2026-11-27).
//! - Each public account's profile carries `X-JOMA-GITHUB:<login>`; a contact's Joma
//!   matches it against that login's gist and its numeric id.
//! - Someone added by GitHub name stays pinned to that login: if their gist later names
//!   another Jami account, Joma offers to add it (user, 2026-11-27: keep the contact).
//!
//! Your own gist is written through `gh`; others' gists are read without a login.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const FILE: &str = "joma.json";
pub const VCARD_KEY: &str = "X-JOMA-GITHUB";
/// Contacts are checked again after this long.
pub const RECHECK: u64 = 3 * 24 * 3600;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub joma: u32,
    pub github_id: u64,
    pub accounts: Vec<Listed>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    pub jami: String,
    #[serde(default)]
    pub label: String,
}

/// What a contact's GitHub claim comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The gist lists this Jami account, and the login is the same GitHub user.
    Verified,
    /// The gist exists but does not list this account; it lists these instead.
    Elsewhere(Vec<Listed>),
    /// No gist, or one from another GitHub user than the profile names.
    Missing,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The verdict for `uri` from a login's gist (`None`: no gist) and the login's numeric id.
pub fn verdict(profile: Option<&Profile>, github_id: u64, uri: &str) -> Verdict {
    match profile {
        Some(p) if p.github_id == github_id && p.accounts.iter().any(|a| a.jami == uri) => Verdict::Verified,
        Some(p) if p.github_id == github_id && !p.accounts.is_empty() => Verdict::Elsewhere(p.accounts.clone()),
        _ => Verdict::Missing,
    }
}

// ---- The profile line ---------------------------------------------------------------

/// The GitHub login a vCard names, if any.
pub fn vcard_login(vcard: &str) -> Option<String> {
    vcard.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim().eq_ignore_ascii_case(VCARD_KEY)).then(|| v.trim().to_owned()).filter(|v| valid_login(v))
    })
}

/// The vCard with our line set to `login` (or removed).
pub fn vcard_with(vcard: &str, login: Option<&str>) -> String {
    let mut out: Vec<String> = vcard
        .lines()
        .filter(|l| !l.split_once(':').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(VCARD_KEY)))
        .map(str::to_owned)
        .collect();
    if let Some(login) = login {
        let at = out.iter().position(|l| l.trim().eq_ignore_ascii_case("END:VCARD")).unwrap_or(out.len());
        out.insert(at, format!("{VCARD_KEY}:{login}"));
    }
    let mut s = out.join("\r\n");
    s.push_str("\r\n");
    s
}

/// GitHub logins: letters, digits and single hyphens, up to 39.
pub fn valid_login(s: &str) -> bool {
    !s.is_empty() && s.len() <= 39 && !s.starts_with('-') && !s.ends_with('-') && !s.contains("--") && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

// ---- Reading others' gists (no login) -----------------------------------------------

fn curl(url: &str) -> Result<Option<Vec<u8>>> {
    let out = std::process::Command::new("curl")
        .args(["-sS", "-L", "--max-time", "20", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: joma", "-w", "\n%{http_code}", url])
        .output()
        .context("curl is not installed")?;
    if !out.status.success() {
        bail!("cannot reach GitHub: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let mut body = out.stdout;
    let pos = body.iter().rposition(|b| *b == b'\n').unwrap_or(0);
    let code: u16 = String::from_utf8_lossy(&body[pos + 1..]).trim().parse().unwrap_or(0);
    body.truncate(pos);
    match code {
        200 => Ok(Some(body)),
        404 => Ok(None),
        403 | 429 => bail!("GitHub's limit for reading without a login is reached; try again within the hour"),
        c => bail!("GitHub answered {c}"),
    }
}

/// GitHub's API; a test profile may point it at a local stand-in (`JOMA_GITHUB_API`).
fn api_base() -> String {
    match (std::env::var_os("JOMA_PROFILE"), std::env::var("JOMA_GITHUB_API")) {
        (Some(_), Ok(url)) if url.starts_with("http://127.0.0.1:") => url.trim_end_matches('/').to_owned(),
        _ => "https://api.github.com".into(),
    }
}

/// A login's numeric id and its `joma.json` (if it has one). `None`: no such login.
pub fn fetch(login: &str) -> Result<Option<(u64, Option<Profile>)>> {
    if !valid_login(login) {
        bail!("{login} is not a GitHub name");
    }
    let base = api_base();
    let Some(user) = curl(&format!("{base}/users/{login}"))? else { return Ok(None) };
    let user: serde_json::Value = serde_json::from_slice(&user)?;
    let id = user["id"].as_u64().ok_or_else(|| anyhow!("GitHub gave no id for {login}"))?;
    let gists = curl(&format!("{base}/users/{login}/gists?per_page=100"))?.unwrap_or_default();
    let gists: serde_json::Value = serde_json::from_slice(&gists).unwrap_or_default();
    let raw = gists.as_array().into_iter().flatten().find_map(|g| g["files"][FILE]["raw_url"].as_str().map(str::to_owned));
    let profile = match raw {
        Some(url) => curl(&url)?.and_then(|b| serde_json::from_slice::<Profile>(&b).ok()),
        None => None,
    };
    Ok(Some((id, profile)))
}

// ---- Writing our own gist (through gh) ----------------------------------------------

/// Through gh, or the Joma GitHub App's token once signed in.
fn gh(args: &[&str], input: Option<&str>) -> Result<serde_json::Value> {
    let out = joma_backup::gh::api(args, input.map(str::as_bytes))?;
    Ok(serde_json::from_slice(&out).unwrap_or_default())
}

/// Our login, numeric id, and our gist (id and content) if there is one.
fn mine() -> Result<(String, u64, Option<(String, Profile)>)> {
    let me = gh(&["user"], None).context("not signed in to GitHub")?;
    let login = me["login"].as_str().ok_or_else(|| anyhow!("gh gave no login"))?.to_owned();
    let id = me["id"].as_u64().ok_or_else(|| anyhow!("gh gave no id"))?;
    let gists = gh(&["gists?per_page=100"], None)?;
    let found = gists.as_array().into_iter().flatten().find(|g| g["files"].get(FILE).is_some()).and_then(|g| g["id"].as_str().map(str::to_owned));
    let gist = match found {
        Some(gid) => {
            let g = gh(&[&format!("gists/{gid}")], None)?;
            let text = g["files"][FILE]["content"].as_str().unwrap_or("{}");
            Some((gid, serde_json::from_str(text).unwrap_or_default()))
        }
        None => None,
    };
    Ok((login, id, gist))
}

/// List (`label` Some) or unlist (`None`) a Jami account in our public gist. Returns our
/// login and what the gist lists now.
pub fn publish(uri: &str, label: Option<&str>) -> Result<(String, Profile)> {
    let (login, id, gist) = mine()?;
    let (gid, mut p) = match gist {
        Some((g, p)) => (Some(g), p),
        None => (None, Profile::default()),
    };
    p.joma = 1;
    p.github_id = id;
    p.accounts.retain(|a| a.jami != uri);
    if let Some(l) = label {
        p.accounts.push(Listed { jami: uri.to_owned(), label: l.to_owned() });
    }
    let content = serde_json::to_string_pretty(&p)?;
    match (gid, p.accounts.is_empty()) {
        (Some(g), true) => {
            gh(&["-X", "DELETE", &format!("gists/{g}")], None)?;
        }
        (Some(g), false) => {
            let body = serde_json::json!({ "files": { FILE: { "content": content } } });
            gh(&["-X", "PATCH", &format!("gists/{g}"), "--input", "-"], Some(&body.to_string()))?;
        }
        (None, true) => {}
        (None, false) => {
            let body = serde_json::json!({
                "description": "Joma: the Jami accounts of this GitHub user",
                "public": true,
                "files": { FILE: { "content": content } }
            });
            gh(&["-X", "POST", "gists", "--input", "-"], Some(&body.to_string()))?;
        }
    }
    Ok((login, p))
}

// ---- What this machine remembers ----------------------------------------------------

/// A contact's GitHub state, kept between starts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Known {
    pub login: String,
    #[serde(default)]
    pub github_id: u64,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub checked: u64,
    /// Added by GitHub name: keep in touch with this login's accounts.
    #[serde(default)]
    pub pinned: bool,
    /// The gist names other accounts now (pinned contacts): offer to add them.
    #[serde(default)]
    pub moved_to: Vec<Listed>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Store {
    /// Contacts by Jami id.
    #[serde(default)]
    pub contacts: BTreeMap<String, Known>,
    /// Our accounts that are public, by Jami id → login.
    #[serde(default)]
    pub mine: BTreeMap<String, String>,
}

fn store_path(account: &str) -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    data.join("joma/github").join(format!("{account}.json"))
}

impl Store {
    pub fn load(account: &str) -> Store {
        std::fs::read_to_string(store_path(account)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, account: &str) {
        let p = store_path(account);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
    }

    /// Record a check of `uri` against `login`.
    pub fn record(&mut self, uri: &str, login: &str, github_id: u64, v: &Verdict) {
        let k = self.contacts.entry(uri.to_owned()).or_default();
        if k.login != login {
            k.login = login.to_owned();
        }
        k.github_id = github_id;
        k.checked = now();
        k.verified = *v == Verdict::Verified;
        k.moved_to = match v {
            Verdict::Elsewhere(list) if k.pinned => list.clone(),
            _ => Vec::new(),
        };
    }

    /// The login a contact is verified as (the badge), if any.
    pub fn badge(&self, uri: &str) -> Option<&str> {
        self.contacts.get(uri).filter(|k| k.verified).map(|k| k.login.as_str())
    }

    pub fn stale(&self, uri: &str, login: &str) -> bool {
        self.contacts.get(uri).is_none_or(|k| k.login != login || now().saturating_sub(k.checked) > RECHECK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VCF: &str = "BEGIN:VCARD\r\nVERSION:2.1\r\nFN:Alice\r\nEND:VCARD\r\n";

    #[test]
    fn profile_line() {
        assert_eq!(vcard_login(VCF), None);
        let with = vcard_with(VCF, Some("alice"));
        assert!(with.contains("X-JOMA-GITHUB:alice\r\nEND:VCARD"));
        assert_eq!(vcard_login(&with).as_deref(), Some("alice"));
        let again = vcard_with(&with, Some("alice2"));
        assert_eq!(again.matches(VCARD_KEY).count(), 1);
        assert_eq!(vcard_login(&vcard_with(&again, None)), None);
        assert_eq!(vcard_login("X-JOMA-GITHUB:not a login\n"), None);
    }

    #[test]
    fn verdicts() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let p = Profile { joma: 1, github_id: 7, accounts: vec![Listed { jami: a.clone(), label: "home".into() }] };
        assert_eq!(verdict(Some(&p), 7, &a), Verdict::Verified);
        // Another GitHub user's gist (a renamed or re-registered login) proves nothing.
        assert_eq!(verdict(Some(&p), 8, &a), Verdict::Missing);
        assert_eq!(verdict(Some(&p), 7, &b), Verdict::Elsewhere(p.accounts.clone()));
        assert_eq!(verdict(None, 7, &a), Verdict::Missing);
    }

    #[test]
    fn pinned_contacts_follow_their_login() {
        let a = "a".repeat(40);
        let c = "c".repeat(40);
        let mut s = Store::default();
        s.contacts.insert(a.clone(), Known { login: "alice".into(), pinned: true, ..Default::default() });
        let other = vec![Listed { jami: c.clone(), label: "new phone".into() }];
        s.record(&a, "alice", 7, &Verdict::Elsewhere(other.clone()));
        assert_eq!(s.contacts[&a].moved_to, other);
        assert_eq!(s.badge(&a), None);
        s.record(&a, "alice", 7, &Verdict::Verified);
        assert!(s.contacts[&a].moved_to.is_empty());
        assert_eq!(s.badge(&a), Some("alice"));
        // Not pinned: no offer, the badge just goes.
        s.record(&c, "carol", 9, &Verdict::Elsewhere(other));
        assert!(s.contacts[&c].moved_to.is_empty());
        assert!(!s.stale(&a, "alice") && s.stale(&a, "bob"));
    }

    #[test]
    fn logins() {
        assert!(valid_login("dz-anaga") && valid_login("a1"));
        assert!(!valid_login("-a") && !valid_login("a--b") && !valid_login("a b") && !valid_login(""));
    }
}
