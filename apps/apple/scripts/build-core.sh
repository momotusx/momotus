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
#            Never for a device, TestFlight or store build: build those without it.
set -euo pipefail

apple="$(cd "$(dirname "$0")/.." && pwd)"
root="$(cd "$apple/../.." && pwd)"
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

# C code built by build scripts (SQLite, ring) must match the app floors.
export MACOSX_DEPLOYMENT_TARGET=15.0 IPHONEOS_DEPLOYMENT_TARGET=18.0 TVOS_DEPLOYMENT_TARGET=18.0

cd "$root"
# The macOS slice is built with the bindgen tool in the same pass, so the
# tool's dependencies share this build. The extra code is never linked into the
# app: nothing in the library references it and the linker strips it.
echo "building mq-ffi and uniffi-bindgen for aarch64-apple-darwin"
CARGO_TARGET_DIR="$target_dir" CARGO_INCREMENTAL=0 \
  cargo build -p mq-ffi --release --target aarch64-apple-darwin -j 4 \
  --features bindgen --lib --bin uniffi-bindgen
for target in "${targets[@]}"; do
  echo "building mq-ffi for $target"
  CARGO_TARGET_DIR="$target_dir" CARGO_INCREMENTAL=0 \
    cargo build -p mq-ffi --lib --release --target "$target" -j 4
done

gen="$(mktemp -d)"
trap 'rm -rf "$gen"' EXIT
mac="$target_dir/aarch64-apple-darwin/release"
"$mac/uniffi-bindgen" generate --library "$mac/libmarquee_core.dylib" --language swift --out-dir "$gen"

mkdir -p "$gen/include"
cp "$gen/marquee_coreFFI.h" "$gen/include/"
cp "$gen/marquee_coreFFI.modulemap" "$gen/include/module.modulemap"

args=(-library "$mac/libmarquee_core.a" -headers "$gen/include")
for target in "${targets[@]}"; do
  args+=(-library "$target_dir/$target/release/libmarquee_core.a" -headers "$gen/include")
done
rm -rf "$out/MarqueeCore.xcframework"
mkdir -p "$out"
xcodebuild -create-xcframework "${args[@]}" -output "$out/MarqueeCore.xcframework" >/dev/null
# The app's build phase refuses a marked core outside Debug: it never ships.
if [[ "$dev_assertions" == true ]]; then
  echo "built with --dev: debug assertions and the loopback Trakt hook" > "$out/MarqueeCore.xcframework/DEV"
fi
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
