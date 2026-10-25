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
Ctrl+Shift+S in a call. With a Joma peer: gliff's pipeline, sharp text, the pointer drawn
on the viewer. Otherwise: Jami's screen share through the portal.

## Quick Messages
Ctrl+. and Alt+1…9 in the window; `joma quick` popup; `joma send`; the Omarchy bar
widget: contacts with pinned and unread on top, then whoever is online; click one for a
quick chat (latest messages, reply box, quick messages) with buttons to call, video
call, call and share the screen, send a file or open the conversation in Joma. Its icon
is the Joma hexagon in the Omarchy theme's accent colour, dimmed while Joma is not running
(click it then to start Joma).

## Command line
`joma chat <to> [n]` (JSON), `joma call <to> [--video]`, `joma share <to>`,
`joma send-file <to> [path]`, `joma open <to>`, `joma quit`, `joma probe [--bench]` (the
GPUs the desktop share can use).

## Install
`scripts/install.sh [host]` builds a release and installs `~/.local/bin/joma`, the icon
and the launcher here or on another machine over SSH. It needs the `jami-daemon` package.

## Log
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
