# Joma plan

Joma is a Jami client for Omarchy: a native Rust/GPUI interface on the Jami daemon (libjami, in
process), on the same network, accounts and history as the Qt client. Between two Joma users on
Hyprland it adds a desktop share built on gliff's streaming pipeline; with anyone else it falls back
to Jami's own screen share.

## Architecture

| Crate | Role |
|---|---|
| `joma-jami` | libjami in process: a C++ shim with one JSON call entry and one JSON signal callback; typed calls and `Event`s in Rust |
| `joma-theme` | Omarchy's current `colors.toml` as a GPUI palette, with a fallback (Tokyo Night) |
| `joma-desk` | Joma desktop share: signalling over in-call or account messages (share without a call), gliff's capture → split → H.264 pipeline on the sharer, decode → BGRA on the viewer, carried over a Jami service tunnel; plus the Jami portal fallback |
| `joma-quick` | Quick Messages config and the socket protocol (CLI) |
| `joma` | The GPUI app and the CLI (`joma`, `joma send`, `joma share`, …) |

Design notes live in `docs/design/`. Decisions are in `docs/DECISIONS.md`.

## Milestones

| | What | State |
|---|---|---|
| **J0** | libjami bridge; accounts, contacts, conversations, messages; the GPUI shell with the Omarchy look | built |
| **J1** | Calls: place, answer, hang up, mute; remote video through daemon sinks | built; audio calls and remote video verified (the 0.2.0 fix of `docs/crashes/2026-10-25-remote-video-sink-abort.md`) |
| **J2** | Quick Messages plugin: contacts (pinned, unread, online), a quick chat with call / share / file buttons in the Omarchy bar; the CLI | built; installed on zoma; chat, presence and share-from-CLI verified between test profiles |
| **J3** | Joma desktop share (gliff pipeline over a Jami tunnel) with fallback to Jami screen share | verified between two profiles: 4:4:4 at ~55 fps (1280x800, Intel encode and decode); GPU tiers and `joma probe`; fallback not yet tested |
| **J4** | Packaging: icon, desktop entry, `scripts/install.sh` (local or SSH), Hyprland binding; later a PKGBUILD | install script and launcher done; omatwin paused (development) |
| **J5** | Remote control (gliff input), clipboard, multi-output picker, file transfer UI, group calls | files: send and receive done (verified between test profiles); the rest planned, remote control and clipboard next |
| **J6** | Living in the bar: a tray icon (StatusNotifierItem) with a status dot instead of a shell plugin; the compact panel under the bar (a floating window), pop-out to the full window and back; Super+Alt+M; start at login (systemd user unit) and in the tray; the setup banner | built (0.4.0-rc.1): `docs/design/05-tray-and-panel.md`; tray, panel, hidden start verified in the nest |
| **J7** | Settings and management: settings view (`config.toml`), contacts page (nicknames, block, requests), clear / delete chats, profile, devices (link, revoke), backup | contacts tab, block, stars, drag order and deleting chats without the contact built in 0.4.0-rc.3; the rest proposed |
| **D1** | Your devices: the delivery test with two linked devices; the "you" group and devices page; link (jami-auth token, QR), rename, revoke | built in 0.4.0-rc.4; delivery through a second device tested (D-18) |
| **D2** | Sending to your devices: files, images, clipboard; to all (Notes to self) or one (a Joma message naming it) | planned |
| **D3** | What contacts see per device: the sum by default, per-device for chosen contacts, invisible devices | planned |
| **D4** | Screen between your devices, then remote control (portal RemoteDesktop) | planned |
| **D5** | Agents through ACP: an agent account per machine, marked as yours | planned |
| **J8** | Multiple accounts: switcher, all connected, add / import / link / remove, combined bar status | proposed (same discussion) |
| **J9** | Packaging: public repository, pre-releases installable with mise (`github:dzanaga/joma-messenger`); later GitHub Actions builds, AUR `joma-messenger-bin` (updates with `omarchy update`), the Omarchy repository | mise pre-releases from 0.4.0-rc.1; the rest proposed |
