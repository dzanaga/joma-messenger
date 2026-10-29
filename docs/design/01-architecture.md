# Architecture

```
joma (GPUI app, CLI) ──── joma-quick (quick.toml, IPC protocol)
  │  events (futures channel)        ▲ $XDG_RUNTIME_DIR/joma.sock
  ▼                                   │ joma CLI (and Hyprland's Super+Alt+M)
joma-jami ── C++ shim (JSON) ── libjami (in process, same data as the Jami app)
joma-desk ── gliff crates (hypr-capture, gliff-vk/va, gliff-sw, gliff-proto/transport)
joma-theme ── ~/.local/state/omarchy/current/theme/colors.toml
joma tray ── ksni (StatusNotifierItem over D-Bus) ── the bar's tray widget
```

- **libjami in process.** The Jami app links libjami the same way. Its C++ API is reached
  through `crates/joma-jami/shim/jami_shim.cpp`: one call entry (`{"m","a"}` →
  `{"r"}|{"e"}`) and one signal callback, both JSON, so the FFI stays tiny. Signals become
  `Event`s on a channel the window drains.
- **Start flags:** none. `NO_LOCAL_VIDEO` drops the daemon's video manager, which also
  owns the audio inputs, and the first call crashes (libjami 17).
- **One instance.** The window owns the daemon and the socket; the CLI and the tray icon
  are clients. If the Jami app runs, Joma waits (same data; two daemons corrupt it).
  The Jami app does not check for Joma, so Joma looks for it every half second
  (`/proc/*/comm` = `jami`): when it starts, Joma stops its daemon and waits; when it
  quits, Joma re-executes itself (libjami cannot start twice in one process), hidden if
  it was hidden. `JOMA_GUARD_QT` turns the check on in a test profile.
- **Sends are confirmed** by the daemon's echo of our own message (it commits locally,
  also offline). Without one within 5 s the message was dropped: Joma says so and puts
  the text back in the draft.
- **Model** (`model.rs`) is plain data fed by events and unit-tested; `app.rs` holds
  actions, `render.rs` the view, `desk.rs` the share handshake.
- **Video** from calls arrives through daemon sinks as BGRA (`attachSink`), copied into
  GPUI images at most once per frame.
