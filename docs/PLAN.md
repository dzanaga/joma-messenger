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
| **J3** | Joma desktop share (gliff pipeline over a Jami tunnel) with fallback to Jami screen share | verified between two profiles: 4:4:4 at ~55 fps (1280x800, Intel encode and decode); GPU tiers and `joma probe`; fallback not yet tested. 0.4.0-rc.6: screen or window picker with thumbnails, live preview, source change while sharing, viewer zoom/pan/follow/full screen (verified in the nest, CPU encode) |
| **J4** | Packaging: icon, desktop entry, `scripts/install.sh` (local or SSH), Hyprland binding; later a PKGBUILD | install script and launcher done; omatwin paused (development) |
| **J5** | Remote control (gliff input), clipboard, multi-output picker, file transfer UI, group calls | files: send and receive done (verified between test profiles); the rest planned, remote control and clipboard next |
| **J6** | Living in the bar: a tray icon (StatusNotifierItem) with a status dot instead of a shell plugin; the compact panel under the bar (a floating window), pop-out to the full window and back; Super+Alt+M; start at login (systemd user unit) and in the tray; the setup banner | built (0.4.0-rc.1): `docs/design/05-tray-and-panel.md`; tray, panel, hidden start verified in the nest |
| **J7** | Settings and management: settings view (`config.toml`), contacts page (nicknames, block, requests), clear / delete chats, profile, devices (link, revoke), backup | contacts tab, block, stars, drag order and deleting chats without the contact built in 0.4.0-rc.3; the rest proposed |
| **D1** | Your devices: the delivery test with two linked devices; the "you" group and devices page; link (jami-auth token, QR), rename, revoke | built in 0.4.0-rc.4; delivery through a second device tested (D-18) |
| **D2** | Sending to your devices: files, images, clipboard; to all (Notes to self) or one (a Joma message naming it) | planned |
| **D3** | What contacts see per device: the sum by default, per-device for chosen contacts, invisible devices | planned |
| **D4** | Screen between your devices, then remote control (portal RemoteDesktop) | planned |
| **D5** | Agents through ACP: an agent account per machine, marked as yours | planned |
| **J8** | Multiple accounts: switcher, all connected, add / import / link / remove, combined bar status | switch, add, link, remove built in 0.4.0-rc.5; notifications from all accounts, import, combined status proposed |
| **J9** | Packaging: public repository, pre-releases installable with mise (`github:dzanaga/joma-messenger`); later GitHub Actions builds, AUR `joma-messenger-bin` (updates with `omarchy update`), the Omarchy repository | mise pre-releases from 0.4.0-rc.1; the rest proposed |
| **U0** | Interface foundation: `ui` module (tokens, icon, button, row, input, menu, badge), own SVG icons, `render.rs` split by view, borders derived from background and foreground; a script that screenshots every view in the nest. Nothing looks different | in progress (0.5.2.dev0): icons, border token, `render/` split done; components, tokens and `scripts/shots.sh` done with U1 |
| **U1** | Restyle of the shell and the conversation: tab strip on top, tiled panels, bubbles, one composer field, empty states, onboarding, the tray panel | built (0.5.2.dev1), verified in the nest |
| **U2** | Restyle of the other views: calls, contacts, devices, accounts, agents, settings as a page, share picker, stage toolbar, watch-together panel | settings page, rounded boxes, contact rows built (0.5.2.dev3); the call panel, picker and stage toolbar keep their layout with the new buttons |
| **U3** | After the restyle, in the user's order: update notice, one-click update and stable / dev channels; replies, reactions, edit and delete; picture viewer; incoming-call overlay and floating call bar; transcription; GIFs | planned |
| **U4** | Sharer's laser and pen on the shared screen (the overlay takes the pointer), screenshot of a share into the conversation, `joma draw` | built in 0.5.2.dev2; verified in the nest between two profiles (pen, laser, right click, screenshot received by the viewer; CPU video) |
| **U5** | Updates: release check, notice, update through mise and restart, stable / pre-release channel | built in 0.5.2.dev4; the notice verified with `JOMA_RELEASES_URL`, the mise install not yet (needs a newer release) |
| **U6** | Replies, reactions, edit and delete (Jami's own: `sendMessage` flags 0 with a parent, 1, 2; `ReactionAdded` / `ReactionRemoved` forwarded by the shim) | built in 0.5.2.dev5, verified in the nest on one profile (the peer was offline: not yet seen arriving on the other side) |
