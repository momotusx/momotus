#!/usr/bin/env bash
# The native core's live check on Linux CI (plans/webos.md v5), as root can set it up: the core refuses loopback and
# LAN peers and links, so the fixture host (service/test/fixture-host.js) and an aria2 seeder run in a network namespace
# on 203.0.113.2, a documentation address the core counts as public, reached over a veth pair. The plugin repository
# is https there, with a certificate from a CA made here, which the core trusts through SSL_CERT_FILE beside the
# system roots. Then service/test/live-native.js runs once per Node in NODES against the ARM binary (binfmt and
# qemu-arm with QEMU_LD_PREFIX on an x86 runner). Needs sudo, iproute2, openssl, aria2, fnm and WS_PATH.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
tests="$here/service/test"
: "${WS_PATH:?set WS_PATH to a node_modules dir with ws@7.5.10}"
: "${NODES:?set NODES, such as 8.12.0 20.12.2}"
work="${RUNNER_TEMP:-/tmp}/webos-fixture"
ns=mqfx
host_ip=203.0.113.1
fx_ip=203.0.113.2
rm -rf "${work:?}"
mkdir -p "$work/seed"

pids=()
# shellcheck disable=SC2329 # run by the trap
cleanup() {
  for p in "${pids[@]}"; do sudo kill "$p" 2>/dev/null || true; done
  sudo ip netns del "$ns" 2>/dev/null || true
  sudo ip link del mqfx0 2>/dev/null || true
}
trap cleanup EXIT

# The namespace and its one route each way.
sudo ip netns add "$ns"
sudo ip link add mqfx0 type veth peer name mqfx1
sudo ip link set mqfx1 netns "$ns"
sudo ip addr add "$host_ip/32" dev mqfx0
sudo ip link set mqfx0 up
sudo ip route add "$fx_ip/32" dev mqfx0
sudo ip netns exec "$ns" ip link set lo up
sudo ip netns exec "$ns" ip addr add "$fx_ip/32" dev mqfx1
sudo ip netns exec "$ns" ip link set mqfx1 up
sudo ip netns exec "$ns" ip route add "$host_ip/32" dev mqfx1

# A CA and a server certificate for the address (a self-signed CA is no end entity for rustls).
openssl req -x509 -newkey rsa:2048 -nodes -days 2 -subj "/CN=Momotus fixture CA" \
  -keyout "$work/ca.key" -out "$work/ca.pem" 2>/dev/null
openssl req -newkey rsa:2048 -nodes -subj "/CN=$fx_ip" -keyout "$work/key.pem" -out "$work/leaf.csr" 2>/dev/null
printf 'subjectAltName=IP:%s\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n' "$fx_ip" >"$work/leaf.ext"
openssl x509 -req -in "$work/leaf.csr" -CA "$work/ca.pem" -CAkey "$work/ca.key" -CAcreateserial -days 2 \
  -extfile "$work/leaf.ext" -out "$work/cert.pem" 2>/dev/null
cat /etc/ssl/certs/ca-certificates.crt "$work/ca.pem" >"$work/roots.pem"

node="$(command -v node)"
# The logs stay this user's files: the redirects are meant outside sudo.
# shellcheck disable=SC2024
sudo ip netns exec "$ns" env FIXTURE_CERT="$work/cert.pem" FIXTURE_KEY="$work/key.pem" \
  "$node" "$tests/fixture-host.js" "$fx_ip" 8000 "$work/seed" >"$work/host.log" 2>&1 &
pids+=("$!")
for _ in $(seq 100); do
  grep -q fixtureHost "$work/host.log" 2>/dev/null && break
  sleep 0.1
done
cat "$work/host.log"
# shellcheck disable=SC2024
sudo ip netns exec "$ns" aria2c --enable-dht=false --enable-dht6=false --bt-enable-lpd=false \
  --enable-peer-exchange=false --seed-ratio=0.0 --listen-port=6881 --check-integrity=true \
  -d "$work/seed" "$work/seed/film.torrent" >"$work/seed.log" 2>&1 &
pids+=("$!")
curl -fsS "http://$fx_ip:8000/info" >/dev/null
curl -fsS --cacert "$work/ca.pem" "https://$fx_ip:8001/repo/manifest.json" >/dev/null

status=0
for v in $NODES; do
  echo "== live-native.js on Node $v"
  SSL_CERT_FILE="$work/roots.pem" MQ_FIXTURE="http://$fx_ip:8000" NODE_PATH="$WS_PATH" \
    fnm exec --using="v$v" node "$tests/live-native.js" || status=1
done
[ "$status" = 0 ] || tail -20 "$work/seed.log"
exit "$status"
