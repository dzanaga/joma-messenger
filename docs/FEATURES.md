# Features

## Layout
A strip of numbered tabs on top (Ctrl+1…6): **chats**, **calls** (what runs now, with
open and end, and past calls and shares), **contacts**, **devices**, **accounts**,
**agents** (a sketch for now). At its right: the version, settings, "to the tray", and
your picture with a dot for your state (green online, grey invisible, yellow connecting,
red offline). Your picture opens a menu: your picture, become invisible / visible,
choose a username, settings, keys, to the tray.

Below, two panels like tiled windows: the list and the conversation, the one in use
bordered in the accent. Messages are bubbles, yours on the right. The conversation's
header has call, video call and share; **⋯** holds star, quick messages, send a file and
remove from the list. Attach, emoji, YouTube, voice and video messages are inside the
message field. Notes that used to be in the status line pass at the bottom of the
window. **F1** (or Ctrl+/) lists every key.

## Account
Creating an account asks for your name (what contacts see), then an optional username:
it is registered on Jami's name server once the account is online, so contacts can find
you by it. Without one, "choose a username" in your picture's menu registers it later. Joma
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
The sliders icon in the tab strip opens a page in place of the list and the
conversation (the icon again, ✕ or Esc closes it), with sections on the left:
**interface** (rounded or square corners; the version shown or hidden), **audio**,
**video**, **desktop share**, **about** (version, account, keys). Audio and video: microphone, speakers and the device that rings;
noise and echo removal (auto, Jami's, the system's, off) and automatic gain; the camera,
its resolution and frame rate; call quality (auto lets Jami adapt; low, medium, high
fix the video bitrate at 0.5, 1.5 or 4 Mb/s, applied again at every start since the
daemon forgets it); the Joma desktop share's frame rate (30 or 60). Saved in Jami's own
configuration, and `share_fps`, `video_quality`, `rounded`, `show_version` in `quick.toml`.

## Your picture
Your picture in the tab strip → **your picture** (or the pencil in Accounts): choose a picture (cropped square) or
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

**become invisible / visible** in your picture's menu: invisible sends contacts nothing, and those who
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

## Connection info
The signal button on a contact (Contacts) or on one of your devices (Devices) shows the
live connections to that contact's or device's devices: the state (connected, TLS
handshake, ICE negotiation, connecting, waiting), the address, the path, and the
channels open on it (messages, conversation sync, file transfers, tunnels). The path is
**relayed** (through the account's TURN server, turn.jami.net by default: shown in yellow,
slow), **Tailscale** (or Tailscale through a DERP relay, also yellow), **local network** or
**direct**. Devices connect when there is something to send, so "not connected now" is
normal. `joma connections [<to>] [--json]` prints the same for every connection, or for one
conversation's members.

## Folder sync
Devices tab → **synced folders**: turn sync on, add a folder, and it is kept the same on
all your devices that run Joma with sync on (Joma runs its own Syncthing, reached
through Jami: no server, no account). Folders from your other devices appear in
`~/joma-sync/`. Each device shows whether it is syncing and the path to it; a relayed
path is yellow (slow for large files). `joma sync on|off`, `joma sync add <path>
[label]`, `joma sync remove <folder>`, `joma sync status [--json]`. Needs `syncthing`
installed.

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

## Replies, reactions, edits
Click a message (right click a file or picture) for what can be done with it:
- **reply**: the message field shows what you answer (✕ or Esc stops); your message
  carries a quote of it;
- **👍 ❤️ 😂 😮 😢 🙏**: a reaction, shown under the message with a count; a click on
  yours takes it back;
- **edit** (your own): the text returns to the message field; Enter replaces it, and
  the message says "edited";
- **delete** (your own, click twice): gone for everyone; its place says "message
  deleted".

These are Jami's own, so the Jami app shows them too.

## Voice and video messages, emoji, YouTube
Beside the message box:
- ☺ **emoji**: a grid; a click types one at the cursor.
- ▶ **YouTube**: searches YouTube for what is typed (yt-dlp, no account), a click puts
  the link in the box. A YouTube link in a conversation shows its thumbnail, title and
  channel (fetched once from YouTube and kept in `~/.cache/joma/youtube`); a click opens
  it in the browser.
- **GIFs** (the picture icon): the GIFs you keep in `~/Pictures/gifs` (`gif_dir` in
  `quick.toml` names another folder); typing in the message field filters them by
  name, a click sends one as a file, so it plays in the Jami app too. No online search:
  that needs an account and a key with Giphy or Tenor.
- 🎤 **voice message** and 🎥 **video message** (from the camera): record, then send
  or ✕. They go as files (`.ogg`, `.webm`); received ones have **play** (mpv: sound
  only for voice, a window for video), click again to stop.

## Transcribing voice messages
A voice message that is here has **transcribe**: its words appear under it and stay in
Joma's cache (`~/.cache/joma/transcripts`); nothing is sent into the conversation.
**Settings → audio → voice messages** chooses the engine:
- **whisper.cpp, local** (the default when `whisper-cli` is installed; AUR package
  `whisper.cpp`): on this machine, on the CPU, language detected. The model (`base`,
  about 150 MB) is downloaded at the first transcription into
  `~/.local/share/joma/models`;
- **superwhisper**: that app does it, in the mode it is set to. In a cloud mode the
  voice message leaves this machine, so Joma never picks it by itself;
- **no transcription**.

## Watching together
Type `@youtube epic fails` (the YouTube button beside the message box writes `@youtube `
for you) and Enter: Joma searches and sends the first five results to the conversation
(as text with links, readable in any Jami app); Joma shows them as cards. Each has **for
me** (plays in your own mpv) and **together** (invites the conversation); when a session
runs, **queue** and **switch** instead. A single YouTube link's card has the same.

In a session everyone who joined plays the video in their own mpv, kept in step (a
seek aims ahead by how long that player takes to buffer; small gaps are closed by
playing up to 10 % faster or slower). The panel above the conversation shows the video,
the time, who watches and the queue, with **play/pause**, **next**, **end** and
**leave**. Every playback action (play, pause, seek, switch, next, end) is a proposal
that passes when at least half of the participants agree, the proposer included: two
people both command at once; in a group the others see it with **yes**/**no** and the
count. Doing it in mpv counts as proposing it (in a group your player stays with the
others until it passes). Adding to the queue needs no vote; at a video's end the queue
goes on. `joma watch-together <to> <url>` starts one from the command line.

## Files
Paste a picture (Ctrl+V), paste files copied in the file manager, or drop files on the
conversation: they wait above the composer (✕ removes one) and Enter sends them, after
the text if any. Pictures (png, jpg, gif, webp, bmp) show in the conversation once here;
a click shows one large inside Joma, with before / after through the conversation's
pictures, **open outside** (the system's viewer) and Esc to close. Pasted pictures are kept in `~/.cache/joma/pasted` (Jami sends files
from where they are).
Send with the clip button in a conversation or `joma send-file`. Received files up to 200 MB download by
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
  messenger app. the **expand** icon in the panel's tab strip pops out to it; the **tray** icon pops
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
Audio or video calls (Ctrl+Shift+C / V). A call coming in is asked about over the whole
window: **answer** (Ctrl+Shift+A), **with video**, **decline**. In a call: mute, camera,
share and hang up (Ctrl+Shift+E) are one bar, in the call's line or floating over the
picture when there is one. Remote video in the call panel; Ctrl+Shift+Enter gives it the whole
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

**Pointing and drawing.** While watching a Joma share, the bar under it has **laser**
(your pointer shows on their screen as a red dot) and **pen** (drag to draw, four
colours); **clear** wipes the drawing on both sides. The sharer sees it on their own
screen, over every window and without catching clicks, and has **clear drawings** in
their share strip; drawings go when the viewer leaves or the source changes.

**The sharer draws too.** The share strip has **laser** and **pen** (four colours):
with one on, your pointer draws on the shared screen instead of clicking, and viewers
see it; a right click on that screen gives the pointer back. **screenshot** (in the
strip, and under the stage for viewers) saves the shared screen with the drawings in
`~/Pictures/joma` and sends it to the conversation. `joma draw laser|pen|off|clear|shot`
does the same from a key binding.

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
`joma send-file <to> [path]`, `joma open <to>`, `joma block <to>`, `joma unblock <to>`, `joma link`, `joma link-device <code>`, `joma accounts`, `joma account <name>`, `joma ui settings|profile|keys|menu|chat-menu|emoji|quick|close`, `joma connections [<to>] [--json]`, `joma sync status|on|off|add|remove`, `joma quit`, `joma probe [--bench]` (the
GPUs the desktop share can use).

## Install
Releases: `mise use -g github:dzanaga/joma-messenger@<v>` (needs the `jami-daemon`
package). Joma points its launcher and login unit at mise's shim, so a new version is
what starts next. `scripts/install.sh [host]` builds a release and installs
`~/.local/bin/joma` instead (for development or another machine over SSH); it refuses
when mise has Joma here, since the two would compete and the launcher would start
whichever ran last.

## Updates
Joma asks GitHub for its releases shortly after it starts and every six hours. When a
newer one exists, **update · 0.6.0** shows in the tab strip (and a desktop notification
says so once): it opens **settings → about**, where **update and restart** installs it
with mise and starts the new version. A Joma not installed with mise shows the command
instead. **Channel**: stable releases only (the default), or pre-releases too
(`prereleases` in `quick.toml`). Versions between stable releases are numbered
`0.5.2.dev0`, `.dev1`, … (`0.5.2-dev.1` for Cargo and in the release tag); publish one
with `scripts/release.sh "notes" --pre`.

## On the phone (Android, first build)
`android/build.sh` builds **joma for Android**: an APK (arm64) at
`~/builds/joma-android/joma-debug.apk`, installed by hand (open it on the phone and allow
installing from that app; Play Protect warns about an unknown developer). It is the
Jami Android app with joma's name, icon, palette, font, icons and words, and its own id
(`io.github.dzanaga.joma`), so it installs beside the Jami app. What Jami Android does,
it does: an account created on the phone or linked from the desktop (desktop: Devices →
paste the code the phone shows), chats with people and groups, requests, adding by
Jami username or ID, voice and video messages, files and pictures, notifications, calls.
Dark by default; it stays running in the background (a permanent notification) since it
has no Google push. joma's own extras (desktop share, presence, watching together) are
not there yet. Verified in an emulator (account, a chat between two local accounts,
notifications); calls, voice messages and files not yet tried. Design:
`docs/design/07-android.md`.

## Log
- Android 0.1.0.dev1 (2026-11-27): the first APK, Jami Android with joma's skin (see
  "On the phone").
- 0.5.2.dev11 (2026-11-27): folder sync between your own devices (Joma's own Syncthing
  over Jami tunnels; tested between two linked test profiles: a folder with a 3 MB
  file arrived on the other device in 5 s, an edit came back in 12 s, local network);
  connection info on contacts and devices and `joma
  connections` (path: relayed, Tailscale, local, direct; tested between two throwaway
  accounts, which connected over the local network); received files up to 200 MB
  download by themselves (was 20 MB).
- 0.5.2.dev10 (2026-11-17): the panel under the bar: pictures fit their bubble, settings
  show their sections as icons.
- 0.5.2.dev9 (2026-11-17): a GIF picker from your own folder.
- 0.5.2.dev8 (2026-11-17): transcribe a voice message (whisper.cpp on this machine, or
  superwhisper if chosen). The steps around the engine were tested with a stand-in:
  whisper.cpp is not installed on this machine, and superwhisper was not called.
- 0.5.2.dev7 (2026-11-17): an incoming call is asked about over the window; the call's
  buttons are one bar (floating over the picture when there is one: that part is not
  yet seen with a real video).
- 0.5.2.dev6 (2026-11-17): a picture viewer inside Joma.
- 0.5.2.dev5 (2026-11-17): reply to a message, react to it, edit and delete your own.
  Fixed: an edited message showed its old text (also messages edited in the Jami app).
- 0.5.2.dev4 (2026-11-17): update notice in the tab strip, update and restart from
  settings → about (through mise), stable or pre-release channel. Tested with a made-up
  release list; the install itself waits for a real newer release.
- 0.5.2.dev3 (2026-11-17): settings as a page with sections; rounded corners on every
  box; contacts rows like the chat rows; a long conversation no longer pushes the
  message field out of its panel.
- 0.5.2.dev2 (2026-11-17): the person
  sharing can use the laser and the pen too (**laser**, **pen** and colours in the
  share strip; the pointer then draws on the shared screen, a right click there gives it
  back); **screenshot** on both sides saves the shared screen with the drawings in
  `~/Pictures/joma` and sends it to the conversation; `joma draw
  laser|pen|off|clear|shot`.
- 0.5.2.dev1 (2026-11-17): the new look, first part: tabs in a strip on top, the list
  and the conversation as tiles, bubbles, one message field with its tools inside, a
  menu on your picture and one on the conversation (⋯), no top bar or status line, F1
  for the keys; settings: rounded or square corners, version shown or hidden.
  `joma ui <part>` opens a part of the interface from a script.
- 0.5.2.dev0 (2026-11-17): Joma draws its own icons (thin lines, one grid) instead of
  the terminal font's glyphs, so they look the same with any font and sit centred;
  borders are hairlines on every theme. First step of the restyle
  (`docs/discussions/2026-11-16-interface.qmd`).
- 0.5.1 (2026-10-31): `@youtube <search>` shares five results in the conversation;
  watching together in any conversation (groups too) with a queue and votes (half of
  the participants decide; two people both command); tighter sync for streams.
- 0.5.0 (2026-10-31): watching together (mpv on both sides, kept in step);
  discussions on agents, watching together, transcription and GIFs.
- 0.4.6 (2026-10-31): laser pointer and pen on a shared screen, shown on the sharer's
  screen; clear on both sides.
- 0.4.5 (2026-10-31): voice and video messages (record, send, play with mpv); an
  emoji picker; YouTube search from the message box and link previews.
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
