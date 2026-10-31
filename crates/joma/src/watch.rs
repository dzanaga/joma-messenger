//! Watching videos together in a conversation: each member who joins plays the video in
//! their own mpv (from the link: nothing of the video goes between peers); Joma keeps
//! play, pause and position in step, and keeps a queue.
//!
//! Every playback action (play, pause, seek, switch to another video, skip, stop) is a
//! proposal that passes when at least half of the participants agree, the proposer
//! included: two people both command at once; three or more vote. Adding to the queue
//! needs no vote. Only the proposer counts the votes and announces the result.
//!
//! Messages go to every other member as account messages, wrapped with the conversation
//! and the session id. Positions carry the sender's clock (machines are NTP-synced).

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const MIME: &str = "application/x-joma-watch+json";

/// Further apart than this, a player seeks; closer, it changes its speed a little (in
/// proportion, up to 10 %) until it is in step.
const SEEK_GAP: f64 = 2.0;
const NUDGE_GAP: f64 = 0.12;
/// A proposal nobody settles goes after this.
pub const PROPOSAL_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Item {
    pub url: String,
    pub title: String,
}

/// What a proposal would do.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "a", rename_all = "snake_case")]
pub enum Action {
    Play { pos: f64 },
    Pause { pos: f64 },
    Seek { pos: f64 },
    /// Play another video now.
    Switch { item: Item },
    /// The next one in the queue.
    Skip,
    /// End the session for everyone.
    Stop,
}

impl Action {
    pub fn label(&self) -> String {
        match self {
            Action::Play { .. } => "play".into(),
            Action::Pause { .. } => "pause".into(),
            Action::Seek { pos } => format!("go to {}", clock(*pos)),
            Action::Switch { item } => format!("switch to “{}”", item.title),
            Action::Skip => "skip to the next".into(),
            Action::Stop => "stop watching".into(),
        }
    }
}

pub fn clock(secs: f64) -> String {
    let s = secs.max(0.) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum WatchMsg {
    /// An invitation: watch `item` (the sender leads the session).
    Start { item: Item },
    Join,
    Leave,
    /// The full state, from the leader to whoever joined.
    Sync { participants: Vec<String>, current: Item, queue: Vec<Item>, playing: bool, pos: f64, at_ms: u64 },
    Propose { pid: u64, action: Action },
    Vote { pid: u64, yes: bool },
    /// A proposal passed (or needed no vote): everyone does it.
    Apply { pid: u64, action: Action, at_ms: u64 },
    /// The leader's position every few seconds, against drift.
    State { playing: bool, pos: f64, at_ms: u64 },
    Queue { item: Item },
}

/// A message with what it belongs to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wire {
    pub conv: String,
    pub sid: String,
    pub m: WatchMsg,
}

impl Wire {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("plain data")
    }

    pub fn decode(text: &str) -> Option<Wire> {
        serde_json::from_str(text).ok()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct Proposal {
    pub pid: u64,
    pub by: String,
    pub action: Action,
    pub yes: BTreeSet<String>,
    pub no: BTreeSet<String>,
    pub at: Instant,
}

/// A session as one member sees it.
pub struct Session {
    pub sid: String,
    pub leader: String,
    /// Who joined, us included once we did.
    pub participants: BTreeSet<String>,
    pub joined: bool,
    pub current: Item,
    pub queue: Vec<Item>,
    /// The agreed state: playing, position, and when (sender's clock).
    pub playing: bool,
    pub pos: f64,
    pub at_ms: u64,
    pub proposals: Vec<Proposal>,
    pub player: Option<Player>,
    pub last_state: Option<Instant>,
    pub last_sync: Option<Instant>,
}

impl Session {
    pub fn new(conv: &str, sid: &str, leader: &str, item: Item) -> Session {
        let _ = conv;
        Session {
            sid: sid.to_owned(),
            leader: leader.to_owned(),
            participants: BTreeSet::from([leader.to_owned()]),
            joined: false,
            current: item,
            queue: Vec::new(),
            playing: false,
            pos: 0.,
            at_ms: now_ms(),
            proposals: Vec::new(),
            player: None,
            last_state: None,
            last_sync: None,
        }
    }

    /// The agreed position now.
    pub fn position(&self) -> f64 {
        if self.playing {
            self.pos + now_ms().saturating_sub(self.at_ms) as f64 / 1000.
        } else {
            self.pos
        }
    }

    /// Enough yes votes (at least half of the participants, the proposer included).
    pub fn passes(&self, yes: usize) -> bool {
        yes * 2 >= self.participants.len().max(1)
    }

    /// Too many no votes for it ever to pass.
    pub fn fails(&self, no: usize) -> bool {
        no * 2 > self.participants.len()
    }

    /// Do an agreed action to the agreed state; true when the session ends.
    pub fn apply(&mut self, action: &Action, at_ms: u64) -> bool {
        match action {
            Action::Play { pos } => (self.playing, self.pos, self.at_ms) = (true, *pos, at_ms),
            Action::Pause { pos } => (self.playing, self.pos, self.at_ms) = (false, *pos, at_ms),
            Action::Seek { pos } => (self.pos, self.at_ms) = (*pos, at_ms),
            Action::Switch { item } => {
                self.queue.retain(|q| q.url != item.url);
                self.current = item.clone();
                (self.playing, self.pos, self.at_ms) = (true, 0., at_ms);
            }
            Action::Skip => {
                if !self.queue.is_empty() {
                    self.current = self.queue.remove(0);
                    (self.playing, self.pos, self.at_ms) = (true, 0., at_ms);
                }
            }
            Action::Stop => return true,
        }
        false
    }

    /// The player follows the agreed state (and the agreed video).
    pub fn sync_player(&mut self) {
        let (url, playing, pos, at) = (self.current.url.clone(), self.playing, self.pos, self.at_ms);
        if let Some(p) = self.player.as_mut() {
            if p.url != url {
                let _ = p.load(&url);
            }
            let _ = p.follow(playing, pos, at);
        }
    }

    /// What the user just did in mpv, as an action (None: nothing we follow).
    pub fn local_action(&self) -> Option<Action> {
        let p = self.player.as_ref()?;
        let (playing, pos) = p.now();
        if playing != self.playing {
            return Some(if playing { Action::Play { pos } } else { Action::Pause { pos } });
        }
        ((pos - self.position()).abs() > 1.5).then_some(Action::Seek { pos })
    }
}

/// What the player reports.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerEvent {
    /// The user paused, resumed or seeked.
    Changed,
    /// The video reached its end.
    Ended,
    Closed,
}

#[derive(Default)]
struct Shared {
    pos: f64,
    paused: bool,
    /// Pause changes we cause ourselves are not the user's: ignore until then.
    quiet_until: Option<Instant>,
    /// Seeks and loads we started that mpv has not finished (its `playback-restart`):
    /// their `seek` events are ours, however long streaming makes them.
    ours: u32,
    /// When the last one started (one that never finishes stops counting after a while).
    ours_at: Option<Instant>,
    /// How long our seeks take to play again (streams buffer): seeks aim that far ahead.
    seek_lag: f64,
}

/// mpv, driven over its JSON IPC socket.
pub struct Player {
    child: Child,
    sock: UnixStream,
    shared: Arc<Mutex<Shared>>,
    path: PathBuf,
    speed: f64,
    pub url: String,
}

impl Player {
    /// Start mpv on `url`, paused at the start; `events` runs on mpv's reader thread.
    pub fn start(url: &str, test_profile: bool, events: impl Fn(PlayerEvent) + Send + 'static) -> Result<Player> {
        let stamp = now_ms();
        let path = std::env::current_dir().unwrap_or_default().join(format!("mpv-watch-{}-{stamp}.sock", std::process::id()));
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
        let shared = Arc::new(Mutex::new(Shared { paused: true, seek_lag: 0.5, ..Default::default() }));
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
                        Some("eof-reached") if v["data"].as_bool() == Some(true) => events(PlayerEvent::Ended),
                        _ => {}
                    },
                    Some("seek") => {
                        if sh.lock().unwrap().ours == 0 && !quiet {
                            events(PlayerEvent::Changed);
                        }
                    }
                    Some("playback-restart") => {
                        let mut s = sh.lock().unwrap();
                        s.ours = s.ours.saturating_sub(1);
                        if let Some(t) = s.ours_at.take() {
                            let took = t.elapsed().as_secs_f64().min(8.);
                            s.seek_lag = s.seek_lag * 0.6 + took * 0.4;
                        }
                    }
                    Some("shutdown") => break,
                    _ => {}
                }
            }
            events(PlayerEvent::Closed);
        })?;
        let mut p = Player { child, sock, shared, path, speed: 1.0, url: url.to_owned() };
        p.command(serde_json::json!(["observe_property", 1, "time-pos"]))?;
        p.command(serde_json::json!(["observe_property", 2, "pause"]))?;
        p.command(serde_json::json!(["observe_property", 3, "eof-reached"]))?;
        Ok(p)
    }

    fn command(&mut self, args: serde_json::Value) -> Result<()> {
        let mut line = serde_json::to_string(&serde_json::json!({ "command": args }))?;
        line.push('\n');
        self.sock.write_all(line.as_bytes())?;
        Ok(())
    }

    fn quiet(&self, ms: u64) {
        self.shared.lock().unwrap().quiet_until = Some(Instant::now() + Duration::from_millis(ms));
    }

    /// Playing, and where.
    pub fn now(&self) -> (bool, f64) {
        let s = self.shared.lock().unwrap();
        (!s.paused, s.pos)
    }

    /// Another video, from its start, paused until the state says otherwise.
    pub fn load(&mut self, url: &str) -> Result<()> {
        self.quiet(1500);
        self.url = url.to_owned();
        {
            let mut s = self.shared.lock().unwrap();
            s.pos = 0.;
            s.ours += 1;
            s.ours_at = Some(Instant::now());
        }
        self.command(serde_json::json!(["loadfile", url, "replace"]))
    }

    /// Follow a state: pause or play, and seek or nudge the speed to the position
    /// (moved on by the time since `at_ms` when playing).
    pub fn follow(&mut self, playing: bool, pos: f64, at_ms: u64) -> Result<()> {
        let target = if playing { pos + now_ms().saturating_sub(at_ms) as f64 / 1000. } else { pos };
        let (now_playing, cur) = self.now();
        self.quiet(700);
        if now_playing != playing {
            self.command(serde_json::json!(["set_property", "pause", !playing]))?;
        }
        let gap = target - cur;
        if gap.abs() > SEEK_GAP || !playing && gap.abs() > NUDGE_GAP {
            let lag = {
                let mut s = self.shared.lock().unwrap();
                // One seek of ours at a time: while it runs, wait rather than pile up.
                if s.ours > 0 && s.ours_at.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
                    return Ok(());
                }
                s.ours = 1;
                s.ours_at = Some(Instant::now());
                s.seek_lag
            };
            // Playing on while the seek buffers: aim where the others will be by then.
            let aim = if playing { target + lag } else { target };
            self.command(serde_json::json!(["seek", aim, "absolute+exact"]))?;
            self.set_speed(1.0)?;
        } else if gap.abs() > NUDGE_GAP && playing {
            // Behind: faster; ahead: slower; in proportion, at most 10 %.
            self.set_speed(1.0 + (gap / 10.).clamp(-0.1, 0.1))?;
        } else {
            self.set_speed(1.0)?;
        }
        Ok(())
    }

    fn set_speed(&mut self, s: f64) -> Result<()> {
        if (self.speed - s).abs() > 0.005 {
            self.speed = s;
            self.command(serde_json::json!(["set_property", "speed", s]))?;
        }
        Ok(())
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

/// The text of an `@youtube` message: the query, then one result per two lines (title ·
/// channel · length, then the link), readable in any Jami client.
pub fn results_message(query: &str, videos: &[crate::media::Video]) -> String {
    let mut s = format!("@youtube {query}");
    for (i, v) in videos.iter().enumerate() {
        let len = crate::media::duration_label(v.duration);
        s.push_str(&format!("\n{}. {}{}{}\n{}", i + 1, v.title, if v.channel.is_empty() { String::new() } else { format!(" · {}", v.channel) }, if len.is_empty() { String::new() } else { format!(" · {len}") }, crate::media::watch_url(&v.id)));
    }
    s
}

/// An `@youtube` message's query and its results (title line, video id), if it is one.
pub fn parse_results(text: &str) -> Option<(String, Vec<(String, String)>)> {
    let mut lines = text.lines();
    let query = lines.next()?.strip_prefix("@youtube ")?.trim().to_owned();
    let mut out = Vec::new();
    let mut title = String::new();
    for l in lines {
        match crate::media::youtube_id(l) {
            Some(id) if l.trim().starts_with("http") => out.push((std::mem::take(&mut title), id)),
            _ => title = l.trim().splitn(2, ". ").nth(1).unwrap_or(l.trim()).to_owned(),
        }
    }
    (!out.is_empty()).then_some((query, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trips() {
        let w = Wire { conv: "c".into(), sid: "s".into(), m: WatchMsg::Propose { pid: 3, action: Action::Switch { item: Item { url: "u".into(), title: "t".into() } } } };
        assert_eq!(Wire::decode(&w.encode()), Some(w));
    }

    #[test]
    fn half_the_participants_decide() {
        let mut s = Session::new("c", "s", "a", Item::default());
        // Two: the proposer alone is half.
        s.participants.insert("b".into());
        assert!(s.passes(1));
        // Three: one more is needed; two noes sink it.
        s.participants.insert("c".into());
        assert!(!s.passes(1) && s.passes(2));
        assert!(!s.fails(1) && s.fails(2));
        // Four: two of four.
        s.participants.insert("d".into());
        assert!(s.passes(2) && !s.fails(2));
    }

    #[test]
    fn actions_move_the_state_and_the_queue() {
        let mut s = Session::new("c", "s", "a", Item { url: "1".into(), title: "one".into() });
        s.queue.push(Item { url: "2".into(), title: "two".into() });
        s.queue.push(Item { url: "3".into(), title: "three".into() });
        s.apply(&Action::Play { pos: 5. }, now_ms());
        assert!(s.playing && s.position() >= 5.);
        s.apply(&Action::Pause { pos: 7. }, now_ms());
        assert!(!s.playing && s.position() == 7.);
        s.apply(&Action::Skip, now_ms());
        assert_eq!(s.current.url, "2");
        s.apply(&Action::Switch { item: Item { url: "3".into(), title: "three".into() } }, now_ms());
        assert_eq!(s.current.url, "3");
        assert!(s.queue.is_empty(), "a switched-to video leaves the queue");
        assert!(s.apply(&Action::Stop, now_ms()));
    }

    #[test]
    fn youtube_results_messages() {
        let vids = vec![crate::media::Video { id: "dQw4w9WgXcQ".into(), title: "Never".into(), channel: "Rick".into(), duration: 213 }];
        let text = results_message("rick", &vids);
        assert!(text.starts_with("@youtube rick\n1. Never · Rick · 3:33\nhttps://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        let (q, list) = parse_results(&text).unwrap();
        assert_eq!(q, "rick");
        assert_eq!(list, vec![("Never · Rick · 3:33".to_owned(), "dQw4w9WgXcQ".to_owned())]);
        assert_eq!(parse_results("hello"), None);
    }
}
