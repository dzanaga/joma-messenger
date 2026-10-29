# Features

## Account
Creating an account asks for your name (what contacts see), then an optional username:
it is registered on Jami's name server once the account is online, so contacts can find
you by it. Without one, "choose a username" in the top bar registers it later. Joma
writes the profile contacts receive (accounts made by Joma 0.2 and earlier had none, so
contacts saw a 40-digit ID; it is added the next time the account comes online).

## Presence
Jami only knows whether a contact has a device on the network (phones with push and a
Jami app in the background always do), so Joma users tell each other more:
- a full green dot: active on Joma (input in the last 5 minutes);
- a faint dot: reachable on Jami, or away on Joma;
- the conversation header says "active now", "away since 14:20", or "on Jami · seen
  14:32" (the last time they were active on Joma, or wrote).

**visible / invisible** in the top bar: invisible sends contacts nothing, and those who
saw you active forget it; you still see theirs. Jami's own reachability cannot be
hidden. Saved as `invisible` in `~/.config/joma/quick.toml`.

## Removing contacts
The bin in a conversation's header takes it off the lists (window, bar, notifications);
it stays in Jami, and its messages keep arriving quietly. The bin next to + in the list
shows the removed ones: **restore** puts one back, **delete for good** (click twice)
removes the contact and the conversation from Jami; add them again with + if needed.
The removed list is `removed` in `~/.config/joma/quick.toml`.

## Messages
Conversations newest first, requests on top (accept with Ctrl+Enter). Add a contact by
Jami username or ID (Ctrl+N). History, typing indicator, unread counts, desktop
notifications when the window is not active. Names come from peers' profiles and the
name server.

## Files
Send with the clip button in a conversation or `joma send-file`. Received files up to 20 MB download by
themselves into `~/Downloads/joma/` (a desktop notification says when); larger ones show
their size in the chat: click to download. Click a received file to open it.

## In the bar
Joma lives in the system tray, which the Omarchy bar shows: the j in the theme's
colour, its dot lavender online, green with unread messages, red offline, grey invisible.
- **Click the icon, or Super+Alt+M (`joma toggle`):** the compact panel drops down under
  the bar at the right: the list, then one conversation at a time (Esc goes back).
  Again: it hides.
- **Right click:** open, invisible, start at login, quit.
- **"full" in the top bar:** pops out to the full window. **"to the tray"** pops back in.
- **Closing the window** (Super+W) hides it to the tray; Joma keeps receiving messages and
  calls. `joma`, the launcher, `joma open` or an incoming call bring it back. Ctrl+Q,
  `joma quit` or Quit in the tray's menu quits.
- **`joma --hidden`** starts in the tray, as the login start does.

## Setting up
At every start Joma writes its launcher entry and icons (`~/.local/share/applications`,
`~/.local/share/icons`). A banner offers the rest, each with one click, and goes away
when done or closed:
- **pin the icon in the bar:** new tray icons wait in the tray's drawer; this adds
  `joma` to the tray's `pinned` in `~/.config/omarchy/shell.json`;
- **Super+Alt+M:** a block in `~/.config/hypr/bindings.lua`, offered only when the keys
  are free;
- **start at login:** the systemd user unit `joma.service` (`joma --hidden`); also in
  the tray's menu.

Each piece names the `joma` that wrote it (the mise shim when installed with mise), and
is rewritten at start if Joma moved. Backups: `shell.json.joma-backup`,
`bindings.lua.joma-backup`.

## Beside the Jami app
Joma and the Jami app share accounts and history, and only one may run at a time. When
the Jami app opens while Joma runs, Joma pauses (a notification says so) and comes back
by itself when the Jami app quits.

## Calls
Audio or video calls (Ctrl+Shift+C / V), answer (Ctrl+Shift+A), hang up (Ctrl+Shift+E),
mute, camera. Remote video in the call panel; F11 fills the window.

## Desktop share
Without a call: the screen button in a conversation's header or `joma share <to>`. The other Joma asks its user ("… wants to show you their
screen": watch or decline, or `joma watch <to>`); no call starts and nothing goes into
the history. Either side stops it; the sharer's share ends when the viewer leaves. If the
peer has no Joma, Joma calls them with the microphone off and shares through Jami.
In a call: Ctrl+Shift+S. With a Joma peer: gliff's pipeline, sharp text, the pointer
drawn on the viewer. Otherwise: Jami's screen share through the portal.

## Quick Messages
Ctrl+. and Alt+1…9 in the window or the panel; `joma send` and `joma send-quick`.

## Command line
`joma chat <to> [n]` (JSON), `joma call <to> [--video]`, `joma share <to>`, `joma watch <to>`,
`joma send-file <to> [path]`, `joma open <to>`, `joma quit`, `joma probe [--bench]` (the
GPUs the desktop share can use).

## Install
`scripts/install.sh [host]` builds a release and installs `~/.local/bin/joma`, the icon
and the launcher here or on another machine over SSH. It needs the `jami-daemon` package.

## Log
- 0.4.0-rc.1 (2026-10-29): Joma lives in the tray (status dot, menu) instead of a bar
  plugin; the compact panel under the bar, pop-out to the full window and back; `joma
  toggle`, `joma --hidden`; the setup banner (pin in the bar, Super+Alt+M, start at
  login); launcher and icons written by Joma itself; installable with mise.
- unreleased: the v2 logo (app icon: the j with its lavender dot on black); the bar
  icon's dot shows the status (online, unread, offline, invisible).
- 0.3.0 (2026-10-28): presence between Joma users (active, away, seen) and invisible mode;
  removing contacts (a removed list to restore from or delete for good);
  usernames (at account creation, or "choose a username"); contacts see your
  name instead of your ID (the missing profile is written; received profiles are read
  from the file the daemon names).
- 0.2.0 (2026-10-25): remote video in calls works (it crashed Joma); the version next to
  "joma" in the top bar; a green dot on online contacts in the list; a paused Joma also
  stops a desktop share in progress.
- 2026-10-25 night: desktop share without a call (falls back to a call with the
  microphone off for peers without Joma), `joma watch`; the pixel-art icon (app and bar,
  configurable unread colour on the j's dot); the `joma quick` window and the quick
  messages under the bar chat are gone.
- 2026-10-25 night: Joma pauses while the Jami app runs, also when it opens after Joma
  (before, both ran on the same data); a message the daemon drops says "not sent" and
  its text returns to the draft.
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
