//! Joma: a Jami client for Omarchy.
//!
//! `joma` opens the window (or raises the running one). The other commands talk to the
//! running Joma: `joma quick` opens the quick-message popup, `joma quick --json` prints
//! what the Omarchy bar plugin shows, `joma send <to> <text>` sends a message.
//!
//! `JOMA_PROFILE=<dir>` runs Joma on its own Jami data under `<dir>` (tests, a second
//! identity); without it Joma uses Jami's own data, shared with the Qt client.

mod app;
mod composer;
mod desk;
mod ipc;
mod model;
mod popup;
mod render;
mod theme;

use gpui::{px, size, App, AppContext, Application, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions};
use joma_quick::ipc::{request, Reply, Request};

use app::*;

const USAGE: &str = "usage: joma                      open Joma (or raise it)
       joma quick                quick-message popup
       joma quick --json         quick-message targets, as JSON
       joma send <to> <text…>    send a message (to: a conversation, a name or an ID)
       joma send-quick <to> <n>  send quick message n (1-9)
       joma --version";

fn cli(args: &[String]) -> Option<i32> {
    let req = match args.first().map(String::as_str) {
        None => return None,
        Some("--version" | "-V") => {
            println!("joma {}", env!("CARGO_PKG_VERSION"));
            return Some(0);
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return Some(0);
        }
        Some("quick") if args.get(1).map(String::as_str) == Some("--json") => Request::Quick,
        Some("quick") => Request::QuickPopup,
        Some("show") => Request::Show,
        Some("send") if args.len() >= 3 => Request::Send { to: args[1].clone(), text: args[2..].join(" ") },
        Some("send-quick") if args.len() == 3 => match args[2].parse::<usize>() {
            Ok(n) if n >= 1 => Request::SendQuick { to: args[1].clone(), index: n - 1 },
            _ => {
                eprintln!("joma: quick messages are numbered from 1");
                return Some(2);
            }
        },
        Some(_) => {
            eprintln!("{USAGE}");
            return Some(2);
        }
    };
    let json = matches!(req, Request::Quick);
    match request(&req) {
        Ok(Reply::Quick(state)) if json => {
            println!("{}", serde_json::to_string(&state).unwrap_or_default());
            Some(0)
        }
        Ok(Reply::Error { message }) => {
            eprintln!("joma: {message}");
            Some(1)
        }
        Ok(_) => Some(0),
        Err(e) => {
            if json {
                // The bar plugin reads this to show Joma as not running.
                println!("{{\"running\":false}}");
                return Some(0);
            }
            eprintln!("joma: {e}");
            Some(1)
        }
    }
}

/// Point the XDG dirs at a separate profile, before anything reads them.
fn use_profile() {
    if let Some(dir) = std::env::var_os("JOMA_PROFILE").map(std::path::PathBuf::from) {
        for (var, sub) in [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"), ("XDG_CACHE_HOME", "cache")] {
            let p = dir.join(sub);
            let _ = std::fs::create_dir_all(&p);
            // SAFETY: single-threaded here, before any other thread starts.
            unsafe { std::env::set_var(var, p) };
        }
    }
}

fn main() {
    use_profile();
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn,joma=info,joma_desk=info".into()))
        .with_writer(std::io::stderr)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli(&args) {
        std::process::exit(code);
    }
    // One Joma at a time: raise the running one.
    if request(&Request::Show).is_ok() {
        return;
    }
    let (tx, rx) = futures::channel::mpsc::unbounded::<AppMsg>();
    let ipc_tx = tx.clone();
    if let Err(e) = ipc::serve(move |p| {
        let _ = ipc_tx.unbounded_send(AppMsg::Ipc(p));
    }) {
        eprintln!("joma: {e}");
    }
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("alt-j", NextConversation, None),
            KeyBinding::new("alt-k", PrevConversation, None),
            KeyBinding::new("alt-down", NextConversation, None),
            KeyBinding::new("alt-up", PrevConversation, None),
            KeyBinding::new("ctrl-tab", NextConversation, None),
            KeyBinding::new("ctrl-shift-tab", PrevConversation, None),
            KeyBinding::new("ctrl-n", NewConversation, None),
            KeyBinding::new("ctrl-k", Search, None),
            KeyBinding::new("ctrl-f", Search, None),
            KeyBinding::new("escape", Cancel, None),
            KeyBinding::new("ctrl-shift-c", CallAudio, None),
            KeyBinding::new("ctrl-shift-v", CallVideo, None),
            KeyBinding::new("ctrl-shift-a", AcceptCall, None),
            KeyBinding::new("ctrl-shift-e", HangUp, None),
            KeyBinding::new("ctrl-shift-m", ToggleMute, None),
            KeyBinding::new("ctrl-shift-o", ToggleCamera, None),
            KeyBinding::new("ctrl-shift-s", ShareDesktop, None),
            KeyBinding::new("ctrl-shift-f", ToggleFavorite, None),
            KeyBinding::new("f11", FocusStage, None),
            KeyBinding::new("ctrl-enter", AcceptRequest, None),
            KeyBinding::new("ctrl-backspace", DeclineRequest, None),
            KeyBinding::new("ctrl-.", ToggleQuick, None),
            KeyBinding::new("alt-1", Quick1, None),
            KeyBinding::new("alt-2", Quick2, None),
            KeyBinding::new("alt-3", Quick3, None),
            KeyBinding::new("alt-4", Quick4, None),
            KeyBinding::new("alt-5", Quick5, None),
            KeyBinding::new("alt-6", Quick6, None),
            KeyBinding::new("alt-7", Quick7, None),
            KeyBinding::new("alt-8", Quick8, None),
            KeyBinding::new("alt-9", Quick9, None),
        ]);
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions { title: Some("Joma".into()), ..Default::default() }),
            app_id: Some("joma".into()),
            ..Default::default()
        };
        let mut rx = Some(rx);
        let tx = tx.clone();
        cx.open_window(options, move |window, cx| {
            let rx = rx.take().expect("one window");
            cx.new(|cx| Joma::new(tx.clone(), rx, window, cx))
        })
        .expect("failed to open the window");
        cx.activate(true);
    });
    joma_jami::stop();
}
