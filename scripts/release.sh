#!/usr/bin/env bash
# Publish the version in Cargo.toml as a GitHub release: build, strip, pack
# joma-<v>-x86_64-linux.tar.gz (+ .sha256), tag v<v>, push, create the release.
#   scripts/release.sh "notes…" [--pre]
set -euo pipefail
cd "$(dirname "$0")/.."
notes=${1:?release notes}
pre=${2:-}
v=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
target=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')
cargo build --release -p joma
n="joma-$v-x86_64-linux"
w=$(mktemp -d)
mkdir "$w/$n"
cp "$target/release/joma" "$w/$n/joma"
strip "$w/$n/joma"
cp README.md LICENSE "$w/$n/"
(cd "$w" && tar czf "$n.tar.gz" "$n" && sha256sum "$n.tar.gz" >"$n.tar.gz.sha256")
git push -q origin main
git tag "v$v"
git push -q origin "v$v"
flags=()
[ "$pre" = --pre ] && flags+=(--prerelease)
gh release create "v$v" "${flags[@]}" --title "Joma $v" \
  --notes "$notes

Install: \`mise use -g github:dzanaga/joma-messenger@$v\` (needs \`jami-daemon\`)." \
  "$w/$n.tar.gz" "$w/$n.tar.gz.sha256"
rm -rf "$w"
