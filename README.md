# Joma

A Jami client for [Omarchy](https://omarchy.org), in Rust and
[GPUI](https://www.gpui.rs). Same network, same accounts and history as the Jami app;
an interface in Omarchy's style; and, between two Joma users on Hyprland, a desktop
share built on [gliff](https://github.com/omacom/gliff)'s streaming pipeline (sharp
4:4:4 text, GPU encode), with Jami's own screen share as the fallback for everyone else.

- **Messages:** conversations, requests, history, typing, unread counts, notifications.
- **Calls:** audio and video, mute, camera, remote video.
- **Desktop share:** Joma to Joma through gliff's capture and H.264 pipeline over a
  Jami peer-to-peer tunnel, with or without a call; Jami's portal screen share with
  other clients.
- **In the bar:** an icon in the system tray (the Omarchy bar shows it) with a status
  dot; a click or Super+Alt+M drops a compact panel under the bar, which pops out to
  the full window.
- **Quick Messages:** canned messages to favourite contacts, from the window
  (Ctrl+. and Alt+1…9) and the CLI (`joma send`).

## Install a pre-release with mise

On Omarchy (or any Arch with Hyprland), with the Jami daemon from Arch:

```
sudo pacman -S --needed jami-daemon
mise use -g github:dzanaga/joma-messenger@<version>
joma
```

At its first start Joma adds itself to the app launcher. A banner offers to pin its
icon in the bar, bind Super+Alt+M and start at login. Updates: the same `mise use`
with the new version.

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
joma [--hidden]           open Joma (or raise it); --hidden starts in the tray
joma toggle               show or hide the panel (Super+Alt+M)
joma share <to>           share your desktop (no call needed)
joma send <to> <text>     send a message to a conversation, name or ID
joma send-quick <to> <n>  send quick message n
```

See [docs/](docs/) for the design, and `AGENTS.md` for how to work on it.
