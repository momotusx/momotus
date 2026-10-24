#!/usr/bin/env bash
# shellcheck disable=SC2029 # the server-side paths are named on the client side, on purpose
# On the Mac, daily and at login (launchd, app.marquee.relay-backup.plist): copies the server's
# newest relay backup set into ~/.local/share/momotus/relay-backups (plans/accounts-social-sync.md
# H9), checks its sums, keeps 7, and shows a notification when the last good pull or the public
# health check is older than 48 hours.
#
#   MQ_RELAY_SSH=<ssh target> deploy/relay/pull-backup.sh
#
# The dry mode the tests use: MQ_PULL_SOURCE (a local folder in place of the server's),
# MQ_PULL_DEST, MQ_PULL_HEALTH, MQ_PULL_MIN_FREE_KB, MQ_PULL_NOW and MQ_PULL_NOTIFY (a file that
# collects the notifications).
set -euo pipefail

DEST=${MQ_PULL_DEST:-$HOME/.local/share/momotus/relay-backups}
HEALTH=${MQ_PULL_HEALTH:-https://momotus.normalisdead.mom/v1/relay}
MIN_FREE_KB=${MQ_PULL_MIN_FREE_KB:-5242880}
NOW=${MQ_PULL_NOW:-$(date +%s)}
KEEP=7
STALE_S=$((48 * 3600))
FRESH_S=$((36 * 3600))
REMOTE_DIR=/var/lib/momotus/backups

mkdir -p "$DEST"
log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $1" >>"$DEST/pull.log"; }
notify() {
  if [ -n "${MQ_PULL_NOTIFY:-}" ]; then
    echo "$1" >>"$MQ_PULL_NOTIFY"
  else
    osascript -e "display notification \"$1\" with title \"Marquee relay backup\"" || true
  fi
}

list() {
  if [ -n "${MQ_PULL_SOURCE:-}" ]; then ls -1 "$MQ_PULL_SOURCE"; else ssh "${MQ_RELAY_SSH:?}" "sudo -n ls -1 $REMOTE_DIR"; fi
}
fetch() {
  if [ -n "${MQ_PULL_SOURCE:-}" ]; then cat "$MQ_PULL_SOURCE/$1"; else ssh "${MQ_RELAY_SSH:?}" "sudo -n cat $REMOTE_DIR/$1"; fi
}

pull() {
  local free newest stamp tmp
  free=$(df -Pk "$DEST" | awk 'NR == 2 { print $4 }')
  if [ "$free" -lt "$MIN_FREE_KB" ]; then
    log "skipped: $((free / 1024)) MiB free, under $((MIN_FREE_KB / 1024)) MiB"
    return 0
  fi
  newest=$(list | grep -E '^[0-9TZ-]+\.SHA256SUMS$' | sort | tail -n 1)
  [ -n "$newest" ] || { log "failed: no finished backup on the server"; return 1; }
  stamp=${newest%.SHA256SUMS}
  # A good pull is a recent backup: a server whose nightly run stopped
  # keeps offering the same old set, and that must not read as fine.
  made=$(date -j -u -f "%Y-%m-%dT%H%M%SZ" "$stamp" +%s 2>/dev/null || echo 0)
  fresh=$((NOW - made < FRESH_S))
  if [ -f "$DEST/$newest" ]; then
    if [ "$fresh" = 1 ]; then echo "$NOW" >"$DEST/.last-pull"; fi
    log "up to date: $stamp$([ "$fresh" = 1 ] || echo ", older than 36 hours")"
    return 0
  fi
  tmp=$(mktemp -d "$DEST/.pull.XXXXXX")
  for file in "$stamp.sqlite" "$stamp.roster.jsonl" "$newest"; do fetch "$file" >"$tmp/$file"; done
  if ! (cd "$tmp" && shasum -a 256 -c "$newest" >/dev/null 2>&1); then
    rm -rf "${tmp:?}"
    log "failed: $stamp does not match its sums; nothing kept"
    return 1
  fi
  # The data first, the sums last: a set without its sums is unfinished.
  mv "$tmp/$stamp.sqlite" "$tmp/$stamp.roster.jsonl" "$DEST"/
  mv "$tmp/$newest" "$DEST"/
  rmdir "$tmp"
  if [ "$fresh" = 1 ]; then echo "$NOW" >"$DEST/.last-pull"; fi
  log "pulled: $stamp$([ "$fresh" = 1 ] || echo ", older than 36 hours")"
  # The newest $KEEP sets stay.
  find "$DEST" -maxdepth 1 -name '*.SHA256SUMS' -exec basename {} \; | sort -r | tail -n +$((KEEP + 1)) | while read -r old; do
    rm -f "${DEST:?}/${old%.SHA256SUMS}".*
  done
}

# The pulled copies hold accounts' sealed data: never kept longer by Time Machine.
[ -n "${MQ_PULL_SOURCE:-}" ] || tmutil addexclusion "$DEST" 2>/dev/null || true

status=0
pull || status=1
if curl -fsS --max-time 10 "$HEALTH" >/dev/null 2>&1; then echo "$NOW" >"$DEST/.last-health"; else log "health check failed: $HEALTH"; fi

for marker in pull health; do
  last=$(cat "$DEST/.last-$marker" 2>/dev/null || echo 0)
  if [ $((NOW - last)) -gt "$STALE_S" ]; then notify "No good relay $marker for more than 48 hours. See $DEST/pull.log"; fi
done
exit "$status"
