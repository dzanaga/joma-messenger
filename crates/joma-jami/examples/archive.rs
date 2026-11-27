//! Export an account's archive, remove the account, import it back: the Jami id must
//! be the same. Run only with XDG_* pointing at a throwaway profile under /tmp.
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let home = std::env::var("XDG_DATA_HOME").unwrap_or_default();
    anyhow::ensure!(home.contains("/tmp/"), "run this only with a throwaway profile under /tmp");
    joma_jami::start(joma_jami::flags::NO_LOCAL_MEDIA, |_| {})?;
    std::thread::sleep(Duration::from_secs(1));
    let a = joma_jami::create_account("archive-test")?;
    let until = Instant::now() + Duration::from_secs(30);
    let mut uri = String::new();
    while uri.is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(500));
        uri = joma_jami::account_uri(&a).unwrap_or_default();
    }
    println!("created {a} uri {uri}");
    let path = format!("{home}/archive.gz");
    joma_jami::export_archive(&a, &path)?;
    println!("exported {} bytes", std::fs::metadata(&path)?.len());
    joma_jami::remove_account(&a)?;
    std::thread::sleep(Duration::from_secs(2));
    let b = joma_jami::import_archive(&path)?;
    let until = Instant::now() + Duration::from_secs(30);
    let mut uri2 = String::new();
    while uri2.is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(500));
        uri2 = joma_jami::account_uri(&b).unwrap_or_default();
    }
    println!("imported {b} uri {uri2}");
    println!("{}", if !uri.is_empty() && uri == uri2 { "SAME ID" } else { "DIFFERENT" });
    joma_jami::stop();
    Ok(())
}
