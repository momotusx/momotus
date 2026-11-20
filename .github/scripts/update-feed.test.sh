#!/bin/bash
# Checks update-feed.sh: roles, the signature, the merge that never lowers a version, and a release without an IPK.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "${tmp:?}"' EXIT
openssl ecparam -name prime256v1 -genkey -noout -out "$tmp/key.pem"
openssl ec -in "$tmp/key.pem" -pubout -out "$tmp/pub.pem" 2>/dev/null

fail() { echo "update-feed.test.sh: $1" >&2; exit 1; }
# assets <dir> <version> <names...>: files whose bytes name the version, so their hashes differ per release.
assets() {
  local d=$1 v=$2
  shift 2
  mkdir -p "$d"
  for n in "$@"; do printf '%s %s' "$n" "$v" >"$d/$n"; done
}
body() { jq -r '.signed' "$1"; }
field() { body "$1" | jq -r "$2"; }
verifies() {
  jq -r '.sig' "$1" | openssl base64 -d -A >"$tmp/sig"
  jq -j .signed "$1" | openssl dgst -sha256 -verify "$tmp/pub.pem" -signature "$tmp/sig" >/dev/null 2>&1
}

# 0.5.0 with every asset.
assets "$tmp/a" 0.5.0 momotus-android-arm64-phone.apk momotus-android-armv7-firetv.apk momotus-webos-0.5.0.ipk SHA256SUMS
bash "$here/update-feed.sh" v0.5.0 "$tmp/a" "" "$tmp/key.pem" >"$tmp/f1.json"
verifies "$tmp/f1.json" || fail "the signature does not verify"
[ "$(field "$tmp/f1.json" 'keys | join(",")')" = "android-arm64,android-armv7,webos" ] || fail "roles: $(body "$tmp/f1.json")"
[ "$(field "$tmp/f1.json" '.webos.url')" = "https://github.com/momotusx/momotus/releases/download/v0.5.0/momotus-webos-0.5.0.ipk" ] ||
  fail "webos url"
[ "$(field "$tmp/f1.json" '."android-arm64".sha256')" = "$(openssl dgst -sha256 -r "$tmp/a/momotus-android-arm64-phone.apk" | cut -d' ' -f1)" ] ||
  fail "arm64 sha256"
[ "$(field "$tmp/f1.json" '."android-arm64".size')" = "$(wc -c <"$tmp/a/momotus-android-arm64-phone.apk" | tr -d ' ')" ] ||
  fail "arm64 size"

# A changed body no longer verifies.
jq '.signed |= sub("0.5.0"; "0.9.0")' "$tmp/f1.json" >"$tmp/bad.json"
if verifies "$tmp/bad.json"; then fail "a changed body still verifies"; fi

# 0.5.1 without an IPK: the APKs move, webos stays at 0.5.0.
assets "$tmp/b" 0.5.1 momotus-android-arm64-phone.apk momotus-android-armv7-firetv.apk
bash "$here/update-feed.sh" v0.5.1 "$tmp/b" "$tmp/f1.json" "$tmp/key.pem" >"$tmp/f2.json"
[ "$(field "$tmp/f2.json" '."android-armv7".version')" = 0.5.1 ] || fail "armv7 did not move to 0.5.1"
[ "$(field "$tmp/f2.json" '.webos.version')" = 0.5.0 ] || fail "webos changed without an IPK"

# Rebuilding the older 0.4.9 never lowers a role, and 0.10.0 sorts above 0.9.x by number.
assets "$tmp/c" 0.4.9 momotus-android-arm64-phone.apk momotus-webos-0.4.9.ipk
bash "$here/update-feed.sh" v0.4.9 "$tmp/c" "$tmp/f2.json" "$tmp/key.pem" >"$tmp/f3.json"
[ "$(body "$tmp/f3.json")" = "$(body "$tmp/f2.json")" ] || fail "an older tag changed the feed"
assets "$tmp/d" 0.10.0 momotus-android-arm64-phone.apk
bash "$here/update-feed.sh" v0.10.0 "$tmp/d" "$tmp/f3.json" "$tmp/key.pem" >"$tmp/f4.json"
[ "$(field "$tmp/f4.json" '."android-arm64".version')" = 0.10.0 ] || fail "0.10.0 did not replace 0.5.1"

# A previous feed that is not ours (here: the changed body) is refused, not signed again.
if bash "$here/update-feed.sh" v0.6.0 "$tmp/a" "$tmp/bad.json" "$tmp/key.pem" >/dev/null 2>&1; then fail "merged into a feed with a bad signature"; fi

# No asset at all, or a tag that is not a version, stops.
mkdir -p "$tmp/e"
if bash "$here/update-feed.sh" v0.6.0 "$tmp/e" "" "$tmp/key.pem" >/dev/null 2>&1; then fail "an empty release made a feed"; fi
if bash "$here/update-feed.sh" updates "$tmp/a" "" "$tmp/key.pem" >/dev/null 2>&1; then fail "a non-version tag made a feed"; fi

echo "update-feed.test.sh: ok"
