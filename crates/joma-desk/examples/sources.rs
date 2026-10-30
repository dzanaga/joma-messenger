//! List what can be shared, and the compositor's window handles; with an argument
//! (`screen:X`, `window:0x…`), save its thumbnail as `thumb.ppm`.
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or("info".into())).init();
    for s in joma_desk::share::sources()? {
        println!("{:<40} {}x{} {} · {}{}", s.source.key(), s.screen.width, s.screen.height, s.label, s.detail, if s.focused { " (focused)" } else { "" });
    }
    for t in hypr_capture::list_toplevels(&Default::default())? {
        println!("toplevel {:?} {:?} {:?}", t.identifier, t.app_id, t.title);
    }
    if let Some(arg) = std::env::args().nth(1) {
        let src = joma_desk::share::Source::parse(&arg)?;
        let t = std::time::Instant::now();
        let p = joma_desk::preview::thumbnail(&src, (480, 270))?;
        println!("thumbnail {}x{} in {:?}", p.width, p.height, t.elapsed());
        let mut ppm = format!("P6 {} {} 255\n", p.width, p.height).into_bytes();
        for px in p.bgra.chunks_exact(4) {
            ppm.extend_from_slice(&[px[2], px[1], px[0]]);
        }
        std::fs::write("thumb.ppm", ppm)?;
    }
    Ok(())
}
