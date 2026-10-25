//! The sharing side: capture the focused Hyprland output and stream it to one viewer.
//!
//! This is gliff's server pipeline used as libraries: `hypr-capture` fills GBM dmabufs from
//! `ext-image-copy-capture`, `gliff-vk` splits each frame into two 4:2:0 planes on the GPU
//! (AVC444, sharp coloured text) and encodes both with VA-API H.264, or `gliff-sw` does it on
//! the CPU. Frames go out in gliff's wire format; the rate control is gliff's (`rate.rs`).
//!
//! The endpoint listens on loopback and is exposed to the call peer as a Jami service, so
//! the bytes travel over Jami's own encrypted peer-to-peer channel. The viewer must open
//! the connection with the token from the offer.

use std::os::fd::AsFd;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use gliff_proto::{ChromaMode, ClientMsg, Codec, OutputInfo, ServerMsg, SessionInfo, VideoPipeline, PROTOCOL_VERSION};
use gliff_transport::Framed;
use gliff_vk::{DmabufPlane, Gpu};
use hypr_capture::{CaptureConfig, CaptureEvent, CapturedFrame, Capturer};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

use crate::gpu::Tier;
use crate::rate::{Ladder, LinkEstimator, RateController, LEVELS};
use crate::va::VaEncoder;
use crate::signal::same;

const MAX_FPS: u32 = 60;

/// What the sharer reports to the app.
#[derive(Debug, Clone)]
pub enum ShareEvent {
    ViewerConnected { pipeline: &'static str },
    ViewerLeft,
    Error(String),
}

/// The output being shared.
#[derive(Debug, Clone)]
pub struct Screen {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

/// True when this session can stream its desktop the Joma way (Hyprland is running).
pub fn available() -> bool {
    hypr_ipc::Instance::discover(None).is_ok()
}

/// The screen the user is most likely looking at: the focused monitor, else the leftmost.
pub fn focused_screen() -> Result<Screen> {
    let inst = hypr_ipc::Instance::discover(None).context("no Hyprland session")?;
    let mons = inst.monitors()?;
    let enabled = || mons.iter().filter(|m| !m.disabled);
    let m = enabled().find(|m| m.focused).or_else(|| enabled().min_by_key(|m| (m.x, m.y))).context("no monitor")?;
    Ok(Screen { name: m.name.clone(), width: m.width, height: m.height, scale: m.scale })
}

/// A running share endpoint. Dropping it stops the stream.
pub struct Share {
    pub port: u16,
    pub token: String,
    pub screen: Screen,
    stop: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Share {
    /// Listen on loopback for the viewer. `events` is called from the share thread.
    pub fn start(token: String, events: impl Fn(ShareEvent) + Send + 'static) -> Result<Share> {
        let screen = focused_screen()?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let (stop, stop_rx) = watch::channel(false);
        let (tok, scr) = (token.clone(), screen.clone());
        let thread = std::thread::Builder::new().name("joma-share".into()).spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(accept_loop(listener, tok, scr, stop_rx, events));
        })?;
        Ok(Share { port, token, screen, stop, thread: Some(thread) })
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

async fn accept_loop(
    listener: std::net::TcpListener,
    token: String,
    screen: Screen,
    mut stop: watch::Receiver<bool>,
    events: impl Fn(ShareEvent),
) {
    let listener = match tokio::net::TcpListener::from_std(listener) {
        Ok(l) => l,
        Err(e) => return events(ShareEvent::Error(e.to_string())),
    };
    loop {
        tokio::select! {
            conn = listener.accept() => {
                let Ok((sock, _)) = conn else { continue };
                // One viewer at a time: a 1:1 call has one peer.
                let r = serve(sock, &token, &screen, &mut stop, &events).await;
                match r {
                    Ok(()) => events(ShareEvent::ViewerLeft),
                    Err(e) => {
                        tracing::warn!(error = %format!("{e:#}"), "share session ended");
                        events(ShareEvent::Error(format!("{e:#}")));
                    }
                }
                if *stop.borrow() {
                    return;
                }
            }
            _ = stop.changed() => return,
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// One message for the writer task, with its payloads.
struct Out(ServerMsg, Vec<Vec<u8>>);

enum Enc {
    Gpu(Box<gliff_vk::Encoder>),
    Va(Box<VaEncoder>),
    Cpu(Box<gliff_sw::Encoder>),
}

impl Enc {
    fn set_rate(&mut self, bitrate: u32, fps: u32, vbv_ms: u32) {
        match self {
            Enc::Gpu(e) => e.set_rate(bitrate, fps, vbv_ms),
            Enc::Va(e) => e.set_rate(bitrate, fps, vbv_ms),
            Enc::Cpu(e) => e.set_rate(bitrate, fps),
        }
    }
}

/// The encoder for the current stream shape.
struct Video {
    tier: Tier,
    gpu: Option<Arc<Gpu>>,
    enc: Option<Enc>,
    /// Stream size, chroma layout and fps the encoder was built for.
    stream: (u32, u32),
    dual: bool,
}

impl Video {
    fn pipeline(&self) -> VideoPipeline {
        match self.tier {
            Tier::Gpu { .. } | Tier::Va(_) => VideoPipeline::Gpu,
            Tier::Cpu => VideoPipeline::Cpu,
        }
    }

    fn max_size(&self) -> (u32, u32) {
        match &self.tier {
            Tier::Gpu { gpu, .. } => gliff_vk::Encoder::max_size(gpu).unwrap_or((4096, 4096)),
            Tier::Va(va) => va.max_size(),
            Tier::Cpu => gliff_sw::Encoder::MAX_SIZE,
        }
    }

    /// The stream size for a frame of `extent`: the extent, scaled down to fit the encoder.
    fn stream_for(&self, extent: (u32, u32)) -> Result<(u32, u32)> {
        let (mw, mh) = self.max_size();
        if extent.0 <= mw && extent.1 <= mh {
            return Ok(extent);
        }
        if !self.tier.takes_dmabufs() {
            // Encoders fed with pixels take them at the captured size.
            bail!("a {}x{} screen is larger than the encoder takes", extent.0, extent.1);
        }
        let k = (mw as f64 / extent.0 as f64).min(mh as f64 / extent.1 as f64);
        Ok(((extent.0 as f64 * k) as u32 & !1, (extent.1 as f64 * k) as u32 & !1))
    }

    fn rebuild(&mut self, stream: (u32, u32), dual: bool, bitrate: u32, fps: u32, vbv_ms: u32) -> Result<()> {
        self.enc = None;
        self.enc = Some(match &self.tier {
            Tier::Gpu { gpu, .. } => {
                let s = gliff_vk::EncoderSettings { width: stream.0, height: stream.1, bitrate, framerate: fps, vbv_ms };
                Enc::Gpu(Box::new(gliff_vk::Encoder::new(gpu, s, dual)?))
            }
            Tier::Va(va) => Enc::Va(Box::new(VaEncoder::new(va, stream.0, stream.1, bitrate, fps, vbv_ms)?)),
            Tier::Cpu => {
                let s = gliff_sw::EncoderSettings { width: stream.0, height: stream.1, bitrate, framerate: fps };
                Enc::Cpu(Box::new(gliff_sw::Encoder::new(s, dual)?))
            }
        });
        self.stream = stream;
        self.dual = dual;
        Ok(())
    }

    fn encode(&mut self, frame: &CapturedFrame, keyframe: bool) -> Result<(Vec<u8>, Option<Vec<u8>>, bool)> {
        let enc = self.enc.as_mut().ok_or_else(|| anyhow!("no encoder"))?;
        let out = match enc {
            Enc::Gpu(e) if frame.buffer.is_shm() || !self.tier.takes_dmabufs() => {
                // Shared memory, or a GPU other than the compositor's: upload the pixels.
                let img = frame.read_bgra()?;
                let f = e.encode_bgra(&img.pixels, keyframe)?;
                (f.main, f.aux, f.keyframe)
            }
            Enc::Va(e) => {
                let img = frame.read_bgra()?;
                let (au, key) = e.encode_bgra(&img.pixels, img.width * 4, keyframe)?;
                (au, None, key)
            }
            Enc::Gpu(e) => {
                let info = &frame.buffer.info;
                let fourcc = drm_fourcc::DrmFourcc::try_from(info.fourcc)
                    .map_err(|_| anyhow!("capture fourcc {:#x} is not a DRM format", info.fourcc))?;
                let plane = DmabufPlane {
                    fd: info.fd.as_fd(),
                    width: info.width,
                    height: info.height,
                    offset: info.planes[0].offset,
                    stride: info.planes[0].stride,
                    fourcc,
                    modifier: info.modifier,
                };
                // The import is cached per ring buffer; the generation changes when the
                // ring is reallocated, so a stale import is never reused.
                let key = (frame.buffer.generation << 32) | frame.buffer.index as u64;
                let f = e.encode_dmabuf_transformed(key, &plane, frame.transform, keyframe)?;
                (f.main, f.aux, f.keyframe)
            }
            Enc::Cpu(e) => {
                let img = frame.read_bgra()?;
                let f = e.encode_bgra(&img.pixels, keyframe)?;
                (f.main, f.aux, f.keyframe)
            }
        };
        Ok(out)
    }
}


async fn serve(
    mut sock: TcpStream,
    token: &str,
    screen: &Screen,
    stop: &mut watch::Receiver<bool>,
    events: &impl Fn(ShareEvent),
) -> Result<()> {
    sock.set_nodelay(true)?;
    let mut line = [0u8; 33];
    tokio::time::timeout(Duration::from_secs(10), sock.read_exact(&mut line)).await.context("no token")??;
    if !same(&line[..32], token.as_bytes()) || line[32] != b'\n' {
        bail!("a connection presented the wrong token");
    }
    let (rd, wr) = sock.into_split();
    let mut rx = Framed::new(rd);
    let mut tx = Framed::new(wr);

    let hello: ClientMsg = rx.read_msg().await?;
    let ClientMsg::Hello { version, caps, .. } = hello else { bail!("the viewer did not say hello") };
    if version != PROTOCOL_VERSION {
        let msg = ServerMsg::error(gliff_proto::ERROR_VERSION, gliff_proto::version_mismatch(version, PROTOCOL_VERSION));
        tx.write_msg(&msg).await?;
        bail!("viewer speaks protocol {version}");
    }
    if !caps.codecs.contains(&Codec::H264) {
        tx.write_msg(&ServerMsg::error(gliff_proto::ERROR_NO_CODEC, "H.264 is required")).await?;
        bail!("viewer has no H.264 decoder");
    }

    let tier = crate::gpu::encode_tier();
    tracing::info!(tier = tier.label(), node = ?tier.node(), "sharing");
    let gpu = match &tier {
        Tier::Gpu { gpu, .. } => Some(gpu.clone()),
        _ => None,
    };
    // Two streams carry full 4:4:4 colour; the CPU tier sends one, as gliff does.
    let can_dual = caps.chroma.contains(&ChromaMode::Dual420) && gpu.is_some();
    let can_single = caps.chroma.contains(&ChromaMode::Single420);
    if !can_dual && !can_single {
        tx.write_msg(&ServerMsg::error(gliff_proto::ERROR_NO_CHROMA, "no common chroma mode")).await?;
        bail!("no common chroma mode");
    }
    let dmabufs = tier.takes_dmabufs();
    let mut video = Video { tier, gpu, enc: None, stream: (0, 0), dual: can_dual };
    events(ShareEvent::ViewerConnected { pipeline: video.pipeline().label() });

    let (cap_tx, mut cap_rx) = mpsc::unbounded_channel::<CaptureEvent>();
    let mut cc = CaptureConfig::new(screen.name.clone());
    // Capture buffers live on the compositor's GPU; another encoder reads them as pixels.
    cc.render_node = crate::gpu::compositor_node();
    cc.cursor = true;
    cc.buffers = 3;
    if !dmabufs {
        cc.prefer_linear = true;
        cc.shm_fallback = true;
    }
    let capturer = Capturer::start(
        cc,
        Box::new(move |ev| {
            let _ = cap_tx.send(ev);
        }),
    )?;

    // A writer task owns the socket, so a slow write never stalls reading acks.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Out>();
    let writer = tokio::spawn(async move {
        while let Some(Out(msg, payloads)) = out_rx.recv().await {
            let parts: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
            tx.write_msg_with_payloads(&msg, &parts).await?;
        }
        anyhow::Ok(())
    });
    let send = |msg: ServerMsg, payloads: Vec<Vec<u8>>| out_tx.send(Out(msg, payloads)).map_err(|_| anyhow!("writer gone"));

    let scale_milli = (screen.scale * 1000.0).round() as u32;
    send(
        ServerMsg::HelloAck {
            version: PROTOCOL_VERSION,
            session: SessionInfo { headless: false, output: screen.name.clone() },
            outputs: vec![OutputInfo {
                name: screen.name.clone(),
                width: screen.width,
                height: screen.height,
                scale_milli,
            }],
            features: gliff_proto::features(),
        },
        vec![],
    )?;
    send(ServerMsg::Ping { t: now_ms() }, vec![])?;

    let pixels = screen.width as u64 * screen.height as u64;
    let mut link = LinkEstimator::new();
    let mut ctl = RateController::new(pixels, if can_dual { 2 } else { 1 }, MAX_FPS, None);
    let mut ladder = Ladder::new(0);
    let mut fps = MAX_FPS;
    let mut want_dual = can_dual;
    let mut pending: Option<CapturedFrame> = None;
    let mut last_sent: Option<Instant> = None;
    let mut force_key = true;
    let mut frame_id = 0u64;
    let mut shape_id = 0u32;
    let started = Instant::now();
    // A still screen may never complete the first capture; gliff's rescue is a fresh
    // full-damage capture after 700 ms.
    let mut rescued = false;
    capturer.request_frame()?;

    let result: Result<()> = loop {
        let pace = Duration::from_secs_f64(1.0 / fps.max(1) as f64);
        let due = last_sent.map(|t| t + pace).unwrap_or_else(Instant::now);
        tokio::select! {
            ev = cap_rx.recv() => match ev {
                Some(CaptureEvent::Frame(f)) => pending = Some(f),
                Some(CaptureEvent::CursorShape { width, height, hot_x, hot_y, argb }) => {
                    shape_id += 1;
                    let msg = ServerMsg::CursorShape { id: shape_id, width, height, hot_x, hot_y, argb_len: argb.len() as u32 };
                    send(msg, vec![argb])?;
                }
                Some(CaptureEvent::CursorPos { x, y, visible }) => {
                    send(ServerMsg::CursorPos { x: x as f64, y: y as f64, shape_id, visible }, vec![])?;
                }
                Some(CaptureEvent::Ready { .. }) => {}
                Some(CaptureEvent::Error(e)) => break Err(anyhow!("capture: {e}")),
                Some(CaptureEvent::Stopped) | None => break Err(anyhow!("capture stopped")),
            },
            msg = rx.read_msg::<ClientMsg>() => {
                let now = Instant::now();
                match msg {
                    Err(gliff_transport::Error::Closed) => break Ok(()),
                    Err(e) => break Err(e.into()),
                    Ok(ClientMsg::Bye) => break Ok(()),
                    Ok(ClientMsg::Pong { t }) => link.seed_rtt(now, now_ms().saturating_sub(t) as f64),
                    Ok(ClientMsg::Ping { t }) => send(ServerMsg::Pong { t, server_now_ms: now_ms() }, vec![])?,
                    Ok(ClientMsg::RequestKeyframe) => force_key = true,
                    Ok(ClientMsg::FrameAck { frame_id, .. }) => {
                        link.on_ack(now, frame_id);
                        let rate_changed = ctl.on_ack(now, &mut link).is_some();
                        let level = ladder.consider(now, &link, ctl.is_slow_start(), ctl.congested_recently(now),
                            pixels, can_dual, can_single, MAX_FPS);
                        if let Some(i) = level {
                            // Resolution steps are not taken yet: the stream keeps its size and
                            // gives up frame rate and chroma detail only.
                            let l = &LEVELS[i];
                            fps = l.fps_cap.min(MAX_FPS);
                            want_dual = can_dual && (l.aux || !can_single);
                            ctl.reconfigure(pixels, if want_dual { 2 } else { 1 }, MAX_FPS);
                        }
                        if rate_changed || level.is_some() {
                            if let Some(e) = video.enc.as_mut() {
                                e.set_rate(ctl.stream_bitrate(), fps, ctl.vbv_ms(fps));
                            }
                        }
                    }
                    Ok(_) => {}
                }
            },
            _ = tokio::time::sleep_until(due.into()), if pending.is_some() => {},
            _ = tokio::time::sleep_until((started + Duration::from_millis(700)).into()), if frame_id == 0 && !rescued => {
                rescued = true;
                capturer.recapture()?;
            }
            _ = stop.changed() => {
                let _ = send(ServerMsg::error(gliff_proto::ERROR_SETUP, "the share was stopped"), vec![]);
                break Ok(());
            }
        }

        // Send the newest frame when the pace and the link allow it.
        let now = Instant::now();
        if pending.is_none() || now < due {
            continue;
        }
        if link.inflight_bytes() >= link.cap_bytes(now, ctl.is_slow_start()) {
            ctl.note_blocked_at_cap();
            continue;
        }
        let frame = pending.take().expect("checked above");
        let stream = video.stream_for(frame.extent())?;
        let dual = want_dual && video.gpu.is_some();
        if video.enc.is_none() || video.stream != stream || video.dual != dual {
            video.rebuild(stream, dual, ctl.stream_bitrate(), fps, ctl.vbv_ms(fps))?;
            let extent = frame.extent();
            let effective = screen.scale * stream.0 as f32 / extent.0.max(1) as f32;
            send(
                ServerMsg::StreamConfig {
                    codec: Codec::H264,
                    chroma: if dual { ChromaMode::Dual420 } else { ChromaMode::Single420 },
                    pipeline: Some(video.pipeline()),
                    width: stream.0,
                    height: stream.1,
                    scale_milli: (effective * 1000.0).round() as u32,
                    // Parameter sets ride in-band on every keyframe.
                    extradata: Vec::new(),
                    aux_extradata: None,
                    view_width: stream.0,
                    view_height: stream.1,
                    fps_cap: fps,
                },
                vec![],
            )?;
            force_key = true;
        }
        let (main, aux, keyframe) = video.encode(&frame, force_key)?;
        // Hand the buffer back to the capture ring before waiting for the next one.
        drop(frame);
        force_key = false;
        frame_id += 1;
        let bytes = main.len() + aux.as_ref().map_or(0, Vec::len);
        link.on_sent(now, frame_id, bytes, false);
        ctl.note_frame_sent(now);
        let msg = ServerMsg::VideoFrame {
            frame_id,
            pts_us: started.elapsed().as_micros() as u64,
            keyframe,
            damage: Vec::new(),
            data_len: main.len() as u32,
            aux_len: aux.as_ref().map_or(0, |a| a.len() as u32),
        };
        let mut payloads = vec![main];
        payloads.extend(aux);
        send(msg, payloads)?;
        last_sent = Some(now);
        capturer.request_frame()?;
    };
    drop(capturer);
    drop(out_tx);
    let _ = writer.await;
    result
}
