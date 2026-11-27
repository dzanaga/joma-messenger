//! Encrypted account backups (D-24), and the private GitHub repo that holds them.
//!
//! **File format** (`<jami id>.jomabk`, also what `joma backup --file` writes), the same
//! on every platform (the Android restore reads it too):
//!
//! ```text
//! JOMA-BACKUP 1\n
//! {"kdf":"argon2id","m_kib":65536,"t":3,"p":1,"salt":"<b64>","nonce":"<b64>",
//!  "cipher":"chacha20-poly1305","account":"<jami id>","label":"…","created":<unix s>}\n
//! <ciphertext of Jami's account archive, with its 16-byte tag>
//! ```
//!
//! The key is Argon2id(password, salt) with the header's parameters (32 bytes); the
//! cipher is ChaCha20-Poly1305 (IETF, 12-byte nonce) with the header line, without its
//! newline, as associated data. Salt (16 bytes) and nonce are random per file. The
//! archive is Jami's own `exportToFile` output: Joma accounts have no archive password,
//! so this encryption is the only protection.
//!
//! **The repo** `<login>/joma-backup` (private: Joma refuses a public one) holds one file
//! per account and `index.json` (labels, Jami ids, dates; no secrets).

use anyhow::{anyhow, bail, Context, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8] = b"JOMA-BACKUP 1\n";
pub const REPO: &str = "joma-backup";
pub const EXT: &str = "jomabk";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub kdf: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
    pub nonce: String,
    pub cipher: String,
    /// The account's Jami id (40 hex digits).
    pub account: String,
    pub label: String,
    /// Unix seconds.
    pub created: u64,
}

/// A wrong password (or a damaged file): the tag did not match.
#[derive(Debug)]
pub struct WrongPassword;

impl std::fmt::Display for WrongPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("wrong password (or a damaged backup)")
    }
}

impl std::error::Error for WrongPassword {}

fn random(n: usize) -> Result<Vec<u8>> {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(b)
}

fn key(password: &str, h: &Header) -> Result<[u8; 32]> {
    if h.kdf != "argon2id" || h.cipher != "chacha20-poly1305" {
        bail!("unknown backup format ({}, {})", h.kdf, h.cipher);
    }
    let salt = B64.decode(&h.salt)?;
    let params = Params::new(h.m_kib, h.t, h.p, Some(32)).map_err(|e| anyhow!("{e}"))?;
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), &salt, &mut out)
        .map_err(|e| anyhow!("{e}"))?;
    Ok(out)
}

/// Why a password is too weak to protect an account key, or None. Deliberately small:
/// long enough, and not one character repeated (the user wants no word lists).
pub fn weak(password: &str) -> Option<&'static str> {
    let n = password.chars().count();
    if n < 10 {
        return Some("use at least 10 characters");
    }
    let mut c = password.chars();
    let first = c.next();
    if c.all(|x| Some(x) == first) {
        return Some("not one character repeated");
    }
    None
}

/// Encrypt an account archive.
pub fn seal(archive: &[u8], password: &str, account: &str, label: &str, created: u64) -> Result<Vec<u8>> {
    seal_with(archive, password, account, label, created, 65536, 3)
}

fn seal_with(archive: &[u8], password: &str, account: &str, label: &str, created: u64, m_kib: u32, t: u32) -> Result<Vec<u8>> {
    let h = Header {
        kdf: "argon2id".into(),
        m_kib,
        t,
        p: 1,
        salt: B64.encode(random(16)?),
        nonce: B64.encode(random(12)?),
        cipher: "chacha20-poly1305".into(),
        account: account.into(),
        label: label.into(),
        created,
    };
    let line = serde_json::to_string(&h)?;
    let k = key(password, &h)?;
    let nonce = B64.decode(&h.nonce)?;
    let ct = ChaCha20Poly1305::new((&k).into())
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: archive, aad: line.as_bytes() })
        .map_err(|_| anyhow!("encryption failed"))?;
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(line.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(&ct);
    Ok(out)
}

/// The header alone (no password needed): which account, when.
pub fn header(data: &[u8]) -> Result<(Header, &[u8], &[u8])> {
    let rest = data.strip_prefix(MAGIC).ok_or_else(|| anyhow!("not a Joma backup"))?;
    let nl = rest.iter().position(|b| *b == b'\n').ok_or_else(|| anyhow!("damaged backup header"))?;
    let (line, ct) = (&rest[..nl], &rest[nl + 1..]);
    let h: Header = serde_json::from_slice(line).context("damaged backup header")?;
    Ok((h, line, ct))
}

/// Decrypt a backup: its header and Jami's archive. A wrong password is a
/// [`WrongPassword`] error.
pub fn open(data: &[u8], password: &str) -> Result<(Header, Vec<u8>)> {
    let (h, line, ct) = header(data)?;
    let k = key(password, &h)?;
    let nonce = B64.decode(&h.nonce)?;
    if nonce.len() != 12 {
        bail!("damaged backup header");
    }
    let plain = ChaCha20Poly1305::new((&k).into())
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: ct, aad: line })
        .map_err(|_| WrongPassword)?;
    Ok((h, plain))
}

// ---- index.json ---------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub joma: u32,
    pub accounts: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub jami: String,
    pub label: String,
    pub file: String,
    pub created: u64,
    pub size: u64,
}

impl Index {
    pub fn upsert(&mut self, e: Entry) {
        self.joma = 1;
        self.accounts.retain(|a| a.jami != e.jami);
        self.accounts.push(e);
        self.accounts.sort_by(|a, b| a.label.cmp(&b.label));
    }

    /// Accounts matching a label, a Jami id (or its start); all when `wanted` is empty.
    pub fn pick(&self, wanted: &[String]) -> Vec<&Entry> {
        if wanted.is_empty() {
            return self.accounts.iter().collect();
        }
        self.accounts
            .iter()
            .filter(|e| wanted.iter().any(|w| e.label.eq_ignore_ascii_case(w) || (w.len() >= 8 && e.jami.starts_with(w.as_str()))))
            .collect()
    }
}

// ---- GitHub through gh ------------------------------------------------------------

/// GitHub reached through the user's `gh` login (step 1 of D-24; the GitHub App comes
/// next). Joma never reads gh's token: it runs `gh api` each time.
pub mod gh {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn api(args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
        let mut c = Command::new("gh");
        c.arg("api").args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = c.spawn().context("gh is not installed")?;
        if let Some(i) = input {
            child.stdin.take().expect("piped").write_all(i)?;
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("gh api {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(out.stdout)
    }

    /// The login gh is signed in as, or why not.
    pub fn login() -> Result<String> {
        let v: serde_json::Value = serde_json::from_slice(&api(&["user"], None).context("gh is not logged in (gh auth login)")?)?;
        v["login"].as_str().map(str::to_owned).ok_or_else(|| anyhow!("gh gave no login"))
    }

    /// Whether the backup repo exists, and whether it is private.
    pub fn repo(login: &str) -> Result<Option<bool>> {
        match api(&[&format!("repos/{login}/{REPO}")], None) {
            Ok(b) => {
                let v: serde_json::Value = serde_json::from_slice(&b)?;
                Ok(Some(v["private"].as_bool().unwrap_or(false)))
            }
            Err(e) if e.to_string().contains("Not Found") => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Make sure the private backup repo exists; refuse a public one.
    pub fn ensure_repo(login: &str) -> Result<()> {
        match repo(login)? {
            Some(true) => Ok(()),
            Some(false) => bail!("github.com/{login}/{REPO} is public: Joma only writes backups into a private repo"),
            None => {
                let body = serde_json::json!({
                    "name": REPO, "private": true, "auto_init": true,
                    "description": "Joma account backups (encrypted)"
                });
                api(&["-X", "POST", "user/repos", "--input", "-"], Some(body.to_string().as_bytes()))?;
                Ok(())
            }
        }
    }

    /// A file's bytes and its blob sha, or None when it does not exist.
    pub fn get(login: &str, path: &str) -> Result<Option<(Vec<u8>, String)>> {
        let meta = match api(&[&format!("repos/{login}/{REPO}/contents/{path}")], None) {
            Ok(b) => b,
            Err(e) if e.to_string().contains("Not Found") => return Ok(None),
            Err(e) => return Err(e),
        };
        let v: serde_json::Value = serde_json::from_slice(&meta)?;
        let sha = v["sha"].as_str().unwrap_or_default().to_owned();
        let raw = api(&["-H", "Accept: application/vnd.github.raw", &format!("repos/{login}/{REPO}/contents/{path}")], None)?;
        Ok(Some((raw, sha)))
    }

    /// Write a file (a new commit).
    pub fn put(login: &str, path: &str, bytes: &[u8], message: &str) -> Result<()> {
        let sha = get(login, path)?.map(|(_, s)| s);
        let mut body = serde_json::json!({ "message": message, "content": B64.encode(bytes) });
        if let Some(s) = sha {
            body["sha"] = s.into();
        }
        api(&["-X", "PUT", &format!("repos/{login}/{REPO}/contents/{path}"), "--input", "-"], Some(body.to_string().as_bytes()))?;
        Ok(())
    }

    pub fn index(login: &str) -> Result<Index> {
        Ok(match get(login, "index.json")? {
            Some((b, _)) => serde_json::from_slice(&b).context("index.json is damaged")?,
            None => Index::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens() {
        let archive = b"jami archive bytes".to_vec();
        // Small parameters keep the test fast; the format carries them.
        let data = seal_with(&archive, "correct horse battery", "a".repeat(40).as_str(), "alice", 1_795_000_000, 1024, 1).unwrap();
        assert!(data.starts_with(MAGIC));
        let (h, plain) = open(&data, "correct horse battery").unwrap();
        assert_eq!(plain, archive);
        assert_eq!((h.label.as_str(), h.created, h.m_kib), ("alice", 1_795_000_000, 1024));
        let err = open(&data, "wrong password!").unwrap_err();
        assert!(err.downcast_ref::<WrongPassword>().is_some());
        // The header is authenticated too.
        let tampered: Vec<u8> = String::from_utf8_lossy(&data).replacen("alice", "mallo", 1).into_bytes();
        assert!(open(&tampered, "correct horse battery").is_err());
        assert!(open(b"nope", "x").is_err());
    }

    #[test]
    fn default_parameters() {
        let data = seal(b"x", "a good long password", "b", "bob", 1).unwrap();
        let (h, _, _) = header(&data).unwrap();
        assert_eq!((h.m_kib, h.t, h.p, h.kdf.as_str(), h.cipher.as_str()), (65536, 3, 1, "argon2id", "chacha20-poly1305"));
        assert_eq!(B64.decode(&h.salt).unwrap().len(), 16);
    }

    /// A fixed sample (password `joma sample password`, archive `sample archive`), so
    /// the format cannot change by accident and the Android restore can test against the
    /// same file. `JOMA_WRITE_SAMPLE=1` writes it anew.
    #[test]
    fn sample_file_opens() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/sample.jomabk");
        if std::env::var_os("JOMA_WRITE_SAMPLE").is_some() {
            let data = seal(b"sample archive", "joma sample password", &"0123456789".repeat(4), "sample", 1_795_000_000).unwrap();
            std::fs::write(path, data).unwrap();
        }
        let data = std::fs::read(path).unwrap();
        let (h, plain) = open(&data, "joma sample password").unwrap();
        assert_eq!(plain, b"sample archive");
        assert_eq!(h.label, "sample");
    }

    #[test]
    fn weak_passwords() {
        assert!(weak("short").is_some());
        assert!(weak("aaaaaaaaaaaa").is_some());
        assert!(weak("a normal password").is_none());
    }

    #[test]
    fn index_upsert_and_pick() {
        let mut i = Index::default();
        let e = |j: &str, l: &str| Entry { jami: j.into(), label: l.into(), file: format!("{j}.jomabk"), created: 1, size: 2 };
        i.upsert(e("aaaaaaaa11", "work"));
        i.upsert(e("bbbbbbbb22", "home"));
        i.upsert(e("aaaaaaaa11", "work2"));
        assert_eq!(i.accounts.len(), 2);
        assert_eq!(i.pick(&[]).len(), 2);
        assert_eq!(i.pick(&["HOME".into()])[0].jami, "bbbbbbbb22");
        assert_eq!(i.pick(&["aaaaaaaa".into()])[0].label, "work2");
        assert!(i.pick(&["aaa".into()]).is_empty());
    }
}
