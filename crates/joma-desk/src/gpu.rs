//! Which hardware encodes and decodes, and a benchmark to compare the choices.
//!
//! Three tiers, best first:
//! - **gpu**: gliff's Vulkan + VA-API pipeline (4:4:4 split on the GPU). On the
//!   compositor's GPU it reads the captured dmabufs in place; on another GPU it takes the
//!   frames as shared memory and uploads them.
//! - **va**: VA-API H.264 with the colour conversion on the CPU, for GPUs gliff's Vulkan
//!   path refuses (RADV before GFX9, such as Polaris). 4:2:0 only. Experimental and only
//!   with `JOMA_VIDEO=va`: it hung a Polaris GPU (see [`crate::va`]).
//! - **cpu**: gliff-sw (OpenH264).
//!
//! `JOMA_VIDEO=gpu|va|cpu` forces a tier, `GLIFF_RENDER_NODE` a device.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use gliff_vk::Gpu;

use crate::va::{Va, VaDecoder, VaEncoder};

/// The hardware a stream runs on.
#[derive(Clone)]
pub enum Tier {
    Gpu { gpu: Arc<Gpu>, node: PathBuf, compositor: bool },
    Va(Va),
    Cpu,
}

impl Tier {
    pub fn label(&self) -> &'static str {
        match self {
            Tier::Gpu { .. } => "gpu",
            Tier::Va(_) => "va",
            Tier::Cpu => "cpu",
        }
    }

    pub fn node(&self) -> Option<&Path> {
        match self {
            Tier::Gpu { node, .. } => Some(node),
            Tier::Va(va) => Some(&va.node),
            Tier::Cpu => None,
        }
    }

    /// True when captured dmabufs can go to the encoder as they are.
    pub fn takes_dmabufs(&self) -> bool {
        matches!(self, Tier::Gpu { compositor: true, .. })
    }
}

/// Every DRM render node, in order.
pub fn render_nodes() -> Vec<PathBuf> {
    let mut nodes: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("renderD")))
        .collect();
    nodes.sort();
    nodes
}

/// The render node the compositor draws with.
pub fn compositor_node() -> PathBuf {
    hypr_capture::render_node(None)
}

fn forced() -> Option<String> {
    std::env::var("JOMA_VIDEO").ok().filter(|v| !v.is_empty())
}

/// The compositor's node first, then the others (or only `GLIFF_RENDER_NODE`).
fn candidates() -> (PathBuf, Vec<PathBuf>) {
    let comp = compositor_node();
    if std::env::var_os("GLIFF_RENDER_NODE").is_some() {
        return (comp.clone(), vec![comp]);
    }
    let mut nodes = vec![comp.clone()];
    nodes.extend(render_nodes().into_iter().filter(|n| *n != comp));
    (comp, nodes)
}

fn vk(node: &Path) -> Option<Arc<Gpu>> {
    match Gpu::open(Some(node)) {
        Ok(g) => Some(g),
        Err(e) => {
            tracing::debug!(node = %node.display(), error = %e, "no gliff GPU pipeline");
            None
        }
    }
}

/// The best tier for sharing this desktop.
pub fn encode_tier() -> Tier {
    let force = forced();
    let (comp, nodes) = candidates();
    let want = |t: &str| force.as_deref().is_none_or(|f| f == t);
    if want("gpu") {
        for n in &nodes {
            if let Some(gpu) = vk(n).filter(|g| g.can_encode()) {
                return Tier::Gpu { gpu, node: n.clone(), compositor: *n == comp };
            }
        }
    }
    if force.as_deref() == Some("va") {
        for n in &nodes {
            if let Some(va) = Va::open(n).ok().filter(Va::can_encode) {
                return Tier::Va(va);
            }
        }
    }
    Tier::Cpu
}

/// The best tier for watching a share.
pub fn decode_tier() -> Tier {
    let force = forced();
    let (comp, nodes) = candidates();
    let want = |t: &str| force.as_deref().is_none_or(|f| f == t);
    if want("gpu") {
        for n in &nodes {
            if let Some(gpu) = vk(n).filter(|g| g.can_decode()) {
                return Tier::Gpu { gpu, node: n.clone(), compositor: *n == comp };
            }
        }
    }
    if force.as_deref() == Some("va") {
        for n in &nodes {
            if let Some(va) = Va::open(n).ok().filter(Va::can_decode) {
                return Tier::Va(va);
            }
        }
    }
    Tier::Cpu
}

/// What one device offers.
pub struct NodeReport {
    pub node: PathBuf,
    pub compositor: bool,
    pub vulkan: std::result::Result<String, String>,
    pub va: std::result::Result<String, String>,
    pub va_encode: bool,
    pub va_decode: bool,
}

pub fn report() -> Vec<NodeReport> {
    let comp = compositor_node();
    render_nodes()
        .into_iter()
        .map(|node| {
            let vulkan = Gpu::open(Some(&node))
                .map(|g| format!("{} ({}), encode {}", g.name, g.driver, if g.can_encode() { g.encoder_api() } else { "none" }))
                .map_err(|e| e.to_string());
            let va = Va::open(&node);
            let (va_encode, va_decode) = va.as_ref().map(|v| (v.can_encode(), v.can_decode())).unwrap_or((false, false));
            NodeReport {
                compositor: node == comp,
                vulkan,
                va: va.map(|v| v.vendor().to_string()).map_err(|e| e.to_string()),
                va_encode,
                va_decode,
                node,
            }
        })
        .collect()
}

/// One benchmark result: milliseconds per frame.
pub struct Bench {
    pub name: String,
    pub encode_ms: f64,
    pub decode_ms: f64,
    pub kbytes_per_frame: f64,
}

/// A desktop-like test picture: flat panels and lines of "text" that move each frame.
fn picture(w: u32, h: u32, t: u32) -> Vec<u8> {
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let panel = if x < w / 4 { [40, 34, 30] } else { [24, 22, 20] };
            let row = (y + t * 3) % 22;
            let glyph = row > 4 && row < 16 && ((x * 7 + y / 22 * 13 + t) % 11) < 6 && x % 9 != 0;
            let c = if glyph { [200, 210, 160] } else { panel };
            px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    px
}

/// Encode and decode `frames` changing frames of `w` x `h` with each tier available.
///
/// A codec that hangs takes its GPU down, and with it the desktop if that GPU drives the
/// screen; so the compositor's GPU is only tried with `display_gpu`, and the VA tier only
/// with `JOMA_VIDEO=va`.
pub fn bench(w: u32, h: u32, frames: u32, display_gpu: bool) -> Vec<(String, Result<Bench>)> {
    let (w, h) = (w & !1, h & !1);
    let pics: Vec<Vec<u8>> = (0..8).map(|t| picture(w, h, t)).collect();
    let bitrate = (w as u64 * h as u64 * 60 / 10).min(40_000_000) as u32;
    let mut out = Vec::new();
    let comp = compositor_node();
    for node in render_nodes() {
        if node == comp && !display_gpu {
            out.push((format!("{} (drives the screen)", node.display()), Err(anyhow::anyhow!("skipped; --display-gpu tries it"))));
            continue;
        }
        let tag = format!("{}{}", node.file_name().unwrap().to_string_lossy(), if node == comp { ", compositor" } else { "" });
        if let Some(gpu) = vk(&node).filter(|g| g.can_encode() && g.can_decode()) {
            out.push((format!("gpu 4:4:4 on {tag}"), bench_gpu(&gpu, w, h, bitrate, frames, &pics)));
        }
        if forced().as_deref() == Some("va") {
            if let Some(va) = Va::open(&node).ok().filter(|v| v.can_encode() && v.can_decode()) {
                out.push((format!("va 4:2:0 on {tag}"), bench_va(&va, w, h, bitrate, frames, &pics)));
            }
        }
    }
    out.push(("cpu 4:2:0".into(), bench_cpu(w, h, bitrate, frames, &pics)));
    out
}

fn bench_gpu(gpu: &Arc<Gpu>, w: u32, h: u32, bitrate: u32, frames: u32, pics: &[Vec<u8>]) -> Result<Bench> {
    let s = gliff_vk::EncoderSettings { width: w, height: h, bitrate, framerate: 60, vbv_ms: 500 };
    let mut enc = gliff_vk::Encoder::new(gpu, s, true)?;
    let mut dec = gliff_vk::Decoder::new(gpu, true, w, h)?;
    let (mut te, mut td, mut bytes) = (0.0, 0.0, 0usize);
    for i in 0..frames {
        let t = Instant::now();
        let f = enc.encode_bgra(&pics[i as usize % pics.len()], i == 0)?;
        te += t.elapsed().as_secs_f64();
        bytes += f.main.len() + f.aux.as_ref().map_or(0, Vec::len);
        let t = Instant::now();
        dec.decode_to_bgra(&f.main, f.aux.as_deref().unwrap_or(&[]))?;
        td += t.elapsed().as_secs_f64();
    }
    Ok(result("", te, td, bytes, frames))
}

fn bench_va(va: &Va, w: u32, h: u32, bitrate: u32, frames: u32, pics: &[Vec<u8>]) -> Result<Bench> {
    let mut enc = VaEncoder::new(va, w, h, bitrate, 60, 500)?;
    let mut dec = VaDecoder::new(va, w, h)?;
    let (mut te, mut td, mut bytes) = (0.0, 0.0, 0usize);
    for i in 0..frames {
        let t = Instant::now();
        let (au, _) = enc.encode_bgra(&pics[i as usize % pics.len()], (w * 4) as usize, i == 0)?;
        te += t.elapsed().as_secs_f64();
        bytes += au.len();
        let t = Instant::now();
        dec.decode(&au)?;
        td += t.elapsed().as_secs_f64();
    }
    Ok(result("", te, td, bytes, frames))
}

fn bench_cpu(w: u32, h: u32, bitrate: u32, frames: u32, pics: &[Vec<u8>]) -> Result<Bench> {
    let s = gliff_sw::EncoderSettings { width: w, height: h, bitrate, framerate: 60 };
    let mut enc = gliff_sw::Encoder::new(s, false)?;
    let mut dec = gliff_sw::Decoder::new(false, w, h)?;
    let (mut te, mut td, mut bytes) = (0.0, 0.0, 0usize);
    for i in 0..frames {
        let t = Instant::now();
        let f = enc.encode_bgra(&pics[i as usize % pics.len()], i == 0)?;
        te += t.elapsed().as_secs_f64();
        bytes += f.main.len();
        let t = Instant::now();
        dec.decode(&f.main, &[])?;
        td += t.elapsed().as_secs_f64();
    }
    Ok(result("", te, td, bytes, frames))
}

fn result(name: &str, te: f64, td: f64, bytes: usize, frames: u32) -> Bench {
    let n = frames.max(1) as f64;
    Bench { name: name.into(), encode_ms: te * 1000.0 / n, decode_ms: td * 1000.0 / n, kbytes_per_frame: bytes as f64 / 1024.0 / n }
}
