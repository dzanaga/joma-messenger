#!/usr/bin/env bash
# A headless emulator for smoke tests of the APK: Google APIs x86_64 (API 34), which runs
# arm64 apps through its ARM translation, so the arm64-only APK installs as it is.
# Software rendering (swiftshader), no audio, no window; its AVD lives under
# $OUT/avd, away from ~/.android. Usage: emulator.sh start|stop|shot <file.png>
set -euo pipefail
OUT="${JOMA_ANDROID_BUILD:-$HOME/builds/joma-android}"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_AVD_HOME="$OUT/avd"
ADB="$ANDROID_HOME/platform-tools/adb"
IMAGE="system-images;android-34;google_apis;x86_64"
NAME=joma-smoke

case "${1:-start}" in
    start)
        mkdir -p "$ANDROID_AVD_HOME"
        if [ ! -d "$ANDROID_AVD_HOME/$NAME.avd" ]; then
            echo no | "$ANDROID_HOME/cmdline-tools/latest/bin/avdmanager" create avd \
                -n "$NAME" -k "$IMAGE" -d pixel_6 -p "$ANDROID_AVD_HOME/$NAME.avd"
        fi
        nohup "$ANDROID_HOME/emulator/emulator" -avd "$NAME" -no-window -no-audio \
            -gpu swiftshader_indirect -no-snapshot -no-boot-anim -memory 4096 \
            > "$OUT/emulator.log" 2>&1 &
        "$ADB" wait-for-device
        until [ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do sleep 2; done
        echo booted
        ;;
    stop) "$ADB" emu kill || true ;;
    shot) "$ADB" exec-out screencap -p > "$2" ;;
    *) echo "usage: $0 start|stop|shot <file>" >&2; exit 2 ;;
esac
