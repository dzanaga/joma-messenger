# Quick Messages

Canned messages (`messages`) and favourite conversations (`favorites`) in
`~/.config/joma/quick.toml`; when favourites are few, the most recent conversations fill
the list (`recent`, default 6).

- In the window: Ctrl+. shows the bar of messages; Alt+1…9 sends one to the open
  conversation; Ctrl+Shift+F stars it.
- `joma quick`: a small window (app id `joma-quick`, float it with a Hyprland rule):
  j/k pick a contact, 1…9 send, Esc closes.
- `joma send` / `joma send-quick`: for scripts and launchers.
- Omarchy bar (`omarchy-plugin/io.github.dzanaga.joma-quick`): unread count; the popup
  sends with `joma send-quick`.

All of them reach the running Joma over `$XDG_RUNTIME_DIR/joma.sock` (one JSON line each
way, `joma_quick::ipc`), since only that process holds the account.
