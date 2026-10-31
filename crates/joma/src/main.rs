//! Joma: a Jami client for Omarchy.
//!
//! `joma` opens the window (or raises the running one). Closing the window hides it and
//! Joma keeps running in the background (see `hypr`); Ctrl+Q quits. The other commands talk to the
//! running Joma: `joma quick --json` prints what the Omarchy bar plugin shows,
//! `joma send <to> <text>` sends a message.
//!
//! `JOMA_PROFILE=<dir>` runs Joma on its own Jami data under `<dir>` (tests, a second
//! identity); without it Joma uses Jami's own data, shared with the Qt client.

mod app;
mod avatar;
mod composer;
mod desk;
mod hypr;
mod ipc;
mod model;
mod presence;
mod setup;
mod stage;
mod tray;
mod render;
mod theme;

use gpui::{px, size, App, AppContext, Application, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowOptions};
use joma_quick::ipc::{request, Reply, Request};

use app::*;

const USAGE: &str = "usage: joma [--hidden]           open Joma (or raise it); --hidden starts in the tray
       joma toggle               show Joma, or hide it when it has the focus
       joma quick --json         the bar widget's contacts and quick messages, as JSON
       joma send <to> <text…>    send a message (to: a conversation, a name or an ID)
       joma send-quick <to> <n>  send quick message n (1-9)
       joma chat <to> [n]        the last n messages (default 20), as JSON; marks them read
       joma call <to> [--video]  call a conversation
       joma share <to> [--source screen:<output>|window:<address>|focused]
                                 share the desktop (in the running call, else without a
                                 call); without --source Joma asks what to share
       joma watch <to>           watch the desktop <to> offers you
       joma send-file <to> [path]
                                 send a file (without a path, Joma asks for one)
       joma open <to>            show a conversation in Joma
       joma avatar <file>|--remove
                                 set your profile picture (cropped square), or remove it
       joma link                 on a new Joma: join your account from another device;
                                 prints the code to give that device
       joma link-device <code>   send your account to the new device showing <code>
       joma accounts             this machine's accounts (* the open one)
       joma account <name>       open another account (a name, username or ID)
       joma block <to>         block a contact: no messages, calls or requests from them
       joma unblock <to>         unblock a contact
       joma quit                 quit Joma (closing the window keeps it running)
       joma probe [--bench [--display-gpu]]
                                 the GPUs and codecs the desktop share can use
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
        Some("probe") => {
            let has = |f: &str| args[1..].iter().any(|a| a == f);
            return Some(probe(has("--bench"), has("--display-gpu")));
        }
        Some("quick") if args.get(1).map(String::as_str) == Some("--json") => Request::Quick,
        Some("show") => Request::Show,
        Some("toggle") => Request::Toggle,
        Some("quit") => Request::Quit,
        Some("chat") if args.len() >= 2 => Request::Chat { to: args[1].clone(), limit: args.get(2).and_then(|n| n.parse().ok()).unwrap_or(20) },
        Some("call") if args.len() >= 2 => Request::Call { to: args[1].clone(), video: args[2..].iter().any(|a| a == "--video") },
        Some("share") if args.len() == 2 || (args.len() == 4 && args[2] == "--source") => {
            Request::ShareScreen { to: args[1].clone(), source: args.get(3).cloned() }
        }
        Some("send-file") if args.len() >= 2 => Request::SendFile {
            to: args[1].clone(),
            // The running Joma has its own working directory.
            path: args.get(2).map(|p| std::fs::canonicalize(p).map_or_else(|_| p.clone(), |c| c.to_string_lossy().into_owned())),
        },
        Some("watch") if args.len() == 2 => Request::Watch { to: args[1].clone() },
        Some("open") if args.len() == 2 => Request::Open { to: args[1].clone() },
        Some("link") if args.len() == 1 => Request::Link,
        Some("avatar") if args.len() == 2 => Request::Avatar {
            path: (args[1] != "--remove").then(|| std::fs::canonicalize(&args[1]).map_or_else(|_| args[1].clone(), |c| c.to_string_lossy().into_owned())),
        },
        Some("accounts") if args.len() == 1 => Request::Accounts,
        Some("account") if args.len() == 2 => Request::UseAccount { to: args[1].clone() },
        Some("link-device") if args.len() == 2 => Request::LinkDevice { token: args[1].clone() },
        Some(b @ ("block" | "unblock")) if args.len() == 2 => Request::Block { to: args[1].clone(), block: b == "block" },
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
        Ok(Reply::Chat(chat)) => {
            println!("{}", serde_json::to_string(&chat).unwrap_or_default());
            Some(0)
        }
        Ok(Reply::Text { text }) => {
            println!("{text}");
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

/// Print what each GPU offers the desktop share, the tiers Joma would pick, and with
/// `--bench` how fast each tier encodes and decodes a desktop-sized stream. The benchmark
/// leaves the GPU that drives the screen alone unless `--display-gpu` is given: a codec
/// that hangs resets its GPU, and the desktop with it.
fn probe(bench: bool, display_gpu: bool) -> i32 {
    use joma_desk::gpu;
    let _ = tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::new("error")).try_init();
    for r in gpu::report() {
        println!("{}{}", r.node.display(), if r.compositor { "  (the compositor's GPU)" } else { "" });
        match &r.vulkan {
            Ok(v) => println!("  gliff GPU pipeline: yes, {v}"),
            Err(e) => println!("  gliff GPU pipeline: no ({e})"),
        }
        match &r.va {
            Ok(v) => println!("  VA-API: {v}; H.264 encode {}, decode {}", yes(r.va_encode), yes(r.va_decode)),
            Err(e) => println!("  VA-API: no ({e})"),
        }
    }
    let at = |t: &gpu::Tier| t.node().map(|n| format!(" on {}", n.display())).unwrap_or_default();
    let (e, d) = (gpu::encode_tier(), gpu::decode_tier());
    println!("sharing uses: {}{}", e.label(), at(&e));
    println!("watching uses: {}{}", d.label(), at(&d));
    if !bench {
        return 0;
    }
    let (w, h) = joma_desk::share::focused_screen().map(|s| (s.width, s.height)).unwrap_or((1920, 1080));
    println!("benchmark: {w}x{h}, 120 changing frames");
    for (name, r) in gpu::bench(w, h, 120, display_gpu) {
        match r {
            Ok(b) => println!(
                "  {name:<36} encode {:6.1} ms ({:5.0} fps)  decode {:6.1} ms  {:6.1} KiB/frame",
                b.encode_ms, 1000.0 / b.encode_ms.max(0.001), b.decode_ms, b.kbytes_per_frame
            ),
            Err(e) => println!("  {name:<36} failed: {e:#}"),
        }
    }
    0
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

fn main() {
    use_profile();
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn,joma=info,joma_desk=info".into()))
        .with_writer(std::io::stderr)
        .init();
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let hidden = args.iter().any(|a| a == "--hidden") || std::env::var_os("JOMA_HIDDEN").is_some();
    args.retain(|a| a != "--hidden");
    if let Some(code) = cli(&args) {
        std::process::exit(code);
    }
    // One Joma at a time: raise the running one (at login, leave it be).
    if request(&if hidden { Request::Quick } else { Request::Show }).is_ok() {
        return;
    }
    // Started from a shell without a desktop (over SSH, a TTY): say how to start it.
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        eprintln!(
            "joma: no desktop in this shell (over SSH?). Start Joma from the desktop, or through \
             your session: systemd-run --user --unit=joma {}",
            setup::exe().display()
        );
        std::process::exit(1);
    }
    // Jami writes some files of a device link into the current directory: keep them in
    // Joma's cache instead of wherever Joma was started.
    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cache")))
    {
        let dir = cache.join("joma");
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::env::set_current_dir(dir);
        }
    }
    setup::refresh();
    let hidden = hidden && hypr::start_hidden();
    hypr::START_HIDDEN.store(hidden, std::sync::atomic::Ordering::SeqCst);
    // Started in the tray, the window opens as the panel; else as a full window.
    hypr::set_shape(if hidden { hypr::Shape::Compact } else { hypr::Shape::Full });
    if !hidden {
        hypr::float_at_start();
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
            KeyBinding::new("ctrl-shift-k", ToggleContacts, None),
            KeyBinding::new("f11", ToggleFullscreen, None),
            KeyBinding::new("ctrl-shift-enter", FocusStage, None),
            KeyBinding::new("ctrl-=", ZoomIn, None),
            KeyBinding::new("ctrl-+", ZoomIn, None),
            KeyBinding::new("ctrl--", ZoomOut, None),
            KeyBinding::new("ctrl-0", ZoomFit, None),
            KeyBinding::new("ctrl-1", TabChats, None),
            KeyBinding::new("ctrl-2", TabCalls, None),
            KeyBinding::new("ctrl-3", TabContacts, None),
            KeyBinding::new("ctrl-4", TabDevices, None),
            KeyBinding::new("ctrl-5", TabAccounts, None),
            KeyBinding::new("ctrl-6", TabAgents, None),
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
            titlebar: Some(TitlebarOptions { title: Some(if hidden { hypr::HIDDEN_TITLE } else { "Joma" }.into()), ..Default::default() }),
            app_id: Some("joma".into()),
            ..Default::default()
        };
        let mut rx = Some(rx);
        let tx = tx.clone();
        cx.open_window(options, move |window, cx| {
            let rx = rx.take().expect("one window");
            // GPUI's Wayland window maps without the title of its options: set it now, so
            // Hyprland's rules on the first title (see `hypr`) see it.
            window.set_window_title(if hidden { hypr::HIDDEN_TITLE } else { "Joma" });
            // Closing hides the window and Joma keeps running; Ctrl+Q quits.
            window.on_window_should_close(cx, |_, _| !hypr::hide());
            cx.new(|cx| Joma::new(tx.clone(), rx, window, cx))
        })
        .expect("failed to open the window");
        cx.activate(true);
    });
    joma_jami::stop();
}
