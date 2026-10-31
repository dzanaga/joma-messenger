# Features

## Layout
A rail of tabs on the left (Ctrl+1…6): **chats**, **calls** (what runs now, with open
and end, and past calls and shares), **contacts**, **devices**, **accounts**, **agents**
(a sketch for now); your picture at the bottom opens it for editing.

## Account
Creating an account asks for your name (what contacts see), then an optional username:
it is registered on Jami's name server once the account is online, so contacts can find
you by it. Without one, "choose a username" in the top bar registers it later. Joma
writes the profile contacts receive (accounts made by Joma 0.2 and earlier had none, so
contacts saw a 40-digit ID; it is added the next time the account comes online).

One machine can hold several accounts, each its own identity with its own contacts and
history. The **accounts** tab shows the open one, the others on this machine (a click
opens one), **create a new account** and **log in with another device** (brings an
account you already have; its username comes with it, nothing to choose) (`joma accounts`,
`joma account <name>`). Joma reopens the last one at start. The others stay connected,
but only the open one is shown and notifies. The bin next to your name removes the
account from this machine: two clicks when your other devices keep it, else typing
`delete`, since an account no other device has is lost for good.

## Settings
The gear at the bottom of the rail: microphone, speakers and the device that rings;
noise and echo removal (auto, Jami's, the system's, off) and automatic gain; the camera,
its resolution and frame rate; call quality (auto lets Jami adapt; low, medium, high
fix the video bitrate at 0.5, 1.5 or 4 Mb/s, applied again at every start since the
daemon forgets it); the Joma desktop share's frame rate (30 or 60). Saved in Jami's own
configuration, and `share_fps`, `video_quality` in `quick.toml`.

## Your picture
Your picture at the bottom of the rail (or the pencil in Accounts): choose a picture (cropped square) or
**draw pixels**: a 16×16 grid with pen, eraser, fill and mirror, colours from a grid of
hues and greys (shift+click a colour: the background), a live preview; "use it" sets
it. Contacts receive it with your profile and see it in their lists, headers and
messages; `joma avatar <file>|--remove`. Pictures of contacts show wherever they appear.

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

## Contacts
The **contacts** tab (Ctrl+3, or Ctrl+Shift+K) lists everyone you added, with or
without a chat:
- requests waiting for you on top (accept or decline);
- each contact with the presence dot, a star, and quick buttons: message, call, video
  call, share the desktop, send files ("waiting" until they accept your request);
- **star:** the contact goes to the top; **drag** contacts (the ⋮ handle, or the whole
  row) to put them in any order. Dropping one among the starred stars it, below them
  unstars it;
- **block** (click twice): Jami's ban. No messages, calls or requests from them, and no
  Joma presence to them; they are not told. The history stays. Blocked contacts are at
  the bottom with **unblock**; unblocking brings the old chat back (Jami also starts a
  new empty one, which Joma hides);
- **remove** (click twice): the contact and your chat with them leave Jami; add them
  again with + if needed.

Saved in `~/.config/joma/quick.toml` (`starred`, `contact_order`).

## Your devices
One Jami account can live on several devices (Joma or the Jami app); each has every
conversation. A message you send reaches your other devices first, and a contact who
was offline gets it from whichever of your devices is online, even if the one you wrote
on is off (tested: sender off, the second device delivered within seconds).
- **Devices** tab: your devices, "this device" first. A dot shows which ones run Joma
  now (active or away; they tell each other, also when you are invisible to contacts).
  The pencil renames this device; ⊘ revokes another (click twice): it loses the account.
- **Link a device:**
  1. on the new machine, Joma's welcome screen: **link to your account** shows a code and
     its QR (or `joma link` prints it);
  2. on a device that has the account: **Devices → paste the code** (the tab shows the
     steps and the commands to copy) (or `joma link-device <code>`; the Jami app scans the QR);
  3. Joma asks whether to send the account to the device at that address; the command
     line sends it without asking. An account with a password asks for it on the new
     device.

## Removing chats
The bin in a conversation's header takes it off the lists (window, bar, notifications);
it stays in Jami, and its messages keep arriving quietly. The bin next to + in the chats
list shows the removed ones: **restore** puts one back, **delete history** (click twice)
deletes the conversation on your devices only. The contact stays: Jami starts a new
empty chat with them. The removed list is `removed` in `~/.config/joma/quick.toml`.

## Messages
Requests on top (accept with Ctrl+Enter), then starred chats (the star in the header,
Ctrl+Shift+F) in the order you drag them to, then the others newest first. Add a contact by
Jami username or ID (Ctrl+N). History, typing indicator, unread counts, desktop
notifications when the window is not active. Names come from peers' profiles and the
name server.

## Files
Paste a picture (Ctrl+V), paste files copied in the file manager, or drop files on the
conversation: they wait above the composer (✕ removes one) and Enter sends them, after
the text if any. Pictures (png, jpg, gif, webp, bmp) show in the conversation once here;
a click opens them. Pasted pictures are kept in `~/.cache/joma/pasted` (Jami sends files
from where they are).
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
- **Opening Joma** (the launcher, `joma`): a floating window, 980×680, centred, like a
  messenger app. **"full" in the panel's top bar** pops out to it; **"to the tray"** pops
  back in.
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
mute, camera. Remote video in the call panel; Ctrl+Shift+Enter gives it the whole
window, F11 the whole screen (as for a shared desktop, below).

## Desktop share
Without a call: the screen button in a conversation's header or `joma share <to>`. The other Joma asks its user ("… wants to show you their
screen": watch or decline, or `joma watch <to>`); no call starts and nothing goes into
the history. Either side stops it; the sharer's share ends when the viewer leaves. If the
peer has no Joma, Joma calls them with the microphone off and shares through Jami.
In a call: Ctrl+Shift+S. With a Joma peer: gliff's pipeline, sharp text, the pointer
drawn on the viewer. Otherwise: Jami's screen share through the portal.

**What to share.** With a Joma peer, Joma first shows every screen and window with a
thumbnail; pick one, or **area…** to draw a rectangle on a screen. A window is sent
alone, even when other windows cover it or it is on another workspace. In a call, "Jami's share" goes through the portal instead.
`joma share <to> --source screen:<output>|window:<address>|area:<output>@x,y,wxh|focused`
skips the question
(addresses as `hyprctl clients` prints them).

**What you share.** While sharing, a strip under the call (or the share's panel) shows
what goes out, as a live picture refreshed about once a second, with its name; "change"
picks another screen or window without interrupting the viewer.

**Watching.** The bar under the stage drags to share the height between the stage and
the chat (double click: the stage takes the window, the contact list steps aside too).
The shared screen fills the stage, fitted; below it: **fit** (all of it),
**fill** (fills the stage, the rest by panning: for an ultrawide, the full height),
**1:1** (one pixel per screen pixel, the sharpest), zoom out/in, **follow** (when zoomed,
the view follows the sharer's pointer) and **full screen**. The mouse wheel or Ctrl with
the touchpad zooms where the pointer is; the touchpad, Shift+wheel or dragging pans
(and stops following); a double click, F11 or the button fills the screen, Esc comes
back. Ctrl+= / Ctrl+- / Ctrl+0 zoom in, out and back to fit.

## Quick Messages
Ctrl+. and Alt+1…9 in the window or the panel; `joma send` and `joma send-quick`.

## Command line
`joma chat <to> [n]` (JSON), `joma call <to> [--video]`, `joma share <to> [--source …]`, `joma watch <to>`,
`joma send-file <to> [path]`, `joma open <to>`, `joma block <to>`, `joma unblock <to>`, `joma link`, `joma link-device <code>`, `joma accounts`, `joma account <name>`, `joma quit`, `joma probe [--bench]` (the
GPUs the desktop share can use).

## Install
`scripts/install.sh [host]` builds a release and installs `~/.local/bin/joma`, the icon
and the launcher here or on another machine over SSH. It needs the `jami-daemon` package.

## Log
- 0.4.4 (2026-10-31): settings: audio devices, noise and echo removal, automatic gain,
  camera and its mode, call quality, the share's frame rate.
- 0.4.3 (2026-10-31): a rail of tabs on the left (chats, calls, contacts,
  devices, accounts, agents; Ctrl+1…6); Calls lists what runs now and past calls and
  shares; Devices explains linking with the commands to run; Accounts holds the open
  account, the others, create / log in with another device; Agents is a placeholder.
  A linked account learns its username from the name server instead of asking for one.
- 0.4.2 (2026-10-31): profile pictures (choose a file, or draw one in pixels), shown
  for everyone in lists, headers and messages; a cleaner look: centred icons, one
  button height, list rows with avatar, time and unread badge, grouped header actions.
- 0.4.1 (2026-10-31): paste or drop pictures and files (they wait above the composer); pictures
  show in the conversation; the conversation stays at its end while pictures load.
  Share an area of a screen (area… draws it with slurp); the bar under the
  stage resizes it against the chat; a stage that takes the window hides the contact
  list too.
- 0.4.0-rc.6 (2026-10-30): desktop share: choose a screen or a single window (with
  thumbnails), change it while sharing, a live preview of what you share; the viewer
  zooms (fit, fill, 1:1, wheel), pans, follows the pointer and goes full screen (F11,
  double click). F11 is now full screen; the stage alone in the window moved to
  Ctrl+Shift+Enter.
- 0.4.0-rc.5 (2026-10-30): several accounts on one machine: switch (Contacts → other
  accounts, `joma account <name>`), create or link another, remove one.
- 0.4.0-rc.4 (2026-10-30): your devices (Contacts → you): link a new device with a code
  or QR (`joma link`, `joma link-device`), rename, revoke; your devices see each other's
  presence. Joma runs in `~/.cache/joma` (Jami leaves link files in the working
  directory); `joma send-file` takes relative paths.
- 0.4.0-rc.3 (2026-10-29): the contacts tab (quick buttons, star, drag to reorder,
  block and unblock, remove); starred chats stay on top in the order you drag them to;
  deleting a chat's history no longer deletes the contact; `joma block`, `joma unblock`.
- 0.4.0-rc.2 (2026-10-29): Joma opens as a floating, centred window instead of a tiled
  one; started over SSH it says how to start it instead of crashing.
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
