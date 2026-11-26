#!/usr/bin/env bash
# Builds Momotus.app for the Mac and makes a notarized, stapled Developer ID dmg of it, on the owner's Mac:
#   ASC_ISSUER_ID=<issuer> apps/apple/scripts/release-mac.sh [store|own] [SETTING=value ...]
# Apple silicon only: the core is built for aarch64-apple-darwin, so the app is arm64.
# Writes Momotus-<version>-mac-<arch>.dmg, its .sha256 and appcast.xml into ~/.local/share/momotus/releases/<version>.
# Nothing is published: the end prints the two uploads (the dmg to v<version>, appcast.xml to `updates`).
# The app checks MARQUEE_UPDATE_FEED (the `updates` release's appcast.xml unless set) and installs what the appcast
# offers when its EdDSA signature matches SUPublicEDKey: the key is ~/.config/momotus/sparkle/ed25519.pem, never in
# the tree. MARQUEE_UPDATE_BASE is where the appcast says the dmg is (the v<version> release unless set).
# Extra SETTING=value arguments go to xcodebuild (a walk: MARQUEE_APP_ID=app.momotus.walk MARKETING_VERSION=0.0.1). `store` (the default) is the Release default: no second engine, no MKV, webm, avi, DTS or
# TrueHD. `own` is the owner's own build (the torrent core and the second engine).
#
# CI cannot do this: Xcode cloud-signs the Developer ID export with the Apple ID signed in to Xcode (Settings,
# Apple Accounts), so there is no certificate file for a runner. CI keeps its ad hoc build; this is the local path.
# devid-release.sh (~/.local/share/signing) does the export, notarytool, stapling and the spctl check.
#
# The checkout needs its own core (build-core.sh runs here) and the .env.*.local files at its root (copy them from
# the main checkout into a worktree). Pass CARGO_TARGET_DIR=$HOME/.cargo-targets/<checkout> for the Rust build.
set -euo pipefail

kind=${1:-store}
case $kind in store | own) ;; *) echo "usage: release-mac.sh [store|own] [SETTING=value ...]" >&2; exit 2 ;; esac
shift $(($# > 0 ? 1 : 0))
feed=${MARQUEE_UPDATE_FEED-https://github.com/momotusx/momotus/releases/download/updates/appcast.xml}
key=$HOME/.config/momotus/sparkle/ed25519.pem
[[ -f $key ]] || { echo "release-mac.sh: $key is missing" >&2; exit 1; }
recipe=${DEVID_RELEASE:-$HOME/.local/share/signing/devid-release.sh}
[[ -x $recipe ]] || { echo "release-mac.sh: $recipe is missing or not executable" >&2; exit 1; }

apple="$(cd "$(dirname "$0")/.." && pwd)"
dd=$(mktemp -d)
trap 'rm -rf "${dd:?}"' EXIT
cd "$apple"
if [[ $kind == own ]]; then scripts/build-core.sh --mac; else STORE=1 scripts/build-core.sh --mac; fi
xcodegen generate -q
xcodebuild build -project Marquee.xcodeproj -scheme Marquee-macOS -configuration Release -derivedDataPath "$dd" \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES MARQUEE_CORE_BUILD="$kind" MARQUEE_UPDATE_FEED="$feed" "$@" -quiet
app="$dd/Build/Products/Release/Momotus.app"
[[ $(/usr/libexec/PlistBuddy -c 'Print :MarqueeCoreBuild' "$app/Contents/Info.plist") == "$kind" ]] \
  || { echo "release-mac.sh: the app is not a $kind build" >&2; exit 1; }
plist=$app/Contents/Info.plist
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$plist")
[[ $(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$plist") == "$version" ]] \
  || { echo "release-mac.sh: CFBundleVersion is not $version (Sparkle compares it)" >&2; exit 1; }
out=$HOME/.local/share/momotus/releases/$version
"$recipe" "$app" "$out"
dmg=$(ls "$out"/*-"$version"-mac-*.dmg)
base=${MARQUEE_UPDATE_BASE:-https://github.com/momotusx/momotus/releases/download/v$version}
# Sparkle's EdDSA signature: ed25519 over the whole dmg, as sign_update makes it.
sig=$(openssl pkeyutl -sign -inkey "$key" -rawin -in "$dmg" | base64)
cat >"$out/appcast.xml" <<X
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
<channel><title>Momotus</title><item>
<title>$version</title>
<sparkle:version>$version</sparkle:version>
<sparkle:shortVersionString>$version</sparkle:shortVersionString>
<sparkle:minimumSystemVersion>15.0</sparkle:minimumSystemVersion>
<enclosure url="$base/$(basename "$dmg")" length="$(stat -f %z "$dmg")" type="application/octet-stream" sparkle:edSignature="$sig"/>
</item></channel></rss>
X
echo "uploads: gh release upload v$version '$dmg' '$dmg.sha256' --repo momotusx/momotus"
echo "         gh release upload updates '$out/appcast.xml' --clobber --repo momotusx/momotus"
