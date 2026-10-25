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
  profile skips notifications and the desktop portal.
- Build: `.cargo/config.toml` (local, ignored) shares Vetro's target dir with incremental
  compilation off. `target/debug` there is a symlink into `/run/media/dzanaga/builds`, a
  btrfs image (zstd) on the external Samsung T5, attached by the user unit
  `builds-disk.path`. If that path is missing, the drive is unplugged: say so rather than
  rebuilding on /home, which is nearly full. Ask before a full `cargo clean`.
- Commit after every change once tests pass (`cargo test --workspace`).
- Docs and code in English.
