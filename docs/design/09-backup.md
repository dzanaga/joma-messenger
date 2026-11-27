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
- **Next.** The badge and adding by GitHub name (step 3), automatic backups (5), the
  GitHub App (4), restore on Android.
