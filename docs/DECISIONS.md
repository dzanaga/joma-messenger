# Decisions

## Proposed

- D-1 Joma is a Jami client: libjami in process, same data as the Jami app, one at a time.
- D-2 The Joma desktop share runs gliff's pipeline (crates as libraries) over a Jami
  service tunnel; the gliff app is never spawned. (User, 2026-10-25: reimplement the
  technique, do not tunnel gliff.)
- D-3 The handshake rides in-call messages (`application/x-joma-desk+json`).
- D-4 Fallback is Jami's portal screen share, on no `Hello`, a decline or 15 s of silence.
- D-5 Quick Messages is an Omarchy shell plugin plus an in-app bar, all
  through the running Joma's socket.
- D-6 View-only desktop share for now; remote control and clipboard later.
- D-7 Licence GPL-3.0-or-later (Joma links libjami, GPL-3).
- D-8 Closing the window hides it on `special:joma`; Joma quits only on Ctrl+Q or
  `joma quit`. (User, 2026-10-25: open it once, it stays in the background.)
- D-9 Received files go to `~/Downloads/joma/`, by themselves up to 20 MB (the Jami app's
  default). (User, 2026-10-25.)
- D-10 The VA-API tier stays, opt-in only (`JOMA_VIDEO=va`), and never on the GPU that
  drives the screen without asking.
- D-11 Desktop share without a call: the handshake as account messages, the viewer
  asked first; peers without Joma get a call with the microphone off. (User, 2026-10-25.)
- D-12 No `joma quick` window, no quick messages under the bar chat: the bar panel is
  the quick way in. (User, 2026-10-25.)
- D-13 Removing a contact only hides it in Joma (`removed` in quick.toml); deleting for
  good is a second step from the removed list (removeContact without ban, then
  removeConversation). (User, 2026-10-25.)
- D-14 Presence: Jami's reachability is a faint dot; Joma users exchange active/away
  (full green dot, "active now", "away since", "seen"); invisible sends nothing.
  (User, 2026-10-28.)
- D-15 No shell plugin: Joma lives in the system tray (StatusNotifierItem) with a status
  dot; the panel is a compact floating window under the bar; a banner offers pinning,
  Super+Alt+M and start at login. (User, 2026-10-29.)
- D-16 The repository is public; pre-releases are installed with mise from GitHub
  releases. (User, 2026-10-29.)
- D-17 A contacts tab beside the chats: quick buttons (message, call, video, share,
  file), star to the top, drag to reorder (contacts freely; chats among the starred,
  the rest by latest message), block = Jami's ban, unblock = addContact. Deleting a chat
  keeps the contact (only removeConversation); removing a contact is a separate action.
  Replaces D-13's "delete for good". (User, 2026-10-29.)
- D-18 Your devices are devices linked to one Jami account (not accounts tied by Joma):
  a "you" group in Contacts and a devices page (link with jami-auth PIN/QR, rename,
  revoke); sending to all devices through Notes to self, to one device with a Joma
  message naming it; screen and remote control only between your own devices, asked
  once per device; delivery relies on Jami's sync, measured first; contacts see one
  sum of your devices unless you show them per contact; agents get their own account,
  not a linked device, and talk through ACP. Devices before multiple accounts.
  (User, 2026-10-30, `docs/discussions/2026-10-30-devices.qmd`.)
- D-19 Joma's share asks what to share (a screen or one window, with thumbnails) and
  shows the sharer a live preview; the viewer zooms, pans, follows the pointer and goes
  full screen. Window capture needs `hypr-capture` vendored with a toplevel source.
  F11 = full screen. (User asked for zoom, full screen, the choice and a preview,
  2026-10-30.)

## Decided

(none yet)
