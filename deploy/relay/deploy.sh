#!/usr/bin/env bash
# Builds the relay at the checked-out trunk commit and deploys it to the server
# (plans/accounts-social-sync.md section 3):
#
#   MQ_RELAY_SSH=<ssh target> deploy/relay/deploy.sh
#
# Static x86_64 musl binary built on this Mac; streamed with the units and backup.py into a
# root-only folder on the server; a same-minute backup; a preflight of the new binary on a copy of
# it (the relay has no schema migration, so a schema change stops here); then the switch, a
# loopback health check that rolls back on failure, and a public one that only reports.
set -euo pipefail

SSH=${MQ_RELAY_SSH:?set MQ_RELAY_SSH to the ssh target of the server}
PUBLIC=https://momotus.normalisdead.mom
TRUNK=feat/protocol-wire
ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

COMMIT=$(git rev-parse --short=8 HEAD)
git merge-base --is-ancestor HEAD "$TRUNK" || { echo "deploy: $COMMIT is not on $TRUNK" >&2; exit 1; }
git diff --quiet HEAD -- crates Cargo.toml Cargo.lock deploy/relay || { echo "deploy: uncommitted changes under crates or deploy/relay" >&2; exit 1; }

TARGET=${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee-relay-deploy}
CARGO_TARGET_DIR=$TARGET CARGO_INCREMENTAL=0 cargo zigbuild --release -j 4 -p mq-sync-server --target x86_64-unknown-linux-musl
BIN_DIR=$TARGET/x86_64-unknown-linux-musl/release
HASH=$(shasum -a 256 "$BIN_DIR/mq-sync-server" | cut -d' ' -f1)
echo "deploy: mq-sync-server $COMMIT sha256 $HASH"

STAGE=$(ssh "$SSH" 'sudo mktemp -d /root/momotus-deploy.XXXXXX')
# shellcheck disable=SC2029 # the staging folder's name expands here, on purpose
tar -cf - -C deploy/relay momotus-relay.service momotus-backup.service momotus-backup.timer backup.py identity.py -C "$BIN_DIR" mq-sync-server |
  ssh "$SSH" "sudo tar -C '$STAGE' -xf -"

# The remote script is quoted: its values come in as arguments.
ssh "$SSH" sudo bash -s -- "$STAGE" "$COMMIT" "$HASH" <<'REMOTE'
set -euo pipefail
STAGE=$1 COMMIT=$2 HASH=$3
cleanup() { systemctl stop momotus-preflight 2>/dev/null || true; rm -rf "${STAGE:?}" /var/lib/momotus-preflight; }
trap cleanup EXIT
health() { # url seconds
  for _ in $(seq "$2"); do curl -fsS --max-time 2 "$1/v1/relay" >/dev/null 2>&1 && return 0; sleep 1; done
  return 1
}

[ "$(sha256sum "$STAGE/mq-sync-server" | cut -d' ' -f1)" = "$HASH" ] || { echo "remote: hash differs" >&2; exit 1; }
install -d -m 0755 /opt/momotus /opt/momotus/bin
install -m 0755 "$STAGE/mq-sync-server" "/opt/momotus/bin/mq-sync-server-$COMMIT"
install -m 0644 "$STAGE/backup.py" "$STAGE/identity.py" /opt/momotus/
install -m 0644 "$STAGE"/momotus-*.service "$STAGE"/momotus-*.timer /etc/systemd/system/
systemctl daemon-reload

# A same-minute backup, then the new binary on a copy of it (or on nothing, the first time).
install -d -o momotus -g momotus -m 0700 /var/lib/momotus-preflight
if [ -f /var/lib/momotus/relay.sqlite ]; then
  STAMP=$(runuser -u momotus -- python3 -I /opt/momotus/backup.py /var/lib/momotus | sed -n 's/^backup //p')
  install -o momotus -g momotus -m 0600 "/var/lib/momotus/backups/$STAMP.sqlite" /var/lib/momotus-preflight/relay.sqlite
  install -o momotus -g momotus -m 0600 "/var/lib/momotus/backups/$STAMP.roster.jsonl" /var/lib/momotus-preflight/relay.roster.jsonl
  echo "remote: backup $STAMP"
fi
# The unit's sandbox, not its flags: no --trust-proxy (no Cloudflare in front of a loopback check).
# systemd-run does not expand %d, so the command reads $CREDENTIALS_DIRECTORY itself. The mail
# credential only when the unit just installed uses it (plans/accounts-email.md E11), so the
# preflight runs what production will.
MAIL_CRED=() MAIL_FLAG=""
if grep -q '^ExecStart=.*--mail' /etc/systemd/system/momotus-relay.service; then
  MAIL_CRED=(-p LoadCredential=ses.json:/etc/momotus/ses.json)
  MAIL_FLAG=" --mail \"\$CREDENTIALS_DIRECTORY/ses.json\""
fi
systemd-run --unit=momotus-preflight --quiet --collect -p User=momotus -p Group=momotus \
  -p LoadCredential=identity.json:/etc/momotus/identity.json "${MAIL_CRED[@]}" -p ProtectSystem=strict -p ProtectHome=yes \
  -p PrivateTmp=yes -p NoNewPrivileges=yes -p ReadWritePaths=/var/lib/momotus-preflight \
  sh -c "exec /opt/momotus/bin/mq-sync-server-$COMMIT --db /var/lib/momotus-preflight/relay.sqlite --listen 127.0.0.1:8789 \
    --public-host momotus.normalisdead.mom --identity \"\$CREDENTIALS_DIRECTORY/identity.json\"$MAIL_FLAG"
if ! health http://127.0.0.1:8789 10; then
  echo "remote: the new binary did not start on a copy of the database; nothing switched:" >&2
  journalctl -u momotus-preflight -n 20 --no-pager >&2 || true
  exit 1
fi
systemctl stop momotus-preflight

PREVIOUS=$(readlink -f /opt/momotus/bin/mq-sync-server 2>/dev/null || true)
ln -sfn "mq-sync-server-$COMMIT" /opt/momotus/bin/mq-sync-server
systemctl enable --quiet momotus-relay momotus-backup.timer
systemctl restart momotus-relay
systemctl start momotus-backup.timer
START=$(date +%s.%N)
# Loopback health, then the client address behind the proxy (the one step
# that exercises --trust-proxy): a budgeted call naming none is refused (400
# noClientAddress), one naming Cloudflare's is taken (a pairing slot, gone in
# 10 minutes). Either failing puts the previous binary back.
pairing() { curl -s -o /dev/null -w '%{http_code}' -X POST -H 'content-type: application/json' "$@" -d '{}' http://127.0.0.1:8787/v1/pairings || true; }
if health http://127.0.0.1:8787 15; then
  echo "remote: loopback health after $(awk "BEGIN { printf \"%.1f\", $(date +%s.%N) - $START }") s"
  without=$(pairing)
  with=$(pairing -H 'cf-connecting-ip: 192.0.2.1')
  echo "remote: pairing without a client address $without, with one $with"
fi
if [ "${with:-}" != 200 ] || [ "${without:-}" != 400 ]; then
  echo "remote: no health or no client address check on loopback; rolling back to ${PREVIOUS:-nothing}" >&2
  journalctl -u momotus-relay -n 20 --no-pager >&2 || true
  if [ -n "$PREVIOUS" ]; then
    ln -sfn "$(basename "$PREVIOUS")" /opt/momotus/bin/mq-sync-server
    systemctl restart momotus-relay
    if health http://127.0.0.1:8787 15; then echo "remote: the previous binary answers again" >&2; else echo "remote: the previous binary does not answer either" >&2; fi
  fi
  exit 1
fi
# The newest three binaries stay.
ls -1t /opt/momotus/bin/mq-sync-server-* | tail -n +4 | xargs -r rm -f --
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
