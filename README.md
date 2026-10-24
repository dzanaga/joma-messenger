# Joma

A Jami client for [Omarchy](https://omarchy.org), in Rust and
[GPUI](https://www.gpui.rs). Same network, same accounts and history as the Jami app;
an interface in Omarchy's style; and, between two Joma users on Hyprland, a desktop
share built on [gliff](https://github.com/omacom/gliff)'s streaming pipeline (sharp
4:4:4 text, GPU encode), with Jami's own screen share as the fallback for everyone else.

- **Messages:** conversations, requests, history, typing, unread counts, notifications.
- **Calls:** audio and video, mute, camera, remote video.
- **Desktop share:** Joma to Joma through gliff's capture and H.264 pipeline over a
  Jami peer-to-peer tunnel; Jami's portal screen share with other clients.
- **Quick Messages:** canned messages to favourite contacts, from the window
  (Ctrl+. and Alt+1…9), a popup (`joma quick`), the CLI (`joma send`) and an Omarchy
  bar widget (`omarchy-plugin/`).

Joma runs the Jami daemon (libjami) inside its own process, so it cannot run at the same
time as the Jami app on the same account: quit Jami first.

## Build

Needs Arch's `jami-daemon` (libjami and its headers), `jsoncpp`, `ffmpeg`, and what
gliff needs (Vulkan loader, libva, mesa). Then `cargo build --release`; the binary is
`target/release/joma`.

## Use

| Keys | |
|---|---|
| Alt+J / Alt+K | next / previous conversation |
| Ctrl+N | add a contact (Jami username or 40-digit ID) |
| Ctrl+K | filter conversations |
| Ctrl+Shift+C / V | audio / video call |
| Ctrl+Shift+A / E | answer / hang up |
| Ctrl+Shift+M / O | mute / camera |
| Ctrl+Shift+S | share your desktop (or stop) |
| F11 | the call fills the window |
| Ctrl+. , Alt+1…9 | quick messages |
| Ctrl+Shift+F | favourite (Quick Messages) |
| Ctrl+Enter / Ctrl+Backspace | accept / decline a request |

```
joma                      open Joma (or raise it)
joma quick                quick-message popup (bind it to a key in Hyprland)
joma send <to> <text>     send a message to a conversation, name or ID
joma send-quick <to> <n>  send quick message n
```

See [docs/](docs/) for the design, and `AGENTS.md` for how to work on it.
