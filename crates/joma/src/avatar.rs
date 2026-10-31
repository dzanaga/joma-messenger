//! Profile pictures: read from the vCards the daemon keeps (Jami sends them with the
//! profile), kept as files for GPUI to show, and drawn as pixel art in Joma.

use std::path::{Path, PathBuf};

/// Standard or URL-safe base64, padding optional.
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=' && !c.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// The picture of a vCard (`PHOTO;ENCODING=BASE64;TYPE=…:<data>`, folded or not).
pub fn vcard_photo(vcard: &str) -> Option<Vec<u8>> {
    let mut lines = vcard.lines().peekable();
    while let Some(line) = lines.next() {
        let upper = line.to_ascii_uppercase();
        if !upper.starts_with("PHOTO") {
            continue;
        }
        let (head, data) = line.split_once(':')?;
        // A URI photo (`VALUE=URI`) is not something we fetch.
        if head.to_ascii_uppercase().contains("VALUE=URI") || data.starts_with("http") {
            return None;
        }
        let mut b64 = data.trim().to_owned();
        // Folded lines continue with a space or a tab.
        while let Some(next) = lines.peek() {
            if next.starts_with(' ') || next.starts_with('\t') {
                b64.push_str(next.trim());
                lines.next();
            } else {
                break;
            }
        }
        let bytes = base64_decode(&b64)?;
        return (bytes.len() > 16).then_some(bytes);
    }
    None
}

fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
}

/// Keep `bytes` as the picture of `uri` in Joma's cache. The name changes with the
/// picture (GPUI caches images by path); the older ones of that uri are removed.
pub fn cache(uri: &str, bytes: &[u8]) -> Option<PathBuf> {
    let dir = std::env::current_dir().ok()?.join("avatars");
    std::fs::create_dir_all(&dir).ok()?;
    let ext = match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png => "png",
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Gif => "gif",
        image::ImageFormat::WebP => "webp",
        image::ImageFormat::Bmp => "bmp",
        _ => return None,
    };
    let prefix = format!("{:016x}-", hash(uri.as_bytes()));
    let path = dir.join(format!("{prefix}{:016x}.{ext}", hash(bytes)));
    if !path.exists() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path)
}

/// The picture in a vCard file (or text), cached for `uri`.
pub fn from_vcard(uri: &str, vcard: &str) -> Option<PathBuf> {
    let text = if !vcard.contains('\n') && Path::new(vcard).is_file() { std::fs::read_to_string(vcard).ok()? } else { vcard.to_owned() };
    cache(uri, &vcard_photo(&text)?)
}

/// A picture made square (centre crop) and `side` pixels wide, as PNG bytes: what goes
/// into the profile (a vCard carries it, so it stays small).
pub fn square_png(path: &Path, side: u32) -> anyhow::Result<Vec<u8>> {
    let img = image::open(path)?;
    let (w, h) = (img.width(), img.height());
    let s = w.min(h);
    let sq = img.crop_imm((w - s) / 2, (h - s) / 2, s, s).resize_exact(side, side, image::imageops::FilterType::Lanczos3);
    let mut out = std::io::Cursor::new(Vec::new());
    sq.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

/// Where this account's own picture is kept (the daemon reads it when the profile is set).
pub fn own_path(account: &str) -> Option<PathBuf> {
    Some(joma_quick::config_path()?.parent()?.join(format!("avatar-{account}.png")))
}

/// The side of a pixel-art avatar, in cells.
pub const GRID: usize = 16;

/// A pixel-art avatar: `GRID`² cells, each a colour (0xRRGGBB) or empty.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PixelArt {
    pub cells: Vec<Option<u32>>,
    /// The colour behind empty cells.
    pub background: u32,
}

impl Default for PixelArt {
    fn default() -> Self {
        PixelArt { cells: vec![None; GRID * GRID], background: 0x1a1b26 }
    }
}

impl PixelArt {
    pub fn get(&self, x: usize, y: usize) -> Option<u32> {
        self.cells.get(y * GRID + x).copied().flatten()
    }

    pub fn set(&mut self, x: usize, y: usize, c: Option<u32>, mirror: bool) {
        if x < GRID && y < GRID {
            self.cells[y * GRID + x] = c;
            if mirror {
                self.cells[y * GRID + (GRID - 1 - x)] = c;
            }
        }
    }

    /// Fill the area of the same colour as (x, y), as a paint bucket does.
    pub fn fill(&mut self, x: usize, y: usize, c: Option<u32>) {
        let from = self.get(x, y);
        if from == c {
            return;
        }
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            if self.get(x, y) != from {
                continue;
            }
            self.cells[y * GRID + x] = c;
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < GRID {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < GRID {
                stack.push((x, y + 1));
            }
        }
    }

    /// The picture, `scale` pixels per cell, as PNG bytes.
    pub fn png(&self, scale: u32) -> Vec<u8> {
        let side = GRID as u32 * scale;
        let img = image::RgbaImage::from_fn(side, side, |x, y| {
            let c = self.get((x / scale) as usize, (y / scale) as usize).unwrap_or(self.background);
            image::Rgba([(c >> 16) as u8, (c >> 8) as u8, c as u8, 255])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        let _ = img.write_to(&mut out, image::ImageFormat::Png);
        out.into_inner()
    }

    fn path(account: &str) -> Option<PathBuf> {
        let base = joma_quick::config_path()?.parent()?.to_path_buf();
        Some(base.join(format!("avatar-{account}.json")))
    }

    /// The drawing kept for an account, to edit it again.
    pub fn load(account: &str) -> Option<PixelArt> {
        let a: PixelArt = serde_json::from_str(&std::fs::read_to_string(Self::path(account)?).ok()?).ok()?;
        (a.cells.len() == GRID * GRID).then_some(a)
    }

    pub fn save(&self, account: &str) {
        if let Some(p) = Self::path(account) {
            let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
            let _ = std::fs::write(p, serde_json::to_string(self).unwrap_or_default());
        }
    }
}

/// The colours offered while drawing: a grid of hues (columns) by lightness (rows), and
/// a row of greys.
pub fn palette() -> Vec<Vec<u32>> {
    const HUES: usize = 12;
    let mut rows = Vec::new();
    for &(s, l) in &[(0.9, 0.82), (0.85, 0.68), (0.8, 0.55), (0.75, 0.42), (0.7, 0.30), (0.6, 0.18)] {
        rows.push((0..HUES).map(|i| hsl(i as f32 / HUES as f32, s, l)).collect());
    }
    rows.push((0..HUES).map(|i| {
        let v = (255.0 * i as f32 / (HUES - 1) as f32).round() as u32;
        (v << 16) | (v << 8) | v
    }).collect());
    rows
}

fn hsl(h: f32, s: f32, l: f32) -> u32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    (to(r) << 16) | (to(g) << 8) | to(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_from_folded_vcard() {
        let png = PixelArt::default().png(1);
        let b64 = base64_encode(&png);
        let (a, b) = b64.split_at(20);
        let card = format!("BEGIN:VCARD\nFN:Ann\nPHOTO;ENCODING=BASE64;TYPE=PNG:{a}\n {b}\nEND:VCARD\n");
        assert_eq!(vcard_photo(&card), Some(png));
        assert_eq!(vcard_photo("BEGIN:VCARD\nFN:Ann\nEND:VCARD"), None);
    }

    fn base64_encode(b: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in b.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                if i <= c.len() {
                    s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }

    #[test]
    fn drawing_fills_mirrors_and_exports() {
        let mut a = PixelArt::default();
        a.set(1, 2, Some(0xff0000), true);
        assert_eq!(a.get(GRID - 2, 2), Some(0xff0000));
        a.fill(0, 0, Some(0x00ff00));
        assert_eq!(a.get(1, 2), Some(0xff0000));
        assert_eq!(a.get(5, 9), Some(0x00ff00));
        let img = image::load_from_memory(&a.png(4)).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (64, 64));
        assert_eq!(img.get_pixel(4, 8).0, [255, 0, 0, 255]);
        assert_eq!(palette().len(), 7);
        assert_eq!(hsl(0.0, 1.0, 0.5), 0xff0000);
    }
}
