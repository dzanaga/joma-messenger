//! The socket the CLI and the Omarchy bar plugin talk to (see `joma_quick::ipc`).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::mpsc;
use std::time::Duration;

use joma_quick::ipc::{Reply, Request};

/// A request waiting for the app's answer.
pub struct Pending {
    pub request: Request,
    pub reply: mpsc::Sender<Reply>,
}

/// Listen on the socket; each request goes to `deliver`. Fails when another Joma runs.
pub fn serve(deliver: impl Fn(Pending) + Send + Sync + 'static) -> anyhow::Result<()> {
    let path = joma_quick::ipc::socket_path().ok_or_else(|| anyhow::anyhow!("XDG_RUNTIME_DIR is not set"))?;
    if std::os::unix::net::UnixStream::connect(&path).is_ok() {
        anyhow::bail!("another Joma is running");
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    let deliver = std::sync::Arc::new(deliver);
    std::thread::Builder::new().name("joma-ipc".into()).spawn(move || {
        for conn in listener.incoming().flatten() {
            // One thread per connection: a request waiting for a password must not hold
            // up the others.
            let deliver = deliver.clone();
            let _ = std::thread::Builder::new().name("joma-ipc-conn".into()).spawn(move || {
                let mut line = String::new();
                let mut reader = BufReader::new(&conn);
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let reply = match serde_json::from_str::<Request>(&line) {
                    Ok(request) => {
                        let wait = if request.slow() { Duration::from_secs(15 * 60) } else { Duration::from_secs(8) };
                        let (tx, rx) = mpsc::channel();
                        deliver(Pending { request, reply: tx });
                        rx.recv_timeout(wait).unwrap_or(Reply::Error { message: "Joma did not answer".into() })
                    }
                    Err(e) => Reply::Error { message: format!("bad request: {e}") },
                };
                let mut conn = conn;
                let _ = conn.write_all(format!("{}\n", serde_json::to_string(&reply).unwrap_or_default()).as_bytes());
            });
        }
    })?;
    Ok(())
}
