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

// ---- GitHub: through gh, or the Joma GitHub App's token ----------------------------

/// GitHub's REST API, reached through the user's `gh` login or, once the user signed in
/// with the Joma GitHub App ([`device`]), through that app's token. Joma never reads gh's
/// token: it runs `gh api` each time. The app's token stays in memory ([`gh::set_token`]).
pub mod gh {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::Mutex;

    static TOKEN: Mutex<Option<String>> = Mutex::new(None);

    /// Use the GitHub App's user token from now on (`None`: back to gh).
    pub fn set_token(token: Option<String>) {
        *TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = token;
    }

    pub fn has_token() -> bool {
        TOKEN.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    fn token() -> Option<String> {
        TOKEN.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// gh is installed and logged in.
    pub fn cli_ready() -> bool {
        Command::new("gh").args(["auth", "status"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    }

    /// GitHub's API; a test profile may point it at a local stand-in (`JOMA_GITHUB_API`).
    pub fn api_base() -> String {
        match (std::env::var_os("JOMA_PROFILE"), std::env::var("JOMA_GITHUB_API")) {
            (Some(_), Ok(url)) if url.starts_with("http://127.0.0.1:") => url.trim_end_matches('/').to_owned(),
            _ => "https://api.github.com".into(),
        }
    }

    /// `gh api`'s arguments as used here: `[-X METHOD] [-H header]… path [--input -]`.
    struct Call {
        method: String,
        headers: Vec<String>,
        path: String,
    }

    fn parse(args: &[&str]) -> Call {
        let mut c = Call { method: String::new(), headers: Vec::new(), path: String::new() };
        let mut i = 0;
        while i < args.len() {
            match args[i] {
                "-X" => {
                    c.method = args.get(i + 1).unwrap_or(&"GET").to_string();
                    i += 1;
                }
                "-H" => {
                    c.headers.push(args.get(i + 1).unwrap_or(&"").to_string());
                    i += 1;
                }
                "--input" => i += 1,
                a => c.path = a.trim_start_matches('/').to_owned(),
            }
            i += 1;
        }
        c
    }

    /// The same call with the app's token, through curl. The token goes to curl in a
    /// private file (never on its command line).
    fn with_token(token: &str, args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
        let c = parse(args);
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let hdr = dir.join(format!("joma-gh-{}-{}", std::process::id(), B64.encode(random(6)?).replace(['/', '+', '='], "x")));
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&hdr)?;
            writeln!(f, "Authorization: Bearer {token}")?;
        }
        let url = format!("{}/{}", api_base(), c.path);
        let mut cmd = Command::new("curl");
        cmd.args(["-sS", "-L", "--max-time", "60", "-H", "User-Agent: joma", "-H", "X-GitHub-Api-Version: 2022-11-28", "-w", "\n%{http_code}"]);
        cmd.arg("-H").arg(format!("@{}", hdr.display()));
        if !c.headers.iter().any(|h| h.to_ascii_lowercase().starts_with("accept:")) {
            cmd.args(["-H", "Accept: application/vnd.github+json"]);
        }
        for h in &c.headers {
            cmd.args(["-H", h]);
        }
        if !c.method.is_empty() {
            cmd.args(["-X", &c.method]);
        }
        if input.is_some() {
            cmd.args(["-H", "Content-Type: application/json", "--data-binary", "@-"]);
        }
        cmd.arg(&url).stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
        let run = (|| -> Result<std::process::Output> {
            let mut child = cmd.spawn().context("curl is not installed")?;
            if let Some(i) = input {
                child.stdin.take().expect("piped").write_all(i)?;
            }
            Ok(child.wait_with_output()?)
        })();
        let _ = std::fs::remove_file(&hdr);
        let out = run?;
        if !out.status.success() {
            bail!("cannot reach GitHub: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        let mut body = out.stdout;
        let pos = body.iter().rposition(|b| *b == b'\n').unwrap_or(0);
        let code: u16 = String::from_utf8_lossy(&body[pos + 1..]).trim().parse().unwrap_or(0);
        body.truncate(pos);
        match code {
            200..=299 => Ok(body),
            401 => bail!("GitHub access was removed or expired: sign in with GitHub again"),
            404 => bail!("{}: Not Found (HTTP 404)", c.path),
            c2 => {
                let msg = serde_json::from_slice::<serde_json::Value>(&body).ok().and_then(|v| v["message"].as_str().map(str::to_owned)).unwrap_or_default();
                bail!("GitHub answered {c2} for {}: {msg}", c.path)
            }
        }
    }

    /// One GitHub API call with `gh api`'s arguments: through the app's token when the
    /// user signed in with it, else through gh.
    pub fn api(args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>> {
        if let Some(t) = token() {
            return with_token(&t, args, input);
        }
        let mut c = Command::new("gh");
        c.arg("api").args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = c.spawn().context("not signed in to GitHub (no gh, no GitHub sign-in)")?;
        if let Some(i) = input {
            child.stdin.take().expect("piped").write_all(i)?;
        }
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("gh api {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(out.stdout)
    }

    /// Calls failed because nobody is signed in: offer "sign in with GitHub".
    pub fn not_signed_in(e: &str) -> bool {
        e.contains("not logged in") || e.contains("not signed in") || e.contains("access was removed") || e.contains("auth login")
    }

    /// The login gh is signed in as, or why not.
    pub fn login() -> Result<String> {
        let v: serde_json::Value = serde_json::from_slice(&api(&["user"], None).context("not signed in to GitHub (sign in with GitHub, or gh auth login)")?)?;
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

// ---- Signing in with the Joma GitHub App (device flow) ------------------------------

/// The device flow of the Joma GitHub App: no client secret, nothing secret in the
/// binary. Joma shows the code, the user types it at github.com/login/device.
pub mod device {
    use super::*;

    /// Where the device flow lives; a test profile may point it at a local stand-in
    /// (`JOMA_GITHUB_WEB`).
    pub fn web_base() -> String {
        match (std::env::var_os("JOMA_PROFILE"), std::env::var("JOMA_GITHUB_WEB")) {
            (Some(_), Ok(url)) if url.starts_with("http://127.0.0.1:") => url.trim_end_matches('/').to_owned(),
            _ => "https://github.com".into(),
        }
    }

    #[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
    pub struct Code {
        pub device_code: String,
        pub user_code: String,
        pub verification_uri: String,
        #[serde(default = "five")]
        pub interval: u64,
        #[serde(default)]
        pub expires_in: u64,
    }

    fn five() -> u64 {
        5
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum Poll {
        Pending,
        /// Wait this many seconds more between polls.
        SlowDown(u64),
        Token(Token),
        /// Denied, expired, or the app has the device flow off: the message says.
        Failed(String),
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub struct Token {
        pub access: String,
        /// Empty when the app's tokens do not expire.
        #[serde(default)]
        pub refresh: String,
        /// Unix seconds; 0: does not expire.
        #[serde(default)]
        pub expires: u64,
    }

    fn post(path: &str, form: &[(&str, &str)]) -> Result<serde_json::Value> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        // The form goes through stdin: a refresh token never shows on a command line.
        let body: Vec<String> = form.iter().map(|(k, v)| format!("{k}={}", v.replace('%', "%25").replace('&', "%26").replace('+', "%2B").replace('=', "%3D"))).collect();
        let mut child = Command::new("curl")
            .args(["-sS", "--max-time", "30", "-H", "Accept: application/json", "-H", "User-Agent: joma", "-X", "POST", "--data-binary", "@-"])
            .arg(format!("{}/{path}", web_base()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("curl is not installed")?;
        child.stdin.take().expect("piped").write_all(body.join("&").as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("cannot reach GitHub: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        serde_json::from_slice(&out.stdout).context("GitHub's answer is not JSON")
    }

    fn explain(v: &serde_json::Value) -> String {
        match v["error"].as_str().unwrap_or("") {
            "device_flow_disabled" => "the Joma GitHub App has the device flow off (its settings: Enable Device Flow)".into(),
            "expired_token" => "the code expired: sign in again".into(),
            "access_denied" => "you cancelled the sign-in on GitHub".into(),
            "incorrect_client_credentials" => "GitHub does not know Joma's app".into(),
            e => format!("GitHub: {} {}", e, v["error_description"].as_str().unwrap_or("")).trim().to_owned(),
        }
    }

    /// Ask GitHub for a code to show the user.
    pub fn start(client_id: &str) -> Result<Code> {
        let v = post("login/device/code", &[("client_id", client_id)])?;
        if v.get("error").is_some() {
            bail!("{}", explain(&v));
        }
        serde_json::from_value(v).context("GitHub gave no code")
    }

    fn token_of(v: &serde_json::Value, now: u64) -> Option<Token> {
        let access = v["access_token"].as_str()?.to_owned();
        let refresh = v["refresh_token"].as_str().unwrap_or("").to_owned();
        let expires = v["expires_in"].as_u64().map_or(0, |s| now + s);
        Some(Token { access, refresh, expires })
    }

    fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }

    /// Ask once whether the user typed the code yet.
    pub fn poll(client_id: &str, code: &Code) -> Result<Poll> {
        let v = post(
            "login/oauth/access_token",
            &[("client_id", client_id), ("device_code", &code.device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")],
        )?;
        Ok(answer(&v, now()))
    }

    pub(crate) fn answer(v: &serde_json::Value, now: u64) -> Poll {
        if let Some(t) = token_of(v, now) {
            return Poll::Token(t);
        }
        match v["error"].as_str().unwrap_or("") {
            "authorization_pending" => Poll::Pending,
            "slow_down" => Poll::SlowDown(v["interval"].as_u64().unwrap_or(10)),
            _ => Poll::Failed(explain(v)),
        }
    }

    /// A new token from a refresh token (no client secret needed with the device flow).
    pub fn refresh(client_id: &str, refresh: &str) -> Result<Token> {
        let v = post("login/oauth/access_token", &[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh)])?;
        token_of(&v, now()).ok_or_else(|| anyhow!("{}", explain(&v)))
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

    #[test]
    fn device_answers() {
        use device::{answer, Poll};
        assert_eq!(answer(&serde_json::json!({"error":"authorization_pending"}), 0), Poll::Pending);
        assert_eq!(answer(&serde_json::json!({"error":"slow_down","interval":10}), 0), Poll::SlowDown(10));
        assert!(matches!(answer(&serde_json::json!({"error":"device_flow_disabled"}), 0), Poll::Failed(m) if m.contains("Enable Device Flow")));
        let t = answer(&serde_json::json!({"access_token":"ghu_x","refresh_token":"ghr_y","expires_in":28800}), 100);
        assert_eq!(t, Poll::Token(device::Token { access: "ghu_x".into(), refresh: "ghr_y".into(), expires: 28900 }));
    }
}
