//! The sharing side: capture a Hyprland output or one window and stream it to one viewer.
//! The user picks the [`Source`]; it can change while the viewer watches.
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
use hypr_capture::{CaptureConfig, CaptureEvent, CapturedFrame, Capturer, Toplevel};
use serde::{Deserialize, Serialize};
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

/// The output or window being shared.
#[derive(Debug, Clone)]
pub struct Screen {
    /// The output's name, or the window's title.
    pub name: String,
    /// Logical pixels for a window (its size in Hyprland), else the mode.
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

/// What to share: a whole output, or one window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Source {
    Screen { output: String },
    /// `address` as `hyprctl clients` prints it, `id` its `stableId` (the Wayland
    /// toplevel identifier); `monitor` is the output it is on.
    Window { address: String, id: String, class: String, title: String, monitor: String },
    /// A rectangle of an output, in its pixels (x, y, width, height).
    Area { output: String, rect: (u32, u32, u32, u32) },
}

/// Cut `bgra` (tight rows, `width` wide) to `rect`, clamped to the picture and even-sized.
pub fn crop_bgra(bgra: &[u8], width: usize, height: usize, rect: (u32, u32, u32, u32)) -> (Vec<u8>, usize, usize) {
    let x = (rect.0 as usize).min(width.saturating_sub(2));
    let y = (rect.1 as usize).min(height.saturating_sub(2));
    let w = (rect.2 as usize).min(width - x) & !1;
    let h = (rect.3 as usize).min(height - y) & !1;
    let mut out = Vec::with_capacity(w * h * 4);
    for row in bgra.chunks_exact(width * 4).skip(y).take(h) {
        out.extend_from_slice(&row[x * 4..(x + w) * 4]);
    }
    (out, w, h)
}

/// A source offered to the user, with what to show for it.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub source: Source,
    pub screen: Screen,
    /// The output name, or the window's class.
    pub label: String,
    /// The window's title, or the monitor's description.
    pub detail: String,
    /// The focused monitor, or the focused window.
    pub focused: bool,
}

impl Source {
    /// Parse `screen:<output>`, `window:<address>` or `focused` (the CLI form).
    pub fn parse(s: &str) -> Result<Source> {
        if s == "focused" {
            return Ok(Source::Screen { output: focused_screen()?.name });
        }
        if let Some(o) = s.strip_prefix("screen:") {
            return Ok(Source::Screen { output: o.to_owned() });
        }
        if let Some(a) = s.strip_prefix("area:") {
            // area:<output>@<x>,<y>,<w>x<h>
            let (output, r) = a.split_once('@').context("area:<output>@<x>,<y>,<w>x<h>")?;
            let n: Vec<u32> = r.split([',', 'x']).map(|v| v.trim().parse()).collect::<std::result::Result<_, _>>().context("area numbers")?;
            let [x, y, w, h] = n[..] else { bail!("area:<output>@<x>,<y>,<w>x<h>") };
            if w < 16 || h < 16 {
                bail!("the area is too small");
            }
            return Ok(Source::Area { output: output.to_owned(), rect: (x, y, w, h) });
        }
        if let Some(a) = s.strip_prefix("window:") {
            let a = a.trim_start_matches("0x");
            return sources()?
                .into_iter()
                .map(|i| i.source)
                .find(|src| matches!(src, Source::Window { address, .. } if address.trim_start_matches("0x") == a))
                .context("no such window");
        }
        bail!("a source is screen:<output>, window:<address> or focused")
    }

    /// The short form [`Source::parse`] reads.
    pub fn key(&self) -> String {
        match self {
            Source::Screen { output } => format!("screen:{output}"),
            Source::Window { address, .. } => format!("window:{address}"),
            Source::Area { output, rect: (x, y, w, h) } => format!("area:{output}@{x},{y},{w}x{h}"),
        }
    }

    /// The part of the captured frame that is sent (an area), else all of it.
    pub fn crop(&self) -> Option<(u32, u32, u32, u32)> {
        match self {
            Source::Area { rect, .. } => Some(*rect),
            _ => None,
        }
    }

    /// Ask the user to draw a rectangle (`slurp`) and make it an area of the output it
    /// starts on, in that output's pixels.
    pub fn draw_area() -> Result<Source> {
        let out = std::process::Command::new("slurp").args(["-f", "%o %x %y %w %h"]).output().context("slurp is not installed")?;
        if !out.status.success() {
            bail!("no area chosen");
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let f: Vec<&str> = text.split_whitespace().collect();
        let [output, x, y, w, h] = f[..] else { bail!("slurp said {text:?}") };
        let num = |v: &str| v.parse::<f64>().context("slurp numbers");
        let inst = hypr_ipc::Instance::discover(None).context("no Hyprland session")?;
        let m = inst.monitors()?.into_iter().find(|m| m.name == output).context("the area is on no screen")?;
        let k = m.scale as f64;
        let px = |v: f64| (v * k).round().max(0.) as u32;
        let (x, y) = (px(num(x)? - m.x as f64), px(num(y)? - m.y as f64));
        let (w, h) = (px(num(w)?).min(m.width.saturating_sub(x)), px(num(h)?).min(m.height.saturating_sub(y)));
        if w < 16 || h < 16 {
            bail!("the area is too small");
        }
        Ok(Source::Area { output: output.to_owned(), rect: (x, y, w & !1, h & !1) })
    }

    /// What it looks like now: the output's mode, or the window's size and title.
    pub fn screen(&self) -> Result<Screen> {
        sources()?
            .into_iter()
            .find(|i| match (&i.source, self) {
                (Source::Screen { output: a }, Source::Screen { output: b }) => a == b,
                (Source::Window { address: a, .. }, Source::Window { address: b, .. }) => a == b,
                (Source::Screen { output: a }, Source::Area { output: b, .. }) => a == b,
                _ => false,
            })
            .map(|i| match self {
                Source::Area { output, rect, .. } => Screen { name: format!("{output} area"), width: rect.2, height: rect.3, scale: i.screen.scale },
                _ => i.screen,
            })
            .context("that screen or window is gone")
    }

    /// The capture for this source.
    pub(crate) fn capture_config(&self) -> CaptureConfig {
        match self {
            Source::Screen { output } | Source::Area { output, .. } => CaptureConfig::new(output.clone()),
            Source::Window { id, class, title, monitor, .. } => {
                let mut cc = CaptureConfig::new(monitor.clone());
                cc.toplevel = Some(Toplevel { identifier: id.clone(), app_id: class.clone(), title: title.clone() });
                cc
            }
        }
    }
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

/// One window as `hyprctl -j clients` lists it.
#[derive(Deserialize)]
struct Client {
    address: String,
    #[serde(default)]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    size: [i32; 2],
    #[serde(default)]
    monitor: i64,
    #[serde(default)]
    class: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    pid: i64,
    #[serde(default, rename = "focusHistoryID")]
    focus_history: i64,
    /// The `ext-foreign-toplevel-list` identifier of this window.
    #[serde(default, rename = "stableId")]
    stable_id: String,
}

/// Every enabled output, then the windows (most recently focused first), Joma's own left
/// out.
pub fn sources() -> Result<Vec<SourceInfo>> {
    let inst = hypr_ipc::Instance::discover(None).context("no Hyprland session")?;
    let mons: Vec<_> = inst.monitors()?.into_iter().filter(|m| !m.disabled).collect();
    let mut out: Vec<SourceInfo> = mons
        .iter()
        .map(|m| SourceInfo {
            source: Source::Screen { output: m.name.clone() },
            screen: Screen { name: m.name.clone(), width: m.width, height: m.height, scale: m.scale },
            label: m.name.clone(),
            detail: m.description.clone(),
            focused: m.focused,
        })
        .collect();
    let mut clients: Vec<Client> = inst.request_json("clients").unwrap_or_default();
    clients.retain(|c| c.mapped && !c.hidden && c.size[0] > 0 && c.size[1] > 0 && c.pid != std::process::id() as i64);
    clients.sort_by_key(|c| c.focus_history);
    for c in clients {
        let Some(m) = mons.iter().find(|m| m.id == c.monitor) else { continue };
        let scale = m.scale;
        out.push(SourceInfo {
            source: Source::Window { address: c.address.clone(), id: c.stable_id.clone(), class: c.class.clone(), title: c.title.clone(), monitor: m.name.clone() },
            screen: Screen {
                name: if c.title.is_empty() { c.class.clone() } else { c.title.clone() },
                width: c.size[0] as u32,
                height: c.size[1] as u32,
                scale,
            },
            label: if c.class.is_empty() { "window".into() } else { c.class.clone() },
            detail: c.title.clone(),
            focused: c.focus_history == 0,
        });
    }
    Ok(out)
}

/// A running share endpoint. Dropping it stops the stream.
pub struct Share {
    pub port: u16,
    pub token: String,
    pub source: Source,
    pub screen: Screen,
    switch: watch::Sender<Source>,
    stop: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Share {
    /// Listen on loopback for the viewer. `events` is called from the share thread.
    pub fn start(token: String, source: Source, events: impl Fn(ShareEvent) + Send + 'static) -> Result<Share> {
        let screen = source.screen()?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let (stop, stop_rx) = watch::channel(false);
        let (switch, switch_rx) = watch::channel(source.clone());
        let (tok, scr) = (token.clone(), screen.clone());
        let thread = std::thread::Builder::new().name("joma-share".into()).spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(accept_loop(listener, tok, scr, switch_rx, stop_rx, events));
        })?;
        Ok(Share { port, token, source, screen, switch, stop, thread: Some(thread) })
    }

    /// Share another output or window; a watching viewer follows without reconnecting.
    pub fn set_source(&mut self, source: Source) -> Result<()> {
        self.screen = source.screen()?;
        self.source = source.clone();
        self.switch.send_replace(source);
        Ok(())
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
    mut source: watch::Receiver<Source>,
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
                let r = serve(sock, &token, &screen, &mut source, &mut stop, &events).await;
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

/// A capture of `source`, sending its events to `tx`.
fn start_capture(source: &Source, dmabufs: bool, tx: mpsc::UnboundedSender<CaptureEvent>) -> Result<Capturer> {
    let mut cc = source.capture_config();
    // Capture buffers live on the compositor's GPU; another encoder reads them as pixels.
    cc.render_node = crate::gpu::compositor_node();
    cc.cursor = true;
    cc.buffers = 3;
    if !dmabufs {
        cc.prefer_linear = true;
        cc.shm_fallback = true;
    }
    Ok(Capturer::start(
        cc,
        Box::new(move |ev| {
            let _ = tx.send(ev);
        }),
    )?)
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
        // Odd windows are cropped to even sides (H.264's 4:2:0), as `read_bgra` does.
        let extent = (extent.0 & !1, extent.1 & !1);
        if extent.0 == 0 || extent.1 == 0 {
            bail!("nothing to share: the window has no size");
        }
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

    fn encode(&mut self, frame: &CapturedFrame, crop: Option<(u32, u32, u32, u32)>, keyframe: bool) -> Result<(Vec<u8>, Option<Vec<u8>>, bool)> {
        let enc = self.enc.as_mut().ok_or_else(|| anyhow!("no encoder"))?;
        if let Some(rect) = crop {
            // An area: cut it out of the frame's pixels, whatever the encoder.
            let img = frame.read_bgra()?;
            let (px, w, _) = crop_bgra(&img.pixels, img.width, img.height, rect);
            return Ok(match enc {
                Enc::Gpu(e) => {
                    let f = e.encode_bgra(&px, keyframe)?;
                    (f.main, f.aux, f.keyframe)
                }
                Enc::Va(e) => {
                    let (au, key) = e.encode_bgra(&px, w * 4, keyframe)?;
                    (au, None, key)
                }
                Enc::Cpu(e) => {
                    let f = e.encode_bgra(&px, keyframe)?;
                    (f.main, f.aux, f.keyframe)
                }
            });
        }
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
    source: &mut watch::Receiver<Source>,
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
    let current = source.borrow_and_update().clone();
    // An area is cut out on the CPU: its capture must be readable there.
    let mut crop = current.crop();
    let mut capturer = start_capture(&current, dmabufs && crop.is_none(), cap_tx.clone())?;

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

    let mut pixels = screen.width as u64 * screen.height as u64;
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
    let mut switched = started;
    let mut frame_id_at_switch = 0u64;
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
                    // Over an area, the pointer is placed in it, and hidden outside.
                    let (x, y, visible) = match crop {
                        Some((cx, cy, cw, ch)) => {
                            let (x, y) = (x - cx as i32, y - cy as i32);
                            (x, y, visible && x >= 0 && y >= 0 && x < cw as i32 && y < ch as i32)
                        }
                        None => (x, y, visible),
                    };
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
            _ = tokio::time::sleep_until((switched + Duration::from_millis(700)).into()), if frame_id == frame_id_at_switch && !rescued => {
                rescued = true;
                capturer.recapture()?;
            }
            _ = stop.changed() => {
                let _ = send(ServerMsg::error(gliff_proto::ERROR_SETUP, "the share was stopped"), vec![]);
                break Ok(());
            }
            changed = source.changed() => {
                if changed.is_err() {
                    continue;
                }
                // Another screen or window: a fresh capture; the encoder follows its size.
                let next = source.borrow_and_update().clone();
                pending = None;
                crop = next.crop();
                drop(std::mem::replace(&mut capturer, match start_capture(&next, dmabufs && crop.is_none(), cap_tx.clone()) {
                    Ok(c) => c,
                    Err(e) => break Err(e),
                }));
                // Events still queued belong to the old capture.
                while cap_rx.try_recv().is_ok() {}
                tracing::info!(source = %next.key(), "share source changed");
                force_key = true;
                frame_id_at_switch = frame_id;
                rescued = false;
                switched = Instant::now();
                capturer.request_frame()?;
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
        let extent = match crop {
            Some((x, y, w, h)) => {
                let (fw, fh) = frame.extent();
                (w.min(fw.saturating_sub(x)), h.min(fh.saturating_sub(y)))
            }
            None => frame.extent(),
        };
        let stream = video.stream_for(extent)?;
        if crop.is_some() && stream != (extent.0 & !1, extent.1 & !1) {
            // An area's pixels go to the encoder as they are: it must take that size.
            break Err(anyhow!("a {}x{} area is larger than the encoder takes", extent.0, extent.1));
        }
        let dual = want_dual && video.gpu.is_some();
        if video.enc.is_none() || video.stream != stream || video.dual != dual {
            if video.stream != stream && video.enc.is_some() {
                // Another source or a resized window: the rate follows the new area.
                pixels = stream.0 as u64 * stream.1 as u64;
                ctl.reconfigure(pixels, if dual { 2 } else { 1 }, MAX_FPS);
            }
            video.rebuild(stream, dual, ctl.stream_bitrate(), fps, ctl.vbv_ms(fps))?;
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
        let (main, aux, keyframe) = video.encode(&frame, crop, force_key)?;
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

#[cfg(test)]
mod tests {
    use super::{crop_bgra, Source};

    #[test]
    fn area_round_trips_and_crops_even() {
        let a = Source::parse("area:DP-1@10,20,301x200").unwrap();
        assert_eq!(a, Source::Area { output: "DP-1".into(), rect: (10, 20, 301, 200) });
        assert_eq!(Source::parse(&a.key()).unwrap(), a);
        assert!(Source::parse("area:DP-1@0,0,4x4").is_err());
        // 4x3 picture, pixel value = its index; cut 3x2 at (1,1): even-sized 2x2.
        let px: Vec<u8> = (0..12u8).flat_map(|i| [i, i, i, 255]).collect();
        let (out, w, h) = crop_bgra(&px, 4, 3, (1, 1, 3, 2));
        assert_eq!((w, h), (2, 2));
        let firsts: Vec<u8> = out.chunks_exact(4).map(|p| p[0]).collect();
        assert_eq!(firsts, [5, 6, 9, 10]);
    }
}
