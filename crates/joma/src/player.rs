//! Voice and video messages played inside Joma (D-26): the sound through mpv (no
//! window), the pictures decoded by ffmpeg into BGRA frames that the chat draws. Both
//! follow one clock: the moment playing started plus the position it started from.
//! Pausing stops both; playing again (or seeking) starts both from the position.
//!
//! Also what a message shows before it plays: its length, the bars of its sound (the
//! Jami app's waveform), and for a video its size and a still picture.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// Bars in a voice message's waveform.
pub const BARS: usize = 40;

/// Frames are decoded at most this big (the longer side) and this often.
const MAX_SIDE: u32 = 720;
const FPS: f64 = 30.;

/// What a voice or video message is.
#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    /// Seconds.
    pub duration: f64,
    /// Width and height as shown (turned like the phone held it).
    pub video: Option<(u32, u32)>,
    pub audio: bool,
}

/// What `ffprobe` says about a file.
pub fn probe(path: &Path) -> Result<Info> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,width,height:stream_side_data=rotation:stream_tags=rotate:format=duration", "-of", "json"])
        .arg(path)
        .output()
        .context("ffprobe")?;
    anyhow::ensure!(out.status.success(), "ffprobe: {}", String::from_utf8_lossy(&out.stderr).trim().to_owned());
    parse_probe(&serde_json::from_slice(&out.stdout)?)
}

fn parse_probe(v: &serde_json::Value) -> Result<Info> {
    let duration = v.pointer("/format/duration").and_then(|d| d.as_str()).and_then(|d| d.parse().ok()).unwrap_or(0.);
    let streams = v.get("streams").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let kind = |s: &serde_json::Value, k: &str| s.get("codec_type").and_then(|c| c.as_str()) == Some(k);
    let audio = streams.iter().any(|s| kind(s, "audio"));
    let video = streams.iter().find(|s| kind(s, "video") && s.get("width").is_some()).and_then(|s| {
        let (w, h) = (s.get("width")?.as_u64()? as u32, s.get("height")?.as_u64()? as u32);
        let rotation = s
            .get("side_data_list")
            .and_then(|l| l.as_array())
            .and_then(|l| l.iter().find_map(|d| d.get("rotation").and_then(|r| r.as_f64())))
            .or_else(|| s.pointer("/tags/rotate").and_then(|r| r.as_str()).and_then(|r| r.parse().ok()))
            .unwrap_or(0.);
        let turned = (rotation.abs() as i64 / 90) % 2 == 1;
        Some(if turned { (h, w) } else { (w, h) })
    });
    // A still image in an audio file (cover art) is not a video.
    let video = video.filter(|_| duration > 0.);
    Ok(Info { duration, video, audio })
}

/// The bars of the sound: `BARS` values between 0 and 1.
pub fn waveform(path: &Path) -> Result<Vec<f32>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "4000", "-f", "s16le", "-"])
        .stdin(Stdio::null())
        .output()
        .context("ffmpeg")?;
    anyhow::ensure!(out.status.success(), "ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim().to_owned());
    let samples: Vec<f32> = out.stdout.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.).collect();
    Ok(bars(&samples, BARS))
}

/// Loudness (RMS) of `n` equal parts of `samples`, scaled so the loudest is 1.
pub fn bars(samples: &[f32], n: usize) -> Vec<f32> {
    if samples.is_empty() || n == 0 {
        return vec![0.; n];
    }
    let mut v: Vec<f32> = (0..n)
        .map(|i| {
            let part = &samples[i * samples.len() / n..((i + 1) * samples.len() / n).max(i * samples.len() / n + 1).min(samples.len())];
            (part.iter().map(|s| s * s).sum::<f32>() / part.len().max(1) as f32).sqrt()
        })
        .collect();
    let max = v.iter().cloned().fold(0., f32::max);
    if max > 0. {
        // Square root: quiet speech still shows.
        v.iter_mut().for_each(|x| *x = (*x / max).sqrt());
    }
    v
}

/// A still picture of a video (kept in Joma's cache), from about its first half second.
pub fn thumbnail(file_id: &str, path: &Path, info: &Info) -> Result<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache"))).context("no cache dir")?;
    let safe: String = file_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let out = cache.join("joma/thumbs").join(format!("{safe}.jpg"));
    if out.is_file() {
        return Ok(out);
    }
    std::fs::create_dir_all(out.parent().unwrap())?;
    let at = (info.duration / 4.).min(0.5);
    let (w, h) = decode_size(info.video.unwrap_or((MAX_SIDE, MAX_SIDE)));
    let r = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-ss", &format!("{at:.2}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={w}:{h}"), "-q:v", "4"])
        .arg(&out)
        .stdin(Stdio::null())
        .output()?;
    anyhow::ensure!(r.status.success() && out.is_file(), "ffmpeg: {}", String::from_utf8_lossy(&r.stderr).trim().to_owned());
    Ok(out)
}

/// Even sizes no bigger than `MAX_SIDE`, keeping the shape.
fn decode_size((w, h): (u32, u32)) -> (u32, u32) {
    let k = (MAX_SIDE as f64 / w.max(h).max(1) as f64).min(1.);
    let even = |x: f64| ((x / 2.).round() as u32).max(1) * 2;
    (even(w as f64 * k), even(h as f64 * k))
}

/// The latest picture, for the chat to draw.
#[derive(Default)]
pub struct Picture {
    pub serial: u64,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// One message being played (or paused at a position).
pub struct Player {
    pub file_id: String,
    pub path: PathBuf,
    pub info: Info,
    /// Where it was when it last started or paused.
    offset: f64,
    /// When it started playing; `None`: paused.
    started: Option<Instant>,
    sound: Option<Child>,
    decoder: Option<Child>,
    stop: Arc<AtomicBool>,
    pub picture: Arc<Mutex<Picture>>,
}

impl Player {
    pub fn new(file_id: &str, path: &Path) -> Result<Player> {
        let info = probe(path)?;
        Ok(Player {
            file_id: file_id.to_owned(),
            path: path.to_owned(),
            info,
            offset: 0.,
            started: None,
            sound: None,
            decoder: None,
            stop: Arc::new(AtomicBool::new(false)),
            picture: Arc::new(Mutex::new(Picture::default())),
        })
    }

    pub fn playing(&self) -> bool {
        self.started.is_some()
    }

    /// Seconds from the start.
    pub fn position(&self) -> f64 {
        let p = self.offset + self.started.map_or(0., |s| s.elapsed().as_secs_f64());
        if self.info.duration > 0. { p.min(self.info.duration) } else { p }
    }

    /// Played to the end (the sound ended, or the clock passed the length).
    pub fn ended(&mut self) -> bool {
        if self.started.is_none() {
            return false;
        }
        let sound_done = self.sound.as_mut().is_some_and(|c| !matches!(c.try_wait(), Ok(None)));
        let raw = self.offset + self.started.map_or(0., |s| s.elapsed().as_secs_f64());
        sound_done || (self.info.duration > 0. && raw >= self.info.duration + 0.3)
    }

    pub fn play(&mut self) -> Result<()> {
        if self.started.is_some() {
            return Ok(());
        }
        if self.info.duration > 0. && self.offset >= self.info.duration - 0.05 {
            self.offset = 0.;
        }
        let at = format!("{:.3}", self.offset);
        if self.info.audio {
            self.sound = Some(
                Command::new("mpv")
                    .args(["--really-quiet", "--no-video", "--force-window=no", "--no-terminal", &format!("--start={at}")])
                    // A test profile never plays on the real speakers.
                    .args(std::env::var_os("JOMA_PROFILE").map(|_| "--ao=null"))
                    .arg(&self.path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .context("mpv")?,
            );
        }
        let start = Instant::now();
        if let Some(size) = self.info.video {
            let (w, h) = decode_size(size);
            let mut child = Command::new("ffmpeg")
                .args(["-v", "error", "-ss", &at, "-i"])
                .arg(&self.path)
                .args(["-an", "-vf", &format!("fps={FPS},scale={w}:{h}"), "-pix_fmt", "bgra", "-f", "rawvideo", "-"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .context("ffmpeg")?;
            let mut out = child.stdout.take().context("ffmpeg output")?;
            self.stop = Arc::new(AtomicBool::new(false));
            let (stop, picture) = (self.stop.clone(), self.picture.clone());
            std::thread::spawn(move || {
                let mut buf = vec![0u8; (w * h * 4) as usize];
                let mut n = 0u64;
                while !stop.load(Ordering::Relaxed) && out.read_exact(&mut buf).is_ok() {
                    // Shown when its time comes on the shared clock.
                    let due = start + Duration::from_secs_f64(n as f64 / FPS);
                    if let Some(wait) = due.checked_duration_since(Instant::now()) {
                        std::thread::sleep(wait);
                    } else if Instant::now().duration_since(due) > Duration::from_millis(100) {
                        // Late: skip this one.
                        n += 1;
                        continue;
                    }
                    let mut p = picture.lock().unwrap();
                    p.serial += 1;
                    (p.width, p.height) = (w, h);
                    std::mem::swap(&mut p.bgra, &mut buf);
                    if buf.len() != (w * h * 4) as usize {
                        buf = vec![0u8; (w * h * 4) as usize];
                    }
                    n += 1;
                }
            });
            self.decoder = Some(child);
        }
        self.started = Some(start);
        Ok(())
    }

    pub fn pause(&mut self) {
        self.offset = self.position();
        self.started = None;
        self.stop.store(true, Ordering::Relaxed);
        for c in [self.sound.take(), self.decoder.take()].into_iter().flatten() {
            let mut c = c;
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Go to `pos` seconds, playing on if it was playing.
    pub fn seek(&mut self, pos: f64) -> Result<()> {
        let was = self.playing();
        self.pause();
        self.offset = pos.clamp(0., self.info.duration.max(0.));
        if was { self.play() } else { Ok(()) }
    }

    /// Back to the start, stopped.
    pub fn rewind(&mut self) {
        self.pause();
        self.offset = 0.;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.pause();
    }
}

/// `m:ss`.
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_probe_and_turns_portrait_video() {
        let v = serde_json::json!({"streams": [
            {"codec_type": "video", "width": 1280, "height": 720, "side_data_list": [{"rotation": 90}]},
            {"codec_type": "audio"}], "format": {"duration": "1.83"}});
        assert_eq!(parse_probe(&v).unwrap(), Info { duration: 1.83, video: Some((720, 1280)), audio: true });
        let a = serde_json::json!({"streams": [{"codec_type": "audio"}], "format": {"duration": "12"}});
        assert_eq!(parse_probe(&a).unwrap(), Info { duration: 12., video: None, audio: true });
    }

    #[test]
    fn bars_and_sizes() {
        let mut s = vec![0.; 100];
        s[90..].iter_mut().for_each(|x| *x = 0.5);
        let b = bars(&s, 10);
        assert_eq!(b.len(), 10);
        assert_eq!(b[9], 1.);
        assert_eq!(b[0], 0.);
        assert_eq!(bars(&[], 4), vec![0.; 4]);
        assert_eq!(decode_size((1280, 720)), (720, 406));
        assert_eq!(decode_size((320, 240)), (320, 240));
        assert_eq!(clock(72.4), "1:12");
    }

    /// A real file: a tone made by ffmpeg plays, pauses, seeks and ends.
    #[test]
    fn plays_a_file() {
        let dir = std::env::temp_dir().join(format!("joma-player-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("v.mp4");
        let ok = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=160x120:rate=30:duration=1", "-f", "lavfi", "-i", "sine=duration=1", "-shortest"])
            .arg(&f)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return;
        }
        assert_eq!(waveform(&f).unwrap().len(), BARS);
        let mut p = Player::new("x", &f).unwrap();
        assert_eq!(p.info.video, Some((160, 120)));
        // No sound in tests: only the pictures.
        p.info.audio = false;
        p.play().unwrap();
        std::thread::sleep(Duration::from_millis(400));
        let serial = p.picture.lock().unwrap().serial;
        assert!(serial > 3, "frames: {serial}");
        assert_eq!(p.picture.lock().unwrap().bgra.len(), 160 * 120 * 4);
        p.pause();
        let at = p.position();
        assert!(at > 0.3 && at < 0.7, "{at}");
        p.seek(0.9).unwrap();
        assert!(!p.playing());
        p.play().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(p.ended());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
