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
it changes. The APK ends at `~/builds/joma-android/joma-debug.apk`.

- **Identity:** `applicationId io.github.dzanaga.joma` (installs beside the Jami app,
  own data), name `joma`, the j icon (adaptive, with a themed-icon layer), the j in
  white as the notification icon.
- **Look:** the desktop's fallback palette (Tokyo Night, read from `crates/joma-theme`
  by `brand.py`), dark by default; JetBrains Mono (the desktop's terminal font, OFL)
  everywhere, including Material's type scale; your bubbles on the accent's tint.
- **Words:** English only (Jami's translations name Jami); chats, requests, voice
  message, video message, send a file, "add by Jami username or ID"; joma where Jami
  named the app, Jami where it names the network.
- **Background:** the `noPush` flavor (no Google services) with "run in the background"
  on by default: a permanent notification keeps the daemon running, as the desktop
  stays in the tray (D-8).

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
- A-4 One palette (Tokyo Night) instead of following a theme: phones have no Omarchy
  theme. Material You colours or a choice of Omarchy themes are possible later.
- A-5 English only, like the desktop.
- A-6 arm64-v8a only, debug-signed (the Android SDK's debug key, outside the repo).

## Open questions

- Should joma's own screens (tab strip, tiles, compact rows) replace Jami's, and which first?
- Push (UnifiedPush through a distributor such as ntfy) or the background service?
- A release key and a channel (GitHub releases, F-Droid repository, Obtainium)?
