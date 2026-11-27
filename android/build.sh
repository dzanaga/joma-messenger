#!/usr/bin/env bash
# Build joma for Android: Jami Android (pinned in jami.env) with Joma's overlay and
# patches, into $JOMA_ANDROID_BUILD (default ~/builds/joma-android), never inside the
# repository. Steps:
#   1. fetch the pinned client and daemon into $OUT/src (once);
#   2. copy the client's gradle project into $OUT/work (the daemon is linked, so its
#      contrib build in daemon/contrib/ is kept between builds);
#   3. lay android/overlay over it, generate the icons, apply android/patches;
#   4. gradle assembleNoPushDebug for $ABIS (default arm64-v8a), copy the APK to
#      $OUT/joma-<version>.apk.
# Usage: android/build.sh [prepare|daemon|apk] (default apk).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(dirname "$HERE")"
# shellcheck source=jami.env
source "$HERE/jami.env"
OUT="${JOMA_ANDROID_BUILD:-$HOME/builds/joma-android}"
SRC="$OUT/src/jami-client-android"
WORK="$OUT/work"
ABIS="${ABIS:-arm64-v8a}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
STEP="${1:-apk}"
# Cross builds must not see this machine's pkg-config files (gnutls found the host's
# tss2 and failed): an empty default path; the contrib build's PKG_CONFIG_PATH still applies.
export PKG_CONFIG_LIBDIR="$OUT/no-host-pkgconfig"
mkdir -p "$PKG_CONFIG_LIBDIR"

case "$OUT" in "$REPO"/*) echo "refusing to build inside the repository: $OUT" >&2; exit 1 ;; esac

# swig, yasm, nasm, gperf and meson: the system's, else Arch's packages unpacked under
# $OUT/hosttools (no root needed; `android/build.sh hosttools` fetches and checks them).
hosttools() {
    local t="$OUT/hosttools"
    mkdir -p "$t/pkgs" "$t/root"
    for u in $(pacman -Sp swig yasm nasm gperf meson python-tqdm); do
        local f="$t/pkgs/${u##*/}"
        [ -f "$f" ] || { curl -sSfL -o "$f" "$u"; curl -sSfL -o "$f.sig" "$u.sig"; }
        gpg --homedir /etc/pacman.d/gnupg --verify "$f.sig" "$f" 2>/dev/null
        tar -C "$t/root" -xf "$f" --exclude .PKGINFO --exclude .BUILDINFO --exclude .MTREE --exclude .INSTALL
    done
}
need_meson=no
command -v meson >/dev/null || need_meson=yes
if ! command -v swig >/dev/null && [ -x "$OUT/hosttools/root/usr/bin/swig" ]; then
    export PATH="$OUT/hosttools/root/usr/bin:$PATH"
    SWIG_LIB="$(echo "$OUT"/hosttools/root/usr/share/swig/*)"
    export SWIG_LIB
fi
if [ "$need_meson" = yes ] && [ -x "$OUT/hosttools/root/usr/bin/meson" ]; then
    export PATH="$PATH:$OUT/hosttools/root/usr/bin"
    PYTHONPATH="$(echo "$OUT"/hosttools/root/usr/lib/python3*/site-packages)${PYTHONPATH:+:$PYTHONPATH}"
    export PYTHONPATH
fi

fetch() {
    if [ ! -d "$SRC/.git" ]; then
        mkdir -p "$(dirname "$SRC")"
        git clone --depth 1 --revision="$JAMI_ANDROID_REV" "$JAMI_ANDROID_URL" "$SRC"
    fi
    if [ ! -d "$SRC/daemon/.git" ]; then
        rmdir "$SRC/daemon" 2>/dev/null || true
        git clone --depth 1 --revision="$JAMI_DAEMON_REV" "$JAMI_DAEMON_URL" "$SRC/daemon"
    fi
    local have
    have="$(git -C "$SRC" rev-parse HEAD)"
    [ "$have" = "$JAMI_ANDROID_REV" ] || { echo "client at $have, jami.env pins $JAMI_ANDROID_REV: remove $SRC" >&2; exit 1; }
    have="$(git -C "$SRC/daemon" rev-parse HEAD)"
    [ "$have" = "$JAMI_DAEMON_REV" ] || { echo "daemon at $have, jami.env pins $JAMI_DAEMON_REV: remove $SRC/daemon" >&2; exit 1; }
}

prepare() {
    mkdir -p "$WORK/jami-android"
    ln -sfn "$SRC/daemon" "$WORK/daemon"
    # A fresh copy of the upstream sources every time (patched files return to their
    # original first); build outputs and the generated SWIG bindings are kept.
    rsync -a --delete \
        --exclude '/build/' --exclude '/app/build/' --exclude '/libjamiclient/build/' \
        --exclude '/app/.cxx/' --exclude '/.gradle/' --exclude '/.kotlin/' \
        --exclude '/libjamiclient/src/main/java/net/jami/daemon/' \
        --exclude '/app/src/collabeditor/node_modules/' \
        --exclude '/local.properties' --exclude '/joma/' \
        "$SRC/jami-android/" "$WORK/jami-android/"
    rm -rf "$WORK/jami-android/joma"
    rsync -a "$HERE/overlay/" "$WORK/jami-android/"
    python3 -I "$HERE/tools/svg2vd.py" "$REPO/assets/icons" "$WORK/jami-android/joma/res/drawable" >/dev/null
    # The phone takes the desktop's current Omarchy theme (JOMA_ANDROID_THEME=<colors.toml>
    # picks another, JOMA_ANDROID_THEME=default the fallback).
    local theme="${JOMA_ANDROID_THEME:-$HOME/.local/state/omarchy/current/theme/colors.toml}"
    python3 -I "$HERE/tools/brand.py" "$REPO/crates/joma-theme/src/lib.rs" "$WORK/jami-android/joma/res" "$theme"
    python3 -I "$HERE/tools/square.py" "$WORK/jami-android/app/src/main/res" "$WORK/jami-android/joma/res"
    for p in "$HERE"/patches/*.patch; do
        [ -e "$p" ] || continue
        patch -s -p1 --forward --no-backup-if-mismatch -d "$WORK/jami-android" < "$p"
    done
    echo "sdk.dir=$ANDROID_HOME" > "$WORK/jami-android/local.properties"
    # The daemon's Java bindings (net.jami.daemon), as Jami's compile.sh makes them.
    (cd "$SRC/daemon/bin/jni" && PACKAGEDIR="$WORK/jami-android/libjamiclient/src/main/java" ./make-swig.sh >/dev/null)
}

gradle() {
    cd "$WORK/jami-android"
    ./gradlew --no-daemon -Parchs="$ABIS" "$@"
}

[ "$STEP" = hosttools ] && { hosttools; exit 0; }
fetch
prepare
case "$STEP" in
    prepare) ;;
    daemon) gradle buildCMakeDebug ;;
    apk)
        gradle assembleNoPushDebug
        apk="$(ls -t "$WORK"/jami-android/app/build/outputs/apk/noPush/debug/*.apk | head -1)"
        # Named by its version (versionName from patches/0001), e.g. joma-0.1.0.dev2.apk.
        ver="$(sed -n 's/^+ *versionName = "\(.*\)"$/\1/p' "$HERE/patches/0001-joma-identity.patch")"
        cp "$apk" "$OUT/joma-${ver:-debug}.apk"
        echo "$OUT/joma-${ver:-debug}.apk"
        ;;
    *) echo "unknown step: $STEP" >&2; exit 2 ;;
esac
