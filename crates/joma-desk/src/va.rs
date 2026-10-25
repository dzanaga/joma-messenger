//! The VA-API tier: hardware H.264 on GPUs that gliff's Vulkan pipeline cannot use.
//!
//! gliff's GPU tier needs Vulkan dmabuf import with DRM format modifiers, which RADV only
//! offers from GFX9 (Vega) on; older GPUs such as Polaris still have a VA-API H.264
//! encoder and decoder. Here the colour conversion runs on the CPU (gliff-sw's AVX2
//! kernels, BT.709 limited range, the same as every other tier) and the pixels go through
//! the driver with `vaPutImage` / `vaGetImage`. Single 4:2:0 stream only: the 4:4:4 split
//! and recombine live in gliff's Vulkan shaders.
//!
//! **Experimental, opt-in (`JOMA_VIDEO=va`).** On 2026-10-25 the first encode on a Radeon
//! 550 (Polaris, VCE 3.4, Mesa 26.2) hung the VCE ring; the GPU reset failed and took the
//! desktop down with it, because that GPU drives the display. Never try this tier on the
//! GPU that drives the screen without the user's go-ahead.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use gliff_sw::convert::{bgra_to_i420, i420_to_bgra};
use gliff_va::{Caps, Display, EncoderSettings, H264Decoder, H264Encoder};

/// A VA-API display with its capabilities.
#[derive(Clone)]
pub struct Va {
    pub node: PathBuf,
    pub display: Arc<Display>,
    pub caps: Caps,
}

impl Va {
    pub fn open(node: &Path) -> Result<Va> {
        let display = Display::open(node)?;
        let caps = display.caps()?;
        Ok(Va { node: node.to_path_buf(), display, caps })
    }

    pub fn can_encode(&self) -> bool {
        self.caps.can_encode().is_ok()
    }

    pub fn can_decode(&self) -> bool {
        self.caps.can_decode().is_ok()
    }

    pub fn vendor(&self) -> &str {
        &self.display.vendor
    }

    pub fn max_size(&self) -> (u32, u32) {
        H264Encoder::max_coded_extent(&self.caps)
    }
}

/// BGRA in, Annex B H.264 out.
pub struct VaEncoder {
    enc: H264Encoder,
    width: usize,
    height: usize,
    coded: (usize, usize),
    y: Vec<u8>,
    uv: Vec<u8>,
}

impl VaEncoder {
    pub fn new(va: &Va, width: u32, height: u32, bitrate: u32, fps: u32, vbv_ms: u32) -> Result<VaEncoder> {
        let s = EncoderSettings { width, height, bitrate, framerate: fps, vbv_ms };
        let coded = (s.coded_width() as usize, s.coded_height() as usize);
        let enc = H264Encoder::new(&va.display, &va.caps, s)?;
        let (cw, ch) = coded;
        Ok(VaEncoder {
            enc,
            width: width as usize,
            height: height as usize,
            coded,
            y: vec![16; cw * ch],
            uv: vec![128; cw * ch / 2],
        })
    }

    pub fn set_rate(&mut self, bitrate: u32, fps: u32, vbv_ms: u32) {
        self.enc.set_rate(bitrate, fps, vbv_ms);
    }

    /// Encode one frame of `width` x `height` pixels, `stride` bytes per row.
    pub fn encode_bgra(&mut self, bgra: &[u8], stride: usize, keyframe: bool) -> Result<(Vec<u8>, bool)> {
        let (w, h) = (self.width, self.height);
        let i420 = bgra_to_i420(bgra, stride, w, h);
        // The coded size is 16-aligned; the SPS crops the padding away.
        let cw = self.coded.0;
        for r in 0..h {
            self.y[r * cw..r * cw + w].copy_from_slice(&i420.y[r * w..(r + 1) * w]);
        }
        let (hw, hh) = (w / 2, h / 2);
        for r in 0..hh {
            let row = &mut self.uv[r * cw..r * cw + w];
            let (u, v) = (&i420.u[r * hw..(r + 1) * hw], &i420.v[r * hw..(r + 1) * hw]);
            for (i, px) in row.chunks_exact_mut(2).enumerate() {
                px[0] = u[i];
                px[1] = v[i];
            }
        }
        self.enc.input().write_nv12(&self.y, &self.uv)?;
        let pending = self.enc.submit(keyframe)?;
        let packet = self.enc.finish(pending)?;
        Ok((packet.data, packet.keyframe))
    }
}

/// Annex B H.264 in, BGRA out.
pub struct VaDecoder {
    dec: H264Decoder,
}

impl VaDecoder {
    pub fn new(va: &Va, width: u32, height: u32) -> Result<VaDecoder> {
        Ok(VaDecoder { dec: H264Decoder::new(&va.display, &va.caps, width, height)? })
    }

    /// The picture as BGRA with its size, or `None` for an access unit without one.
    pub fn decode(&mut self, au: &[u8]) -> Result<Option<(Vec<u8>, u32, u32)>> {
        let Some(idx) = self.dec.decode(au)? else { return Ok(None) };
        let (w, h) = self.dec.display_size().ok_or_else(|| anyhow!("a picture before its SPS"))?;
        let surface = &self.dec.surfaces()[idx];
        surface.sync()?;
        let (y, uv) = surface.read_nv12()?;
        // The surface is the coded size; rows are tightly packed at that width.
        let cw = y.len() / (h.div_ceil(16) * 16).max(1) as usize;
        let (hw, hh) = (cw / 2, uv.len() / cw.max(1));
        let mut u = vec![0u8; hw * hh];
        let mut v = vec![0u8; hw * hh];
        for (i, px) in uv.chunks_exact(2).enumerate() {
            u[i] = px[0];
            v[i] = px[1];
        }
        let bgra = i420_to_bgra(&y, &u, &v, (cw, hw, hw), w as usize, h as usize);
        Ok(Some((bgra, w, h)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round trip through the device in `JOMA_TEST_VA` (a render node); skipped without
    /// it. A GPU hang here can take the desktop down: never point it at the GPU that
    /// drives the screen (see the module notes).
    #[test]
    fn round_trip_on_hardware() {
        let Some(node) = std::env::var_os("JOMA_TEST_VA") else {
            eprintln!("JOMA_TEST_VA not set; skipped");
            return;
        };
        let va = Va::open(std::path::Path::new(&node)).unwrap();
        let (w, h) = (320u32, 180u32);
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        for (i, px) in bgra.chunks_exact_mut(4).enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            px.copy_from_slice(&[(x * 255 / w) as u8, (y * 255 / h) as u8, 128, 255]);
        }
        let mut enc = VaEncoder::new(&va, w, h, 4_000_000, 30, 500).unwrap();
        let mut dec = VaDecoder::new(&va, w, h).unwrap();
        let mut out = None;
        for i in 0..3 {
            let (au, _) = enc.encode_bgra(&bgra, (w * 4) as usize, i == 0).unwrap();
            out = dec.decode(&au).unwrap().or(out);
        }
        let (px, ow, oh) = out.expect("a picture");
        assert_eq!((ow, oh), (w, h));
        let err: u64 = px.iter().zip(&bgra).enumerate().filter(|(i, _)| i % 4 != 3).map(|(_, (a, b))| (*a as i32 - *b as i32).unsigned_abs() as u64).sum();
        let mean = err as f64 / (w * h * 3) as f64;
        assert!(mean < 6.0, "mean error {mean}");
    }
}
