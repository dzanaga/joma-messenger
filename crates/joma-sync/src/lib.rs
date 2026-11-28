//! Joma's own Syncthing (D-23): started and stopped with Joma, with its own home under
//! Joma's data, so it never touches the user's Syncthing. It finds no peers by itself:
//! discovery, relays and NAT traversal are off and it listens on 127.0.0.1 only. Joma
//! tells it where each of your other devices is (the local end of a Jami tunnel to that
//! device's Syncthing) and which folders they share.
//!
//! Syncthing is driven through its REST API on 127.0.0.1 with an API key Joma chooses.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A running Syncthing.
pub struct Syncthing {
    pub home: PathBuf,
    /// Where it listens for other devices (127.0.0.1 only): the port Joma exposes as a
    /// Jami service.
    pub listen_port: u16,
    child: Option<Child>,
    /// Its REST API (a cheap handle, usable from any thread).
    pub api: Api,
}

/// Syncthing's REST API on 127.0.0.1.
#[derive(Debug, Clone)]
pub struct Api {
    pub gui_port: u16,
    api_key: String,
    /// The device ID of this Syncthing.
    pub id: String,
}

impl std::ops::Deref for Syncthing {
    type Target = Api;
    fn deref(&self) -> &Api {
        &self.api
    }
}

/// A folder in Syncthing's config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub label: String,
    pub path: String,
    /// `sendreceive`, `receiveonly`, …
    #[serde(default)]
    pub kind: String,
    /// The device IDs it is shared with (ours included).
    pub devices: Vec<String>,
}

/// Where a folder stands.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FolderStatus {
    pub id: String,
    pub label: String,
    pub path: String,
    /// `idle`, `scanning`, `syncing`, `error`…
    pub state: String,
    pub files: u64,
    pub bytes: u64,
    /// What this device still needs from the others.
    pub need_files: u64,
    pub need_bytes: u64,
    pub error: String,
    /// Syncthing's folder type (`receiveonly`: we only read it).
    pub kind: String,
    /// A folder we only read: files changed or deleted here (kept on our side).
    pub local_changed: u64,
}

/// One of the other devices Syncthing knows, and whether it is connected now.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DeviceStatus {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub address: String,
    pub in_bytes: u64,
    pub out_bytes: u64,
}

/// A free TCP port on 127.0.0.1.
pub fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// Whether the `syncthing` binary is installed.
pub fn available() -> bool {
    Command::new("syncthing").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn random_key() -> String {
    // Joma's own key; read only by Joma, valid for this run. /dev/urandom is enough here.
    let mut b = [0u8; 24];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Syncthing {
    /// Start Syncthing with its home in `home` (created and set up on the first start).
    /// `default_dir` is where folders shared by your other devices appear.
    pub fn start(home: &Path, default_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(home)?;
        stop_stale(home);
        if !home.join("config.xml").exists() {
            let out = Command::new("syncthing")
                .arg("generate")
                .arg(format!("--home={}", home.display()))
                // v2 has no --no-default-folder flag; the variable does it.
                .env("STNODEFAULTFOLDER", "1")
                .output()
                .context("syncthing is not installed")?;
            if !out.status.success() {
                bail!("syncthing generate failed: {}", String::from_utf8_lossy(&out.stderr).trim());
            }
        }
        let gui_port = free_port()?;
        let api_key = random_key();
        let log = std::fs::File::create(home.join("joma-syncthing.log"))?;
        let child = Command::new("syncthing")
            .arg("serve")
            .arg(format!("--home={}", home.display()))
            .arg(format!("--gui-address=127.0.0.1:{gui_port}"))
            .arg(format!("--gui-apikey={api_key}"))
            .args(["--no-browser", "--no-restart", "--no-upgrade"])
            .env("STNODEFAULTFOLDER", "1")
            .env("STNOUPGRADE", "1")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .context("cannot start syncthing")?;
        let mut st = Syncthing { home: home.to_path_buf(), listen_port: 0, child: Some(child), api: Api { gui_port, api_key, id: String::new() } };
        st.wait_ready(Duration::from_secs(30))?;
        st.api.id = st.get("/rest/system/status")?["myID"].as_str().unwrap_or_default().to_string();
        // The listen port is chosen once and kept: other devices' tunnels point at it.
        let opts = st.get("/rest/config/options")?;
        let kept = opts["listenAddresses"]
            .as_array()
            .and_then(|a| a.iter().filter_map(Value::as_str).find_map(|s| s.strip_prefix("tcp://127.0.0.1:")?.parse().ok()));
        st.listen_port = match kept {
            Some(p) => p,
            None => free_port()?,
        };
        std::fs::create_dir_all(default_dir)?;
        st.patch(
            "/rest/config/options",
            &json!({
                "listenAddresses": [format!("tcp://127.0.0.1:{}", st.listen_port)],
                "globalAnnounceEnabled": false,
                "localAnnounceEnabled": false,
                "relaysEnabled": false,
                "natEnabled": false,
                "urAccepted": -1,
                "crashReportingEnabled": false,
                "autoUpgradeIntervalH": 0,
                "startBrowser": false,
            }),
        )?;
        st.patch("/rest/config/defaults/folder", &json!({ "path": default_dir.to_string_lossy() }))?;
        Ok(st)
    }

    fn wait_ready(&mut self, within: Duration) -> Result<()> {
        let until = Instant::now() + within;
        loop {
            if self.get("/rest/system/ping").is_ok() {
                return Ok(());
            }
            if let Some(Ok(Some(status))) = self.child.as_mut().map(|c| c.try_wait()) {
                bail!("syncthing exited ({status}); see {}", self.home.join("joma-syncthing.log").display());
            }
            if Instant::now() > until {
                bail!("syncthing did not answer within {}s", within.as_secs());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Ask Syncthing to stop, and wait for it.
    pub fn stop(&mut self) {
        let _ = self.api.request("POST", "/rest/system/shutdown", None);
        if let Some(mut c) = self.child.take() {
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                if let Ok(Some(_)) = c.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

impl Api {
    // ---- REST -----------------------------------------------------------------

    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let mut s = TcpStream::connect(("127.0.0.1", self.gui_port))?;
        s.set_read_timeout(Some(Duration::from_secs(20)))?;
        write!(
            s,
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-API-Key: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.api_key,
            body.len()
        )?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw)?;
        let (code, text) = parse_response(&raw)?;
        if !(200..300).contains(&code) {
            bail!("syncthing {method} {path}: HTTP {code}: {}", text.trim());
        }
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text).or(Ok(Value::String(text)))
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        self.request("GET", path, None)
    }

    pub fn put(&self, path: &str, body: &Value) -> Result<Value> {
        self.request("PUT", path, Some(body))
    }

    pub fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        self.request("PATCH", path, Some(body))
    }

    // ---- Devices and folders --------------------------------------------------

    /// Add (or update) another device, reached at `address` (`tcp://127.0.0.1:<port>`,
    /// the local end of a Jami tunnel). Folders it shares are accepted by themselves.
    pub fn set_device(&self, id: &str, name: &str, addresses: &[String]) -> Result<()> {
        self.set_device_with(id, name, addresses, true)
    }

    /// A contact's device: its folders are not accepted by themselves (Joma joins the
    /// ones you accept).
    pub fn set_peer_device(&self, id: &str, name: &str, addresses: &[String]) -> Result<()> {
        self.set_device_with(id, name, addresses, false)
    }

    fn set_device_with(&self, id: &str, name: &str, addresses: &[String], auto: bool) -> Result<()> {
        self.put(
            &format!("/rest/config/devices/{id}"),
            &json!({ "deviceID": id, "name": name, "addresses": addresses, "autoAcceptFolders": auto, "introducer": false }),
        )?;
        Ok(())
    }

    pub fn remove_device(&self, id: &str) -> Result<()> {
        self.request("DELETE", &format!("/rest/config/devices/{id}"), None)?;
        Ok(())
    }

    /// The other devices in the config.
    pub fn device_ids(&self) -> Result<Vec<String>> {
        let v = self.get("/rest/config/devices")?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| d["deviceID"].as_str())
            .filter(|d| *d != self.id)
            .map(str::to_owned)
            .collect())
    }

    /// Share a folder (created if new) with `devices` besides us.
    pub fn set_folder(&self, id: &str, label: &str, path: &Path, devices: &[String]) -> Result<()> {
        let kind = self.folders()?.into_iter().find(|f| f.id == id).map(|f| f.kind).unwrap_or_else(|| "sendreceive".into());
        self.set_folder_as(id, label, path, devices, &kind)
    }

    /// The same with Syncthing's folder type: `sendreceive`, or `receiveonly` for a folder
    /// we may only read (what we change stays here). Old versions of changed and deleted
    /// files are kept 30 days in the folder's `.stversions`.
    pub fn set_folder_as(&self, id: &str, label: &str, path: &Path, devices: &[String], kind: &str) -> Result<()> {
        std::fs::create_dir_all(path)?;
        let mut all: Vec<Value> = vec![json!({ "deviceID": self.id })];
        all.extend(devices.iter().filter(|d| **d != self.id).map(|d| json!({ "deviceID": d })));
        self.put(
            &format!("/rest/config/folders/{id}"),
            &json!({
                "id": id, "label": label, "path": path.to_string_lossy(), "type": kind, "devices": all, "fsWatcherEnabled": true,
                "versioning": { "type": "staggered", "params": { "maxAge": "2592000", "cleanInterval": "3600" } }
            }),
        )?;
        Ok(())
    }

    /// Share every folder with one more device (your devices share everything).
    pub fn share_all_with(&self, device: &str) -> Result<()> {
        self.share_all_but(device, &[])
    }

    /// Every folder but `except` (switched off for that device).
    pub fn share_all_but(&self, device: &str, except: &[String]) -> Result<()> {
        for f in self.folders()? {
            if !except.contains(&f.id) && !f.devices.iter().any(|d| d == device) {
                let mut devices = f.devices.clone();
                devices.push(device.to_owned());
                self.set_folder(&f.id, &f.label, Path::new(&f.path), &devices)?;
            }
        }
        Ok(())
    }

    /// Add one device to a folder, or take it off.
    pub fn share_with(&self, folder: &str, device: &str, on: bool) -> Result<()> {
        let Some(f) = self.folders()?.into_iter().find(|f| f.id == folder) else { return Ok(()) };
        let has = f.devices.iter().any(|d| d == device);
        if has == on {
            return Ok(());
        }
        let devices: Vec<String> = if on { f.devices.iter().cloned().chain([device.to_owned()]).collect() } else { f.devices.iter().filter(|d| *d != device).cloned().collect() };
        self.set_folder_as(&f.id, &f.label, Path::new(&f.path), &devices, &f.kind)
    }

    /// A folder we only read: undo our local changes (the shared files come back).
    pub fn revert(&self, id: &str) -> Result<()> {
        self.request("POST", &format!("/rest/db/revert?folder={id}"), None)?;
        Ok(())
    }

    pub fn remove_folder(&self, id: &str) -> Result<()> {
        self.request("DELETE", &format!("/rest/config/folders/{id}"), None)?;
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let v = self.get("/rest/config/folders")?;
        Ok(v.as_array()
            .into_iter()
            .flatten()
            .map(|f| Folder {
                id: f["id"].as_str().unwrap_or_default().into(),
                label: f["label"].as_str().unwrap_or_default().into(),
                path: f["path"].as_str().unwrap_or_default().into(),
                kind: f["type"].as_str().unwrap_or("sendreceive").into(),
                devices: f["devices"].as_array().into_iter().flatten().filter_map(|d| d["deviceID"].as_str().map(str::to_owned)).collect(),
            })
            .collect())
    }

    pub fn folder_status(&self) -> Result<Vec<FolderStatus>> {
        let mut out = Vec::new();
        for f in self.folders()? {
            let s = self.get(&format!("/rest/db/status?folder={}", f.id)).unwrap_or(Value::Null);
            let n = |k: &str| s[k].as_u64().unwrap_or(0);
            out.push(FolderStatus {
                state: s["state"].as_str().unwrap_or("unknown").into(),
                files: n("globalFiles"),
                bytes: n("globalBytes"),
                need_files: n("needFiles"),
                need_bytes: n("needBytes"),
                error: s["error"].as_str().unwrap_or_default().into(),
                kind: f.kind,
                local_changed: n("receiveOnlyChangedFiles") + n("receiveOnlyChangedDeletes") + n("receiveOnlyChangedDirectories"),
                id: f.id,
                label: f.label,
                path: f.path,
            });
        }
        Ok(out)
    }

    pub fn device_status(&self) -> Result<Vec<DeviceStatus>> {
        let conns = self.get("/rest/system/connections")?;
        let names: std::collections::HashMap<String, String> = self
            .get("/rest/config/devices")?
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| Some((d["deviceID"].as_str()?.to_owned(), d["name"].as_str().unwrap_or_default().to_owned())))
            .collect();
        Ok(self
            .device_ids()?
            .into_iter()
            .map(|id| {
                let c = &conns["connections"][&id];
                DeviceStatus {
                    name: names.get(&id).cloned().unwrap_or_default(),
                    connected: c["connected"].as_bool().unwrap_or(false),
                    address: c["address"].as_str().unwrap_or_default().into(),
                    in_bytes: c["inBytesTotal"].as_u64().unwrap_or(0),
                    out_bytes: c["outBytesTotal"].as_u64().unwrap_or(0),
                    id,
                }
            })
            .collect())
    }
}

impl Drop for Syncthing {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Stop a Syncthing left running on this home by an earlier Joma that was killed rather
/// than quit (it would hold the ports and the database). Matched by the exact `--home`
/// argument, so the user's own Syncthing is never touched.
fn stale_pids(home: &Path) -> Vec<i32> {
    let want = format!("--home={}", home.display());
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .map(|c| {
                    let args: Vec<&[u8]> = c.split(|b| *b == 0).collect();
                    args.first().is_some_and(|a| a.ends_with(b"syncthing")) && args.iter().any(|a| *a == want.as_bytes())
                })
                .unwrap_or(false)
        })
        .collect()
}

fn stop_stale(home: &Path) {
    let pids = stale_pids(home);
    for pid in &pids {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    let until = Instant::now() + Duration::from_secs(5);
    while !stale_pids(home).is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Status code and body of a raw HTTP/1.1 response (chunked or not).
fn parse_response(raw: &[u8]) -> Result<(u16, String)> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| anyhow!("no HTTP response"))?;
    let code: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| anyhow!("bad HTTP status line"))?;
    let chunked = head.lines().any(|l| l.to_ascii_lowercase().starts_with("transfer-encoding:") && l.to_ascii_lowercase().contains("chunked"));
    if !chunked {
        return Ok((code, body.to_string()));
    }
    let mut out = String::new();
    let mut rest = body;
    while let Some((size, after)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || after.len() < n {
            break;
        }
        out.push_str(&after[..n]);
        rest = after[n..].trim_start_matches("\r\n");
    }
    Ok((code, out))
}

/// A folder id from a label: Syncthing ids are short and stable; Joma's start with `joma-`.
pub fn folder_id(label: &str) -> String {
    let slug: String = label.chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect::<String>().to_lowercase();
    format!("joma-{}-{}", if slug.is_empty() { "folder".into() } else { slug }, &random_key()[..6])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_chunked() {
        let (c, b) = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!((c, b.as_str()), (200, "{}"));
        let (c, b) = parse_response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n").unwrap();
        assert_eq!((c, b.as_str()), (200, "{\"a\":1}"));
        assert_eq!(parse_response(b"HTTP/1.1 403 Forbidden\r\n\r\nno").unwrap().0, 403);
        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn folder_ids() {
        let id = folder_id("My Notes!");
        assert!(id.starts_with("joma-mynotes-") && id.len() == "joma-mynotes-".len() + 6, "{id}");
        assert!(folder_id("???").starts_with("joma-folder-"));
    }

    /// A Syncthing left behind by a killed Joma is stopped by the next start.
    #[test]
    fn a_stale_instance_is_stopped() {
        if !available() {
            return;
        }
        let base = std::env::temp_dir().join(format!("joma-sync-stale-{}", std::process::id()));
        let home = base.join("home");
        let first = Syncthing::start(&home, &base.join("sync")).unwrap();
        // Never stopped, as if Joma had been killed.
        std::mem::forget(first);
        std::thread::sleep(Duration::from_millis(500));
        assert!(!stale_pids(&home).is_empty());
        let second = Syncthing::start(&home, &base.join("sync")).unwrap();
        assert!(second.get("/rest/system/ping").is_ok());
        drop(second);
        std::thread::sleep(Duration::from_millis(500));
        assert!(stale_pids(&home).is_empty(), "{:?}", stale_pids(&home));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Two real Syncthing instances, paired directly on 127.0.0.1 (as two Jami tunnels
    /// would pair them), sync a file. Skipped without the syncthing binary.
    #[test]
    fn two_instances_sync_a_file() {
        if !available() {
            eprintln!("syncthing not installed: skipped");
            return;
        }
        let base = std::env::temp_dir().join(format!("joma-sync-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let a = Syncthing::start(&base.join("a/home"), &base.join("a/sync")).unwrap();
        let b = Syncthing::start(&base.join("b/home"), &base.join("b/sync")).unwrap();
        a.set_device(&b.id, "b", &[format!("tcp://127.0.0.1:{}", b.listen_port)]).unwrap();
        b.set_device(&a.id, "a", &[format!("tcp://127.0.0.1:{}", a.listen_port)]).unwrap();
        let dir = base.join("a/notes");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "hi from a").unwrap();
        a.set_folder("joma-notes-test01", "notes", &dir, &[b.id.clone()]).unwrap();
        // b accepts the folder by itself, under its default folder path.
        let until = Instant::now() + Duration::from_secs(90);
        let found = loop {
            let hit = b.folders().unwrap().into_iter().find(|f| f.id == "joma-notes-test01");
            if let Some(f) = &hit {
                let p = Path::new(&f.path).join("hello.txt");
                if std::fs::read_to_string(&p).ok().as_deref() == Some("hi from a") {
                    break Some(f.path.clone());
                }
            }
            if Instant::now() > until {
                break None;
            }
            std::thread::sleep(Duration::from_millis(500));
        };
        let devices = a.device_status().unwrap();
        drop((a, b));
        let _ = std::fs::remove_dir_all(&base);
        let path = found.expect("the file did not reach b");
        assert!(path.starts_with(&*base.join("b/sync").to_string_lossy()), "{path}");
        assert!(devices.iter().any(|d| d.connected), "{devices:?}");
    }
}
