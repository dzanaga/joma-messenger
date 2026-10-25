# Decisions

## Proposed

- D-1 Joma is a Jami client: libjami in process, same data as the Jami app, one at a time.
- D-2 The Joma desktop share runs gliff's pipeline (crates as libraries) over a Jami
  service tunnel; the gliff app is never spawned. (User, 2026-10-25: reimplement the
  technique, do not tunnel gliff.)
- D-3 The handshake rides in-call messages (`application/x-joma-desk+json`).
- D-4 Fallback is Jami's portal screen share, on no `Hello`, a decline or 15 s of silence.
- D-5 Quick Messages is an Omarchy shell plugin plus an in-app bar and a popup, all
  through the running Joma's socket.
- D-6 View-only desktop share for now; remote control and clipboard later.
- D-7 Licence GPL-3.0-or-later (Joma links libjami, GPL-3).
- D-8 Closing the window hides it on `special:joma`; Joma quits only on Ctrl+Q or
  `joma quit`. (User, 2026-10-25: open it once, it stays in the background.)
- D-9 Received files go to `~/Downloads/joma/`, by themselves up to 20 MB (the Jami app's
  default). (User, 2026-10-25.)
- D-10 The VA-API tier stays, opt-in only (`JOMA_VIDEO=va`), and never on the GPU that
  drives the screen without asking.

## Decided

(none yet)
