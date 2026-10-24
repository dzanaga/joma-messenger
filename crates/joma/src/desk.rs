//! Desktop share during a call. With a peer running Joma, the desktop goes through Joma's
//! own stream (gliff's pipeline, carried by a Jami service tunnel). With anyone else, or
//! when the Joma way fails, it falls back to Jami's screen share through the portal.
//!
//! 1. When a call becomes current, each Joma sends `Hello` as an in-call message.
//! 2. Share pressed, peer said hello, Hyprland here: start the share endpoint, expose it
//!    to the peer only, and send `Offer` (service, device, token).
//! 3. The viewer opens a tunnel to the service, connects with the token and sends
//!    `Accept` once pictures arrive. `Decline`, or no `Accept` within 15 s, makes the
//!    sharer switch to the portal share.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use joma_desk::portal::PortalShare;
use joma_desk::share::{Share, ShareEvent};
use joma_desk::signal::{self, DeskMsg};
use joma_desk::view::Viewer;
use joma_jami::Details;

const ACCEPT_TIMEOUT: Duration = Duration::from_secs(15);

pub enum Sharing {
    Off,
    /// Asking the portal; the user is picking a screen.
    Picking,
    Joma { share: Share, service: String, offered: Instant, accepted: bool },
    Portal { portal: PortalShare, input: String },
}

pub enum Viewing {
    Off,
    /// Asking the sharer's daemon which device advertises the service.
    Querying { request: u32, uri: String, service: String, device: String, token: String },
    Opening { tunnel: String, token: String },
    On { viewer: Viewer, tunnel: String, accepted: bool },
}

pub struct Session {
    pub sharing: Sharing,
    pub viewing: Viewing,
    /// Why the last Joma attempt fell back, for the status line.
    pub note: Option<String>,
}

impl Default for Session {
    fn default() -> Self {
        Session { sharing: Sharing::Off, viewing: Viewing::Off, note: None }
    }
}

impl Session {
    pub fn sharing_label(&self) -> Option<&'static str> {
        match self.sharing {
            Sharing::Off => None,
            Sharing::Picking => Some("choosing a screen…"),
            Sharing::Joma { accepted: false, .. } => Some("offering desktop (Joma)…"),
            Sharing::Joma { accepted: true, .. } => Some("sharing desktop · Joma"),
            Sharing::Portal { .. } => Some("sharing screen · Jami"),
        }
    }
}

/// What the app must do after the controller handled something.
#[derive(Debug, PartialEq, Eq)]
pub enum After {
    Nothing,
    /// Fall back to the portal share for this call.
    Portal(String),
}

#[derive(Default)]
pub struct Desk {
    pub sessions: HashMap<String, Session>,
}

fn send(account: &str, call: &str, msg: &DeskMsg) {
    let mut payloads = Details::new();
    payloads.insert(signal::MIME.into(), msg.encode());
    if let Err(e) = joma_jami::send_call_message(account, call, &payloads) {
        tracing::warn!(error = %e, "could not send a desk message");
    }
}

impl Desk {
    pub fn session(&mut self, call: &str) -> &mut Session {
        self.sessions.entry(call.to_owned()).or_default()
    }

    /// Say hello when a call becomes current.
    pub fn hello(&mut self, account: &str, call: &str) {
        send(account, call, &DeskMsg::Hello { v: signal::VERSION, share: joma_desk::share::available(), view: true });
    }

    /// Start sharing: Joma's way if the peer can take it, else the portal.
    pub fn start(
        &mut self,
        account: &str,
        call: &str,
        peer: &str,
        peer_joma: bool,
        events: impl Fn(ShareEvent) + Send + 'static,
    ) -> After {
        if !matches!(self.session(call).sharing, Sharing::Off) {
            return After::Nothing;
        }
        if !peer_joma {
            return After::Portal(call.to_owned());
        }
        if !joma_desk::share::available() {
            self.session(call).note = Some("not on Hyprland: using Jami's screen share".into());
            return After::Portal(call.to_owned());
        }
        match self.offer(account, call, peer, events) {
            Ok(()) => After::Nothing,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "Joma share failed; falling back");
                self.session(call).note = Some(format!("Joma share failed ({e}); using Jami's"));
                After::Portal(call.to_owned())
            }
        }
    }

    fn offer(&mut self, account: &str, call: &str, peer: &str, events: impl Fn(ShareEvent) + Send + 'static) -> Result<()> {
        let token = signal::token();
        let share = Share::start(token.clone(), events)?;
        let service = joma_jami::expose_service(account, "joma-desk", share.port, &[peer])?;
        let msg = DeskMsg::Offer {
            v: signal::VERSION,
            service: service.clone(),
            device: joma_jami::device_id(account)?,
            uri: joma_jami::account_uri(account)?,
            token,
            output: share.screen.name.clone(),
            width: share.screen.width,
            height: share.screen.height,
        };
        send(account, call, &msg);
        self.session(call).sharing = Sharing::Joma { share, service, offered: Instant::now(), accepted: false };
        Ok(())
    }

    /// The portal answered: share its stream as the call's video.
    pub fn portal_ready(&mut self, account: &str, call: &str, portal: Result<PortalShare>) {
        let s = self.session(call);
        let portal = match portal {
            Ok(p) => p,
            Err(e) => {
                s.sharing = Sharing::Off;
                s.note = Some(format!("screen share cancelled ({e})"));
                return;
            }
        };
        // Open the source first, as the Qt client does, so the daemon takes the fd now.
        let input = joma_jami::open_video_input(&portal.resource).unwrap_or_default();
        let mut media = joma_jami::current_media(account, call).unwrap_or_default();
        match media.iter_mut().find(|m| m.get("MEDIA_TYPE").map(String::as_str) == Some(joma_jami::VIDEO)) {
            Some(v) => {
                v.insert("SOURCE".into(), portal.resource.clone());
                v.insert("MUTED".into(), "false".into());
                v.insert("ENABLED".into(), "true".into());
            }
            None => media.push(joma_jami::media(joma_jami::VIDEO, "video_0", &portal.resource, false)),
        }
        if let Err(e) = joma_jami::request_media_change(account, call, &media) {
            s.note = Some(format!("could not share: {e}"));
        }
        s.sharing = Sharing::Portal { portal, input };
    }

    pub fn picking(&mut self, call: &str) {
        self.session(call).sharing = Sharing::Picking;
    }

    /// Stop sharing. `camera` is the source to put back for a portal share (empty: none).
    pub fn stop(&mut self, account: &str, call: &str, camera: &str, camera_muted: bool) {
        let s = self.session(call);
        match std::mem::replace(&mut s.sharing, Sharing::Off) {
            Sharing::Joma { share, service, .. } => {
                send(account, call, &DeskMsg::Stop);
                let _ = joma_jami::remove_service(account, &service);
                drop(share);
            }
            Sharing::Portal { portal, input } => {
                let mut media = joma_jami::current_media(account, call).unwrap_or_default();
                if let Some(v) = media.iter_mut().find(|m| m.get("SOURCE").is_some_and(|s| s.starts_with("display://"))) {
                    v.insert("SOURCE".into(), camera.into());
                    v.insert("MUTED".into(), camera_muted.to_string());
                }
                let _ = joma_jami::request_media_change(account, call, &media);
                if !input.is_empty() {
                    let _ = joma_jami::close_video_input(&input);
                }
                drop(portal);
            }
            Sharing::Off | Sharing::Picking => {}
        }
    }

    /// A call ended: drop everything it had.
    pub fn end(&mut self, account: &str, call: &str) {
        if let Some(mut s) = self.sessions.remove(call) {
            if let Sharing::Joma { service, .. } = &s.sharing {
                let _ = joma_jami::remove_service(account, service);
            }
            if let Viewing::Opening { tunnel, .. } | Viewing::On { tunnel, .. } = &s.viewing {
                let _ = joma_jami::close_tunnel(account, tunnel);
            }
            s.sharing = Sharing::Off;
            s.viewing = Viewing::Off;
        }
    }

    /// An in-call message from the peer. Returns true for a `Hello` (with its abilities).
    pub fn message(&mut self, account: &str, call: &str, peer: &str, msg: DeskMsg) -> (Option<(bool, bool)>, After) {
        match msg {
            DeskMsg::Hello { share, view, .. } => return (Some((share, view)), After::Nothing),
            DeskMsg::Offer { service, device, uri, token, .. } => {
                let s = self.session(call);
                if let Viewing::Opening { tunnel, .. } | Viewing::On { tunnel, .. } = &s.viewing {
                    let _ = joma_jami::close_tunnel(account, tunnel);
                }
                s.viewing = Viewing::Off;
                let uri = if uri.is_empty() { peer.to_owned() } else { uri };
                // The daemon names the device that advertises the service (the id format
                // it wants); the offer's device is the fallback.
                match joma_jami::query_peer_services(account, &uri) {
                    Ok(request) => self.session(call).viewing = Viewing::Querying { request, uri, service, device, token },
                    Err(_) => self.open(account, call, &uri, &device, &service, token),
                }
            }
            DeskMsg::Accept => {
                if let Sharing::Joma { accepted, .. } = &mut self.session(call).sharing {
                    *accepted = true;
                }
            }
            DeskMsg::Decline { reason } => {
                if matches!(self.session(call).sharing, Sharing::Joma { .. }) {
                    self.stop(account, call, "", true);
                    self.session(call).note = Some(format!("peer could not view ({reason}); using Jami's share"));
                    return (None, After::Portal(call.to_owned()));
                }
            }
            DeskMsg::Stop => {
                let s = self.session(call);
                if let Viewing::Opening { tunnel, .. } | Viewing::On { tunnel, .. } = &s.viewing {
                    let _ = joma_jami::close_tunnel(account, tunnel);
                }
                s.viewing = Viewing::Off;
            }
        }
        (None, After::Nothing)
    }

    fn open(&mut self, account: &str, call: &str, uri: &str, device: &str, service: &str, token: String) {
        tracing::info!(%uri, %device, %service, "opening the desktop-share tunnel");
        match joma_jami::open_tunnel(account, uri, device, service, "joma-desk") {
            Ok(tunnel) => self.session(call).viewing = Viewing::Opening { tunnel, token },
            Err(e) => {
                self.session(call).viewing = Viewing::Off;
                send(account, call, &DeskMsg::Decline { reason: e.to_string() });
            }
        }
    }

    /// The peer's service list arrived: open the tunnel to the advertising device.
    pub fn peer_services(&mut self, account: &str, request: u32, peer: &str, services: &str) {
        let list: Vec<serde_json::Value> = serde_json::from_str(services).unwrap_or_default();
        let found = self.sessions.iter().find_map(|(call, s)| match &s.viewing {
            Viewing::Querying { request: r, uri, service, device, token }
                if *r == request || (request == 0 && crate::model::bare_uri(uri) == crate::model::bare_uri(peer)) =>
            {
                let advertised = list
                    .iter()
                    .find(|v| v.get("id").and_then(|i| i.as_str()) == Some(service.as_str()))
                    .and_then(|v| v.get("device").and_then(|d| d.as_str()))
                    .filter(|d| !d.is_empty())
                    .map(str::to_owned);
                Some((call.clone(), uri.clone(), advertised.unwrap_or_else(|| device.clone()), service.clone(), token.clone()))
            }
            _ => None,
        });
        if let Some((call, uri, device, service, token)) = found {
            self.open(account, &call, &uri, &device, &service, token);
        }
    }

    /// The tunnel to a sharer is up: start viewing through it.
    pub fn tunnel_opened(&mut self, tunnel_id: &str, port: u16, changed: impl Fn() + Send + 'static) {
        for s in self.sessions.values_mut() {
            if let Viewing::Opening { tunnel, token } = &s.viewing {
                if tunnel == tunnel_id {
                    let (tunnel, token) = (tunnel.clone(), token.clone());
                    s.viewing = match Viewer::start(port, token, changed) {
                        Ok(viewer) => Viewing::On { viewer, tunnel, accepted: false },
                        Err(e) => {
                            s.note = Some(format!("cannot view: {e}"));
                            Viewing::Off
                        }
                    };
                    return;
                }
            }
        }
    }

    pub fn tunnel_closed(&mut self, account: &str, tunnel_id: &str, reason: &str) -> Option<String> {
        for (call, s) in self.sessions.iter_mut() {
            let ours = matches!(&s.viewing, Viewing::Opening { tunnel, .. } | Viewing::On { tunnel, .. } if tunnel == tunnel_id);
            if ours {
                let never_shown = matches!(s.viewing, Viewing::Opening { .. } | Viewing::On { accepted: false, .. });
                s.viewing = Viewing::Off;
                if never_shown {
                    send(account, call, &DeskMsg::Decline { reason: reason.to_owned() });
                }
                return Some(call.clone());
            }
        }
        None
    }

    /// Housekeeping on each UI tick: accept once pictures flow, fall back on silence.
    pub fn tick(&mut self, account: &str) -> Vec<After> {
        let mut out = Vec::new();
        let calls: Vec<String> = self.sessions.keys().cloned().collect();
        for call in calls {
            let s = self.session(&call);
            if let Viewing::On { viewer, accepted, .. } = &mut s.viewing {
                let st = viewer.state.lock().unwrap();
                if !*accepted && st.picture.is_some() {
                    *accepted = true;
                    drop(st);
                    send(account, &call, &DeskMsg::Accept);
                } else if let Some(why) = st.ended.clone() {
                    let never_shown = st.picture.is_none();
                    drop(st);
                    s.viewing = Viewing::Off;
                    if never_shown {
                        send(account, &call, &DeskMsg::Decline { reason: why });
                    }
                }
            }
            let s = self.session(&call);
            if let Sharing::Joma { offered, accepted: false, .. } = &s.sharing {
                if offered.elapsed() > ACCEPT_TIMEOUT {
                    self.stop(account, &call, "", true);
                    self.session(&call).note = Some("no answer to the Joma share; using Jami's".into());
                    out.push(After::Portal(call.clone()));
                }
            }
        }
        out
    }

    pub fn viewer(&self, call: &str) -> Option<&Viewer> {
        match &self.sessions.get(call)?.viewing {
            Viewing::On { viewer, .. } => Some(viewer),
            _ => None,
        }
    }
}
