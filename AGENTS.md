# Working on Joma

- Read [docs/PLAN.md](docs/PLAN.md) and the short notes in [docs/design/](docs/design/).
  Keep them short and update them in the same change as the code.
- [docs/FEATURES.md](docs/FEATURES.md) is the manual and the log: a change a user can see
  updates it in the same commit. Decisions go in [docs/DECISIONS.md](docs/DECISIONS.md),
  one line each; only the user moves an item to **Decided**.
- Agents can drive Joma (user, 2026-11-26): a feature a user can trigger also gets its
  `joma` command in the same change (through the socket like `joma send`, with `--json`
  and a non-zero exit on failure), with a test. Passwords and approvals are never taken on
  the command line, from the environment or through a pipe: Joma asks in its own window.
- Discussions with the user live on the dev board (below), since 2026-11-24: an item of
  kind `discussion` with `intro` and `outro` (markdown), `date`, `file`, and `questions`
  (`n`, `title`, `group`, `text` in markdown, `answer`, `done` = ticked, i.e. agreed,
  `settled` = made moot by later work; in the file: `- [ ] A: (settled by later work)`);
  closing one sets `status: closed` and an optional `closeNote` for the file. The
  user reads and answers there. `docs/discussions/YYYY-MM-DD-topic.qmd` is the record in
  the repo: numbered questions with `- [ ] A:` lines (ticked = agreed), written when the
  discussion starts and brought up to date with the board's answers at every review.
  Pictures go to the board's assets and to `docs/discussions/img/`.
- **Dev board** (experiment since 2026-11-24): <https://claude.ai/artifact/29FwBMrCPy8fhseivVL5Zs>,
  read and written with the `ArtifactData` tool. Collections: `items` (features, todos,
  ideas, designs, discussions with their `questions`), `decisions`, `scratch` (the user's
  quick ideas), `notes` (per item, `by: "you" | "claude"`), `meta/state` (`lastReview`).
  When the user says "review the dev board": read new scratch notes, notes from the user
  and items or decisions changed since `lastReview`; copy answers into the discussion
  files' `A:` lines, move decisions the user marked decided into **Decided**, rewrite
  decisions the user corrected (`original` holds the old text) in DECISIONS.md, turn scratch
  notes into items (and TODO.md entries when they are tasks) and mark them `read` with a
  `result`, answer notes with a `claude` note; set `lastReview` and a one-line
  `reviewNote`; commit. When the repo changes (a new discussion, a feature built or
  verified), update the board in the same step. A new discussion is created on the board
  and in `docs/discussions/` in the same step.
- `upstream/` holds read-only clones of jami-daemon, jami-client-qt and gliff for
  reference (ignored by git). gliff's crates are used as git dependencies pinned to a
  reviewed revision; `crates/joma-desk/src/rate.rs` is vendored from gliff (MIT), and so
  is `crates/hypr-capture` (window capture added, see its README).
- In the nest, run `grim` with the nest's `WAYLAND_DISPLAY` set:
  with the host's display, `grim -o HEADLESS-1` captures the user's real screen.
- **Never touch the user's real Jami data or desktop in tests.** Joma shares
  `~/.local/share/jami` with the Jami app. Tests run with `JOMA_PROFILE=<dir>` (its own
  data, config and socket) and `HOME=<dir>/home` after `scripts/test-profile.sh <dir>`
  (silent ALSA audio), inside the hidden nested Hyprland of `scripts/nest.sh`. A test
  profile skips notifications and the desktop portal, and never opens the real camera
  (`JOMA_CAMERA=file:///…/clip.mp4` gives it one). Known limit: the ALSA `null`
  capture is unpaced, so libjami's `AudioInput::readFromDevice` spins during a test call
  and hanging up blocks in `SIPCall::stopAllMedia` (joins that loop). End test calls by
  stopping the profiles; real audio devices are paced and not affected.
- **Check that a test profile is silent before any audio test** (`grep audioApi
  <profile>/config/jami/dring.yml` says `alsa`, plugin `nulldev`): on 2026-10-31 carol and
  dave were on pulseaudio (real microphone and speakers) through a test call. With the
  null device, libjami's local recorder spins like the calls do and stopping it never
  returns: voice messages cannot be recorded in the nest (Joma stops it off the UI
  thread; send a prepared `.ogg` with `joma send-file` to test the rest).
- **Test profiles never register usernames**: the public name server cannot take a name
  back (`register_name` refuses under `JOMA_PROFILE`). On 2026-10-29 a retried onboarding
  typed into the username step and registered `arol` for a throwaway account. Check the
  state before retyping into a flow.
- In the nest, `grim -o $NEST_HEADLESS` takes screenshots (plain `grim` waits forever);
  `hyprctl dispatch 'hl.dsp.focus({ window = "pid:<pid>" })'` picks the window `wtype`
  types into.
- **Never run codec experiments on the GPU that drives the screen** without asking the
  user first. A hung encoder resets its GPU, and a failed reset takes the desktop down:
  this happened on 2026-10-25 (VA-API encode on the Radeon 550, see `crates/joma-desk/src/va.rs`).
  `joma probe --bench` skips that GPU unless given `--display-gpu`.
- Build: `.cargo/config.toml` (local, ignored) builds into
  `/home/dzanaga/builds/joma-target` (since 2026-11-17: the internal disk has room;
  before, an external T5 volume shared with Vetro). Never build inside the repository.
- **Stop test instances with `scripts/stop-tests.sh`** (it matches `JOMA_PROFILE` in the
  process environment), never by binary path or name: on 2026-11-17 the user's launcher
  pointed at the dev build (`~/.local/bin/joma` → the target folder) and a cleanup by
  path stopped their own Joma. For the same reason a `cargo build` changes what the
  user's launcher starts next.
- Clicks and drags in the nest: `cargo run -p joma-desk --example pointer -- <w> <h>
  move:x,y down up right sleep:ms` (a virtual pointer over the whole layout; `wtype`
  only types). `joma ui <part>` opens overlays; `scripts/shots.sh` screenshots every tab.
- Commit after every change once tests pass (`cargo test --workspace`).
- Docs and code in English.
- **TODO.md** (local hotfix list, not committed): before starting any task, check it for unticked `- [ ]` items and ask the user whether to fix those first (if the user asked for the TODOs, start there directly). Only checkbox items are tasks; other lines are the user's notes, leave them alone. When an item is fixed and committed, tick it (`- [x]`) and append the commit hash. The user removes an item once verified.
