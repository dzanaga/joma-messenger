//! Share the focused output and view it in the same process, without Jami: checks the
//! capture → encode → decode path. `[JOMA_SOURCE=screen:X|window:0x…] loopback [seconds] [out.ppm]`.
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or("info".into())).init();
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let out = std::env::args().nth(2);
    let token = joma_desk::signal::token();
    let source = joma_desk::share::Source::parse(&std::env::var("JOMA_SOURCE").unwrap_or_else(|_| "focused".into()))?;
    let share = joma_desk::share::Share::start(token.clone(), source, |e| println!("share: {e:?}"))?;
    println!("sharing {} {}x{} on port {}", share.screen.name, share.screen.width, share.screen.height, share.port);
    let viewer = joma_desk::view::Viewer::start(share.port, token, || {})?;
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_millis(500));
        let s = viewer.state.lock().unwrap();
        if let Some(e) = &s.ended {
            println!("ended: {e}");
            break;
        }
        if let Some(p) = &s.picture {
            println!("picture #{} {}x{} fps={:.1} local={} remote={} chroma={}", p.serial, p.width, p.height, s.fps,
                s.local_pipeline, s.remote_pipeline, s.chroma);
        }
    }
    if let (Some(path), Some(p)) = (out, viewer.state.lock().unwrap().picture.clone()) {
        let mut ppm = format!("P6\n{} {}\n255\n", p.width, p.height).into_bytes();
        for px in p.bgra.chunks_exact(4) {
            ppm.extend_from_slice(&[px[2], px[1], px[0]]);
        }
        std::fs::write(&path, ppm)?;
        println!("wrote {path}");
    }
    drop(viewer);
    drop(share);
    Ok(())
}
