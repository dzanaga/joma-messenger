//! What Joma knows about the account: conversations, messages, names and calls. It is
//! fed daemon events and has no UI, so it is tested on its own.

use std::collections::{HashMap, HashSet};

use joma_jami::{Details, Event, SwarmMessage};

/// A message as Joma shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Msg {
    pub id: String,
    pub author: String,
    pub text: String,
    pub ts: i64,
    pub kind: MsgKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    Text,
    /// A call, a member joining, a file: shown as a quiet line.
    Event,
}

impl Msg {
    /// The visible form of a swarm commit, or `None` for commits that are not shown
    /// (merges, edits, reactions, votes).
    pub fn from_swarm(m: &SwarmMessage) -> Option<Msg> {
        let (text, kind) = match m.kind.as_str() {
            "text/plain" => {
                // An edited or deleted message keeps its id; the newest edition wins.
                let text = m.editions.first().and_then(|e| e.get("body")).map(String::as_str).unwrap_or(m.text());
                if text.is_empty() && m.body.contains_key("edit") {
                    return None;
                }
                (text.to_owned(), MsgKind::Text)
            }
            "application/call-history+json" => {
                let secs = m.body.get("duration").and_then(|d| d.parse::<u64>().ok()).map(|ms| ms / 1000);
                let text = match secs {
                    Some(s) if s > 0 => format!("call · {}:{:02}", s / 60, s % 60),
                    _ => "missed call".to_owned(),
                };
                (text, MsgKind::Event)
            }
            "application/data-transfer+json" => {
                let name = m.body.get("displayName").cloned().unwrap_or_default();
                (format!("file · {name}"), MsgKind::Event)
            }
            "member" => {
                let action = m.body.get("action").map(String::as_str).unwrap_or("");
                let verb = match action {
                    "add" => "was invited",
                    "join" => "joined",
                    "remove" => "left",
                    "ban" => "was removed",
                    _ => return None,
                };
                (verb.to_owned(), MsgKind::Event)
            }
            "initial" => ("conversation started".to_owned(), MsgKind::Event),
            _ => return None,
        };
        Some(Msg { id: m.id.clone(), author: m.author().to_owned(), text, ts: m.timestamp(), kind })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    /// The title was set on the conversation, not made from member names.
    pub own_title: bool,
    /// Member URIs other than ourselves.
    pub peers: Vec<String>,
    /// Oldest first.
    pub messages: Vec<Msg>,
    pub unread: u32,
    /// An incoming request not yet accepted.
    pub request: bool,
    /// Someone is typing.
    pub composing: bool,
    /// The newest message was loaded (the history may be partial before it).
    pub loaded: bool,
}

impl Conversation {
    pub fn last(&self) -> Option<&Msg> {
        self.messages.last()
    }

    pub fn last_ts(&self) -> i64 {
        self.last().map(|m| m.ts).unwrap_or(0)
    }

    /// Add or replace messages, keeping time order. Returns how many were new.
    pub fn merge(&mut self, msgs: impl IntoIterator<Item = Msg>) -> usize {
        let mut added = 0;
        for m in msgs {
            if let Some(old) = self.messages.iter_mut().find(|o| o.id == m.id) {
                *old = m;
            } else {
                self.messages.push(m);
                added += 1;
            }
        }
        self.messages.sort_by_key(|m| m.ts);
        added
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallState {
    Incoming,
    Connecting,
    Ringing,
    Current,
    Hold,
    Over,
}

impl CallState {
    pub fn parse(s: &str) -> CallState {
        match s {
            "INCOMING" => CallState::Incoming,
            "CONNECTING" | "SEARCHING" => CallState::Connecting,
            "RINGING" => CallState::Ringing,
            "CURRENT" => CallState::Current,
            "HOLD" => CallState::Hold,
            // HUNGUP, BUSY, FAILURE, INACTIVE, OVER
            _ => CallState::Over,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CallState::Incoming => "incoming call",
            CallState::Connecting => "connecting",
            CallState::Ringing => "ringing",
            CallState::Current => "in call",
            CallState::Hold => "on hold",
            CallState::Over => "call ended",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Call {
    pub id: String,
    pub peer: String,
    pub conversation: Option<String>,
    pub state: CallState,
    pub video: bool,
    pub muted_audio: bool,
    pub muted_video: bool,
    pub started: Option<std::time::Instant>,
    /// The peer said hello in Joma's handshake: it can take Joma's desktop share.
    pub peer_joma: bool,
    pub peer_can_share: bool,
    pub hello_sent: bool,
    /// Decoders of this call's remote video, to attach a sink to.
    pub sinks: Vec<String>,
}

/// A Jami URI without its scheme or host part: `ring:abc` and `abc@ring.dht` are `abc`.
pub fn bare_uri(uri: &str) -> &str {
    let uri = uri.strip_prefix("ring:").or_else(|| uri.strip_prefix("jami:")).unwrap_or(uri);
    uri.split('@').next().unwrap_or(uri)
}

/// The display name inside a vCard (`FN:`).
pub fn vcard_name(vcard: &str) -> Option<String> {
    vcard.lines().find_map(|l| l.strip_prefix("FN:").or_else(|| l.strip_prefix("FN;CHARSET=utf-8:"))).map(|n| n.trim().to_owned()).filter(|n| !n.is_empty())
}

#[derive(Debug, Default)]
pub struct Model {
    pub account: String,
    /// Our own URI (40 hex digits).
    pub me: String,
    pub my_name: String,
    pub registration: String,
    pub conversations: HashMap<String, Conversation>,
    /// Names and registered names of peers, by bare URI.
    pub names: HashMap<String, String>,
    pub calls: HashMap<String, Call>,
    /// Pending `loadConversation` requests.
    pub loading: HashMap<u32, String>,
    /// Peers (bare URIs) reachable now.
    pub online: HashSet<String>,
}

/// What an event changed, for the UI.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Changed {
    pub any: bool,
    /// A message arrived from someone else in this conversation.
    pub incoming: Option<(String, String)>,
    pub incoming_call: Option<String>,
    /// Names to resolve.
    pub lookup: Vec<String>,
    /// Conversations to (re)load.
    pub reload: Vec<String>,
}

impl Model {
    /// An incoming conversation request: shown with its sender until accepted.
    pub fn add_request(&mut self, id: &str, metadata: &Details) {
        let mut member = Details::new();
        member.insert("uri".into(), metadata.get("from").cloned().unwrap_or_default());
        self.set_conversation(id, metadata, &[member], true);
    }
}

impl Model {
    pub fn name_of(&self, uri: &str) -> String {
        let bare = bare_uri(uri);
        if bare == self.me {
            return "you".into();
        }
        self.names.get(bare).cloned().unwrap_or_else(|| format!("{}…", &bare[..bare.len().min(8)]))
    }

    /// Conversations newest first; requests first of all.
    pub fn ordered(&self) -> Vec<&Conversation> {
        let mut v: Vec<&Conversation> = self.conversations.values().collect();
        v.sort_by(|a, b| b.request.cmp(&a.request).then(b.last_ts().cmp(&a.last_ts())).then(a.id.cmp(&b.id)));
        v
    }

    /// A member of the conversation is online.
    pub fn is_online(&self, c: &Conversation) -> bool {
        c.peers.iter().any(|p| self.online.contains(p))
    }

    pub fn total_unread(&self) -> u32 {
        self.conversations.values().map(|c| c.unread).sum()
    }

    /// A title for a conversation: its own, else its members' names.
    pub fn title_for(&self, infos: &Details, peers: &[String]) -> String {
        if let Some(t) = infos.get("title").filter(|t| !t.is_empty()) {
            return t.clone();
        }
        if peers.is_empty() {
            return "Notes to self".into();
        }
        peers.iter().map(|p| self.name_of(p)).collect::<Vec<_>>().join(", ")
    }

    /// Refresh a conversation's members and title from the daemon's answers.
    pub fn set_conversation(&mut self, id: &str, infos: &Details, members: &[Details], request: bool) {
        let peers: Vec<String> = members
            .iter()
            .filter_map(|m| m.get("uri"))
            .map(|u| bare_uri(u).to_owned())
            .filter(|u| *u != self.me)
            .collect();
        let title = self.title_for(infos, &peers);
        let c = self.conversations.entry(id.to_owned()).or_insert_with(|| Conversation { id: id.to_owned(), ..Default::default() });
        c.peers = peers;
        c.title = title;
        c.own_title = infos.get("title").is_some_and(|t| !t.is_empty());
        c.request = request;
    }

    /// Recompute titles made from member names, after a name arrived.
    pub fn retitle(&mut self) {
        let titles: Vec<(String, String)> = self
            .conversations
            .values()
            .filter(|c| !c.own_title && !c.peers.is_empty())
            .map(|c| (c.id.clone(), c.peers.iter().map(|p| self.name_of(p)).collect::<Vec<_>>().join(", ")))
            .collect();
        for (id, t) in titles {
            if let Some(c) = self.conversations.get_mut(&id) {
                c.title = t;
            }
        }
    }

    /// The conversation with exactly this peer, if any.
    pub fn conversation_with(&self, peer: &str) -> Option<&Conversation> {
        let peer = bare_uri(peer);
        self.conversations.values().find(|c| c.peers.len() == 1 && c.peers[0] == peer)
    }

    /// Find a conversation by id, title or a member's name or URI.
    pub fn resolve(&self, query: &str) -> Option<&Conversation> {
        if let Some(c) = self.conversations.get(query) {
            return Some(c);
        }
        let q = query.to_lowercase();
        self.conversations.values().find(|c| {
            c.title.to_lowercase() == q
                || c.peers.iter().any(|p| p == bare_uri(query) || self.name_of(p).to_lowercase() == q)
        })
    }

    pub fn apply(&mut self, ev: &Event) -> Changed {
        let mut ch = Changed::default();
        match ev {
            Event::Registration { account, state } if *account == self.account => {
                self.registration = state.clone();
                ch.any = true;
            }
            Event::SwarmLoaded { request, account, conversation, messages } if *account == self.account => {
                self.loading.remove(request);
                if let Some(c) = self.conversations.get_mut(conversation) {
                    c.merge(messages.iter().filter_map(Msg::from_swarm));
                    c.loaded = true;
                    for m in &c.messages {
                        if !self.names.contains_key(bare_uri(&m.author)) && bare_uri(&m.author) != self.me && !m.author.is_empty() {
                            ch.lookup.push(bare_uri(&m.author).to_owned());
                        }
                    }
                    ch.any = true;
                }
            }
            Event::MessageReceived { account, conversation, message } | Event::MessageUpdated { account, conversation, message }
                if *account == self.account =>
            {
                let Some(msg) = Msg::from_swarm(message) else { return ch };
                let mine = bare_uri(&msg.author) == self.me;
                let received = matches!(ev, Event::MessageReceived { .. });
                if let Some(c) = self.conversations.get_mut(conversation) {
                    let added = c.merge([msg.clone()]);
                    if added > 0 && received && !mine && msg.kind == MsgKind::Text {
                        c.unread += 1;
                        c.composing = false;
                        ch.incoming = Some((conversation.clone(), msg.text.clone()));
                    }
                    ch.any = true;
                } else {
                    ch.reload.push(conversation.clone());
                }
            }
            Event::ConversationReady { account, conversation } if *account == self.account => {
                ch.reload.push(conversation.clone());
            }
            Event::ConversationRequest { account, conversation, metadata } if *account == self.account => {
                self.add_request(conversation, metadata);
                if let Some(from) = metadata.get("from") {
                    ch.lookup.push(bare_uri(from).to_owned());
                }
                ch.any = true;
            }
            Event::ConversationRemoved { account, conversation } if *account == self.account => {
                self.conversations.remove(conversation);
                ch.any = true;
            }
            Event::MemberEvent { account, conversation, .. } | Event::ConversationProfile { account, conversation, .. }
                if *account == self.account =>
            {
                ch.reload.push(conversation.clone());
            }
            Event::Composing { account, conversation, from, composing } if *account == self.account => {
                if bare_uri(from) != self.me {
                    if let Some(c) = self.conversations.get_mut(conversation) {
                        c.composing = *composing;
                        ch.any = true;
                    }
                }
            }
            Event::NameFound { account, state, address, name, .. } if *account == self.account => {
                if *state == 0 && !name.is_empty() && !self.names.contains_key(bare_uri(address)) {
                    self.names.insert(bare_uri(address).to_owned(), name.clone());
                    self.retitle();
                    ch.any = true;
                }
            }
            Event::ProfileReceived { account, from, vcard } if *account == self.account => {
                if let Some(n) = vcard_name(vcard) {
                    self.names.insert(bare_uri(from).to_owned(), n);
                    self.retitle();
                    ch.any = true;
                }
            }
            Event::IncomingCall { account, call, from, media } if *account == self.account => {
                let video = media.iter().any(|m| m.get("MEDIA_TYPE").map(String::as_str) == Some(joma_jami::VIDEO));
                let peer = bare_uri(from).to_owned();
                let conversation = self.conversation_with(&peer).map(|c| c.id.clone());
                self.calls.insert(call.clone(), Call::new(call, &peer, conversation, CallState::Incoming, video));
                ch.incoming_call = Some(call.clone());
                ch.any = true;
            }
            Event::CallState { account, call, state, .. } if *account == self.account => {
                let state = CallState::parse(state);
                if state == CallState::Over {
                    self.calls.remove(call);
                } else if let Some(c) = self.calls.get_mut(call) {
                    if state == CallState::Current && c.started.is_none() {
                        c.started = Some(std::time::Instant::now());
                    }
                    c.state = state;
                }
                ch.any = true;
            }
            Event::Presence { account, uri, online } if *account == self.account => {
                let uri = bare_uri(uri).to_owned();
                ch.any = if *online { self.online.insert(uri) } else { self.online.remove(&uri) };
            }
            Event::DecodingStarted { id, .. } => {
                // The receive side of a call is named after the call.
                if let Some(c) = self.calls.values_mut().find(|c| id.contains(&c.id)) {
                    if !c.sinks.contains(id) {
                        c.sinks.push(id.clone());
                    }
                    ch.any = true;
                }
            }
            Event::DecodingStopped { id } => {
                for c in self.calls.values_mut() {
                    c.sinks.retain(|s| s != id);
                }
                ch.any = true;
            }
            _ => {}
        }
        ch
    }
}

impl Call {
    pub fn new(id: &str, peer: &str, conversation: Option<String>, state: CallState, video: bool) -> Call {
        Call {
            id: id.to_owned(),
            peer: bare_uri(peer).to_owned(),
            conversation,
            state,
            video,
            muted_audio: false,
            muted_video: !video,
            started: None,
            peer_joma: false,
            peer_can_share: false,
            hello_sent: false,
            sinks: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swarm(id: &str, author: &str, body: &str, ts: i64) -> SwarmMessage {
        let mut b = Details::new();
        b.insert("author".into(), author.into());
        b.insert("body".into(), body.into());
        b.insert("timestamp".into(), ts.to_string());
        SwarmMessage { id: id.into(), kind: "text/plain".into(), body: b, ..Default::default() }
    }

    fn model() -> Model {
        let mut m = Model { account: "acc".into(), me: "me".into(), ..Default::default() };
        m.set_conversation("c1", &Details::new(), &[[("uri".to_string(), "bob".to_string())].into()], false);
        m
    }

    #[test]
    fn incoming_messages_count_as_unread_once() {
        let mut m = model();
        let ev = Event::MessageReceived { account: "acc".into(), conversation: "c1".into(), message: swarm("1", "bob", "hi", 5) };
        assert_eq!(m.apply(&ev).incoming, Some(("c1".into(), "hi".into())));
        assert_eq!(m.apply(&ev).incoming, None);
        let mine = Event::MessageReceived { account: "acc".into(), conversation: "c1".into(), message: swarm("2", "me", "yo", 6) };
        m.apply(&mine);
        assert_eq!(m.conversations["c1"].unread, 1);
        assert_eq!(m.conversations["c1"].messages.len(), 2);
    }

    #[test]
    fn history_merges_in_time_order() {
        let mut m = model();
        m.loading.insert(7, "c1".into());
        let ev = Event::SwarmLoaded {
            request: 7,
            account: "acc".into(),
            conversation: "c1".into(),
            messages: vec![swarm("b", "bob", "two", 2), swarm("a", "me", "one", 1)],
        };
        let ch = m.apply(&ev);
        assert_eq!(ch.lookup, ["bob"]);
        let texts: Vec<_> = m.conversations["c1"].messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["one", "two"]);
    }

    #[test]
    fn names_retitle_conversations() {
        let mut m = model();
        assert!(m.conversations["c1"].title.starts_with("bob"));
        m.apply(&Event::NameFound { account: "acc".into(), query: "".into(), state: 0, address: "bob".into(), name: "Bobby".into() });
        assert_eq!(m.conversations["c1"].title, "Bobby");
        assert_eq!(m.resolve("bobby").map(|c| c.id.as_str()), Some("c1"));
    }

    #[test]
    fn uris_and_vcards() {
        assert_eq!(bare_uri("ring:abc"), "abc");
        assert_eq!(bare_uri("abc@ring.dht"), "abc");
        assert_eq!(vcard_name("BEGIN:VCARD\nFN:Ann Lee\nEND:VCARD"), Some("Ann Lee".into()));
    }

    #[test]
    fn presence_marks_conversations_online() {
        let mut m = model();
        assert!(!m.is_online(&m.conversations["c1"]));
        let ev = Event::Presence { account: "acc".into(), uri: "ring:bob".into(), online: true };
        assert!(m.apply(&ev).any);
        assert!(m.is_online(&m.conversations["c1"]));
        assert!(!m.apply(&ev).any);
        m.apply(&Event::Presence { account: "acc".into(), uri: "bob".into(), online: false });
        assert!(!m.is_online(&m.conversations["c1"]));
    }

    #[test]
    fn calls_follow_state() {
        let mut m = model();
        m.apply(&Event::IncomingCall { account: "acc".into(), call: "k".into(), from: "ring:bob".into(), media: vec![] });
        assert_eq!(m.calls["k"].conversation.as_deref(), Some("c1"));
        m.apply(&Event::CallState { account: "acc".into(), call: "k".into(), state: "CURRENT".into(), code: 0 });
        assert!(m.calls["k"].started.is_some());
        m.apply(&Event::CallState { account: "acc".into(), call: "k".into(), state: "OVER".into(), code: 0 });
        assert!(m.calls.is_empty());
    }
}
