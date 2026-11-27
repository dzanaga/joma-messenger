//! The devices this machine has already shown for an account (D-24, discussion
//! 2026-11-26-github question 13). A device that joins without this machine linking it
//! (a restore from the backup, or a link made on another device) is new: the devices tab
//! asks "keep / revoke" until it is kept or revoked.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn path(account: &str) -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    data.join("joma/devices-seen").join(account)
}

/// The devices seen so far; `None` the first time (nothing to compare with).
pub fn load(account: &str) -> Option<BTreeSet<String>> {
    let text = std::fs::read_to_string(path(account)).ok()?;
    Some(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

pub fn save(account: &str, seen: &BTreeSet<String>) {
    let p = path(account);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut text: String = seen.iter().map(|s| format!("{s}\n")).collect();
    if text.is_empty() {
        text.push('\n');
    }
    let _ = std::fs::write(p, text);
}

/// Devices in `now` that were not seen. The first time, and while this machine links a
/// device itself (`quiet`), every device counts as seen.
pub fn fresh(seen: Option<&BTreeSet<String>>, now: &[String], this: &str, quiet: bool) -> Vec<String> {
    match seen {
        None => Vec::new(),
        Some(_) if quiet => Vec::new(),
        Some(s) => now.iter().filter(|d| *d != this && !s.contains(*d)).cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_devices() {
        let s: BTreeSet<String> = ["a".into(), "b".into()].into();
        let now = vec!["a".to_owned(), "b".into(), "c".into(), "me".into()];
        assert_eq!(fresh(Some(&s), &now, "me", false), vec!["c".to_owned()]);
        assert!(fresh(None, &now, "me", false).is_empty());
        assert!(fresh(Some(&s), &now, "me", true).is_empty());
    }
}
