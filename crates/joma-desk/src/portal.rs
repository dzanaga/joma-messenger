//! The fallback: Jami's own screen share, for peers that do not run Joma.
//!
//! As in the Qt client, the desktop portal (ScreenCast) asks the user what to share and
//! hands back a PipeWire remote; the daemon's `pipewiregrab` reads it from a
//! `display://pipewire pid:… fd:… node:…` source and sends it as the call's video. The
//! daemon runs in this process, so the fd is ours to give.

use std::os::fd::{AsRawFd, OwnedFd};

use anyhow::{anyhow, Context, Result};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use futures::channel::oneshot;

/// A portal screencast session, kept open while the call shares it.
pub struct PortalShare {
    /// The video source to give the call.
    pub resource: String,
    _fd: OwnedFd,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PortalShare {
    /// Ask the portal for a screen or window. Blocks until the user picks one or cancels.
    pub fn start() -> Result<PortalShare> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(OwnedFd, u32)>>();
        let (stop, stop_rx) = oneshot::channel::<()>();
        let thread = std::thread::Builder::new().name("joma-portal".into()).spawn(move || {
            // ashpd runs on async-io here (the backend GPUI selects), so any executor drives it.
            futures::executor::block_on(async move {
                let session = async {
                    let proxy = Screencast::new().await?;
                    let session = proxy.create_session().await?;
                    proxy
                        .select_sources(
                            &session,
                            CursorMode::Embedded,
                            SourceType::Monitor | SourceType::Window,
                            false,
                            None,
                            PersistMode::DoNot,
                        )
                        .await?;
                    let response = proxy.start(&session, None).await?.response()?;
                    let node = response.streams().first().map(|s| s.pipe_wire_node_id()).context("nothing chosen")?;
                    let fd = proxy.open_pipe_wire_remote(&session).await?;
                    anyhow::Ok((proxy, session, fd, node))
                }
                .await;
                match session {
                    Ok((_proxy, session, fd, node)) => {
                        let _ = ready_tx.send(Ok((fd, node)));
                        // Keep the session until the share stops.
                        let _ = stop_rx.await;
                        let _ = session.close().await;
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            });
        })?;
        let (fd, node) = ready_rx.recv().map_err(|_| anyhow!("the portal thread stopped"))??;
        let resource = format!("display://pipewire pid:{} fd:{} node:{}", std::process::id(), fd.as_raw_fd(), node);
        // The daemon duplicates the fd when it opens the source; ours stays open as long
        // as the share, in case the daemon reopens the source.
        Ok(PortalShare { resource, _fd: fd, stop: Some(stop), thread: Some(thread) })
    }
}

impl Drop for PortalShare {
    fn drop(&mut self) {
        if let Some(s) = self.stop.take() {
            let _ = s.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
