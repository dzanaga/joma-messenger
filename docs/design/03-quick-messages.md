# Quick Messages

Canned messages (`messages`) and favourite conversations (`favorites`) in
`~/.config/joma/quick.toml`; when favourites are few, the most recent conversations fill
the list (`recent`, default 6).

- In the window: Ctrl+. shows the bar of messages; Alt+1…9 sends one to the open
  conversation; Ctrl+Shift+F stars it.
- `joma quick`: a small window (app id `joma-quick`, float it with a Hyprland rule):
  j/k pick a contact, 1…9 send, Esc closes.
- `joma send` / `joma send-quick`: for scripts and launchers.
- Omarchy bar (`omarchy-plugin/io.github.dzanaga.joma-quick`): unread count. The panel
  lists contacts ranked by `QuickConfig::rank`: pinned (favourites) and unread first,
  then online ones; offline recent ones fill up to `recent`. A click opens a quick chat
  (`joma chat`, polled every 2 s while open) with a reply box, the quick messages, and
  call / video / share screen / send file / open buttons (`joma call`, `joma share`,
  `joma send-file`, `joma open`).
- Presence: Joma subscribes to every contact (`subscribeBuddy`); `NewBuddyNotification`
  status above 0 counts as online.
- `joma share <to>` places an audio call and starts the desktop share once it is
  connected: at the peer's Joma hello, or after 3 s without one (Jami's portal share).
- `joma send-file <to>` without a path opens the desktop's file chooser from Joma
  (skipped in test profiles unless `JOMA_PORTAL=1`).

All of them reach the running Joma over `$XDG_RUNTIME_DIR/joma.sock` (one JSON line each
way, `joma_quick::ipc`), since only that process holds the account.
