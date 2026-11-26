#!/usr/bin/env bash
# Builds Momotus.app for the Mac and makes a notarized, stapled Developer ID dmg of it, on the owner's Mac:
#   ASC_ISSUER_ID=<issuer> apps/apple/scripts/release-mac.sh [store|own]
# Apple silicon only: the core is built for aarch64-apple-darwin, so the app is arm64.
# Writes Momotus-<version>-mac-<arch>.dmg and its .sha256 into ~/.local/share/momotus/releases/<version>.
# Nothing is published. `store` (the default) is the Release default: no second engine, no MKV, webm, avi, DTS or
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
case $kind in store | own) ;; *) echo "usage: release-mac.sh [store|own]" >&2; exit 2 ;; esac
recipe=${DEVID_RELEASE:-$HOME/.local/share/signing/devid-release.sh}
[[ -x $recipe ]] || { echo "release-mac.sh: $recipe is missing or not executable" >&2; exit 1; }

apple="$(cd "$(dirname "$0")/.." && pwd)"
dd=$(mktemp -d)
trap 'rm -rf "${dd:?}"' EXIT
cd "$apple"
if [[ $kind == own ]]; then scripts/build-core.sh --mac; else STORE=1 scripts/build-core.sh --mac; fi
xcodegen generate -q
xcodebuild build -project Marquee.xcodeproj -scheme Marquee-macOS -configuration Release -derivedDataPath "$dd" \
  ARCHS=arm64 ONLY_ACTIVE_ARCH=YES MARQUEE_CORE_BUILD="$kind" -quiet
app="$dd/Build/Products/Release/Momotus.app"
[[ $(/usr/libexec/PlistBuddy -c 'Print :MarqueeCoreBuild' "$app/Contents/Info.plist") == "$kind" ]] \
  || { echo "release-mac.sh: the app is not a $kind build" >&2; exit 1; }
version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
"$recipe" "$app" "$HOME/.local/share/momotus/releases/$version"
