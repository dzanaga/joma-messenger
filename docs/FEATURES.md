# Features

## Messages
Conversations newest first, requests on top (accept with Ctrl+Enter). Add a contact by
Jami username or ID (Ctrl+N). History, typing indicator, unread counts, desktop
notifications when the window is not active. Names come from peers' profiles and the
name server.

## Files
Send with the bar widget or `joma send-file`. Received files up to 20 MB download by
themselves into `~/Downloads/joma/` (a desktop notification says when); larger ones show
their size in the chat: click to download. Click a received file to open it.

## Running in the background
Closing the window hides it (Hyprland's special workspace `special:joma`): Joma keeps
receiving messages and calls, the bar widget keeps working, and `joma`, the launcher,
`joma open` or an incoming call bring the window back. Ctrl+Q or `joma quit` quits.

## Calls
Audio or video calls (Ctrl+Shift+C / V), answer (Ctrl+Shift+A), hang up (Ctrl+Shift+E),
mute, camera. Remote video in the call panel; F11 fills the window.

## Desktop share
Without a call: the screen button in a conversation's header, the bar widget's share
button or `joma share <to>`. The other Joma asks its user ("… wants to show you their
screen": watch or decline, or `joma watch <to>`); no call starts and nothing goes into
the history. Either side stops it; the sharer's share ends when the viewer leaves. If the
peer has no Joma, Joma calls them with the microphone off and shares through Jami.
In a call: Ctrl+Shift+S. With a Joma peer: gliff's pipeline, sharp text, the pointer
drawn on the viewer. Otherwise: Jami's screen share through the portal.

## Quick Messages
Ctrl+. and Alt+1…9 in the window; `joma send`; the Omarchy bar widget: contacts with
pinned and unread on top, then whoever is online; click one for a quick chat (latest
messages, reply box) with buttons to call, video call, share the screen, send a file or
open the conversation in Joma. Its icon is the pixel-art jm in the bar's colour; the j's
dot fills green while messages are unread (`unreadColor` in the widget's entry in
`~/.config/omarchy/shell.json`). Dimmed while Joma is not running (click it then to
start Joma).

## Command line
`joma chat <to> [n]` (JSON), `joma call <to> [--video]`, `joma share <to>`, `joma watch <to>`,
`joma send-file <to> [path]`, `joma open <to>`, `joma quit`, `joma probe [--bench]` (the
GPUs the desktop share can use).

## Install
`scripts/install.sh [host]` builds a release and installs `~/.local/bin/joma`, the icon
and the launcher here or on another machine over SSH. It needs the `jami-daemon` package.

## Log
- 2026-10-25 night: desktop share without a call (falls back to a call with the
  microphone off for peers without Joma), `joma watch`; the pixel-art icon (app and bar,
  configurable unread colour on the j's dot); the `joma quick` window and the quick
  messages under the bar chat are gone.
- 2026-10-25 evening: receiving files (`~/Downloads/joma/`, automatic up to 20 MB);
  closing the window keeps Joma running; `joma quit`; the bar icon is the Joma hexagon
  in the theme's accent colour.
- 2026-10-25: presence (online dots), quick chat in the bar widget, calls, desktop share
  and files from the bar and the CLI; file transfers (send); the neon hexagon icon and
  launcher; `scripts/install.sh`; GPU tiers for the share (the Intel GPU encodes when
  the display GPU cannot), `joma probe`.
- 2026-10-25: first build. libjami bridge, Omarchy-styled GPUI interface, messages,
  requests, calls, Joma desktop share over a Jami tunnel (verified between two test
  profiles), portal fallback, Quick Messages (window, popup, CLI, bar plugin).
