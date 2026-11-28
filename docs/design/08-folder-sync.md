# Folder sync

D-23, discussion `docs/discussions/2026-11-24-folder-sync.qmd`.

- **Engine.** Syncthing, Joma's own instance per account (`crates/joma-sync`): home in
  `$XDG_DATA_HOME/joma/syncthing/<account>`, started with sync on and stopped with Joma or
  the account. Listens on 127.0.0.1 only; global and local discovery, relays and NAT
  traversal are off, so it finds nobody by itself. Driven through its REST API with a key
  Joma picks at each start.
- **Pairing and transport.** The listen port is a Jami service `joma-sync` exposed to our
  own account. Each Joma tells its other devices `{dev, st, service, name}` as an account
  message `application/x-joma-sync+json` (at start, when a device comes online, every
  5 minutes; a device that did not know us answers). The device that hears it opens a
  Jami tunnel to the sender's service and gives Syncthing the tunnel's local end
  (`tcp://127.0.0.1:<port>`) as the sender's address. Both sides do it; Syncthing keeps
  one connection.
- **Folders.** Every folder is shared with every device of the account. Folders a device
  shares are accepted by themselves under `~/joma-sync/`. Removing a folder only makes
  Syncthing forget it.
- **Paths.** No size limit for now (user, 2026-11-27). The devices tab and `joma sync
  status` show the Jami path to each device (from the connection list, see connection
  info) and warn on relayed ones. Later: sync only on direct paths, a self-hosted server
  for the rest.
- **Killed, not quit.** A Syncthing left by a Joma that was killed keeps running; the
  next start stops any Syncthing whose `--home` is Joma's own (never the user's), and
  `scripts/stop-tests.sh` stops a test profile's Syncthing.
- **Tested** (2026-11-27): two test profiles linked as two devices of one account, sync
  on in both, a folder added on one: its files reached the other's `~/joma-sync/` in
  5 s and an edit came back in 12 s, over a Jami tunnel on the local network.
- **Contacts and conversations** (0.5.3.dev4, `crates/joma/src/shares.rs`; discussion
  `2026-11-27-files`, question 2). One model: a share is a Syncthing folder with an
  owner and members (`write`, `admin`), kept in `$XDG_DATA_HOME/joma/shares/<account>.json`.
  A member who reads has a `receiveonly` folder. A conversation's files are one share
  per member who sent files (`jc-<conv>-<member>`, under
  `~/joma-sync/conversations/<conversation>/<member>/`, sub-folders by kind), so each
  writes their own. Wire (account messages, same MIME, with `kind`): `offer` (owner to
  members), `join` (a device that holds it: its Syncthing id and the `joma-sync-peers`
  service, exposed to the members only; every member's device that holds it pairs and
  answers), `members`, `leave`. Contacts' devices are added with `autoAcceptFolders`
  off, so nothing arrives without joining; own devices keep accepting everything except
  folders switched off for them. Staggered versioning, 30 days. Joining syncs through
  the joining device; your other devices get it from that one.
- **Not enforced by the other side.** Readers are `receiveonly` because their Joma sets
  it; a modified client could still send changes, which the owner's Syncthing (a
  `sendreceive` folder) would take. Honest members only, for now.
- **Tested** (2026-11-28, two test accounts that are contacts, profiles `fa` and `fb`):
  files sent in the conversation landed in `documents/` and `images/`, the offer reached
  the other side, joining synced 2 MB over a Jami tunnel; deleting and editing on the
  reading side left the owner's files alone; write permission given, a file written by
  the member reached the owner; the reverse direction joined by itself after a restart
  (once the fix that auto-joins conversations we take part in was in); a plain folder
  shared with the contact (`joma sync share`) arrived as `receiveonly`. The files view
  was not seen on screen (the host's screen was off at night).
- **Not yet.** Tailscale straight to the other Syncthing without a tunnel, the folder
  list in the GitHub backup, agents, the phone browsing a home device's folder.
