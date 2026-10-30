//! Presence between Joma users. Jami's own presence only says a device announced itself
//! on the network in the last ten minutes (phones with push and a Jami app in the
//! background always do), so Joma tells its contacts whether its user is active or away
//! with account messages of its own MIME type, which other clients ignore. Invisible
//! sends nothing, and tells contacts to forget what they had.
//!
//! Activity comes from the compositor (`ext-idle-notify-v1`): away after [`IDLE`]
//! without input. Without that protocol Joma counts as active while it runs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use joma_desk::signal::now;
use serde::{Deserialize, Serialize};

pub const MIME: &str = "application/x-joma-presence+json";

/// Away after this long without input.
pub const IDLE: Duration = Duration::from_secs(5 * 60);
/// Each online contact hears from us this often; a state not refreshed for [`EXPIRE`]
/// is dropped, and so is a message that old (delivered late through the DHT).
pub const REFRESH: Duration = Duration::from_secs(4 * 60);
pub const EXPIRE: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum State {
    Active,
    /// Without input since (unix seconds).
    Away { since: u64 },
    /// Invisible or quitting: forget what you knew.
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wire {
    #[serde(flatten)]
    pub state: State,
    /// Unix seconds when sent.
    pub ts: u64,
    /// The sending device, on messages between our own devices.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dev: String,
}

impl Wire {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("plain data")
    }

    pub fn decode(text: &str) -> Option<Wire> {
        serde_json::from_str(text).ok()
    }
}

/// What we know of a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    pub state: State,
    pub received: Instant,
}

#[derive(Default)]
pub struct Presence {
    /// Peers' Joma state, by bare URI.
    pub peers: HashMap<String, Peer>,
    /// The last time a peer was active as far as we saw (unix seconds): kept after their
    /// state expires, for "seen".
    pub seen: HashMap<String, u64>,
    /// When we last told each peer.
    sent: HashMap<String, Instant>,
    /// What we last told everyone.
    told: Option<State>,
    /// Unix seconds since we are idle; 0 while active.
    idle_since: Arc<AtomicU64>,
}

impl Presence {
    /// Start watching the compositor's idle notifications.
    pub fn start() -> Presence {
        let p = Presence::default();
        let idle = p.idle_since.clone();
        std::thread::Builder::new()
            .name("joma-idle".into())
            .spawn(move || {
                if let Err(e) = idle::watch(IDLE, idle) {
                    tracing::info!("no idle notifications ({e}); counting as active");
                }
            })
            .ok();
        p
    }

    pub fn mine(&self, invisible: bool) -> State {
        match self.idle_since.load(Ordering::Relaxed) {
            _ if invisible => State::Hidden,
            0 => State::Active,
            since => State::Away { since },
        }
    }

    /// Who to tell what, now: everyone in `contacts` when our state changed, else the
    /// `online` ones not told within [`REFRESH`]. Hidden is said once, on the change.
    pub fn due(&mut self, state: State, contacts: &[String], online: impl Fn(&str) -> bool, at: Instant) -> Vec<(String, Wire)> {
        let changed = self.told != Some(state);
        if changed {
            self.told = Some(state);
            self.sent.clear();
        }
        if state == State::Hidden && !changed {
            return Vec::new();
        }
        let wire = Wire { state, ts: now(), dev: String::new() };
        let mut out = Vec::new();
        for c in contacts {
            let stale = self.sent.get(c).is_none_or(|t| at.duration_since(*t) >= REFRESH);
            if (changed && state == State::Hidden) || (stale && online(c)) {
                self.sent.insert(c.clone(), at);
                out.push((c.clone(), wire.clone()));
            }
        }
        out
    }

    /// A peer's message. True when we should answer at once: we knew nothing of them, so
    /// they may have just started and know nothing of us.
    pub fn received(&mut self, peer: &str, wire: Wire, at: Instant) -> bool {
        if now().saturating_sub(wire.ts) > EXPIRE.as_secs() {
            return false;
        }
        let known = self.peers.contains_key(peer);
        match wire.state {
            State::Hidden => {
                // Quitting or turning invisible: active until now.
                if self.state_of(peer) == Some(State::Active) {
                    self.seen.insert(peer.to_owned(), wire.ts);
                }
                self.peers.remove(peer);
                return false;
            }
            State::Active => {
                self.seen.insert(peer.to_owned(), wire.ts);
            }
            State::Away { since } => {
                let s = self.seen.entry(peer.to_owned()).or_default();
                *s = (*s).max(since);
            }
        }
        self.peers.insert(peer.to_owned(), Peer { state: wire.state, received: at });
        if !known {
            self.sent.remove(peer);
        }
        !known
    }

    /// Jami says the peer has no device online: no Joma either.
    pub fn gone(&mut self, peer: &str) {
        self.peers.remove(peer);
    }

    /// Drop states not refreshed in time. True when something went.
    pub fn expire(&mut self, at: Instant) -> bool {
        let before = self.peers.len();
        self.peers.retain(|_, p| at.duration_since(p.received) < EXPIRE);
        before != self.peers.len()
    }

    pub fn state_of(&self, peer: &str) -> Option<State> {
        self.peers.get(peer).map(|p| p.state)
    }
}

/// The compositor's idle notifications, on a thread of their own.
mod idle {
    use super::*;
    use wayland_client::protocol::{wl_registry, wl_seat};
    use wayland_client::{Connection, Dispatch, QueueHandle};
    use wayland_protocols::ext::idle_notify::v1::client::{ext_idle_notification_v1 as note, ext_idle_notifier_v1 as notifier};

    struct S {
        seat: Option<wl_seat::WlSeat>,
        notifier: Option<notifier::ExtIdleNotifierV1>,
        idle: Arc<AtomicU64>,
        timeout: Duration,
    }

    pub fn watch(timeout: Duration, idle: Arc<AtomicU64>) -> anyhow::Result<()> {
        let conn = Connection::connect_to_env()?;
        let mut queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut s = S { seat: None, notifier: None, idle, timeout };
        queue.roundtrip(&mut s)?;
        let (Some(seat), Some(n)) = (&s.seat, &s.notifier) else {
            anyhow::bail!("the compositor has no ext-idle-notify-v1");
        };
        n.get_idle_notification(timeout.as_millis() as u32, seat, &qh, ());
        loop {
            queue.blocking_dispatch(&mut s)?;
        }
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for S {
        fn event(s: &mut S, reg: &wl_registry::WlRegistry, ev: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<S>) {
            if let wl_registry::Event::Global { name, interface, .. } = ev {
                match interface.as_str() {
                    "wl_seat" if s.seat.is_none() => s.seat = Some(reg.bind(name, 1, qh, ())),
                    "ext_idle_notifier_v1" => s.notifier = Some(reg.bind(name, 1, qh, ())),
                    _ => {}
                }
            }
        }
    }

    impl Dispatch<note::ExtIdleNotificationV1, ()> for S {
        fn event(s: &mut S, _: &note::ExtIdleNotificationV1, ev: note::Event, _: &(), _: &Connection, _: &QueueHandle<S>) {
            let v = match ev {
                // Idle since the last input, `timeout` ago.
                note::Event::Idled => now().saturating_sub(s.timeout.as_secs()).max(1),
                note::Event::Resumed => 0,
                _ => return,
            };
            s.idle.store(v, Ordering::Relaxed);
        }
    }

    wayland_client::delegate_noop!(S: ignore wl_seat::WlSeat);
    wayland_client::delegate_noop!(S: ignore notifier::ExtIdleNotifierV1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contacts() -> Vec<String> {
        vec!["a".into(), "b".into()]
    }

    #[test]
    fn wire_round_trips() {
        let w = Wire { state: State::Away { since: 5 }, ts: 9, dev: String::new() };
        assert_eq!(Wire::decode(&w.encode()), Some(w));
        assert_eq!(Wire::decode(r#"{"state":"active","ts":1}"#), Some(Wire { state: State::Active, ts: 1, dev: String::new() }));
    }

    #[test]
    fn tells_online_contacts_then_refreshes() {
        let mut p = Presence::default();
        let t = Instant::now();
        let to = |v: Vec<(String, Wire)>| v.into_iter().map(|(c, _)| c).collect::<Vec<_>>();
        assert_eq!(to(p.due(State::Active, &contacts(), |c| c == "a", t)), ["a"]);
        assert!(p.due(State::Active, &contacts(), |c| c == "a", t + Duration::from_secs(10)).is_empty());
        // b comes online: told at the next tick.
        assert_eq!(to(p.due(State::Active, &contacts(), |_| true, t + Duration::from_secs(11))), ["b"]);
        assert_eq!(to(p.due(State::Active, &contacts(), |_| true, t + REFRESH)), ["a"]);
        // A change goes to every online contact at once.
        assert_eq!(to(p.due(State::Away { since: 1 }, &contacts(), |_| true, t + REFRESH)), ["a", "b"]);
    }

    #[test]
    fn hidden_is_said_once_to_everyone() {
        let mut p = Presence::default();
        let t = Instant::now();
        p.due(State::Active, &contacts(), |_| true, t);
        assert_eq!(p.due(State::Hidden, &contacts(), |_| false, t).len(), 2);
        assert!(p.due(State::Hidden, &contacts(), |_| true, t + REFRESH).is_empty());
    }

    #[test]
    fn received_states_expire_and_hidden_forgets() {
        let mut p = Presence::default();
        let t = Instant::now();
        assert!(p.received("a", Wire { state: State::Active, ts: now(), dev: String::new() }, t));
        assert!(!p.received("a", Wire { state: State::Active, ts: now(), dev: String::new() }, t));
        assert_eq!(p.state_of("a"), Some(State::Active));
        // Late messages are ignored.
        assert!(!p.received("b", Wire { state: State::Active, ts: now() - 3600, dev: String::new() }, t));
        assert!(p.expire(t + EXPIRE));
        assert_eq!(p.state_of("a"), None);
        assert!(p.seen.contains_key("a"));
        p.received("a", Wire { state: State::Active, ts: now(), dev: String::new() }, t);
        p.received("a", Wire { state: State::Hidden, ts: now(), dev: String::new() }, t);
        assert_eq!(p.state_of("a"), None);
    }
}
