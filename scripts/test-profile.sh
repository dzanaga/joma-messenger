#!/usr/bin/env bash
# Make a JOMA_PROFILE silent: Jami's audio goes to ALSA's null device, muted both ways,
# so test calls never touch the real microphone or speakers. Run once the profile's
# daemon has written its config (after the first start), with Joma stopped.
set -euo pipefail
f="${1:?profile dir}/config/jami/dring.yml"
sed -i 's/^    plugin: default/    plugin: "null"/; s/^  audioApi: pulseaudio/  audioApi: alsa/; s/^  captureMuted: false/  captureMuted: true/; s/^  playbackMuted: false/  playbackMuted: true/' "$f"
grep -E 'plugin|audioApi|Muted' "$f"
