#!/usr/bin/env bash
# Prepare a silent JOMA_PROFILE for tests: Jami's audio goes to an ALSA sink that drops
# everything (muted both ways), so test calls never touch the real microphone or
# speakers. Run it after the profile's first start (the daemon writes dring.yml), with
# Joma stopped, then start Joma with HOME=<profile>/home so ALSA reads that .asoundrc:
#   scripts/test-profile.sh /tmp/claude-1000/jt/a
#   HOME=/tmp/claude-1000/jt/a/home JOMA_PROFILE=/tmp/claude-1000/jt/a joma
# (libjami cannot run calls without local audio: NO_LOCAL_AUDIO/VIDEO crash it.)
set -euo pipefail
dir="${1:?profile dir}"
mkdir -p "$dir/home"
cat >"$dir/home/.asoundrc" <<'ALSA'
# Test profile: every Jami audio device is a sink that drops everything.
pcm.nulldev {
    @args [ CARD ]
    @args.CARD { type string default "0" }
    type null
}
ALSA
f="$dir/config/jami/dring.yml"
sed -i 's/^    plugin: .*/    plugin: nulldev/; s/^  audioApi: .*/  audioApi: alsa/; s/^  captureMuted: false/  captureMuted: true/; s/^  playbackMuted: false/  playbackMuted: true/' "$f"
grep -E 'plugin|audioApi|Muted' "$f"
