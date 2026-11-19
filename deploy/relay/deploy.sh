#!/usr/bin/env bash
# Builds the relay at the checked-out trunk commit and deploys one instance of it to the server
# (plans/accounts-social-sync.md section 3):
#
#   MQ_RELAY_SSH=<ssh target> deploy/relay/deploy.sh [INSTANCE_FILE]
#
# INSTANCE_FILE (deploy/relay/momotus.env when none is given) names the instance: its service user
# and folders (/opt/<NAME>, /var/lib/<NAME>, /etc/<NAME>), units, ports, public host and flags;
# render.sh makes its units from the templates here. Static x86_64 musl binary built on this Mac;
# streamed with the units and backup.py into a root-only folder on the server; a same-minute backup;
# a preflight of the new binary on a copy of it (the relay has no schema migration, so a schema
# change stops here); then the switch, a loopback health check that rolls back on failure, and a
# public one that only reports.
set -euo pipefail

SSH=${MQ_RELAY_SSH:?set MQ_RELAY_SSH to the ssh target of the server}
TRUNK=feat/protocol-wire
ROOT=$(git rev-parse --show-toplevel)
INSTANCE=$(cd "$(dirname "${1:-$ROOT/deploy/relay/momotus.env}")" && pwd)/$(basename "${1:-momotus.env}")
cd "$ROOT"
NAME="" HOST=""
# shellcheck source=/dev/null # the instance file, momotus.env or another app's
. "$INSTANCE"
PUBLIC=https://$HOST

COMMIT=$(git rev-parse --short=8 HEAD)
git merge-base --is-ancestor HEAD "$TRUNK" || { echo "deploy: $COMMIT is not on $TRUNK" >&2; exit 1; }
git diff --quiet HEAD -- crates Cargo.toml Cargo.lock deploy/relay || { echo "deploy: uncommitted changes under crates or deploy/relay" >&2; exit 1; }

TARGET=${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee-relay-deploy}
CARGO_TARGET_DIR=$TARGET CARGO_INCREMENTAL=0 cargo zigbuild --release -j 4 -p mq-sync-server --target x86_64-unknown-linux-musl
BIN_DIR=$TARGET/x86_64-unknown-linux-musl/release
HASH=$(shasum -a 256 "$BIN_DIR/mq-sync-server" | cut -d' ' -f1)
echo "deploy: mq-sync-server $COMMIT sha256 $HASH, instance $NAME"
UNITS=$(mktemp -d)
trap 'rm -rf "$UNITS"' EXIT
# Checks the instance's values and writes them quoted as instance.env: the server reads only that.
deploy/relay/render.sh "$INSTANCE" "$UNITS"

# shellcheck disable=SC2029 # the instance name and the staging folder's expand here, on purpose
STAGE=$(ssh "$SSH" "sudo mktemp -d /root/$NAME-deploy.XXXXXX")
# shellcheck disable=SC2029
tar -cf - -C "$UNITS" "$NAME-relay.service" "$NAME-backup.service" "$NAME-backup.timer" instance.env -C "$ROOT/deploy/relay" backup.py identity.py reports.py -C "$BIN_DIR" mq-sync-server |
  ssh "$SSH" "sudo tar -C '$STAGE' -xf -"

# The remote script is quoted. Its arguments are a mktemp folder and two hex strings (ssh joins
# arguments into one command line); the instance's values come in the staged instance.env.
# shellcheck disable=SC2029
ssh "$SSH" sudo bash -s -- "$STAGE" "$COMMIT" "$HASH" <<'REMOTE'
set -euo pipefail
STAGE=$1 COMMIT=$2 HASH=$3
NAME="" PORT="" PREFLIGHT_PORT="" HOST="" FLAGS="" CREDENTIALS=""
# shellcheck source=/dev/null # render.sh wrote it: assignments of checked, quoted values
. "$STAGE/instance.env"
PRE=$NAME-preflight
cleanup() { systemctl stop "$PRE" 2>/dev/null || true; rm -rf "${STAGE:?}" "/var/lib/$PRE"; }
trap cleanup EXIT
health() { # url seconds
  for _ in $(seq "$2"); do curl -fsS --max-time 2 "$1/v1/relay" >/dev/null 2>&1 && return 0; sleep 1; done
  return 1
}

[ "$(sha256sum "$STAGE/mq-sync-server" | cut -d' ' -f1)" = "$HASH" ] || { echo "remote: hash differs" >&2; exit 1; }
install -d -m 0755 "/opt/$NAME" "/opt/$NAME/bin"
install -m 0755 "$STAGE/mq-sync-server" "/opt/$NAME/bin/mq-sync-server-$COMMIT"
install -m 0644 "$STAGE/backup.py" "$STAGE/identity.py" "$STAGE/reports.py" "/opt/$NAME/"
install -m 0644 "$STAGE/$NAME"-*.service "$STAGE/$NAME"-*.timer /etc/systemd/system/
systemctl daemon-reload

# A same-minute backup, then the new binary on a copy of it (or on nothing, the first time).
install -d -o "$NAME" -g "$NAME" -m 0700 "/var/lib/$PRE"
if [ -f "/var/lib/$NAME/relay.sqlite" ]; then
  STAMP=$(runuser -u "$NAME" -- python3 -I "/opt/$NAME/backup.py" "/var/lib/$NAME" | sed -n 's/^backup //p')
  install -o "$NAME" -g "$NAME" -m 0600 "/var/lib/$NAME/backups/$STAMP.sqlite" "/var/lib/$PRE/relay.sqlite"
  install -o "$NAME" -g "$NAME" -m 0600 "/var/lib/$NAME/backups/$STAMP.roster.jsonl" "/var/lib/$PRE/relay.roster.jsonl"
  echo "remote: backup $STAMP"
fi
# The unit's sandbox and flags, not its --trust-proxy (no Cloudflare in front of a loopback check),
# with the instance's credentials (plans/accounts-email.md E11) and server-side flags, and the
# preflight's own files for anything the flags keep under the data folder (the reports database).
# systemd-run does not expand %d, so the command reads $CREDENTIALS_DIRECTORY itself.
CRED=(-p "LoadCredential=identity.json:/etc/$NAME/identity.json")
for c in $CREDENTIALS; do CRED+=(-p "LoadCredential=$c:/etc/$NAME/$c"); done
PRE_FLAGS=${FLAGS//%d\//\"\$CREDENTIALS_DIRECTORY\"/}
PRE_FLAGS=${PRE_FLAGS//\/var\/lib\/$NAME\//\/var\/lib\/$PRE\/}
systemd-run --unit="$PRE" --quiet --collect -p "User=$NAME" -p "Group=$NAME" "${CRED[@]}" \
  -p "EnvironmentFile=-/etc/$NAME/relay.env" -p ProtectSystem=strict -p ProtectHome=yes \
  -p PrivateTmp=yes -p NoNewPrivileges=yes -p "ReadWritePaths=/var/lib/$PRE" \
  sh -c "exec /opt/$NAME/bin/mq-sync-server-$COMMIT --db /var/lib/$PRE/relay.sqlite --listen 127.0.0.1:$PREFLIGHT_PORT \
    --public-host $HOST --identity \"\$CREDENTIALS_DIRECTORY/identity.json\" $PRE_FLAGS \$RELAY_FLAGS"
if ! health "http://127.0.0.1:$PREFLIGHT_PORT" 10; then
  echo "remote: the new binary did not start on a copy of the database; nothing switched:" >&2
  journalctl -u "$PRE" -n 20 --no-pager >&2 || true
  exit 1
fi
systemctl stop "$PRE"

PREVIOUS=$(readlink -f "/opt/$NAME/bin/mq-sync-server" 2>/dev/null || true)
ln -sfn "mq-sync-server-$COMMIT" "/opt/$NAME/bin/mq-sync-server"
systemctl enable --quiet "$NAME-relay" "$NAME-backup.timer"
systemctl restart "$NAME-relay"
systemctl start "$NAME-backup.timer"
START=$(date +%s.%N)
# Loopback health, then the client address behind the proxy (the one step
# that exercises --trust-proxy): a budgeted call naming none is refused (400
# noClientAddress), one naming Cloudflare's is taken (a pairing slot, gone in
# 10 minutes). Either failing puts the previous binary back.
pairing() { curl -s -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' "$@" -d '{}' "http://127.0.0.1:$PORT/v1/pairings" || true; }
if health "http://127.0.0.1:$PORT" 15; then
  echo "remote: loopback health after $(awk "BEGIN { printf \"%.1f\", $(date +%s.%N) - $START }") s"
  without=$(pairing)
  with=$(pairing -H 'cf-connecting-ip: 192.0.2.1')
  echo "remote: pairing without a client address $without, with one $with"
fi
if [ "${with:-}" != 200 ] || [ "${without:-}" != 400 ]; then
  echo "remote: no health or no client address check on loopback; rolling back to ${PREVIOUS:-nothing}" >&2
  journalctl -u "$NAME-relay" -n 20 --no-pager >&2 || true
  if [ -n "$PREVIOUS" ]; then
    ln -sfn "$(basename "$PREVIOUS")" "/opt/$NAME/bin/mq-sync-server"
    systemctl restart "$NAME-relay"
    if health "http://127.0.0.1:$PORT" 15; then echo "remote: the previous binary answers again" >&2; else echo "remote: the previous binary does not answer either" >&2; fi
  fi
  exit 1
fi
# The newest three binaries stay.
ls -1t "/opt/$NAME/bin/mq-sync-server-"* | tail -n +4 | xargs -r rm -f --
REMOTE

START=$(date +%s)
for _ in $(seq 15); do
  if INFO=$(curl -fsS --max-time 5 "$PUBLIC/v1/relay"); then
    echo "deploy: public health after $(($(date +%s) - START)) s: $INFO"
    exit 0
  fi
  sleep 1
done
echo "deploy: the relay is up on the server but $PUBLIC does not answer: check cloudflared and the route" >&2
exit 1
