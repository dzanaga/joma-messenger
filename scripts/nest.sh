#!/usr/bin/env bash
# A hidden nested Hyprland for tests: its window sits on a special workspace of the host
# (never focused), with a headless output inside. Prints the env to use it:
#   eval "$(scripts/nest.sh start)"   …   scripts/nest.sh stop
set -euo pipefail
dir=${NEST_DIR:-/tmp/claude-1000/joma-nest}
case "${1:-start}" in
start)
  mkdir -p "$dir"
  hyprctl eval 'hl.window_rule({ match = { class = "^(hyprland|Hyprland|aquamarine)$" }, workspace = "special:jomanest silent", no_initial_focus = true })' >/dev/null
  cat >"$dir/hypr.lua" <<'LUA'
hl.monitor({ output = "", mode = "1280x800", position = "auto", scale = 1 })
hl.config({ misc = { force_default_wallpaper = 0, disable_hyprland_logo = true }, animations = { enabled = false } })
LUA
  before=$(ls -1 "$XDG_RUNTIME_DIR/hypr" | sort)
  env -u HYPRLAND_INSTANCE_SIGNATURE setsid Hyprland --config "$dir/hypr.lua" >"$dir/hypr.log" 2>&1 &
  echo $! >"$dir/pid"
  sig=""
  for _ in $(seq 50); do
    sleep 0.2
    sig=$(comm -13 <(echo "$before") <(ls -1 "$XDG_RUNTIME_DIR/hypr" | sort) | head -1)
    [ -n "$sig" ] && [ -S "$XDG_RUNTIME_DIR/hypr/$sig/.socket.sock" ] && break
  done
  [ -n "$sig" ] || { echo "nested Hyprland did not start" >&2; exit 1; }
  sleep 1
  HYPRLAND_INSTANCE_SIGNATURE=$sig hyprctl output create headless >/dev/null
  sleep 0.5
  wl=$(HYPRLAND_INSTANCE_SIGNATURE=$sig hyprctl -j instances 2>/dev/null | python3 -c "import json,sys;print(next(i['wl_socket'] for i in json.load(sys.stdin) if i['instance']=='$sig'))")
  head=$(HYPRLAND_INSTANCE_SIGNATURE=$sig hyprctl -j monitors | python3 -c "import json,sys;print(next(m['name'] for m in json.load(sys.stdin) if m['name'].startswith('HEADLESS')))")
  HYPRLAND_INSTANCE_SIGNATURE=$sig hyprctl dispatch "hl.dsp.focus({ monitor = \"$head\" })" >/dev/null || true
  echo "$sig" >"$dir/sig"
  echo "export HYPRLAND_INSTANCE_SIGNATURE=$sig WAYLAND_DISPLAY=$wl NEST_HEADLESS=$head"
  ;;
stop)
  [ -f "$dir/pid" ] && kill "$(cat "$dir/pid")" 2>/dev/null || true
  rm -f "$dir/pid" "$dir/sig"
  ;;
esac
