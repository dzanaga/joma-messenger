//! Daemon signals as Rust values.

use serde_json::Value;

use crate::{Details, Media, SwarmMessage};

/// A daemon signal. Signals Joma does not use come through as [`Event::Other`].
#[derive(Debug, Clone)]
pub enum Event {
    AccountsChanged,
    Registration { account: String, state: String },
    AccountDetails { account: String },
    ContactAdded { account: String, uri: String, confirmed: bool },
    ContactRemoved { account: String, uri: String },
    TrustRequest { account: String, conversation: String, from: String },
    NameFound { account: String, query: String, state: i64, address: String, name: String },
    ProfileReceived { account: String, from: String, vcard: String },
    Composing { account: String, conversation: String, from: String, composing: bool },
    MessageStatus { account: String, conversation: String, peer: String, message: String, state: i64 },
    SwarmLoaded { request: u32, account: String, conversation: String, messages: Vec<SwarmMessage> },
    MessageReceived { account: String, conversation: String, message: SwarmMessage },
    MessageUpdated { account: String, conversation: String, message: SwarmMessage },
    ConversationReady { account: String, conversation: String },
    ConversationRemoved { account: String, conversation: String },
    ConversationRequest { account: String, conversation: String, metadata: Details },
    MemberEvent { account: String, conversation: String, member: String, event: i64 },
    ConversationProfile { account: String, conversation: String, profile: Details },
    CallState { account: String, call: String, state: String, code: i64 },
    IncomingCall { account: String, call: String, from: String, media: Vec<Media> },
    /// An in-call message: MIME type to payload.
    CallMessage { account: String, call: String, from: String, payloads: Details },
    MediaChangeRequested { account: String, call: String, media: Vec<Media> },
    MediaNegotiation { call: String, event: String, media: Vec<Media> },
    DecodingStarted { id: String, width: i64, height: i64, mixer: bool },
    DecodingStopped { id: String },
    PeerServices { request: u32, account: String, peer: String, status: i64, services: String },
    TunnelOpened { account: String, tunnel: String, port: u16 },
    TunnelClosed { account: String, tunnel: String, reason: String },
    Other { signal: String, args: Vec<Value> },
}

fn s(a: &[Value], i: usize) -> String {
    a.get(i).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn n(a: &[Value], i: usize) -> i64 {
    a.get(i).and_then(Value::as_i64).unwrap_or(0)
}

fn b(a: &[Value], i: usize) -> bool {
    a.get(i).and_then(Value::as_bool).unwrap_or(false)
}

fn de<T: for<'de> serde::Deserialize<'de> + Default>(a: &[Value], i: usize) -> T {
    a.get(i).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

impl Event {
    pub(crate) fn parse(v: &Value) -> Event {
        let signal = v.get("s").and_then(Value::as_str).unwrap_or_default();
        let empty = Vec::new();
        let a = v.get("a").and_then(Value::as_array).unwrap_or(&empty);
        match signal {
            "AccountsChanged" => Event::AccountsChanged,
            "RegistrationStateChanged" => Event::Registration { account: s(a, 0), state: s(a, 1) },
            "AccountDetailsChanged" | "VolatileDetailsChanged" => Event::AccountDetails { account: s(a, 0) },
            "ContactAdded" => Event::ContactAdded { account: s(a, 0), uri: s(a, 1), confirmed: b(a, 2) },
            "ContactRemoved" => Event::ContactRemoved { account: s(a, 0), uri: s(a, 1) },
            "IncomingTrustRequest" => {
                Event::TrustRequest { account: s(a, 0), conversation: s(a, 1), from: s(a, 2) }
            }
            "RegisteredNameFound" => Event::NameFound {
                account: s(a, 0),
                query: s(a, 1),
                state: n(a, 2),
                address: s(a, 3),
                name: s(a, 4),
            },
            "ProfileReceived" => Event::ProfileReceived { account: s(a, 0), from: s(a, 1), vcard: s(a, 2) },
            "ComposingStatusChanged" => Event::Composing {
                account: s(a, 0),
                conversation: s(a, 1),
                from: s(a, 2),
                composing: n(a, 3) != 0,
            },
            "AccountMessageStatusChanged" => Event::MessageStatus {
                account: s(a, 0),
                conversation: s(a, 1),
                peer: s(a, 2),
                message: s(a, 3),
                state: n(a, 4),
            },
            "SwarmLoaded" => Event::SwarmLoaded {
                request: n(a, 0) as u32,
                account: s(a, 1),
                conversation: s(a, 2),
                messages: de(a, 3),
            },
            "SwarmMessageReceived" => {
                Event::MessageReceived { account: s(a, 0), conversation: s(a, 1), message: de(a, 2) }
            }
            "SwarmMessageUpdated" => {
                Event::MessageUpdated { account: s(a, 0), conversation: s(a, 1), message: de(a, 2) }
            }
            "ConversationReady" => Event::ConversationReady { account: s(a, 0), conversation: s(a, 1) },
            "ConversationRemoved" => Event::ConversationRemoved { account: s(a, 0), conversation: s(a, 1) },
            "ConversationRequestReceived" => {
                Event::ConversationRequest { account: s(a, 0), conversation: s(a, 1), metadata: de(a, 2) }
            }
            "ConversationMemberEvent" => Event::MemberEvent {
                account: s(a, 0),
                conversation: s(a, 1),
                member: s(a, 2),
                event: n(a, 3),
            },
            "ConversationProfileUpdated" => {
                Event::ConversationProfile { account: s(a, 0), conversation: s(a, 1), profile: de(a, 2) }
            }
            "StateChange" => Event::CallState { account: s(a, 0), call: s(a, 1), state: s(a, 2), code: n(a, 3) },
            "IncomingCall" => Event::IncomingCall { account: s(a, 0), call: s(a, 1), from: s(a, 2), media: de(a, 3) },
            "IncomingMessage" => {
                Event::CallMessage { account: s(a, 0), call: s(a, 1), from: s(a, 2), payloads: de(a, 3) }
            }
            "MediaChangeRequested" => Event::MediaChangeRequested { account: s(a, 0), call: s(a, 1), media: de(a, 2) },
            "MediaNegotiationStatus" => Event::MediaNegotiation { call: s(a, 0), event: s(a, 1), media: de(a, 2) },
            "DecodingStarted" => {
                Event::DecodingStarted { id: s(a, 0), width: n(a, 2), height: n(a, 3), mixer: b(a, 4) }
            }
            "DecodingStopped" => Event::DecodingStopped { id: s(a, 0) },
            "PeerServicesReceived" => Event::PeerServices {
                request: n(a, 0) as u32,
                account: s(a, 1),
                peer: s(a, 2),
                status: n(a, 3),
                services: s(a, 4),
            },
            "ServiceTunnelOpened" => Event::TunnelOpened { account: s(a, 0), tunnel: s(a, 1), port: n(a, 2) as u16 },
            "ServiceTunnelClosed" => Event::TunnelClosed { account: s(a, 0), tunnel: s(a, 1), reason: s(a, 2) },
            _ => Event::Other { signal: signal.to_owned(), args: a.clone() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_swarm_message() {
        let v = json!({"s": "SwarmMessageReceived", "a": ["acc", "conv", {
            "id": "c1", "type": "text/plain", "linearizedParent": "c0",
            "body": {"author": "abc", "body": "ciao", "timestamp": "1700000000"},
            "reactions": [], "editions": [], "status": {"abc": 3}
        }]});
        let Event::MessageReceived { message, .. } = Event::parse(&v) else { panic!() };
        assert_eq!(message.text(), "ciao");
        assert_eq!(message.author(), "abc");
        assert_eq!(message.timestamp(), 1_700_000_000);
    }

    #[test]
    fn unknown_signals_are_kept() {
        let v = json!({"s": "AudioMeter", "a": ["x", 0.5]});
        assert!(matches!(Event::parse(&v), Event::Other { signal, .. } if signal == "AudioMeter"));
    }
}
