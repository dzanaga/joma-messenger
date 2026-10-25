#!/usr/bin/env bash
# Install Joma into ~/.local on this machine, or on another one over SSH.
#
#   scripts/install.sh               build (release) and install here
#   scripts/install.sh zavito        build here, copy to zavito and install there
#   scripts/install.sh --no-build …  install the last release build
#
# Installs ~/.local/bin/joma, the icon (hicolor) and ~/.local/share/applications/joma.desktop.
# Joma needs the jami-daemon package (libjami); the script checks for it and says what
# to install, but never installs packages itself.
set -euo pipefail

repo="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
build=1
if [[ ${1:-} == --no-build ]]; then build=0; shift; fi
host="${1:-}"

target="$(cd "$repo" && cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
bin="$target/release/joma"
if ((build)); then
  (cd "$repo" && cargo build --release -p joma)
fi
[[ -x $bin ]] || { echo "no release build at $bin" >&2; exit 1; }

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/bin" "$stage/applications" "$stage/icons/hicolor/scalable/apps"
cp "$bin" "$stage/bin/joma"
cp "$repo/assets/icon/joma.svg" "$stage/icons/hicolor/scalable/apps/joma.svg"
for s in 32 48 64 128 256 512; do
  mkdir -p "$stage/icons/hicolor/${s}x${s}/apps"
  cp "$repo/assets/icon/joma-$s.png" "$stage/icons/hicolor/${s}x${s}/apps/joma.png"
done
cp "$repo/assets/joma.desktop" "$stage/applications/joma.desktop"

# Runs on the target machine, with the staged files in $1.
install_here='
set -e
s="$1"
mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons
install -m755 "$s/bin/joma" ~/.local/bin/joma.new && mv ~/.local/bin/joma.new ~/.local/bin/joma
cp -r "$s/icons/." ~/.local/share/icons/
sed "s|@BIN@|$HOME/.local/bin/joma|" "$s/applications/joma.desktop" > ~/.local/share/applications/joma.desktop
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t ~/.local/share/icons/hicolor 2>/dev/null || true
command -v update-desktop-database >/dev/null && update-desktop-database ~/.local/share/applications 2>/dev/null || true
echo "installed $(~/.local/bin/joma --version) on $(hostname)"
if ! pacman -Q jami-daemon >/dev/null 2>&1; then
  echo "missing: the jami-daemon package. Install it with: sudo pacman -S jami-daemon"
fi
missing=$(ldd ~/.local/bin/joma | grep "not found" || true)
[ -z "$missing" ] || { echo "libraries not found:"; echo "$missing"; }
'

if [[ -z $host ]]; then
  bash -c "$install_here" _ "$stage"
else
  remote="$(ssh "$host" mktemp -d)"
  rsync -a --info=progress2 "$stage/" "$host:$remote/"
  ssh "$host" bash -c "$(printf '%q' "$install_here")" _ "$remote"
  ssh "$host" rm -rf "$remote"
fi
