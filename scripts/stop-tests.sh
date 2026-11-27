#!/usr/bin/env bash
# Stop the Joma test instances, and only those: processes started with JOMA_PROFILE.
# Never match by binary path or name: the user's own Joma may run the same dev build.
for p in $(pgrep -x joma); do
  if tr '\0' '\n' <"/proc/$p/environ" 2>/dev/null | grep -q '^JOMA_PROFILE='; then
    kill "$p" && echo "stopped test instance $p"
  fi
done
# Joma's own Syncthing (folder sync) inherits JOMA_PROFILE: stop it too.
for p in $(pgrep -x syncthing); do
  if tr '\0' '\n' <"/proc/$p/environ" 2>/dev/null | grep -q '^JOMA_PROFILE='; then
    kill "$p" 2>/dev/null && echo "stopped test syncthing $p"
  fi
done
