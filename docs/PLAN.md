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
| `joma-desk` | Joma desktop share: signalling over in-call messages, gliff's capture → split → H.264 pipeline on the sharer, decode → BGRA on the viewer, carried over a Jami service tunnel; plus the Jami portal fallback |
| `joma-quick` | Quick Messages config and the socket protocol (CLI, bar plugin) |
| `joma` | The GPUI app and the CLI (`joma`, `joma quick`, `joma send`) |

Design notes live in `docs/design/`. Decisions are in `docs/DECISIONS.md`.

## Milestones

| | What | State |
|---|---|---|
| **J0** | libjami bridge; accounts, contacts, conversations, messages; the GPUI shell with the Omarchy look | built |
| **J1** | Calls: place, answer, hang up, mute; remote video through daemon sinks | built; audio calls verified, remote video not yet tested |
| **J2** | Quick Messages plugin: canned messages to favourite contacts from a launcher popup and the CLI | built |
| **J3** | Joma desktop share (gliff pipeline over a Jami tunnel) with fallback to Jami screen share | Joma share verified between two profiles on one machine; fallback not yet tested |
| **J4** | Packaging: PKGBUILD, desktop entry, Hyprland bindings, omatwin | planned |
| **J5** | Remote control (gliff input), clipboard, multi-output picker, file transfer UI, group calls | planned |
