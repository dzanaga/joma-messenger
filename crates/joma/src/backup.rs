//! `joma backup` and `joma restore` (D-24, step 1): accounts encrypted with one password
//! into the private repo `<login>/joma-backup` through the user's `gh` login, or into a
//! file. The password is asked in Joma's own window, never on the command line; the work
//! runs off the UI thread.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use joma_backup::{gh, Entry, WrongPassword, EXT};

/// One account to back up.
#[derive(Debug, Clone)]
pub struct Account {
    /// The daemon's local id.
    pub id: String,
    /// The Jami id (40 hex digits).
    pub uri: String,
    pub label: String,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A private scratch file for the plain archive, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Scratch> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("joma-backup");
        std::fs::create_dir_all(&dir)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        Ok(Scratch(dir.join(name)))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn file_name(a: &Account) -> String {
    format!("{}.{EXT}", a.uri)
}

/// The account's sync state beside its archive (the archive's format stays Jami's own,
/// which the phone reads too).
fn sync_name(uri: &str) -> String {
    format!("{uri}.sync.{EXT}")
}

/// Keep a restored account's sync state for Joma to offer.
fn keep_sync_state(uri: &str, data: &[u8], password: &str) {
    if let Ok((_, plain)) = joma_backup::open(data, password) {
        if let Some(p) = crate::sync::restored_path(uri) {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, plain);
        }
    }
}

fn sealed(a: &Account, password: &str) -> Result<Vec<u8>> {
    let tmp = Scratch::new(&format!("{}.gz", a.uri))?;
    joma_jami::export_archive(&a.id, &tmp.0.to_string_lossy())?;
    let archive = std::fs::read(&tmp.0)?;
    joma_backup::seal(&archive, password, &a.uri, &a.label, now())
}

/// Back up `accounts`: into `file` (a file for one account, or a folder), else into the
/// GitHub repo through gh. Returns what to tell the user.
pub fn backup(accounts: &[Account], password: &str, file: Option<&Path>) -> Result<String> {
    if let Some(why) = joma_backup::weak(password) {
        bail!("password too weak: {why}");
    }
    if let Some(target) = file {
        let mut done = Vec::new();
        for a in accounts {
            let data = sealed(a, password)?;
            let path = if target.is_dir() || accounts.len() > 1 { target.join(file_name(a)) } else { target.to_path_buf() };
            if let Some(d) = path.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::write(&path, data).with_context(|| format!("cannot write {}", path.display()))?;
            if let (Some(state), Some(dir)) = (crate::sync::saved(&a.id), path.parent()) {
                std::fs::write(dir.join(sync_name(&a.uri)), joma_backup::seal(&state, password, &a.uri, "sync", now())?)?;
            }
            done.push(format!("{} → {}", a.label, path.display()));
        }
        return Ok(done.join("\n"));
    }
    let login = gh::login()?;
    if let Err(e) = gh::ensure_repo(&login) {
        // The app's token may not create repos, and reaches only the repos it was given.
        if gh::has_token() && !e.to_string().contains("is public") {
            bail!(
                "Joma's GitHub App cannot reach github.com/{login}/{repo}: create it (private) at https://github.com/new?name={repo}&visibility=private, then give the app that repo at {} ({e:#})",
                crate::signin::install_url(),
                repo = joma_backup::REPO
            );
        }
        return Err(e);
    }
    let mut index = gh::index(&login)?;
    let mut done = Vec::new();
    for a in accounts {
        let data = sealed(a, password)?;
        let name = file_name(a);
        gh::put(&login, &name, &data, &format!("backup of {}", a.label))?;
        if let Some(state) = crate::sync::saved(&a.id) {
            gh::put(&login, &sync_name(&a.uri), &joma_backup::seal(&state, password, &a.uri, "sync", now())?, &format!("synced folders of {}", a.label))?;
        }
        index.upsert(Entry { jami: a.uri.clone(), label: a.label.clone(), file: name, created: now(), size: data.len() as u64 });
        done.push(a.label.clone());
    }
    gh::put(&login, "index.json", serde_json::to_string_pretty(&index)?.as_bytes(), "update the index")?;
    Ok(format!("backed up to github.com/{login}/{}: {}", joma_backup::REPO, done.join(", ")))
}

/// Restore accounts: from `file` (a backup file or a folder of them), else from the
/// repo. `wanted`: labels or Jami ids (empty: all). `present`: Jami ids already on this
/// machine, skipped. Returns what to tell the user.
pub fn restore(wanted: &[String], password: &str, file: Option<&Path>, present: &[String]) -> Result<String> {
    let mut sources: Vec<(String, Vec<u8>)> = Vec::new();
    match file {
        Some(p) if p.is_dir() => {
            for e in std::fs::read_dir(p)?.flatten() {
                if e.path().extension().is_some_and(|x| x == EXT) && !e.file_name().to_string_lossy().contains(".sync.") {
                    sources.push((e.path().display().to_string(), std::fs::read(e.path())?));
                }
            }
        }
        Some(p) => sources.push((p.display().to_string(), std::fs::read(p).with_context(|| format!("cannot read {}", p.display()))?)),
        None => {
            let login = gh::login()?;
            if gh::repo(&login)?.is_none() {
                bail!("github.com/{login}/{} does not exist: nothing backed up yet", joma_backup::REPO);
            }
            let index = gh::index(&login)?;
            let picked = index.pick(wanted);
            if picked.is_empty() {
                bail!("no backed-up account matches (joma backup list)");
            }
            for e in picked {
                let (data, _) = gh::get(&login, &e.file)?.ok_or_else(|| anyhow!("{} is in the index but not in the repo", e.file))?;
                sources.push((e.label.clone(), data));
            }
        }
    }
    let mut done = Vec::new();
    for (name, data) in sources {
        let (h, _, _) = joma_backup::header(&data).with_context(|| name.clone())?;
        if file.is_some() && !wanted.is_empty() && !wanted.iter().any(|w| h.label.eq_ignore_ascii_case(w) || h.account.starts_with(w.as_str())) {
            continue;
        }
        if present.contains(&h.account) {
            done.push(format!("{}: already on this machine", h.label));
            continue;
        }
        let (_, archive) = match joma_backup::open(&data, password) {
            Ok(x) => x,
            Err(e) if e.downcast_ref::<WrongPassword>().is_some() => bail!("wrong password"),
            Err(e) => return Err(e),
        };
        let tmp = Scratch::new(&format!("{}.gz", h.account))?;
        std::fs::write(&tmp.0, &archive)?;
        joma_jami::import_archive(&tmp.0.to_string_lossy())?;
        // Its synced folders, to offer setting them up again.
        let state = match file {
            Some(p) => std::fs::read(if p.is_dir() { p.join(sync_name(&h.account)) } else { p.with_file_name(sync_name(&h.account)) }).ok(),
            None => gh::login().ok().and_then(|l| gh::get(&l, &sync_name(&h.account)).ok().flatten()).map(|(d, _)| d),
        };
        if let Some(d) = state {
            keep_sync_state(&h.account, &d, password);
        }
        // The daemon reads the archive while it loads the account.
        std::thread::sleep(std::time::Duration::from_secs(3));
        done.push(format!("{}: restored", h.label));
    }
    if done.is_empty() {
        bail!("no backup matches");
    }
    Ok(done.join("\n"))
}

/// `joma backup list`: what the repo holds.
pub fn list() -> Result<String> {
    let login = gh::login()?;
    if gh::repo(&login)?.is_none() {
        return Ok(format!("github.com/{login}/{}: not created yet", joma_backup::REPO));
    }
    let index = gh::index(&login)?;
    if index.accounts.is_empty() {
        return Ok("no accounts backed up yet".into());
    }
    Ok(index
        .accounts
        .iter()
        .map(|e| format!("{}  {}  {}  {} bytes", e.label, e.jami, date(e.created), e.size))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// `joma github status`.
pub fn status() -> String {
    match gh::login() {
        Err(e) => format!("GitHub: not connected ({e:#})"),
        Ok(login) => {
            let repo = match gh::repo(&login) {
                Ok(Some(true)) => format!("github.com/{login}/{} (private)", joma_backup::REPO),
                Ok(Some(false)) => format!("github.com/{login}/{} is PUBLIC: Joma will not write to it", joma_backup::REPO),
                Ok(None) => "no backup repo yet (joma backup creates it)".into(),
                Err(e) => format!("backup repo: {e:#}"),
            };
            let last = gh::index(&login)
                .ok()
                .and_then(|i| i.accounts.iter().map(|e| e.created).max())
                .map(|t| format!("\nlast backup: {}", date(t)))
                .unwrap_or_default();
            format!("GitHub: {login} (through gh)\n{repo}{last}")
        }
    }
}

fn date(unix: u64) -> String {
    let out = std::process::Command::new("date").args(["-d", &format!("@{unix}"), "+%Y-%m-%d %H:%M"]).output();
    out.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_else(|| unix.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_and_weak_refusal() {
        let a = Account { id: "x".into(), uri: "ab".repeat(20), label: "me".into() };
        assert_eq!(file_name(&a), format!("{}.jomabk", "ab".repeat(20)));
        let e = backup(&[a], "short", Some(Path::new("/nonexistent"))).unwrap_err();
        assert!(e.to_string().contains("too weak"));
    }

    #[test]
    fn restore_reports_wrong_password_and_present() {
        let dir = std::env::temp_dir().join(format!("joma-restore-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sample = concat!(env!("CARGO_MANIFEST_DIR"), "/../joma-backup/testdata/sample.jomabk");
        std::fs::copy(sample, dir.join("s.jomabk")).unwrap();
        let e = restore(&[], "not the password", Some(&dir), &[]).unwrap_err();
        assert_eq!(e.to_string(), "wrong password");
        let ok = restore(&[], "x", Some(&dir), &["0123456789".repeat(4)]).unwrap();
        assert_eq!(ok, "sample: already on this machine");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
