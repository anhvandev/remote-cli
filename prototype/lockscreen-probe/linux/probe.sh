#!/bin/sh
# PROTOTYPE, throwaway. Question: on Wayland, can a root process capture the screen (DRM/KMS)
# and inject a key (uinput) at the lock screen and at the login greeter?
# Run detached so it survives logout:  sudo systemd-run --unit=lockscreen-probe "$PWD/probe.sh" 30
# Deps: ffmpeg (with kmsgrab + vaapi), python3-evdev.  Env: CARD=/dev/dri/card1 to pick another GPU.
ROUNDS=${1:-30}
OUT=/var/tmp/lockscreen-probe
CARD=${CARD:-/dev/dri/card0}
DIR=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"
log() { echo "$(date -Is) $*" | tee -a "$OUT/probe.log"; }

log "start rounds=$ROUNDS card=$CARD uid=$(id -u)"
i=1
while [ "$i" -le "$ROUNDS" ]; do
  log "round $i"
  for s in $(loginctl list-sessions --no-legend | awk '{print $1}'); do
    log "  session $s $(loginctl show-session "$s" -p Name -p Class -p Type -p State -p LockedHint -p Seat | tr '\n' ' ')"
  done
  python3 "$DIR/uinput_key.py" >>"$OUT/probe.log" 2>&1 && log "  key: sent" || log "  key: FAILED"
  sleep 1
  # vaapi path handles tiled/compressed framebuffers (Intel/AMD); plain hwdownload only works for linear ones.
  if ffmpeg -loglevel error -y -device "$CARD" -f kmsgrab -i - -frames:v 1 \
       -vf 'hwmap=derive_device=vaapi,scale_vaapi=format=nv12,hwdownload,format=nv12' \
       "$OUT/$i-kms.jpg" >>"$OUT/probe.log" 2>&1; then log "  kms(vaapi): $i-kms.jpg"
  elif ffmpeg -loglevel error -y -device "$CARD" -f kmsgrab -i - -frames:v 1 -vf 'hwdownload,format=bgr0' \
       "$OUT/$i-kms.jpg" >>"$OUT/probe.log" 2>&1; then log "  kms(linear): $i-kms.jpg"
  else log "  kms: FAILED"; fi
  sleep 9
  i=$((i + 1))
done
log done
