# Account backups

D-24, discussion `docs/discussions/2026-11-26-github.qmd` (step 1 built: backup and restore
with `gh` or a file).

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
- **Next.** The GitHub App (device flow), restore on Android, the restore screen on a
  fresh install, the badge, automatic backups.
