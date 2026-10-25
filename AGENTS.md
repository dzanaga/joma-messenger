# Working on Joma

- Read [docs/PLAN.md](docs/PLAN.md) and the short notes in [docs/design/](docs/design/).
  Keep them short and update them in the same change as the code.
- [docs/FEATURES.md](docs/FEATURES.md) is the manual and the log: a change a user can see
  updates it in the same commit. Decisions go in [docs/DECISIONS.md](docs/DECISIONS.md),
  one line each; only the user moves an item to **Decided**.
- Discussions with the user: `docs/discussions/YYYY-MM-DD-topic.qmd`, numbered questions
  with `- [ ] A:` answer lines, as in Vetro.
- `upstream/` holds read-only clones of jami-daemon, jami-client-qt and gliff for
  reference (ignored by git). gliff's crates are used as git dependencies pinned to a
  reviewed revision; `crates/joma-desk/src/rate.rs` is vendored from gliff (MIT).
- **Never touch the user's real Jami data or desktop in tests.** Joma shares
  `~/.local/share/jami` with the Jami app. Tests run with `JOMA_PROFILE=<dir>` (its own
  data, config and socket) and `HOME=<dir>/home` after `scripts/test-profile.sh <dir>`
  (silent ALSA audio), inside the hidden nested Hyprland of `scripts/nest.sh`. A test
  profile skips notifications and the desktop portal. Known limit: the ALSA `null`
  capture is unpaced, so libjami's `AudioInput::readFromDevice` spins during a test call
  and hanging up blocks in `SIPCall::stopAllMedia` (joins that loop). End test calls by
  stopping the profiles; real audio devices are paced and not affected.
- In the nest, `grim -o $NEST_HEADLESS` takes screenshots (plain `grim` waits forever);
  `hyprctl dispatch 'hl.dsp.focus({ window = "pid:<pid>" })'` picks the window `wtype`
  types into.
- **Never run codec experiments on the GPU that drives the screen** without asking the
  user first. A hung encoder resets its GPU, and a failed reset takes the desktop down:
  this happened on 2026-10-25 (VA-API encode on the Radeon 550, see `crates/joma-desk/src/va.rs`).
  `joma probe --bench` skips that GPU unless given `--display-gpu`.
- Build: `.cargo/config.toml` (local, ignored) builds into
  `/run/media/dzanaga/builds/vetro-target`, shared with Vetro (GPUI is built once). That
  is a btrfs volume (zstd) on the user's external Samsung T5, attached by the user unit
  `builds-disk.path`; Vetro's `target` links to it. /home is nearly full: never build
  there. If the volume is missing, the drive is unplugged: say so.
- Commit after every change once tests pass (`cargo test --workspace`).
- Docs and code in English.
