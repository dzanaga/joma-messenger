//! The system keyring (Secret Service, through `secret-tool`): only for automatic backups
//! (D-24 question 4, user 2026-11-28: optional, off by default). Turning automatic
//! backups on stores the backup password and the GitHub App's refresh token; turning
//! them off deletes both. Secrets go to `secret-tool` through its stdin, never its
//! command line. A test profile never touches the real keyring: it keeps the entries in
//! private files under its own folder.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

pub const PASSWORD: &str = "backup-password";
pub const REFRESH: &str = "github-refresh";

fn attrs(kind: &str) -> Vec<String> {
    vec!["service".into(), "joma-messenger".into(), "kind".into(), kind.into()]
}

/// A test profile's stand-in for the keyring.
fn test_file(kind: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("JOMA_PROFILE").map(|p| std::path::PathBuf::from(p).join("keyring").join(kind))
}

pub fn available() -> bool {
    if test_file("x").is_some() {
        return true;
    }
    Command::new("secret-tool").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

pub fn store(kind: &str, secret: &str) -> Result<()> {
    if let Some(f) = test_file(kind) {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::create_dir_all(f.parent().expect("has a parent"))?;
        let _ = std::fs::remove_file(&f);
        std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&f)?.write_all(secret.as_bytes())?;
        return Ok(());
    }
    let mut child = Command::new("secret-tool")
        .arg("store")
        .arg(format!("--label=Joma: {kind}"))
        .args(attrs(kind))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("secret-tool is not installed (libsecret)")?;
    child.stdin.take().expect("piped").write_all(secret.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("the keyring refused it: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

pub fn lookup(kind: &str) -> Option<String> {
    if let Some(f) = test_file(kind) {
        return std::fs::read_to_string(f).ok().filter(|s| !s.is_empty());
    }
    let out = Command::new("secret-tool").arg("lookup").args(attrs(kind)).stderr(Stdio::null()).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    (out.status.success() && !s.is_empty()).then(|| s.trim_end_matches('\n').to_owned())
}

pub fn clear(kind: &str) {
    if let Some(f) = test_file(kind) {
        let _ = std::fs::remove_file(f);
        return;
    }
    let _ = Command::new("secret-tool").arg("clear").args(attrs(kind)).stdout(Stdio::null()).stderr(Stdio::null()).status();
}
