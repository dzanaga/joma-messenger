# Tray and panel

Joma lives in the system tray, like Signal: no shell plugin, so a package (or mise) is
the whole install, and it works with any tray (Omarchy's bar, waybar, KDE).

- **Icon** (`tray.rs`, `ksni`): a StatusNotifierItem with id `joma`. Its pixmaps (16 to
  64 px) are drawn from the logo's shapes (`assets/joma-logo-v2.png`, as
  `scripts/icon.py`): the j in the theme's `foreground`, the dot lavender `#b193f7`
  online, green `#39ff88` with unread, red `#f7768e` offline, grey `#7a7f8a` invisible.
  A gap of at least one pixel stays under the dot. `Tray::set` updates it from `tick`
  when the look changes. `JOMA_TRAY=0` leaves it out.
- **Activate** (left click) sends `TrayAction::Toggle`; the menu: open, invisible, start
  at login, quit.
- **Window shapes** (`hypr.rs`): compact, a floating 460×680 window under the bar
  (the monitor's reserved top) at the right; full, tiled. `joma toggle` shows the
  window in its shape on the current workspace, or hides it to `special:joma` when it has
  the focus. Not pinned: a pinned window loses keys in Hyprland 0.56. Started with
  `--hidden`, a window rule on the first title (`joma-start-hidden`) maps it straight
  into `special:joma`, and the shape is compact; a normal start is full.
- **Narrow layout** (`render.rs`): below 700 px wide, one column: the list, or one
  conversation with a back button (Esc).
- **Setup** (`setup.rs`): the launcher entry and icons are written at every start
  (embedded in the binary). The banner offers pinning (`pinned` of the `omarchy.tray`
  entry in `shell.json`, key order kept), Super+Alt+M (a `-- joma:` … `-- joma end`
  block in `bindings.lua`, checked with `hyprctl configerrors`, restored on error) and
  the systemd user unit. Test profiles never touch systemd or Hyprland.
- **Testing in the nest:** the tray goes on the session bus, which the nest shares
  with the real desktop: run test profiles with a private `dbus-daemon` (and a
  quickshell `SystemTray` to see the icons), or `JOMA_TRAY=0`. `wtype` loses the first
  key of each call into GPUI: start each with `-k Shift_L`.
