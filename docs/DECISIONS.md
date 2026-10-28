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

## Decided

(none yet)
