//! Small pictures of a [`Source`]: a thumbnail for each choice in the picker, and a live
//! preview of what the peer sees while sharing.
//!
//! Both capture into shared memory (the compositor copies the frame out; no encoder, no
//! render node of ours) and shrink the frame on the CPU with a box filter.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use hypr_capture::{CaptureEvent, CaptureMode, Capturer};

use crate::share::Source;
use crate::view::Picture;

/// How long a capture may take before it counts as failed (a hidden window may never
/// complete one).
const FRAME_TIMEOUT: Duration = Duration::from_secs(2);

fn start(source: &Source) -> Result<(Capturer, Receiver<CaptureEvent>)> {
    let mut cc = source.capture_config();
    cc.mode = CaptureMode::Shm;
    cc.cursor = false;
    cc.buffers = 1;
    let (tx, rx) = mpsc::channel();
    let cap = Capturer::start(
        cc,
        Box::new(move |ev| {
            let _ = tx.send(ev);
        }),
    )?;
    Ok((cap, rx))
}

/// The next frame, shrunk to fit `max`.
fn grab(cap: &Capturer, rx: &Receiver<CaptureEvent>, max: (u32, u32), serial: u64) -> Result<Picture> {
    cap.request_frame()?;
    let t = Instant::now();
    let mut rescued = false;
    loop {
        // A still screen may withhold its first frame; ask again with full damage.
        let wait = if rescued { FRAME_TIMEOUT } else { Duration::from_millis(700) };
        match rx.recv_timeout(wait.saturating_sub(t.elapsed()).max(Duration::from_millis(1))) {
            Ok(CaptureEvent::Frame(f)) => {
                let img = f.read_bgra()?;
                return Ok(shrink(&img.pixels, img.width, img.height, max, serial));
            }
            Ok(CaptureEvent::Error(e)) => return Err(anyhow!("capture: {e}")),
            Ok(CaptureEvent::Stopped) => return Err(anyhow!("the screen or window is gone")),
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) if !rescued => {
                rescued = true;
                cap.recapture()?;
            }
            Err(RecvTimeoutError::Timeout) => return Err(anyhow!("no picture came")),
            Err(RecvTimeoutError::Disconnected) => return Err(anyhow!("the capture stopped")),
        }
    }
}

/// One picture of `source`, at most `max` pixels.
pub fn thumbnail(source: &Source, max: (u32, u32)) -> Result<Picture> {
    let (cap, rx) = start(source)?;
    grab(&cap, &rx, max, 1)
}

/// Box-filter `bgra` (tight rows) down to fit `max`, keeping the aspect; opaque alpha.
pub fn shrink(bgra: &[u8], width: usize, height: usize, max: (u32, u32), serial: u64) -> Picture {
    let k = (max.0 as f64 / width.max(1) as f64).min(max.1 as f64 / height.max(1) as f64).min(1.0);
    let (w, h) = (((width as f64 * k).round() as usize).max(1), ((height as f64 * k).round() as usize).max(1));
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let (y0, y1) = (y * height / h, ((y + 1) * height / h).max(y * height / h + 1));
        for x in 0..w {
            let (x0, x1) = (x * width / w, ((x + 1) * width / w).max(x * width / w + 1));
            let mut sum = [0u32; 3];
            for sy in y0..y1 {
                let row = &bgra[(sy * width + x0) * 4..(sy * width + x1) * 4];
                for px in row.chunks_exact(4) {
                    sum[0] += px[0] as u32;
                    sum[1] += px[1] as u32;
                    sum[2] += px[2] as u32;
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = &mut out[(y * w + x) * 4..(y * w + x) * 4 + 4];
            o.copy_from_slice(&[(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, 0xff]);
        }
    }
    Picture { width: w as u32, height: h as u32, bgra: Arc::new(out), serial }
}

/// A live, low-rate picture of what is shared. Dropping it stops the capture.
pub struct Preview {
    pub picture: Arc<Mutex<Option<Picture>>>,
    source: mpsc::Sender<Option<Source>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Preview {
    /// A new picture every `every`, at most `max` pixels; `changed` runs after each.
    pub fn start(source: Source, max: (u32, u32), every: Duration, changed: impl Fn() + Send + 'static) -> Result<Preview> {
        let picture = Arc::new(Mutex::new(None));
        let (tx, rx) = mpsc::channel::<Option<Source>>();
        let pic = picture.clone();
        let thread = std::thread::Builder::new().name("joma-preview".into()).spawn(move || {
            let mut source = source;
            let mut serial = 0u64;
            'restart: loop {
                let (cap, frames) = match start(&source) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::debug!(error = %e, "no preview");
                        // Wait for another source, or the end.
                        match rx.recv() {
                            Ok(Some(s)) => {
                                source = s;
                                continue 'restart;
                            }
                            _ => return,
                        }
                    }
                };
                loop {
                    serial += 1;
                    match grab(&cap, &frames, max, serial) {
                        Ok(p) => *pic.lock().unwrap() = Some(p),
                        Err(e) => tracing::debug!(error = %e, "preview frame failed"),
                    }
                    changed();
                    match rx.recv_timeout(every) {
                        Ok(Some(s)) => {
                            source = s;
                            drop(cap);
                            continue 'restart;
                        }
                        Ok(None) | Err(RecvTimeoutError::Disconnected) => return,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                }
            }
        })?;
        Ok(Preview { picture, source: tx, thread: Some(thread) })
    }

    pub fn set_source(&self, source: Source) {
        let _ = self.source.send(Some(source));
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.source.send(None);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::shrink;

    #[test]
    fn shrink_averages_and_keeps_aspect() {
        // 4x2: left half black, right half white.
        let mut px = Vec::new();
        for _ in 0..2 {
            for x in 0..4 {
                let v = if x < 2 { 0 } else { 255 };
                px.extend_from_slice(&[v, v, v, 0]);
            }
        }
        let p = shrink(&px, 4, 2, (2, 2), 7);
        assert_eq!((p.width, p.height, p.serial), (2, 1, 7));
        assert_eq!(&p.bgra[..], &[0, 0, 0, 255, 255, 255, 255, 255]);
        // Never enlarged.
        let p = shrink(&px, 4, 2, (100, 100), 1);
        assert_eq!((p.width, p.height), (4, 2));
    }
}
