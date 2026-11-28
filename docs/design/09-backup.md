# Account backups

D-24, discussion `docs/discussions/2026-11-26-github.qmd` (step 1 built: backup and restore
with `gh` or a file; step 2 built in 0.5.3.dev1: the restore screen and new devices).

- **What.** Jami's account archive (`exportToFile`: keys, contacts, devices, settings; no
  history), which Joma accounts write unencrypted, encrypted with one password for all
  accounts. Restoring is `addAccount` with `Account.archivePath`: the same Jami id, a new
  device certificate, so it also adds a machine while the old one works.
- **File format** (`crates/joma-backup`, the reference; Android must read it too):
  `JOMA-BACKUP 1\n`, one JSON header line (`kdf: argon2id`, `m_kib` 65536, `t` 3, `p` 1,
  `salt` 16 bytes and `nonce` 12 bytes in base64, `cipher: chacha20-poly1305`, `account`,
  `label`, `created`), `\n`, then the ciphertext with its 16-byte tag. Key = Argon2id
  (v1.3) of the password and salt, 32 bytes; ChaCha20-Poly1305 (IETF) with the header
  line (without its newline) as associated data. `crates/joma-backup/testdata/sample.jomabk`
  (password `joma sample password`, plaintext `sample archive`) is the shared test vector.
- **Where.** `github.com/<login>/joma-backup` (private; created by Joma through `gh`, a
  public one is refused): `<jami id>.jomabk` per account and `index.json` (`joma: 1`,
  `accounts: [{jami, label, file, created, size}]`). Or `--file`.
- **Password.** Asked in Joma's window (masked, typed twice for a backup), never on the
  command line; the command waits up to 15 minutes. At least 10 characters, not one
  character repeated. The plain archive only exists in `$XDG_RUNTIME_DIR/joma-backup/`
  (0700) while it is encrypted or imported.
- **Tested** (2026-11-27, test profiles): backup to a file with the password typed in the
  window; restore into another profile: wrong password refused, right one restores the
  account with the same Jami id. The GitHub path is not run in tests (it would create the
  repo in the user's account).
- **Restore screen** (0.5.3.dev1). The welcome screen's "restore from your backup" reads
  `index.json` through `gh` off the UI thread, lists the accounts (those already here
  greyed), and asks the password once in the card; a wrong one asks again. `joma
  restore` on the welcome screen shows the same card. The restored account opens by
  itself (`AccountsChanged`).
- **New devices** (question 13). `seen.rs` keeps, per account, the devices this machine
  has shown (`$XDG_DATA_HOME/joma/devices-seen/<account>`). A device that appears while
  this machine is not linking one, and not in the first 5 minutes of the account on this
  machine, is new: Devices asks keep / revoke until one is chosen.
- **Tested** (2026-11-27, test profiles): a file backup of `sync-a`, the welcome screen's
  restore without a `gh` login (the error and its hint), `joma restore --file` on a fresh
  profile with a wrong then the right password; the original profile lists the restored
  device as new, `joma devices keep` keeps it; the restored profile shows none as new.
  The devices tab's card was not seen (the host's screen was off, so the nest did not
  draw).
- **Badge** (step 3, 0.5.3.dev2, `crates/joma/src/github.rs`). Our gist `joma.json` lists
  only the accounts switched public (user, 2026-11-27: backed up is not public), written
  with `gh api gists`; the profile line `X-JOMA-GITHUB:<login>` is written into
  `profile.vcf` and sent with `updateProfile` (which keeps unknown lines), and put back
  at start when missing (question 14). Contacts' claims are read with `curl` from the
  public API without a login (60 calls an hour), matched on the numeric `github_id`,
  cached per account in `$XDG_DATA_HOME/joma/github/<account>.json`, checked again
  after 3 days. A contact added by GitHub name is pinned to the login; when its gist
  lists other accounts, the conversation offers to add them (question 8, user
  2026-11-27). `JOMA_GITHUB_API=http://127.0.0.1:…` points a test profile at a stand-in.
- **Tested** (2026-11-27, test profile `links` with a local stand-in API): `joma github
  find` (JSON, and an unknown user), `joma github add` pins and verifies the contact;
  after the stand-in's gist listed another account, the restart's check dropped the
  badge and offered the new one. Publishing to a real gist was not run (it would write
  to your GitHub); the header, the add card and the banner were not seen on screen (the
  host's screen was off).
- **The GitHub App** (step 4, 0.5.3.dev3). Registered by the user on 2026-11-28 from a
  manifest (`scripts/github-app.sh`: a local page posts it to GitHub, GitHub sends the
  browser back with a code, the script keeps the app id, slug and client ID in
  `crates/joma/github-app.json` and drops the private key and secret). Permissions:
  Gists write, Contents write, Metadata read. Device flow in `joma_backup::device`
  (curl, form through stdin); the token goes to `joma_backup::gh::set_token` and every
  `gh api`-style call then goes through curl with the token in a private header file
  (`-H @file`), never on a command line. gh still wins when it is logged in. The app
  cannot create repos (no Administration permission, as agreed): Joma names
  `github.com/new?name=joma-backup&visibility=private` and the app's install page.
- **Automatic backups** (step 5, user 2026-11-28: a password prompt by default, the
  keyring as an option). `crates/joma/src/keyring.rs` (`secret-tool`, secrets through
  stdin; a test profile uses private files under its folder instead), `signin.rs`
  (`Auto` in `$XDG_CONFIG_HOME/joma/backup.toml`: on, last). Turning on asks the password
  (twice), backs up all accounts, then keeps password and refresh token; the tick runs
  one backup a day (a failure retries after an hour); turning off clears both.
- **Tested** (2026-11-28, profile `sa`, `JOMA_GITHUB_WEB`/`JOMA_GITHUB_API` at a local
  stand-in without gh): sign-in (pending twice, then the token), a backup refused with
  the create-it-yourself message when the stand-in refuses `user/repos`, automatic
  backups on (repo created, account and index written, password and refresh token
  kept), after a restart the daily backup ran with a refreshed token, off cleared both.
  The device flow against the real GitHub waits on "Enable Device Flow" in the app's
  settings (checked: GitHub answers `device_flow_disabled`).
- **Next.** Restore on Android (A1).
