#!/usr/bin/env bash
# Write android/patches/<name>.patch from files edited in the work tree
# ($OUT/work/jami-android), compared with the pinned upstream copy.
# Usage: mkpatch.sh <name> <path relative to jami-android>...
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="${JOMA_ANDROID_BUILD:-$HOME/builds/joma-android}"
UP="$OUT/src/jami-client-android/jami-android"
WORK="$OUT/work/jami-android"
name="$1"; shift
out="$HERE/patches/$name.patch"
: > "$out"
for f in "$@"; do
    diff -u --label "a/$f" --label "b/$f" "$UP/$f" "$WORK/$f" >> "$out" || true
done
echo "$out: $(grep -c '^@@' "$out") hunks"
