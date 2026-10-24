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

## Decided

(none yet)
