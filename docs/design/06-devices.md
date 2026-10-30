# Your devices

A Jami account lives on several devices (D-18). Joma uses Jami's linking and adds a
"you" group in Contacts.

- **Linking** (daemon 2025+, `jami-auth`): the new device creates an account with
  `Account.archiveURL = jami-auth`; `DeviceAuthStateChanged` brings the token
  (`jami-auth://<id>/<pin>`, shown as text and QR). The old device calls `addDevice`
  with it; `AddDeviceStateChanged` reports CONNECTING, then AUTHENTICATING with the new
  device's address, and `confirmAddDevice` sends the archive. The new device answers its
  own AUTHENTICATING with `provideAccountAuthentication` (empty for accounts without a
  password) and ends with DONE (error empty on success; else the empty account is
  removed). `joma link` / `joma link-device` do the same from a shell; the latter
  confirms without asking.
- **A linked account learns things late:** this device's id is in the *volatile*
  details (`Account.deviceID`), the name may be only in `profile.vcf`. Joma re-reads
  them when the account registers (`refresh_identity`) and never writes a profile from
  an empty name.
- **Presence between your devices:** the presence wire carries `dev`; Joma sends it to
  its own URI (all our devices get account messages sent to it) every 4 minutes and on a
  change, whatever "invisible" says. Old Joma versions ignore the field.
- **Delivery:** nothing of Joma's own. Jami announces a new commit to our other devices
  first; a contact syncs from any member device it reaches. Tested in the nest: sender
  offline, contact back later, the second device delivered within 10 s.
- **Accounts (J8, first part):** the daemon keeps every account enabled; Joma shows one
  (`quick.toml` `account`, else the first). Switching closes the view (refused during a
  call), tells contacts we left, and opens the other. "Other account" goes back to the
  welcome screen remembering the accounts there were: the new one is the one not among
  them. Removing calls `removeAccount` (local only; other devices keep the account, and
  this device stays in their list until revoked).
