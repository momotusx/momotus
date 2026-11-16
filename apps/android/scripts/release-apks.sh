#!/bin/bash
# Builds the two release APKs, the one release round shared by this Mac and CI (.github/workflows/release-please.yml):
#   momotus-android-arm64-phone.apk    arm64-v8a, phones
#   momotus-android-armv7-firetv.apk   armeabi-v7a, TV sticks with a 32-bit userland (plans/m6-android.md D10)
# plus SHA256SUMS, into dist/android at the repository root (or the folder given as $1).
#
# The core comes from build-core.sh (both ABIs; it needs MARQUEE_TMDB_READ_TOKEN, from the environment or
# .env.tmdb.local). The hosted relay and link host come from .env.hosted.local or the environment
# (MARQUEE_SYNC_SERVER, MARQUEE_LINK_HOST); unset, build.gradle.kts's release defaults apply. GOOGLE_WEB_CLIENT_ID
# from the environment, else build.gradle.kts reads .env.accounts.local itself.
# Signing: apps/android/keystore.local.properties when present; without it AGP signs with this machine's debug key
# and the outputs are named *-debugsigned.apk, so such a file is never taken for a release. RELEASE_SHA1 (hex, no
# colons) makes the script fail when an APK has any other signer.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
ANDROID=$ROOT/apps/android
OUT=${1:-$ROOT/dist/android}
export ANDROID_HOME=${ANDROID_HOME:-/opt/homebrew/share/android-commandlinetools}
TOOLS=$ANDROID_HOME/build-tools/36.0.0

if [ -f "$ROOT/.env.hosted.local" ]; then
  set -a
  # shellcheck source=/dev/null
  source "$ROOT/.env.hosted.local"
  set +a
fi

bash "$ANDROID/scripts/build-core.sh"

# No configuration cache: it would store the signing passwords on disk.
(cd "$ANDROID" && bash scripts/gw.sh --no-daemon --no-configuration-cache :app:assembleRelease \
  -Pmarquee.syncServer="${MARQUEE_SYNC_SERVER:-}" \
  -Pmarquee.linkHost="${MARQUEE_LINK_HOST:-}" \
  -Pmarquee.googleWebClientId="${GOOGLE_WEB_CLIENT_ID:-}")

suffix=-debugsigned
if [ -f "$ANDROID/keystore.local.properties" ]; then suffix=; fi
mkdir -p "$OUT"
rm -f "$OUT"/momotus-android-*.apk "$OUT/SHA256SUMS"
built=$ANDROID/app/build/outputs/apk/release
cp "$built/app-arm64-v8a-release.apk" "$OUT/momotus-android-arm64-phone$suffix.apk"
cp "$built/app-armeabi-v7a-release.apk" "$OUT/momotus-android-armv7-firetv$suffix.apk"

cd "$OUT"
for apk in momotus-android-*.apk; do
  echo "== $apk ($(wc -c <"$apk") bytes)"
  "$TOOLS/aapt2" dump badging "$apk" | grep -E "^(package|native-code):"
  signers=$("$TOOLS/apksigner" verify --print-certs "$apk" | awk '/certificate SHA-1 digest/ {print $NF}' | sort -u)
  echo "signer SHA-1: $signers"
  if [ -n "${RELEASE_SHA1:-}" ] && [ "$signers" != "$(echo "$RELEASE_SHA1" | tr '[:upper:]' '[:lower:]' | tr -d :)" ]; then
    echo "release-apks.sh: $apk is not signed by the release key ($RELEASE_SHA1)" >&2
    exit 1
  fi
done
shasum -a 256 momotus-android-*.apk >SHA256SUMS
cat SHA256SUMS
