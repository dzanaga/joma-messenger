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
- **Not yet.** Contacts (folders shared in a conversation), Tailscale straight to the
  other Syncthing without a tunnel, the folder list in the GitHub backup, agents.
