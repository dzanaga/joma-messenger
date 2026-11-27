# joma for Android

A first sideloadable APK (0.1.0.devN, arm64) so joma is on the phone too: same name,
icon, colours, font and words as the desktop, the same Jami network and accounts.

## Architecture

```
android/jami.env       Jami Android + daemon revisions (pinned, like gliff's crates)
android/build.sh       fetch → ~/builds/joma-android/src, copy → work/, overlay, patches, gradle
android/overlay/joma/  res/: icon, font, styles, strings (a flavor's res wins over main's)
android/patches/       small diffs to Jami Android's sources (application id, defaults)
android/tools/         svg2vd.py (assets/icons → vector drawables), brand.py (palette,
                       font aliases), mkpatch.sh (work-tree edits → a patch)
```

The app is **Jami Android with joma's skin**: its daemon (libjami and contribs, built
by its CMake for `arm64-v8a`), its service layer (`libjamiclient`: accounts,
conversations, calls, files) and its screens, rebranded. Nothing of Jami Android is
copied into the repository; the build copies the pinned sources to
`~/builds/joma-android/work`, lays the overlay over them and applies the patches. The
overlay is added as the `noPush` flavor's resource folder, so each file names only what
it changes. The APK ends at `~/builds/joma-android/joma-<version>.apk` (for example `joma-0.1.0.dev2.apk`).

- **Identity:** `applicationId io.github.dzanaga.joma` (installs beside the Jami app,
  own data), name `joma`, the j icon (adaptive, with a themed-icon layer), the j in
  white as the notification icon.
- **Look (0.1.0.dev2, the desktop's 0.5.2):** the Omarchy theme of the machine that
  builds it (`~/.local/state/omarchy/current/theme/colors.toml`, or
  `JOMA_ANDROID_THEME=<colors.toml>`; the desktop's fallback, Tokyo Night, without one),
  read by `brand.py` with the desktop's roles: flat fills, hairlines, your bubbles
  `sent` (a little accent), theirs the panel's fill, icons in the bright text colour,
  a chosen filter as the selection's fill, the accent for state and the main action
  only. Square everywhere: radius dimens at 0, Material's corner styles (search bar,
  pills, the main button) at 0, and `square.py` copies the drawables that write their
  radius in place with 0 (avatars stay round). JetBrains Mono (the desktop's terminal
  font, OFL) everywhere, including Material's type scale. Dark by default.
- **Words:** English only (Jami's translations name Jami); chats, requests, voice
  message, video message, send a file, "add by Jami username or ID"; joma where Jami
  named the app, Jami where it names the network.
- **Background:** the `noPush` flavor (no Google services) with "run in the background"
  on by default: a permanent notification keeps the daemon running, as the desktop
  stays in the tray (D-8).

## Building

`android/build.sh` (`hosttools` | `prepare` | `apk`). Needs the Android SDK in
`~/Android/Sdk` (platform 37, build-tools 37.0.0, NDK 30.0.16248370, CMake 4.1.2:
what Jami's `app/build.gradle.kts` asks for), JDK 17, Rust with the
`aarch64-linux-android` target (Jami's `yffi`), node/npm (Jami's collaborative
editor). swig, yasm, nasm, gperf and meson come from Arch's packages unpacked under
`~/builds/joma-android/hosttools` when the system has none (no root). The first build
compiles libjami's contribs (about an hour); later ones take a minute or two.
Lessons: the client's submodule pin can lag the client (pin the daemon of the
client's date); the host's pkg-config files must stay out of the cross build
(`PKG_CONFIG_LIBDIR`); the SWIG bindings are generated before gradle.
`android/tools/emulator.sh` runs a headless x86_64 emulator (API 34, ARM translation)
for smoke tests.

## Verified (2026-11-27, emulator, 0.1.0.dev1)

Starts; the account wizard (dark, joma's words) creates an account without a username
or password; the chat list; a second local account adds the first by its Jami ID, the
request arrives under Requests, is accepted, messages go both ways (delivered and
read), the notification shows with the j and reply actions; the background service
runs; settings; installing over keeps the data. Not tried: calls, voice and video
messages, files (the emulator has no audio and no files to pick), linking with the
desktop, a real phone.

0.1.0.dev2 (2026-11-27, emulator): the list, the filters and a conversation in the new
look. The splash before the app starts still follows the system's light or dark.

## Decisions (to revisit after testing on the phone)

- A-1 Build on Jami Android's whole app and reskin it, rather than new screens on its
  service layer: everything (groups, voice and video messages, files, calls, link a
  device, notifications) works on day one; joma's own screens can replace Jami's one
  by one later.
- A-2 Overlay and patches on pinned upstream sources, not a fork in the repository:
  the repository stays small and a Jami update is a revision bump plus re-checking the
  patches.
- A-3 No push (no Firebase, no UnifiedPush): a background service instead; costs some
  battery. UnifiedPush is the next candidate if battery matters.
- A-4 The palette of the building machine's Omarchy theme, fixed at build time: phones
  have no Omarchy theme. Following the desktop's theme through sync, or a light
  option, later (discussion 2026-11-27-one-joma, question 9).
- A-5 English only, like the desktop.
- A-6 arm64-v8a only, debug-signed (the Android SDK's debug key, `~/.android`,
  outside the repo: an APK built on another machine cannot update this one).
- A-7 Jami Android's latest commit (2026-11-23) with the daemon of that day, not the
  last release: the client's code needs it, and Jami's releases are cut from master.

## Open questions

- Should joma's own screens (tab strip, tiles, compact rows) replace Jami's, and which first?
- Hide what Jami Android has and joma does not (feeds, SIP accounts, management
  server, donations), or keep it?
- Push (UnifiedPush through a distributor such as ntfy) or the background service?
- A release key and a channel (GitHub releases, F-Droid repository, Obtainium)?
