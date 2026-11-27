//! Media around messages: voice and video messages (played with mpv), YouTube links
//! (found in text, searched with yt-dlp, previewed with their thumbnail and title).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// A file a voice or video message plays from.
pub fn is_audio(path: &Path) -> bool {
    ext_in(path, &["ogg", "opus", "oga", "m4a", "mp3", "wav", "flac", "aac"])
}

pub fn is_video(path: &Path) -> bool {
    ext_in(path, &["webm", "mp4", "mkv", "mov", "avi"])
}

fn ext_in(path: &Path, list: &[&str]) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| list.contains(&e.to_ascii_lowercase().as_str()))
}

/// Play a file with mpv: sound only for audio, a window for video.
pub fn play(path: &Path) -> std::io::Result<Child> {
    let mut c = Command::new("mpv");
    c.arg("--really-quiet").arg("--force-window=no");
    if is_audio(path) {
        c.arg("--no-video");
    } else {
        c.arg("--force-window=yes").arg("--title=Joma");
    }
    c.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
}

/// The video id of the first YouTube link in a text.
pub fn youtube_id(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        let w = word.trim_matches(|c: char| c == '<' || c == '>' || c == '(' || c == ')');
        let rest = w.strip_prefix("https://").or_else(|| w.strip_prefix("http://")).unwrap_or(w);
        let rest = rest.strip_prefix("www.").or_else(|| rest.strip_prefix("m.")).unwrap_or(rest);
        let id = if let Some(r) = rest.strip_prefix("youtu.be/") {
            r.split(['?', '&', '#', '/']).next()
        } else if let Some(r) = rest.strip_prefix("youtube.com/watch?") {
            r.split('&').find_map(|kv| kv.strip_prefix("v="))
        } else if let Some(r) = rest.strip_prefix("youtube.com/shorts/") {
            r.split(['?', '&', '#', '/']).next()
        } else {
            None
        };
        if let Some(id) = id.filter(|i| i.len() == 11 && i.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) {
            return Some(id.to_owned());
        }
    }
    None
}

pub fn watch_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// One search result.
#[derive(Clone, Debug, Default)]
pub struct Video {
    pub id: String,
    pub title: String,
    pub channel: String,
    /// Seconds; 0 when live or unknown.
    pub duration: u64,
}

/// Search YouTube with yt-dlp (no account, no API key). Blocks for a few seconds.
pub fn search(query: &str, n: usize) -> anyhow::Result<Vec<Video>> {
    let out = Command::new("yt-dlp")
        .args(["--flat-playlist", "-J", "--no-warnings"])
        .arg(format!("ytsearch{n}:{query}"))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| anyhow::anyhow!("yt-dlp is not installed ({e})"))?;
    if !out.status.success() {
        anyhow::bail!("yt-dlp failed: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or(""));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    Ok(v["entries"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    Some(Video {
                        id: e["id"].as_str()?.to_owned(),
                        title: e["title"].as_str().unwrap_or_default().to_owned(),
                        channel: e["channel"].as_str().or(e["uploader"].as_str()).unwrap_or_default().to_owned(),
                        duration: e["duration"].as_f64().unwrap_or(0.) as u64,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

fn cache_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_default().join("youtube")
}

/// The thumbnail and title of a video, fetched once and kept in Joma's cache.
#[derive(Clone, Debug, Default)]
pub struct Preview {
    pub thumb: Option<PathBuf>,
    pub title: String,
    pub channel: String,
}

/// What is cached for `id` already (nothing is fetched).
pub fn cached(id: &str) -> Option<Preview> {
    let dir = cache_dir();
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{id}.json"))).ok()?).ok()?;
    let thumb = dir.join(format!("{id}.jpg"));
    Some(Preview {
        thumb: thumb.is_file().then_some(thumb),
        title: meta["title"].as_str().unwrap_or_default().to_owned(),
        channel: meta["author_name"].as_str().unwrap_or_default().to_owned(),
    })
}

/// Fetch the title (YouTube's oEmbed) and the thumbnail of `id` into the cache. Blocks.
pub fn fetch(id: &str) -> anyhow::Result<Preview> {
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)?;
    let curl = |url: &str, to: &Path| -> anyhow::Result<()> {
        let ok = Command::new("curl").args(["-fsSL", "--max-time", "10", "-o"]).arg(to).arg(url).status()?.success();
        if !ok {
            anyhow::bail!("could not fetch {url}");
        }
        Ok(())
    };
    let meta = dir.join(format!("{id}.json"));
    curl(&format!("https://www.youtube.com/oembed?format=json&url={}", watch_url(id)), &meta)?;
    let _ = curl(&format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"), &dir.join(format!("{id}.jpg")));
    cached(id).ok_or_else(|| anyhow::anyhow!("no preview"))
}

pub fn duration_label(secs: u64) -> String {
    if secs == 0 {
        return String::new();
    }
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_youtube_ids() {
        assert_eq!(youtube_id("look https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=3"), Some("dQw4w9WgXcQ".into()));
        assert_eq!(youtube_id("https://youtu.be/dQw4w9WgXcQ?si=x"), Some("dQw4w9WgXcQ".into()));
        assert_eq!(youtube_id("youtube.com/shorts/abcdefghijk"), Some("abcdefghijk".into()));
        assert_eq!(youtube_id("https://example.com/watch?v=dQw4w9WgXcQ"), None);
        assert_eq!(youtube_id("no link"), None);
        assert_eq!(duration_label(3725), "1:02:05");
        assert!(is_audio(Path::new("a/voice.OGG")) && is_video(Path::new("v.webm")));
        assert!(is_audio(Path::new("audio_20261127_173803.m4a")));
        assert!(offline_mode(&serde_json::json!({"processing": "offline"})));
        assert!(offline_mode(&serde_json::json!({"processing": "Offline Lite"})));
        assert!(!offline_mode(&serde_json::json!({"processing": "cloud"})));
        assert!(!offline_mode(&serde_json::json!({})));
        let st = serde_json::json!({"last": {"session_id": 3, "text": " ciao ", "warning": null}});
        assert_eq!(session_done(&st, 3).unwrap().unwrap(), "ciao");
        assert!(session_done(&st, 4).is_none());
        let quiet = serde_json::json!({"last": {"session_id": 1, "text": "", "warning": "No speech detected"}});
        assert_eq!(session_done(&quiet, 1).unwrap().unwrap_err().to_string(), "no speech detected");
        assert_eq!(superwhisper_result(r#"{"text":"hi"}"#).unwrap(), "hi");
    }
}

// ---- Transcription of voice messages -------------------------------------------------

/// Is `program` on the PATH?
pub fn installed(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// The engine to use: the setting, or by itself Superwhisper when it is installed and
/// set to work offline (nothing more to install), else whisper.cpp when it is here.
/// `None`: off. A cloud mode is used only when chosen in the settings.
pub fn transcriber(setting: &str) -> Option<&'static str> {
    match setting {
        "off" => None,
        "superwhisper" => installed("superwhisper").then_some("superwhisper"),
        "whisper" => installed("whisper-cli").then_some("whisper"),
        _ if superwhisper_offline() => Some("superwhisper"),
        _ => installed("whisper-cli").then_some("whisper"),
    }
}

/// Superwhisper is installed and transcribes on this machine (checked once a minute).
pub fn superwhisper_offline() -> bool {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static SEEN: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    if let Some((at, v)) = *SEEN.lock().unwrap() {
        if at.elapsed() < Duration::from_secs(60) {
            return v;
        }
    }
    let v = installed("superwhisper")
        && Command::new("superwhisper")
            .args(["models", "show"])
            .stderr(Stdio::null())
            .output()
            .ok()
            .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
            .is_some_and(|j| offline_mode(&j));
    *SEEN.lock().unwrap() = Some((Instant::now(), v));
    v
}

/// `superwhisper models show` says it processes offline ("offline", "offline-lite"…).
fn offline_mode(j: &serde_json::Value) -> bool {
    j.get("processing").and_then(|p| p.as_str()).is_some_and(|p| p.to_ascii_lowercase().starts_with("offline"))
}

/// whisper.cpp's multilingual `base` model (about 150 MB), kept with Joma's data.
pub fn whisper_model() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    data.join("joma/models/ggml-base.bin")
}

pub const WHISPER_MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";

/// Download the model (once, when the user asks for a first transcription).
pub fn fetch_whisper_model() -> anyhow::Result<()> {
    let path = whisper_model();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = path.with_extension("part");
    let out = Command::new("curl").args(["-fsSL", "--max-time", "900", "-o"]).arg(&part).arg(WHISPER_MODEL_URL).output()?;
    anyhow::ensure!(out.status.success(), "download failed: {}", String::from_utf8_lossy(&out.stderr).trim().to_owned());
    std::fs::rename(part, path)?;
    Ok(())
}

/// Superwhisper transcribes in the background: `transcribe` answers
/// `{"ok":true,"session_id":N}` and `status` later shows that session under `last`.
fn superwhisper_result(answer: &str) -> anyhow::Result<String> {
    let v: serde_json::Value = serde_json::from_str(answer.trim()).map_err(|_| anyhow::anyhow!("superwhisper: {}", answer.trim()))?;
    if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
        return Ok(t.to_owned());
    }
    anyhow::ensure!(v.get("ok").and_then(|o| o.as_bool()) == Some(true), "superwhisper: {}", answer.trim());
    let session = v.get("session_id").and_then(|s| s.as_i64()).ok_or_else(|| anyhow::anyhow!("superwhisper: no session"))?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(600);
    while std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let Ok(out) = Command::new("superwhisper").arg("status").stderr(Stdio::null()).output() else { continue };
        let Ok(st) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { continue };
        if let Some(r) = session_done(&st, session) {
            return r;
        }
    }
    anyhow::bail!("superwhisper took more than ten minutes")
}

/// The result of `session` in a `superwhisper status`, once it is done.
fn session_done(status: &serde_json::Value, session: i64) -> Option<anyhow::Result<String>> {
    let last = status.get("last")?;
    if last.get("session_id")?.as_i64()? != session {
        return None;
    }
    let text = last.get("text").and_then(|t| t.as_str()).unwrap_or("").trim().to_owned();
    if !text.is_empty() {
        return Some(Ok(text));
    }
    // No text and no warning yet: still working.
    let warn = last.get("warning").or_else(|| last.get("error")).and_then(|w| w.as_str())?;
    Some(Err(anyhow::anyhow!("{}", warn.to_lowercase())))
}

fn transcript_path(file_id: &str) -> Option<PathBuf> {
    let safe: String = file_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join("joma/transcripts").join(format!("{safe}.txt")))
}

/// A transcript made earlier.
pub fn cached_transcript(file_id: &str) -> Option<String> {
    std::fs::read_to_string(transcript_path(file_id)?).ok()
}

/// Transcribe a voice message with `engine` and keep the text in Joma's cache (it is
/// not sent anywhere by Joma; "superwhisper" works in that app's own mode).
pub fn transcribe(engine: &str, file_id: &str, audio: &Path) -> anyhow::Result<String> {
    let text = match engine {
        "superwhisper" => {
            let out = Command::new("superwhisper").arg("transcribe").arg(audio).output()?;
            anyhow::ensure!(out.status.success(), "superwhisper: {}", String::from_utf8_lossy(&out.stderr).trim().to_owned());
            superwhisper_result(&String::from_utf8_lossy(&out.stdout))?
        }
        _ => {
            // whisper.cpp reads 16 kHz mono WAV.
            let wav = std::env::temp_dir().join(format!("joma-transcribe-{}.wav", std::process::id()));
            let conv = Command::new("ffmpeg").args(["-y", "-loglevel", "error", "-i"]).arg(audio).args(["-ar", "16000", "-ac", "1", "-f", "wav"]).arg(&wav).output()?;
            anyhow::ensure!(conv.status.success(), "ffmpeg: {}", String::from_utf8_lossy(&conv.stderr).trim().to_owned());
            let out = Command::new("whisper-cli").arg("-m").arg(whisper_model()).arg("-f").arg(&wav).args(["-l", "auto", "-nt", "-np"]).output();
            let _ = std::fs::remove_file(&wav);
            let out = out?;
            anyhow::ensure!(out.status.success(), "whisper-cli: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("failed").to_owned());
            String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join(" ")
        }
    };
    anyhow::ensure!(!text.is_empty(), "nothing was heard");
    if let Some(p) = transcript_path(file_id) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, &text);
    }
    Ok(text)
}
