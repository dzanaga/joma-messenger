//! Watching a video together: each side plays it in its own mpv (from the link, so
//! nothing of the video goes between peers); Joma keeps play, pause and position in
//! step. The messages ride the call (or account messages without a call) like the desk
//! handshake. Positions carry the sender's clock; both machines are assumed NTP-synced.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const MIME: &str = "application/x-joma-watch+json";

/// Further apart than this, the follower seeks; closer, it nudges its speed.
const SEEK_GAP: f64 = 0.8;
const NUDGE_GAP: f64 = 0.15;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum WatchMsg {
    /// Watch `url` together (`title` for the invitation).
    Start { url: String, title: String },
    /// The player's state at the sender's `at_ms` (unix milliseconds).
    State { playing: bool, pos: f64, at_ms: u64 },
    /// The sender joined (it sends its state from now on).
    Join,
    Stop,
}

impl WatchMsg {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("plain data")
    }

    pub fn decode(text: &str) -> Option<WatchMsg> {
        serde_json::from_str(text).ok()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// What the player reports: a change the user made, or the player closing.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerEvent {
    /// The user paused, resumed or seeked: send our state.
    Changed,
    Closed,
}

#[derive(Default)]
struct Shared {
    pos: f64,
    paused: bool,
    /// Changes we cause ourselves (from the peer) are not the user's: ignore until then.
    quiet_until: Option<Instant>,
}

/// mpv, driven over its JSON IPC socket.
pub struct Player {
    child: Child,
    sock: UnixStream,
    shared: Arc<Mutex<Shared>>,
    path: PathBuf,
    speed: f64,
}

impl Player {
    /// Start mpv on `url`, paused at the start; `events` runs on mpv's reader thread.
    pub fn start(url: &str, test_profile: bool, events: impl Fn(PlayerEvent) + Send + 'static) -> Result<Player> {
        let path = std::env::current_dir().unwrap_or_default().join(format!("mpv-watch-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut cmd = Command::new("mpv");
        cmd.arg(format!("--input-ipc-server={}", path.display()))
            .args(["--force-window=yes", "--keep-open=yes", "--pause", "--title=Joma · watching together", "--really-quiet"]);
        if test_profile {
            // Tests: no sound on the real speakers, no decoding on the display GPU.
            cmd.args(["--ao=null", "--hwdec=no"]);
        }
        let child = cmd.arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().context("mpv is not installed")?;
        let mut sock = None;
        for _ in 0..50 {
            if let Ok(s) = UnixStream::connect(&path) {
                sock = Some(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let sock = sock.context("mpv did not open its control socket")?;
        let shared = Arc::new(Mutex::new(Shared { paused: true, ..Default::default() }));
        let reader = sock.try_clone()?;
        let sh = shared.clone();
        std::thread::Builder::new().name("joma-mpv".into()).spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                let quiet = sh.lock().unwrap().quiet_until.is_some_and(|t| Instant::now() < t);
                match v["event"].as_str() {
                    Some("property-change") => match v["name"].as_str() {
                        Some("time-pos") => {
                            if let Some(p) = v["data"].as_f64() {
                                sh.lock().unwrap().pos = p;
                            }
                        }
                        Some("pause") => {
                            let p = v["data"].as_bool().unwrap_or(true);
                            let changed = {
                                let mut s = sh.lock().unwrap();
                                let c = s.paused != p;
                                s.paused = p;
                                c
                            };
                            if changed && !quiet {
                                events(PlayerEvent::Changed);
                            }
                        }
                        _ => {}
                    },
                    Some("seek") if !quiet => events(PlayerEvent::Changed),
                    Some("shutdown") => break,
                    _ => {}
                }
            }
            events(PlayerEvent::Closed);
        })?;
        let mut p = Player { child, sock, shared, path, speed: 1.0 };
        p.command(serde_json::json!(["observe_property", 1, "time-pos"]))?;
        p.command(serde_json::json!(["observe_property", 2, "pause"]))?;
        Ok(p)
    }

    fn command(&mut self, args: serde_json::Value) -> Result<()> {
        let mut line = serde_json::to_string(&serde_json::json!({ "command": args }))?;
        line.push('\n');
        self.sock.write_all(line.as_bytes())?;
        Ok(())
    }

    /// Our state now, for the peer.
    pub fn state(&self) -> WatchMsg {
        let s = self.shared.lock().unwrap();
        WatchMsg::State { playing: !s.paused, pos: s.pos, at_ms: now_ms() }
    }

    /// Follow the peer's state: pause or play, and seek or nudge the speed to its
    /// position (moved on by the time the message took when it plays).
    pub fn follow(&mut self, playing: bool, pos: f64, at_ms: u64) -> Result<()> {
        let target = if playing { pos + now_ms().saturating_sub(at_ms) as f64 / 1000. } else { pos };
        let (cur, paused) = {
            let s = self.shared.lock().unwrap();
            (s.pos, s.paused)
        };
        self.shared.lock().unwrap().quiet_until = Some(Instant::now() + Duration::from_millis(700));
        if paused == playing {
            self.command(serde_json::json!(["set_property", "pause", !playing]))?;
        }
        let gap = target - cur;
        if gap.abs() > SEEK_GAP || !playing && gap.abs() > NUDGE_GAP {
            self.command(serde_json::json!(["seek", target, "absolute+exact"]))?;
            self.set_speed(1.0)?;
        } else if gap.abs() > NUDGE_GAP {
            // Behind: a little faster; ahead: a little slower.
            self.set_speed(if gap > 0. { 1.05 } else { 0.95 })?;
        } else {
            self.set_speed(1.0)?;
        }
        Ok(())
    }

    fn set_speed(&mut self, s: f64) -> Result<()> {
        if (self.speed - s).abs() > f64::EPSILON {
            self.speed = s;
            self.command(serde_json::json!(["set_property", "speed", s]))?;
        }
        Ok(())
    }

    pub fn playing(&self) -> bool {
        !self.shared.lock().unwrap().paused
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.command(serde_json::json!(["quit"]));
        std::thread::sleep(Duration::from_millis(100));
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A watch-together session with one peer.
pub struct Session {
    pub peer: String,
    pub url: String,
    pub title: String,
    /// We started it: our state is the reference, sent every few seconds.
    pub lead: bool,
    /// None until we join (an invitation waiting for an answer).
    pub player: Option<Player>,
    pub last_sent: Option<Instant>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        for m in [
            WatchMsg::Start { url: "https://youtu.be/x".into(), title: "t".into() },
            WatchMsg::State { playing: true, pos: 12.5, at_ms: 1 },
            WatchMsg::Join,
            WatchMsg::Stop,
        ] {
            assert_eq!(WatchMsg::decode(&m.encode()), Some(m));
        }
    }
}
