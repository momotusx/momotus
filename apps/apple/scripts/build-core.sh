#!/usr/bin/env bash
# Builds the Rust core (mq-ffi) as static libraries for Apple targets, generates
# the UniFFI Swift bindings, and packages both into apps/apple/Core/:
#   Core/MarqueeCore.xcframework   static library slices plus the C header
#   Core/marquee_core.swift        generated Swift bindings
# Both are build outputs (not committed); run this before the first Xcode build.
#
# Usage: apps/apple/scripts/build-core.sh [--tvos | --dev]
#   --tvos   also build the tvOS device slice (needed to build Marquee-tvOS)
#   --dev    debug assertions on, which compiles the bridge's dev hooks in (Trakt from
#            a local fake on this machine, `MQ_DEV_TRAKT`), in its own target dir, with
#            the macOS and iOS simulator slices only: for tests and simulator walks.
#            The app refuses it outside Debug (it greps every slice for the hook).
#
# Own installs (Xcode, TestFlight) carry the torrent engine: mq-ffi builds with
# `--features torrent`, and a slice without its mark (MQ_ENGINE_TORRENT) fails the
# build (check-engine.sh). STORE=1 builds the store core, with no engine, on purpose
# (App Store Guideline 5.2.3; plans/play-every-version.md, Phase 0).
#
# Each slice is built as a staticlib alone (`cargo rustc --crate-type staticlib`): with
# mq-ffi's three crate types cargo passes no `-C lto`, so the profile's thin LTO never
# applied. The Swift bindings are read from the macOS staticlib by the pinned generator
# the Android script uses (installed once):
#   cargo install --path apps/android/tools/uniffi-bindgen --locked --target-dir <scratch>
set -euo pipefail

apple="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$apple/../.." && pwd)"
# The Trakt app (client id and redirect URI, read by `option_env!`) when this
# machine has it: .env.trakt.local at the repository root, gitignored. Without
# the file the core has no Trakt app (notConfigured), as in CI or a fresh clone.
for env in "$root/.env.trakt.local"; do
  if [[ -f "$env" ]]; then
    set -a
    source "$env"
    set +a
  fi
done
# One target dir per checkout (see the repository rules).
target_dir="${CARGO_TARGET_DIR:-$HOME/.cargo-targets/marquee-apple}"
out="$apple/Core"

targets=(aarch64-apple-ios aarch64-apple-ios-sim)
dev_assertions=false
if [[ "${1:-}" == "--tvos" ]]; then
  targets+=(aarch64-apple-tvos)
elif [[ "${1:-}" == "--dev" ]]; then
  targets=(aarch64-apple-ios-sim)
  dev_assertions=true
  target_dir="${target_dir}-dev"
fi
export CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=$dev_assertions
features=(--features torrent)
if [[ "${STORE:-}" == 1 ]]; then
  features=()
fi

# C code built by build scripts (SQLite, ring) must match the app floors.
export MACOSX_DEPLOYMENT_TARGET=15.0 IPHONEOS_DEPLOYMENT_TARGET=18.0 TVOS_DEPLOYMENT_TARGET=18.0

cd "$root"
for target in aarch64-apple-darwin "${targets[@]}"; do
  echo "building mq-ffi for $target"
  CARGO_TARGET_DIR="$target_dir" CARGO_INCREMENTAL=0 \
    cargo rustc -p mq-ffi --lib --release --target "$target" -j 4 ${features[@]+"${features[@]}"} --crate-type staticlib
done

# A shipped core carries no dev hook: the MQ_DEV_ variables and the e2e recorder's set_dev_replay
# are compiled in only with debug assertions (--dev). Checked on every library built here.
# Every slice of an own build carries the engine; STORE=1 leaves it out and says so.
engine_libs=()
for target in aarch64-apple-darwin "${targets[@]}"; do
  engine_libs+=("$target_dir/$target/release/libmarquee_core.a")
done
"$apple/scripts/check-engine.sh" "${engine_libs[@]}"

if [[ $dev_assertions == false ]]; then
  for target in aarch64-apple-darwin "${targets[@]}"; do
    lib="$target_dir/$target/release/libmarquee_core.a"
    if grep -aqE 'MQ_DEV_|set_dev_replay' "$lib"; then
      echo "build-core.sh: $lib carries a dev hook (MQ_DEV_ or set_dev_replay): debug assertions were on" >&2
      exit 1
    fi
  done
fi

gen="$(mktemp -d)"
trap 'rm -rf "$gen"' EXIT
mac="$target_dir/aarch64-apple-darwin/release"
uniffi-bindgen generate --library "$mac/libmarquee_core.a" --language swift --out-dir "$gen"

# The module map sits in a folder named after its module: Xcode copies every xcframework's headers into one
# include folder, where a top-level module.modulemap collides with another package's (LibDovi's, through
# AetherEngine), and clang finds `marquee_coreFFI/module.modulemap` by the module's name.
mkdir -p "$gen/include/marquee_coreFFI"
cp "$gen/marquee_coreFFI.h" "$gen/include/marquee_coreFFI/"
cp "$gen/marquee_coreFFI.modulemap" "$gen/include/marquee_coreFFI/module.modulemap"

args=(-library "$mac/libmarquee_core.a" -headers "$gen/include")
for target in "${targets[@]}"; do
  args+=(-library "$target_dir/$target/release/libmarquee_core.a" -headers "$gen/include")
done
rm -rf "$out/MarqueeCore.xcframework"
mkdir -p "$out"
xcodebuild -create-xcframework "${args[@]}" -output "$out/MarqueeCore.xcframework" >/dev/null
# The libraries are 140 MB or more each: hard-link them instead of keeping copies.
for lib in "$out"/MarqueeCore.xcframework/*/libmarquee_core.a; do
  case "$(basename "$(dirname "$lib")")" in
    macos-arm64) src="$mac/libmarquee_core.a" ;;
    ios-arm64) src="$target_dir/aarch64-apple-ios/release/libmarquee_core.a" ;;
    ios-arm64-simulator) src="$target_dir/aarch64-apple-ios-sim/release/libmarquee_core.a" ;;
    tvos-arm64) src="$target_dir/aarch64-apple-tvos/release/libmarquee_core.a" ;;
    *) continue ;;
  esac
  cmp -s "$src" "$lib" && ln -f "$src" "$lib"
done
cp "$gen/marquee_core.swift" "$out/marquee_core.swift"
echo "wrote $out/MarqueeCore.xcframework and $out/marquee_core.swift"
