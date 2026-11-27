//! Two throwaway accounts in one isolated daemon become contacts, then print the
//! connections each one sees and the path they take (`joma_jami::links`). Never
//! registers a username. Run with an isolated profile only:
//!   d=/tmp/claude-1000/jt/links; XDG_DATA_HOME=$d/data XDG_CONFIG_HOME=$d/config \
//!   XDG_CACHE_HOME=$d/cache cargo run -p joma-jami --example links -- 60
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let home = std::env::var("XDG_DATA_HOME").unwrap_or_default();
    anyhow::ensure!(home.contains("/tmp/"), "run this only with XDG_* pointing at a throwaway profile under /tmp");
    joma_jami::start(joma_jami::flags::NO_LOCAL_MEDIA, |_| {})?;
    std::thread::sleep(Duration::from_secs(1));
    let mut accounts = joma_jami::accounts()?;
    while accounts.len() < 2 {
        accounts.push(joma_jami::create_account(&format!("links-{}", accounts.len()))?);
    }
    let (a, b) = (accounts[0].clone(), accounts[1].clone());
    let (ua, ub) = (joma_jami::account_uri(&a)?, joma_jami::account_uri(&b)?);
    println!("a {ua}\nb {ub}");
    joma_jami::add_contact(&a, &ub)?;
    let secs: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    let until = Instant::now() + Duration::from_secs(secs);
    let mut accepted = false;
    while Instant::now() < until {
        std::thread::sleep(Duration::from_secs(5));
        if !accepted {
            for r in joma_jami::conversation_requests(&b).unwrap_or_default() {
                if let Some(id) = r.get("id") {
                    println!("b accepts request {id}: {:?}", joma_jami::accept_request(&b, id));
                    accepted = true;
                }
            }
        }
        for c in joma_jami::conversations(&a).unwrap_or_default() {
            let _ = joma_jami::send_message(&a, &c, "ping", "");
        }
        println!("   a reg={:?} b reg={:?} b requests={}", joma_jami::volatile_details(&a)?.get("Account.registrationStatus"),
            joma_jami::volatile_details(&b)?.get("Account.registrationStatus"), joma_jami::conversation_requests(&b)?.len());
        let la = joma_jami::links::list(&a, "")?;
        println!("-- after {}s: a sees {} connection(s)", secs - until.saturating_duration_since(Instant::now()).as_secs(), la.len());
        for l in &la {
            println!("   peer {}… device {}… status {} ({}) {} → {:?} channels {:?}",
                &l.peer[..l.peer.len().min(8)], &l.device[..l.device.len().min(8)], l.status,
                joma_jami::links::status_label(&l.status), l.remote_address, l.path, l.channels);
        }
        if la.iter().any(|l| l.status == "0" && !l.remote_address.is_empty()) {
            break;
        }
    }
    joma_jami::stop();
    Ok(())
}
