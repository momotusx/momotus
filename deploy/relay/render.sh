#!/usr/bin/env bash
# Renders the relay's units for one instance (deploy.sh runs it; run it by hand to read them):
#
#   deploy/relay/render.sh deploy/relay/momotus.env OUT_DIR
#
# writes OUT_DIR/<NAME>-relay.service, <NAME>-backup.service and <NAME>-backup.timer from the
# templates next to this file, and OUT_DIR/instance.env: the instance's values, checked and quoted,
# the only form of them deploy.sh sends to the server. bash 3.2 safe (macOS).
set -euo pipefail

INSTANCE=${1:?usage: render.sh INSTANCE_FILE OUT_DIR}
OUT=${2:?usage: render.sh INSTANCE_FILE OUT_DIR}
HERE=$(cd "$(dirname "$0")" && pwd)
NAME="" TITLE="" HOST="" PORT="" PREFLIGHT_PORT="" FLAGS="" CREDENTIALS=""
# shellcheck source=/dev/null # the instance file, momotus.env or another app's
. "$INSTANCE"

# Every value lands in a unit and in a root shell on the server: only what a value of its kind needs.
check() { # name value pattern
  local nl=$'\n'
  if [ -z "$2" ] || [[ $2 == *"$nl"* ]] || printf '%s' "$2" | LC_ALL=C grep -qv -E "^$3\$"; then
    echo "render: $INSTANCE: $1 must match $3" >&2
    exit 1
  fi
}
check NAME "$NAME" '[a-z0-9]+(-[a-z0-9]+)*'
check TITLE "$TITLE" '[A-Za-z0-9 ._()-]+'
check HOST "$HOST" '[a-z0-9]([a-z0-9-]*[a-z0-9])?(\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+'
check PORT "$PORT" '[0-9]{1,5}'
check PREFLIGHT_PORT "$PREFLIGHT_PORT" '[0-9]{1,5}'
[ -z "$FLAGS" ] || check FLAGS "$FLAGS" '[A-Za-z0-9 %/._=-]+'
FILE='[a-z0-9][a-z0-9_-]*(\.[a-z0-9_-]+)*'
[ -z "$CREDENTIALS" ] || check CREDENTIALS "$CREDENTIALS" "$FILE( $FILE)*"
[ "$PORT" != "$PREFLIGHT_PORT" ] || { echo "render: $INSTANCE: PORT and PREFLIGHT_PORT must differ" >&2; exit 1; }

render() { # template
  local line c
  while IFS= read -r line; do
    case $line in
    '#'*) printf '%s\n' "$line" ;;
    @CREDENTIALS@) for c in $CREDENTIALS; do echo "LoadCredential=$c:/etc/$NAME/$c"; done ;;
    *)
      line=${line//@NAME@/$NAME}
      line=${line//@TITLE@/$TITLE}
      line=${line//@PORT@/$PORT}
      line=${line//@HOST@/$HOST}
      line=${line//@FLAGS@/$FLAGS}
      printf '%s\n' "$line"
      ;;
    esac
  done <"$HERE/$1"
}

mkdir -p "$OUT"
render relay.service >"$OUT/$NAME-relay.service"
render backup.service >"$OUT/$NAME-backup.service"
render backup.timer >"$OUT/$NAME-backup.timer"
for v in NAME HOST PORT PREFLIGHT_PORT FLAGS CREDENTIALS; do printf '%s=%q\n' "$v" "${!v}"; done >"$OUT/instance.env"
