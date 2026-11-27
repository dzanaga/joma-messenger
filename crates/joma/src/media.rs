//! Media around messages: voice and video messages (played with mpv), YouTube links
//! (found in text, searched with yt-dlp, previewed with their thumbnail and title).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
        let st = serde_json::json!({"last": {"session_id": 3, "text": " ciao ", "warning": null, "silence_removal": {"segments": [[16000, 48000]]}}});
        let h = session_done(&st, 3).unwrap().unwrap();
        assert_eq!((h.text.as_str(), h.speech), ("ciao", vec![(1., 3.)]));
        assert!(session_done(&st, 4).is_none());
        let quiet = serde_json::json!({"last": {"session_id": 1, "text": "", "warning": "No speech detected"}});
        assert_eq!(session_done(&quiet, 1).unwrap().err().unwrap().to_string(), "no speech detected");
        assert_eq!(superwhisper_result(r#"{"text":"hi"}"#).unwrap().text, "hi");
        assert!(session_done(&serde_json::json!({"last": {"session_id": 1, "text": ""}}), 1).is_none());
    }

    #[test]
    fn subtitles() {
        // Two stretches: the words go to each in proportion, lines break at sentences.
        let c = spread("Dunque, questa è la prova. Di un cane", &[(1.75, 6.7), (9.0, 12.0)], 12.);
        assert_eq!(c.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), vec!["Dunque, questa è la prova.", "Di un cane"]);
        assert!((c[0].start - 1.75).abs() < 1e-9);
        // The second line is in the second stretch, and the first ends before it.
        assert!(c[1].start >= 9.0 && c[1].end <= 12.7 && c[0].end <= c[1].start);
        assert_eq!(cue_at(&c, 2.0), Some("Dunque, questa è la prova."));
        assert_eq!(cue_at(&c, 0.5), None);
        // No stretches: the whole length; long text in lines of seven words.
        let long = spread("a b c d e f g h i", &[], 9.);
        assert_eq!(long.len(), 2);
        assert_eq!(long[0].text, "a b c d e f g");
        assert!(long.iter().all(|c| c.end <= 9.6));
        assert!(spread("", &[], 3.).is_empty());
        let w = whisper_lines("[00:00:00.000 --> 00:00:02.500]   Ciao a tutti.\n[00:01:02.000 --> 00:01:03.000]  Sì\n");
        assert_eq!(w, vec![Cue { start: 0., end: 2.5, text: "Ciao a tutti.".into() }, Cue { start: 62., end: 63., text: "Sì".into() }]);
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

/// One line of subtitles: shown from `start` to `end` (seconds).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// What an engine heard: the text, and the stretches (seconds) where someone speaks or
/// the timed lines it gave.
struct Heard {
    text: String,
    speech: Vec<(f64, f64)>,
    cues: Vec<Cue>,
}

/// Superwhisper transcribes in the background: `transcribe` answers
/// `{"ok":true,"session_id":N}` and `status` later shows that session under `last`,
/// with the stretches its voice detection found (`silence_removal.segments`, 16 kHz samples).
fn superwhisper_result(answer: &str) -> anyhow::Result<Heard> {
    let v: serde_json::Value = serde_json::from_str(answer.trim()).map_err(|_| anyhow::anyhow!("superwhisper: {}", answer.trim()))?;
    if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
        return Ok(Heard { text: t.to_owned(), speech: Vec::new(), cues: Vec::new() });
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
fn session_done(status: &serde_json::Value, session: i64) -> Option<anyhow::Result<Heard>> {
    let last = status.get("last")?;
    if last.get("session_id")?.as_i64()? != session {
        return None;
    }
    let text = last.get("text").and_then(|t| t.as_str()).unwrap_or("").trim().to_owned();
    if !text.is_empty() {
        let speech = last
            .pointer("/silence_removal/segments")
            .and_then(|s| s.as_array())
            .map(|l| l.iter().filter_map(|r| Some((r.get(0)?.as_f64()? / 16000., r.get(1)?.as_f64()? / 16000.))).collect())
            .unwrap_or_default();
        return Some(Ok(Heard { text, speech, cues: Vec::new() }));
    }
    // No text and no warning yet: still working.
    let warn = last.get("warning").or_else(|| last.get("error")).and_then(|w| w.as_str())?;
    Some(Err(anyhow::anyhow!("{}", warn.to_lowercase())))
}

/// whisper.cpp's lines: `[00:00:01.000 --> 00:00:03.500]  text`.
fn whisper_lines(out: &str) -> Vec<Cue> {
    let secs = |t: &str| -> Option<f64> {
        let mut p = t.trim().split(':').rev();
        let s: f64 = p.next()?.parse().ok()?;
        let m: f64 = p.next().unwrap_or("0").parse().ok()?;
        let h: f64 = p.next().unwrap_or("0").parse().ok()?;
        Some(h * 3600. + m * 60. + s)
    };
    out.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix('[')?;
            let (times, text) = rest.split_once(']')?;
            let (a, b) = times.split_once("-->")?;
            let text = text.trim();
            (!text.is_empty()).then(|| Cue { start: secs(a).unwrap_or(0.), end: secs(b).unwrap_or(0.), text: text.to_owned() })
        })
        .collect()
}

/// Lines of at most this many words.
const CUE_WORDS: usize = 7;

/// Subtitles from a text and where the speech is: the words are spread over the speech
/// at an even pace (no engine here gives each word's time), cut into short lines at the
/// ends of sentences and of stretches. Without stretches: over the whole `duration`.
pub fn spread(text: &str, speech: &[(f64, f64)], duration: f64) -> Vec<Cue> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut parts: Vec<(f64, f64)> = speech.iter().copied().filter(|(a, b)| b > a).collect();
    if parts.is_empty() {
        parts.push((0., duration.max(0.5)));
    }
    let total: f64 = parts.iter().map(|(a, b)| b - a).sum();
    // The time and stretch of the speech `x` seconds in.
    let at = |x: f64| -> (f64, usize) {
        let mut left = x;
        for (i, (a, b)) in parts.iter().enumerate() {
            if left <= b - a || i == parts.len() - 1 {
                return (a + left.min(b - a), i);
            }
            left -= b - a;
        }
        (0., 0)
    };
    let n = words.len().max(1) as f64;
    let mut cues: Vec<Cue> = Vec::new();
    let mut line: Vec<&str> = Vec::new();
    let mut line_start = (0., 0usize);
    for (i, w) in words.iter().enumerate() {
        let here = at(total * i as f64 / n);
        if line.is_empty() {
            line_start = here;
        } else if here.1 != line_start.1 {
            // A new stretch of speech starts a new line.
            let end = parts[line_start.1].1;
            cues.push(Cue { start: line_start.0, end, text: line.join(" ") });
            line.clear();
            line_start = here;
        }
        line.push(w);
        let sentence_end = w.ends_with(['.', '?', '!', ':', ';']);
        if line.len() >= CUE_WORDS || sentence_end || i + 1 == words.len() {
            let next = at(total * (i + 1) as f64 / n);
            let end = if next.1 == line_start.1 { next.0 } else { parts[line_start.1].1 };
            cues.push(Cue { start: line_start.0, end, text: line.join(" ") });
            line.clear();
        }
    }
    // Each line stays a little longer, until the next one.
    for i in 0..cues.len() {
        let next = cues.get(i + 1).map_or(f64::MAX, |c| c.start);
        cues[i].end = (cues[i].end + 0.6).min(next).max(cues[i].start + 0.3);
    }
    cues
}

/// The line to show at `pos` seconds.
pub fn cue_at(cues: &[Cue], pos: f64) -> Option<&str> {
    cues.iter().find(|c| pos >= c.start && pos < c.end).map(|c| c.text.as_str())
}

fn cache_path(file_id: &str, ext: &str) -> Option<PathBuf> {
    let safe: String = file_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join("joma/transcripts").join(format!("{safe}.{ext}")))
}

/// A transcript made earlier.
pub fn cached_transcript(file_id: &str) -> Option<String> {
    std::fs::read_to_string(cache_path(file_id, "txt")?).ok()
}

/// Its subtitles, made with it.
pub fn cached_cues(file_id: &str) -> Option<Vec<Cue>> {
    serde_json::from_str(&std::fs::read_to_string(cache_path(file_id, "cues.json")?).ok()?).ok()
}

/// Transcribe a voice or video message with `engine` and keep the text and its
/// subtitles in Joma's cache (Joma sends them nowhere; "superwhisper" works in that
/// app's own mode).
pub fn transcribe(engine: &str, file_id: &str, media: &Path) -> anyhow::Result<String> {
    // Both engines get 16 kHz mono WAV: the sound of a video too.
    let wav = std::env::temp_dir().join(format!("joma-transcribe-{}-{}.wav", std::process::id(), file_id.len()));
    let conv = Command::new("ffmpeg").args(["-y", "-loglevel", "error", "-i"]).arg(media).args(["-vn", "-ar", "16000", "-ac", "1", "-f", "wav"]).arg(&wav).output()?;
    anyhow::ensure!(conv.status.success(), "ffmpeg: {}", String::from_utf8_lossy(&conv.stderr).trim().to_owned());
    let duration = std::fs::metadata(&wav).map(|m| m.len().saturating_sub(44) as f64 / 32000.).unwrap_or(0.);
    let heard = (|| -> anyhow::Result<Heard> {
        match engine {
            "superwhisper" => {
                let out = Command::new("superwhisper").arg("transcribe").arg(&wav).output()?;
                anyhow::ensure!(out.status.success(), "superwhisper: {}", String::from_utf8_lossy(&out.stderr).trim().to_owned());
                superwhisper_result(&String::from_utf8_lossy(&out.stdout))
            }
            _ => {
                let out = Command::new("whisper-cli").arg("-m").arg(whisper_model()).arg("-f").arg(&wav).args(["-l", "auto", "-np"]).output()?;
                anyhow::ensure!(out.status.success(), "whisper-cli: {}", String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("failed").to_owned());
                let cues = whisper_lines(&String::from_utf8_lossy(&out.stdout));
                let text = cues.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join(" ");
                Ok(Heard { text, speech: Vec::new(), cues })
            }
        }
    })();
    let _ = std::fs::remove_file(&wav);
    let heard = heard?;
    let text = heard.text.split_whitespace().collect::<Vec<_>>().join(" ");
    anyhow::ensure!(!text.is_empty(), "nothing was heard");
    // whisper.cpp's own lines, cut shorter; else spread over the speech.
    let cues = if heard.cues.is_empty() {
        spread(&text, &heard.speech, duration)
    } else {
        heard.cues.iter().flat_map(|c| spread(&c.text, &[(c.start, c.end)], c.end)).collect()
    };
    if let Some(p) = cache_path(file_id, "txt") {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, &text);
    }
    if let Some(p) = cache_path(file_id, "cues.json") {
        let _ = std::fs::write(p, serde_json::to_string(&cues).unwrap_or_default());
    }
    Ok(text)
}

