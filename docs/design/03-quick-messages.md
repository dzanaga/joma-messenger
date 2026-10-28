# Quick Messages

Canned messages (`messages`) and favourite conversations (`favorites`) in
`~/.config/joma/quick.toml`; when favourites are few, the most recent conversations fill
the list (`recent`, default 6).

- In the window: Ctrl+. shows the bar of messages; Alt+1…9 sends one to the open
  conversation; Ctrl+Shift+F stars it.
- `joma send` / `joma send-quick`: for scripts and launchers.
- Omarchy bar (`omarchy-plugin/io.github.dzanaga.joma-quick`): the v2 logo
  (`JomaGlyph.qml`, the shapes of `assets/joma-logo-v2.png`) in the bar's colour; the
  j's dot shows the status, from the widget settings `onlineColor` (lavender
  `#b193f7`), `unreadColor` (`#39ff88`), `offlineColor` (`#f7768e`, also while Joma is
  not running) and `invisibleColor` (`#7a7f8a`); `joma quick --json` says `invisible`. The panel
  lists contacts ranked by `QuickConfig::rank`: pinned (favourites) and unread first,
  then online ones; offline recent ones fill up to `recent`. A click opens a quick chat
  (`joma chat`, polled every 2 s while open) with a reply box and
  call / video / share screen / send file / open buttons (`joma call`, `joma share`,
  `joma send-file`, `joma open`).
- `omarchy-shell io.github.dzanaga.joma-quick toggle` (bound to Super+Alt+M here) opens
  the panel on the focused monitor: the service owns the IPC target, since every bar
  instantiates the widget and only one handler per target is used.
- Presence: Joma subscribes to every contact (`subscribeBuddy`); `NewBuddyNotification`
  status above 0 counts as reachable (a device announced on the DHT in the last ten
  minutes, or connected). On top, Joma's own presence (`presence.rs`): account messages
  of type `application/x-joma-presence+json` (`active`, `away` with `since`, `hidden`)
  to the one-to-one contacts still listed, on every change and every 4 minutes to the
  reachable ones; a state not refreshed in 10 minutes is dropped. Away comes from the
  compositor's `ext-idle-notify-v1` after 5 minutes.
- `joma share <to>` shares in the call with `<to>` if one runs; otherwise without a
  call (see `desk.rs`): the handshake goes as account messages (`sendAccountTextMessage`,
  kept in no history), the viewer's Joma answers `Hello` and asks its user (`joma watch
  <to>` or the watch button). No `Hello` within 8 s: the peer has no Joma, so Joma calls
  with the microphone off and shares through Jami's portal once connected.
- `joma send-file <to>` without a path opens the desktop's file chooser from Joma
  (skipped in test profiles unless `JOMA_PORTAL=1`).

All of them reach the running Joma over `$XDG_RUNTIME_DIR/joma.sock` (one JSON line each
way, `joma_quick::ipc`), since only that process holds the account.
