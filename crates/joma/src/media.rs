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
    }
}
