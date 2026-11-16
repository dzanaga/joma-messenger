#!/usr/bin/env bash
# Screenshot every tab of a test profile in the hidden nest, with the user's Omarchy
# theme and font copied into the profile:
#   scripts/shots.sh <profile dir> <out dir> [joma binary]
# The profile must already hold an account (see CLAUDE.md on test profiles).
set -euo pipefail
profile=${1:?profile dir}; out=${2:?out dir}
bin=${3:-$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')/debug/joma}
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out" "$profile/home/.local/state/omarchy/current/theme" "$profile/home/.config/alacritty"
cp ~/.local/state/omarchy/current/theme/colors.toml "$profile/home/.local/state/omarchy/current/theme/" 2>/dev/null || true
cp ~/.config/alacritty/alacritty.toml "$profile/home/.config/alacritty/" 2>/dev/null || true
eval "$("$here/nest.sh" start)"
cd "$profile"
JOMA_TRAY=0 HOME="$profile/home" JOMA_PROFILE="$profile" setsid "$bin" >"$profile/joma.log" 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null || true; "$here/nest.sh" stop' EXIT
sleep "${SHOTS_WAIT:-9}"
hyprctl dispatch "hl.dsp.focus({ window = \"pid:$pid\" })" >/dev/null || true
shot() { sleep 0.8; timeout 10 grim -o "$NEST_HEADLESS" "$out/$1.png" || echo "no screenshot: $1" >&2; }
n=1
for tab in chats calls contacts devices accounts agents; do
  wtype -M ctrl "$n" -m ctrl; shot "$tab"; n=$((n + 1))
done
wtype -M ctrl 1 -m ctrl
wtype -k F1; shot keys; wtype -k Escape
# Extra steps: each argument after the binary is "name:wtype args" (e.g.
# "find:-M ctrl k -m ctrl") or "name:ui:<what>" (`joma ui`, e.g. "settings:ui:settings").
shift 3 2>/dev/null || shift $#
joma() { HOME="$profile/home" JOMA_PROFILE="$profile" "$bin" "$@" >/dev/null; }
for step in "$@"; do
  name=${step%%:*}; rest=${step#*:}
  case $rest in
    ui:*) joma ui "${rest#ui:}" ;;
    # shellcheck disable=SC2086
    *) wtype $rest ;;
  esac
  shot "$name"
done
