//! Start the daemon in the current XDG dirs, make sure one account exists, print signals.
//! Run with an isolated profile: XDG_DATA_HOME=... XDG_CONFIG_HOME=... XDG_CACHE_HOME=...
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    joma_jami::start(joma_jami::flags::NO_LOCAL_MEDIA, |e| match e {
        joma_jami::Event::Other { .. } => {}
        e => println!("event: {e:?}"),
    })?;
    std::thread::sleep(Duration::from_secs(1));
    let mut accounts = joma_jami::accounts()?;
    if accounts.is_empty() {
        let name = std::env::args().nth(1).unwrap_or_else(|| "joma-smoke".into());
        accounts.push(joma_jami::create_account(&name)?);
    }
    for a in &accounts {
        println!("account {a} uri={} device={}", joma_jami::account_uri(a)?, joma_jami::device_id(a)?);
    }
    let secs = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(15);
    std::thread::sleep(Duration::from_secs(secs));
    joma_jami::stop();
    Ok(())
}
