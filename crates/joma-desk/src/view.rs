//! The viewing side: connect to a sharer through the Jami tunnel, decode its stream and
//! keep the newest picture for the UI.
//!
//! Decoding is gliff's client pipeline: two VA-API H.264 decoders and the `gliff-vk`
//! recombine shader rebuild full 4:4:4 colour, or `gliff-sw` (OpenH264) on the CPU. The UI
//! here is GPUI, which takes CPU images, so each picture is read back once as BGRA.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use gliff_proto::{ChromaMode, ClientCaps, ClientMsg, Codec, ServerMsg, PROTOCOL_VERSION};
use gliff_transport::Framed;
use crate::gpu::Tier;
use crate::va::VaDecoder;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::watch;

/// One decoded picture, BGRA with opaque alpha, `width * 4` bytes per row.
#[derive(Clone)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub bgra: Arc<Vec<u8>>,
    /// Increases with every new picture.
    pub serial: u64,
}

/// The sharer's pointer, in stream pixels. Its image is premultiplied BGRA.
#[derive(Clone, Default)]
pub struct Pointer {
    pub x: f64,
    pub y: f64,
    pub visible: bool,
    pub width: u32,
    pub height: u32,
    pub hot_x: i32,
    pub hot_y: i32,
    pub bgra: Arc<Vec<u8>>,
    pub shape_serial: u64,
}

#[derive(Clone, Default)]
pub struct ViewState {
    pub picture: Option<Picture>,
    pub pointer: Pointer,
    /// `gpu`, `va` or `cpu` here; `gpu` or `cpu` for the sharer (gliff's wire labels).
    pub local_pipeline: &'static str,
    pub remote_pipeline: &'static str,
    pub chroma: &'static str,
    pub fps: f32,
    pub ended: Option<String>,
    /// What we drew on the shared screen (the sharer's overlay shows it too).
    pub ink: crate::ink::Canvas,
}

/// A running viewer. Dropping it disconnects.
pub struct Viewer {
    pub state: Arc<Mutex<ViewState>>,
    ink: tokio::sync::mpsc::UnboundedSender<crate::ink::Ink>,
    stop: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Viewer {
    /// Connect to the tunnel on `127.0.0.1:port`. `changed` runs on the viewer thread
    /// after each new picture or state change.
    pub fn start(port: u16, token: String, changed: impl Fn() + Send + Sync + 'static) -> Result<Viewer> {
        let state = Arc::new(Mutex::new(ViewState::default()));
        let (stop, stop_rx) = watch::channel(false);
        let (ink, ink_rx) = tokio::sync::mpsc::unbounded_channel();
        let st = state.clone();
        let thread = std::thread::Builder::new().name("joma-view".into()).spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            let changed = Arc::new(changed);
            let (st2, tok2, ch2) = (st.clone(), token.clone(), changed.clone());
            rt.spawn(async move {
                if let Err(e) = ink_channel(port, tok2, ink_rx, st2, move || ch2()).await {
                    tracing::debug!(error = %e, "ink channel ended");
                }
            });
            let r = rt.block_on(run(port, token, st.clone(), stop_rx, &|| changed()));
            st.lock().unwrap().ended = Some(match r {
                Ok(()) => "The share ended".into(),
                Err(e) => format!("{e:#}"),
            });
            changed();
        })?;
        Ok(Viewer { state, ink, stop, thread: Some(thread) })
    }

    /// Draw or point on the shared screen: shown here at once, and sent to the sharer.
    pub fn ink(&self, msg: crate::ink::Ink) {
        self.state.lock().unwrap().ink.apply(&msg);
        let _ = self.ink.send(msg);
    }
}

/// The drawing channel to the sharer: our ink out, its `clear` back.
async fn ink_channel(
    port: u16,
    token: String,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<crate::ink::Ink>,
    state: Arc<Mutex<ViewState>>,
    changed: impl Fn(),
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    // Connect on the first drawing only: a viewer that never draws opens nothing more.
    let Some(first) = rx.recv().await else { return Ok(()) };
    let mut sock = connect(port).await?;
    sock.set_nodelay(true)?;
    sock.write_all(format!("{token}I").as_bytes()).await?;
    let (rd, mut wr) = sock.into_split();
    let mut lines = BufReader::new(rd).lines();
    let mut pending = Some(first);
    loop {
        if let Some(msg) = pending.take() {
            let mut text = serde_json::to_string(&msg)?;
            text.push('\n');
            wr.write_all(text.as_bytes()).await?;
        }
        tokio::select! {
            msg = rx.recv() => match msg {
                Some(m) => pending = Some(m),
                None => return Ok(()),
            },
            line = lines.next_line() => {
                let Some(line) = line? else { return Ok(()) };
                if let Ok(msg) = serde_json::from_str::<crate::ink::Ink>(&line) {
                    state.lock().unwrap().ink.apply(&msg);
                    changed();
                }
            }
        }
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

enum Dec {
    Gpu(Box<gliff_vk::Decoder>),
    Va(Box<VaDecoder>),
    Cpu(Box<gliff_sw::Decoder>),
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Make every alpha byte opaque: the GPU path writes BGRX.
fn opaque(bgra: &mut [u8]) {
    for px in bgra.chunks_exact_mut(4) {
        px[3] = 0xff;
    }
}

async fn connect(port: u16) -> Result<TcpStream> {
    // The tunnel listener accepts at once but opens the peer channel lazily; retry a
    // refused connect briefly in case the listener is not up yet.
    let mut last = None;
    for _ in 0..20 {
        match TcpStream::connect(("127.0.0.1", port)).await {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(anyhow!("cannot reach the share tunnel: {}", last.map(|e| e.to_string()).unwrap_or_default()))
}

async fn run(
    port: u16,
    token: String,
    state: Arc<Mutex<ViewState>>,
    mut stop: watch::Receiver<bool>,
    changed: &impl Fn(),
) -> Result<()> {
    let mut sock = connect(port).await?;
    sock.set_nodelay(true)?;
    sock.write_all(format!("{token}\n").as_bytes()).await?;
    let (rd, wr) = sock.into_split();
    let mut rx = Framed::new(rd);
    let mut tx = Framed::new(wr);

    let tier = crate::gpu::decode_tier();
    tracing::info!(tier = tier.label(), node = ?tier.node(), "watching");
    let chroma = if matches!(tier, Tier::Gpu { .. }) {
        vec![ChromaMode::Dual420, ChromaMode::Single420]
    } else if matches!(tier, Tier::Va(_)) {
        // Rebuilding 4:4:4 needs gliff's Vulkan shaders.
        vec![ChromaMode::Single420]
    } else {
        // The CPU decoder recombines Dual420 too, but one stream halves its work.
        vec![ChromaMode::Single420, ChromaMode::Dual420]
    };
    let caps = ClientCaps {
        codecs: vec![Codec::H264],
        max_width: 8192,
        max_height: 8192,
        chroma,
        features: gliff_proto::features(),
    };
    tx.write_msg(&ClientMsg::Hello { version: PROTOCOL_VERSION, keymap: String::new(), caps }).await?;
    state.lock().unwrap().local_pipeline = tier.label();

    let mut dec: Option<Dec> = None;
    let mut size = (0u32, 0u32);
    let mut serial = 0u64;
    let mut shape_serial = 0u64;
    let mut window = (std::time::Instant::now(), 0u32);
    loop {
        let msg = tokio::select! {
            m = rx.read_msg::<ServerMsg>() => m,
            _ = stop.changed() => {
                let _ = tx.write_msg(&ClientMsg::Bye).await;
                return Ok(());
            }
        };
        let msg = match msg {
            Ok(m) => m,
            Err(gliff_transport::Error::Closed) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        match msg {
            ServerMsg::HelloAck { version, .. } if version != PROTOCOL_VERSION => {
                bail!("the sharer speaks protocol {version}")
            }
            ServerMsg::HelloAck { .. } => {}
            ServerMsg::Ping { t } => tx.write_msg(&ClientMsg::Pong { t }).await?,
            ServerMsg::Error { message, .. } => bail!("{message}"),
            ServerMsg::StreamConfig { codec, chroma, pipeline, width, height, .. } => {
                if codec != Codec::H264 {
                    bail!("unsupported codec {codec:?}");
                }
                let dual = chroma == ChromaMode::Dual420;
                size = (width, height);
                let hw = match &tier {
                    Tier::Gpu { gpu, .. } => gliff_vk::Decoder::new(gpu, dual, width, height).map(|d| Dec::Gpu(Box::new(d))).map_err(|e| anyhow!(e)),
                    Tier::Va(va) if !dual => VaDecoder::new(va, width, height).map(|d| Dec::Va(Box::new(d))),
                    _ => Err(anyhow!("no hardware decoder")),
                };
                dec = Some(match hw {
                    Ok(d) => d,
                    Err(e) => {
                        if !matches!(tier, Tier::Cpu) {
                            tracing::warn!(error = %e, "hardware decoder failed; decoding on the CPU");
                        }
                        Dec::Cpu(Box::new(gliff_sw::Decoder::new(dual, width, height)?))
                    }
                });
                let mut s = state.lock().unwrap();
                s.remote_pipeline = pipeline.map(|p| p.label()).unwrap_or("?");
                s.chroma = if dual { "4:4:4" } else { "4:2:0" };
                s.local_pipeline = match dec {
                    Some(Dec::Gpu(_)) => "gpu",
                    Some(Dec::Va(_)) => "va",
                    _ => "cpu",
                };
            }
            ServerMsg::VideoFrame { frame_id, data_len, aux_len, .. } => {
                let main = rx.read_payload(data_len).await?;
                let aux = if aux_len > 0 { rx.read_payload(aux_len).await? } else { Default::default() };
                let Some(d) = dec.as_mut() else { continue };
                let decoded = match d {
                    Dec::Gpu(d) => d.decode_to_bgra(&main, &aux).map(|o| o.map(|p| (p, size.0, size.1))).map_err(|e| anyhow!(e)),
                    Dec::Va(d) => d.decode(&main),
                    Dec::Cpu(d) => d.decode(&main, &aux).map(|o| o.map(|f| (f.pixels, f.width, f.height))).map_err(|e| anyhow!(e)),
                };
                match decoded {
                    Ok(Some((mut px, width, height))) => {
                        opaque(&mut px);
                        serial += 1;
                        let mut s = state.lock().unwrap();
                        s.picture = Some(Picture { width, height, bgra: Arc::new(px), serial });
                        window.1 += 1;
                        let el = window.0.elapsed();
                        if el >= Duration::from_secs(1) {
                            s.fps = window.1 as f32 / el.as_secs_f32();
                            window = (std::time::Instant::now(), 0);
                        }
                        drop(s);
                        changed();
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "decode failed; asking for a keyframe");
                        tx.write_msg(&ClientMsg::RequestKeyframe).await?;
                    }
                }
                tx.write_msg(&ClientMsg::FrameAck { frame_id, decoded_at_ms: now_ms() }).await?;
            }
            ServerMsg::CursorShape { width, height, hot_x, hot_y, argb_len, .. } => {
                let argb = rx.read_payload(argb_len).await?;
                shape_serial += 1;
                let mut s = state.lock().unwrap();
                let p = &mut s.pointer;
                (p.width, p.height, p.hot_x, p.hot_y) = (width, height, hot_x, hot_y);
                p.bgra = Arc::new(argb.to_vec());
                p.shape_serial = shape_serial;
                drop(s);
                changed();
            }
            ServerMsg::CursorPos { x, y, visible, .. } => {
                let mut s = state.lock().unwrap();
                (s.pointer.x, s.pointer.y, s.pointer.visible) = (x, y, visible);
                drop(s);
                changed();
            }
            _ => {}
        }
    }
}
